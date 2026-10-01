//! Who owns what: an engine stays on its thread, a project owns what it resolves
//! through, and nothing handed out borrows from either.

mod common;

use pdx_engine::{Engine, EngineError, ProjectResolver, ResolutionMetadata};

/// Compiles only if `T` is not `Send`. With two blanket implementations, one for every
/// type and one for `Send` types, naming the method is ambiguous exactly when `T` is
/// `Send`.
macro_rules! assert_not_send {
    ($t:ty) => {
        const _: fn() = || {
            trait AmbiguousIfSend<A> {
                fn some_item() {}
            }
            impl<T: ?Sized> AmbiguousIfSend<()> for T {}
            impl<T: ?Sized + Send> AmbiguousIfSend<u8> for T {}
            let _ = <$t as AmbiguousIfSend<_>>::some_item;
        };
    };
}

/// As above, for `Sync`.
macro_rules! assert_not_sync {
    ($t:ty) => {
        const _: fn() = || {
            trait AmbiguousIfSync<A> {
                fn some_item() {}
            }
            impl<T: ?Sized> AmbiguousIfSync<()> for T {}
            impl<T: ?Sized + Sync> AmbiguousIfSync<u8> for T {}
            let _ = <$t as AmbiguousIfSync<_>>::some_item;
        };
    };
}

// An engine's parser state is its thread's: it can neither move to another thread nor
// be shared with one, and neither can a project, which borrows it.
assert_not_send!(Engine);
assert_not_sync!(Engine);
assert_not_send!(ProjectResolver<'static>);
assert_not_sync!(ProjectResolver<'static>);

#[test]
fn a_thread_has_one_engine_at_a_time() {
    let engine = Engine::new().unwrap();
    assert!(matches!(
        Engine::new(),
        Err(EngineError::EngineAlreadyOnThread)
    ));
    // Another thread has its own, alongside this one.
    std::thread::spawn(|| {
        let other = Engine::new().expect("an engine of its own");
        other.extract("python", "a.py", b"x = 1\n").unwrap();
    })
    .join()
    .unwrap();
    drop(engine);
    // Once it is gone the thread can start another.
    let again = Engine::new().unwrap();
    again.extract("python", "a.py", b"x = 1\n").unwrap();
}

#[test]
fn a_project_frees_everything_when_dropped_after_an_error() {
    let engine = Engine::new().unwrap();
    let dir = common::fixtures().join("resolve/python_cross_file_calls");
    let sources = common::source_files(&dir);
    {
        let mut resolver = ProjectResolver::new(&engine).unwrap();
        let (rel, lang, source) = &sources[0];
        resolver.extract_and_add(lang, rel, source).unwrap();
        let extract = engine.extract(lang, rel, source).unwrap();
        // A project holds the project itself and the result it resolves through.
        assert_eq!(engine.live_handles(), 2);
        // The same path twice is refused, and the project cannot run after it.
        let err = resolver.add_file(&extract, source).unwrap_err();
        assert!(matches!(err, EngineError::Invalid(_)), "{err}");
        // Dropped unrun: the project ends and the results it held are freed.
    }
    assert_eq!(engine.live_handles(), 0);

    // A run that fails is cleaned up too.
    let mut resolver = ProjectResolver::new(&engine).unwrap();
    let (rel, lang, source) = &sources[0];
    resolver.extract_and_add(lang, rel, source).unwrap();
    resolver
        .set_metadata(&ResolutionMetadata::default())
        .unwrap();
    let err = resolver
        .set_metadata(&ResolutionMetadata::default())
        .unwrap_err();
    assert!(matches!(err, EngineError::Invalid(_)), "{err}");
    assert_eq!(resolver.run().unwrap_err(), err);
    assert_eq!(engine.live_handles(), 0);
}

#[test]
fn answers_outlive_their_project_and_engine() {
    let dir = common::fixtures().join("resolve/python_cross_file_calls");
    let run = {
        let engine = Engine::new().unwrap();
        let mut resolver = ProjectResolver::new(&engine).unwrap();
        for (rel, lang, source) in common::source_files(&dir) {
            resolver.extract_and_add(lang, &rel, &source).unwrap();
        }
        resolver.run().unwrap()
        // The project ends, then the engine shuts down.
    };
    assert!(!run.resolutions.is_empty());
    for r in &run.resolutions {
        assert!(!r.site_ref.rel_path.is_empty());
        assert!(!r.target_qn.is_empty());
        assert!(!r.site.callee_text.is_empty());
    }
}

#[test]
fn an_engine_that_extracted_frees_every_result() {
    let engine = Engine::new().unwrap();
    let dir = common::fixtures().join("smoke");
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        // A smoke fixture is named after its language.
        let lang = name.split('.').next().unwrap();
        engine
            .extract(lang, &name, &std::fs::read(&path).unwrap())
            .unwrap();
        assert_eq!(engine.live_handles(), 0, "{name}");
    }
}
