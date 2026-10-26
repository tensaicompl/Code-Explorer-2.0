//! Which definition owns a site inside a nested callable (docs/plan/ISSUES.md, issue 75;
//! decision 47).
//!
//! A function boundary owns the sites under it only when that exact declaration
//! occurrence is one of the file's extracted definitions. A nested callable the
//! extractor does not represent keeps its own lexical scope (its parameters and locals
//! bind inside it) but owns nothing: its sites belong to the nearest represented
//! enclosing definition, and their node-type path starts at that definition's node and
//! runs through the nested syntax. At file scope a site still has no owner.

mod common;

use pdx_engine::{Call, DefinitionKind, Engine, FileExtract, LexicalFacts, ProjectResolver};

fn extract(language: &str, rel_path: &str, source: &str) -> FileExtract {
    Engine::new()
        .unwrap()
        .extract(language, rel_path, source.as_bytes())
        .unwrap_or_else(|e| panic!("{rel_path}: {e}"))
}

/// The calls (not references) to `callee`, in source order.
fn calls<'e>(e: &'e FileExtract, callee: &str, reference: bool) -> Vec<&'e Call> {
    let mut found: Vec<&Call> = e
        .calls
        .iter()
        .filter(|c| c.is_reference == reference && e.text(&c.callee_text) == callee)
        .collect();
    found.sort_by_key(|c| c.span.map(|s| s.start_byte));
    found
}

/// The qualified name of the definition a call names as its caller, if any.
fn caller(e: &FileExtract, call: &Call) -> Option<String> {
    call.caller.map(|i| e.qualified_name(i as usize))
}

fn path(call: &Call) -> Vec<&str> {
    call.ast_path.iter().copied().collect()
}

fn defined(e: &FileExtract, name: &str) -> usize {
    e.definitions.iter().filter(|d| d.name == name).count()
}

const NESTED: &str = "def helper():
    return 1


def outer():
    def inner():
        helper()
        register(helper)
    return inner


helper()
";

#[test]
fn unmodelled_nested_call_is_owned_by_the_outer_definition() {
    let e = extract("python", "pkg/nested.py", NESTED);
    // The premise: the extractor represents `outer`, not the `inner` nested in it.
    assert_eq!(defined(&e, "outer"), 1);
    assert_eq!(defined(&e, "inner"), 0);
    let found = calls(&e, "helper", false);
    assert_eq!(found.len(), 2, "the nested call and the module-level call");
    assert_eq!(caller(&e, found[0]).as_deref(), Some("pkg.nested.outer"));
}

#[test]
fn site_path_starts_at_the_modelled_owner() {
    let e = extract("python", "pkg/nested.py", NESTED);
    // From outer's node, through inner's definition, to the call.
    assert_eq!(
        path(calls(&e, "helper", false)[0]),
        [
            "function_definition",
            "block",
            "function_definition",
            "block",
            "expression_statement",
            "call"
        ]
    );
}

#[test]
fn callable_reference_in_nested_callable_is_owned_by_the_outer_definition() {
    let e = extract("python", "pkg/nested.py", NESTED);
    let references = calls(&e, "helper", true);
    assert_eq!(references.len(), 1);
    assert_eq!(
        caller(&e, references[0]).as_deref(),
        Some("pkg.nested.outer")
    );
    assert_eq!(
        path(references[0])[..3],
        ["function_definition", "block", "function_definition"]
    );
}

#[test]
fn module_level_call_stays_file_scoped() {
    let e = extract("python", "pkg/nested.py", NESTED);
    let module_call = calls(&e, "helper", false)[1];
    // No owner, or the file's own module definition: never a callable.
    if let Some(index) = module_call.caller {
        assert_eq!(
            e.definitions[index as usize].kind,
            DefinitionKind::Module,
            "a module-level call is owned by the file"
        );
    }
    assert_eq!(
        path(module_call),
        ["module", "expression_statement", "call"]
    );
}

const SAME_NAME: &str = "def helper():
    return 1


def outer():
    def helper():
        target()
    return helper


def target():
    return 2
";

#[test]
fn nested_same_name_does_not_borrow_a_top_level_occurrence() {
    // The nested `helper` has the qualified name of the top-level one; only its
    // declaration occurrence tells them apart.
    let e = extract("python", "pkg/same.py", SAME_NAME);
    assert_eq!(
        defined(&e, "helper"),
        1,
        "only the top-level helper is extracted"
    );
    let found = calls(&e, "target", false);
    assert_eq!(found.len(), 1);
    assert_eq!(caller(&e, found[0]).as_deref(), Some("pkg.same.outer"));
    assert_eq!(
        path(found[0])[..3],
        ["function_definition", "block", "function_definition"]
    );
}

const SHADOWING: &str = "def outer(run):
    def inner(run):
        run()
    return inner


def other():
    def inner2(job):
        job()
    job()
";

#[test]
fn nested_parameters_stay_in_the_nested_lexical_scope() {
    let e = extract("python", "pkg/shadow.py", SHADOWING);
    // `run` inside inner is inner's own parameter.
    let run = calls(&e, "run", false);
    assert_eq!(run.len(), 1);
    assert!(run[0].lexical.contains(LexicalFacts::LOCALLY_BOUND));
    assert_eq!(caller(&e, run[0]).as_deref(), Some("pkg.shadow.outer"));
    // `job` is inner2's parameter: bound inside inner2, and not after it closes.
    let job = calls(&e, "job", false);
    assert_eq!(job.len(), 2);
    assert!(job[0].lexical.contains(LexicalFacts::LOCALLY_BOUND));
    assert!(
        !job[1].lexical.contains(LexicalFacts::LOCALLY_BOUND),
        "inner2's parameter leaked into other"
    );
    assert_eq!(caller(&e, job[0]).as_deref(), Some("pkg.shadow.other"));
    assert_eq!(caller(&e, job[1]).as_deref(), Some("pkg.shadow.other"));
}

