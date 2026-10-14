//! Issue 111: Rust typed resolution reads a file's `use` declarations from its syntax
//! tree. A declaration's text split at commas gave the names of a nested or
//! multi-line list no binding, or a wrong one (`on` as `crate::on`), and `pub use`
//! none; since issue 104 an imported name is a typed answer only through the path its
//! import gives, so each of those calls lost its answer.
//!
//! One real repository, written to a checkout, extracted by the engine, registered and
//! resolved with typed resolution itself. Each file imports in one of Rust's forms and
//! calls what it imports; each test asserts the call's band, target and the engine's
//! strategy. The repository has a second module named `routing` and a function deep in
//! `a::b::c`, so that a binding or a glob read wrongly cannot resolve by a module's last
//! name alone.

use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use pdx_core::bands::Band;
use pdx_core::config::PdxConfig;
use pdx_core::index::discover::discover;
use pdx_core::index::extract::{ExtractLimits, ExtractStage};
use pdx_core::resolve::registry::{DefinitionRef, SymbolRegistry};
use pdx_core::resolve::stages::{Resolution, ResolveReport, resolve};

const FILES: &[(&str, &str)] = &[
    (
        "Cargo.toml",
        "[package]\nname = \"uses\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nhyper = \"1\"\n",
    ),
    (
        "src/lib.rs",
        "pub mod a;\npub mod alias;\npub mod ambiguous;\npub mod deep;\npub mod external;\npub mod globs;\npub mod helpers;\npub mod inline;\npub mod multi;\npub mod nested;\npub mod other;\npub mod parent;\npub mod routing;\npub mod selfs;\npub mod simple;\npub mod unimported;\npub mod visibility;\n",
    ),
    (
        "src/routing/mod.rs",
        "mod method_routing;\n\npub use self::method_routing::any;\n\npub struct MethodFilter;\n\npub fn on(x: u32) -> u32 {\n    x\n}\n\npub fn on_service(x: u32) -> u32 {\n    x\n}\n\npub fn route_to(x: u32) -> u32 {\n    x\n}\n\npub fn fetch(x: u32) -> u32 {\n    x\n}\n\npub fn solo_route(x: u32) -> u32 {\n    x\n}\n",
    ),
    (
        "src/routing/method_routing.rs",
        "pub fn any(x: u32) -> u32 {\n    x\n}\n",
    ),
    (
        "src/other/mod.rs",
        "pub struct Thing;\n\npub fn thing(x: u32) -> u32 {\n    x\n}\n\npub fn fetch(x: u32) -> u32 {\n    x\n}\n\npub fn lonely(x: u32) -> u32 {\n    x\n}\n",
    ),
    (
        "src/helpers.rs",
        "pub fn helper_one(x: u32) -> u32 {\n    x\n}\n",
    ),
    ("src/a/mod.rs", "pub mod b;\npub mod routing;\n"),
    (
        "src/a/routing.rs",
        "pub fn route_to(x: u32) -> u32 {\n    x + 1\n}\n",
    ),
    ("src/a/b/mod.rs", "pub mod c;\n"),
    (
        "src/a/b/c.rs",
        "pub fn deep_fn(x: u32) -> u32 {\n    x\n}\n\npub fn glob_target(x: u32) -> u32 {\n    x\n}\n",
    ),
    (
        "src/parent/mod.rs",
        "pub mod child;\n\npub fn helper(x: u32) -> u32 {\n    x\n}\n",
    ),
    // 15: `super`.
    (
        "src/parent/child.rs",
        "use super::helper;\n\npub fn call(x: u32) -> u32 {\n    helper(x)\n}\n",
    ),
    // 1: a simple import.
    (
        "src/simple.rs",
        "use crate::routing::on;\n\npub fn call(x: u32) -> u32 {\n    on(x)\n}\n",
    ),
    // 2: a nested group.
    (
        "src/nested.rs",
        "use crate::{routing::{on, MethodFilter}, other::thing};\n\npub fn call(x: u32) -> u32 {\n    let _ = MethodFilter;\n    on(x) + thing(x)\n}\n",
    ),
    // 3: a deeply nested group.
    (
        "src/deep.rs",
        "use crate::{a::{b::{c::{deep_fn}}}};\n\npub fn call(x: u32) -> u32 {\n    deep_fn(x)\n}\n",
    ),
    // 4 and 11: a multi-line group with comments, and a re-export.
    (
        "src/multi.rs",
        "use crate::{\n    routing::{\n        on_service, // a route helper\n        /* re-exported */ any,\n    },\n    other::thing,\n};\n\npub fn call(x: u32) -> u32 {\n    on_service(x) + any(x) + thing(x)\n}\n",
    ),
    // 5 and 6: aliases, alone and nested.
    (
        "src/alias.rs",
        "use crate::routing::on as route_on;\nuse crate::{routing::{on_service as service, MethodFilter}, other::thing as other_thing};\n\npub fn call(x: u32) -> u32 {\n    let _ = MethodFilter;\n    route_on(x) + service(x) + other_thing(x)\n}\n",
    ),
    // 7: `self`, alone and renamed.
    (
        "src/selfs.rs",
        "use crate::routing::{self};\nuse crate::other::{self as o, Thing};\n\npub fn call(x: u32) -> u32 {\n    let _ = Thing;\n    routing::route_to(x) + o::thing(x)\n}\n",
    ),
    // 8: globs, alone and nested.
    (
        "src/globs.rs",
        "use crate::a::b::c::*;\nuse crate::{a::{routing::*}, helpers::*, routing::MethodFilter};\n\npub fn call(x: u32) -> u32 {\n    let _ = MethodFilter;\n    glob_target(x) + helper_one(x) + route_to(x)\n}\n",
    ),
    // 9 and 10: visibility before `use`.
    (
        "src/visibility.rs",
        "pub use crate::routing::on;\npub(crate) use crate::routing::on_service;\npub(in crate) use crate::other::thing;\n\npub fn call(x: u32) -> u32 {\n    on(x) + on_service(x) + thing(x)\n}\n",
    ),
    // 12: one name bound to two paths, in two scopes.
    (
        "src/ambiguous.rs",
        "use crate::routing::fetch;\n\npub fn call(x: u32) -> u32 {\n    fetch(x)\n}\n\n#[cfg(test)]\nmod tests {\n    use crate::other::fetch;\n\n    pub fn probe(x: u32) -> u32 {\n        fetch(x)\n    }\n}\n",
    ),
    // 13: an external crate's function with a namesake in the repository.
    (
        "src/external.rs",
        "use hyper::upgrade::on;\n\npub fn call(x: u32) -> u32 {\n    on(x)\n}\n",
    ),
    // 14: an inline module.
    (
        "src/inline.rs",
        "mod inner {\n    pub fn deep_inner(x: u32) -> u32 {\n        x\n    }\n}\n\nuse self::inner::deep_inner;\n\npub fn call(x: u32) -> u32 {\n    deep_inner(x)\n}\n",
    ),
    // Evidence that names no module the function is in, or none at all: no answer.
    (
        "src/fnscope.rs",
        "pub fn inner(x: u32) -> u32 {\n    use crate::helpers::helper_one;\n    helper_one(x)\n}\n\npub fn outer(x: u32) -> u32 {\n    helper_one(x)\n}\n",
    ),
    (
        "src/unimported.rs",
        "use crate::other::solo_route;\n\npub fn call(x: u32) -> u32 {\n    solo_route(x) + lonely(x)\n}\n",
    ),
];

