//! A0.1's site dataset (decision 64, issue 95): every call site of a checkout as
//! Stage 3 settles it, with the definitions it can name, for an audit of structural
//! accuracy against a semantic oracle (`scripts/a0/`).
//!
//! It runs the pipeline's own Stages 1 to 3 ([`discover`], [`ExtractStage`],
//! [`SymbolRegistry::build`], [`stages::resolve`]) in this process, with no cache, and
//! writes what they decided; it decides nothing and changes no stage. Beside each
//! site's band and target it records what A0.2's counterfactuals read: the engine's
//! answer (kept for a `blocked` site too), the definitions of the repository that
//! answer names, and where the callee's name and its receiver's first part come from.
//!
//! This is no oracle and not P5-06's benchmark. The oracle is a compiler-grade indexer
//! run beside it, and nothing here is ever its ground truth.
//!
//! One JSON object a line, in a fixed order: a header, then for each file in path
//! order the file, its definitions in index order and its sites in index order.

use std::io::Write;
use std::path::Path;

use pdx_core::bands::{EngineVerdict, from_engine};
use pdx_core::config::PdxConfig;
use pdx_core::index::discover;
use pdx_core::index::extract::{ExtractLimits, ExtractStage, FileOutcome};
use pdx_core::resolve::blocklist;
use pdx_core::resolve::registry::{DefinitionRef, NameProvenance, SymbolRegistry};
use pdx_core::resolve::stages::{self, EngineAnswer, Unconfirmed};
use pdx_engine::{Call, TextRef};
use serde_json::{Value, json};

/// The dataset's format; a change to any field's meaning moves it.
pub const FORMAT: u32 = 1;

/// The format of [`versions`]' report; a change to any field's meaning moves it.
pub const VERSIONS_FORMAT: u32 = 1;

/// The versions this executable was built with, as `pdx-bench versions` prints them:
/// the version the engine it links reports at run time, the `pdx_core` constants
/// compiled into it and the worker protocol it speaks. `scripts/a0/run.py` measures
/// nothing unless they are the checkout's own (issue 115), so a stale executable is
/// never recorded under the source's versions. Only what the executable itself holds
/// is reported: the engine's surface version is not in its interface.
pub fn versions() -> Value {
    use pdx_core::consts;
    json!({
        "format": VERSIONS_FORMAT,
        "engine_version": pdx_engine::Engine::version(),
        "pdx_core": {
            "ENGINE_VERSION": consts::ENGINE_VERSION,
            "EXTRACT_CACHE_FORMAT_VERSION": consts::EXTRACT_CACHE_FORMAT_VERSION,
            "LANGUAGE_MATRIX_VERSION": consts::LANGUAGE_MATRIX_VERSION,
            "SECRET_DETECTOR_VERSION": consts::SECRET_DETECTOR_VERSION,
            "SEGMENT_SCHEMA_VERSION": consts::SEGMENT_SCHEMA_VERSION,
        },
        "worker_protocol": pdx_engine::isolate::PROTOCOL,
    })
}

/// Why the dataset could not be made.
#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    /// A stage failed.
    #[error("{stage}: {message}")]
    Stage {
        /// The stage.
        stage: &'static str,
        /// What it reported.
        message: String,
    },
    /// The dataset could not be written.
    #[error("writing the dataset: {0}")]
    Write(#[from] std::io::Error),
}

fn failed<E: std::fmt::Display>(stage: &'static str) -> impl Fn(E) -> AuditError {
    move |e| AuditError::Stage {
        stage,
        message: e.to_string(),
    }
}

/// How many of each record the dataset holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Files.
    pub files: usize,
    /// Definitions.
    pub definitions: usize,
    /// Call sites, which Stage 3 resolved.
    pub resolved: usize,
    /// Sites that are not call sites.
    pub unconfirmed: usize,
}

/// One site of the dataset: Stage 3's outcome for it.
struct Site<'a> {
    call_index: u32,
    call: &'a Call,
    /// `resolved`, or why the site is not a call site.
    status: &'static str,
    band: Option<&'static str>,
    target: Option<&'a DefinitionRef>,
    candidates: &'a [DefinitionRef],
    engine: Option<&'a EngineAnswer>,
}

