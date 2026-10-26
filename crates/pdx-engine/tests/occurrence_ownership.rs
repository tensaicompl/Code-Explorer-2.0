//! A site's owner is joined to its definition by declaration occurrence, never by
//! comparing qualified names (docs/plan/ISSUES.md, issue 76; decision 48).
//!
//! The walk names the callable it finds a call in by rules of its own, and the
//! definition extractor names the definition by others; for some declarations the two
//! differ. Each fixture here is one such declaration, and its sites must be owned by the
//! definition the graph has, their node-type path starting at its node. The private
//! occurrence is byte-precise and carries its source buffer: two definitions on one line
//! are told apart, and the macro-expanded text of C and C++ is a buffer of its own.

mod common;

use pdx_engine::{Call, DefinitionKind, Engine, FileExtract, ProjectResolver};

fn extract(language: &str, rel_path: &str, source: &str) -> FileExtract {
    Engine::new()
        .unwrap()
        .extract(language, rel_path, source.as_bytes())
        .unwrap_or_else(|e| panic!("{rel_path}: {e}"))
}

/// The positioned calls (or, with `reference`, callable references) to `callee`, in
/// source order: the ones a segment can store.
fn sites<'e>(e: &'e FileExtract, callee: &str, reference: bool) -> Vec<&'e Call> {
    let mut found: Vec<&Call> = e
        .calls
        .iter()
        .filter(|c| {
            c.is_reference == reference && c.span.is_some() && e.text(&c.callee_text) == callee
        })
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

/// The one positioned call to `callee`, owned by `owner`, its path starting at `node`.
fn assert_owned(e: &FileExtract, callee: &str, owner: &str, node: &str) {
    let found = sites(e, callee, false);
    assert_eq!(found.len(), 1, "{callee}: {found:?}");
    assert_eq!(caller(e, found[0]).as_deref(), Some(owner), "{callee}");
    assert_eq!(path(found[0])[0], node, "{callee}");
}

const GENERIC_IMPL: &str = "use std::fmt;

pub struct Upgrade<F> {
    callback: F,
}

impl<F> fmt::Debug for Upgrade<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        audit_debug();
        register(audit_debug);
        f.debug_struct(\"Upgrade\").finish()
    }
}

fn audit_debug() {}

fn register<T>(_hook: T) {}
";

#[test]
fn rust_method_of_a_generic_type_owns_its_calls() {
    // The walk names the frame `Upgrade<F>.fmt`; the definition is `Upgrade.fmt`.
    let e = extract("rust", "src/upgrade.rs", GENERIC_IMPL);
    assert!(
        e.definitions
            .iter()
            .enumerate()
            .any(|(i, d)| e.qualified_name(i) == "src.upgrade.Upgrade.fmt"
                && d.kind == DefinitionKind::Method)
    );
    assert_owned(
        &e,
        "audit_debug",
        "src.upgrade.Upgrade.fmt",
        "function_item",
    );
    assert_owned(&e, "register", "src.upgrade.Upgrade.fmt", "function_item");
}

#[test]
fn rust_method_of_a_generic_type_owns_its_callable_references() {
    let e = extract("rust", "src/upgrade.rs", GENERIC_IMPL);
    let references = sites(&e, "audit_debug", true);
    assert_eq!(references.len(), 1);
    assert_eq!(
        caller(&e, references[0]).as_deref(),
        Some("src.upgrade.Upgrade.fmt")
    );
    assert_eq!(path(references[0])[0], "function_item");
}

const CFG_TWINS: &str = "#[cfg(feature = \"fast\")]
fn pick_strategy() {
    run_fast_path();
}

#[cfg(not(feature = \"fast\"))]
fn pick_strategy() {
    run_slow_path();
}

fn run_fast_path() {}

fn run_slow_path() {}
";

#[test]
fn rust_cfg_twins_each_own_their_calls() {
    // Each definition's name carries its `cfg` predicate; the walk's carries none, and
    // is the same for both.
    let e = extract("rust", "src/twin.rs", CFG_TWINS);
    let twins: Vec<String> = e
        .definitions
        .iter()
        .enumerate()
        .filter(|(_, d)| d.name == "pick_strategy")
        .map(|(i, _)| e.qualified_name(i))
        .collect();
    assert_eq!(twins.len(), 2, "{twins:?}");
    assert_ne!(twins[0], twins[1]);
    assert_owned(&e, "run_fast_path", &twins[0], "function_item");
    assert_owned(&e, "run_slow_path", &twins[1], "function_item");
}

