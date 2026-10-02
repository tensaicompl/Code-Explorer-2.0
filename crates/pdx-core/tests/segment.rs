//! Segments (specification 4.3): written once, byte for byte the same from the same
//! rows, read back exactly through typed queries, refused when they cannot be trusted.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use pdx_core::bands::{Band, CandidateBand};
use pdx_core::ids::{self, NodeId, NodeKey, RepoId, SiteKey};
use pdx_core::kinds::{ContractKind, EdgeKind, LayerRole, NodeKind, SiteKind};
use pdx_core::model::{
    Authority, BandCounts, CandidateSite, Contract, ContractDirection, Coverage, Edge, Evidence,
    FactKind, FileRecord, FileStatus, IdentityStrength, Metric, Node, OccurrenceRole, Props,
    SemanticOccurrence, Site, Span, Verdict,
};
use pdx_core::segment::queries::testing;
use pdx_core::segment::{
    PreciseSource, SegmentData, SegmentError, SegmentMeta, SegmentProfile, SegmentReader,
    SegmentWriter, WrittenSegment,
};
use serde_json::json;
use sha2::{Digest, Sha256};

// --- fixtures ----------------------------------------------------------------------

/// A temporary directory that can be removed even when it holds read-only segments,
/// which Windows otherwise refuses to delete.
struct Scratch(tempfile::TempDir);

impl Scratch {
    fn new() -> Self {
        Self(
            tempfile::Builder::new()
                .prefix("pdx-segment-test-")
                .tempdir()
                .expect("a scratch directory"),
        )
    }

    fn path(&self) -> &Path {
        self.0.path()
    }

    fn dir(&self, name: &str) -> PathBuf {
        let dir = self.path().join(name);
        std::fs::create_dir_all(&dir).expect("a directory");
        dir
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fn writable(dir: &Path) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    writable(&path);
                } else if let Ok(metadata) = path.metadata() {
                    let mut permissions = metadata.permissions();
                    #[allow(clippy::permissions_set_readonly_false)]
                    permissions.set_readonly(false);
                    let _ = std::fs::set_permissions(&path, permissions);
                }
            }
        }
        writable(self.0.path());
    }
}

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const JAVA: &str = "src/main/Inventory.java";
const GUIDE: &str = "docs/guide.md";

fn repo() -> RepoId {
    RepoId::parse("ce63551447285fd4").expect("a repository id")
}

fn file_id(path: &str) -> NodeId {
    NodeKey::file(&repo(), path).node_id().expect("an id")
}

fn method(qn: &str) -> NodeKey {
    NodeKey::definition(&repo(), NodeKind::Method, JAVA, qn, "")
}

fn id(key: &NodeKey) -> NodeId {
    key.node_id().expect("an id")
}

fn props(pairs: &[(&str, serde_json::Value)]) -> Props {
    let mut props = Props::default();
    for (k, v) in pairs {
        props.insert(k, v.clone());
    }
    props
}

