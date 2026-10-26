//! The determinism and performance runs (4.13.1, 4.13.3; decision 49).
//!
//! **Determinism**, per golden repository, three builds in fresh worker processes:
//!
//! - A: a cold cache, one Stage 2 worker;
//! - B: another, independent cold cache, [`Settings::parallel_workers`] workers;
//! - C: B's cache, now warm, the same workers.
//!
//! All three must be ready; A and B must have read nothing from their caches, and C
//! must have read every extraction B stored. A, B and C must be the same bytes,
//! compared in chunks with the first differing offset reported, and each reported hash
//! must be its file's. Any difference fails the run and promotes nothing.
//!
//! **Performance**, per golden and scale repository: [`SAMPLES`] builds, each in a fresh
//! process with a fresh cold cache, Stage 2 at [`Settings::perf_workers`] workers and
//! the default memory budget. Each must be ready, read nothing from its cache and have
//! its resident memory observed; the samples must agree on the segment's hash. A worker
//! stopped at the memory ceiling fails a golden's run; for a scale repository it is the
//! recorded outcome (`out_of_memory`), with how far it got, and no further sample.
//!
//! Results are staged and promoted only when every repository has succeeded.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use pdx_core::consts::{DEFAULT_MEMORY_BUDGET_BYTES, EXTRACT_CACHE_FORMAT_VERSION};
use pdx_core::index::Stage;
use sha2::{Digest, Sha256};

use super::area::Area;
use super::host::{self, Host};
use super::measure::{self, MeasureError};
use super::plan::{BenchEntry, BenchPlan, BenchPlanError, Kind};
use super::process::{
    self, Ended, MEMORY_CEILING_BYTES, ProcessError, RssProbe, SAMPLE_INTERVAL, SystemProbe,
};
use super::results::{
    self, Completed, Determinism, OutOfMemory, PDX_VERSION, Performance, Provenance, ResultFile,
    ResultsError, SUMMARY_FILE, Sample, SummaryFile,
};
use super::timer::StageTimer;
use super::worker::{self, BuildStatus, WORKER, WorkerRequest, WorkerResponse};
use crate::golden::cache::{self, Cache, CacheError};
use crate::golden::git::{self, FetchError, Git, Prepared};
use crate::lock::LockError;

/// Variables a benchmark refuses to run with, whatever their value: the engine's
/// extraction switches (`golden::run::EXTRACTION_SWITCHES`, which a test keeps these
/// equal to), isolation, and `pdx`'s own limits, which the harness sets itself.
pub const REFUSED_VARIABLES: [&str; 9] = [
    "PDX_ENGINE_WALK_MAX_NODES",
    "PDX_ENGINE_WALK_DEFS_MAX",
    "PDX_ENGINE_TS_TYPE_BUDGET",
    "PDX_ENGINE_LSP_MAX_WALK_DEPTH",
    "PDX_ENGINE_LSP_DISABLED",
    "PDX_ENGINE_INDEX_QUARANTINE_FILE",
    "PDX_ENGINE_ISOLATE",
    "PDX_WORKERS",
    "PDX_MEM_BUDGET_MB",
];

/// Performance samples per repository.
pub const SAMPLES: usize = 3;

/// Bytes compared at a time.
const CHUNK: usize = 64 * 1024;

