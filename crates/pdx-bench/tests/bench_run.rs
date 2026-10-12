//! Determinism and performance runs (P2-14) with synthetic builders: every check a
//! run makes, made to fail one at a time, and the result files it writes or keeps. No
//! fetch, no worker process, no network; `bench_worker.rs` runs the real worker.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use pdx_bench::bench::area::Area;
use pdx_bench::bench::host::Host;
use pdx_bench::bench::plan::{BenchEntry, BenchPlan, BenchPlanError, Kind, bench_plan};
use pdx_bench::bench::results::{PDX_VERSION, ResultFile};
use pdx_bench::bench::run::{
    self, BenchError, BuildOutcome, BuildSpec, Builder, Built, Checkouts, Modes, REFUSED_VARIABLES,
    Run, Settings, Stopped,
};
use pdx_bench::bench::timer::{STAGES, StageRecord, StageTimer, StageTimes};
use pdx_bench::bench::worker::{BuildStatus, WorkerResponse};
use pdx_bench::golden::run::EXTRACTION_SWITCHES;
use pdx_bench::lock::BenchRepos;
use pdx_core::index::{Progress, Stage};
use serde_json::Value;

const MS: u64 = 1_000_000;

fn lock(text: &str) -> BenchRepos {
    toml::from_str(text).expect("a lock")
}

fn golden_entry(language: &str, repo: &str) -> String {
    format!(
        "[[golden]]\nlanguage = \"{language}\"\nurl = \"git.example/acme/{repo}\"\n\
         default_branch = \"main\"\ncommit = \"{}\"\nlicence = \"MIT\"\nfetch_date = \"2026-01-01\"\n",
        "a".repeat(40)
    )
}

fn scale_entry(name: &str, indexed_only: bool) -> String {
    format!(
        "[[scale]]\nname = \"{name}\"\nurl = \"git.example/big/{name}\"\ndefault_branch = \"main\"\n\
         commit = \"{}\"\nlicence = \"GPL-2.0-only\"\nfetch_date = \"2026-01-01\"\nindexed_only = {indexed_only}\n",
        "b".repeat(40)
    )
}

fn plan_of(entries: &[String]) -> Result<BenchPlan, BenchPlanError> {
    let mut text = "lock_version = 1\nfetch_date = \"2026-01-01\"\n".to_owned();
    for kind in ["golden", "scale"] {
        if !entries
            .iter()
            .any(|e| e.starts_with(&format!("[[{kind}]]")))
        {
            let _ = writeln!(text, "{kind} = []");
        }
    }
    for entry in entries {
        text.push_str(entry);
    }
    bench_plan(&lock(&text))
}

fn plan() -> BenchPlan {
    plan_of(&[
        golden_entry("python", "flask"),
        golden_entry("go", "gin"),
        scale_entry("kubernetes", true),
    ])
    .expect("a plan")
}

fn settings() -> Settings {
    Settings {
        parallel_workers: 3,
        perf_workers: 2,
        memory_budget_bytes: 1 << 29,
        ceiling: 1 << 32,
    }
}

fn host() -> Host {
    Host {
        os: "linux".into(),
        architecture: "x86_64".into(),
        logical_cpus: 4,
        total_memory_bytes: 1 << 33,
        cpu_brand: None,
        benchmark_workers: 2,
        benchmark_memory_budget_bytes: 1 << 29,
    }
}

/// Checkouts that are directories of a temporary directory, never fetched.
struct LocalCheckouts {
    root: PathBuf,
    asked: Vec<String>,
}

impl Checkouts for LocalCheckouts {
    fn checkout(
        &mut self,
        entry: &BenchEntry,
        _err: &mut dyn Write,
    ) -> Result<PathBuf, BenchError> {
        self.asked.push(entry.key());
        let dir = self.root.join(entry.key().replace('/', "-"));
        fs::create_dir_all(&dir).expect("checkout dir");
        Ok(dir)
    }
}