/// A small precise segment with a row in every table and every kind of value: NULLs and
/// non-NULLs, JSON, several bands, an observed edge, engine fields, candidates,
/// contracts, evidence, occurrences, metrics, coverage and Unicode text. One function,
/// because its rows share ids that splitting it would scatter.
#[allow(clippy::too_many_lines)]
fn fixture() -> SegmentData {
    let meta = SegmentMeta::new(
        repo(),
        "https://github.com/tensaicompl/Code-Explorer-2.0",
        COMMIT,
        SegmentProfile::Precise,
        vec![
            PreciseSource {
                provider: "scip-java".to_owned(),
                version: "0.10.2".to_owned(),
                container_digest: "sha256:aaaa".to_owned(),
            },
            PreciseSource {
                provider: "rust-analyzer".to_owned(),
                version: "2026-09-01".to_owned(),
                container_digest: "sha256:bbbb".to_owned(),
            },
        ],
    );
    let mut data = SegmentData::new(meta);

    data.files = vec![
        FileRecord {
            file_id: file_id(JAVA),
            path: JAVA.to_owned(),
            language: "java".to_owned(),
            status: FileStatus::Parsed,
            status_reason: None,
            blob_sha: "1111111111111111111111111111111111111111".to_owned(),
            size_bytes: 3330,
            line_count: 120,
        },
        FileRecord {
            file_id: file_id(GUIDE),
            path: GUIDE.to_owned(),
            language: "markdown".to_owned(),
            status: FileStatus::Partial,
            status_reason: Some("an unclosed code fence".to_owned()),
            blob_sha: "2222222222222222222222222222222222222222".to_owned(),
            size_bytes: 812,
            line_count: 40,
        },
    ];

    let repo_node = Node::new(&NodeKey::repo(&repo()), "Code-Explorer-2.0").expect("a node");
    let mut java_file = Node::new(&NodeKey::file(&repo(), JAVA), "Inventory.java").expect("a node");
    java_file.file_id = Some(file_id(JAVA));
    java_file.parent_id = Some(repo_node.node_id.clone());
    let mut guide_file = Node::new(&NodeKey::file(&repo(), GUIDE), "guide.md").expect("a node");
    guide_file.file_id = Some(file_id(GUIDE));
    guide_file.parent_id = Some(repo_node.node_id.clone());
    let class_key = NodeKey::definition(&repo(), NodeKind::Class, JAVA, "com.acme.Inventory", "");
    let mut class = Node::new(&class_key, "Inventory")
        .expect("a node")
        .with_lines(Some(3), Some(118));
    class.file_id = Some(file_id(JAVA));
    class.parent_id = Some(java_file.node_id.clone());
    class.layer_role = Some(LayerRole::Domain);
    class.doc = Some("Tracks stock levels — Bestände, in-store and online.".to_owned());
    let mut add = Node::new(&method("com.acme.Inventory.add"), "add")
        .expect("a node")
        .with_lines(Some(12), Some(18));
    add.file_id = Some(file_id(JAVA));
    add.parent_id = Some(class.node_id.clone());
    add.signature = Some("(String,int)".to_owned());
    add.doc = Some("Adds an item's quantity; rejects a negative one.".to_owned());
    add.props = props(&[
        ("visibility", json!("public")),
        ("is_entry_point", json!(false)),
    ]);
    let mut remove = Node::new(&method("com.acme.Inventory.remove"), "remove")
        .expect("a node")
        .with_lines(Some(20), Some(31));
    remove.file_id = Some(file_id(JAVA));
    remove.parent_id = Some(class.node_id.clone());
    remove.props = props(&[("nested", json!({"z": [1, 2], "a": "ü"}))]);
    let mut section = Node::new(
        &NodeKey::definition(&repo(), NodeKind::Doc, GUIDE, "docs/guide.md#Überblick", ""),
        "Überblick",
    )
    .expect("a node")
    .with_lines(Some(1), Some(14));
    section.file_id = Some(file_id(GUIDE));
    section.parent_id = Some(guide_file.node_id.clone());
    section.doc = Some("Ein Überblick über das Lager.".to_owned());

    let fingerprint = ids::ast_fingerprint(
        &["method_declaration", "method_invocation"],
        "remove",
        "this",
        1,
    )
    .expect("a fingerprint");
    let site_key = SiteKey::new(JAVA, Some(add.node_id.clone()), SiteKind::Call, fingerprint);
    let site = Site::new(
        &site_key,
        file_id(JAVA),
        Span {
            start_byte: 310,
            end_byte: 325,
            start_line: 14,
            start_col: 8,
            end_line: 14,
            end_col: 23,
        },
    )
    .expect("a site")
    .with_texts(Some("remove"), Some("this"));
    let import_fingerprint = ids::ast_fingerprint(&["import_declaration"], "java.util.Map", "", 1)
        .expect("a fingerprint");
    let import_site = Site::new(
        &SiteKey::new(JAVA, None, SiteKind::Import, import_fingerprint),
        file_id(JAVA),
        Span {
            start_byte: 0,
            end_byte: 22,
            start_line: 1,
            start_col: 0,
            end_line: 1,
            end_col: 22,
        },
    )
    .expect("a site")
    .with_texts(Some("java.util.Map"), None);

    let calls = Edge::new(
        add.node_id.clone(),
        remove.node_id.clone(),
        EdgeKind::Calls,
        Band::Typed,
        Some(site.site_id.clone()),
    )
    .with_engine(0.95, "lsp_typed", 1);
    let mut uses = Edge::new(
        add.node_id.clone(),
        class.node_id.clone(),
        EdgeKind::UsesType,
        Band::Exact,
        None,
    );
    uses.props = props(&[("via", json!("parameter"))]);
    let mut observed = Edge::new(
        remove.node_id.clone(),
        add.node_id.clone(),
        EdgeKind::Calls,
        Band::Precise,
        None,
    );
    observed.observed = true;
    observed.weight = 3;
    let changes = Edge::new(
        java_file.node_id.clone(),
        guide_file.node_id.clone(),
        EdgeKind::ChangesWith,
        Band::Exact,
        None,
    );

    let candidate = CandidateSite {
        site_id: import_site.site_id.clone(),
        src: java_file.node_id.clone(),
        callee_name: "Map.put".to_owned(),
        band: CandidateBand::Candidate,
        candidate_ids: vec![add.node_id.clone(), remove.node_id.clone()],
        engine_score: Some(0.5),
        engine_strategy: Some("suffix_match".to_owned()),
        reason: "two definitions survive; the call's \"receiver\" is untyped".to_owned(),
    };

    let contract_key =
        NodeKey::estate(NodeKind::ApiContract, "GET /orders/{id}").expect("an estate kind");
    let contract = Contract {
        contract_id: id(&contract_key),
        kind: ContractKind::ApiContract,
        key: "GET /orders/{}".to_owned(),
        namespace_key: "GET /orders/{id}".to_owned(),
        identity_strength: IdentityStrength::Declared,
        owner_node_id: Some(add.node_id.clone()),
        direction: ContractDirection::Provides,
        props: props(&[("spec", json!("openapi.yaml"))]),
    };
    let consumed = Contract {
        contract_id: id(
            &NodeKey::estate(NodeKind::Channel, "unresolved:ce63551447285fd4:orders")
                .expect("an estate kind"),
        ),
        kind: ContractKind::Channel,
        key: "orders".to_owned(),
        namespace_key: "unresolved:ce63551447285fd4:orders".to_owned(),
        identity_strength: IdentityStrength::Unresolved,
        owner_node_id: None,
        direction: ContractDirection::Consumes,
        props: Props::default(),
    };

    let evidence = Evidence {
        evidence_id: "ev-0001".to_owned(),
        fact_kind: FactKind::Edge,
        fact_id: observed.edge_id.to_string(),
        provider: "scip-java".to_owned(),
        provider_version: "0.10.2".to_owned(),
        authority: Authority::Compiler,
        verdict: Verdict::Supports,
        target_node_id: None,
        file_id: Some(file_id(JAVA)),
        start_byte: Some(410),
        end_byte: Some(420),
        metadata: props(&[("symbol", json!("com/acme/Inventory#add()."))]),
    };
    let occurrence = SemanticOccurrence {
        occ_id: "occ-0001".to_owned(),
        provider: "scip-java".to_owned(),
        provider_symbol: "com/acme/Inventory#remove().".to_owned(),
        target_node_id: Some(remove.node_id.clone()),
        file_id: file_id(JAVA),
        start_byte: 310,
        end_byte: 316,
        start_line: 14,
        start_col: 8,
        role: OccurrenceRole::Reference,
        site_id: Some(site.site_id.clone()),
        enclosing_node_id: Some(add.node_id.clone()),
    };

    let mut java_bands = BandCounts::default();
    java_bands.add(Band::Typed, 1);
    java_bands.add(Band::Candidate, 1);
    let mut markdown_bands = BandCounts::default();
    markdown_bands.add(Band::Unresolved, 0);

    data.metrics = vec![
        Metric {
            node_id: add.node_id.clone(),
            metric: "cyclomatic".to_owned(),
            value: 2.0,
        },
        Metric {
            node_id: add.node_id.clone(),
            metric: "loc".to_owned(),
            value: 7.0,
        },
        Metric {
            node_id: remove.node_id.clone(),
            metric: "importance".to_owned(),
            value: 1.25,
        },
    ];
    data.coverage = vec![
        Coverage {
            language: "java".to_owned(),
            files: 1,
            parsed: 1,
            partial: 0,
            failed: 0,
            skipped: 0,
            symbols: 4,
            call_sites: 2,
            by_band: java_bands,
            observed_links: 0,
        },
        Coverage {
            language: "markdown".to_owned(),
            files: 1,
            parsed: 0,
            partial: 1,
            failed: 0,
            skipped: 0,
            symbols: 1,
            call_sites: 0,
            by_band: markdown_bands,
            observed_links: 0,
        },
    ];
    data.nodes = vec![
        repo_node, java_file, guide_file, class, add, remove, section,
    ];
    data.sites = vec![site, import_site];
    data.edges = vec![calls, uses, observed, changes];
    data.candidates = vec![candidate];
    data.contracts = vec![contract, consumed];
    data.evidence = vec![evidence];
    data.semantic_occurrences = vec![occurrence];
    data
}

