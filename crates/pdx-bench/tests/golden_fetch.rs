//! Preparing a pinned checkout, against local repositories: never the network. Every
//! fixture is made with a fixed author, committer and date, and with the user's and
//! the system's Git configuration shut out.

use std::path::{Path, PathBuf};
use std::process::Command;

use pdx_bench::golden::cache::Cache;
use pdx_bench::golden::git::{FetchError, Git, Prepared, prepare_checkout};

/// A scratch area: a source repository's home, and the harness's cache.
struct Scratch {
    dir: tempfile::TempDir,
    global: PathBuf,
}

impl Scratch {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("a directory");
        let global = dir.path().join("test-gitconfig");
        std::fs::write(&global, "").expect("an empty configuration");
        Self { dir, global }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// Git in `dir`, as a fixture author: fixed identity and dates, no user settings.
    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", &self.global)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "PDX Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@git.example")
            .env("GIT_COMMITTER_NAME", "PDX Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@git.example")
            .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
            .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    /// A source repository at `name` on branch `main`.
    fn source(&self, name: &str) -> PathBuf {
        let dir = self.path(name);
        std::fs::create_dir(&dir).expect("a directory");
        self.git(&dir, &["-c", "init.defaultBranch=main", "init", "--quiet"]);
        self.git(&dir, &["config", "core.autocrlf", "false"]);
        // Serve any commit by id, as public hosts do.
        self.git(&dir, &["config", "uploadpack.allowAnySHA1InWant", "true"]);
        dir
    }

    /// Commits `files` to the repository at `dir`; the new commit's id.
    fn commit(&self, dir: &Path, files: &[(&str, &[u8])], message: &str) -> String {
        for (path, bytes) in files {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
            std::fs::write(path, bytes).expect("a file");
        }
        self.git(dir, &["add", "--all"]);
        self.git(dir, &["commit", "--quiet", "-m", message]);
        self.git(dir, &["rev-parse", "HEAD"])
    }

    /// The harness's Git, with its hooks directory and empty global configuration in
    /// a cache of this scratch area.
    fn harness(&self) -> (Cache, Git) {
        let cache = Cache::open(&self.path("cache")).expect("a cache");
        let git = Git::new(
            cache.no_hooks().expect("hooks"),
            cache.git_global().expect("a global configuration"),
        );
        (cache, git)
    }
}

/// A `file://` URL for a local repository.
fn file_url(dir: &Path) -> String {
    let text = dir.to_string_lossy().replace('\\', "/");
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}

#[test]
fn fetch_checks_out_the_exact_pinned_commit() {
    let s = Scratch::new();
    let source = s.source("source");
    let pinned = s.commit(&source, &[("src/a.py", b"VALUE = 1\n")], "pinned");
    let later = s.commit(&source, &[("src/a.py", b"VALUE = 2\n")], "later");
    assert_ne!(pinned, later);
    let (_cache, git) = s.harness();
    let checkout = s.path("checkout");

    let prepared =
        prepare_checkout(&git, &checkout, &file_url(&source), &pinned).expect("prepared");
    assert_eq!(prepared, Prepared::Fetched);
    // The pin, never the branch head that has moved past it; detached; clean.
    assert_eq!(s.git(&checkout, &["rev-parse", "HEAD"]), pinned);
    assert_eq!(
        std::fs::read(checkout.join("src/a.py")).expect("the file"),
        b"VALUE = 1\n"
    );
    let symbolic = Command::new("git")
        .arg("-C")
        .arg(&checkout)
        .args(["symbolic-ref", "-q", "HEAD"])
        .output()
        .expect("git runs");
    assert!(!symbolic.status.success(), "HEAD is detached");
    assert_eq!(s.git(&checkout, &["status", "--porcelain"]), "");

    // A second run reuses the commit already there: the remote is not needed at all.
    std::fs::rename(&source, s.path("gone")).expect("the remote goes away");
    std::fs::write(checkout.join("src/a.py"), b"tampered\n").expect("a change");
    std::fs::write(checkout.join("stray.txt"), b"left behind\n").expect("a stray file");
    let prepared = prepare_checkout(&git, &checkout, &file_url(&source), &pinned).expect("reused");
    assert_eq!(prepared, Prepared::Reused);
    assert_eq!(
        std::fs::read(checkout.join("src/a.py")).expect("the file"),
        b"VALUE = 1\n"
    );
    assert!(
        !checkout.join("stray.txt").exists(),
        "the cache checkout is cleaned"
    );
    assert_eq!(s.git(&checkout, &["status", "--porcelain"]), "");
}

