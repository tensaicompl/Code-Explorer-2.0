//! The graph model's vocabularies (4.2.3, 4.2.4) and rows (4.3), and the exact
//! serialised form of each: these strings are what segments, caches and the API store,
//! so every one is written out here by hand.

use pdx_core::bands::{Band, CandidateBand};
use pdx_core::ids::{self, NodeId, NodeKey, RepoId, SiteKey};
use pdx_core::kinds::{
    ContractKind, EdgeCategory, EdgeKind, LayerRole, NodeCategory, NodeKind, SiteKind,
};
use pdx_core::model::{
    BandCounts, CandidateSite, Contract, ContractDirection, Coverage, Edge, IdentityStrength,
    Metric, Node, Props, Site, Span,
};
use serde_json::json;

fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("serialisable")
}

fn names<T: serde::Serialize>(all: &[T]) -> Vec<String> {
    all.iter()
        .map(|v| json(v).trim_matches('"').to_owned())
        .collect()
}

// --- vocabularies ----------------------------------------------------------------

#[test]
fn node_kinds_are_4_2_3() {
    let structural = [
        "Repo",
        "Folder",
        "File",
        "Module",
        "Class",
        "Interface",
        "Enum",
        "Struct",
        "Trait",
        "TypeAlias",
        "Function",
        "Method",
        "Constructor",
        "Field",
        "Variable",
        "Macro",
        "Route",
        "Test",
        "Resource",
        "ConfigKey",
        "Doc",
    ];
    let contract = [
        "ApiContract",
        "RpcMethod",
        "Channel",
        "Table",
        "Column",
        "Artifact",
        "ArtifactVersion",
    ];
    let architecture = [
        "System",
        "BoundedContext",
        "Service",
        "Library",
        "Database",
        "Broker",
        "Frontend",
        "BatchJob",
        "Deployment",
        "LayerRole",
    ];
    let all: Vec<&str> = structural
        .iter()
        .chain(&contract)
        .chain(&architecture)
        .copied()
        .collect();
    assert_eq!(names(NodeKind::ALL), all);
    assert_eq!(NodeKind::ALL.len(), 38);
    for kind in NodeKind::ALL {
        let name = kind.as_str();
        let category = if structural.contains(&name) {
            NodeCategory::Structural
        } else if contract.contains(&name) {
            NodeCategory::Contract
        } else {
            NodeCategory::Architecture
        };
        assert_eq!(kind.category(), category, "{name}");
        assert_eq!(
            kind.is_estate_level(),
            category != NodeCategory::Structural,
            "{name}"
        );
        assert_eq!(NodeKind::parse(name), Some(*kind));
        assert_eq!(
            serde_json::from_str::<NodeKind>(&format!("\"{name}\"")).expect("json"),
            *kind
        );
    }
    for wrong in [
        "repo",
        "REPO",
        "type_alias",
        "Typealias",
        "ArchitectureRule",
        "",
    ] {
        assert_eq!(NodeKind::parse(wrong), None, "{wrong}");
    }
}

