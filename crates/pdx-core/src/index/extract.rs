//! Stage 2 of the indexing pipeline: extract (specification 4.5).
//!
//! Takes Stage 1's files and extracts, through the engine, every candidate with a
//! language; every other file is carried forward with the reason it has no
//! extraction. For each file extracted, in this order:
//!
//! 1. its bytes are read, after checking without following links that the path is
//!    still the regular file discovery found, and never more of it than discovery saw;
//! 2. its [`BlobSha`] is taken over those **original** bytes;
//! 3. its secret values are masked in the same buffer ([`crate::secrets`]), which
//!    keeps every byte offset;
//! 4. its [`SourceDigest`] is taken over the **normalised** bytes;
//! 5. the cache is asked for the extraction keyed by the [`CacheKey`], and the engine
//!    is given the normalised bytes only when the cache has no usable one.
//!
//! The original bytes never leave this stage: not to the engine, not to the cache,
//! not into an error. A redacted file is never opened at all.
//!
//! An extraction is cached only when it is clean: not truncated and no work lost. And
//! while any of the engine's switches that change what it extracts is set (the node
//! budget among them, [`pdx_engine::EXTRACTION_SWITCHES`]), no cache is read or
//! written at all. A cached extraction is used only when it is the
//! one the engine would return for these normalised bytes, at this path, as this
//! language; anything else is a miss, extracted afresh and replaced.
//!
//! Files are extracted in batches ([`plan_batches`]) by a pool of workers sized to the
//! memory budget, each worker with an engine (or an isolated extractor) of its own.
//! The outcome is in the order the files were given, whatever order the workers
//! finish in. An engine that refuses a file costs that file; a crash costs the file
//! that caused it; an isolated worker that does not answer in time fails the stage.

mod cache;
mod plan;

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use pdx_engine::isolate::{
    ExtractFailure, ExtractOutcome, Extractor, IsolatedExtractor, IsolationError, SourceFile,
    WorkerCommand, isolation_requested,
};
use pdx_engine::{Engine, EngineError, FileExtract, SourceDigest, extraction_switch_set};

use crate::config::SecretsConfig;
use crate::index::discover::{DiscoveredFile, Disposition};
use crate::languages::Language;
use crate::secrets::{self, SecretPolicyDigest};

pub use cache::{
    BlobSha, CacheDefect, CacheEnvelope, CacheKey, CacheObjectId, CacheStoreError, ExtractCache,
    FsCache,
};
pub use plan::{Batch, BatchPlan, ExtractLimits, plan_batches};

/// What a worker extracts with: one per worker thread, never shared.
pub trait ExtractBackend {
    /// Extracts a batch: one outcome per file, in order.
    ///
    /// # Errors
    ///
    /// When isolated extraction fails as a whole: no worker process, or one that did
    /// not answer in time.
    fn extract_batch(
        &mut self,
        files: &[SourceFile],
    ) -> Result<Vec<ExtractOutcome>, IsolationError>;
}

impl ExtractBackend for Extractor {
    fn extract_batch(
        &mut self,
        files: &[SourceFile],
    ) -> Result<Vec<ExtractOutcome>, IsolationError> {
        Extractor::extract_batch(self, files)
    }
}

/// Makes a worker's backend, on the worker's own thread.
pub type BackendFactory<'a> = dyn Fn() -> Result<Box<dyn ExtractBackend>, ExtractError> + Sync + 'a;

/// The production backend: the engine in this thread, or, when isolation is
/// requested (`PDX_ENGINE_ISOLATE=1`), workers started from this program.
///
/// # Errors
///
/// The engine's failure to start, or this program not being found.
pub fn default_backend() -> Result<Box<dyn ExtractBackend>, ExtractError> {
    let extractor = if isolation_requested() {
        let worker = WorkerCommand::current_exe().map_err(ExtractError::WorkerProgram)?;
        Extractor::Isolated(IsolatedExtractor::new(worker))
    } else {
        Extractor::InProcess(Engine::new().map_err(ExtractError::Engine)?)
    };
    Ok(Box::new(extractor))
}