/// How the synthetic builder answers one build.
#[derive(Clone, Debug)]
struct Answer {
    bytes: Vec<u8>,
    status: BuildStatus,
    report_hash: Option<String>,
    cache_hits: Option<u64>,
    extract_ns: u64,
    resolve_ns: u64,
    rss: Option<u64>,
    stop: Option<u64>,
}

impl Answer {
    fn ok(bytes: &[u8]) -> Self {
        Self {
            bytes: bytes.to_vec(),
            status: BuildStatus::Ready,
            report_hash: None,
            cache_hits: None,
            extract_ns: 100 * MS,
            resolve_ns: 100 * MS,
            rss: Some(1024),
            stop: None,
        }
    }
}

/// A builder that writes the segment it is told to and models a cache: a cache
/// directory used before answers with what was stored in it.
struct Synthetic {
    specs: Vec<BuildSpec>,
    stored: BTreeMap<PathBuf, u64>,
    answer: Box<AnswerFn>,
}

/// What the builder answers build number `n` of a run with.
type AnswerFn = dyn FnMut(&BuildSpec, usize) -> Answer;

impl Synthetic {
    fn new(answer: impl FnMut(&BuildSpec, usize) -> Answer + 'static) -> Self {
        Self {
            specs: Vec::new(),
            stored: BTreeMap::new(),
            answer: Box::new(answer),
        }
    }

    fn identical() -> Self {
        Self::new(|spec, _| Answer::ok(spec.commit.as_bytes()))
    }
}

const FILES: u64 = 100;

fn stage_times(extract_ns: u64, resolve_ns: u64) -> StageTimes {
    let records: Vec<StageRecord> = STAGES
        .into_iter()
        .map(|stage| StageRecord {
            stage,
            items: if stage == Stage::Extract { FILES } else { 7 },
            duration_ns: match stage {
                Stage::Extract => extract_ns,
                Stage::Resolve => resolve_ns,
                _ => MS,
            },
        })
        .collect();
    StageTimes::try_from(records).expect("five stages")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("f");
    fs::write(&path, bytes).expect("write");
    run::file_sha256(&path).expect("hash").0
}

impl Builder for Synthetic {
    fn build(&mut self, spec: &BuildSpec) -> Result<BuildOutcome, BenchError> {
        let call = self.specs.len();
        self.specs.push(spec.clone());
        let answer = (self.answer)(spec, call);
        if let Some(peak) = answer.stop {
            let mut progress = StageTimer::new();
            progress
                .observe(&Progress::Started(Stage::Discover), 0)
                .expect("start");
            progress
                .observe(
                    &Progress::Finished {
                        stage: Stage::Discover,
                        items: 22_000,
                    },
                    5 * MS,
                )
                .expect("finish");
            progress
                .observe(&Progress::Started(Stage::Extract), 6 * MS)
                .expect("start");
            return Ok(BuildOutcome::Stopped(Stopped {
                peak_rss_bytes: peak,
                rss_samples: 1_000,
                wall_ns: 20_000 * MS,
                progress,
            }));
        }
        fs::write(&spec.destination, &answer.bytes).expect("segment");
        let previous = self.stored.get(&spec.cache_dir).copied();
        let (hits, writes) = match previous {
            Some(stored) => (stored, 0),
            None => (0, FILES - 1),
        };
        self.stored.entry(spec.cache_dir.clone()).or_insert(writes);
        Ok(BuildOutcome::Built(Built {
            response: WorkerResponse {
                status: answer.status,
                content_sha256: answer
                    .report_hash
                    .unwrap_or_else(|| sha256_hex(&answer.bytes)),
                size_bytes: answer.bytes.len() as u64,
                files: FILES,
                stages: stage_times(answer.extract_ns, answer.resolve_ns),
                total_wall_ns: answer.extract_ns + answer.resolve_ns + 3 * MS,
                effective_workers: spec.workers as u64,
                cache_hits: answer.cache_hits.unwrap_or(hits),
                cache_misses: FILES - hits,
                cache_writes: writes,
                degraded: Vec::new(),
            },
            peak_rss_bytes: answer.rss,
            rss_samples: 3,
        }))
    }
}

