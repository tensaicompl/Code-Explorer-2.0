//! Stage 2 with isolated workers, started from this binary as a build starts them: a
//! crash costs the file that caused it, and a worker that hangs fails the stage, with
//! nothing of its batch used or cached (docs/plan/ISSUES.md, issue 27).

use std::path::Path;
use std::time::{Duration, Instant};

use pdx_core::config::PdxConfig;
use pdx_core::index::discover::discover;
use pdx_core::index::extract::{
    ExtractBackend, ExtractError, ExtractLimits, ExtractReport, ExtractStage, FileOutcome, FsCache,
};
use pdx_engine::isolate::{
    Extractor, IsolatedExtractor, IsolationError, WORKER_SUBCOMMAND, WorkerCommand,
};

const SOURCE: &str = "def f(x):\n    return g(x)\n";

/// Three Python files, `a.py`, `b.py` and `c.py`, and a cache, extracted by one
/// isolated worker with `switch` set for it alone.
fn run(
    switch: &str,
    timeout: Duration,
) -> (
    Result<ExtractReport, ExtractError>,
    FsCache,
    tempfile::TempDir,
) {
    let checkout = tempfile::tempdir().unwrap();
    for name in ["a.py", "b.py", "c.py"] {
        std::fs::write(checkout.path().join(name), SOURCE).unwrap();
    }
    let cache_dir = tempfile::tempdir().unwrap();
    let cache = FsCache::at(cache_dir.path().join("extract"));
    let config = PdxConfig::default();
    let files = discover(checkout.path(), &config).unwrap();
    let factory = move || -> Result<Box<dyn ExtractBackend>, ExtractError> {
        let worker = WorkerCommand::new(env!("CARGO_BIN_EXE_pdx"))
            .arg(WORKER_SUBCOMMAND)
            .env(switch, "b.py");
        Ok(Box::new(Extractor::Isolated(
            IsolatedExtractor::with_timeout(worker, timeout),
        )))
    };
    let limits = ExtractLimits {
        requested_workers: 1,
        memory_budget_bytes: 1 << 20,
    };
    let report = ExtractStage::new(checkout.path(), &config.secrets, limits)
        .with_backend(&factory)
        .with_cache(&cache)
        .run(&files);
    (report, cache, cache_dir)
}

fn entries(cache: &FsCache) -> usize {
    fn count(dir: &Path) -> usize {
        std::fs::read_dir(dir).map_or(0, |entries| {
            entries
                .map(|e| e.unwrap().path())
                .map(|p| if p.is_dir() { count(&p) } else { 1 })
                .sum()
        })
    }
    count(cache.root())
}

#[test]
fn a_crash_in_an_isolated_worker_costs_one_file() {
    let (report, cache, _dir) = run("PDX_ENGINE_TEST_CRASH_ON", Duration::from_secs(60));
    let report = report.expect("a crash does not fail the stage");
    let outcomes: Vec<(&str, bool)> = report
        .files
        .iter()
        .map(|f| {
            (
                f.path.as_str(),
                matches!(f.outcome, FileOutcome::EngineCrashed { .. }),
            )
        })
        .collect();
    assert_eq!(outcomes, [("a.py", false), ("b.py", true), ("c.py", false)]);
    assert!(report.degraded());
    assert_eq!(entries(&cache), 2, "the crashed file was cached");
}

#[test]
fn an_isolated_worker_timeout_fails_the_stage() {
    let timeout = Duration::from_secs(2);
    let started = Instant::now();
    let (report, cache, _dir) = run("PDX_ENGINE_TEST_HANG_ON", timeout);
    let took = started.elapsed();
    match report {
        Err(ExtractError::Isolation(IsolationError::Timeout { after, .. })) => {
            assert_eq!(after, timeout);
        }
        other => panic!("not a timeout: {other:?}"),
    }
    assert!(took >= timeout && took < timeout * 15, "took {took:?}");
    assert_eq!(
        entries(&cache),
        0,
        "part of the batch that timed out was cached"
    );
}
