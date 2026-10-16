//! The golden format (decision 44) and its builder: segment rows in, the committed
//! structure out. The expected values here are written out, never derived with the
//! builder's own filtering or ordering.

use std::path::Path;

use pdx_bench::golden::{GoldenEdge, GoldenExpected, NodeKindCount};
use pdx_core::bands::Band;
use pdx_core::ids::{NodeKey, RepoId, SiteId, SiteKey, ast_fingerprint};
use pdx_core::index::extract::ExtractLimits;
use pdx_core::index::{IndexRequest, build_segment};
use pdx_core::kinds::{EdgeKind, NodeKind, SiteKind};
use pdx_core::model::{Edge, Node};
use pdx_core::segment::{RepoIdSource, SegmentReader};

fn repo() -> RepoId {
    RepoId::parse("ce63551447285fd4").expect("a repo id")
}

fn node(kind: NodeKind, qn: &str) -> Node {
    Node::new(&NodeKey::definition(&repo(), kind, "src/m.py", qn, ""), qn).expect("a node")
}

fn site(n: u32) -> SiteId {
    // Ordinals count from 1.
    let fingerprint = ast_fingerprint(&["call"], "f", "", n + 1).expect("a fingerprint");
    SiteKey::new("src/m.py", None, SiteKind::Call, fingerprint)
        .site_id()
        .expect("a site id")
}

fn edge(src: &Node, dst: &Node, kind: EdgeKind, band: Band, n: u32) -> Edge {
    Edge::new(
        src.node_id.clone(),
        dst.node_id.clone(),
        kind,
        band,
        Some(site(n)),
    )
}

fn tuple(src: &str, dst: &str, kind: &str, band: &str) -> GoldenEdge {
    GoldenEdge {
        src_qn: src.to_owned(),
        dst_qn: dst.to_owned(),
        kind: kind.to_owned(),
        band: band.to_owned(),
    }
}

#[test]
fn golden_builder_emits_only_drawn_edges() {
    let a = node(NodeKind::Function, "m.a");
    let b = node(NodeKind::Function, "m.b");
    // One edge per band, all eleven, in the order 4.2.2 lists them.
    let edges: Vec<Edge> = Band::ALL
        .iter()
        .enumerate()
        .map(|(i, band)| edge(&a, &b, EdgeKind::Calls, *band, u32::try_from(i).unwrap()))
        .collect();
    assert_eq!(edges.len(), 11);
    let golden = GoldenExpected::from_rows(&[a.clone(), b.clone()], &edges).expect("a golden");
    // The drawn bands of 4.2.2, written out: exactly these four, sorted by spelling.
    assert_eq!(
        golden.drawn_edges,
        vec![
            tuple("m.a", "m.b", "CALLS", "import-guided"),
            tuple("m.a", "m.b", "CALLS", "inheritance-guided"),
            tuple("m.a", "m.b", "CALLS", "precise"),
            tuple("m.a", "m.b", "CALLS", "typed"),
        ]
    );
    // And nothing at all from the seven that are not drawn, weak evidence (`exact`,
    // `scoped`; issue 102) among them.
    let none = [
        "exact",
        "scoped",
        "candidate",
        "external",
        "blocked",
        "unresolved",
        "contradicted",
    ];
    let undrawn: Vec<Edge> = none
        .iter()
        .enumerate()
        .map(|(i, spelling)| {
            let band = Band::parse(spelling).expect("a band");
            edge(
                &a,
                &b,
                EdgeKind::Calls,
                band,
                100 + u32::try_from(i).unwrap(),
            )
        })
        .collect();
    let golden = GoldenExpected::from_rows(&[a, b], &undrawn).expect("a golden");
    assert!(golden.drawn_edges.is_empty());
}

#[test]
fn golden_builder_keeps_duplicate_tuples() {
    let a = node(NodeKind::Function, "m.a");
    let b = node(NodeKind::Function, "m.b");
    // Two rows, two sites, two edge ids, one tuple: both are kept.
    let first = edge(&a, &b, EdgeKind::Calls, Band::ImportGuided, 1);
    let second = edge(&a, &b, EdgeKind::Calls, Band::ImportGuided, 2);
    assert_ne!(first.edge_id, second.edge_id);
    let golden = GoldenExpected::from_rows(&[a, b], &[first, second]).expect("a golden");
    assert_eq!(
        golden.drawn_edges,
        vec![
            tuple("m.a", "m.b", "CALLS", "import-guided"),
            tuple("m.a", "m.b", "CALLS", "import-guided"),
        ]
    );
}

