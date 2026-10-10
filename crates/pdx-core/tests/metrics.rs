//! Per-symbol metrics (4.12.1, P2-09): `loc`, `cyclomatic`, `cognitive`, `loop_depth`,
//! `transitive_loop_depth`, `fan_in`, `fan_out` and `importance`, and `props.recursive`,
//! through the whole pipeline over checkouts written here. Every expected value is
//! computed by hand from the fixture's source, never by running the code under test.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use pdx_core::bands::{Band, CandidateBand};
use pdx_core::config::PdxConfig;
use pdx_core::ids::{NodeId, NodeKey, RepoId};
use pdx_core::index::derive::{DeriveInput, DerivedGraph, derive};
use pdx_core::index::discover::discover;
use pdx_core::index::extract::{ExtractLimits, ExtractStage};
use pdx_core::kinds::{EdgeKind, LayerRole, NodeKind};
use pdx_core::metrics::complexity::{cyclomatic, loc};
use pdx_core::metrics::fanio::count;
use pdx_core::metrics::importance::importance;
use pdx_core::metrics::{
    COGNITIVE, CYCLOMATIC, FAN_IN, FAN_OUT, IMPORTANCE, LOC, LOOP_DEPTH, METRICS,
    TRANSITIVE_LOOP_DEPTH,
};
use pdx_core::model::{Edge, Node};
use pdx_core::resolve::registry::SymbolRegistry;
use pdx_core::resolve::stages::resolve;
use pdx_engine::Span;
use serde_json::Value;

const REPO: &str = "ce63551447285fd4";

/// A derived graph, and the engine's complexity facts for every definition, by file
/// and name, as Stage 2 extracted them.
struct Run {
    graph: DerivedGraph,
    engine: BTreeMap<(String, String), (u32, u32, u32)>,
}

/// The pipeline over files written to a fresh checkout, discovery's order reversed or
/// not.
fn pipeline(files: &[(&str, &str)], reverse: bool) -> Run {
    let dir = tempfile::Builder::new()
        .prefix("pdx-metrics-test-")
        .tempdir()
        .expect("a directory");
    for (path, content) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
        fs::write(&path, content).expect("a file");
    }
    let root = dir.path();
    let config = PdxConfig::load(root).expect("the configuration loads");
    let mut discovered = discover(root, &config).expect("discovery succeeds");
    if reverse {
        discovered.reverse();
    }
    let limits = ExtractLimits {
        requested_workers: if reverse { 1 } else { 2 },
        memory_budget_bytes: 64 << 20,
    };
    let extracted = ExtractStage::new(root, &config.secrets, limits)
        .run(&discovered)
        .expect("extraction succeeds");
    let registry =
        SymbolRegistry::build(root, &discovered, extracted).expect("the registry builds");
    let mut engine = BTreeMap::new();
    for path in registry.files() {
        for d in registry.extract(path).map_or(&[][..], |e| &e.definitions) {
            engine.insert(
                (path.to_owned(), d.name.clone()),
                (d.cyclomatic, d.cognitive, d.loop_depth),
            );
        }
    }
    let report = resolve(root, &registry).expect("resolution succeeds");
    let graph = derive(&DeriveInput {
        repo: &RepoId::parse(REPO).expect("a repo id"),
        repo_name: "fixture",
        registry: &registry,
        resolution: &report,
        root,
        config: &config,
    })
    .expect("derivation succeeds");
    Run { graph, engine }
}

fn graph(files: &[(&str, &str)]) -> DerivedGraph {
    pipeline(files, false).graph
}

/// The one symbol named `name` in the file at `path`: not its file's module, a route,
/// or the variable the engine records beside a Java field.
fn node<'g>(graph: &'g DerivedGraph, path: &str, name: &str) -> &'g Node {
    let file = &graph.file_nodes[path];
    let found: Vec<_> = graph
        .nodes
        .iter()
        .filter(|n| n.name == name && n.file_id.as_ref() == Some(file))
        .filter(|n| {
            !matches!(
                n.kind,
                NodeKind::Route | NodeKind::Module | NodeKind::Variable
            )
        })
        .collect();
    assert_eq!(found.len(), 1, "one node {name} in {path}");
    found[0]
}

/// A node's metrics, by name.
fn metrics<'g>(graph: &'g DerivedGraph, id: &NodeId) -> BTreeMap<&'g str, f64> {
    graph
        .metrics
        .iter()
        .filter(|m| &m.node_id == id)
        .map(|m| (m.metric.as_str(), m.value))
        .collect()
}

/// One metric of the node `name` in `path`.
fn metric(graph: &DerivedGraph, path: &str, name: &str, metric: &str) -> Option<f64> {
    metrics(graph, &node(graph, path, name).node_id)
        .get(metric)
        .copied()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-12
}

// --- complexity -------------------------------------------------------------------------

