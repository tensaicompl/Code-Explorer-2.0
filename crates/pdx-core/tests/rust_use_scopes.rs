//! Issue 114: a Rust `use` binds only in its lexical scope, in the typed pass and in
//! Stage 3 alike. A block's binding is seen in the block and the blocks inside it, the
//! nearest binding wins, and the enclosing one is seen again after the block; a module
//! starts a namespace of its own, so an inline module sees its parent module's names
//! only through `use super::...`, and a sibling's never.
//!
//! One real repository, written to a checkout, extracted by the engine, registered and
//! resolved twice: with typed resolution itself, and by Stage 3 alone, with no answer
//! of the engine's, so that Stage 3's own reading of scopes is what decides. `one` and
//! `two` define the same names, so a binding read in the wrong scope reaches the wrong
//! target, never no target.

use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use pdx_core::bands::Band;
use pdx_core::config::PdxConfig;
use pdx_core::index::discover::discover;
use pdx_core::index::extract::{ExtractLimits, ExtractStage};
use pdx_core::resolve::registry::{DefinitionRef, SymbolRegistry};
use pdx_core::resolve::stages::{Resolution, ResolveReport, resolve, resolve_with};
use pdx_engine::{ProjectResolution, RunHealth, RunStatus};

const FILES: &[(&str, &str)] = &[
    (
        "Cargo.toml",
        "[package]\nname = \"scopes\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    ),
    (
        "src/lib.rs",
        "pub mod amb;\npub mod deep;\npub mod mac;\npub mod mb2;\npub mod mb3;\npub mod mb3d;\npub mod mb4;\npub mod mb5;\npub mod one;\npub mod repro;\npub mod s01;\npub mod s02;\npub mod s03;\npub mod s04;\npub mod s05;\npub mod s06;\npub mod s07;\npub mod s08;\npub mod s09;\npub mod s10;\npub mod s10b;\npub mod s11;\npub mod s12;\npub mod s12b;\npub mod s12c;\npub mod s13;\npub mod s14;\npub mod s15;\npub mod two;\n",
    ),
    (
        "src/one.rs",
        "pub fn x() -> u32 {\n    1\n}\n\npub fn fetch() -> u32 {\n    1\n}\n\npub fn thing() -> u32 {\n    1\n}\n\npub struct Counter;\n\nimpl Counter {\n    pub fn new() -> Self {\n        Counter\n    }\n\n    pub fn load(&self) -> u32 {\n        1\n    }\n}\n",
    ),
    (
        "src/two.rs",
        "pub fn x() -> u32 {\n    2\n}\n\npub fn fetch() -> u32 {\n    2\n}\n\npub fn thing() -> u32 {\n    2\n}\n",
    ),
    ("src/deep/mod.rs", "pub mod inner;\n"),
    ("src/deep/inner.rs", "pub fn globbed() -> u32 {\n    3\n}\n"),
    // S1: a `use` of the file's module, seen in its functions.
    (
        "src/s01.rs",
        "use crate::one::x;\n\npub fn f() -> u32 {\n    x()\n}\n",
    ),
    // S2: an inline module's `use`, seen in its functions.
    (
        "src/s02.rs",
        "mod inner {\n    use crate::one::x;\n\n    pub fn f() -> u32 {\n        x()\n    }\n}\n",
    ),
    // S3: an inline module's `use`, not seen outside it.
    (
        "src/s03.rs",
        "mod inner {\n    use crate::one::x;\n}\n\npub fn g() -> u32 {\n    x()\n}\n",
    ),
    // S4 and S15: sibling modules bind one name each to their own.
    (
        "src/s04.rs",
        "mod a {\n    use crate::one::x;\n\n    pub fn f() -> u32 {\n        x()\n    }\n}\n\nmod b {\n    use crate::two::x;\n\n    pub fn g() -> u32 {\n        x()\n    }\n}\n",
    ),
    // S5: a function's own `use`, seen in it.
    (
        "src/s05.rs",
        "pub fn f() -> u32 {\n    use crate::one::x;\n    x()\n}\n",
    ),
    // S6: a function's `use`, not seen in another function.
    (
        "src/s06.rs",
        "pub fn f() -> u32 {\n    use crate::one::x;\n    x()\n}\n\npub fn g() -> u32 {\n    x()\n}\n",
    ),
    // S7: a block's `use`, seen in the block.
    (
        "src/s07.rs",
        "pub fn f() -> u32 {\n    let a = {\n        use crate::one::x;\n        x()\n    };\n    a\n}\n",
    ),
    // S8: a block's `use`, not seen after the block.
    (
        "src/s08.rs",
        "pub fn f() -> u32 {\n    let a = {\n        use crate::one::x;\n        x()\n    };\n    a + x()\n}\n",
    ),
    // S9: the inner binding shadows the outer.
    (
        "src/s09.rs",
        "use crate::one::x;\n\npub fn f() -> u32 {\n    use crate::two::x;\n    x()\n}\n",
    ),
    // S10: the outer binding is seen again after the inner block.
    (
        "src/s10.rs",
        "use crate::one::x;\n\npub fn f() -> u32 {\n    let a = {\n        use crate::two::x;\n        x()\n    };\n    a + x()\n}\n",
    ),
    // S9 and S10 inside a function: its body's binding, a block's inside it.
    (
        "src/s10b.rs",
        "pub fn f() -> u32 {\n    use crate::one::x;\n    let a = {\n        use crate::two::x;\n        x()\n    };\n    a + x()\n}\n",
    ),
    // S11: aliases in a nested list, in a function.
    (
        "src/s11.rs",
        "pub fn f() -> u32 {\n    use crate::{one::{x as first}, two::{x as second}};\n    first() + second()\n}\n",
    ),
    // S12: a function's glob, not seen in another function.
    (
        "src/s12.rs",
        "pub fn f() -> u32 {\n    use crate::deep::inner::*;\n    globbed()\n}\n\npub fn g() -> u32 {\n    globbed()\n}\n",
    ),
    // S12: a nearer glob that brings the name in shadows the outer `use`.
    (
        "src/s12b.rs",
        "use crate::one::x;\n\npub fn f() -> u32 {\n    use crate::two::*;\n    x()\n}\n",
    ),
    // S12: in one scope, a `use` of the name shadows a glob that brings it in.
    (
        "src/s12c.rs",
        "pub fn f() -> u32 {\n    use crate::one::x;\n    use crate::two::*;\n    x()\n}\n",
    ),
    // S13: `{self}` in a function binds the module there only.
    (
        "src/s13.rs",
        "pub fn f() -> u32 {\n    use crate::deep::inner::{self as nested};\n    nested::globbed()\n}\n\npub fn g() -> u32 {\n    nested::globbed()\n}\n",
    ),
    // S14: one scope binding a name to two paths is no evidence.
    (
        "src/s14.rs",
        "pub fn f() -> u32 {\n    use crate::one::fetch;\n    use crate::two::fetch;\n    fetch()\n}\n",
    ),
    // S15: one name in two scopes is no ambiguity.
    (
        "src/s15.rs",
        "mod a {\n    use crate::one::fetch;\n\n    pub fn f() -> u32 {\n        fetch()\n    }\n}\n\npub fn g() -> u32 {\n    use crate::two::fetch;\n    fetch()\n}\n",
    ),
    // A module boundary: the parent's `use` is no binding of the child's.
    (
        "src/mb2.rs",
        "use crate::one::x;\n\nmod inner {\n    pub fn f() -> u32 {\n        x()\n    }\n}\n",
    ),
    // The child imports the parent's binding by name.
    (
        "src/mb3.rs",
        "use crate::one::x;\n\nmod inner {\n    use super::x;\n\n    pub fn f() -> u32 {\n        x()\n    }\n}\n",
    ),
    // The child imports a function the parent defines.
    (
        "src/mb3d.rs",
        "pub fn local() -> u32 {\n    4\n}\n\nmod inner {\n    use super::local;\n\n    pub fn f() -> u32 {\n        local()\n    }\n}\n",
    ),
    // The child imports the parent's names by a glob.
    (
        "src/mb4.rs",
        "use crate::one::x;\n\nmod inner {\n    use super::*;\n\n    pub fn f() -> u32 {\n        x()\n    }\n}\n",
    ),
    // A module boundary: the parent's glob is no glob of the child's.
    (
        "src/mb5.rs",
        "use crate::deep::inner::*;\n\nmod inner_mod {\n    pub fn f() -> u32 {\n        globbed()\n    }\n}\n",
    ),
    // A macro's arguments are read in the scope of its invocation (the engine's own
    // answers are in pdx-engine's use_scopes test; here, what Stage 3 makes of them).
    (
        "src/mac.rs",
        "macro_rules! fresh {\n    () => {\n        AtomicU32::new(0).load(std::sync::atomic::Ordering::SeqCst)\n    };\n}\n\npub fn f() -> u32 {\n    use std::sync::atomic::AtomicU32;\n    let c = AtomicU32::new(0);\n    assert_eq!(c.load(std::sync::atomic::Ordering::SeqCst), 0);\n    1\n}\n\npub fn g() -> u32 {\n    let c = AtomicU32::new(0);\n    assert_eq!(c.load(std::sync::atomic::Ordering::SeqCst), 0);\n    1\n}\n\npub fn h() -> u32 {\n    use std::sync::atomic::AtomicU32;\n    fresh!()\n}\n\npub fn k() -> u32 {\n    fresh!()\n}\n",
    ),
    // The issue's reproduction: a test module's import of another `fetch`.
    (
        "src/repro.rs",
        "pub fn fetch() -> u32 {\n    0\n}\n\npub fn call() -> u32 {\n    fetch()\n}\n\npub fn outer() -> u32 {\n    fetch()\n}\n\n#[cfg(test)]\nmod tests {\n    use crate::one::fetch;\n\n    fn probe() -> u32 {\n        fetch()\n    }\n}\n",
    ),
    // Stage 3's reproduction: each `fetch` imported in its own scope; and the same
    // with the test module's in a list.
    (
        "src/amb.rs",
        "use crate::one::fetch;\n\npub fn call() -> u32 {\n    fetch()\n}\n\n#[cfg(test)]\nmod tests {\n    use crate::two::fetch;\n\n    fn probe() -> u32 {\n        fetch()\n    }\n}\n\n#[cfg(test)]\nmod listed {\n    use crate::{two::{fetch}};\n\n    fn probe() -> u32 {\n        fetch()\n    }\n}\n",
    ),
];

struct Resolved {
    _dir: tempfile::TempDir,
    reg: SymbolRegistry,
    typed: ResolveReport,
    stage3: ResolveReport,
}

/// The repository, resolved once for every test: with typed resolution, and by Stage
/// 3 alone.
fn resolved() -> &'static Resolved {
    static RESOLVED: OnceLock<Resolved> = OnceLock::new();
    RESOLVED.get_or_init(|| {
        let dir = tempfile::Builder::new()
            .prefix("pdx-rust-use-scopes-test-")
            .tempdir()
            .expect("a directory");
        for (path, content) in FILES {
            let path = dir.path().join(path);
            fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
            fs::write(&path, content).expect("a file");
        }
        let root: &Path = dir.path();
        let config = PdxConfig::load(root).expect("the configuration loads");
        let files = discover(root, &config).expect("discovery succeeds");
        let limits = ExtractLimits {
            requested_workers: 2,
            memory_budget_bytes: 64 << 20,
        };
        let report = ExtractStage::new(root, &config.secrets, limits)
            .run(&files)
            .expect("extraction succeeds");
        let reg = SymbolRegistry::build(root, &files, report).expect("the registry builds");
        let typed = resolve(root, &reg).expect("resolution succeeds");
        let health = RunHealth {
            status: RunStatus::Clean,
            files: 0,
            files_resolved: 0,
            files_untyped: 0,
            files_empty: 0,
            files_over_budget: 0,
            files_source_unavailable: 0,
            files_not_reached: 0,
            pass_failures: 0,
        };
        let stage3 = resolve_with(
            &reg,
            &ProjectResolution {
                resolutions: Vec::new(),
                health,
            },
        )
        .expect("resolution succeeds");
        Resolved {
            _dir: dir,
            reg,
            typed,
            stage3,
        }
    })
}

