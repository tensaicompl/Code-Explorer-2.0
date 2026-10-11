//! A golden run with synthetic producers: comparison never writes, acceptance is
//! explicit and all-or-nothing, stale and stray files are handled, and only a ready
//! `pdx index` makes a golden. No fetch, no `pdx`, no network.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use pdx_bench::golden::cache::{Cache, CacheError, remove_owned_file};
use pdx_bench::golden::index::{IndexError, PdxRun, index_arguments, interpret, sibling_pdx};
use pdx_bench::golden::plan::GoldenEntry;
use pdx_bench::golden::run::{
    self, EXTRACTION_SWITCHES, GoldenError, Mode, Produced, Producer, preflight,
};
use pdx_bench::golden::{GoldenEdge, GoldenExpected, NodeKindCount};

fn entry(language: &str, repo: &str) -> GoldenEntry {
    GoldenEntry {
        language: language.to_owned(),
        repo: repo.to_owned(),
        url: format!("git.example/acme/{repo}"),
        commit: "0123456789abcdef0123456789abcdef01234567".to_owned(),
    }
}

fn golden(edges: usize) -> GoldenExpected {
    GoldenExpected {
        node_counts: vec![NodeKindCount {
            kind: "Function".to_owned(),
            count: 2,
        }],
        drawn_edges: (0..edges)
            .map(|i| GoldenEdge {
                src_qn: format!("m.f{i}"),
                dst_qn: "m.g".to_owned(),
                kind: "CALLS".to_owned(),
                band: "exact".to_owned(),
            })
            .collect(),
    }
}

/// A producer answering from a table: a golden per repository, or a failure.
struct Table(BTreeMap<String, Result<GoldenExpected, String>>);

impl Producer for Table {
    fn produce(&mut self, entry: &GoldenEntry) -> Result<Produced, GoldenError> {
        match self.0.get(&entry.key()).expect("a planned repository") {
            Ok(expected) => Ok(Produced {
                expected: expected.clone(),
                content_sha256: "ab".repeat(32),
            }),
            Err(why) => Err(GoldenError::Index(IndexError::Failed {
                code: "1".to_owned(),
                stderr: why.clone(),
            })),
        }
    }
}

/// A workspace's `bench/` with its golden directory and cache.
struct Bench {
    dir: tempfile::TempDir,
}

impl Bench {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().expect("a directory"),
        }
    }
    fn golden(&self) -> PathBuf {
        self.dir.path().join("golden")
    }
    fn cache(&self) -> Cache {
        Cache::open(&self.dir.path().join(".cache")).expect("a cache")
    }
    fn commit(&self, language: &str, repo: &str, bytes: &[u8]) -> PathBuf {
        let dir = self.golden().join(language);
        std::fs::create_dir_all(&dir).expect("a directory");
        let path = dir.join(format!("{repo}.expected.json"));
        std::fs::write(&path, bytes).expect("a golden");
        path
    }
    fn run(
        &self,
        plan: &[GoldenEntry],
        mode: Mode,
        producer: &mut dyn Producer,
    ) -> (Result<(), GoldenError>, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let result = run::run(
            &self.golden(),
            &self.cache(),
            plan,
            mode,
            producer,
            &mut out,
            &mut err,
        );
        (
            result,
            String::from_utf8(out).expect("UTF-8"),
            String::from_utf8(err).expect("UTF-8"),
        )
    }
}

fn table(rows: &[(&str, Result<GoldenExpected, &str>)]) -> Table {
    Table(
        rows.iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone().map_err(str::to_owned)))
            .collect(),
    )
}

#[test]
fn compare_passes_when_every_golden_matches() {
    let bench = Bench::new();
    let plan = [entry("go", "beta"), entry("python", "shop")];
    bench.commit("go", "beta", &golden(1).to_canonical_bytes());
    bench.commit("python", "shop", &golden(2).to_canonical_bytes());
    let mut producer = table(&[("go/beta", Ok(golden(1))), ("python/shop", Ok(golden(2)))]);
    let (result, out, _) = bench.run(&plan, Mode::Compare, &mut producer);
    result.expect("matches");
    assert_eq!(
        out,
        format!(
            "go/beta: matches, 2 nodes, 1 drawn edges, segment {0}\n\
             python/shop: matches, 2 nodes, 2 drawn edges, segment {0}\n\
             golden: 2 repositories match\n",
            "ab".repeat(32)
        )
    );
}