/// The same seven callables in six languages, and each one's hand-computed values:
/// `(cyclomatic, cognitive, loop_depth)`.
///
/// - `straight`: no decision. `cyclomatic` = 1 + 0 = 1; `cognitive` 0; no loop.
/// - `one_if`: one `if`. `cyclomatic` = 1 + 1 = 2; `cognitive` = 1 (the `if`, nesting
///   0); no loop.
/// - `two_ifs`: two `if`s one after the other. `cyclomatic` = 1 + 2 = 3; `cognitive` =
///   1 + 1 = 2; no loop.
/// - `for_loop`: one `for`. `cyclomatic` = 1 + 1 = 2; `cognitive` = 1; one loop deep.
/// - `while_loop`: one `while` (Go's condition-only `for`). `cyclomatic` = 2;
///   `cognitive` = 1; one loop deep.
/// - `loop_if`: an `if` inside a `for`. `cyclomatic` = 1 + 2 = 3; `cognitive` = 1 (the
///   `for`) + (1 + 1) (the `if`, nested once) = 3; one loop deep.
/// - `nested`: an `if` inside a `for` inside a `for`. `cyclomatic` = 1 + 3 = 4;
///   `cognitive` = 1 + (1 + 1) + (1 + 2) = 6; two loops deep.
const EXPECTED: [(&str, (f64, f64, f64)); 7] = [
    ("straight", (1.0, 0.0, 0.0)),
    ("one_if", (2.0, 1.0, 0.0)),
    ("two_ifs", (3.0, 2.0, 0.0)),
    ("for_loop", (2.0, 1.0, 1.0)),
    ("while_loop", (2.0, 1.0, 1.0)),
    ("loop_if", (3.0, 3.0, 1.0)),
    ("nested", (4.0, 6.0, 2.0)),
];

const JAVA: &str = r"package com.acme.shop;

public class Calc {
    int straight(int x) {
        int y = x + 1;
        return y;
    }

    int one_if(int x) {
        if (x > 0) { return 1; }
        return 0;
    }

    int two_ifs(int x, int y) {
        if (x > 0) { x++; }
        if (y > 0) { y++; }
        return x + y;
    }

    int for_loop(int n) {
        int s = 0;
        for (int i = 0; i < n; i++) { s += i; }
        return s;
    }

    int while_loop(int n) {
        while (n > 0) { n--; }
        return n;
    }

    int loop_if(int n) {
        int s = 0;
        for (int i = 0; i < n; i++) {
            if (i > 2) { s++; }
        }
        return s;
    }

    int nested(int n) {
        int s = 0;
        for (int i = 0; i < n; i++) {
            for (int j = 0; j < n; j++) {
                if (i == j) { s++; }
            }
        }
        return s;
    }
}
";

const PYTHON: &str = r"def straight(x):
    y = x + 1
    return y


def one_if(x):
    if x > 0:
        return 1
    return 0


def two_ifs(x, y):
    if x > 0:
        x += 1
    if y > 0:
        y += 1
    return x + y


def for_loop(n):
    s = 0
    for i in range(n):
        s += i
    return s


def while_loop(n):
    while n > 0:
        n -= 1
    return n


def loop_if(n):
    s = 0
    for i in range(n):
        if i > 2:
            s += 1
    return s


def nested(n):
    s = 0
    for i in range(n):
        for j in range(n):
            if i == j:
                s += 1
    return s
";

const GO: &str = r"package calc

func straight(x int) int {
	y := x + 1
	return y
}

func one_if(x int) int {
	if x > 0 {
		return 1
	}
	return 0
}

func two_ifs(x int, y int) int {
	if x > 0 {
		x++
	}
	if y > 0 {
		y++
	}
	return x + y
}

func for_loop(n int) int {
	s := 0
	for i := 0; i < n; i++ {
		s += i
	}
	return s
}

func while_loop(n int) int {
	for n > 0 {
		n--
	}
	return n
}

func loop_if(n int) int {
	s := 0
	for i := 0; i < n; i++ {
		if i > 2 {
			s++
		}
	}
	return s
}

func nested(n int) int {
	s := 0
	for i := 0; i < n; i++ {
		for j := 0; j < n; j++ {
			if i == j {
				s++
			}
		}
	}
	return s
}
";

const TYPESCRIPT: &str = r"export function straight(x: number): number {
  const y = x + 1;
  return y;
}

export function one_if(x: number): number {
  if (x > 0) { return 1; }
  return 0;
}

export function two_ifs(x: number, y: number): number {
  if (x > 0) { x++; }
  if (y > 0) { y++; }
  return x + y;
}

export function for_loop(n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) { s += i; }
  return s;
}

export function while_loop(n: number): number {
  while (n > 0) { n--; }
  return n;
}

export function loop_if(n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    if (i > 2) { s++; }
  }
  return s;
}

