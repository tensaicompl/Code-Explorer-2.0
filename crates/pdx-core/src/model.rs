//! The graph's rows: nodes, evidence sites, edges, non-drawn call sites, contracts,
//! metrics and coverage, shaped as the segment stores them (specification 4.3).
//!
//! Each field is a column of its table, with the same name; a column that may be NULL
//! is an `Option`. Closed vocabularies are typed, and free-form extras (`props`) are a
//! map kept in key order, so the same row always serialises to the same bytes. These
//! are the rows only: building them is the pipeline's, storing them the segment
//! writer's.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bands::{Band, CandidateBand};
use crate::ids::{self, AstFingerprint, EdgeId, IdError, NodeId, NodeKey, SiteId, SiteKey};
use crate::kinds::{ContractKind, EdgeKind, LayerRole, NodeKind, SiteKind};
use crate::vocabulary::vocabulary;

/// A row's free-form properties (the `props` column): language-specific extras,
/// visibility, `is_entry_point`, `is_test` and the like.
///
/// Kept in key order at every level, so equal properties serialise identically
/// whatever order they were set in. Empty serialises as `{}`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Props(BTreeMap<String, Value>);

impl Props {
    /// Sets a property, replacing any value it had.
    pub fn insert(&mut self, key: &str, value: Value) {
        self.0.insert(key.to_owned(), value);
    }

    /// A property's value.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A node (the `nodes` table).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Its identity, from its [`NodeKey`] alone.
    pub node_id: NodeId,
    /// Its kind.
    pub kind: NodeKind,
    /// Its short name.
    pub name: String,
    /// Its qualified name.
    pub qualified_name: String,
    /// The `File` node it is defined in; `None` for nodes in no file.
    pub file_id: Option<NodeId>,
    /// Its parent in the containment tree (`CONTAINS`); `None` for a repository.
    pub parent_id: Option<NodeId>,
    /// Its first line, when it has one. Metadata, never identity.
    pub start_line: Option<u32>,
    /// Its last line, when it has one. Metadata, never identity.
    pub end_line: Option<u32>,
    /// Its layer role, for a module or class that has one.
    pub layer_role: Option<LayerRole>,
    /// Its signature as declared.
    pub signature: Option<String>,
    /// Its documentation.
    pub doc: Option<String>,
    /// Everything else.
    pub props: Props,
}

impl Node {
    /// A node identified by `key`, with its kind and qualified name taken from it and
    /// nothing else set. Lines and every other field are the caller's to fill in, and
    /// none of them is part of the id.
    ///
    /// # Errors
    ///
    /// [`IdError::Nul`] if a field of the key contains a NUL byte.
    pub fn new(key: &NodeKey, name: &str) -> Result<Self, IdError> {
        Ok(Self {
            node_id: key.node_id()?,
            kind: key.kind,
            name: name.to_owned(),
            qualified_name: key.qualified_name.clone(),
            file_id: None,
            parent_id: None,
            start_line: None,
            end_line: None,
            layer_role: None,
            signature: None,
            doc: None,
            props: Props::default(),
        })
    }

    /// The node with its lines set. Lines are where the node is, not what it is: the
    /// id stays as the key made it.
    #[must_use]
    pub fn with_lines(mut self, start_line: Option<u32>, end_line: Option<u32>) -> Self {
        self.start_line = start_line;
        self.end_line = end_line;
        self
    }
}

/// Where a site is in its file: byte offsets, and lines and columns. Stored on the
/// site, and no part of its identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Span {
    /// First byte.
    pub start_byte: u32,
    /// Byte past the end.
    pub end_byte: u32,
    /// First line.
    pub start_line: u32,
    /// First column.
    pub start_col: u32,
    /// Last line.
    pub end_line: u32,
    /// Column past the end.
    pub end_col: u32,
}

