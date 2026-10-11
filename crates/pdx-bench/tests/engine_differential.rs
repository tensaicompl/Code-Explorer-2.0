//! The engine differential (P2-16): its canonical comparison, offline, on a committed
//! synthetic pair of row dumps, and its corpora's coverage of the language matrix and
//! the golden repositories. No reference engine, no network: the live comparison is
//! `make engine-differential`, nightly.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pdx_bench::differential::{self, Manifest};
use pdx_bench::lock;
use pdx_core::languages::{self, ENGINE_DIALECTS, Tier};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn data(name: &str) -> PathBuf {
    root()
        .join("crates/pdx-bench/tests/data/engine-differential")
        .join(name)
}

const REFERENCE_PROJECT: &str = "tmp-reference-project";
const VENDORED_PROJECT: &str = "__pdx__";

fn harness(args: &[&str]) -> Output {
    Command::new("python3")
        .arg(root().join("scripts/engine/differential.py"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("python3 runs the harness")
}

fn canonical(rows: &Path, project: &str, side: &str) -> Vec<u8> {
    let out = harness(&[
        "canonical",
        "--project",
        project,
        "--side",
        side,
        rows.to_str().expect("utf-8"),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

fn compare(reference: &Path, vendored: &Path) -> Output {
    harness(&[
        "compare",
        "--reference",
        reference.to_str().expect("utf-8"),
        "--reference-project",
        REFERENCE_PROJECT,
        "--vendored",
        vendored.to_str().expect("utf-8"),
        "--vendored-project",
        VENDORED_PROJECT,
    ])
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// The canonical comparison the live differential applies to every corpus: rows that
/// are the same facts canonicalise to the same bytes whatever their order and project
/// name, both sides take the interface's kind for an engine label, and one changed fact
/// fails with a bounded account of it.
#[test]
fn engine_differential_identical() {
    let reference = data("reference.jsonl");
    let vendored = data("vendored.jsonl");

    // The same facts: identical, and the same canonical bytes.
    let out = compare(&reference, &vendored);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("identical: files 2, definitions 4, calls 3, imports 1"));
    let from_reference = canonical(&reference, REFERENCE_PROJECT, "reference");
    let from_vendored = canonical(&vendored, VENDORED_PROJECT, "vendored");
    assert!(!from_reference.is_empty());
    assert_eq!(from_reference, from_vendored);

    // Input order is not evidence.
    let tmp = tempfile::tempdir().expect("tempdir");
    let rows = fs::read_to_string(&vendored).expect("rows");
    let mut reversed: Vec<&str> = rows.lines().rev().collect();
    reversed.push("");
    let reversed = reversed.join("\n");
    let reordered = tmp.path().join("reordered.jsonl");
    fs::write(&reordered, reversed).expect("write");
    assert_eq!(
        canonical(&reordered, VENDORED_PROJECT, "vendored"),
        from_vendored
    );

    // The interface's kind mapping applies to both: a Property is a field.
    let canonical_text = String::from_utf8(from_reference).expect("ascii");
    assert!(
        canonical_text.contains(r#""kind":"field","label":"Property""#),
        "{canonical_text}"
    );
    assert!(canonical_text.contains(r#""entry":{"dm":"@.pkg.models","lb":"Property""#));

    // A changed fact fails: one call's callee, on one side.
    let changed = rows.replacen(
        r#""callee":"User","line":5"#,
        r#""callee":"Admin","line":5"#,
        1,
    );
    assert_ne!(
        changed, rows,
        "the fixture holds the call the mutation changes"
    );
    let mutated = tmp.path().join("mutated.jsonl");
    fs::write(&mutated, changed).expect("write");
    let out = compare(&reference, &mutated);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    let report = text(&out);
    assert!(
        report.contains(
            "compare: calls: 1 row(s) only the reference has, 1 only the vendored engine has"
        ),
        "{report}"
    );
    assert!(report.contains("reference only:") && report.contains(r#""callee":"User""#));
    assert!(report.contains("vendored only:") && report.contains(r#""callee":"Admin""#));
}

fn manifest() -> Manifest {
    let text = fs::read_to_string(root().join(differential::MANIFEST)).expect("the manifest");
    differential::parse(&text).expect("parses")
}

#[test]
fn the_differential_manifest_holds_its_rules() {
    let repos = lock::bench_repos(&root().join("bench/repos.lock")).expect("the lock");
    let dirs = differential::fixture_dirs(&root()).expect("fixtures");
    differential::check_manifest(&manifest(), &repos, &dirs).expect("the manifest is complete");
}

#[test]
fn every_typed_tier_language_has_a_differential_fixture() {
    let typed: Vec<&str> = languages::all()
        .iter()
        .filter(|l| l.tier == Tier::Typed)
        .map(|l| l.id)
        .collect();
    assert_eq!(typed.len(), 12, "the typed tier P2-16 covers");
    let manifest = manifest();
    for language in typed {
        let fixtures: Vec<&str> = manifest
            .fixture
            .iter()
            .filter(|f| f.language == language)
            .map(|f| f.name.as_str())
            .collect();
        assert!(
            !fixtures.is_empty(),
            "no differential fixture for {language}"
        );
    }
}

#[test]
fn every_typed_tier_golden_has_a_differential_subset() {
    let repos = lock::bench_repos(&root().join("bench/repos.lock")).expect("the lock");
    let typed: Vec<String> = differential::typed_goldens(&repos)
        .expect("goldens")
        .iter()
        .map(pdx_bench::golden::plan::GoldenEntry::key)
        .collect();
    assert_eq!(typed.len(), 9, "every golden but Ada's is typed");
    let subsets: Vec<String> = manifest().golden.iter().map(|g| g.key.clone()).collect();
    assert_eq!(subsets, typed);
}

#[test]
fn every_fixture_file_is_its_corpus_language() {
    for fixture in &manifest().fixture {
        let corpus = differential::fixture_corpus(&root(), fixture)
            .unwrap_or_else(|e| panic!("{}: {e}", fixture.name));
        assert!(!corpus.files.is_empty());
        assert!(
            corpus.files.len() >= 2,
            "{}: a fixture of one file",
            fixture.name
        );
    }
}

#[test]
fn the_harness_dialects_are_the_matrix_s() {
    // pdx-bench gives each file its engine language from the matrix; this is the only
    // dialect there is, and a new one must be met by the reference's configuration.
    assert_eq!(ENGINE_DIALECTS.len(), 1);
    assert_eq!(
        (
            ENGINE_DIALECTS[0].language,
            ENGINE_DIALECTS[0].extension,
            ENGINE_DIALECTS[0].engine_id
        ),
        ("typescript", ".tsx", "tsx")
    );
}

/// `resolve_dump`, the C tool behind the typed-resolution fixtures, has its own table of
/// extensions; each must be the matrix's language for it, and every typed-tier
/// language must have one.
#[test]
fn resolve_dump_extensions_agree_with_the_matrix() {
    let source = fs::read_to_string(root().join("engine/tests/resolve_dump.c")).expect("source");
    let table = source
        .split("} EXTENSIONS[] = {")
        .nth(1)
        .and_then(|rest| rest.split("};").next())
        .expect("the EXTENSIONS table");
    let pairs: Vec<(String, String)> = table
        .split('{')
        .skip(1)
        .filter_map(|cell| {
            let quoted: Vec<&str> = cell.split('"').collect();
            (quoted.len() >= 4).then(|| (quoted[1].to_owned(), quoted[3].to_owned()))
        })
        .collect();
    assert!(pairs.len() >= 18, "{pairs:?}");
    for (extension, engine) in &pairs {
        let path = format!("file{extension}");
        let language = languages::detect(&path, b"", |_| false)
            .unwrap_or_else(|| panic!("{extension} is no language of the matrix"));
        assert_eq!(language.engine_language(&path), engine, "{extension}");
    }
    for language in languages::all().iter().filter(|l| l.tier == Tier::Typed) {
        assert!(
            pairs
                .iter()
                .any(|(extension, _)| language.extensions.contains(&extension.as_str())),
            "resolve_dump reads no file of {}",
            language.id
        );
    }
}
