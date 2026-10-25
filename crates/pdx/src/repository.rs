//! The repository `pdx index` builds a segment of, from local Git metadata only:
//! its top level, its `HEAD` commit, whether its working tree is that commit, and its
//! identity (P2-11, decision 36).
//!
//! Git is run directly, never through a shell, with a fixed subcommand and the
//! repository path as a separate argument; the environment variables that would point
//! it at another repository are cleared, no prompt is allowed and no command contacts
//! a remote. Git's output is bounded and checked, and its error output is never shown:
//! a remote URL can carry credentials.
//!
//! The identity of a repository with no server (4.2.1): the HTTPS clone URL of the
//! remote named `origin`, made canonical by `pdx_core::ids::canonical_clone_url`, and
//! the `repo_id` derived from it. A remote that is not an HTTPS clone URL (SSH, the
//! `git@host:path` form, `git://`, `http://`, a path) is refused, never guessed into
//! one. A repository with a server configured has a registered identity this build
//! cannot ask for (P4-06), so none is derived for it.

use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use pdx_core::ids::{RepoId, canonical_clone_url, repo_id_from_url};

/// Environment variables that would make Git look at a repository other than the one
/// named by path, or read configuration the command line injected (a repository's
/// identity and cleanliness are its own, never an environment's).
const ROUTING: [&str; 10] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG",
];

/// The most output any metadata query may give.
const LIMIT: usize = 64 * 1024;

/// Why the repository could not be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepositoryError {
    /// `git` could not be run.
    GitUnavailable,
    /// The path is no directory inside a Git working tree.
    NotARepository(String),
    /// Git refuses the repository because another user owns it (`safe.directory`).
    NotSafe(String),
    /// The repository has no commit yet.
    NoCommit,
    /// Git answered with something that is not what was asked for.
    UnexpectedOutput(&'static str),
    /// The working tree has changes `HEAD` does not hold.
    Dirty,
    /// The repository has no remote named `origin`.
    NoOrigin,
    /// `origin` is not an HTTPS clone URL; says what kind it is, never what it is.
    NotHttps(&'static str),
    /// A server is configured, so the identity is the server's.
    ServerIdentity(&'static str),
}

impl fmt::Display for RepositoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GitUnavailable => f.write_str("git could not be run; `pdx index` needs it"),
            Self::NotARepository(path) => write!(f, "{path}: not a Git repository"),
            Self::NotSafe(path) => write!(
                f,
                "{path}: Git refuses a repository another user owns; add it to Git's \
                 safe.directory if it is trusted"
            ),
            Self::NoCommit => f.write_str("the repository has no commit to index yet"),
            Self::UnexpectedOutput(what) => write!(f, "git gave an unexpected {what}"),
            Self::Dirty => f.write_str(
                "the working tree is not exactly its commit: modified, staged or untracked \
                 files (only the repository's .gitignore files ignore one; \
                 .git/info/exclude and global excludes do not), or files Git was told not \
                 to check (skip-worktree, sparse checkout, assume-unchanged); a segment is \
                 of a commit, so commit or remove them first",
            ),
            Self::NoOrigin => f.write_str(
                "the repository has no remote named origin; its identity comes from \
                 origin's HTTPS clone URL",
            ),
            Self::NotHttps(kind) => write!(
                f,
                "origin is {kind}, not an HTTPS clone URL; the repository's identity comes \
                 from an HTTPS origin and is never guessed from another form"
            ),
            Self::ServerIdentity(source) => write!(
                f,
                "{source} configures a server, whose registered repository identity this \
                 build cannot obtain; no identity is derived in its place"
            ),
        }
    }
}

/// A repository's identity: what a segment's meta records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// The URL-derived `repo_id` (4.2.1, rule 5).
    pub repo_id: RepoId,
    /// The canonical clone URL: no credentials, no `.git`, the host lowercased.
    pub repo_url: String,
    /// The repository's name, shown on its node: the canonical URL's last path segment.
    pub name: String,
}

