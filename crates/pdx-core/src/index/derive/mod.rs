//! Stage 4, derive (specification 4.5): the persistent graph of one repository, built
//! from the symbol registry and Stage 3's resolutions.
//!
//! This is where semantic facts first get persistent identities (4.2.1): the
//! repository, its folders and files, every extracted definition, the modules of the
//! language matrix's rules, call and reference sites, and the routes and tests derived
//! from them. Every id comes from the fields 4.2.1 names and nothing else, so no line,
//! offset, insertion order or checkout location reaches one.
//!
//! The parts:
//!
//! - [`containment`]: `Repo`, `Folder` and `File` nodes and file records, then one node
//!   per extracted definition, physically parented to the definition that encloses it
//!   or its file (Appendix B.3).
//! - [`modules`]: one `Module` node per module of the registry (issue 48), and
//!   `props.module` on the symbols in it.
//! - [`calls`]: Stage 3's resolutions as sites, drawn `CALLS` and `CALL_REFERENCE`
//!   edges and non-drawn candidate rows (issue 49).
//! - [`tests`]: the matrix's test rules (issue 31), `Test` nodes and `TESTS` edges
//!   (Appendix B.5).
//! - [`routes`]: `Route` nodes and `DEFINES_ROUTE` edges for Spring, `FastAPI` and
//!   Express.
//! - [`entry`]: the entry points Appendix B.2 names that the facts prove.
//!
//! The result ([`DerivedGraph`]) is sorted by stored identity, ready for the segment
//! writer; nothing here writes a segment.

use std::collections::{BTreeMap, BTreeSet};

use pdx_engine::SiteRef;
use serde_json::Value;

use crate::bands::Band;
use crate::ids::{EdgeId, IdError, NodeId, RepoId, SiteId};
use crate::model::{CandidateSite, Edge, FileRecord, Node, Site};
use crate::resolve::registry::{DefinitionRef, ModuleKey, SymbolRegistry};
use crate::resolve::stages::{ResolveReport, Unconfirmed};

pub mod calls;
pub mod containment;
pub mod entry;
pub mod modules;
pub mod routes;
pub mod tests;

/// What Stage 4 derives from.
pub struct DeriveInput<'a> {
    /// The repository's identity.
    pub repo: &'a RepoId,
    /// The repository's name, shown on its node; no part of any id.
    pub repo_name: &'a str,
    /// Stage 3's registry: the files, their extractions, modules and imports.
    pub registry: &'a SymbolRegistry,
    /// Stage 3's resolutions.
    pub resolution: &'a ResolveReport,
}

/// Why a call or reference site is not persisted: a site's identity and row need its
/// position in the source and its node-type path, and a fact without them is counted
/// rather than given a position it does not have.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Missing {
    /// The engine recorded no position in the raw source: a site found in preprocessed
    /// text, or one typed resolution found itself.
    Position,
    /// The engine recorded no node-type path.
    AstPath,
}

/// A resolved site that is not persisted, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unmaterialized {
    /// The site.
    pub site_ref: SiteRef,
    /// The band it resolved to.
    pub band: Band,
    /// What it lacks.
    pub missing: Missing,
}

/// What route derivation found and could not use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteDiagnostic {
    /// The file the evidence is in.
    pub path: String,
    /// The route's method, as written.
    pub method: String,
    /// The route's path, as written.
    pub route: String,
    /// What is missing.
    pub problem: RouteProblem,
}

/// Why route evidence did not give a complete route.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RouteProblem {
    /// The registration names no handler the facts identify (an inline function, an
    /// expression, a name nothing resolves).
    NoHandler,
    /// The route is drawn, but no source site evidences it: the framework's
    /// annotation is not a site the engine records.
    NoSite,
}

/// What the stage reports beside the graph, for coverage.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diagnostics {
    /// Stage 3's sites that are not call sites, carried for counting (never rows).
    pub unconfirmed: Vec<(SiteRef, Unconfirmed)>,
    /// Resolved sites that are not persisted, by site.
    pub unmaterialized: Vec<Unmaterialized>,
    /// Route evidence that did not give a complete route.
    pub routes: Vec<RouteDiagnostic>,
}