/// Writes the dataset of the checkout at `root` to `out`.
///
/// # Errors
///
/// [`AuditError`] when a stage fails or the dataset cannot be written.
pub fn write_dataset(root: &Path, out: &mut dyn Write) -> Result<Counts, AuditError> {
    let config = PdxConfig::load(root).map_err(failed("configuration"))?;
    let all = discover::discover(root, &config).map_err(failed("discover"))?;
    let (files, _unportable) = discover::split_unportable(all);
    // One worker with room for any file: the limits decide how files are batched,
    // never what is extracted.
    let limits = ExtractLimits {
        requested_workers: 1,
        memory_budget_bytes: 1 << 40,
    };
    let extracted = ExtractStage::new(root, &config.secrets, limits)
        .run(&files)
        .map_err(failed("extract"))?;
    let registry = SymbolRegistry::build(root, &files, extracted).map_err(failed("registry"))?;
    let report = stages::resolve(root, &registry).map_err(failed("resolve"))?;

    let mut counts = Counts::default();
    line(
        out,
        &json!({
            "t": "header",
            "format": FORMAT,
            "engine_health": format!("{:?}", report.engine_health),
        }),
    )?;
    let mut resolved = report.resolutions.iter().peekable();
    let mut unconfirmed = report.unconfirmed.iter().peekable();
    for path in registry.files() {
        let extract = registry.extract(path);
        line(
            out,
            &json!({
                "t": "file",
                "path": path,
                "language": registry.language(path).map(|l| l.id),
                "outcome": registry.outcome(path).map(outcome_name),
                "source_len": extract.map(|e| e.source_len),
            }),
        )?;
        counts.files += 1;
        counts.definitions += write_definitions(&registry, path, out)?;
        // The file's sites in index order: Stage 3 reports call sites and the rest
        // apart, each by path then index.
        loop {
            let r = resolved.peek().filter(|s| s.site_ref.rel_path == path);
            let u = unconfirmed.peek().filter(|s| s.site_ref.rel_path == path);
            let take_resolved = match (r, u) {
                (None, None) => break,
                (Some(r), Some(u)) => r.site_ref.call_index < u.site_ref.call_index,
                (Some(_), None) => true,
                (None, Some(_)) => false,
            };
            let site = if take_resolved {
                let Some(site) = resolved.next() else { break };
                counts.resolved += 1;
                Site {
                    call_index: site.site_ref.call_index,
                    call: &site.site,
                    status: "resolved",
                    band: Some(site.resolution.band().as_str()),
                    target: site.resolution.target(),
                    candidates: site.resolution.candidates(),
                    engine: site.resolution.engine(),
                }
            } else {
                let Some(site) = unconfirmed.next() else {
                    break;
                };
                counts.unconfirmed += 1;
                Site {
                    call_index: site.site_ref.call_index,
                    call: &site.site,
                    status: match site.reason {
                        Unconfirmed::TypedOnly => "typed_only",
                        Unconfirmed::Reference => "reference",
                        Unconfirmed::EngineSite => "engine_site",
                    },
                    band: None,
                    target: None,
                    candidates: &[],
                    engine: site.engine.as_ref(),
                }
            };
            line(out, &site_record(&registry, path, &site)?)?;
        }
    }
    Ok(counts)
}

/// Writes a file's definitions in index order; how many.
fn write_definitions(
    registry: &SymbolRegistry,
    path: &str,
    out: &mut dyn Write,
) -> Result<usize, AuditError> {
    let Some(extract) = registry.extract(path) else {
        return Ok(0);
    };
    for (index, definition) in (0u32..).zip(&extract.definitions) {
        let r = DefinitionRef {
            path: path.into(),
            index,
        };
        line(
            out,
            &json!({
                "t": "def",
                "path": path,
                "i": index,
                "name": definition.name,
                "qn": registry.qualified_name(&r),
                "kind": serde_json::to_value(definition.kind).map_err(std::io::Error::from)?,
                "engine_kind": definition.engine_kind,
                "span": definition.span.as_ref().map(|s| [s.start_byte, s.end_byte]),
                "body": definition.body_span.as_ref().map(|s| [s.start_byte, s.end_byte]),
                "parent": definition.parent,
                "declaring_type": registry.declaring_type(&r).map(|t| t.index),
            }),
        )?;
    }
    Ok(extract.definitions.len())
}

