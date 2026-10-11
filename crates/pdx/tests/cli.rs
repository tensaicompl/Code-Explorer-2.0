//! The command line (P2-11): `pdx index`, `pdx coverage` and `pdx version`, run as the
//! built binary against real Git repositories made here.
//!
//! Every fixture repository is made with the user's and the system's Git configuration
//! shut out, fixed author and committer names, emails and dates, and `core.autocrlf`
//! off, so its commit is the same on every host. The binary is run with its caches,
//! home and Git configuration pointed into the test's own directories.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const ORIGIN: &str = "https://git.example/acme/shop.git";

/// The directories a test owns: the repository, a home for the binary's caches and
/// Git configuration, and somewhere else to run from.
struct World {
    repo: tempfile::TempDir,
    home: tempfile::TempDir,
    elsewhere: tempfile::TempDir,
}

impl World {
    fn new() -> Self {
        let home = tempfile::tempdir().expect("a home");
        fs::write(home.path().join("gitconfig"), "").expect("an empty Git configuration");
        Self {
            repo: tempfile::Builder::new()
                .prefix("pdx-cli-test-")
                .tempdir()
                .expect("a repository"),
            home,
            elsewhere: tempfile::tempdir().expect("a working directory"),
        }
    }

    fn root(&self) -> &Path {
        self.repo.path()
    }

    /// Git, isolated from the user's and the system's configuration.
    fn git(&self, args: &[&str]) {
        let status = self
            .isolated(&mut Command::new("git"))
            .current_dir(self.root())
            .args(args)
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?}");
    }

    fn isolated<'c>(&self, command: &'c mut Command) -> &'c mut Command {
        let home = self.home.path();
        command
            .env("GIT_CONFIG_GLOBAL", home.join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Fixture Author")
            .env("GIT_AUTHOR_EMAIL", "author@git.example")
            .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00+00:00")
            .env("GIT_COMMITTER_NAME", "Fixture Committer")
            .env("GIT_COMMITTER_EMAIL", "committer@git.example")
            .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00+00:00")
            .env("HOME", home)
            .env("XDG_CACHE_HOME", home.join("cache"))
            .env("LOCALAPPDATA", home.join("local"))
            .env_remove("PDX_WORKERS")
            .env_remove("PDX_MEM_BUDGET_MB")
            .env_remove("PDX_ENGINE_ISOLATE")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
    }

    fn write(&self, path: &str, content: &str) -> &Self {
        let path = self.root().join(path);
        fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
        fs::write(path, content).expect("a file");
        self
    }

    /// A repository holding `files`, committed once, with `origin` as its remote.
    fn repository(files: &[(&str, &str)], origin: Option<&str>) -> Self {
        let world = Self::new();
        world.git(&["init", "-q"]);
        world.git(&["config", "core.autocrlf", "false"]);
        world.git(&["config", "commit.gpgsign", "false"]);
        for (path, content) in files {
            world.write(path, content);
        }
        world.git(&["add", "-A"]);
        world.git(&["commit", "-q", "--no-verify", "-m", "fixture"]);
        if let Some(origin) = origin {
            world.git(&["remote", "add", "origin", origin]);
        }
        world
    }

    /// The built binary, run in `cwd` with `args`.
    fn pdx_in(&self, cwd: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_pdx"));
        self.isolated(&mut command).current_dir(cwd).args(args);
        for (key, value) in env {
            command.env(key, value);
        }
        command.output().expect("pdx runs")
    }

    /// The binary, run from a directory that is not the repository, naming it.
    fn pdx(&self, args: &[&str]) -> Output {
        let root = self.root().to_str().expect("a UTF-8 path").to_owned();
        let mut full: Vec<&str> = args.to_vec();
        if full.first() == Some(&"index") {
            full.insert(1, &root);
        }
        self.pdx_in(self.elsewhere.path(), &full, &[])
    }

    fn segment(&self) -> PathBuf {
        self.root().join(".pdx").join("segment.db")
    }
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("an exit code")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("UTF-8 output")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("UTF-8 output")
}

/// Stdout parsed as exactly one JSON value, and nothing else.
fn json(output: &Output) -> Value {
    let text = stdout(output);
    assert!(text.ends_with('\n'), "{text}");
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"))
}