#[test]
fn compare_never_writes_and_accept_is_explicit() {
    let bench = Bench::new();
    let plan = [entry("python", "shop")];
    let committed = bench.commit("python", "shop", &golden(1).to_canonical_bytes());
    let before = std::fs::read(&committed).expect("the golden");
    let mut producer = table(&[("python/shop", Ok(golden(3)))]);

    // A difference fails comparison and leaves the committed golden alone.
    let (result, _, err) = bench.run(&plan, Mode::Compare, &mut producer);
    assert!(
        matches!(result, Err(GoldenError::Mismatch(1))),
        "{result:?}"
    );
    assert!(err.contains("python/shop: differs"), "{err}");
    assert!(
        err.contains("actual only: m.f1 -> m.g CALLS exact"),
        "{err}"
    );
    assert_eq!(std::fs::read(&committed).expect("the golden"), before);

    // A golden in another format with the same graph is a difference too.
    let pretty = serde_json::to_vec(&golden(1)).expect("JSON");
    std::fs::write(&committed, &pretty).expect("reformatted");
    let mut same = table(&[("python/shop", Ok(golden(1)))]);
    let (result, _, err) = bench.run(&plan, Mode::Compare, &mut same);
    assert!(matches!(result, Err(GoldenError::Mismatch(1))));
    assert!(err.contains("not in the canonical format"), "{err}");
    assert_eq!(std::fs::read(&committed).expect("the golden"), pretty);

    // Only --accept replaces it.
    let (result, out, _) = bench.run(&plan, Mode::Accept, &mut producer);
    result.expect("accepted");
    assert_eq!(
        std::fs::read(&committed).expect("the golden"),
        golden(3).to_canonical_bytes()
    );
    assert!(
        out.contains("python/shop: accepted bench/golden/python/shop.expected.json"),
        "{out}"
    );
}

#[test]
fn compare_fails_on_a_missing_golden_and_creates_none() {
    let bench = Bench::new();
    let plan = [entry("python", "shop")];
    let mut producer = table(&[("python/shop", Ok(golden(1)))]);
    let (result, _, err) = bench.run(&plan, Mode::Compare, &mut producer);
    assert!(matches!(result, Err(GoldenError::Mismatch(1))));
    assert!(err.contains("python/shop: no committed golden"), "{err}");
    assert!(!bench.golden().exists(), "comparison created bench/golden/");
}

#[test]
fn accept_is_all_or_nothing() {
    let bench = Bench::new();
    let plan = [entry("go", "alpha"), entry("python", "broken")];
    let committed = bench.commit("go", "alpha", &golden(1).to_canonical_bytes());
    let before = std::fs::read(&committed).expect("the golden");
    // The first repository produces a new graph; the second fails after it.
    let mut producer = table(&[
        ("go/alpha", Ok(golden(5))),
        ("python/broken", Err("the engine crashed")),
    ]);
    let (result, out, _) = bench.run(&plan, Mode::Accept, &mut producer);
    match result {
        Err(GoldenError::Repository { key, .. }) => assert_eq!(key, "python/broken"),
        other => panic!("expected the second repository's failure, got {other:?}"),
    }
    assert_eq!(
        std::fs::read(&committed).expect("the golden"),
        before,
        "promoted part-way"
    );
    assert!(!bench.golden().join("python").exists());
    assert!(out.is_empty(), "{out}");
}

#[test]
fn stale_goldens_fail_comparison_and_go_on_accept() {
    let bench = Bench::new();
    let plan = [entry("python", "shop")];
    bench.commit("python", "shop", &golden(1).to_canonical_bytes());
    let stale = bench.commit("ruby", "gone", &golden(1).to_canonical_bytes());
    let mut producer = table(&[("python/shop", Ok(golden(1)))]);

    let (result, _, err) = bench.run(&plan, Mode::Compare, &mut producer);
    assert!(matches!(result, Err(GoldenError::Mismatch(1))));
    assert!(
        err.contains("ruby/gone.expected.json: a committed golden for no [[golden]]"),
        "{err}"
    );
    assert!(stale.exists());

    let (result, out, _) = bench.run(&plan, Mode::Accept, &mut producer);
    result.expect("accepted");
    assert!(!stale.exists());
    assert!(
        !bench.golden().join("ruby").exists(),
        "its empty directory is removed"
    );
    assert!(out.contains("ruby/gone.expected.json: removed"), "{out}");
    assert!(bench.golden().join("python/shop.expected.json").exists());
}

