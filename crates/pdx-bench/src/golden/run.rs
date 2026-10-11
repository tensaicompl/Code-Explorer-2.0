//! A golden run: every repository of the plan produced and staged, then compared with
//! `bench/golden/` or, with `--accept`, promoted into it.
//!
//! Comparison never changes `bench/golden/`: a missing, stale, differing or
//! differently formatted golden is a failure, and only `--accept` replaces one.
//! Acceptance is all-or-nothing: every repository is fetched, indexed and staged under
//! `bench/.cache/generated/` first, one at a time, and only when all have succeeded are
//! the staged files promoted and stale goldens removed. A failure part-way leaves every
//! committed golden as it was.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use pdx_core::segment::{SegmentError, SegmentReader};

use super::cache::{self, Cache, CacheError};
use super::expected::{ExpectedError, GoldenEdge, GoldenExpected};
use super::git::{self, FetchError, Git, Prepared};
use super::index::{self, IndexError};
use super::plan::{GoldenEntry, PlanError};
use crate::lock::LockError;

/// The engine's content-changing switches (`pdx_engine::EXTRACTION_SWITCHES`, which a
/// test keeps this equal to). A golden is never built with one set.
pub const EXTRACTION_SWITCHES: [&str; 6] = [
    "PDX_ENGINE_WALK_MAX_NODES",
    "PDX_ENGINE_WALK_DEFS_MAX",
    "PDX_ENGINE_TS_TYPE_BUDGET",
    "PDX_ENGINE_LSP_MAX_WALK_DEPTH",
    "PDX_ENGINE_LSP_DISABLED",
    "PDX_ENGINE_INDEX_QUARANTINE_FILE",
];

/// The golden suffix.
const SUFFIX: &str = ".expected.json";

/// How many differing edges a mismatch lists each way.
const SHOW: usize = 5;