fn write(data: &SegmentData, scratch: &Scratch, name: &str) -> WrittenSegment {
    let dir = scratch.dir(name);
    SegmentWriter::new()
        .build_in(&scratch.dir(&format!("{name}-build")))
        .write(data, &dir.join("segment.db"))
        .expect("the segment is written")
}

fn node_named(data: &SegmentData, name: &str) -> Node {
    data.nodes
        .iter()
        .find(|n| n.name == name)
        .cloned()
        .expect("a fixture node")
}

/// A writable copy of a segment and a connection to it, to damage it in one way.
fn damaged_copy(
    segment: &WrittenSegment,
    scratch: &Scratch,
    name: &str,
) -> (PathBuf, rusqlite::Connection) {
    let copy = scratch.dir("damaged").join(name);
    std::fs::copy(&segment.path, &copy).expect("a copy");
    let mut permissions = copy.metadata().expect("metadata").permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    std::fs::set_permissions(&copy, permissions).expect("writable");
    let conn = rusqlite::Connection::open(&copy).expect("a connection");
    (copy, conn)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        })
}

// --- the schema --------------------------------------------------------------------

#[test]
fn schema_sql_is_4_3_verbatim() {
    let spec = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/spec/4.3-segment-file.md"
    ))
    .expect("4.3");
    let ddl = spec.split("## 7. DDL").nth(1).expect("4.3's DDL section");
    let fenced = ddl.split("```sql\n").nth(1).expect("its SQL block");
    let block = &fenced[..=fenced.find("\n```").expect("the block's end")];
    let schema = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/segment/schema.sql"
    ))
    .expect("schema.sql");
    assert_eq!(schema, block, "schema.sql must be 4.3's DDL byte for byte");
    assert_eq!(pdx_core::segment::SCHEMA, block);
}