/// A workspace-like directory: the area, the results and the checkouts.
struct Fixture {
    dir: tempfile::TempDir,
    area: Area,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let area = Area::open(&dir.path().join("area")).expect("area");
        Self { dir, area }
    }

    fn results_root(&self) -> PathBuf {
        self.dir.path().join("results")
    }

    fn results_dir(&self) -> PathBuf {
        self.results_root().join(PDX_VERSION)
    }

    fn run(
        &self,
        plan: &BenchPlan,
        modes: Modes,
        builder: &mut dyn Builder,
    ) -> (Result<(), BenchError>, String, String, Vec<String>) {
        let mut checkouts = LocalCheckouts {
            root: self.dir.path().join("checkouts"),
            asked: Vec::new(),
        };
        let run = Run {
            plan,
            modes,
            settings: settings(),
            host: host(),
            area: &self.area,
            results_root: self.results_root(),
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let result = run::run(&run, &mut checkouts, builder, &mut out, &mut err);
        (
            result,
            String::from_utf8(out).expect("utf-8"),
            String::from_utf8(err).expect("utf-8"),
            checkouts.asked,
        )
    }

    fn result(&self, name: &str) -> Value {
        let bytes = fs::read(self.results_dir().join(name)).expect("a result file");
        serde_json::from_slice(&bytes).expect("json")
    }

    fn snapshot(&self) -> BTreeMap<String, Vec<u8>> {
        let mut files = BTreeMap::new();
        if let Ok(entries) = fs::read_dir(self.results_dir()) {
            for entry in entries.flatten() {
                files.insert(
                    entry.file_name().to_string_lossy().into_owned(),
                    fs::read(entry.path()).expect("read"),
                );
            }
        }
        files
    }
}

const DETERMINISM: Modes = Modes {
    determinism: true,
    performance: false,
};
const PERF: Modes = Modes {
    determinism: false,
    performance: true,
};
const BOTH: Modes = Modes {
    determinism: true,
    performance: true,
};

fn failure_of(result: Result<(), BenchError>) -> BenchError {
    match result {
        Err(BenchError::Repository { source, .. }) => *source,
        Err(other) => other,
        Ok(()) => panic!("the run succeeded"),
    }
}

// --- plan ----------------------------------------------------------------------------

#[test]
fn determinism_is_the_goldens_and_perf_adds_the_scale_repositories() {
    let plan = plan();
    let keys = |entries: Vec<&BenchEntry>| entries.iter().map(|e| e.key()).collect::<Vec<_>>();
    assert_eq!(keys(plan.determinism()), ["go/gin", "python/flask"]);
    assert_eq!(
        keys(plan.performance()),
        ["go/gin", "python/flask", "kubernetes"]
    );
    assert_eq!(plan.scale[0].kind, Kind::Scale);
    assert_eq!(plan.scale[0].cache_components(), ["scale", "kubernetes"]);
    assert_eq!(plan.golden[1].cache_components(), ["python", "flask"]);
}

#[test]
fn a_scale_repository_must_be_indexed_only() {
    let refused = plan_of(&[scale_entry("kubernetes", false)]);
    assert!(
        matches!(&refused, Err(BenchPlanError::NotIndexedOnly(name)) if name == "kubernetes"),
        "{refused:?}"
    );
}

#[test]
fn two_repositories_never_share_a_result_file() {
    for entries in [
        vec![golden_entry("python", "flask"), scale_entry("flask", true)],
        vec![golden_entry("python", "Flask"), scale_entry("flask", true)],
        vec![golden_entry("python", "summary"), golden_entry("go", "x")],
        vec![scale_entry("linux", true), scale_entry("LINUX", true)],
    ] {
        let refused = plan_of(&entries);
        assert!(
            matches!(
                refused,
                Err(BenchPlanError::Collision(..) | BenchPlanError::Unsafe { .. })
            ),
            "{refused:?}"
        );
    }
    assert!(matches!(
        plan_of(&[scale_entry("scale", true)]),
        Err(BenchPlanError::Unsafe { .. })
    ));
    assert!(matches!(
        plan_of(&[scale_entry("../up", true)]),
        Err(BenchPlanError::Unsafe { .. })
    ));
}