const SHAPES: &str = "def setup(f):
    def wrapper(self):
        self.check()
        return f(self)
    return wrapper


def build():
    class Local:
        def run(self):
            target()
    return Local


def target():
    return 1
";

#[test]
fn decorator_wrapper_and_local_class_methods_are_owned_by_the_outer_definition() {
    let e = extract("python", "pkg/shapes.py", SHAPES);
    assert_eq!(defined(&e, "wrapper"), 0);
    assert_eq!(defined(&e, "Local"), 0);
    assert_eq!(defined(&e, "run"), 0);
    assert_eq!(
        caller(&e, calls(&e, "self.check", false)[0]).as_deref(),
        Some("pkg.shapes.setup")
    );
    assert_eq!(
        caller(&e, calls(&e, "f", false)[0]).as_deref(),
        Some("pkg.shapes.setup")
    );
    let local = calls(&e, "target", false);
    assert_eq!(caller(&e, local[0]).as_deref(), Some("pkg.shapes.build"));
    assert_eq!(
        path(local[0])[..3],
        ["function_definition", "block", "class_definition"]
    );
}

const MODELLED_NESTED: &str = "export function outer(): void {
  function inner(): void {
    target();
  }
  inner();
}

function target(): void {}
";

#[test]
fn modelled_nested_function_keeps_its_own_ownership() {
    // TypeScript's extractor represents named nested functions, so `inner` owns the
    // call inside it.
    let e = extract("typescript", "src/nested.ts", MODELLED_NESTED);
    assert_eq!(defined(&e, "inner"), 1);
    let found = calls(&e, "target", false);
    assert_eq!(found.len(), 1);
    let owner = caller(&e, found[0]).expect("an owner");
    assert_eq!(owner.rsplit('.').next(), Some("inner"), "{owner}");
    assert_eq!(path(found[0])[0], "function_declaration");
    assert_eq!(
        path(found[0])
            .iter()
            .filter(|t| **t == "function_declaration")
            .count(),
        1,
        "the path starts at inner, not at outer"
    );
    let inner_call = calls(&e, "inner", false);
    let outer = caller(&e, inner_call[0]).expect("an owner");
    assert_eq!(outer.rsplit('.').next(), Some("outer"), "{outer}");
}

const TYPED_NESTED: &str = "package p

class Store {
    fun add() {}
}

fun helper() {}

fun outer() {
    fun inner() {
        helper()
        val s = Store()
        s.add()
    }
    inner()
}
";

#[test]
fn typed_answers_still_join_calls_in_unmodelled_nested_callables() {
    // Typed resolution joins each answer to its call by the callable it found the call
    // in, here the local `inner`, which the extractor does not represent; the graph's
    // owner is `outer`. The answers must still reach the calls.
    let engine = Engine::new().unwrap();
    let mut resolver = ProjectResolver::new(&engine).unwrap();
    let path = "src/main/kotlin/p/A.kt";
    let extract = resolver
        .extract_and_add("kotlin", path, TYPED_NESTED.as_bytes())
        .unwrap();
    assert_eq!(defined(&extract, "inner"), 0);
    let resolution = resolver.run().unwrap();
    for (callee, target) in [
        ("helper", "src.main.kotlin.p.A.helper"),
        ("s.add", "src.main.kotlin.p.A.Store.add"),
    ] {
        let index = extract
            .calls
            .iter()
            .position(|c| !c.is_reference && extract.text(&c.callee_text) == callee)
            .expect("the nested call");
        assert_eq!(
            caller(&extract, &extract.calls[index]).as_deref(),
            Some("src.main.kotlin.p.A.outer")
        );
        let answer = resolution
            .resolutions
            .iter()
            .find(|r| r.site_ref.rel_path == path && r.site_ref.call_index as usize == index)
            .unwrap_or_else(|| panic!("no typed answer for {callee}"));
        assert_eq!(answer.target_qn, target);
    }
}

#[test]
fn nested_typed_answers_are_the_same_from_a_cached_extraction() {
    // The owner is no part of a file's surface: resolution reports an answer's site by
    // the callable the call was found in, so a cached extraction answers exactly as a
    // fresh one, nested calls included.
    let engine = Engine::new().unwrap();
    let path = "src/main/kotlin/p/A.kt";
    let mut fresh = ProjectResolver::new(&engine).unwrap();
    let extract = fresh
        .extract_and_add("kotlin", path, TYPED_NESTED.as_bytes())
        .unwrap();
    let from_fresh = fresh.run().unwrap();
    let from_cache = common::round_trip(&extract);
    assert_eq!(from_cache, extract);
    let mut cached = ProjectResolver::new(&engine).unwrap();
    cached
        .add_file(&from_cache, TYPED_NESTED.as_bytes())
        .unwrap();
    assert!(!from_fresh.resolutions.is_empty());
    assert_eq!(cached.run().unwrap(), from_fresh);
}