const MINIFIED: &str = "function alpha(){track_alpha()}function beta(){track_beta()}\
function track_alpha(){}function track_beta(){}track_alpha();\n";

#[test]
fn functions_on_one_line_each_own_their_calls() {
    // Every definition here starts and ends on line 1: only bytes tell them apart.
    let e = extract("javascript", "dist/bundle.js", MINIFIED);
    let on_line_one = e
        .definitions
        .iter()
        .filter(|d| d.span.is_some_and(|s| s.start_line == 1 && s.end_line == 1))
        .count();
    assert_eq!(on_line_one, 4);
    let alpha = sites(&e, "track_alpha", false);
    assert_eq!(alpha.len(), 2, "one in alpha, one at file scope");
    assert_eq!(caller(&e, alpha[0]).as_deref(), Some("dist.bundle.alpha"));
    assert_owned(&e, "track_beta", "dist.bundle.beta", "function_declaration");
    // The call after the last definition, on the same line, is the file's.
    let module_call = alpha[1];
    if let Some(index) = module_call.caller {
        assert_eq!(e.definitions[index as usize].kind, DefinitionKind::Module);
    }
    assert_eq!(path(module_call)[0], "program");
}

const ADA_SPEC: &str = "package Sensor is
   type Device is null record;
   function Powered (This : Device) return Boolean;
   procedure Power_Down (This : in out Device)
     with Post => not Powered (This);
end Sensor;
";

#[test]
fn ada_specification_aspect_is_owned_by_its_subprogram() {
    // The walk qualifies a specification's subprogram by its package
    // (`sensor.Sensor.Power_Down`); the definition is `sensor.Power_Down`.
    let e = extract("ada", "sensor.ads", ADA_SPEC);
    assert_owned(&e, "Powered", "sensor.Power_Down", "subprogram_declaration");
}

const MACRO_STATEMENT: &str = "#define YY_IGNORE_BEGIN
static void release_symbol(int kind) {}

static void destroy_symbol(int kind) {
  YY_IGNORE_BEGIN
  switch (kind) {
    default:
      release_symbol(kind);
      break;
  }
}
";

#[test]
fn c_statement_misread_as_a_function_is_owned_by_its_function() {
    // Without the preprocessor, `YY_IGNORE_BEGIN switch (kind) {...}` reads as a
    // function named `switch` nested in destroy_symbol. The file has no definition for
    // it: its sites are destroy_symbol's, as any unrepresented nested callable's are
    // (issue 75), no longer exempt in C (issue 76).
    let e = extract("c", "src/destroy.c", MACRO_STATEMENT);
    assert_owned(
        &e,
        "release_symbol",
        "src.destroy.destroy_symbol",
        "function_definition",
    );
}

const FRAME_LEFT_OPEN: &str = "static void prepare(void) {
  setup_once();
}

#ifdef HAVE_THREADS
static void run_threads(int n) {
  int a;
  for (a = 0; a < n; ++a) {
#ifdef __MVS__
    if (join_thread(a) != 0) {
#else
    if (join_thread(a)) {
#endif
      report_failure(a);
    }
  }
}
#endif
";

#[test]
fn c_frame_left_open_owns_no_site_it_does_not_hold() {
    // The region after `prepare` does not parse, and the parser marks it as trivia, so
    // the walk keeps `prepare`'s frame open across it (issue 79). `run_threads` has no
    // definition: the calls in it are the file's, never `prepare`'s, whose declaration
    // does not hold them, and their path starts at the root.
    let e = extract("c", "src/leak.c", FRAME_LEFT_OPEN);
    assert_owned(&e, "setup_once", "src.leak.prepare", "function_definition");
    let failure = sites(&e, "report_failure", false);
    assert_eq!(failure.len(), 1);
    if let Some(index) = failure[0].caller {
        assert_eq!(e.definitions[index as usize].kind, DefinitionKind::Module);
    }
    assert_eq!(path(failure[0])[0], "translation_unit");
}

const TEST_MACRO: &str = "#include <gtest/gtest.h>

int checked_total() { return 2; }

TEST(Totals, AddsUp) {
  EXPECT_EQ(checked_total(), 2);
}
";

#[test]
fn cpp_test_macro_owns_its_calls() {
    // The definition is named from the macro's arguments; the walk names it `TEST`.
    let e = extract("cpp", "test/suite.cc", TEST_MACRO);
    assert_owned(
        &e,
        "checked_total",
        "test.suite.TEST_Totals_AddsUp",
        "function_definition",
    );
}

const OUT_OF_LINE_TEMPLATE: &str = "template <typename T> struct Box {
  int size() const;
};

