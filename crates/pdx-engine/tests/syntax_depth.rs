//! Semantic extraction is admitted by syntax depth, the same on every platform
//! (docs/plan/ISSUES.md, issue 89; decision 58).
//!
//! A few KB nested 1,000 deep took up to a minute to extract, because extraction's work
//! grows faster than linearly with nesting in many places. Now a tree deeper than 512
//! (the root at depth 1) is not extracted at all: the extraction holds no fact, loses
//! work once, and is the same on every run. Depth is a count, never a time: nothing
//! here asserts how long anything takes, and nothing sets a variable.

use std::fmt::Write as _;

use pdx_engine::{Engine, FileExtract};

fn extract(language: &str, rel_path: &str, source: &str) -> FileExtract {
    Engine::new()
        .unwrap()
        .extract(language, rel_path, source.as_bytes())
        .unwrap()
}

/// A C function returning `f(0)` inside `k` parentheses: `k + 7` deep.
fn returning(k: usize) -> String {
    format!(
        "int g(void) {{ return {}f(0){}; }}\n",
        "(".repeat(k),
        ")".repeat(k)
    )
}

fn assert_refused(what: &str, extract: &FileExtract) {
    assert_eq!(extract.extraction_lost, 1, "{what}: work was not lost once");
    assert!(
        extract.definitions.is_empty()
            && extract.calls.is_empty()
            && extract.imports.is_empty()
            && extract.usages.is_empty(),
        "{what}: facts were extracted from a refused tree"
    );
    assert!(!extract.truncated, "{what}");
}

#[test]
fn depths_511_and_512_are_extracted_and_513_is_not() {
    for (depth, admitted) in [(511, true), (512, true), (513, false)] {
        let extract = extract("c", "src/deep.c", &returning(depth - 7));
        if admitted {
            assert_eq!(extract.extraction_lost, 0, "depth {depth}");
            assert!(
                extract.calls.iter().any(|c| c.callee_text == "f"),
                "depth {depth}: {:?}",
                extract.calls
            );
        } else {
            assert_refused("depth 513", &extract);
        }
    }
}

/// The issue's shapes, `n` levels deep.
fn shapes(n: usize) -> Vec<(&'static str, &'static str, String)> {
    let mut java = String::from("package p;\n");
    for i in 0..n {
        write!(java, "class C{i} {{ ").unwrap();
    }
    java.push_str("void m() { f(); }");
    java.push_str(&"}".repeat(n));
    java.push('\n');
    let mut js = String::new();
    for i in 0..n {
        write!(js, "function f{i}() {{ ").unwrap();
    }
    js.push_str("g();");
    js.push_str(&"}".repeat(n));
    js.push('\n');
    let py = format!(
        "def h():\n    v = a{}\n    return f(v)\n",
        ".b".repeat(2 * n)
    );
    let c = format!("int v = {}0{};\n", "f(".repeat(n), ")".repeat(n));
    vec![
        ("java", "src/main/java/p/C.java", java),
        ("javascript", "web/f.js", js),
        ("python", "app/h.py", py),
        ("c", "src/calls.c", c),
    ]
}

#[test]
fn nested_classes_functions_member_chains_and_calls_past_the_depth_are_refused() {
    // 300 levels of each is past 512 deep; 150 is not.
    for (language, path, source) in shapes(300) {
        let first = extract(language, path, &source);
        assert_refused(&format!("{language} nested 300"), &first);
        assert_eq!(
            extract(language, path, &source),
            first,
            "{language}: not the same on every run"
        );
    }
    for (language, path, source) in shapes(150) {
        let extract = extract(language, path, &source);
        assert!(
            !extract.calls.is_empty(),
            "{language} nested 150 was not extracted"
        );
    }
}

#[test]
fn a_thousand_levels_are_refused_by_structure() {
    // The issue's own reproducers, 1,000 deep, which took 45 to 56 seconds: refused
    // before any semantic pass. The assertion is what the extraction holds, not a time.
    for (language, path, source) in shapes(1000) {
        assert_refused(
            &format!("{language} nested 1000"),
            &extract(language, path, &source),
        );
    }
}
