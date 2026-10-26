//! The extraction cache (4.5 Stage 2): what an entry is keyed by, how it is encoded,
//! and the local filesystem implementation.
//!
//! The cache is an optimisation and never a source of truth. An entry that is missing,
//! malformed, of another format, stored under another key or taken from other source
//! bytes is a miss: the file is extracted again and the entry replaced. Nothing a
//! cache holds is ever returned unless it is exactly the extraction the request would
//! produce.
//!
//! An entry holds the extraction's record ([`ExtractRecord`]): its text as ranges of the
//! source and its qualified names structurally, never the source itself (issue 90,
//! format 6). A hit is bound to the source being extracted, which Stage 2 has read and
//! normalised already: its length and digest are checked, and every range must lie
//! within it, or the entry is a miss. No range is ever clamped or read unchecked.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use pdx_engine::{BindError, ExtractRecord};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::consts::{
    ENGINE_VERSION, EXTRACT_CACHE_FORMAT_VERSION, EXTRACT_CACHE_MAX_ENTRY_BYTES,
    LANGUAGE_MATRIX_VERSION,
};
use crate::secrets::SecretPolicyDigest;

/// A file's Git blob identity: SHA-1 over `blob <length in decimal ASCII>\0` and the
/// file's **original** bytes, exactly as `git hash-object` computes it, written as 40
/// lowercase hex digits.
///
/// It identifies the file as the repository holds it, before secret normalisation,
/// and is what the cache and later the segment's `files.blob_sha` key on. It is not
/// [`pdx_engine::SourceDigest`], which is a SHA-256 of the normalised bytes the engine
/// was given; the two are computed over different bytes and are never interchangeable.
/// SHA-1 is used here only because Git's identity is SHA-1, never as a security hash.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BlobSha([u8; 20]);

impl BlobSha {
    /// The blob identity of `original`, the file's bytes as read.
    pub fn of(original: &[u8]) -> Self {
        let mut hasher = sha1::Sha1::new();
        hasher.update(b"blob ");
        hasher.update(original.len().to_string().as_bytes());
        hasher.update([0]);
        hasher.update(original);
        Self(hasher.finalize().into())
    }

    /// The identity's bytes.
    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }
}

impl std::fmt::Display for BlobSha {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl std::fmt::Debug for BlobSha {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BlobSha({self})")
    }
}

/// What an extraction depends on, and so what a cached one is stored under: the
/// engine's and the language matrix's versions, the secret policy, the language the
/// file is extracted as, its path, and its original bytes.
///
/// The path is part of it because qualified names and the surface depend on it, so
/// two files with the same bytes have different extractions. The language is part of
/// it because a repository's `[languages]` can assign the same path and bytes another
/// language without any version changing. Nothing else enters it: no checkout
/// location, time, machine, worker count or memory budget.
///
/// The engine's node budget is not part of it, nor any other of its switches that
/// change what it extracts: while one is set, no cache is read or written at all
/// ([`pdx_engine::EXTRACTION_SWITCHES`]).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CacheKey {
    /// [`ENGINE_VERSION`].
    pub engine_version: u32,
    /// [`LANGUAGE_MATRIX_VERSION`].
    pub language_matrix_version: u32,
    /// The effective secret policy.
    pub secret_policy_digest: SecretPolicyDigest,
    /// The file's language, by the matrix's id. The engine language it is extracted
    /// as follows from this and the path.
    pub language_id: String,
    /// The file's path below the repository root, `/`-separated.
    pub rel_path: String,
    /// The file's original bytes.
    pub blob_sha: BlobSha,
}

impl CacheKey {
    /// The key for a file under this build's engine and language matrix.
    pub fn new(
        secret_policy_digest: SecretPolicyDigest,
        language_id: &str,
        rel_path: &str,
        blob_sha: BlobSha,
    ) -> Self {
        Self {
            engine_version: ENGINE_VERSION,
            language_matrix_version: LANGUAGE_MATRIX_VERSION,
            secret_policy_digest,
            language_id: language_id.to_owned(),
            rel_path: rel_path.to_owned(),
            blob_sha,
        }
    }

