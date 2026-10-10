//! The indexing pipeline (specification 4.5): one function, [`build_segment`], runs
//! its stages in order over a checkout and writes one segment.
//!
//! 1. [`discover`]: the checkout's files, each with its language and disposition; files
//!    whose path no host-independent identity can be made from are held back
//!    ([`discover::split_unportable`], issue 53).
//! 2. [`extract`]: the engine's extraction of each candidate, through the cache.
//! 3. Resolve ([`crate::resolve`]): the symbol registry, then every call site's band.
//! 4. [`derive`]: the graph with its identities, contracts, layer roles and metrics,
//!    and the coverage rows ([`crate::coverage`]).
//! 5. [`write`]: the segment, through the segment writer.
//!
//! Each stage runs in a `tracing` span and is reported to an optional progress
//! callback; neither has any effect on what is built. Whether the segment is degraded
//! is decided once, from every file's outcome ([`report`]). A fatal error in any stage
//! returns that stage's error and publishes nothing.

pub mod derive;
pub mod discover;
pub mod extract;
pub mod report;
pub mod write;

use std::path::{Path, PathBuf};

use crate::config::{ConfigError, PdxConfig};
use crate::coverage::{self, CoverageError};
use crate::ids::RepoId;
use crate::index::derive::{DeriveError, DeriveInput};
use crate::index::discover::DiscoverError;
use crate::index::extract::{
    BackendFactory, ExtractCache, ExtractError, ExtractLimits, ExtractStage,
};
use crate::resolve::registry::{RegistryError, SymbolRegistry};
use crate::resolve::stages::{ResolutionError, resolve};
use crate::segment::{SegmentError, SegmentMeta, SegmentProfile, SegmentWriter};

pub use report::{
    DegradedReason, DeriveSummary, FileCounts, ResolutionSummary, RowCounts, SegmentReport,
    SegmentStatus,
};

/// A stage of the pipeline, as progress reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Stage {
    /// Stage 1.
    Discover,
    /// Stage 2.
    Extract,
    /// Stage 3.
    Resolve,
    /// Stage 4.
    Derive,
    /// Stage 5.
    Write,
}

impl Stage {
    /// The stage's name, as its `tracing` span is named.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Discover => "discover",
            Self::Extract => "extract",
            Self::Resolve => "resolve",
            Self::Derive => "derive",
            Self::Write => "write",
        }
    }
}

/// What a build reports as it goes: each stage started, then finished with the number
/// of things it produced. Reported from the building thread, in stage order, the same
/// for the same checkout; never a file's content or a path outside the checkout.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Progress {
    /// A stage started.
    Started(Stage),
    /// A stage finished.
    Finished {
        /// The stage.
        stage: Stage,
        /// What it produced: the files discovered (Discover), the files given to
        /// extraction (Extract), the call sites resolved (Resolve), the graph's nodes
        /// (Derive), the segment's size in bytes (Write).
        items: u64,
    },
}

/// What to build: a structural segment of the checkout at `root`, at `destination`.
///
/// Every input the pipeline depends on is here or in the checkout (its `pdx.toml`):
/// nothing is read from the environment, and the extraction limits come resolved.
pub struct IndexRequest<'a> {
    /// The checkout.
    pub root: &'a Path,
    /// The repository's registered identity.
    pub repo_id: RepoId,
    /// The repository's URL (`meta.repo_url`).
    pub repo_url: String,
    /// The repository's name, shown on its node; no part of any identity.
    pub repo_name: String,
    /// The commit the checkout is, in lower-case hex (`meta.commit_sha`).
    pub commit_sha: String,
    /// Stage 2's workers and memory budget, already resolved by the caller.
    pub limits: ExtractLimits,
    /// The extraction cache, if any.
    pub cache: Option<&'a dyn ExtractCache>,
    /// The extraction backend, if not the production one.
    pub backend: Option<&'a BackendFactory<'a>>,
    /// Where the segment is written: nothing may be there.
    pub destination: PathBuf,
    /// Where the segment is built before it is written, if not the system's
    /// temporary directory.
    pub build_dir: Option<PathBuf>,
    /// Called as each stage starts and finishes.
    pub progress: Option<&'a dyn Fn(&Progress)>,
}

