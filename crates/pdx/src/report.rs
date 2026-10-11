//! What the commands print: the index report (JSON with `--json`, text otherwise), the
//! coverage report, the versions and the progress lines.
//!
//! The JSON reports are their own types, written deliberately, never a dump of the
//! pipeline's structures: every field is a fact about the repository and its segment,
//! the same on every machine that builds the same commit. How the build ran (workers,
//! memory budget, cache, time, the checkout's location) is in none of them.

use std::fmt::Write as _;

use pdx_core::bands::Band;
use pdx_core::consts::{ENGINE_VERSION, LANGUAGE_MATRIX_VERSION, SEGMENT_SCHEMA_VERSION};
use pdx_core::index::{DegradedReason, Progress, SegmentReport, SegmentStatus, Stage};
use pdx_core::model::Coverage;
use pdx_core::segment::SegmentMeta;
use pdx_engine::{RunHealth, RunStatus};
use serde::Serialize;

use crate::repository::Identity;

/// `pdx version`: the program and the three versions a segment is built under.
pub fn version_text() -> String {
    format!(
        "pdx {}\nengine {ENGINE_VERSION}\nschema {SEGMENT_SCHEMA_VERSION}\nmatrix {LANGUAGE_MATRIX_VERSION}\n",
        env!("CARGO_PKG_VERSION")
    )
}

/// The `pdx index --json` report.
#[derive(Clone, Debug, Serialize)]
pub struct IndexReport {
    /// `ready` or `degraded`.
    pub status: &'static str,
    /// `structural`.
    pub profile: &'static str,
    /// What was indexed.
    pub repository: RepositoryReport,
    /// What was written.
    pub segment: SegmentOutput,
    /// The repository's files by what became of them.
    pub files: FilesReport,
    /// The segment's rows per table.
    pub rows: RowsReport,
    /// What resolution found.
    pub resolution: ResolutionReport,
    /// What derivation did not store.
    pub derive: DeriveReport,
    /// The coverage rows, as written, by language.
    pub coverage: Vec<Coverage>,
    /// Files held back for a path no identity can be made from, sorted.
    pub unportable: Vec<String>,
    /// Why the segment is degraded; empty when it is ready.
    pub degraded_reasons: Vec<DegradedReport>,
}

/// The repository indexed.
#[derive(Clone, Debug, Serialize)]
pub struct RepositoryReport {
    /// Its identity.
    pub repo_id: String,
    /// Where the identity came from: `url` (derived from the clone URL) or
    /// `registered` (a server's), as the segment records it.
    pub repo_id_source: &'static str,
    /// Its canonical clone URL.
    pub repo_url: String,
    /// Its name.
    pub name: String,
    /// The commit indexed.
    pub commit_sha: String,
}

/// The segment written.
#[derive(Clone, Debug, Serialize)]
pub struct SegmentOutput {
    /// Where, as the user named it (`.pdx/segment.db` by default).
    pub output: String,
    /// `lowerhex(sha256)` of its bytes.
    pub content_sha256: String,
    /// Its size.
    pub size_bytes: u64,
}

/// The repository's files.
#[derive(Clone, Debug, Serialize)]
pub struct FilesReport {
    files: u64,
    parsed: u64,
    partial: u64,
    failed: u64,
    skipped: u64,
    skipped_size: u64,
    binary: u64,
    redacted: u64,
}

/// The rows of each table.
#[derive(Clone, Debug, Serialize)]
pub struct RowsReport {
    files: u64,
    nodes: u64,
    sites: u64,
    edges: u64,
    candidates: u64,
    contracts: u64,
    metrics: u64,
    coverage: u64,
}

/// What resolution found.
#[derive(Clone, Debug, Serialize)]
pub struct ResolutionReport {
    call_sites: u64,
    unconfirmed: u64,
    engine_health: EngineHealthReport,
}

/// How typed resolution's run went.
#[derive(Clone, Debug, Serialize)]
pub struct EngineHealthReport {
    status: &'static str,
    files: u32,
    files_resolved: u32,
    files_untyped: u32,
    files_empty: u32,
    files_over_budget: u32,
    files_source_unavailable: u32,
    files_not_reached: u32,
    pass_failures: u32,
}

