//! Isolated extraction survives the engine aborting: the worker dies, the indexing
//! process does not, and every other file is extracted. A worker that hangs is stopped
//! and reaped when its exchange times out, and the timeout is reported as itself, not
//! as a crash.
//!
//! The worker is this crate's own binary started with its hidden `engine-worker`
//! subcommand, exactly as an isolated build starts it. The engine is made to abort on a
//! named file by its own test switch, `PDX_ENGINE_TEST_CRASH_ON`, and to spin forever on
//! one by `PDX_ENGINE_TEST_HANG_ON`; both exist only in a test build and are set for the
//! worker alone.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use pdx_engine::isolate::{
    ExtractFailure, ExtractOutcome, IsolatedExtractor, IsolationError, SourceFile,
    WORKER_SUBCOMMAND, WorkerCommand,
};
use pdx_engine::{Engine, ProjectResolver};

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
fn isolation_carries_impl_relations() {
    // Protocol 4 carries an extraction's `impl Trait for Type` relations (issue 42): a
    // worker's extraction of a Rust file has them, exactly as this process's.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../engine/tests/fixtures/bases");
    let files: Vec<SourceFile> = ["one_impl.rs", "traits.rs"]
        .iter()
        .map(|name| SourceFile {
            language: "rust".to_owned(),
            rel_path: format!("src/{name}"),
            source: std::fs::read(dir.join(name)).unwrap(),
        })
        .collect();
    let expected = in_process(&files);
    let mut isolated = IsolatedExtractor::new(worker());
    let outcomes = isolated.extract_batch(&files).expect("a worker");
    assert_eq!(outcomes, expected);
    let relations: Vec<usize> = outcomes
        .iter()
        .map(|o| match o {
            ExtractOutcome::Extracted(e) => e.impl_traits.len(),
            failed @ ExtractOutcome::Failed(_) => panic!("not extracted: {failed:?}"),
        })
        .collect();
    assert_eq!(relations, [1, 4]);
}

#[test]
fn isolation_carries_site_paths_and_derivation_facts() {
    // Protocol 6 carries each call's node-type path and arguments and each
    // definition's decorators, parameter types and route bindings with their
    // declaring nodes (issues 46, 47 and 54): a worker's extraction has them, exactly
    // as this process's.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../engine/tests/fixtures/facts");
    let files: Vec<SourceFile> = [
        ("python", "routes.py"),
        ("java", "UserController.java"),
        ("javascript", "server.js"),
    ]
    .iter()
    .map(|(language, name)| SourceFile {
        language: (*language).to_owned(),
        rel_path: format!("src/{name}"),
        source: std::fs::read(dir.join(name)).unwrap(),
    })
    .collect();
    let expected = in_process(&files);
    let mut isolated = IsolatedExtractor::new(worker());
    let outcomes = isolated.extract_batch(&files).expect("a worker");
    assert_eq!(outcomes, expected);
    let mut facts = (0, 0, 0, 0);
    for outcome in &outcomes {
        let ExtractOutcome::Extracted(e) = outcome else {
            panic!("not extracted: {outcome:?}");
        };
        facts.0 += e.calls.iter().filter(|c| !c.ast_path.is_empty()).count();
        facts.1 += e.calls.iter().filter(|c| !c.args.is_empty()).count();
        facts.2 += e
            .definitions
            .iter()
            .filter(|d| !d.decorators.is_empty())
            .count();
        facts.3 += e
            .definitions
            .iter()
            .map(|d| d.routes.iter().filter(|r| !r.ast_path.is_empty()).count())
            .sum::<usize>();
    }
    assert!(facts.0 > 0 && facts.1 > 0 && facts.2 > 0, "{facts:?}");
    // FastAPI: three handlers, one with two decorators; Spring: one path, two paths,
    // and two paths by two methods.
    assert_eq!(
        facts.3,
        4 + 7,
        "every route binding, positioned, through the worker"
    );
}

#[test]
fn a_program_that_is_not_a_worker_is_refused() {
    // This binary without the subcommand says something that is not the protocol.
    let mut isolated = IsolatedExtractor::new(WorkerCommand::new(env!("CARGO_BIN_EXE_pdx")));
    let err = isolated.extract_batch(&files()[..1]).unwrap_err();
    assert!(err.to_string().contains("did not start"), "{err}");
}

/// Whether a process with this id exists, a zombie included: an id that is gone was
/// reaped.
fn exists(pid: u32) -> bool {
    #[cfg(unix)]
    let out = Command::new("sh")
        .args(["-c", &format!("kill -0 {pid}")])
        .stderr(Stdio::null())
        .output()
        .expect("sh runs");
    #[cfg(unix)]
    return out.status.success();
    #[cfg(windows)]
    {
        let out = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .stderr(Stdio::null())
            .output()
            .expect("tasklist runs");
        String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\""))
    }
}

