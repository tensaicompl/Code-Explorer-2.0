//! Issue 114: typed resolution reads a Rust `use` in its lexical scope, and a macro in
//! the scope of its invocation. Here, what crosses the interface: the call the per-file
//! typed pass adds for a built-in's argument it answers, placed in its scope. The
//! answers themselves, a `macro_rules!` expansion's included (walked from a buffer of
//! its own, with no position to add a call at), are `engine/tests/rust_use_scopes.c`'s.

use pdx_engine::Engine;

const MACROS: &str = "macro_rules! fresh {\n    () => {\n        AtomicU32::new(0).load(std::sync::atomic::Ordering::SeqCst)\n    };\n}\n\npub fn f() -> u32 {\n    use std::sync::atomic::AtomicU32;\n    let c = AtomicU32::new(0);\n    assert_eq!(c.load(std::sync::atomic::Ordering::SeqCst), 0);\n    1\n}\n\npub fn g() -> u32 {\n    let c = AtomicU32::new(0);\n    assert_eq!(c.load(std::sync::atomic::Ordering::SeqCst), 0);\n    1\n}\n\npub fn h() -> u32 {\n    use std::sync::atomic::AtomicU32;\n    fresh!()\n}\n\npub fn k() -> u32 {\n    fresh!()\n}\n";

#[test]
fn a_macro_is_read_in_its_invocations_scope() {
    // `f` imports `AtomicU32` in its body, `g` does not. The per-file typed pass answers
    // the call to `load` inside `assert_eq!` where the import is in force, and adds the
    // answered call to the extraction, in `f`'s body's scope (the standard library's
    // methods are no answers a project reports).
    let engine = Engine::new().unwrap();
    let extract = engine
        .extract("rust", "src/mac.rs", MACROS.as_bytes())
        .unwrap();
    let load_calls = |caller: &str| {
        extract
            .calls
            .iter()
            .filter(|c| extract.text(&c.callee_text) == "load")
            .filter(|c| {
                c.caller
                    .is_some_and(|i| extract.definitions[i as usize].name == caller)
            })
            .map(|c| c.use_scope)
            .collect::<Vec<_>>()
    };
    // `f`'s body is the first scope after the file's.
    assert_eq!(load_calls("f"), [Some(1)], "the built-in's argument in `f`");
    assert_eq!(load_calls("g"), [], "the built-in's argument in `g`");
}