int measure_box() { return 3; }

template <typename T> int Box<T>::size() const { return measure_box(); }
";

#[test]
fn cpp_template_member_defined_out_of_line_owns_its_calls() {
    // The walk names the frame through `Box<T>`; the definition does not.
    let e = extract("cpp", "src/box.cpp", OUT_OF_LINE_TEMPLATE);
    // The function node inside the template owns its body; the template does not.
    assert_owned(&e, "measure_box", "src.box.size", "function_definition");
}

const EXPORTED_CLASS: &str = "#define PANEL_API

int default_width() { return 4; }
int measure_panel() { return 2; }

class PANEL_API Panel : public Base {
 public:
  int width = default_width();
  int size() const { return measure_panel(); }
};
";

/// The definition named `qualified_name`.
fn definition<'e>(e: &'e FileExtract, qualified_name: &str) -> &'e pdx_engine::Definition {
    let names = e.qualified_names();
    let index = (0..names.len())
        .find(|&i| names.get(i) == qualified_name)
        .unwrap_or_else(|| panic!("no {qualified_name}"));
    &e.definitions[index]
}

#[test]
fn cpp_method_adopted_from_expanded_text_owns_its_raw_calls() {
    // The raw parse misreads the exported class, so the class and its method are
    // definitions only the macro-expanded text gave. No byte of the raw source maps to
    // that text: the raw function node spanning exactly the method's lines, with its
    // name on the first, stands for it (decision 48), and owns the raw call in it.
    let e = extract("cpp", "src/panel.cpp", EXPORTED_CLASS);
    assert_eq!(
        definition(&e, "src.panel.Panel.size").kind,
        DefinitionKind::Method
    );
    let raw = sites(&e, "measure_panel", false);
    assert_eq!(raw.len(), 1);
    assert_eq!(caller(&e, raw[0]).as_deref(), Some("src.panel.Panel.size"));
    // The call the expanded text holds is no site: it is owned by its occurrence there,
    // exactly the adopted definition's.
    let expanded: Vec<&Call> = e
        .calls
        .iter()
        .filter(|c| c.span.is_none() && e.text(&c.callee_text) == "measure_panel")
        .collect();
    assert_eq!(expanded.len(), 1);
    assert_eq!(
        caller(&e, expanded[0]).as_deref(),
        Some("src.panel.Panel.size")
    );
}

#[test]
fn cpp_superseded_definition_hands_its_sites_to_its_replacement() {
    // The raw parse also misreads the class as a function named after its base; the
    // expanded text's class definition supersedes that one. A call in the class body,
    // outside any method, was the superseded function's and is now its replacement's.
    let e = extract("cpp", "src/panel.cpp", EXPORTED_CLASS);
    assert_eq!(
        definition(&e, "src.panel.Panel").kind,
        DefinitionKind::Class
    );
    assert!(e.definitions.iter().all(|d| d.name != "Base"));
    let raw = sites(&e, "default_width", false);
    assert_eq!(raw.len(), 1);
    assert_eq!(caller(&e, raw[0]).as_deref(), Some("src.panel.Panel"));
}

const SECONDARY_CONSTRUCTOR: &str = "package p

class Key(val names: List<String>) {
    constructor(first: String, vararg rest: String) : this(listOf(first) + rest.asList())
}
";

#[test]
fn kotlin_secondary_constructor_owns_its_delegation_calls() {
    // The walk can give a secondary constructor no name, so it opens no function frame
    // for it; its definition's node still owns the sites under it.
    let e = extract("kotlin", "src/main/kotlin/p/Key.kt", SECONDARY_CONSTRUCTOR);
    let constructor = "src.main.kotlin.p.Key.Key.Key";
    assert_eq!(definition(&e, constructor).kind, DefinitionKind::Method);
    assert_owned(&e, "listOf", constructor, "secondary_constructor");
    assert_owned(&e, "rest.asList", constructor, "secondary_constructor");
}

const DELEGATE_ACCESS: &str = "package p