/// Why a benchmark run failed.
#[derive(Debug, thiserror::Error)]
pub enum BenchError {
    /// A refused variable is set.
    #[error(
        "{0} is set: benchmarks run the engine in process with its defaults and the harness's own limits; unset it and run again"
    )]
    Environment(&'static str),
    /// The directory is not this workspace's root.
    #[error(
        "{0} is not the workspace root (no Cargo.toml, bench/repos.lock and crates/pdx-bench there); run from the root"
    )]
    Workspace(PathBuf),
    /// The lock file cannot be read.
    #[error(transparent)]
    Lock(#[from] LockError),
    /// The lock makes no plan.
    #[error(transparent)]
    Plan(#[from] BenchPlanError),
    /// A cache path is unsafe.
    #[error(transparent)]
    Cache(#[from] CacheError),
    /// Fetching failed.
    #[error(transparent)]
    Fetch(#[from] FetchError),
    /// The worker could not be run.
    #[error(transparent)]
    Process(#[from] ProcessError),
    /// The worker failed.
    #[error("the benchmark worker failed (exit {code}): {stderr}")]
    Worker {
        /// Its exit code, or `signal`.
        code: String,
        /// The start of its standard error.
        stderr: String,
    },
    /// The worker's answer is not one.
    #[error("the benchmark worker's answer is not one: {0}")]
    Response(String),
    /// The worker's progress file is not one.
    #[error("the benchmark worker's progress file: {0}")]
    Progress(String),
    /// `--only` names no repository of the plan.
    #[error("no repository {0} in bench/repos.lock to measure; the plan has: {1}")]
    UnknownRepository(String, String),
    /// A build is degraded.
    #[error(
        "the {label} build is degraded ({reasons}); baselines are taken from ready builds only"
    )]
    Degraded {
        /// Which build.
        label: String,
        /// Its reasons, counted.
        reasons: String,
    },
    /// A golden's worker exceeded the memory ceiling.
    #[error(
        "the {label} build exceeded the memory ceiling of {ceiling} bytes (resident {peak} bytes) and was stopped"
    )]
    OverCeiling {
        /// Which build.
        label: String,
        /// The ceiling.
        ceiling: u64,
        /// What was observed.
        peak: u64,
    },
    /// A performance sample's resident memory was never observed.
    #[error("the resident memory of sample {0}'s worker could not be observed")]
    NoRss(usize),
    /// A performance sample read its cold cache.
    #[error("sample {sample} read {hits} extraction(s) from a cache that should have been cold")]
    WarmSample {
        /// The sample.
        sample: usize,
        /// What it read.
        hits: u64,
    },
    /// A performance sample's segment is not the one it reported.
    #[error("sample {0}'s segment does not hash to the hash it reported")]
    SampleHash(usize),
    /// The samples built different segments.
    #[error("the {SAMPLES} samples built different segments ({0})")]
    SamplesDiffer(String),
    /// A measure could not be taken.
    #[error(transparent)]
    Measure(#[from] MeasureError),
    /// Determinism failed.
    #[error("{0} determinism failure(s); nothing was written")]
    NotDeterministic(usize),
    /// The results could not be written.
    #[error(transparent)]
    Results(#[from] ResultsError),
    /// One repository failed.
    #[error("{key}: {source}")]
    Repository {
        /// The repository.
        key: String,
        /// Why.
        source: Box<BenchError>,
    },
    /// A file could not be read.
    #[error("{path}: {source}")]
    Io {
        /// The path.
        path: PathBuf,
        /// What went wrong.
        #[source]
        source: io::Error,
    },
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> BenchError + '_ {
    move |source| BenchError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Refuses to run when a refused variable is set; names it, never its value.
///
/// # Errors
///
/// [`BenchError::Environment`] for the first one set.
pub fn preflight(is_set: impl Fn(&str) -> bool) -> Result<(), BenchError> {
    match REFUSED_VARIABLES.iter().find(|name| is_set(name)) {
        Some(name) => Err(BenchError::Environment(name)),
        None => Ok(()),
    }
}

/// What a run measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Modes {
    /// `--determinism`.
    pub determinism: bool,
    /// `--perf`.
    pub performance: bool,
}

/// The limits builds run with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Determinism builds B and C: the parallelism available, at least 2, so the
    /// comparison with build A's single worker is never of two equal settings.
    pub parallel_workers: usize,
    /// Performance builds: the parallelism available, at least 1.
    pub perf_workers: usize,
    /// Stage 2's memory budget: `DEFAULT_MEMORY_BUDGET_BYTES`.
    pub memory_budget_bytes: u64,
    /// The resident memory above which a worker is stopped.
    pub ceiling: u64,
}

impl Settings {
    /// This machine's.
    pub fn current() -> Self {
        let available = host::available_workers();
        Self {
            parallel_workers: available.max(2),
            perf_workers: available,
            memory_budget_bytes: DEFAULT_MEMORY_BUDGET_BYTES,
            ceiling: MEMORY_CEILING_BYTES,
        }
    }
}

/// One build to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildSpec {
    /// Its name in messages.
    pub label: String,
    /// The checkout.
    pub checkout: PathBuf,
    /// `https://<lock url>`.
    pub repo_url: String,
    /// The commit.
    pub commit: String,
    /// The extraction cache.
    pub cache_dir: PathBuf,
    /// Where the segment goes.
    pub destination: PathBuf,
    /// The build's own directory: its temporary segment and progress file.
    pub work_dir: PathBuf,
    /// Stage 2's workers.
    pub workers: usize,
    /// Stage 2's memory budget.
    pub memory_budget_bytes: u64,
}

/// A build that ran to the end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Built {
    /// The worker's answer.
    pub response: WorkerResponse,
    /// Its largest resident-memory sample.
    pub peak_rss_bytes: Option<u64>,
    /// How many samples were taken.
    pub rss_samples: u64,
}

/// A build stopped at the memory ceiling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stopped {
    /// The sample over the ceiling.
    pub peak_rss_bytes: u64,
    /// How many samples were taken.
    pub rss_samples: u64,
    /// From the worker's start to its stop.
    pub wall_ns: u64,
    /// Its progress, replayed.
    pub progress: StageTimer,
}

/// What a build did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildOutcome {
    /// It finished.
    Built(Built),
    /// It was stopped at the ceiling.
    Stopped(Stopped),
}