#[test]
fn segments_hold_exactly_the_schema() {
    let scratch = Scratch::new();
    let segment = write(&fixture(), &scratch, "schema");
    let conn = rusqlite::Connection::open_with_flags(
        &segment.path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("a connection");
    let mut statement = conn.prepare(testing::SCHEMA_OBJECTS).expect("a statement");
    let objects: Vec<(String, String)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("rows")
        .collect::<Result<_, _>>()
        .expect("rows");
    let names = |kind: &str| -> Vec<&str> {
        objects
            .iter()
            .filter(|(t, _)| t == kind)
            .map(|(_, n)| n.as_str())
            .collect()
    };
    assert_eq!(
        names("table"),
        [
            "candidates",
            "contracts",
            "coverage",
            "edges",
            "evidence",
            "files",
            "fts",
            "fts_config",
            "fts_data",
            "fts_docsize",
            "fts_idx",
            "meta",
            "metrics",
            "nodes",
            "semantic_occurrences",
            "sites",
        ]
    );
    assert_eq!(
        names("index"),
        [
            "candidates_src",
            "contracts_key",
            "edges_dst",
            "edges_site",
            "edges_src",
            "evidence_fact",
            "nodes_file",
            "nodes_kind_name",
            "nodes_parent",
            "semocc_file",
            "semocc_target",
            "sites_file",
            "sites_node",
        ]
    );
    assert!(names("trigger").is_empty() && names("view").is_empty());
}

// --- acceptance ------------------------------------------------------------------

#[test]
fn segment_roundtrip() {
    let scratch = Scratch::new();
    let data = fixture();
    let segment = write(&data, &scratch, "roundtrip");
    let reader = SegmentReader::open(&segment.path).expect("the segment opens");

    let mut expected_meta = data.meta.clone();
    expected_meta.precise_sources.sort();
    assert_eq!(reader.meta(), &expected_meta);

    for node in &data.nodes {
        assert_eq!(
            reader.node(&node.node_id).expect("a query").as_ref(),
            Some(node),
            "{}",
            node.name
        );
    }
    let class = node_named(&data, "Inventory");
    let mut methods: Vec<Node> = data
        .nodes
        .iter()
        .filter(|n| n.parent_id.as_ref() == Some(&class.node_id))
        .cloned()
        .collect();
    methods.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    assert_eq!(reader.children(&class.node_id).expect("a query"), methods);

    for file in &data.files {
        assert_eq!(
            reader.file(&file.file_id).expect("a query").as_ref(),
            Some(file)
        );
        assert_eq!(
            reader.file_by_path(&file.path).expect("a query").as_ref(),
            Some(file)
        );
    }
    for site in &data.sites {
        assert_eq!(
            reader.site(&site.site_id).expect("a query").as_ref(),
            Some(site)
        );
    }
    for src in data.nodes.iter().map(|n| &n.node_id) {
        let mut out: Vec<Edge> = data
            .edges
            .iter()
            .filter(|e| &e.src == src)
            .cloned()
            .collect();
        out.sort_by(|a, b| a.edge_id.cmp(&b.edge_id));
        assert_eq!(reader.edges_from(src, &[]).expect("a query"), out);
        let mut into: Vec<Edge> = data
            .edges
            .iter()
            .filter(|e| &e.dst == src)
            .cloned()
            .collect();
        into.sort_by(|a, b| a.edge_id.cmp(&b.edge_id));
        assert_eq!(reader.edges_to(src, &[]).expect("a query"), into);
        let candidates: Vec<CandidateSite> = data
            .candidates
            .iter()
            .filter(|c| &c.src == src)
            .cloned()
            .collect();
        assert_eq!(reader.candidates_from(src).expect("a query"), candidates);
        let mut metrics: Vec<Metric> = data
            .metrics
            .iter()
            .filter(|m| &m.node_id == src)
            .cloned()
            .collect();
        metrics.sort_by(|a, b| a.metric.cmp(&b.metric));
        assert_eq!(reader.metrics(src).expect("a query"), metrics);
    }
    let mut contracts = data.contracts.clone();
    contracts.sort_by(|a, b| a.contract_id.cmp(&b.contract_id));
    assert_eq!(reader.contracts(None, None).expect("a query"), contracts);
    let mut coverage = data.coverage.clone();
    coverage.sort_by(|a, b| a.language.cmp(&b.language));
    assert_eq!(reader.coverage_all().expect("a query"), coverage);
    assert_eq!(
        reader.coverage("java").expect("a query").as_ref(),
        Some(&data.coverage[0])
    );
    let evidence = &data.evidence[0];
    assert_eq!(
        reader
            .evidence_for(FactKind::Edge, &evidence.fact_id)
            .expect("a query"),
        vec![evidence.clone()]
    );
    assert_eq!(
        reader.occurrences_in_file(&file_id(JAVA)).expect("a query"),
        data.semantic_occurrences
    );
}

#[test]
fn absent_rows_are_absent_not_errors() {
    let scratch = Scratch::new();
    let segment = write(&fixture(), &scratch, "absent");
    let reader = SegmentReader::open(&segment.path).expect("the segment opens");
    let absent = id(&method("com.acme.Inventory.missing"));
    assert_eq!(reader.node(&absent).expect("a query"), None);
    assert_eq!(reader.children(&absent).expect("a query"), vec![]);
    assert_eq!(reader.edges_from(&absent, &[]).expect("a query"), vec![]);
    assert_eq!(reader.node_location(&absent).expect("a query"), None);
    assert_eq!(reader.file_by_path("nowhere.java").expect("a query"), None);
    assert_eq!(reader.coverage("cobol").expect("a query"), None);
    assert_eq!(
        reader
            .occurrences_in_file(&file_id(GUIDE))
            .expect("a query"),
        vec![]
    );
}

#[test]
fn segment_is_byte_identical_across_builds() {
    let scratch = Scratch::new();
    let forwards = fixture();
    // The same segment, every table's rows in another order, the candidate set listed
    // the other way round and the precise sources reversed.
    let mut backwards = fixture();
    backwards.files.reverse();
    backwards.nodes.reverse();
    backwards.nodes.rotate_left(3);
    backwards.sites.reverse();
    backwards.edges.reverse();
    backwards.edges.swap(0, 2);
    backwards.evidence.reverse();
    backwards.semantic_occurrences.reverse();
    backwards.candidates[0].candidate_ids.reverse();
    backwards.contracts.reverse();
    backwards.metrics.reverse();
    backwards.coverage.reverse();
    backwards.meta.precise_sources.reverse();
    assert_ne!(forwards, backwards, "the two inputs really differ in order");

    // Built in different temporary directories, written to different final ones.
    let a = SegmentWriter::new()
        .build_in(&scratch.dir("temp one"))
        .write(&forwards, &scratch.dir("first build").join("segment.db"))
        .expect("written");
    let b = SegmentWriter::new()
        .build_in(&scratch.dir("temp-two-ü"))
        .write(
            &backwards,
            &scratch.dir("second'build").join("other-name.db"),
        )
        .expect("written");
    let bytes_a = std::fs::read(&a.path).expect("bytes");
    let bytes_b = std::fs::read(&b.path).expect("bytes");
    assert_eq!(bytes_a.len(), bytes_b.len());
    assert!(bytes_a == bytes_b, "the two builds differ");
    assert_eq!(a.content_sha256, b.content_sha256);
    assert_eq!(a.size_bytes, bytes_a.len() as u64);
    // Built with its full-text index, which is part of the bytes compared.
    let reader = SegmentReader::open(&b.path).expect("opens");
    assert!(!reader.search("Inventory", 10).expect("a search").is_empty());
    eprintln!(
        "both builds: {} bytes, sha256 {}",
        bytes_a.len(),
        a.content_sha256
    );
}

#[test]
fn reader_refuses_wrong_schema_version() {
    let scratch = Scratch::new();
    let segment = write(&fixture(), &scratch, "versions");
    let (copy, conn) = damaged_copy(&segment, &scratch, "v2.db");
    conn.execute(testing::SET_META, ["schema_version", "2"])
        .expect("damaged");
    drop(conn);
    match SegmentReader::open(&copy) {
        Err(SegmentError::WrongSchemaVersion {
            expected: 1,
            found: 2,
        }) => {}
        other => panic!("expected a wrong-schema refusal, got {other:?}"),
    }
    let message = SegmentReader::open(&copy).expect_err("refused").to_string();
    assert!(message.contains('2') && message.contains('1'), "{message}");

    let (copy, conn) = damaged_copy(&segment, &scratch, "missing.db");
    conn.execute(testing::DELETE_META, ["schema_version"])
        .expect("damaged");
    drop(conn);
    assert!(matches!(
        SegmentReader::open(&copy),
        Err(SegmentError::MissingMeta("schema_version"))
    ));

    for (name, value) in [
        ("text.db", "one"),
        ("signed.db", "+1"),
        ("padded.db", "01"),
        ("empty.db", ""),
    ] {
        let (copy, conn) = damaged_copy(&segment, &scratch, name);
        conn.execute(testing::SET_META, ["schema_version", value])
            .expect("damaged");
        drop(conn);
        match SegmentReader::open(&copy) {
            Err(SegmentError::MalformedMeta { key, .. }) => assert_eq!(key, "schema_version"),
            other => panic!("{value:?}: expected malformed meta, got {other:?}"),
        }
    }
}

#[test]
fn fts_finds_qualified_names() {
    let scratch = Scratch::new();
    let data = fixture();
    let segment = write(&data, &scratch, "fts");
    let reader = SegmentReader::open(&segment.path).expect("opens");
    // "acme" is in qualified names only: no name or documentation holds it.
    let hits = reader.search("acme", 10).expect("a search");
    let mut found: Vec<&str> = hits
        .iter()
        .map(|h| h.node.qualified_name.as_str())
        .collect();
    found.sort_unstable();
    assert_eq!(
        found,
        [
            "com.acme.Inventory",
            "com.acme.Inventory.add",
            "com.acme.Inventory.remove"
        ]
    );
    // A column filter on the qualified name finds the one method by its full name.
    let hits = reader
        .search("qualified_name:\"com.acme.Inventory.remove\"", 10)
        .expect("a search");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].node, node_named(&data, "remove"));
}