#[test]
fn the_committed_lock_plans_ten_goldens_and_kubernetes_and_linux() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let repos = pdx_bench::lock::bench_repos(&root.join("bench/repos.lock")).expect("the lock");
    let plan = bench_plan(&repos).expect("a plan");
    assert_eq!(plan.determinism().len(), 10);
    let scale: Vec<String> = plan.scale.iter().map(BenchEntry::key).collect();
    assert_eq!(scale, ["kubernetes", "linux"]);
    assert_eq!(plan.performance().len(), 12);
}

// --- preflight -------------------------------------------------------------------------

#[test]
fn the_engine_switches_isolation_and_limits_are_refused_by_name() {
    assert_eq!(REFUSED_VARIABLES[..6], EXTRACTION_SWITCHES);
    assert_eq!(REFUSED_VARIABLES[..6], pdx_engine::EXTRACTION_SWITCHES);
    assert_eq!(REFUSED_VARIABLES[6], pdx_engine::isolate::ISOLATE_ENV);
    assert!(REFUSED_VARIABLES.contains(&"PDX_WORKERS"));
    assert!(REFUSED_VARIABLES.contains(&"PDX_MEM_BUDGET_MB"));
    for name in REFUSED_VARIABLES {
        let error = run::preflight(|n| n == name).expect_err("refused");
        let message = error.to_string();
        assert!(message.starts_with(name), "{message}");
        assert!(!message.contains("secret-value"));
    }
    run::preflight(|_| false).expect("nothing set");
}

// --- determinism ---------------------------------------------------------------------

#[test]
fn identical_builds_pass_and_are_recorded() {
    let fixture = Fixture::new();
    let mut builder = Synthetic::identical();
    let (result, out, _err, asked) = fixture.run(&plan(), DETERMINISM, &mut builder);
    result.expect("deterministic");
    assert_eq!(
        asked,
        ["go/gin", "python/flask"],
        "no scale repository is built"
    );
    assert!(
        out.contains("determinism: python/flask: identical"),
        "{out}"
    );
    let flask = fixture.result("flask.json");
    let determinism = &flask["determinism"];
    assert_eq!(determinism["cold_builds_byte_identical"], true);
    assert_eq!(determinism["cold_warm_byte_identical"], true);
    assert_eq!(determinism["rebuild_equivalent"], true);
    assert_eq!(determinism["workers_single"], 1);
    assert_eq!(determinism["workers_parallel"], 3);
    assert_eq!(determinism["warm_cache_hits"], FILES - 1);
    assert_eq!(flask["provenance"]["engine_version"], 7);
    assert_eq!(flask["provenance"]["kind"], "golden");
    assert_eq!(flask["provenance"]["language"], "python");
    assert!(flask.get("performance").is_none());
    let summary = fixture.result("summary.json");
    assert_eq!(summary["determinism"]["repositories_tested"], 2);
    assert_eq!(summary["determinism"]["repositories_identical"], 2);
    assert_eq!(summary["determinism"]["determinism_rate"], 1.0);
    assert_eq!(summary["determinism"]["cold_warm_equivalent"], 2);
    assert_eq!(summary["determinism"]["rebuild_equivalence_rate"], 1.0);
    assert!(!fixture.results_dir().join("kubernetes.json").exists());
}