fn site_record(
    registry: &SymbolRegistry,
    path: &str,
    site: &Site<'_>,
) -> Result<Value, AuditError> {
    let call = site.call;
    let language = registry.language(path).map_or("", |l| l.id);
    let callee = registry
        .text(path, &call.callee_text)
        .map(std::borrow::Cow::into_owned)
        .unwrap_or_default();
    let callee_span = match &call.callee_text {
        TextRef::Source { start, end } => Some([*start, *end]),
        TextRef::Owned(_) => None,
    };
    let (receiver, name) = stages::split_callee(&callee);
    let head = receiver.map(first_part);
    let engine = site.engine.map(|e| {
        let verdict = match from_engine(e.score, e.strategy.as_str(), e.candidates) {
            EngineVerdict::Typed => "typed",
            EngineVerdict::HintFor(_) => "hint",
            EngineVerdict::Diagnostic => "diagnostic",
        };
        // The definitions the answer names, as Stage 3 maps a typed answer: in its
        // file, with exactly its qualified name.
        let mapped: Vec<Value> = e.target_rel_path.as_deref().map_or_else(Vec::new, |p| {
            registry
                .by_qualified_name(&e.target_qn)
                .iter()
                .filter(|r| r.path == p)
                .map(|r| json!([r.path, r.index]))
                .collect()
        });
        json!({
            "qn": e.target_qn,
            "path": e.target_rel_path,
            "score": e.score,
            "strategy": e.strategy.as_str(),
            "engine_strategy": e.engine_strategy,
            "candidates": e.candidates,
            "verdict": verdict,
            "mapped": mapped,
        })
    });
    // Stage 4 persists a site, and draws its edge, only where it has a position and a
    // node-type path (the site's identity, 4.2.1); a site typed resolution found itself
    // has neither and is counted in coverage alone.
    let materialized = call.span.is_some() && !call.ast_path.is_empty();
    let engine_site = registry
        .extract(path)
        .is_some_and(|e| site.call_index as usize >= e.calls.len());
    Ok(json!({
        "t": "site",
        "path": path,
        "i": site.call_index,
        "status": site.status,
        "materialized": materialized,
        "engine_site": engine_site,
        "band": site.band,
        "target": site.target.map(|t| json!([t.path, t.index])),
        "candidates": site.candidates.iter().map(|c| json!([c.path, c.index])).collect::<Vec<_>>(),
        "span": call.span.as_ref().map(|s| [s.start_byte, s.end_byte]),
        "line": call.span.as_ref().map(|s| s.start_line),
        "callee": callee,
        "callee_span": callee_span,
        "receiver_text": call.receiver_text,
        "receiver": receiver,
        "name": name,
        "caller": call.caller,
        "lexical": serde_json::to_value(call.lexical).map_err(std::io::Error::from)?,
        "typed_only": call.typed_only,
        "is_reference": call.is_reference,
        "blocklisted": blocklist::is_blocked(language, name),
        "name_provenance": provenance(registry, path, name),
        "head": head,
        "head_provenance": head.map(|h| provenance(registry, path, h)),
        "engine": engine,
    }))
}

/// A receiver's first part, as Stage 3 reads it: up to its first `.`, `->`, `::` or
/// `\`.
fn first_part(receiver: &str) -> &str {
    ["->", "::", ".", "\\"]
        .iter()
        .filter_map(|sep| receiver.find(sep))
        .min()
        .map_or(receiver, |i| &receiver[..i])
}

/// Where a name a file uses comes from, as the registry says.
fn provenance(registry: &SymbolRegistry, path: &str, name: &str) -> &'static str {
    match registry.name_provenance(path, name) {
        NameProvenance::Defined(_) => "defined",
        NameProvenance::InternalImport(_) => "internal_import",
        NameProvenance::ExternalImport(_) => "external_import",
        NameProvenance::Unknown => "unknown",
    }
}

fn outcome_name(outcome: &FileOutcome) -> &'static str {
    match outcome {
        FileOutcome::Extracted { .. } => "extracted",
        _ => "not_extracted",
    }
}

fn line(out: &mut dyn Write, value: &Value) -> Result<(), AuditError> {
    serde_json::to_writer(&mut *out, value).map_err(std::io::Error::from)?;
    out.write_all(b"\n")?;
    Ok(())
}