impl From<&RunHealth> for EngineHealthReport {
    fn from(h: &RunHealth) -> Self {
        Self {
            status: match h.status {
                RunStatus::Clean => "clean",
                RunStatus::Degraded => "degraded",
            },
            files: h.files,
            files_resolved: h.files_resolved,
            files_untyped: h.files_untyped,
            files_empty: h.files_empty,
            files_over_budget: h.files_over_budget,
            files_source_unavailable: h.files_source_unavailable,
            files_not_reached: h.files_not_reached,
            pass_failures: h.pass_failures,
        }
    }
}

/// What derivation counted and did not store.
#[derive(Clone, Debug, Serialize)]
pub struct DeriveReport {
    unmaterialized: u64,
    routes: u64,
    contracts: u64,
    metrics: u64,
}

/// One reason a segment is degraded, as a tagged object.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DegradedReport {
    /// `failed + skipped(size) > 5%` of all files.
    FailedOverThreshold {
        /// Failed files.
        failed: u64,
        /// Files skipped for their size.
        skipped_size: u64,
        /// Every file.
        files: u64,
    },
    /// The engine crashed on these files.
    EngineCrash {
        /// The files.
        paths: Vec<String>,
    },
    /// These files were larger than the memory budget.
    SkippedMemory {
        /// The files.
        paths: Vec<String>,
    },
    /// These extractions were truncated.
    Truncated {
        /// The files.
        paths: Vec<String>,
    },
    /// These extractions lost work.
    ExtractionLost {
        /// The files.
        paths: Vec<String>,
    },
}

impl From<&DegradedReason> for DegradedReport {
    fn from(reason: &DegradedReason) -> Self {
        match reason {
            DegradedReason::FailedOverThreshold {
                failed,
                skipped_size,
                files,
            } => Self::FailedOverThreshold {
                failed: *failed,
                skipped_size: *skipped_size,
                files: *files,
            },
            DegradedReason::EngineCrash { paths } => Self::EngineCrash {
                paths: paths.clone(),
            },
            DegradedReason::SkippedMemory { paths } => Self::SkippedMemory {
                paths: paths.clone(),
            },
            DegradedReason::Truncated { paths } => Self::Truncated {
                paths: paths.clone(),
            },
            DegradedReason::ExtractionLost { paths } => Self::ExtractionLost {
                paths: paths.clone(),
            },
        }
    }
}

impl IndexReport {
    /// The report of a build of the repository `identity` names, written to `output`.
    pub fn new(report: &SegmentReport, identity: &Identity, output: &str) -> Self {
        let f = &report.files;
        let r = &report.rows;
        let d = &report.derive;
        Self {
            status: report.status.as_str(),
            profile: report.meta.profile.as_str(),
            repository: RepositoryReport {
                repo_id: report.meta.repo_id.as_str().to_owned(),
                repo_id_source: report.meta.repo_id_source.as_str(),
                repo_url: report.meta.repo_url.clone(),
                name: identity.name.clone(),
                commit_sha: report.meta.commit_sha.clone(),
            },
            segment: SegmentOutput {
                output: output.to_owned(),
                content_sha256: report.segment.content_sha256.clone(),
                size_bytes: report.segment.size_bytes,
            },
            files: FilesReport {
                files: f.files,
                parsed: f.parsed,
                partial: f.partial,
                failed: f.failed,
                skipped: f.skipped,
                skipped_size: f.skipped_size,
                binary: f.binary,
                redacted: f.redacted,
            },
            rows: RowsReport {
                files: r.files,
                nodes: r.nodes,
                sites: r.sites,
                edges: r.edges,
                candidates: r.candidates,
                contracts: r.contracts,
                metrics: r.metrics,
                coverage: r.coverage,
            },
            resolution: ResolutionReport {
                call_sites: report.resolution.call_sites,
                unconfirmed: report.resolution.unconfirmed,
                engine_health: EngineHealthReport::from(&report.resolution.engine_health),
            },
            derive: DeriveReport {
                unmaterialized: d.unmaterialized,
                routes: d.routes,
                contracts: d.contracts,
                metrics: d.metrics,
            },
            coverage: report.coverage.clone(),
            unportable: report.unportable.clone(),
            degraded_reasons: report.degraded.iter().map(DegradedReport::from).collect(),
        }
    }

    /// Whether the segment is degraded.
    pub fn degraded(&self) -> bool {
        self.status == SegmentStatus::Degraded.as_str()
    }
}