/// Why a golden run failed.
#[derive(Debug, thiserror::Error)]
pub enum GoldenError {
    /// An engine switch is set.
    #[error(
        "{0} is set: goldens are never built with an engine extraction switch; unset it and run again"
    )]
    Switch(&'static str),
    /// The directory is not this workspace's root.
    #[error(
        "{0} is not the workspace root (no Cargo.toml, bench/repos.lock and crates/pdx-bench there); run from the root"
    )]
    Workspace(PathBuf),
    /// No `pdx` beside the harness.
    #[error(
        "no pdx binary at {0}; build it first: cargo build -p pdx -p pdx-bench (make golden does)"
    )]
    NoPdx(PathBuf),
    /// The lock file cannot be read.
    #[error(transparent)]
    Lock(#[from] LockError),
    /// The lock makes no plan.
    #[error(transparent)]
    Plan(#[from] PlanError),
    /// A cache or golden path is unsafe.
    #[error(transparent)]
    Cache(#[from] CacheError),
    /// `bench/golden/` holds something that is no golden.
    #[error("{0} is not a golden file or language directory; bench/golden/ holds goldens only")]
    UnknownContent(PathBuf),
    /// One repository failed.
    #[error("{key}: {source}")]
    Repository {
        /// The repository.
        key: String,
        /// Why.
        source: Box<GoldenError>,
    },
    /// Fetching failed.
    #[error(transparent)]
    Fetch(#[from] FetchError),
    /// Indexing failed.
    #[error(transparent)]
    Index(#[from] IndexError),
    /// The segment could not be read.
    #[error(transparent)]
    Segment(#[from] SegmentError),
    /// The rows make no golden, or a committed golden is not one.
    #[error(transparent)]
    Expected(#[from] ExpectedError),
    /// Goldens differ from what the index produces.
    #[error(
        "{0} golden(s) do not match; if the change is intended, run pdx-bench golden --accept and justify it in the commit body"
    )]
    Mismatch(usize),
    /// A file could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The path.
        path: PathBuf,
        /// What went wrong.
        #[source]
        source: std::io::Error,
    },
}

fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> GoldenError + '_ {
    move |source| GoldenError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Refuses to run when any engine extraction switch is set, whatever its value. The
/// error names the variable, never its value.
///
/// # Errors
///
/// [`GoldenError::Switch`] for the first one set.
pub fn preflight(is_set: impl Fn(&str) -> bool) -> Result<(), GoldenError> {
    match EXTRACTION_SWITCHES.iter().find(|name| is_set(name)) {
        Some(name) => Err(GoldenError::Switch(name)),
        None => Ok(()),
    }
}

/// The workspace root `dir`, made absolute: it must hold `Cargo.toml`,
/// `bench/repos.lock` and `crates/pdx-bench`.
///
/// # Errors
///
/// [`GoldenError::Workspace`] otherwise.
pub fn workspace_root(dir: &Path) -> Result<PathBuf, GoldenError> {
    let root = dir
        .canonicalize()
        .map_err(|_| GoldenError::Workspace(dir.to_path_buf()))?;
    if root.join("Cargo.toml").is_file()
        && root.join("bench/repos.lock").is_file()
        && root.join("crates/pdx-bench").is_dir()
    {
        Ok(root)
    } else {
        Err(GoldenError::Workspace(dir.to_path_buf()))
    }
}

/// Compare with the committed goldens, or accept new ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Fail on any difference; never write `bench/golden/`.
    Compare,
    /// Replace `bench/golden/` with what was produced, once every repository succeeded.
    Accept,
}

/// One repository's produced golden.
#[derive(Clone, Debug)]
pub struct Produced {
    /// The golden.
    pub expected: GoldenExpected,
    /// The segment it was read from, by content hash.
    pub content_sha256: String,
}

/// Produces a repository's golden. The production producer fetches, indexes and reads
/// ([`PdxProducer`]); tests supply their own.
pub trait Producer {
    /// The golden of `entry`.
    ///
    /// # Errors
    ///
    /// Whatever stops it; the run then fails with nothing promoted.
    fn produce(&mut self, entry: &GoldenEntry) -> Result<Produced, GoldenError>;
}

/// The production producer: the pinned checkout, `pdx index` on it, its segment read.
#[derive(Debug)]
pub struct PdxProducer {
    pdx: PathBuf,
    cache: Cache,
    git: Git,
}

impl PdxProducer {
    /// A producer running `pdx`, with its checkouts and segments in `cache`.
    ///
    /// # Errors
    ///
    /// [`GoldenError::NoPdx`] when `pdx` is not a file; cache errors.
    pub fn new(pdx: PathBuf, cache: Cache) -> Result<Self, GoldenError> {
        if !pdx.is_file() {
            return Err(GoldenError::NoPdx(pdx));
        }
        let git = Git::new(cache.no_hooks()?, cache.git_global()?);
        Ok(Self { pdx, cache, git })
    }
}

impl Producer for PdxProducer {
    fn produce(&mut self, entry: &GoldenEntry) -> Result<Produced, GoldenError> {
        let checkout = self.cache.checkout(entry)?;
        let prepared =
            git::prepare_checkout(&self.git, &checkout, &entry.fetch_url(), &entry.commit)?;
        let how = match prepared {
            Prepared::Reused => "checkout reused",
            Prepared::Fetched => "commit fetched",
        };
        eprintln!("pdx-bench: {}: {how}; indexing", entry.key());
        let segments = self.cache.segments_root()?;
        let segment = self.cache.segment(entry)?;
        cache::remove_owned_file(&segments, &segment)?;
        let run = index::run_pdx(&self.pdx, &checkout, &segment, &self.git.environment())?;
        let ready = index::interpret(&run, &segment)?;
        let reader = SegmentReader::open(&segment)?;
        let expected = GoldenExpected::from_rows(&reader.nodes_all()?, &reader.edges_all()?)?;
        Ok(Produced {
            expected,
            content_sha256: ready.content_sha256,
        })
    }
}

/// What a staged golden summarises to.
struct Staged {
    entry: GoldenEntry,
    path: PathBuf,
    nodes: u64,
    edges: usize,
    content_sha256: String,
}

/// The committed goldens below `golden_dir`, by `<language>/<file>`: language
/// directories holding `*.expected.json` files only. Absent is empty.
///
/// # Errors
///
/// [`GoldenError::UnknownContent`] for anything else there; cache errors for a
/// symlink.
pub fn committed_goldens(golden_dir: &Path) -> Result<BTreeMap<String, PathBuf>, GoldenError> {
    let mut found = BTreeMap::new();
    if !cache::is_real_dir(golden_dir)? {
        return Ok(found);
    }
    for language in fs::read_dir(golden_dir).map_err(io_error(golden_dir))? {
        let language = language.map_err(io_error(golden_dir))?.path();
        if !cache::is_real_dir(&language).unwrap_or(false) {
            return Err(GoldenError::UnknownContent(language));
        }
        for file in fs::read_dir(&language).map_err(io_error(&language))? {
            let file = file.map_err(io_error(&language))?.path();
            let named = file
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.len() > SUFFIX.len() && n.ends_with(SUFFIX));
            let regular = fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_file());
            if !named || !regular {
                return Err(GoldenError::UnknownContent(file));
            }
            let key = format!(
                "{}/{}",
                language
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default(),
                file.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default()
            );
            found.insert(key, file);
        }
    }
    Ok(found)
}