// --- full-text search ----------------------------------------------------------------

#[test]
fn fts_search_semantics() {
    let scratch = Scratch::new();
    let data = fixture();
    let segment = write(&data, &scratch, "search");
    let reader = SegmentReader::open(&segment.path).expect("opens");
    // By name, by documentation (Unicode included), and nothing.
    assert_eq!(
        reader.search("name:remove", 10).expect("a search")[0]
            .node
            .name,
        "remove"
    );
    assert_eq!(
        reader.search("negative", 10).expect("a search")[0]
            .node
            .name,
        "add"
    );
    assert_eq!(
        reader.search("Überblick", 10).expect("a search")[0]
            .node
            .name,
        "Überblick"
    );
    assert!(
        reader
            .search("nonexistentterm", 10)
            .expect("a search")
            .is_empty()
    );
    // A limit, and an order that does not depend on chance: best score first, then id.
    let all = reader.search("acme", 10).expect("a search");
    assert_eq!(
        reader.search("acme", 2).expect("a search"),
        all[..2].to_vec()
    );
    assert!(
        all.windows(2)
            .all(|w| (w[0].score, &w[0].node.node_id) <= (w[1].score, &w[1].node.node_id))
    );
    assert_eq!(
        reader.search("acme", 10).expect("a search"),
        all,
        "the same search, the same order"
    );
    // Malformed syntax is an error, never an empty answer.
    for bad in ["\"unterminated", "AND", "name:", "(acme"] {
        assert!(
            matches!(
                reader.search(bad, 10),
                Err(SegmentError::InvalidSearch { .. })
            ),
            "{bad:?}"
        );
    }
}

#[test]
fn equal_scores_are_ordered_by_node_id() {
    let scratch = Scratch::new();
    let mut data = SegmentData::new(SegmentMeta::new(
        repo(),
        "https://github.com/tensaicompl/Code-Explorer-2.0",
        COMMIT,
        SegmentProfile::Structural,
        vec![],
    ));
    // Two nodes indexed with exactly the same text score exactly the same.
    let a = Node::new(
        &NodeKey::definition(&repo(), NodeKind::Function, "a.py", "twin", ""),
        "twin",
    )
    .expect("a node");
    let b = Node::new(
        &NodeKey::definition(&repo(), NodeKind::Function, "b.py", "twin", ""),
        "twin",
    )
    .expect("a node");
    data.nodes = vec![b.clone(), a.clone()];
    let segment = write(&data, &scratch, "twins");
    let hits = SegmentReader::open(&segment.path)
        .expect("opens")
        .search("twin", 10)
        .expect("a search");
    assert_eq!(hits.len(), 2);
    assert_eq!(
        hits[0].score.to_bits(),
        hits[1].score.to_bits(),
        "exactly equal scores"
    );
    let mut ids = [a.node_id, b.node_id];
    ids.sort();
    assert_eq!(
        [hits[0].node.node_id.clone(), hits[1].node.node_id.clone()],
        ids
    );
}

