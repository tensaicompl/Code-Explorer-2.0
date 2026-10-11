//! The benchmark worker: one `build_segment` in a process of its own, so every sample
//! starts from a fresh process and its resident memory is its own.
//!
//! `pdx-bench` serves it when its first argument is [`WORKER`], before the command
//! line is parsed, so it is no subcommand and appears in no help. It reads one
//! [`WorkerRequest`] (JSON) from standard input, builds the segment with the engine in
//! this process (never an isolated worker: the harness refuses `PDX_ENGINE_ISOLATE`),
//! and writes one [`WorkerResponse`] (JSON) to standard output. A failure is one line
//! on standard error and exit 1.
//!
//! The repository's identity is derived from its URL exactly as `pdx index` derives it
//! from `origin` (decision 36), so a benchmark segment is byte-identical to the one
//! `pdx index` writes for the same commit.
//!
//! As each stage starts and finishes, the worker appends a line to the request's
//! progress file (stage, count, nanoseconds since the worker's origin), so a worker
//! stopped part-way still says how far it got.

use std::cell::RefCell;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use pdx_core::ids::{canonical_clone_url, repo_id_from_url};
use pdx_core::index::extract::{ExtractCache, ExtractLimits, FsCache};
use pdx_core::index::{
    DegradedReason, IndexRequest, Progress, SegmentReport, SegmentStatus, Stage, build_segment,
};
use pdx_core::segment::RepoIdSource;
use serde::{Deserialize, Serialize};

use super::timer::{self, Clock, MonotonicClock, StageTimer, StageTimes, TimerError};

/// The hidden first argument that makes `pdx-bench` a worker.
pub const WORKER: &str = "__pdx-bench-worker";

/// The most bytes of request the worker reads.
const REQUEST_LIMIT: u64 = 64 * 1024;

/// One build.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerRequest {
    /// The checkout: clean, at `commit`.
    pub checkout: PathBuf,
    /// The URL the checkout was fetched from: `https://<lock url>`.
    pub repo_url: String,
    /// The commit the checkout is.
    pub commit: String,
    /// The extraction cache's directory: empty for a cold build.
    pub cache_dir: PathBuf,
    /// Where the segment is written: nothing may be there.
    pub destination: PathBuf,
    /// Where the segment is built before it is written.
    pub build_dir: PathBuf,
    /// The progress file: nothing may be there.
    pub progress: PathBuf,
    /// Stage 2's workers.
    pub workers: usize,
    /// Stage 2's memory budget.
    pub memory_budget_bytes: u64,
}

/// A segment's status, as 4.4 spells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildStatus {
    /// Ready.
    Ready,
    /// Degraded.
    Degraded,
}

/// One degraded reason: its kind and how many files it names, never the files.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasonCount {
    /// The reason.
    pub kind: String,
    /// The files it concerns.
    pub files: u64,
}

/// What one build did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerResponse {
    /// Ready or degraded.
    pub status: BuildStatus,
    /// The segment's content hash, as the writer computed it.
    pub content_sha256: String,
    /// Its size.
    pub size_bytes: u64,
    /// The files given to extraction (`Finished(Extract)`).
    pub files: u64,
    /// The five stages, with their counts and durations.
    pub stages: StageTimes,
    /// The whole `build_segment` call, timed apart from the stages.
    pub total_wall_ns: u64,
    /// Stage 2's workers allowed by the budget.
    pub effective_workers: u64,
    /// Extractions read from the cache.
    pub cache_hits: u64,
    /// Extractions the cache had no entry for.
    pub cache_misses: u64,
    /// Extractions stored in the cache.
    pub cache_writes: u64,
    /// Why the segment is degraded, if it is.
    pub degraded: Vec<ReasonCount>,
}

/// Why the worker built nothing.
#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    /// The request could not be read.
    #[error("the request is not one this worker reads: {0}")]
    Request(String),
    /// The URL is not one an identity is made from.
    #[error("the repository URL is not an HTTPS clone URL")]
    Url,
    /// The progress file could not be written.
    #[error("the progress file: {0}")]
    Progress(String),
    /// The build failed.
    #[error("the build failed: {0}")]
    Build(String),
    /// The progress events time no build.
    #[error(transparent)]
    Timer(#[from] TimerError),
    /// The response could not be written.
    #[error("the response could not be written: {0}")]
    Response(std::io::Error),
}

/// One progress line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgressLine {
    /// `started` or `finished`.
    pub event: String,
    /// The stage.
    #[serde(with = "timer::stage_name")]
    pub stage: Stage,
    /// A finished stage's count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<u64>,
    /// When, from the worker's origin.
    pub at_ns: u64,
}

impl ProgressLine {
    fn of(progress: &Progress, at_ns: u64) -> Self {
        match *progress {
            Progress::Started(stage) => Self {
                event: "started".to_owned(),
                stage,
                items: None,
                at_ns,
            },
            Progress::Finished { stage, items } => Self {
                event: "finished".to_owned(),
                stage,
                items: Some(items),
                at_ns,
            },
        }
    }