    /// The name a cache stores the entry under: a SHA-256 over
    ///
    /// ```text
    /// "pdx-extract-cache" 0x00
    /// u32 BE  EXTRACT_CACHE_FORMAT_VERSION
    /// u32 BE  engine_version
    /// u32 BE  language_matrix_version
    /// 32      secret_policy_digest
    /// u64 BE  byte length of language_id, then its UTF-8 bytes
    /// u64 BE  byte length of rel_path, then its UTF-8 bytes
    /// 20      blob_sha
    /// ```
    ///
    /// Every field has a fixed width or a length before it, so no two keys encode
    /// alike. The name is only where the entry is kept: the entry holds the key
    /// itself, and is used only if it is this key.
    pub fn object_id(&self) -> CacheObjectId {
        let mut h = Sha256::new();
        h.update(b"pdx-extract-cache\0");
        h.update(EXTRACT_CACHE_FORMAT_VERSION.to_be_bytes());
        h.update(self.engine_version.to_be_bytes());
        h.update(self.language_matrix_version.to_be_bytes());
        h.update(self.secret_policy_digest.as_bytes());
        for text in [&self.language_id, &self.rel_path] {
            h.update((text.len() as u64).to_be_bytes());
            h.update(text.as_bytes());
        }
        h.update(self.blob_sha.as_bytes());
        CacheObjectId(h.finalize().into())
    }
}

/// The name a cache entry is stored under, written as 64 lowercase hex digits.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CacheObjectId([u8; 32]);

impl CacheObjectId {
    /// The id an entry's name spells: 64 lowercase hex digits, or none.
    fn parse(name: &str) -> Option<Self> {
        let bytes = name.as_bytes();
        if bytes.len() != 64 {
            return None;
        }
        let mut id = [0u8; 32];
        for (i, pair) in bytes.chunks(2).enumerate() {
            let digit = |c: u8| match c {
                b'0'..=b'9' => Some(c - b'0'),
                b'a'..=b'f' => Some(c - b'a' + 10),
                _ => None,
            };
            id[i] = (digit(pair[0])? << 4) | digit(pair[1])?;
        }
        Some(Self(id))
    }
}

impl std::fmt::Display for CacheObjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl std::fmt::Debug for CacheObjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CacheObjectId({self})")
    }
}

/// A cache entry: the extraction's record, with the format and the key it was stored
/// under. Encoded with postcard. It holds no source: the extraction's own fields, its
/// text as ranges of the source, and its surface only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheEnvelope {
    /// [`EXTRACT_CACHE_FORMAT_VERSION`] when written.
    pub format_version: u32,
    /// The key it was stored under.
    pub key: CacheKey,
    /// The extraction's record.
    pub extract: ExtractRecord,
}

/// [`CacheEnvelope`], borrowed, for encoding without a copy: the same fields in the
/// same order, so the same bytes.
#[derive(Serialize)]
struct EnvelopeRef<'a> {
    format_version: u32,
    key: &'a CacheKey,
    extract: &'a ExtractRecord,
}

impl CacheEnvelope {
    /// The entry for `extract` stored under `key`, encoded: its ranges as ranges.
    ///
    /// # Panics
    ///
    /// Never: every key and extraction encodes.
    pub fn encode(key: &CacheKey, extract: &ExtractRecord) -> Vec<u8> {
        postcard::to_stdvec(&EnvelopeRef {
            format_version: EXTRACT_CACHE_FORMAT_VERSION,
            key,
            extract,
        })
        .expect("an extraction encodes")
    }

    /// The extraction an encoded entry holds, if it is exactly one entry of this
    /// format stored under `key`.
    ///
    /// # Errors
    ///
    /// [`CacheDefect`] for anything else: bytes that do not decode, decode with bytes
    /// left over, are of another format, or were stored under another key.
    pub fn decode(bytes: &[u8], key: &CacheKey) -> Result<ExtractRecord, CacheDefect> {
        let (format_version, _) =
            postcard::take_from_bytes::<u32>(bytes).map_err(|_| CacheDefect::Malformed)?;
        if format_version != EXTRACT_CACHE_FORMAT_VERSION {
            return Err(CacheDefect::WrongFormat(format_version));
        }
        let (envelope, rest) =
            postcard::take_from_bytes::<Self>(bytes).map_err(|_| CacheDefect::Malformed)?;
        if !rest.is_empty() {
            return Err(CacheDefect::TrailingBytes(rest.len()));
        }
        if envelope.key != *key {
            return Err(CacheDefect::WrongKey);
        }
        Ok(envelope.extract)
    }
}

