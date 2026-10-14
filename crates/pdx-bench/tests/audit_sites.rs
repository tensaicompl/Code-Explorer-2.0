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
fn a_blocklisted_name_is_typed_with_a_qualifying_answer_and_blocked_without() {
    // Stage 3 takes a qualifying typed answer before the blocklist (issue 103); the
    // dataset keeps the name's blocklisting and the engine's answer either way, which
    // is what A0 counts blocked typed answers by.
    let rows = dataset(&[(
        "app/main.py",
        "class Store:\n    def update(self):\n        return 1\n\n\ndef caller():\n    s = Store()\n    return s.update()\n\n\ndef untyped(t):\n    return t.update()\n",
    )]);
    let found = sites(&rows, "update");
    let [typed, blocked] = found[..] else {
        panic!("two sites: {found:?}");
    };
    assert_eq!(typed["callee"], "s.update");
    assert_eq!(typed["band"], "typed");
    assert_eq!(typed["blocklisted"], true);
    assert_eq!(typed["engine"]["verdict"], "typed");
    assert_eq!(typed["engine"]["mapped"].as_array().map(Vec::len), Some(1));
    let def = rows
        .iter()
        .find(|r| r["t"] == "def" && r["name"] == "update")
        .expect("the method");
    assert_eq!(typed["target"][1], def["i"]);
    assert_eq!(blocked["callee"], "t.update");
    assert_eq!(blocked["band"], "blocked");
    assert_eq!(blocked["blocklisted"], true);
    assert_eq!(blocked["target"], Value::Null);
    assert_ne!(blocked["engine"]["verdict"], "typed");
}

/// `pdx-bench versions` (issue 115) is what `scripts/a0/run.py` compares with the
/// checkout's source before it measures anything: the executable's own engine's answer
/// and the constants compiled into it, never text from the checkout.
#[test]
fn versions_report_what_the_executable_was_built_with() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_pdx-bench"))
        .arg("versions")
        .output()
        .expect("pdx-bench runs");
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).expect("one JSON document");
    assert_eq!(report, pdx_bench::audit::versions());
    assert_eq!(report["format"], pdx_bench::audit::VERSIONS_FORMAT);
    assert_eq!(report["engine_version"], pdx_engine::Engine::version());
    assert_eq!(
        report["engine_version"],
        pdx_core::consts::ENGINE_VERSION.to_string()
    );
    let core = report["pdx_core"].as_object().expect("the constants");
    for (name, value) in [
        ("ENGINE_VERSION", pdx_core::consts::ENGINE_VERSION),
        (
            "EXTRACT_CACHE_FORMAT_VERSION",
            pdx_core::consts::EXTRACT_CACHE_FORMAT_VERSION,
        ),
        (
            "LANGUAGE_MATRIX_VERSION",
            pdx_core::consts::LANGUAGE_MATRIX_VERSION,
        ),
        (
            "SECRET_DETECTOR_VERSION",
            pdx_core::consts::SECRET_DETECTOR_VERSION,
        ),
        (
            "SEGMENT_SCHEMA_VERSION",
            pdx_core::consts::SEGMENT_SCHEMA_VERSION,
        ),
    ] {
        assert_eq!(core[name], value, "{name}");
    }
    assert_eq!(core.len(), 5);
    assert_eq!(report["worker_protocol"], pdx_engine::isolate::PROTOCOL);
    assert_eq!(report.as_object().expect("an object").len(), 4);
}
