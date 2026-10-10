//! A symbol's `importance` (4.12.1):
//!
//! ```text
//! importance = sqrt(fan_in) × visibility_factor × test_penalty
//! ```
//!
//! with `visibility_factor` 1.0 for a public symbol and 0.6 otherwise, and
//! `test_penalty` 0.3 for a test and 1.0 otherwise. Both are read from the graph, never
//! from the engine again: public is the node's `props.visibility = "public"`, which is
//! stored only where the engine's visibility is evidence (issue 68), so a symbol whose
//! visibility is unknown is not public (DECISIONS); a test is a `Test` node, the
//! stricter test semantics of Appendix B.5, whose `props.is_test` must agree. `fan_in`
//! is the stored row's value. Nothing is rounded.

use std::collections::BTreeMap;

use pdx_engine::Definition;
use serde_json::Value;

use crate::ids::NodeId;
use crate::index::derive::{Builder, DeriveError};
use crate::kinds::NodeKind;

use super::{FAN_IN, IMPORTANCE, Rows};

/// `visibility_factor` for a public symbol.
pub const PUBLIC: f64 = 1.0;
/// `visibility_factor` for any other: non-public, or of unknown visibility.
pub const NOT_PUBLIC: f64 = 0.6;
/// `test_penalty` for a test.
pub const TEST: f64 = 0.3;
/// `test_penalty` for anything else.
pub const NOT_TEST: f64 = 1.0;

/// 4.12.1's importance of a symbol called `fan_in` times.
pub fn importance(fan_in: f64, public: bool, test: bool) -> f64 {
    let visibility = if public { PUBLIC } else { NOT_PUBLIC };
    let penalty = if test { TEST } else { NOT_TEST };
    fan_in.sqrt() * visibility * penalty
}

/// Every target's `importance`, from its stored `fan_in`.
///
/// # Errors
///
/// [`DeriveError::Invariant`] for a target whose `props.is_test` disagrees with its
/// kind, or that has no `fan_in`; [`DeriveError::Missing`] for one with no node.
pub(crate) fn emit(
    graph: &Builder,
    targets: &BTreeMap<NodeId, &Definition>,
    rows: &mut Rows,
) -> Result<(), DeriveError> {
    for id in targets.keys() {
        let node = graph.nodes.get(id).ok_or_else(|| DeriveError::Missing {
            path: id.as_str().to_owned(),
            detail: "a metric target with no node",
        })?;
        let test = node.kind == NodeKind::Test;
        if node.props.get("is_test") != Some(&Value::Bool(test)) {
            return Err(DeriveError::Invariant {
                node: id.clone(),
                detail: "props.is_test disagrees with the node's kind",
            });
        }
        let public = node.props.get("visibility").and_then(Value::as_str) == Some("public");
        let fan_in = rows.get(id, FAN_IN).ok_or_else(|| DeriveError::Invariant {
            node: id.clone(),
            detail: "a metric target with no fan_in",
        })?;
        rows.add(id, IMPORTANCE, importance(fan_in, public, test))?;
    }
    Ok(())
}