fn file_key(entry: &GoldenEntry) -> String {
    format!("{}/{}", entry.language, entry.file_name())
}

/// Runs the plan: produces and stages every golden, then compares or accepts.
///
/// Summaries go to `out`, one deterministic line per repository; differences go to
/// `err`. `bench/golden/` is `golden_dir`; it is written only in [`Mode::Accept`],
/// and only after every repository has been staged.
///
/// # Errors
///
/// [`GoldenError::Repository`] when one repository fails (nothing is promoted);
/// [`GoldenError::Mismatch`] when comparison finds differences;
/// [`GoldenError::UnknownContent`] for a stray file in `bench/golden/`; file errors.
pub fn run(
    golden_dir: &Path,
    cache: &Cache,
    plan: &[GoldenEntry],
    mode: Mode,
    producer: &mut dyn Producer,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), GoldenError> {
    let committed = committed_goldens(golden_dir)?;

    let mut staged = Vec::with_capacity(plan.len());
    for entry in plan {
        let in_context = |source: GoldenError| GoldenError::Repository {
            key: entry.key(),
            source: Box::new(source),
        };
        let golden = producer.produce(entry).map_err(in_context)?;
        let path = cache.staged(entry).map_err(|e| in_context(e.into()))?;
        cache::write_file(&path, &golden.expected.to_canonical_bytes())
            .map_err(|e| in_context(e.into()))?;
        staged.push(Staged {
            entry: entry.clone(),
            path,
            nodes: golden.expected.node_total(),
            edges: golden.expected.drawn_edges.len(),
            content_sha256: golden.content_sha256,
        });
    }

    match mode {
        Mode::Compare => compare(&committed, &staged, out, err),
        Mode::Accept => accept(golden_dir, &committed, &staged, out),
    }
}

fn compare(
    committed: &BTreeMap<String, PathBuf>,
    staged: &[Staged],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), GoldenError> {
    let mut mismatches = 0;
    for s in staged {
        let actual = fs::read(&s.path).map_err(io_error(&s.path))?;
        let Some(path) = committed.get(&file_key(&s.entry)) else {
            mismatches += 1;
            let _ = writeln!(err, "{}: no committed golden", s.entry.key());
            continue;
        };
        let expected = fs::read(path).map_err(io_error(path))?;
        if expected == actual {
            let _ = writeln!(
                out,
                "{}: matches, {} nodes, {} drawn edges, segment {}",
                s.entry.key(),
                s.nodes,
                s.edges,
                s.content_sha256
            );
        } else {
            mismatches += 1;
            describe_difference(&s.entry.key(), &expected, &actual, err);
        }
    }
    let planned: Vec<String> = staged.iter().map(|s| file_key(&s.entry)).collect();
    for key in committed.keys().filter(|k| !planned.contains(k)) {
        mismatches += 1;
        let _ = writeln!(
            err,
            "{key}: a committed golden for no [[golden]] entry of bench/repos.lock"
        );
    }
    if mismatches > 0 {
        return Err(GoldenError::Mismatch(mismatches));
    }
    let _ = writeln!(out, "golden: {} repositories match", staged.len());
    Ok(())
}

