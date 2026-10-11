//! The result files (4.13.2): `bench/results/<pdx_version>/<repo>.json` per repository
//! and `summary.json`, results format [`RESULTS_FORMAT_VERSION`].
//!
//! - **Provenance.** Each file says what produced it: the `pdx` version, the engine,
//!   segment schema and language matrix versions, and the repository (key, kind,
//!   language, commit). Never a branch, a path or the time.
//! - **Merging.** A run writes the sections it measured (`determinism`,
//!   `performance`) and keeps the other one from the file already there, so a
//!   determinism run does not erase a performance baseline, or the reverse. A field
//!   this format does not know (a later task's) is kept as it was.
//! - **Staleness.** When the file's provenance is not this run's, its sections are
//!   stale and dropped. A file that also holds fields this format does not know is
//!   then refused, never rewritten: whether those are stale cannot be told.
//! - **Format.** Canonical JSON: keys sorted at every level whatever the JSON
//!   library's own order, two-space indentation, one trailing newline.
//! - **Promotion.** Every file is staged first; only when all are staged are they
//!   copied into place, each written beside its target and renamed over it. A symlink
//!   where a result belongs is refused.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use pdx_core::consts::{ENGINE_VERSION, LANGUAGE_MATRIX_VERSION, SEGMENT_SCHEMA_VERSION};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::host::Host;
use super::plan::{BenchEntry, Kind};
use super::timer::StageRecord;
use crate::golden::cache::{self, CacheError};
use crate::golden::plan::check_component;

/// The results format this harness writes and reads.
pub const RESULTS_FORMAT_VERSION: u64 = 1;

/// The summary's file name.
pub const SUMMARY_FILE: &str = "summary.json";

/// This build's version: the results directory's name.
pub const PDX_VERSION: &str = env!("CARGO_PKG_VERSION");

const FORMAT_KEY: &str = "results_format_version";
const PROVENANCE_KEY: &str = "provenance";
const DETERMINISM_KEY: &str = "determinism";
const PERFORMANCE_KEY: &str = "performance";
const KNOWN: [&str; 4] = [FORMAT_KEY, PROVENANCE_KEY, DETERMINISM_KEY, PERFORMANCE_KEY];

/// What produced a repository's result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    /// The `pdx` version.
    pub pdx_version: String,
    /// `ENGINE_VERSION`.
    pub engine_version: u32,
    /// `SEGMENT_SCHEMA_VERSION`.
    pub segment_schema_version: u32,
    /// `LANGUAGE_MATRIX_VERSION`.
    pub language_matrix_version: u32,
    /// The repository's key: `<language>/<repo>`, or a scale repository's name.
    pub repository: String,
    /// `golden` or `scale`.
    pub kind: String,
    /// A golden's language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// The pinned commit.
    pub commit: String,
}

impl Provenance {
    /// This build's provenance for `entry`.
    pub fn of(entry: &BenchEntry) -> Self {
        Self {
            pdx_version: PDX_VERSION.to_owned(),
            engine_version: ENGINE_VERSION,
            segment_schema_version: SEGMENT_SCHEMA_VERSION,
            language_matrix_version: LANGUAGE_MATRIX_VERSION,
            repository: entry.key(),
            kind: entry.kind_name().to_owned(),
            language: entry.language().map(str::to_owned),
            commit: entry.commit.clone(),
        }
    }
}

/// What produced a summary: the versions alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SummaryProvenance {
    /// The `pdx` version.
    pub pdx_version: String,
    /// `ENGINE_VERSION`.
    pub engine_version: u32,
    /// `SEGMENT_SCHEMA_VERSION`.
    pub segment_schema_version: u32,
    /// `LANGUAGE_MATRIX_VERSION`.
    pub language_matrix_version: u32,
}

impl SummaryProvenance {
    /// This build's.
    pub fn current() -> Self {
        Self {
            pdx_version: PDX_VERSION.to_owned(),
            engine_version: ENGINE_VERSION,
            segment_schema_version: SEGMENT_SCHEMA_VERSION,
            language_matrix_version: LANGUAGE_MATRIX_VERSION,
        }
    }
}