/// A JSON value, pretty-printed, newline-terminated.
pub fn json<T: Serialize>(value: &T) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".to_owned());
    text.push('\n');
    text
}

/// One degraded reason in words.
fn reason_text(reason: &DegradedReport) -> String {
    match reason {
        DegradedReport::FailedOverThreshold {
            failed,
            skipped_size,
            files,
        } => {
            format!("failed + skipped(size) = {failed} + {skipped_size} of {files} files, over 5%")
        }
        DegradedReport::EngineCrash { paths } => {
            format!(
                "the engine crashed on {}: {}",
                paths.len(),
                paths.join(", ")
            )
        }
        DegradedReport::SkippedMemory { paths } => format!(
            "{} file(s) larger than the memory budget: {}",
            paths.len(),
            paths.join(", ")
        ),
        DegradedReport::Truncated { paths } => format!(
            "{} extraction(s) truncated: {}",
            paths.len(),
            paths.join(", ")
        ),
        DegradedReport::ExtractionLost { paths } => format!(
            "{} extraction(s) lost work: {}",
            paths.len(),
            paths.join(", ")
        ),
    }
}

/// The index report in words.
pub fn index_text(report: &IndexReport) -> String {
    let mut out = String::new();
    let repo = &report.repository;
    let _ = writeln!(
        out,
        "{} {} segment of {}",
        report.status, report.profile, repo.repo_url
    );
    let _ = writeln!(out, "  repo_id   {}", repo.repo_id);
    let _ = writeln!(out, "  commit    {}", repo.commit_sha);
    let _ = writeln!(out, "  segment   {}", report.segment.output);
    let _ = writeln!(out, "  sha256    {}", report.segment.content_sha256);
    let _ = writeln!(out, "  size      {} bytes", report.segment.size_bytes);
    let f = &report.files;
    let _ = writeln!(
        out,
        "files      {} ({} parsed, {} partial, {} failed, {} skipped, {} binary, {} redacted)",
        f.files, f.parsed, f.partial, f.failed, f.skipped, f.binary, f.redacted
    );
    let r = &report.rows;
    let _ = writeln!(
        out,
        "graph      {} nodes, {} edges, {} sites, {} candidates, {} contracts, {} metrics",
        r.nodes, r.edges, r.sites, r.candidates, r.contracts, r.metrics
    );
    let _ = writeln!(
        out,
        "resolution {} call sites, {} unconfirmed",
        report.resolution.call_sites, report.resolution.unconfirmed
    );
    for row in &report.coverage {
        let drawn: u64 = Band::ALL
            .iter()
            .filter(|b| b.is_drawn())
            .map(|b| row.by_band.get(*b))
            .sum();
        let _ = writeln!(
            out,
            "  {:<12} {} files, {} symbols, {} call sites ({} drawn)",
            row.language, row.files, row.symbols, row.call_sites, drawn
        );
    }
    if !report.unportable.is_empty() {
        let _ = writeln!(
            out,
            "unportable {} file(s) held back: {}",
            report.unportable.len(),
            report.unportable.join(", ")
        );
    }
    for reason in &report.degraded_reasons {
        let _ = writeln!(out, "degraded: {}", reason_text(reason));
    }
    out
}

/// The `pdx coverage --json` report.
#[derive(Clone, Debug, Serialize)]
pub struct CoverageReport {
    /// The segment's repository.
    pub repo_id: String,
    /// Where the identity came from, as the segment records it.
    pub repo_id_source: &'static str,
    /// Its canonical clone URL.
    pub repo_url: String,
    /// The commit it is of.
    pub commit_sha: String,
    /// Its profile.
    pub profile: &'static str,
    /// The coverage rows, by language.
    pub coverage: Vec<Coverage>,
}

impl CoverageReport {
    /// The report of a segment with this meta and these rows.
    pub fn new(meta: &SegmentMeta, coverage: Vec<Coverage>) -> Self {
        Self {
            repo_id: meta.repo_id.as_str().to_owned(),
            repo_id_source: meta.repo_id_source.as_str(),
            repo_url: meta.repo_url.clone(),
            commit_sha: meta.commit_sha.clone(),
            profile: meta.profile.as_str(),
            coverage,
        }
    }
}

