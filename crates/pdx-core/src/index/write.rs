//! Stage 5 of the indexing pipeline: write (specification 4.5).
//!
//! Assembles the segment's rows from what the earlier stages produced, Stage 4's graph
//! and the coverage, and hands them to [`SegmentWriter`], which owns everything a
//! segment's bytes depend on: checking the rows, their canonical order, the schema,
//! the full-text index, `VACUUM INTO`, read-only publication and the content hash.
//!
//! Only the rows a stage of this build produces are written. The `evidence` and
//! `semantic_occurrences` tables are empty in a structural segment: they hold compiler
//! evidence and occurrences, which a precise build adds (P5). Nothing of the build
//! itself (its time, its checkout's location, its workers, budget, cache or progress)
//! reaches a row, so the same logical input is the same bytes.

use std::path::Path;

use crate::index::derive::DerivedGraph;
use crate::model::Coverage;
use crate::segment::{SegmentData, SegmentError, SegmentMeta, SegmentWriter, WrittenSegment};

/// The segment's rows: the meta, every table Stage 4 derived and the coverage.
pub fn segment_data(
    meta: SegmentMeta,
    graph: DerivedGraph,
    coverage: Vec<Coverage>,
) -> SegmentData {
    let mut data = SegmentData::new(meta);
    data.files = graph.files;
    data.nodes = graph.nodes;
    data.sites = graph.sites;
    data.edges = graph.edges;
    data.candidates = graph.candidates;
    data.contracts = graph.contracts;
    data.metrics = graph.metrics;
    data.coverage = coverage;
    data
}

/// Writes the segment at `destination`, building it in a temporary directory inside
/// `build_dir` when one is given (the system's otherwise).
///
/// # Errors
///
/// [`SegmentError`] as [`SegmentWriter::write`] reports it: a destination that exists
/// or cannot be written to, rows that cannot be stored, SQLite and file errors.
/// Nothing is left at the destination on an error.
pub fn write(
    data: &SegmentData,
    destination: &Path,
    build_dir: Option<&Path>,
) -> Result<WrittenSegment, SegmentError> {
    let writer = match build_dir {
        Some(dir) => SegmentWriter::new().build_in(dir),
        None => SegmentWriter::new(),
    };
    writer.write(data, destination)
}
