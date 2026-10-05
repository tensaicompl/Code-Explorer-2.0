//! Stage 3's resolutions as graph rows (issue 49): one site per resolved call or
//! confirmed reference, a `CALLS` or `CALL_REFERENCE` edge for a drawn band, and a
//! candidate row for a band that is not drawn.
//!
//! A site's identity is 4.2.1's: its file, the node of the definition it is in (none
//! at file scope), its kind, and its fingerprint of the engine's node-type path from
//! that definition, its callee and receiver as resolution reads them
//! ([`split_callee`]), and its ordinal among the definition's sites with the same path
//! and texts. The ordinal counts every site the extraction positions in the source, in
//! source order, however each resolved, so a resolution changing never renumbers a
//! site. No line, offset or call index is part of an identity.
//!
//! A resolved site with no position in the raw source, or no node-type path, is not
//! persisted: it is counted ([`super::Diagnostics::unmaterialized`]) rather than given
//! a position it does not have. Stage 3's unconfirmed sites are not call sites and are
//! carried for counting only.

use std::collections::BTreeMap;

use pdx_engine::{Call, SiteRef};

use crate::bands::CandidateBand;
use crate::ids::{NodeId, SiteKey, ast_fingerprint};
use crate::kinds::{EdgeKind, SiteKind};
use crate::model::{CandidateSite, Edge, Site, Span};
use crate::resolve::registry::{DefinitionRef, SymbolRegistry};
use crate::resolve::stages::split_callee;

use super::containment::is_file_definition;
use super::{Builder, DeriveError, DeriveInput, Missing, Unmaterialized};

/// A drawn invocation: its caller, target and edge. What `TESTS` edges are made from.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DrawnCall {
    /// The definition the call is made in; `None` at file scope.
    pub(crate) caller: Option<DefinitionRef>,
    /// Its `CALLS` edge.
    pub(crate) edge: Edge,
}

/// The enclosing definition a call is attributed to, unless that is the file itself.
fn enclosing(registry: &SymbolRegistry, path: &str, call: &Call) -> Option<u32> {
    let index = call.caller?;
    let definition = registry.extract(path)?.definitions.get(index as usize)?;
    (!is_file_definition(definition, path)).then_some(index)
}

/// Every positioned site's ordinal: among the sites of one file with the same
/// enclosing definition, node-type path, callee and receiver, its position in source
/// order, counting from 1.
pub(crate) fn ordinals(registry: &SymbolRegistry) -> BTreeMap<SiteRef, u32> {
    type Group<'a> = (Option<u32>, &'a [String], &'a str, &'a str);
    let mut out = BTreeMap::new();
    for path in registry.files() {
        let Some(extract) = registry.extract(path) else {
            continue;
        };
        let mut groups: BTreeMap<Group<'_>, Vec<(u32, u32, u32)>> = BTreeMap::new();
        for (index, call) in extract.calls.iter().enumerate() {
            let (Some(span), false) = (call.span, call.ast_path.is_empty()) else {
                continue;
            };
            let Ok(index) = u32::try_from(index) else {
                continue;
            };
            let (receiver, name) = split_callee(&call.callee_text);
            groups
                .entry((
                    enclosing(registry, path, call),
                    call.ast_path.as_slice(),
                    name,
                    receiver.unwrap_or(""),
                ))
                .or_default()
                .push((span.start_byte, span.end_byte, index));
        }
        for mut members in groups.into_values() {
            members.sort_unstable();
            for (ordinal, (_, _, index)) in (1u32..).zip(members) {
                out.insert(
                    SiteRef {
                        rel_path: path.to_owned(),
                        call_index: index,
                    },
                    ordinal,
                );
            }
        }
    }
    out
}

fn model_span(span: pdx_engine::Span) -> Span {
    Span {
        start_byte: span.start_byte,
        end_byte: span.end_byte,
        start_line: span.start_line,
        start_col: span.start_col,
        end_line: span.end_line,
        end_col: span.end_col,
    }
}

