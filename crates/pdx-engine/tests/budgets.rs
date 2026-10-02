//! What the engine's work budgets do to an extraction, and how the extraction says so.
//!
//! Work lost to a budget running out, or to an allocation failing, crosses the
//! interface as `FileExtract::extraction_lost`; a walk stopped by the node budget, as
//! `truncated` (docs/plan/ISSUES.md, issues 21, 25 and 26). Each budget is set in a
//! child process, so no other test sees it.
//!
//! Every switch of the engine's that changes what it extracts is named in
//! `EXTRACTION_SWITCHES`, and every other variable the engine reads is accounted for
//! here, so a refreshed engine with a new switch fails until it is classified.

mod common;

use std::collections::BTreeSet;
use std::path::Path;

use pdx_engine::{EXTRACTION_SWITCHES, Engine, NODE_BUDGET_ENV, extraction_switch_set};

/// TypeScript whose types the resolver evaluates while the file is extracted.
const TYPED: &[u8] = b"interface Item { name: string; qty: number }
class Store {
    private items: Map<string, Item> = new Map();
    add(item: Item): void { this.items.set(item.name, item); }
    get(name: string): Item | undefined { return this.items.get(name); }
    total(): number { let t = 0; for (const i of this.items.values()) { t += i.qty; } return t; }
}
export function build(): Store { const s = new Store(); s.add({ name: 'a', qty: 1 }); return s; }
";

#[test]
fn nothing_is_lost_or_truncated_normally() {
    assert_eq!(
        extraction_switch_set(),
        None,
        "the tests run without a switch set"
    );
    let engine = Engine::new().unwrap();
    for (rel_path, language, source) in common::source_files(&common::fixtures().join("smoke")) {
        let extract = engine.extract(language, &rel_path, &source).unwrap();
        assert_eq!(extract.extraction_lost, 0, "{rel_path}");
        assert!(!extract.truncated, "{rel_path}");
    }
    let extract = engine.extract("typescript", "store.ts", TYPED).unwrap();
    assert_eq!(extract.extraction_lost, 0);
}

#[test]
fn a_starved_budget_is_reported_as_lost_work() {
    // The TypeScript resolver's budget, cut to nothing.
    if common::rerun_in_child(
        "a_starved_budget_is_reported_as_lost_work",
        &[("PDX_ENGINE_TS_TYPE_BUDGET", "1")],
    ) {
        return;
    }
    let engine = Engine::new().unwrap();
    let extract = engine.extract("typescript", "store.ts", TYPED).unwrap();
    assert!(extract.extraction_lost > 0, "nothing reported lost");
    // The surface carries the same loss, so a run resolving the file counts it.
    assert!(!extract.truncated);
}

#[test]
fn the_node_budget_variable_is_the_one_the_engine_reads() {
    if common::rerun_in_child(
        "the_node_budget_variable_is_the_one_the_engine_reads",
        &[(NODE_BUDGET_ENV, "4")],
    ) {
        return;
    }
    assert_eq!(extraction_switch_set(), Some(NODE_BUDGET_ENV));
    let engine = Engine::new().unwrap();
    let extract = engine.extract("typescript", "store.ts", TYPED).unwrap();
    assert!(
        extract.truncated,
        "a four-node budget did not truncate the walk"
    );
}

#[test]
fn a_switch_is_set_by_any_value() {
    // Set at all, even empty or to what reads as off, it counts as set: whoever set it
    // meant something by it, and no cache may guess what.
    for name in EXTRACTION_SWITCHES {
        for value in ["", "0", "not a number"] {
            if common::rerun_in_child("a_switch_is_set_by_any_value", &[(name, value)]) {
                continue;
            }
            assert!(extraction_switch_set().is_some());
            return;
        }
    }
}

/// The variables the engine reads that leave what it extracts as it is, and why.
const NEUTRAL: [(&str, &str); 9] = [
    (
        "PDX_ENGINE_LOG_LEVEL",
        "log output, which the interface silences",
    ),
    (
        "PDX_ENGINE_LOG_FORMAT",
        "log output, which the interface silences",
    ),
    ("PDX_ENGINE_LSP_DEBUG", "debugging output"),
    ("PDX_ENGINE_LSP_KOTLIN_AST", "debugging output"),
    (
        "PDX_ENGINE_THREAD_STACK_MB",
        "the stack size of the engine's threads",
    ),
    (
        "PDX_ENGINE_CACHE_DIR",
        "where the engine keeps files of its own",
    ),
    (
        "PDX_ENGINE_CACHE_DIR_MISSING",
        "a sentinel value, not a variable",
    ),
    (
        "PDX_ENGINE_MI_THREAD_DONE",
        "the allocator's per-thread clean-up",
    ),
    (
        "PDX_ENGINE_INDEX_MARKER_FILE",
        "a progress log the engine appends to",
    ),
];

/// Named only in a comment: read by nothing.
const UNREAD: [&str; 1] = ["PDX_ENGINE_MEM_BUDGET_MB"];

#[test]
fn every_engine_variable_is_classified() {
    fn walk(dir: &Path, out: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "c" || e == "h") {
                let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();
                let mut rest = text.as_str();
                while let Some(at) = rest.find("\"PDX_ENGINE_") {
                    let name: String = rest[at + 1..]
                        .chars()
                        .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
                        .collect();
                    out.insert(name);
                    rest = &rest[at + 1..];
                }
            }
        }
    }
    let engine = common::fixtures().join("../..");
    let mut names = BTreeSet::new();
    for dir in ["src", "api", "include"] {
        walk(&engine.join(dir), &mut names);
    }
    assert!(names.contains(NODE_BUDGET_ENV), "the scan found nothing");
    let unclassified: Vec<&String> = names
        .iter()
        .filter(|n| {
            !n.starts_with("PDX_ENGINE_TEST_")
                && !EXTRACTION_SWITCHES.contains(&n.as_str())
                && !NEUTRAL.iter().any(|(neutral, _)| neutral == n)
                && !UNREAD.contains(&n.as_str())
        })
        .collect();
    assert!(
        unclassified.is_empty(),
        "engine variables neither extraction switches nor neutral: {unclassified:?}"
    );
    // And every name classified is one the engine has.
    for name in EXTRACTION_SWITCHES
        .iter()
        .chain(NEUTRAL.iter().map(|(n, _)| n))
        .chain(UNREAD.iter())
    {
        assert!(names.contains(*name), "{name} is not in the engine");
    }
}