/// The persistent graph of one repository, sorted by stored identity: files by path,
/// nodes by id, sites by id, edges by id, candidates by site.
#[derive(Clone, Debug, PartialEq)]
pub struct DerivedGraph {
    /// One record per discovered file.
    pub files: Vec<FileRecord>,
    /// Every node.
    pub nodes: Vec<Node>,
    /// Every evidence site.
    pub sites: Vec<Site>,
    /// Every edge.
    pub edges: Vec<Edge>,
    /// Every non-drawn call site.
    pub candidates: Vec<CandidateSite>,
    /// The repository's node.
    pub repo_node: NodeId,
    /// Each file's node, by path.
    pub file_nodes: BTreeMap<String, NodeId>,
    /// Each extracted definition's node: the one map every part of the stage reads.
    /// The engine's file-level module definition maps to its file's node.
    pub definition_nodes: BTreeMap<DefinitionRef, NodeId>,
    /// Each module's node.
    pub module_nodes: BTreeMap<ModuleKey, NodeId>,
    /// What the stage could not persist, for coverage.
    pub diagnostics: Diagnostics,
}

impl DerivedGraph {
    /// A node, by id.
    pub fn node(&self, id: &NodeId) -> Option<&Node> {
        self.nodes
            .binary_search_by(|n| n.node_id.cmp(id))
            .ok()
            .map(|i| &self.nodes[i])
    }
}

/// Why the stage could not derive the graph.
#[derive(Debug, thiserror::Error)]
pub enum DeriveError {
    /// An identity could not be computed (a NUL in a field).
    #[error("an identity could not be computed: {0}")]
    Id(#[from] IdError),
    /// Two different nodes with one id: two facts that 4.2.1 cannot tell apart.
    #[error("two different nodes share the id {0}")]
    DuplicateNode(NodeId),
    /// Two different sites with one id.
    #[error("two different sites share the id {0}")]
    DuplicateSite(SiteId),
    /// Two different edges with one id.
    #[error("two different edges share the id {0}")]
    DuplicateEdge(EdgeId),
    /// A path that is not repository-relative POSIX: a host-native path, which would
    /// give the same file different identities on different hosts.
    #[error("{0}: not a repository-relative POSIX path")]
    NotRepositoryPath(String),
    /// A fact refers to something the stage has no node for.
    #[error("{path}: {detail}")]
    Missing {
        /// The file.
        path: String,
        /// What is missing.
        detail: &'static str,
    },
}

/// Derives the persistent graph of a repository.
///
/// # Errors
///
/// [`DeriveError`] when a path is not repository-relative POSIX, an identity cannot be
/// computed, two different facts would share an identity, or a resolution names a
/// definition the registry does not have.
pub fn derive(input: &DeriveInput<'_>) -> Result<DerivedGraph, DeriveError> {
    let tests = tests::test_definitions(input.registry);
    let mut graph = containment::build(input, &tests)?;
    modules::build(&mut graph, input)?;
    let drawn = calls::materialise(&mut graph, input)?;
    tests::link(&mut graph, &tests, &drawn)?;
    routes::build(&mut graph, input)?;
    entry::mark(&mut graph, input, &tests)?;
    Ok(graph.finish())
}

/// The graph as it is built: every part adds through it, and it refuses two different
/// facts with one identity rather than keep either.
pub(crate) struct Builder {
    pub(crate) files: BTreeMap<String, FileRecord>,
    pub(crate) nodes: BTreeMap<NodeId, Node>,
    pub(crate) sites: BTreeMap<SiteId, Site>,
    pub(crate) edges: BTreeMap<EdgeId, Edge>,
    pub(crate) candidates: BTreeMap<SiteId, CandidateSite>,
    pub(crate) repo_node: NodeId,
    pub(crate) file_nodes: BTreeMap<String, NodeId>,
    pub(crate) folder_nodes: BTreeMap<String, NodeId>,
    pub(crate) definition_nodes: BTreeMap<DefinitionRef, NodeId>,
    /// The engine's file-level module definitions, which are their files, not symbols.
    pub(crate) file_definitions: BTreeSet<DefinitionRef>,
    pub(crate) module_nodes: BTreeMap<ModuleKey, NodeId>,
    pub(crate) diagnostics: Diagnostics,
}

impl Builder {
    pub(crate) fn add_node(&mut self, node: Node) -> Result<(), DeriveError> {
        match self.nodes.get(&node.node_id) {
            Some(existing) if *existing == node => Ok(()),
            Some(_) => Err(DeriveError::DuplicateNode(node.node_id)),
            None => {
                self.nodes.insert(node.node_id.clone(), node);
                Ok(())
            }
        }
    }