/// An evidence site: a call, reference, import or other place a relation is seen (the
/// `sites` table).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Site {
    /// Its identity, from its [`SiteKey`] alone.
    pub site_id: SiteId,
    /// The `File` node it is in.
    pub file_id: NodeId,
    /// The definition it is in; `None` at the top of a file.
    pub enclosing_node_id: Option<NodeId>,
    /// What it is.
    pub site_kind: SiteKind,
    /// Where it is in its definition's syntax tree.
    pub ast_fingerprint: AstFingerprint,
    /// Where it is in its file.
    #[serde(flatten)]
    pub span: Span,
    /// The callee's text, or for a site other than a call, the site's text.
    pub callee_text: Option<String>,
    /// The receiver's text.
    pub receiver_text: Option<String>,
}

impl Site {
    /// The site `key` identifies, in the file `file_id`, at `span`. The span is stored
    /// and is no part of the id.
    ///
    /// # Errors
    ///
    /// [`IdError::Nul`] if the key's path contains a NUL byte.
    pub fn new(key: &SiteKey, file_id: NodeId, span: Span) -> Result<Self, IdError> {
        Ok(Self {
            site_id: key.site_id()?,
            file_id,
            enclosing_node_id: key.enclosing_node_id.clone(),
            site_kind: key.site_kind,
            ast_fingerprint: key.ast_fingerprint.clone(),
            span,
            callee_text: None,
            receiver_text: None,
        })
    }

    /// The site with its callee and receiver texts set.
    #[must_use]
    pub fn with_texts(mut self, callee_text: Option<&str>, receiver_text: Option<&str>) -> Self {
        self.callee_text = callee_text.map(str::to_owned);
        self.receiver_text = receiver_text.map(str::to_owned);
        self
    }
}

/// An edge (the `edges` table).
///
/// The band is the classification every consumer reads; the engine's score, strategy
/// and candidate count are kept beside it, verbatim, for calibration, and never decide
/// it. Observation at run time is a separate flag that changes neither the band nor
/// the identity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    /// Its identity: source, target, kind and site.
    pub edge_id: EdgeId,
    /// Where it starts.
    pub src: NodeId,
    /// Where it ends.
    pub dst: NodeId,
    /// Its kind.
    pub kind: EdgeKind,
    /// How sure the graph is of it.
    pub band: Band,
    /// Whether it was seen at run time. Always false in a repository's segment.
    pub observed: bool,
    /// The engine's score, verbatim, when the engine answered for the site.
    pub engine_score: Option<f64>,
    /// The engine's strategy, verbatim, as the engine spelled it.
    pub engine_strategy: Option<String>,
    /// How many candidates the engine considered.
    pub engine_candidates: Option<u32>,
    /// The site it was seen at; `None` only for an edge no site evidences.
    pub site_id: Option<SiteId>,
    /// How many times it occurs, 1 unless aggregated.
    pub weight: u32,
    /// Everything else.
    pub props: Props,
}

impl Edge {
    /// An edge with the band the caller decided, its id computed from source, target,
    /// kind and site, weight 1, not observed, and no engine fields.
    pub fn new(
        src: NodeId,
        dst: NodeId,
        kind: EdgeKind,
        band: Band,
        site_id: Option<SiteId>,
    ) -> Self {
        Self {
            edge_id: ids::edge_id(&src, &dst, kind, site_id.as_ref()),
            src,
            dst,
            kind,
            band,
            observed: false,
            engine_score: None,
            engine_strategy: None,
            engine_candidates: None,
            site_id,
            weight: 1,
            props: Props::default(),
        }
    }

    /// The edge with the engine's answer for its site recorded, verbatim. The band is
    /// left exactly as it was.
    #[must_use]
    pub fn with_engine(mut self, score: f64, strategy: &str, candidates: u32) -> Self {
        self.engine_score = Some(score);
        self.engine_strategy = Some(strategy.to_owned());
        self.engine_candidates = Some(candidates);
        self
    }
}

/// A call site that draws no edge (the `candidates` table): one row per site, with the
/// targets that survived, if any.
///
/// Its band is a [`CandidateBand`], so a drawn band cannot be stored here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CandidateSite {
    /// The site.
    pub site_id: SiteId,
    /// The node the call is made from.
    pub src: NodeId,
    /// The name called.
    pub callee_name: String,
    /// Why nothing is drawn.
    pub band: CandidateBand,
    /// The targets that survived: a set, which a segment stores sorted by id.
    pub candidate_ids: Vec<NodeId>,
    /// The engine's score, verbatim.
    pub engine_score: Option<f64>,
    /// The engine's strategy, verbatim.
    pub engine_strategy: Option<String>,
    /// Why, in words.
    pub reason: String,
}