/// Runs builds. The production builder runs the worker process ([`WorkerBuilder`]);
/// tests supply their own.
pub trait Builder {
    /// Runs `spec`.
    ///
    /// # Errors
    ///
    /// Whatever stops it; the run then fails with nothing promoted.
    fn build(&mut self, spec: &BuildSpec) -> Result<BuildOutcome, BenchError>;
}

/// The production builder: `<program> __pdx-bench-worker`, monitored.
pub struct WorkerBuilder {
    program: PathBuf,
    probe: Box<dyn RssProbe>,
    ceiling: u64,
}

impl WorkerBuilder {
    /// Runs `program` (this harness's own executable) as the worker, stopping it above
    /// `ceiling` bytes.
    pub fn new(program: PathBuf, ceiling: u64) -> Self {
        Self {
            program,
            probe: Box::new(SystemProbe::new()),
            ceiling,
        }
    }
}

impl Builder for WorkerBuilder {
    fn build(&mut self, spec: &BuildSpec) -> Result<BuildOutcome, BenchError> {
        let progress = spec.work_dir.join("progress.jsonl");
        let request = WorkerRequest {
            checkout: spec.checkout.clone(),
            repo_url: spec.repo_url.clone(),
            commit: spec.commit.clone(),
            cache_dir: spec.cache_dir.clone(),
            destination: spec.destination.clone(),
            build_dir: spec.work_dir.clone(),
            progress: progress.clone(),
            workers: spec.workers,
            memory_budget_bytes: spec.memory_budget_bytes,
        };
        let input =
            serde_json::to_vec(&request).map_err(|e| BenchError::Response(e.to_string()))?;
        let mut command = Command::new(&self.program);
        command.arg(WORKER);
        for name in REFUSED_VARIABLES {
            command.env_remove(name);
        }
        let started = Instant::now();
        let run = process::run_monitored(
            command,
            &input,
            self.probe.as_mut(),
            self.ceiling,
            SAMPLE_INTERVAL,
        )?;
        let wall_ns = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        match run.ended {
            Ended::OverCeiling => {
                let progress = worker::read_progress(&progress)
                    .map_err(BenchError::Progress)?
                    .unwrap_or_default();
                Ok(BuildOutcome::Stopped(Stopped {
                    peak_rss_bytes: run.peak.peak().unwrap_or(0),
                    rss_samples: run.peak.samples(),
                    wall_ns,
                    progress,
                }))
            }
            Ended::Exited(Some(0)) => {
                let response: WorkerResponse = serde_json::from_slice(&run.stdout.bytes)
                    .map_err(|e| BenchError::Response(e.to_string()))?;
                Ok(BuildOutcome::Built(Built {
                    response,
                    peak_rss_bytes: run.peak.peak(),
                    rss_samples: run.peak.samples(),
                }))
            }
            Ended::Exited(code) => Err(BenchError::Worker {
                code: code.map_or_else(|| "signal".to_owned(), |c| c.to_string()),
                stderr: run.stderr.text(),
            }),
        }
    }
}

/// Provides checkouts.
pub trait Checkouts {
    /// The checkout of `entry` at its pinned commit.
    ///
    /// # Errors
    ///
    /// Whatever stops it.
    fn checkout(&mut self, entry: &BenchEntry, err: &mut dyn Write) -> Result<PathBuf, BenchError>;
}

/// Pinned checkouts below `bench/.cache/repos/`, fetched as the golden harness fetches
/// them (P2-12): `<language>/<repo>` for goldens, shared with `make golden`, and
/// `scale/<name>` for scale repositories.
#[derive(Debug)]
pub struct GitCheckouts {
    cache: Cache,
    git: Git,
}

impl GitCheckouts {
    /// Checkouts in `cache`.
    ///
    /// # Errors
    ///
    /// Cache errors.
    pub fn new(cache: Cache) -> Result<Self, BenchError> {
        let git = Git::new(cache.no_hooks()?, cache.git_global()?);
        Ok(Self { cache, git })
    }
}

impl Checkouts for GitCheckouts {
    fn checkout(&mut self, entry: &BenchEntry, err: &mut dyn Write) -> Result<PathBuf, BenchError> {
        let [first, second] = entry.cache_components();
        let mut parent = self.cache.root().join("repos");
        cache::ensure_dir(&parent)?;
        parent.push(first);
        cache::ensure_dir(&parent)?;
        let dir = parent.join(second);
        let prepared = git::prepare_checkout(&self.git, &dir, &entry.fetch_url(), &entry.commit)?;
        let how = match prepared {
            Prepared::Reused => "checkout reused",
            Prepared::Fetched => "commit fetched",
        };
        let _ = writeln!(err, "pdx-bench: {}: {how}", entry.key());
        Ok(dir)
    }
}

