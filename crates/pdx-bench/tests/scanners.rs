//! The provenance scanner detects what it must detect, and passes this tree.
//!
//! The scanner is the mechanism that keeps upstream names out of the source, so a
//! scanner that silently stops matching is worse than no scanner at all. These
//! tests run it against deliberately broken fixtures.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root resolves")
}

/// Runs the scanner against a directory and returns its exit status and output.
fn scan(dir: &Path) -> (bool, String) {
    let root = repo_root();
    let out = Command::new(root.join("scripts/provenance-scan.sh"))
        .arg(dir)
        .current_dir(&root)
        .output()
        .expect("the scanner runs");
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

/// Writes one file into a fresh temporary directory and scans it.
fn scan_fixture(name: &str, contents: &str) -> (bool, String) {
    let dir = std::env::temp_dir().join(format!("pdx-scan-{}-{}", std::process::id(), name));
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("fixture directory");
    std::fs::write(src.join(name), contents).expect("fixture file");
    let result = scan(&dir);
    std::fs::remove_dir_all(&dir).ok();
    result
}

/// A literal term from the deny list, read at run time.
///
/// The term is not written here: this file would then contain the very string the
/// scanner must reject, and the scanner would rightly fail on itself. Reading the
/// list also keeps the test honest when the list changes.
fn a_denied_literal() -> String {
    let list = std::fs::read_to_string(repo_root().join("scripts/provenance-denylist.txt"))
        .expect("the deny list is readable");
    list.lines()
        // A line may carry a replacement after a tab, used when sanitising
        // vendored sources. Only the pattern is a string to plant.
        .map(|l| l.split('\t').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        // Plain literals only: an entry carrying regular-expression syntax is not
        // a string we can plant as it stands.
        .find(|l| l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .map(str::to_owned)
        .expect("the deny list holds at least one plain literal")
}

#[test]
fn provenance_scan_detects() {
    let planted = format!(
        "// adapted from {}\nexport const x = 1;\n",
        a_denied_literal()
    );
    let (ok, out) = scan_fixture("a.ts", &planted);
    assert!(
        !ok,
        "the scanner passed a file naming a ported reference:\n{out}"
    );
    assert!(
        out.contains("denied string"),
        "the failure does not say what was found:\n{out}"
    );
}

#[test]
fn provenance_scan_detects_marker_words() {
    let (ok, out) = scan_fixture("b.rs", "fn f() {\n    // TODO: finish\n}\n");
    assert!(
        !ok,
        "the scanner passed a marker word in non-test code:\n{out}"
    );
    assert!(out.contains("marker word"), "unexpected failure:\n{out}");
}

#[test]
fn provenance_scan_allows_marker_words_in_tests() {
    // Test code may carry them: the rule protects shipped source.
    let (ok, out) = scan_fixture("c.test.ts", "it('x', () => {\n  // TODO: extend\n});\n");
    assert!(
        ok,
        "the scanner rejected a marker word inside a test:\n{out}"
    );
}

#[test]
fn provenance_scan_detects_unlisted_urls() {
    // Assembled rather than written, so that this file carries no address of its
    // own for the scanner to object to.
    let url = format!("{}://{}/{}", "https", "telemetry.invalid", "v1");
    let (ok, out) = scan_fixture("d.ts", &format!("const beacon = '{url}';\n"));
    assert!(
        !ok,
        "the scanner passed an undeclared outbound address:\n{out}"
    );
    assert!(
        out.contains("not on the allow list"),
        "unexpected failure:\n{out}"
    );
}

#[test]
fn provenance_scan_passes_a_clean_fixture() {
    let (ok, out) = scan_fixture("e.rs", "/// A function.\npub fn f() -> u8 {\n    1\n}\n");
    assert!(ok, "the scanner rejected a clean file:\n{out}");
}

#[test]
fn provenance_scan_passes_this_tree() {
    let (ok, out) = scan(&repo_root());
    assert!(ok, "the scanner rejects the current tree:\n{out}");
}

#[test]
fn the_deny_list_and_its_readers_agree() {
    // The plan extractor redacts using the same list the scanner enforces, so
    // that there is one definition of what counts as provenance.
    let root = repo_root();
    let list = root.join("scripts/provenance-denylist.txt");
    assert!(list.is_file(), "the deny list is missing");

    let extractor = std::fs::read_to_string(root.join("scripts/plan/extract-plan.py"))
        .expect("the extractor is present");
    assert!(
        extractor.contains("scripts/provenance-denylist.txt"),
        "the extractor does not read the canonical deny list"
    );

    let entries = std::fs::read_to_string(&list)
        .expect("deny list readable")
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .count();
    assert!(entries >= 5, "the deny list has only {entries} entries");
}
