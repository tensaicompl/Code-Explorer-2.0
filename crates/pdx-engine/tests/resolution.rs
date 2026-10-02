//! Typed resolution through the safe interface: the same answers from fresh and from
//! cached extractions, each answer owned and complete.

mod common;

use std::path::Path;

use pdx_engine::{
    Engine, EngineError, FileExtract, LexicalFacts, ProjectResolution, ProjectResolver, RunStatus,
    SourceDifference, SourceDigest, Strategy,
};

/// Resolves a fixture from fresh extractions: each file extracted by the resolver's
/// engine and resolved through the engine's own result.
fn fresh(engine: &Engine, dir: &Path) -> (Vec<FileExtract>, ProjectResolution) {
    let mut resolver = ProjectResolver::new(engine).unwrap();
    let extracts = common::source_files(dir)
        .into_iter()
        .map(|(rel, lang, source)| resolver.extract_and_add(lang, &rel, &source).unwrap())
        .collect();
    if let Some(m) = common::metadata(dir) {
        resolver.set_metadata(&m).unwrap();
    }
    (extracts, resolver.run().unwrap())
}

/// Resolves a fixture from extractions as a cache returns them: serialised, read back,
/// and resolved through their surfaces, never extracted again.
fn cached(engine: &Engine, dir: &Path, extracts: &[FileExtract]) -> ProjectResolution {
    let mut resolver = ProjectResolver::new(engine).unwrap();
    for (extract, (rel, _, source)) in extracts.iter().zip(common::source_files(dir)) {
        assert_eq!(extract.rel_path, rel);
        let bytes = postcard::to_stdvec(extract).unwrap();
        let from_cache: FileExtract = postcard::from_bytes(&bytes).unwrap();
        resolver.add_file(&from_cache, &source).unwrap();
    }
    if let Some(m) = common::metadata(dir) {
        resolver.set_metadata(&m).unwrap();
    }
    resolver.run().unwrap()
}

#[test]
fn resolving_cached_extractions_gives_the_fresh_answers() {
    let engine = Engine::new().unwrap();
    let fixtures = common::resolution_fixtures();
    assert!(fixtures.len() >= 20, "only {} fixtures", fixtures.len());
    for dir in fixtures {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let (extracts, from_fresh) = fresh(&engine, &dir);
        let from_cache = cached(&engine, &dir, &extracts);
        assert_eq!(
            from_fresh, from_cache,
            "{name}: cached answers differ from fresh"
        );
        assert_eq!(
            from_fresh.health.status,
            RunStatus::Clean,
            "{name}: {:?}",
            from_fresh.health
        );
        assert_eq!(from_fresh.health.files as usize, extracts.len(), "{name}");
        // A fixture has typed answers exactly when the reference engine's recorded
        // answers for it include calls (engine/tests/fixtures/resolve/*/expected.tsv).
        let recorded = std::fs::read_to_string(dir.join("expected.tsv")).unwrap();
        let reference_answers = recorded
            .lines()
            .any(|l| l.starts_with("CALLS\t") || l.starts_with("CALL_REFERENCE\t"));
        assert_eq!(
            !from_fresh.resolutions.is_empty(),
            reference_answers,
            "{name}: typed answers {} where the reference has {}",
            from_fresh.resolutions.len(),
            if reference_answers { "some" } else { "none" }
        );
        assert_eq!(
            engine.live_handles(),
            0,
            "{name}: engine objects left alive"
        );
    }
}

#[test]
fn an_answer_carries_its_site_and_both_strategies() {
    let engine = Engine::new().unwrap();
    let dir = common::fixtures().join("resolve/python_cross_file_calls");
    let (_, run) = fresh(&engine, &dir);
    let call = run
        .resolutions
        .iter()
        .find(|r| r.site_ref.rel_path == "app/service.py" && r.target_qn == "app.models.make_user")
        .expect("the cross-file call to make_user");
    assert_eq!(call.strategy, Strategy::LspTyped);
    assert_eq!(call.engine_strategy.as_deref(), Some("lsp_callable_alias"));
    assert_eq!(call.target_rel_path.as_deref(), Some("app/models.py"));
    assert_eq!(call.candidates, 1);
    assert_eq!(call.site.callee_text, "make_user");
    assert!(!call.site.is_reference);
    assert!(call.site.span.is_some());

    // The same callable passed as a value is a reference, resolved as one.
    let reference = run
        .resolutions
        .iter()
        .find(|r| r.site.is_reference && r.target_qn == "app.models.make_user")
        .expect("make_user passed to apply");
    assert!(reference.site.lexical.is_empty() || !reference.site.typed_only);

    // A built-in resolves to no file of the project.
    let builtin = run
        .resolutions
        .iter()
        .find(|r| r.target_qn == "builtins.len")
        .expect("len");
    assert_eq!(builtin.target_rel_path, None);
}