/// What a Git query gave.
struct Answer {
    success: bool,
    stdout: Vec<u8>,
    /// Whether there was more than [`LIMIT`] of it (only the first part is kept).
    truncated: bool,
    /// Whether Git said it refuses the repository's ownership (`safe.directory`).
    unsafe_ownership: bool,
}

/// Runs `git -C <dir> <args>`: no shell, routing variables cleared, no prompt, no
/// optional locks, stdout bounded, stderr read only to recognise Git's ownership
/// refusal and never shown.
fn git(dir: &Path, args: &[&str]) -> Result<Answer, RepositoryError> {
    let mut command = Command::new("git");
    command.arg("-C").arg(dir).args(args);
    for name in ROUTING {
        command.env_remove(name);
    }
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|_| RepositoryError::GitUnavailable)?;
    // Stderr is drained on its own thread (bounded) so a chatty Git never blocks on it.
    let err_reader = child.stderr.take();
    let err_thread = std::thread::spawn(move || {
        let mut text = Vec::new();
        if let Some(reader) = err_reader {
            let _ = reader.take(4096).read_to_end(&mut text);
        }
        text
    });
    let mut stdout = Vec::new();
    let reader = child.stdout.take().ok_or(RepositoryError::GitUnavailable)?;
    let read = reader.take(LIMIT as u64 + 1).read_to_end(&mut stdout);
    let truncated = stdout.len() > LIMIT;
    if truncated {
        stdout.truncate(LIMIT);
        let _ = child.kill();
    }
    let status = child.wait().map_err(|_| RepositoryError::GitUnavailable)?;
    let stderr = err_thread.join().unwrap_or_default();
    if !truncated {
        read.map_err(|_| RepositoryError::GitUnavailable)?;
    }
    Ok(Answer {
        success: status.success(),
        stdout,
        truncated,
        unsafe_ownership: String::from_utf8_lossy(&stderr).contains("dubious ownership"),
    })
}

/// Whether any NUL-terminated record `git -C <dir> <args>` writes satisfies `pred`,
/// reading its output as a stream (a large repository's listing is never held whole).
fn git_any_record(
    dir: &Path,
    args: &[&str],
    pred: impl Fn(&[u8]) -> bool,
) -> Result<bool, RepositoryError> {
    let mut command = Command::new("git");
    command.arg("-C").arg(dir).args(args);
    for name in ROUTING {
        command.env_remove(name);
    }
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|_| RepositoryError::GitUnavailable)?;
    let mut reader =
        std::io::BufReader::new(child.stdout.take().ok_or(RepositoryError::GitUnavailable)?);
    let mut record = Vec::new();
    let mut found = false;
    loop {
        record.clear();
        let n = std::io::BufRead::read_until(&mut reader, 0, &mut record)
            .map_err(|_| RepositoryError::GitUnavailable)?;
        if n == 0 {
            break;
        }
        if record.last() == Some(&0) {
            record.pop();
        }
        if pred(&record) {
            found = true;
            let _ = child.kill();
            break;
        }
    }
    let status = child.wait().map_err(|_| RepositoryError::GitUnavailable)?;
    if !found && !status.success() {
        return Err(RepositoryError::UnexpectedOutput("file listing"));
    }
    Ok(found)
}

/// One line of output as text: UTF-8, no control character, the final newline gone.
fn line(answer: &Answer, what: &'static str) -> Result<String, RepositoryError> {
    if answer.truncated {
        return Err(RepositoryError::UnexpectedOutput(what));
    }
    let text =
        std::str::from_utf8(&answer.stdout).map_err(|_| RepositoryError::UnexpectedOutput(what))?;
    let text = text.strip_suffix('\n').unwrap_or(text);
    let text = text.strip_suffix('\r').unwrap_or(text);
    if text.is_empty() || text.chars().any(char::is_control) {
        return Err(RepositoryError::UnexpectedOutput(what));
    }
    Ok(text.to_owned())
}

