//! A symbol's size and complexity (4.12.1): `loc`, `cyclomatic`, `cognitive`,
//! `loop_depth`, `transitive_loop_depth`, and `props.recursive` on a symbol in a cycle
//! of drawn calls.
//!
//! `cyclomatic`, `cognitive` and `loop_depth` are the engine's facts for each definition
//! (Appendix D.4), never recomputed and never re-read from the source, and a callable's
//! only (a function, method or constructor, a test among them): for any other kind the
//! engine counts nothing, so a value would be made up, not measured (DECISIONS). The
//! engine counts a language's table of branching node types, and what each metric is,
//! with its deviations from the textbook definitions, is issue 67:
//!
//! - `cyclomatic` is the `McCabe` number, `1 + decisions`, the decisions being the
//!   engine's count of branching nodes: a straight-line callable is 1, each `if`, loop
//!   or `case` adds one.
//! - `cognitive` is the engine's sum, over branching nodes, of one plus the number of
//!   branching nodes enclosing it.
//! - `loop_depth` is the engine's deepest nesting of loops.
//!
//! `loc` is the definition's whole span, `end_line - start_line + 1`, for every target
//! with a valid span; a target without one has no `loc` and a diagnostic, never a
//! made-up 0.
//!
//! `transitive_loop_depth` is the deepest `loop_depth` reachable from a callable along
//! drawn `CALLS` edges, itself included: the maximum, never a sum, since a call does not
//! prove the callee runs inside the caller's loop. Cycles are condensed (strongly
//! connected components, found in node-id order), so a recursive graph ends without a
//! limit; every target in a component of more than one node, or calling itself, is
//! flagged `props.recursive = true`.

use std::collections::{BTreeMap, BTreeSet};

use pdx_engine::{Definition, DefinitionKind, Span};

use crate::ids::NodeId;
use crate::index::derive::{DeriveError, MetricDiagnostic};
use crate::kinds::EdgeKind;
use crate::model::Edge;

use super::{COGNITIVE, CYCLOMATIC, LOC, LOOP_DEPTH, Rows, TRANSITIVE_LOOP_DEPTH};

/// Whether a definition kind is a callable, the kinds the engine measures complexity
/// for.
pub const fn is_callable(kind: DefinitionKind) -> bool {
    matches!(
        kind,
        DefinitionKind::Function | DefinitionKind::Method | DefinitionKind::Constructor
    )
}

/// The `McCabe` cyclomatic complexity from the engine's count of decisions: one path, and
/// one more per decision.
pub fn cyclomatic(decisions: u32) -> f64 {
    f64::from(decisions) + 1.0
}

/// A definition's lines, its whole span's; `None` for a span that is missing or does
/// not run forward from line 1 or later.
pub fn loc(span: Option<Span>) -> Option<f64> {
    let span = span?;
    (span.start_line >= 1 && span.end_line >= span.start_line)
        .then(|| f64::from(span.end_line - span.start_line) + 1.0)
}

/// Whether two definitions give a node the same metrics.
pub(crate) fn same_facts(a: &Definition, b: &Definition) -> bool {
    a.kind == b.kind
        && a.span == b.span
        && a.cyclomatic == b.cyclomatic
        && a.cognitive == b.cognitive
        && a.loop_depth == b.loop_depth
}

/// Each target's own metrics: `loc`, and a callable's `cyclomatic`, `cognitive` and
/// `loop_depth`.
pub(crate) fn local(
    targets: &BTreeMap<NodeId, &Definition>,
    rows: &mut Rows,
    diagnostics: &mut Vec<MetricDiagnostic>,
) -> Result<(), DeriveError> {
    for (id, definition) in targets {
        match loc(definition.span) {
            Some(lines) => rows.add(id, LOC, lines)?,
            None => diagnostics.push(MetricDiagnostic {
                node_id: id.clone(),
                metric: LOC,
            }),
        }
        if is_callable(definition.kind) {
            rows.add(id, CYCLOMATIC, cyclomatic(definition.cyclomatic))?;
            rows.add(id, COGNITIVE, f64::from(definition.cognitive))?;
            rows.add(id, LOOP_DEPTH, f64::from(definition.loop_depth))?;
        }
    }
    Ok(())
}

/// Each callable target's `transitive_loop_depth`, from the drawn `CALLS` edges among
/// `edges`; returns the targets in a cycle, to be flagged `recursive`.
pub(crate) fn transitive<'a>(
    edges: impl Iterator<Item = &'a Edge>,
    targets: &BTreeMap<NodeId, &Definition>,
    rows: &mut Rows,
) -> Result<BTreeSet<NodeId>, DeriveError> {
    let calls: Vec<(&NodeId, &NodeId)> = edges
        .filter(|e| e.kind == EdgeKind::Calls)
        .map(|e| (&e.src, &e.dst))
        .collect();
    let local: BTreeMap<&NodeId, u32> = targets
        .iter()
        .filter(|(_, d)| is_callable(d.kind))
        .map(|(id, d)| (id, d.loop_depth))
        .collect();
    let (depths, cyclic) = propagate(&local, &calls);
    for (id, depth) in &depths {
        if local.contains_key(id) {
            rows.add(id, TRANSITIVE_LOOP_DEPTH, f64::from(*depth))?;
        }
    }
    Ok(cyclic
        .into_iter()
        .filter(|id| targets.contains_key(*id))
        .cloned()
        .collect())
}