/// Why a cache entry was not used. Says what was wrong with it, never what it holds.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CacheDefect {
    /// It could not be read.
    #[error("unreadable: {0}")]
    Unreadable(io::ErrorKind),
    /// It is larger than any entry may be.
    #[error("{0} bytes, larger than any entry may be")]
    TooLarge(u64),
    /// It does not decode as an entry.
    #[error("not an entry")]
    Malformed,
    /// It decodes as an entry with bytes left over.
    #[error("{0} bytes after the entry")]
    TrailingBytes(usize),
    /// It is of another format.
    #[error("format {0}, not {EXTRACT_CACHE_FORMAT_VERSION}")]
    WrongFormat(u32),
    /// It was stored under another key.
    #[error("stored under another key")]
    WrongKey,
    /// It is not the extraction of the source being extracted: another language,
    /// path, length or digest.
    #[error("taken from other source")]
    OtherSource,
    /// It is an extraction no cache may hold: truncated, or one that lost work.
    #[error("an extraction that is not clean")]
    Unclean,
    /// Its text ranges or qualified names do not fit the source it was taken from
    /// (issue 90): it is no extraction of that source.
    #[error("text or names that do not fit its source")]
    MalformedText,
    /// Its `use` scopes are not a well-formed table (issue 114).
    #[error("use scopes that are not well formed")]
    MalformedScopes,
}

impl From<BindError> for CacheDefect {
    fn from(e: BindError) -> Self {
        match e {
            BindError::Length { .. } | BindError::Digest => Self::OtherSource,
            BindError::Range { .. }
            | BindError::QualifiedName { .. }
            | BindError::Prefix { .. } => Self::MalformedText,
            BindError::UseScope(_) => Self::MalformedScopes,
        }
    }
}

/// A cache entry could not be stored. The extraction is not affected.
#[derive(Debug, thiserror::Error)]
#[error("cannot store cache entry {object}: {source}")]
pub struct CacheStoreError {
    /// The entry's name.
    pub object: CacheObjectId,
    /// What failed.
    #[source]
    pub source: io::Error,
}

/// Somewhere extractions are cached. Shared by every worker of Stage 2 at once, so an
/// implementation serialises only what it must: two keys never wait on each other.
pub trait ExtractCache: Sync {
    /// The extraction record stored under `key`: `Ok(None)` when there is none, and an
    /// error for one that exists but cannot be used. An implementation never returns an
    /// extraction stored under another key. The caller binds it to the source it is
    /// extracting ([`ExtractRecord::bind`]) before using it.
    ///
    /// # Errors
    ///
    /// [`CacheDefect`] for an entry that exists and cannot be used.
    fn load(&self, key: &CacheKey) -> Result<Option<ExtractRecord>, CacheDefect>;

    /// Stores `extract` under `key`, replacing any entry, so that a reader sees the
    /// old entry or the new one whole, never part of one.
    ///
    /// # Errors
    ///
    /// When it cannot be stored.
    fn store(&self, key: &CacheKey, extract: &ExtractRecord) -> Result<(), CacheStoreError>;
}

/// The extraction cache in a local directory.
///
/// An entry is the file `<root>/v<format>/<first two hex digits>/<object id>`, written
/// whole to a temporary file in the same directory and renamed into place, so a
/// reader never sees part of one, and two writers of the same key leave one whole
/// entry. On Unix every directory it creates is private to the user (0700), and so is
/// every entry (0600). Where the cache lives is the caller's choice; nothing here
/// picks a location.
///
/// Retention (decision 85, issue 133): an entry's modification time is its last use, set
/// when it is written and again on every hit, and [`FsCache::prune`] removes the least
/// recently used entries first until the cache fits its size. Nothing this cache read or
/// wrote, and nothing used since it was made, is ever removed: a build prunes only what
/// no build has used while it ran. [`FsCache::remove_retired_formats`] removes the
/// directories of formats no build can read again.
#[derive(Clone, Debug)]
pub struct FsCache {
    root: PathBuf,
    opened: SystemTime,
    used: Arc<Mutex<HashSet<CacheObjectId>>>,
}

