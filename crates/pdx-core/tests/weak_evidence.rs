//! Issue 102 (R3, decision 65): `exact` and `scoped` are weak evidence. They keep their
//! names, order and the one candidate their stage narrowed to, and are measured and
//! counted, but never draw an edge: no target, no `CALLS` or `TESTS` edge, a
//! `candidate` row whose reason names the band. `typed`, `import-guided` and
//! `inheritance-guided` draw as before.
//!
//! One real crate, written to a checkout and resolved through discovery, extraction,
//! typed resolution and Stage 3, then built to a segment and read back: one site of each
//! band (R3-1 to R3-10). Each site is one typed resolution cannot answer, so the band is
//! Stage 3's (since issue 105 it types a parameter's, a constructor's or `self`'s
//! receiver, and a type's member through an import): a tuple struct's constructor, a
//! call on `self` in a trait's default method, methods on values nothing types.

use std::fs;
use std::path::Path;

use pdx_core::bands::{Band, CandidateBand};
use pdx_core::config::PdxConfig;
use pdx_core::ids::RepoId;
use pdx_core::index::discover::discover;
use pdx_core::index::extract::{ExtractLimits, ExtractStage};
use pdx_core::index::{IndexRequest, build_segment};
use pdx_core::kinds::EdgeKind;
use pdx_core::model::{CandidateSite, Edge, Node};
use pdx_core::resolve::registry::{DefinitionRef, SymbolRegistry};
use pdx_core::resolve::stages::{Resolution, ResolveReport, resolve};
use pdx_core::segment::{RepoIdSource, SegmentReader};

const FILES: &[(&str, &str)] = &[
    (
        "Cargo.toml",
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    ),
    (
        "src/lib.rs",
        "pub mod kinds;\npub mod other;\npub mod twins;\npub mod uses;\n",
    ),
    (
        "src/kinds.rs",
        "pub struct Gauge {\n    pub n: u8,\n}\n\nimpl Gauge {\n    pub fn assemble(n: u8) -> Gauge {\n        Gauge { n }\n    }\n\n    pub fn reading(&self) -> u8 {\n        self.n\n    }\n}\n\npub struct Pair(pub u8, pub u8);\n\npub trait Scale {\n    fn unit(&self) -> u8;\n    fn twice(&self) -> u8 {\n        self.unit() * 2\n    }\n}\n\npub struct Wrap<H>(pub H);\n\nimpl<H> Scale for Wrap<H> {\n    fn unit(&self) -> u8 {\n        1\n    }\n}\n\npub fn local_helper() -> u8 {\n    1\n}\n\npub fn calls_local() -> u8 {\n    local_helper()\n}\n",
    ),
    (
        "src/other.rs",
        "pub struct Other;\n\nimpl Other {\n    pub fn tick(&self) -> u8 {\n        2\n    }\n}\n",
    ),
    (
        "src/twins.rs",
        "pub struct Clock;\n\npub struct Left;\n\nimpl Left {\n    pub fn chime(&self) -> u8 {\n        1\n    }\n}\n\npub struct Right;\n\nimpl Right {\n    pub fn chime(&self) -> u8 {\n        2\n    }\n}\n",
    ),
    (
        "src/uses.rs",
        "use crate::kinds::{Pair};\nuse crate::twins::Clock;\n\npub struct Counter;\n\nimpl Counter {\n    pub fn tick(&self) -> u8 {\n        1\n    }\n}\n\npub fn grouped() -> Pair {\n    Pair(1, 2)\n}\n\npub fn by_value<T>(g: T) -> u8 {\n    g.reading()\n}\n\npub fn scoped_tick<T>(c: T) -> u8 {\n    c.tick()\n}\n\npub fn ambiguous(x: &Clock) -> u8 {\n    x.chime()\n}\n\n#[test]\nfn reads_a_gauge() {\n    let p = Pair(3, 4);\n    let _ = make_gauge(p).reading();\n}\n",
    ),
];

const LIMITS: ExtractLimits = ExtractLimits {
    requested_workers: 2,
    memory_budget_bytes: 64 << 20,
};

