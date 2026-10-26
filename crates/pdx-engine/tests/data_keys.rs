//! Data documents' key paths (P2-18, decision 85, issue 131): a JSON, YAML or TOML file
//! gives one definition per distinct key path, to `PDXE_DATA_KEY_MAX_DEPTH` keys, met in
//! document order, each the child of the key that holds it and counting its
//! occurrences; an array's elements share one path, `items[]`.

use pdx_engine::{DefinitionKind, Engine, FileExtract};

fn extract(engine: &Engine, language: &str, path: &str, source: &str) -> FileExtract {
    engine
        .extract(language, path, source.as_bytes())
        .unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Each key: its path below the file, its parent's path, its occurrences and the line
/// of its first occurrence, in the extraction's order.
fn keys(e: &FileExtract) -> Vec<(String, Option<String>, u32, u32)> {
    let names = e.qualified_names();
    let module = e
        .definitions
        .iter()
        .position(|d| d.kind == DefinitionKind::Module)
        .map(|i| names.get(i).to_owned());
    let below = |qn: &str| -> String {
        module
            .as_deref()
            .and_then(|m| qn.strip_prefix(m))
            .and_then(|rest| rest.strip_prefix('.'))
            .unwrap_or(qn)
            .to_owned()
    };
    e.definitions
        .iter()
        .enumerate()
        .filter(|(_, d)| d.kind != DefinitionKind::Module)
        .map(|(i, d)| {
            (
                below(names.get(i)),
                d.parent.map(|p| below(names.get(p as usize))),
                d.occurrences,
                d.span.map_or(0, |s| s.start_line),
            )
        })
        .collect()
}

fn key(
    path: &str,
    parent: Option<&str>,
    occurrences: u32,
    line: u32,
) -> (String, Option<String>, u32, u32) {
    (
        path.to_owned(),
        parent.map(str::to_owned),
        occurrences,
        line,
    )
}

#[test]
fn json_key_paths() {
    let engine = Engine::new().unwrap();
    let e = extract(
        &engine,
        "json",
        "shop/catalog.json",
        r#"{
  "name": "shop",
  "items": [
    {"id": 1, "tags": ["a"]},
    {"id": 2, "price": {"amount": 3}}
  ],
  "deep": {"a": {"b": {"c": {"d": 1}}}},
  "dotted.key": true,
  "name": "again"
}
"#,
    );
    assert_eq!(e.extraction_lost, 0);
    assert_eq!(
        keys(&e),
        [
            key("name", None, 2, 2),
            key("items", None, 1, 3),
            key("items[].id", Some("items"), 2, 4),
            key("items[].tags", Some("items"), 1, 4),
            key("items[].price", Some("items"), 1, 5),
            key("items[].price.amount", Some("items[].price"), 1, 5),
            key("deep", None, 1, 7),
            key("deep.a", Some("deep"), 1, 7),
            key("deep.a.b", Some("deep.a"), 1, 7),
            key("deep.a.b.c", Some("deep.a.b"), 1, 7),
            key("\"dotted.key\"", None, 1, 8),
        ]
    );
}

#[test]
fn yaml_key_paths() {
    let engine = Engine::new().unwrap();
    let e = extract(
        &engine,
        "yaml",
        "deploy/app.yaml",
        "spec:\n  containers:\n    - name: a\n      image: x\n    - name: b\n---\nspec:\n  replicas: 2\n  flow: {k: 1, j}\n",
    );
    assert_eq!(e.extraction_lost, 0);
    assert_eq!(
        keys(&e),
        [
            key("spec", None, 2, 1),
            key("spec.containers", Some("spec"), 1, 2),
            key("spec.containers[].name", Some("spec.containers"), 2, 3),
            key("spec.containers[].image", Some("spec.containers"), 1, 4),
            key("spec.replicas", Some("spec"), 1, 8),
            key("spec.flow", Some("spec"), 1, 9),
            key("spec.flow.k", Some("spec.flow"), 1, 9),
            key("spec.flow.j", Some("spec.flow"), 1, 9),
        ]
    );
}

#[test]
fn toml_key_paths() {
    let engine = Engine::new().unwrap();
    let e = extract(
        &engine,
        "toml",
        "Cargo.toml",
        "title = \"x\"\na.b.c = 1\n[server]\nport = 8080\n[server.tls]\ncert = \"c\"\n[[products]]\nname = \"p1\"\n[[products]]\nname = \"p2\"\nsku = 3\n",
    );
    assert_eq!(e.extraction_lost, 0);
    assert_eq!(
        keys(&e),
        [
            key("title", None, 1, 1),
            key("a", None, 1, 2),
            key("a.b", Some("a"), 1, 2),
            key("a.b.c", Some("a.b"), 1, 2),
            key("server", None, 2, 3),
            key("server.port", Some("server"), 1, 4),
            key("server.tls", Some("server"), 1, 5),
            key("server.tls.cert", Some("server.tls"), 1, 6),
            key("products", None, 2, 7),
            key("products[].name", Some("products"), 2, 8),
            key("products[].sku", Some("products"), 1, 11),
        ]
    );
}

#[test]
fn data_keys_stop_at_the_depth() {
    let engine = Engine::new().unwrap();
    assert_eq!(
        pdx_engine::DATA_KEY_MAX_DEPTH,
        4,
        "the engine's depth is the specification's"
    );
    let e = extract(
        &engine,
        "yaml",
        "deep.yaml",
        "a:\n  b:\n    c:\n      d:\n        e:\n          f: 1\n",
    );
    let paths: Vec<String> = keys(&e).into_iter().map(|k| k.0).collect();
    assert_eq!(paths, ["a", "a.b", "a.b.c", "a.b.c.d"]);
    // 10,000 keys of one array's elements are one path.
    let wide = format!(
        "[{}]",
        (0..10_000)
            .map(|i| format!("{{\"k\": {i}}}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let w = extract(&engine, "json", "wide.json", &wide);
    assert_eq!(keys(&w), [key("[].k", None, 10_000, 1)]);
}
