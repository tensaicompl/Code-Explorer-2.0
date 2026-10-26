//! The parser's error recovery is bounded (issue 93, decision 74, patch 0038), on every
//! platform: a file whose recovery passes work linear in the bytes read is refused,
//! losing work once, with no fact, the same way every time; a file below it extracts.
//! The work itself is counted in the engine's `parse_recovery` test.

use std::fmt::Write as _;

use pdx_engine::Engine;

/// JavaScript after an unclosed parenthesis: one error node holds the rest of the file.
fn open_paren(bytes: usize) -> String {
    let mut source = String::from("let x = (\n");
    let mut n = 0;
    while source.len() < bytes {
        let _ = writeln!(source, "function f{n}() {{ g{n}(); }}");
        n += 1;
    }
    source
}

#[test]
fn parse_recovery_bound_is_deterministic() {
    let engine = Engine::new().unwrap();
    let source = open_paren(1 << 20);
    let first = engine
        .extract("javascript", "web/open.js", source.as_bytes())
        .unwrap();
    let second = engine
        .extract("javascript", "web/open.js", source.as_bytes())
        .unwrap();
    assert_eq!(first.extraction_lost, 1, "the parse was not stopped");
    assert!(first.definitions.is_empty() && first.calls.is_empty() && first.imports.is_empty());
    assert_eq!(first, second, "the same bytes, another result");
}

#[test]
fn parse_recovery_admits_what_stays_below_the_bound() {
    let engine = Engine::new().unwrap();
    // The same shape, short: its error node is narrow enough.
    let short = open_paren(16 * 1024);
    let e = engine
        .extract("javascript", "web/short.js", short.as_bytes())
        .unwrap();
    assert_eq!(e.extraction_lost, 0);
    assert!(!e.definitions.is_empty());
    // A linear error shape, 1 MiB.
    let mut classes = String::new();
    let mut n = 0;
    while classes.len() < 1 << 20 {
        let _ = writeln!(classes, "class C{n} {{ class I {{ fun m() {{ I() }} }} }}");
        n += 1;
    }
    let k = engine
        .extract("kotlin", "src/Nested.kt", classes.as_bytes())
        .unwrap();
    assert_eq!(k.extraction_lost, 0);
    assert!(!k.definitions.is_empty());
}
