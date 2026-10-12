//! A site's node-type path is complete or absent, and one file's paths are bounded, in
//! every language (docs/plan/ISSUES.md, issue 88; decision 57).
//!
//! Every call, possible callable reference and route binding stored the node types from
//! its owner down to itself, so calls nested inside one another stored entries
//! quadratic in their nesting. One budget per file, shared by every producer of a path,
//! now holds all of them together to 500,000 entries (and one to 1,024). The first
//! request past it is lost work, once; no path is computed after it, and every site
//! keeps its place in the extraction without one.
//!
//! The fixture is statements of 100 nested calls in one function: shallow enough that
//! nothing but the total is reached, deep enough that 60 statements pass it. C, Python,
//! Java and JavaScript, whose walks share one implementation, behave alike. Nothing here
//! sets a variable, and the expectations are the same on Linux, macOS and Windows.

use std::fmt::Write as _;

use pdx_engine::{Engine, FileExtract};

/// 60 statements of `f` called 100 deep, in one function of `language`.
fn source(language: &str) -> String {
    statements(language, 60)
}

/// `count` statements of `f` called 100 deep, in one function of `language`.
fn statements(language: &str, count: usize) -> String {
    let chain = format!("{}f(0){}", "f(".repeat(99), ")".repeat(99));
    let mut s = String::new();
    match language {
        "c" => {
            s.push_str("void work(void) {\n");
            for _ in 0..count {
                writeln!(s, "    {chain};").unwrap();
            }
            s.push_str("    shallow();\n}\n");
        }
        "python" => {
            s.push_str("def work():\n");
            for _ in 0..count {
                writeln!(s, "    {chain}").unwrap();
            }
            s.push_str("    shallow()\n");
        }
        "java" => {
            s.push_str("package p;\n\nclass Work {\n    void work() {\n");
            for _ in 0..count {
                writeln!(s, "        {chain};").unwrap();
            }
            s.push_str("        shallow();\n    }\n}\n");
        }
        "javascript" => {
            s.push_str("function work() {\n");
            for _ in 0..count {
                writeln!(s, "    {chain};").unwrap();
            }
            s.push_str("    shallow();\n}\n");
        }
        other => panic!("no fixture for {other}"),
    }
    s
}

fn extract(language: &str, rel_path: &str, source: &str) -> FileExtract {
    Engine::new()
        .unwrap()
        .extract(language, rel_path, source.as_bytes())
        .unwrap()
}

#[test]
fn a_files_paths_are_bounded_complete_or_absent_in_every_language() {
    for (language, path, owner, call) in [
        ("c", "src/work.c", "function_definition", "call_expression"),
        ("python", "app/work.py", "function_definition", "call"),
        (
            "java",
            "src/main/java/p/Work.java",
            "method_declaration",
            "method_invocation",
        ),
        (
            "javascript",
            "web/work.js",
            "function_declaration",
            "call_expression",
        ),
    ] {
        let extract = extract(language, path, &source(language));
        let calls: Vec<_> = extract
            .calls
            .iter()
            .filter(|c| !c.is_reference && c.span.is_some())
            .collect();
        // Every call is still there: only the path is absent.
        assert_eq!(
            calls.iter().filter(|c| c.callee_text == "f").count(),
            60 * 100,
            "{language}: calls were dropped with their paths"
        );
        assert!(
            calls.iter().any(|c| c.callee_text == "shallow"),
            "{language}"
        );
        let mut entries = 0usize;
        let mut first_absent = None;
        for (i, c) in calls.iter().enumerate() {
            if c.ast_path.is_empty() {
                first_absent.get_or_insert(i);
                continue;
            }
            assert!(
                first_absent.is_none(),
                "{language}: call {i} has a path after call {first_absent:?} had none"
            );
            // Complete: from the owner's node type down to the call's own.
            assert_eq!(
                c.ast_path.first().map(String::as_str),
                Some(owner),
                "{language}: {:?}",
                c.ast_path
            );
            assert_eq!(
                c.ast_path.last().map(String::as_str),
                Some(call),
                "{language}"
            );
            entries += c.ast_path.len();
        }
        let first_absent = first_absent.unwrap_or_else(|| panic!("{language}: nothing was cut"));
        assert!(
            first_absent > 0 && entries <= 500_000,
            "{language}: {entries} entries before call {first_absent}"
        );
        assert!(
            calls[first_absent..].iter().all(|c| c.ast_path.is_empty()),
            "{language}: a later site, the shallow one included, kept a path"
        );
        // Exactly one loss for the paths: the same function with 40 statements stays
        // under the budget, and whatever else it loses (Python's and JavaScript's type
        // evaluators have budgets of their own) this loses once more.
        let under = Engine::new()
            .unwrap()
            .extract(language, path, statements(language, 40).as_bytes())
            .unwrap();
        assert!(
            under
                .calls
                .iter()
                .filter(|c| c.span.is_some())
                .all(|c| !c.ast_path.is_empty()),
            "{language}: 40 statements already pass the budget"
        );
        assert_eq!(
            extract.extraction_lost,
            under.extraction_lost + 1,
            "{language}: the path budget lost work other than once"
        );
        // The same on every run.
        assert_eq!(extract_again(language, path), extract, "{language}");
    }
}

fn extract_again(language: &str, path: &str) -> FileExtract {
    extract(language, path, &source(language))
}

#[test]
fn a_file_under_the_budget_keeps_every_path() {
    // 40 statements spend 40 × 10,300 = 412,000 entries in C: every path is kept, and
    // nothing is lost.
    let chain = format!("{}f(0){}", "f(".repeat(99), ")".repeat(99));
    let mut s = String::from("void work(void) {\n");
    for _ in 0..40 {
        writeln!(s, "    {chain};").unwrap();
    }
    s.push_str("}\n");
    let extract = extract("c", "src/under.c", &s);
    assert_eq!(extract.extraction_lost, 0);
    let entries: usize = extract.calls.iter().map(|c| c.ast_path.len()).sum();
    assert_eq!(entries, 40 * (100 * 100 + 3 * 100));
    assert!(
        extract
            .calls
            .iter()
            .filter(|c| c.span.is_some())
            .all(|c| !c.ast_path.is_empty())
    );
}

#[test]
fn a_late_sites_path_is_the_one_its_owner_gives() {
    // A site added after the walk (here the Kotlin resolver's `getValue` for a delegated
    // property) gets its path once, from its owner. The path a nearest-function climb
    // used to give it first was always replaced before anything read it; it is no
    // longer computed, and the path that remains is exactly what engine 7 gave before
    // the budget (`e7865a0`).
    let source = "package p

class Expression {
    private val cachedHash by lazy { compute() }

    override fun hashCode(): Int = cachedHash

    fun compute(): Int = 7
}
";
    let extract = extract("kotlin", "src/main/kotlin/p/Expression.kt", source);
    let get_value = extract
        .calls
        .iter()
        .find(|c| c.callee_text == "getValue")
        .expect("the resolver adds the delegate's getValue");
    assert_eq!(
        get_value.ast_path,
        ["function_declaration", "function_body", "simple_identifier"]
    );
    assert_eq!(extract.extraction_lost, 0);
}