// --- queries ---------------------------------------------------------------------

#[test]
fn edge_band_filters() {
    let scratch = Scratch::new();
    let data = fixture();
    let segment = write(&data, &scratch, "bands");
    let reader = SegmentReader::open(&segment.path).expect("opens");
    let add = node_named(&data, "add").node_id;
    let bands = |filter: &[Band]| -> Vec<Band> {
        reader
            .edges_from(&add, filter)
            .expect("a query")
            .iter()
            .map(|e| e.band)
            .collect()
    };
    let mut every = bands(&[]);
    every.sort();
    assert_eq!(every, [Band::Exact, Band::Typed]);
    assert_eq!(bands(&[Band::Typed]), [Band::Typed]);
    let mut both = bands(&[Band::Typed, Band::Exact]);
    both.sort();
    assert_eq!(both, [Band::Exact, Band::Typed]);
    assert!(bands(&[Band::Candidate, Band::Precise]).is_empty());
    // Undrawn bands are not dropped unless asked.
    let remove = node_named(&data, "remove").node_id;
    assert_eq!(
        reader.edges_from(&remove, &[]).expect("a query")[0].band,
        Band::Precise
    );
}

#[test]
fn contracts_filter_by_kind_and_key() {
    let scratch = Scratch::new();
    let segment = write(&fixture(), &scratch, "contracts");
    let reader = SegmentReader::open(&segment.path).expect("opens");
    assert_eq!(reader.contracts(None, None).expect("a query").len(), 2);
    let api = reader
        .contracts(Some(ContractKind::ApiContract), None)
        .expect("a query");
    assert_eq!(api.len(), 1);
    assert_eq!(
        reader
            .contracts(None, Some("GET /orders/{}"))
            .expect("a query"),
        api
    );
    assert_eq!(
        reader
            .contracts(Some(ContractKind::ApiContract), Some("GET /orders/{}"))
            .expect("a query"),
        api
    );
    assert!(
        reader
            .contracts(Some(ContractKind::Channel), Some("GET /orders/{}"))
            .expect("a query")
            .is_empty()
    );
    assert_eq!(
        reader
            .contracts(Some(ContractKind::Channel), Some("orders"))
            .expect("a query")
            .len(),
        1
    );
}

#[test]
fn snippet_spans() {
    let scratch = Scratch::new();
    let data = fixture();
    let segment = write(&data, &scratch, "spans");
    let reader = SegmentReader::open(&segment.path).expect("opens");
    let add = node_named(&data, "add");
    let location = reader
        .node_location(&add.node_id)
        .expect("a query")
        .expect("a location");
    assert_eq!(
        (
            location.path.as_deref(),
            location.start_line,
            location.end_line
        ),
        (Some(JAVA), Some(12), Some(18))
    );
    let repo_node = node_named(&data, "Code-Explorer-2.0");
    let location = reader
        .node_location(&repo_node.node_id)
        .expect("a query")
        .expect("a location");
    assert_eq!(
        (location.path, location.start_line, location.end_line),
        (None, None, None)
    );
    let site = &data.sites[0];
    assert_eq!(
        reader
            .site(&site.site_id)
            .expect("a query")
            .expect("a site")
            .span,
        site.span
    );
}

#[test]
fn candidate_sets_are_stored_sorted() {
    let scratch = Scratch::new();
    let mut data = fixture();
    data.candidates[0].candidate_ids.sort();
    data.candidates[0].candidate_ids.reverse();
    let segment = write(&data, &scratch, "sets");
    let reader = SegmentReader::open(&segment.path).expect("opens");
    let src = data.candidates[0].src.clone();
    let stored = reader
        .candidates_from(&src)
        .expect("a query")
        .remove(0)
        .candidate_ids;
    let mut sorted = data.candidates[0].candidate_ids.clone();
    sorted.sort();
    assert_eq!(stored, sorted);
}

// --- verification ----------------------------------------------------------------

#[test]
fn foreign_key_violations_are_refused() {
    let scratch = Scratch::new();
    let data = fixture();
    let segment = write(&data, &scratch, "references");
    // A copy whose one change is a node pointing at a file that is not there, planted
    // with enforcement off; the original opens, the copy does not.
    SegmentReader::open(&segment.path).expect("the original opens");
    let (copy, conn) = damaged_copy(&segment, &scratch, "orphan.db");
    conn.execute(testing::FOREIGN_KEYS_OFF, [])
        .expect("enforcement off");
    let orphan = file_id("src/main/Gone.java");
    conn.execute(
        testing::SET_NODE_FILE,
        [node_named(&data, "add").node_id.as_str(), orphan.as_str()],
    )
    .expect("damaged");
    drop(conn);
    match SegmentReader::open(&copy) {
        Err(SegmentError::ForeignKeyViolation {
            table,
            parent,
            count,
            ..
        }) => {
            assert_eq!(
                (table.as_str(), parent.as_str(), count),
                ("nodes", "files", 1)
            );
        }
        other => panic!("expected a foreign-key refusal, got {other:?}"),
    }
}