/// A persisted site for a call, of `kind`, and the definition it is in; or what it
/// lacks to be one.
pub(crate) fn call_site(
    graph: &Builder,
    registry: &SymbolRegistry,
    ordinals: &BTreeMap<SiteRef, u32>,
    site_ref: &SiteRef,
    call: &Call,
    kind: SiteKind,
) -> Result<Result<(Site, Option<DefinitionRef>), Missing>, DeriveError> {
    let Some(span) = call.span else {
        return Ok(Err(Missing::Position));
    };
    let Some(&ordinal) = ordinals.get(site_ref) else {
        return Ok(Err(Missing::AstPath));
    };
    let path = site_ref.rel_path.as_str();
    let (receiver, name) = split_callee(&call.callee_text);
    let node_types: Vec<&str> = call.ast_path.iter().map(String::as_str).collect();
    let fingerprint = ast_fingerprint(&node_types, name, receiver.unwrap_or(""), ordinal)?;
    let caller = enclosing(registry, path, call).map(|index| DefinitionRef {
        path: path.to_owned(),
        index,
    });
    let enclosing_node = caller
        .as_ref()
        .map(|r| graph.definition_node(r).cloned())
        .transpose()?;
    let key = SiteKey::new(path, enclosing_node, kind, fingerprint);
    let file = graph.file_node(path)?.clone();
    let site = Site::new(&key, file, model_span(span))?.with_texts(Some(name), receiver);
    Ok(Ok((site, caller)))
}

/// Why a site draws no edge, in words.
fn reason(band: CandidateBand, candidates: usize) -> &'static str {
    match band {
        CandidateBand::Candidate if candidates > 1 => "several definitions survive every stage",
        CandidateBand::Candidate => "a candidate no stage validates",
        CandidateBand::External => "the target is outside the repository",
        CandidateBand::Blocked => "the callee's name is on the generic-name blocklist",
        CandidateBand::Unresolved => "no definition the name can reach",
        CandidateBand::Contradicted => "a precise index contradicts the edge",
    }
}

/// Persists every resolved site: drawn edges and candidate rows. Returns the drawn
/// invocations, for `TESTS` edges.
pub(crate) fn materialise(
    graph: &mut Builder,
    input: &DeriveInput<'_>,
) -> Result<Vec<DrawnCall>, DeriveError> {
    let registry = input.registry;
    let ordinals = ordinals(registry);
    graph.diagnostics.unconfirmed = input
        .resolution
        .unconfirmed
        .iter()
        .map(|u| (u.site_ref.clone(), u.reason))
        .collect();
    let mut drawn = Vec::new();
    for resolved in &input.resolution.resolutions {
        let resolution = &resolved.resolution;
        let band = resolution.band();
        let reference = resolved.site.is_reference;
        let kind = if reference {
            SiteKind::Reference
        } else {
            SiteKind::Call
        };
        let (site, caller) = match call_site(
            graph,
            registry,
            &ordinals,
            &resolved.site_ref,
            &resolved.site,
            kind,
        )? {
            Ok(found) => found,
            Err(missing) => {
                graph.diagnostics.unmaterialized.push(Unmaterialized {
                    site_ref: resolved.site_ref.clone(),
                    band,
                    missing,
                });
                continue;
            }
        };
        let path = resolved.site_ref.rel_path.as_str();
        let src = match &caller {
            Some(r) => graph.definition_node(r)?.clone(),
            None => graph.file_node(path)?.clone(),
        };
        let site_id = site.site_id.clone();
        let called_name = site.callee_text.clone().unwrap_or_default();
        graph.add_site(site)?;
        let engine = resolution.engine();
        if band.is_drawn() {
            let target = resolution.target().ok_or(DeriveError::Missing {
                path: path.to_owned(),
                detail: "a drawn resolution with no target",
            })?;
            let dst = graph.definition_node(target)?.clone();
            let edge_kind = if reference {
                EdgeKind::CallReference
            } else {
                EdgeKind::Calls
            };
            let mut edge = Edge::new(src, dst, edge_kind, band, Some(site_id));
            if let Some(e) = engine {
                edge.engine_score = Some(e.score);
                edge.engine_strategy.clone_from(&e.engine_strategy);
                edge.engine_candidates = Some(e.candidates);
            }
            graph.add_edge(edge.clone())?;
            if !reference {
                drawn.push(DrawnCall { caller, edge });
            }
        } else {
            let candidate_band =
                CandidateBand::try_from(band).map_err(|_| DeriveError::Missing {
                    path: path.to_owned(),
                    detail: "a band that is neither drawn nor a candidate band",
                })?;
            let mut candidate_ids: Vec<NodeId> = resolution
                .candidates()
                .iter()
                .map(|c| graph.definition_node(c).cloned())
                .collect::<Result<_, _>>()?;
            candidate_ids.sort();
            candidate_ids.dedup();
            let reason = reason(candidate_band, candidate_ids.len());
            graph.add_candidate(CandidateSite {
                site_id,
                src,
                callee_name: called_name,
                band: candidate_band,
                candidate_ids,
                engine_score: engine.map(|e| e.score),
                engine_strategy: engine.and_then(|e| e.engine_strategy.clone()),
                engine_candidates: engine.map(|e| e.candidates),
                reason: reason.to_owned(),
            })?;
        }
    }
    Ok(drawn)
}
