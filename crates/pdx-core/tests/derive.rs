//! Stage 4, derive (P2-07): containment, modules, call materialisation, routes, tests
//! and entry points, through the whole pipeline: a checkout, discovered, extracted by
//! the engine, registered, resolved (typed resolution included) and derived.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use pdx_core::config::PdxConfig;
use pdx_core::ids::{NodeId, RepoId};
use pdx_core::index::derive::{DeriveInput, DerivedGraph, derive};
use pdx_core::index::discover::discover;
use pdx_core::index::extract::{ExtractLimits, ExtractStage};
use pdx_core::kinds::{EdgeKind, NodeKind};
use pdx_core::model::Node;
use pdx_core::resolve::registry::SymbolRegistry;
use pdx_core::resolve::stages::{ResolveReport, resolve};

/// The repository id every fixture is derived under (4.2.1's reference repository).
fn repo() -> RepoId {
    RepoId::parse("ce63551447285fd4").expect("a repo id")
}

struct Checkout {
    dir: tempfile::TempDir,
}

impl Checkout {
    fn new(files: &[(&str, &str)]) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("pdx-derive-test-")
            .tempdir()
            .expect("a directory");
        for (path, content) in files {
            let path = dir.path().join(path);
            fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
            fs::write(&path, content).expect("a file");
        }
        Self { dir }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }
}

/// Everything the pipeline produced for a checkout.
struct Run {
    registry: SymbolRegistry,
    report: ResolveReport,
    graph: DerivedGraph,
}

fn run(checkout: &Checkout) -> Run {
    run_with(checkout, false, 2)
}

/// The pipeline, with discovery's output reversed or not before the later stages see
/// it, and extraction on `workers` workers.
fn run_with(checkout: &Checkout, reverse_discovery: bool, workers: usize) -> Run {
    let root = checkout.root();
    let config = PdxConfig::load(root).expect("the configuration loads");
    let mut files = discover(root, &config).expect("discovery succeeds");
    if reverse_discovery {
        files.reverse();
    }
    let limits = ExtractLimits {
        requested_workers: workers,
        memory_budget_bytes: 64 << 20,
    };
    let extracted = ExtractStage::new(root, &config.secrets, limits)
        .run(&files)
        .expect("extraction succeeds");
    let registry = SymbolRegistry::build(root, &files, extracted).expect("the registry builds");
    let report = resolve(root, &registry).expect("resolution succeeds");
    let repo = repo();
    let graph = derive(&DeriveInput {
        repo: &repo,
        repo_name: "fixture",
        registry: &registry,
        resolution: &report,
    })
    .expect("derivation succeeds");
    Run {
        registry,
        report,
        graph,
    }
}

/// The nodes of a kind.
fn nodes_of(graph: &DerivedGraph, kind: NodeKind) -> Vec<&Node> {
    graph.nodes.iter().filter(|n| n.kind == kind).collect()
}

/// The one node of a kind with this name.
fn node<'g>(graph: &'g DerivedGraph, kind: NodeKind, name: &str) -> &'g Node {
    let found: Vec<&Node> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == kind && n.name == name)
        .collect();
    assert_eq!(found.len(), 1, "{kind} {name}: {found:#?}");
    found[0]
}

fn prop_bool(node: &Node, key: &str) -> bool {
    node.props.get(key).and_then(serde_json::Value::as_bool) == Some(true)
}

/// A projection of a derived graph a snapshot can hold: counts by kind and band, then
/// every node below the files with its identity, sorted. No temporary path, no
/// pointer, no hash order.
fn projection(graph: &DerivedGraph) -> String {
    let mut out = String::new();
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    for n in &graph.nodes {
        *kinds.entry(n.kind.as_str()).or_default() += 1;
    }
    let entry_points = graph
        .nodes
        .iter()
        .filter(|n| prop_bool(n, "is_entry_point"))
        .count();
    writeln!(out, "nodes: {kinds:?}").unwrap();
    writeln!(out, "entry points: {entry_points}").unwrap();
    let mut edges: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for e in &graph.edges {
        *edges.entry((e.kind.as_str(), e.band.as_str())).or_default() += 1;
    }
    writeln!(out, "edges: {edges:?}").unwrap();
    let mut candidates: BTreeMap<&str, usize> = BTreeMap::new();
    for c in &graph.candidates {
        *candidates.entry(c.band.as_str()).or_default() += 1;
    }
    writeln!(out, "candidates: {candidates:?}").unwrap();
    writeln!(
        out,
        "files: {}",
        graph
            .files
            .iter()
            .map(|f| format!("{}={}", f.path, f.status.as_str()))
            .collect::<Vec<_>>()
            .join(" ")
    )
    .unwrap();
    let names: BTreeMap<&NodeId, &Node> = graph.nodes.iter().map(|n| (&n.node_id, n)).collect();
    let mut lines: Vec<String> = graph
        .nodes
        .iter()
        .filter(|n| !matches!(n.kind, NodeKind::Repo | NodeKind::Folder | NodeKind::File))
        .map(|n| {
            let parent = n
                .parent_id
                .as_ref()
                .and_then(|p| names.get(p))
                .map_or_else(String::new, |p| format!("{}:{}", p.kind, p.name));
            let module = n
                .props
                .get("module")
                .and_then(|m| m.as_str())
                .and_then(|m| names.get(&NodeId::parse(m).ok()?))
                .map_or_else(String::new, |m| format!(" module={}", m.qualified_name));
            let flags = format!(
                "{}{}",
                if prop_bool(n, "is_test") { " test" } else { "" },
                if prop_bool(n, "is_entry_point") {
                    " entry"
                } else {
                    ""
                },
            );
            format!(
                "{} {} {} parent={parent}{module}{flags} id={}",
                n.kind, n.qualified_name, n.name, n.node_id
            )
        })
        .collect();
    lines.sort();
    for line in lines {
        writeln!(out, "  {line}").unwrap();
    }
    out
}

// --- golden fixtures: every typed language, and Ada ---------------------------------

/// Every typed language of the matrix has a `derive_<lang>` fixture below, and so does
/// Ada: the list is the matrix's, not one kept by hand.
#[test]
fn every_typed_language_has_a_derive_fixture() {
    let source = include_str!("derive.rs");
    let mut required: Vec<&str> = pdx_core::languages::all()
        .iter()
        .filter(|l| l.tier == pdx_core::languages::Tier::Typed)
        .map(|l| l.id)
        .collect();
    required.push("ada");
    for id in required {
        assert!(
            source.contains(&format!("fn derive_{id}() {{")),
            "no derive_{id} fixture"
        );
    }
}

/// Runs a fixture and snapshots its projection.
fn golden(name: &str, files: &[(&str, &str)]) -> Run {
    let checkout = Checkout::new(files);
    let run = run(&checkout);
    insta::assert_snapshot!(name, projection(&run.graph));
    run
}

#[test]
fn derive_java() {
    let run = golden(
        "derive_java",
        &[
            (
                "src/main/java/com/acme/Shop.java",
                "package com.acme;\n\npublic class Shop {\n  int total() { return helper() + Util.twice(2); }\n  int helper() { return 1; }\n}\n",
            ),
            (
                "src/main/java/com/acme/Util.java",
                "package com.acme;\n\npublic class Util {\n  static int twice(int x) { return 2 * x; }\n  static int twice(int x, int y) { return x * y; }\n}\n",
            ),
            (
                "src/main/java/com/acme/App.java",
                "package com.acme;\n\npublic class App {\n  public static void main(String[] args) { new Shop().total(); }\n}\n",
            ),
            (
                "src/test/java/com/acme/ShopTest.java",
                "package com.acme;\n\nimport org.junit.jupiter.api.Test;\n\nclass ShopTest {\n  @Test void totals() { new Shop().total(); check(); }\n  void check() {}\n}\n",
            ),
        ],
    );
    let g = &run.graph;
    // One package across two source trees: one module, under their common folder.
    let module = node(g, NodeKind::Module, "com.acme");
    assert_eq!(module.qualified_name, "java:com.acme");
    assert_eq!(module.file_id, None);
    // Overloads are two nodes, told apart by their parameter types.
    let twice: Vec<&Node> = g.nodes.iter().filter(|n| n.name == "twice").collect();
    assert_eq!(twice.len(), 2);
    assert_ne!(twice[0].node_id, twice[1].node_id);
    // main(String[]) is an entry point; the test is a Test node and an entry point.
    assert!(prop_bool(
        node(g, NodeKind::Method, "main"),
        "is_entry_point"
    ));
    assert!(prop_bool(
        node(g, NodeKind::Test, "totals"),
        "is_entry_point"
    ));
    assert_eq!(node(g, NodeKind::Method, "check").kind, NodeKind::Method);
}

#[test]
fn derive_kotlin() {
    let run = golden(
        "derive_kotlin",
        &[
            (
                "src/main/kotlin/com/acme/Shop.kt",
                "package com.acme\n\nclass Shop {\n    fun total(): Int = helper()\n    fun helper(): Int = 1\n}\n",
            ),
            (
                "src/test/kotlin/com/acme/ShopTest.kt",
                "package com.acme\n\nimport org.junit.jupiter.api.Test\n\nclass ShopTest {\n    @Test fun totals() { Shop().total() }\n    fun check() {}\n}\n",
            ),
        ],
    );
    let g = &run.graph;
    assert_eq!(
        node(g, NodeKind::Module, "com.acme").qualified_name,
        "kotlin:com.acme"
    );
    assert_eq!(nodes_of(g, NodeKind::Test).len(), 1);
    assert_eq!(node(g, NodeKind::Method, "check").kind, NodeKind::Method);
}

#[test]
fn derive_typescript() {
    let run = golden(
        "derive_typescript",
        &[
            (
                "tsconfig.json",
                "{ \"compilerOptions\": { \"baseUrl\": \".\", \"paths\": { \"@/*\": [\"src/*\"] } } }\n",
            ),
            (
                "src/lib/format.ts",
                "export function formatName(s: string): string {\n  return s.trim();\n}\n",
            ),
            (
                "src/app/show.ts",
                "import { formatName } from '@/lib/format';\n\nexport function show(): string {\n  return formatName(' x ');\n}\n",
            ),
            (
                "src/app/show.test.ts",
                "import { show } from './show';\n\nit('shows', () => {\n  show();\n});\n",
            ),
        ],
    );
    let g = &run.graph;
    // Directory modules: `src/app` spans two files, `src/lib` is one.
    let app = node(g, NodeKind::Module, "src/app");
    assert_eq!(app.qualified_name, "typescript:src/app");
    assert_eq!(app.file_id, None);
    let lib = node(g, NodeKind::Module, "src/lib");
    assert!(lib.file_id.is_some());
    // An anonymous `it(...)` test has no node of its own.
    assert!(nodes_of(g, NodeKind::Test).is_empty());
}

#[test]
fn derive_javascript() {
    let run = golden(
        "derive_javascript",
        &[
            (
                "src/lib.js",
                "function twice(x) {\n  return 2 * x;\n}\n\nmodule.exports = { twice };\n",
            ),
            (
                "src/app.js",
                "const { twice } = require('./lib');\n\nfunction run4() {\n  return twice(2);\n}\n\nrun4();\n",
            ),
        ],
    );
    let g = &run.graph;
    assert_eq!(
        node(g, NodeKind::Module, "src").qualified_name,
        "javascript:src"
    );
    assert!(!g.edges.is_empty() || !g.candidates.is_empty());
}