/// The coverage report in words: one block per language, every band named, zeros
/// included, so a gap is visible.
pub fn coverage_text(report: &CoverageReport) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} segment of {} at {}",
        report.profile, report.repo_url, report.commit_sha
    );
    for row in &report.coverage {
        let _ = writeln!(out, "{}", row.language);
        let _ = writeln!(
            out,
            "  files {}: {} parsed, {} partial, {} failed, {} skipped",
            row.files, row.parsed, row.partial, row.failed, row.skipped
        );
        let _ = writeln!(
            out,
            "  symbols {}, call sites {}, observed links {}",
            row.symbols, row.call_sites, row.observed_links
        );
        let bands: Vec<String> = Band::ALL
            .iter()
            .map(|b| format!("{} {}", b.as_str(), row.by_band.get(*b)))
            .collect();
        let _ = writeln!(out, "  bands: {}", bands.join(", "));
    }
    out
}

/// What a stage produces, as a progress line counts it.
fn unit(stage: Stage) -> &'static str {
    match stage {
        Stage::Discover | Stage::Extract => "files",
        Stage::Resolve => "call sites",
        Stage::Derive => "nodes",
        Stage::Write => "bytes",
    }
}

/// A progress line for one event of the pipeline.
pub fn progress_line(progress: &Progress) -> String {
    match progress {
        Progress::Started(stage) => format!("pdx: {}...\n", stage.as_str()),
        Progress::Finished { stage, items } => {
            format!("pdx: {} done, {items} {}\n", stage.as_str(), unit(*stage))
        }
    }
}

/// Writes progress lines to `sink` when `tty` (standard error is a terminal), and
/// nothing otherwise.
pub fn progress_to(tty: bool, sink: &mut dyn std::io::Write, progress: &Progress) {
    if tty {
        let _ = sink.write_all(progress_line(progress).as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAGES: [Stage; 5] = [
        Stage::Discover,
        Stage::Extract,
        Stage::Resolve,
        Stage::Derive,
        Stage::Write,
    ];

    fn events() -> Vec<Progress> {
        STAGES
            .iter()
            .enumerate()
            .flat_map(|(i, stage)| {
                [
                    Progress::Started(*stage),
                    Progress::Finished {
                        stage: *stage,
                        items: 10 + i as u64,
                    },
                ]
            })
            .collect()
    }

    #[test]
    fn progress_is_written_on_a_terminal_only() {
        let mut tty = Vec::new();
        let mut piped = Vec::new();
        for event in events() {
            progress_to(true, &mut tty, &event);
            progress_to(false, &mut piped, &event);
        }
        assert!(piped.is_empty());
        assert_eq!(
            String::from_utf8(tty).expect("UTF-8"),
            "pdx: discover...\npdx: discover done, 10 files\n\
             pdx: extract...\npdx: extract done, 11 files\n\
             pdx: resolve...\npdx: resolve done, 12 call sites\n\
             pdx: derive...\npdx: derive done, 13 nodes\n\
             pdx: write...\npdx: write done, 14 bytes\n"
        );
    }

    #[test]
    fn the_version_names_the_four_versions() {
        assert_eq!(version_text(), "pdx 0.1.0\nengine 5\nschema 3\nmatrix 2\n");
    }

    #[test]
    fn degraded_reasons_are_tagged_objects() {
        let reason = DegradedReport::from(&DegradedReason::EngineCrash {
            paths: vec!["src/a.py".to_owned()],
        });
        assert_eq!(
            serde_json::to_value(&reason).expect("JSON"),
            serde_json::json!({"kind": "engine_crash", "paths": ["src/a.py"]})
        );
        let reason = DegradedReport::from(&DegradedReason::FailedOverThreshold {
            failed: 2,
            skipped_size: 1,
            files: 30,
        });
        assert_eq!(
            serde_json::to_value(&reason).expect("JSON"),
            serde_json::json!({"kind": "failed_over_threshold", "failed": 2, "skipped_size": 1, "files": 30})
        );
        for (reason, kind) in [
            (
                DegradedReason::SkippedMemory { paths: vec![] },
                "skipped_memory",
            ),
            (DegradedReason::Truncated { paths: vec![] }, "truncated"),
            (
                DegradedReason::ExtractionLost { paths: vec![] },
                "extraction_lost",
            ),
        ] {
            let value = serde_json::to_value(DegradedReport::from(&reason)).expect("JSON");
            assert_eq!(value["kind"], kind);
        }
    }
}