/// The top level of the working tree `path` is in.
///
/// # Errors
///
/// [`RepositoryError::NotARepository`] when it is in none; others when Git fails.
pub fn toplevel(path: &Path) -> Result<PathBuf, RepositoryError> {
    let shown = path.display().to_string();
    if !path.is_dir() {
        return Err(RepositoryError::NotARepository(shown));
    }
    let answer = git(path, &["rev-parse", "--show-toplevel"])?;
    if !answer.success {
        if answer.unsafe_ownership {
            return Err(RepositoryError::NotSafe(shown));
        }
        return Err(RepositoryError::NotARepository(shown));
    }
    Ok(PathBuf::from(line(&answer, "top-level directory")?))
}

/// The commit `HEAD` is: 40 or 64 lower-case hex characters.
///
/// # Errors
///
/// [`RepositoryError::NoCommit`] for a repository with none.
pub fn head_commit(root: &Path) -> Result<String, RepositoryError> {
    let answer = git(root, &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"])?;
    if !answer.success {
        return Err(RepositoryError::NoCommit);
    }
    let sha = line(&answer, "commit")?;
    let hex = sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if !hex || !matches!(sha.len(), 40 | 64) {
        return Err(RepositoryError::UnexpectedOutput("commit"));
    }
    Ok(sha)
}

/// Whether the working tree is exactly `HEAD`, as discovery will read it: nothing
/// modified, deleted or staged; no untracked file the repository's `.gitignore` files do
/// not ignore (discovery reads no other ignore file, so a file hidden only by
/// `.git/info/exclude` or the user's global excludes would be indexed, and counts); and
/// no tracked file Git has been told not to look at (skip-worktree, a sparse checkout's
/// absent files, assume-unchanged), whose bytes may not be HEAD's. `.pdx`, where PDX
/// itself writes, never counts (issue 99).
///
/// # Errors
///
/// [`RepositoryError::Dirty`] when it is not.
pub fn require_clean(root: &Path) -> Result<(), RepositoryError> {
    let answer = git(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=no",
            "--ignore-submodules=none",
            "--",
            ".",
            ":(exclude,top).pdx",
        ],
    )?;
    // More than the bound is certainly a change; only an empty answer is clean.
    if answer.truncated || (answer.success && !answer.stdout.is_empty()) {
        return Err(RepositoryError::Dirty);
    }
    if !answer.success {
        return Err(RepositoryError::UnexpectedOutput("status"));
    }
    let untracked = git_any_record(
        root,
        &[
            "ls-files",
            "-z",
            "--others",
            "--exclude-per-directory=.gitignore",
            "--",
            ".",
            ":(exclude,top).pdx",
        ],
        |_| true,
    )?;
    // `ls-files -v` tags an assume-unchanged entry in lower case and a skip-worktree
    // one `S`.
    let hidden = git_any_record(
        root,
        &["ls-files", "-z", "-v", "--", ".", ":(exclude,top).pdx"],
        |record| {
            record
                .first()
                .is_some_and(|t| t.is_ascii_lowercase() || *t == b'S')
        },
    )?;
    if untracked || hidden {
        return Err(RepositoryError::Dirty);
    }
    Ok(())
}

/// `origin`'s URL exactly as configured; `None` when there is no `origin`. Read from
/// the configuration, never through `url.<base>.insteadOf`: a user's rewrite rule (an
/// HTTPS host fetched over SSH, say) must not change the repository's identity.
fn origin(root: &Path) -> Result<Option<String>, RepositoryError> {
    let answer = git(root, &["config", "--get", "remote.origin.url"])?;
    if !answer.success {
        return Ok(None);
    }
    line(&answer, "origin URL").map(Some)
}

/// What kind of remote a URL that is not an HTTPS clone URL is, to say so without
/// repeating it.
pub fn remote_kind(url: &str) -> &'static str {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("ssh://") || lower.starts_with("git+ssh://") {
        "an SSH URL"
    } else if lower.starts_with("git://") {
        "a git:// URL"
    } else if lower.starts_with("http://") {
        "an http:// URL"
    } else if lower.starts_with("file://") || url.starts_with('/') || url.starts_with('.') {
        "a local path"
    } else if lower.starts_with("https://") {
        "an https:// URL without a host and path"
    } else if !url.contains("://") && url.contains(':') {
        "an SSH remote (host:path)"
    } else {
        "not a URL Git clones over HTTPS"
    }
}