#[test]
fn derive_python() {
    let run = golden(
        "derive_python",
        &[
            ("pkg/__init__.py", ""),
            (
                "pkg/core.py",
                "from pkg.util import double\n\n\nclass Shop:\n    def total(self):\n        return double(self.base())\n\n    def base(self):\n        return 1\n",
            ),
            ("pkg/util.py", "def double(x):\n    return 2 * x\n"),
            (
                "tests/unit/test_core.py",
                "from pkg.core import Shop\n\n\ndef helper():\n    return Shop()\n\n\ndef test_total():\n    assert helper().total() == 2\n",
            ),
        ],
    );
    let g = &run.graph;
    assert_eq!(
        node(g, NodeKind::Module, "pkg.core").qualified_name,
        "python:pkg.core"
    );
    assert_eq!(node(g, NodeKind::Test, "test_total").kind, NodeKind::Test);
    assert_eq!(
        node(g, NodeKind::Function, "helper").kind,
        NodeKind::Function
    );
}

#[test]
fn derive_go() {
    let run = golden(
        "derive_go",
        &[
            ("go.mod", "module example.com/acme\n"),
            (
                "pkg/a/a.go",
                "package a\n\nfunc Hello() int { return helper() }\n\nfunc helper() int { return 1 }\n",
            ),
            (
                "pkg/a/a_test.go",
                "package a\n\nimport \"testing\"\n\nfunc TestHello(t *testing.T) {\n\tif Hello() != 1 {\n\t\tt.Fail()\n\t}\n}\n\nfunc setup() {}\n",
            ),
            (
                "cmd/app/main.go",
                "package main\n\nimport \"example.com/acme/pkg/a\"\n\nfunc main() { a.Hello() }\n",
            ),
        ],
    );
    let g = &run.graph;
    let module = node(g, NodeKind::Module, "example.com/acme/pkg/a");
    assert_eq!(module.qualified_name, "go:example.com/acme/pkg/a");
    assert_eq!(nodes_of(g, NodeKind::Test).len(), 1);
    assert_eq!(
        node(g, NodeKind::Function, "setup").kind,
        NodeKind::Function
    );
    assert!(prop_bool(
        node(g, NodeKind::Function, "main"),
        "is_entry_point"
    ));
}

#[test]
fn derive_c() {
    let run = golden(
        "derive_c",
        &[
            ("lib/shop.h", "int total(int x);\n"),
            (
                "lib/shop.c",
                "#include \"shop.h\"\n\nstatic int helper(int x) { return x; }\n\nint total(int x) { return helper(x); }\n",
            ),
            (
                "app/main.c",
                "#include \"../lib/shop.h\"\n\nint main(void) { return total(1); }\n",
            ),
        ],
    );
    let g = &run.graph;
    assert_eq!(node(g, NodeKind::Module, "lib").qualified_name, "c:lib");
    assert!(prop_bool(
        node(g, NodeKind::Function, "main"),
        "is_entry_point"
    ));
}

#[test]
fn derive_cpp() {
    let run = golden(
        "derive_cpp",
        &[(
            "src/shapes.cpp",
            "namespace geo {\nint area(int x) { return x * x; }\n}\nnamespace draw {\nint paint(int x) { return geo::area(x); }\n}\nint main() { return draw::paint(2); }\n",
        )],
    );
    let g = &run.graph;
    // One file, two namespaces: two modules, each in that file.
    let geo = node(g, NodeKind::Module, "geo");
    let draw = node(g, NodeKind::Module, "draw");
    assert_eq!(geo.file_id, draw.file_id);
    let area = node(g, NodeKind::Function, "area");
    assert_eq!(
        area.props.get("module").and_then(|m| m.as_str()),
        Some(geo.node_id.as_str())
    );
}

#[test]
fn derive_csharp() {
    let run = golden(
        "derive_csharp",
        &[
            (
                "src/Shop.cs",
                "namespace Acme.Shop;\n\npublic class Cart\n{\n    public int Total() => Helper();\n    int Helper() => 1;\n}\n",
            ),
            (
                "tests/CartTests.cs",
                "using Acme.Shop;\nusing Xunit;\n\nnamespace Acme.Shop.Tests;\n\npublic class CartTests\n{\n    [Fact]\n    public void Totals() { new Cart().Total(); }\n    void Check() {}\n}\n",
            ),
            (
                "Program.cs",
                "var cart = new Acme.Shop.Cart();\nSystem.Console.WriteLine(cart.Total());\n",
            ),
        ],
    );
    let g = &run.graph;
    assert_eq!(
        node(g, NodeKind::Module, "Acme.Shop").qualified_name,
        "csharp:Acme.Shop"
    );
    assert_eq!(nodes_of(g, NodeKind::Test).len(), 1);
    let program = g.node(&g.file_nodes["Program.cs"]).expect("Program.cs");
    assert!(prop_bool(program, "is_entry_point"));
}

#[test]
fn derive_rust() {
    let run = golden(
        "derive_rust",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"shop\"\nversion = \"0.1.0\"\n",
            ),
            (
                "src/lib.rs",
                "pub mod cart;\n\npub fn total() -> i32 {\n    cart::Cart::new().sum()\n}\n",
            ),
            (
                "src/cart.rs",
                "pub trait Summed {\n    fn sum(&self) -> i32;\n}\n\npub struct Cart;\n\nimpl Cart {\n    pub fn new() -> Self {\n        Cart\n    }\n}\n\nimpl Summed for Cart {\n    fn sum(&self) -> i32 {\n        1\n    }\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn sums() {\n        assert_eq!(super::Cart::new().sum(), 1);\n    }\n}\n",
            ),
        ],
    );
    let g = &run.graph;
    assert_eq!(
        node(g, NodeKind::Module, "crate::cart").qualified_name,
        "rust:crate::cart"
    );
    assert_eq!(nodes_of(g, NodeKind::Test).len(), 1);
}

#[test]
fn derive_php() {
    let run = golden(
        "derive_php",
        &[
            (
                "src/Cart.php",
                "<?php\nnamespace App;\n\nclass Cart {\n    public function total() { return $this->helper(); }\n    public function helper() { return 1; }\n}\n",
            ),
            (
                "tests/CartTest.php",
                "<?php\nnamespace App\\Tests;\n\nuse App\\Cart;\n\nclass CartTest extends TestCase {\n    public function testTotal() { (new Cart())->total(); }\n    public function makeCart() { return new Cart(); }\n}\n",
            ),
        ],
    );
    let g = &run.graph;
    assert_eq!(node(g, NodeKind::Module, "App").qualified_name, "php:App");
    assert_eq!(nodes_of(g, NodeKind::Test).len(), 1);
    assert_eq!(node(g, NodeKind::Method, "makeCart").kind, NodeKind::Method);
}

#[test]
fn derive_perl() {
    let run = golden(
        "derive_perl",
        &[
            (
                "lib/Acme/Shop.pm",
                "package Acme::Shop;\nsub total { return helper(); }\nsub helper { return 1; }\n1;\n",
            ),
            (
                "t/shop.t",
                "use Acme::Shop;\nsub check { 1 }\nAcme::Shop::total();\n",
            ),
        ],
    );
    let g = &run.graph;
    // Perl's package declarations do not cross the interface: no module (issue 41),
    // and no per-definition test convention: no Test node.
    assert!(nodes_of(g, NodeKind::Module).is_empty());
    assert!(nodes_of(g, NodeKind::Test).is_empty());
}

#[test]
fn derive_ada() {
    let run = golden(
        "derive_ada",
        &[
            (
                "src/shop.ads",
                "package Shop is\n   function Total return Integer;\nend Shop;\n",
            ),
            (
                "src/shop.adb",
                "package body Shop is\n   function Helper return Integer is\n   begin\n      return 1;\n   end Helper;\n   function Total return Integer is\n   begin\n      return Helper;\n   end Total;\nend Shop;\n",
            ),
        ],
    );
    let g = &run.graph;
    assert!(!nodes_of(g, NodeKind::Function).is_empty());
}

// --- named acceptance: routes -------------------------------------------------------

/// The route nodes of a graph, by name.
fn routes(graph: &DerivedGraph) -> BTreeMap<String, &Node> {
    nodes_of(graph, NodeKind::Route)
        .into_iter()
        .map(|n| (n.name.clone(), n))
        .collect()
}

/// The `DEFINES_ROUTE` edge into a route.
fn defines(graph: &DerivedGraph, route: &Node) -> pdx_core::model::Edge {
    let found: Vec<_> = graph
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::DefinesRoute && e.dst == route.node_id)
        .collect();
    assert_eq!(found.len(), 1, "{}: {found:#?}", route.name);
    found[0].clone()
}

const CONTROLLER_PATH: &str = "src/main/java/com/acme/web/UserController.java";

/// A Spring controller, with `before` inserted ahead of its first mapping.
fn controller(before: &str) -> Checkout {
    let source = format!(
        "package com.acme.web;\n\nimport org.springframework.web.bind.annotation.*;\n\n@RestController\n@RequestMapping(\"/api\")\npublic class UserController {{\n{before}  @GetMapping(\"/users\")\n  public String list() {{ return \"\"; }}\n\n  @PostMapping(\"/users\")\n  public String create(String name) {{ return name; }}\n\n  @DeleteMapping(\"/users/{{id}}\")\n  public void remove(long id) {{}}\n\n  public String helper() {{ return \"\"; }}\n}}\n"
    );
    Checkout::new(&[(CONTROLLER_PATH, source.as_str())])
}

/// The site an edge names, which must exist.
fn site_of_edge<'g>(
    graph: &'g DerivedGraph,
    edge: &pdx_core::model::Edge,
) -> &'g pdx_core::model::Site {
    let id = edge.site_id.as_ref().expect("the edge names its site");
    graph
        .sites
        .iter()
        .find(|s| &s.site_id == id)
        .expect("the site the edge names")
}

