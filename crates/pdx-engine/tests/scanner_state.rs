//! Grammar scanners keep their state within the parser's buffer and never end the
//! process, in every language (docs/plan/ISSUES.md, issue 91; decision 60).
//!
//! The parser keeps an external scanner's state between tokens in a 1,024-byte buffer.
//! YAML's scanner wrote past it from 254 nested sequences (510 bytes of `- - - x`
//! crashed the process), Markdown's copied its open blocks past it, and Kotlin's
//! called `abort()` at 1,025 open string templates; it all happened during the parse,
//! before the depth ceiling (issue 89). Each scanner now holds what the buffer holds and
//! refuses the token that would need more, so the parse recovers. Each source here
//! extracts, the same on every run; the exact bounds are tested on the scanners
//! themselves (`engine/tests/scanner_state.c`). Nothing here sets a variable.

use std::fmt::Write as _;

use pdx_engine::{Engine, FileExtract, FileStatus};

fn extract(language: &str, rel_path: &str, source: &str) -> FileExtract {
    Engine::new()
        .unwrap()
        .extract(language, rel_path, source.as_bytes())
        .unwrap()
}

fn yaml_sequences(levels: usize) -> String {
    format!("{}x\n", "- ".repeat(levels))
}

fn yaml_mappings(levels: usize) -> String {
    let mut s = String::new();
    for i in 0..levels - 1 {
        writeln!(s, "{}k{i}:", "  ".repeat(i)).unwrap();
    }
    writeln!(s, "{}v: 1", "  ".repeat(levels - 1)).unwrap();
    s
}

fn markdown_quotes(levels: usize) -> String {
    format!("{} text\n", ">".repeat(levels))
}

fn markdown_lists(levels: usize) -> String {
    let mut s = String::new();
    for i in 0..levels {
        writeln!(s, "{}- item", "  ".repeat(i)).unwrap();
    }
    s
}

fn kotlin_templates(levels: usize) -> String {
    format!(
        "fun m() {{ val s = {}1{} }}\n",
        "\"${".repeat(levels),
        "}\"".repeat(levels)
    )
}

fn python_blocks(levels: usize) -> String {
    let mut s = String::from("def f(x):\n");
    for i in 1..=levels {
        let line = if i == levels { "return x" } else { "if x:" };
        writeln!(s, "{}{line}", " ".repeat(i)).unwrap();
    }
    s
}

#[test]
fn the_old_crash_is_a_partial_parse() {
    // 510 bytes: 254 compact sequences, one more than the scanner's state holds.
    let source = yaml_sequences(254);
    assert_eq!(source.len(), 510);
    let first = extract("yaml", "config/seq.yaml", &source);
    assert_eq!(first.status, FileStatus::Partial);
    assert_eq!(first.extraction_lost, 0);
    assert_eq!(extract("yaml", "config/seq.yaml", &source), first);
}

/// A language, a path, a source nested `n` deep, and the depths to try.
type Shape = (
    &'static str,
    &'static str,
    fn(usize) -> String,
    &'static [usize],
);

#[test]
fn each_scanner_extracts_either_side_of_its_ceiling_the_same_on_every_run() {
    let shapes: [Shape; 6] = [
        (
            "yaml",
            "config/seq.yaml",
            yaml_sequences,
            &[252, 253, 254, 400],
        ),
        (
            "yaml",
            "config/deep.yaml",
            yaml_mappings,
            &[252, 253, 254, 400],
        ),
        (
            "markdown",
            "docs/deep.md",
            markdown_quotes,
            &[253, 254, 255, 300],
        ),
        (
            "markdown",
            "docs/lists.md",
            markdown_lists,
            &[253, 254, 255, 300],
        ),
        (
            "kotlin",
            "src/main/kotlin/Deep.kt",
            kotlin_templates,
            &[1023, 1024, 1025, 1100],
        ),
        (
            "python",
            "app/deep.py",
            python_blocks,
            &[510, 511, 512, 520],
        ),
    ];
    for (language, path, shape, levels) in shapes {
        for &n in levels {
            let source = shape(n);
            let first = extract(language, path, &source);
            assert_ne!(first.status, FileStatus::Failed, "{language} {n}");
            assert_eq!(
                extract(language, path, &source),
                first,
                "{language} {n}: not the same on every run"
            );
        }
    }
}