#[test]
fn edge_kinds_are_4_2_4() {
    let expected: [(&str, EdgeCategory); 37] = [
        ("CONTAINS", EdgeCategory::Containment),
        ("CALLS", EdgeCategory::Structural),
        ("CALL_REFERENCE", EdgeCategory::Structural),
        ("USAGE", EdgeCategory::Structural),
        ("IMPORTS", EdgeCategory::Structural),
        ("INHERITS", EdgeCategory::Structural),
        ("IMPLEMENTS", EdgeCategory::Structural),
        ("OVERRIDES", EdgeCategory::Structural),
        ("USES_TYPE", EdgeCategory::Structural),
        ("READS_FIELD", EdgeCategory::Structural),
        ("WRITES_FIELD", EdgeCategory::Structural),
        ("THROWS", EdgeCategory::Structural),
        ("TESTS", EdgeCategory::Structural),
        ("DEFINES_ROUTE", EdgeCategory::Structural),
        ("READS_CONFIG", EdgeCategory::Structural),
        ("EXPOSES", EdgeCategory::Contract),
        ("CONSUMES", EdgeCategory::Contract),
        ("PUBLISHES", EdgeCategory::Contract),
        ("SUBSCRIBES", EdgeCategory::Contract),
        ("READS_TABLE", EdgeCategory::Contract),
        ("WRITES_TABLE", EdgeCategory::Contract),
        ("DEFINES_TABLE", EdgeCategory::Contract),
        ("PRODUCES_ARTIFACT", EdgeCategory::Contract),
        ("LINKS_ARTIFACT", EdgeCategory::Contract),
        ("HTTP_CALLS", EdgeCategory::DerivedCrossRepo),
        ("RPC_CALLS", EdgeCategory::DerivedCrossRepo),
        ("MESSAGES", EdgeCategory::DerivedCrossRepo),
        ("SHARES_TABLE", EdgeCategory::DerivedCrossRepo),
        ("DEPENDS_ON_ARTIFACT", EdgeCategory::DerivedCrossRepo),
        ("MEMBER_OF", EdgeCategory::Architecture),
        ("DEPLOYED_AS", EdgeCategory::Architecture),
        ("HAS_ROLE", EdgeCategory::Architecture),
        ("LAYER_DEPENDS", EdgeCategory::Architecture),
        ("UPSTREAM_OF", EdgeCategory::Architecture),
        ("VIOLATES", EdgeCategory::Architecture),
        ("CHANGES_WITH", EdgeCategory::History),
        ("SIMILAR_TO", EdgeCategory::History),
    ];
    let spellings: Vec<&str> = expected.iter().map(|(name, _)| *name).collect();
    assert_eq!(names(EdgeKind::ALL), spellings);
    for (kind, (name, category)) in EdgeKind::ALL.iter().zip(expected) {
        assert_eq!(kind.category(), category, "{name}");
        assert_eq!(EdgeKind::parse(name), Some(*kind));
    }
    // 4.2.2: the kinds that carry a band.
    let banded: Vec<&str> = EdgeKind::ALL
        .iter()
        .filter(|k| k.requires_band())
        .map(|k| k.as_str())
        .collect();
    assert_eq!(
        banded,
        [
            "CALLS",
            "IMPORTS",
            "INHERITS",
            "IMPLEMENTS",
            "OVERRIDES",
            "USES_TYPE",
            "PUBLISHES",
            "SUBSCRIBES",
            "READS_TABLE",
            "WRITES_TABLE",
            "LINKS_ARTIFACT",
            "HTTP_CALLS",
            "RPC_CALLS",
        ]
    );
    for wrong in ["calls", "Calls", "CALL", "CALLREFERENCE", ""] {
        assert_eq!(EdgeKind::parse(wrong), None, "{wrong}");
    }
}

#[test]
fn layer_roles_site_kinds_and_contract_vocabularies() {
    assert_eq!(
        names(LayerRole::ALL),
        [
            "api",
            "service",
            "domain",
            "persistence",
            "adapter",
            "port",
            "infra",
            "test",
            "unknown"
        ]
    );
    assert_eq!(
        names(SiteKind::ALL),
        [
            "call",
            "reference",
            "import",
            "type_ref",
            "field_rw",
            "route",
            "contract"
        ]
    );
    assert_eq!(
        names(IdentityStrength::ALL),
        ["exact", "declared", "unresolved"]
    );
    assert_eq!(
        names(ContractDirection::ALL),
        ["provides", "consumes", "both"]
    );
    // The contract kinds are exactly the contract node kinds, spelled the same.
    assert_eq!(
        names(ContractKind::ALL),
        [
            "ApiContract",
            "RpcMethod",
            "Channel",
            "Table",
            "Column",
            "Artifact",
            "ArtifactVersion"
        ]
    );
    let contract_nodes: Vec<NodeKind> = NodeKind::ALL
        .iter()
        .copied()
        .filter(|k| k.category() == NodeCategory::Contract)
        .collect();
    assert_eq!(
        ContractKind::ALL
            .iter()
            .map(|k| k.node_kind())
            .collect::<Vec<_>>(),
        contract_nodes
    );
    for kind in NodeKind::ALL {
        assert_eq!(
            ContractKind::from_node_kind(*kind).map(ContractKind::node_kind),
            (kind.category() == NodeCategory::Contract).then_some(*kind)
        );
    }
    for wrong in ["API", "Api", "providing", "Exact"] {
        assert_eq!(LayerRole::parse(wrong), None);
        assert_eq!(ContractDirection::parse(wrong), None);
        assert_eq!(IdentityStrength::parse(wrong), None);
    }
}

