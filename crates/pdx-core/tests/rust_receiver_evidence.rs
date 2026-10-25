//! Issues 105, 113 and 116 (R5): a Rust call's target is drawn only on positive
//! evidence, read as Rust reads it, and what proves nothing is no evidence.
//!
//! - **Proven.** A field's declared type (never a method of the field's name), a
//!   method's declared return type, an annotation, a constructor's path, `?` of a
//!   `Result` or an `Option`, `.await` of what an `async fn` returns, a path through a
//!   crate's name or an alias, an impl of an imported or generic type named as extraction
//!   names its owner (113B). A field and a method of one name share their qualified name:
//!   the typed answer names the method.
//! - **Unproven.** `?` of another type, `.await` of a value no `async fn` returned, a
//!   one-element tuple, a trait impl's method where a `where` predicate decides the
//!   method (rustc takes the predicate's, not the impl's).
//! - **Issue 116.** A method called on a value is not decided by an external import of
//!   its receiver's first name: a `let` shadows the import, and a path's result is a
//!   value.
//!
//! Every repository here is real: written to a checkout, discovered, extracted by the
//! engine, registered and resolved with typed resolution itself, so each band is Stage
//! 3's.

use std::fs;
use std::path::Path;

use pdx_core::bands::Band;
use pdx_core::config::PdxConfig;
use pdx_core::index::discover::discover;
use pdx_core::index::extract::{ExtractLimits, ExtractStage};
use pdx_core::resolve::registry::{DefinitionRef, SymbolRegistry};
use pdx_core::resolve::stages::{Resolution, ResolveReport, resolve};

const FILES: &[(&str, &str)] = &[
    (
        "Cargo.toml",
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nalpha = { path = \"crates/alpha\" }\nfutures-util = \"0.3\"\ntokio = \"1\"\n",
    ),
    (
        "crates/alpha/Cargo.toml",
        "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    ),
    (
        "crates/alpha/src/lib.rs",
        "pub fn compute(x: i32) -> i32 {\n    x * 2\n}\n\npub struct Meter {\n    pub v: i32,\n}\n\nimpl Meter {\n    pub fn read(&self) -> i32 {\n        self.v\n    }\n}\n",
    ),
    (
        "src/lib.rs",
        "pub mod calls;\npub mod direct;\npub mod model;\npub mod net;\npub mod parts;\npub mod pool;\n",
    ),
    (
        "src/model.rs",
        r"pub struct Gear {
    pub teeth: u32,
}

impl Gear {
    pub fn spin(&self) -> u32 {
        self.teeth
    }
}

pub struct Pager(pub u32);

impl Pager {
    pub fn page(&self) -> u32 {
        self.0
    }
}

pub struct Shell<T>(pub T);

pub struct Holder {
    pub get: Gear,
}

impl Holder {
    pub fn get(&self) -> Pager {
        Pager(1)
    }
}

pub fn plain() -> Gear {
    Gear { teeth: 1 }
}

pub async fn later() -> Gear {
    plain()
}

pub fn shelled() -> Shell<Gear> {
    Shell(plain())
}

pub fn maybe() -> Option<Gear> {
    Some(plain())
}
",
    ),
    (
        "src/calls.rs",
        r"use crate::model::Gear as Wheel;
use crate::model::{later, maybe, plain, shelled, Gear, Holder};

pub fn field_then_method(h: &Holder) -> u32 {
    h.get.spin()
}

pub fn method_then_method(h: &Holder) -> u32 {
    h.get().page()
}

pub fn annotated() -> u32 {
    let g: Gear = Default::default();
    g.spin()
}

pub fn aliased() -> u32 {
    let w = Wheel { teeth: 2 };
    w.spin()
}

pub async fn awaited() -> u32 {
    later().await.spin()
}

pub async fn not_a_future() -> u32 {
    plain().await.spin()
}

pub fn an_option() -> Option<u32> {
    Some(maybe()?.spin())
}

pub fn not_a_result() -> Option<u32> {
    Some(shelled()?.spin())
}

pub fn one_tuple() -> u32 {
    (plain(),).spin()
}

pub fn crate_named() -> i32 {
    alpha::compute(1)
}

pub fn crate_type() -> i32 {
    let m = alpha::Meter { v: 3 };
    m.read()
}
",
    ),
    // Issue 113B: impls of an imported type and of a generic type.
    (
        "src/parts.rs",
        r"use crate::model::Gear;

pub trait Parts {
    fn parts(&self) -> u32;
}

impl Parts for Gear {
    fn parts(&self) -> u32 {
        self.spin()
    }
}

pub struct Router<S>(pub S);

impl<S> Router<S> {
    pub fn route(&self) -> u32 {
        self.finish()
    }

    pub fn finish(&self) -> u32 {
        0
    }
}
",
    ),
    (
        "src/pool.rs",
        r"pub trait Duplicate {
    fn duplicate(&self) -> Self;
}

pub struct Pool<S>(pub S);

impl<S> Duplicate for Pool<S> {
    fn duplicate(&self) -> Self {
        unimplemented!()
    }
}

impl<S> Pool<S> {
    pub fn plain_twin(&self, other: &Pool<S>) -> Pool<S> {
        other.duplicate()
    }
}

impl<S> Pool<S>
where
    Pool<S>: Duplicate,
{
    pub fn bound_twin(&self, other: &Pool<S>) -> Pool<S> {
        other.duplicate()
    }
}
",
    ),
    // A trait impl's method a whole path names: `Self::` in an impl of a type with no
    // arguments is the type's own path.
    (
        "src/direct.rs",
        r"pub trait Load<S> {
    fn load(state: &S) -> Self;
}

pub trait Make {
    fn make() -> Self;
}

pub struct Plain;

impl<S: Send> Load<S> for Plain {
    fn load(_state: &S) -> Self {
        Plain
    }
}

impl Make for Plain {
    fn make() -> Self {
        Plain
    }
}

impl Plain {
    pub fn loaded() -> Plain {
        Self::load(&())
    }

    pub fn made() -> Plain {
        Self::make()
    }
}
",
    ),
    // Issue 116's two shapes, and the import itself.
    (
        "src/net.rs",
        r"use futures_util::stream;
use tokio::net::TcpListener;

pub trait TapIo {
    fn tap_io(self) -> u32;
}

pub struct Feed(pub u32);

impl Feed {
    pub fn chunk_text(&self) -> u32 {
        self.0
    }
}

pub async fn served(addr: u32) -> u32 {
    TcpListener::bind(addr).await.unwrap().tap_io()
}

pub async fn shadowed(client: &Client) -> u32 {
    let stream = client.get().await;
    stream.chunk_text()
}

pub fn imported() -> u32 {
    stream::iter(1)
}
",
    ),
];