/// The identity of a repository whose `origin` is `url`.
///
/// # Errors
///
/// [`RepositoryError::NotHttps`] for anything but an HTTPS clone URL.
pub fn identity_from_origin(url: &str) -> Result<Identity, RepositoryError> {
    let repo_url =
        canonical_clone_url(url).map_err(|_| RepositoryError::NotHttps(remote_kind(url)))?;
    let repo_id =
        repo_id_from_url(&repo_url).map_err(|_| RepositoryError::NotHttps(remote_kind(url)))?;
    let name = repo_name(&repo_url).ok_or(RepositoryError::NotHttps(remote_kind(url)))?;
    Ok(Identity {
        repo_id,
        repo_url,
        name,
    })
}

/// A canonical clone URL's last non-empty path segment: the repository's name.
pub fn repo_name(canonical: &str) -> Option<String> {
    let rest = canonical.strip_prefix("https://")?;
    let (_, path) = rest.split_once('/')?;
    path.split('/')
        .rev()
        .find(|segment| !segment.is_empty())
        .map(str::to_owned)
}

/// The repository's identity, read from Git, unless a server owns it.
///
/// # Errors
///
/// [`RepositoryError::ServerIdentity`] when a server is configured or its identity is
/// cached; [`RepositoryError::NoOrigin`] or [`RepositoryError::NotHttps`] otherwise.
pub fn identity(root: &Path, server_configured: bool) -> Result<Identity, RepositoryError> {
    if server_configured {
        return Err(RepositoryError::ServerIdentity("pdx.toml [server]"));
    }
    if root.join(".pdx").join("repo.toml").exists() {
        return Err(RepositoryError::ServerIdentity(".pdx/repo.toml"));
    }
    let url = origin(root)?.ok_or(RepositoryError::NoOrigin)?;
    identity_from_origin(&url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_https_origin_gives_a_canonical_identity() {
        let id = identity_from_origin("https://git.example/acme/shop.git").expect("an identity");
        assert_eq!(id.repo_url, "https://git.example/acme/shop");
        assert_eq!(id.name, "shop");
        // The repo_id is the URL's, as 4.2.1 rule 5 derives it.
        assert_eq!(
            id.repo_id,
            repo_id_from_url("https://git.example/acme/shop").expect("an id")
        );
        // Credentials, case and `.git` make no other identity.
        let same = identity_from_origin("HTTPS://x-access-token:s3cr3t@Git.Example/acme/shop.git")
            .expect("an identity");
        assert_eq!(same, id);
        assert!(!same.repo_url.contains("s3cr3t"));
    }

    #[test]
    fn the_name_is_the_urls_last_segment_never_the_checkouts() {
        assert_eq!(
            repo_name("https://git.example/acme/shop").as_deref(),
            Some("shop")
        );
        assert_eq!(
            repo_name("https://git.example/a/b/c/").as_deref(),
            Some("c")
        );
        assert_eq!(
            repo_name("https://git.example:8443/team/svc").as_deref(),
            Some("svc")
        );
        assert_eq!(repo_name("https://git.example/").as_deref(), None);
    }

    #[test]
    fn other_remotes_are_refused_and_never_repeated() {
        let http = format!("{}://{}:{}@git.example/shop.git", "http", "user", "hunter2");
        for (url, kind) in [
            ("git@git.example:acme/shop.git", "an SSH remote (host:path)"),
            ("ssh://git@git.example/acme/shop.git", "an SSH URL"),
            ("git://git.example/acme/shop.git", "a git:// URL"),
            (http.as_str(), "an http:// URL"),
            ("/srv/git/shop.git", "a local path"),
            ("../shop", "a local path"),
            ("file:///srv/git/shop.git", "a local path"),
        ] {
            let error = identity_from_origin(url).expect_err(url);
            assert_eq!(error, RepositoryError::NotHttps(kind), "{url}");
            let message = error.to_string();
            assert!(!message.contains("git.example"), "{message}");
            assert!(!message.contains("hunter2"), "{message}");
        }
    }
}