#[test]
fn fetch_keeps_lf_line_endings() {
    let s = Scratch::new();
    let source = s.source("source");
    let text = b"line one\nline two\n";
    let pinned = s.commit(&source, &[("notes.txt", text)], "lf");
    let (_cache, git) = s.harness();
    let checkout = s.path("checkout");
    prepare_checkout(&git, &checkout, &file_url(&source), &pinned).expect("prepared");
    assert_eq!(
        std::fs::read(checkout.join("notes.txt")).expect("the file"),
        text
    );

    // Even when the checkout's own configuration has been set to convert, the next
    // preparation checks out LF: the harness sets the line-ending policy every time.
    s.git(&checkout, &["config", "core.autocrlf", "true"]);
    std::fs::remove_file(checkout.join("notes.txt")).expect("removed");
    prepare_checkout(&git, &checkout, &file_url(&source), &pinned).expect("prepared");
    let bytes = std::fs::read(checkout.join("notes.txt")).expect("the file");
    assert_eq!(bytes, text);
    assert!(!bytes.contains(&b'\r'));
}

#[test]
fn fetch_refuses_a_cache_with_another_origin() {
    let s = Scratch::new();
    let source = s.source("source");
    let pinned = s.commit(&source, &[("a.txt", b"a\n")], "a");
    let (_cache, git) = s.harness();
    let checkout = s.path("checkout");
    prepare_checkout(&git, &checkout, &file_url(&source), &pinned).expect("prepared");
    s.git(
        &checkout,
        &[
            "remote",
            "set-url",
            "origin",
            &file_url(&s.path("elsewhere")),
        ],
    );
    match prepare_checkout(&git, &checkout, &file_url(&source), &pinned) {
        Err(e @ FetchError::ForeignCheckout { .. }) => {
            assert!(e.to_string().contains("delete it"), "{e}");
        }
        other => panic!("expected a refused checkout, got {other:?}"),
    }
}

#[test]
fn fetch_refuses_what_is_not_its_checkout() {
    let s = Scratch::new();
    let source = s.source("source");
    let pinned = s.commit(&source, &[("a.txt", b"a\n")], "a");
    let (_cache, git) = s.harness();
    // A directory with content and no repository.
    let stray = s.path("stray");
    std::fs::create_dir(&stray).expect("a directory");
    std::fs::write(stray.join("keep.txt"), b"mine\n").expect("a file");
    assert!(matches!(
        prepare_checkout(&git, &stray, &file_url(&source), &pinned),
        Err(FetchError::ForeignCheckout { .. })
    ));
    assert!(stray.join("keep.txt").exists());
    // A `.git` file pointing at another repository's Git directory.
    let redirected = s.path("redirected");
    std::fs::create_dir(&redirected).expect("a directory");
    std::fs::write(redirected.join(".git"), b"gitdir: /elsewhere\n").expect("a file");
    assert!(matches!(
        prepare_checkout(&git, &redirected, &file_url(&source), &pinned),
        Err(FetchError::ForeignCheckout { .. })
    ));
}

#[test]
fn fetch_of_a_commit_the_remote_lacks_fails() {
    let s = Scratch::new();
    let source = s.source("source");
    s.commit(&source, &[("a.txt", b"a\n")], "a");
    let (_cache, git) = s.harness();
    let checkout = s.path("checkout");
    let missing = "0123456789abcdef0123456789abcdef01234567";
    assert!(prepare_checkout(&git, &checkout, &file_url(&source), missing).is_err());
    // Nothing was checked out in its place.
    assert!(!checkout.join("a.txt").exists());
}