#[test]
fn routes_spring() {
    let run = run(&controller(""));
    let g = &run.graph;
    let found = routes(g);
    let names: Vec<&str> = found.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        [
            "DELETE /api/users/{id}",
            "GET /api/users",
            "POST /api/users"
        ]
    );
    let list = node(g, NodeKind::Method, "list");
    let route = found["GET /api/users"];
    // Identity: the handler's file, and {"handler","method","path"} compactly.
    assert_eq!(
        route.qualified_name,
        r#"{"handler":"src.main.java.com.acme.web.UserController.list","method":"GET","path":"/api/users"}"#
    );
    assert_eq!(route.node_id.as_str(), "CGDNAXVJSZCGQ3AWN0X6NPR9GE");
    assert_eq!(route.file_id.as_ref(), Some(&g.file_nodes[CONTROLLER_PATH]));
    assert_eq!(
        route.parent_id.as_ref(),
        Some(&g.file_nodes[CONTROLLER_PATH])
    );
    // From the handler to the route, at the annotation that declares it (4.2.4).
    let edge = defines(g, route);
    assert_eq!((&edge.src, &edge.dst), (&list.node_id, &route.node_id));
    let site = site_of_edge(g, &edge);
    assert_eq!(site.site_kind, pdx_core::kinds::SiteKind::Route);
    assert_eq!(site.enclosing_node_id.as_ref(), Some(&list.node_id));
    assert_eq!(site.callee_text.as_deref(), Some("GetMapping"));
    assert!(site.span.start_line > 0 && site.span.end_byte > site.span.start_byte);
    assert_eq!(site.span.start_line, 8, "the annotation's own line");
    assert!(
        g.diagnostics.routes.is_empty(),
        "{:#?}",
        g.diagnostics.routes
    );
    // Lines above it move nothing that is identity.
    let moved = run_with(&controller("\n\n  // moved\n\n"), false, 2);
    let moved_route = routes(&moved.graph)["GET /api/users"].clone();
    let moved_edge = defines(&moved.graph, &moved_route);
    assert_eq!(moved_route.node_id, route.node_id);
    assert_eq!(moved_edge.site_id, edge.site_id);
    assert_eq!(moved_edge.edge_id, edge.edge_id);
    assert_ne!(site_of_edge(&moved.graph, &moved_edge).span.start_line, 8);
    assert!(prop_bool(list, "is_entry_point"));
    assert!(!prop_bool(
        node(g, NodeKind::Method, "helper"),
        "is_entry_point"
    ));
}

#[test]
fn routes_fastapi() {
    let checkout = Checkout::new(&[(
        "app/main.py",
        "from fastapi import FastAPI\n\napp = FastAPI()\n\n\n@app.get(\"/users/{user_id}\")\ndef read_user(user_id: int) -> dict:\n    return {\"id\": user_id}\n\n\n@app.post(\"/users\")\ndef create_user(name: str) -> dict:\n    return {\"name\": name}\n\n\ndef helper() -> int:\n    return 1\n",
    )]);
    let run = run(&checkout);
    let g = &run.graph;
    let found = routes(g);
    let names: Vec<&str> = found.keys().map(String::as_str).collect();
    assert_eq!(names, ["GET /users/{user_id}", "POST /users"]);
    let read = node(g, NodeKind::Function, "read_user");
    let route = found["GET /users/{user_id}"];
    let edge = defines(g, route);
    assert_eq!(edge.src, read.node_id);
    // The decorator call is the route's evidence: a `route` site of its own.
    let site_id = edge.site_id.expect("a site");
    let site = g
        .sites
        .iter()
        .find(|s| s.site_id == site_id)
        .expect("the site");
    assert_eq!(site.site_kind, pdx_core::kinds::SiteKind::Route);
    assert_eq!(site.callee_text.as_deref(), Some("get"));
    assert_eq!(site.receiver_text.as_deref(), Some("app"));
    assert!(prop_bool(read, "is_entry_point"));
    // The application object is the bootstrap entry point; a helper is not one.
    assert!(prop_bool(
        node(g, NodeKind::Variable, "app"),
        "is_entry_point"
    ));
    assert!(!prop_bool(
        node(g, NodeKind::Function, "helper"),
        "is_entry_point"
    ));
}

#[test]
fn routes_express() {
    let checkout = Checkout::new(&[(
        "src/server.js",
        "const express = require('express');\n\nconst app = express();\n\nfunction listUsers(req, res) {\n  res.json([]);\n}\n\nfunction createUser(req, res) {\n  res.json({});\n}\n\napp.get('/users', listUsers);\napp.post('/users', createUser);\napp.get('/inline', (req, res) => res.json([]));\n\nfunction start() {\n  app.listen(3000);\n}\n",
    )]);
    let run = run(&checkout);
    let g = &run.graph;
    let found = routes(g);
    let names: Vec<&str> = found.keys().map(String::as_str).collect();
    assert_eq!(names, ["GET /users", "POST /users"]);
    let list = node(g, NodeKind::Function, "listUsers");
    let edge = defines(g, found["GET /users"]);
    assert_eq!(edge.src, list.node_id);
    let site_id = edge.site_id.expect("the registration is the site");
    let site = g
        .sites
        .iter()
        .find(|s| s.site_id == site_id)
        .expect("the site");
    assert_eq!(site.site_kind, pdx_core::kinds::SiteKind::Route);
    assert!(prop_bool(list, "is_entry_point"));
    // An inline handler is identified by nothing: no route, and the stage says so.
    assert!(
        g.diagnostics.routes.iter().any(|d| d.route == "/inline"
            && d.problem == pdx_core::index::derive::RouteProblem::NoHandler)
    );
    // `listen` makes the function that calls it an entry point.
    assert!(prop_bool(
        node(g, NodeKind::Function, "start"),
        "is_entry_point"
    ));
}

// --- named acceptance: tests ----------------------------------------------------------

/// The edges of a kind from a node.
fn edges_from(graph: &DerivedGraph, src: &NodeId, kind: EdgeKind) -> Vec<pdx_core::model::Edge> {
    let mut found: Vec<_> = graph
        .edges
        .iter()
        .filter(|e| &e.src == src && e.kind == kind)
        .cloned()
        .collect();
    found.sort_by(|a, b| a.site_id.cmp(&b.site_id));
    found
}

#[test]
fn tests_junit() {
    let checkout = Checkout::new(&[
        (
            "src/main/java/com/acme/Shop.java",
            "package com.acme;\n\npublic class Shop {\n  public int total() { return 1; }\n  public int tax() { return 2; }\n}\n",
        ),
        (
            "src/test/java/com/acme/ShopTest.java",
            "package com.acme;\n\nimport org.junit.jupiter.api.Test;\n\nclass ShopTest {\n  @Test\n  void totals() {\n    Shop shop = new Shop();\n    shop.total();\n    shop.tax();\n    check();\n  }\n\n  void check() {}\n}\n",
        ),
    ]);
    let run = run(&checkout);
    let g = &run.graph;
    let test = node(g, NodeKind::Test, "totals");
    assert!(prop_bool(test, "is_test") && prop_bool(test, "is_entry_point"));
    assert_eq!(
        test.props.get("declared_kind").and_then(|k| k.as_str()),
        Some("Method")
    );
    // Every drawn call the test makes has its CALLS edge and a TESTS edge beside it:
    // same target, same band, same site.
    let calls = edges_from(g, &test.node_id, EdgeKind::Calls);
    let tests = edges_from(g, &test.node_id, EdgeKind::Tests);
    assert!(calls.len() >= 3, "{calls:#?}");
    assert_eq!(calls.len(), tests.len());
    for (call, test_edge) in calls.iter().zip(&tests) {
        assert_eq!(
            (&call.dst, call.band, &call.site_id),
            (&test_edge.dst, test_edge.band, &test_edge.site_id)
        );
        assert!(call.band.is_drawn());
    }
    // The helper in the test class is a method, not a test.
    let check = node(g, NodeKind::Method, "check");
    assert!(!prop_bool(check, "is_test"));
    assert!(tests.iter().any(|t| t.dst == check.node_id));
}

#[test]
fn tests_pytest() {
    let checkout = Checkout::new(&[
        ("packages/api/shop/__init__.py", ""),
        (
            "packages/api/shop/core.py",
            "def total():\n    return 1\n\n\ndef tax():\n    return 2\n",
        ),
        (
            "packages/api/tests/unit/test_core.py",
            "from shop.core import total, tax\n\n\ndef helper():\n    return total()\n\n\ndef test_total():\n    assert helper() == 1\n    assert tax() == 2\n    assert pick() > 0\n\n\nclass TestTax:\n    def test_tax(self):\n        assert tax() == 2\n\n    def make(self):\n        return 1\n",
        ),
        // Two `pick`s, neither imported: the test's call to it is a candidate.
        ("packages/api/shop/a.py", "def pick():\n    return 1\n"),
        ("packages/api/shop/b.py", "def pick():\n    return 2\n"),
    ]);
    let run = run(&checkout);
    let g = &run.graph;
    // `tests/**` at any depth makes the file a test file; within it, only pytest's own
    // tests are Test nodes.
    let tests: Vec<&str> = nodes_of(g, NodeKind::Test)
        .iter()
        .map(|n| n.name.as_str())
        .collect();
    assert_eq!(tests.len(), 2, "{tests:?}");
    assert!(tests.contains(&"test_total") && tests.contains(&"test_tax"));
    assert_eq!(
        node(g, NodeKind::Function, "helper").kind,
        NodeKind::Function
    );
    assert_eq!(node(g, NodeKind::Method, "make").kind, NodeKind::Method);
    let test_total = node(g, NodeKind::Test, "test_total");
    let calls = edges_from(g, &test_total.node_id, EdgeKind::Calls);
    let tests = edges_from(g, &test_total.node_id, EdgeKind::Tests);
    assert!(!calls.is_empty());
    assert_eq!(calls.len(), tests.len());
    let helper = node(g, NodeKind::Function, "helper");
    assert!(tests.iter().any(|t| t.dst == helper.node_id
        && t.band == calls.iter().find(|c| c.dst == helper.node_id).unwrap().band));
    // Non-drawn calls never become TESTS edges.
    let drawn_sites: std::collections::BTreeSet<_> =
        calls.iter().map(|c| c.site_id.clone()).collect();
    assert!(tests.iter().all(|t| drawn_sites.contains(&t.site_id)));
    let pick = g
        .candidates
        .iter()
        .find(|c| c.src == test_total.node_id && c.callee_name == "pick")
        .expect("a candidate row for the ambiguous call");
    assert_eq!(pick.candidate_ids.len(), 2);
    assert!(tests.iter().all(|t| !pick.candidate_ids.contains(&t.dst)));
}

// --- issue 31: test files ---------------------------------------------------------

#[test]
fn test_rules_follow_issue_31() {
    use pdx_core::index::derive::tests::is_test_path;
    let lang = |id| pdx_core::languages::by_id(id).expect("a language");
    let (ts, js, py, java, c) = (
        lang("typescript"),
        lang("javascript"),
        lang("python"),
        lang("java"),
        lang("c"),
    );
    for path in [
        "src/a.test.ts",
        "src/a.test.tsx",
        "src/a.test.mts",
        "src/a.test.cts",
        "src/a.spec.ts",
        "src/a.spec.tsx",
        "src/a.spec.mts",
        "src/a.spec.cts",
        "src/__tests__/a.ts",
        "pkg/web/__tests__/deep/a.ts",
    ] {
        assert!(is_test_path(ts, path), "{path}");
    }
    // JavaScript takes TypeScript's convention on its own extensions, never `.test.ts`.
    for path in [
        "src/a.test.js",
        "src/a.test.jsx",
        "src/a.test.mjs",
        "src/a.test.cjs",
        "src/a.spec.js",
        "src/__tests__/a.js",
    ] {
        assert!(is_test_path(js, path), "{path}");
    }
    assert!(!is_test_path(js, "src/a.test.ts.js"));
    assert!(!is_test_path(js, "src/a.ts"));
    assert!(!is_test_path(ts, "src/test.ts"));
    assert!(!is_test_path(ts, "src/a.testing.ts"));
    // Directories at any depth, on component boundaries; several names consecutive.
    assert!(is_test_path(py, "tests/a.py"));
    assert!(is_test_path(py, "packages/api/tests/a.py"));
    assert!(!is_test_path(py, "packages/api/mytests/a.py"));
    assert!(!is_test_path(py, "tests.py"));
    assert!(is_test_path(java, "src/test/java/A.java"));
    assert!(is_test_path(java, "service/src/test/java/A.java"));
    assert!(
        !is_test_path(java, "src/main/test/A.java"),
        "not consecutive"
    );
    assert!(!is_test_path(java, "test/src/A.java"));
    // `test*`: any directory component beginning `test`, case-sensitively.
    assert!(is_test_path(c, "test/a.c"));
    assert!(is_test_path(c, "lib/testing/a.c"));
    assert!(!is_test_path(c, "lib/Test/a.c"));
    assert!(!is_test_path(c, "lib/a.c"));
}

