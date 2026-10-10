//! What a build reports beside its segment ([`SegmentReport`]), and the degraded
//! status of 4.5, decided here and only here.
//!
//! The report is the build's, not the segment's: it may say how the build went (its
//! extraction statistics, typed resolution's health), and nothing of it is written
//! into the segment, whose bytes depend on the repository alone.

use pdx_engine::RunHealth;

use crate::index::derive::containment::file_status;
use crate::index::discover::DiscoveredFile;
use crate::index::extract::{ExtractStats, FileOutcome};
use crate::model::{Coverage, FileStatus};
use crate::resolve::registry::SymbolRegistry;
use crate::segment::{SegmentMeta, WrittenSegment};

/// A finished segment's status (4.4's `segments.status`, less `failed`: a build that
/// fails returns an error and publishes nothing).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SegmentStatus {
    /// Complete, with no reason to doubt it.
    Ready,
    /// Complete, with at least one [`DegradedReason`].
    Degraded,
}

impl SegmentStatus {
    /// `ready` or `degraded`, as 4.4 spells them.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Degraded => "degraded",
        }
    }
}

/// Why a segment is degraded (4.5, "Degraded status"), in this order when several
/// hold. Paths are repository-relative, sorted.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DegradedReason {
    /// `failed + skipped(size) > 5%` of the repository's files, strictly.
    FailedOverThreshold {
        /// Files that failed: the engine could not parse them, refused them or crashed.
        failed: u64,
        /// Files skipped for their size.
        skipped_size: u64,
        /// Every file of the repository.
        files: u64,
    },
    /// The engine crashed on these files (each also counted as failed).
    EngineCrash {
        /// The files.
        paths: Vec<String>,
    },
    /// These files were larger than the whole memory budget, and never read.
    SkippedMemory {
        /// The files.
        paths: Vec<String>,
    },
    /// These files' extractions were truncated by the node budget.
    Truncated {
        /// The files.
        paths: Vec<String>,
    },
    /// These files' extractions lost work.
    ExtractionLost {
        /// The files.
        paths: Vec<String>,
    },
}

/// The repository's files, by what became of them: every file once in `files`, and
/// in at most one of the other counts but `skipped_size`, which counts the files of
/// `skipped` skipped for their size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FileCounts {
    /// Every file discovered.
    pub files: u64,
    /// Parsed by the engine without an error region.
    pub parsed: u64,
    /// Parsed, with an error region the engine reported.
    pub partial: u64,
    /// Not parsed, refused by the engine or crashed on.
    pub failed: u64,
    /// Not extracted: too large, beyond the memory budget, of no language, or of an
    /// unportable path.
    pub skipped: u64,
    /// Of `skipped`, too large.
    pub skipped_size: u64,
    /// Not text.
    pub binary: u64,
    /// Withheld under the secrets policy.
    pub redacted: u64,
}

/// How many rows each table of the segment holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RowCounts {
    /// `files`.
    pub files: u64,
    /// `nodes`.
    pub nodes: u64,
    /// `sites`.
    pub sites: u64,
    /// `edges`.
    pub edges: u64,
    /// `candidates`.
    pub candidates: u64,
    /// `contracts`.
    pub contracts: u64,
    /// `metrics`.
    pub metrics: u64,
    /// `coverage`.
    pub coverage: u64,
}

/// What Stage 3 found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolutionSummary {
    /// Call sites resolved: the coverage rows' `call_sites`, summed.
    pub call_sites: u64,
    /// Sites that are not call sites (never counted in coverage).
    pub unconfirmed: u64,
    /// How typed resolution's run went. A degraded run is reported, and does not make
    /// the segment degraded: 4.5 names no such reason, and the absence of an answer is
    /// never evidence.
    pub engine_health: RunHealth,
}

/// What Stage 4 counted and did not store.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DeriveSummary {
    /// Resolved call sites with no position in the file or no node-type path: call
    /// sites in coverage, with no site row.
    pub unmaterialized: u64,
    /// Route evidence that gave no route.
    pub routes: u64,
    /// Contract evidence that gave no contract.
    pub contracts: u64,
    /// Metrics a symbol has no row for.
    pub metrics: u64,
}