struct Fixture {
    _dir: tempfile::TempDir,
    _out: tempfile::TempDir,
    reg: SymbolRegistry,
    report: ResolveReport,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    candidates: Vec<CandidateSite>,
    by_band: Vec<(Band, u64)>,
}

fn fixture() -> &'static Fixture {
    static FIXTURE: std::sync::OnceLock<Fixture> = std::sync::OnceLock::new();
    FIXTURE.get_or_init(|| {
        let dir = tempfile::Builder::new()
            .prefix("pdx-weak-evidence-test-")
            .tempdir()
            .expect("a directory");
        for (path, content) in FILES {
            let path = dir.path().join(path);
            fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
            fs::write(&path, content).expect("a file");
        }
        let root: &Path = dir.path();
        let config = PdxConfig::load(root).expect("the configuration loads");
        let files = discover(root, &config).expect("discovery succeeds");
        let extracted = ExtractStage::new(root, &config.secrets, LIMITS)
            .run(&files)
            .expect("extraction succeeds");
        let reg = SymbolRegistry::build(root, &files, extracted).expect("the registry builds");
        let report = resolve(root, &reg).expect("resolution succeeds");
        // The same checkout, built whole to a segment and read back.
        let out = tempfile::tempdir().expect("an output directory");
        let destination = out.path().join("segment.db");
        build_segment(IndexRequest {
            root,
            repo_id: RepoId::parse("ce63551447285fd4").expect("a repo id"),
            repo_id_source: RepoIdSource::Registered,
            repo_url: "https://git.example/acme/fixture.git".to_owned(),
            repo_name: "fixture".to_owned(),
            commit_sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            limits: LIMITS,
            cache: None,
            backend: None,
            destination: destination.clone(),
            build_dir: None,
            progress: None,
        })
        .expect("the build succeeds");
        let segment = SegmentReader::open(&destination).expect("the segment opens");
        let nodes = segment.nodes_all().expect("nodes");
        let edges = segment.edges_all().expect("edges");
        let candidates = nodes
            .iter()
            .flat_map(|n| segment.candidates_from(&n.node_id).expect("candidates"))
            .collect();
        let rust = segment
            .coverage("rust")
            .expect("coverage")
            .expect("rust coverage");
        let by_band = Band::ALL
            .iter()
            .map(|b| (*b, rust.by_band.get(*b)))
            .collect();
        Fixture {
            _dir: dir,
            _out: out,
            reg,
            report,
            nodes,
            edges,
            candidates,
            by_band,
        }
    })
}

/// What the one site of `path` spelled `callee` resolved to, by its line.
fn at(path: &str, callee: &str, line: u32) -> &'static Resolution {
    let f = fixture();
    let found: Vec<&Resolution> = f
        .report
        .resolutions
        .iter()
        .filter(|s| {
            s.site_ref.rel_path == path
                && s.call(&f.reg)
                    .and_then(|c| c.span)
                    .is_some_and(|span| span.start_line == line)
                && f.reg
                    .text(path, &s.call(&f.reg).expect("a call").callee_text)
                    .is_some_and(|t| t == callee)
        })
        .map(|s| &s.resolution)
        .collect();
    assert_eq!(found.len(), 1, "{callee} in {path}:{line}: {found:?}");
    found[0]
}

/// The definition, and its node, whose qualified name is `qn`.
fn def(qn: &str) -> (DefinitionRef, &'static Node) {
    let f = fixture();
    let found: Vec<DefinitionRef> = f
        .reg
        .files()
        .flat_map(|path| {
            let defs = f.reg.extract(path).map_or(0, |e| e.definitions.len());
            (0..defs).filter_map(move |i| {
                let r = DefinitionRef {
                    path: path.into(),
                    index: u32::try_from(i).ok()?,
                };
                (f.reg.qualified_name(&r).as_deref() == Some(qn)).then_some(r)
            })
        })
        .collect();
    let [r] = &found[..] else {
        panic!("one definition {qn}: {found:?}");
    };
    let nodes: Vec<&Node> = f.nodes.iter().filter(|n| n.qualified_name == qn).collect();
    let [node] = &nodes[..] else {
        panic!("one node {qn}: {nodes:?}");
    };
    (r.clone(), node)
}