    fn progress(&self) -> Option<Progress> {
        match (self.event.as_str(), self.items) {
            ("started", None) => Some(Progress::Started(self.stage)),
            ("finished", Some(items)) => Some(Progress::Finished {
                stage: self.stage,
                items,
            }),
            _ => None,
        }
    }
}

/// Replays a progress file: every complete line, in order. A last line the worker
/// was stopped while writing (no newline) is ignored; any other bad line is an error.
///
/// # Errors
///
/// A line that is no progress event, or events out of order.
pub fn replay(bytes: &[u8]) -> Result<StageTimer, String> {
    let mut timer = StageTimer::new();
    let complete = match bytes.iter().rposition(|b| *b == b'\n') {
        Some(end) => &bytes[..=end],
        None => &[][..],
    };
    for (number, line) in complete.split(|b| *b == b'\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        let parsed: ProgressLine = serde_json::from_slice(line)
            .map_err(|_| format!("progress line {} is not a progress event", number + 1))?;
        let event = parsed
            .progress()
            .ok_or_else(|| format!("progress line {} is not a progress event", number + 1))?;
        timer
            .observe(&event, parsed.at_ns)
            .map_err(|e| e.to_string())?;
    }
    Ok(timer)
}

/// Serves one request from `input`, answering on `output`.
///
/// # Errors
///
/// [`WorkerError`]; nothing is written to `output` then.
pub fn serve(input: impl Read, output: &mut dyn Write) -> Result<(), WorkerError> {
    let mut text = Vec::new();
    input
        .take(REQUEST_LIMIT + 1)
        .read_to_end(&mut text)
        .map_err(|e| WorkerError::Request(e.to_string()))?;
    if text.len() as u64 > REQUEST_LIMIT {
        return Err(WorkerError::Request("it is too long".to_owned()));
    }
    let request: WorkerRequest =
        serde_json::from_slice(&text).map_err(|e| WorkerError::Request(e.to_string()))?;
    let response = build(&request)?;
    let mut line =
        serde_json::to_vec(&response).map_err(|e| WorkerError::Request(e.to_string()))?;
    line.push(b'\n');
    output.write_all(&line).map_err(WorkerError::Response)?;
    output.flush().map_err(WorkerError::Response)
}

/// What the progress callback keeps: the timer, the file, and the first failure.
struct Log {
    timer: StageTimer,
    file: File,
    failure: Option<WorkerError>,
}

/// Builds the request's segment.
///
/// # Errors
///
/// [`WorkerError`].
pub fn build(request: &WorkerRequest) -> Result<WorkerResponse, WorkerError> {
    let repo_url = canonical_clone_url(&request.repo_url).map_err(|_| WorkerError::Url)?;
    let repo_id = repo_id_from_url(&repo_url).map_err(|_| WorkerError::Url)?;
    let repo_name = repo_url
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .ok_or(WorkerError::Url)?
        .to_owned();
    let file = new_file(&request.progress)?;
    let cache = FsCache::at(&request.cache_dir);
    let clock = MonotonicClock::start();
    let log = RefCell::new(Log {
        timer: StageTimer::new(),
        file,
        failure: None,
    });
    let progress = |event: &Progress| {
        let at = clock.now_ns();
        let mut log = log.borrow_mut();
        if log.failure.is_some() {
            return;
        }
        if let Err(e) = log.timer.observe(event, at) {
            log.failure = Some(e.into());
            return;
        }
        let mut line = serde_json::to_vec(&ProgressLine::of(event, at)).unwrap_or_default();
        line.push(b'\n');
        if let Err(e) = log.file.write_all(&line) {
            log.failure = Some(WorkerError::Progress(e.to_string()));
        }
    };
    let index = IndexRequest {
        root: &request.checkout,
        repo_id,
        repo_id_source: RepoIdSource::Url,
        repo_url,
        repo_name,
        commit_sha: request.commit.clone(),
        limits: ExtractLimits {
            requested_workers: request.workers,
            memory_budget_bytes: request.memory_budget_bytes,
        },
        cache: Some(&cache as &dyn ExtractCache),
        backend: None,
        destination: request.destination.clone(),
        build_dir: Some(request.build_dir.clone()),
        progress: Some(&progress),
    };
    let started = Instant::now();
    let built = build_segment(index);
    let total_wall_ns = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
    let report = built.map_err(|e| WorkerError::Build(e.to_string()))?;
    let Log { timer, failure, .. } = log.into_inner();
    if let Some(failure) = failure {
        return Err(failure);
    }
    Ok(respond(&report, timer.complete()?, total_wall_ns))
}

fn respond(report: &SegmentReport, stages: StageTimes, total_wall_ns: u64) -> WorkerResponse {
    let stats = &report.extract;
    WorkerResponse {
        status: match report.status {
            SegmentStatus::Ready => BuildStatus::Ready,
            SegmentStatus::Degraded => BuildStatus::Degraded,
        },
        content_sha256: report.segment.content_sha256.clone(),
        size_bytes: report.segment.size_bytes,
        files: stages.items(Stage::Extract),
        stages,
        total_wall_ns,
        effective_workers: stats.effective_workers as u64,
        cache_hits: stats.cache_hits,
        cache_misses: stats.cache_misses,
        cache_writes: stats.cache_writes,
        degraded: report.degraded.iter().map(reason_count).collect(),
    }
}