#[test]
fn the_three_builds_are_cold_single_cold_parallel_and_warm_parallel() {
    let fixture = Fixture::new();
    let mut builder = Synthetic::identical();
    let (result, ..) = fixture.run(&plan(), DETERMINISM, &mut builder);
    result.expect("deterministic");
    let [a, b, c] = [&builder.specs[0], &builder.specs[1], &builder.specs[2]];
    assert_eq!(a.workers, 1);
    assert_eq!((b.workers, c.workers), (3, 3));
    assert_ne!(a.cache_dir, b.cache_dir, "two independent cold caches");
    assert_eq!(
        b.cache_dir, c.cache_dir,
        "the warm build reuses the parallel one's"
    );
    assert_ne!(a.destination, b.destination);
    assert_ne!(b.destination, c.destination);
    assert_ne!(a.work_dir, b.work_dir);
}

#[test]
fn different_bytes_fail_even_when_the_reported_hashes_agree() {
    let fixture = Fixture::new();
    let mut builder = Synthetic::new(|spec, call| {
        let mut answer = Answer::ok(b"0123456789");
        if call == 1 {
            answer.bytes = b"01234X6789".to_vec();
            answer.report_hash = Some(sha256_hex(b"0123456789"));
        }
        let _ = spec;
        answer
    });
    let (result, out, err, _) = fixture.run(&plan(), DETERMINISM, &mut builder);
    assert!(matches!(
        failure_of(result),
        BenchError::NotDeterministic(_)
    ));
    assert!(
        err.contains("cold single and cold parallel differ")
            && err.contains("first difference at byte 5"),
        "{err}"
    );
    assert!(err.contains("does not hash to"), "{err}");
    assert!(!out.contains("bench: wrote"));
    assert!(fixture.snapshot().is_empty(), "nothing is written");
}

#[test]
fn a_warm_build_that_differs_fails() {
    let fixture = Fixture::new();
    let mut builder = Synthetic::new(|_, call| {
        Answer::ok(if call == 2 {
            b"warm and different"
        } else {
            b"cold"
        })
    });
    let (result, _, err, _) = fixture.run(&plan(), DETERMINISM, &mut builder);
    assert!(matches!(
        failure_of(result),
        BenchError::NotDeterministic(_)
    ));
    assert!(
        err.contains("cold single and warm parallel differ"),
        "{err}"
    );
    assert!(err.contains("sizes 4 and 18"), "{err}");
}

#[test]
fn a_warm_build_that_reads_nothing_from_its_cache_fails() {
    let fixture = Fixture::new();
    let mut builder = Synthetic::new(|_, call| {
        let mut answer = Answer::ok(b"same");
        if call == 2 {
            answer.cache_hits = Some(0);
        }
        answer
    });
    let (result, _, err, _) = fixture.run(&plan(), DETERMINISM, &mut builder);
    assert!(matches!(
        failure_of(result),
        BenchError::NotDeterministic(_)
    ));
    assert!(
        err.contains("warm parallel build read 0 extraction(s)"),
        "{err}"
    );
}

#[test]
fn a_cold_build_that_reads_a_cache_fails() {
    let fixture = Fixture::new();
    let mut builder = Synthetic::new(|_, call| {
        let mut answer = Answer::ok(b"same");
        if call == 1 {
            answer.cache_hits = Some(12);
        }
        answer
    });
    let (result, _, err, _) = fixture.run(&plan(), DETERMINISM, &mut builder);
    assert!(matches!(
        failure_of(result),
        BenchError::NotDeterministic(_)
    ));
    assert!(
        err.contains("cold parallel build read 12 extraction(s)"),
        "{err}"
    );
}

#[test]
fn a_degraded_build_makes_no_baseline() {
    let fixture = Fixture::new();
    let mut builder = Synthetic::new(|_, _| Answer {
        status: BuildStatus::Degraded,
        ..Answer::ok(b"same")
    });
    let (result, ..) = fixture.run(&plan(), BOTH, &mut builder);
    assert!(matches!(failure_of(result), BenchError::Degraded { .. }));
    assert!(fixture.snapshot().is_empty());
}

// --- performance ---------------------------------------------------------------------

fn golden_only() -> BenchPlan {
    plan_of(&[golden_entry("python", "flask")]).expect("a plan")
}