/// The one definition named `name` in `path`.
fn def(path: &str, name: &str) -> DefinitionRef {
    let reg = &resolved().reg;
    let found: Vec<&DefinitionRef> = reg
        .by_name(name)
        .iter()
        .filter(|r| r.path == path)
        .collect();
    assert_eq!(found.len(), 1, "{name} in {path}: {found:?}");
    found[0].clone()
}

/// What the call sites of `path` spelled `callee` resolved to in `report`, in site
/// order.
fn sites<'r>(report: &'r ResolveReport, path: &str, callee: &str) -> Vec<&'r Resolution> {
    let reg = &resolved().reg;
    report
        .resolutions
        .iter()
        .filter(|s| {
            s.site_ref.rel_path == path
                && reg
                    .text(path, &s.site.callee_text)
                    .is_some_and(|t| t == callee)
        })
        .map(|s| &s.resolution)
        .collect()
}

/// The sites of `path` made from the function named `caller`, in site order.
fn from<'r>(report: &'r ResolveReport, path: &str, caller: &str) -> Vec<&'r Resolution> {
    let reg = &resolved().reg;
    report
        .resolutions
        .iter()
        .filter(|s| {
            s.site_ref.rel_path == path
                && s.site.caller.is_some_and(|index| {
                    reg.definition(&DefinitionRef {
                        path: path.into(),
                        index,
                    })
                    .is_some_and(|d| d.name == caller)
                })
        })
        .map(|s| &s.resolution)
        .collect()
}