/// A run.
#[derive(Debug)]
pub struct Run<'a> {
    /// The repositories.
    pub plan: &'a BenchPlan,
    /// What to measure.
    pub modes: Modes,
    /// The builds' limits.
    pub settings: Settings,
    /// The machine.
    pub host: Host,
    /// The working area.
    pub area: &'a Area,
    /// `bench/results/`.
    pub results_root: PathBuf,
    /// The repositories to measure, by key (`kubernetes`, `go/gin`); empty for every one
    /// the run's modes cover. Every other result is kept as it is.
    pub only: Vec<String>,
}

/// The three determinism builds of an entry checked out at `checkout`, in `dir`.
pub fn determinism_specs(
    entry: &BenchEntry,
    checkout: &Path,
    dir: &Path,
    settings: &Settings,
) -> [BuildSpec; 3] {
    let spec = |label: &str, cache: &str, segment: &str, work: &str, workers: usize| BuildSpec {
        label: label.to_owned(),
        checkout: checkout.to_path_buf(),
        repo_url: entry.fetch_url(),
        commit: entry.commit.clone(),
        cache_dir: dir.join(cache),
        destination: dir.join(segment),
        work_dir: dir.join(work),
        workers,
        memory_budget_bytes: settings.memory_budget_bytes,
    };
    [
        spec("cold single", "cache-a", "a.db", "build-a", 1),
        spec(
            "cold parallel",
            "cache-b",
            "b.db",
            "build-b",
            settings.parallel_workers,
        ),
        spec(
            "warm parallel",
            "cache-b",
            "warm.db",
            "build-warm",
            settings.parallel_workers,
        ),
    ]
}

/// Performance sample `n` of an entry, in its fresh directory `dir`.
pub fn sample_spec(
    entry: &BenchEntry,
    checkout: &Path,
    dir: &Path,
    n: usize,
    settings: &Settings,
) -> BuildSpec {
    BuildSpec {
        label: format!("sample {n}"),
        checkout: checkout.to_path_buf(),
        repo_url: entry.fetch_url(),
        commit: entry.commit.clone(),
        cache_dir: dir.join("cache"),
        destination: dir.join("segment.db"),
        work_dir: dir.join("build"),
        workers: settings.perf_workers,
        memory_budget_bytes: settings.memory_budget_bytes,
    }
}

/// The first byte at which two files differ: `None` when they are the same bytes.
///
/// # Errors
///
/// When either cannot be read.
pub fn first_difference(left: &Path, right: &Path) -> Result<Option<u64>, BenchError> {
    let mut left_file = File::open(left).map_err(io_error(left))?;
    let mut right_file = File::open(right).map_err(io_error(right))?;
    let (mut left_chunk, mut right_chunk) = (vec![0_u8; CHUNK], vec![0_u8; CHUNK]);
    let mut offset = 0_u64;
    loop {
        let left_len = read_full(&mut left_file, &mut left_chunk).map_err(io_error(left))?;
        let right_len = read_full(&mut right_file, &mut right_chunk).map_err(io_error(right))?;
        let common = left_len.min(right_len);
        if let Some(at) = left_chunk[..common]
            .iter()
            .zip(&right_chunk[..common])
            .position(|(l, r)| l != r)
        {
            return Ok(Some(offset + at as u64));
        }
        if left_len != right_len {
            return Ok(Some(offset + common as u64));
        }
        if left_len == 0 {
            return Ok(None);
        }
        offset += left_len as u64;
    }
}