/// A golden's determinism and rebuild equivalence (4.13.1, 4.13.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Determinism {
    /// Build A: a cold cache, one worker.
    pub cold_single_hash: String,
    /// Build B: another cold cache, `workers_parallel` workers.
    pub cold_parallel_hash: String,
    /// Build C: B's cache, warm, `workers_parallel` workers.
    pub warm_parallel_hash: String,
    /// A and B are the same bytes.
    pub cold_builds_byte_identical: bool,
    /// A and C are the same bytes.
    pub cold_warm_byte_identical: bool,
    /// C's hash is A's: the cache changed nothing.
    pub rebuild_equivalent: bool,
    /// Build A's workers.
    pub workers_single: u64,
    /// Builds B and C's workers.
    pub workers_parallel: u64,
    /// The segment's size.
    pub segment_size_bytes: u64,
    /// The files given to extraction.
    pub files: u64,
    /// Extractions build B stored in its cache.
    pub cold_cache_writes: u64,
    /// Extractions build C read from it.
    pub warm_cache_hits: u64,
}

/// One performance sample: a fresh process, a cold cache.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    /// Its number, from 1.
    pub sample: u64,
    /// `ready`.
    pub status: String,
    /// The segment's content hash.
    pub content_sha256: String,
    /// The segment's size.
    pub size_bytes: u64,
    /// The files discovered.
    pub files_discovered: u64,
    /// The files given to extraction.
    pub files: u64,
    /// The call sites resolved.
    pub call_sites: u64,
    /// The graph's nodes.
    pub nodes: u64,
    /// Stage 2's requested workers.
    pub workers: u64,
    /// Stage 2's memory budget.
    pub memory_budget_bytes: u64,
    /// Stage 1.
    pub discover_ns: u64,
    /// Stage 2.
    pub extract_ns: u64,
    /// Stage 3.
    pub resolve_ns: u64,
    /// Stage 4.
    pub derive_ns: u64,
    /// Stage 5.
    pub write_ns: u64,
    /// The whole build, timed apart from the stages.
    pub total_wall_ns: u64,
    /// `files / (extract + resolve)`.
    pub files_per_second: f64,
    /// The largest resident-memory sample.
    pub peak_rss_bytes: u64,
    /// How many samples it is the largest of.
    pub rss_samples: u64,
}

/// A repository measured to the end.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Completed {
    /// The machine and limits.
    pub host: Host,
    /// How often resident memory was sampled.
    pub rss_sample_interval_ms: u64,
    /// The resident memory a worker is stopped above.
    pub memory_ceiling_bytes: u64,
    /// The segment's content hash, the same in every sample.
    pub content_sha256: String,
    /// The files given to extraction.
    pub files: u64,
    /// The median throughput.
    pub median_files_per_second: f64,
    /// The median of each stage and of the whole build.
    pub median_discover_ns: u64,
    /// Stage 2.
    pub median_extract_ns: u64,
    /// Stage 3.
    pub median_resolve_ns: u64,
    /// Stage 4.
    pub median_derive_ns: u64,
    /// Stage 5.
    pub median_write_ns: u64,
    /// The whole build.
    pub median_total_wall_ns: u64,
    /// The largest peak of any sample.
    pub peak_rss_bytes: u64,
    /// Every sample, as measured.
    pub samples: Vec<Sample>,
}

/// A repository whose worker was stopped at the memory ceiling.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutOfMemory {
    /// The machine and limits.
    pub host: Host,
    /// How often resident memory was sampled.
    pub rss_sample_interval_ms: u64,
    /// The resident memory the worker exceeded.
    pub memory_ceiling_bytes: u64,
    /// The sample that was stopped; no further sample is taken.
    pub sample: u64,
    /// Stage 2's requested workers.
    pub workers: u64,
    /// Stage 2's memory budget.
    pub memory_budget_bytes: u64,
    /// The largest resident-memory sample, the one over the ceiling.
    pub peak_rss_bytes: u64,
    /// How many samples were taken.
    pub rss_samples: u64,
    /// From the worker's start to its stop.
    pub wall_ns_at_stop: u64,
    /// The stage running when it was stopped.
    pub stage_reached: String,
    /// The stages it finished, with their counts and durations.
    pub completed_stages: Vec<StageRecord>,
    /// The files discovered, if Stage 1 finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_discovered: Option<u64>,
    /// The files given to extraction, if Stage 2 finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_extracted: Option<u64>,
    /// Clean extractions Stage 2 had stored in its cold cache when stopped: a lower
    /// bound on the files it had extracted.
    pub cached_extractions_at_stop: u64,
}

