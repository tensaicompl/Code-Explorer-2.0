//! Issue 109: typed resolution answers `Type::method` with a trait impl's method only
//! when the impl is proven to apply, from the header extraction records for it. Here,
//! through the safe interface: the same answers from the engine's own results and from
//! surfaces as a cache returns them (the header must cross the surface for the second
//! to prove anything), the header in the surface, and a surface of the version before
//! the header refused, never read as this one. The answers in every shape, and the
//! headers themselves, are `engine/tests/rust_impl_bounds.c`'s; Stage 3's,
//! `crates/pdx-core/tests/rust_impl_bounds.rs`'s.

mod common;

use pdx_engine::{Engine, ExtractRecord, FileExtract, ProjectResolution, ProjectResolver};

const FILES: &[(&str, &str)] = &[
    (
        "src/make.rs",
        "pub trait Show {}\n\npub struct A;\npub struct B;\n\nimpl Show for A {}\n\npub trait Make {\n    fn make() -> u8;\n}\n\npub struct W<X>(pub X);\n\nimpl<X: Show> Make for W<X> {\n    fn make() -> u8 {\n        1\n    }\n}\n\npub fn proven() -> u8 {\n    W::<A>::make()\n}\n\npub fn unproven() -> u8 {\n    W::<B>::make()\n}\n",
    ),
    // The engine differential's site (axum-core/src/extract/mod.rs:129) and the issue's
    // (axum-extra/src/extract/with_rejection.rs:196), their shapes.
    (
        "src/extract.rs",
        "pub struct ViaRequest;\npub struct ViaParts;\n\npub trait FromRequestParts<S> {\n    fn from_request_parts(parts: u32, state: &S) -> Self;\n}\n\npub trait FromRequest<S, M = ViaRequest> {\n    fn from_request(req: u32, state: &S) -> Self;\n}\n\nimpl<S, T> FromRequest<S, ViaParts> for T\nwhere\n    S: Send + Sync,\n    T: FromRequestParts<S>,\n{\n    fn from_request(req: u32, state: &S) -> Self {\n        T::from_request_parts(req, state)\n    }\n}\n\nimpl<S, T> FromRequest<S> for Option<T>\nwhere\n    T: FromRequest<S>,\n    S: Send + Sync,\n{\n    fn from_request(req: u32, state: &S) -> Self {\n        Some(T::from_request(req, state))\n    }\n}\n\npub struct WithRejection<E>(pub E);\n\nimpl<E, S> FromRequest<S> for WithRejection<E>\nwhere\n    E: FromRequest<S>,\n{\n    fn from_request(req: u32, state: &S) -> Self {\n        WithRejection(E::from_request(req, state))\n    }\n}\n\npub struct Extractor;\n\nimpl<S> FromRequestParts<S> for Extractor {\n    fn from_request_parts(_parts: u32, _state: &S) -> Self {\n        Extractor\n    }\n}\n\npub fn issue() -> WithRejection<Extractor> {\n    WithRejection::<Extractor>::from_request(1, &())\n}\n",
    ),
];

/// Every file extracted by the resolver's engine and resolved through its own result.
fn fresh(engine: &Engine) -> (Vec<FileExtract>, ProjectResolution) {
    let mut resolver = ProjectResolver::new(engine).unwrap();
    let extracts = FILES
        .iter()
        .map(|(rel, source)| {
            resolver
                .extract_and_add("rust", rel, source.as_bytes())
                .unwrap()
        })
        .collect();
    (extracts, resolver.run().unwrap())
}

/// The same extractions as a cache returns them, resolved through their surfaces.
fn cached(engine: &Engine, extracts: &[FileExtract]) -> ProjectResolution {
    let mut resolver = ProjectResolver::new(engine).unwrap();
    for (extract, (rel, source)) in extracts.iter().zip(FILES) {
        assert_eq!(&extract.rel_path, rel);
        resolver
            .add_file(&common::round_trip(extract), source.as_bytes())
            .unwrap();
    }
    resolver.run().unwrap()
}

