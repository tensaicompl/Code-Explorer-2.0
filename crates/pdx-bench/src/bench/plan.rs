//! Which repositories a benchmark run measures (P2-14, 4.13): every `[[golden]]` entry
//! of `bench/repos.lock` for determinism, and every golden and `[[scale]]` entry for
//! performance. Goldens are checked as the golden harness checks them
//! ([`golden_plan`]); scale entries the same way, and each must be `indexed_only`.
//! Every entry has a result file, `bench/results/<version>/<name>.json`, so no two may
//! share a name, on a case-insensitive file system too, and none may be the summary's.

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::results::SUMMARY_FILE;
use crate::golden::plan::{
    GoldenEntry, PlanError, check_component, check_url, golden_plan, is_full_sha,
};
use crate::lock::BenchRepos;

/// The directory scale checkouts live in, below `bench/.cache/repos/`: no language of
/// the matrix is named so.
pub const SCALE_DIRECTORY: &str = "scale";

/// What kind of repository an entry is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A golden repository, with the language it benchmarks.
    Golden {
        /// The language.
        language: String,
    },
    /// A scale repository: timed and measured, never redistributed.
    Scale,
}

/// One repository a benchmark run measures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchEntry {
    /// Golden or scale.
    pub kind: Kind,
    /// Its result file's stem: a golden's repository component, a scale entry's name.
    pub name: String,
    /// The lock's URL, scheme-less, as checked.
    pub url: String,
    /// The pinned commit.
    pub commit: String,
}

impl BenchEntry {
    /// `<language>/<repo>` for a golden, the name for a scale repository.
    pub fn key(&self) -> String {
        match &self.kind {
            Kind::Golden { language } => format!("{language}/{}", self.name),
            Kind::Scale => self.name.clone(),
        }
    }

    /// `golden` or `scale`.
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            Kind::Golden { .. } => "golden",
            Kind::Scale => "scale",
        }
    }

    /// The language, for a golden.
    pub fn language(&self) -> Option<&str> {
        match &self.kind {
            Kind::Golden { language } => Some(language),
            Kind::Scale => None,
        }
    }

    /// The only URL it is fetched from.
    pub fn fetch_url(&self) -> String {
        format!("https://{}", self.url)
    }

    /// Its directory below an area of `bench/.cache/`: `<language>/<repo>` or
    /// `scale/<name>`, the same two components for its checkout and its builds.
    pub fn cache_components(&self) -> [&str; 2] {
        match &self.kind {
            Kind::Golden { language } => [language.as_str(), self.name.as_str()],
            Kind::Scale => [SCALE_DIRECTORY, self.name.as_str()],
        }
    }

    /// Its result file's name.
    pub fn result_file(&self) -> String {
        format!("{}.json", self.name)
    }

    /// Its result file's path below `bench/results/<version>/`.
    pub fn result_path(&self) -> PathBuf {
        PathBuf::from(self.result_file())
    }

    fn from_golden(golden: GoldenEntry) -> Self {
        Self {
            kind: Kind::Golden {
                language: golden.language,
            },
            name: golden.repo,
            url: golden.url,
            commit: golden.commit,
        }
    }
}

/// Why the lock makes no benchmark plan.
#[derive(Debug, thiserror::Error)]
pub enum BenchPlanError {
    /// The goldens make no plan.
    #[error(transparent)]
    Golden(#[from] PlanError),
    /// A scale entry has no name.
    #[error("scale entry #{0} has no name")]
    NoName(usize),
    /// A scale entry's name or URL is not safe.
    #[error("scale entry #{index}: its {what} is not acceptable ({reason})")]
    Unsafe {
        /// The entry's position among the scale entries, from 1.
        index: usize,
        /// `name` or `URL`.
        what: &'static str,
        /// Why.
        reason: &'static str,
    },
    /// The commit is not a full lower-case SHA.
    #[error("scale entry {0}: the commit is not 40 lower-case hex characters")]
    Commit(String),
    /// A scale entry may be redistributed: P2-14 never measures one.
    #[error(
        "scale entry {0} is not indexed_only = true; a scale repository is only ever indexed, never redistributed"
    )]
    NotIndexedOnly(String),
    /// Two entries would write one result file.
    #[error("{0} and {1} would write the same result file")]
    Collision(String, String),
}

/// The repositories of a lock, checked and sorted: goldens by language then
/// repository, scale repositories by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchPlan {
    /// The `[[golden]]` entries.
    pub golden: Vec<BenchEntry>,
    /// The `[[scale]]` entries.
    pub scale: Vec<BenchEntry>,
}

impl BenchPlan {
    /// What `--determinism` builds: the goldens only.
    pub fn determinism(&self) -> Vec<&BenchEntry> {
        self.golden.iter().collect()
    }

    /// What `--perf` measures: the goldens, then the scale repositories.
    pub fn performance(&self) -> Vec<&BenchEntry> {
        self.golden.iter().chain(&self.scale).collect()
    }

    /// Every entry, goldens first.
    pub fn all(&self) -> Vec<&BenchEntry> {
        self.performance()
    }
}

/// The benchmark plan of a lock. Nothing is fetched until every entry passes.
///
/// # Errors
///
/// [`BenchPlanError`] for the first entry that fails, or a name two entries share.
pub fn bench_plan(repos: &BenchRepos) -> Result<BenchPlan, BenchPlanError> {
    let golden: Vec<BenchEntry> = golden_plan(repos)?
        .into_iter()
        .map(BenchEntry::from_golden)
        .collect();
    let mut scale = Vec::with_capacity(repos.scale.len());
    for (i, entry) in repos.scale.iter().enumerate() {
        let index = i + 1;
        let name = entry.name.clone().ok_or(BenchPlanError::NoName(index))?;
        check_component(&name).map_err(|reason| BenchPlanError::Unsafe {
            index,
            what: "name",
            reason,
        })?;
        if name.eq_ignore_ascii_case(SCALE_DIRECTORY) {
            return Err(BenchPlanError::Unsafe {
                index,
                what: "name",
                reason: "the name of the scale checkouts' own directory",
            });
        }
        check_url(&entry.url).map_err(|reason| BenchPlanError::Unsafe {
            index,
            what: "URL",
            reason,
        })?;
        if !is_full_sha(&entry.commit) {
            return Err(BenchPlanError::Commit(name));
        }
        if !entry.indexed_only {
            return Err(BenchPlanError::NotIndexedOnly(name));
        }
        scale.push(BenchEntry {
            kind: Kind::Scale,
            name,
            url: entry.url.clone(),
            commit: entry.commit.clone(),
        });
    }
    scale.sort_by(|a, b| a.name.cmp(&b.name));
    // The summary's file is taken too.
    let mut seen: BTreeMap<String, String> =
        BTreeMap::from([(SUMMARY_FILE.to_ascii_lowercase(), "the summary".to_owned())]);
    for entry in golden.iter().chain(&scale) {
        if let Some(first) = seen.insert(entry.result_file().to_ascii_lowercase(), entry.key()) {
            return Err(BenchPlanError::Collision(first, entry.key()));
        }
    }
    if let Some(language) = golden
        .iter()
        .filter_map(BenchEntry::language)
        .find(|l| l.eq_ignore_ascii_case(SCALE_DIRECTORY))
    {
        return Err(BenchPlanError::Collision(
            format!("the golden language {language}"),
            "the scale checkouts' directory".to_owned(),
        ));
    }
    Ok(BenchPlan { golden, scale })
}
