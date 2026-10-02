//! The extraction cache (4.5 Stage 2): what an entry is keyed by, how it is encoded,
//! and the local filesystem implementation.
//!
//! The cache is an optimisation and never a source of truth. An entry that is missing,
//! malformed, of another format, stored under another key or taken from other source
//! bytes is a miss: the file is extracted again and the entry replaced. Nothing a
//! cache holds is ever returned unless it is exactly the extraction the request would
//! produce.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use pdx_engine::FileExtract;
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

/// A cache entry: the extraction, with the format and the key it was stored under.
/// Encoded with postcard. It holds no source: the extraction's own fields and its
/// surface only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheEnvelope {
    /// [`EXTRACT_CACHE_FORMAT_VERSION`] when written.
    pub format_version: u32,
    /// The key it was stored under.
    pub key: CacheKey,
    /// The extraction.
    pub extract: FileExtract,
}

/// [`CacheEnvelope`], borrowed, for encoding without a copy: the same fields in the
/// same order, so the same bytes.
#[derive(Serialize)]
struct EnvelopeRef<'a> {
    format_version: u32,
    key: &'a CacheKey,
    extract: &'a FileExtract,
}

impl CacheEnvelope {
    /// The entry for `extract` stored under `key`, encoded.
    ///
    /// # Panics
    ///
    /// Never: every key and extraction encodes.
    pub fn encode(key: &CacheKey, extract: &FileExtract) -> Vec<u8> {
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
    pub fn decode(bytes: &[u8], key: &CacheKey) -> Result<FileExtract, CacheDefect> {
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
    /// The extraction stored under `key`: `Ok(None)` when there is none, and an error
    /// for one that exists but cannot be used. An implementation never returns an
    /// extraction stored under another key.
    ///
    /// # Errors
    ///
    /// [`CacheDefect`] for an entry that exists and cannot be used.
    fn load(&self, key: &CacheKey) -> Result<Option<FileExtract>, CacheDefect>;

    /// Stores `extract` under `key`, replacing any entry, so that a reader sees the
    /// old entry or the new one whole, never part of one.
    ///
    /// # Errors
    ///
    /// When it cannot be stored.
    fn store(&self, key: &CacheKey, extract: &FileExtract) -> Result<(), CacheStoreError>;
}

/// The extraction cache in a local directory.
///
/// An entry is the file `<root>/v<format>/<first two hex digits>/<object id>`, written
/// whole to a temporary file in the same directory and renamed into place, so a
/// reader never sees part of one, and two writers of the same key leave one whole
/// entry. On Unix every directory it creates is private to the user (0700), and so is
/// every entry (0600). Where the cache lives is the caller's choice; nothing here
/// picks a location.
#[derive(Clone, Debug)]
pub struct FsCache {
    root: PathBuf,
}

impl FsCache {
    /// The cache in `root`, created when the first entry is stored.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
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
    fn load(&self, key: &CacheKey) -> Result<Option<FileExtract>, CacheDefect> {
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
        CacheEnvelope::decode(&bytes, key).map(Some)
    }

    fn store(&self, key: &CacheKey, extract: &FileExtract) -> Result<(), CacheStoreError> {
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
        Ok(())
    }
}

/// Creates `dir` and its missing parents, private to the user on Unix.
fn create_private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}
