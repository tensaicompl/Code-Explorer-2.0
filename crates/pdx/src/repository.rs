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
/// named by path.
const ROUTING: [&str; 7] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
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
    /// The repository has no commit yet.
    NoCommit,
    /// Git answered with something that is not what was asked for.
    UnexpectedOutput(&'static str),
    /// The working tree has changes `HEAD` does not hold.
    Dirty {
        /// Whether `.pdx` or a path under it is among them (a segment written there).
        pdx: bool,
    },
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
            Self::NoCommit => f.write_str("the repository has no commit to index yet"),
            Self::UnexpectedOutput(what) => write!(f, "git gave an unexpected {what}"),
            Self::Dirty { pdx } => {
                f.write_str(
                    "the working tree has uncommitted changes (modified, staged or untracked \
                     files); a segment is of a commit, so commit or remove them first",
                )?;
                if *pdx {
                    f.write_str(", and add .pdx/ to .gitignore")?;
                }
                Ok(())
            }
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
}

/// Runs `git -C <dir> <args>`: no shell, routing variables cleared, no prompt, no
/// optional locks, stdout bounded, stderr discarded.
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
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|_| RepositoryError::GitUnavailable)?;
    let mut stdout = Vec::new();
    let reader = child.stdout.take().ok_or(RepositoryError::GitUnavailable)?;
    let read = reader.take(LIMIT as u64 + 1).read_to_end(&mut stdout);
    let truncated = stdout.len() > LIMIT;
    if truncated {
        stdout.truncate(LIMIT);
        let _ = child.kill();
    }
    let status = child.wait().map_err(|_| RepositoryError::GitUnavailable)?;
    read.map_err(|_| RepositoryError::GitUnavailable)?;
    Ok(Answer {
        success: status.success(),
        stdout,
        truncated,
    })
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

/// Whether the working tree is exactly `HEAD`: nothing modified, deleted or staged, and
/// no untracked file Git does not ignore. Ignored files do not count.
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
            "--untracked-files=all",
            "--ignore-submodules=none",
        ],
    )?;
    if !answer.success {
        return Err(RepositoryError::UnexpectedOutput("status"));
    }
    if answer.stdout.is_empty() {
        return Ok(());
    }
    Err(RepositoryError::Dirty {
        pdx: mentions_pdx(&answer.stdout),
    })
}

/// Whether porcelain v1 `-z` output names `.pdx` or a path under it.
fn mentions_pdx(status: &[u8]) -> bool {
    status
        .split(|b| *b == 0)
        .map(|record| record.get(3..).unwrap_or(record))
        .any(|path| path == b".pdx" || path.starts_with(b".pdx/"))
}

/// `origin`'s fetch URL, as Git resolves it; `None` when there is no `origin`.
fn origin(root: &Path) -> Result<Option<String>, RepositoryError> {
    let answer = git(root, &["remote", "get-url", "origin"])?;
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

    #[test]
    fn porcelain_paths_under_pdx_are_noticed() {
        assert!(mentions_pdx(b"?? .pdx/segment.db\0"));
        assert!(mentions_pdx(b"?? .pdx\0"));
        assert!(mentions_pdx(b" M a.py\0?? .pdx/segment.db\0"));
        assert!(!mentions_pdx(b" M a.py\0?? b.py\0"));
        assert!(!mentions_pdx(b"?? src/.pdx/x\0"));
    }
}
