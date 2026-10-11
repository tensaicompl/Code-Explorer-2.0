//! Fetching a golden repository at its pinned commit into a disposable checkout.
//!
//! Git runs as a process with fixed arguments, never through a shell, and:
//!
//! - never prompts (`GIT_TERMINAL_PROMPT=0`): a public fetch succeeds or fails;
//! - reads no global or system configuration (`GIT_CONFIG_GLOBAL` an empty file of
//!   the harness's, `GIT_CONFIG_NOSYSTEM=1`), so the user's line endings, filters,
//!   aliases, hooks and credential helpers decide nothing;
//! - runs no hook: `core.hooksPath` is an empty directory of the harness's on every
//!   command, whatever the checkout's own configuration says;
//! - never touches submodules (`submodule.recurse=false`, and no submodule command);
//! - is free of the variables that would point it at another repository.
//!
//! Nothing in a fetched repository is ever executed: no hook, build, package manager
//! or script. The checkout is the pinned commit, detached, with LF line endings
//! (`core.autocrlf=false`, `core.eol=lf`), verified and clean before it is indexed.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::cache::{self, CacheError};

/// Environment variables that point Git at another repository, index or object store;
/// cleared for every command.
const ROUTING: [&str; 7] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
];

/// The most of a failing command's standard error a message carries.
const STDERR_LIMIT: usize = 512;

/// Why a checkout could not be prepared.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    /// A cache path is unsafe.
    #[error(transparent)]
    Cache(#[from] CacheError),
    /// Git could not be started.
    #[error("cannot run git: {0}")]
    Spawn(std::io::Error),
    /// A Git command failed.
    #[error("git {command} failed in {dir}: {stderr}")]
    Git {
        /// The subcommand.
        command: String,
        /// Where.
        dir: PathBuf,
        /// The start of its standard error.
        stderr: String,
    },
    /// The cached checkout is not one the harness made for this repository.
    #[error(
        "{dir} is not this golden's checkout ({reason}); it is disposable: delete it and run again"
    )]
    ForeignCheckout {
        /// The checkout.
        dir: PathBuf,
        /// Why.
        reason: &'static str,
    },
    /// The pinned object is not a commit.
    #[error("{0} is not a commit in the fetched repository")]
    NotACommit(String),
    /// After checkout, `HEAD` is not the pin.
    #[error("the checkout's HEAD is {found}, not the pinned {pinned}")]
    WrongHead {
        /// The pin.
        pinned: String,
        /// What `HEAD` is.
        found: String,
    },
    /// The checkout is not clean after preparation.
    #[error("{0} is not clean after checkout")]
    Dirty(PathBuf),
}

/// How the harness runs Git.
#[derive(Clone, Debug)]
pub struct Git {
    hooks: PathBuf,
    global: PathBuf,
}

impl Git {
    /// Git with hooks pointed at `hooks` (an empty directory) and `global` (an empty
    /// file) as its global configuration.
    pub fn new(hooks: PathBuf, global: PathBuf) -> Self {
        Self { hooks, global }
    }

    /// The environment every Git the harness starts runs with, `pdx index`'s own
    /// included: no prompt, the empty global configuration, no system configuration.
    pub fn environment(&self) -> [(&'static str, OsString); 3] {
        [
            ("GIT_TERMINAL_PROMPT", OsString::from("0")),
            ("GIT_CONFIG_GLOBAL", self.global.clone().into_os_string()),
            ("GIT_CONFIG_NOSYSTEM", OsString::from("1")),
        ]
    }

    /// A Git command in `dir` with the harness's environment and fixed settings.
    pub fn command(&self, dir: &Path) -> Command {
        let mut hooks = OsString::from("core.hooksPath=");
        hooks.push(&self.hooks);
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(dir)
            .arg("-c")
            .arg(hooks)
            .args(["-c", "submodule.recurse=false"])
            .args(["-c", "advice.detachedHead=false"])
            .stdin(Stdio::null());
        for name in ROUTING {
            command.env_remove(name);
        }
        for (name, value) in self.environment() {
            command.env(name, value);
        }
        command
    }

    /// Runs `git <args>` in `dir`; its standard output on success.
    fn run(&self, dir: &Path, args: &[&str]) -> Result<Vec<u8>, FetchError> {
        let output = self
            .command(dir)
            .args(args)
            .output()
            .map_err(FetchError::Spawn)?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr: String = stderr.chars().take(STDERR_LIMIT).collect();
            Err(FetchError::Git {
                command: args.first().copied().unwrap_or_default().to_owned(),
                dir: dir.to_path_buf(),
                stderr: stderr.trim().to_owned(),
            })
        }
    }