/// What became of one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileOutcome {
    /// Extracted, by the engine or from the cache: the same extraction either way.
    Extracted {
        /// The extraction.
        extract: Box<FileExtract>,
        /// The file's original bytes.
        blob_sha: BlobSha,
    },
    /// The engine refused or failed to extract it.
    EngineFailed {
        /// The file's original bytes.
        blob_sha: BlobSha,
        /// Why.
        error: EngineError,
    },
    /// The engine crashed on it, in an isolated worker.
    EngineCrashed {
        /// The file's original bytes.
        blob_sha: BlobSha,
    },
    /// Larger than the whole memory budget: never read.
    SkippedMemory,
    /// Discovery found it binary: never extracted.
    Binary,
    /// Its path matches a secret pattern: never opened.
    Redacted,
    /// Larger than `[discover] max_file_bytes`: never opened.
    SkippedSize,
    /// No language: never extracted.
    UnknownLanguage,
}

impl FileOutcome {
    /// Whether this file makes the stage degraded: a crash, a skip for memory, or an
    /// extraction that is truncated or lost work. An extraction the parser found
    /// errors in is not degraded; its status says so.
    pub fn degrades(&self) -> bool {
        match self {
            Self::Extracted { extract, .. } => extract.truncated || extract.extraction_lost > 0,
            Self::EngineCrashed { .. } | Self::SkippedMemory => true,
            _ => false,
        }
    }

    /// The file's blob identity, for a file that was read.
    pub fn blob_sha(&self) -> Option<BlobSha> {
        match self {
            Self::Extracted { blob_sha, .. }
            | Self::EngineFailed { blob_sha, .. }
            | Self::EngineCrashed { blob_sha } => Some(*blob_sha),
            _ => None,
        }
    }
}

/// One file of the stage's input and what became of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedFile {
    /// Its path below the root, as discovery gave it.
    pub path: String,
    /// Its language, as discovery assigned it.
    pub language: Option<&'static Language>,
    /// What became of it.
    pub outcome: FileOutcome,
    /// Its lines, counted from the bytes Stage 2 read; `None` for a file it never read
    /// (redacted, binary, too large, skipped for memory, of no language).
    pub line_count: Option<u64>,
}

/// What the stage did, beside its outcomes: never part of what it produced, which
/// does not depend on any of this.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtractStats {
    /// Whether a cache was read and written: a cache was given and none of the
    /// engine's extraction switches was set.
    pub cache_enabled: bool,
    /// Files given to the engine.
    pub engine_files: u64,
    /// Files whose extraction came from the cache.
    pub cache_hits: u64,
    /// Files the cache had no entry for.
    pub cache_misses: u64,
    /// Files the cache had an entry for that could not be used.
    pub cache_unusable: u64,
    /// Extractions stored.
    pub cache_writes: u64,
    /// Extractions that could not be stored.
    pub cache_write_failures: u64,
    /// The workers the budget allows.
    pub effective_workers: usize,
    /// The workers that ran.
    pub threads: usize,
    /// The bytes of source each worker could hold.
    pub per_worker_budget: u64,
    /// Batches extracted.
    pub batches: usize,
    /// The most bytes of source the workers held at any one time.
    pub peak_source_bytes: u64,
}

/// Stage 2's result: every file it was given, in the order given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractReport {
    /// The files and their outcomes.
    pub files: Vec<ExtractedFile>,
    /// What the stage did.
    pub stats: ExtractStats,
}

impl ExtractReport {
    /// Whether any file degrades the stage ([`FileOutcome::degrades`]).
    pub fn degraded(&self) -> bool {
        self.files.iter().any(|f| f.outcome.degrades())
    }
}

/// How a discovered file differs from what discovery found, now that Stage 2 reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceChange {
    /// It, or a directory above it, is gone.
    Missing,
    /// It, or a directory above it, is now a symlink.
    Symlink,
    /// It is no longer a regular file, or a directory above it no longer a directory.
    NotAFile,
    /// Its size is not the one discovery recorded.
    Size {
        /// The size discovery recorded.
        discovered: u64,
        /// The size now, or more than discovery recorded.
        now: u64,
    },
    /// It now holds a NUL byte.
    Binary,
}

