//! A0.1's site dataset (`pdx-bench audit-sites`, decision 64): what Stage 3 decided,
//! written as it decided it. The dataset's band for a site is the one the pipeline
//! gives it, and every field the audit reads is present.

use std::fs;

use pdx_bench::audit::{FORMAT, write_dataset};
use serde_json::Value;

fn dataset(files: &[(&str, &str)]) -> Vec<Value> {
    let dir = tempfile::tempdir().expect("a temporary directory");
    for (path, text) in files {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().expect("a parent")).expect("a directory");
        fs::write(full, text).expect("a file");
    }
    let mut out = Vec::new();
    let counts = write_dataset(dir.path(), &mut out).expect("the dataset");
    let rows: Vec<Value> = String::from_utf8(out)
        .expect("UTF-8")
        .lines()
        .map(|l| serde_json::from_str(l).expect("one JSON object a line"))
        .collect();
    assert_eq!(
        rows.len(),
        1 + counts.files + counts.definitions + counts.resolved + counts.unconfirmed
    );
    rows
}

fn sites<'a>(rows: &'a [Value], name: &str) -> Vec<&'a Value> {
    rows.iter()
        .filter(|r| r["t"] == "site" && r["name"] == name)
        .collect()
}

#[test]
fn a_site_carries_its_band_target_and_what_the_audit_reads() {
    let rows = dataset(&[(
        "app/main.py",
        "def compute_total(n):\n    return n\n\n\ndef caller():\n    return compute_total(1)\n",
    )]);
    assert_eq!(rows[0]["t"], "header");
    assert_eq!(rows[0]["format"], FORMAT);
    let def = rows
        .iter()
        .find(|r| r["t"] == "def" && r["name"] == "compute_total")
        .expect("the definition");
    let span = def["span"].as_array().expect("a span");
    assert!(span[0].as_u64() < span[1].as_u64());
    let [site] = sites(&rows, "compute_total")[..] else {
        panic!("one site");
    };
    assert_eq!(site["status"], "resolved");
    assert!(
        ["typed", "exact", "import-guided", "scoped"].contains(&site["band"].as_str().unwrap())
    );
    assert_eq!(site["target"][0], "app/main.py");
    assert_eq!(site["target"][1], def["i"]);
    assert_eq!(site["materialized"], true);
    assert_eq!(site["callee"], "compute_total");
    let callee = site["callee_span"].as_array().expect("source bytes");
    assert_eq!(
        callee[1].as_u64().unwrap() - callee[0].as_u64().unwrap(),
        13
    );
    for key in [
        "blocklisted",
        "name_provenance",
        "lexical",
        "engine",
        "caller",
    ] {
        assert!(site.get(key).is_some(), "{key}");
    }
}

#[test]
fn a_blocklisted_site_is_blocked_and_keeps_the_engines_answer() {
    let rows = dataset(&[(
        "app/main.py",
        "class Store:\n    def update(self):\n        return 1\n\n\ndef caller():\n    s = Store()\n    return s.update()\n",
    )]);
    let [site] = sites(&rows, "update")[..] else {
        panic!("one site");
    };
    assert_eq!(site["band"], "blocked");
    assert_eq!(site["blocklisted"], true);
    assert_eq!(site["target"], Value::Null);
    // Stage 3 settles the blocklist first; the engine's answer stays beside it.
    if let Some(engine) = site["engine"].as_object() {
        assert!(engine.contains_key("verdict"));
        assert!(engine.contains_key("mapped"));
    }
}