export function nested(n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    for (let j = 0; j < n; j++) {
      if (i === j) { s++; }
    }
  }
  return s;
}
";

const RUST: &str = r"pub fn straight(x: i32) -> i32 {
    let y = x + 1;
    y
}

pub fn one_if(x: i32) -> i32 {
    if x > 0 {
        return 1;
    }
    0
}

pub fn two_ifs(mut x: i32, mut y: i32) -> i32 {
    if x > 0 {
        x += 1;
    }
    if y > 0 {
        y += 1;
    }
    x + y
}

pub fn for_loop(n: i32) -> i32 {
    let mut s = 0;
    for i in 0..n {
        s += i;
    }
    s
}

pub fn while_loop(mut n: i32) -> i32 {
    while n > 0 {
        n -= 1;
    }
    n
}

pub fn loop_if(n: i32) -> i32 {
    let mut s = 0;
    for i in 0..n {
        if i > 2 {
            s += 1;
        }
    }
    s
}

pub fn nested(n: i32) -> i32 {
    let mut s = 0;
    for i in 0..n {
        for j in 0..n {
            if i == j {
                s += 1;
            }
        }
    }
    s
}
";

const C: &str = r"int straight(int x) {
  int y = x + 1;
  return y;
}

int one_if(int x) {
  if (x > 0) { return 1; }
  return 0;
}

int two_ifs(int x, int y) {
  if (x > 0) { x++; }
  if (y > 0) { y++; }
  return x + y;
}

int for_loop(int n) {
  int s = 0;
  for (int i = 0; i < n; i++) { s += i; }
  return s;
}

int while_loop(int n) {
  while (n > 0) { n--; }
  return n;
}

int loop_if(int n) {
  int s = 0;
  for (int i = 0; i < n; i++) {
    if (i > 2) { s++; }
  }
  return s;
}

int nested(int n) {
  int s = 0;
  for (int i = 0; i < n; i++) {
    for (int j = 0; j < n; j++) {
      if (i == j) { s++; }
    }
  }
  return s;
}
";

const COMPLEXITY: [(&str, &str); 6] = [
    ("src/main/java/com/acme/shop/Calc.java", JAVA),
    ("calc/calc.py", PYTHON),
    ("calc/calc.go", GO),
    ("calc/calc.ts", TYPESCRIPT),
    ("calc/src/lib.rs", RUST),
    ("calc/calc.c", C),
];

#[test]
fn cyclomatic_known_values() {
    let g = graph(&COMPLEXITY);
    for (path, _) in COMPLEXITY {
        for (name, (expected, _, _)) in EXPECTED {
            assert_eq!(
                metric(&g, path, name, CYCLOMATIC),
                Some(expected),
                "{path} {name}"
            );
        }
    }
    // The McCabe number of no decisions is one path.
    assert_eq!(cyclomatic(0).to_bits(), 1.0f64.to_bits());
    assert_eq!(cyclomatic(3).to_bits(), 4.0f64.to_bits());
}

#[test]
fn cognitive_and_loop_depth_known_values() {
    let g = graph(&COMPLEXITY);
    for (path, _) in COMPLEXITY {
        for (name, (_, cognitive, loop_depth)) in EXPECTED {
            assert_eq!(
                metric(&g, path, name, COGNITIVE),
                Some(cognitive),
                "{path} {name}"
            );
            assert_eq!(
                metric(&g, path, name, LOOP_DEPTH),
                Some(loop_depth),
                "{path} {name}"
            );
        }
    }
}

#[test]
fn complexity_engine_facts_cross_stage_boundary() {
    // The engine's facts for each callable are the hand-computed values (decisions,
    // not yet the McCabe number), and Stage 4's rows are those same facts, crossing
    // the stage boundary unchanged but for `cyclomatic`'s one path.
    let run = pipeline(&COMPLEXITY, false);
    for (path, _) in COMPLEXITY {
        for (name, (cyclomatic, cognitive, loop_depth)) in EXPECTED {
            let (decisions, engine_cognitive, engine_loops) =
                run.engine[&(path.to_owned(), name.to_owned())];
            assert_eq!(
                (
                    f64::from(decisions) + 1.0,
                    f64::from(engine_cognitive),
                    f64::from(engine_loops)
                ),
                (cyclomatic, cognitive, loop_depth),
                "the engine's facts for {path} {name}"
            );
            let rows = metrics(&run.graph, &node(&run.graph, path, name).node_id);
            assert_eq!(
                (rows[CYCLOMATIC], rows[COGNITIVE], rows[LOOP_DEPTH]),
                (cyclomatic, cognitive, loop_depth),
                "the rows of {path} {name}"
            );
        }
    }
}