fn reason_count(reason: &DegradedReason) -> ReasonCount {
    let (kind, files) = match reason {
        DegradedReason::FailedOverThreshold {
            failed,
            skipped_size,
            ..
        } => ("failed_over_threshold", failed + skipped_size),
        DegradedReason::EngineCrash { paths } => ("engine_crash", paths.len() as u64),
        DegradedReason::SkippedMemory { paths } => ("skipped_memory", paths.len() as u64),
        DegradedReason::Truncated { paths } => ("truncated", paths.len() as u64),
        DegradedReason::ExtractionLost { paths } => ("extraction_lost", paths.len() as u64),
    };
    ReasonCount {
        kind: kind.to_owned(),
        files,
    }
}

/// A new file at `path`: nothing may be there, not even a symlink.
fn new_file(path: &Path) -> Result<File, WorkerError> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| WorkerError::Progress(e.to_string()))
}

/// Reads a progress file the worker wrote, if there is one.
///
/// # Errors
///
/// The file system's, or a progress file that is not one.
pub fn read_progress(path: &Path) -> Result<Option<StageTimer>, String> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_file() => {}
        Ok(_) => return Err("the progress file is not a regular file".to_owned()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    }
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    replay(&bytes).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::timer::STAGES;

    fn lines(events: &[(Progress, u64)]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for (event, at) in events {
            bytes.extend(serde_json::to_vec(&ProgressLine::of(event, *at)).expect("json"));
            bytes.push(b'\n');
        }
        bytes
    }

    #[test]
    fn a_progress_file_replays_to_the_stage_reached() {
        let mut bytes = lines(&[
            (Progress::Started(Stage::Discover), 0),
            (
                Progress::Finished {
                    stage: Stage::Discover,
                    items: 7,
                },
                40,
            ),
            (Progress::Started(Stage::Extract), 41),
        ]);
        // The worker was stopped while writing its next line.
        bytes.extend(b"{\"event\":\"fin");
        let timer = replay(&bytes).expect("replays");
        assert_eq!(timer.reached(), Some(Stage::Extract));
        assert_eq!(timer.finished()[0].items, 7);
        assert_eq!(timer.finished()[0].duration_ns, 40);
        assert!(replay(b"not json\n").is_err());
        assert!(
            replay(b"{\"event\":\"started\",\"stage\":\"discover\",\"items\":3,\"at_ns\":0}\n")
                .is_err()
        );
        assert_eq!(replay(b"").expect("empty").reached(), None);
    }

    #[test]
    fn a_progress_file_out_of_order_is_an_error() {
        let bytes = lines(&[(Progress::Started(Stage::Resolve), 0)]);
        assert!(replay(&bytes).is_err());
    }

    #[test]
    fn requests_and_responses_are_typed() {
        let request = WorkerRequest {
            checkout: "/c".into(),
            repo_url: "https://git.example/acme/shop".into(),
            commit: "a".repeat(40),
            cache_dir: "/cache".into(),
            destination: "/out/segment.db".into(),
            build_dir: "/out".into(),
            progress: "/out/progress.jsonl".into(),
            workers: 2,
            memory_budget_bytes: 1,
        };
        let json = serde_json::to_string(&request).expect("json");
        assert_eq!(
            serde_json::from_str::<WorkerRequest>(&json).expect("parses"),
            request
        );
        let mut extra: serde_json::Value = serde_json::from_str(&json).expect("value");
        extra["shell"] = serde_json::Value::Bool(true);
        assert!(serde_json::from_value::<WorkerRequest>(extra).is_err());

        let mut timer = StageTimer::new();
        for (i, stage) in STAGES.into_iter().enumerate() {
            let at = i as u64 * 10;
            timer.observe(&Progress::Started(stage), at).expect("start");
            timer
                .observe(&Progress::Finished { stage, items: 1 }, at + 5)
                .expect("finish");
        }
        let response = WorkerResponse {
            status: BuildStatus::Ready,
            content_sha256: "0".repeat(64),
            size_bytes: 4096,
            files: 1,
            stages: timer.complete().expect("complete"),
            total_wall_ns: 60,
            effective_workers: 2,
            cache_hits: 0,
            cache_misses: 1,
            cache_writes: 1,
            degraded: Vec::new(),
        };
        let json = serde_json::to_string(&response).expect("json");
        assert!(json.contains("\"status\":\"ready\""), "{json}");
        assert_eq!(
            serde_json::from_str::<WorkerResponse>(&json).expect("parses"),
            response
        );
    }

    #[test]
    fn a_request_that_is_not_one_builds_nothing() {
        let mut out = Vec::new();
        assert!(matches!(
            serve(&b"{}"[..], &mut out),
            Err(WorkerError::Request(_))
        ));
        let long = vec![b' '; usize::try_from(REQUEST_LIMIT).expect("fits") + 2];
        assert!(matches!(
            serve(&long[..], &mut out),
            Err(WorkerError::Request(_))
        ));
        assert!(out.is_empty());
    }
}