/// A small Python service: two modules, calls between them, a test.
const SHOP: [(&str, &str); 5] = [
    (".gitignore", ".pdx/\n"),
    ("shop/__init__.py", ""),
    (
        "shop/orders.py",
        "from shop.prices import total\n\n\ndef checkout(items):\n    return total(items) + fee()\n\n\ndef fee():\n    return 1\n",
    ),
    (
        "shop/prices.py",
        "def total(items):\n    s = 0\n    for item in items:\n        s += item\n    return s\n",
    ),
    (
        "tests/test_orders.py",
        "from shop.orders import checkout\n\n\ndef test_checkout():\n    assert checkout([1, 2]) == 4\n",
    ),
];

// --- the three acceptance tests ---------------------------------------------------------

#[test]
fn cli_index_fixture() {
    let world = World::repository(&SHOP, Some(ORIGIN));
    let output = world.pdx(&["index", "--json"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    // Stdout is the report alone; stderr holds no progress when it is not a terminal.
    let report = json(&output);
    assert_eq!(stderr(&output), "");
    let text = stdout(&output);
    let root = world.root().to_string_lossy().into_owned();
    assert!(!text.contains(&root), "the checkout's location in {text}");
    assert!(!text.contains("pdx-cli-test-"), "{text}");
    // The segment is the repository's, where the report says.
    assert!(world.segment().is_file());
    assert_eq!(report["segment"]["output"], ".pdx/segment.db");
    // The identity is the URL's, and the report says so (issue 71).
    assert_eq!(report["repository"]["repo_id_source"], "url");
    insta::assert_snapshot!("cli_index_fixture", text);
}

#[test]
fn cli_exit_code_degraded() {
    // One file of five is over `max_file_bytes`: 20% skipped for size, over 4.5's 5%.
    let big = "x = 1\n".repeat(100);
    let world = World::repository(
        &[
            (".gitignore", ".pdx/\n"),
            ("pdx.toml", "[discover]\nmax_file_bytes = 256\n"),
            (
                "src/a.py",
                "def a():\n    return b()\n\n\ndef b():\n    return 1\n",
            ),
            ("src/c.py", "def c():\n    return 3\n"),
            ("data/big.py", big.as_str()),
        ],
        Some(ORIGIN),
    );
    let output = world.pdx(&["index", "--json"]);
    assert_eq!(code(&output), 2, "{}", stderr(&output));
    // A degraded build is a build: the segment and the whole report are there.
    assert!(world.segment().is_file());
    let report = json(&output);
    assert_eq!(report["status"], "degraded");
    assert_eq!(
        report["degraded_reasons"],
        serde_json::json!([{
            "kind": "failed_over_threshold",
            "failed": 0,
            "skipped_size": 1,
            "files": 5
        }])
    );
    // Without --json, the human report says so too, with the same exit code.
    let world_human = World::repository(
        &[
            (".gitignore", ".pdx/\n"),
            ("pdx.toml", "[discover]\nmax_file_bytes = 256\n"),
            ("src/a.py", "def a():\n    return 1\n"),
            ("data/big.py", big.as_str()),
        ],
        Some(ORIGIN),
    );
    let output = world_human.pdx(&["index"]);
    assert_eq!(code(&output), 2, "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("degraded"), "{text}");
    assert!(text.contains("failed + skipped(size)"), "{text}");
}

#[test]
fn cli_help_snapshot() {
    let world = World::new();
    let mut combined = String::new();
    for args in [
        &["--help"][..],
        &["index", "--help"][..],
        &["coverage", "--help"][..],
        &["version", "--help"][..],
    ] {
        let output = world.pdx_in(world.elsewhere.path(), args, &[]);
        assert_eq!(code(&output), 0, "{args:?}: {}", stderr(&output));
        assert_eq!(stderr(&output), "", "{args:?}");
        let _ = write!(combined, "$ pdx {}\n{}\n", args.join(" "), stdout(&output));
    }
    // The hidden worker and later tasks' commands are not offered.
    for absent in [
        "engine-worker",
        "serve",
        "mcp",
        "diff",
        "install",
        "bench",
        "precise",
    ] {
        assert!(!combined.contains(absent), "{absent} in help:\n{combined}");
    }
    insta::assert_snapshot!("cli_help_snapshot", combined);
}

// --- exit codes ---------------------------------------------------------------------------

#[test]
fn cli_index_ready_exits_zero_with_a_human_report() {
    let world = World::repository(&SHOP, Some(ORIGIN));
    let output = world.pdx(&["index"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(stderr(&output), "");
    let text = stdout(&output);
    for expected in [
        "ready",
        ".pdx/segment.db",
        "sha256",
        "files",
        "nodes",
        "call sites",
        "python",
    ] {
        assert!(text.contains(expected), "{expected} not in {text}");
    }
    assert!(!text.contains(&world.root().to_string_lossy().into_owned()));
    insta::assert_snapshot!("cli_index_human", text);
}

#[test]
fn cli_usage_errors_exit_one_never_two() {
    let world = World::repository(&SHOP, Some(ORIGIN));
    for args in [
        &["index", "--json", "report"][..],
        &["index", "--profile", "precise"][..],
        &["index", "--profile", "bogus"][..],
        &["index", "--no-such-flag"][..],
        &["no-such-command"][..],
        &["coverage", "--out", "x.db"][..],
        &["version", "extra"][..],
        &[][..],
    ] {
        let output = world.pdx(args);
        assert_eq!(code(&output), 1, "{args:?}: {}", stderr(&output));
        assert_eq!(stdout(&output), "", "{args:?}");
        assert!(
            stderr(&output).starts_with("pdx: "),
            "{args:?}: {}",
            stderr(&output)
        );
    }
    // No segment was made by any of them.
    assert!(!world.segment().exists());
}

#[test]
fn cli_runtime_errors_exit_one_with_nothing_on_stdout() {
    let world = World::repository(&SHOP, Some(ORIGIN));
    let fails = |args: &[&str], env: &[(&str, &str)], says: &str| {
        let root = world.root().to_str().expect("a UTF-8 path").to_owned();
        let mut full: Vec<&str> = vec!["index", root.as_str()];
        full.extend_from_slice(args);
        let output = world.pdx_in(world.elsewhere.path(), &full, env);
        assert_eq!(code(&output), 1, "{args:?} {env:?}: {}", stderr(&output));
        assert_eq!(stdout(&output), "", "{args:?} {env:?}");
        let err = stderr(&output);
        assert!(
            err.starts_with("pdx: ") && err.contains(says),
            "{says}: {err}"
        );
        assert_eq!(err.lines().count(), 1, "{err}");
    };
    fails(&[], &[("PDX_WORKERS", "0")], "PDX_WORKERS");
    fails(&[], &[("PDX_WORKERS", "two")], "PDX_WORKERS");
    fails(&[], &[("PDX_WORKERS", " 2")], "PDX_WORKERS");
    fails(&[], &[("PDX_WORKERS", "-1")], "PDX_WORKERS");
    fails(
        &[],
        &[("PDX_WORKERS", "99999999999999999999999")],
        "PDX_WORKERS",
    );
    fails(&[], &[("PDX_MEM_BUDGET_MB", "0")], "PDX_MEM_BUDGET_MB");
    fails(&[], &[("PDX_MEM_BUDGET_MB", "1.5")], "PDX_MEM_BUDGET_MB");
    fails(
        &[],
        &[("PDX_MEM_BUDGET_MB", "18446744073709551615")],
        "PDX_MEM_BUDGET_MB",
    );
    fails(&["--out", "missing/dir/x.db"], &[], "missing/dir/x.db");
    assert!(!world.segment().exists());
    // The destination is never overwritten.
    let output = world.pdx(&["index"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let before = fs::read(world.segment()).expect("the segment");
    fails(&[], &[], "already exists");
    assert_eq!(fs::read(world.segment()).expect("the segment"), before);
}

#[test]
fn cli_index_refuses_what_is_not_a_clean_commit() {
    // Not a repository at all.
    let world = World::new();
    world.write("a.py", "def a():\n    return 1\n");
    let output = world.pdx(&["index"]);
    assert_eq!(code(&output), 1);
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).contains("not a Git repository"),
        "{}",
        stderr(&output)
    );
    assert!(!world.segment().exists());

    // A modified tracked file, a staged file, an untracked file: dirty, refused, and
    // nothing written (not even `.pdx`).
    for change in ["modify", "stage", "untracked", "delete"] {
        let world = World::repository(&SHOP, Some(ORIGIN));
        match change {
            "modify" => {
                world.write("shop/prices.py", "def total(items):\n    return 0\n");
            }
            "stage" => {
                world.write("shop/new.py", "def new():\n    return 1\n");
                world.git(&["add", "shop/new.py"]);
            }
            "untracked" => {
                world.write("shop/extra.py", "def extra():\n    return 1\n");
            }
            _ => fs::remove_file(world.root().join("shop/prices.py")).expect("removed"),
        }
        let output = world.pdx(&["index"]);
        assert_eq!(code(&output), 1, "{change}: {}", stderr(&output));
        assert!(
            stderr(&output).contains("uncommitted"),
            "{change}: {}",
            stderr(&output)
        );
        assert!(!world.root().join(".pdx").exists(), "{change}");
    }

    // An ignored file is not a change.
    let world = World::repository(
        &[
            (".gitignore", ".pdx/\n*.log\n"),
            ("a.py", "def a():\n    return 1\n"),
        ],
        Some(ORIGIN),
    );
    world.write("debug.log", "noise\n");
    let output = world.pdx(&["index"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));

    // No commit yet.
    let world = World::new();
    world.git(&["init", "-q"]);
    world.git(&["remote", "add", "origin", ORIGIN]);
    let output = world.pdx(&["index"]);
    assert_eq!(code(&output), 1);
    assert!(stderr(&output).contains("no commit"), "{}", stderr(&output));
}

// --- identity -----------------------------------------------------------------------------

#[test]
fn cli_identity_comes_from_an_https_origin_only() {
    // No origin: no identity, never one made up.
    let world = World::repository(&SHOP, None);
    let output = world.pdx(&["index"]);
    assert_eq!(code(&output), 1);
    assert!(stderr(&output).contains("origin"), "{}", stderr(&output));

    // SSH, scp-like, git:// and http:// remotes are refused, never guessed into HTTPS,
    // and never echoed (a remote can hold credentials, assembled here at run time).
    let plain_http = format!(
        "{}://{}:{}@git.example/acme/shop.git",
        "http", "user", "hunter2"
    );
    for origin in [
        "git@git.example:acme/shop.git",
        "ssh://git@git.example/acme/shop.git",
        "git://git.example/acme/shop.git",
        plain_http.as_str(),
        "/srv/git/shop.git",
    ] {
        let world = World::repository(&SHOP, Some(origin));
        let output = world.pdx(&["index"]);
        assert_eq!(code(&output), 1, "{origin}");
        let err = stderr(&output);
        assert!(err.contains("HTTPS"), "{origin}: {err}");
        assert!(!err.contains("git.example"), "{origin} echoed: {err}");
        assert!(!err.contains("hunter2"), "{origin} echoed: {err}");
        assert!(!world.segment().exists());
    }

    // Credentials in an HTTPS origin never reach the report or the segment's meta.
    let world = World::repository(
        &SHOP,
        Some("https://x-access-token:s3cr3t@Git.Example/acme/shop.git"),
    );
    let output = world.pdx(&["index", "--json"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let report = json(&output);
    assert_eq!(
        report["repository"]["repo_url"],
        "https://git.example/acme/shop"
    );
    assert!(!stdout(&output).contains("s3cr3t"));
    let human = world.pdx_in(world.root(), &["coverage"], &[]);
    assert!(!stdout(&human).contains("s3cr3t"));
    // The same repository under the same URL is the same identity: the fixture's.
    let plain = World::repository(&SHOP, Some(ORIGIN));
    let plain_report = json(&plain.pdx(&["index", "--json"]));
    assert_eq!(
        report["repository"]["repo_id"],
        plain_report["repository"]["repo_id"]
    );
    assert_eq!(report["repository"]["name"], "shop");
}

#[test]
fn cli_never_derives_an_id_for_a_server_repository() {
    // With a server configured the registered id is the server's (4.2.1), which this
    // build cannot ask for: refused, before any token is looked at.
    let mut files = SHOP.to_vec();
    files.push((
        "pdx.toml",
        "[server]\nurl = \"https://api.example.com\"\ntoken_ref = \"env:PDX_TOKEN\"\n",
    ));
    let world = World::repository(&files, Some(ORIGIN));
    let output = world.pdx_in(
        world.elsewhere.path(),
        &["index", world.root().to_str().expect("UTF-8")],
        &[("PDX_TOKEN", "t0ken-never-read")],
    );
    assert_eq!(code(&output), 1, "{}", stderr(&output));
    let err = stderr(&output);
    assert!(err.contains("server"), "{err}");
    assert!(!err.contains("t0ken-never-read"), "{err}");
    assert!(!world.segment().exists());
}

// --- output -------------------------------------------------------------------------------

#[test]
fn cli_index_output_belongs_to_the_repository() {
    // Run from a directory inside the repository: the repository is indexed, and the
    // default output is at its root.
    let world = World::repository(&SHOP, Some(ORIGIN));
    let output = world.pdx_in(&world.root().join("shop"), &["index", "--json"], &[]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(world.segment().is_file());
    assert!(!world.root().join("shop/.pdx").exists());
    assert_eq!(json(&output)["files"]["files"], 5);

    // Run from elsewhere naming the repository: still the repository's root.
    let world = World::repository(&SHOP, Some(ORIGIN));
    let output = world.pdx(&["index"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(world.segment().is_file());
    assert!(!world.elsewhere.path().join(".pdx").exists());

    // A relative --out is relative to the repository root, whoever runs the command.
    let world = World::repository(
        &[
            (".gitignore", ".pdx/\nout/\n"),
            ("a.py", "def a():\n    return 1\n"),
        ],
        Some(ORIGIN),
    );
    fs::create_dir(world.root().join("out")).expect("a directory");
    let output = world.pdx(&["index", "--out", "out/index.db", "--json"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(world.root().join("out/index.db").is_file());
    assert_eq!(json(&output)["segment"]["output"], "out/index.db");
    assert!(!world.root().join(".pdx").exists());

    // An absolute --out is used as given, and reported as given.
    let world = World::repository(&SHOP, Some(ORIGIN));
    let target = world.elsewhere.path().join("abs.db");
    let target_text = target.to_str().expect("UTF-8").to_owned();
    let output = world.pdx(&["index", "--out", &target_text, "--json"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(target.is_file());
    assert_eq!(json(&output)["segment"]["output"], target_text.as_str());
}

/// The shop, with `.pdx` ignored whatever it is (`.pdx/` matches a directory only).
fn ignoring_pdx() -> Vec<(&'static str, &'static str)> {
    let mut files = SHOP.to_vec();
    files[0] = (".gitignore", ".pdx\n");
    files
}

#[cfg(unix)]
#[test]
fn cli_index_refuses_a_pdx_that_is_not_a_directory() {
    // A `.pdx` symlink (here to a directory elsewhere) or file is never written through.
    // (Ignored as `.pdx`, so the working tree is clean and the output is what refuses.)
    let world = World::repository(&ignoring_pdx(), Some(ORIGIN));
    std::os::unix::fs::symlink(world.elsewhere.path(), world.root().join(".pdx"))
        .expect("a symlink");
    let output = world.pdx(&["index"]);
    assert_eq!(code(&output), 1, "{}", stderr(&output));
    assert!(stderr(&output).contains(".pdx"), "{}", stderr(&output));
    assert_eq!(
        fs::read_dir(world.elsewhere.path()).expect("a dir").count(),
        0
    );
}

#[test]
fn cli_index_refuses_a_pdx_file() {
    let world = World::repository(&ignoring_pdx(), Some(ORIGIN));
    fs::write(world.root().join(".pdx"), "not a directory").expect("a file");
    let output = world.pdx(&["index"]);
    assert_eq!(code(&output), 1, "{}", stderr(&output));
    assert!(stderr(&output).contains(".pdx"), "{}", stderr(&output));
}

#[test]
fn cli_json_report_carries_no_machine_state() {
    let world = World::repository(&SHOP, Some(ORIGIN));
    let output = world.pdx(&["index", "--json"]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let report = json(&output);
    let mut keys = Vec::new();
    collect_keys(&report, &mut keys);
    for forbidden in [
        "workers",
        "threads",
        "effective_workers",
        "memory_budget",
        "per_worker_budget",
        "cache_path",
        "cache_hits",
        "peak_source_bytes",
        "host",
        "pid",
        "elapsed",
        "built_at",
    ] {
        assert!(
            !keys.iter().any(|k| k == forbidden),
            "{forbidden} in {keys:?}"
        );
    }
    // Two builds of one commit on different worker counts and budgets: the same report.
    let again = World::repository(&SHOP, Some(ORIGIN));
    let other = again.pdx_in(
        again.elsewhere.path(),
        &["index", again.root().to_str().expect("UTF-8"), "--json"],
        &[("PDX_WORKERS", "1"), ("PDX_MEM_BUDGET_MB", "8")],
    );
    assert_eq!(code(&other), 0, "{}", stderr(&other));
    assert_eq!(stdout(&other), stdout(&output));
}

fn collect_keys(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                out.push(k.clone());
                collect_keys(v, out);
            }
        }
        Value::Array(items) => {
            for v in items {
                collect_keys(v, out);
            }
        }
        _ => {}
    }
}

// --- version and coverage -----------------------------------------------------------------

#[test]
fn cli_version_prints_four_versions() {
    let world = World::new();
    let output = world.pdx_in(world.elsewhere.path(), &["version"], &[]);
    assert_eq!(code(&output), 0);
    // Written out here, not taken from the code under test.
    assert_eq!(stdout(&output), "pdx 0.1.0\nengine 3\nschema 3\nmatrix 2\n");
    assert_eq!(stderr(&output), "");
}

#[test]
fn cli_coverage_reads_the_repository_segment() {
    let world = World::repository(&SHOP, Some(ORIGIN));
    let indexed = world.pdx(&["index", "--json"]);
    assert_eq!(code(&indexed), 0, "{}", stderr(&indexed));
    let report = json(&indexed);

    // Human: every language row, every band named, from inside the repository.
    let output = world.pdx_in(&world.root().join("tests"), &["coverage"], &[]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let text = stdout(&output);
    for band in [
        "precise",
        "typed",
        "import-guided",
        "inheritance-guided",
        "exact",
        "scoped",
        "candidate",
        "external",
        "blocked",
        "unresolved",
        "contradicted",
    ] {
        assert!(text.contains(band), "{band} not in {text}");
    }
    for row in report["coverage"].as_array().expect("rows") {
        let language = row["language"].as_str().expect("a language");
        assert!(text.contains(language), "{language} not in {text}");
    }
    insta::assert_snapshot!("cli_coverage_human", text);

    // JSON: the segment's identity and the same rows as the index report, all bands.
    let output = world.pdx_in(world.root(), &["coverage", "--json"], &[]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let coverage = json(&output);
    assert_eq!(coverage["coverage"], report["coverage"]);
    assert_eq!(coverage["repo_id"], report["repository"]["repo_id"]);
    // The source the segment recorded, read back: the index report's, and `url`.
    assert_eq!(
        coverage["repo_id_source"],
        report["repository"]["repo_id_source"]
    );
    assert_eq!(coverage["repo_id_source"], "url");
    assert_eq!(coverage["commit_sha"], report["repository"]["commit_sha"]);
    assert_eq!(coverage["profile"], "structural");
    for row in coverage["coverage"].as_array().expect("rows") {
        assert_eq!(row["by_band"].as_object().expect("bands").len(), 11);
    }
    // Reading changes nothing: the segment's bytes are as written.
    assert_eq!(
        fs::read(world.segment()).expect("the segment").len() as u64,
        report["segment"]["size_bytes"].as_u64().expect("a size")
    );
}

#[test]
fn cli_coverage_errors_exit_one() {
    // No segment.
    let world = World::repository(&SHOP, Some(ORIGIN));
    let output = world.pdx_in(world.root(), &["coverage"], &[]);
    assert_eq!(code(&output), 1);
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).contains(".pdx/segment.db"),
        "{}",
        stderr(&output)
    );
    // Not a segment.
    fs::create_dir(world.root().join(".pdx")).expect("a directory");
    fs::write(world.segment(), "not a database").expect("a file");
    let output = world.pdx_in(world.root(), &["coverage", "--json"], &[]);
    assert_eq!(code(&output), 1);
    assert_eq!(stdout(&output), "");
    // Not a repository.
    let output = world.pdx_in(world.elsewhere.path(), &["coverage"], &[]);
    assert_eq!(code(&output), 1);
}