#[test]
fn complexity_metrics_only_on_callables() {
    // A callable has all eight metrics; any other symbol (a class, a field, an
    // interface, an enum) only the four that do not measure a body's control flow.
    let g = graph(&[(
        "src/main/java/com/acme/shop/Shop.java",
        "package com.acme.shop;\n\npublic class Shop {\n    private int stock;\n    public Shop() { stock = 0; }\n    int total(int n) {\n        for (int i = 0; i < n; i++) { stock++; }\n        return stock;\n    }\n    interface Priced {}\n    enum Size { S, M }\n}\n",
    )]);
    let names =
        |id: &NodeId| -> Vec<String> { metrics(&g, id).keys().map(|k| (*k).to_owned()).collect() };
    let path = "src/main/java/com/acme/shop/Shop.java";
    for name in ["Shop", "Priced", "Size"] {
        let n = g
            .nodes
            .iter()
            .find(|n| n.name == name && n.kind != NodeKind::Method)
            .expect("the node");
        assert_eq!(
            names(&n.node_id),
            [FAN_IN, FAN_OUT, IMPORTANCE, LOC],
            "{name}"
        );
    }
    let stock = node(&g, path, "stock");
    assert_eq!(names(&stock.node_id), [FAN_IN, FAN_OUT, IMPORTANCE, LOC]);
    let total = node(&g, path, "total");
    let mut all = METRICS.to_vec();
    all.sort_unstable();
    assert_eq!(names(&total.node_id), all);
    assert_eq!(metric(&g, path, "total", LOOP_DEPTH), Some(1.0));
}

// --- loc --------------------------------------------------------------------------------

#[test]
fn loc_uses_definition_span() {
    let g = graph(&[
        (
            "src/main/java/com/acme/shop/Lines.java",
            // Line 3 to line 9 is the class; line 4 to line 8 the method, its blank
            // line and closing brace included.
            "package com.acme.shop;\n\npublic class Lines {\n    int five(int x) {\n        int y = x;\n\n        return y;\n    }\n}\n",
        ),
        (
            "pkg/lines.py",
            // `one` is line 1; `four` lines 4 to 7, a blank line within.
            "def one(): return 1\n\n\ndef four(x):\n    y = x\n\n    return y\n",
        ),
    ]);
    assert_eq!(
        metric(&g, "src/main/java/com/acme/shop/Lines.java", "Lines", LOC),
        Some(7.0)
    );
    assert_eq!(
        metric(&g, "src/main/java/com/acme/shop/Lines.java", "five", LOC),
        Some(5.0)
    );
    assert_eq!(metric(&g, "pkg/lines.py", "one", LOC), Some(1.0));
    assert_eq!(metric(&g, "pkg/lines.py", "four", LOC), Some(4.0));
    // Lines, never bytes or characters; no span, or one that does not run forward
    // from line 1, is no `loc`, never 0.
    let span = |start_line, end_line| Span {
        start_byte: 0,
        end_byte: 9_999,
        start_line,
        start_col: 0,
        end_line,
        end_col: 0,
    };
    assert_eq!(loc(Some(span(4, 8))), Some(5.0));
    assert_eq!(loc(Some(span(4, 4))), Some(1.0));
    assert_eq!(loc(Some(span(8, 4))), None);
    assert_eq!(loc(Some(span(0, 4))), None);
    assert_eq!(loc(None), None);
    // Every target with a span has its `loc`; none is missing here.
    assert!(g.diagnostics.metrics.is_empty());
}

// --- fan-in and fan-out -----------------------------------------------------------------

#[test]
fn fan_in_out_drawn_calls_only() {
    let g = graph(&[
        (
            "pkg/a.py",
            // `run` calls `helper` twice (two drawn CALLS edges) and passes it once
            // (a CALL_REFERENCE edge, not a call).
            "def helper():\n    return 1\n\n\ndef run(xs):\n    xs.append(helper)\n    return helper() + helper()\n",
        ),
        // `tally_rows` is defined twice and imported nowhere: a call to it is a
        // candidate row, never an edge, and counts for neither.
        ("pkg/b.py", "def tally_rows():\n    return 2\n"),
        ("pkg/c.py", "def tally_rows():\n    return 3\n"),
        ("pkg/d.py", "def caller():\n    return tally_rows()\n"),
        // A test's call is a CALLS edge, mirrored by a TESTS edge that counts nothing.
        (
            "tests/test_a.py",
            "from pkg.a import helper\n\n\ndef test_helper():\n    assert helper() == 1\n",
        ),
    ]);
    assert!(g.edges.iter().any(|e| e.kind == EdgeKind::CallReference));
    assert!(g.edges.iter().any(|e| e.kind == EdgeKind::Tests));
    assert!(
        g.candidates
            .iter()
            .any(|c| c.band == CandidateBand::Candidate)
    );
    // helper: two calls from `run`, one from the test.
    assert_eq!(metric(&g, "pkg/a.py", "helper", FAN_IN), Some(3.0));
    assert_eq!(metric(&g, "pkg/a.py", "helper", FAN_OUT), Some(0.0));
    assert_eq!(metric(&g, "pkg/a.py", "run", FAN_OUT), Some(2.0));
    assert_eq!(metric(&g, "pkg/a.py", "run", FAN_IN), Some(0.0));
    assert_eq!(
        metric(&g, "tests/test_a.py", "test_helper", FAN_OUT),
        Some(1.0)
    );
    // A candidate is not a drawn call: zero, stored.
    assert_eq!(metric(&g, "pkg/b.py", "tally_rows", FAN_IN), Some(0.0));
    assert_eq!(metric(&g, "pkg/c.py", "tally_rows", FAN_IN), Some(0.0));
    assert_eq!(metric(&g, "pkg/d.py", "caller", FAN_OUT), Some(0.0));
    // The totals are the drawn calls' weights, exactly.
    let calls: f64 = g
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Calls)
        .map(|e| f64::from(e.weight))
        .sum();
    let fan_in: f64 = g
        .metrics
        .iter()
        .filter(|m| m.metric == FAN_IN)
        .map(|m| m.value)
        .sum();
    assert!(close(calls, fan_in), "{calls} {fan_in}");
}

