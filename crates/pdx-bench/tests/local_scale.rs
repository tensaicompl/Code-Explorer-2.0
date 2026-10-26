//! P2-17's acceptance on the local reference profile (decision 80: eight processors):
//! Kubernetes indexed with the structural profile at no less than 400 files a second
//! and no more than 4 GiB of peak resident memory (G1's scale criterion, issue 82).
//!
//! It reads the result `make local-scale` records, the performance harness run over
//! Kubernetes alone on eight processors, so it is ignored unless that target runs it.

use std::fs;
use std::path::{Path, PathBuf};

use pdx_bench::bench::results::{PDX_VERSION, Performance, ResultFile};

/// The reference profile's processors, so its workers.
const LOCAL_PROFILE_WORKERS: u64 = 8;
/// The slowest the profile may index: extracted files over extraction plus typed
/// resolution, the median of three cold samples.
const MIN_FILES_PER_SECOND: f64 = 400.0;
/// The most resident memory any sample may reach.
const MAX_PEAK_RSS_BYTES: u64 = 4 << 30;

fn result_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../bench/results")
        .join(PDX_VERSION)
        .join("kubernetes.json")
}

#[test]
#[ignore = "run by `make local-scale`, after it measures Kubernetes on the local profile"]
fn kubernetes_local_scale() {
    let path = result_path();
    let bytes = fs::read(&path).expect("make local-scale records Kubernetes' result");
    let result = ResultFile::parse(&bytes, &path).expect("a result file");
    let completed = match result.performance {
        Some(Performance::Completed(completed)) => completed,
        other => panic!("Kubernetes was not measured to the end: {other:?}"),
    };
    for sample in &completed.samples {
        assert_eq!(
            sample.workers, LOCAL_PROFILE_WORKERS,
            "sample {} ran {} workers, not the local profile's",
            sample.sample, sample.workers
        );
    }
    assert!(
        completed.median_files_per_second >= MIN_FILES_PER_SECOND,
        "{:.1} files/s, below {MIN_FILES_PER_SECOND}",
        completed.median_files_per_second
    );
    assert!(
        completed.peak_rss_bytes <= MAX_PEAK_RSS_BYTES,
        "peak RSS {} MiB, above {} MiB",
        completed.peak_rss_bytes >> 20,
        MAX_PEAK_RSS_BYTES >> 20
    );
}