#[test]
fn perf_takes_medians_and_the_maximum_peak_and_keeps_every_sample() {
    let fixture = Fixture::new();
    // 100 files over (extract + resolve): 1 s → 100/s, 1/3 s → 300/s, 1/2 s → 200/s.
    let durations = [500 * MS, 166_666_667, 250 * MS];
    let peaks = [10, 40, 20];
    let mut builder = Synthetic::new(move |_, call| Answer {
        extract_ns: durations[call],
        resolve_ns: durations[call],
        rss: Some(peaks[call]),
        ..Answer::ok(b"segment")
    });
    let (result, out, ..) = fixture.run(&golden_only(), PERF, &mut builder);
    result.expect("measured");
    assert!(
        out.contains("perf: python/flask: 200.0 files/s (median of 3)"),
        "{out}"
    );
    let performance = &fixture.result("flask.json")["performance"];
    assert_eq!(performance["outcome"], "completed");
    assert_eq!(performance["median_files_per_second"], 200.0);
    assert_eq!(
        performance["peak_rss_bytes"], 40,
        "the maximum, never the median"
    );
    assert_eq!(performance["median_extract_ns"], 250 * MS);
    assert_eq!(performance["samples"].as_array().expect("samples").len(), 3);
    assert_eq!(performance["samples"][1]["peak_rss_bytes"], 40);
    assert_eq!(performance["samples"][0]["workers"], 2);
    assert_eq!(performance["samples"][0]["memory_budget_bytes"], 1 << 29);
    assert_eq!(performance["rss_sample_interval_ms"], 10);
    assert_eq!(performance["host"]["benchmark_workers"], 2);
    // Every sample is a fresh cold cache of its own.
    let caches: Vec<&PathBuf> = builder.specs.iter().map(|s| &s.cache_dir).collect();
    assert_eq!(caches.len(), 3);
    assert!(caches[0] != caches[1] && caches[1] != caches[2] && caches[0] != caches[2]);
}

#[test]
fn a_sample_that_reads_its_cache_fails() {
    let fixture = Fixture::new();
    let mut builder = Synthetic::new(|_, call| Answer {
        cache_hits: (call == 1).then_some(5),
        ..Answer::ok(b"segment")
    });
    let (result, ..) = fixture.run(&golden_only(), PERF, &mut builder);
    assert!(matches!(
        failure_of(result),
        BenchError::WarmSample { sample: 2, hits: 5 }
    ));
}

#[test]
fn samples_that_build_different_segments_fail() {
    let fixture = Fixture::new();
    let mut builder =
        Synthetic::new(|_, call| Answer::ok(if call == 1 { b"other" } else { b"segment" }));
    let (result, ..) = fixture.run(&golden_only(), PERF, &mut builder);
    assert!(matches!(failure_of(result), BenchError::SamplesDiffer(_)));
}

#[test]
fn a_sample_whose_memory_was_never_observed_fails() {
    let fixture = Fixture::new();
    let mut builder = Synthetic::new(|_, _| Answer {
        rss: None,
        ..Answer::ok(b"segment")
    });
    let (result, ..) = fixture.run(&golden_only(), PERF, &mut builder);
    assert!(matches!(failure_of(result), BenchError::NoRss(1)));
}

#[test]
fn a_zero_duration_sample_fails() {
    let fixture = Fixture::new();
    let mut builder = Synthetic::new(|_, _| Answer {
        extract_ns: 0,
        resolve_ns: 0,
        ..Answer::ok(b"segment")
    });
    let (result, ..) = fixture.run(&golden_only(), PERF, &mut builder);
    assert!(matches!(failure_of(result), BenchError::Measure(_)));
}

#[test]
fn a_golden_over_the_memory_ceiling_fails() {
    let fixture = Fixture::new();
    let mut builder = Synthetic::new(|_, _| Answer {
        stop: Some(7 << 30),
        ..Answer::ok(b"segment")
    });
    let (result, ..) = fixture.run(&golden_only(), PERF, &mut builder);
    assert!(matches!(failure_of(result), BenchError::OverCeiling { .. }));
    assert!(fixture.snapshot().is_empty());
}