/// The drawn edges of `kind` into the node `qn`, by their bands.
fn edges_into(kind: EdgeKind, qn: &str) -> Vec<Band> {
    let (_, node) = def(qn);
    fixture()
        .edges
        .iter()
        .filter(|e| e.kind == kind && e.dst == node.node_id && e.band.is_drawn())
        .map(|e| e.band)
        .collect()
}

/// The candidate rows naming the node `qn`.
fn rows_naming(qn: &str) -> Vec<&'static CandidateSite> {
    let (_, node) = def(qn);
    fixture()
        .candidates
        .iter()
        .filter(|c| c.candidate_ids.contains(&node.node_id))
        .collect()
}

fn assert_drawn(got: &Resolution, band: Band, target: &DefinitionRef) {
    assert!(band.is_drawn());
    assert_eq!(got.band(), band, "{got:?}");
    assert_eq!(got.target(), Some(target), "{got:?}");
    assert!(got.candidates().is_empty(), "{got:?}");
}

#[test]
fn r3_1_typed_remains_drawn() {
    let (helper, _) = def("src.kinds.local_helper");
    assert_drawn(at("src/kinds.rs", "local_helper", 37), Band::Typed, &helper);
    assert_eq!(
        edges_into(EdgeKind::Calls, "src.kinds.local_helper"),
        [Band::Typed]
    );
}

#[test]
fn r3_2_import_guided_remains_drawn() {
    let (pair, _) = def("src.kinds.Pair");
    assert_drawn(at("src/uses.rs", "Pair", 13), Band::ImportGuided, &pair);
    // The call in `grouped` and the test's: both drawn, the test's with its `TESTS`.
    assert_eq!(
        edges_into(EdgeKind::Calls, "src.kinds.Pair"),
        [Band::ImportGuided, Band::ImportGuided]
    );
    assert_eq!(
        edges_into(EdgeKind::Tests, "src.kinds.Pair"),
        [Band::ImportGuided]
    );
}

#[test]
fn r3_3_inheritance_guided_remains_drawn() {
    let (unit, _) = def("src.kinds.Scale.unit");
    assert_drawn(
        at("src/kinds.rs", "self.unit", 20),
        Band::InheritanceGuided,
        &unit,
    );
    assert_eq!(
        edges_into(EdgeKind::Calls, "src.kinds.Scale.unit"),
        [Band::InheritanceGuided]
    );
}

#[test]
fn r3_4_exact_keeps_its_candidate_and_draws_nothing() {
    let (reading, node) = def("src.kinds.Gauge.reading");
    let got = at("src/uses.rs", "g.reading", 17);
    assert_eq!(got.band(), Band::Exact, "{got:?}");
    assert_eq!(got.target(), None, "{got:?}");
    assert_eq!(got.weak_candidate(), Some(&reading));
    assert_eq!(got.candidates(), [reading]);
    // No edge into it at all, drawn or not; a candidate row for each of its two sites.
    assert!(
        fixture()
            .edges
            .iter()
            .all(|e| e.dst != node.node_id || !e.band.is_weak_evidence())
    );
    assert!(edges_into(EdgeKind::Calls, "src.kinds.Gauge.reading").is_empty());
    let rows = rows_naming("src.kinds.Gauge.reading");
    assert_eq!(rows.len(), 2, "{rows:#?}");
    for row in rows {
        assert_eq!(row.band, CandidateBand::Candidate);
        assert_eq!(row.candidate_ids, std::slice::from_ref(&node.node_id));
        assert!(row.reason.starts_with("exact: "), "{}", row.reason);
    }
}