/// Reads until `buffer` is full or the file ends.
fn read_full(file: &mut File, buffer: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match file.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

/// `lowerhex(sha256)` of a file's bytes, and its size.
///
/// # Errors
///
/// When it cannot be read.
pub fn file_sha256(path: &Path) -> Result<(String, u64), BenchError> {
    let mut file = File::open(path).map_err(io_error(path))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; CHUNK];
    let mut size = 0_u64;
    loop {
        let n = read_full(&mut file, &mut buffer).map_err(io_error(path))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        size += n as u64;
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    Ok((hex, size))
}

/// Whether a reported hash and size are the file's.
fn verified(path: &Path, response: &WorkerResponse) -> Result<bool, BenchError> {
    let (hash, size) = file_sha256(path)?;
    Ok(hash == response.content_sha256 && size == response.size_bytes)
}

/// What a degraded sample lost, said with the throughput: the first such sample's
/// reasons (the samples' segments are equal, so their reasons are too).
fn degraded_note(samples: &[Sample]) -> String {
    samples
        .iter()
        .find(|sample| !sample.degraded.is_empty())
        .map(|sample| {
            let reasons: Vec<String> = sample
                .degraded
                .iter()
                .map(|r| format!("{} {} file(s)", r.kind, r.files))
                .collect();
            format!(", degraded ({})", reasons.join("; "))
        })
        .unwrap_or_default()
}

/// The degraded reason of work extraction refuses by design: a file past one of its
/// bounds (issue 89), never cached.
const LOST_WORK: &str = "extraction_lost";

fn require_ready(label: &str, response: &WorkerResponse) -> Result<(), BenchError> {
    match response.status {
        BuildStatus::Ready => Ok(()),
        BuildStatus::Degraded => Err(BenchError::Degraded {
            label: label.to_owned(),
            reasons: response
                .degraded
                .iter()
                .map(|r| format!("{}: {} file(s)", r.kind, r.files))
                .collect::<Vec<_>>()
                .join("; "),
        }),
    }
}

/// A completed build, or the ceiling's error.
fn completed(spec: &BuildSpec, outcome: BuildOutcome, ceiling: u64) -> Result<Built, BenchError> {
    match outcome {
        BuildOutcome::Built(built) => Ok(built),
        BuildOutcome::Stopped(stopped) => Err(BenchError::OverCeiling {
            label: spec.label.clone(),
            ceiling,
            peak: stopped.peak_rss_bytes,
        }),
    }
}

fn short(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

/// Checks a golden's three builds; every finding is a line of the failure.
///
/// # Errors
///
/// [`BenchError`] when a build is not ready or a file cannot be read.
pub fn check_determinism(
    specs: &[BuildSpec; 3],
    builds: &[Built; 3],
) -> Result<Result<Determinism, Vec<String>>, BenchError> {
    for (spec, built) in specs.iter().zip(builds) {
        require_ready(&spec.label, &built.response)?;
    }
    let [single, parallel, warm] = builds.each_ref().map(|built| &built.response);
    let mut findings = Vec::new();
    for (left, right) in [(0, 1), (0, 2)] {
        let (x, y) = (&builds[left].response, &builds[right].response);
        let offset = first_difference(&specs[left].destination, &specs[right].destination)?;
        if offset.is_some() || x.content_sha256 != y.content_sha256 {
            findings.push(format!(
                "{} and {} differ: sizes {} and {}, hashes {} and {}, first difference at byte {}",
                specs[left].label,
                specs[right].label,
                x.size_bytes,
                y.size_bytes,
                short(&x.content_sha256),
                short(&y.content_sha256),
                offset.map_or_else(|| "none (same bytes)".to_owned(), |o| o.to_string()),
            ));
        }
    }
    for (spec, built) in specs.iter().zip(builds) {
        if !verified(&spec.destination, &built.response)? {
            findings.push(format!(
                "the {} build's segment does not hash to the {} it reported",
                spec.label,
                short(&built.response.content_sha256)
            ));
        }
    }
    for (spec, built) in specs[..2].iter().zip(&builds[..2]) {
        if built.response.cache_hits > 0 {
            findings.push(format!(
                "the {} build read {} extraction(s) from a cache that should have been cold",
                spec.label, built.response.cache_hits
            ));
        }
    }
    if warm.cache_hits == 0 || warm.cache_hits != parallel.cache_writes {
        findings.push(format!(
            "the {} build read {} extraction(s) from its cache, not the {} the {} build stored",
            specs[2].label, warm.cache_hits, parallel.cache_writes, specs[1].label
        ));
    }
    if !findings.is_empty() {
        return Ok(Err(findings));
    }
    Ok(Ok(Determinism {
        cold_single_hash: single.content_sha256.clone(),
        cold_parallel_hash: parallel.content_sha256.clone(),
        warm_parallel_hash: warm.content_sha256.clone(),
        cold_builds_byte_identical: true,
        cold_warm_byte_identical: true,
        rebuild_equivalent: single.content_sha256 == warm.content_sha256,
        workers_single: specs[0].workers as u64,
        workers_parallel: specs[1].workers as u64,
        segment_size_bytes: single.size_bytes,
        files: single.files,
        cold_cache_writes: parallel.cache_writes,
        warm_cache_hits: warm.cache_hits,
    }))
}

/// One completed performance sample's record. A scale repository's sample may be
/// degraded by lost work alone, the refusals extraction makes by design, and records
/// its reasons (decision 86); a golden's must be ready.
///
/// # Errors
///
/// [`BenchError`] for a degraded sample not admitted so, a warm or unobserved one, or
/// one whose segment is not what it reported.
pub fn sample_of(
    n: usize,
    spec: &BuildSpec,
    built: &Built,
    kind: &Kind,
) -> Result<Sample, BenchError> {
    let response = &built.response;
    let admitted = matches!(kind, Kind::Scale)
        && !response.degraded.is_empty()
        && response
            .degraded
            .iter()
            .all(|reason| reason.kind == LOST_WORK);
    if !admitted {
        require_ready(&spec.label, response)?;
    }
    if response.cache_hits > 0 {
        return Err(BenchError::WarmSample {
            sample: n,
            hits: response.cache_hits,
        });
    }
    let peak = built
        .peak_rss_bytes
        .filter(|bytes| *bytes > 0)
        .ok_or(BenchError::NoRss(n))?;
    if !verified(&spec.destination, response)? {
        return Err(BenchError::SampleHash(n));
    }
    let stages = &response.stages;
    Ok(Sample {
        sample: n as u64,
        status: match response.status {
            BuildStatus::Ready => "ready",
            BuildStatus::Degraded => "degraded",
        }
        .to_owned(),
        degraded: response.degraded.clone(),
        content_sha256: response.content_sha256.clone(),
        size_bytes: response.size_bytes,
        files_discovered: stages.items(Stage::Discover),
        files: response.files,
        call_sites: stages.items(Stage::Resolve),
        nodes: stages.items(Stage::Derive),
        workers: spec.workers as u64,
        memory_budget_bytes: spec.memory_budget_bytes,
        discover_ns: stages.duration_ns(Stage::Discover),
        extract_ns: stages.duration_ns(Stage::Extract),
        resolve_ns: stages.duration_ns(Stage::Resolve),
        derive_ns: stages.duration_ns(Stage::Derive),
        write_ns: stages.duration_ns(Stage::Write),
        total_wall_ns: response.total_wall_ns,
        files_per_second: measure::files_per_second(
            response.files,
            stages.duration_ns(Stage::Extract),
            stages.duration_ns(Stage::Resolve),
        )?,
        peak_rss_bytes: peak,
        rss_samples: built.rss_samples,
    })
}

/// The baseline of a repository's samples: medians of time and throughput, the
/// maximum of the peaks, every sample kept.
///
/// # Errors
///
/// [`BenchError::SamplesDiffer`] when they built different segments; measure errors.
pub fn aggregate(
    samples: Vec<Sample>,
    host: &Host,
    settings: &Settings,
) -> Result<Completed, BenchError> {
    let first = samples.first().ok_or(MeasureError::Empty)?;
    if samples
        .iter()
        .any(|s| s.content_sha256 != first.content_sha256)
    {
        let hashes: Vec<&str> = samples.iter().map(|s| short(&s.content_sha256)).collect();
        return Err(BenchError::SamplesDiffer(hashes.join(", ")));
    }
    let median = |field: fn(&Sample) -> u64| {
        measure::median_u64(&samples.iter().map(field).collect::<Vec<_>>())
    };
    let rates: Vec<f64> = samples.iter().map(|s| s.files_per_second).collect();
    let peaks: Vec<u64> = samples.iter().map(|s| s.peak_rss_bytes).collect();
    Ok(Completed {
        host: host.clone(),
        rss_sample_interval_ms: u64::try_from(SAMPLE_INTERVAL.as_millis()).unwrap_or(u64::MAX),
        memory_ceiling_bytes: settings.ceiling,
        content_sha256: first.content_sha256.clone(),
        files: first.files,
        median_files_per_second: measure::median_f64(&rates)?,
        median_discover_ns: median(|s| s.discover_ns)?,
        median_extract_ns: median(|s| s.extract_ns)?,
        median_resolve_ns: median(|s| s.resolve_ns)?,
        median_derive_ns: median(|s| s.derive_ns)?,
        median_write_ns: median(|s| s.write_ns)?,
        median_total_wall_ns: median(|s| s.total_wall_ns)?,
        peak_rss_bytes: measure::maximum(&peaks)?,
        samples,
    })
}

/// Clean extractions stored in a cache directory: its object files, never its
/// temporary ones.
pub fn cached_extractions(cache_dir: &Path) -> u64 {
    let objects = cache_dir.join(format!("v{EXTRACT_CACHE_FORMAT_VERSION}"));
    let Ok(prefixes) = fs::read_dir(&objects) else {
        return 0;
    };
    let mut count = 0;
    for prefix in prefixes.flatten() {
        if !prefix.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Ok(files) = fs::read_dir(prefix.path()) else {
            continue;
        };
        count += files
            .flatten()
            .filter(|f| f.file_type().is_ok_and(|t| t.is_file()))
            .filter(|f| {
                f.file_name()
                    .to_str()
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_hexdigit()))
            })
            .count() as u64;
    }
    count
}