vocabulary! {
    /// How firmly a contract's namespace is established (4.7.1).
    pub enum IdentityStrength {
        /// `exact`.
        Exact => "exact",
        /// `declared`.
        Declared => "declared",
        /// `unresolved`.
        Unresolved => "unresolved",
    }
}

vocabulary! {
    /// Which side of a contract a repository is on.
    pub enum ContractDirection {
        /// It provides the contract.
        Provides => "provides",
        /// It consumes it.
        Consumes => "consumes",
        /// Both.
        Both => "both",
    }
}

/// A contract a repository provides or consumes (the `contracts` table).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contract {
    /// The contract's node id: estate-level, over its `namespace_key`.
    pub contract_id: NodeId,
    /// Its kind.
    pub kind: ContractKind,
    /// The normalised matching key.
    pub key: String,
    /// The identity and the key; prefixed `unresolved:<repo_id>:` when the identity
    /// is unresolved.
    pub namespace_key: String,
    /// How firmly the identity is established.
    pub identity_strength: IdentityStrength,
    /// The node in this repository that defines or produces it; `None` when the
    /// repository only consumes it.
    pub owner_node_id: Option<NodeId>,
    /// Which side the repository is on.
    pub direction: ContractDirection,
    /// Everything else.
    pub props: Props,
}

/// One metric of one node (the `metrics` table). Metric names are open: 4.12.1's
/// (`loc`, `cyclomatic`, `fan_in` and the rest) and any a later analysis adds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Metric {
    /// The node.
    pub node_id: NodeId,
    /// The metric's name.
    pub metric: String,
    /// Its value.
    pub value: f64,
}

/// A count per band, every band always present, in 4.2.2's order: the `by_band`
/// column of coverage.
///
/// Serialised as an object from each band's name, as [`Band`] spells it, to its count,
/// in [`Band::ALL`]'s order whatever order the counts were added in. Reading one
/// requires every band exactly once and nothing else.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BandCounts([u64; 11]);

impl BandCounts {
    fn index(band: Band) -> usize {
        usize::from(10 - band.confidence_rank())
    }

    /// The count for a band.
    pub fn get(&self, band: Band) -> u64 {
        self.0[Self::index(band)]
    }

    /// Adds `count` to a band's.
    pub fn add(&mut self, band: Band, count: u64) {
        self.0[Self::index(band)] += count;
    }

    /// The counts' sum.
    pub fn total(&self) -> u64 {
        self.0.iter().sum()
    }
}

impl Serialize for BandCounts {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(Band::ALL.len()))?;
        for band in Band::ALL {
            map.serialize_entry(band.as_str(), &self.get(*band))?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for BandCounts {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let entries = BTreeMap::<String, u64>::deserialize(deserializer)?;
        let mut counts = Self::default();
        for (name, count) in &entries {
            let band = Band::parse(name)
                .ok_or_else(|| D::Error::custom(format_args!("{name:?} is not a band")))?;
            counts.add(band, *count);
        }
        if entries.len() != Band::ALL.len() {
            return Err(D::Error::custom("by_band needs a count for every band"));
        }
        Ok(counts)
    }
}

/// What indexing covered in one language (the `coverage` table).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    /// The Appendix A language id, or `unknown`.
    pub language: String,
    /// Files in the language.
    pub files: u64,
    /// Of which parsed.
    pub parsed: u64,
    /// Of which parsed with errors.
    pub partial: u64,
    /// Of which failed.
    pub failed: u64,
    /// Of which skipped.
    pub skipped: u64,
    /// Definitions found.
    pub symbols: u64,
    /// Call sites found.
    pub call_sites: u64,
    /// Call sites by the band each ended in.
    pub by_band: BandCounts,
    /// Links observed at run time.
    pub observed_links: u64,
}

vocabulary! {
    /// What happened to a file when it was indexed (the `files` table).
    pub enum FileStatus {
        /// Parsed without errors.
        Parsed => "parsed",
        /// Parsed, with errors in the tree.
        Partial => "partial",
        /// Extraction failed.
        Failed => "failed",
        /// Not extracted: too large, or excluded.
        Skipped => "skipped",
        /// Not text.
        Binary => "binary",
        /// Withheld under the secrets policy.
        Redacted => "redacted",
    }
}