class Expression {
    private val cachedHash by lazy { compute() }

    override fun hashCode(): Int = cachedHash

    fun compute(): Int = 7
}
";

#[test]
fn kotlin_delegate_access_added_after_the_walk_is_owned_by_its_function() {
    // The resolver adds a `getValue` call for the delegated property after the walk;
    // it is owned from the tree by the same rule as the walk's own sites.
    let e = extract("kotlin", "src/main/kotlin/p/Expression.kt", DELEGATE_ACCESS);
    assert_owned(
        &e,
        "getValue",
        "src.main.kotlin.p.Expression.Expression.hashCode",
        "function_declaration",
    );
}

const TYPED_GENERIC: &str = "pub struct Store;

impl Store {
    pub fn add(&self) {}
}

pub fn helper() {}

pub struct Wrapper<T> {
    inner: T,
}

impl<T> Wrapper<T> {
    pub fn run(&self) {
        helper();
        let store = Store;
        store.add();
    }
}
";

/// The typed answers for the positioned calls to `callee` in `path`.
fn typed_targets(
    resolution: &pdx_engine::ProjectResolution,
    extract: &FileExtract,
    path: &str,
    callee: &str,
) -> Vec<String> {
    let index = extract
        .calls
        .iter()
        .position(|c| !c.is_reference && c.span.is_some() && extract.text(&c.callee_text) == callee)
        .unwrap_or_else(|| panic!("no call to {callee}"));
    resolution
        .resolutions
        .iter()
        .filter(|r| r.site_ref.rel_path == path && r.site_ref.call_index as usize == index)
        .map(|r| r.target_qn.clone())
        .collect()
}

#[test]
fn typed_answers_join_calls_in_a_method_of_a_generic_type() {
    // Typed resolution joins an answer to its call by the name the walk gives the
    // callable, here `Wrapper<T>.run`: it must keep doing so, while the graph's owner is
    // the definition `Wrapper.run`.
    let engine = Engine::new().unwrap();
    let mut resolver = ProjectResolver::new(&engine).unwrap();
    let path = "src/wrapper.rs";
    let extract = resolver
        .extract_and_add("rust", path, TYPED_GENERIC.as_bytes())
        .unwrap();
    let resolution = resolver.run().unwrap();
    for (callee, target) in [
        ("helper", "src.wrapper.helper"),
        ("store.add", "src.wrapper.Store.add"),
    ] {
        let call = sites(&extract, callee, false)[0];
        assert_eq!(
            caller(&extract, call).as_deref(),
            Some("src.wrapper.Wrapper.run")
        );
        assert_eq!(
            typed_targets(&resolution, &extract, path, callee),
            [target],
            "{callee}"
        );
    }
}

#[test]
fn typed_answers_in_a_method_of_a_generic_type_are_the_same_from_a_cached_extraction() {
    // The owner's occurrence is in no file's surface, nor in its extraction: a cached
    // extraction answers exactly as a fresh one.
    let engine = Engine::new().unwrap();
    let path = "src/wrapper.rs";
    let mut fresh = ProjectResolver::new(&engine).unwrap();
    let extract = fresh
        .extract_and_add("rust", path, TYPED_GENERIC.as_bytes())
        .unwrap();
    let from_fresh = fresh.run().unwrap();
    let from_cache = common::round_trip(&extract);
    assert_eq!(from_cache, extract);
    let mut cached = ProjectResolver::new(&engine).unwrap();
    cached
        .add_file(&from_cache, TYPED_GENERIC.as_bytes())
        .unwrap();
    assert!(!from_fresh.resolutions.is_empty());
    assert_eq!(cached.run().unwrap(), from_fresh);
}

#[test]
fn typed_answers_in_a_cpp_test_macro_are_the_same_from_a_cached_extraction() {
    let engine = Engine::new().unwrap();
    let path = "test/suite.cc";
    let mut fresh = ProjectResolver::new(&engine).unwrap();
    let extract = fresh
        .extract_and_add("cpp", path, TEST_MACRO.as_bytes())
        .unwrap();
    let from_fresh = fresh.run().unwrap();
    let from_cache = common::round_trip(&extract);
    let mut cached = ProjectResolver::new(&engine).unwrap();
    cached.add_file(&from_cache, TEST_MACRO.as_bytes()).unwrap();
    assert_eq!(cached.run().unwrap(), from_fresh);
}