/// The `n`th site of `path` spelled `callee` in `report`.
fn nth<'r>(report: &'r ResolveReport, path: &str, callee: &str, n: usize) -> &'r Resolution {
    let found = sites(report, path, callee);
    assert!(n < found.len(), "{callee} in {path}: {found:?}");
    found[n]
}

/// A drawn edge to `target` in `band`.
fn drawn(got: &Resolution, band: Band, target: &DefinitionRef, what: &str) {
    assert_eq!(got.band(), band, "{what}: {got:?}");
    assert_eq!(got.target(), Some(target), "{what}");
}

/// No trusted edge to `wrong`: neither typed nor import-guided may name it.
fn never(got: &Resolution, wrong: &DefinitionRef, what: &str) {
    assert!(
        !(matches!(got.band(), Band::Typed | Band::ImportGuided) && got.target() == Some(wrong)),
        "{what}: {got:?}"
    );
}

/// No typed answer and no import-guided edge: nothing in force binds the name.
fn unbound(got: &Resolution, what: &str) {
    assert!(got.engine().is_none(), "{what}: {got:?}");
    assert!(
        !matches!(got.band(), Band::Typed | Band::ImportGuided),
        "{what}: {got:?}"
    );
}

#[test]
fn s01_a_file_use_is_seen_in_its_functions() {
    let r = resolved();
    let one = def("src/one.rs", "x");
    drawn(
        nth(&r.typed, "src/s01.rs", "x", 0),
        Band::Typed,
        &one,
        "typed",
    );
    drawn(
        nth(&r.stage3, "src/s01.rs", "x", 0),
        Band::ImportGuided,
        &one,
        "stage 3",
    );
}