#[test]
fn a_scale_repository_over_the_ceiling_is_recorded_as_out_of_memory() {
    let fixture = Fixture::new();
    let mut builder = Synthetic::new(|spec, _| Answer {
        stop: spec
            .checkout
            .ends_with("kubernetes")
            .then_some((6 << 30) + 1),
        ..Answer::ok(b"segment")
    });
    let (result, out, _, asked) = fixture.run(&plan(), PERF, &mut builder);
    result.expect("recorded");
    assert_eq!(asked, ["go/gin", "python/flask", "kubernetes"]);
    assert!(
        out.contains("perf: kubernetes: out of memory: stopped in the extract stage"),
        "{out}"
    );
    let scale_builds = builder
        .specs
        .iter()
        .filter(|s| s.checkout.ends_with("kubernetes"))
        .count();
    assert_eq!(scale_builds, 1, "no further sample once stopped");
    let kubernetes = fixture.result("kubernetes.json");
    assert_eq!(kubernetes["provenance"]["kind"], "scale");
    assert!(kubernetes["provenance"].get("language").is_none());
    let performance = &kubernetes["performance"];
    assert_eq!(performance["outcome"], "out_of_memory");
    assert_eq!(performance["stage_reached"], "extract");
    assert_eq!(performance["peak_rss_bytes"], (6_u64 << 30) + 1);
    assert_eq!(performance["memory_ceiling_bytes"], 1_u64 << 32);
    assert_eq!(performance["files_discovered"], 22_000);
    assert!(performance.get("files_extracted").is_none());
    assert_eq!(performance["completed_stages"][0]["stage"], "discover");
    let summary = fixture.result("summary.json");
    let p = &summary["performance"];
    assert_eq!(p["repositories_measured"], 3);
    assert_eq!(p["goldens_measured"], 2);
    assert_eq!(p["scale_repositories_measured"], 1);
    assert_eq!(p["scale_repositories_completed"], 0);
    assert_eq!(p["all_completed"], false);
    assert_eq!(p["scale"]["kubernetes"]["outcome"], "out_of_memory");
}

// --- results ---------------------------------------------------------------------------