/// A short account of how two goldens differ: totals and the first few edges each
/// side has and the other lacks. Never the whole lists.
fn describe_difference(key: &str, expected: &[u8], actual: &[u8], err: &mut dyn Write) {
    let (Ok(e), Ok(a)) = (
        GoldenExpected::parse(expected),
        GoldenExpected::parse(actual),
    ) else {
        let _ = writeln!(err, "{key}: the committed golden is not a golden");
        return;
    };
    let _ = writeln!(
        err,
        "{key}: differs: nodes {} expected, {} actual; drawn edges {} expected, {} actual",
        e.node_total(),
        a.node_total(),
        e.drawn_edges.len(),
        a.drawn_edges.len()
    );
    if e == a {
        let _ = writeln!(
            err,
            "{key}: same graph, other bytes: not in the canonical format"
        );
        return;
    }
    let (only_expected, only_actual) = edge_difference(&e.drawn_edges, &a.drawn_edges);
    for (label, edges) in [
        ("expected only", only_expected),
        ("actual only", only_actual),
    ] {
        for edge in edges.iter().take(SHOW) {
            let _ = writeln!(
                err,
                "{key}:   {label}: {} -> {} {} {}",
                edge.src_qn, edge.dst_qn, edge.kind, edge.band
            );
        }
        if edges.len() > SHOW {
            let _ = writeln!(err, "{key}:   {label}: and {} more", edges.len() - SHOW);
        }
    }
    let changed: Vec<String> = e
        .node_counts
        .iter()
        .zip(&a.node_counts)
        .filter(|(x, y)| x != y)
        .map(|(x, y)| format!("{} {} -> {}", x.kind, x.count, y.count))
        .collect();
    if !changed.is_empty() {
        let _ = writeln!(err, "{key}:   node counts: {}", changed.join(", "));
    }
}

/// The edges of two sorted lists that the other lacks, counting duplicates.
fn edge_difference<'a>(
    expected: &'a [GoldenEdge],
    actual: &'a [GoldenEdge],
) -> (Vec<&'a GoldenEdge>, Vec<&'a GoldenEdge>) {
    let (mut i, mut j) = (0, 0);
    let (mut only_e, mut only_a) = (Vec::new(), Vec::new());
    while i < expected.len() && j < actual.len() {
        match expected[i].sort_key().cmp(&actual[j].sort_key()) {
            std::cmp::Ordering::Less => {
                only_e.push(&expected[i]);
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                only_a.push(&actual[j]);
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                i += 1;
                j += 1;
            }
        }
    }
    only_e.extend(&expected[i..]);
    only_a.extend(&actual[j..]);
    (only_e, only_a)
}

fn accept(
    golden_dir: &Path,
    committed: &BTreeMap<String, PathBuf>,
    staged: &[Staged],
    out: &mut dyn Write,
) -> Result<(), GoldenError> {
    cache::ensure_dir(golden_dir)?;
    for s in staged {
        let language = golden_dir.join(&s.entry.language);
        cache::ensure_dir(&language)?;
        let target = language.join(s.entry.file_name());
        let bytes = fs::read(&s.path).map_err(io_error(&s.path))?;
        cache::write_file(&target, &bytes)?;
        let _ = writeln!(
            out,
            "{}: accepted bench/golden/{}, {} nodes, {} drawn edges, segment {}",
            s.entry.key(),
            file_key(&s.entry),
            s.nodes,
            s.edges,
            s.content_sha256
        );
    }
    let planned: Vec<String> = staged.iter().map(|s| file_key(&s.entry)).collect();
    let mut removed = 0;
    for (key, path) in committed.iter().filter(|(k, _)| !planned.contains(k)) {
        cache::remove_owned_file(golden_dir, path)?;
        removed += 1;
        let _ = writeln!(out, "{key}: removed, no longer in bench/repos.lock");
        if let Some(language) = path.parent()
            && fs::read_dir(language).is_ok_and(|mut entries| entries.next().is_none())
        {
            fs::remove_dir(language).map_err(io_error(language))?;
        }
    }
    let _ = writeln!(
        out,
        "golden: accepted {} repositories, removed {removed} stale golden(s)",
        staged.len()
    );
    Ok(())
}
