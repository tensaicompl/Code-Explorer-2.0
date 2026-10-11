//! A golden repository's committed expectation (plan 5.7): the node count of every node
//! kind and the drawn edges as `(src_qn, dst_qn, kind, band)`, in the v0 format of
//! decision 44.
//!
//! The format is the structural graph only. It holds no identity (node, edge or site
//! id), path, line, source, signature, prop, timestamp or machine state, and nothing a
//! lock file or a version constant already records: the same graph serialises to the
//! same bytes on every machine, forever.

use std::collections::{BTreeMap, HashMap};

use pdx_core::kinds::NodeKind;
use pdx_core::model::{Edge, Node};
use serde::{Deserialize, Serialize};

/// One repository's expected structural graph.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoldenExpected {
    /// Every node kind once, zero counts included, by kind spelling.
    pub node_counts: Vec<NodeKindCount>,
    /// One entry per stored drawn edge, duplicates kept, by
    /// `(src_qn, dst_qn, kind, band)`.
    pub drawn_edges: Vec<GoldenEdge>,
}

/// How many nodes of one kind the segment holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeKindCount {
    /// The kind, as `NodeKind::as_str` spells it.
    pub kind: String,
    /// How many.
    pub count: u64,
}

/// One drawn edge, by its endpoints' qualified names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoldenEdge {
    /// The source node's qualified name.
    pub src_qn: String,
    /// The destination node's qualified name.
    pub dst_qn: String,
    /// The edge kind, as `EdgeKind::as_str` spells it.
    pub kind: String,
    /// The band, as `Band::as_str` spells it.
    pub band: String,
}

impl GoldenEdge {
    /// The tuple a golden is sorted by (5.7), in its order.
    pub fn sort_key(&self) -> (&str, &str, &str, &str) {
        (&self.src_qn, &self.dst_qn, &self.kind, &self.band)
    }
}

/// Why a segment's rows make no golden.
#[derive(Debug, thiserror::Error)]
pub enum ExpectedError {
    /// Two nodes share an id.
    #[error("node {0} appears twice")]
    DuplicateNode(String),
    /// An edge's endpoint is no node of the segment.
    #[error("edge {edge} refers to node {node}, which the segment does not hold")]
    MissingEndpoint {
        /// The edge.
        edge: String,
        /// The missing node.
        node: String,
    },
    /// A committed golden is not JSON in the golden's shape.
    #[error("not a golden: {0}")]
    Parse(#[from] serde_json::Error),
}

impl GoldenExpected {
    /// The golden of a segment's rows. Pure: nothing but the rows and the closed
    /// vocabularies decide it.
    ///
    /// Node kinds come from `NodeKind::ALL`, each once whatever the segment holds, so
    /// a new kind is a reviewed golden change. An edge is kept exactly when its band
    /// is drawn (`Band::is_drawn`), once per stored row: two rows with one tuple stay
    /// two entries.
    ///
    /// # Errors
    ///
    /// [`ExpectedError::DuplicateNode`] or [`ExpectedError::MissingEndpoint`], which a
    /// verified segment never has.
    pub fn from_rows(nodes: &[Node], edges: &[Edge]) -> Result<Self, ExpectedError> {
        let mut names: BTreeMap<&str, &str> = BTreeMap::new();
        let mut counts: HashMap<NodeKind, u64> = HashMap::new();
        for node in nodes {
            if names
                .insert(node.node_id.as_str(), &node.qualified_name)
                .is_some()
            {
                return Err(ExpectedError::DuplicateNode(node.node_id.to_string()));
            }
            *counts.entry(node.kind).or_insert(0) += 1;
        }
        let mut node_counts: Vec<NodeKindCount> = NodeKind::ALL
            .iter()
            .map(|kind| NodeKindCount {
                kind: kind.as_str().to_owned(),
                count: counts.get(kind).copied().unwrap_or(0),
            })
            .collect();
        node_counts.sort_by(|a, b| a.kind.cmp(&b.kind));

        let name_of = |edge: &Edge, id: &pdx_core::ids::NodeId| -> Result<String, ExpectedError> {
            names
                .get(id.as_str())
                .map(|qn| (*qn).to_owned())
                .ok_or_else(|| ExpectedError::MissingEndpoint {
                    edge: edge.edge_id.to_string(),
                    node: id.to_string(),
                })
        };
        let mut drawn_edges = Vec::new();
        for edge in edges.iter().filter(|e| e.band.is_drawn()) {
            drawn_edges.push(GoldenEdge {
                src_qn: name_of(edge, &edge.src)?,
                dst_qn: name_of(edge, &edge.dst)?,
                kind: edge.kind.as_str().to_owned(),
                band: edge.band.as_str().to_owned(),
            });
        }
        drawn_edges.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
        Ok(Self {
            node_counts,
            drawn_edges,
        })
    }

    /// The golden's committed bytes: UTF-8 JSON, pretty-printed with two-space
    /// indentation, fields in declaration order, `\n` line endings and a final `\n`.
    ///
    /// # Panics
    ///
    /// Never: the golden is strings and numbers.
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = serde_json::to_vec_pretty(self).expect("a golden serialises");
        bytes.push(b'\n');
        bytes
    }

    /// A golden read back from its bytes, refusing any field the format lacks.
    ///
    /// # Errors
    ///
    /// [`ExpectedError::Parse`] for bytes that are not a golden.
    pub fn parse(bytes: &[u8]) -> Result<Self, ExpectedError> {
        Ok(serde_json::from_slice(bytes)?)
    }

    /// How many nodes in all.
    pub fn node_total(&self) -> u64 {
        self.node_counts.iter().map(|c| c.count).sum()
    }
}