// --- identities ---------------------------------------------------------------------

#[test]
fn module_id_fixed_vectors() {
    use pdx_core::index::derive::modules::module_node_key;
    use pdx_core::resolve::registry::ModuleKey;
    // Computed outside the crate (Python hashlib, base32 Crockford by hand), against
    // 4.2.1's reference repository id.
    for (language, scope, name, id) in [
        ("java", "", "com.acme.shop", "55G35296BGG2AQWVHDDXJJXD1M"),
        ("python", "", "pkg.sub", "X7VNB2146K5J6FVR2TWT9783HB"),
        ("python", "lib", "pkg.sub", "J5Z60XNE5PYTSWP0EBK78V58YJ"),
        (
            "go",
            "",
            "example.com/acme/pkg",
            "0N6YZHJWWNYR1WV3S30G8J7Q5D",
        ),
        (
            "rust",
            "crates/a",
            "crate::service",
            "W7DPJAGX5MAX8TT51WAQ3WVAAE",
        ),
        (
            "rust",
            "crates/b",
            "crate::service",
            "CT62A3GAKD1W2AG6E4ED6X8XXX",
        ),
        ("typescript", "", "src/api", "PM423SES9K6HFMTAWW56BPBYN7"),
        ("javascript", "", "src/api", "9DT6FP0NHCXCTXSPQ4MH0H6F7C"),
    ] {
        let key = ModuleKey {
            language,
            scope: scope.to_owned(),
            name: name.to_owned(),
        };
        let node_key = module_node_key(&repo(), &key);
        assert_eq!(node_key.kind, NodeKind::Module);
        assert_eq!(node_key.path, scope);
        assert_eq!(node_key.qualified_name, format!("{language}:{name}"));
        assert_eq!(
            node_key.node_id().unwrap().as_str(),
            id,
            "{language} {scope} {name}"
        );
    }
}

#[test]
fn overload_disambiguators_from_parameter_types() {
    use pdx_core::ids::overload_disambiguators;
    use pdx_core::index::derive::containment::normalise_type;
    // 4.2.1: types as declared, whitespace removed, generic arguments erased; the
    // hashes computed outside the crate.
    assert_eq!(normalise_type("java", "List<Map<String, Integer>>"), "List");
    assert_eq!(normalise_type("java", " int [] "), "int[]");
    assert_eq!(normalise_type("python", "list[int]"), "list");
    assert_eq!(
        normalise_type("python", "Optional[dict[str, int]]"),
        "Optional"
    );
    assert_eq!(normalise_type("go", "[]int"), "[]int");
    assert_eq!(normalise_type("javascript", "?"), "?");
    let signatures = [
        ["int"].map(|t| normalise_type("java", t)).join(","),
        ["int", "int"].map(|t| normalise_type("java", t)).join(","),
        ["List<String>", "int"]
            .map(|t| normalise_type("java", t))
            .join(","),
    ];
    let refs: Vec<&str> = signatures.iter().map(String::as_str).collect();
    assert_eq!(
        overload_disambiguators(&refs).unwrap(),
        ["6da88c34", "9ddf8874", "90a7807f"]
    );
    // Identical signatures in a dynamic language are numbered in file order.
    assert_eq!(
        overload_disambiguators(&["?,?", "?,?"]).unwrap(),
        ["f8e407f9-1", "f8e407f9-2"]
    );
}

// --- modules (Part U) ---------------------------------------------------------------

/// The node of the module with this key.
fn module<'g>(graph: &'g DerivedGraph, language: &str, scope: &str, name: &str) -> &'g Node {
    let found: Vec<&NodeId> = graph
        .module_nodes
        .iter()
        .filter(|(k, _)| k.language == language && k.scope == scope && k.name == name)
        .map(|(_, id)| id)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "{language} {scope} {name}: {:#?}",
        graph.module_nodes
    );
    graph.node(found[0]).expect("the module's node")
}

fn files_prop(node: &Node) -> Vec<String> {
    node.props
        .get("files")
        .and_then(|f| f.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

const CART: &str = "src/main/java/com/acme/shop/Cart.java";
const ORDER: &str = "src/main/java/com/acme/shop/Order.java";
const INVOICE: &str = "src/main/java/com/acme/shop/Invoice.java";

/// Every module shape Part U names: a Java package in two files (three with
/// `invoice`), one Python dotted name under two source roots, one `crate::` name in two
/// crates, and TypeScript and JavaScript in one directory.
fn modules_checkout(invoice: bool) -> Checkout {
    let mut files = vec![
        (
            ORDER,
            "package com.acme.shop;\n\npublic class Order {\n  public int id() { return 2; }\n}\n",
        ),
        (
            CART,
            "package com.acme.shop;\n\npublic class Cart {\n  public int total() { return 1; }\n}\n",
        ),
        ("lib/pkg/__init__.py", ""),
        ("lib/pkg/sub.py", "def one():\n    return 1\n"),
        ("src/pkg/__init__.py", ""),
        ("src/pkg/sub.py", "def one():\n    return 1\n"),
        (
            "crates/a/Cargo.toml",
            "[package]\nname = \"a\"\nversion = \"0.1.0\"\n",
        ),
        ("crates/a/src/lib.rs", "pub mod service;\n"),
        ("crates/a/src/service.rs", "pub fn run() {}\n"),
        (
            "crates/b/Cargo.toml",
            "[package]\nname = \"b\"\nversion = \"0.1.0\"\n",
        ),
        ("crates/b/src/lib.rs", "pub mod service;\n"),
        ("crates/b/src/service.rs", "pub fn run() {}\n"),
        (
            "web/api/client.ts",
            "export function get(): number {\n  return 1;\n}\n",
        ),
        (
            "web/api/legacy.js",
            "function get() {\n  return 1;\n}\n\nmodule.exports = { get };\n",
        ),
    ];
    if invoice {
        files.push((
            INVOICE,
            "package com.acme.shop;\n\npublic class Invoice {\n  public int due() { return 3; }\n}\n",
        ));
    }
    Checkout::new(&files)
}

#[test]
fn single_file_module_has_file_parent() {
    let run = run(&modules_checkout(false));
    let g = &run.graph;
    let sub = module(g, "python", "lib", "pkg.sub");
    let file = &g.file_nodes["lib/pkg/sub.py"];
    assert_eq!(sub.file_id.as_ref(), Some(file));
    assert_eq!(sub.parent_id.as_ref(), Some(file));
    assert!(sub.props.get("files").is_none());
}

#[test]
fn multi_file_module_is_one_node() {
    let run = run(&modules_checkout(false));
    let g = &run.graph;
    let shop: Vec<&Node> = nodes_of(g, NodeKind::Module)
        .into_iter()
        .filter(|n| n.qualified_name == "java:com.acme.shop")
        .collect();
    assert_eq!(shop.len(), 1, "{shop:#?}");
    let shop = shop[0];
    assert_eq!(shop.file_id, None);
    // The nearest folder holding every member, not a member file.
    let folder = g
        .node(shop.parent_id.as_ref().expect("a parent"))
        .expect("the parent");
    assert_eq!(
        (folder.kind, folder.name.as_str()),
        (NodeKind::Folder, "shop")
    );
    assert_eq!(
        folder.node_id,
        pdx_core::ids::NodeKey::folder(&repo(), "src/main/java/com/acme/shop")
            .node_id()
            .unwrap()
    );
}

#[test]
fn module_members_sorted() {
    // Discovery order, file order and the order of the packages' declarations do not
    // matter: the members are sorted by path.
    let run = run(&modules_checkout(false));
    let shop = module(&run.graph, "java", "", "com.acme.shop");
    assert_eq!(files_prop(shop), [CART, ORDER]);
    let run = run_with(&modules_checkout(true), true, 1);
    let shop = module(&run.graph, "java", "", "com.acme.shop");
    assert_eq!(files_prop(shop), [CART, INVOICE, ORDER]);
}

#[test]
fn module_id_does_not_depend_on_file_count() {
    let two = run(&modules_checkout(false));
    let three = run(&modules_checkout(true));
    let id = |r: &Run| {
        module(&r.graph, "java", "", "com.acme.shop")
            .node_id
            .clone()
    };
    assert_eq!(id(&two), id(&three));
    assert_eq!(id(&two).as_str(), "55G35296BGG2AQWVHDDXJJXD1M");
    // A one-file module and the same module spanning files: one id.
    let one = run(&Checkout::new(&[(
        CART,
        "package com.acme.shop;\n\npublic class Cart {}\n",
    )]));
    assert_eq!(id(&one), id(&two));
}

#[test]
fn module_id_distinguishes_language() {
    let run = run(&modules_checkout(false));
    let g = &run.graph;
    let ts = module(g, "typescript", "", "web/api");
    let js = module(g, "javascript", "", "web/api");
    assert_ne!(ts.node_id, js.node_id);
    assert_eq!(ts.qualified_name, "typescript:web/api");
    assert_eq!(js.qualified_name, "javascript:web/api");
    assert_eq!(
        ts.parent_id.as_ref(),
        Some(&g.file_nodes["web/api/client.ts"])
    );
    assert_eq!(
        js.parent_id.as_ref(),
        Some(&g.file_nodes["web/api/legacy.js"])
    );
    // No two modules of the repository share an id.
    let ids: std::collections::BTreeSet<&NodeId> = g.module_nodes.values().collect();
    assert_eq!(ids.len(), g.module_nodes.len());
}

#[test]
fn module_id_distinguishes_scope() {
    let run = run(&modules_checkout(false));
    let g = &run.graph;
    assert_eq!(
        module(g, "python", "lib", "pkg.sub").node_id.as_str(),
        "J5Z60XNE5PYTSWP0EBK78V58YJ"
    );
    assert_ne!(
        module(g, "python", "src", "pkg.sub").node_id,
        module(g, "python", "lib", "pkg.sub").node_id
    );
    assert_eq!(
        module(g, "rust", "crates/a", "crate::service")
            .node_id
            .as_str(),
        "W7DPJAGX5MAX8TT51WAQ3WVAAE"
    );
    assert_eq!(
        module(g, "rust", "crates/b", "crate::service")
            .node_id
            .as_str(),
        "CT62A3GAKD1W2AG6E4ED6X8XXX"
    );
}

#[test]
fn symbols_keep_physical_parent() {
    let run = run(&modules_checkout(false));
    let g = &run.graph;
    let cart = node(g, NodeKind::Class, "Cart");
    assert_eq!(cart.parent_id.as_ref(), Some(&g.file_nodes[CART]));
    let total = node(g, NodeKind::Method, "total");
    assert_eq!(total.parent_id.as_ref(), Some(&cart.node_id));
    // Nothing is parented to a spanning module.
    let shop = &module(g, "java", "", "com.acme.shop").node_id;
    assert!(g.nodes.iter().all(|n| n.parent_id.as_ref() != Some(shop)));
}

#[test]
fn symbols_reference_semantic_module() {
    let run = run(&modules_checkout(false));
    let g = &run.graph;
    let shop = module(g, "java", "", "com.acme.shop").node_id.as_str();
    for (kind, name) in [
        (NodeKind::Class, "Cart"),
        (NodeKind::Method, "total"),
        (NodeKind::Class, "Order"),
        (NodeKind::Method, "id"),
    ] {
        assert_eq!(
            node(g, kind, name)
                .props
                .get("module")
                .and_then(|m| m.as_str()),
            Some(shop),
            "{name}"
        );
    }
    let lib_one: Vec<&Node> = nodes_of(g, NodeKind::Function)
        .into_iter()
        .filter(|n| n.name == "one")
        .collect();
    let modules: std::collections::BTreeSet<&str> = lib_one
        .iter()
        .filter_map(|n| n.props.get("module").and_then(|m| m.as_str()))
        .collect();
    assert_eq!(
        modules.len(),
        2,
        "each source root's `one` in its own module"
    );
}

// --- sites (issue 46, Part P) -----------------------------------------------------

const SHOP: &str = "src/main/java/com/acme/Shop.java";

/// A Java class whose `run` has `body`, with `before` ahead of its first method.
fn shop(before: &str, body: &str) -> Checkout {
    let source = format!(
        "package com.acme;\n\npublic class Shop {{\n{before}  int helper() {{ return 1; }}\n\n  int other() {{ return 2; }}\n\n  int run() {{\n{body}    return 0;\n  }}\n}}\n"
    );
    Checkout::new(&[(SHOP, source.as_str())])
}

const TWO_HELPERS: &str = "    helper();\n    helper();\n";

/// The sites whose callee is `name`, in source order.
fn sites_named<'g>(graph: &'g DerivedGraph, name: &str) -> Vec<&'g pdx_core::model::Site> {
    let mut found: Vec<_> = graph
        .sites
        .iter()
        .filter(|s| s.callee_text.as_deref() == Some(name))
        .collect();
    found.sort_by_key(|s| s.span.start_byte);
    found
}