/// How large a cache is: its current format's entries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// The entries of the current format.
    pub entries: u64,
    /// Their size, in bytes.
    pub bytes: u64,
    /// Every retired format's directory (`v<format>`), with its size in bytes.
    pub retired: Vec<(String, u64)>,
}

/// What a prune removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pruned {
    /// The entries removed.
    pub entries: u64,
    /// Their size, in bytes.
    pub bytes: u64,
    /// The size left, in bytes.
    pub kept_bytes: u64,
}

/// One entry as a prune sees it.
struct Entry {
    used: SystemTime,
    path: PathBuf,
    id: Option<CacheObjectId>,
    bytes: u64,
}

/// A temporary file older than this is a write that never finished: removed.
const STALE_TEMPORARY: Duration = Duration::from_hours(24);

impl FsCache {
    /// The cache in `root`, created when the first entry is stored.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            opened: SystemTime::now(),
            used: Arc::default(),
        }
    }

    fn note_used(&self, key: &CacheKey) {
        self.used
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.object_id());
    }

    /// The directory of the current format's entries.
    fn current_dir(&self) -> PathBuf {
        self.root.join(format!("v{EXTRACT_CACHE_FORMAT_VERSION}"))
    }

    /// Removes the directory of every retired cache format (`v<N>`, N not the current
    /// format), whose entries no build can read again, and gives each with the bytes it
    /// held. Nothing else in the cache's directory is touched, and a link is never
    /// followed.
    ///
    /// # Errors
    ///
    /// When the cache's directory cannot be read or a retired directory removed.
    pub fn remove_retired_formats(&self) -> io::Result<Vec<(String, u64)>> {
        let retired = self.retired_formats()?;
        for (name, _) in &retired {
            fs::remove_dir_all(self.root.join(name))?;
        }
        Ok(retired)
    }

    fn retired_formats(&self) -> io::Result<Vec<(String, u64)>> {
        let current = format!("v{EXTRACT_CACHE_FORMAT_VERSION}");
        let listing = match fs::read_dir(&self.root) {
            Ok(listing) => listing,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut retired = Vec::new();
        for item in listing {
            let item = item?;
            let Ok(name) = item.file_name().into_string() else {
                continue;
            };
            let is_format = name
                .strip_prefix('v')
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
            if is_format && name != current && fs::symlink_metadata(item.path())?.is_dir() {
                let bytes = tree_bytes(&item.path())?;
                retired.push((name, bytes));
            }
        }
        retired.sort();
        Ok(retired)
    }

    /// The cache's size: its current format's entries, and every retired format's
    /// directory.
    ///
    /// # Errors
    ///
    /// When the cache's directory cannot be read.
    pub fn stats(&self) -> io::Result<CacheStats> {
        let entries = self.entries()?;
        Ok(CacheStats {
            entries: entries.len() as u64,
            bytes: entries.iter().map(|e| e.bytes).sum(),
            retired: self.retired_formats()?,
        })
    }

    /// Removes the current format's entries least recently used first, the oldest last
    /// use first and by path among equals, until they hold at most `max_bytes`, never
    /// one this cache read or wrote or any used since it was made. A write left
    /// unfinished for a day is removed too.
    ///
    /// # Errors
    ///
    /// When the cache's directory cannot be read or an entry removed.
    pub fn prune(&self, max_bytes: u64) -> io::Result<Pruned> {
        let mut entries = self.entries()?;
        let mut pruned = Pruned {
            kept_bytes: entries.iter().map(|e| e.bytes).sum(),
            ..Pruned::default()
        };
        let now = SystemTime::now();
        for temp in entries.iter().filter(|e| e.id.is_none()) {
            if now.duration_since(temp.used).unwrap_or_default() > STALE_TEMPORARY {
                remove_entry(&temp.path)?;
                pruned.kept_bytes -= temp.bytes;
            }
        }
        entries.retain(|e| e.id.is_some());
        entries.sort_by(|a, b| (a.used, &a.path).cmp(&(b.used, &b.path)));
        let used = self
            .used
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        for entry in &entries {
            if pruned.kept_bytes <= max_bytes {
                break;
            }
            let in_use = entry.used >= self.opened || entry.id.is_some_and(|id| used.contains(&id));
            if in_use {
                continue;
            }
            remove_entry(&entry.path)?;
            pruned.entries += 1;
            pruned.bytes += entry.bytes;
            pruned.kept_bytes -= entry.bytes;
        }
        Ok(pruned)
    }

    /// The current format's entries and unfinished writes (no id).
    fn entries(&self) -> io::Result<Vec<Entry>> {
        let mut entries = Vec::new();
        let shards = match fs::read_dir(self.current_dir()) {
            Ok(shards) => shards,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(entries),
            Err(e) => return Err(e),
        };
        for shard in shards {
            let shard = shard?;
            if !fs::symlink_metadata(shard.path())?.is_dir() {
                continue;
            }
            for item in fs::read_dir(shard.path())? {
                let item = item?;
                let metadata = fs::symlink_metadata(item.path())?;
                if !metadata.is_file() {
                    continue;
                }
                let name = item.file_name();
                let name = name.to_string_lossy();
                let id = CacheObjectId::parse(&name);
                if id.is_none() && !name.starts_with(".tmp-") {
                    continue;
                }
                entries.push(Entry {
                    used: metadata.modified()?,
                    path: item.path(),
                    id,
                    bytes: metadata.len(),
                });
            }
        }
        Ok(entries)
    }

    /// Its directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where the entry for `key` is kept.
    pub fn object_path(&self, key: &CacheKey) -> PathBuf {
        let id = key.object_id().to_string();
        self.root
            .join(format!("v{EXTRACT_CACHE_FORMAT_VERSION}"))
            .join(&id[..2])
            .join(id)
    }
}