/// The deepest local depth reachable from every node of `local` along `calls`, itself
/// included, and the nodes in a cycle (a component of several nodes, or one calling
/// itself). A node with no local depth (no callable, or no symbol) adds none but is
/// passed through.
fn propagate<'a>(
    local: &BTreeMap<&'a NodeId, u32>,
    calls: &[(&'a NodeId, &'a NodeId)],
) -> (BTreeMap<&'a NodeId, u32>, BTreeSet<&'a NodeId>) {
    let mut successors: BTreeMap<&NodeId, BTreeSet<&NodeId>> = BTreeMap::new();
    for id in local.keys() {
        successors.entry(id).or_default();
    }
    for (src, dst) in calls {
        successors.entry(src).or_default().insert(dst);
        successors.entry(dst).or_default();
    }
    let components = components(&successors);
    let mut component_of: BTreeMap<&NodeId, usize> = BTreeMap::new();
    for (index, members) in components.iter().enumerate() {
        for id in members {
            component_of.insert(id, index);
        }
    }
    // Components come sinks first: every component a component reaches is done before it.
    let mut depth = vec![0u32; components.len()];
    let mut cyclic = BTreeSet::new();
    for (index, members) in components.iter().enumerate() {
        let mut deepest = 0;
        let mut recursive = members.len() > 1;
        for id in members {
            deepest = deepest.max(local.get(id).copied().unwrap_or(0));
            for next in &successors[id] {
                let reached = component_of[next];
                if reached == index {
                    recursive |= next == id;
                } else {
                    deepest = deepest.max(depth[reached]);
                }
            }
        }
        depth[index] = deepest;
        if recursive {
            cyclic.extend(members.iter().copied());
        }
    }
    let depths = component_of
        .into_iter()
        .map(|(id, index)| (id, depth[index]))
        .collect();
    (depths, cyclic)
}

/// Tarjan's bookkeeping: each visited node's index and low link, and the stack.
struct State<'a> {
    index: BTreeMap<&'a NodeId, usize>,
    low: BTreeMap<&'a NodeId, usize>,
    stack: Vec<&'a NodeId>,
    on_stack: BTreeSet<&'a NodeId>,
}

/// Visits a node: numbers it, stacks it, and returns its frame, its successors to
/// visit in id order.
fn visit<'a>(
    id: &'a NodeId,
    successors: &BTreeMap<&'a NodeId, BTreeSet<&'a NodeId>>,
    state: &mut State<'a>,
) -> (&'a NodeId, Vec<&'a NodeId>) {
    let n = state.index.len();
    state.index.insert(id, n);
    state.low.insert(id, n);
    state.stack.push(id);
    state.on_stack.insert(id);
    let mut pending: Vec<&NodeId> = successors[id].iter().copied().collect();
    pending.reverse();
    (id, pending)
}