fn site_ids(graph: &DerivedGraph, name: &str) -> Vec<pdx_core::ids::SiteId> {
    sites_named(graph, name)
        .iter()
        .map(|s| s.site_id.clone())
        .collect()
}

fn node_ids(graph: &DerivedGraph) -> std::collections::BTreeSet<NodeId> {
    graph.nodes.iter().map(|n| n.node_id.clone()).collect()
}

#[test]
fn identical_sites_get_deterministic_ordinals() {
    let run = run(&shop("", TWO_HELPERS));
    let g = &run.graph;
    let sites = sites_named(g, "helper");
    assert_eq!(sites.len(), 2);
    assert_ne!(sites[0].site_id, sites[1].site_id);
    // The fingerprint is 4.2.1's over the engine's node-type path from the method, the
    // texts, and the ordinal in source order: recomputed here from the facts.
    let call = run
        .registry
        .extract(SHOP)
        .unwrap()
        .calls
        .iter()
        .find(|c| c.callee_text == "helper")
        .unwrap();
    assert_eq!(
        call.ast_path.first().map(String::as_str),
        Some("method_declaration")
    );
    assert_eq!(
        call.ast_path.last().map(String::as_str),
        Some("method_invocation")
    );
    let path: Vec<&str> = call.ast_path.iter().map(String::as_str).collect();
    for (ordinal, site) in (1u32..).zip(&sites) {
        assert_eq!(
            site.ast_fingerprint,
            pdx_core::ids::ast_fingerprint(&path, "helper", "", ordinal).unwrap()
        );
        assert_eq!(
            site.enclosing_node_id.as_ref(),
            Some(&node(g, NodeKind::Method, "run").node_id)
        );
    }
    // And the same on every run.
    assert_eq!(
        site_ids(&run_with(&shop("", TWO_HELPERS), true, 1).graph, "helper"),
        site_ids(g, "helper")
    );
}

#[test]
fn line_insertion_keeps_node_and_site_ids() {
    let base = run(&shop("", TWO_HELPERS));
    let moved = run(&shop("\n\n\n", "\n    helper();\n\n\n    helper();\n"));
    let (b, m) = (&base.graph, &moved.graph);
    assert_ne!(
        sites_named(b, "helper")[0].span.start_line,
        sites_named(m, "helper")[0].span.start_line,
        "the lines did move"
    );
    assert_eq!(site_ids(b, "helper"), site_ids(m, "helper"));
    assert_eq!(node_ids(b), node_ids(m));
}

#[test]
fn moving_a_call_without_restructuring_keeps_its_fingerprint() {
    let base = run(&shop("", TWO_HELPERS));
    let one_line = run(&shop("", "    helper(); helper();\n"));
    assert_eq!(
        site_ids(&base.graph, "helper"),
        site_ids(&one_line.graph, "helper")
    );
}

#[test]
fn changing_ast_nesting_changes_fingerprint() {
    let base = run(&shop("", TWO_HELPERS));
    let nested = run(&shop(
        "",
        "    if (true) {\n      helper();\n    }\n    helper();\n",
    ));
    let before = site_ids(&base.graph, "helper");
    let after = sites_named(&nested.graph, "helper");
    assert_eq!(after.len(), 2);
    // The nested call is a different site; the other is now the first of its kind.
    assert!(!before.contains(&after[0].site_id));
    assert_eq!(after[1].site_id, before[0]);
}

#[test]
fn unrelated_call_and_definition_keep_identities() {
    let base = run(&shop("", TWO_HELPERS));
    let more = run(&shop(
        "  int unrelated() { return other(); }\n\n",
        "    other();\n    helper();\n    other();\n    helper();\n",
    ));
    let (b, m) = (&base.graph, &more.graph);
    assert_eq!(site_ids(b, "helper"), site_ids(m, "helper"));
    assert!(node_ids(b).is_subset(&node_ids(m)));
}

#[test]
fn moving_a_function_keeps_its_node_id() {
    let base = run(&shop("", TWO_HELPERS));
    let source = format!(
        "package com.acme;\n\npublic class Shop {{\n  int run() {{\n{TWO_HELPERS}    return 0;\n  }}\n\n  int other() {{ return 2; }}\n\n  int helper() {{ return 1; }}\n}}\n"
    );
    let swapped = run(&Checkout::new(&[(SHOP, source.as_str())]));
    assert_eq!(node_ids(&base.graph), node_ids(&swapped.graph));
    assert_eq!(
        site_ids(&base.graph, "helper"),
        site_ids(&swapped.graph, "helper")
    );
}

#[test]
fn unrelated_definition_keeps_overload_ids() {
    let overloads = |extra: &str| {
        let source = format!(
            "package com.acme;\n\npublic class Calc {{\n  int f(int a) {{ return a; }}\n{extra}  int f(String s) {{ return 0; }}\n  int f(java.util.List<String> xs, int n) {{ return n; }}\n}}\n"
        );
        run(&Checkout::new(&[(
            "src/main/java/com/acme/Calc.java",
            source.as_str(),
        )]))
    };
    let ids = |r: &Run| -> Vec<(String, NodeId)> {
        let mut found: Vec<_> = nodes_of(&r.graph, NodeKind::Method)
            .into_iter()
            .filter(|n| n.name == "f")
            .map(|n| (n.signature.clone().unwrap_or_default(), n.node_id.clone()))
            .collect();
        found.sort();
        found
    };
    let base = overloads("");
    assert_eq!(ids(&base).len(), 3);
    let more = overloads("  int g() { return 1; }\n  int f2(int a) { return a; }\n");
    assert_eq!(ids(&base), ids(&more));
    // Each overload's id is its signature's (4.2.1), whatever its position.
    let f_int = pdx_core::ids::NodeKey::definition(
        &repo(),
        NodeKind::Method,
        "src/main/java/com/acme/Calc.java",
        &base
            .graph
            .nodes
            .iter()
            .find(|n| n.name == "f")
            .unwrap()
            .qualified_name,
        "6da88c34",
    )
    .node_id()
    .unwrap();
    assert!(ids(&base).iter().any(|(_, id)| *id == f_int));
}

// --- routes' identities -------------------------------------------------------------

fn fastapi(before: &str, method: &str, route: &str) -> Checkout {
    let source = format!(
        "from fastapi import FastAPI\n\napp = FastAPI()\n{before}\n\n@app.{method}(\"{route}\")\ndef handler() -> dict:\n    return {{}}\n"
    );
    Checkout::new(&[("app/main.py", source.as_str())])
}

fn route_id(run: &Run) -> NodeId {
    let found = nodes_of(&run.graph, NodeKind::Route);
    assert_eq!(found.len(), 1, "{found:#?}");
    found[0].node_id.clone()
}

#[test]
fn route_id_follows_method_and_path_not_line() {
    let base = route_id(&run(&fastapi("", "get", "/users")));
    assert_ne!(base, route_id(&run(&fastapi("", "post", "/users"))));
    assert_ne!(base, route_id(&run(&fastapi("", "get", "/accounts"))));
    assert_eq!(
        base,
        route_id(&run(&fastapi("\n\n\n# moved\n", "get", "/users")))
    );
}

// --- no checkout path ---------------------------------------------------------------

/// A repository with modules, routes, tests and calls.
fn everything() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            CART,
            "package com.acme.shop;\n\npublic class Cart {\n  public int total() { return tax(); }\n  int tax() { return 1; }\n}\n",
        ),
        (
            "src/test/java/com/acme/shop/CartTest.java",
            "package com.acme.shop;\n\nimport org.junit.jupiter.api.Test;\n\nclass CartTest {\n  @Test\n  void totals() { new Cart().total(); }\n}\n",
        ),
        (
            "app/main.py",
            "from fastapi import FastAPI\n\napp = FastAPI()\n\n\n@app.get(\"/users\")\ndef users() -> list:\n    return []\n",
        ),
        (
            "src/server.js",
            "const express = require('express');\nconst app = express();\nfunction list(req, res) { res.json([]); }\napp.get('/items', list);\n",
        ),
        ("lib/pkg/__init__.py", ""),
        ("lib/pkg/sub.py", "def one():\n    return 1\n"),
    ]
}