/// A scale repository's stopped sample, as data.
pub fn out_of_memory(
    n: usize,
    spec: &BuildSpec,
    stopped: &Stopped,
    host: &Host,
    settings: &Settings,
) -> OutOfMemory {
    let finished = stopped.progress.finished();
    let items = |stage: Stage| finished.iter().find(|r| r.stage == stage).map(|r| r.items);
    OutOfMemory {
        host: host.clone(),
        rss_sample_interval_ms: u64::try_from(SAMPLE_INTERVAL.as_millis()).unwrap_or(u64::MAX),
        memory_ceiling_bytes: settings.ceiling,
        sample: n as u64,
        workers: spec.workers as u64,
        memory_budget_bytes: spec.memory_budget_bytes,
        peak_rss_bytes: stopped.peak_rss_bytes,
        rss_samples: stopped.rss_samples,
        wall_ns_at_stop: stopped.wall_ns,
        stage_reached: stopped
            .progress
            .reached()
            .map_or("none", Stage::as_str)
            .to_owned(),
        completed_stages: finished.to_vec(),
        files_discovered: items(Stage::Discover),
        files_extracted: items(Stage::Extract),
        cached_extractions_at_stop: cached_extractions(&spec.cache_dir),
    }
}

fn mib(bytes: u64) -> u64 {
    bytes / (1024 * 1024)
}