fn function_id(name: &str) -> NodeId {
    NodeKey::definition(
        &RepoId::parse(REPO).expect("a repo id"),
        NodeKind::Function,
        "a.py",
        name,
        "",
    )
    .node_id()
    .expect("an id")
}

#[test]
fn fan_counts_edge_weight() {
    // An edge standing for several calls counts each; other kinds count nothing.
    let (a, b, c) = (function_id("a"), function_id("b"), function_id("c"));
    let mut heavy = Edge::new(a.clone(), b.clone(), EdgeKind::Calls, Band::Exact, None);
    heavy.weight = 3;
    let light = Edge::new(a.clone(), c.clone(), EdgeKind::Calls, Band::Exact, None);
    let mut tests = Edge::new(c.clone(), b.clone(), EdgeKind::Tests, Band::Exact, None);
    tests.weight = 5;
    let reference = Edge::new(
        c.clone(),
        b.clone(),
        EdgeKind::CallReference,
        Band::Exact,
        None,
    );
    let fans = count([&heavy, &light, &tests, &reference].into_iter());
    assert!(close(fans.fan_out(&a), 4.0));
    assert!(close(fans.fan_in(&b), 3.0));
    assert!(close(fans.fan_in(&c), 1.0));
    assert!(close(fans.fan_out(&c), 0.0));
    assert!(close(fans.fan_in(&a), 0.0));
}

#[test]
fn self_call_counts_both_fans() {
    let g = graph(&[(
        "pkg/fact.py",
        "def fact(n):\n    return n * fact(n - 1) if n else 1\n\n\ndef plain(n):\n    return n\n",
    )]);
    assert_eq!(metric(&g, "pkg/fact.py", "fact", FAN_IN), Some(1.0));
    assert_eq!(metric(&g, "pkg/fact.py", "fact", FAN_OUT), Some(1.0));
    let fact = node(&g, "pkg/fact.py", "fact");
    assert_eq!(fact.props.get("recursive"), Some(&Value::Bool(true)));
    // No `recursive = false` anywhere else.
    assert_eq!(
        node(&g, "pkg/fact.py", "plain").props.get("recursive"),
        None
    );
}

// --- transitive loop depth --------------------------------------------------------------

const ACYCLIC: &str = r"def leaf_two(n):
    for i in range(n):
        for j in range(n):
            print(i, j)


def caller_zero(n):
    return leaf_two(n)


def leaf_one(n):
    for i in range(n):
        print(i)


def caller_three(n):
    for i in range(n):
        for j in range(n):
            for k in range(n):
                print(i, j, k)
    return leaf_one(n)


def chain_a(n):
    return chain_b(n)


def chain_b(n):
    return chain_c(n)


def chain_c(n):
    for i in range(n):
        print(i)
";

#[test]
fn transitive_loop_depth_acyclic() {
    let g = graph(&[("pkg/acyclic.py", ACYCLIC)]);
    let depth = |name: &str| {
        (
            metric(&g, "pkg/acyclic.py", name, LOOP_DEPTH),
            metric(&g, "pkg/acyclic.py", name, TRANSITIVE_LOOP_DEPTH),
        )
    };
    // A depth-0 caller of a depth-2 callee reaches 2.
    assert_eq!(depth("caller_zero"), (Some(0.0), Some(2.0)));
    assert_eq!(depth("leaf_two"), (Some(2.0), Some(2.0)));
    // A depth-3 caller of a depth-1 callee stays 3: the maximum, never 3 + 1.
    assert_eq!(depth("caller_three"), (Some(3.0), Some(3.0)));
    assert_eq!(depth("leaf_one"), (Some(1.0), Some(1.0)));
    // a -> b -> c: c's depth reaches a, two calls away.
    assert_eq!(depth("chain_a"), (Some(0.0), Some(1.0)));
    assert_eq!(depth("chain_b"), (Some(0.0), Some(1.0)));
    assert_eq!(depth("chain_c"), (Some(1.0), Some(1.0)));
    assert!(g.nodes.iter().all(|n| n.props.get("recursive").is_none()));
}