/// The strongly connected components of a graph, by Tarjan's algorithm without
/// recursion, starting from nodes in id order and following successors in id order:
/// each component's members in id order, components in the order they close, sinks
/// first.
fn components<'a>(successors: &BTreeMap<&'a NodeId, BTreeSet<&'a NodeId>>) -> Vec<Vec<&'a NodeId>> {
    let mut state = State {
        index: BTreeMap::new(),
        low: BTreeMap::new(),
        stack: Vec::new(),
        on_stack: BTreeSet::new(),
    };
    let mut out = Vec::new();
    for &root in successors.keys() {
        if state.index.contains_key(root) {
            continue;
        }
        // Each frame: a node and the successors it has still to visit.
        let mut frames: Vec<(&NodeId, Vec<&NodeId>)> = vec![visit(root, successors, &mut state)];
        while let Some((id, pending)) = frames.last_mut() {
            let id = *id;
            if let Some(next) = pending.pop() {
                if !state.index.contains_key(next) {
                    frames.push(visit(next, successors, &mut state));
                } else if state.on_stack.contains(next) {
                    let low = state.low[id].min(state.index[next]);
                    state.low.insert(id, low);
                }
                continue;
            }
            frames.pop();
            if let Some((parent, _)) = frames.last() {
                let low = state.low[parent].min(state.low[id]);
                state.low.insert(parent, low);
            }
            if state.low[id] == state.index[id] {
                let mut members = Vec::new();
                while let Some(member) = state.stack.pop() {
                    state.on_stack.remove(member);
                    members.push(member);
                    if member == id {
                        break;
                    }
                }
                members.sort();
                out.push(members);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::NodeKey;
    use crate::ids::RepoId;
    use crate::kinds::NodeKind;

    fn id(name: &str) -> NodeId {
        let repo = RepoId::parse("ce63551447285fd4").expect("a repo id");
        NodeKey::definition(&repo, NodeKind::Function, "a.py", name, "")
            .node_id()
            .expect("an id")
    }

    /// The depths and cyclic nodes of a graph given by names, with its edges in the
    /// order given.
    fn run(
        local: &[(&str, u32)],
        calls: &[(&str, &str)],
    ) -> (BTreeMap<String, u32>, BTreeSet<String>) {
        let names: BTreeMap<String, NodeId> = local
            .iter()
            .map(|(n, _)| *n)
            .chain(calls.iter().flat_map(|(a, b)| [*a, *b]))
            .map(|n| (n.to_owned(), id(n)))
            .collect();
        let by_id: BTreeMap<&NodeId, &str> = names.iter().map(|(n, i)| (i, n.as_str())).collect();
        let local: BTreeMap<&NodeId, u32> = local.iter().map(|(n, d)| (&names[*n], *d)).collect();
        let calls: Vec<(&NodeId, &NodeId)> = calls
            .iter()
            .map(|(a, b)| (&names[*a], &names[*b]))
            .collect();
        let (depths, cyclic) = propagate(&local, &calls);
        (
            depths
                .into_iter()
                .map(|(i, d)| (by_id[i].to_owned(), d))
                .collect(),
            cyclic.into_iter().map(|i| by_id[i].to_owned()).collect(),
        )
    }

    fn depth(result: &(BTreeMap<String, u32>, BTreeSet<String>), name: &str) -> u32 {
        result.0[name]
    }

    #[test]
    fn the_deepest_reachable_depth_never_a_sum() {
        // caller 0 -> callee 2: the caller reaches 2. caller 3 -> callee 1: still 3.
        let r = run(
            &[("a", 0), ("b", 2), ("c", 3), ("d", 1)],
            &[("a", "b"), ("c", "d")],
        );
        assert_eq!((depth(&r, "a"), depth(&r, "b")), (2, 2));
        assert_eq!((depth(&r, "c"), depth(&r, "d")), (3, 1));
        // a -> b -> c, the depth found two calls away; 1 + 2 is never 3.
        let r = run(&[("a", 1), ("b", 0), ("c", 2)], &[("a", "b"), ("b", "c")]);
        assert_eq!((depth(&r, "a"), depth(&r, "b"), depth(&r, "c")), (2, 2, 2));
        assert!(r.1.is_empty());
    }

    #[test]
    fn cycles_are_condensed_and_flagged() {
        // a <-> b (depths 1 and 0) calls c (depth 3); d calls itself; e calls a.
        let local = [("a", 1), ("b", 0), ("c", 3), ("d", 2), ("e", 0)];
        let calls = [("a", "b"), ("b", "a"), ("b", "c"), ("d", "d"), ("e", "a")];
        let r = run(&local, &calls);
        assert_eq!((depth(&r, "a"), depth(&r, "b")), (3, 3));
        assert_eq!((depth(&r, "c"), depth(&r, "d"), depth(&r, "e")), (3, 2, 3));
        let cyclic: Vec<&str> = r.1.iter().map(String::as_str).collect();
        assert_eq!(cyclic, ["a", "b", "d"]);
        // Every order of the edges and of the nodes' names gives the same answer.
        let mut reversed = calls;
        reversed.reverse();
        let mut local_reversed = local;
        local_reversed.reverse();
        assert_eq!(run(&local_reversed, &reversed), r);
        for rotation in 0..calls.len() {
            let mut rotated = calls;
            rotated.rotate_left(rotation);
            assert_eq!(run(&local, &rotated), r);
        }
    }

    #[test]
    fn a_long_chain_and_a_large_cycle_end() {
        // Far deeper than any call stack: the walk is iterative and needs no limit.
        let names: Vec<String> = (0..20_000).map(|i| format!("f{i:05}")).collect();
        let local: Vec<(&str, u32)> = names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.as_str(), u32::from(i == names.len() - 1)))
            .collect();
        let mut calls: Vec<(&str, &str)> = names
            .windows(2)
            .map(|w| (w[0].as_str(), w[1].as_str()))
            .collect();
        let r = run(&local, &calls);
        assert_eq!(depth(&r, "f00000"), 1);
        assert!(r.1.is_empty());
        calls.push((names[names.len() - 1].as_str(), names[0].as_str()));
        let r = run(&local, &calls);
        assert_eq!(depth(&r, "f00000"), 1);
        assert_eq!(r.1.len(), names.len());
    }
}