#[test]
fn reader_refuses_malformed_meta() {
    let scratch = Scratch::new();
    let segment = write(&fixture(), &scratch, "meta");
    let cases: [(&str, &str, Option<&str>); 9] = [
        ("profile", "debug", None),
        ("repo_id", "estate", None),
        ("repo_id", "CE63551447285FD4", None),
        ("language_matrix_version", "v1", None),
        ("engine_version", "-1", None),
        ("precise_sources", "{not json", None),
        (
            "precise_sources",
            r#"[{"provider":"b","version":"1","container_digest":"x"},{"provider":"a","version":"1","container_digest":"x"}]"#,
            None,
        ),
        ("commit_sha", "HEAD", None),
        ("built_at", "2026-10-02T00:00:00Z", Some("added")),
    ];
    for (i, (key, value, how)) in cases.iter().enumerate() {
        let (copy, conn) = damaged_copy(&segment, &scratch, &format!("meta{i}.db"));
        let statement = if how.is_some() {
            testing::ADD_META
        } else {
            testing::SET_META
        };
        conn.execute(statement, [key, value]).expect("damaged");
        drop(conn);
        let error = SegmentReader::open(&copy).expect_err("refused");
        let ((SegmentError::UnexpectedMeta(found), Some(_))
        | (SegmentError::MalformedMeta { key: found, .. }, None)) = (&error, how)
        else {
            panic!("{key} = {value:?}: unexpected {error:?}");
        };
        assert_eq!(found, key);
    }
    for key in ["repo_id", "profile", "precise_sources", "pdx_version"] {
        let (copy, conn) = damaged_copy(&segment, &scratch, &format!("without-{key}.db"));
        conn.execute(testing::DELETE_META, [key]).expect("damaged");
        drop(conn);
        assert!(
            matches!(SegmentReader::open(&copy), Err(SegmentError::MissingMeta(k)) if k == key),
            "{key}"
        );
    }
    // A structural segment with precise sources is not a segment the writer makes.
    let (copy, conn) = damaged_copy(&segment, &scratch, "structural.db");
    conn.execute(testing::SET_META, ["profile", "structural"])
        .expect("damaged");
    drop(conn);
    assert!(matches!(
        SegmentReader::open(&copy),
        Err(SegmentError::MalformedMeta { .. })
    ));
}

