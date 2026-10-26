//! Data documents in the graph (P2-18, decision 85, issue 131): a JSON, YAML or TOML file
//! gives one `Variable` node per distinct key path, to `DATA_KEY_MAX_DEPTH` keys, each the
//! child of the key that holds it, with how often the path occurs in
//! `props.occurrences`; data keys get no metrics. Built end to end and read back from
//! the segment.

use std::collections::BTreeMap;
use std::fs;

use pdx_core::consts::DATA_KEY_MAX_DEPTH;
use pdx_core::ids::RepoId;
use pdx_core::index::extract::ExtractLimits;
use pdx_core::index::{IndexRequest, SegmentReport, SegmentStatus, build_segment};
use pdx_core::kinds::NodeKind;
use pdx_core::model::Node;
use pdx_core::segment::{RepoIdSource, SegmentReader};

const FILES: &[(&str, &str)] = &[
    (
        "config/catalog.json",
        r#"{
  "name": "shop",
  "items": [
    {"id": 1, "tags": ["a"]},
    {"id": 2, "price": {"amount": 3}},
    {"id": 3}
  ]
}
"#,
    ),
    (
        "deploy/deep.yaml",
        "a:\n  b:\n    c:\n      d:\n        e:\n          f: 1\n",
    ),
    (
        "app/main.py",
        "def run(items):\n    for i in items:\n        if i:\n            print(i)\n",
    ),
];

struct Built {
    _dir: tempfile::TempDir,
    _out: tempfile::TempDir,
    report: SegmentReport,
}

fn build() -> Built {
    let dir = tempfile::tempdir().expect("a directory");
    for (path, content) in FILES {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
        fs::write(&path, content).expect("a file");
    }
    let out = tempfile::tempdir().expect("an output directory");
    let report = build_segment(IndexRequest {
        root: dir.path(),
        repo_id: RepoId::parse("ce63551447285fd4").expect("a repo id"),
        repo_id_source: RepoIdSource::Registered,
        repo_url: "https://git.example/acme/data.git".to_owned(),
        repo_name: "data".to_owned(),
        commit_sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        limits: ExtractLimits {
            requested_workers: 2,
            memory_budget_bytes: 64 << 20,
        },
        cache: None,
        backend: None,
        destination: out.path().join("segment.db"),
        build_dir: None,
        progress: None,
    })
    .expect("the build succeeds");
    assert_eq!(
        report.status,
        SegmentStatus::Ready,
        "{:#?}",
        report.degraded
    );
    Built {
        _dir: dir,
        _out: out,
        report,
    }
}

/// The data keys of one file, by their path below the file's module.
fn keys<'n>(nodes: &'n [Node], file: &Node) -> BTreeMap<String, &'n Node> {
    nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Variable && n.file_id == Some(file.node_id))
        .map(|n| {
            let below = n
                .qualified_name
                .rsplit_once(&format!("{}.", stem(file)))
                .map_or(n.qualified_name.as_str(), |(_, rest)| rest);
            (below.to_owned(), n)
        })
        .collect()
}

fn stem(file: &Node) -> String {
    let name = file.name.rsplit('/').next().unwrap_or(&file.name);
    name.split('.').next().unwrap_or(name).to_owned()
}

fn file<'n>(nodes: &'n [Node], path: &str) -> &'n Node {
    nodes
        .iter()
        .find(|n| n.kind == NodeKind::File && n.qualified_name == path)
        .unwrap_or_else(|| panic!("no file node {path}"))
}

fn occurrences(node: &Node) -> u64 {
    node.props
        .get("occurrences")
        .and_then(serde_json::Value::as_u64)
        .expect("a data key counts its occurrences")
}

#[test]
fn data_keys_one_node_per_key_path() {
    let built = build();
    let reader = SegmentReader::open(&built.report.segment.path).expect("the segment opens");
    let nodes = reader.nodes_all().expect("nodes");
    let catalog = file(&nodes, "config/catalog.json");
    let keys = keys(&nodes, catalog);
    assert_eq!(
        keys.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "items",
            "items[].id",
            "items[].price",
            "items[].price.amount",
            "items[].tags",
            "name",
        ]
    );
    let counts: Vec<(&str, u64)> = keys
        .iter()
        .map(|(path, n)| (path.as_str(), occurrences(n)))
        .collect();
    assert_eq!(
        counts,
        [
            ("items", 1),
            ("items[].id", 3),
            ("items[].price", 1),
            ("items[].price.amount", 1),
            ("items[].tags", 1),
            ("name", 1),
        ]
    );
    // The keys form a tree: each is the child of the key that holds it.
    let id = |path: &str| keys[path].node_id;
    assert_eq!(keys["items[].id"].parent_id, Some(id("items")));
    assert_eq!(
        keys["items[].price.amount"].parent_id,
        Some(id("items[].price"))
    );
    assert_eq!(keys["name"].parent_id, Some(catalog.node_id));
    assert_eq!(keys["items[].id"].name, "id");
}

#[test]
fn data_keys_depth_bounded() {
    let built = build();
    let reader = SegmentReader::open(&built.report.segment.path).expect("the segment opens");
    let nodes = reader.nodes_all().expect("nodes");
    let deep = keys(&nodes, file(&nodes, "deploy/deep.yaml"));
    assert_eq!(DATA_KEY_MAX_DEPTH, 4);
    assert_eq!(
        DATA_KEY_MAX_DEPTH,
        pdx_engine::DATA_KEY_MAX_DEPTH,
        "the engine's"
    );
    assert_eq!(
        deep.keys().map(String::as_str).collect::<Vec<_>>(),
        ["a", "a.b", "a.b.c", "a.b.c.d"]
    );
}

#[test]
fn data_keys_have_no_metrics() {
    let built = build();
    let reader = SegmentReader::open(&built.report.segment.path).expect("the segment opens");
    let nodes = reader.nodes_all().expect("nodes");
    for path in ["config/catalog.json", "deploy/deep.yaml"] {
        for (key, node) in keys(&nodes, file(&nodes, path)) {
            assert_eq!(
                reader.metrics(&node.node_id).expect("metrics"),
                [],
                "{path}: {key} has metrics"
            );
        }
    }
    // A symbol of code still has its own.
    let run = nodes
        .iter()
        .find(|n| n.name == "run")
        .expect("the function's node");
    assert!(!reader.metrics(&run.node_id).expect("metrics").is_empty());
}