impl ExtractCache for FsCache {
    fn load(&self, key: &CacheKey) -> Result<Option<ExtractRecord>, CacheDefect> {
        let path = self.object_path(key);
        let file = match fs::File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(CacheDefect::Unreadable(e.kind())),
        };
        let mut bytes = Vec::new();
        file.take(EXTRACT_CACHE_MAX_ENTRY_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| CacheDefect::Unreadable(e.kind()))?;
        if bytes.len() as u64 > EXTRACT_CACHE_MAX_ENTRY_BYTES {
            return Err(CacheDefect::TooLarge(bytes.len() as u64));
        }
        let record = CacheEnvelope::decode(&bytes, key)?;
        // A hit is the entry's last use; recording it is best effort, since retention
        // only orders what it removes by it.
        self.note_used(key);
        let _ = fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .and_then(|f| f.set_modified(SystemTime::now()));
        Ok(Some(record))
    }

    fn store(&self, key: &CacheKey, extract: &ExtractRecord) -> Result<(), CacheStoreError> {
        let path = self.object_path(key);
        let failed = |source| CacheStoreError {
            object: key.object_id(),
            source,
        };
        let dir = path.parent().expect("an entry is inside the cache");
        create_private_dir(dir).map_err(failed)?;
        let mut temp = tempfile::Builder::new()
            .prefix(".tmp-")
            .tempfile_in(dir)
            .map_err(failed)?;
        temp.write_all(&CacheEnvelope::encode(key, extract))
            .map_err(failed)?;
        temp.flush().map_err(failed)?;
        temp.persist(&path).map_err(|e| failed(e.error))?;
        self.note_used(key);
        Ok(())
    }
}

/// Removes a cache entry another build may have removed already.
fn remove_entry(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// The bytes of the regular files below `dir`, never following a link.
fn tree_bytes(dir: &Path) -> io::Result<u64> {
    let mut total = 0;
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for item in fs::read_dir(&dir)? {
            let item = item?;
            let metadata = fs::symlink_metadata(item.path())?;
            if metadata.is_dir() {
                pending.push(item.path());
            } else if metadata.is_file() {
                total += metadata.len();
            }
        }
    }
    Ok(total)
}

/// Creates `dir` and its missing parents, private to the user on Unix.
fn create_private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}