// --- rows ------------------------------------------------------------------------

fn repo() -> RepoId {
    RepoId::parse("ce63551447285fd4").expect("a registered form")
}

fn method() -> NodeId {
    NodeKey::definition(
        &repo(),
        NodeKind::Method,
        "src/main/Inventory.java",
        "com.acme.Inventory.add",
        "",
    )
    .node_id()
    .expect("an id")
}

fn site_key() -> SiteKey {
    let fingerprint = ids::ast_fingerprint(
        &[
            "method_declaration",
            "block",
            "expression_statement",
            "method_invocation",
        ],
        "add",
        "items",
        1,
    )
    .expect("a fingerprint");
    SiteKey::new(
        "src/main/Inventory.java",
        Some(method()),
        SiteKind::Call,
        fingerprint,
    )
}

#[test]
fn node_serialised_form() {
    let file = NodeKey::file(&repo(), "src/main/Inventory.java")
        .node_id()
        .expect("an id");
    let mut node = Node::new(
        &NodeKey::definition(
            &repo(),
            NodeKind::Method,
            "src/main/Inventory.java",
            "com.acme.Inventory.add",
            "",
        ),
        "add",
    )
    .expect("a node");
    node.file_id = Some(file.clone());
    node.parent_id = Some(file);
    node.start_line = Some(12);
    node.end_line = Some(18);
    node.layer_role = Some(LayerRole::Domain);
    node.signature = Some("(String,int)".to_owned());
    node.props.insert("visibility", json!("public"));
    node.props.insert("is_entry_point", json!(false));
    assert_eq!(
        json(&node),
        r#"{"node_id":"FC0C2RXS73G18HY3HRG3C0EX2Z","kind":"Method","name":"add","qualified_name":"com.acme.Inventory.add","file_id":"2M70XN97JJTJTTYPJ2PTZ4GJHP","parent_id":"2M70XN97JJTJTTYPJ2PTZ4GJHP","start_line":12,"end_line":18,"layer_role":"domain","signature":"(String,int)","doc":null,"props":{"is_entry_point":false,"visibility":"public"}}"#
    );
    assert_eq!(
        serde_json::from_str::<Node>(&json(&node)).expect("json"),
        node
    );

    // A node with nothing optional: the repository's own.
    let repo_node = Node::new(&NodeKey::repo(&repo()), "Code-Explorer-2.0").expect("a node");
    assert_eq!(
        json(&repo_node),
        r#"{"node_id":"NP5899JPZZTZSSAGDXKHNYR17C","kind":"Repo","name":"Code-Explorer-2.0","qualified_name":"","file_id":null,"parent_id":null,"start_line":null,"end_line":null,"layer_role":null,"signature":null,"doc":null,"props":{}}"#
    );
}