struct Resolved {
    _dir: tempfile::TempDir,
    reg: SymbolRegistry,
    report: ResolveReport,
}

fn resolved() -> &'static Resolved {
    static RESOLVED: std::sync::OnceLock<Resolved> = std::sync::OnceLock::new();
    RESOLVED.get_or_init(|| {
        let dir = tempfile::Builder::new()
            .prefix("pdx-rust-receiver-evidence-test-")
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

/// What the call sites of `path` whose callee is spelled `callee` resolved to, in
/// source order.
fn all_at(path: &str, callee: &str) -> Vec<&'static Resolution> {
    let r = resolved();
    let mut found: Vec<_> = r
        .report
        .resolutions
        .iter()
        .filter(|s| {
            s.site_ref.rel_path == path
                && r.reg
                    .text(path, &s.site.callee_text)
                    .is_some_and(|t| t == callee)
        })
        .collect();
    found.sort_by_key(|s| s.site.span.map(|span| span.start_byte));
    found.into_iter().map(|s| &s.resolution).collect()
}

fn at(path: &str, callee: &str) -> &'static Resolution {
    let found = all_at(path, callee);
    assert_eq!(found.len(), 1, "{callee} in {path}: {found:?}");
    found[0]
}

/// The callable definitions named `name` declared in the type `ty` of `path`, or, with
/// no type, its free functions of the name.
fn one(path: &str, ty: Option<&str>, name: &str) -> DefinitionRef {
    let reg = &resolved().reg;
    let found: Vec<DefinitionRef> = reg
        .by_name(name)
        .iter()
        .filter(|r| {
            r.path == path
                && reg.definition(r).is_some_and(|d| {
                    matches!(
                        d.kind,
                        pdx_engine::DefinitionKind::Function | pdx_engine::DefinitionKind::Method
                    )
                })
                && reg
                    .declaring_type(r)
                    .and_then(|t| reg.definition(&t))
                    .map(|d| d.name.clone())
                    .as_deref()
                    == ty
        })
        .cloned()
        .collect();
    assert_eq!(found.len(), 1, "{ty:?}.{name} in {path}: {found:?}");
    found[0].clone()
}

fn assert_typed(got: &Resolution, target: &DefinitionRef) {
    assert_eq!(got.band(), Band::Typed, "{got:?}");
    assert_eq!(got.target(), Some(target), "{got:?}");
}

/// No typed answer, and nothing drawn.
fn assert_not_drawn(got: &Resolution) {
    assert!(!got.band().is_drawn(), "{got:?}");
}