#[test]
fn a_failure_part_way_leaves_every_result_as_it_was() {
    let fixture = Fixture::new();
    let mut good = Synthetic::identical();
    fixture
        .run(&plan(), BOTH, &mut good)
        .0
        .expect("a first run");
    let before = fixture.snapshot();
    assert_eq!(before.len(), 4, "{:?}", before.keys());
    // The second repository's second build fails: the first one's new results must
    // not be promoted alone.
    let mut failing = Synthetic::new(|spec, _| {
        let mut answer = Answer::ok(b"changed");
        if spec.checkout.ends_with("python-flask") {
            answer.status = BuildStatus::Degraded;
        }
        answer
    });
    let (result, ..) = fixture.run(&plan(), BOTH, &mut failing);
    assert!(result.is_err());
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn a_determinism_run_keeps_the_performance_baseline_and_the_reverse() {
    let fixture = Fixture::new();
    fixture
        .run(&plan(), PERF, &mut Synthetic::identical())
        .0
        .expect("perf");
    fixture
        .run(&plan(), DETERMINISM, &mut Synthetic::identical())
        .0
        .expect("determinism");
    let flask = fixture.result("flask.json");
    assert!(flask.get("performance").is_some() && flask.get("determinism").is_some());
    let kubernetes = fixture.result("kubernetes.json");
    assert!(kubernetes.get("performance").is_some());
    let summary = fixture.result("summary.json");
    assert!(summary.get("performance").is_some() && summary.get("determinism").is_some());
    fixture
        .run(&plan(), PERF, &mut Synthetic::identical())
        .0
        .expect("perf again");
    assert!(fixture.result("flask.json").get("determinism").is_some());
}

#[test]
fn a_later_format_s_field_survives_and_a_stale_one_is_refused() {
    let fixture = Fixture::new();
    fixture
        .run(&plan(), BOTH, &mut Synthetic::identical())
        .0
        .expect("first");
    let path = fixture.results_dir().join("flask.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).expect("read")).expect("json");
    value["accuracy"] = serde_json::json!({"precision": 0.97});
    fs::write(&path, serde_json::to_vec_pretty(&value).expect("json")).expect("write");
    fixture
        .run(&plan(), DETERMINISM, &mut Synthetic::identical())
        .0
        .expect("second");
    assert_eq!(fixture.result("flask.json")["accuracy"]["precision"], 0.97);

    // The same file from engine 5: its sections are stale, and its unknown field
    // cannot be told fresh, so it is refused and nothing is written.
    value["provenance"]["engine_version"] = Value::from(5);
    fs::write(&path, serde_json::to_vec_pretty(&value).expect("json")).expect("write");
    let before = fixture.snapshot();
    let (result, ..) = fixture.run(&plan(), DETERMINISM, &mut Synthetic::identical());
    let message = failure_of(result).to_string();
    assert!(
        message.contains("engine_version") && message.contains("accuracy"),
        "{message}"
    );
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn a_stale_baseline_is_dropped_not_merged() {
    let fixture = Fixture::new();
    fixture
        .run(&plan(), BOTH, &mut Synthetic::identical())
        .0
        .expect("first");
    let path = fixture.results_dir().join("flask.json");
    let mut old = ResultFile::parse(&fs::read(&path).expect("read"), &path).expect("parses");
    old.provenance.engine_version = 5;
    fs::write(&path, old.to_bytes()).expect("write");
    fixture
        .run(&plan(), DETERMINISM, &mut Synthetic::identical())
        .0
        .expect("second");
    let flask = fixture.result("flask.json");
    assert_eq!(flask["provenance"]["engine_version"], 7);
    assert!(
        flask.get("performance").is_none(),
        "an engine 5 baseline is dropped"
    );
    assert!(flask.get("determinism").is_some());
}

#[test]
fn result_files_are_canonical_and_name_no_path() {
    let fixture = Fixture::new();
    fixture
        .run(&plan(), BOTH, &mut Synthetic::identical())
        .0
        .expect("run");
    for (name, bytes) in fixture.snapshot() {
        let text = String::from_utf8(bytes).expect("utf-8");
        assert!(text.ends_with("}\n"), "{name}");
        let value: Value = serde_json::from_str(&text).expect("json");
        let canonical = pdx_bench::bench::results::canonical_bytes(&value);
        assert_eq!(text.as_bytes(), canonical, "{name} is not canonical");
        let root = fixture.dir.path().to_string_lossy().into_owned();
        assert!(!text.contains(&root), "{name} names a path");
        assert!(!text.contains("branch"), "{name}");
    }
}

#[cfg(unix)]
#[test]
fn a_symlink_where_a_result_belongs_is_refused() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.results_dir()).expect("mkdir");
    let elsewhere = fixture.dir.path().join("elsewhere.json");
    std::os::unix::fs::symlink(&elsewhere, fixture.results_dir().join("flask.json"))
        .expect("symlink");
    let (result, ..) = fixture.run(&plan(), DETERMINISM, &mut Synthetic::identical());
    assert!(result.is_err());
    assert!(!elsewhere.exists());

    let fixture = Fixture::new();
    fs::create_dir_all(fixture.dir.path().join("real")).expect("mkdir");
    std::os::unix::fs::symlink(fixture.dir.path().join("real"), fixture.results_root())
        .expect("symlink");
    let mut builder = Synthetic::identical();
    let (result, ..) = fixture.run(&plan(), DETERMINISM, &mut builder);
    assert!(result.is_err());
    assert!(builder.specs.is_empty(), "refused before anything is built");
}