/// A file of the repository (the `files` table).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    /// The id of the file's `File` node.
    pub file_id: NodeId,
    /// Its repository-relative POSIX path.
    pub path: String,
    /// Its Appendix A language id, or `unknown`.
    pub language: String,
    /// What happened to it.
    pub status: FileStatus,
    /// Why, when its status needs a reason.
    pub status_reason: Option<String>,
    /// Its git blob hash, in hex.
    pub blob_sha: String,
    /// Its size.
    pub size_bytes: u64,
    /// Its number of lines.
    pub line_count: u64,
}

vocabulary! {
    /// What kind of fact a provider's verdict is about (the `evidence` table).
    pub enum FactKind {
        /// An edge.
        Edge => "edge",
        /// A node.
        Node => "node",
        /// A non-drawn call site.
        Candidate => "candidate",
    }
}

vocabulary! {
    /// How authoritative a provider is (the `evidence` table).
    pub enum Authority {
        /// A compiler-backed index.
        Compiler => "compiler",
        /// The engine's typed resolution.
        EngineTyped => "engine_typed",
        /// Structural resolution.
        Structural => "structural",
        /// A person.
        Manual => "manual",
    }
}

vocabulary! {
    /// A provider's verdict on a fact (the `evidence` table).
    pub enum Verdict {
        /// It confirms the fact.
        Supports => "supports",
        /// It resolves the same site elsewhere.
        Contradicts => "contradicts",
        /// Providers disagree.
        Conflict => "conflict",
    }
}

/// A provider's verdict on a precise, contradicted or manual fact (the `evidence`
/// table). Rows are stored as given: deriving them is precise merging's (4.6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    /// The verdict's identity, as its producer computed it.
    pub evidence_id: String,
    /// What kind of fact it is about.
    pub fact_kind: FactKind,
    /// The fact's id: an edge, node or site id, by kind.
    pub fact_id: String,
    /// Who gave it.
    pub provider: String,
    /// The provider's version.
    pub provider_version: String,
    /// How authoritative it is.
    pub authority: Authority,
    /// What it says.
    pub verdict: Verdict,
    /// The provider's target, when it differs from the fact's.
    pub target_node_id: Option<NodeId>,
    /// The file the verdict points into.
    pub file_id: Option<NodeId>,
    /// Where in the file it starts.
    pub start_byte: Option<u32>,
    /// Where it ends.
    pub end_byte: Option<u32>,
    /// Everything else.
    pub metadata: Props,
}

vocabulary! {
    /// What a compiler provider says an occurrence is (the `semantic_occurrences` table).
    pub enum OccurrenceRole {
        /// A definition.
        Definition => "definition",
        /// A reference.
        Reference => "reference",
        /// A read.
        Read => "read",
        /// A write.
        Write => "write",
        /// An import.
        Import => "import",
        /// A reference to a type.
        TypeReference => "type_reference",
        /// An implementation.
        Implementation => "implementation",
        /// An override.
        Override => "override",
    }
}

/// An occurrence a compiler provider reported, in a precise segment (the
/// `semantic_occurrences` table). Stored as given: producing them is the precise
/// band's (4.6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticOccurrence {
    /// The occurrence's identity, as its producer computed it.
    pub occ_id: String,
    /// Who reported it.
    pub provider: String,
    /// The provider's symbol.
    pub provider_symbol: String,
    /// The node it maps to, when it maps to one.
    pub target_node_id: Option<NodeId>,
    /// The file it is in.
    pub file_id: NodeId,
    /// Where it starts.
    pub start_byte: u32,
    /// Where it ends.
    pub end_byte: u32,
    /// Its first line.
    pub start_line: u32,
    /// Its first column.
    pub start_col: u32,
    /// What it is.
    pub role: OccurrenceRole,
    /// The structural site it falls on, when exactly one does.
    pub site_id: Option<SiteId>,
    /// The definition it is in.
    pub enclosing_node_id: Option<NodeId>,
}