#[test]
fn lexical_facts_reach_the_answers_sites() {
    let engine = Engine::new().unwrap();
    let dir = common::fixtures().join("resolve/python_unresolved_member_call");
    let (extracts, _) = fresh(&engine, &dir);
    let unresolved = extracts
        .iter()
        .flat_map(|e| &e.calls)
        .filter(|c| c.lexical.contains(LexicalFacts::UNRESOLVED_MEMBER))
        .count();
    assert!(
        unresolved > 0,
        "no call marked as an unresolved member call"
    );
}

#[test]
fn every_strategy_round_trips_through_its_name() {
    for s in Strategy::ALL {
        assert_eq!(Strategy::parse(s.as_str()), Some(s));
    }
    assert_eq!(Strategy::parse("lsp_callable_alias"), None);
}

#[test]
fn a_source_of_another_length_is_rejected() {
    let engine = Engine::new().unwrap();
    let extract = engine
        .extract("python", "a.py", b"def f():\n    return 1\n")
        .unwrap();
    let mut resolver = ProjectResolver::new(&engine).unwrap();
    let err = resolver
        .add_file(&extract, b"def f():\n    return 12\n")
        .unwrap_err();
    assert_eq!(
        err,
        EngineError::SourceMismatch {
            rel_path: "a.py".into(),
            difference: SourceDifference::Length {
                extracted: 22,
                given: 23
            },
        }
    );
    // The project is unusable after an error, and says why.
    assert_eq!(resolver.run().unwrap_err(), err);
    assert_eq!(engine.live_handles(), 0);
}

#[test]
fn same_length_different_source_is_rejected() {
    // Two sources of exactly the same length that differ in content. Resolving the
    // first's extraction with the second's tree would mix two files' facts, so the
    // second is refused, and refused for its content, not its length.
    let first: &[u8] = b"def f():\n    return g()\n";
    let second: &[u8] = b"def f():\n    return h()\n";
    assert_eq!(first.len(), second.len());
    assert_ne!(first, second);

    let engine = Engine::new().unwrap();
    let extract = engine.extract("python", "a.py", first).unwrap();
    let mut resolver = ProjectResolver::new(&engine).unwrap();
    let err = resolver.add_file(&extract, second).unwrap_err();
    assert_eq!(
        err,
        EngineError::SourceMismatch {
            rel_path: "a.py".into(),
            difference: SourceDifference::Content,
        }
    );
    let message = err.to_string();
    assert!(message.contains("same length"), "{message}");
    assert!(
        !message.contains("return"),
        "the message shows source: {message}"
    );
    drop(resolver);
    assert_eq!(engine.live_handles(), 0);

    // The source it was taken from is accepted.
    let mut resolver = ProjectResolver::new(&engine).unwrap();
    resolver.add_file(&extract, first).unwrap();
    assert!(resolver.run().unwrap().health.is_clean());
}

#[test]
fn the_source_digest_is_sha256_of_the_extracted_bytes() {
    // Known vectors (FIPS 180-2): the digest is SHA-256 over exactly the bytes handed
    // to the engine, the same in every extraction and every process.
    assert_eq!(
        SourceDigest::of(b"").to_string(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        SourceDigest::of(b"abc").to_string(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let engine = Engine::new().unwrap();
    let source = b"x = 1\n";
    let extract = engine.extract("python", "a.py", source).unwrap();
    assert_eq!(extract.source_digest, SourceDigest::of(source));
    assert_eq!(extract.source_len, source.len() as u64);
    let mut resolver = ProjectResolver::new(&engine).unwrap();
    let fresh = resolver.extract_and_add("python", "a.py", source).unwrap();
    assert_eq!(fresh.source_digest, extract.source_digest);
}
