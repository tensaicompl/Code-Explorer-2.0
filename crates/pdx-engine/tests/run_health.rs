//! A run says whether typed resolution did all its work, apart from what it found
//! (docs/plan/ISSUES.md, issue 21).
//!
//! No answer is never evidence of a clean run on its own: a project with nothing to
//! resolve is clean, a project whose resolution was made to skip a file is degraded
//! even when that leaves no answer at all, a degraded run still reports the answers it
//! found, and an answer lost inside typed resolution degrades the run too. A file is
//! made to be skipped by the engine's own test switch, which marks the file named in
//! `PDX_ENGINE_TEST_LSP_SKIP_ON` exactly as its node budget would; an answer is lost by
//! `PDX_ENGINE_TEST_FAIL_ANSWER_ON`, which fails its copy into the result as a failed
//! allocation would. Those tests run in a child process, so a switch reaches no other
//! test.

mod common;

use std::path::Path;

use pdx_engine::{Engine, ProjectResolution, ProjectResolver, RunHealth, RunStatus};

const SKIP: &str = "PDX_ENGINE_TEST_LSP_SKIP_ON";
const FAIL_ANSWER: &str = "PDX_ENGINE_TEST_FAIL_ANSWER_ON";

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

/// The two-file cross-file fixture, resolved fresh and from cached extractions.
fn cross_file_run() -> ProjectResolution {
    resolve(
        &common::fixtures().join("resolve/python_cross_file_calls"),
        &["app/models.py", "app/service.py"],
    )
}

#[test]
fn the_cross_file_fixture_is_clean_and_answers_make_user() {
    // The control for the test below: without the fault, the run is clean and
    // includes the answers the fault removes.
    let run = cross_file_run();
    assert_eq!(run.health.status, RunStatus::Clean);
    assert_eq!(run.health.pass_failures, 0);
    assert_eq!(run.resolutions.len(), 8);
    assert_eq!(
        run.resolutions
            .iter()
            .filter(|r| r.target_qn.contains("make_user"))
            .count(),
        2
    );
}

#[test]
fn typed_resolution_internal_failure_degrades_run() {
    if common::rerun_in_child(
        "typed_resolution_internal_failure_degrades_run",
        &[(FAIL_ANSWER, "make_user")],
    ) {
        return;
    }
    // Typed resolution finds both answers to make_user, and both are lost while they
    // are copied into the result. The run still completes and resolves both files, but
    // it cannot claim to be clean, and the answers it did not lose remain.
    let run = cross_file_run();
    let h = run.health;
    assert_eq!(h.status, RunStatus::Degraded);
    assert!(h.pass_failures > 0, "{h:?}");
    assert_eq!(h.files_resolved, 2);
    assert_eq!(run.resolutions.len(), 6, "{:?}", run.resolutions);
    assert!(
        run.resolutions
            .iter()
            .all(|r| !r.target_qn.contains("make_user")),
        "{:?}",
        run.resolutions
    );
}
