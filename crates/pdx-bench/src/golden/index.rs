//! Running `pdx index` on a checkout, and what its result means for a golden.
//!
//! The harness runs the real `pdx` binary built from this workspace (the sibling of
//! the harness's own executable), never the pipeline in process: a golden is what the
//! command line produces. The segment goes to an absolute path below
//! `bench/.cache/segments/`, never into the checkout, so a run leaves the third-party
//! tree clean. Only a ready index makes a golden: a degraded one (exit 2) would freeze
//! a defect as the expected answer, so it fails the run, as does any error.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

/// The most of `pdx`'s standard error a message carries.
const STDERR_LIMIT: usize = 1024;

/// The most degraded reasons, and paths per reason, a message lists.
const LIST_LIMIT: usize = 5;

/// What `pdx index` did, as the harness saw it.
#[derive(Clone, Debug)]
pub struct PdxRun {
    /// Its exit code, if it exited with one.
    pub code: Option<i32>,
    /// Its standard output: the JSON report.
    pub stdout: Vec<u8>,
    /// Its standard error.
    pub stderr: Vec<u8>,
}

/// A ready index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexReady {
    /// The segment's content hash, as `pdx` reported it.
    pub content_sha256: String,
    /// Its size.
    pub size_bytes: u64,
}

/// Why an index makes no golden.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    /// `pdx` could not be started.
    #[error("cannot run {path}: {source}")]
    Spawn {
        /// The binary.
        path: PathBuf,
        /// Why.
        #[source]
        source: std::io::Error,
    },
    /// The index is degraded: no golden is taken from it.
    #[error("pdx index produced a degraded segment, which is no golden baseline: {0}")]
    Degraded(String),
    /// `pdx` failed.
    #[error("pdx index failed (exit {code}): {stderr}")]
    Failed {
        /// Its exit code, or `signal`.
        code: String,
        /// The start of its standard error.
        stderr: String,
    },
    /// `pdx` exited 0 without the report a ready index has.
    #[error("pdx index exited 0 but {0}")]
    Unexpected(String),
}

/// The parts of `pdx index --json`'s report the harness reads; the rest is ignored.
#[derive(Deserialize)]
struct Report {
    status: String,
    segment: SegmentReport,
    #[serde(default)]
    degraded_reasons: Vec<DegradedReason>,
}

#[derive(Deserialize)]
struct SegmentReport {
    content_sha256: String,
    size_bytes: u64,
}

#[derive(Deserialize)]
struct DegradedReason {
    kind: String,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    failed: Option<u64>,
    #[serde(default)]
    skipped_size: Option<u64>,
    #[serde(default)]
    files: Option<u64>,
}

impl DegradedReason {
    fn describe(&self) -> String {
        if let (Some(failed), Some(skipped), Some(files)) =
            (self.failed, self.skipped_size, self.files)
        {
            return format!(
                "{} (failed {failed}, skipped for size {skipped}, of {files} files)",
                self.kind
            );
        }
        let shown: Vec<&str> = self
            .paths
            .iter()
            .take(LIST_LIMIT)
            .map(String::as_str)
            .collect();
        let more = self.paths.len().saturating_sub(shown.len());
        let tail = if more > 0 {
            format!(", and {more} more")
        } else {
            String::new()
        };
        format!(
            "{} ({} file(s): {}{tail})",
            self.kind,
            self.paths.len(),
            shown.join(", ")
        )
    }
}

/// The `pdx` executable beside `harness`, the harness's own executable: the binary
/// built from the same workspace, never one found on the `PATH`.
pub fn sibling_pdx(harness: &Path) -> Option<PathBuf> {
    let dir = harness.parent()?;
    Some(dir.join(format!("pdx{}", std::env::consts::EXE_SUFFIX)))
}

/// The arguments the harness gives `pdx`: the structural profile and the JSON report,
/// explicitly, and the segment's absolute path.
pub fn index_arguments(checkout: &Path, segment: &Path) -> Vec<OsString> {
    vec![
        OsString::from("index"),
        checkout.as_os_str().to_owned(),
        OsString::from("--out"),
        segment.as_os_str().to_owned(),
        OsString::from("--profile"),
        OsString::from("structural"),
        OsString::from("--json"),
    ]
}

/// Runs `pdx index` on `checkout`, writing `segment`, with `environment` added (the
/// harness's Git settings) and its output captured, never shown.
///
/// # Errors
///
/// [`IndexError::Spawn`] when `pdx` cannot be started.
pub fn run_pdx(
    pdx: &Path,
    checkout: &Path,
    segment: &Path,
    environment: &[(&str, OsString)],
) -> Result<PdxRun, IndexError> {
    let mut command = Command::new(pdx);
    command
        .args(index_arguments(checkout, segment))
        .stdin(Stdio::null());
    for (name, value) in environment {
        command.env(name, value);
    }
    let output = command.output().map_err(|source| IndexError::Spawn {
        path: pdx.to_path_buf(),
        source,
    })?;
    Ok(PdxRun {
        code: output.status.code(),
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

/// What a `pdx index` run means for a golden: ready (exit 0, a ready report, the
/// segment written) or no golden at all. Exit 2 is degraded and refused with its
/// reasons; exit 1 or anything else is a failure. Nothing is ever taken from a
/// segment a failed run may have left.
///
/// # Errors
///
/// [`IndexError::Degraded`], [`IndexError::Failed`] or [`IndexError::Unexpected`].
pub fn interpret(run: &PdxRun, segment: &Path) -> Result<IndexReady, IndexError> {
    match run.code {
        Some(0) => {
            let report: Report = serde_json::from_slice(&run.stdout).map_err(|e| {
                IndexError::Unexpected(format!("its report is not the JSON expected: {e}"))
            })?;
            if report.status != "ready" {
                return Err(IndexError::Unexpected(format!(
                    "its report's status is {:?}, not \"ready\"",
                    report.status
                )));
            }
            if !segment.is_file() {
                return Err(IndexError::Unexpected(
                    "no segment is where it was asked to write one".to_owned(),
                ));
            }
            Ok(IndexReady {
                content_sha256: report.segment.content_sha256,
                size_bytes: report.segment.size_bytes,
            })
        }
        Some(2) => {
            let reasons = match serde_json::from_slice::<Report>(&run.stdout) {
                Ok(report) if !report.degraded_reasons.is_empty() => report
                    .degraded_reasons
                    .iter()
                    .take(LIST_LIMIT)
                    .map(DegradedReason::describe)
                    .collect::<Vec<_>>()
                    .join("; "),
                _ => "no reason could be read from its report".to_owned(),
            };
            Err(IndexError::Degraded(reasons))
        }
        code => {
            let stderr = String::from_utf8_lossy(&run.stderr);
            Err(IndexError::Failed {
                code: code.map_or_else(|| "signal".to_owned(), |c| c.to_string()),
                stderr: stderr
                    .chars()
                    .take(STDERR_LIMIT)
                    .collect::<String>()
                    .trim()
                    .to_owned(),
            })
        }
    }
}