#[test]
fn s02_an_inline_modules_use_is_seen_in_it() {
    let r = resolved();
    let one = def("src/one.rs", "x");
    drawn(
        nth(&r.typed, "src/s02.rs", "x", 0),
        Band::Typed,
        &one,
        "typed",
    );
    drawn(
        nth(&r.stage3, "src/s02.rs", "x", 0),
        Band::ImportGuided,
        &one,
        "stage 3",
    );
}

#[test]
fn s03_an_inline_modules_use_is_not_seen_outside_it() {
    let r = resolved();
    unbound(nth(&r.typed, "src/s03.rs", "x", 0), "typed");
    unbound(nth(&r.stage3, "src/s03.rs", "x", 0), "stage 3");
}

#[test]
fn s04_sibling_modules_bind_their_own() {
    let r = resolved();
    let (one, two) = (def("src/one.rs", "x"), def("src/two.rs", "x"));
    for (report, band) in [(&r.typed, Band::Typed), (&r.stage3, Band::ImportGuided)] {
        drawn(nth(report, "src/s04.rs", "x", 0), band, &one, "module a");
        drawn(nth(report, "src/s04.rs", "x", 1), band, &two, "module b");
    }
}

#[test]
fn s05_a_functions_use_is_seen_in_it() {
    let r = resolved();
    let one = def("src/one.rs", "x");
    drawn(
        nth(&r.typed, "src/s05.rs", "x", 0),
        Band::Typed,
        &one,
        "typed",
    );
    drawn(
        nth(&r.stage3, "src/s05.rs", "x", 0),
        Band::ImportGuided,
        &one,
        "stage 3",
    );
}