struct Resolved {
    _dir: tempfile::TempDir,
    reg: SymbolRegistry,
    report: ResolveReport,
}

/// The repository, resolved once for every test.
fn resolved() -> &'static Resolved {
    static RESOLVED: OnceLock<Resolved> = OnceLock::new();
    RESOLVED.get_or_init(|| {
        let dir = tempfile::Builder::new()
            .prefix("pdx-rust-use-trees-test-")
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
        let report = resolve(root, &reg).expect("resolution succeeds");
        Resolved {
            _dir: dir,
            reg,
            report,
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

/// What the call sites of `path` spelled `callee` resolved to, in site order.
fn all_at(path: &str, callee: &str) -> Vec<&'static Resolution> {
    let r = resolved();
    r.report
        .resolutions
        .iter()
        .filter(|s| {
            s.site_ref.rel_path == path
                && r.reg
                    .text(path, &s.site.callee_text)
                    .is_some_and(|t| t == callee)
        })
        .map(|s| &s.resolution)
        .collect()
}

/// What the one call site of `path` spelled `callee` resolved to.
fn at(path: &str, callee: &str) -> &'static Resolution {
    let found = all_at(path, callee);
    assert_eq!(found.len(), 1, "{callee} in {path}: {found:?}");
    found[0]
}

/// A typed answer from `caller_path`'s call `callee` to `name` in `target_path`.
fn typed(caller_path: &str, callee: &str, target_path: &str, name: &str, strategy: &str) {
    let got = at(caller_path, callee);
    assert_eq!(got.band(), Band::Typed, "{caller_path} {callee}: {got:?}");
    assert_eq!(
        got.target(),
        Some(&def(target_path, name)),
        "{caller_path} {callee}"
    );
    assert_eq!(
        got.engine_strategy(),
        Some(strategy),
        "{caller_path} {callee}"
    );
}

const PATH: &str = "lsp_short_name_unique";
const ALIAS: &str = "lsp_import_alias";

#[test]
fn a_simple_import_is_path_evidence() {
    typed("src/simple.rs", "on", "src/routing/mod.rs", "on", PATH);
}

#[test]
fn a_nested_group_binds_each_name_to_its_own_path() {
    // `crate::{routing::{on, MethodFilter}, other::thing}`: `on` is
    // `crate::routing::on`, never `crate::on`.
    typed("src/nested.rs", "on", "src/routing/mod.rs", "on", PATH);
    typed("src/nested.rs", "thing", "src/other/mod.rs", "thing", PATH);
}

#[test]
fn a_deeply_nested_group_keeps_every_level() {
    typed("src/deep.rs", "deep_fn", "src/a/b/c.rs", "deep_fn", PATH);
}

#[test]
fn a_multi_line_group_with_comments_reads_as_one_line() {
    typed(
        "src/multi.rs",
        "on_service",
        "src/routing/mod.rs",
        "on_service",
        PATH,
    );
    typed("src/multi.rs", "thing", "src/other/mod.rs", "thing", PATH);
    // `any` is defined in `routing::method_routing` and re-exported by `routing`.
    typed(
        "src/multi.rs",
        "any",
        "src/routing/method_routing.rs",
        "any",
        PATH,
    );
}

#[test]
fn an_alias_reaches_the_function_it_renames() {
    typed(
        "src/alias.rs",
        "route_on",
        "src/routing/mod.rs",
        "on",
        ALIAS,
    );
    typed(
        "src/alias.rs",
        "service",
        "src/routing/mod.rs",
        "on_service",
        ALIAS,
    );
    typed(
        "src/alias.rs",
        "other_thing",
        "src/other/mod.rs",
        "thing",
        ALIAS,
    );
}

#[test]
fn self_binds_the_module_under_its_name_or_its_alias() {
    // Two modules are named `routing`; only `{self}` says which one `routing::` is.
    typed(
        "src/selfs.rs",
        "routing::route_to",
        "src/routing/mod.rs",
        "route_to",
        PATH,
    );
    typed(
        "src/selfs.rs",
        "o::thing",
        "src/other/mod.rs",
        "thing",
        PATH,
    );
}

#[test]
fn a_glob_brings_in_its_modules_names() {
    // A glob in a nested list is its whole path's: `crate::a::routing`, never the
    // crate's own `routing`, which defines a `route_to` too.
    typed(
        "src/globs.rs",
        "route_to",
        "src/a/routing.rs",
        "route_to",
        PATH,
    );
    typed(
        "src/globs.rs",
        "glob_target",
        "src/a/b/c.rs",
        "glob_target",
        PATH,
    );
    typed(
        "src/globs.rs",
        "helper_one",
        "src/helpers.rs",
        "helper_one",
        PATH,
    );
}

#[test]
fn a_visibility_before_use_still_binds_the_name() {
    typed("src/visibility.rs", "on", "src/routing/mod.rs", "on", PATH);
    typed(
        "src/visibility.rs",
        "on_service",
        "src/routing/mod.rs",
        "on_service",
        PATH,
    );
    typed(
        "src/visibility.rs",
        "thing",
        "src/other/mod.rs",
        "thing",
        PATH,
    );
}

#[test]
fn super_crate_and_inline_modules_anchor_the_path() {
    typed(
        "src/parent/child.rs",
        "helper",
        "src/parent/mod.rs",
        "helper",
        PATH,
    );
    typed(
        "src/inline.rs",
        "deep_inner",
        "src/inline.rs",
        "deep_inner",
        PATH,
    );
}

#[test]
fn one_name_in_a_module_and_its_test_module_is_two_bindings() {
    // `fetch` is `crate::routing::fetch` in the module and `crate::other::fetch` in its
    // test module. Issue 111 read both into one map for the file and withheld the name;
    // Stage 3 then drew both calls to `routing::fetch`, the test module's wrongly. Each
    // `use` binds in its own scope (issue 114): each call is its scope's import's, and
    // nothing is ambiguous. One scope binding a name twice is rust_use_scopes' S14.
    let calls = all_at("src/ambiguous.rs", "fetch");
    assert_eq!(calls.len(), 2, "{calls:?}");
    for (got, target) in calls
        .into_iter()
        .zip(["src/routing/mod.rs", "src/other/mod.rs"])
    {
        assert_eq!(got.band(), Band::Typed, "{got:?}");
        assert_eq!(got.target(), Some(&def(target, "fetch")), "{got:?}");
    }
}

#[test]
fn a_functions_use_is_no_evidence_outside_it() {
    // `inner` imports `helper_one` in its body; `outer`, in the same file, imports
    // nothing and in Rust cannot call it. The body's `use` binds in the body (issue
    // 114): `inner`'s call is typed through it, and `outer`'s is never typed or
    // import-guided, as it would be were a function's imports read as the file's.
    let calls = all_at("src/fnscope.rs", "helper_one");
    assert_eq!(calls.len(), 2, "{calls:?}");
    let (inner, outer) = (calls[0], calls[1]);
    assert_eq!(inner.band(), Band::Typed, "{inner:?}");
    assert_eq!(inner.target(), Some(&def("src/helpers.rs", "helper_one")));
    assert!(outer.engine().is_none(), "{outer:?}");
    assert!(
        !matches!(outer.band(), Band::Typed | Band::ImportGuided),
        "{outer:?}"
    );
}

#[test]
fn an_external_crates_import_stays_external() {
    // `use hyper::upgrade::on;`: the repository's own `on` is never the answer.
    let got = at("src/external.rs", "on");
    assert_eq!(got.band(), Band::External, "{got:?}");
    assert!(got.engine().is_none(), "{got:?}");
    assert_ne!(got.target(), Some(&def("src/routing/mod.rs", "on")));
}

#[test]
fn wrong_or_missing_evidence_gives_no_typed_answer() {
    // `solo_route` is imported from a module it is not in, and `lonely` is not imported
    // at all: each is the repository's only function of its name, which is no evidence.
    for callee in ["solo_route", "lonely"] {
        let got = at("src/unimported.rs", callee);
        assert!(got.engine().is_none(), "{callee}: {got:?}");
        assert_ne!(got.band(), Band::Typed, "{callee}: {got:?}");
    }
}
