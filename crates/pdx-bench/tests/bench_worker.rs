//! The real benchmark worker: `pdx-bench __pdx-bench-worker`, run by the harness on a
//! small project-authored fixture. Two cold builds and a warm one must be the same
//! bytes, the samples measured, and the worker hidden from the command line. No
//! network: the checkout is a local directory.

use std::fmt::Write as _;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use pdx_bench::bench::area::Area;
use pdx_bench::bench::host::Host;
use pdx_bench::bench::plan::{BenchEntry, bench_plan};
use pdx_bench::bench::process::MEMORY_CEILING_BYTES;
use pdx_bench::bench::results::PDX_VERSION;
use pdx_bench::bench::run::{self, BenchError, Checkouts, Modes, Run, Settings, WorkerBuilder};
use pdx_bench::bench::worker::WORKER;
use serde_json::Value;

const HARNESS: &str = env!("CARGO_BIN_EXE_pdx-bench");

/// A small Python project with calls between modules and a test.
fn write_fixture(root: &Path) {
    let files: [(&str, String); 4] = [
        (
            "pkg/__init__.py",
            "from .core import run\n".to_owned(),
        ),
        (
            "pkg/core.py",
            "from .util import helper, other\n\n\ndef run(n):\n    total = 0\n    for i in range(n):\n        total += helper(i)\n    return other(total)\n"
                .to_owned(),
        ),
        (
            "pkg/util.py",
            (0..40).fold(String::new(), |mut text, i| {
                let _ = writeln!(text, "def f{i}(x):\n    return x + {i}\n\n");
                text
            }) + "def helper(x):\n    return f1(x) + f2(x)\n\n\ndef other(x):\n    return helper(x)\n",
        ),
        (
            "tests/test_core.py",
            "from pkg.core import run\n\n\ndef test_run():\n    assert run(3) >= 0\n".to_owned(),
        ),
    ];
    for (path, text) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().expect("a parent")).expect("mkdir");
        fs::write(path, text).expect("write");
    }
}

struct Fixture(PathBuf);

impl Checkouts for Fixture {
    fn checkout(
        &mut self,
        _entry: &BenchEntry,
        _err: &mut dyn Write,
    ) -> Result<PathBuf, BenchError> {
        Ok(self.0.clone())
    }
}

#[test]
fn two_cold_builds_and_a_warm_one_of_a_fixture_are_identical_and_measured() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let checkout = tmp.path().join("checkout");
    write_fixture(&checkout);
    let lock = format!(
        "lock_version = 1\nfetch_date = \"2026-01-01\"\nscale = []\n[[golden]]\nlanguage = \"python\"\n\
         url = \"git.example/acme/fixture\"\ndefault_branch = \"main\"\ncommit = \"{}\"\n\
         licence = \"MIT\"\nfetch_date = \"2026-01-01\"\n",
        "c".repeat(40)
    );
    let plan = bench_plan(&toml::from_str(&lock).expect("a lock")).expect("a plan");
    let area = Area::open(&tmp.path().join("area")).expect("area");
    let settings = Settings {
        parallel_workers: 3,
        perf_workers: 2,
        memory_budget_bytes: 512 * 1024 * 1024,
        ceiling: MEMORY_CEILING_BYTES,
    };
    let run = Run {
        plan: &plan,
        modes: Modes {
            determinism: true,
            performance: true,
        },
        settings,
        host: Host::current(settings.perf_workers, settings.memory_budget_bytes),
        area: &area,
        results_root: tmp.path().join("results"),
    };
    let mut builder = WorkerBuilder::new(PathBuf::from(HARNESS), MEMORY_CEILING_BYTES);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let result = run::run(
        &run,
        &mut Fixture(checkout.clone()),
        &mut builder,
        &mut out,
        &mut err,
    );
    let (out, err) = (
        String::from_utf8(out).expect("utf-8"),
        String::from_utf8(err).expect("utf-8"),
    );
    result.unwrap_or_else(|e| panic!("{e}\n{err}"));
    assert!(
        out.contains("determinism: python/fixture: identical"),
        "{out}"
    );
    assert!(out.contains("perf: python/fixture: "), "{out}");

    let results = tmp.path().join("results").join(PDX_VERSION);
    let file: Value =
        serde_json::from_slice(&fs::read(results.join("fixture.json")).expect("read"))
            .expect("json");
    let d = &file["determinism"];
    assert_eq!(d["cold_single_hash"], d["cold_parallel_hash"]);
    assert_eq!(d["cold_single_hash"], d["warm_parallel_hash"]);
    assert_eq!(d["files"], 4);
    assert!(d["warm_cache_hits"].as_u64().expect("hits") > 0);
    assert_eq!(d["warm_cache_hits"], d["cold_cache_writes"]);
    let p = &file["performance"];
    assert_eq!(p["outcome"], "completed");
    assert_eq!(p["content_sha256"], d["cold_single_hash"]);
    assert!(p["peak_rss_bytes"].as_u64().expect("rss") > 0);
    assert!(p["median_files_per_second"].as_f64().expect("rate") > 0.0);
    for sample in p["samples"].as_array().expect("samples") {
        assert_eq!(sample["status"], "ready");
        assert_eq!(sample["files"], 4);
        let stages: u64 = [
            "discover_ns",
            "extract_ns",
            "resolve_ns",
            "derive_ns",
            "write_ns",
        ]
        .iter()
        .map(|k| sample[k].as_u64().expect("ns"))
        .sum();
        assert!(sample["total_wall_ns"].as_u64().expect("wall") >= stages);
    }
    // The checkout is left as it was: the segments and caches are the area's.
    let mut names: Vec<String> = walk(&checkout);
    names.sort();
    assert_eq!(
        names,
        [
            "pkg/__init__.py",
            "pkg/core.py",
            "pkg/util.py",
            "tests/test_core.py"
        ]
    );
}

fn walk(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("read").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                found.push(
                    path.strip_prefix(root)
                        .expect("below")
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    found
}

#[test]
fn the_worker_is_served_before_parsing_and_answers_a_bad_request_with_one_line() {
    let mut child = Command::new(HARNESS)
        .arg(WORKER)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawns");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"{\"checkout\": 1}")
        .expect("write");
    let output = child.wait_with_output().expect("waits");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(stderr.starts_with("pdx-bench worker: "), "{stderr}");
}

#[test]
fn the_worker_appears_in_no_help_and_bench_needs_a_mode() {
    let help = Command::new(HARNESS).arg("--help").output().expect("runs");
    assert_eq!(help.status.code(), Some(0));
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(text.contains("bench") && !text.contains(WORKER), "{text}");
    let bench = Command::new(HARNESS).arg("bench").output().expect("runs");
    assert_eq!(
        bench.status.code(),
        Some(1),
        "a usage error exits 1, never 2"
    );
    let positional = Command::new(HARNESS)
        .args(["bench", "--perf", "linux"])
        .output()
        .expect("runs");
    assert_eq!(positional.status.code(), Some(1));
}

#[test]
fn a_refused_variable_is_named_and_its_value_never_shown() {
    let output = Command::new(HARNESS)
        .args(["bench", "--perf"])
        .env("PDX_MEM_BUDGET_MB", "a-distinctive-value")
        .output()
        .expect("runs");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("PDX_MEM_BUDGET_MB is set"), "{stderr}");
    assert!(!stderr.contains("a-distinctive-value"), "{stderr}");
}