impl std::fmt::Display for SourceChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => f.write_str("it is gone"),
            Self::Symlink => f.write_str("it is now a symlink"),
            Self::NotAFile => f.write_str("it is no longer a regular file"),
            Self::Size { discovered, now } => {
                write!(f, "its size changed from {discovered} to {now} bytes")
            }
            Self::Binary => f.write_str("it now holds a NUL byte"),
        }
    }
}

/// Why Stage 2 failed as a whole. Names a file by its path, never its content.
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    /// The limits allow no work.
    #[error("invalid extraction limits: {0}")]
    InvalidLimits(&'static str),
    /// A file changed between discovery and extraction: the checkout is not the one
    /// discovered, and nothing built from it would be consistent.
    #[error("{path}: changed since discovery: {change}")]
    CheckoutChanged {
        /// The file.
        path: String,
        /// How.
        change: SourceChange,
    },
    /// A path from discovery that is not one a checkout can hold.
    #[error("{0}: not a path below the root")]
    BadPath(String),
    /// A file Stage 2 does not extract was given to be read: only a candidate with a
    /// language is ever opened.
    #[error("{0}: not a file Stage 2 extracts")]
    NotExtracted(String),
    /// A file could not be read.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: String,
        /// What went wrong.
        #[source]
        source: io::Error,
    },
    /// A worker's engine could not start.
    #[error("the engine could not start: {0}")]
    Engine(#[source] EngineError),
    /// The program to start isolated workers from could not be found.
    #[error("cannot find this program to start engine workers: {0}")]
    WorkerProgram(#[source] io::Error),
    /// Isolated extraction failed as a whole. A timeout is one: fatal to the build,
    /// which publishes no segment.
    #[error(transparent)]
    Isolation(#[from] IsolationError),
    /// The engine returned an extraction of another file than it was given.
    #[error("{0}: the engine answered for another file")]
    WrongAnswer(String),
    /// The worker pool could not be built.
    #[error("cannot start the extraction workers: {0}")]
    Workers(String),
}

/// A file's source, ready for the engine: read, identified and normalised.
///
/// Stage 3 reads a file again through this, after Stage 2, rather than keep every
/// source in memory, and checks that `blob_sha` is still Stage 2's and `digest` the
/// extraction's before it resolves.
pub struct PreparedSource {
    /// The normalised bytes: the original's length, with every secret value masked.
    pub bytes: Vec<u8>,
    /// The original bytes' Git blob identity.
    pub blob_sha: BlobSha,
    /// The normalised bytes' digest, as the engine records it.
    pub digest: SourceDigest,
    /// Its lines: the line feeds, and one more for a last line without one; 0 for an
    /// empty file. Masking never touches a line ending, so it is the original's count.
    pub line_count: u64,
}

/// The lines of `bytes`: its line feeds, and one more for a last line without one.
pub fn count_lines(bytes: &[u8]) -> u64 {
    let feeds = bytes.iter().fold(0u64, |n, &b| n + u64::from(b == b'\n'));
    feeds + u64::from(bytes.last().is_some_and(|&b| b != b'\n'))
}

impl std::fmt::Debug for PreparedSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedSource")
            .field("len", &self.bytes.len())
            .field("blob_sha", &self.blob_sha)
            .field("digest", &self.digest)
            .finish_non_exhaustive()
    }
}

/// Reads and prepares a file Stage 2 extracts: its original bytes are read into one
/// buffer, identified, then normalised in place.
///
/// # Errors
///
/// [`ExtractError::NotExtracted`] for any file but a candidate with a language, which
/// is never opened: a redacted file above all.
/// [`ExtractError::CheckoutChanged`] when the path is no longer the regular file of
/// the size discovery found, and any read failure.
pub fn prepare_source(root: &Path, file: &DiscoveredFile) -> Result<PreparedSource, ExtractError> {
    let (Disposition::Candidate, Some(language)) = (file.disposition, file.language) else {
        return Err(ExtractError::NotExtracted(file.path.clone()));
    };
    let mut bytes = read_original(root, file)?;
    let blob_sha = BlobSha::of(&bytes);
    secrets::normalise_in_place(&mut bytes, language.id);
    let digest = SourceDigest::of(&bytes);
    let line_count = count_lines(&bytes);
    Ok(PreparedSource {
        bytes,
        blob_sha,
        digest,
        line_count,
    })
}