#[test]
fn golden_node_counts_include_every_kind() {
    let nodes = [
        node(NodeKind::Function, "m.a"),
        node(NodeKind::Function, "m.b"),
        node(NodeKind::Class, "m.C"),
    ];
    let golden = GoldenExpected::from_rows(&nodes, &[]).expect("a golden");
    assert_eq!(golden.node_counts.len(), NodeKind::ALL.len());
    for kind in NodeKind::ALL {
        let count = golden
            .node_counts
            .iter()
            .filter(|c| c.kind == kind.as_str())
            .map(|c| c.count)
            .collect::<Vec<_>>();
        let expected = match kind {
            NodeKind::Function => 2,
            NodeKind::Class => 1,
            _ => 0,
        };
        assert_eq!(count, vec![expected], "{kind:?}");
    }
    assert_eq!(golden.node_total(), 3);
}

#[test]
fn golden_node_counts_are_sorted_by_spelling_not_declaration() {
    let golden = GoldenExpected::from_rows(&[], &[]).expect("a golden");
    let kinds: Vec<&str> = golden.node_counts.iter().map(|c| c.kind.as_str()).collect();
    // The declaration order starts with Repo, Folder, File; the spelling order with
    // the kinds below. A golden in declaration order would be another file.
    assert_eq!(kinds[..3], ["ApiContract", "Artifact", "ArtifactVersion"]);
    let declared: Vec<&str> = NodeKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_ne!(kinds, declared);
    assert!(kinds.windows(2).all(|w| w[0] < w[1]));
    assert!(golden.node_counts.iter().all(|c| c.count == 0));
}

#[test]
fn golden_edges_are_sorted_by_the_tuple() {
    let a = node(NodeKind::Function, "m.a");
    let b = node(NodeKind::Function, "m.b");
    let c = node(NodeKind::Class, "m.C");
    // Given out of order: src, then dst, then kind, then band decide, in that order.
    let edges = [
        edge(&b, &a, EdgeKind::Calls, Band::ImportGuided, 1),
        edge(&a, &b, EdgeKind::Calls, Band::Typed, 2),
        edge(&a, &b, EdgeKind::Calls, Band::ImportGuided, 3),
        edge(&a, &c, EdgeKind::Calls, Band::InheritanceGuided, 4),
        edge(&a, &b, EdgeKind::UsesType, Band::ImportGuided, 5),
        edge(&a, &a, EdgeKind::Calls, Band::InheritanceGuided, 6),
    ];
    let golden = GoldenExpected::from_rows(&[a, b, c], &edges).expect("a golden");
    assert_eq!(
        golden.drawn_edges,
        vec![
            tuple("m.a", "m.C", "CALLS", "inheritance-guided"),
            tuple("m.a", "m.a", "CALLS", "inheritance-guided"),
            tuple("m.a", "m.b", "CALLS", "import-guided"),
            tuple("m.a", "m.b", "CALLS", "typed"),
            tuple("m.a", "m.b", "USES_TYPE", "import-guided"),
            tuple("m.b", "m.a", "CALLS", "import-guided"),
        ]
    );
}

#[test]
fn golden_builder_refuses_an_unknown_endpoint() {
    let a = node(NodeKind::Function, "m.a");
    let b = node(NodeKind::Function, "m.b");
    let dangling = edge(&a, &b, EdgeKind::Calls, Band::Typed, 1);
    assert!(GoldenExpected::from_rows(std::slice::from_ref(&a), &[dangling]).is_err());
    assert!(GoldenExpected::from_rows(&[a.clone(), a], &[]).is_err());
}

#[test]
fn golden_canonical_bytes_are_exact() {
    let golden = GoldenExpected {
        node_counts: vec![
            NodeKindCount {
                kind: "Class".to_owned(),
                count: 12,
            },
            NodeKindCount {
                kind: "Function".to_owned(),
                count: 0,
            },
        ],
        drawn_edges: vec![tuple("a.A", "b.B", "CALLS", "typed")],
    };
    let expected = "{\n  \"node_counts\": [\n    {\n      \"kind\": \"Class\",\n      \"count\": 12\n    },\n    {\n      \"kind\": \"Function\",\n      \"count\": 0\n    }\n  ],\n  \"drawn_edges\": [\n    {\n      \"src_qn\": \"a.A\",\n      \"dst_qn\": \"b.B\",\n      \"kind\": \"CALLS\",\n      \"band\": \"typed\"\n    }\n  ]\n}\n";
    assert_eq!(
        String::from_utf8(golden.to_canonical_bytes()).expect("UTF-8"),
        expected
    );
    assert_eq!(
        GoldenExpected::parse(expected.as_bytes()).expect("parses"),
        golden
    );
    // An empty graph is still both fields.
    let empty = GoldenExpected {
        node_counts: vec![],
        drawn_edges: vec![],
    };
    assert_eq!(
        empty.to_canonical_bytes(),
        b"{\n  \"node_counts\": [],\n  \"drawn_edges\": []\n}\n"
    );
}