#[allow(clippy::cast_precision_loss)] // for display only
fn seconds(ns: u64) -> f64 {
    ns as f64 / 1e9
}

fn in_context(entry: &BenchEntry) -> impl Fn(BenchError) -> BenchError + '_ {
    move |source| BenchError::Repository {
        key: entry.key(),
        source: Box::new(source),
    }
}

fn prepare(spec: &BuildSpec) -> Result<(), BenchError> {
    cache::ensure_dir(&spec.work_dir)?;
    Ok(())
}

impl Run<'_> {
    fn determinism(
        &self,
        entry: &BenchEntry,
        checkout: &Path,
        builder: &mut dyn Builder,
        err: &mut dyn Write,
    ) -> Result<Result<Determinism, Vec<String>>, BenchError> {
        let dir = self.area.fresh_determinism(entry)?;
        let specs = determinism_specs(entry, checkout, &dir, &self.settings);
        let mut builds = Vec::with_capacity(3);
        for spec in &specs {
            prepare(spec)?;
            let _ = writeln!(
                err,
                "pdx-bench: {}: determinism, {} build ({} worker(s))",
                entry.key(),
                spec.label,
                spec.workers
            );
            builds.push(completed(
                spec,
                builder.build(spec)?,
                self.settings.ceiling,
            )?);
        }
        let builds: [Built; 3] = builds
            .try_into()
            .map_err(|_| BenchError::Response("three builds".to_owned()))?;
        check_determinism(&specs, &builds)
    }

    fn performance(
        &self,
        entry: &BenchEntry,
        checkout: &Path,
        builder: &mut dyn Builder,
        err: &mut dyn Write,
    ) -> Result<Performance, BenchError> {
        let mut samples = Vec::with_capacity(SAMPLES);
        for n in 1..=SAMPLES {
            let dir = self.area.fresh_sample(entry, n)?;
            let spec = sample_spec(entry, checkout, &dir, n, &self.settings);
            prepare(&spec)?;
            let _ = writeln!(
                err,
                "pdx-bench: {}: performance, sample {n} of {SAMPLES} ({} worker(s))",
                entry.key(),
                spec.workers
            );
            match builder.build(&spec)? {
                BuildOutcome::Built(built) => {
                    samples.push(sample_of(n, &spec, &built, &entry.kind)?);
                }
                BuildOutcome::Stopped(stopped) => {
                    return match entry.kind {
                        Kind::Golden { .. } => Err(BenchError::OverCeiling {
                            label: spec.label,
                            ceiling: self.settings.ceiling,
                            peak: stopped.peak_rss_bytes,
                        }),
                        Kind::Scale => Ok(Performance::OutOfMemory(out_of_memory(
                            n,
                            &spec,
                            &stopped,
                            &self.host,
                            &self.settings,
                        ))),
                    };
                }
            }
        }
        Ok(Performance::Completed(aggregate(
            samples,
            &self.host,
            &self.settings,
        )?))
    }

    /// The merged result files of every planned repository and the summary, by file
    /// name, and the summary's sections.
    fn result_files(
        &self,
        results_dir: &Path,
        determinism: &BTreeMap<String, Determinism>,
        performance: &BTreeMap<String, Performance>,
    ) -> Result<BTreeMap<String, Vec<u8>>, BenchError> {
        let mut files = BTreeMap::new();
        let mut current: Vec<(&BenchEntry, ResultFile)> = Vec::new();
        for entry in self.plan.all() {
            let key = entry.key();
            let path = results_dir.join(entry.result_file());
            let existing = results::read_existing(&path)?
                .map(|bytes| ResultFile::parse(&bytes, &path))
                .transpose()?;
            let provenance = Provenance::of(entry);
            let fresh_determinism = determinism.get(&key).cloned();
            let fresh_performance = performance.get(&key).cloned();
            if fresh_determinism.is_none() && fresh_performance.is_none() {
                if let Some(old) = existing.filter(|old| old.provenance == provenance) {
                    current.push((entry, old));
                }
                continue;
            }
            let fresh = ResultFile::new(provenance, fresh_determinism, fresh_performance);
            let merged = results::merge(existing, fresh, &path)?;
            files.insert(entry.result_file(), merged.to_bytes());
            current.push((entry, merged));
        }
        let pairs: Vec<(&BenchEntry, &ResultFile)> = current.iter().map(|(e, f)| (*e, f)).collect();
        let summary_path = results_dir.join(SUMMARY_FILE);
        let existing = results::read_existing(&summary_path)?
            .map(|bytes| SummaryFile::parse(&bytes, &summary_path))
            .transpose()?;
        let summary = SummaryFile::merged(existing, results::summarise(&pairs), &summary_path)?;
        files.insert(SUMMARY_FILE.to_owned(), summary.to_bytes());
        Ok(files)
    }
}