/// A repository's performance outcome.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Performance {
    /// Every sample completed.
    Completed(Completed),
    /// A scale repository's worker exceeded the memory ceiling.
    OutOfMemory(OutOfMemory),
}

impl Performance {
    /// `completed` or `out_of_memory`.
    pub fn outcome(&self) -> &'static str {
        match self {
            Self::Completed(_) => "completed",
            Self::OutOfMemory(_) => "out_of_memory",
        }
    }

    /// The peak resident memory observed.
    pub fn peak_rss_bytes(&self) -> u64 {
        match self {
            Self::Completed(c) => c.peak_rss_bytes,
            Self::OutOfMemory(o) => o.peak_rss_bytes,
        }
    }
}

/// One repository's result file.
#[derive(Clone, Debug, PartialEq)]
pub struct ResultFile {
    /// What produced it.
    pub provenance: Provenance,
    /// Its determinism, if measured.
    pub determinism: Option<Determinism>,
    /// Its performance, if measured.
    pub performance: Option<Performance>,
    /// Top-level fields this format does not know, kept as they were.
    pub unknown: Map<String, Value>,
}

/// The summary.
#[derive(Clone, Debug, PartialEq)]
pub struct SummaryFile {
    /// What produced it.
    pub provenance: SummaryProvenance,
    /// Top-level fields this format does not know, kept as they were.
    pub unknown: Map<String, Value>,
    /// Its determinism and performance sections.
    pub sections: Map<String, Value>,
}

/// Why results cannot be written.
#[derive(Debug, thiserror::Error)]
pub enum ResultsError {
    /// A results path is unsafe.
    #[error(transparent)]
    Cache(#[from] CacheError),
    /// A file is not a result file of this format.
    #[error("{path} is not a result file this harness reads: {reason}")]
    Unreadable {
        /// The file.
        path: PathBuf,
        /// Why.
        reason: String,
    },
    /// A stale file holds fields this format does not know.
    #[error(
        "{path} is from another build ({what} differ) and holds fields this harness does not know ({fields}); refusing to rewrite it: move it aside and run again"
    )]
    StaleUnknown {
        /// The file.
        path: PathBuf,
        /// What differs.
        what: String,
        /// The unknown fields.
        fields: String,
    },
    /// A file could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The path.
        path: PathBuf,
        /// What went wrong.
        #[source]
        source: io::Error,
    },
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> ResultsError + '_ {
    move |source| ResultsError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn unreadable(path: &Path, reason: impl Into<String>) -> ResultsError {
    ResultsError::Unreadable {
        path: path.to_path_buf(),
        reason: reason.into(),
    }
}

/// `value` with every object's keys in sorted order.
fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            let mut out = Map::new();
            for (key, value) in entries {
                out.insert(key.clone(), sorted(value));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

/// The canonical bytes of `value`.
pub fn canonical_bytes(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(&sorted(value)).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}

fn to_value<T: Serialize>(item: &T) -> Value {
    serde_json::to_value(item).unwrap_or(Value::Null)
}

fn section<T: DeserializeOwned>(
    map: &Map<String, Value>,
    key: &str,
    path: &Path,
) -> Result<Option<T>, ResultsError> {
    map.get(key)
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()
        .map_err(|e| unreadable(path, format!("its {key} section: {e}")))
}

/// The top-level object of a result file, its format checked.
fn top_level(bytes: &[u8], path: &Path) -> Result<Map<String, Value>, ResultsError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|e| unreadable(path, e.to_string()))?;
    let Value::Object(map) = value else {
        return Err(unreadable(path, "it is not a JSON object"));
    };
    match map.get(FORMAT_KEY).and_then(Value::as_u64) {
        Some(RESULTS_FORMAT_VERSION) => Ok(map),
        Some(other) => Err(unreadable(
            path,
            format!("it is results format {other}, not {RESULTS_FORMAT_VERSION}"),
        )),
        None => Err(unreadable(path, format!("it has no {FORMAT_KEY}"))),
    }
}

