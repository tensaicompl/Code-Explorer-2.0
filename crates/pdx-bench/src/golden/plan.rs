//! Which repositories a golden run processes, and where their files go: the
//! `[[golden]]` entries of `bench/repos.lock`, checked before anything is fetched and
//! sorted by language, then repository, whatever order the lock lists them in. Scale
//! repositories are never part of it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::lock::{self, BenchRepos};

/// The lock format this harness reads.
pub const LOCK_VERSION: u32 = 1;

/// One golden repository to process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenEntry {
    /// The language directory its golden sits in: the language it benchmarks, never a
    /// filter on what is indexed.
    pub language: String,
    /// The repository: the lock URL's last path segment, without a `.git`.
    pub repo: String,
    /// The lock's URL, scheme-less, as checked.
    pub url: String,
    /// The pinned commit: 40 lower-case hex characters.
    pub commit: String,
}

impl GoldenEntry {
    /// `<language>/<repo>`, the entry's name in every message.
    pub fn key(&self) -> String {
        format!("{}/{}", self.language, self.repo)
    }

    /// The only URL a golden is fetched from: HTTPS, built from the checked lock text.
    pub fn fetch_url(&self) -> String {
        format!("https://{}", self.url)
    }

    /// The golden's file name in its language directory.
    pub fn file_name(&self) -> String {
        format!("{}.expected.json", self.repo)
    }

    /// The golden's path below `bench/golden/`.
    pub fn relative_path(&self) -> PathBuf {
        PathBuf::from(&self.language).join(self.file_name())
    }
}

/// Why the lock makes no golden plan. Never quotes a rejected URL, which could hold
/// credentials.
#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    /// The lock is another format.
    #[error("bench/repos.lock is lock_version {0}; this harness reads version {LOCK_VERSION}")]
    LockVersion(u32),
    /// A golden entry has no language.
    #[error("golden entry #{0} has no language")]
    NoLanguage(usize),
    /// A path component is not safe on every platform.
    #[error("golden entry #{index}: its {what} is not a safe file name component ({reason})")]
    UnsafeComponent {
        /// The entry's position in the lock, from 1.
        index: usize,
        /// `language` or `repository`.
        what: &'static str,
        /// Why.
        reason: &'static str,
    },
    /// The URL is not a scheme-less repository location.
    #[error("golden entry #{index}: its URL is not a scheme-less repository location ({reason})")]
    Url {
        /// The entry's position in the lock, from 1.
        index: usize,
        /// Why.
        reason: &'static str,
    },
    /// The commit is not a full lower-case SHA.
    #[error("golden entry {0}: the commit is not 40 lower-case hex characters")]
    Commit(String),
    /// The licence is not permitted.
    #[error("golden entry {key}: licence {licence} is not on the allow list")]
    Licence {
        /// The entry.
        key: String,
        /// Its licence.
        licence: String,
    },
    /// Two entries would write one golden file.
    #[error("golden entries {0} and {1} would write the same golden file")]
    Duplicate(String, String),
}

/// The golden plan of a lock: every `[[golden]]` entry, checked, sorted by
/// `(language, repo)`. Nothing is fetched until every entry passes.
///
/// # Errors
///
/// [`PlanError`] for the first entry that fails.
pub fn golden_plan(repos: &BenchRepos) -> Result<Vec<GoldenEntry>, PlanError> {
    if repos.lock_version != LOCK_VERSION {
        return Err(PlanError::LockVersion(repos.lock_version));
    }
    let mut plan = Vec::with_capacity(repos.golden.len());
    for (i, entry) in repos.golden.iter().enumerate() {
        let index = i + 1;
        let language = entry.language.clone().ok_or(PlanError::NoLanguage(index))?;
        check_component(&language).map_err(|reason| PlanError::UnsafeComponent {
            index,
            what: "language",
            reason,
        })?;
        check_url(&entry.url).map_err(|reason| PlanError::Url { index, reason })?;
        let last = entry.url.rsplit('/').next().unwrap_or_default();
        let repo = last.strip_suffix(".git").unwrap_or(last).to_owned();
        check_component(&repo).map_err(|reason| PlanError::UnsafeComponent {
            index,
            what: "repository",
            reason,
        })?;
        let golden = GoldenEntry {
            language,
            repo,
            url: entry.url.clone(),
            commit: entry.commit.clone(),
        };
        if !is_full_sha(&golden.commit) {
            return Err(PlanError::Commit(golden.key()));
        }
        if !lock::licence_allowed(&entry.licence) {
            return Err(PlanError::Licence {
                key: golden.key(),
                licence: entry.licence.clone(),
            });
        }
        plan.push(golden);
    }
    plan.sort_by(|a, b| (&a.language, &a.repo).cmp(&(&b.language, &b.repo)));
    // One file per entry, on a case-insensitive file system too.
    let mut seen: BTreeMap<(String, String), String> = BTreeMap::new();
    for entry in &plan {
        let folded = (
            entry.language.to_ascii_lowercase(),
            entry.repo.to_ascii_lowercase(),
        );
        if let Some(first) = seen.insert(folded, entry.key()) {
            return Err(PlanError::Duplicate(first, entry.key()));
        }
    }
    Ok(plan)
}

/// Whether `commit` is a full SHA-1 in lower-case hex.
pub fn is_full_sha(commit: &str) -> bool {
    commit.len() == 40
        && commit
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Names Windows reserves for devices, with or without an extension.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Whether `name` is one path component every supported platform stores as written.
/// Unsafe names are refused, never rewritten into another name.
///
/// # Errors
///
/// Why it is not.
pub fn check_component(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("empty");
    }
    if name == "." || name == ".." {
        return Err("a relative component");
    }
    if name.contains(['/', '\\']) {
        return Err("a path separator");
    }
    if name.chars().any(char::is_control) {
        return Err("a control character");
    }
    if name.contains(['<', '>', ':', '"', '|', '?', '*']) {
        return Err("a character Windows forbids");
    }
    if name.ends_with(' ') || name.ends_with('.') {
        return Err("a trailing space or dot");
    }
    let stem = name.split('.').next().unwrap_or(name);
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        return Err("a name Windows reserves");
    }
    Ok(())
}

/// Whether `url` is a scheme-less repository location, `host[:port]/segment/…`: no
/// scheme, user information, query, fragment, backslash, whitespace or control
/// character, a host, at least one path segment, and no empty, `.` or `..` segment.
/// Path segments hold letters, digits, `-`, `.`, `_` and `~` only.
///
/// # Errors
///
/// Why it is not; never the text itself.
pub fn check_url(url: &str) -> Result<(), &'static str> {
    if url.contains("://") {
        return Err("a scheme");
    }
    if url.contains('@') {
        return Err("user information");
    }
    if url.contains('?') || url.contains('#') {
        return Err("a query or fragment");
    }
    if url.contains('\\') {
        return Err("a backslash");
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("whitespace or a control character");
    }
    let mut segments = url.split('/');
    let host = segments.next().unwrap_or_default();
    let (name, port) = host.split_once(':').unwrap_or((host, ""));
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    {
        return Err("no host, or a host that is not a name");
    }
    if host.contains(':') && (port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit())) {
        return Err("a port that is not a number");
    }
    let path: Vec<&str> = segments.collect();
    if path.is_empty() {
        return Err("no repository path");
    }
    for segment in path {
        if segment.is_empty() {
            return Err("an empty path segment");
        }
        if segment == "." || segment == ".." {
            return Err("a relative path segment");
        }
        if !segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
        {
            return Err("a path segment with other characters");
        }
    }
    Ok(())
}