#[test]
fn fetch_never_initialises_submodules() {
    let s = Scratch::new();
    let sub = s.source("sub");
    let sub_commit = s.commit(&sub, &[("from_submodule.txt", b"elsewhere\n")], "sub");
    let source = s.source("source");
    std::fs::write(
        source.join(".gitmodules"),
        format!(
            "[submodule \"vendor/sub\"]\n\tpath = vendor/sub\n\turl = {}\n",
            file_url(&sub)
        ),
    )
    .expect(".gitmodules");
    s.git(
        &source,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{sub_commit},vendor/sub"),
        ],
    );
    let pinned = s.commit(&source, &[("main.txt", b"main\n")], "with a gitlink");
    let (_cache, git) = s.harness();
    let checkout = s.path("checkout");
    prepare_checkout(&git, &checkout, &file_url(&source), &pinned).expect("prepared");
    assert!(checkout.join("main.txt").exists());
    let gitlink = checkout.join("vendor/sub");
    let entries = std::fs::read_dir(&gitlink).map_or(0, Iterator::count);
    assert_eq!(entries, 0, "the submodule's content is never fetched");
    assert_eq!(s.git(&checkout, &["status", "--porcelain"]), "");
}

#[test]
fn git_commands_never_prompt_or_read_user_settings() {
    let s = Scratch::new();
    let (cache, git) = s.harness();
    let command = git.command(&s.path("x"));
    let args: Vec<String> = command
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let hooks = cache.no_hooks().expect("hooks");
    assert!(
        args.contains(&format!("core.hooksPath={}", hooks.display())),
        "{args:?}"
    );
    assert!(
        args.contains(&"submodule.recurse=false".to_owned()),
        "{args:?}"
    );
    let envs: Vec<(String, Option<String>)> = command
        .get_envs()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.map(|v| v.to_string_lossy().into_owned()),
            )
        })
        .collect();
    let value = |name: &str| envs.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    assert_eq!(value("GIT_TERMINAL_PROMPT"), Some(Some("0".to_owned())));
    assert_eq!(value("GIT_CONFIG_NOSYSTEM"), Some(Some("1".to_owned())));
    assert_eq!(
        value("GIT_CONFIG_GLOBAL"),
        Some(Some(
            cache
                .root()
                .join("git-global")
                .to_string_lossy()
                .into_owned()
        ))
    );
    assert_eq!(
        value("GIT_DIR"),
        Some(None),
        "routing variables are cleared"
    );
    assert_eq!(
        std::fs::read(cache.root().join("git-global")).expect("the file"),
        b""
    );
}

#[cfg(unix)]
#[test]
fn fetch_runs_no_hook() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new();
    let source = s.source("source");
    let pinned = s.commit(&source, &[("a.txt", b"a\n")], "a");
    let (_cache, git) = s.harness();
    let checkout = s.path("checkout");
    prepare_checkout(&git, &checkout, &file_url(&source), &pinned).expect("prepared");

    // A hook planted in the cached checkout, which plain Git would run on checkout.
    let sentinel = s.path("hook-ran");
    let hook = checkout.join(".git/hooks/post-checkout");
    std::fs::write(
        &hook,
        format!("#!/bin/sh\ntouch '{}'\n", sentinel.display()),
    )
    .expect("a hook");
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).expect("executable");
    s.git(&checkout, &["checkout", "--quiet", "--detach", &pinned]);
    assert!(sentinel.exists(), "the planted hook works for plain Git");
    std::fs::remove_file(&sentinel).expect("reset");

    prepare_checkout(&git, &checkout, &file_url(&source), &pinned).expect("prepared");
    assert!(!sentinel.exists(), "the harness ran a repository hook");
}

#[cfg(unix)]
#[test]
fn fetch_refuses_a_symlinked_checkout() {
    let s = Scratch::new();
    let source = s.source("source");
    let pinned = s.commit(&source, &[("a.txt", b"a\n")], "a");
    let (_cache, git) = s.harness();
    // Someone else's directory, with a file of theirs, behind a link where the
    // checkout belongs.
    let theirs = s.path("theirs");
    std::fs::create_dir(&theirs).expect("a directory");
    std::fs::write(theirs.join("precious.txt"), b"keep\n").expect("a file");
    let checkout = s.path("checkout");
    std::os::unix::fs::symlink(&theirs, &checkout).expect("a link");
    match prepare_checkout(&git, &checkout, &file_url(&source), &pinned) {
        Err(FetchError::Cache(_)) => {}
        other => panic!("expected a refused link, got {other:?}"),
    }
    assert_eq!(
        std::fs::read(theirs.join("precious.txt")).expect("kept"),
        b"keep\n"
    );
    assert!(
        !theirs.join(".git").exists(),
        "nothing was made through the link"
    );
}