/// Runs the plan: determinism, then performance, then the results, promoted only when
/// everything succeeded. One deterministic line per repository goes to `out`;
/// progress and failures go to `err`.
///
/// # Errors
///
/// [`BenchError::NotDeterministic`] when a golden's builds differ;
/// [`BenchError::Repository`] when a repository fails; results errors.
pub fn run(
    run: &Run<'_>,
    checkouts: &mut dyn Checkouts,
    builder: &mut dyn Builder,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), BenchError> {
    let results_dir = run.results_root.join(PDX_VERSION);
    for name in &run.only {
        if !run.plan.all().iter().any(|entry| entry.key() == *name) {
            let keys: Vec<String> = run.plan.all().iter().map(|e| e.key()).collect();
            return Err(BenchError::UnknownRepository(name.clone(), keys.join(", ")));
        }
    }
    let measured =
        |entry: &&BenchEntry| run.only.is_empty() || run.only.iter().any(|n| *n == entry.key());
    // Unsafe result paths stop the run before anything is built.
    cache::ensure_dir(&run.results_root)?;
    cache::ensure_dir(&results_dir)?;
    let mut checked_out: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut checkout = |entry: &BenchEntry, err: &mut dyn Write| -> Result<PathBuf, BenchError> {
        if let Some(dir) = checked_out.get(&entry.key()) {
            return Ok(dir.clone());
        }
        let dir = checkouts.checkout(entry, err).map_err(in_context(entry))?;
        checked_out.insert(entry.key(), dir.clone());
        Ok(dir)
    };

    let mut determinism = BTreeMap::new();
    if run.modes.determinism {
        let mut failures = 0;
        for entry in run.plan.determinism().into_iter().filter(measured) {
            let dir = checkout(entry, err)?;
            match run
                .determinism(entry, &dir, builder, err)
                .map_err(in_context(entry))?
            {
                Ok(result) => {
                    let _ = writeln!(
                        out,
                        "determinism: {}: identical, {} files, cold and warm rebuilds equivalent, segment {}",
                        entry.key(),
                        result.files,
                        result.cold_single_hash
                    );
                    determinism.insert(entry.key(), result);
                }
                Err(findings) => {
                    failures += findings.len();
                    for finding in findings {
                        let _ = writeln!(err, "determinism: {}: {finding}", entry.key());
                    }
                }
            }
        }
        if failures > 0 {
            return Err(BenchError::NotDeterministic(failures));
        }
    }

    let mut performance = BTreeMap::new();
    if run.modes.performance {
        for entry in run.plan.performance().into_iter().filter(measured) {
            let dir = checkout(entry, err)?;
            let result = run
                .performance(entry, &dir, builder, err)
                .map_err(in_context(entry))?;
            let line = match &result {
                Performance::Completed(c) => format!(
                    "{:.1} files/s (median of {SAMPLES}), {} files, peak RSS {} MiB, median wall {:.2} s{}",
                    c.median_files_per_second,
                    c.files,
                    mib(c.peak_rss_bytes),
                    seconds(c.median_total_wall_ns),
                    degraded_note(&c.samples)
                ),
                Performance::OutOfMemory(o) => format!(
                    "out of memory: stopped in the {} stage at {} MiB, above the {} MiB ceiling",
                    o.stage_reached,
                    mib(o.peak_rss_bytes),
                    mib(o.memory_ceiling_bytes)
                ),
            };
            let _ = writeln!(out, "perf: {}: {line}", entry.key());
            performance.insert(entry.key(), result);
        }
    }

    let files = run.result_files(&results_dir, &determinism, &performance)?;
    let staging = run.area.fresh_staging(PDX_VERSION)?;
    results::stage_and_promote(&staging, &results_dir, &files)?;
    let _ = writeln!(
        out,
        "bench: wrote {} file(s) to bench/results/{PDX_VERSION}/",
        files.len()
    );
    Ok(())
}
