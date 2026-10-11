//! Stage 3's resolution (P2-14): `resolve(root, &registry)` over the generated fixture,
//! the registry built once, outside the timed loop. `cargo bench -p pdx-core --bench
//! resolve`.

mod common;

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use pdx_core::resolve::stages::resolve;

fn resolve_structural_fixture(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("a directory");
    common::write_project(dir.path());
    let registry = common::registry(dir.path());
    // What is timed must be what the fixture is known to resolve to.
    common::guard_resolution(&resolve(dir.path(), &registry).expect("resolution"));
    c.bench_function("resolve_structural_fixture", |b| {
        b.iter(|| resolve(black_box(dir.path()), black_box(&registry)).expect("resolution"));
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(30);
    targets = resolve_structural_fixture
}
criterion_main!(benches);