#[test]
fn engine_isolate_times_out_and_reaps_worker() {
    const TIMEOUT: Duration = Duration::from_secs(2);
    let files = files();
    let hanger = files
        .iter()
        .position(|f| f.rel_path == "python.py")
        .expect("the python fixture");
    let others: Vec<SourceFile> = files
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != hanger)
        .map(|(_, f)| f.clone())
        .collect();

    let mut isolated = IsolatedExtractor::with_timeout(
        worker().env("PDX_ENGINE_TEST_HANG_ON", "python.py"),
        TIMEOUT,
    );
    // A worker that answers in time is used as usual.
    let first = isolated.extract_batch(&others[..1]).expect("a worker");
    assert!(
        matches!(first[0], ExtractOutcome::Extracted(_)),
        "{first:?}"
    );
    let hung = isolated.worker_id().expect("a running worker");
    assert!(exists(hung), "the running worker is visible");

    // The batch with the file the engine spins on: the exchange fails as a whole, as a
    // timeout, once the timeout has passed and not long after.
    let started = Instant::now();
    let err = isolated
        .extract_batch(&files[hanger - 1..=hanger + 1])
        .unwrap_err();
    let took = started.elapsed();
    let IsolationError::Timeout { after, worker } = err else {
        panic!("not a timeout: {err}");
    };
    assert_eq!(after, TIMEOUT);
    assert_eq!(worker, hung, "the worker that hung is the one reported");
    assert!(took >= TIMEOUT, "gave up after {took:?}");
    assert!(took < TIMEOUT * 15, "took {took:?} to give up");
    // Not a crash: nothing counted as one, no file failed with its reason, nothing
    // retried.
    assert_eq!(isolated.crashes(), 0);
    assert!(!err.to_string().contains("engine_crash"), "{err}");
    // The worker was stopped and reaped before the error came back.
    assert!(isolated.worker_id().is_none());
    assert!(!exists(hung), "the hung worker {hung} is still there");

    // The extractor still works: a new worker extracts every other file exactly as
    // the process does.
    let after_timeout = isolated.extract_batch(&others).expect("a new worker");
    assert_eq!(after_timeout, in_process(&others));
    let second = isolated.worker_id().expect("a running worker");
    drop(isolated);
    assert!(
        !exists(second),
        "a dropped extractor left its worker {second}"
    );
}

#[cfg(unix)]
#[test]
fn a_worker_that_never_introduces_itself_times_out() {
    // The introduction is bounded like every other exchange. `sleep` says nothing.
    let mut isolated = IsolatedExtractor::with_timeout(
        WorkerCommand::new("sleep").arg("600"),
        Duration::from_millis(500),
    );
    let err = isolated.extract_batch(&files()[..1]).unwrap_err();
    let IsolationError::Timeout { worker, .. } = err else {
        panic!("not a timeout: {err}");
    };
    assert!(!exists(worker), "the silent worker {worker} is still there");
    assert!(isolated.worker_id().is_none());
}

#[test]
fn extraction_lost_is_what_the_surface_carries() {
    // A worker whose TypeScript budget is cut to nothing loses work extracting; the
    // count it reports is the one its surface carries, so resolving the extraction
    // here, where the budget is not cut, counts exactly that much lost work.
    let source = b"interface Item { name: string; qty: number }
class Store {
    private items: Map<string, Item> = new Map();
    add(item: Item): void { this.items.set(item.name, item); }
    total(): number { let t = 0; for (const i of this.items.values()) { t += i.qty; } return t; }
}
export function build(): Store { const s = new Store(); s.add({ name: 'a', qty: 1 }); return s; }
"
    .to_vec();
    let file = SourceFile {
        language: "typescript".into(),
        rel_path: "store.ts".into(),
        source: source.clone(),
    };
    let engine = Engine::new().unwrap();
    let resolve = |outcome: &ExtractOutcome| {
        let ExtractOutcome::Extracted(extract) = outcome else {
            panic!("not extracted: {outcome:?}");
        };
        let mut project = ProjectResolver::new(&engine).unwrap();
        project.add_file(extract, &source).unwrap();
        (extract.extraction_lost, project.run().unwrap().health)
    };

    let mut starved = IsolatedExtractor::new(worker().env("PDX_ENGINE_TS_TYPE_BUDGET", "1"));
    let (lost, health) = resolve(&starved.extract_batch(std::slice::from_ref(&file)).unwrap()[0]);
    assert!(lost > 0, "nothing lost under a starved budget");
    assert_eq!(health.pass_failures, lost, "{health:?}");
    assert!(!health.is_clean());

    let (lost, health) = resolve(&engine.extract("typescript", "store.ts", &source).into());
    assert_eq!(lost, 0);
    assert_eq!(health.pass_failures, 0, "{health:?}");
    assert!(health.is_clean());
}