fn unknown_fields(map: &Map<String, Value>) -> Map<String, Value> {
    map.iter()
        .filter(|(k, _)| !KNOWN.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

impl ResultFile {
    /// A new file with the sections given.
    pub fn new(
        provenance: Provenance,
        determinism: Option<Determinism>,
        performance: Option<Performance>,
    ) -> Self {
        Self {
            provenance,
            determinism,
            performance,
            unknown: Map::new(),
        }
    }

    /// Parses a result file.
    ///
    /// # Errors
    ///
    /// [`ResultsError::Unreadable`] for anything but a result file of this format.
    pub fn parse(bytes: &[u8], path: &Path) -> Result<Self, ResultsError> {
        let map = top_level(bytes, path)?;
        let provenance = section(&map, PROVENANCE_KEY, path)?
            .ok_or_else(|| unreadable(path, "it has no provenance"))?;
        Ok(Self {
            provenance,
            determinism: section(&map, DETERMINISM_KEY, path)?,
            performance: section(&map, PERFORMANCE_KEY, path)?,
            unknown: unknown_fields(&map),
        })
    }

    /// The canonical bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut map = self.unknown.clone();
        map.insert(FORMAT_KEY.to_owned(), Value::from(RESULTS_FORMAT_VERSION));
        map.insert(PROVENANCE_KEY.to_owned(), to_value(&self.provenance));
        if let Some(d) = &self.determinism {
            map.insert(DETERMINISM_KEY.to_owned(), to_value(d));
        }
        if let Some(p) = &self.performance {
            map.insert(PERFORMANCE_KEY.to_owned(), to_value(p));
        }
        canonical_bytes(&Value::Object(map))
    }
}

/// What differs between two repository provenances, in words.
fn provenance_difference(old: &Provenance, new: &Provenance) -> String {
    let mut differ = Vec::new();
    if old.pdx_version != new.pdx_version {
        differ.push("pdx_version");
    }
    if old.engine_version != new.engine_version {
        differ.push("engine_version");
    }
    if old.segment_schema_version != new.segment_schema_version {
        differ.push("segment_schema_version");
    }
    if old.language_matrix_version != new.language_matrix_version {
        differ.push("language_matrix_version");
    }
    if old.repository != new.repository || old.kind != new.kind || old.language != new.language {
        differ.push("repository");
    }
    if old.commit != new.commit {
        differ.push("commit");
    }
    differ.join(", ")
}

/// The file a run writes for a repository: this run's sections over the existing
/// file's, when the existing one is of this build; this run's alone, when it is stale.
///
/// # Errors
///
/// [`ResultsError::StaleUnknown`] for a stale file with fields this format does not
/// know.
pub fn merge(
    existing: Option<ResultFile>,
    fresh: ResultFile,
    path: &Path,
) -> Result<ResultFile, ResultsError> {
    let Some(old) = existing else {
        return Ok(fresh);
    };
    if old.provenance == fresh.provenance {
        return Ok(ResultFile {
            provenance: fresh.provenance,
            determinism: fresh.determinism.or(old.determinism),
            performance: fresh.performance.or(old.performance),
            unknown: old.unknown,
        });
    }
    if !old.unknown.is_empty() {
        return Err(ResultsError::StaleUnknown {
            path: path.to_path_buf(),
            what: provenance_difference(&old.provenance, &fresh.provenance),
            fields: old.unknown.keys().cloned().collect::<Vec<_>>().join(", "),
        });
    }
    Ok(fresh)
}

/// Reads `path` if it is there: a regular file or nothing, never a symlink.
///
/// # Errors
///
/// [`CacheError::Refused`] for anything but a regular file; I/O errors.
pub fn read_existing(path: &Path) -> Result<Option<Vec<u8>>, ResultsError> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_file() => fs::read(path).map(Some).map_err(io_error(path)),
        Ok(_) => Err(CacheError::Refused(path.to_path_buf()).into()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_error(path)(e)),
    }
}

