//! Rust's `impl Trait for Type` relations cross the safe boundary whole
//! (docs/plan/ISSUES.md, issue 42): in the engine's order and spelling, an empty block's
//! included, through a fresh extraction, a serialised one and a rebuilt one.

use std::path::Path;

use pdx_engine::{Engine, FileExtract, ImplTrait, ProjectResolver};

fn extract(engine: &Engine, rel_path: &str, source: &[u8]) -> FileExtract {
    engine
        .extract("rust", rel_path, source)
        .unwrap_or_else(|e| panic!("{rel_path}: {e}"))
}

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../engine/tests/fixtures/bases")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn relation(trait_name: &str, struct_name: &str, struct_qn: &str) -> ImplTrait {
    ImplTrait {
        trait_name: trait_name.to_owned(),
        struct_name: struct_name.to_owned(),
        struct_qn: struct_qn.to_owned(),
    }
}

#[test]
fn impl_traits_cross_the_safe_boundary() {
    let engine = Engine::new().unwrap();
    // None: a struct with an inherent impl and no trait.
    let none = extract(
        &engine,
        "src/plain.rs",
        b"pub struct Plain;\nimpl Plain {\n    pub fn new() -> Self {\n        Plain\n    }\n}\n",
    );
    assert!(none.impl_traits.is_empty());

    // One, from an empty block: no method stands for it.
    let one = extract(&engine, "src/one_impl.rs", &fixture("one_impl.rs"));
    assert_eq!(
        one.impl_traits,
        [relation("Marker", "Tagged", "src.one_impl.Tagged")]
    );
    assert!(
        !one.definitions.iter().any(|d| d
            .parent
            .is_some_and(|p| one.definitions[p as usize].name == "Tagged")),
        "the empty block defines nothing"
    );

    // Several, in the source's order, with a trait named through its module kept
    // whole and one's type arguments left off.
    let several = extract(&engine, "src/traits.rs", &fixture("traits.rs"));
    assert_eq!(
        several.impl_traits,
        [
            relation("Shape", "Square", "src.traits.Square"),
            relation("Named", "Square", "src.traits.Square"),
            relation("fmt::Display", "Square", "src.traits.Square"),
            relation("From", "Square", "src.traits.Square"),
        ]
    );

    // Only Rust records them.
    let java = engine
        .extract(
            "java",
            "src/Square.java",
            b"class Square implements Shape {}\n",
        )
        .unwrap();
    assert!(java.impl_traits.is_empty());
}

#[test]
fn impl_traits_survive_serialisation() {
    let engine = Engine::new().unwrap();
    let fresh = extract(&engine, "src/traits.rs", &fixture("traits.rs"));
    let again: FileExtract = postcard::from_bytes(&postcard::to_stdvec(&fresh).unwrap()).unwrap();
    assert_eq!(again, fresh);
    assert_eq!(again.impl_traits.len(), 4);
}

#[test]
fn a_cached_extraction_with_impl_traits_resolves() {
    // Resolving from a cached extraction rebuilds the engine's result from it, its
    // relations included; the answers are the fresh ones.
    let engine = Engine::new().unwrap();
    let source = fixture("traits.rs");
    let fresh_extract = extract(&engine, "src/traits.rs", &source);
    let mut fresh = ProjectResolver::new(&engine).unwrap();
    fresh
        .extract_and_add("rust", "src/traits.rs", &source)
        .unwrap();
    let fresh = fresh.run().unwrap();

    let cached_extract: FileExtract =
        postcard::from_bytes(&postcard::to_stdvec(&fresh_extract).unwrap()).unwrap();
    let mut cached = ProjectResolver::new(&engine).unwrap();
    cached.add_file(&cached_extract, &source).unwrap();
    assert_eq!(cached.run().unwrap(), fresh);
}
