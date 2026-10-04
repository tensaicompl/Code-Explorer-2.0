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
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pdx_core::config::PdxConfig;
use pdx_core::consts::{
    ENGINE_VERSION, EXTRACT_BATCH_MAX_FILES, EXTRACT_CACHE_FORMAT_VERSION, LANGUAGE_MATRIX_VERSION,
    SECRET_DETECTOR_VERSION,
};
use pdx_core::index::discover::{DiscoveredFile, discover};
use pdx_core::index::extract::{
    BlobSha, CacheDefect, CacheEnvelope, CacheKey, ExtractBackend, ExtractCache, ExtractError,
    ExtractLimits, ExtractReport, ExtractStage, FileOutcome, FsCache, SourceChange, plan_batches,
    prepare_source,
};
use pdx_core::secrets::{PRIVATE_KEY_LABELS, SecretPolicyDigest};
use pdx_engine::isolate::{ExtractFailure, ExtractOutcome, Extractor, IsolationError, SourceFile};
use pdx_engine::{Engine, EngineError, FileExtract, FileStatus, NODE_BUDGET_ENV, SourceDigest};

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
                        Twist::Truncate => extract.truncated = true,
                        Twist::LoseWork => extract.extraction_lost = 3,
                        Twist::StatusFailed => extract.status = FileStatus::Failed,
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
    if std::env::var_os(CHILD).is_some() {
        return false;
    }
    let out = Command::new(std::env::current_exe().expect("the test binary"))
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .envs(env.iter().copied())
        .output()
        .expect("the test binary runs");
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
        LANGUAGE_MATRIX_VERSION, 1,
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
        EXTRACT_CACHE_FORMAT_VERSION, 3,
        "the vector below is format 3's"
    );
    assert_eq!(
        SECRET_DETECTOR_VERSION, 2,
        "the vector below is detector 2's, through the policy digest"
    );
    let id = key.object_id().to_string();
    assert_eq!(
        id,
        "9e53439d946efd2d697239ab10b5633cf770e93b55779a34dcd991f1cadb33d4"
    );
    let path = FsCache::at("cache").object_path(&key);
    assert_eq!(path, Path::new("cache").join("v3").join("9e").join(&id));
    // Moving a byte between the two strings changes the id: their lengths are part of
    // the encoding.
    let mut shifted = key.clone();
    shifted.language_id = "pythons".into();
    shifted.rel_path = "rc/app.py".into();
    assert_ne!(shifted.object_id(), key.object_id());
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
        FileExtract {
            truncated: true,
            ..clean.clone()
        },
        FileExtract {
            extraction_lost: 1,
            ..clean.clone()
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
        assert_eq!(cache.cache.load(&key), Ok(Some(clean.clone())));
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
    let other_digest = FileExtract {
        source_digest: SourceDigest::of(b"other bytes"),
        ..clean.clone()
    };
    let other_len = FileExtract {
        source_len: clean.source_len + 1,
        ..clean.clone()
    };
    let other_path = FileExtract {
        rel_path: "src/other.py".into(),
        ..clean.clone()
    };
    let other_language = FileExtract {
        language: "ruby".into(),
        ..clean.clone()
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
        assert_eq!(cache.cache.load(&key), Ok(Some(clean.clone())), "{what}");
    }
}

#[test]
fn cache_format_2_is_a_miss() {
    // Format 2 held extractions without their impl relations (issue 42), format 1
    // also definitions without base classes and extractions without a declared
    // namespace. Their entries are never decoded as the current format: not in the
    // directory their format used, which is never read, and not at the current path
    // with their format written in them.
    for old_format in [1, 2] {
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
            extract: clean.clone(),
        })
        .unwrap();
        let cache = Cache::new();
        let current = cache.cache.object_path(&key);
        let current_text = current.to_string_lossy().replace('\\', "/");
        assert!(current_text.contains("/v3/"), "{current_text}");
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
            Ok(Some(clean)),
            "the format-{old_format} entry was not replaced"
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
    let good = CacheEnvelope::encode(&key, &clean);
    let other_key = CacheKey {
        rel_path: "src/other.py".into(),
        ..key.clone()
    };
    let wrong_version = postcard::to_stdvec(&CacheEnvelope {
        format_version: EXTRACT_CACHE_FORMAT_VERSION + 1,
        key: key.clone(),
        extract: clean.clone(),
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
            Ok(Some(clean.clone())),
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
    cache.cache.store(&key, &clean).unwrap();
    let renamed = FileExtract {
        status: FileStatus::Partial,
        ..clean.clone()
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
        &CacheEnvelope::encode(&key, &clean)[..10],
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
        let fields = serde_json::to_vec(extract).unwrap();
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