const RECURSIVE: &str = r"def ping(n):
    for i in range(n):
        print(i)
    return pong(n - 1)


def pong(n):
    sink(n)
    return ping(n - 1)


def sink(n):
    for i in range(n):
        for j in range(n):
            print(i, j)


def fact(n):
    return n * fact(n - 1) if n else 1


def outer(n):
    return ping(n)
";

#[test]
fn transitive_loop_depth_recursive_scc() {
    let run = |reverse| {
        pipeline(
            &[("pkg/rec.py", RECURSIVE), ("pkg/acyclic.py", ACYCLIC)],
            reverse,
        )
        .graph
    };
    let g = run(false);
    let transitive = |name: &str| metric(&g, "pkg/rec.py", name, TRANSITIVE_LOOP_DEPTH);
    // ping <-> pong: depths 1 and 0, and pong calls sink (2): the component reaches 2.
    assert_eq!(transitive("ping"), Some(2.0));
    assert_eq!(transitive("pong"), Some(2.0));
    assert_eq!(transitive("sink"), Some(2.0));
    // fact calls itself and nothing else: its own depth.
    assert_eq!(transitive("fact"), Some(0.0));
    // A caller of the cycle reaches beyond it.
    assert_eq!(transitive("outer"), Some(2.0));
    let recursive: BTreeSet<&str> = g
        .nodes
        .iter()
        .filter(|n| n.props.get("recursive") == Some(&Value::Bool(true)))
        .map(|n| n.name.as_str())
        .collect();
    assert_eq!(recursive, BTreeSet::from(["fact", "ping", "pong"]));
    assert!(
        g.nodes
            .iter()
            .filter_map(|n| n.props.get("recursive"))
            .all(|v| v == &Value::Bool(true))
    );
    // The same rows and flags whatever order the files are found in.
    assert_eq!(run(true), g);
}

// --- importance -------------------------------------------------------------------------

#[test]
fn importance_formula() {
    // sqrt(fan_in) × visibility_factor × test_penalty, unrounded.
    // fan_in 0: sqrt(0) = 0, whatever else holds.
    assert!(close(importance(0.0, true, false), 0.0));
    assert!(close(importance(0.0, false, true), 0.0));
    // Public, not a test: sqrt(4) × 1.0 × 1.0 = 2.0.
    assert!(close(importance(4.0, true, false), 2.0));
    // Not public, not a test: sqrt(4) × 0.6 × 1.0 = 1.2.
    assert!(close(importance(4.0, false, false), 1.2));
    // Public test: sqrt(9) × 1.0 × 0.3 = 0.9.
    assert!(close(importance(9.0, true, true), 0.9));
    // Non-public test: sqrt(9) × 0.6 × 0.3 = 0.54.
    assert!(close(importance(9.0, false, true), 0.54));
    // A non-integral root: sqrt(2) × 1.0 = 1.41421356…
    assert!(close(
        importance(2.0, true, false),
        std::f64::consts::SQRT_2
    ));

    // Through the graph: a Go exported function called four times is public (2.0),
    // an unexported one not (1.2); a Java method, whose visibility the engine does not
    // read (issue 68), is unknown and so not public: one call gives 0.6, never 1.0.
    let g = graph(&[
        ("shop/go.mod", "module example.com/shop\n\ngo 1.22\n"),
        (
            "shop/calc.go",
            "package shop\n\nfunc Exported() int { return 1 }\n\nfunc unexported() int { return 2 }\n\nfunc Caller() int {\n\treturn Exported() + Exported() + Exported() + Exported() + unexported() + unexported() + unexported() + unexported()\n}\n",
        ),
        (
            "src/main/java/com/acme/shop/Prices.java",
            "package com.acme.shop;\n\npublic class Prices {\n    public int base() { return 1; }\n    public int total() { return base(); }\n}\n",
        ),
        (
            "src/test/java/com/acme/shop/PricesTest.java",
            "package com.acme.shop;\n\nimport org.junit.jupiter.api.Test;\n\nclass PricesTest {\n    @Test\n    void shared() {}\n\n    @Test\n    void other() { shared(); }\n}\n",
        ),
        (
            "tests/test_prices.py",
            "def test_shared():\n    assert True\n\n\ndef test_other():\n    test_shared()\n",
        ),
    ]);
    let value = |path: &str, name: &str| metric(&g, path, name, IMPORTANCE).expect("importance");
    assert!(close(value("shop/calc.go", "Exported"), 2.0));
    assert!(close(value("shop/calc.go", "unexported"), 1.2));
    assert!(close(value("shop/calc.go", "Caller"), 0.0));
    let java = "src/main/java/com/acme/shop/Prices.java";
    assert_eq!(node(&g, java, "base").props.get("visibility"), None);
    assert!(close(value(java, "base"), 0.6));
    // A test called once: a Python test is public (1.0 × 0.3); a Java one, of unknown
    // visibility, is not (0.6 × 0.3).
    let python_test = node(&g, "tests/test_prices.py", "test_shared");
    assert_eq!(python_test.kind, NodeKind::Test);
    assert!(close(value("tests/test_prices.py", "test_shared"), 0.3));
    let java_test = node(&g, "src/test/java/com/acme/shop/PricesTest.java", "shared");
    assert_eq!(java_test.kind, NodeKind::Test);
    assert!(close(
        value("src/test/java/com/acme/shop/PricesTest.java", "shared"),
        0.18
    ));
    // Every importance is the formula of its own stored fan_in.
    for n in &g.nodes {
        let rows = metrics(&g, &n.node_id);
        let Some(stored) = rows.get(IMPORTANCE) else {
            continue;
        };
        let public = n.props.get("visibility").and_then(Value::as_str) == Some("public");
        let expected = importance(rows[FAN_IN], public, n.kind == NodeKind::Test);
        assert_eq!(stored.to_bits(), expected.to_bits(), "{}", n.qualified_name);
    }
}