/// The targets the calls of `path` spelled `callee` are answered with.
fn answers(
    extracts: &[FileExtract],
    run: &ProjectResolution,
    path: &str,
    callee: &str,
) -> Vec<String> {
    let extract = extracts
        .iter()
        .find(|e| e.rel_path == path)
        .expect("the file");
    run.resolutions
        .iter()
        .filter(|r| r.site_ref.rel_path == path && extract.text(&r.site.callee_text) == callee)
        .map(|r| r.target_qn.clone())
        .collect()
}

#[test]
fn impl_bounds_are_proven_fresh_and_cached_alike() {
    let engine = Engine::new().unwrap();
    let (extracts, from_fresh) = fresh(&engine);
    let from_cache = cached(&engine, &extracts);
    assert_eq!(from_fresh, from_cache, "cached answers differ from fresh");
    for run in [&from_fresh, &from_cache] {
        // T2: `X: Show` is proven for `A` by its impl.
        let proven = answers(&extracts, run, "src/make.rs", "W::<A>::make");
        assert_eq!(proven.len(), 1, "{proven:?}");
        assert!(proven[0].ends_with(".make.W.make"), "{proven:?}");
        // T4: `B` has no impl of `Show`.
        assert_eq!(
            answers(&extracts, run, "src/make.rs", "W::<B>::make"),
            Vec::<String>::new()
        );
        // The issue: `Extractor` implements only `FromRequestParts`; `WithRejection`'s
        // own `from_request` is not the call's target.
        assert_eq!(
            answers(
                &extracts,
                run,
                "src/extract.rs",
                "WithRejection::<Extractor>::from_request"
            ),
            Vec::<String>::new()
        );
        // The differential's: an impl's `T` is not the blanket impl's by its name.
        assert_eq!(
            answers(&extracts, run, "src/extract.rs", "T::from_request"),
            Vec::<String>::new()
        );
    }
}

#[test]
fn a_trait_impls_header_crosses_the_surface() {
    let engine = Engine::new().unwrap();
    let make = engine
        .extract("rust", "src/make.rs", FILES[0].1.as_bytes())
        .unwrap();
    let contains = |needle: &[u8]| {
        make.surface
            .as_bytes()
            .windows(needle.len())
            .any(|w| w == needle)
    };
    assert!(extract_starts_with(&make, b"{\"v\":6,"), "surface 6");
    // `impl Show for A`'s relation, and `impl<X: Show> Make for W<X>`'s relation and
    // method, carry their headers; an inherent or free definition none.
    assert!(contains(b"\"impl_bounds\":[\"s:plain\"]"));
    assert!(contains(
        b"\"impl_bounds\":[\"s:plain\",\"p:X\",\"a:X\",\"w:X\",\"b:Show\"]"
    ));
    assert!(contains(b"\"impl_bounds\":null"));
}

fn extract_starts_with(extract: &FileExtract, prefix: &[u8]) -> bool {
    extract.surface.as_bytes().starts_with(prefix)
}

#[test]
fn a_surface_of_version_five_is_refused() {
    // Surface 5 had no headers: an impl read from it would have none, and could never be
    // told from one with no requirement. A surface of another version is refused whole,
    // never read as this one.
    let engine = Engine::new().unwrap();
    let extract = engine
        .extract("rust", "src/make.rs", FILES[0].1.as_bytes())
        .unwrap();
    let mut bytes = common::record_bytes(&extract);
    let at = bytes
        .windows(7)
        .position(|w| w == b"{\"v\":6,")
        .expect("the record holds the surface");
    bytes[at + 5] = b'5';
    let record: ExtractRecord = postcard::from_bytes(&bytes).expect("the record decodes");
    let older = record
        .bind(extract.shared_source())
        .expect("the record binds");
    let mut resolver = ProjectResolver::new(&engine).unwrap();
    assert!(
        resolver.add_file(&older, FILES[0].1.as_bytes()).is_err(),
        "a surface of version 5 was accepted"
    );
}