    /// Whether `git <args>` in `dir` succeeds; its failure is an answer, not an error.
    fn succeeds(&self, dir: &Path, args: &[&str]) -> Result<bool, FetchError> {
        let status = self
            .command(dir)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(FetchError::Spawn)?;
        Ok(status.success())
    }

    fn text(&self, dir: &Path, args: &[&str]) -> Result<String, FetchError> {
        Ok(String::from_utf8_lossy(&self.run(dir, args)?)
            .trim()
            .to_owned())
    }
}

/// What preparing a checkout did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prepared {
    /// The pinned commit was already in the checkout; nothing was fetched.
    Reused,
    /// The pinned commit was fetched.
    Fetched,
}

/// Makes `dir` the checkout of `commit` from `remote`: a Git repository the harness
/// owns, whose `origin` is exactly `remote`, at `commit` detached, clean, with LF line
/// endings. The commit is fetched only when the checkout lacks it, by its id alone,
/// shallow and without tags: never a branch or a tag that might since have moved.
///
/// `dir` must be a real directory or absent, its parent a real directory. A checkout
/// whose `origin` is another remote is refused, never repointed: it is disposable,
/// and deleting it is the user's explicit act.
///
/// # Errors
///
/// [`FetchError`] for an unsafe path, a foreign checkout, a failed Git command, a pin
/// that is no commit, or a checkout that is not exactly the pin afterwards.
pub fn prepare_checkout(
    git: &Git,
    dir: &Path,
    remote: &str,
    commit: &str,
) -> Result<Prepared, FetchError> {
    if cache::is_real_dir(dir)? {
        let dot_git = dir.join(".git");
        if !cache::is_real_dir(&dot_git).map_err(|_| FetchError::ForeignCheckout {
            dir: dir.to_path_buf(),
            reason: "its .git is not a directory",
        })? {
            if std::fs::read_dir(dir)
                .map_err(|source| {
                    FetchError::Cache(CacheError::Io {
                        path: dir.to_path_buf(),
                        source,
                    })
                })?
                .next()
                .is_some()
            {
                return Err(FetchError::ForeignCheckout {
                    dir: dir.to_path_buf(),
                    reason: "not a Git repository",
                });
            }
            git.run(dir, &["init", "--quiet"])?;
        }
    } else {
        cache::ensure_dir(dir)?;
        git.run(dir, &["init", "--quiet"])?;
    }
    if git.text(dir, &["rev-parse", "--git-dir"])? != ".git" {
        return Err(FetchError::ForeignCheckout {
            dir: dir.to_path_buf(),
            reason: "it belongs to another repository",
        });
    }
    if git.succeeds(dir, &["remote", "get-url", "origin"])? {
        if git.text(dir, &["remote", "get-url", "origin"])? != remote {
            return Err(FetchError::ForeignCheckout {
                dir: dir.to_path_buf(),
                reason: "its origin is another remote",
            });
        }
    } else {
        git.run(dir, &["remote", "add", "origin", remote])?;
    }
    git.run(dir, &["config", "core.autocrlf", "false"])?;
    git.run(dir, &["config", "core.eol", "lf"])?;

    let object = format!("{commit}^{{commit}}");
    let prepared = if git.succeeds(dir, &["cat-file", "-e", &object])? {
        Prepared::Reused
    } else {
        git.run(
            dir,
            &[
                "fetch",
                "--quiet",
                "--no-tags",
                "--depth=1",
                "origin",
                commit,
            ],
        )?;
        Prepared::Fetched
    };
    if git.text(dir, &["cat-file", "-t", commit])? != "commit" {
        return Err(FetchError::NotACommit(commit.to_owned()));
    }
    git.run(dir, &["checkout", "--quiet", "--detach", "--force", commit])?;
    git.run(dir, &["reset", "--quiet", "--hard", commit])?;
    git.run(dir, &["clean", "-ffdxq"])?;
    let head = git.text(dir, &["rev-parse", "HEAD"])?;
    if head != commit {
        return Err(FetchError::WrongHead {
            pinned: commit.to_owned(),
            found: head,
        });
    }
    if !git.run(dir, &["status", "--porcelain"])?.is_empty() {
        return Err(FetchError::Dirty(dir.to_path_buf()));
    }
    Ok(prepared)
}
