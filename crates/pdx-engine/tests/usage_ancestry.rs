//! A usage's ancestors are answered from the walk's own path, in every language
//! (docs/plan/ISSUES.md, issue 90; decision 59).
//!
//! Within the admitted depth, classifying a usage used to climb to the root with
//! `ts_node_parent`, itself a descent from the root, for every usage: a 400-link Python
//! member chain took 3 s, eight of them 25 s, whatever the machine made of that. The
//! walk now answers each question from its own frames. The issue's fixtures extract
//! whole, with every fact they had, and answering from the frames gives exactly what
//! climbing gave: a child process extracts the same sources with the test switch
//! `PDX_ENGINE_TEST_CLIMB_ANCESTRY`, which answers by climbing. Nothing here asserts a
//! time; the work is counted by the engine's own test (`engine/tests/usage_ancestry.c`).

mod common;

use std::fmt::Write as _;
use std::path::PathBuf;

use pdx_engine::{DefinitionKind, Engine, FileExtract};

const CLIMB: &str = "PDX_ENGINE_TEST_CLIMB_ANCESTRY";
const OUT: &str = "PDX_USAGE_ANCESTRY_OUT";

fn extract(language: &str, rel_path: &str, source: &str) -> FileExtract {
    Engine::new()
        .unwrap()
        .extract(language, rel_path, source.as_bytes())
        .unwrap()
}

/// A Python function holding `count` member chains of `links` links.
fn python_chains(links: usize, count: usize) -> String {
    let mut s = String::from("def h():\n");
    for _ in 0..count {
        writeln!(s, "    v = a{}", ".b".repeat(links)).unwrap();
    }
    s.push_str("    return f(v)\n");
    s
}

fn java_classes(n: usize) -> String {
    let mut s = String::from("package p;\n");
    for i in 0..n {
        write!(s, "class C{i} {{ ").unwrap();
    }
    s.push_str("void m() { f(); }");
    s.push_str(&"}".repeat(n));
    s.push('\n');
    s
}

fn js_functions(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        write!(s, "function f{i}(a{i}) {{ var v{i} = a{i}; ").unwrap();
    }
    s.push_str("g(a0);");
    s.push_str(&"}".repeat(n));
    s.push('\n');
    s
}

/// The issue's shapes, smaller (climbing them at full size is the slowness the issue is
/// about), and the rules the frames serve in the languages that have them.
fn sources() -> Vec<(&'static str, &'static str, String)> {
    vec![
        ("python", "app/h.py", python_chains(100, 1)),
        ("python", "app/chains.py", python_chains(100, 4)),
        ("java", "src/main/java/p/C.java", java_classes(60)),
        ("javascript", "web/f.js", js_functions(60)),
        (
            "python",
            "app/rules.py",
            "import json as j\n\
             from collections import OrderedDict as OD\n\
             counter = 0\n\
             def outer(run, size=len, key=sorted):\n    \
                 global counter\n    \
                 total = 0\n    \
                 def inner(value=total, fn=lambda x=size: x):\n        \
                     nonlocal total\n        \
                     total = value + fn()\n    \
                 run(inner, key=j.dumps, value=OD())\n    \
                 run(obj.method)\n    \
                 return counter\n"
                .to_string(),
        ),
        (
            "javascript",
            "web/rules.js",
            "import def, { a as b } from './m';\n\
             function f(x) { var h = x; if (x) { let s = h; var late = s; } register(o.p, b); return def(late); }\n"
                .to_string(),
        ),
        (
            "rust",
            "src/rules.rs",
            "use std::collections::{HashMap, HashSet as Set, self};\n\
             fn run(map: HashMap<i32, i32>, s: Set<i32>) -> usize { apply(inner::g, collections::BTreeMap::new); map.len() + s.len() }\n"
                .to_string(),
        ),
    ]
}

/// Every extraction of the sources, as text.
fn extractions() -> String {
    let mut out = String::new();
    for (language, path, source) in sources() {
        writeln!(out, "{:?}", extract(language, path, &source)).unwrap();
    }
    out
}

#[test]
fn the_issues_fixtures_extract_whole() {
    // A 400-link member chain: every link a usage, nothing lost.
    let chain = extract("python", "app/h.py", &python_chains(400, 1));
    assert_eq!(chain.extraction_lost, 0);
    assert!(!chain.truncated);
    assert!(
        chain
            .calls
            .iter()
            .any(|c| chain.text(&c.callee_text) == "f")
    );
    let longest = format!("a{}", ".b".repeat(400));
    assert!(chain.usages.iter().any(|u| chain.text(&u.name) == longest));
    assert!(chain.usages.iter().any(|u| chain.text(&u.name) == "a"));
    // Eight of them in one function: eight times the chain's usages.
    let chains = extract("python", "app/chains.py", &python_chains(400, 8));
    assert_eq!(chains.extraction_lost, 0);
    let one = chain
        .usages
        .iter()
        .filter(|u| chain.text(&u.name).starts_with('a'))
        .count();
    assert_eq!(
        chains
            .usages
            .iter()
            .filter(|u| chains.text(&u.name).starts_with('a'))
            .count(),
        8 * one
    );
    // 250 nested Java classes: every class, and the call in the innermost method.
    let java = extract("java", "src/main/java/p/C.java", &java_classes(250));
    assert_eq!(java.extraction_lost, 0);
    assert_eq!(
        java.definitions
            .iter()
            .filter(|d| d.kind == DefinitionKind::Class)
            .count(),
        250
    );
    assert!(java.calls.iter().any(|c| java.text(&c.callee_text) == "f"));
    // 240 nested JavaScript functions: every function, and the innermost call.
    let js = extract("javascript", "web/f.js", &js_functions(240));
    assert_eq!(js.extraction_lost, 0);
    assert_eq!(
        js.definitions
            .iter()
            .filter(|d| d.kind == DefinitionKind::Function)
            .count(),
        240
    );
    assert!(js.calls.iter().any(|c| js.text(&c.callee_text) == "g"));
    // The same on every run.
    assert_eq!(extract("python", "app/h.py", &python_chains(400, 1)), chain);
}

#[test]
fn the_frames_answer_what_climbing_answered() {
    const TEST: &str = "the_frames_answer_what_climbing_answered";
    if let Some(out) = std::env::var_os(OUT) {
        // A child: extract with every question answered by climbing.
        std::fs::write(PathBuf::from(out), extractions()).unwrap();
        return;
    }
    let own = extractions();
    let dir = std::env::temp_dir().join(format!("pdx-usage-ancestry-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("climbing");
    assert!(common::rerun_in_child(
        TEST,
        &[(CLIMB, "1"), (OUT, out.to_str().unwrap())]
    ));
    assert_eq!(
        std::fs::read_to_string(&out).unwrap(),
        own,
        "the frames and the climbs disagree"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
