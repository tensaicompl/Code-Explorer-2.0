//! The pinned reference and benchmark repository lists.
//!
//! Both lock files are written once, by hand or by the pinning task, and read
//! here. Nothing resolves a reference dynamically: the vendoring scripts and the
//! benchmark harness use the commits recorded in these files, so that a build is
//! reproducible and accuracy numbers are comparable across releases.

use std::path::Path;

use serde::Deserialize;

/// Licences permitted for inclusion or as a dependency.
///
/// Anything outside this list is forbidden for inclusion. Copyleft software may
/// only be invoked as a separate process through a documented interface.
pub const LICENCE_ALLOW_LIST: &[&str] = &[
    "MIT",
    "ISC",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "Apache-2.0",
    "Zlib",
    "MPL-2.0",
    "Unlicense",
    "CC0-1.0",
    "0BSD",
];

/// Errors reading a lock file.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// The file could not be read.
    #[error("reading {path}: {source}")]
    Read {
        /// The file that could not be read.
        path: String,
        /// The underlying cause.
        source: std::io::Error,
    },
    /// The file is not valid for its schema.
    #[error("parsing {path}: {source}")]
    Parse {
        /// The file that could not be parsed.
        path: String,
        /// The underlying cause.
        source: toml::de::Error,
    },
}

/// One reference this project takes code, algorithms or packages from.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub struct Reference {
    /// Short name used by the vendoring scripts.
    pub name: String,
    /// Location, without a scheme.
    pub url: String,
    /// The branch the commit was observed on.
    pub default_branch: String,
    /// The exact commit pinned.
    pub commit: String,
    /// The licence, as read from the reference's own licence file.
    pub licence: String,
    /// How the reference is used: vendored, ported, or consumed as a package.
    pub mode: String,
    /// Where its contribution lands in this tree.
    pub lands_in: String,
    /// When the pin was taken.
    pub fetch_date: String,
    /// How the licence was established, when metadata alone was not enough.
    #[serde(default)]
    pub licence_source: Option<String>,
}

/// One grammar, pinned per language of the language matrix.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub struct Grammar {
    /// Language identifier from the language matrix.
    pub language: String,
    /// Location, without a scheme.
    pub url: String,
    /// The branch the commit was observed on.
    pub default_branch: String,
    /// The exact commit pinned.
    pub commit: String,
    /// The licence, as read from the grammar's own licence file.
    pub licence: String,
    /// When the pin was taken.
    pub fetch_date: String,
}

/// The reference lock file.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub struct References {
    /// Schema version of this file.
    pub lock_version: u32,
    /// When the pins were taken.
    pub fetch_date: String,
    /// Licences permitted for inclusion or as a dependency.
    pub licence_allow_list: Vec<String>,
    /// Every reference, in the order the plan lists them.
    #[serde(rename = "reference")]
    pub references: Vec<Reference>,
    /// Every grammar, one per language.
    #[serde(rename = "grammar")]
    pub grammars: Vec<Grammar>,
}

/// One repository the benchmark indexes.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub struct BenchRepo {
    /// Language identifier, for a golden repository.
    #[serde(default)]
    pub language: Option<String>,
    /// Short name, for a scale repository.
    #[serde(default)]
    pub name: Option<String>,
    /// Location, without a scheme.
    pub url: String,
    /// The branch the commit was observed on.
    pub default_branch: String,
    /// The exact commit pinned.
    pub commit: String,
    /// The licence, as read from the repository's own licence file.
    pub licence: String,
    /// When the pin was taken.
    pub fetch_date: String,
    /// The repository this one replaced, when the plan's original choice failed.
    #[serde(default)]
    pub replaces: Option<String>,
    /// Why the replacement was made.
    #[serde(default)]
    pub replacement_reason: Option<String>,
    /// How the licence was established, when metadata alone was not enough.
    #[serde(default)]
    pub licence_source: Option<String>,
    /// Set when the repository is only ever indexed, never redistributed.
    #[serde(default)]
    pub indexed_only: bool,
}

/// The benchmark repository lock file.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub struct BenchRepos {
    /// Schema version of this file.
    pub lock_version: u32,
    /// When the pins were taken.
    pub fetch_date: String,
    /// Repositories whose expected graphs are committed.
    #[serde(rename = "golden")]
    pub golden: Vec<BenchRepo>,
    /// Large repositories used for timing and memory, nightly only.
    #[serde(rename = "scale")]
    pub scale: Vec<BenchRepo>,
}

fn load<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, LockError> {
    let text = std::fs::read_to_string(path).map_err(|source| LockError::Read {
        path: path.display().to_string(),
        source,
    })?;
    toml::from_str(&text).map_err(|source| LockError::Parse {
        path: path.display().to_string(),
        source,
    })
}

/// Reads the reference lock file.
///
/// # Errors
/// Returns an error when the file cannot be read or does not match the schema.
pub fn references(path: &Path) -> Result<References, LockError> {
    load(path)
}

/// Reads the benchmark repository lock file.
///
/// # Errors
/// Returns an error when the file cannot be read or does not match the schema.
pub fn bench_repos(path: &Path) -> Result<BenchRepos, LockError> {
    load(path)
}

/// True when a licence string is permitted for inclusion or as a dependency.
///
/// A licence carrying an exception is matched on its base identifier, which is
/// what the allow list constrains.
#[must_use]
pub fn licence_allowed(licence: &str) -> bool {
    let base = licence
        .split_once(" WITH ")
        .map_or(licence, |(base, _exception)| base)
        .trim();
    LICENCE_ALLOW_LIST.contains(&base)
}