#[test]
fn s06_a_functions_use_is_not_seen_in_another() {
    let r = resolved();
    let one = def("src/one.rs", "x");
    drawn(
        nth(&r.typed, "src/s06.rs", "x", 0),
        Band::Typed,
        &one,
        "f, typed",
    );
    unbound(nth(&r.typed, "src/s06.rs", "x", 1), "g, typed");
    unbound(nth(&r.stage3, "src/s06.rs", "x", 1), "g, stage 3");
}

#[test]
fn s07_a_blocks_use_is_seen_in_it() {
    let r = resolved();
    let one = def("src/one.rs", "x");
    drawn(
        nth(&r.typed, "src/s07.rs", "x", 0),
        Band::Typed,
        &one,
        "typed",
    );
    drawn(
        nth(&r.stage3, "src/s07.rs", "x", 0),
        Band::ImportGuided,
        &one,
        "stage 3",
    );
}

#[test]
fn s08_a_blocks_use_is_not_seen_after_it() {
    let r = resolved();
    let one = def("src/one.rs", "x");
    drawn(
        nth(&r.typed, "src/s08.rs", "x", 0),
        Band::Typed,
        &one,
        "in the block",
    );
    unbound(nth(&r.typed, "src/s08.rs", "x", 1), "after it, typed");
    unbound(nth(&r.stage3, "src/s08.rs", "x", 1), "after it, stage 3");
}

#[test]
fn s09_the_inner_binding_shadows_the_outer() {
    let r = resolved();
    let (one, two) = (def("src/one.rs", "x"), def("src/two.rs", "x"));
    drawn(
        nth(&r.typed, "src/s09.rs", "x", 0),
        Band::Typed,
        &two,
        "typed",
    );
    drawn(
        nth(&r.stage3, "src/s09.rs", "x", 0),
        Band::ImportGuided,
        &two,
        "stage 3",
    );
    never(nth(&r.typed, "src/s09.rs", "x", 0), &one, "typed");
}

#[test]
fn s10_the_outer_binding_is_seen_again_after_the_block() {
    let r = resolved();
    let (one, two) = (def("src/one.rs", "x"), def("src/two.rs", "x"));
    for (report, band) in [(&r.typed, Band::Typed), (&r.stage3, Band::ImportGuided)] {
        drawn(
            nth(report, "src/s10.rs", "x", 0),
            band,
            &two,
            "in the block",
        );
        drawn(nth(report, "src/s10.rs", "x", 1), band, &one, "after it");
    }
}

#[test]
fn s10_a_blocks_binding_shadows_its_functions_until_it_ends() {
    // The §13 case: `x` is `one::x` in the body, `two::x` in the block inside it, and
    // `one::x` again after the block.
    let r = resolved();
    let (one, two) = (def("src/one.rs", "x"), def("src/two.rs", "x"));
    for (report, band) in [(&r.typed, Band::Typed), (&r.stage3, Band::ImportGuided)] {
        drawn(
            nth(report, "src/s10b.rs", "x", 0),
            band,
            &two,
            "in the block",
        );
        drawn(nth(report, "src/s10b.rs", "x", 1), band, &one, "after it");
    }
}