/// Reads a candidate Stage 2 does not extract, such as a manifest of no language
/// (`go.mod`), for what later stages read from it: with the same checks and bound as
/// [`prepare_source`], identified, and normalised as a file of its language, or of none,
/// so a secret value in it is masked as in any other file. Only a candidate is ever
/// opened.
///
/// # Errors
///
/// [`ExtractError::NotExtracted`] for anything but a candidate, and as for
/// [`prepare_source`].
pub fn prepare_candidate(
    root: &Path,
    file: &DiscoveredFile,
) -> Result<PreparedSource, ExtractError> {
    if file.disposition != Disposition::Candidate {
        return Err(ExtractError::NotExtracted(file.path.clone()));
    }
    let mut bytes = read_original(root, file)?;
    let blob_sha = BlobSha::of(&bytes);
    secrets::normalise_in_place(&mut bytes, file.language.map_or("", |l| l.id));
    let digest = SourceDigest::of(&bytes);
    let line_count = count_lines(&bytes);
    Ok(PreparedSource {
        bytes,
        blob_sha,
        digest,
        line_count,
    })
}

/// The file's original bytes, read only if the path, followed component by component
/// without following a link, is still a regular file of the size discovery recorded,
/// and never more than that size.
///
/// Each component is checked before the file is opened, which keeps a symlink placed
/// since discovery from being followed out of the checkout; a change made between the
/// check and the read is outside what a portable check can see.
fn read_original(root: &Path, file: &DiscoveredFile) -> Result<Vec<u8>, ExtractError> {
    let changed = |change| ExtractError::CheckoutChanged {
        path: file.path.clone(),
        change,
    };
    let io_error = |source| ExtractError::Io {
        path: file.path.clone(),
        source,
    };
    let components: Vec<&str> = file.path.split('/').collect();
    let mut path = root.to_path_buf();
    for (i, component) in components.iter().enumerate() {
        if matches!(*component, "" | "." | "..")
            || (cfg!(windows) && component.contains(['\\', ':']))
        {
            return Err(ExtractError::BadPath(file.path.clone()));
        }
        path.push(component);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(changed(SourceChange::Missing));
            }
            Err(e) => return Err(io_error(e)),
        };
        if metadata.file_type().is_symlink() {
            return Err(changed(SourceChange::Symlink));
        }
        let last = i + 1 == components.len();
        if (last && !metadata.is_file()) || (!last && !metadata.is_dir()) {
            return Err(changed(SourceChange::NotAFile));
        }
        if last && metadata.len() != file.size_bytes {
            return Err(changed(SourceChange::Size {
                discovered: file.size_bytes,
                now: metadata.len(),
            }));
        }
    }
    let mut bytes = Vec::with_capacity(usize::try_from(file.size_bytes).unwrap_or(0));
    fs::File::open(&path)
        .and_then(|f| {
            f.take(file.size_bytes.saturating_add(1))
                .read_to_end(&mut bytes)
        })
        .map_err(io_error)?;
    if bytes.len() as u64 != file.size_bytes {
        return Err(changed(SourceChange::Size {
            discovered: file.size_bytes,
            now: bytes.len() as u64,
        }));
    }
    if bytes.contains(&0) {
        return Err(changed(SourceChange::Binary));
    }
    Ok(bytes)
}

/// Stage 2, configured.
pub struct ExtractStage<'a> {
    root: PathBuf,
    policy: SecretPolicyDigest,
    limits: ExtractLimits,
    cache: Option<&'a dyn ExtractCache>,
    backend: Option<&'a BackendFactory<'a>>,
}