/// A finished build: the segment written, its status and what it covered.
#[derive(Clone, Debug, PartialEq)]
pub struct SegmentReport {
    /// The segment's meta, as written.
    pub meta: SegmentMeta,
    /// The segment: where it is, its content hash and size.
    pub segment: WrittenSegment,
    /// Ready or degraded.
    pub status: SegmentStatus,
    /// Every reason it is degraded; empty when it is ready.
    pub degraded: Vec<DegradedReason>,
    /// The coverage rows, as written.
    pub coverage: Vec<Coverage>,
    /// The repository's files, by what became of them.
    pub files: FileCounts,
    /// Files held back for a path no identity can be made from (issue 53), sorted:
    /// counted as skipped, in no table but coverage.
    pub unportable: Vec<String>,
    /// The segment's rows.
    pub rows: RowCounts,
    /// What Stage 3 found.
    pub resolution: ResolutionSummary,
    /// What Stage 4 did not store.
    pub derive: DeriveSummary,
    /// How Stage 2 went.
    pub extract: ExtractStats,
}

/// The repository's files by what became of them, and every reason the segment is
/// degraded (4.5): the threshold over all files, then Stage 2's explicit reasons.
pub(crate) fn assess(
    registry: &SymbolRegistry,
    unportable: &[DiscoveredFile],
) -> (FileCounts, Vec<DegradedReason>) {
    let mut counts = FileCounts::default();
    let mut crashed = Vec::new();
    let mut memory = Vec::new();
    let mut truncated = Vec::new();
    let mut lost = Vec::new();
    for path in registry.files() {
        let Some(outcome) = registry.outcome(path) else {
            continue;
        };
        counts.files += 1;
        let (status, reason) = file_status(outcome);
        match status {
            FileStatus::Parsed => counts.parsed += 1,
            FileStatus::Partial => counts.partial += 1,
            FileStatus::Failed => counts.failed += 1,
            FileStatus::Skipped => counts.skipped += 1,
            FileStatus::Binary => counts.binary += 1,
            FileStatus::Redacted => counts.redacted += 1,
        }
        if status == FileStatus::Skipped && reason == Some("size") {
            counts.skipped_size += 1;
        }
        match outcome {
            FileOutcome::EngineCrashed { .. } => crashed.push(path.to_owned()),
            FileOutcome::SkippedMemory => memory.push(path.to_owned()),
            FileOutcome::Extracted { extract, .. } => {
                if extract.truncated {
                    truncated.push(path.to_owned());
                }
                if extract.extraction_lost > 0 {
                    lost.push(path.to_owned());
                }
            }
            _ => {}
        }
    }
    let held_back = unportable.len() as u64;
    counts.files += held_back;
    counts.skipped += held_back;

    let mut reasons = Vec::new();
    if over_threshold(counts.failed + counts.skipped_size, counts.files) {
        reasons.push(DegradedReason::FailedOverThreshold {
            failed: counts.failed,
            skipped_size: counts.skipped_size,
            files: counts.files,
        });
    }
    if !crashed.is_empty() {
        reasons.push(DegradedReason::EngineCrash { paths: crashed });
    }
    if !memory.is_empty() {
        reasons.push(DegradedReason::SkippedMemory { paths: memory });
    }
    if !truncated.is_empty() {
        reasons.push(DegradedReason::Truncated { paths: truncated });
    }
    if !lost.is_empty() {
        reasons.push(DegradedReason::ExtractionLost { paths: lost });
    }
    (counts, reasons)
}

/// Whether `count` is more than 5% of `files`, strictly, in integers: `count / files >
/// 1 / 20` exactly when `20 × count > files`. Exactly 5% is not more.
fn over_threshold(count: u64, files: u64) -> bool {
    count.saturating_mul(20) > files
}

#[cfg(test)]
mod tests {
    use super::over_threshold;

    #[test]
    fn the_threshold_is_strictly_more_than_five_percent() {
        assert!(!over_threshold(0, 0));
        assert!(!over_threshold(1, 20));
        assert!(over_threshold(1, 19));
        assert!(!over_threshold(5, 100));
        assert!(over_threshold(6, 100));
        assert!(!over_threshold(50, 1000));
        assert!(over_threshold(51, 1000));
        assert!(!over_threshold(0, 1));
        assert!(over_threshold(1, 1));
        assert!(over_threshold(u64::MAX, u64::MAX - 1));
    }
}