#[test]
fn s11_aliases_in_a_functions_nested_list() {
    let r = resolved();
    let (one, two) = (def("src/one.rs", "x"), def("src/two.rs", "x"));
    let first = nth(&r.typed, "src/s11.rs", "first", 0);
    drawn(first, Band::Typed, &one, "first");
    assert_eq!(first.engine_strategy(), Some("lsp_import_alias"));
    drawn(
        nth(&r.typed, "src/s11.rs", "second", 0),
        Band::Typed,
        &two,
        "second",
    );
}

#[test]
fn s12_a_glob_binds_in_its_scope() {
    let r = resolved();
    let globbed = def("src/deep/inner.rs", "globbed");
    drawn(
        nth(&r.typed, "src/s12.rs", "globbed", 0),
        Band::Typed,
        &globbed,
        "f, typed",
    );
    drawn(
        nth(&r.stage3, "src/s12.rs", "globbed", 0),
        Band::ImportGuided,
        &globbed,
        "f, stage 3",
    );
    unbound(nth(&r.typed, "src/s12.rs", "globbed", 1), "g, typed");
    unbound(nth(&r.stage3, "src/s12.rs", "globbed", 1), "g, stage 3");
}

#[test]
fn s12_a_nearer_glob_shadows_the_outer_use() {
    // Rust: the block's glob brings `x` in, and the nearest scope that names it wins.
    // Stage 3 cannot tell what a glob brings in, so it keeps both and draws neither.
    let r = resolved();
    let (one, two) = (def("src/one.rs", "x"), def("src/two.rs", "x"));
    drawn(
        nth(&r.typed, "src/s12b.rs", "x", 0),
        Band::Typed,
        &two,
        "typed",
    );
    never(nth(&r.stage3, "src/s12b.rs", "x", 0), &one, "stage 3");
}

#[test]
fn s12_a_use_shadows_a_glob_of_its_own_scope() {
    // Rust: an explicit import shadows a glob import of the same scope.
    let r = resolved();
    let one = def("src/one.rs", "x");
    drawn(
        nth(&r.typed, "src/s12c.rs", "x", 0),
        Band::Typed,
        &one,
        "typed",
    );
    drawn(
        nth(&r.stage3, "src/s12c.rs", "x", 0),
        Band::ImportGuided,
        &one,
        "stage 3",
    );
}

#[test]
fn s13_self_binds_the_module_in_its_scope() {
    let r = resolved();
    let globbed = def("src/deep/inner.rs", "globbed");
    drawn(
        nth(&r.typed, "src/s13.rs", "nested::globbed", 0),
        Band::Typed,
        &globbed,
        "f",
    );
    unbound(nth(&r.typed, "src/s13.rs", "nested::globbed", 1), "g");
}

#[test]
fn s14_one_scope_binding_a_name_twice_is_no_evidence() {
    let r = resolved();
    let (one, two) = (def("src/one.rs", "fetch"), def("src/two.rs", "fetch"));
    for (report, what) in [(&r.typed, "typed"), (&r.stage3, "stage 3")] {
        let got = nth(report, "src/s14.rs", "fetch", 0);
        never(got, &one, what);
        never(got, &two, what);
    }
    assert!(nth(&r.typed, "src/s14.rs", "fetch", 0).engine().is_none());
}

#[test]
fn s15_one_name_in_two_scopes_is_no_ambiguity() {
    let r = resolved();
    let (one, two) = (def("src/one.rs", "fetch"), def("src/two.rs", "fetch"));
    for (report, band) in [(&r.typed, Band::Typed), (&r.stage3, Band::ImportGuided)] {
        drawn(
            nth(report, "src/s15.rs", "fetch", 0),
            band,
            &one,
            "module a",
        );
        drawn(nth(report, "src/s15.rs", "fetch", 1), band, &two, "g");
    }
}

#[test]
fn a_child_module_does_not_inherit_its_parents_use() {
    let r = resolved();
    unbound(nth(&r.typed, "src/mb2.rs", "x", 0), "typed");
    unbound(nth(&r.stage3, "src/mb2.rs", "x", 0), "stage 3");
}

