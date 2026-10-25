//! Stage 2, extract (specification 4.5): what is extracted and what is carried
//! forward, the extraction cache's identity and integrity, the memory budget, worker
//! order, and that no secret reaches the engine or the cache.
//!
//! The engine is the real one, in process, behind a backend that counts what it is
//! given and can, for a test that says so, change what it returns (a truncation, lost
//! work, a failure, a crash or a timeout) the way the engine or an isolated worker
//! would report it. Tests that need an engine switch from the environment run in a
//! child process of this binary, so no other test sees the switch.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pdx_core::config::PdxConfig;
use pdx_core::consts::{
    ENGINE_VERSION, EXTRACT_BATCH_MAX_FILES, EXTRACT_CACHE_FORMAT_VERSION, LANGUAGE_MATRIX_VERSION,
    SECRET_DETECTOR_VERSION, SEGMENT_SCHEMA_VERSION,
};
use pdx_core::index::discover::{DiscoveredFile, discover};
use pdx_core::index::extract::{
    BlobSha, CacheDefect, CacheEnvelope, CacheKey, ExtractBackend, ExtractCache, ExtractError,
    ExtractLimits, ExtractReport, ExtractStage, FileOutcome, FsCache, SourceChange, plan_batches,
    prepare_source,
};
use pdx_core::secrets::{PRIVATE_KEY_LABELS, SecretPolicyDigest};
use pdx_engine::isolate::{ExtractFailure, ExtractOutcome, Extractor, IsolationError, SourceFile};
use pdx_engine::{
    Engine, EngineError, ExtractRecord, FileExtract, FileStatus, LexicalFacts, MacroMember,
    NODE_BUDGET_ENV, SourceDigest, TextRef,
};

/// `extract` with its record changed by `f` and bound again to its source, as a twisted
/// backend answers.
fn twist(extract: &mut FileExtract, f: impl FnOnce(&mut ExtractRecord)) {
    let mut record = extract.record().clone();
    f(&mut record);
    *extract = record
        .bind(extract.shared_source())
        .expect("a twist keeps the source");
}

// --- fixtures ----------------------------------------------------------------------

/// A checkout in a temporary directory.
struct Checkout {
    dir: tempfile::TempDir,
}

impl Checkout {
    fn new() -> Self {
        Self {
            dir: tempfile::Builder::new()
                .prefix("pdx-extract-test-")
                .tempdir()
                .expect("a directory"),
        }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn write(&self, relative: &str, content: impl AsRef<[u8]>) -> &Self {
        let path = self.root().join(relative);
        fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
        fs::write(&path, content).expect("a file");
        self
    }

    fn config(&self) -> PdxConfig {
        PdxConfig::load(self.root()).expect("the configuration loads")
    }

    fn discover(&self) -> Vec<DiscoveredFile> {
        discover(self.root(), &self.config()).expect("discovery succeeds")
    }
}

/// A cache in a temporary directory of its own.
struct Cache {
    dir: tempfile::TempDir,
    cache: FsCache,
}

impl Cache {
    fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("pdx-extract-cache-")
            .tempdir()
            .expect("a directory");
        let cache = FsCache::at(dir.path().join("extract"));
        Self { dir, cache }
    }

