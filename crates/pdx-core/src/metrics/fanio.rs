//! A symbol's `fan_in` and `fan_out` (4.12.1): the drawn calls into and out of it in
//! its repository, from the graph's `CALLS` edges and nothing else.
//!
//! Each edge counts its `weight`, so an edge standing for several calls counts them
//! all; a call from a symbol to itself counts in both. Candidate rows, `CALL_REFERENCE`,
//! `TESTS`, imports, type uses, routes and contracts are not calls drawn to a symbol
//! and count nothing. Every target has both metrics, zero included; a call whose
//! caller is a file (top-level code) counts in its callee's `fan_in` and gives the
//! file nothing, since a file is no symbol. Other repositories' calls are the
//! estate's.

use std::collections::BTreeMap;

use pdx_engine::Definition;

use crate::ids::NodeId;
use crate::index::derive::DeriveError;
use crate::kinds::EdgeKind;
use crate::model::Edge;

use super::{FAN_IN, FAN_OUT, Rows};

/// The weighted drawn calls into and out of each node.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fans {
    /// Calls into each node.
    pub fan_in: BTreeMap<NodeId, f64>,
    /// Calls out of each node.
    pub fan_out: BTreeMap<NodeId, f64>,
}

impl Fans {
    /// A node's `fan_in`: zero when nothing calls it.
    pub fn fan_in(&self, id: &NodeId) -> f64 {
        self.fan_in.get(id).copied().unwrap_or(0.0)
    }

    /// A node's `fan_out`: zero when it calls nothing.
    pub fn fan_out(&self, id: &NodeId) -> f64 {
        self.fan_out.get(id).copied().unwrap_or(0.0)
    }
}

/// Counts the `CALLS` edges among `edges`, each by its weight. Sums of whole numbers
/// below 2^53 are exact in an `f64`.
pub fn count<'a>(edges: impl Iterator<Item = &'a Edge>) -> Fans {
    let mut fans = Fans::default();
    for edge in edges.filter(|e| e.kind == EdgeKind::Calls) {
        let weight = f64::from(edge.weight);
        *fans.fan_out.entry(edge.src.clone()).or_default() += weight;
        *fans.fan_in.entry(edge.dst.clone()).or_default() += weight;
    }
    fans
}

/// Every target's `fan_in` and `fan_out`.
pub(crate) fn emit(
    targets: &BTreeMap<NodeId, &Definition>,
    fans: &Fans,
    rows: &mut Rows,
) -> Result<(), DeriveError> {
    for id in targets.keys() {
        rows.add(id, FAN_IN, fans.fan_in(id))?;
        rows.add(id, FAN_OUT, fans.fan_out(id))?;
    }
    Ok(())
}