#[test]
fn a_child_module_does_not_inherit_its_parents_glob() {
    let r = resolved();
    unbound(nth(&r.typed, "src/mb5.rs", "globbed", 0), "typed");
    unbound(nth(&r.stage3, "src/mb5.rs", "globbed", 0), "stage 3");
}

#[test]
fn a_child_module_imports_its_parents_binding_by_name() {
    let r = resolved();
    let one = def("src/one.rs", "x");
    drawn(
        nth(&r.typed, "src/mb3.rs", "x", 0),
        Band::Typed,
        &one,
        "typed",
    );
    drawn(
        nth(&r.stage3, "src/mb3.rs", "x", 0),
        Band::ImportGuided,
        &one,
        "stage 3",
    );
    let local = def("src/mb3d.rs", "local");
    drawn(
        nth(&r.typed, "src/mb3d.rs", "local", 0),
        Band::Typed,
        &local,
        "a definition",
    );
}

#[test]
fn a_childs_glob_of_its_parent_is_an_import_not_inheritance() {
    // `use super::*` brings the parent's names in, as an import of the child's: the same
    // call without it (mb2) sees nothing.
    let r = resolved();
    let one = def("src/one.rs", "x");
    drawn(
        nth(&r.typed, "src/mb4.rs", "x", 0),
        Band::Typed,
        &one,
        "typed",
    );
    drawn(
        nth(&r.stage3, "src/mb4.rs", "x", 0),
        Band::ImportGuided,
        &one,
        "stage 3",
    );
}

#[test]
fn a_macros_imported_name_is_read_in_its_invocations_scope() {
    // `AtomicU32::new` in `f` is the standard library's through `f`'s own `use`, so
    // Stage 3 settles it external; in `g` nothing binds the name, so it is never taken
    // for that import, nor for one of the repository.
    let r = resolved();
    let f = &from(&r.typed, "src/mac.rs", "f")[0];
    assert_eq!(f.band(), Band::External, "{f:?}");
    let g = &from(&r.typed, "src/mac.rs", "g")[0];
    assert_ne!(g.band(), Band::External, "{g:?}");
    assert!(
        !matches!(g.band(), Band::Typed | Band::ImportGuided),
        "{g:?}"
    );
}

#[test]
fn a_test_modules_import_draws_no_edge_outside_it() {
    // The issue's reproduction. Engines 8 and 9 read the test module's `use` as the
    // whole file's and drew `call`'s own `fetch()` to `one::fetch`, typed. A call
    // outside the test module reaches the module's own `fetch`; the test module's
    // reaches the one it imports.
    let r = resolved();
    let (own, imported) = (def("src/repro.rs", "fetch"), def("src/one.rs", "fetch"));
    for n in [0, 1] {
        let got = nth(&r.typed, "src/repro.rs", "fetch", n);
        never(got, &imported, "outside the test module");
        drawn(got, Band::Typed, &own, "outside the test module");
    }
    drawn(
        nth(&r.typed, "src/repro.rs", "fetch", 2),
        Band::Typed,
        &imported,
        "in the test module",
    );
}

#[test]
fn stage_3_reads_each_import_in_its_scope() {
    // Stage 3's reproduction: with the file's imports read as one set, it drew the test
    // module's call import-guided to the module's `one::fetch`. Each call now sees the
    // import of its own scope.
    let r = resolved();
    let (one, two) = (def("src/one.rs", "fetch"), def("src/two.rs", "fetch"));
    drawn(
        nth(&r.stage3, "src/amb.rs", "fetch", 0),
        Band::ImportGuided,
        &one,
        "the module's",
    );
    let probe = nth(&r.stage3, "src/amb.rs", "fetch", 1);
    never(probe, &one, "the test module's");
    drawn(probe, Band::ImportGuided, &two, "the test module's");
    // Stage 3 reads a brace list's names as imported from its prefix's module (`crate`,
    // as at the top of a file): no `fetch` there, so the list draws nothing, and never
    // the module's import.
    never(
        nth(&r.stage3, "src/amb.rs", "fetch", 2),
        &one,
        "the listed module's",
    );
}