#[test]
fn site_serialised_form() {
    let file = NodeKey::file(&repo(), "src/main/Inventory.java")
        .node_id()
        .expect("an id");
    let site = Site::new(
        &site_key(),
        file,
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
    .with_texts(Some("add"), Some("items"));
    assert_eq!(
        json(&site),
        r#"{"site_id":"8Q0S4A5X7HM4PN0YV3FCEAFV2V","file_id":"2M70XN97JJTJTTYPJ2PTZ4GJHP","enclosing_node_id":"FC0C2RXS73G18HY3HRG3C0EX2Z","site_kind":"call","ast_fingerprint":"b72a1edb5ebc15504aff07917b0760bc15da4cdc57e725700662a10a2d9e755f","start_byte":310,"end_byte":325,"start_line":14,"start_col":8,"end_line":14,"end_col":23,"callee_text":"add","receiver_text":"items"}"#
    );
    assert_eq!(
        serde_json::from_str::<Site>(&json(&site)).expect("json"),
        site
    );
}

#[test]
fn edge_serialised_form() {
    let src = method();
    let dst = NodeKey::definition(
        &repo(),
        NodeKind::Method,
        "src/main/Store.java",
        "com.acme.Store.save",
        "",
    )
    .node_id()
    .expect("an id");
    let site = site_key().site_id().expect("an id");
    let edge = Edge::new(src, dst, EdgeKind::Calls, Band::Typed, Some(site)).with_engine(
        0.95,
        "lsp_typed",
        1,
    );
    assert_eq!(
        json(&edge),
        r#"{"edge_id":"04K6NBGNS5YV1FH28A5J5KK36A","src":"FC0C2RXS73G18HY3HRG3C0EX2Z","dst":"639NYMS5S3YSVEMV5N1G4T7HEQ","kind":"CALLS","band":"typed","observed":false,"engine_score":0.95,"engine_strategy":"lsp_typed","engine_candidates":1,"site_id":"8Q0S4A5X7HM4PN0YV3FCEAFV2V","weight":1,"props":{}}"#
    );
    assert_eq!(
        serde_json::from_str::<Edge>(&json(&edge)).expect("json"),
        edge
    );
    // The engine's strategy is kept verbatim, whatever it is, and never sets the band.
    let hinted = Edge::new(method(), method(), EdgeKind::Calls, Band::Exact, None).with_engine(
        0.4,
        "unique_name",
        3,
    );
    assert_eq!(
        (hinted.band, hinted.engine_strategy.as_deref()),
        (Band::Exact, Some("unique_name"))
    );
    // Observation is orthogonal: it changes neither the band nor the identity.
    let mut observed = hinted.clone();
    observed.observed = true;
    assert_eq!(
        (observed.band, &observed.edge_id),
        (hinted.band, &hinted.edge_id)
    );
}

#[test]
fn candidate_site_serialised_form() {
    let other = NodeKey::definition(
        &repo(),
        NodeKind::Method,
        "src/main/Store.java",
        "com.acme.Store.save",
        "",
    )
    .node_id()
    .expect("an id");
    let candidate = CandidateSite {
        site_id: site_key().site_id().expect("an id"),
        src: method(),
        callee_name: "save".to_owned(),
        band: CandidateBand::Candidate,
        candidate_ids: vec![other, method()],
        engine_score: Some(0.5),
        engine_strategy: Some("suffix_match".to_owned()),
        engine_candidates: Some(2),
        reason: "two definitions survive the scoped stage".to_owned(),
    };
    assert_eq!(
        json(&candidate),
        r#"{"site_id":"8Q0S4A5X7HM4PN0YV3FCEAFV2V","src":"FC0C2RXS73G18HY3HRG3C0EX2Z","callee_name":"save","band":"candidate","candidate_ids":["639NYMS5S3YSVEMV5N1G4T7HEQ","FC0C2RXS73G18HY3HRG3C0EX2Z"],"engine_score":0.5,"engine_strategy":"suffix_match","engine_candidates":2,"reason":"two definitions survive the scoped stage"}"#
    );
    assert_eq!(
        serde_json::from_str::<CandidateSite>(&json(&candidate)).expect("json"),
        candidate
    );
    // A drawn band can never be stored as a candidate row.
    for drawn in ["typed", "precise", "exact"] {
        let row =
            json(&candidate).replace(r#""band":"candidate""#, &format!(r#""band":"{drawn}""#));
        assert!(
            serde_json::from_str::<CandidateSite>(&row).is_err(),
            "{drawn}"
        );
    }
}

#[test]
fn contract_metric_and_coverage_serialised_forms() {
    let key = NodeKey::estate(NodeKind::ApiContract, "GET /orders/{id}").expect("an estate kind");
    let contract = Contract {
        contract_id: key.node_id().expect("an id"),
        kind: ContractKind::ApiContract,
        key: "GET /orders/{}".to_owned(),
        namespace_key: "GET /orders/{id}".to_owned(),
        identity_strength: IdentityStrength::Declared,
        owner_node_id: Some(method()),
        direction: ContractDirection::Provides,
        props: Props::default(),
    };
    assert_eq!(
        json(&contract),
        r#"{"contract_id":"FMXRAER9WFVM1JB93F13E9RTFT","kind":"ApiContract","key":"GET /orders/{}","namespace_key":"GET /orders/{id}","identity_strength":"declared","owner_node_id":"FC0C2RXS73G18HY3HRG3C0EX2Z","direction":"provides","props":{}}"#
    );
    assert_eq!(
        serde_json::from_str::<Contract>(&json(&contract)).expect("json"),
        contract
    );

    let metric = Metric {
        node_id: method(),
        metric: "cyclomatic".to_owned(),
        value: 4.0,
    };
    assert_eq!(
        json(&metric),
        r#"{"node_id":"FC0C2RXS73G18HY3HRG3C0EX2Z","metric":"cyclomatic","value":4.0}"#
    );
    // Every 4.12.1 metric is a name the row takes, with no change to the model.
    for name in [
        "loc",
        "cyclomatic",
        "cognitive",
        "loop_depth",
        "transitive_loop_depth",
        "fan_in",
        "fan_out",
        "importance",
    ] {
        let m = Metric {
            node_id: method(),
            metric: name.to_owned(),
            value: 1.0,
        };
        assert_eq!(serde_json::from_str::<Metric>(&json(&m)).expect("json"), m);
    }

    let mut by_band = BandCounts::default();
    by_band.add(Band::Typed, 40);
    by_band.add(Band::Candidate, 3);
    by_band.add(Band::Typed, 2);
    by_band.add(Band::Unresolved, 1);
    let coverage = Coverage {
        language: "java".to_owned(),
        files: 10,
        parsed: 9,
        partial: 1,
        failed: 0,
        skipped: 0,
        symbols: 120,
        call_sites: 46,
        by_band,
        observed_links: 0,
    };
    assert_eq!(
        json(&coverage),
        r#"{"language":"java","files":10,"parsed":9,"partial":1,"failed":0,"skipped":0,"symbols":120,"call_sites":46,"by_band":{"precise":0,"typed":42,"import-guided":0,"inheritance-guided":0,"exact":0,"scoped":0,"candidate":3,"external":0,"blocked":0,"unresolved":1,"contradicted":0},"observed_links":0}"#
    );
    assert_eq!(
        serde_json::from_str::<Coverage>(&json(&coverage)).expect("json"),
        coverage
    );
    assert_eq!(coverage.by_band.get(Band::Typed), 42);
    assert_eq!(coverage.by_band.total(), 46);
}

#[test]
fn band_counts_are_complete_and_ordered() {
    // Whatever order counts arrive in, the serialised form is the same: all eleven
    // bands, in 4.2.2's order.
    let mut forwards = BandCounts::default();
    let mut backwards = BandCounts::default();
    for (i, band) in Band::ALL.iter().enumerate() {
        forwards.add(*band, i as u64);
    }
    for (i, band) in Band::ALL.iter().enumerate().rev() {
        backwards.add(*band, i as u64);
    }
    assert_eq!(json(&forwards), json(&backwards));
    assert_eq!(
        json(&forwards),
        r#"{"precise":0,"typed":1,"import-guided":2,"inheritance-guided":3,"exact":4,"scoped":5,"candidate":6,"external":7,"blocked":8,"unresolved":9,"contradicted":10}"#
    );
    // A count is required for every band, and only for bands.
    assert!(serde_json::from_str::<BandCounts>(r#"{"precise":0}"#).is_err());
    let with_extra = json(&forwards).replace(r#"{"precise""#, r#"{"observed":1,"precise""#);
    assert!(serde_json::from_str::<BandCounts>(&with_extra).is_err());
}

#[test]
fn props_are_ordered_by_key() {
    // Props are a sorted map, nested objects included, so the same props always
    // serialise to the same bytes whatever order they were set in.
    let mut a = Props::default();
    a.insert("zeta", json!({"b": 1, "a": [3, 2, 1]}));
    a.insert("alpha", json!(true));
    let mut b = Props::default();
    b.insert("alpha", json!(true));
    b.insert("zeta", json!({"a": [3, 2, 1], "b": 1}));
    assert_eq!(json(&a), json(&b));
    assert_eq!(json(&a), r#"{"alpha":true,"zeta":{"a":[3,2,1],"b":1}}"#);
    assert_eq!(json(&Props::default()), "{}");
}