/// The ratio of two counts, as a rate.
#[allow(clippy::cast_precision_loss)] // counts of repositories
fn rate(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

/// The summary of a run's merged result files: every planned repository's file that
/// is of this build, in plan order.
pub fn summarise(files: &[(&BenchEntry, &ResultFile)]) -> Map<String, Value> {
    let mut sections = Map::new();
    let tested: Vec<(&BenchEntry, &Determinism)> = files
        .iter()
        .filter_map(|(e, f)| f.determinism.as_ref().map(|d| (*e, d)))
        .collect();
    if !tested.is_empty() {
        let identical = tested
            .iter()
            .filter(|(_, d)| d.cold_builds_byte_identical && d.cold_warm_byte_identical)
            .count();
        let equivalent = tested.iter().filter(|(_, d)| d.rebuild_equivalent).count();
        let repositories: Vec<String> = tested.iter().map(|(e, _)| e.key()).collect();
        sections.insert(
            DETERMINISM_KEY.to_owned(),
            serde_json::json!({
                "repositories": repositories,
                "repositories_tested": tested.len(),
                "repositories_identical": identical,
                "determinism_rate": rate(identical, tested.len()),
                "cold_warm_equivalent": equivalent,
                "rebuild_equivalence_rate": rate(equivalent, tested.len()),
            }),
        );
    }
    let measured: Vec<(&BenchEntry, &Performance)> = files
        .iter()
        .filter_map(|(e, f)| f.performance.as_ref().map(|p| (*e, p)))
        .collect();
    if !measured.is_empty() {
        let mut goldens = Map::new();
        let mut scale = Map::new();
        for (entry, performance) in &measured {
            let mut line = Map::new();
            line.insert("outcome".into(), Value::from(performance.outcome()));
            line.insert(
                "peak_rss_bytes".into(),
                Value::from(performance.peak_rss_bytes()),
            );
            match performance {
                Performance::Completed(c) => {
                    line.insert(
                        "median_files_per_second".into(),
                        serde_json::Number::from_f64(c.median_files_per_second)
                            .map_or(Value::Null, Value::Number),
                    );
                    line.insert(
                        "median_total_wall_ns".into(),
                        Value::from(c.median_total_wall_ns),
                    );
                    line.insert("files".into(), Value::from(c.files));
                }
                Performance::OutOfMemory(o) => {
                    line.insert("stage_reached".into(), Value::from(o.stage_reached.clone()));
                    line.insert(
                        "memory_ceiling_bytes".into(),
                        Value::from(o.memory_ceiling_bytes),
                    );
                }
            }
            let target = match entry.kind {
                Kind::Golden { .. } => &mut goldens,
                Kind::Scale => &mut scale,
            };
            target.insert(entry.key(), Value::Object(line));
        }
        let completed =
            |map: &Map<String, Value>| map.values().filter(|v| v["outcome"] == "completed").count();
        let (goldens_completed, scale_completed) = (completed(&goldens), completed(&scale));
        let mut performance = Map::new();
        performance.insert("repositories_measured".into(), Value::from(measured.len()));
        performance.insert("goldens_measured".into(), Value::from(goldens.len()));
        performance.insert("goldens_completed".into(), Value::from(goldens_completed));
        performance.insert(
            "scale_repositories_measured".into(),
            Value::from(scale.len()),
        );
        performance.insert(
            "scale_repositories_completed".into(),
            Value::from(scale_completed),
        );
        performance.insert(
            "all_completed".into(),
            Value::from(goldens_completed + scale_completed == measured.len()),
        );
        performance.insert("goldens".into(), Value::Object(goldens));
        performance.insert("scale".into(), Value::Object(scale));
        sections.insert(PERFORMANCE_KEY.to_owned(), Value::Object(performance));
    }
    sections
}

impl SummaryFile {
    /// Parses a summary.
    ///
    /// # Errors
    ///
    /// [`ResultsError::Unreadable`] for anything but a summary of this format.
    pub fn parse(bytes: &[u8], path: &Path) -> Result<Self, ResultsError> {
        let map = top_level(bytes, path)?;
        let provenance = section(&map, PROVENANCE_KEY, path)?
            .ok_or_else(|| unreadable(path, "it has no provenance"))?;
        let sections = map
            .iter()
            .filter(|(k, _)| *k == DETERMINISM_KEY || *k == PERFORMANCE_KEY)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Ok(Self {
            provenance,
            unknown: unknown_fields(&map),
            sections,
        })
    }

    /// The summary of this run, keeping an existing summary's unknown fields when it is
    /// of this build.
    ///
    /// # Errors
    ///
    /// [`ResultsError::StaleUnknown`] when it is stale and holds unknown fields.
    pub fn merged(
        existing: Option<Self>,
        sections: Map<String, Value>,
        path: &Path,
    ) -> Result<Self, ResultsError> {
        let provenance = SummaryProvenance::current();
        let unknown = match existing {
            Some(old) if old.provenance == provenance => old.unknown,
            Some(old) if !old.unknown.is_empty() => {
                return Err(ResultsError::StaleUnknown {
                    path: path.to_path_buf(),
                    what: "versions".to_owned(),
                    fields: old.unknown.keys().cloned().collect::<Vec<_>>().join(", "),
                });
            }
            _ => Map::new(),
        };
        Ok(Self {
            provenance,
            unknown,
            sections,
        })
    }

    /// The canonical bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut map = self.unknown.clone();
        map.insert(FORMAT_KEY.to_owned(), Value::from(RESULTS_FORMAT_VERSION));
        map.insert(PROVENANCE_KEY.to_owned(), to_value(&self.provenance));
        for (k, v) in &self.sections {
            map.insert(k.clone(), v.clone());
        }
        canonical_bytes(&Value::Object(map))
    }
}

