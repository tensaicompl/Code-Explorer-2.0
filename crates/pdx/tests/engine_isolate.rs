//! Isolated extraction survives the engine aborting: the worker dies, the indexing
//! process does not, and every other file is extracted.
//!
//! The worker is this crate's own binary started with its hidden `engine-worker`
//! subcommand, exactly as an isolated build starts it. The engine is made to abort on a
//! named file by its own test switch, `PDX_ENGINE_TEST_CRASH_ON`, which exists only in a
//! test build and is set for the worker alone.

use std::path::Path;

use pdx_engine::Engine;
use pdx_engine::isolate::{
    ExtractFailure, ExtractOutcome, IsolatedExtractor, SourceFile, WORKER_SUBCOMMAND, WorkerCommand,
};

fn worker() -> WorkerCommand {
    WorkerCommand::new(env!("CARGO_BIN_EXE_pdx")).arg(WORKER_SUBCOMMAND)
}

/// The smoke fixtures, one per language of the matrix, named after their language.
fn files() -> Vec<SourceFile> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../engine/tests/fixtures/smoke");
    let mut files: Vec<SourceFile> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| {
            let path = e.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            SourceFile {
                language: name.split('.').next().unwrap().to_owned(),
                rel_path: name,
                source: std::fs::read(&path).unwrap(),
            }
        })
        .collect();
    files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    files
}

/// Each file extracted in this process, for comparison.
fn in_process(files: &[SourceFile]) -> Vec<ExtractOutcome> {
    let engine = Engine::new().unwrap();
    files
        .iter()
        .map(|f| engine.extract(&f.language, &f.rel_path, &f.source).into())
        .collect()
}

#[test]
fn engine_isolate_recovers() {
    let files = files();
    let crasher = files
        .iter()
        .position(|f| f.rel_path == "python.py")
        .expect("the python fixture");
    assert!(
        crasher > 0 && crasher + 1 < files.len(),
        "files on both sides of it"
    );

    let mut isolated =
        IsolatedExtractor::new(worker().env("PDX_ENGINE_TEST_CRASH_ON", "python.py"));
    // Batches of four, so the crash takes down a batch with other files in it, and
    // more batches follow it.
    let mut outcomes = Vec::new();
    for batch in files.chunks(4) {
        outcomes.extend(isolated.extract_batch(batch).expect("a worker"));
    }

    assert_eq!(outcomes.len(), files.len());
    // The file the engine aborted on is the one failure, for the reason the spec names.
    assert_eq!(
        outcomes[crasher],
        ExtractOutcome::Failed(ExtractFailure::EngineCrash)
    );
    assert_eq!(
        ExtractFailure::EngineCrash.reason(),
        "engine_crash",
        "the reason files are recorded with"
    );
    // The worker died with it at least once, and the parent went on.
    assert!(isolated.crashes() >= 1);
    // Every other file, before and after it, in its batch and in later ones, is
    // extracted, and extracted exactly as in process: nothing a dying worker sent is
    // used.
    let expected = in_process(&files);
    for (i, (got, want)) in outcomes.iter().zip(&expected).enumerate() {
        if i == crasher {
            continue;
        }
        assert!(
            matches!(got, ExtractOutcome::Extracted(_)),
            "{}: {got:?}",
            files[i].rel_path
        );
        assert_eq!(got, want, "{}", files[i].rel_path);
    }
}

#[test]
fn isolation_does_not_change_what_is_extracted() {
    // Without a crash, the worker's extractions are the process's own, however the
    // files are batched.
    let files = files();
    let expected = in_process(&files);
    for size in [1, 5, files.len()] {
        let mut isolated = IsolatedExtractor::new(worker());
        let mut outcomes = Vec::new();
        for batch in files.chunks(size) {
            outcomes.extend(isolated.extract_batch(batch).expect("a worker"));
        }
        assert_eq!(outcomes, expected, "batches of {size}");
        assert_eq!(isolated.crashes(), 0);
    }
}

#[test]
fn a_program_that_is_not_a_worker_is_refused() {
    // This binary without the subcommand says something that is not the protocol.
    let mut isolated = IsolatedExtractor::new(WorkerCommand::new(env!("CARGO_BIN_EXE_pdx")));
    let err = isolated.extract_batch(&files()[..1]).unwrap_err();
    assert!(err.to_string().contains("did not start"), "{err}");
}