#[test]
fn stored_values_are_decoded_strictly() {
    let scratch = Scratch::new();
    let data = fixture();
    let segment = write(&data, &scratch, "values");
    let add = node_named(&data, "add").node_id;

    let (copy, conn) = damaged_copy(&segment, &scratch, "kind.db");
    conn.execute(testing::SET_NODE_KIND, [add.as_str(), "method"])
        .expect("damaged");
    drop(conn);
    let reader = SegmentReader::open(&copy).expect("opens: the meta and references are intact");
    assert!(matches!(
        reader.node(&add),
        Err(SegmentError::InvalidStoredVocabulary { column: "kind", .. })
    ));

    // A drawn band in the candidates table: the schema's CHECK forbids it, so it is
    // planted with checks off, and the reader still refuses it rather than coerce it.
    for drawn in ["typed", "precise", "exact"] {
        let (copy, conn) = damaged_copy(&segment, &scratch, &format!("band-{drawn}.db"));
        conn.execute(testing::CHECKS_OFF, []).expect("checks off");
        conn.execute(
            testing::SET_CANDIDATE_BAND,
            [data.candidates[0].site_id.as_str(), drawn],
        )
        .expect("damaged");
        drop(conn);
        let reader = SegmentReader::open(&copy).expect("opens");
        let src = &data.candidates[0].src;
        assert!(
            matches!(
                reader.candidates_from(src),
                Err(SegmentError::InvalidStoredVocabulary { column: "band", .. })
            ),
            "{drawn}"
        );
    }

    // Incomplete band counts are an error, not zeros.
    let (copy, conn) = damaged_copy(&segment, &scratch, "bands.db");
    conn.execute(testing::SET_COVERAGE_BANDS, ["java", r#"{"typed":1}"#])
        .expect("damaged");
    drop(conn);
    let reader = SegmentReader::open(&copy).expect("opens");
    assert!(matches!(
        reader.coverage("java"),
        Err(SegmentError::InvalidStoredJson {
            column: "by_band",
            ..
        })
    ));
}

// --- the file --------------------------------------------------------------------

#[test]
fn finished_segments_are_read_only() {
    let scratch = Scratch::new();
    let segment = write(&fixture(), &scratch, "readonly");
    let metadata = segment.path.metadata().expect("metadata");
    assert!(metadata.permissions().readonly());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(metadata.permissions().mode() & 0o777, 0o444);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_READONLY: u32 = 0x1;
        assert_ne!(metadata.file_attributes() & FILE_ATTRIBUTE_READONLY, 0);
    }
    // A segment is never overwritten, even by the same rows.
    let again = SegmentWriter::new().write(&fixture(), &segment.path);
    assert!(matches!(again, Err(SegmentError::DestinationExists(_))));
    assert!(matches!(
        SegmentWriter::new().write(
            &fixture(),
            &scratch.path().join("no such dir").join("segment.db")
        ),
        Err(SegmentError::InvalidDestination { .. })
    ));
}

#[test]
fn reading_never_changes_the_segment() {
    let scratch = Scratch::new();
    let data = fixture();
    let segment = write(&data, &scratch, "immutable");
    let before = std::fs::read(&segment.path).expect("bytes");
    let listing = |dir: &Path| -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("a listing")
            .map(|e| {
                e.expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    };
    let dir = segment.path.parent().expect("a directory").to_path_buf();
    {
        let reader = SegmentReader::open(&segment.path).expect("opens");
        for node in &data.nodes {
            reader.node(&node.node_id).expect("a query");
            reader.children(&node.node_id).expect("a query");
            reader
                .edges_from(&node.node_id, &[Band::Typed])
                .expect("a query");
            reader.edges_to(&node.node_id, &[]).expect("a query");
            reader.candidates_from(&node.node_id).expect("a query");
            reader.metrics(&node.node_id).expect("a query");
            reader.node_location(&node.node_id).expect("a query");
        }
        reader.contracts(None, None).expect("a query");
        reader.coverage_all().expect("a query");
        reader.search("acme OR Überblick", 10).expect("a search");
        assert_eq!(
            listing(&dir),
            ["segment.db"],
            "no journal, WAL or shared-memory file while open"
        );
    }
    assert_eq!(listing(&dir), ["segment.db"]);
    assert!(
        std::fs::read(&segment.path).expect("bytes") == before,
        "the reader changed the file"
    );
    assert_eq!(sha256_hex(&before), segment.content_sha256);
    let uri = SegmentReader::uri(&segment.path).expect("a URI");
    assert!(
        uri.starts_with("file://") && uri.ends_with("?mode=ro&immutable=1"),
        "{uri}"
    );
}

#[test]
fn content_sha256_is_the_hash_of_the_file() {
    let scratch = Scratch::new();
    let segment = write(&fixture(), &scratch, "hash");
    let bytes = std::fs::read(&segment.path).expect("bytes");
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let independent = hasher.finalize().iter().fold(String::new(), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    });
    assert_eq!(segment.content_sha256, independent);
    assert_eq!(segment.content_sha256.len(), 64);
    assert!(
        segment
            .content_sha256
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    );
    // SQLite's own file header: this is the finished database, not a dump.
    assert!(bytes.starts_with(b"SQLite format 3\0"));
}

#[test]
fn hostile_text_stays_data() {
    let scratch = Scratch::new();
    let mut data = fixture();
    data.contracts[0].key = "GET /it's \"quoted\"; DROP TABLE nodes; --".to_owned();
    data.nodes[0].name = "Robert'); DROP TABLE nodes;--".to_owned();
    // A destination whose path a naive statement would break on.
    let dir = scratch.dir("it's a dir — ü");
    let segment = SegmentWriter::new()
        .write(&data, &dir.join("seg'ment.db"))
        .expect("written");
    let reader = SegmentReader::open(&segment.path).expect("opens");
    let key = data.contracts[0].key.clone();
    assert_eq!(
        reader.contracts(None, Some(&key)).expect("a query").len(),
        1
    );
    assert_eq!(
        reader
            .node(&data.nodes[0].node_id)
            .expect("a query")
            .expect("a node")
            .name,
        data.nodes[0].name
    );
    for query in ["\"DROP TABLE nodes\"", "\"it's\"", "\"'); DROP\""] {
        reader
            .search(query, 10)
            .expect("a search: quoted text is a phrase, never SQL");
    }
    assert!(matches!(
        reader.search("x'); DROP TABLE nodes; --", 10),
        Err(SegmentError::InvalidSearch { .. })
    ));
    // Still all there.
    assert_eq!(reader.contracts(None, None).expect("a query").len(), 2);
}

#[test]
fn rows_that_cannot_be_stored_are_refused() {
    let scratch = Scratch::new();
    let attempt = |data: &SegmentData, name: &str| {
        SegmentWriter::new()
            .build_in(&scratch.dir("refused-build"))
            .write(data, &scratch.dir("refused").join(name))
    };

    let mut duplicate = fixture();
    duplicate.nodes.push(duplicate.nodes[3].clone());
    assert!(matches!(
        attempt(&duplicate, "dup.db"),
        Err(SegmentError::DuplicateRow { table: "nodes", .. })
    ));

    let mut twice = fixture();
    let first = twice.candidates[0].candidate_ids[0].clone();
    twice.candidates[0].candidate_ids.push(first);
    assert!(matches!(
        attempt(&twice, "twice.db"),
        Err(SegmentError::InvalidRow {
            table: "candidates",
            ..
        })
    ));

    let mut nan = fixture();
    nan.metrics[0].value = f64::NAN;
    assert!(matches!(
        attempt(&nan, "nan.db"),
        Err(SegmentError::InvalidRow {
            table: "metrics",
            ..
        })
    ));

    let mut infinite = fixture();
    infinite.edges[0].engine_score = Some(f64::INFINITY);
    assert!(matches!(
        attempt(&infinite, "inf.db"),
        Err(SegmentError::InvalidRow { table: "edges", .. })
    ));

    let mut structural = fixture();
    structural.meta.profile = SegmentProfile::Structural;
    assert!(matches!(
        attempt(&structural, "structural.db"),
        Err(SegmentError::InvalidRow { table: "meta", .. })
    ));

    let mut other_version = fixture();
    other_version.meta.schema_version = 2;
    assert!(matches!(
        attempt(&other_version, "v2.db"),
        Err(SegmentError::InvalidRow { table: "meta", .. })
    ));

    let mut wrong_file = fixture();
    wrong_file.files[0].file_id = file_id("elsewhere.java");
    assert!(matches!(
        attempt(&wrong_file, "file.db"),
        Err(SegmentError::InvalidRow { table: "files", .. })
    ));

    // A reference to a file the segment does not have fails as it is inserted.
    let mut orphan = fixture();
    orphan.nodes[3].file_id = Some(file_id("src/main/Gone.java"));
    assert!(matches!(
        attempt(&orphan, "orphan.db"),
        Err(SegmentError::Sqlite(_))
    ));

    // Nothing was left behind by any refusal.
    let left: Vec<_> = std::fs::read_dir(scratch.dir("refused"))
        .expect("a listing")
        .collect();
    assert!(left.is_empty(), "{left:?}");
}