impl<'a> ExtractStage<'a> {
    /// The stage for the checkout at `root` under the secret policy `secrets`, within
    /// `limits`, with no cache and the production backend.
    pub fn new(root: &Path, secrets: &SecretsConfig, limits: ExtractLimits) -> Self {
        Self {
            root: root.to_path_buf(),
            policy: SecretPolicyDigest::of(secrets),
            limits,
            cache: None,
            backend: None,
        }
    }

    /// Uses `cache`.
    #[must_use]
    pub fn with_cache(mut self, cache: &'a dyn ExtractCache) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Makes each worker's backend with `factory` instead of [`default_backend`].
    #[must_use]
    pub fn with_backend(mut self, factory: &'a BackendFactory<'a>) -> Self {
        self.backend = Some(factory);
        self
    }

    /// The secret policy's digest, which every cache key carries.
    pub fn policy(&self) -> SecretPolicyDigest {
        self.policy
    }

    /// Runs the stage over `files`, Stage 1's output.
    ///
    /// # Errors
    ///
    /// [`ExtractError`] when the stage cannot produce a consistent result: invalid
    /// limits, a file changed since discovery or unreadable, an engine that cannot
    /// start, or isolated extraction that fails as a whole (a timeout included). A
    /// file the engine refuses or crashes on is an outcome, not an error.
    ///
    /// # Panics
    ///
    /// If a worker panics, which the stage passes on. Never otherwise: every file
    /// either has an outcome before the workers start or is in exactly one batch.
    pub fn run(&self, files: &[DiscoveredFile]) -> Result<ExtractReport, ExtractError> {
        let plan = plan_batches(files, self.limits)?;
        // A switch that changes what extraction contains, which no key names.
        let cache = if extraction_switch_set().is_some() {
            None
        } else {
            self.cache
        };

        let mut outcomes: Vec<Option<FileOutcome>> = files
            .iter()
            .map(|f| match f.disposition {
                Disposition::Redacted => Some(FileOutcome::Redacted),
                Disposition::Binary => Some(FileOutcome::Binary),
                Disposition::SkippedSize => Some(FileOutcome::SkippedSize),
                Disposition::Candidate if f.language.is_none() => {
                    Some(FileOutcome::UnknownLanguage)
                }
                Disposition::Candidate => None,
            })
            .collect();
        for &i in &plan.memory_skipped {
            outcomes[i] = Some(FileOutcome::SkippedMemory);
        }

        let counters = Counters::default();
        let mut line_counts: Vec<Option<u64>> = vec![None; files.len()];
        if plan.threads() > 0 {
            for (index, outcome, lines) in self.run_batches(files, &plan, cache, &counters)? {
                outcomes[index] = Some(outcome);
                line_counts[index] = Some(lines);
            }
        }

        let files = files
            .iter()
            .zip(outcomes)
            .zip(line_counts)
            .map(|((file, outcome), line_count)| ExtractedFile {
                path: file.path.clone(),
                language: file.language,
                outcome: outcome.expect("every file has an outcome"),
                line_count,
            })
            .collect();
        Ok(ExtractReport {
            files,
            stats: ExtractStats {
                cache_enabled: cache.is_some(),
                engine_files: counters.engine_files.load(Ordering::Relaxed),
                cache_hits: counters.cache_hits.load(Ordering::Relaxed),
                cache_misses: counters.cache_misses.load(Ordering::Relaxed),
                cache_unusable: counters.cache_unusable.load(Ordering::Relaxed),
                cache_writes: counters.cache_writes.load(Ordering::Relaxed),
                cache_write_failures: counters.cache_write_failures.load(Ordering::Relaxed),
                effective_workers: plan.effective_workers,
                threads: plan.threads(),
                per_worker_budget: plan.per_worker_budget,
                batches: plan.batches.len(),
                peak_source_bytes: counters.peak_source_bytes.load(Ordering::Relaxed),
            },
        })
    }

    /// Extracts every batch on a pool of [`BatchPlan::threads`] workers, each with a
    /// backend of its own, taking batches in order until none is left or one fails.
    /// Returns each extracted file's index and outcome.
    fn run_batches(
        &self,
        files: &[DiscoveredFile],
        plan: &BatchPlan,
        cache: Option<&dyn ExtractCache>,
        counters: &Counters,
    ) -> Result<Vec<(usize, FileOutcome, u64)>, ExtractError> {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(plan.threads())
            .thread_name(|i| format!("pdx-extract-{i}"))
            // A parse recurses as deep as the source nests; the stack a main thread
            // gets, not the smaller one of a spawned thread.
            .stack_size(8 << 20)
            .build()
            .map_err(|e| ExtractError::Workers(e.to_string()))?;
        let next = AtomicUsize::new(0);
        let stop = AtomicBool::new(false);
        // Each batch's result goes to its own slot, so the order batches finish in
        // never reaches the output.
        let slots: Vec<BatchSlot> = plan.batches.iter().map(|_| Mutex::new(None)).collect();
        let failures: Mutex<Vec<(usize, ExtractError)>> = Mutex::new(Vec::new());
        let fail = |batch: usize, error: ExtractError| {
            stop.store(true, Ordering::SeqCst);
            failures
                .lock()
                .expect("no worker panics holding it")
                .push((batch, error));
        };

        pool.broadcast(|_| {
            let backend = match self.backend {
                Some(factory) => factory(),
                None => default_backend(),
            };
            let mut backend = match backend {
                Ok(backend) => backend,
                Err(e) => return fail(0, e),
            };
            while !stop.load(Ordering::SeqCst) {
                let index = next.fetch_add(1, Ordering::SeqCst);
                let Some(batch) = plan.batches.get(index) else {
                    break;
                };
                match self.extract_batch(&mut *backend, files, batch, cache, counters) {
                    Ok(outcomes) => {
                        *slots[index].lock().expect("no worker panics holding it") = Some(outcomes);
                    }
                    Err(e) => return fail(index, e),
                }
            }
        });

        let mut failures = failures.into_inner().expect("no worker panicked");
        if !failures.is_empty() {
            // The earliest batch's failure, whichever worker met it first.
            failures.sort_by_key(|(batch, _)| *batch);
            return Err(failures.swap_remove(0).1);
        }
        Ok(slots
            .into_iter()
            .flat_map(|slot| {
                slot.into_inner()
                    .expect("no worker panicked")
                    .unwrap_or_default()
            })
            .collect())
    }

    /// One batch: each file read, identified and normalised; taken from the cache
    /// when it holds this file's clean extraction; the rest extracted together and
    /// the clean ones stored. Every source buffer is dropped before this returns.
    fn extract_batch(
        &self,
        backend: &mut dyn ExtractBackend,
        files: &[DiscoveredFile],
        batch: &Batch,
        cache: Option<&dyn ExtractCache>,
        counters: &Counters,
    ) -> Result<Vec<(usize, FileOutcome, u64)>, ExtractError> {
        let mut done = Vec::with_capacity(batch.files.len());
        let mut sources = Vec::new();
        let mut pending = Vec::new();
        let mut held = Held::new(counters);
        for &index in &batch.files {
            let file = &files[index];
            let language = file.language.expect("a planned file has a language");
            let prepared = prepare_source(&self.root, file)?;
            held.add(prepared.bytes.len() as u64);
            let engine_language = language.engine_language(&file.path);
            let key = CacheKey::new(self.policy, language.id, &file.path, prepared.blob_sha);
            if let Some(cache) = cache {
                let usable = cache.load(&key).and_then(|found| {
                    found
                        .map(|extract| {
                            check_cached(&extract, engine_language, &file.path, &prepared)
                                .map(|()| extract)
                        })
                        .transpose()
                });
                match usable {
                    Ok(Some(extract)) => {
                        counters.cache_hits.fetch_add(1, Ordering::Relaxed);
                        held.sub(prepared.bytes.len() as u64);
                        done.push((
                            index,
                            FileOutcome::Extracted {
                                extract: Box::new(extract),
                                blob_sha: prepared.blob_sha,
                            },
                            prepared.line_count,
                        ));
                        continue;
                    }
                    Ok(None) => counters.cache_misses.fetch_add(1, Ordering::Relaxed),
                    Err(_) => counters.cache_unusable.fetch_add(1, Ordering::Relaxed),
                };
            }
            pending.push((
                index,
                key,
                prepared.blob_sha,
                prepared.digest,
                prepared.line_count,
            ));
            sources.push(SourceFile {
                language: engine_language.to_owned(),
                rel_path: file.path.clone(),
                source: prepared.bytes,
            });
        }
        if sources.is_empty() {
            return Ok(done);
        }

        counters
            .engine_files
            .fetch_add(sources.len() as u64, Ordering::Relaxed);
        // A failure here (an isolated worker that timed out included) fails the
        // stage before anything of the batch is stored or returned.
        let outcomes = backend.extract_batch(&sources)?;
        for ((index, key, blob_sha, digest, lines), (source, outcome)) in
            pending.into_iter().zip(sources.into_iter().zip(outcomes))
        {
            let outcome = match outcome {
                ExtractOutcome::Extracted(extract) => {
                    if extract.rel_path != source.rel_path
                        || extract.language != source.language
                        || extract.source_len != source.source.len() as u64
                        || extract.source_digest != digest
                    {
                        return Err(ExtractError::WrongAnswer(source.rel_path));
                    }
                    if let Some(cache) = cache
                        && is_clean(&extract)
                    {
                        let counter = match cache.store(&key, &extract) {
                            Ok(()) => &counters.cache_writes,
                            Err(_) => &counters.cache_write_failures,
                        };
                        counter.fetch_add(1, Ordering::Relaxed);
                    }
                    FileOutcome::Extracted { extract, blob_sha }
                }
                ExtractOutcome::Failed(ExtractFailure::EngineCrash) => {
                    FileOutcome::EngineCrashed { blob_sha }
                }
                ExtractOutcome::Failed(ExtractFailure::Engine(error)) => {
                    FileOutcome::EngineFailed { blob_sha, error }
                }
            };
            done.push((index, outcome, lines));
        }
        // The batch's sources are released here, with `held`.
        done.sort_by_key(|(index, _, _)| *index);
        Ok(done)
    }
}

/// Where a worker leaves one batch's outcomes, by file index.
type BatchSlot = Mutex<Option<Vec<(usize, FileOutcome, u64)>>>;

/// Whether an extraction may be cached: it is complete and lost no work.
fn is_clean(extract: &FileExtract) -> bool {
    !extract.truncated && extract.extraction_lost == 0
}

/// Whether a cached extraction is the one the engine would return now: clean, and
/// taken as this language, at this path, from these normalised bytes.
fn check_cached(
    extract: &FileExtract,
    engine_language: &str,
    rel_path: &str,
    source: &PreparedSource,
) -> Result<(), CacheDefect> {
    if !is_clean(extract) {
        return Err(CacheDefect::Unclean);
    }
    if extract.language != engine_language
        || extract.rel_path != rel_path
        || extract.source_len != source.bytes.len() as u64
        || extract.source_digest != source.digest
    {
        return Err(CacheDefect::OtherSource);
    }
    Ok(())
}

/// What the workers count between them.
#[derive(Default)]
struct Counters {
    engine_files: AtomicU64,
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
    cache_unusable: AtomicU64,
    cache_writes: AtomicU64,
    cache_write_failures: AtomicU64,
    held_source_bytes: AtomicU64,
    peak_source_bytes: AtomicU64,
}

/// The source bytes one batch holds, counted into the stage's total while held and
/// out of it when released or dropped.
struct Held<'c> {
    counters: &'c Counters,
    bytes: u64,
}

impl<'c> Held<'c> {
    fn new(counters: &'c Counters) -> Self {
        Self { counters, bytes: 0 }
    }

    fn add(&mut self, bytes: u64) {
        self.bytes += bytes;
        let now = self
            .counters
            .held_source_bytes
            .fetch_add(bytes, Ordering::SeqCst)
            + bytes;
        self.counters
            .peak_source_bytes
            .fetch_max(now, Ordering::SeqCst);
    }

    fn sub(&mut self, bytes: u64) {
        self.bytes -= bytes;
        self.counters
            .held_source_bytes
            .fetch_sub(bytes, Ordering::SeqCst);
    }
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        let bytes = self.bytes;
        self.sub(bytes);
    }
}