#[test]
fn no_identity_depends_on_the_checkout() {
    let first = Checkout::new(&everything());
    let second = Checkout::new(&everything());
    let (a, b) = (run(&first), run(&second));
    assert_ne!(first.root(), second.root());
    assert_eq!(
        a.graph, b.graph,
        "two checkouts of one tree derive one graph"
    );
    let root = first.root().to_str().unwrap().to_owned();
    let temp = std::env::temp_dir();
    let temp = temp.to_str().unwrap();
    let text = serde_json::to_string(&(
        &a.graph.files,
        &a.graph.nodes,
        &a.graph.sites,
        &a.graph.edges,
        &a.graph.candidates,
    ))
    .unwrap();
    for needle in [root.as_str(), temp, "pdx-derive-test-"] {
        assert!(!text.contains(needle), "{needle} reached the graph");
    }
    for record in &a.graph.files {
        assert!(pdx_core::index::derive::containment::is_repository_path(
            &record.path
        ));
    }
}

// --- paths (Part Y) -----------------------------------------------------------------

#[test]
fn host_native_paths_are_refused() {
    use pdx_core::index::derive::containment::is_repository_path;
    for good in [
        "a.py",
        "src/a.py",
        "src/main/java/A.java",
        "ab:c/d.py",
        ".github/x.yml",
    ] {
        assert!(is_repository_path(good), "{good}");
    }
    for bad in [
        "",
        "/home/x/a.py",
        "src\\a.py",
        "C:/src/a.py",
        "c:\\src\\a.py",
        "a:b/c.py",
        "src//a.py",
        "./src/a.py",
        "src/../a.py",
        "src/",
    ] {
        assert!(!is_repository_path(bad), "{bad}");
    }
}

/// A name with a backslash is a POSIX file name, which Windows reads as a path: given
/// to the stage as a discovered path, it is refused, never made an identity.
#[cfg(unix)]
#[test]
fn backslash_path_never_reaches_an_identity() {
    let checkout = Checkout::new(&[("src\\a.py", "def a():\n    return 1\n")]);
    let root = checkout.root();
    let config = PdxConfig::load(root).unwrap();
    let files = discover(root, &config).unwrap();
    assert!(files.iter().any(|f| f.path == "src\\a.py"));
    let limits = ExtractLimits {
        requested_workers: 1,
        memory_budget_bytes: 64 << 20,
    };
    let extracted = ExtractStage::new(root, &config.secrets, limits)
        .run(&files)
        .unwrap();
    let registry = SymbolRegistry::build(root, &files, extracted).unwrap();
    let report = resolve(root, &registry).unwrap();
    let refused = derive(&DeriveInput {
        repo: &repo(),
        repo_name: "fixture",
        registry: &registry,
        resolution: &report,
    });
    assert!(
        matches!(&refused, Err(pdx_core::index::derive::DeriveError::NotRepositoryPath(p)) if p == "src\\a.py"),
        "{refused:?}"
    );
}

// --- file records (Part W) ----------------------------------------------------------

#[test]
fn file_records_are_faithful() {
    use pdx_core::model::FileStatus;
    let big = "x = 1\n".repeat(30);
    let checkout = Checkout::new(&[
        ("pdx.toml", "[discover]\nmax_file_bytes = 100\n"),
        ("src/a.py", "def a():\n    return 1\n"),
        (
            "src/broken.py",
            "def fine():\n    return 1\n\n\ndef broken(x:\n    return x\n",
        ),
        ("config/.env.production", "MODE=1\n"),
        ("assets/logo.bin", "\0\u{1}binary"),
        ("notes/readme.unknownext", "text\n"),
        ("src/big.py", big.as_str()),
    ]);
    let run = run(&checkout);
    let g = &run.graph;
    let record = |path: &str| {
        g.files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path}: {:#?}", g.files))
    };
    let paths: Vec<&str> = g.files.iter().map(|f| f.path.as_str()).collect();
    let mut sorted = paths.clone();
    sorted.sort_unstable();
    assert_eq!(paths, sorted);
    assert_eq!(paths.len(), 7, "every discovered file has a record");
    for f in &g.files {
        assert_eq!(Some(&f.file_id), g.file_nodes.get(&f.path));
    }

    // Read and parsed: Git's blob id of the bytes as read (`git hash-object`), its lines.
    let a = record("src/a.py");
    assert_eq!(
        (a.status, a.status_reason.as_deref()),
        (FileStatus::Parsed, None)
    );
    assert_eq!(
        a.blob_sha.as_deref(),
        Some("b1bfde2df873c68eb18a9b0954aaf23f71e981f2")
    );
    assert_eq!(
        (a.line_count, a.size_bytes, a.language.as_str()),
        (Some(2), 22, "python")
    );
    // Read, with a region the parse could not recover: `partial`, never `parsed`.
    let broken = record("src/broken.py");
    assert_eq!(
        (broken.status, broken.status_reason.as_deref()),
        (FileStatus::Partial, None)
    );
    assert_eq!(
        broken.blob_sha.as_deref(),
        Some("ac87d2024c4b6fafbc08903b6e279010ecc8381d")
    );
    assert_eq!(broken.line_count, Some(6));
    // Never read: no blob id, no line count (issue 51).
    for (path, status, reason) in [
        ("config/.env.production", FileStatus::Redacted, None),
        ("assets/logo.bin", FileStatus::Binary, None),
        (
            "notes/readme.unknownext",
            FileStatus::Skipped,
            Some("unknown_language"),
        ),
        ("src/big.py", FileStatus::Skipped, Some("size")),
    ] {
        let r = record(path);
        assert_eq!(
            (r.status, r.status_reason.as_deref()),
            (status, reason),
            "{path}"
        );
        assert_eq!(
            (r.blob_sha.as_deref(), r.line_count),
            (None, None),
            "{path}"
        );
    }
}

#[test]
fn engine_failures_are_never_parsed() {
    use pdx_core::index::derive::containment::file_status;
    use pdx_core::index::extract::{BlobSha, FileOutcome};
    use pdx_core::model::FileStatus;
    let blob_sha = BlobSha::of(b"x");
    for (outcome, expected) in [
        (
            FileOutcome::EngineFailed {
                blob_sha,
                error: pdx_engine::EngineError::Internal,
            },
            (FileStatus::Failed, Some("engine_error")),
        ),
        (
            FileOutcome::EngineCrashed { blob_sha },
            (FileStatus::Failed, Some("engine_crash")),
        ),
        (
            FileOutcome::SkippedMemory,
            (FileStatus::Skipped, Some("memory")),
        ),
        (
            FileOutcome::SkippedSize,
            (FileStatus::Skipped, Some("size")),
        ),
        (FileOutcome::Binary, (FileStatus::Binary, None)),
        (FileOutcome::Redacted, (FileStatus::Redacted, None)),
        (
            FileOutcome::UnknownLanguage,
            (FileStatus::Skipped, Some("unknown_language")),
        ),
    ] {
        assert_eq!(file_status(&outcome), expected, "{outcome:?}");
    }
}

// --- materialisation (issue 49, Part Q) ---------------------------------------------

/// The site of a resolution, by its file and position.
fn site_of<'g>(
    graph: &'g DerivedGraph,
    path: &str,
    call: &pdx_engine::Call,
) -> Option<&'g pdx_core::model::Site> {
    let file = &graph.file_nodes[path];
    let span = call.span?;
    graph.sites.iter().find(|s| {
        &s.file_id == file
            && s.span.start_byte == span.start_byte
            && s.span.end_byte == span.end_byte
    })
}

#[test]
fn drawn_edges_copy_engine_calibration() {
    let run = run(&Checkout::new(&[(
        "pkg/a.py",
        "def helper():\n    return 1\n\n\ndef run(xs):\n    xs.append(helper)\n    return helper()\n",
    )]));
    let g = &run.graph;
    let mut checked = 0;
    for resolved in &run.report.resolutions {
        let r = &resolved.resolution;
        let (true, Some(engine)) = (r.band().is_drawn(), r.engine()) else {
            continue;
        };
        let site = site_of(g, &resolved.site_ref.rel_path, &resolved.site).expect("a site");
        let edge = g
            .edges
            .iter()
            .find(|e| e.site_id.as_ref() == Some(&site.site_id))
            .expect("an edge");
        let expected = if resolved.site.is_reference {
            EdgeKind::CallReference
        } else {
            EdgeKind::Calls
        };
        assert_eq!(
            (edge.kind, edge.band, edge.observed),
            (expected, r.band(), false)
        );
        assert_eq!(
            edge.engine_score.map(f64::to_bits),
            Some(engine.score.to_bits())
        );
        assert_eq!(edge.engine_strategy, engine.engine_strategy);
        assert_eq!(edge.engine_candidates, Some(engine.candidates));
        checked += 1;
    }
    assert_eq!(checked, 2, "a call and a callable reference, both typed");
}

/// The stages over a checkout's registry, with these answers from the engine.
fn with_answers(
    registry: &SymbolRegistry,
    answers: Vec<pdx_engine::TypedResolution>,
) -> DerivedGraph {
    let health = pdx_engine::RunHealth {
        status: pdx_engine::RunStatus::Clean,
        files: 0,
        files_resolved: 0,
        files_untyped: 0,
        files_empty: 0,
        files_over_budget: 0,
        files_source_unavailable: 0,
        files_not_reached: 0,
        pass_failures: 0,
    };
    let report = pdx_core::resolve::stages::resolve_with(
        registry,
        &pdx_engine::ProjectResolution {
            resolutions: answers,
            health,
        },
    )
    .expect("resolution succeeds");
    derive(&DeriveInput {
        repo: &repo(),
        repo_name: "fixture",
        registry,
        resolution: &report,
    })
    .expect("derivation succeeds")
}

#[test]
fn candidate_rows_copy_engine_calibration() {
    // The engine says `tally`'s target is in no file of the project: an external
    // candidate row, with the engine's numbers exactly as given, and no edge.
    let checkout = Checkout::new(&[
        ("util.py", "def tally():\n    return 1\n"),
        ("main.py", "def run_all():\n    return tally()\n"),
    ]);
    let run = run(&checkout);
    let calls = &run.registry.extract("main.py").unwrap().calls;
    let index = calls.iter().position(|c| c.callee_text == "tally").unwrap();
    let answer = pdx_engine::TypedResolution {
        site_ref: pdx_engine::SiteRef {
            rel_path: "main.py".into(),
            call_index: u32::try_from(index).unwrap(),
        },
        site: calls[index].clone(),
        target_qn: "builtins.tally".into(),
        target_rel_path: None,
        score: 0.437_5,
        strategy: pdx_engine::Strategy::LspTyped,
        engine_strategy: Some("lsp_builtin_guess".into()),
        candidates: 7,
    };
    let g = with_answers(&run.registry, vec![answer]);
    let site = site_of(&g, "main.py", &calls[index]).expect("a site");
    assert!(
        g.edges
            .iter()
            .all(|e| e.site_id.as_ref() != Some(&site.site_id))
    );
    let row = g
        .candidates
        .iter()
        .find(|c| c.site_id == site.site_id)
        .expect("a candidate row");
    assert_eq!(row.band, pdx_core::bands::CandidateBand::External);
    assert_eq!(
        row.engine_score.map(f64::to_bits),
        Some(0.437_5f64.to_bits())
    );
    assert_eq!(row.engine_strategy.as_deref(), Some("lsp_builtin_guess"));
    assert_eq!(row.engine_candidates, Some(7));
    assert!(row.candidate_ids.is_empty());
    // Several survivors: candidate ids are the definitions' nodes, sorted, once each.
    let several = run_and_candidates(&[
        ("a/util.py", "def tally():\n    return 1\n"),
        ("b/util.py", "def tally():\n    return 2\n"),
        ("main.py", "def run_all():\n    return tally()\n"),
    ]);
    assert_eq!(several.len(), 1, "{several:#?}");
    let ids = &several[0].candidate_ids;
    assert_eq!(ids.len(), 2);
    assert!(ids.windows(2).all(|w| w[0] < w[1]));
}

