//! A run says whether typed resolution did all its work, apart from what it found
//! (docs/plan/ISSUES.md, issue 21).
//!
//! No answer is never evidence of a clean run on its own: a project with nothing to
//! resolve is clean, a project whose resolution was made to skip a file is degraded
//! even when that leaves no answer at all, and a degraded run still reports the answers
//! it found. A file is made to be skipped by the engine's own test switch, which marks
//! the file named in `PDX_ENGINE_TEST_LSP_SKIP_ON` exactly as its node budget would;
//! those tests run in a child process, so the switch reaches no other test.

mod common;

use std::path::Path;

use pdx_engine::{Engine, ProjectResolution, ProjectResolver, RunHealth, RunStatus};

const SKIP: &str = "PDX_ENGINE_TEST_LSP_SKIP_ON";

/// Resolves the named files of a fixture, fresh and from their extractions as a cache
/// would hold them, and checks the two runs agree, health included.
fn resolve(dir: &Path, files: &[&str]) -> ProjectResolution {
    let engine = Engine::new().unwrap();
    let sources: Vec<(String, &str, Vec<u8>)> = files
        .iter()
        .map(|f| {
            let lang = common::language_of(f).expect("a known language");
            ((*f).to_owned(), lang, std::fs::read(dir.join(f)).unwrap())
        })
        .collect();

    let mut fresh = ProjectResolver::new(&engine).unwrap();
    let extracts: Vec<_> = sources
        .iter()
        .map(|(rel, lang, source)| fresh.extract_and_add(lang, rel, source).unwrap())
        .collect();
    let fresh = fresh.run().unwrap();

    let mut cached = ProjectResolver::new(&engine).unwrap();
    for (extract, (_, _, source)) in extracts.iter().zip(&sources) {
        cached.add_file(extract, source).unwrap();
    }
    let cached = cached.run().unwrap();
    assert_eq!(fresh, cached, "a cached run differs from a fresh one");
    fresh
}

fn counted(h: &RunHealth) -> u32 {
    h.files_resolved
        + h.files_untyped
        + h.files_empty
        + h.files_over_budget
        + h.files_source_unavailable
        + h.files_not_reached
}

#[test]
fn a_project_with_nothing_to_resolve_is_clean() {
    let run = resolve(
        &common::fixtures().join("health/quiet"),
        &["empty.py", "notes.md", "plain.py"],
    );
    assert!(run.resolutions.is_empty());
    let h = run.health;
    assert_eq!(h.status, RunStatus::Clean);
    assert_eq!(
        (h.files, h.files_resolved, h.files_untyped, h.files_empty),
        (3, 1, 1, 1)
    );
    assert_eq!(counted(&h), h.files);
}

#[test]
fn a_degraded_run_is_degraded_even_with_no_answer() {
    if common::rerun_in_child(
        "a_degraded_run_is_degraded_even_with_no_answer",
        &[(SKIP, "calls.py")],
    ) {
        return;
    }
    let run = resolve(&common::fixtures().join("health/one_file"), &["calls.py"]);
    assert!(run.resolutions.is_empty(), "{:?}", run.resolutions);
    let h = run.health;
    assert_eq!(h.status, RunStatus::Degraded);
    assert!(!h.is_clean());
    assert_eq!((h.files, h.files_over_budget, h.files_resolved), (1, 1, 0));
    assert_eq!(counted(&h), h.files);
}

#[test]
fn the_same_file_unskipped_is_clean_and_answered() {
    // The control for the test above: without the switch, the one file resolves.
    let run = resolve(&common::fixtures().join("health/one_file"), &["calls.py"]);
    assert!(!run.resolutions.is_empty());
    assert_eq!(run.health.status, RunStatus::Clean);
    assert_eq!(run.health.files_resolved, 1);
}

#[test]
fn a_degraded_run_keeps_the_answers_it_found() {
    if common::rerun_in_child(
        "a_degraded_run_keeps_the_answers_it_found",
        &[(SKIP, "app/service.py")],
    ) {
        return;
    }
    let run = resolve(
        &common::fixtures().join("resolve/python_cross_file_calls"),
        &["app/models.py", "app/service.py"],
    );
    let h = run.health;
    assert_eq!(h.status, RunStatus::Degraded);
    assert_eq!((h.files_resolved, h.files_over_budget), (1, 1));
    // The skipped file's answers are gone; the other file's remain.
    assert!(!run.resolutions.is_empty());
    assert!(
        run.resolutions
            .iter()
            .all(|r| r.site_ref.rel_path == "app/models.py"),
        "{:?}",
        run.resolutions
    );
}