#[test]
fn golden_format_has_no_other_field() {
    for text in [
        r#"{"node_counts":[],"drawn_edges":[],"generated_at":"2026-01-01"}"#,
        r#"{"node_counts":[{"kind":"Class","count":1,"path":"x"}],"drawn_edges":[]}"#,
        r#"{"node_counts":[],"drawn_edges":[{"src_qn":"a","dst_qn":"b","kind":"CALLS","band":"typed","edge_id":"x"}]}"#,
        r#"{"node_counts":[]}"#,
    ] {
        assert!(GoldenExpected::parse(text.as_bytes()).is_err(), "{text}");
    }
}

/// A checkout on disk with these files.
fn checkout(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a directory");
    for (path, content) in files {
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
        std::fs::write(path, content).expect("a file");
    }
    dir
}

#[test]
fn golden_from_a_built_segment_holds_its_drawn_edges_only() {
    // A real segment, built by the pipeline: one call it resolves, one it cannot.
    let source = checkout(&[(
        "src/shop.py",
        "def compute_total_price():\n    return 1\n\n\ndef checkout_basket():\n    compute_total_price()\n    unknown_helper_xyz()\n",
    )]);
    let out = tempfile::tempdir().expect("a directory");
    let destination = out.path().join("segment.db");
    build_segment(IndexRequest {
        root: source.path(),
        repo_id: repo(),
        repo_id_source: RepoIdSource::Registered,
        repo_url: "https://git.example/acme/shop".to_owned(),
        repo_name: "shop".to_owned(),
        commit_sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        limits: ExtractLimits {
            requested_workers: 1,
            memory_budget_bytes: 64 << 20,
        },
        cache: None,
        backend: None,
        destination: destination.clone(),
        build_dir: None,
        progress: None,
    })
    .expect("the build succeeds");
    let reader = SegmentReader::open(Path::new(&destination)).expect("the segment opens");
    let nodes = reader.nodes_all().expect("nodes");
    let edges = reader.edges_all().expect("edges");
    let golden = GoldenExpected::from_rows(&nodes, &edges).expect("a golden");

    // The unresolved call is a non-drawn state of the segment, and never a golden edge.
    let caller = nodes
        .iter()
        .find(|n| n.name == "checkout_basket")
        .expect("the caller");
    assert!(
        !reader
            .candidates_from(&caller.node_id)
            .expect("candidates")
            .is_empty(),
        "the unresolved call is stored as a candidate row"
    );
    assert!(
        golden
            .drawn_edges
            .iter()
            .all(|e| e.dst_qn != "unknown_helper_xyz")
    );
    // The drawn edges, by the drawn spellings of 4.2.2 written out here (`exact` and
    // `scoped` are not drawn, issue 102).
    let drawn = ["precise", "typed", "import-guided", "inheritance-guided"];
    let qn = |id: &pdx_core::ids::NodeId| {
        nodes
            .iter()
            .find(|n| &n.node_id == id)
            .expect("an endpoint")
            .qualified_name
            .clone()
    };
    let mut expected: Vec<GoldenEdge> = edges
        .iter()
        .filter(|e| drawn.contains(&e.band.as_str()))
        .map(|e| tuple(&qn(&e.src), &qn(&e.dst), e.kind.as_str(), e.band.as_str()))
        .collect();
    expected.sort_by(|x, y| {
        (&x.src_qn, &x.dst_qn, &x.kind, &x.band).cmp(&(&y.src_qn, &y.dst_qn, &y.kind, &y.band))
    });
    assert_eq!(golden.drawn_edges, expected);
    assert!(
        golden
            .drawn_edges
            .iter()
            .any(|e| e.kind == "CALLS" && e.dst_qn.ends_with("compute_total_price")),
        "{:?}",
        golden.drawn_edges
    );
    assert_eq!(golden.node_total(), u64::try_from(nodes.len()).unwrap());
}