fn run_and_candidates(files: &[(&str, &str)]) -> Vec<pdx_core::model::CandidateSite> {
    let run = run(&Checkout::new(files));
    run.graph
        .candidates
        .iter()
        .filter(|c| c.callee_name == "tally")
        .cloned()
        .collect()
}

#[test]
fn unconfirmed_sites_are_counted_never_drawn() {
    // `handler = compute` with no typed answer is not a call site: no site, no edge,
    // no candidate row; the stage carries it for counting.
    let checkout = Checkout::new(&[
        ("lib.py", "def compute():\n    return 1\n"),
        ("main.py", "from lib import compute\n\nhandler = compute\n"),
    ]);
    let run = run(&checkout);
    let calls = &run.registry.extract("main.py").unwrap().calls;
    let index = calls
        .iter()
        .position(|c| c.is_reference && c.callee_text == "compute")
        .expect("a reference");
    let g = with_answers(&run.registry, Vec::new());
    let site_ref = pdx_engine::SiteRef {
        rel_path: "main.py".into(),
        call_index: u32::try_from(index).unwrap(),
    };
    assert!(
        g.diagnostics
            .unconfirmed
            .iter()
            .any(|(s, reason)| *s == site_ref
                && *reason == pdx_core::resolve::stages::Unconfirmed::Reference)
    );
    assert!(site_of(&g, "main.py", &calls[index]).is_none());
    assert!(g.edges.iter().all(|e| e.kind != EdgeKind::CallReference));
    assert!(g.candidates.iter().all(|c| c.callee_name != "compute"));
}

#[test]
fn site_without_raw_position_is_counted_not_placed() {
    // A macro's expansion is found in preprocessed text: its calls have no position in
    // the file, so they are counted, never stored at 0:0.
    let run = run(&Checkout::new(&[(
        "src/main.c",
        "#define TWICE(f) f(); f()\n\nstatic int helper(void) { return 1; }\n\nint run(void) {\n  TWICE(helper);\n  helper();\n  return 0;\n}\n",
    )]));
    let g = &run.graph;
    let unplaced: Vec<_> = g
        .diagnostics
        .unmaterialized
        .iter()
        .filter(|u| u.missing == pdx_core::index::derive::Missing::Position)
        .collect();
    assert!(!unplaced.is_empty(), "{:#?}", g.diagnostics);
    for u in &unplaced {
        let call =
            &run.registry.extract("src/main.c").unwrap().calls[u.site_ref.call_index as usize];
        assert_eq!(call.span, None);
    }
    assert!(
        g.sites
            .iter()
            .all(|s| s.span.start_line > 0 && s.span.end_byte > 0)
    );
    // The one written call is a site and an edge.
    assert_eq!(
        sites_named(g, "helper")
            .iter()
            .filter(|s| s.site_kind == pdx_core::kinds::SiteKind::Call)
            .count(),
        1
    );
}

// --- determinism (Part X) -----------------------------------------------------------

#[test]
fn derive_is_invariant_to_input_order() {
    let checkout = Checkout::new(&everything());
    let base = run(&checkout);
    let reversed = run_with(&checkout, true, 1);
    let wide = run_with(&checkout, false, 4);
    assert_eq!(base.graph, reversed.graph);
    assert_eq!(base.graph, wide.graph);
    // Resolutions and unconfirmed sites in another order: the same graph.
    let mut report = base.report.clone();
    report.resolutions.reverse();
    report.unconfirmed.reverse();
    let again = derive(&DeriveInput {
        repo: &repo(),
        repo_name: "fixture",
        registry: &base.registry,
        resolution: &report,
    })
    .unwrap();
    assert_eq!(base.graph, again);
    // Sorted by stored identity.
    let g = &base.graph;
    assert!(g.files.windows(2).all(|w| w[0].path < w[1].path));
    assert!(g.nodes.windows(2).all(|w| w[0].node_id < w[1].node_id));
    assert!(g.sites.windows(2).all(|w| w[0].site_id < w[1].site_id));
    assert!(g.edges.windows(2).all(|w| w[0].edge_id < w[1].edge_id));
    assert!(g.candidates.windows(2).all(|w| w[0].site_id < w[1].site_id));
}

// --- into a segment -------------------------------------------------------------------

#[test]
fn derived_graph_writes_a_segment() {
    use pdx_core::segment::{
        SegmentData, SegmentMeta, SegmentProfile, SegmentReader, SegmentWriter,
    };
    let checkout = Checkout::new(&everything());
    let run = run(&checkout);
    let g = &run.graph;
    let mut data = SegmentData::new(SegmentMeta::new(
        repo(),
        "https://github.com/tensaicompl/Code-Explorer-2.0",
        "0123456789abcdef0123456789abcdef01234567",
        SegmentProfile::Structural,
        Vec::new(),
    ));
    data.files.clone_from(&g.files);
    data.nodes.clone_from(&g.nodes);
    data.sites.clone_from(&g.sites);
    data.edges.clone_from(&g.edges);
    data.candidates.clone_from(&g.candidates);
    let out = tempfile::tempdir().unwrap();
    let destination = out.path().join("segment.db");
    SegmentWriter::new()
        .build_in(out.path())
        .write(&data, &destination)
        .expect("the derived graph is a valid segment");
    let reader = SegmentReader::open(&destination).unwrap();
    for n in &g.nodes {
        assert_eq!(reader.node(&n.node_id).unwrap().as_ref(), Some(n));
    }
    for s in &g.sites {
        assert_eq!(reader.site(&s.site_id).unwrap().as_ref(), Some(s));
    }
    for f in &g.files {
        assert_eq!(reader.file_by_path(&f.path).unwrap().as_ref(), Some(f));
    }
    let totals = node(g, NodeKind::Test, "totals");
    let mut stored = reader.edges_from(&totals.node_id, &[]).unwrap();
    stored.sort_by(|a, b| a.edge_id.cmp(&b.edge_id));
    let mut derived: Vec<_> = g
        .edges
        .iter()
        .filter(|e| e.src == totals.node_id)
        .cloned()
        .collect();
    derived.sort_by(|a, b| a.edge_id.cmp(&b.edge_id));
    assert!(!derived.is_empty());
    assert_eq!(stored, derived);
}

// --- review closure: routes have real sites (issue 54) ---------------------------------

/// Every route of a graph, by name, with its `DEFINES_ROUTE` edges and their sites.
fn route_edges(graph: &DerivedGraph) -> BTreeMap<String, Vec<pdx_core::model::Edge>> {
    let mut out: BTreeMap<String, Vec<pdx_core::model::Edge>> = BTreeMap::new();
    for route in nodes_of(graph, NodeKind::Route) {
        let edges = graph
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::DefinesRoute && e.dst == route.node_id)
            .cloned()
            .collect();
        out.insert(route.name.clone(), edges);
    }
    out
}

fn spring(methods: &str) -> Checkout {
    let source = format!(
        "package com.acme.web;\n\nimport org.springframework.web.bind.annotation.*;\n\n@RestController\n@RequestMapping(\"/api\")\npublic class Api {{\n{methods}}}\n"
    );
    Checkout::new(&[("src/main/java/com/acme/web/Api.java", source.as_str())])
}

#[test]
fn spring_route_has_real_site() {
    let source = "package com.acme.web;\n\nimport org.springframework.web.bind.annotation.*;\n\n@RestController\n@RequestMapping(\"/api\")\npublic class Api {\n  @GetMapping(\"/users\")\n  public String list() { return \"\"; }\n}\n";
    let path = "src/main/java/com/acme/web/Api.java";
    let run = run(&Checkout::new(&[(path, source)]));
    let g = &run.graph;
    let edges = route_edges(g);
    let edge = &edges["GET /api/users"][0];
    let site = site_of_edge(g, edge);
    // The site is the annotation itself, positioned in the file.
    assert_eq!(
        &source[site.span.start_byte as usize..site.span.end_byte as usize],
        "@GetMapping(\"/users\")"
    );
    // Its identity is 4.2.1's, recomputed here from the facts: the handler, the
    // annotation's node-type path from it, its name, no receiver, ordinal 1.
    let list = node(g, NodeKind::Method, "list");
    let fingerprint = pdx_core::ids::ast_fingerprint(
        &["method_declaration", "modifiers", "annotation"],
        "GetMapping",
        "",
        1,
    )
    .unwrap();
    assert_eq!(site.ast_fingerprint, fingerprint);
    let key = pdx_core::ids::SiteKey::new(
        path,
        Some(list.node_id.clone()),
        pdx_core::kinds::SiteKind::Route,
        fingerprint,
    );
    assert_eq!(site.site_id, key.site_id().unwrap());
    assert_eq!(edge.band, pdx_core::bands::Band::Exact);
}

#[test]
fn spring_multiple_paths_create_multiple_routes() {
    let run = run(&spring(
        "  @GetMapping({\"/a\", \"/b\"})\n  public String both() { return \"\"; }\n",
    ));
    let g = &run.graph;
    let edges = route_edges(g);
    assert_eq!(
        edges.keys().map(String::as_str).collect::<Vec<_>>(),
        ["GET /api/a", "GET /api/b"]
    );
    let handler = node(g, NodeKind::Method, "both");
    let (a, b) = (&edges["GET /api/a"], &edges["GET /api/b"]);
    assert_eq!((a.len(), b.len()), (1, 1));
    assert_eq!((&a[0].src, &b[0].src), (&handler.node_id, &handler.node_id));
    // One annotation, one site, an edge to each route.
    assert_eq!(a[0].site_id, b[0].site_id);
    assert_ne!(a[0].edge_id, b[0].edge_id);
    assert_eq!(
        g.sites
            .iter()
            .filter(|s| s.site_kind == pdx_core::kinds::SiteKind::Route)
            .count(),
        1
    );
}

#[test]
fn spring_request_mapping_multiple_methods() {
    let run = run(&spring(
        "  @RequestMapping(path = {\"/x\", \"/y\"}, method = {RequestMethod.GET, RequestMethod.POST})\n  public String both() { return \"\"; }\n\n  @RequestMapping(\"/any\")\n  public String any() { return \"\"; }\n",
    ));
    let edges = route_edges(&run.graph);
    assert_eq!(
        edges.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "ANY /api/any",
            "GET /api/x",
            "GET /api/y",
            "POST /api/x",
            "POST /api/y"
        ]
    );
    let sites: std::collections::BTreeSet<_> = edges
        .iter()
        .filter(|(name, _)| !name.starts_with("ANY"))
        .map(|(_, e)| e[0].site_id.clone())
        .collect();
    assert_eq!(
        sites.len(),
        1,
        "four bindings of one annotation share its site"
    );
}