#[test]
fn stray_files_in_the_golden_tree_stop_both_modes() {
    let bench = Bench::new();
    let plan = [entry("python", "shop")];
    let golden_path = bench.commit("python", "shop", &golden(1).to_canonical_bytes());
    let stray = bench.golden().join("python").join("notes.md");
    std::fs::write(&stray, b"mine\n").expect("a stray file");
    for mode in [Mode::Compare, Mode::Accept] {
        let mut producer = table(&[("python/shop", Ok(golden(2)))]);
        let (result, _, _) = bench.run(&plan, mode, &mut producer);
        assert!(
            matches!(result, Err(GoldenError::UnknownContent(_))),
            "{mode:?}"
        );
        assert!(stray.exists());
        assert_eq!(
            std::fs::read(&golden_path).expect("the golden"),
            golden(1).to_canonical_bytes(),
            "{mode:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn accept_never_writes_through_a_link() {
    let bench = Bench::new();
    let plan = [entry("python", "shop")];
    let theirs = bench.dir.path().join("theirs.json");
    std::fs::write(&theirs, b"keep\n").expect("a file");
    std::fs::create_dir_all(bench.golden().join("python")).expect("a directory");
    std::os::unix::fs::symlink(&theirs, bench.golden().join("python/shop.expected.json"))
        .expect("a link");
    let mut producer = table(&[("python/shop", Ok(golden(1)))]);
    let (result, _, _) = bench.run(&plan, Mode::Accept, &mut producer);
    assert!(result.is_err(), "{result:?}");
    assert_eq!(std::fs::read(&theirs).expect("kept"), b"keep\n");
}

#[test]
fn preflight_refuses_every_extraction_switch() {
    preflight(|_| false).expect("nothing set");
    for name in EXTRACTION_SWITCHES {
        let e = preflight(|n| n == name).expect_err("refused");
        let message = e.to_string();
        assert!(message.starts_with(name), "{message}");
        assert!(!message.contains("secret-value"), "{message}");
    }
}

#[test]
fn extraction_switch_list_matches_the_engine() {
    assert_eq!(EXTRACTION_SWITCHES, pdx_engine::EXTRACTION_SWITCHES);
}

fn ran(code: Option<i32>, stdout: &str, stderr: &str) -> PdxRun {
    PdxRun {
        code,
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

const READY: &str = r#"{"status":"ready","profile":"structural","segment":{"output":"x","content_sha256":"cafe","size_bytes":4096},"degraded_reasons":[]}"#;

#[test]
fn a_ready_index_is_the_only_success() {
    let dir = tempfile::tempdir().expect("a directory");
    let segment = dir.path().join("segment.db");
    std::fs::write(&segment, b"segment").expect("a segment");
    let ready = interpret(&ran(Some(0), READY, ""), &segment).expect("ready");
    assert_eq!(ready.content_sha256, "cafe");
    assert_eq!(ready.size_bytes, 4096);
    // Exit 0, but no segment where it was asked for, or no ready report.
    let missing = dir.path().join("absent.db");
    assert!(matches!(
        interpret(&ran(Some(0), READY, ""), &missing),
        Err(IndexError::Unexpected(_))
    ));
    let degraded_status = READY.replace("\"ready\"", "\"degraded\"");
    assert!(matches!(
        interpret(&ran(Some(0), &degraded_status, ""), &segment),
        Err(IndexError::Unexpected(_))
    ));
    assert!(matches!(
        interpret(&ran(Some(0), "not json", ""), &segment),
        Err(IndexError::Unexpected(_))
    ));
}

#[test]
fn a_degraded_index_is_refused_with_its_reasons() {
    let dir = tempfile::tempdir().expect("a directory");
    let segment = dir.path().join("segment.db");
    std::fs::write(&segment, b"segment").expect("a segment left behind");
    let degraded = r#"{"status":"degraded","segment":{"content_sha256":"cafe","size_bytes":1},"degraded_reasons":[{"kind":"failed_over_threshold","failed":3,"skipped_size":1,"files":20},{"kind":"engine_crash","paths":["src/a.c"]}]}"#;
    match interpret(&ran(Some(2), degraded, ""), &segment) {
        Err(IndexError::Degraded(reasons)) => {
            assert!(
                reasons
                    .contains("failed_over_threshold (failed 3, skipped for size 1, of 20 files)"),
                "{reasons}"
            );
            assert!(
                reasons.contains("engine_crash (1 file(s): src/a.c)"),
                "{reasons}"
            );
        }
        other => panic!("expected a refused degraded index, got {other:?}"),
    }
}

#[test]
fn a_failed_index_is_refused() {
    let dir = tempfile::tempdir().expect("a directory");
    let segment = dir.path().join("segment.db");
    std::fs::write(&segment, b"segment").expect("a segment left behind");
    for code in [Some(1), Some(101), None] {
        match interpret(
            &ran(code, "", "pdx: the working tree is not clean"),
            &segment,
        ) {
            Err(IndexError::Failed { stderr, .. }) => {
                assert_eq!(stderr, "pdx: the working tree is not clean");
            }
            other => panic!("{code:?}: expected a failure, got {other:?}"),
        }
    }
    // A long standard error is cut short.
    let long = "x".repeat(10_000);
    match interpret(&ran(Some(1), "", &long), &segment) {
        Err(IndexError::Failed { stderr, .. }) => assert_eq!(stderr.len(), 1024),
        other => panic!("{other:?}"),
    }
}

#[test]
fn pdx_is_run_explicitly_and_from_beside_the_harness() {
    let checkout = Path::new("/w/bench/.cache/repos/python/shop");
    let segment = Path::new("/w/bench/.cache/segments/python/shop/segment.db");
    assert_eq!(
        index_arguments(checkout, segment),
        [
            "index",
            "/w/bench/.cache/repos/python/shop",
            "--out",
            "/w/bench/.cache/segments/python/shop/segment.db",
            "--profile",
            "structural",
            "--json",
        ]
        .map(OsString::from)
    );
    assert_eq!(
        sibling_pdx(Path::new("/w/target/release/pdx-bench")),
        Some(PathBuf::from(format!(
            "/w/target/release/pdx{}",
            std::env::consts::EXE_SUFFIX
        )))
    );
}

#[test]
fn only_harness_segments_are_removed() {
    let dir = tempfile::tempdir().expect("a directory");
    let root = dir.path().join("segments");
    std::fs::create_dir_all(root.join("python/shop")).expect("directories");
    let owned = root.join("python/shop/segment.db");
    std::fs::write(&owned, b"old").expect("a segment");
    let outside = dir.path().join("elsewhere.db");
    std::fs::write(&outside, b"theirs").expect("a file");
    for path in [outside.clone(), root.join("../elsewhere.db"), root.clone()] {
        assert!(
            matches!(remove_owned_file(&root, &path), Err(CacheError::Outside(_))),
            "{}",
            path.display()
        );
    }
    assert!(outside.exists());
    // A directory where a segment belongs is refused, not emptied.
    assert!(matches!(
        remove_owned_file(&root, &root.join("python/shop")),
        Err(CacheError::Refused(_))
    ));
    remove_owned_file(&root, &owned).expect("removed");
    assert!(!owned.exists());
    remove_owned_file(&root, &owned).expect("absent is fine");
}

#[cfg(unix)]
#[test]
fn a_linked_cache_root_is_refused() {
    let dir = tempfile::tempdir().expect("a directory");
    let theirs = dir.path().join("theirs");
    std::fs::create_dir(&theirs).expect("a directory");
    let cache = dir.path().join(".cache");
    std::os::unix::fs::symlink(&theirs, &cache).expect("a link");
    assert!(matches!(Cache::open(&cache), Err(CacheError::Refused(_))));
    let file = dir.path().join("file-cache");
    std::fs::write(&file, b"").expect("a file");
    assert!(matches!(Cache::open(&file), Err(CacheError::Refused(_))));
}

#[test]
fn the_workspace_root_is_required() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let resolved = run::workspace_root(&root).expect("the workspace root");
    assert!(resolved.is_absolute());
    let elsewhere = tempfile::tempdir().expect("a directory");
    assert!(matches!(
        run::workspace_root(elsewhere.path()),
        Err(GoldenError::Workspace(_))
    ));
    assert!(matches!(
        run::workspace_root(&root.join("crates")),
        Err(GoldenError::Workspace(_))
    ));
}
