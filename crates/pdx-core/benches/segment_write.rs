//! Stage 5's write (P2-14): `SegmentWriter::write` of the generated fixture's segment,
//! its rows built once, outside the timed loop, and a fresh destination for every
//! write (the previous one removed untimed). `cargo bench -p pdx-core --bench
//! segment_write`.

mod common;

use std::hint::black_box;
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use pdx_core::resolve::stages::resolve;
use pdx_core::segment::{SegmentReader, SegmentWriter};

fn segment_write_structural_fixture(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("a directory");
    let checkout = dir.path().join("checkout");
    common::write_project(&checkout);
    let registry = common::registry(&checkout);
    let report = resolve(&checkout, &registry).expect("resolution");
    common::guard_resolution(&report);
    let data = common::segment(&checkout, &registry, &report);
    let out = dir.path().join("out");
    std::fs::create_dir(&out).expect("the output directory");
    let writer = SegmentWriter::new().build_in(&out);

    // What is timed must write a segment the reader accepts, with every row.
    let destination = out.join("segment.db");
    writer.write(&data, &destination).expect("a segment");
    let reader = SegmentReader::open(&destination).expect("the segment reads back");
    assert_eq!(reader.nodes_all().expect("nodes").len(), data.nodes.len());
    assert_eq!(reader.edges_all().expect("edges").len(), data.edges.len());
    drop(reader);

    c.bench_function("segment_write_structural_fixture", |b| {
        b.iter_custom(|iterations| {
            let mut total = Duration::ZERO;
            for _ in 0..iterations {
                std::fs::remove_file(&destination).expect("the previous segment removed");
                let started = Instant::now();
                let written = writer
                    .write(black_box(&data), &destination)
                    .expect("a segment");
                total += started.elapsed();
                black_box(written);
            }
            total
        });
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(20);
    targets = segment_write_structural_fixture
}
criterion_main!(benches);