#[test]
fn r3_5_scoped_keeps_its_candidate_and_draws_nothing() {
    let (tick, node) = def("src.uses.Counter.tick");
    let got = at("src/uses.rs", "c.tick", 21);
    assert_eq!(got.band(), Band::Scoped, "{got:?}");
    assert_eq!(got.target(), None, "{got:?}");
    assert_eq!(got.weak_candidate(), Some(&tick));
    assert!(edges_into(EdgeKind::Calls, "src.uses.Counter.tick").is_empty());
    let [row] = rows_naming("src.uses.Counter.tick")[..] else {
        panic!("one row");
    };
    assert_eq!(row.band, CandidateBand::Candidate);
    assert_eq!(row.candidate_ids, std::slice::from_ref(&node.node_id));
    assert!(row.reason.starts_with("scoped: "), "{}", row.reason);
}

#[test]
fn r3_6_candidate_is_unchanged() {
    let got = at("src/uses.rs", "x.chime", 25);
    assert_eq!(got.band(), Band::Candidate, "{got:?}");
    assert_eq!(got.target(), None);
    assert_eq!(got.weak_candidate(), None);
    let (left, left_node) = def("src.twins.Left.chime");
    let (right, right_node) = def("src.twins.Right.chime");
    let mut expected = vec![left, right];
    expected.sort();
    assert_eq!(got.candidates(), expected);
    let [row] = rows_naming("src.twins.Left.chime")[..] else {
        panic!("one row");
    };
    assert_eq!(row.band, CandidateBand::Candidate);
    let mut ids = vec![left_node.node_id, right_node.node_id];
    ids.sort();
    assert_eq!(row.candidate_ids, ids);
    assert_eq!(row.reason, "several definitions survive every stage");
}

#[test]
fn r3_7_r3_8_exact_and_scoped_still_report_their_bands() {
    // Coverage counts Stage 3's bands: two `exact` sites and one `scoped`, as before.
    let by_band = &fixture().by_band;
    let count = |b: Band| by_band.iter().find(|(x, _)| *x == b).map(|(_, n)| *n);
    assert_eq!(count(Band::Exact), Some(2));
    assert_eq!(count(Band::Scoped), Some(1));
    assert_eq!(count(Band::Candidate), Some(1));
    assert_eq!(count(Band::Typed), Some(1));
    assert_eq!(count(Band::ImportGuided), Some(2));
    assert_eq!(count(Band::InheritanceGuided), Some(1));
}

#[test]
fn r3_9_exact_and_scoped_never_enter_calls_or_tests() {
    let f = fixture();
    assert!(
        f.edges.iter().all(|e| !e.band.is_weak_evidence()),
        "{:#?}",
        f.edges
            .iter()
            .filter(|e| e.band.is_weak_evidence())
            .collect::<Vec<_>>()
    );
    // The test calls `reading` on a value too: no `TESTS` edge for it.
    assert!(edges_into(EdgeKind::Tests, "src.kinds.Gauge.reading").is_empty());
    // Every call edge is one of the three drawn resolution bands.
    for e in f.edges.iter().filter(|e| e.kind == EdgeKind::Calls) {
        assert!(
            matches!(
                e.band,
                Band::Typed | Band::ImportGuided | Band::InheritanceGuided
            ),
            "{e:?}"
        );
    }
}

#[test]
fn r3_10_no_reader_sees_weak_evidence_as_a_target() {
    let f = fixture();
    // Stage 3: a weak band never has a target, whatever reads it.
    for r in &f.report.resolutions {
        let got = &r.resolution;
        if got.band().is_weak_evidence() {
            assert_eq!(got.target(), None, "{got:?}");
            assert_eq!(got.candidates().len(), 1, "{got:?}");
        } else {
            assert_eq!(got.weak_candidate(), None, "{got:?}");
        }
        assert_eq!(got.target().is_some(), got.band().is_drawn(), "{got:?}");
    }
    // The segment: a weak band's row is a candidate row, its band one the table names.
    for row in &f.candidates {
        assert!(!Band::from(row.band).is_drawn());
        assert!(!Band::from(row.band).is_weak_evidence());
    }
    let (_, reading) = def("src.kinds.Gauge.reading");
    let segment_edges_into_reading = f.edges.iter().filter(|e| e.dst == reading.node_id).count();
    assert_eq!(segment_edges_into_reading, 0);
}