#[test]
fn visibility_props_only_where_engine_evidence() {
    // The engine's visibility is a language's naming convention for Go and Python, and
    // is stored there; elsewhere it does not read a declaration's modifiers (a Java
    // `public` method is not public to it, a Rust private function is), so nothing is
    // stored rather than something wrong (issue 68).
    let g = graph(&[
        ("shop/go.mod", "module example.com/shop\n\ngo 1.22\n"),
        (
            "shop/v.go",
            "package shop\n\nfunc Exported() {}\n\nfunc internal() {}\n",
        ),
        (
            "pkg/v.py",
            "def public():\n    pass\n\n\ndef _private():\n    pass\n",
        ),
        (
            "src/main/java/com/acme/shop/V.java",
            "package com.acme.shop;\n\npublic class V {\n    public void open() {}\n    private void closed() {}\n}\n",
        ),
        ("crate/src/lib.rs", "pub fn open() {}\nfn closed() {}\n"),
        (
            "web/v.ts",
            "export function open() {}\nfunction closed() {}\n",
        ),
    ]);
    let visibility = |path: &str, name: &str| {
        node(&g, path, name)
            .props
            .get("visibility")
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    assert_eq!(
        visibility("shop/v.go", "Exported").as_deref(),
        Some("public")
    );
    assert_eq!(
        visibility("shop/v.go", "internal").as_deref(),
        Some("non_public")
    );
    assert_eq!(visibility("pkg/v.py", "public").as_deref(), Some("public"));
    assert_eq!(
        visibility("pkg/v.py", "_private").as_deref(),
        Some("non_public")
    );
    for (path, names) in [
        ("src/main/java/com/acme/shop/V.java", ["open", "closed"]),
        ("crate/src/lib.rs", ["open", "closed"]),
        ("web/v.ts", ["open", "closed"]),
    ] {
        for name in names {
            assert_eq!(visibility(path, name), None, "{path} {name}");
        }
    }
}

// --- the metric surface -----------------------------------------------------------------

#[test]
fn metrics_exclude_file_level_module() {
    let g = graph(&[
        (
            "pkg/main.py",
            // A top-level call: its caller is the file, which is no symbol.
            "def compute_total():\n    return 1\n\n\ncompute_total()\n",
        ),
        ("pkg/__init__.py", ""),
        // An inline module definition: a TypeScript namespace.
        (
            "web/inline.ts",
            "namespace Inner {\n  export function inside() { return 1; }\n}\n",
        ),
    ]);
    let file = &g.file_nodes["pkg/main.py"];
    assert!(
        g.edges
            .iter()
            .any(|e| e.kind == EdgeKind::Calls && &e.src == file),
        "a CALLS edge from the file"
    );
    let with_metrics: BTreeSet<&NodeId> = g.metrics.iter().map(|m| &m.node_id).collect();
    for n in &g.nodes {
        let target = matches!(
            n.kind,
            NodeKind::Function
                | NodeKind::Method
                | NodeKind::Constructor
                | NodeKind::Class
                | NodeKind::Interface
                | NodeKind::Enum
                | NodeKind::Struct
                | NodeKind::Trait
                | NodeKind::TypeAlias
                | NodeKind::Field
                | NodeKind::Variable
                | NodeKind::Macro
                | NodeKind::Test
        );
        assert_eq!(
            with_metrics.contains(&n.node_id),
            target,
            "{} {}",
            n.kind,
            n.qualified_name
        );
    }
    // No file, folder, repository, semantic module or inline module has a metric.
    let inner = g
        .nodes
        .iter()
        .find(|n| n.kind == NodeKind::Module && n.name == "Inner")
        .expect("the inline module");
    assert!(metrics(&g, &inner.node_id).is_empty());
    assert_eq!(metric(&g, "web/inline.ts", "inside", LOC), Some(1.0));
    assert!(metrics(&g, file).is_empty());
    // The file's call still counts in its callee's fan_in.
    assert_eq!(
        metric(&g, "pkg/main.py", "compute_total", FAN_IN),
        Some(1.0)
    );
}

#[test]
fn metrics_are_deterministic() {
    let files: Vec<(&str, &str)> = COMPLEXITY
        .iter()
        .copied()
        .chain([("pkg/rec.py", RECURSIVE), ("pkg/acyclic.py", ACYCLIC)])
        .collect();
    let forward = pipeline(&files, false).graph;
    let backward = pipeline(&files, true).graph;
    assert_eq!(forward.metrics, backward.metrics);
    assert_eq!(forward, backward);
    // Sorted by node then metric, each pair once, every value finite and a metric
    // 4.12.1 names.
    assert!(!forward.metrics.is_empty());
    for pair in forward.metrics.windows(2) {
        assert!(
            (&pair[0].node_id, &pair[0].metric) < (&pair[1].node_id, &pair[1].metric),
            "{pair:?}"
        );
    }
    for m in &forward.metrics {
        assert!(m.value.is_finite());
        assert!(METRICS.contains(&m.metric.as_str()), "{}", m.metric);
    }
}

#[test]
fn derive_returns_layer_roles_and_metrics() {
    // One Stage 4 call, no segment written: roles on modules and classes, every metric
    // on a callable, sorted rows, recursion flagged.
    let g = graph(&[(
        "src/main/java/com/acme/service/Orders.java",
        "package com.acme.service;\n\nimport org.springframework.stereotype.Repository;\n\n@Repository\npublic class Orders {\n    int walk(int n) {\n        for (int i = 0; i < n; i++) {\n            if (i > 1) { return walk(i - 1); }\n        }\n        return 0;\n    }\n\n    int start() { return walk(3); }\n}\n",
    )]);
    let path = "src/main/java/com/acme/service/Orders.java";
    let class = node(&g, path, "Orders");
    assert_eq!(class.layer_role, Some(LayerRole::Persistence));
    let module = g
        .nodes
        .iter()
        .find(|n| n.kind == NodeKind::Module)
        .expect("the package");
    assert_eq!(module.layer_role, Some(LayerRole::Service));
    let walk = metrics(&g, &node(&g, path, "walk").node_id);
    // walk: a `for` and an `if` in it: cyclomatic 3, cognitive 1 + 2 = 3, one loop;
    // lines 7 to 12; it calls itself once.
    assert_eq!(
        walk,
        BTreeMap::from([
            (COGNITIVE, 3.0),
            (CYCLOMATIC, 3.0),
            (FAN_IN, 2.0),
            (FAN_OUT, 1.0),
            (IMPORTANCE, 2f64.sqrt() * 0.6),
            (LOC, 6.0),
            (LOOP_DEPTH, 1.0),
            (TRANSITIVE_LOOP_DEPTH, 1.0),
        ])
    );
    let start = metrics(&g, &node(&g, path, "start").node_id);
    assert!(close(start[TRANSITIVE_LOOP_DEPTH], 1.0));
    assert!(close(start[FAN_OUT], 1.0));
    assert_eq!(
        node(&g, path, "walk").props.get("recursive"),
        Some(&Value::Bool(true))
    );
    assert_eq!(node(&g, path, "start").props.get("recursive"), None);
    assert!(
        g.metrics
            .windows(2)
            .all(|p| (&p[0].node_id, &p[0].metric) < (&p[1].node_id, &p[1].metric))
    );
}

#[test]
fn roles_and_metrics_are_pure() {
    // Roles and metrics are computed from the facts they are given: nothing reads a
    // file, opens a socket, runs a program or Git, reads the environment, the clock
    // or a random source.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut checked = 0;
    for dir in ["layers", "metrics"] {
        for entry in fs::read_dir(src.join(dir)).expect("the module") {
            let path = entry.expect("an entry").path();
            let text = fs::read_to_string(&path).expect("source");
            for forbidden in [
                "std::fs",
                "fs::",
                "File::",
                "std::net",
                "TcpStream",
                "UdpSocket",
                "std::process",
                "Command",
                "std::env",
                "env::var",
                "SystemTime",
                "Instant",
                "chrono",
                "rand::",
                "thread_rng",
                "git2",
                "HashMap",
                "HashSet",
            ] {
                assert!(
                    !text.contains(forbidden),
                    "{} uses {forbidden}",
                    path.display()
                );
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 6);
}
