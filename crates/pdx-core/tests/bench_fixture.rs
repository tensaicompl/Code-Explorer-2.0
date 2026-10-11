//! The benchmarks' fixture (P2-14) holds what they rely on, checked on every
//! `make check` rather than only under `cargo bench`: resolution settles its call
//! sites in the bands the `resolve` benchmark expects, and the segment the
//! `segment_write` benchmark writes reads back whole.

#[path = "../benches/common/mod.rs"]
mod common;

use pdx_core::resolve::stages::resolve;
use pdx_core::segment::{SegmentReader, SegmentWriter};

#[test]
fn the_resolve_benchmark_fixture_resolves_as_expected() {
    let dir = tempfile::tempdir().expect("a directory");
    common::write_project(dir.path());
    let registry = common::registry(dir.path());
    let report = resolve(dir.path(), &registry).expect("resolution");
    common::guard_resolution(&report);
    // Resolution is read-only over the registry: a second run is the same.
    assert_eq!(
        resolve(dir.path(), &registry)
            .expect("resolution")
            .resolutions,
        report.resolutions
    );
}

#[test]
fn the_segment_write_benchmark_fixture_writes_and_reads_back() {
    let dir = tempfile::tempdir().expect("a directory");
    let checkout = dir.path().join("checkout");
    common::write_project(&checkout);
    let registry = common::registry(&checkout);
    let report = resolve(&checkout, &registry).expect("resolution");
    let data = common::segment(&checkout, &registry, &report);
    assert!(
        data.nodes.len() > 500 && data.edges.len() > 500,
        "a non-trivial segment"
    );
    assert!(!data.sites.is_empty() && !data.candidates.is_empty() && !data.coverage.is_empty());
    let first = dir.path().join("first.db");
    let second = dir.path().join("second.db");
    let a = SegmentWriter::new().write(&data, &first).expect("written");
    let b = SegmentWriter::new().write(&data, &second).expect("written");
    assert_eq!(a.content_sha256, b.content_sha256);
    let reader = SegmentReader::open(&first).expect("reads back");
    assert_eq!(reader.nodes_all().expect("nodes").len(), data.nodes.len());
    assert_eq!(reader.edges_all().expect("edges").len(), data.edges.len());
}