const MODEL: &str = "src/model.rs";
const CALLS: &str = "src/calls.rs";

#[test]
fn e1_a_fields_type_is_the_fields_never_the_namesake_methods() {
    // `h.get` is the field (a `Gear`); `h.get()` the method (a `Pager`). The method's
    // typed answer names the qualified name the field shares: the method is its target.
    assert_typed(at(CALLS, "h.get.spin"), &one(MODEL, Some("Gear"), "spin"));
    assert_typed(at(CALLS, "h.get"), &one(MODEL, Some("Holder"), "get"));
    assert_typed(
        at(CALLS, "h.get().page"),
        &one(MODEL, Some("Pager"), "page"),
    );
}

#[test]
fn e2_an_annotation_an_alias_and_a_constructor_type_a_binding() {
    let spin = one(MODEL, Some("Gear"), "spin");
    assert_typed(at(CALLS, "g.spin"), &spin);
    assert_typed(at(CALLS, "w.spin"), &spin);
}

#[test]
fn e3_await_unwraps_only_what_an_async_fn_returns() {
    let spin = one(MODEL, Some("Gear"), "spin");
    let awaited = all_at(CALLS, "later().await.spin");
    assert_typed(awaited[0], &spin);
    assert_not_drawn(at(CALLS, "plain().await.spin"));
}

#[test]
fn e4_try_unwraps_only_a_result_or_an_option() {
    assert_typed(
        at(CALLS, "maybe()?.spin"),
        &one(MODEL, Some("Gear"), "spin"),
    );
    assert_not_drawn(at(CALLS, "shelled()?.spin"));
}

#[test]
fn e5_a_one_element_tuple_is_no_element() {
    assert_not_drawn(at(CALLS, "(plain(),).spin"));
}

#[test]
fn e6_a_crates_name_anchors_a_path() {
    const ALPHA: &str = "crates/alpha/src/lib.rs";
    assert_typed(at(CALLS, "alpha::compute"), &one(ALPHA, None, "compute"));
    assert_typed(at(CALLS, "m.read"), &one(ALPHA, Some("Meter"), "read"));
}

#[test]
fn e7_an_impl_of_an_imported_or_generic_type_joins_its_sites() {
    // Issue 113B: the typed pass names the caller as extraction names the owner, the
    // file's module and the impl's type as written, so its answers reach the sites.
    const PARTS: &str = "src/parts.rs";
    assert_typed(at(PARTS, "self.spin"), &one(MODEL, Some("Gear"), "spin"));
    assert_typed(
        at(PARTS, "self.finish"),
        &one(PARTS, Some("Router"), "finish"),
    );
}

#[test]
fn e8_a_where_predicate_decides_the_method_not_the_impl() {
    // Without the predicate, the impl that requires nothing is the method; under
    // `where Pool<S>: Duplicate` rustc takes the predicate's method, so the impl's is no
    // typed answer.
    const POOL: &str = "src/pool.rs";
    let calls = all_at(POOL, "other.duplicate");
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_typed(calls[0], &one(POOL, Some("Pool"), "duplicate"));
    assert_ne!(calls[1].band(), Band::Typed, "{:?}", calls[1]);
    assert_not_drawn(calls[1]);
}

#[test]
fn e10_a_method_a_whole_path_names_is_proven_like_any_other() {
    // `Self::load` names `Plain`'s `Load` impl's method by its whole path; the impl
    // needs `S: Send`, which nothing at the call proves (issue 109): no typed answer, and
    // nothing drawn. `Self::make`'s impl requires nothing.
    const DIRECT: &str = "src/direct.rs";
    let load = at(DIRECT, "Self::load");
    assert_ne!(load.band(), Band::Typed, "{load:?}");
    assert_not_drawn(load);
    assert_typed(
        at(DIRECT, "Self::make"),
        &one(DIRECT, Some("Plain"), "make"),
    );
}

#[test]
fn e9_a_method_on_a_value_is_not_its_receivers_import() {
    // Issue 116: neither a `let` that shadows `stream` nor a value an external path made
    // is the external crate's; their methods fall to the repository's names, weak. The
    // import itself is still external.
    const NET: &str = "src/net.rs";
    let chunk = at(NET, "stream.chunk_text");
    assert_eq!(chunk.band(), Band::Exact, "{chunk:?}");
    assert_eq!(
        chunk.weak_candidate(),
        Some(&one(NET, Some("Feed"), "chunk_text"))
    );
    let tap = at(NET, "TcpListener::bind(addr).await.unwrap().tap_io");
    assert_ne!(tap.band(), Band::External, "{tap:?}");
    assert_not_drawn(tap);
    assert_eq!(at(NET, "stream::iter").band(), Band::External);
}
