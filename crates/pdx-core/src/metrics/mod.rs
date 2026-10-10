//! Per-symbol metrics (4.12.1), Stage 4: the segment's `metrics` rows, from the facts
//! the earlier stages hold and nothing read again.
//!
//! A metric target is a symbol: the node of an extracted definition, except the
//! engine's file-level module (its file, which is no symbol) and an inline module
//! definition (architecture structure, L4, not an L5 symbol). Repositories, folders,
//! files, semantic modules, routes and every other node have none.
//!
//! - [`complexity`]: `loc`, `cyclomatic`, `cognitive`, `loop_depth`,
//!   `transitive_loop_depth` and `props.recursive`.
//! - [`fanio`]: `fan_in` and `fan_out`, from drawn `CALLS` edges.
//! - [`importance`]: `importance`, from the stored `fan_in`.
//!
//! [`Rows`] holds them: one value per `(node_id, metric)`, finite, a second refused,
//! in `(node_id, metric)` order.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use pdx_engine::{Definition, DefinitionKind};
use serde_json::Value;

use crate::ids::NodeId;
use crate::index::derive::{Builder, DeriveError, DeriveInput};
use crate::model::Metric;
use crate::resolve::registry::SymbolRegistry;

pub mod complexity;
pub mod fanio;
pub mod importance;

/// `loc`.
pub const LOC: &str = "loc";
/// `cyclomatic`.
pub const CYCLOMATIC: &str = "cyclomatic";
/// `cognitive`.
pub const COGNITIVE: &str = "cognitive";
/// `loop_depth`.
pub const LOOP_DEPTH: &str = "loop_depth";
/// `transitive_loop_depth`.
pub const TRANSITIVE_LOOP_DEPTH: &str = "transitive_loop_depth";
/// `fan_in`.
pub const FAN_IN: &str = "fan_in";
/// `fan_out`.
pub const FAN_OUT: &str = "fan_out";
/// `importance`.
pub const IMPORTANCE: &str = "importance";

/// Every metric 4.12.1 names, in name order.
pub const METRICS: [&str; 8] = [
    COGNITIVE,
    CYCLOMATIC,
    FAN_IN,
    FAN_OUT,
    IMPORTANCE,
    LOC,
    LOOP_DEPTH,
    TRANSITIVE_LOOP_DEPTH,
];

/// The metric rows of a graph as they are made: one value per node and metric.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rows(BTreeMap<(NodeId, &'static str), f64>);

impl Rows {
    /// Adds a row.
    ///
    /// # Errors
    ///
    /// [`DeriveError::DuplicateMetric`] when the node already has the metric, whatever
    /// its value; [`DeriveError::NonFiniteMetric`] for a value that is not finite.
    pub fn add(
        &mut self,
        node: &NodeId,
        metric: &'static str,
        value: f64,
    ) -> Result<(), DeriveError> {
        if !value.is_finite() {
            return Err(DeriveError::NonFiniteMetric(node.clone(), metric));
        }
        match self.0.entry((node.clone(), metric)) {
            Entry::Occupied(_) => Err(DeriveError::DuplicateMetric(node.clone(), metric)),
            Entry::Vacant(slot) => {
                slot.insert(value);
                Ok(())
            }
        }
    }

    /// A node's value of a metric.
    pub fn get(&self, node: &NodeId, metric: &'static str) -> Option<f64> {
        self.0.get(&(node.clone(), metric)).copied()
    }

    /// The rows, by node then metric.
    pub fn into_rows(self) -> Vec<Metric> {
        self.0
            .into_iter()
            .map(|((node_id, metric), value)| Metric {
                node_id,
                metric: metric.to_owned(),
                value,
            })
            .collect()
    }
}

/// The metric targets, by node, each with its definition's facts.
///
/// # Errors
///
/// [`DeriveError::Missing`] for a definition the registry does not have;
/// [`DeriveError::Invariant`] for two definitions with different facts on one node.
pub(crate) fn targets<'a>(
    graph: &Builder,
    registry: &'a SymbolRegistry,
) -> Result<BTreeMap<NodeId, &'a Definition>, DeriveError> {
    let mut targets: BTreeMap<NodeId, &'a Definition> = BTreeMap::new();
    for (r, id) in &graph.definition_nodes {
        if graph.file_definitions.contains(r) {
            continue;
        }
        let definition = registry.definition(r).ok_or_else(|| DeriveError::Missing {
            path: r.path.clone(),
            detail: "a definition the registry does not have",
        })?;
        if definition.kind == DefinitionKind::Module {
            continue;
        }
        match targets.entry(id.clone()) {
            Entry::Vacant(slot) => {
                slot.insert(definition);
            }
            Entry::Occupied(slot) if complexity::same_facts(slot.get(), definition) => {}
            Entry::Occupied(_) => {
                return Err(DeriveError::Invariant {
                    node: id.clone(),
                    detail: "two definitions with different facts share a node",
                });
            }
        }
    }
    Ok(targets)
}

/// Derives every metric target's metrics and flags recursive symbols, after the
/// graph's `CALLS` edges and tests are final.
///
/// # Errors
///
/// [`DeriveError`] when a target's facts are missing or disagree, or a row would be a
/// duplicate or not finite.
pub(crate) fn derive(graph: &mut Builder, input: &DeriveInput<'_>) -> Result<(), DeriveError> {
    let targets = targets(graph, input.registry)?;
    let mut rows = Rows::default();
    complexity::local(&targets, &mut rows, &mut graph.diagnostics.metrics)?;
    let fans = fanio::count(graph.edges.values());
    fanio::emit(&targets, &fans, &mut rows)?;
    let recursive = complexity::transitive(graph.edges.values(), &targets, &mut rows)?;
    importance::emit(graph, &targets, &mut rows)?;
    for id in recursive {
        graph.set_prop(&id, "recursive", Value::Bool(true));
    }
    graph.metrics = rows;
    Ok(())
}