    pub(crate) fn add_site(&mut self, site: Site) -> Result<(), DeriveError> {
        match self.sites.get(&site.site_id) {
            Some(existing) if *existing == site => Ok(()),
            Some(_) => Err(DeriveError::DuplicateSite(site.site_id)),
            None => {
                self.sites.insert(site.site_id.clone(), site);
                Ok(())
            }
        }
    }

    pub(crate) fn add_edge(&mut self, edge: Edge) -> Result<(), DeriveError> {
        match self.edges.get(&edge.edge_id) {
            Some(existing) if *existing == edge => Ok(()),
            Some(_) => Err(DeriveError::DuplicateEdge(edge.edge_id)),
            None => {
                self.edges.insert(edge.edge_id.clone(), edge);
                Ok(())
            }
        }
    }

    pub(crate) fn add_candidate(&mut self, candidate: CandidateSite) -> Result<(), DeriveError> {
        match self.candidates.get(&candidate.site_id) {
            Some(existing) if *existing == candidate => Ok(()),
            Some(_) => Err(DeriveError::DuplicateSite(candidate.site_id)),
            None => {
                self.candidates.insert(candidate.site_id.clone(), candidate);
                Ok(())
            }
        }
    }

    /// Sets a property of a node the stage made.
    pub(crate) fn set_prop(&mut self, id: &NodeId, key: &str, value: Value) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.props.insert(key, value);
        }
    }

    /// The node of a definition.
    pub(crate) fn definition_node(&self, r: &DefinitionRef) -> Result<&NodeId, DeriveError> {
        self.definition_nodes
            .get(r)
            .ok_or_else(|| DeriveError::Missing {
                path: r.path.clone(),
                detail: "a definition with no node",
            })
    }

    /// The node of a file.
    pub(crate) fn file_node(&self, path: &str) -> Result<&NodeId, DeriveError> {
        self.file_nodes
            .get(path)
            .ok_or_else(|| DeriveError::Missing {
                path: path.to_owned(),
                detail: "a file with no node",
            })
    }

    fn finish(mut self) -> DerivedGraph {
        // Diagnostics in site and path order, whatever order the input came in.
        let diagnostics = &mut self.diagnostics;
        diagnostics.unconfirmed.sort_by(|a, b| a.0.cmp(&b.0));
        diagnostics
            .unmaterialized
            .sort_by(|a, b| a.site_ref.cmp(&b.site_ref));
        diagnostics.routes.sort_by(|a, b| {
            (&a.path, &a.method, &a.route, a.problem)
                .cmp(&(&b.path, &b.method, &b.route, b.problem))
        });
        DerivedGraph {
            files: self.files.into_values().collect(),
            nodes: self.nodes.into_values().collect(),
            sites: self.sites.into_values().collect(),
            edges: self.edges.into_values().collect(),
            candidates: self.candidates.into_values().collect(),
            repo_node: self.repo_node,
            file_nodes: self.file_nodes,
            definition_nodes: self.definition_nodes,
            module_nodes: self.module_nodes,
            diagnostics: self.diagnostics,
        }
    }
}