/// Why a build failed. Each stage's own error, unchanged.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    /// The repository's `pdx.toml` could not be read or is invalid.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// Stage 1 failed.
    #[error(transparent)]
    Discover(#[from] DiscoverError),
    /// Stage 2 failed: a changed checkout, an engine that cannot start, a worker that
    /// did not answer in time.
    #[error(transparent)]
    Extract(#[from] ExtractError),
    /// The symbol registry could not be built.
    #[error(transparent)]
    Registry(#[from] RegistryError),
    /// Stage 3 failed.
    #[error(transparent)]
    Resolve(#[from] ResolutionError),
    /// Stage 4 failed.
    #[error(transparent)]
    Derive(#[from] DeriveError),
    /// Coverage could not be counted.
    #[error(transparent)]
    Coverage(#[from] CoverageError),
    /// The segment could not be written, or the request's meta or destination cannot be
    /// (checked before any stage runs).
    #[error(transparent)]
    Write(#[from] SegmentError),
}

/// Builds the structural segment of a checkout: Stages 1 to 5 of 4.5.
///
/// The request's meta and destination are checked first, so a build that could not be
/// written does no work. The segment is degraded, and the build still succeeds, when a
/// [`DegradedReason`] holds.
///
/// # Errors
///
/// [`IndexError`] with the failing stage's own error. Nothing is published at the
/// destination then.
pub fn build_segment(request: IndexRequest<'_>) -> Result<SegmentReport, IndexError> {
    let IndexRequest {
        root,
        repo_id,
        repo_url,
        repo_name,
        commit_sha,
        limits,
        cache,
        backend,
        destination,
        build_dir,
        progress,
    } = request;
    let build = tracing::info_span!("build_segment", repo_id = %repo_id, commit = %commit_sha);
    let _build = build.enter();
    let report = Reporter(progress);
    let meta = SegmentMeta::new(
        repo_id.clone(),
        &repo_url,
        &commit_sha,
        SegmentProfile::Structural,
        Vec::new(),
    );
    meta.check()?;
    SegmentWriter::check_destination(&destination)?;

    let found = report.stage(Stage::Discover, |span| {
        let found = discovered(root)?;
        span.record("files", found.files.len() + found.unportable.len());
        Ok(((found.files.len() + found.unportable.len()) as u64, found))
    })?;
    let extracted = report.stage(Stage::Extract, |span| {
        span.record("files", found.files.len());
        let mut stage = ExtractStage::new(root, &found.config.secrets, limits);
        if let Some(cache) = cache {
            stage = stage.with_cache(cache);
        }
        if let Some(backend) = backend {
            stage = stage.with_backend(backend);
        }
        let extracted = stage.run(&found.files)?;
        Ok((extracted.files.len() as u64, extracted))
    })?;
    let extract_stats = extracted.stats.clone();
    let (registry, resolution) = report.stage(Stage::Resolve, |span| {
        let registry = SymbolRegistry::build(root, &found.files, extracted)?;
        let resolution = resolve(root, &registry)?;
        span.record("call_sites", resolution.resolutions.len());
        Ok((resolution.resolutions.len() as u64, (registry, resolution)))
    })?;
    let (graph, coverage, files, degraded) = report.stage(Stage::Derive, |span| {
        let graph = derive::derive(&DeriveInput {
            repo: &repo_id,
            repo_name: &repo_name,
            registry: &registry,
            resolution: &resolution,
            root,
            config: &found.config,
        })?;
        let coverage = coverage::rows(&registry, &resolution, &found.unportable)?;
        let (files, degraded) = report::assess(&registry, &found.unportable);
        span.record("nodes", graph.nodes.len());
        Ok((graph.nodes.len() as u64, (graph, coverage, files, degraded)))
    })?;
    let summary = Summary::of(&graph, &coverage, &resolution);
    let segment = report.stage(Stage::Write, |span| {
        let data = write::segment_data(meta.clone(), graph, coverage.clone());
        let segment = write::write(&data, &destination, build_dir.as_deref())?;
        span.record("bytes", segment.size_bytes);
        Ok((segment.size_bytes, segment))
    })?;

    let status = if degraded.is_empty() {
        SegmentStatus::Ready
    } else {
        tracing::warn!(reasons = degraded.len(), "the segment is degraded");
        SegmentStatus::Degraded
    };
    let mut unportable: Vec<String> = found.unportable.into_iter().map(|f| f.path).collect();
    unportable.sort();
    Ok(SegmentReport {
        meta,
        segment,
        status,
        degraded,
        coverage,
        files,
        unportable,
        rows: summary.rows,
        resolution: summary.resolution,
        derive: summary.derive,
        extract: extract_stats,
    })
}

/// Stage 1's result: the configuration, the files the later stages index, and the
/// files held back for an unportable path (issue 53).
struct Found {
    config: PdxConfig,
    files: Vec<discover::DiscoveredFile>,
    unportable: Vec<discover::DiscoveredFile>,
}

/// Stage 1: the repository's configuration and files.
fn discovered(root: &Path) -> Result<Found, IndexError> {
    let config = PdxConfig::load(root)?;
    let all = discover::discover(root, &config)?;
    let (files, unportable) = discover::split_unportable(all);
    Ok(Found {
        config,
        files,
        unportable,
    })
}

/// Runs each stage in its span, between its progress reports.
struct Reporter<'a>(Option<&'a dyn Fn(&Progress)>);

impl Reporter<'_> {
    fn emit(&self, progress: &Progress) {
        if let Some(callback) = self.0 {
            callback(progress);
        }
    }

    /// Runs `work` as `stage`: started, in the stage's span, then finished with the
    /// count `work` returns beside its result.
    fn stage<T>(
        &self,
        stage: Stage,
        work: impl FnOnce(&tracing::Span) -> Result<(u64, T), IndexError>,
    ) -> Result<T, IndexError> {
        self.emit(&Progress::Started(stage));
        let span = match stage {
            Stage::Discover => tracing::info_span!("discover", files = tracing::field::Empty),
            Stage::Extract => tracing::info_span!("extract", files = tracing::field::Empty),
            Stage::Resolve => tracing::info_span!("resolve", call_sites = tracing::field::Empty),
            Stage::Derive => tracing::info_span!("derive", nodes = tracing::field::Empty),
            Stage::Write => tracing::info_span!("write", bytes = tracing::field::Empty),
        };
        let (items, result) = {
            let _entered = span.enter();
            work(&span)?
        };
        self.emit(&Progress::Finished { stage, items });
        Ok(result)
    }
}

/// The report's counts of what was built.
struct Summary {
    rows: RowCounts,
    resolution: ResolutionSummary,
    derive: DeriveSummary,
}

impl Summary {
    fn of(
        graph: &derive::DerivedGraph,
        coverage: &[crate::model::Coverage],
        resolution: &crate::resolve::stages::ResolveReport,
    ) -> Self {
        Self {
            rows: RowCounts {
                files: graph.files.len() as u64,
                nodes: graph.nodes.len() as u64,
                sites: graph.sites.len() as u64,
                edges: graph.edges.len() as u64,
                candidates: graph.candidates.len() as u64,
                contracts: graph.contracts.len() as u64,
                metrics: graph.metrics.len() as u64,
                coverage: coverage.len() as u64,
            },
            resolution: ResolutionSummary {
                call_sites: resolution.resolutions.len() as u64,
                unconfirmed: resolution.unconfirmed.len() as u64,
                engine_health: resolution.engine_health,
            },
            derive: DeriveSummary {
                unmaterialized: graph.diagnostics.unmaterialized.len() as u64,
                routes: graph.diagnostics.routes.len() as u64,
                contracts: graph.diagnostics.contracts.len() as u64,
                metrics: graph.diagnostics.metrics.len() as u64,
            },
        }
    }
}