/// Writes every file to `staging` (each `(name, bytes)`), then, once all are staged,
/// copies each into `results` beside its target and renames it over the target. A
/// failure while staging leaves `results` untouched.
///
/// # Errors
///
/// [`ResultsError`] for an unsafe path or a failed write.
pub fn stage_and_promote(
    staging: &Path,
    results: &Path,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<(), ResultsError> {
    let mut staged = Vec::with_capacity(files.len());
    for (name, bytes) in files {
        check_component(name).map_err(|reason| unreadable(&staging.join(name), reason))?;
        let path = staging.join(name);
        cache::write_file(&path, bytes)?;
        staged.push((name, path));
    }
    for (name, path) in staged {
        let bytes = fs::read(&path).map_err(io_error(&path))?;
        let target = results.join(name);
        if read_existing(&target)?.as_deref() == Some(&bytes[..]) {
            continue;
        }
        let temporary = results.join(format!(".{name}.staged"));
        cache::write_file(&temporary, &bytes)?;
        fs::rename(&temporary, &target).map_err(io_error(&target))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn golden() -> BenchEntry {
        BenchEntry {
            kind: Kind::Golden {
                language: "python".into(),
            },
            name: "flask".into(),
            url: "git.example/acme/flask".into(),
            commit: "a".repeat(40),
        }
    }

    fn determinism() -> Determinism {
        Determinism {
            cold_single_hash: "1".repeat(64),
            cold_parallel_hash: "1".repeat(64),
            warm_parallel_hash: "1".repeat(64),
            cold_builds_byte_identical: true,
            cold_warm_byte_identical: true,
            rebuild_equivalent: true,
            workers_single: 1,
            workers_parallel: 4,
            segment_size_bytes: 4096,
            files: 10,
            cold_cache_writes: 9,
            warm_cache_hits: 9,
        }
    }

    pub(crate) fn host() -> Host {
        Host {
            os: "linux".into(),
            architecture: "x86_64".into(),
            logical_cpus: 4,
            total_memory_bytes: 1 << 33,
            cpu_brand: None,
            benchmark_workers: 4,
            benchmark_memory_budget_bytes: 1 << 29,
        }
    }

    fn sample(n: u64, rate: f64) -> Sample {
        Sample {
            sample: n,
            status: "ready".into(),
            content_sha256: "1".repeat(64),
            size_bytes: 4096,
            files_discovered: 12,
            files: 10,
            call_sites: 20,
            nodes: 30,
            workers: 4,
            memory_budget_bytes: 1 << 29,
            discover_ns: 1,
            extract_ns: 2,
            resolve_ns: 3,
            derive_ns: 4,
            write_ns: 5,
            total_wall_ns: 16,
            files_per_second: rate,
            peak_rss_bytes: 100,
            rss_samples: 3,
        }
    }

    fn performance() -> Performance {
        Performance::Completed(Completed {
            host: host(),
            rss_sample_interval_ms: 10,
            memory_ceiling_bytes: 1 << 32,
            content_sha256: "1".repeat(64),
            files: 10,
            median_files_per_second: 200.0,
            median_discover_ns: 1,
            median_extract_ns: 2,
            median_resolve_ns: 3,
            median_derive_ns: 4,
            median_write_ns: 5,
            median_total_wall_ns: 16,
            peak_rss_bytes: 100,
            samples: vec![sample(1, 100.0), sample(2, 300.0), sample(3, 200.0)],
        })
    }

    #[test]
    fn a_result_file_round_trips_canonically() {
        let file = ResultFile::new(
            Provenance::of(&golden()),
            Some(determinism()),
            Some(performance()),
        );
        let bytes = file.to_bytes();
        let text = String::from_utf8(bytes.clone()).expect("utf-8");
        assert!(text.ends_with("}\n") && !text.ends_with("\n\n"));
        assert!(text.contains("\"outcome\": \"completed\""), "{text}");
        assert!(text.contains("\"results_format_version\": 1"));
        let back = ResultFile::parse(&bytes, Path::new("f.json")).expect("parses");
        assert_eq!(back, file);
        assert_eq!(back.to_bytes(), bytes);
        // Keys are sorted at every level.
        let determinism = text.find("\"determinism\"").expect("determinism");
        let performance = text.find("\"performance\"").expect("performance");
        let provenance = text.find("\"provenance\"").expect("provenance");
        assert!(determinism < performance && performance < provenance);
        assert!(!text.contains("/home") && !text.contains("branch"));
    }

    #[test]
    fn canonical_json_sorts_whatever_the_insertion_order() {
        let mut map = Map::new();
        map.insert("z".into(), Value::from(1));
        map.insert(
            "a".into(),
            serde_json::json!({"y": 1, "b": [ {"d": 1, "c": 2} ]}),
        );
        let text = String::from_utf8(canonical_bytes(&Value::Object(map))).expect("utf-8");
        assert_eq!(
            text,
            "{\n  \"a\": {\n    \"b\": [\n      {\n        \"c\": 2,\n        \"d\": 1\n      }\n    ],\n    \"y\": 1\n  },\n  \"z\": 1\n}\n"
        );
    }

    #[test]
    fn a_field_from_a_later_format_survives_a_rewrite() {
        let mut old = ResultFile::new(Provenance::of(&golden()), None, Some(performance()));
        old.unknown
            .insert("accuracy".into(), serde_json::json!({"precision": 0.9}));
        let bytes = old.to_bytes();
        let existing = ResultFile::parse(&bytes, Path::new("f.json")).expect("parses");
        let fresh = ResultFile::new(Provenance::of(&golden()), Some(determinism()), None);
        let merged = merge(Some(existing), fresh, Path::new("f.json")).expect("merges");
        assert_eq!(merged.unknown["accuracy"]["precision"], 0.9);
        // A determinism run keeps the performance section, and the reverse.
        assert_eq!(merged.performance, Some(performance()));
        assert_eq!(merged.determinism, Some(determinism()));
        let fresh = ResultFile::new(Provenance::of(&golden()), None, Some(performance()));
        let again = merge(Some(merged.clone()), fresh, Path::new("f.json")).expect("merges");
        assert_eq!(again.determinism, Some(determinism()));
    }

    #[test]
    fn stale_sections_are_dropped_and_stale_unknown_fields_refused() {
        let mut old_provenance = Provenance::of(&golden());
        old_provenance.engine_version = 5;
        let old = ResultFile::new(
            old_provenance.clone(),
            Some(determinism()),
            Some(performance()),
        );
        let fresh = ResultFile::new(Provenance::of(&golden()), Some(determinism()), None);
        let merged = merge(Some(old), fresh.clone(), Path::new("f.json")).expect("merges");
        assert_eq!(
            merged.performance, None,
            "an engine 5 baseline is not engine 6's"
        );
        assert_eq!(merged.provenance.engine_version, ENGINE_VERSION);

        let mut old = ResultFile::new(old_provenance, None, Some(performance()));
        old.unknown.insert("accuracy".into(), Value::from(1));
        let refused = merge(Some(old), fresh, Path::new("f.json"));
        match refused {
            Err(ResultsError::StaleUnknown { what, fields, .. }) => {
                assert_eq!(what, "engine_version");
                assert_eq!(fields, "accuracy");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_file_of_another_format_is_refused() {
        let path = Path::new("f.json");
        for text in [
            "[]",
            "{}",
            "{\"results_format_version\": 2, \"provenance\": {}}",
            "{\"results_format_version\": 1}",
            "{\"results_format_version\": 1, \"provenance\": {\"pdx_version\": 1}}",
        ] {
            assert!(
                matches!(
                    ResultFile::parse(text.as_bytes(), path),
                    Err(ResultsError::Unreadable { .. })
                ),
                "{text}"
            );
        }
    }

    #[test]
    fn the_summary_counts_and_names_the_scale_repositories() {
        let golden = golden();
        let scale = BenchEntry {
            kind: Kind::Scale,
            name: "kubernetes".into(),
            url: "git.example/k/kubernetes".into(),
            commit: "b".repeat(40),
        };
        let golden_file = ResultFile::new(
            Provenance::of(&golden),
            Some(determinism()),
            Some(performance()),
        );
        let oom = Performance::OutOfMemory(OutOfMemory {
            host: host(),
            rss_sample_interval_ms: 10,
            memory_ceiling_bytes: 100,
            sample: 1,
            workers: 4,
            memory_budget_bytes: 1 << 29,
            peak_rss_bytes: 101,
            rss_samples: 50,
            wall_ns_at_stop: 1_000,
            stage_reached: "extract".into(),
            completed_stages: Vec::new(),
            files_discovered: Some(5),
            files_extracted: None,
            cached_extractions_at_stop: 3,
        });
        let scale_file = ResultFile::new(Provenance::of(&scale), None, Some(oom));
        let sections = summarise(&[(&golden, &golden_file), (&scale, &scale_file)]);
        let d = &sections["determinism"];
        assert_eq!(d["repositories_tested"], 1);
        assert_eq!(d["repositories_identical"], 1);
        assert_eq!(d["determinism_rate"], 1.0);
        assert_eq!(d["cold_warm_equivalent"], 1);
        assert_eq!(d["rebuild_equivalence_rate"], 1.0);
        let p = &sections["performance"];
        assert_eq!(p["repositories_measured"], 2);
        assert_eq!(p["goldens_measured"], 1);
        assert_eq!(p["scale_repositories_measured"], 1);
        assert_eq!(p["scale_repositories_completed"], 0);
        assert_eq!(p["all_completed"], false);
        assert_eq!(p["scale"]["kubernetes"]["outcome"], "out_of_memory");
        assert_eq!(p["scale"]["kubernetes"]["stage_reached"], "extract");
        assert_eq!(
            p["goldens"]["python/flask"]["median_files_per_second"],
            200.0
        );
        let summary = SummaryFile::merged(None, sections, Path::new("s.json")).expect("summary");
        let bytes = summary.to_bytes();
        let back = SummaryFile::parse(&bytes, Path::new("s.json")).expect("parses");
        assert_eq!(back, summary);
    }

    #[test]
    fn promotion_is_staged_and_refuses_symlinks() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let staging = tmp.path().join("staging");
        let results = tmp.path().join("results");
        fs::create_dir(&staging).expect("mkdir");
        fs::create_dir(&results).expect("mkdir");
        let mut files = BTreeMap::new();
        files.insert("flask.json".to_owned(), b"{}\n".to_vec());
        files.insert("summary.json".to_owned(), b"{\"a\": 1}\n".to_vec());
        stage_and_promote(&staging, &results, &files).expect("promotes");
        assert_eq!(fs::read(results.join("flask.json")).expect("read"), b"{}\n");
        assert_eq!(fs::read_dir(&results).expect("read").count(), 2);

        // A file that cannot be staged leaves the results as they were.
        let mut bad = files.clone();
        bad.insert("summary.json".to_owned(), b"changed\n".to_vec());
        bad.insert("zz:escape.json".to_owned(), b"x".to_vec());
        assert!(stage_and_promote(&staging, &results, &bad).is_err());
        assert_eq!(
            fs::read(results.join("summary.json")).expect("read"),
            b"{\"a\": 1}\n"
        );
        #[cfg(unix)]
        {
            fs::remove_file(results.join("flask.json")).expect("rm");
            std::os::unix::fs::symlink(tmp.path().join("elsewhere"), results.join("flask.json"))
                .expect("symlink");
            assert!(matches!(
                stage_and_promote(&staging, &results, &files),
                Err(ResultsError::Cache(CacheError::Refused(_)))
            ));
            assert!(!tmp.path().join("elsewhere").exists());
            assert!(matches!(
                read_existing(&results.join("flask.json")),
                Err(ResultsError::Cache(CacheError::Refused(_)))
            ));
        }
    }
}