    /// Every entry file, by path below the cache, with its bytes.
    fn entries(&self) -> BTreeMap<String, Vec<u8>> {
        fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            let Ok(entries) = fs::read_dir(dir) else {
                return;
            };
            for entry in entries {
                let path = entry.expect("an entry").path();
                if path.is_dir() {
                    walk(&path, root, out);
                } else {
                    let rel = path.strip_prefix(root).unwrap().to_string_lossy();
                    out.insert(rel.replace('\\', "/"), fs::read(&path).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(self.dir.path(), self.dir.path(), &mut out);
        out
    }
}

/// What the probe does to what the engine returns.
#[derive(Clone, Copy, Debug, Default)]
enum Twist {
    #[default]
    None,
    /// Every extraction reports a truncated walk.
    Truncate,
    /// Every extraction reports lost work.
    LoseWork,
    /// Every extraction reports a failed parse.
    StatusFailed,
    /// The engine refuses the named file.
    FailOn(&'static str),
    /// The engine crashes on the named file.
    CrashOn(&'static str),
    /// An isolated worker times out on a batch with the named file.
    TimeoutOn(&'static str),
    /// A batch with the named file takes a while.
    SlowOn(&'static str),
}

/// What the backends of one stage run were given.
#[derive(Default)]
struct Probe {
    twist: Twist,
    /// Files given to the engine.
    files: AtomicUsize,
    /// The bytes of each batch given to the engine.
    batch_bytes: Mutex<Vec<u64>>,
    /// Bytes being extracted at once, and the most at any time.
    in_flight: AtomicU64,
    peak: AtomicU64,
    /// Every source given to the engine.
    sources: Mutex<Vec<SourceFile>>,
    /// The first file of each batch, in the order batches finished.
    finished: Mutex<Vec<String>>,
}

impl Probe {
    fn new(twist: Twist) -> Arc<Self> {
        Arc::new(Self {
            twist,
            ..Self::default()
        })
    }

    fn files(&self) -> usize {
        self.files.load(Ordering::SeqCst)
    }
}

/// The production in-process backend, counted.
struct Counting {
    engine: Extractor,
    probe: Arc<Probe>,
}

impl ExtractBackend for Counting {
    fn extract_batch(
        &mut self,
        files: &[SourceFile],
    ) -> Result<Vec<ExtractOutcome>, IsolationError> {
        let p = &self.probe;
        let bytes: u64 = files.iter().map(|f| f.source.len() as u64).sum();
        let now = p.in_flight.fetch_add(bytes, Ordering::SeqCst) + bytes;
        p.peak.fetch_max(now, Ordering::SeqCst);
        p.batch_bytes.lock().unwrap().push(bytes);
        p.files.fetch_add(files.len(), Ordering::SeqCst);
        p.sources.lock().unwrap().extend(files.iter().cloned());
        let names = |name: &str| files.iter().any(|f| f.rel_path == name);

        let result = match p.twist {
            Twist::TimeoutOn(name) if names(name) => Err(IsolationError::Timeout {
                after: Duration::from_millis(1),
                worker: 0,
            }),
            _ => {
                if let Twist::SlowOn(name) = p.twist
                    && names(name)
                {
                    std::thread::sleep(Duration::from_millis(300));
                }
                let mut outcomes = self.engine.extract_batch(files)?;
                for (file, outcome) in files.iter().zip(&mut outcomes) {
                    let ExtractOutcome::Extracted(extract) = outcome else {
                        continue;
                    };
                    match p.twist {
                        Twist::Truncate => twist(extract, |r| r.truncated = true),
                        Twist::LoseWork => twist(extract, |r| r.extraction_lost = 3),
                        Twist::StatusFailed => twist(extract, |r| r.status = FileStatus::Failed),
                        Twist::FailOn(name) if file.rel_path == name => {
                            *outcome = ExtractOutcome::Failed(ExtractFailure::Engine(
                                EngineError::Internal,
                            ));
                        }
                        Twist::CrashOn(name) if file.rel_path == name => {
                            *outcome = ExtractOutcome::Failed(ExtractFailure::EngineCrash);
                        }
                        _ => {}
                    }
                }
                Ok(outcomes)
            }
        };
        p.finished.lock().unwrap().push(files[0].rel_path.clone());
        p.in_flight.fetch_sub(bytes, Ordering::SeqCst);
        result
    }
}

fn backend(
    probe: &Arc<Probe>,
) -> impl Fn() -> Result<Box<dyn ExtractBackend>, ExtractError> + Sync {
    let probe = Arc::clone(probe);
    move || {
        let engine = Engine::new().map_err(ExtractError::Engine)?;
        Ok(Box::new(Counting {
            engine: Extractor::InProcess(engine),
            probe: Arc::clone(&probe),
        }) as Box<dyn ExtractBackend>)
    }
}

const LIMITS: ExtractLimits = ExtractLimits {
    requested_workers: 2,
    memory_budget_bytes: 64 << 20,
};

/// Runs the stage over a checkout under its own configuration.
fn run(checkout: &Checkout, cache: Option<&Cache>, probe: &Arc<Probe>) -> ExtractReport {
    try_run(checkout, &checkout.config(), cache, probe, LIMITS).expect("the stage runs")
}

fn try_run(
    checkout: &Checkout,
    config: &PdxConfig,
    cache: Option<&Cache>,
    probe: &Arc<Probe>,
    limits: ExtractLimits,
) -> Result<ExtractReport, ExtractError> {
    let files = discover(checkout.root(), config).expect("discovery succeeds");
    run_files(checkout.root(), &files, config, cache, probe, limits)
}

fn run_files(
    root: &Path,
    files: &[DiscoveredFile],
    config: &PdxConfig,
    cache: Option<&Cache>,
    probe: &Arc<Probe>,
    limits: ExtractLimits,
) -> Result<ExtractReport, ExtractError> {
    let factory = backend(probe);
    let mut stage = ExtractStage::new(root, &config.secrets, limits).with_backend(&factory);
    if let Some(cache) = cache {
        stage = stage.with_cache(&cache.cache);
    }
    stage.run(files)
}

/// The extraction of `path`.
fn extract_of<'r>(report: &'r ExtractReport, path: &str) -> &'r FileExtract {
    match &outcome_of(report, path) {
        FileOutcome::Extracted { extract, .. } => extract,
        other => panic!("{path}: {other:?}"),
    }
}

fn outcome_of<'r>(report: &'r ExtractReport, path: &str) -> &'r FileOutcome {
    &report
        .files
        .iter()
        .find(|f| f.path == path)
        .unwrap_or_else(|| panic!("{path} is not in the report"))
        .outcome
}

/// The key the stage stores `path` under, from its original bytes.
fn key_for(checkout: &Checkout, config: &PdxConfig, path: &str, language: &str) -> CacheKey {
    let original = fs::read(checkout.root().join(path)).unwrap();
    CacheKey::new(
        SecretPolicyDigest::of(&config.secrets),
        language,
        path,
        BlobSha::of(&original),
    )
}

/// Writes `bytes` as the entry for `key`.
fn plant(cache: &Cache, key: &CacheKey, bytes: &[u8]) {
    let path = cache.cache.object_path(key);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

const CHILD: &str = "PDX_CORE_TEST_CHILD";
const ROOT_ENV: &str = "PDX_CORE_TEST_ROOT";
const CACHE_ENV: &str = "PDX_CORE_TEST_CACHE";

/// Runs the calling test again in a child process of this binary with `env` set
/// there only; returns `false` in the child, which runs the test's child half.
fn rerun_in_child(test: &str, env: &[(&str, &str)]) -> bool {
    rerun_in_child_at(test, None, env)
}

/// [`rerun_in_child`], with the child's working directory at `cwd` when there is one.
fn rerun_in_child_at(test: &str, cwd: Option<&Path>, env: &[(&str, &str)]) -> bool {
    if std::env::var_os(CHILD).is_some() {
        return false;
    }
    let mut child = Command::new(std::env::current_exe().expect("the test binary"));
    child
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .envs(env.iter().copied());
    if let Some(cwd) = cwd {
        child.current_dir(cwd);
    }
    let out = child.output().expect("the test binary runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{test} failed in its child:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("1 passed"), "{test} did not run:\n{stdout}");
    true
}

fn in_child() -> bool {
    std::env::var_os(CHILD).is_some()
}

/// The checkout and cache the parent named, in the child.
fn child_dirs() -> (PathBuf, PathBuf) {
    (
        PathBuf::from(std::env::var_os(ROOT_ENV).expect("the parent names the checkout")),
        PathBuf::from(std::env::var_os(CACHE_ENV).expect("the parent names the cache")),
    )
}

/// Runs the stage in the child over the parent's checkout and cache.
fn run_in_child_dirs(probe: &Arc<Probe>) -> (ExtractReport, FsCache) {
    let (root, cache_dir) = child_dirs();
    let config = PdxConfig::load(&root).unwrap();
    let files = discover(&root, &config).unwrap();
    let cache = FsCache::at(cache_dir);
    let factory = backend(probe);
    let report = ExtractStage::new(&root, &config.secrets, LIMITS)
        .with_backend(&factory)
        .with_cache(&cache)
        .run(&files)
        .expect("the stage runs");
    (report, cache)
}

const PYTHON: &[u8] = b"def greet(name):\n    return 'hello ' + name\n\n\nclass Greeter:\n    def run(self):\n        return greet('world')\n";

/// TypeScript whose types the resolver evaluates while the file is extracted, so a
/// starved type budget loses work on it.
const TYPED: &[u8] = b"interface Item { name: string; qty: number }
class Store {
    private items: Map<string, Item> = new Map();
    add(item: Item): void { this.items.set(item.name, item); }
    total(): number { let t = 0; for (const i of this.items.values()) { t += i.qty; } return t; }
}
export function build(): Store { const s = new Store(); s.add({ name: 'a', qty: 1 }); return s; }
";

// --- the named acceptance tests ----------------------------------------------------

#[test]
fn cache_hit_skips_engine() {
    let checkout = Checkout::new();
    checkout
        .write("src/app.py", PYTHON)
        .write("src/store.ts", TYPED)
        .write("docs/guide.md", "# Guide\n\nRun `app`.\n");
    let cache = Cache::new();

    let first_probe = Probe::new(Twist::None);
    let first = run(&checkout, Some(&cache), &first_probe);
    assert_eq!(first_probe.files(), 3, "a cold cache extracts every file");
    assert_eq!(first.stats.cache_misses, 3);
    assert_eq!(first.stats.cache_writes, 3);
    assert_eq!(first.stats.cache_hits, 0);
    assert_eq!(cache.entries().len(), 3);

    let second_probe = Probe::new(Twist::None);
    let second = run(&checkout, Some(&cache), &second_probe);
    assert_eq!(second_probe.files(), 0, "a warm cache extracts nothing");
    assert_eq!(second.stats.cache_hits, 3);
    assert_eq!(second.stats.cache_writes, 0);
    // The same extractions, outcomes and order, from the cache.
    assert_eq!(second.files, first.files);
    assert!(matches!(
        outcome_of(&second, "src/app.py"),
        FileOutcome::Extracted { .. }
    ));
}

/// The blob id `git hash-object` gives `bytes`, read from standard input.
fn git_hash_object(bytes: &[u8]) -> String {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let mut git = Command::new("git")
        .args(["hash-object", "--stdin"])
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("git runs");
    git.stdin.take().unwrap().write_all(bytes).unwrap();
    let out = git.wait_with_output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

#[test]
fn blob_sha_matches_git() {
    // Independently computed: sha1("blob <len>\0" + bytes).
    let fixed: [(&[u8], &str); 5] = [
        (b"", "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"),
        (b"hello world\n", "3b18e512dba79e4c8300dd08aeb37f8e728b8dad"),
        (
            b"line one\r\nline two\r\n",
            "cf9b2a85b62bc2fd67c5ed43a1d0009df848ac8a",
        ),
        (
            &[0xff, 0xfe, 0x80, 0x41, 0x0a],
            "ebf21e5fbba60d5eddadbf1fe47670a9aeb4bff4",
        ),
        (b"a\nb\n", "422c2b7ab3b3c668038da977e4e93a5fc623169c"),
    ];
    for (bytes, expected) in fixed {
        assert_eq!(BlobSha::of(bytes).to_string(), expected, "{bytes:?}");
        assert_eq!(git_hash_object(bytes), expected, "{bytes:?}");
    }
    // Larger, mixed endings, and every byte value but NUL.
    let mut mixed: Vec<u8> = (1..=255).collect();
    mixed.extend_from_slice(b"\r\nend\nmore\r\n");
    mixed.extend(std::iter::repeat_n(b'z', 70_000));
    assert_eq!(BlobSha::of(&mixed).to_string(), git_hash_object(&mixed));

    // Through the stage: the identity of the bytes in the checkout, before secret
    // values are masked, not of what the engine was given.
    let scheme = "postgres";
    let original = format!(
        "URL = \"{scheme}://app:{}@db/app\"\r\nNAME = 'svc'\r\n",
        ["pdxsynth", "-blob-", "0001"].concat()
    );
    let checkout = Checkout::new();
    checkout.write("src/conf.py", &original);
    let probe = Probe::new(Twist::None);
    let report = run(&checkout, None, &probe);
    let FileOutcome::Extracted { blob_sha, extract } = outcome_of(&report, "src/conf.py") else {
        panic!("not extracted");
    };
    assert_eq!(blob_sha.to_string(), git_hash_object(original.as_bytes()));
    assert_ne!(
        extract.source_digest,
        SourceDigest::of(original.as_bytes()),
        "the engine was given the original bytes"
    );
}

/// Writes a Python file of exactly `size` bytes.
fn sized(checkout: &Checkout, path: &str, size: usize) {
    let mut content = b"# ".to_vec();
    content.resize(size - 1, b'a');
    content.push(b'\n');
    checkout.write(path, content);
}

#[test]
fn memory_budget_batches() {
    let sizes = [300, 500, 700, 900, 1000, 200, 400, 600, 800, 100];
    let checkout = Checkout::new();
    for (i, size) in sizes.iter().enumerate() {
        sized(&checkout, &format!("f{i:02}.py"), *size);
    }
    sized(&checkout, "big.py", 5000);
    let files = checkout.discover();
    let limits = ExtractLimits {
        requested_workers: 4,
        memory_budget_bytes: 2000,
    };

    // The plan: the file larger than the budget is skipped; the largest of the rest
    // (1000 bytes) lets two of the four workers hold one at once; each holds at most
    // 1000; batches are filled greedily in path order.
    let plan = plan_batches(&files, limits).unwrap();
    let big = files.iter().position(|f| f.path == "big.py").unwrap();
    assert_eq!(plan.memory_skipped, vec![big]);
    assert_eq!(plan.effective_workers, 2);
    assert_eq!(plan.per_worker_budget, 1000);
    assert!(plan.effective_workers as u64 * 1000 <= limits.memory_budget_bytes);
    let bytes: Vec<u64> = plan.batches.iter().map(|b| b.bytes).collect();
    assert_eq!(bytes, vec![800, 700, 900, 1000, 600, 600, 900]);
    for batch in &plan.batches {
        let sum: u64 = batch.files.iter().map(|&i| files[i].size_bytes).sum();
        assert_eq!(sum, batch.bytes);
        assert!(batch.bytes <= plan.per_worker_budget);
    }
    let planned: Vec<usize> = plan.batches.iter().flat_map(|b| b.files.clone()).collect();
    let expected: Vec<usize> = (0..files.len()).filter(|&i| i != big).collect();
    assert_eq!(planned, expected, "every other file, once, in order");

    // The stage, run on that plan: what the engine is given at once, and what the
    // stage holds at once, stay within the budget.
    let probe = Probe::new(Twist::None);
    let report = try_run(&checkout, &checkout.config(), None, &probe, limits).unwrap();
    let mut given = probe.batch_bytes.lock().unwrap().clone();
    given.sort_unstable();
    let mut planned_bytes = bytes.clone();
    planned_bytes.sort_unstable();
    assert_eq!(
        given, planned_bytes,
        "the engine is given the planned batches"
    );
    assert!(probe.peak.load(Ordering::SeqCst) <= 2000);
    assert!(report.stats.peak_source_bytes <= 2000);
    assert!(report.stats.peak_source_bytes >= 600);
    assert_eq!(report.stats.effective_workers, 2);
    assert_eq!(report.stats.threads, 2);
    assert_eq!(report.stats.per_worker_budget, 1000);
    assert_eq!(report.stats.batches, 7);

    // The skipped file degrades the stage; every other file is extracted; the order
    // is discovery's.
    assert_eq!(outcome_of(&report, "big.py"), &FileOutcome::SkippedMemory);
    assert!(report.degraded());
    assert_eq!(probe.files(), sizes.len());
    for i in 0..sizes.len() {
        extract_of(&report, &format!("f{i:02}.py"));
    }
    let order: Vec<&str> = report.files.iter().map(|f| f.path.as_str()).collect();
    let discovered: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(order, discovered);
}

#[test]
fn absurd_limits_neither_overflow_nor_allocate() {
    let checkout = Checkout::new();
    for i in 0..20 {
        sized(&checkout, &format!("m{i:02}.py"), 50 + i);
    }
    let files = checkout.discover();
    for limits in [
        ExtractLimits {
            requested_workers: usize::MAX,
            memory_budget_bytes: u64::MAX,
        },
        ExtractLimits {
            requested_workers: 1,
            memory_budget_bytes: u64::MAX,
        },
        ExtractLimits {
            requested_workers: usize::MAX,
            memory_budget_bytes: 69,
        },
    ] {
        let plan = plan_batches(&files, limits).unwrap();
        assert!(
            plan.batches
                .iter()
                .all(|b| b.bytes <= plan.per_worker_budget)
        );
        assert!(
            plan.batches
                .iter()
                .all(|b| b.files.len() <= EXTRACT_BATCH_MAX_FILES as usize)
        );
        assert!(plan.threads() <= plan.batches.len());
        let probe = Probe::new(Twist::None);
        let report = try_run(&checkout, &checkout.config(), None, &probe, limits).unwrap();
        assert_eq!(probe.files(), 20, "{limits:?}");
        assert!(!report.degraded());
    }
    for (workers, budget, why) in [(0, 1000, "no workers"), (2, 0, "a memory budget of zero")] {
        let limits = ExtractLimits {
            requested_workers: workers,
            memory_budget_bytes: budget,
        };
        match plan_batches(&files, limits) {
            Err(ExtractError::InvalidLimits(reason)) => assert_eq!(reason, why),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn secret_policy_change_invalidates_cache() {
    let checkout = Checkout::new();
    checkout.write("src/app.py", PYTHON);
    let cache = Cache::new();
    let default = checkout.config();
    // A pattern that matches nothing in this checkout still changes the policy.
    let stricter =
        PdxConfig::parse("[secrets]\npatterns = [\"*.p12\"]\n", Path::new("pdx.toml")).unwrap();
    assert_ne!(
        SecretPolicyDigest::of(&default.secrets),
        SecretPolicyDigest::of(&stricter.secrets)
    );
    let (a, b) = (
        key_for(&checkout, &default, "src/app.py", "python"),
        key_for(&checkout, &stricter, "src/app.py", "python"),
    );
    assert_ne!(a, b);
    assert_ne!(a.object_id(), b.object_id());

    let first = Probe::new(Twist::None);
    try_run(&checkout, &default, Some(&cache), &first, LIMITS).unwrap();
    assert_eq!(first.files(), 1);
    let second = Probe::new(Twist::None);
    let report = try_run(&checkout, &stricter, Some(&cache), &second, LIMITS).unwrap();
    assert_eq!(second.files(), 1, "the other policy's entry was used");
    assert_eq!(report.stats.cache_hits, 0);
    assert_eq!(cache.entries().len(), 2);
    // Each policy finds its own entry afterwards.
    for config in [&default, &stricter] {
        let probe = Probe::new(Twist::None);
        try_run(&checkout, config, Some(&cache), &probe, LIMITS).unwrap();
        assert_eq!(probe.files(), 0);
    }
}

// --- the cache's identity ------------------------------------------------------------

#[test]
fn cache_key_includes_path() {
    let checkout = Checkout::new();
    checkout.write("src/a.py", PYTHON).write("src/b.py", PYTHON);
    let cache = Cache::new();
    let config = checkout.config();
    let (a, b) = (
        key_for(&checkout, &config, "src/a.py", "python"),
        key_for(&checkout, &config, "src/b.py", "python"),
    );
    assert_eq!(a.blob_sha, b.blob_sha);
    assert_ne!(a.object_id(), b.object_id());

    let probe = Probe::new(Twist::None);
    let first = run(&checkout, Some(&cache), &probe);
    assert_eq!(
        probe.files(),
        2,
        "the same bytes at another path are another file"
    );
    assert_eq!(cache.entries().len(), 2);
    let probe = Probe::new(Twist::None);
    let second = run(&checkout, Some(&cache), &probe);
    assert_eq!(probe.files(), 0);
    for path in ["src/a.py", "src/b.py"] {
        assert_eq!(extract_of(&second, path).rel_path, path);
        assert_eq!(extract_of(&second, path), extract_of(&first, path));
    }
    assert_ne!(
        extract_of(&second, "src/a.py").surface,
        extract_of(&second, "src/b.py").surface
    );
}

#[test]
fn cache_key_includes_language() {
    let checkout = Checkout::new();
    checkout.write("views/page.tpl", "x = 1\nprint(x)\n");
    let cache = Cache::new();
    let as_language = |language: &str| {
        PdxConfig::parse(
            &format!("[languages]\nextra = {{ \".tpl\" = \"{language}\" }}\n"),
            Path::new("pdx.toml"),
        )
        .unwrap()
    };
    let (python, ruby) = (as_language("python"), as_language("ruby"));
    assert_eq!(
        LANGUAGE_MATRIX_VERSION, 2,
        "the matrix version is the same for both"
    );

    let probe = Probe::new(Twist::None);
    let first = try_run(&checkout, &python, Some(&cache), &probe, LIMITS).unwrap();
    assert_eq!(extract_of(&first, "views/page.tpl").language, "python");
    let probe = Probe::new(Twist::None);
    let second = try_run(&checkout, &ruby, Some(&cache), &probe, LIMITS).unwrap();
    assert_eq!(probe.files(), 1, "the python extraction was reused as ruby");
    assert_eq!(extract_of(&second, "views/page.tpl").language, "ruby");
    let probe = Probe::new(Twist::None);
    let third = try_run(&checkout, &python, Some(&cache), &probe, LIMITS).unwrap();
    assert_eq!(probe.files(), 0);
    assert_eq!(extract_of(&third, "views/page.tpl").language, "python");
}

#[test]
fn cache_object_id_fixed_vector() {
    // Independently computed: sha256 over the encoding `CacheKey::object_id`
    // documents, for this key.
    let blob = BlobSha::of(b"print('hi')\n");
    assert_eq!(blob.to_string(), "9f1b437537a2acdadafd3174f6f0af9c1a04f5e4");
    let key = CacheKey::new(
        SecretPolicyDigest::of(&PdxConfig::default().secrets),
        "python",
        "src/app.py",
        blob,
    );
    assert_eq!(key.engine_version, ENGINE_VERSION);
    assert_eq!(key.language_matrix_version, LANGUAGE_MATRIX_VERSION);
    assert_eq!(
        EXTRACT_CACHE_FORMAT_VERSION, 9,
        "the vector below is format 9's"
    );
    assert_eq!(
        (ENGINE_VERSION, LANGUAGE_MATRIX_VERSION),
        (14, 2),
        "the vector below is engine 14's and matrix 2's"
    );
    assert_eq!(
        SECRET_DETECTOR_VERSION, 5,
        "the vector below is detector 5's, through the policy digest"
    );
    // Over "pdx-extract-cache" 0x00, u32 BE 9, 14 and 2, the policy digest
    // 6aa1dd65…3b99 of {"detector_version":5,"patterns":["*.key","*.pem","*id_rsa*",
    // ".env*"]}, u64 BE 6 "python", u64 BE 10 "src/app.py" and the blob above. Engines
    // 3's to 13's are in cache_key_includes_engine_version. Format 8's (before issue 117)
    // was d769d391…a28b at engine 12, format 7's (before issue 114)
    // was da5d96b4…0cb3 at engine 10, format 6's (before issue 110) b8e9117f…1d31 at
    // engine 9, and format 5's (texts copied, before issue 90) 59c9f740…550a: each format
    // moved every key. Detector 4's was 88e79c01…c826 (issue 96), detector 3's
    // c1c95f71…8e28 and detector 2's 6ff3be1c…d177: each policy moved every key, and
    // the format did not move.
    let id = key.object_id().to_string();
    assert_eq!(
        id,
        "26d5d2bc63f1b10976321d38bb2cd35a127d20af10267536e45144bc6d199f3a"
    );
    let path = FsCache::at("cache").object_path(&key);
    assert_eq!(path, Path::new("cache").join("v9").join("26").join(&id));
    // Moving a byte between the two strings changes the id: their lengths are part of
    // the encoding.
    let mut shifted = key.clone();
    shifted.language_id = "pythons".into();
    shifted.rel_path = "rc/app.py".into();
    assert_ne!(shifted.object_id(), key.object_id());
}

#[test]
fn cache_key_includes_engine_version() {
    // An engine whose output can differ is another key: what engines 3 to 13 cached is
    // never found by engine 14 (issues 74 to 76, 83, 112, 111, 110, 114, 109, 107A and
    // 105).
    // Independently computed, as the vector above, with each engine version, under
    // format 9 (issue 117) and detector 5 (issue 96); detector 4's were
    // 93f612d3…fa3b, e0ad0220…9bc0, b9e43a4e…1a32, a882233e…607b, d49c2318…caad, d1ff8375…d85b,
    // adcf486f…88e1, 271b3c8a…f765, c74297c2…7e46, d58503f2…6ffa, 8341f6ac…ca7c, 88e79c01…c826; format 8's were 0446dc3c…a3d6, d84b3204…90bb, 0061bed3…7cf4,
    // 622d6caa…fb0e, 8cb1d243…5e5a, 17673fba…932a, d75ad477…735b, 7295ddd4…f6bd,
    // 50829086…865a and d769d391…a28b, format 7's were c21a4e24…a7c, 7d29db63…639f, 022aa61d…c299,
    // f935e676…e0ec, 38141cd1…7c9c, cb2ebe0d…7c4d, fa0e0d93…7bd4 and da5d96b4…0cb3, format
    // 6's 3fa196f4…50cb, 23be5c37…785d, b7bbfafb…c8b0,
    // 37947f52…7ffd, 24ad54eb…5e31, 5d5e1d9a…0b68 and b8e9117f…1d31, and format 5's
    // 088819a1…0d81, ba0eb494…1508, 582d9e04…2bd4, c44fe667…0069 and 59c9f740…550a.
    let key = CacheKey::new(
        SecretPolicyDigest::of(&PdxConfig::default().secrets),
        "python",
        "src/app.py",
        BlobSha::of(b"print('hi')\n"),
    );
    let cache = FsCache::at("cache");
    let mut ids = Vec::new();
    for (engine, expected) in [
        (
            3,
            "b963f2521b8c64b6cb54122a1adacf4bc13dcced9e5f5e5781501e3353cd89ef",
        ),
        (
            4,
            "491bbf847491f58bf477fdf5797071efa485fc449f48cf3347b639d3b590e9dc",
        ),
        (
            5,
            "536f85e77f1e29e9b9e954f13cc8f60b34f2592de8a152a4f44f13484d21b809",
        ),
        (
            6,
            "7ee801e251b1b4067973a7cad62554bd8fa883c6a44007cf13f494abfd0d54d7",
        ),
        (
            7,
            "2131036f21346af32e011de3c9374f47b9a6ce86ff421914e4f19cd6043830f6",
        ),
        (
            8,
            "810eca8bcf0dfd82d42963fa7fa9ebea975ea2c20c46ef6a8e78e3605dca938d",
        ),
        (
            9,
            "362c9e58af3c68c019254e7d3c3f7304faf33b07d6aa95cc4053cc4fd5aecb28",
        ),
        (
            10,
            "64cf65000d1d4dcfe95cd224b1ccbdb2cd1a973cadccade13c430896eabcc278",
        ),
        (
            11,
            "0b6ee8704ca57dfcc2d2d004eb8b33048befc55c8000ab468a95bcb44935717a",
        ),
        (
            12,
            "8ffbc45bcd01cb3fd4479be1fcb063902ab339f275f251624097b15fc8375e44",
        ),
        (
            13,
            "8d75bfa006a6bf844418032971dd9e411e5f9740838a702641e3b66609bb1858",
        ),
        (
            14,
            "26d5d2bc63f1b10976321d38bb2cd35a127d20af10267536e45144bc6d199f3a",
        ),
    ] {
        let mut versioned = key.clone();
        versioned.engine_version = engine;
        assert_eq!(
            versioned.object_id().to_string(),
            expected,
            "engine {engine}"
        );
        let path = cache.object_path(&versioned);
        assert!(path.starts_with(Path::new("cache").join("v9")), "{path:?}");
        ids.push(versioned.object_id());
    }
    assert_eq!(key.object_id(), ids[11], "the current key is engine 14's");
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 12, "each engine version is another key");
}

#[test]
fn engine_version_is_the_engines() {
    // The version every cache key and segment records is the one the linked engine
    // reports of itself: neither may move without the other.
    assert_eq!(ENGINE_VERSION.to_string(), Engine::version());
    // An engine change is an engine version, not a segment layout: issues 75 and 76
    // changed the facts the engine supplies to unchanged identity encodings and DDL,
    // and engine_version already enters the cache key and the build key (decisions 47
    // and 48).
    assert_eq!(SEGMENT_SCHEMA_VERSION, 3);
}

#[test]
fn cache_is_independent_of_checkout_root() {
    let one = Checkout::new();
    let two = Checkout::new();
    for checkout in [&one, &two] {
        checkout
            .write("src/app.py", PYTHON)
            .write("web/store.ts", TYPED);
    }
    assert_ne!(one.root(), two.root());
    let cache = Cache::new();
    for (path, language) in [("src/app.py", "python"), ("web/store.ts", "typescript")] {
        let file = |c: &Checkout| c.discover().into_iter().find(|f| f.path == path).unwrap();
        let (a, b) = (
            prepare_source(one.root(), &file(&one)).unwrap(),
            prepare_source(two.root(), &file(&two)).unwrap(),
        );
        assert_eq!(a.blob_sha, b.blob_sha);
        assert_eq!(a.digest, b.digest);
        let (ka, kb) = (
            key_for(&one, &one.config(), path, language),
            key_for(&two, &two.config(), path, language),
        );
        assert_eq!(ka, kb);
        assert_eq!(ka.object_id(), kb.object_id());
    }
    assert_eq!(
        SecretPolicyDigest::of(&one.config().secrets),
        SecretPolicyDigest::of(&two.config().secrets)
    );
    let probe = Probe::new(Twist::None);
    let first = run(&one, Some(&cache), &probe);
    let probe = Probe::new(Twist::None);
    let second = run(&two, Some(&cache), &probe);
    assert_eq!(
        probe.files(),
        0,
        "the other checkout's entries are this one's"
    );
    assert_eq!(first.files, second.files);
}

// --- what is cached ------------------------------------------------------------------

#[test]
fn node_budget_disables_cache() {
    if in_child() {
        // The budget is set, generously enough that nothing is truncated: the cache
        // is still neither read nor written.
        let probe = Probe::new(Twist::None);
        let (report, _) = run_in_child_dirs(&probe);
        assert!(!report.stats.cache_enabled);
        assert_eq!(probe.files(), 1, "the clean entry was used");
        assert_eq!(report.stats.cache_hits + report.stats.cache_writes, 0);
        assert!(!extract_of(&report, "src/app.py").truncated);
        return;
    }
    let checkout = Checkout::new();
    checkout.write("src/app.py", PYTHON);
    let cache = Cache::new();
    let probe = Probe::new(Twist::None);
    run(&checkout, Some(&cache), &probe);
    let before = cache.entries();
    assert_eq!(before.len(), 1, "a clean entry exists");

    rerun_in_child(
        "node_budget_disables_cache",
        &[
            (NODE_BUDGET_ENV, "1000000"),
            (ROOT_ENV, checkout.root().to_str().unwrap()),
            (CACHE_ENV, cache.cache.root().to_str().unwrap()),
        ],
    );
    assert_eq!(
        cache.entries(),
        before,
        "the budgeted run wrote to the cache"
    );
    // Without the budget, the cache is used again.
    let probe = Probe::new(Twist::None);
    let report = run(&checkout, Some(&cache), &probe);
    assert!(report.stats.cache_enabled);
    assert_eq!(probe.files(), 0);
}

#[test]
fn extraction_switches_disable_cache() {
    if in_child() {
        let probe = Probe::new(Twist::None);
        let (report, _) = run_in_child_dirs(&probe);
        assert!(!report.stats.cache_enabled);
        assert_eq!(probe.files(), 2, "a clean entry was used");
        assert_eq!(report.stats.cache_hits + report.stats.cache_writes, 0);
        return;
    }
    let checkout = Checkout::new();
    checkout
        .write("src/app.py", PYTHON)
        .write("web/store.ts", TYPED);
    let cache = Cache::new();
    let probe = Probe::new(Twist::None);
    run(&checkout, Some(&cache), &probe);
    let before = cache.entries();
    assert_eq!(before.len(), 2);
    // A quarantine list naming a file makes the engine return it empty, which a cache
    // must never keep.
    let elsewhere = tempfile::tempdir().unwrap();
    let quarantine = elsewhere.path().join("quarantine.txt");
    fs::write(&quarantine, "src/app.py\nweb/store.ts\n").unwrap();
    assert_eq!(pdx_engine::EXTRACTION_SWITCHES.len(), 6);
    for (switch, value) in [
        (NODE_BUDGET_ENV, "1000000"),
        ("PDX_ENGINE_WALK_DEFS_MAX", "100000000"),
        ("PDX_ENGINE_TS_TYPE_BUDGET", "1"),
        ("PDX_ENGINE_LSP_MAX_WALK_DEPTH", "1"),
        ("PDX_ENGINE_LSP_DISABLED", "1"),
        (
            "PDX_ENGINE_INDEX_QUARANTINE_FILE",
            quarantine.to_str().unwrap(),
        ),
    ] {
        assert!(pdx_engine::EXTRACTION_SWITCHES.contains(&switch));
        rerun_in_child(
            "extraction_switches_disable_cache",
            &[
                (switch, value),
                (ROOT_ENV, checkout.root().to_str().unwrap()),
                (CACHE_ENV, cache.cache.root().to_str().unwrap()),
            ],
        );
        assert_eq!(cache.entries(), before, "{switch} let the cache be written");
    }
    let probe = Probe::new(Twist::None);
    run(&checkout, Some(&cache), &probe);
    assert_eq!(probe.files(), 0);
}

#[test]
fn truncated_extraction_is_not_cached() {
    let checkout = Checkout::new();
    checkout.write("src/app.py", PYTHON);
    let cache = Cache::new();
    let probe = Probe::new(Twist::Truncate);
    let report = run(&checkout, Some(&cache), &probe);
    assert!(extract_of(&report, "src/app.py").truncated);
    assert!(report.degraded());
    assert_eq!(report.stats.cache_writes, 0);
    assert!(cache.entries().is_empty());
    // Untruncated, it is extracted again, and cached.
    let probe = Probe::new(Twist::None);
    let report = run(&checkout, Some(&cache), &probe);
    assert_eq!(probe.files(), 1);
    assert!(!report.degraded());
    assert_eq!(cache.entries().len(), 1);
}

#[test]
fn lost_work_extraction_is_not_cached() {
    if in_child() {
        // The engine's type budget cut to nothing: extraction loses work, the stage
        // says it is degraded, and nothing is cached (the budget is one of the
        // switches under which no cache is used at all).
        let probe = Probe::new(Twist::None);
        let (report, _) = run_in_child_dirs(&probe);
        assert!(extract_of(&report, "web/store.ts").extraction_lost > 0);
        assert!(report.degraded());
        assert_eq!(report.stats.cache_writes, 0);
        return;
    }
    let checkout = Checkout::new();
    checkout.write("web/store.ts", TYPED);
    let cache = Cache::new();
    rerun_in_child(
        "lost_work_extraction_is_not_cached",
        &[
            ("PDX_ENGINE_TS_TYPE_BUDGET", "1"),
            (ROOT_ENV, checkout.root().to_str().unwrap()),
            (CACHE_ENV, cache.cache.root().to_str().unwrap()),
        ],
    );
    assert!(
        cache.entries().is_empty(),
        "the lossy extraction was cached"
    );

    // With the cache in use, an extraction that reports lost work is returned,
    // degraded, and not stored.
    let probe = Probe::new(Twist::LoseWork);
    let lossy = run(&checkout, Some(&cache), &probe);
    assert!(lossy.stats.cache_enabled);
    assert!(extract_of(&lossy, "web/store.ts").extraction_lost > 0);
    assert!(lossy.degraded());
    assert_eq!(lossy.stats.cache_writes, 0);
    assert!(cache.entries().is_empty());

    // The loss was transient: without it, the engine runs again, the extraction is
    // clean, and that is what is cached and then reused.
    let probe = Probe::new(Twist::None);
    let clean = run(&checkout, Some(&cache), &probe);
    assert_eq!(probe.files(), 1);
    assert_eq!(extract_of(&clean, "web/store.ts").extraction_lost, 0);
    assert!(!clean.degraded());
    assert_eq!(cache.entries().len(), 1);
    let probe = Probe::new(Twist::None);
    let third = run(&checkout, Some(&cache), &probe);
    assert_eq!(probe.files(), 0);
    assert_eq!(third.files, clean.files);
}

#[test]
fn cache_rejects_unclean_entries() {
    // An entry that is truncated or lost work is never used, however it got there.
    let checkout = Checkout::new();
    checkout.write("src/app.py", PYTHON);
    let config = checkout.config();
    let key = key_for(&checkout, &config, "src/app.py", "python");
    let clean = {
        let probe = Probe::new(Twist::None);
        extract_of(&run(&checkout, None, &probe), "src/app.py").clone()
    };
    for unclean in [
        ExtractRecord {
            truncated: true,
            ..clean.record().clone()
        },
        ExtractRecord {
            extraction_lost: 1,
            ..clean.record().clone()
        },
    ] {
        let cache = Cache::new();
        plant(&cache, &key, &CacheEnvelope::encode(&key, &unclean));
        let probe = Probe::new(Twist::None);
        let report = run(&checkout, Some(&cache), &probe);
        assert_eq!(report.stats.cache_unusable, 1);
        assert_eq!(probe.files(), 1);
        assert_eq!(extract_of(&report, "src/app.py"), &clean);
        // Replaced with the clean one.
        assert_eq!(cache.cache.load(&key), Ok(Some(clean.record().clone())));
    }
}

#[test]
fn cache_rejects_wrong_source_digest() {
    let checkout = Checkout::new();
    checkout.write("src/app.py", PYTHON);
    let config = checkout.config();
    let key = key_for(&checkout, &config, "src/app.py", "python");
    let cache = Cache::new();
    let probe = Probe::new(Twist::None);
    let clean = extract_of(&run(&checkout, Some(&cache), &probe), "src/app.py").clone();

    // Entries under the right key that are not this source's extraction.
    let other_digest = ExtractRecord {
        source_digest: SourceDigest::of(b"other bytes"),
        ..clean.record().clone()
    };
    let other_len = ExtractRecord {
        source_len: clean.source_len + 1,
        ..clean.record().clone()
    };
    let other_path = ExtractRecord {
        rel_path: "src/other.py".into(),
        ..clean.record().clone()
    };
    let other_language = ExtractRecord {
        language: "ruby".into(),
        ..clean.record().clone()
    };
    for (what, wrong) in [
        ("digest", other_digest),
        ("length", other_len),
        ("path", other_path),
        ("language", other_language),
    ] {
        plant(&cache, &key, &CacheEnvelope::encode(&key, &wrong));
        let probe = Probe::new(Twist::None);
        let report = run(&checkout, Some(&cache), &probe);
        assert_eq!(report.stats.cache_unusable, 1, "{what}");
        assert_eq!(probe.files(), 1, "{what}: the wrong entry was used");
        assert_eq!(extract_of(&report, "src/app.py"), &clean, "{what}");
        assert_eq!(
            cache.cache.load(&key),
            Ok(Some(clean.record().clone())),
            "{what}"
        );
    }
}

#[test]
fn cache_formats_before_9_are_a_miss() {
    // Format 8 held Rust definitions without their impl's trait and header, and impl
    // relations without their header (issue 117); format 7 held extractions without
    // Rust's `use` scopes and calls without the scope
    // they are in (issue 114); format 6 no members Rust's macro invocations may write
    // (issue 110); format 5 held every text copied and qualified names whole, with surface 4 (issue
    // 90); format 4 a definition's one route without its declaring node (issue 54);
    // format 3 calls without their node-type path and arguments and definitions
    // without decorators, parameter types and route (issues 46 and 47); format 2 also
    // extractions without impl relations (issue 42), format 1 definitions without base
    // classes and extractions without a declared namespace. Their entries are never
    // decoded as the current format: not in the directory their format used, which is
    // never read, and not at the current path with their format written in them.
    for old_format in [1, 2, 3, 4, 5, 6, 7, 8] {
        let checkout = Checkout::new();
        checkout.write("src/app.py", PYTHON);
        let config = checkout.config();
        let key = key_for(&checkout, &config, "src/app.py", "python");
        let clean = {
            let probe = Probe::new(Twist::None);
            extract_of(&run(&checkout, None, &probe), "src/app.py").clone()
        };
        let old = postcard::to_stdvec(&CacheEnvelope {
            format_version: old_format,
            key: key.clone(),
            extract: clean.record().clone(),
        })
        .unwrap();
        let cache = Cache::new();
        let current = cache.cache.object_path(&key);
        let current_text = current.to_string_lossy().replace('\\', "/");
        assert!(current_text.contains("/v9/"), "{current_text}");
        let old_dir = cache
            .cache
            .root()
            .join(format!("v{old_format}"))
            .join(&current.file_name().unwrap().to_string_lossy()[..2]);
        fs::create_dir_all(&old_dir).unwrap();
        fs::write(old_dir.join(current.file_name().unwrap()), &old).unwrap();
        plant(&cache, &key, &old);
        assert_eq!(
            cache.cache.load(&key),
            Err(CacheDefect::WrongFormat(old_format))
        );

        let probe = Probe::new(Twist::None);
        let report = run(&checkout, Some(&cache), &probe);
        assert_eq!(probe.files(), 1, "a format-{old_format} entry was used");
        assert_eq!(report.stats.cache_hits, 0);
        assert_eq!(report.stats.cache_unusable, 1);
        assert_eq!(
            cache.cache.load(&key),
            Ok(Some(clean.record().clone())),
            "the format-{old_format} entry was not replaced"
        );
    }
}

#[test]
fn a_cache_hit_is_bound_to_the_source_being_extracted() {
    // An entry holds the record, its text as ranges of the source (issue 90): a hit is
    // bound to the bytes this run read and normalised, and reads exactly what a fresh
    // extraction reads. A file whose bytes change is another key, and never served.
    let checkout = Checkout::new();
    checkout
        .write(
            "src/g.c",
            "#define TWICE(v) twice((v) + 1)\nint g(int a) { return f(a, f(a, h(a))) + TWICE(a); }\n",
        )
        .write(
            "app/a.py",
            "class A:\n    class B:\n        def m(self):\n            return self.x.y(z)\n\n\nv = a.b.c\n",
        );
    let fresh = {
        let probe = Probe::new(Twist::None);
        run(&checkout, None, &probe)
    };
    let cache = Cache::new();
    let probe = Probe::new(Twist::None);
    let cold = run(&checkout, Some(&cache), &probe);
    assert_eq!(probe.files(), 2);
    // Every extraction of the first run is gone; the second binds the entries afresh.
    drop(cold);
    let probe = Probe::new(Twist::None);
    let warm = run(&checkout, Some(&cache), &probe);
    assert_eq!(probe.files(), 0, "not from the cache");
    assert_eq!(warm.stats.cache_hits, 2);
    for path in ["src/g.c", "app/a.py"] {
        let (hit, extracted) = (extract_of(&warm, path), extract_of(&fresh, path));
        assert_eq!(hit, extracted, "{path}");
        assert_eq!(
            hit.source(),
            &fs::read(checkout.root().join(path)).unwrap()[..]
        );
        for (call, fresh_call) in hit.calls.iter().zip(&extracted.calls) {
            assert_eq!(
                hit.text(&call.callee_text),
                extracted.text(&fresh_call.callee_text)
            );
            for (arg, fresh_arg) in call.args.iter().zip(&fresh_call.args) {
                assert_eq!(hit.text(&arg.expr), extracted.text(&fresh_arg.expr));
            }
        }
        for (usage, fresh_usage) in hit.usages.iter().zip(&extracted.usages) {
            assert_eq!(hit.text(&usage.name), extracted.text(&fresh_usage.name));
        }
        for i in 0..hit.definitions.len() {
            assert_eq!(hit.qualified_name(i), extracted.qualified_name(i));
        }
        assert!(
            hit.calls
                .iter()
                .any(|c| matches!(c.callee_text, TextRef::Source { .. })),
            "{path}: no range"
        );
    }
    // The same length, other bytes: another blob, another key, extracted again.
    let changed = fs::read_to_string(checkout.root().join("app/a.py"))
        .unwrap()
        .replace("self.x.y", "self.q.y");
    checkout.write("app/a.py", &changed);
    let probe = Probe::new(Twist::None);
    let report = run(&checkout, Some(&cache), &probe);
    assert_eq!(
        probe.files(),
        1,
        "the changed file was served from the cache"
    );
    assert_eq!(report.stats.cache_hits, 1);
    let e = extract_of(&report, "app/a.py");
    assert!(e.calls.iter().any(|c| e.text(&c.callee_text) == "self.q.y"));
}

#[test]
fn cache_rejects_text_past_its_source() {
    // An entry under this file's key, of this source's length and digest, whose ranges
    // or names do not fit it, is no extraction of it: a miss, replaced, never clamped
    // or read (issue 90).
    let checkout = Checkout::new();
    checkout.write("src/app.py", PYTHON);
    let config = checkout.config();
    let key = key_for(&checkout, &config, "src/app.py", "python");
    let clean = {
        let probe = Probe::new(Twist::None);
        extract_of(&run(&checkout, None, &probe), "src/app.py").clone()
    };
    let end = u32::try_from(clean.source_len).unwrap();
    let mut past = clean.record().clone();
    past.calls[0].callee_text = TextRef::Source {
        start: 0,
        end: end + 1,
    };
    let mut reversed = clean.record().clone();
    reversed.calls[0].callee_text = TextRef::Source { start: 2, end: 1 };
    for (what, wrong) in [("past the source", past), ("reversed", reversed)] {
        let cache = Cache::new();
        plant(&cache, &key, &CacheEnvelope::encode(&key, &wrong));
        assert_eq!(cache.cache.load(&key), Ok(Some(wrong.clone())), "{what}");
        let probe = Probe::new(Twist::None);
        let report = run(&checkout, Some(&cache), &probe);
        assert_eq!(report.stats.cache_unusable, 1, "{what}");
        assert_eq!(report.stats.cache_hits, 0, "{what}");
        assert_eq!(probe.files(), 1, "{what}: the entry was used");
        assert_eq!(extract_of(&report, "src/app.py"), &clean, "{what}");
        assert_eq!(
            cache.cache.load(&key),
            Ok(Some(clean.record().clone())),
            "{what}: not replaced"
        );
    }
}

#[test]
fn cache_entries_grow_with_the_source_not_the_nesting() {
    // An entry holds its text as ranges of the source (issue 90): twice the nesting is at
    // most twice the entry, where copies of every attribute chain's prefix, or of every
    // nested class's enclosing names, would be four times. Four copies of a construct
    // 40, 80 and 160 deep.
    type Shape = (&'static str, &'static str, fn(usize) -> String);
    let shapes: [Shape; 2] = [
        ("app/h.py", "python", |n| {
            format!("def h():\n    v = a{}\n    return f(v)\n", ".b".repeat(n))
        }),
        ("src/main/java/p/C.java", "java", |n| {
            format!(
                "package p;\n{}void m() {{ f(); }}{}\n",
                (0..n).fold(String::new(), |mut s, i| {
                    let _ = write!(s, "class C{i} {{ ");
                    s
                }),
                "}".repeat(n)
            )
        }),
    ];
    for (path, language, shape) in shapes {
        let mut sizes = Vec::new();
        for depth in [40, 80, 160] {
            let checkout = Checkout::new();
            checkout.write(path, shape(depth).repeat(4));
            let cache = Cache::new();
            let probe = Probe::new(Twist::None);
            let report = run(&checkout, Some(&cache), &probe);
            assert_eq!(report.stats.cache_writes, 1, "{path} at {depth}");
            let key = key_for(&checkout, &checkout.config(), path, language);
            sizes.push(fs::metadata(cache.cache.object_path(&key)).unwrap().len());
            // What it stores is the extraction's own record: every range still a range,
            // nothing spelled out on the way in.
            assert_eq!(
                cache.cache.load(&key),
                Ok(Some(extract_of(&report, path).record().clone())),
                "{path} at {depth}"
            );
        }
        assert!(
            sizes[1] <= 3 * sizes[0] && sizes[2] <= 3 * sizes[1],
            "{path}: entries {sizes:?}"
        );
    }
}

#[test]
fn cached_extractions_keep_impl_relations() {
    // `impl Trait for Type` relations survive the cache whole, the empty block's
    // included, which no method definition could stand in for (issue 42).
    let checkout = Checkout::new();
    checkout.write(
        "src/lib.rs",
        "pub trait Marker {}\npub trait Shape { fn area(&self) -> f64; }\npub struct Square;\nimpl Marker for Square {}\nimpl Shape for Square { fn area(&self) -> f64 { 1.0 } }\n",
    );
    let cache = Cache::new();
    let probe = Probe::new(Twist::None);
    let fresh = run(&checkout, Some(&cache), &probe);
    let probe = Probe::new(Twist::None);
    let cached = run(&checkout, Some(&cache), &probe);
    assert_eq!(probe.files(), 0, "not from the cache");
    assert_eq!(cached.files, fresh.files);
    let relations: Vec<(&str, &str, &str)> = extract_of(&cached, "src/lib.rs")
        .impl_traits
        .iter()
        .map(|t| {
            (
                t.trait_name.as_str(),
                t.struct_name.as_str(),
                t.struct_qn.as_str(),
            )
        })
        .collect();
    assert_eq!(
        relations,
        [
            ("Marker", "Square", "src.lib.Square"),
            ("Shape", "Square", "src.lib.Square")
        ]
    );
}

#[test]
fn cached_extractions_keep_site_paths_and_derivation_facts() {
    // Each call's node-type path and arguments and each definition's decorators,
    // parameter types and routes, with their positions and paths, survive the cache
    // whole (issues 46, 47 and 54).
    let checkout = Checkout::new();
    checkout.write(
        "app/main.py",
        "from fastapi import FastAPI\n\napp = FastAPI()\n\n\n@app.get(\"/users/{user_id}\")\ndef read_user(user_id: int) -> dict:\n    return {\"id\": helper(user_id)}\n\n\ndef helper(x):\n    return x\n",
    );
    let cache = Cache::new();
    let probe = Probe::new(Twist::None);
    let fresh = run(&checkout, Some(&cache), &probe);
    let probe = Probe::new(Twist::None);
    let cached = run(&checkout, Some(&cache), &probe);
    assert_eq!(probe.files(), 0, "not from the cache");
    assert_eq!(cached.files, fresh.files);
    let e = extract_of(&cached, "app/main.py");
    let helper = e
        .calls
        .iter()
        .find(|c| e.text(&c.callee_text) == "helper")
        .unwrap();
    assert_eq!(
        helper.ast_path,
        [
            "function_definition",
            "block",
            "return_statement",
            "dictionary",
            "pair",
            "call"
        ]
    );
    assert_eq!(helper.args.len(), 1);
    let read = e
        .definitions
        .iter()
        .find(|d| d.name == "read_user")
        .unwrap();
    assert_eq!(read.decorators, ["@app.get(\"/users/{user_id}\")"]);
    assert_eq!(read.signature_param_types, ["int"]);
    let route = &read.routes[..];
    assert_eq!(route.len(), 1);
    assert_eq!(
        (route[0].method.as_str(), route[0].path.as_str()),
        ("GET", "/users/{user_id}")
    );
    assert_eq!(
        route[0].ast_path,
        ["decorated_definition", "decorator", "call"]
    );
    assert!(route[0].span.is_some(), "the route keeps its position");
}

#[test]
fn cached_extractions_keep_bases_and_namespaces() {
    let checkout = Checkout::new();
    checkout
        .write(
            "src/main/java/com/acme/Derived.java",
            "package com.acme;\npublic class Derived extends Base implements Runnable {\n  public void run() {}\n}\n",
        )
        .write("pkg/shapes.py", "class Derived(Zeta, Alpha):\n    pass\n");
    let cache = Cache::new();
    let probe = Probe::new(Twist::None);
    let fresh = run(&checkout, Some(&cache), &probe);
    let probe = Probe::new(Twist::None);
    let cached = run(&checkout, Some(&cache), &probe);
    assert_eq!(probe.files(), 0, "not from the cache");
    assert_eq!(cached.files, fresh.files);
    let java = extract_of(&cached, "src/main/java/com/acme/Derived.java");
    assert_eq!(java.declared_namespace.as_deref(), Some("com.acme"));
    let derived = java
        .definitions
        .iter()
        .find(|d| d.name == "Derived")
        .unwrap();
    assert_eq!(derived.base_classes[0], "Base");
    let python = extract_of(&cached, "pkg/shapes.py");
    let derived = python
        .definitions
        .iter()
        .find(|d| d.name == "Derived")
        .unwrap();
    assert_eq!(derived.base_classes, ["Zeta", "Alpha"]);
}

#[test]
fn cache_rejects_corruption() {
    let checkout = Checkout::new();
    checkout.write("src/app.py", PYTHON);
    let config = checkout.config();
    let key = key_for(&checkout, &config, "src/app.py", "python");
    let cache = Cache::new();
    let probe = Probe::new(Twist::None);
    let clean = extract_of(&run(&checkout, Some(&cache), &probe), "src/app.py").clone();
    let good = CacheEnvelope::encode(&key, clean.record());
    let other_key = CacheKey {
        rel_path: "src/other.py".into(),
        ..key.clone()
    };
    let wrong_version = postcard::to_stdvec(&CacheEnvelope {
        format_version: EXTRACT_CACHE_FORMAT_VERSION + 1,
        key: key.clone(),
        extract: clean.record().clone(),
    })
    .unwrap();

    let cases: Vec<(&str, Vec<u8>, CacheDefect)> = vec![
        ("empty", Vec::new(), CacheDefect::Malformed),
        (
            "cut short",
            good[..good.len() / 2].to_vec(),
            CacheDefect::Malformed,
        ),
        (
            "trailing bytes",
            [good.as_slice(), &[0, 1, 2]].concat(),
            CacheDefect::TrailingBytes(3),
        ),
        (
            "another format",
            wrong_version,
            CacheDefect::WrongFormat(EXTRACT_CACHE_FORMAT_VERSION + 1),
        ),
        (
            "another key",
            CacheEnvelope::encode(&other_key, &clean),
            CacheDefect::WrongKey,
        ),
        (
            "not postcard",
            [
                u8::try_from(EXTRACT_CACHE_FORMAT_VERSION).unwrap(),
                0xff,
                0xff,
                0xff,
                0xff,
                0xff,
                0xff,
                0xff,
                0xff,
                0xff,
            ]
            .to_vec(),
            CacheDefect::Malformed,
        ),
    ];
    for (what, bytes, defect) in cases {
        plant(&cache, &key, &bytes);
        assert_eq!(cache.cache.load(&key), Err(defect), "{what}");
        let probe = Probe::new(Twist::None);
        let report = run(&checkout, Some(&cache), &probe);
        assert_eq!(report.stats.cache_unusable, 1, "{what}");
        assert_eq!(probe.files(), 1, "{what}");
        assert_eq!(extract_of(&report, "src/app.py"), &clean, "{what}");
        assert_eq!(
            cache.cache.load(&key),
            Ok(Some(clean.record().clone())),
            "{what}: not replaced"
        );
    }
}

#[test]
fn cache_writes_are_atomic_and_private() {
    let checkout = Checkout::new();
    checkout.write("src/app.py", PYTHON);
    let config = checkout.config();
    let key = key_for(&checkout, &config, "src/app.py", "python");
    let probe = Probe::new(Twist::None);
    let clean = extract_of(&run(&checkout, None, &probe), "src/app.py").clone();
    let cache = Cache::new();

    // Two writes of the same key leave one whole entry and nothing else.
    cache.cache.store(&key, clean.record()).unwrap();
    let renamed = ExtractRecord {
        status: FileStatus::Partial,
        ..clean.record().clone()
    };
    cache.cache.store(&key, &renamed).unwrap();
    let entries = cache.entries();
    assert_eq!(entries.len(), 1, "{:?}", entries.keys());
    assert_eq!(cache.cache.load(&key), Ok(Some(renamed)));

    // A write cut off before its rename leaves a temporary file, which is never read
    // as the entry.
    let object = cache.cache.object_path(&key);
    let dir = object.parent().unwrap();
    fs::write(
        dir.join(".tmp-interrupted"),
        &CacheEnvelope::encode(&key, clean.record())[..10],
    )
    .unwrap();
    fs::remove_file(&object).unwrap();
    assert_eq!(cache.cache.load(&key), Ok(None));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        cache.cache.store(&key, &clean).unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&object), 0o600);
        let mut d = dir;
        loop {
            assert_eq!(mode(d), 0o700, "{}", d.display());
            if d == cache.cache.root() {
                break;
            }
            d = d.parent().unwrap();
        }
    }
}

#[test]
fn a_failed_parse_is_cached_like_any_clean_extraction() {
    // A parse that failed is the engine's deterministic answer for these bytes, not a
    // failure of the run: it is cached when it is otherwise clean.
    let checkout = Checkout::new();
    checkout.write("src/app.py", PYTHON);
    let cache = Cache::new();
    let probe = Probe::new(Twist::StatusFailed);
    let report = run(&checkout, Some(&cache), &probe);
    assert_eq!(extract_of(&report, "src/app.py").status, FileStatus::Failed);
    assert!(!report.degraded());
    assert_eq!(report.stats.cache_writes, 1);
    let probe = Probe::new(Twist::None);
    let again = run(&checkout, Some(&cache), &probe);
    assert_eq!(probe.files(), 0);
    assert_eq!(extract_of(&again, "src/app.py").status, FileStatus::Failed);
}

// --- failures --------------------------------------------------------------------------

#[test]
fn engine_errors_cost_one_file_and_are_not_cached() {
    let checkout = Checkout::new();
    checkout
        .write("a.py", PYTHON)
        .write("b.py", PYTHON)
        .write("c.py", PYTHON);
    let cache = Cache::new();
    let probe = Probe::new(Twist::FailOn("b.py"));
    let report = run(&checkout, Some(&cache), &probe);
    assert!(matches!(
        outcome_of(&report, "b.py"),
        FileOutcome::EngineFailed {
            error: EngineError::Internal,
            ..
        }
    ));
    extract_of(&report, "a.py");
    extract_of(&report, "c.py");
    assert!(
        !report.degraded(),
        "a refused file is counted, not degraded"
    );
    assert_eq!(report.stats.cache_writes, 2);
    let probe = Probe::new(Twist::None);
    run(&checkout, Some(&cache), &probe);
    assert_eq!(probe.files(), 1, "only the failed file is extracted again");
}

#[test]
fn engine_crashes_degrade_and_are_not_cached() {
    let checkout = Checkout::new();
    checkout.write("a.py", PYTHON).write("b.py", PYTHON);
    let cache = Cache::new();
    let probe = Probe::new(Twist::CrashOn("a.py"));
    let report = run(&checkout, Some(&cache), &probe);
    let FileOutcome::EngineCrashed { blob_sha } = outcome_of(&report, "a.py") else {
        panic!("not a crash");
    };
    assert_eq!(*blob_sha, BlobSha::of(PYTHON));
    extract_of(&report, "b.py");
    assert!(report.degraded());
    assert_eq!(report.stats.cache_writes, 1);
}

#[test]
fn an_isolated_timeout_fails_the_stage() {
    let checkout = Checkout::new();
    checkout
        .write("a.py", PYTHON)
        .write("b.py", PYTHON)
        .write("c.py", PYTHON);
    let cache = Cache::new();
    let probe = Probe::new(Twist::TimeoutOn("b.py"));
    let limits = ExtractLimits {
        requested_workers: 1,
        memory_budget_bytes: 64 << 20,
    };
    let err = try_run(&checkout, &checkout.config(), Some(&cache), &probe, limits).unwrap_err();
    assert!(
        matches!(err, ExtractError::Isolation(IsolationError::Timeout { .. })),
        "{err:?}"
    );
    // Nothing of the batch that timed out was stored, and no outcome came back.
    assert!(cache.entries().is_empty());
}

#[test]
fn files_without_an_extraction_are_carried_forward() {
    let checkout = Checkout::new();
    checkout
        .write("pdx.toml", "[discover]\nmax_file_bytes = 200\n")
        .write("src/app.py", "x = 1\n")
        .write("assets/logo.py", b"\x00\x01binary")
        .write("src/huge.py", vec![b'#'; 300])
        .write("certs/server.pem", "not read")
        .write(".env", "never read")
        .write("notes.unknownext", "plain text");
    let files = checkout.discover();
    // Everything but the file to extract disappears after discovery: the stage
    // succeeding proves it opened none of them.
    for gone in [
        "assets/logo.py",
        "src/huge.py",
        "certs/server.pem",
        ".env",
        "notes.unknownext",
    ] {
        fs::remove_file(checkout.root().join(gone)).unwrap();
    }
    let cache = Cache::new();
    let probe = Probe::new(Twist::None);
    let report = run_files(
        checkout.root(),
        &files,
        &checkout.config(),
        Some(&cache),
        &probe,
        LIMITS,
    )
    .unwrap();
    assert_eq!(outcome_of(&report, "assets/logo.py"), &FileOutcome::Binary);
    assert_eq!(
        outcome_of(&report, "src/huge.py"),
        &FileOutcome::SkippedSize
    );
    assert_eq!(
        outcome_of(&report, "certs/server.pem"),
        &FileOutcome::Redacted
    );
    assert_eq!(outcome_of(&report, ".env"), &FileOutcome::Redacted);
    assert_eq!(
        outcome_of(&report, "notes.unknownext"),
        &FileOutcome::UnknownLanguage
    );
    // The two files that are extracted: the source, and the configuration, which is
    // a TOML file of the repository like any other.
    let extracted = ["pdx.toml", "src/app.py"];
    for path in extracted {
        extract_of(&report, path);
    }
    assert_eq!(probe.files(), 2);
    assert_eq!(cache.entries().len(), 2);
    assert!(!report.degraded());
    for f in &report.files {
        let read = extracted.contains(&f.path.as_str());
        assert_eq!(f.outcome.blob_sha().is_some(), read, "{}", f.path);
    }
}

/// A change made to a checkout after discovery.
type Change = Box<dyn Fn(&Path)>;

#[test]
fn only_candidates_are_ever_prepared() {
    let checkout = Checkout::new();
    checkout
        .write("src/app.py", PYTHON)
        .write("certs/server.pem", "never opened")
        .write("assets/logo.py", b"\x00binary")
        .write("notes.unknownext", "text");
    for file in checkout.discover() {
        let prepared = prepare_source(checkout.root(), &file);
        if file.path == "src/app.py" {
            assert_eq!(prepared.unwrap().blob_sha, BlobSha::of(PYTHON));
        } else {
            assert!(
                matches!(&prepared, Err(ExtractError::NotExtracted(p)) if *p == file.path),
                "{}: {prepared:?}",
                file.path
            );
        }
    }
}

#[test]
fn a_changed_checkout_is_an_error() {
    let secret = ["pdxsynth", "-changed-", "0001"].concat();
    let content = format!("token = \"{secret}\"\n");
    let cases: Vec<(&str, Change, SourceChange)> = vec![
        (
            "deleted",
            Box::new(|p: &Path| fs::remove_file(p).unwrap()),
            SourceChange::Missing,
        ),
        (
            "grown",
            Box::new(|p: &Path| {
                let mut bytes = fs::read(p).unwrap();
                bytes.push(b'\n');
                fs::write(p, bytes).unwrap();
            }),
            SourceChange::Size {
                discovered: content.len() as u64,
                now: content.len() as u64 + 1,
            },
        ),
        (
            "a directory",
            Box::new(|p: &Path| {
                fs::remove_file(p).unwrap();
                fs::create_dir(p).unwrap();
            }),
            SourceChange::NotAFile,
        ),
        (
            "binary",
            Box::new(|p: &Path| {
                let mut bytes = fs::read(p).unwrap();
                bytes[0] = 0;
                fs::write(p, bytes).unwrap();
            }),
            SourceChange::Binary,
        ),
    ];
    for (what, change, expected) in cases {
        let checkout = Checkout::new();
        checkout.write("src/app.py", &content);
        let files = checkout.discover();
        change(&checkout.root().join("src/app.py"));
        let probe = Probe::new(Twist::None);
        let err = run_files(
            checkout.root(),
            &files,
            &checkout.config(),
            None,
            &probe,
            LIMITS,
        )
        .unwrap_err();
        match &err {
            ExtractError::CheckoutChanged { path, change } => {
                assert_eq!(path, "src/app.py", "{what}");
                assert_eq!(*change, expected, "{what}");
            }
            other => panic!("{what}: {other:?}"),
        }
        // The error names the file, never its content.
        assert!(!format!("{err} {err:?}").contains(&secret), "{what}");
        assert_eq!(probe.files(), 0, "{what}");
    }
}

#[cfg(unix)]
#[test]
fn a_symlink_placed_since_discovery_is_not_followed() {
    let outside = Checkout::new();
    outside.write("src/app.py", PYTHON);
    for link in ["src/app.py", "src"] {
        let checkout = Checkout::new();
        checkout.write("src/app.py", PYTHON);
        let files = checkout.discover();
        let path = checkout.root().join(link);
        if path.is_dir() {
            fs::remove_dir_all(&path).unwrap();
        } else {
            fs::remove_file(&path).unwrap();
        }
        std::os::unix::fs::symlink(outside.root().join(link), &path).unwrap();
        let probe = Probe::new(Twist::None);
        let err = run_files(
            checkout.root(),
            &files,
            &checkout.config(),
            None,
            &probe,
            LIMITS,
        )
        .unwrap_err();
        assert!(
            matches!(
                err,
                ExtractError::CheckoutChanged {
                    change: SourceChange::Symlink,
                    ..
                }
            ),
            "{link}: {err:?}"
        );
        assert_eq!(probe.files(), 0);
    }
}

// --- workers -------------------------------------------------------------------------

/// The engine's smoke fixtures and a few dozen generated files.
fn many_files() -> Checkout {
    let checkout = Checkout::new();
    let smoke = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../engine/tests/fixtures/smoke");
    for entry in fs::read_dir(smoke).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        checkout.write(&format!("smoke/{name}"), fs::read(&path).unwrap());
    }
    for i in 0..30 {
        checkout.write(
            &format!("gen/m{i:02}.py"),
            format!("def f{i}(x):\n    return g{i}(x) + {i}\n"),
        );
    }
    checkout
}

#[test]
fn parallel_extraction_is_order_deterministic() {
    let checkout = many_files();
    let files = checkout.discover();
    let largest = files.iter().map(|f| f.size_bytes).max().unwrap();
    let first = files[0].path.clone();
    let serial = {
        let probe = Probe::new(Twist::None);
        let limits = ExtractLimits {
            requested_workers: 1,
            memory_budget_bytes: 64 << 20,
        };
        try_run(&checkout, &checkout.config(), None, &probe, limits).unwrap()
    };
    assert!(
        serial
            .files
            .iter()
            .all(|f| matches!(f.outcome, FileOutcome::Extracted { .. }))
    );

    for (workers, budget) in [
        (2, 64 << 20),
        (7, 64 << 20),
        (3, largest * 3),
        (5, largest * 5),
    ] {
        // The batch holding the first file is slowed, so later batches finish first.
        let probe = Probe::new(Twist::SlowOn(Box::leak(first.clone().into_boxed_str())));
        let limits = ExtractLimits {
            requested_workers: workers,
            memory_budget_bytes: budget,
        };
        let report = try_run(&checkout, &checkout.config(), None, &probe, limits).unwrap();
        assert_eq!(
            report.files, serial.files,
            "{workers} workers, budget {budget}"
        );
        if report.stats.threads > 1 {
            let finished = probe.finished.lock().unwrap().clone();
            assert_ne!(
                finished.first(),
                Some(&first),
                "the batches finished in order"
            );
        }
    }

    // From the cache, and into it, the same again.
    let cache = Cache::new();
    for _ in 0..2 {
        let probe = Probe::new(Twist::None);
        let report = run(&checkout, Some(&cache), &probe);
        assert_eq!(report.files, serial.files);
    }
}

// --- secrets -------------------------------------------------------------------------

/// A checkout with synthetic secrets planted in source, YAML, JSON, connection URIs,
/// private-key blocks, a bearer header and a redacted `.env`, and the planted values.
fn secret_checkout() -> (Checkout, Vec<String>) {
    let tag = |n: u32| format!("{}{n:04}", ["pdx", "synth", "leak"].concat());
    let token = format!("{}Bearer{}", tag(1), "Token");
    let access_id = format!("{}{}", "AKIA", "PDXSYNTHLEAK0001");
    let body = [
        "UERYU1lOVEhMRUFLS0VZTUFURVJJQUwwMDAxUERYU1lOVEhMRUFLS0VZMDAx",
        "UERYU1lOVEhMRUFL",
    ];
    let label = PRIVATE_KEY_LABELS[1];
    let (scheme_a, scheme_b) = ("postgres", "mysql");
    let source = format!(
        "DB_PASSWORD = \"{}\"\nHEADERS = {{\"Authorization\": \"Bearer {token}\"}}\nURL = \"{scheme_a}://app:{}@db/app\"\nAWS_ID = \"{access_id}\"\nKEY = \"-----BEGIN {label}-----\\n{}\\n{}\\n-----END {label}-----\\n\"\n\n\ndef connect():\n    return URL\n",
        tag(2),
        tag(3),
        body[0],
        body[1],
    );
    let yaml = format!(
        "database:\r\n  password: {}\r\n  url: {scheme_b}://svc:{}@db:3306/x\r\n  user: app\r\ntls:\r\n  key: |\r\n    -----BEGIN {label}-----\r\n    {}\r\n    {}\r\n    -----END {label}-----\r\n",
        tag(4),
        tag(5),
        body[0],
        body[1],
    );
    let json = format!(
        "{{\"apiKey\": \"{}\", \"clientSecret\": \"{}\", \"name\": \"svc\"}}\n",
        tag(6),
        tag(7)
    );
    let checkout = Checkout::new();
    checkout
        .write("src/settings.py", &source)
        .write("config/app.yaml", &yaml)
        .write("config/app.json", &json)
        .write(".env", format!("TOKEN={}\n", tag(8)));
    let mut planted: Vec<String> = (2..=8).map(tag).collect();
    planted.extend([token, access_id, body[0].to_owned(), body[1].to_owned()]);
    (checkout, planted)
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[test]
fn secrets_do_not_reach_engine_or_cache() {
    let (checkout, planted) = secret_checkout();
    let files = checkout.discover();
    // The redacted file is gone before the stage runs: it is never opened.
    fs::remove_file(checkout.root().join(".env")).unwrap();
    let cache = Cache::new();
    let probe = Probe::new(Twist::None);
    let report = run_files(
        checkout.root(),
        &files,
        &checkout.config(),
        Some(&cache),
        &probe,
        LIMITS,
    )
    .unwrap();
    assert_eq!(outcome_of(&report, ".env"), &FileOutcome::Redacted);

    // What the engine was given: every planted value masked, nothing moved, the key
    // blocks' markers kept.
    let sources = probe.sources.lock().unwrap().clone();
    assert_eq!(sources.len(), 3);
    assert!(sources.iter().all(|s| s.rel_path != ".env"));
    for given in &sources {
        let original = fs::read(checkout.root().join(&given.rel_path)).unwrap();
        assert_eq!(given.source.len(), original.len(), "{}", given.rel_path);
        for (i, (a, b)) in original.iter().zip(&given.source).enumerate() {
            if matches!(a, b'\r' | b'\n') {
                assert_eq!(a, b, "{}: a line ending moved at {i}", given.rel_path);
            }
        }
        for secret in &planted {
            assert!(
                !contains(&given.source, secret),
                "{secret} reached the engine"
            );
        }
        if given.rel_path != "config/app.json" {
            assert!(contains(&given.source, "-----BEGIN"), "{}", given.rel_path);
        }
    }
    // What the engine returned: no field, no surface.
    for f in &report.files {
        let FileOutcome::Extracted { extract, .. } = &f.outcome else {
            continue;
        };
        // The record holds the text the source holds as ranges of it; the source it is
        // bound to is the normalised one (issue 90).
        let fields = serde_json::to_vec(extract.record()).unwrap();
        for secret in &planted {
            assert!(
                !contains(extract.source(), secret),
                "{secret} in {}'s bound source",
                f.path
            );
        }
        for secret in &planted {
            assert!(
                !contains(&fields, secret),
                "{secret} in {}'s extraction",
                f.path
            );
            assert!(
                !contains(extract.surface.as_bytes(), secret),
                "{secret} in {}'s surface",
                f.path
            );
        }
    }
    // What the cache holds: one entry per extracted file, none of them secret.
    let entries = cache.entries();
    assert_eq!(entries.len(), 3);
    for bytes in entries.values() {
        for secret in &planted {
            assert!(!contains(bytes, secret), "{secret} in the cache");
        }
    }
}

#[test]
fn checkout_root_is_never_part_of_an_entry() {
    // The entries of a cache filled from one checkout are byte-identical to those of
    // another checkout of the same files: no absolute path is in them.
    let entries = |checkout: &Checkout| {
        let cache = Cache::new();
        let probe = Probe::new(Twist::None);
        run(checkout, Some(&cache), &probe);
        let all = cache.entries();
        for bytes in all.values() {
            let root = checkout.root().to_string_lossy();
            assert!(!String::from_utf8_lossy(bytes).contains(root.as_ref()));
        }
        all
    };
    let (one, two) = (Checkout::new(), Checkout::new());
    for checkout in [&one, &two] {
        checkout
            .write("src/app.py", PYTHON)
            .write("web/store.ts", TYPED);
    }
    assert_eq!(entries(&one), entries(&two));
}

// --- C-family input isolation (issue 83) ---------------------------------------------
//
// The engine's C and C++ preprocessing used to load every file a source #included,
// relative to the process's working directory: an extraction then held what files
// Stage 2 never read (so never masked), possibly from outside the repository, that no
// cache key names. Since engine 7 the normalised buffer is the whole input (decision
// 51). Each test runs the stage in child processes whose working directories differ,
// as `pdx index` run from different places would.

const OUT_ENV: &str = "PDX_CORE_TEST_OUT";

/// What a child's stage run extracted: every extraction as JSON by path, and the
/// cache hits.
#[derive(serde::Serialize, serde::Deserialize)]
struct ChildRun {
    extracts: BTreeMap<String, String>,
    cache_hits: u64,
}

/// The child half: the stage over the parent's checkout, with the parent's cache when it
/// names one, written where the parent says.
fn child_run() {
    let root = PathBuf::from(std::env::var_os(ROOT_ENV).expect("the parent names the checkout"));
    let config = PdxConfig::load(&root).unwrap();
    let files = discover(&root, &config).unwrap();
    let cache = std::env::var_os(CACHE_ENV).map(FsCache::at);
    let probe = Probe::new(Twist::None);
    let factory = backend(&probe);
    let mut stage = ExtractStage::new(&root, &config.secrets, LIMITS).with_backend(&factory);
    if let Some(cache) = &cache {
        stage = stage.with_cache(cache);
    }
    let report = stage.run(&files).expect("the stage runs");
    let extracts = report
        .files
        .iter()
        .filter_map(|f| match &f.outcome {
            FileOutcome::Extracted { extract, .. } => {
                // The record, and each call's callee read from the bound source
                // (issue 90): the record holds the text as ranges.
                let view = serde_json::json!({
                    "record": extract.record(),
                    "callees": extract
                        .calls
                        .iter()
                        .map(|c| extract.text(&c.callee_text))
                        .collect::<Vec<_>>(),
                });
                Some((f.path.clone(), view.to_string()))
            }
            _ => None,
        })
        .collect();
    let out = ChildRun {
        extracts,
        cache_hits: report.stats.cache_hits,
    };
    let path = std::env::var_os(OUT_ENV).expect("the parent names the output");
    fs::write(path, serde_json::to_vec(&out).unwrap()).unwrap();
}

/// Runs `test`'s child half with its working directory at `cwd`, over the checkout at
/// `root`, with `cache` when there is one.
fn run_at(test: &str, cwd: &Path, root: &Path, cache: Option<&Path>) -> ChildRun {
    run_at_with(test, cwd, root, cache, &[])
}

/// [`run_at`], with `extra` set in the child's environment too.
fn run_at_with(
    test: &str,
    cwd: &Path,
    root: &Path,
    cache: Option<&Path>,
    extra: &[(&str, &str)],
) -> ChildRun {
    let out = tempfile::NamedTempFile::new().unwrap();
    let mut env = vec![
        (ROOT_ENV, root.to_str().unwrap()),
        (OUT_ENV, out.path().to_str().unwrap()),
    ];
    if let Some(cache) = cache {
        env.push((CACHE_ENV, cache.to_str().unwrap()));
    }
    env.extend_from_slice(extra);
    assert!(rerun_in_child_at(test, Some(cwd), &env));
    serde_json::from_slice(&fs::read(out.path()).unwrap()).unwrap()
}

/// A directory of temporary files, written by relative path.
fn files_in(dir: &Path, files: &[(&str, &str)]) {
    for (relative, content) in files {
        let path = dir.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
}

/// The `field` of every `list` entry of a child's extraction record of `path`: its
/// imports' modules.
fn names_in(run: &ChildRun, path: &str, list: &str, field: &str) -> Vec<String> {
    let extract: serde_json::Value = serde_json::from_str(&run.extracts[path]).unwrap();
    extract["record"][list]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry[field].as_str().unwrap().to_owned())
        .collect()
}

/// A child's extraction's callees, read from its source.
fn callees(run: &ChildRun, path: &str) -> Vec<String> {
    let extract: serde_json::Value = serde_json::from_str(&run.extracts[path]).unwrap();
    extract["callees"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn c_family_extraction_is_independent_of_the_working_directory() {
    const TEST: &str = "c_family_extraction_is_independent_of_the_working_directory";
    if in_child() {
        return child_run();
    }
    let (base, nowhere) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (repo, elsewhere, empty) = (
        base.path().join("repo"),
        base.path().join("elsewhere"),
        nowhere.path().to_path_buf(),
    );
    files_in(
        &repo,
        &[
            (
                "src/app.c",
                "#include \"app.h\"\n#include \"../include/shared.h\"\n#include \"../../outside/external.h\"\n#define LOCAL_CALL(x) local_target(x)\nint run(void) { return HEADER_CALL(1) + SHARED_CALL(2) + EXTERNAL_CALL(3) + LOCAL_CALL(4); }\n",
            ),
            (
                "src/app.cpp",
                "#if __has_include(\"app.h\")\n#define PICK() has_header_branch()\n#else\n#define PICK() no_header_branch()\n#endif\n#include \"app.h\"\nint run2() { return PICK() + HEADER_CALL(5); }\n",
            ),
            (
                "src/app.h",
                "#define HEADER_CALL(x) header_target_repo(x)\nint header_defined_fn(void) { return header_inner_call(); }\n",
            ),
            (
                "include/shared.h",
                "#define SHARED_CALL(x) shared_target_repo(x)\n",
            ),
        ],
    );
    files_in(
        base.path(),
        &[(
            "outside/external.h",
            "#define EXTERNAL_CALL(x) outside_target(x)\n",
        )],
    );
    files_in(
        &elsewhere,
        &[
            (
                "src/app.h",
                "#define HEADER_CALL(x) header_target_elsewhere(x)\n",
            ),
            (
                "include/shared.h",
                "#define SHARED_CALL(x) shared_target_elsewhere(x)\n",
            ),
        ],
    );

    let at_root = run_at(TEST, &repo, &repo, None);
    let at_elsewhere = run_at(TEST, &elsewhere, &repo, None);
    let at_empty = run_at(TEST, &empty, &repo, None);
    assert_eq!(at_root.extracts.len(), 4);
    for (path, extract) in &at_empty.extracts {
        assert_eq!(&at_root.extracts[path], extract, "{path}: from the root");
        assert_eq!(
            &at_elsewhere.extracts[path], extract,
            "{path}: from elsewhere"
        );
    }
    // Nothing of a header reaches a source that includes it, and the source's own macros
    // and its include directives are kept.
    for path in ["src/app.c", "src/app.cpp"] {
        for name in [
            "header_target",
            "shared_target",
            "outside_target",
            "header_inner_call",
            "has_header_branch",
        ] {
            assert!(
                !callees(&at_root, path).iter().any(|c| c.contains(name)),
                "{path}: a header's {name}"
            );
        }
    }
    assert!(callees(&at_root, "src/app.c").contains(&"local_target".to_owned()));
    assert!(callees(&at_root, "src/app.cpp").contains(&"no_header_branch".to_owned()));
    assert_eq!(
        names_in(&at_root, "src/app.c", "imports", "module_text"),
        ["app.h", "../include/shared.h", "../../outside/external.h"]
    );
}

#[test]
fn ambient_header_change_cannot_change_a_cached_extraction() {
    // A header is not an extraction input: changing it leaves the including file's key
    // unchanged, so a cache hit and a fresh extraction must be the same.
    const TEST: &str = "ambient_header_change_cannot_change_a_cached_extraction";
    if in_child() {
        return child_run();
    }
    let checkout = Checkout::new();
    checkout
        .write(
            "src/app.c",
            "#include \"app.h\"\n#define LOCAL 1\nint run(void) { return HEADER_CALL(1); }\n",
        )
        .write(
            "src/app.h",
            "#define HEADER_CALL(x) header_target_before(x)\n",
        );
    let cache = Cache::new();
    let cache_dir = cache.dir.path().join("extract");
    let root = checkout.root();

    let cold = run_at(TEST, root, root, Some(&cache_dir));
    assert_eq!(cold.cache_hits, 0);
    checkout.write(
        "src/app.h",
        "#define HEADER_CALL(x) header_target_after_change(x)\n",
    );
    let warm = run_at(TEST, root, root, Some(&cache_dir));
    assert_eq!(
        warm.cache_hits, 1,
        "src/app.c is a hit, the changed header is not"
    );
    let fresh = run_at(TEST, root, root, None);
    assert_eq!(warm.extracts["src/app.c"], fresh.extracts["src/app.c"]);
    assert_eq!(cold.extracts["src/app.c"], fresh.extracts["src/app.c"]);
    assert_ne!(cold.extracts["src/app.h"], fresh.extracts["src/app.h"]);
}

#[test]
fn cache_entries_of_engine_six_are_never_served() {
    // Engine 6 loaded the files a C or C++ source included, so what it cached may hold a
    // header's content. Engine 7 reads none, and the version in the key keeps every
    // engine 6 entry from being found (issue 83).
    let checkout = Checkout::new();
    checkout.write(
        "src/app.c",
        "#define LOCAL_CALL(x) local_target(x)\nint run(void) { return LOCAL_CALL(1); }\n",
    );
    let config = checkout.config();
    let fresh = run(&checkout, None, &Probe::new(Twist::None));
    let mut stale = extract_of(&fresh, "src/app.c").record().clone();
    stale.calls[0].callee_text = TextRef::Owned("ambient_header_target".into());
    let mut key = key_for(&checkout, &config, "src/app.c", "c");
    key.engine_version = 6;
    let cache = Cache::new();
    plant(&cache, &key, &CacheEnvelope::encode(&key, &stale));

    let probe = Probe::new(Twist::None);
    let report = run(&checkout, Some(&cache), &probe);
    assert_eq!(report.stats.cache_hits, 0, "an engine 6 entry was served");
    assert_eq!(probe.files(), 1);
    assert_eq!(
        extract_of(&report, "src/app.c"),
        extract_of(&fresh, "src/app.c")
    );
}

#[test]
fn cache_entries_of_engine_seven_are_never_served() {
    // Engine 7 extracted a bare Rust call of a function its block declares as a call
    // bound to nothing; since patch 0028 (issue 104) engine 8 marks it locally bound,
    // so resolution by name never reaches a namesake elsewhere. Both are the same
    // source, path, language and policy: only the version in the key keeps what engine
    // 7 cached from being served (issue 112). Cold, the engine 7 entry is a miss and
    // the file is extracted again, as engine 8 does; the old entry is left as it was,
    // an entry of the current engine is written beside it, and that is what a warm run
    // serves.
    const PATH: &str = "src/strip.rs";
    let checkout = Checkout::new();
    checkout.write(
        PATH,
        "pub fn capture(segment: &str) -> usize {\n    fn find_first_not_double(needle: u8) -> usize {\n        needle as usize\n    }\n\n    find_first_not_double(b'{') + segment.len()\n}\n",
    );
    let config = checkout.config();
    let local_call = |extract: &FileExtract| -> usize {
        let found: Vec<usize> = (0..extract.record().calls.len())
            .filter(|&i| {
                extract.text(&extract.record().calls[i].callee_text) == "find_first_not_double"
            })
            .collect();
        assert_eq!(found.len(), 1, "one call of the block's function");
        found[0]
    };
    let bound = |extract: &FileExtract| -> bool {
        extract.record().calls[local_call(extract)]
            .lexical
            .contains(LexicalFacts::LOCALLY_BOUND)
    };
    let fresh = run(&checkout, None, &Probe::new(Twist::None));
    let fresh = extract_of(&fresh, PATH);
    assert!(
        bound(fresh),
        "the current engine marks the call locally bound"
    );

    // What engine 7 cached for these bytes: the same extraction, the call bound to
    // nothing.
    let mut stale = fresh.record().clone();
    let call = &mut stale.calls[local_call(fresh)];
    call.lexical =
        LexicalFacts::from_bits(call.lexical.bits() & !LexicalFacts::LOCALLY_BOUND.bits());
    let mut key = key_for(&checkout, &config, PATH, "rust");
    key.engine_version = 7;
    let cache = Cache::new();
    let stale_bytes = CacheEnvelope::encode(&key, &stale);
    plant(&cache, &key, &stale_bytes);
    let stale_path = cache.cache.object_path(&key);

    let probe = Probe::new(Twist::None);
    let cold = run(&checkout, Some(&cache), &probe);
    assert_eq!(cold.stats.cache_hits, 0, "an engine 7 entry was served");
    assert_eq!(probe.files(), 1, "the file was not extracted again");
    let cold = extract_of(&cold, PATH);
    assert!(bound(cold), "the call is not locally bound");
    assert_eq!(cold, fresh);
    assert_eq!(
        fs::read(&stale_path).unwrap(),
        stale_bytes,
        "the engine 7 entry changed"
    );
    let current = cache
        .cache
        .object_path(&key_for(&checkout, &config, PATH, "rust"));
    assert_ne!(current, stale_path);
    assert!(
        current.is_file(),
        "no entry of the current engine was written"
    );
    assert_eq!(cache.entries().len(), 2);

    let probe = Probe::new(Twist::None);
    let warm = run(&checkout, Some(&cache), &probe);
    assert_eq!(
        warm.stats.cache_hits, 1,
        "the current engine's entry was not served"
    );
    assert_eq!(probe.files(), 0);
    assert_eq!(extract_of(&warm, PATH), cold);
}

#[test]
fn cache_entries_of_engine_eight_are_never_served() {
    // Engine 8 read a Rust `use` declaration as text and split its brace list at every
    // comma, so a nested list over several lines bound none of its names: here
    // `AtomicU32`. Engine 9 reads the syntax tree (issue 111): the static's type is
    // known, the per-file typed pass answers `COUNTER.load(...)` inside `assert_eq!`,
    // and the call that answer adds is part of the extraction. Same source, path,
    // language and policy: only the version in the key keeps what engine 8 cached, the
    // extraction without that call, from being served.
    const PATH: &str = "src/lib.rs";
    let checkout = Checkout::new();
    checkout.write(
        PATH,
        "use std::{\n    fmt,\n    sync::atomic::{AtomicU32, Ordering},\n};\n\nstatic COUNTER: AtomicU32 = AtomicU32::new(0);\n\npub fn check() -> fmt::Result {\n    assert_eq!(COUNTER.load(Ordering::SeqCst), 0);\n    Ok(())\n}\n",
    );
    let config = checkout.config();
    let load_calls = |extract: &FileExtract| -> Vec<usize> {
        (0..extract.record().calls.len())
            .filter(|&i| extract.text(&extract.record().calls[i].callee_text) == "load")
            .collect()
    };
    let fresh = run(&checkout, None, &Probe::new(Twist::None));
    let fresh = extract_of(&fresh, PATH);
    let [load] = load_calls(fresh)[..] else {
        panic!("the current engine adds one `load` call");
    };

    // What engine 8 cached for these bytes: the same extraction without that call.
    let mut stale = fresh.record().clone();
    stale.calls.remove(load);
    let mut key = key_for(&checkout, &config, PATH, "rust");
    key.engine_version = 8;
    let cache = Cache::new();
    let stale_bytes = CacheEnvelope::encode(&key, &stale);
    plant(&cache, &key, &stale_bytes);
    let stale_path = cache.cache.object_path(&key);

    let probe = Probe::new(Twist::None);
    let cold = run(&checkout, Some(&cache), &probe);
    assert_eq!(cold.stats.cache_hits, 0, "an engine 8 entry was served");
    assert_eq!(probe.files(), 1, "the file was not extracted again");
    let cold = extract_of(&cold, PATH);
    assert_eq!(load_calls(cold).len(), 1, "the `load` call is missing");
    assert_eq!(cold, fresh);
    assert_eq!(
        fs::read(&stale_path).unwrap(),
        stale_bytes,
        "the engine 8 entry changed"
    );
    let current = cache
        .cache
        .object_path(&key_for(&checkout, &config, PATH, "rust"));
    assert_ne!(current, stale_path);
    assert!(
        current.is_file(),
        "no entry of the current engine was written"
    );

    let probe = Probe::new(Twist::None);
    let warm = run(&checkout, Some(&cache), &probe);
    assert_eq!(
        warm.stats.cache_hits, 1,
        "the current engine's entry was not served"
    );
    assert_eq!(probe.files(), 0);
    assert_eq!(extract_of(&warm, PATH), cold);
}

#[test]
fn cache_entries_of_engine_nine_are_never_served() {
    // Engine 9 recorded no members a Rust `macro_rules!` invocation may write. Engine 10
    // records them (issue 110): here `Payload.from`, which the file's own macro writes
    // beside the handwritten `From<()>` impl. Same source, path, language and policy:
    // only the version in the key keeps what engine 9 cached, the extraction without
    // them, from being served.
    const PATH: &str = "src/lib.rs";
    let checkout = Checkout::new();
    checkout.write(
        PATH,
        "pub struct Payload(u8);\n\nimpl From<()> for Payload {\n    fn from(_: ()) -> Self {\n        Payload(0)\n    }\n}\n\nmacro_rules! make_impl {\n    ($ty:ty) => {\n        impl From<$ty> for Payload {\n            fn from(_: $ty) -> Self {\n                Payload(1)\n            }\n        }\n    };\n}\n\nmake_impl!(u32);\n",
    );
    let config = checkout.config();
    let fresh = run(&checkout, None, &Probe::new(Twist::None));
    let fresh = extract_of(&fresh, PATH);
    assert_eq!(
        fresh.record().macro_members,
        [MacroMember {
            type_name: "Payload".into(),
            member: Some("from".into()),
        }],
        "the current engine records the member the macro writes"
    );

    // What engine 9 cached for these bytes: the same extraction without it.
    let mut stale = fresh.record().clone();
    stale.macro_members.clear();
    let mut key = key_for(&checkout, &config, PATH, "rust");
    key.engine_version = 9;
    let cache = Cache::new();
    let stale_bytes = CacheEnvelope::encode(&key, &stale);
    plant(&cache, &key, &stale_bytes);
    let stale_path = cache.cache.object_path(&key);

    let probe = Probe::new(Twist::None);
    let cold = run(&checkout, Some(&cache), &probe);
    assert_eq!(cold.stats.cache_hits, 0, "an engine 9 entry was served");
    assert_eq!(probe.files(), 1, "the file was not extracted again");
    let cold = extract_of(&cold, PATH);
    assert_eq!(
        cold.record().macro_members.len(),
        1,
        "the member is missing"
    );
    assert_eq!(cold, fresh);
    assert_eq!(
        fs::read(&stale_path).unwrap(),
        stale_bytes,
        "the engine 9 entry changed"
    );
    let current = cache
        .cache
        .object_path(&key_for(&checkout, &config, PATH, "rust"));
    assert_ne!(current, stale_path);
    assert!(
        current.is_file(),
        "no entry of the current engine was written"
    );

    let probe = Probe::new(Twist::None);
    let warm = run(&checkout, Some(&cache), &probe);
    assert_eq!(
        warm.stats.cache_hits, 1,
        "the current engine's entry was not served"
    );
    assert_eq!(probe.files(), 0);
    assert_eq!(extract_of(&warm, PATH), cold);
}

#[test]
fn cache_entries_of_engine_ten_are_never_served() {
    // Engine 10 read no `use` inside a function's body. Engine 11 binds it in the body's
    // scope (issue 114): `AtomicU32` is known there, the per-file typed pass answers
    // `counter.load(...)` inside `assert_eq!`, and the call that answer adds is part of
    // the extraction, placed in the body's scope. The engine 10 entry is planted in the
    // current cache format, so only the engine version in the key keeps it, the
    // extraction without that call or any scope, from being served.
    const PATH: &str = "src/lib.rs";
    let checkout = Checkout::new();
    checkout.write(
        PATH,
        "pub fn check() -> bool {\n    use std::sync::atomic::AtomicU32;\n    let counter = AtomicU32::new(0);\n    assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 0);\n    true\n}\n",
    );
    let config = checkout.config();
    let load_calls = |extract: &FileExtract| -> Vec<usize> {
        (0..extract.record().calls.len())
            .filter(|&i| extract.text(&extract.record().calls[i].callee_text) == "load")
            .collect()
    };
    let fresh = run(&checkout, None, &Probe::new(Twist::None));
    let fresh = extract_of(&fresh, PATH);
    let [load] = load_calls(fresh)[..] else {
        panic!("the current engine adds one `load` call");
    };
    assert_eq!(
        fresh.record().calls[load].use_scope,
        Some(1),
        "the call is placed in the body's scope"
    );

    // What engine 10 cached for these bytes, in the current format: the extraction
    // without that call and without scopes.
    let mut stale = fresh.record().clone();
    stale.calls.remove(load);
    stale.use_scopes.clear();
    stale.scoped_imports.clear();
    for call in &mut stale.calls {
        call.use_scope = None;
    }
    let mut key = key_for(&checkout, &config, PATH, "rust");
    key.engine_version = 10;
    let cache = Cache::new();
    let stale_bytes = CacheEnvelope::encode(&key, &stale);
    plant(&cache, &key, &stale_bytes);
    let stale_path = cache.cache.object_path(&key);
    assert!(
        stale_path
            .to_string_lossy()
            .replace('\\', "/")
            .contains(&format!("/v{EXTRACT_CACHE_FORMAT_VERSION}/")),
        "the engine 10 entry is in the current format"
    );

    let probe = Probe::new(Twist::None);
    let cold = run(&checkout, Some(&cache), &probe);
    assert_eq!(cold.stats.cache_hits, 0, "an engine 10 entry was served");
    assert_eq!(probe.files(), 1, "the file was not extracted again");
    let cold = extract_of(&cold, PATH);
    assert_eq!(load_calls(cold).len(), 1, "the `load` call is missing");
    assert_eq!(cold, fresh);
    assert_eq!(
        fs::read(&stale_path).unwrap(),
        stale_bytes,
        "the engine 10 entry changed"
    );
    let current = cache
        .cache
        .object_path(&key_for(&checkout, &config, PATH, "rust"));
    assert_ne!(current, stale_path);
    assert!(
        current.is_file(),
        "no entry of the current engine was written"
    );

    let probe = Probe::new(Twist::None);
    let warm = run(&checkout, Some(&cache), &probe);
    assert_eq!(
        warm.stats.cache_hits, 1,
        "the current engine's entry was not served"
    );
    assert_eq!(probe.files(), 0);
    assert_eq!(extract_of(&warm, PATH), cold);
}

#[test]
fn cache_entries_of_engine_eleven_are_never_served() {
    // Engine 11 answered `Form::<u8>::from_request` inside `assert!` with `Form`'s own
    // impl, whose header requires `S: Send`, which nothing proves, and added the call that
    // answer found to the extraction. Engine 12 answers a trait impl's method only when
    // its header is proven (issue 109): no answer, no call, and a surface (version 6)
    // carrying the impl's header. The engine 11 entry is planted in the current cache
    // format, so only the engine version in the key keeps it, the extraction with that
    // call, from being served.
    const PATH: &str = "src/lib.rs";
    const SOURCE: &str = "pub trait FromRequest<S> {\n    fn from_request(req: u32, state: &S) -> Self;\n}\n\npub struct Form<T>(pub T);\n\npub fn check() {\n    assert!(Form::<u8>::from_request(1, &()).0 == 1);\n}\n\nimpl<T, S: Send> FromRequest<S> for Form<T> {\n    fn from_request(_req: u32, _state: &S) -> Self {\n        unimplemented!()\n    }\n}\n";
    let checkout = Checkout::new();
    checkout.write(PATH, SOURCE);
    let config = checkout.config();
    let from_request_calls = |extract: &FileExtract| -> Vec<usize> {
        (0..extract.record().calls.len())
            .filter(|&i| extract.text(&extract.record().calls[i].callee_text) == "from_request")
            .collect()
    };
    let fresh = run(&checkout, None, &Probe::new(Twist::None));
    let fresh = extract_of(&fresh, PATH);
    assert!(
        from_request_calls(fresh).is_empty(),
        "the current engine answers the unproven call"
    );
    assert!(
        fresh.record().surface.as_bytes().starts_with(b"{\"v\":7,"),
        "the current engine's surface is version 7 (6 at engine 12)"
    );

    // The call engine 11 added is the one the current engine adds when the bound is
    // gone: the same bytes but for the header's `S: Send`, spaces of its length in its
    // place, so every range stays where it is.
    let unbounded = SOURCE.replace("S: Send>", "S      >");
    assert_eq!(unbounded.len(), SOURCE.len());
    let variant = Checkout::new();
    variant.write(PATH, &unbounded);
    let variant = run(&variant, None, &Probe::new(Twist::None));
    let variant = extract_of(&variant, PATH);
    let [added] = from_request_calls(variant)[..] else {
        panic!("the proven impl's call is added");
    };
    assert!(variant.record().calls[added].typed_only);

    // What engine 11 cached for these bytes, in the current format: the extraction with
    // that call.
    let mut stale = fresh.record().clone();
    stale
        .calls
        .insert(added, variant.record().calls[added].clone());
    let mut key = key_for(&checkout, &config, PATH, "rust");
    key.engine_version = 11;
    let cache = Cache::new();
    let stale_bytes = CacheEnvelope::encode(&key, &stale);
    plant(&cache, &key, &stale_bytes);
    let stale_path = cache.cache.object_path(&key);
    assert!(
        stale_path
            .to_string_lossy()
            .replace('\\', "/")
            .contains(&format!("/v{EXTRACT_CACHE_FORMAT_VERSION}/")),
        "the engine 11 entry is in the current format"
    );

    let probe = Probe::new(Twist::None);
    let cold = run(&checkout, Some(&cache), &probe);
    assert_eq!(cold.stats.cache_hits, 0, "an engine 11 entry was served");
    assert_eq!(probe.files(), 1, "the file was not extracted again");
    let cold = extract_of(&cold, PATH);
    assert!(
        from_request_calls(cold).is_empty(),
        "the unproven call is back"
    );
    assert_eq!(cold, fresh);
    assert_eq!(
        fs::read(&stale_path).unwrap(),
        stale_bytes,
        "the engine 11 entry changed"
    );
    let current = cache
        .cache
        .object_path(&key_for(&checkout, &config, PATH, "rust"));
    assert_ne!(current, stale_path);
    assert!(
        current.is_file(),
        "no entry of the current engine was written"
    );

    let probe = Probe::new(Twist::None);
    let warm = run(&checkout, Some(&cache), &probe);
    assert_eq!(
        warm.stats.cache_hits, 1,
        "the current engine's entry was not served"
    );
    assert_eq!(probe.files(), 0);
    assert_eq!(extract_of(&warm, PATH), cold);
}

#[test]
fn cache_entries_of_engine_twelve_are_never_served() {
    // Engine 12 extracted no call whose function carries a turbofish: `decode::<u8>(raw)`
    // had no callee and was dropped. Engine 13 extracts it as `decode`'s call (issue
    // 107A). The engine 12 entry is planted in the current cache format, so only the
    // engine version in the key keeps it, the extraction without that call, from being
    // served.
    const PATH: &str = "src/lib.rs";
    let checkout = Checkout::new();
    checkout.write(
        PATH,
        "pub fn decode<T>(raw: u8) -> u8 {\n    raw\n}\n\npub fn run(raw: u8) -> u8 {\n    decode::<u8>(raw)\n}\n",
    );
    let config = checkout.config();
    let decode_calls = |extract: &FileExtract| -> Vec<usize> {
        (0..extract.record().calls.len())
            .filter(|&i| extract.text(&extract.record().calls[i].callee_text) == "decode")
            .collect()
    };
    let fresh = run(&checkout, None, &Probe::new(Twist::None));
    let fresh = extract_of(&fresh, PATH);
    let [decode] = decode_calls(fresh)[..] else {
        panic!("the current engine extracts the turbofish call once");
    };

    // What engine 12 cached for these bytes, in the current format: the extraction
    // without that call.
    let mut stale = fresh.record().clone();
    stale.calls.remove(decode);
    let mut key = key_for(&checkout, &config, PATH, "rust");
    key.engine_version = 12;
    let cache = Cache::new();
    let stale_bytes = CacheEnvelope::encode(&key, &stale);
    plant(&cache, &key, &stale_bytes);
    let stale_path = cache.cache.object_path(&key);
    assert!(
        stale_path
            .to_string_lossy()
            .replace('\\', "/")
            .contains(&format!("/v{EXTRACT_CACHE_FORMAT_VERSION}/")),
        "the engine 12 entry is in the current format"
    );

    let probe = Probe::new(Twist::None);
    let cold = run(&checkout, Some(&cache), &probe);
    assert_eq!(cold.stats.cache_hits, 0, "an engine 12 entry was served");
    assert_eq!(probe.files(), 1, "the file was not extracted again");
    let cold = extract_of(&cold, PATH);
    assert_eq!(decode_calls(cold).len(), 1, "the turbofish call is missing");
    assert_eq!(cold, fresh);
    assert_eq!(
        fs::read(&stale_path).unwrap(),
        stale_bytes,
        "the engine 12 entry changed"
    );
    let current = cache
        .cache
        .object_path(&key_for(&checkout, &config, PATH, "rust"));
    assert_ne!(current, stale_path);
    assert!(
        current.is_file(),
        "no entry of the current engine was written"
    );

    let probe = Probe::new(Twist::None);
    let warm = run(&checkout, Some(&cache), &probe);
    assert_eq!(
        warm.stats.cache_hits, 1,
        "the current engine's entry was not served"
    );
    assert_eq!(probe.files(), 0);
    assert_eq!(extract_of(&warm, PATH), cold);
}

#[test]
fn cache_entries_of_engine_thirteen_are_never_served() {
    // Engine 13's surface (version 6) carried no Rust impl method's declared return type
    // and no function's `async`: from it the cross-file pass typed no value a method
    // returns. Engine 14's (version 7) carries both, and its per-file typed answers read
    // a Rust path as Rust's name resolution does (issue 105). The engine 13 entry is
    // planted in the current cache format, so only the engine version in the key keeps
    // it, the extraction with that surface, from being served.
    const PATH: &str = "src/lib.rs";
    let checkout = Checkout::new();
    checkout.write(
        PATH,
        "pub struct Job;\n\nimpl Job {\n    pub async fn start(&self) -> Job {\n        Job\n    }\n}\n",
    );
    let config = checkout.config();
    let fresh = run(&checkout, None, &Probe::new(Twist::None));
    let fresh = extract_of(&fresh, PATH);
    let surface = std::str::from_utf8(fresh.record().surface.as_bytes()).expect("JSON");
    assert!(surface.starts_with("{\"v\":7,"), "the surface is version 7");
    assert!(
        surface.contains("\"rust_return_type\":\"Job\""),
        "{surface}"
    );
    assert!(surface.contains("\"rust_is_async\":true"), "{surface}");

    // What engine 13 cached for these bytes, in the current format: the extraction with
    // a version 6 surface, which has neither fact.
    let mut older = surface.replacen("{\"v\":7,", "{\"v\":6,", 1);
    for fact in [
        ",\"rust_return_type\":\"Job\"",
        ",\"rust_return_type\":null",
        ",\"rust_is_async\":true",
        ",\"rust_is_async\":false",
    ] {
        older = older.replace(fact, "");
    }
    assert!(!older.contains("rust_"), "{older}");
    let mut stale = fresh.record().clone();
    stale.surface = serde_json::from_str(&serde_json::to_string(&older).unwrap())
        .expect("a surface from its bytes");
    let mut key = key_for(&checkout, &config, PATH, "rust");
    key.engine_version = 13;
    let cache = Cache::new();
    let stale_bytes = CacheEnvelope::encode(&key, &stale);
    plant(&cache, &key, &stale_bytes);
    let stale_path = cache.cache.object_path(&key);
    assert!(
        stale_path
            .to_string_lossy()
            .replace('\\', "/")
            .contains(&format!("/v{EXTRACT_CACHE_FORMAT_VERSION}/")),
        "the engine 13 entry is in the current format"
    );

    let probe = Probe::new(Twist::None);
    let cold = run(&checkout, Some(&cache), &probe);
    assert_eq!(cold.stats.cache_hits, 0, "an engine 13 entry was served");
    assert_eq!(probe.files(), 1, "the file was not extracted again");
    let cold = extract_of(&cold, PATH);
    assert_eq!(cold, fresh);
    assert_eq!(
        fs::read(&stale_path).unwrap(),
        stale_bytes,
        "the engine 13 entry changed"
    );
    let current = cache
        .cache
        .object_path(&key_for(&checkout, &config, PATH, "rust"));
    assert_ne!(current, stale_path);
    assert!(
        current.is_file(),
        "no entry of the current engine was written"
    );

    let probe = Probe::new(Twist::None);
    let warm = run(&checkout, Some(&cache), &probe);
    assert_eq!(
        warm.stats.cache_hits, 1,
        "the current engine's entry was not served"
    );
    assert_eq!(probe.files(), 0);
    assert_eq!(extract_of(&warm, PATH), cold);
}

#[test]
fn c_family_engine_reads_only_the_normalised_bytes() {
    // Stage 2 gives the engine the masked buffer and never opens a redacted file. A
    // source that includes itself (the X-macro idiom), or includes a redacted file, must
    // not get the original bytes back from disk: the URI the source's own macro expands
    // to is the masked one.
    const TEST: &str = "c_family_engine_reads_only_the_normalised_bytes";
    if in_child() {
        return child_run();
    }
    let tag = |n: u32| format!("{}{n:04}", ["pdx", "synth", "leak"].concat());
    let (base, nowhere) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (repo, empty) = (base.path().join("repo"), nowhere.path());
    let vault = format!(
        "#define VAULT_URI \"{}://app:{}@db/app\"\n#define CONNECT() connect_db(VAULT_URI)\n#ifndef VAULT_SELF\n#define VAULT_SELF\n#include \"vault.c\"\nvoid open_vault(void) {{ CONNECT(); }}\n#endif\n",
        "postgres",
        tag(11)
    );
    let key = format!("#define DEPLOY_KEY_ARG \"{}\"\n", tag(12));
    files_in(
        &repo,
        &[
            ("src/vault.c", vault.as_str()),
            ("keys/id_rsa_deploy.h", key.as_str()),
            (
                "src/deploy.c",
                "#include \"../keys/id_rsa_deploy.h\"\n#define LOCAL 1\nvoid deploy(void) { push_key(DEPLOY_KEY_ARG); }\n",
            ),
        ],
    );
    let planted = [tag(11), tag(12)];
    let cache = Cache::new();
    let cache_dir = cache.dir.path().join("extract");

    let at_root = run_at(TEST, &repo, &repo, Some(&cache_dir));
    let at_empty = run_at(TEST, empty, &repo, None);
    assert_eq!(
        at_root.extracts.keys().collect::<Vec<_>>(),
        ["src/deploy.c", "src/vault.c"],
        "the key file is redacted"
    );
    assert_eq!(at_root.extracts, at_empty.extracts);
    // The source's own macro still expands, to the masked URI.
    let masked = format!("postgres://app:{}@db/app", "X".repeat(tag(11).len()));
    assert!(at_root.extracts["src/vault.c"].contains(&masked));
    for (path, extract) in &at_root.extracts {
        for secret in &planted {
            assert!(!extract.contains(secret.as_str()), "{secret} in {path}");
        }
    }
    for bytes in cache.entries().values() {
        for secret in &planted {
            assert!(!contains(bytes, secret), "{secret} in the cache");
        }
    }
}

#[test]
fn date_and_time_cannot_change_a_cached_extraction() {
    // __DATE__ and __TIME__ are fixed (issue 84): the clock, the time zone and the locale
    // are no extraction inputs, so an extraction cached under one is the one made fresh
    // under another, and the key needs none of them.
    const TEST: &str = "date_and_time_cannot_change_a_cached_extraction";
    if in_child() {
        return child_run();
    }
    let checkout = Checkout::new();
    checkout.write(
        "src/stamp.c",
        "void record_stamp(const char *, const char *);\n\n#define STAMP() record_stamp(__DATE__, __TIME__)\n\nvoid f(void) {\n    STAMP();\n}\n",
    );
    let cache = Cache::new();
    let cache_dir = cache.dir.path().join("extract");
    let root = checkout.root();

    // Zones as POSIX rules, east and west of UTC, which need no zone database.
    let cold = run_at_with(
        TEST,
        root,
        root,
        Some(&cache_dir),
        &[("TZ", "UTC0"), ("LC_ALL", "C")],
    );
    let warm = run_at_with(
        TEST,
        root,
        root,
        Some(&cache_dir),
        &[("TZ", "NZST-12"), ("LC_ALL", "de_DE.UTF-8")],
    );
    let fresh = run_at_with(
        TEST,
        root,
        root,
        None,
        &[("TZ", "PST8"), ("LC_ALL", "fr_FR.UTF-8")],
    );
    assert_eq!(cold.cache_hits, 0);
    assert_eq!(
        warm.cache_hits, 1,
        "the second run is served from the cache"
    );
    assert_eq!(cold.extracts, fresh.extracts);
    assert_eq!(warm.extracts, fresh.extracts);
    let stamp: serde_json::Value = serde_json::from_str(&fresh.extracts["src/stamp.c"]).unwrap();
    let values: Vec<&str> = stamp["record"]["calls"]
        .as_array()
        .unwrap()
        .iter()
        .zip(stamp["callees"].as_array().unwrap())
        .filter(|(_, callee)| *callee == "record_stamp")
        .flat_map(|(c, _)| c["args"].as_array().unwrap())
        .filter_map(|a| a["value"].as_str())
        .collect();
    assert_eq!(values, ["Jan 01 1970", "00:00:00"]);
}

#[test]
fn c_family_preprocessor_sizeof_is_platform_independent() {
    // `sizeof` in `#if` has one fixed data model (issue 85): LP64, as 64-bit Linux has it,
    // on every platform. The expectations are the same on Linux, macOS and Windows, and
    // the key names no platform: a cache filled on one is right on any other.
    let checkout = Checkout::new();
    checkout.write(
        "src/model.c",
        "int model_lp64(void);\n#if sizeof(long) == 8 && sizeof(long double) == 16 && sizeof(char *) == 8\n#define MODEL() model_lp64()\n#elif sizeof(long) == 4\n#define MODEL() model_llp64()\n#else\n#define MODEL() model_other()\n#endif\nint f(void) { return MODEL(); }\n",
    );
    let cache = Cache::new();
    let cold = run(&checkout, Some(&cache), &Probe::new(Twist::None));
    let warm_probe = Probe::new(Twist::None);
    let warm = run(&checkout, Some(&cache), &warm_probe);
    let fresh = run(&checkout, None, &Probe::new(Twist::None));
    assert_eq!(
        warm_probe.files(),
        0,
        "the second run is served from the cache"
    );
    assert_eq!(cold.files, fresh.files);
    assert_eq!(warm.files, fresh.files);
    let model = extract_of(&fresh, "src/model.c");
    let callees: Vec<String> = model
        .calls
        .iter()
        .map(|c| model.text(&c.callee_text).into_owned())
        .collect();
    let callees: Vec<&str> = callees.iter().map(String::as_str).collect();
    assert!(callees.contains(&"model_lp64"), "{callees:?}");
    assert!(
        !callees.contains(&"model_llp64") && !callees.contains(&"model_other"),
        "{callees:?}"
    );
}

/// `D24` would call `leaf` 2^24 times: its preprocessing passes its ceilings (issue 87).
fn c_bomb() -> String {
    let mut source = String::from("void before(void);\n#define D0 leaf();\n");
    for i in 1..=24 {
        writeln!(source, "#define D{i} D{} D{}", i - 1, i - 1).unwrap();
    }
    source.push_str("void run(void) { before(); D24 }\n");
    source
}

/// A `#line` four billion lines on: one marker in the expanded text, never padding.
const C_FAR_LINE: &str =
    "#define KEEP 1\nint before(void);\n#line 4294967295\nvoid after(void) { callee(); }\n";

#[test]
fn preprocessing_past_its_ceilings_is_lost_work_and_never_cached() {
    // Issue 87: a preprocessing that passes a ceiling is discarded, and the file is
    // extracted from its source alone, losing work once. That extraction degrades and
    // is never cached, so every run, cold or warm, extracts it again and gets the same;
    // a far #line is clean, and cached.
    let checkout = Checkout::new();
    checkout.write("src/bomb.c", c_bomb());
    checkout.write("src/far.c", C_FAR_LINE);
    let cache = Cache::new();
    let probe = Probe::new(Twist::None);
    let cold = run(&checkout, Some(&cache), &probe);
    assert_eq!(probe.files(), 2);
    let bomb = extract_of(&cold, "src/bomb.c");
    assert_eq!(bomb.extraction_lost, 1, "lost work is counted once");
    let callees: Vec<String> = bomb
        .calls
        .iter()
        .map(|c| bomb.text(&c.callee_text).into_owned())
        .collect();
    let callees: Vec<&str> = callees.iter().map(String::as_str).collect();
    assert!(
        callees.contains(&"before") && !callees.contains(&"leaf"),
        "{callees:?}"
    );
    assert_eq!(extract_of(&cold, "src/far.c").extraction_lost, 0);
    assert!(cold.degraded());
    assert_eq!(
        cold.stats.cache_writes, 1,
        "only the clean extraction is cached"
    );
    assert_eq!(cache.entries().len(), 1);

    // A second cold run, with a cache of its own, then the first cache warm.
    let other = Cache::new();
    let second = run(&checkout, Some(&other), &Probe::new(Twist::None));
    let warm_probe = Probe::new(Twist::None);
    let warm = run(&checkout, Some(&cache), &warm_probe);
    assert_eq!(
        warm_probe.files(),
        1,
        "the lossy extraction is never served from the cache"
    );
    assert_eq!(warm.stats.cache_hits, 1);
    assert_eq!(warm.stats.cache_writes, 0);
    assert!(warm.degraded() && second.degraded());
    assert_eq!(second.files, cold.files);
    assert_eq!(warm.files, cold.files);
    assert_eq!(other.entries(), cache.entries());
}

/// 60 statements of 100 nested calls in one C function: past the file's node-type path
/// budget (issue 88), shallow enough to reach nothing else.
fn c_path_bomb() -> String {
    let chain = format!("{}f(0){}", "f(".repeat(99), ")".repeat(99));
    let mut source = String::from("void work(void) {\n");
    for _ in 0..60 {
        writeln!(source, "    {chain};").unwrap();
    }
    source.push_str("}\n");
    source
}

#[test]
fn path_budget_exhaustion_is_lost_work_and_never_cached() {
    // Issue 88: an extraction whose node-type paths passed the file's budget loses work
    // once; it is never cached, so every run, cold or warm, extracts it again and gets
    // the same. A file under the budget is clean, and cached.
    let checkout = Checkout::new();
    checkout.write("src/paths.c", c_path_bomb());
    checkout.write("src/app.py", PYTHON);
    let cache = Cache::new();
    let cold = run(&checkout, Some(&cache), &Probe::new(Twist::None));
    let lossy = extract_of(&cold, "src/paths.c");
    assert_eq!(lossy.extraction_lost, 1, "lost work is counted once");
    assert!(lossy.calls.iter().any(|c| c.ast_path.is_empty()));
    assert!(lossy.calls.iter().any(|c| !c.ast_path.is_empty()));
    assert!(cold.degraded());
    assert_eq!(
        cold.stats.cache_writes, 1,
        "only the clean extraction is cached"
    );

    let other = Cache::new();
    let second = run(&checkout, Some(&other), &Probe::new(Twist::None));
    let warm_probe = Probe::new(Twist::None);
    let warm = run(&checkout, Some(&cache), &warm_probe);
    assert_eq!(
        warm_probe.files(),
        1,
        "the lossy extraction is never served"
    );
    assert_eq!(warm.stats.cache_hits, 1);
    assert_eq!(warm.stats.cache_writes, 0);
    assert_eq!(second.files, cold.files);
    assert_eq!(warm.files, cold.files);
    assert_eq!(other.entries(), cache.entries());
}

/// 300 Java classes nested inside one another: past the syntax depth (issue 89).
fn java_deep_classes() -> String {
    let mut source = String::from("package p;\n");
    for i in 0..300 {
        write!(source, "class C{i} {{ ").unwrap();
    }
    source.push_str("void m() { f(); }");
    source.push_str(&"}".repeat(300));
    source.push('\n');
    source
}

#[test]
fn syntax_depth_refusal_is_lost_work_and_never_cached() {
    // Issue 89: a file whose tree is deeper than the engine admits is not extracted; it
    // holds no fact, loses work once, and is never cached, so every run, cold or warm,
    // refuses it the same way.
    let checkout = Checkout::new();
    checkout.write("src/main/java/p/Deep.java", java_deep_classes());
    checkout.write("src/app.py", PYTHON);
    let cache = Cache::new();
    let cold = run(&checkout, Some(&cache), &Probe::new(Twist::None));
    let refused = extract_of(&cold, "src/main/java/p/Deep.java");
    assert_eq!(refused.extraction_lost, 1);
    assert!(refused.definitions.is_empty() && refused.calls.is_empty());
    assert!(cold.degraded());
    assert_eq!(
        cold.stats.cache_writes, 1,
        "only the clean extraction is cached"
    );

    let other = Cache::new();
    let second = run(&checkout, Some(&other), &Probe::new(Twist::None));
    let warm_probe = Probe::new(Twist::None);
    let warm = run(&checkout, Some(&cache), &warm_probe);
    assert_eq!(
        warm_probe.files(),
        1,
        "the refused extraction is never served"
    );
    assert_eq!(warm.stats.cache_hits, 1);
    assert_eq!(second.files, cold.files);
    assert_eq!(warm.files, cold.files);
    assert_eq!(other.entries(), cache.entries());
}