#[test]
fn spring_multiple_routes_have_deterministic_ids() {
    let methods = "  @GetMapping({\"/a\", \"/b\"})\n  public String both() { return \"\"; }\n";
    let first = run(&spring(methods));
    let second = run_with(&spring(methods), true, 1);
    let ids = |r: &Run| -> Vec<(String, String, String, String)> {
        route_edges(&r.graph)
            .into_iter()
            .map(|(name, e)| {
                (
                    name,
                    e[0].dst.as_str().to_owned(),
                    e[0].edge_id.as_str().to_owned(),
                    e[0].site_id.as_ref().unwrap().as_str().to_owned(),
                )
            })
            .collect()
    };
    assert_eq!(ids(&first), ids(&second));
    // Each Route id is the accepted identity for its own path, whatever else the
    // annotation lists.
    for path in ["/api/a", "/api/b"] {
        let key = pdx_core::index::derive::routes::route_node_key(
            &repo(),
            "src/main/java/com/acme/web/Api.java",
            "src.main.java.com.acme.web.Api.both",
            "GET",
            path,
        );
        assert_eq!(
            routes(&first.graph)[&format!("GET {path}")].node_id,
            key.node_id().unwrap()
        );
    }
}

#[test]
fn route_line_shift_keeps_route_site_id() {
    let files = |pad: &str| -> Vec<(String, String)> {
        vec![
            (
                "src/main/java/com/acme/web/Api.java".to_owned(),
                format!(
                    "package com.acme.web;\n\nimport org.springframework.web.bind.annotation.*;\n\n@RestController\npublic class Api {{\n{pad}  @GetMapping({{\"/a\", \"/b\"}})\n  public String both() {{ return \"\"; }}\n}}\n"
                ),
            ),
            (
                "app/main.py".to_owned(),
                format!(
                    "from fastapi import FastAPI\n\napp = FastAPI()\n{pad}\n\n@app.get(\"/one\")\n@app.post(\"/two\")\ndef handler() -> dict:\n    return {{}}\n"
                ),
            ),
            (
                "src/server.js".to_owned(),
                format!(
                    "const express = require('express');\nconst app = express();\n{pad}function list(req, res) {{ res.json([]); }}\napp.get('/items', list);\n"
                ),
            ),
        ]
    };
    let ids = |pad: &str| {
        let owned = files(pad);
        let borrowed: Vec<(&str, &str)> = owned
            .iter()
            .map(|(p, c)| (p.as_str(), c.as_str()))
            .collect();
        let run = run(&Checkout::new(&borrowed));
        route_edges(&run.graph)
            .into_iter()
            .map(|(name, e)| (name, e[0].site_id.clone(), e[0].edge_id.clone()))
            .collect::<Vec<_>>()
    };
    let before = ids("");
    assert_eq!(before.len(), 5, "{before:#?}");
    assert_eq!(before, ids("\n\n\n// a comment\n\n"));
}

#[test]
fn every_non_containment_edge_has_a_site() {
    let mut files = everything();
    files.extend([
        (
            "pkg/refs.py",
            "def helper():\n    return 1\n\n\ndef run(xs):\n    xs.append(helper)\n    return helper()\n",
        ),
        (
            "src/main/java/com/acme/web/Api.java",
            "package com.acme.web;\n\nimport org.springframework.web.bind.annotation.*;\n\n@RestController\npublic class Api {\n  @GetMapping({\"/a\", \"/b\"})\n  public String both() { return \"\"; }\n}\n",
        ),
    ]);
    let run = run(&Checkout::new(&files));
    let g = &run.graph;
    let sites: std::collections::BTreeSet<_> = g.sites.iter().map(|s| &s.site_id).collect();
    let mut kinds = std::collections::BTreeSet::new();
    for edge in &g.edges {
        assert_ne!(edge.kind, EdgeKind::Contains, "containment is parent_id");
        let site = edge
            .site_id
            .as_ref()
            .unwrap_or_else(|| panic!("a {} edge with no site: {edge:#?}", edge.kind));
        assert!(
            sites.contains(site),
            "a {} edge names a missing site",
            edge.kind
        );
        kinds.insert(edge.kind.as_str());
    }
    // Every kind P2-07 produces is held to it here.
    for kind in ["CALLS", "CALL_REFERENCE", "TESTS", "DEFINES_ROUTE"] {
        assert!(kinds.contains(kind), "no {kind} edge to check: {kinds:?}");
    }
}

// --- review closure: entry points are Appendix B.2's (issue 55) ------------------------

#[test]
fn exported_typescript_function_is_not_entry_point() {
    let run = run(&Checkout::new(&[(
        "src/h.ts",
        "export function helper(): number {\n  return 1;\n}\n",
    )]));
    let helper = node(&run.graph, NodeKind::Function, "helper");
    // The engine flags it; the graph does not take that for an entry point.
    assert!(prop_bool(helper, "engine_entry_point"));
    assert!(!prop_bool(helper, "is_entry_point"));
}

#[test]
fn exported_javascript_function_is_not_entry_point() {
    let run = run(&Checkout::new(&[(
        "src/h.js",
        "export function helper() {\n  return 1;\n}\n",
    )]));
    let helper = node(&run.graph, NodeKind::Function, "helper");
    assert!(prop_bool(helper, "engine_entry_point"));
    assert!(!prop_bool(helper, "is_entry_point"));
}

#[test]
fn main_entry_points_survive_engine_flag_filter() {
    let run = run(&Checkout::new(&[
        (
            "src/main/java/com/acme/App.java",
            "package com.acme;\n\npublic class App {\n  public static void main(String[] args) {}\n  public void main(int x) {}\n}\n",
        ),
        (
            "src/main/kotlin/Main.kt",
            "package app\n\nfun main(args: Array<String>) {}\n",
        ),
        ("csrc/main.c", "int main(void) { return 0; }\n"),
        (
            "cppsrc/main.cpp",
            "namespace geo { int main() { return 1; } }\nint main(int argc, char** argv) { return 0; }\n",
        ),
        ("cmd/tool/main.go", "package main\n\nfunc main() {}\n"),
        ("crate/src/main.rs", "fn main() {}\n"),
        ("crate/src/lib.rs", "pub fn main() {}\n"),
        (
            "dotnet/Program.cs",
            "namespace App { class Program { static void Main(string[] args) {} } }\n",
        ),
        ("py/tool.py", "def main():\n    return 0\n"),
    ]));
    let g = &run.graph;
    let entry = |path: &str, name: &str| -> Vec<bool> {
        let file = &g.file_nodes[path];
        g.nodes
            .iter()
            .filter(|n| n.name == name && n.file_id.as_ref() == Some(file))
            .map(|n| prop_bool(n, "is_entry_point"))
            .collect()
    };
    let mut java = entry("src/main/java/com/acme/App.java", "main");
    java.sort_unstable();
    assert_eq!(java, [false, true], "main(String[]) only");
    assert_eq!(entry("src/main/kotlin/Main.kt", "main"), [true]);
    assert_eq!(entry("csrc/main.c", "main"), [true]);
    let mut cpp = entry("cppsrc/main.cpp", "main");
    cpp.sort_unstable();
    assert_eq!(cpp, [false, true], "the global namespace's main only");
    assert_eq!(entry("cmd/tool/main.go", "main"), [true]);
    assert_eq!(entry("crate/src/main.rs", "main"), [true]);
    assert_eq!(
        entry("crate/src/lib.rs", "main"),
        [false],
        "a library's main"
    );
    assert_eq!(entry("dotnet/Program.cs", "Main"), [true]);
    assert_eq!(entry("py/tool.py", "main"), [false], "no convention");
}

#[test]
fn entry_point_categories_are_kept() {
    let run = run(&Checkout::new(&[
        (
            "src/main/java/com/acme/Boot.java",
            "package com.acme;\n\nimport org.springframework.boot.autoconfigure.SpringBootApplication;\n\n@SpringBootApplication\npublic class Boot {}\n",
        ),
        (
            "app/main.py",
            "from fastapi import FastAPI\n\napp = FastAPI()\n\n\n@app.get(\"/x\")\ndef x() -> dict:\n    return {}\n",
        ),
        (
            "web/server.ts",
            "import express from 'express';\n\nconst app = express();\n\nfunction list(req: any, res: any): void {\n  res.json([]);\n}\n\napp.get('/items', list);\n\nexport function start(): void {\n  app.listen(3000);\n}\n",
        ),
        ("tests/test_x.py", "def test_one():\n    assert True\n"),
    ]));
    let g = &run.graph;
    let at = |path: &str, kind: NodeKind, name: &str| -> bool {
        let file = &g.file_nodes[path];
        let found: Vec<&Node> = g
            .nodes
            .iter()
            .filter(|n| n.kind == kind && n.name == name && n.file_id.as_ref() == Some(file))
            .collect();
        assert_eq!(found.len(), 1, "{path} {kind} {name}");
        prop_bool(found[0], "is_entry_point")
    };
    assert!(at(
        "src/main/java/com/acme/Boot.java",
        NodeKind::Class,
        "Boot"
    ));
    assert!(
        at("app/main.py", NodeKind::Variable, "app"),
        "the FastAPI application"
    );
    assert!(
        at("app/main.py", NodeKind::Function, "x"),
        "a FastAPI route handler"
    );
    assert!(
        at("web/server.ts", NodeKind::Function, "list"),
        "a TypeScript route handler"
    );
    assert!(
        at("web/server.ts", NodeKind::Function, "start"),
        "it calls listen"
    );
    assert!(at("tests/test_x.py", NodeKind::Test, "test_one"));
    // An Express application object is not itself an entry point; its server's start is.
    assert!(!at("web/server.ts", NodeKind::Variable, "app"));
}

// --- review closure: repeated non-callables (issue 56) --------------------------------

#[test]
fn duplicate_noncallables_have_unique_deterministic_ids() {
    let source = "X = 1\nX = 2\n\n\nclass A:\n    pass\n\n\nclass A:\n    pass\n";
    let first = run(&Checkout::new(&[("pkg/a.py", source)]));
    let again = run_with(&Checkout::new(&[("pkg/a.py", source)]), true, 1);
    for kind in [NodeKind::Variable, NodeKind::Class] {
        let ids = |r: &Run| -> Vec<NodeId> {
            let mut found: Vec<&Node> = nodes_of(&r.graph, kind);
            found.sort_by_key(|n| n.start_line);
            found.iter().map(|n| n.node_id.clone()).collect()
        };
        let a = ids(&first);
        assert_eq!(a.len(), 2, "{kind}");
        assert_ne!(a[0], a[1], "{kind}: two declarations, two ids");
        assert_eq!(a, ids(&again), "{kind}: the same ids every run");
        // 4.2.1's rule: the empty signature's hash, numbered in file order.
        let qn = if kind == NodeKind::Class {
            "pkg.a.A"
        } else {
            "pkg.a.X"
        };
        for (n, id) in (1..).zip(&a) {
            let key = pdx_core::ids::NodeKey::definition(
                &repo(),
                kind,
                "pkg/a.py",
                qn,
                &format!("e3b0c442-{n}"),
            );
            assert_eq!(*id, key.node_id().unwrap(), "{kind} {n}");
        }
    }
}
