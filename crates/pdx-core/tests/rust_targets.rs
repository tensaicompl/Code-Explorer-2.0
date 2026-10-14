//! Issue 104 (decision 65, R2): a typed or import-guided answer for a Rust call names
//! the definition the call reaches, or Stage 3 draws nothing. A0 found the Rust
//! golden's trusted bands drawing 17 calls to the wrong definition: a function chosen
//! by its last name alone, whatever the path or the import said; an external crate's
//! function taken for a namesake of the repository's; a local function, a pattern
//! binding and a trait's method taken for namesakes; an impl a blanket impl or a macro
//! hides.
//!
//! Every repository here is real: written to a checkout, discovered, extracted by the
//! engine, registered and resolved with typed resolution itself, so each answer is the
//! engine's own and each band Stage 3's. E1 to E10 are the issue's classes, each with
//! the call A0 found and the calls around it that must keep their answers.
//!
//! The engine's fallback keeps its label, `lsp_short_name_unique`, and now answers
//! only on path evidence: the one function of the call's last name in the module the
//! call's path, or its import, names. A name unique in the repository and nothing
//! more has no answer.

use std::fs;
use std::path::Path;

use pdx_core::bands::Band;
use pdx_core::config::PdxConfig;
use pdx_core::index::discover::discover;
use pdx_core::index::extract::{ExtractLimits, ExtractStage};
use pdx_core::resolve::registry::{DefinitionRef, SymbolRegistry};
use pdx_core::resolve::stages::{Resolution, ResolveReport, Unconfirmed, resolve};
use pdx_engine::Strategy;

// --- fixtures ----------------------------------------------------------------------

struct Checkout {
    dir: tempfile::TempDir,
}

impl Checkout {
    fn new(files: &[(&str, &str)]) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("pdx-rust-targets-test-")
            .tempdir()
            .expect("a directory");
        for (path, content) in files {
            let path = dir.path().join(path);
            fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
            fs::write(&path, content).expect("a file");
        }
        Self { dir }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    /// The registry and Stage 3's report, typed resolution included.
    fn resolved(&self) -> (SymbolRegistry, ResolveReport) {
        let config = PdxConfig::load(self.root()).expect("the configuration loads");
        let files = discover(self.root(), &config).expect("discovery succeeds");
        let limits = ExtractLimits {
            requested_workers: 2,
            memory_budget_bytes: 64 << 20,
        };
        let report = ExtractStage::new(self.root(), &config.secrets, limits)
            .run(&files)
            .expect("extraction succeeds");
        let reg = SymbolRegistry::build(self.root(), &files, report).expect("the registry builds");
        let report = resolve(self.root(), &reg).expect("resolution succeeds");
        (reg, report)
    }
}

const CRATE: (&str, &str) = (
    "Cargo.toml",
    "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
);

/// The one definition named `name` in `path`.
fn def(reg: &SymbolRegistry, path: &str, name: &str) -> DefinitionRef {
    let found: Vec<&DefinitionRef> = reg
        .by_name(name)
        .iter()
        .filter(|r| r.path == path)
        .collect();
    assert_eq!(found.len(), 1, "{name} in {path}: {found:?}");
    found[0].clone()
}

/// What the call sites of `path` spelled `callee` resolved to, in site order.
fn all_at<'r>(
    reg: &SymbolRegistry,
    report: &'r ResolveReport,
    path: &str,
    callee: &str,
) -> Vec<&'r Resolution> {
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

/// What the one call site of `path` spelled `callee` resolved to.
fn at<'r>(
    reg: &SymbolRegistry,
    report: &'r ResolveReport,
    path: &str,
    callee: &str,
) -> &'r Resolution {
    let found = all_at(reg, report, path, callee);
    assert_eq!(found.len(), 1, "{callee} in {path}: {found:?}");
    found[0]
}

/// A typed answer to `target`, as the engine names its strategy.
fn assert_typed(got: &Resolution, target: &DefinitionRef, engine_strategy: &str) {
    assert_eq!(got.band(), Band::Typed, "{got:?}");
    assert_eq!(got.target(), Some(target), "{got:?}");
    assert_eq!(got.engine_strategy(), Some(engine_strategy), "{got:?}");
}

/// Whatever Stage 3 made of a site, never `wrong`: not as the target of a drawn band,
/// and not as the engine's typed answer.
fn assert_never(got: &Resolution, wrong: &DefinitionRef) {
    assert_ne!(got.target(), Some(wrong), "{got:?}");
    if let Some(e) = got.engine() {
        assert!(
            e.strategy != Strategy::LspTyped || e.target_rel_path.as_deref() != Some(&wrong.path),
            "{got:?}"
        );
    }
}

// --- E1: a module-qualified call reaches its own module ------------------------------

#[test]
fn e1_a_module_qualified_call_reaches_the_module_its_path_names() {
    // `from_request::expand` in `axum-macros/src/lib.rs` was answered with the crate
    // root's own `expand`: the fallback took the last name and dropped the path. Here
    // two child modules and the root each define `expand`, one module a file and one a
    // directory.
    let checkout = Checkout::new(&[
        CRATE,
        (
            "src/lib.rs",
            "mod a;\nmod b;\n\nfn expand(x: u32) -> u32 {\n    x\n}\n\npub fn first(x: u32) -> u32 {\n    a::expand(x)\n}\n\npub fn second(x: u32) -> u32 {\n    b::expand(x)\n}\n\npub fn local(x: u32) -> u32 {\n    expand(x)\n}\n",
        ),
        (
            "src/a.rs",
            "pub(crate) fn expand(x: u32) -> u32 {\n    x + 1\n}\n",
        ),
        (
            "src/b/mod.rs",
            "pub(crate) fn expand(x: u32) -> u32 {\n    x + 2\n}\n",
        ),
    ]);
    let (reg, report) = checkout.resolved();
    let root = def(&reg, "src/lib.rs", "expand");
    let a = def(&reg, "src/a.rs", "expand");
    let b = def(&reg, "src/b/mod.rs", "expand");
    assert_typed(
        at(&reg, &report, "src/lib.rs", "a::expand"),
        &a,
        "lsp_short_name_unique",
    );
    assert_typed(
        at(&reg, &report, "src/lib.rs", "b::expand"),
        &b,
        "lsp_short_name_unique",
    );
    assert_typed(
        at(&reg, &report, "src/lib.rs", "expand"),
        &root,
        "lsp_direct",
    );
}

#[test]
fn e1_paths_through_crate_names_inline_modules_and_imports_are_path_evidence() {
    // What A0 found the qualifier check must not lose: a crate's name with `-` in its
    // package and `_` in its path, an inline module the qualified names leave out, a
    // module the file imports, and a bare name a `use` brings in from a re-export.
    let checkout = Checkout::new(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/app-core\", \"crates/web\"]\n",
        ),
        (
            "crates/app-core/Cargo.toml",
            "[package]\nname = \"app-core\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ),
        ("crates/app-core/src/lib.rs", "pub mod languages;\n"),
        (
            "crates/app-core/src/languages.rs",
            "pub fn all() -> u32 {\n    1\n}\n",
        ),
        (
            "crates/app-core/tests/derive.rs",
            "#[test]\nfn every_language() {\n    let n = app_core::languages::all();\n    assert_eq!(n, 1);\n}\n",
        ),
        (
            "crates/web/Cargo.toml",
            "[package]\nname = \"web\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ),
        (
            "crates/web/src/lib.rs",
            "pub mod limits;\npub mod routing;\npub mod strip;\npub mod util;\n",
        ),
        (
            "crates/web/src/limits.rs",
            "pub fn total() -> u32 {\n    let a = cgroup::read();\n    a + self::cgroup::fetch()\n}\n\npub mod cgroup {\n    pub fn read() -> u32 {\n        2\n    }\n\n    pub fn fetch() -> u32 {\n        3\n    }\n}\n",
        ),
        (
            "crates/web/src/routing/mod.rs",
            "mod method_routing;\n\npub use self::method_routing::on;\n",
        ),
        (
            "crates/web/src/routing/method_routing.rs",
            "pub fn on(x: u32) -> u32 {\n    x\n}\n",
        ),
        (
            "crates/web/src/util.rs",
            "pub fn helper(x: u32) -> u32 {\n    x + 1\n}\n",
        ),
        (
            "crates/web/src/strip.rs",
            "use crate::routing::on;\nuse crate::util;\n\npub fn capture(x: u32) -> u32 {\n    on(x) + util::helper(x)\n}\n",
        ),
    ]);
    let (reg, report) = checkout.resolved();
    let all = def(&reg, "crates/app-core/src/languages.rs", "all");
    assert_typed(
        at(
            &reg,
            &report,
            "crates/app-core/tests/derive.rs",
            "app_core::languages::all",
        ),
        &all,
        "lsp_short_name_unique",
    );
    let limits = "crates/web/src/limits.rs";
    assert_typed(
        at(&reg, &report, limits, "cgroup::read"),
        &def(&reg, limits, "read"),
        "lsp_short_name_unique",
    );
    assert_typed(
        at(&reg, &report, limits, "self::cgroup::fetch"),
        &def(&reg, limits, "fetch"),
        "lsp_short_name_unique",
    );
    let strip = "crates/web/src/strip.rs";
    assert_typed(
        at(&reg, &report, strip, "on"),
        &def(&reg, "crates/web/src/routing/method_routing.rs", "on"),
        "lsp_short_name_unique",
    );
    assert_typed(
        at(&reg, &report, strip, "util::helper"),
        &def(&reg, "crates/web/src/util.rs", "helper"),
        "lsp_short_name_unique",
    );
}

// --- E2: an external crate's function is external -----------------------------------

#[test]
fn e2_an_external_crates_function_is_external_not_a_namesake() {
    // `multer::parse_boundary` and `hyper::upgrade::on` were answered with the
    // repository's functions of those names. A path that starts with a dependency of
    // the caller's crate that is not in the repository leads outside it.
    let checkout = Checkout::new(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/web\", \"crates/macros\"]\n",
        ),
        (
            "crates/web/Cargo.toml",
            "[package]\nname = \"web\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nmulter = \"3\"\nhyper = \"1\"\n",
        ),
        (
            "crates/web/src/lib.rs",
            "pub mod multipart;\npub mod routing;\npub mod ws;\n",
        ),
        (
            "crates/web/src/multipart.rs",
            "pub fn boundary(s: &str) -> usize {\n    multer::parse_boundary(s)\n}\n",
        ),
        (
            "crates/web/src/routing.rs",
            "pub fn on(x: u32) -> u32 {\n    x\n}\n",
        ),
        (
            "crates/web/src/ws.rs",
            "pub fn upgrade(x: u32) -> u32 {\n    hyper::upgrade::on(x)\n}\n",
        ),
        (
            "crates/macros/Cargo.toml",
            "[package]\nname = \"macros\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ),
        (
            "crates/macros/src/lib.rs",
            "pub fn parse_boundary(s: &str) -> usize {\n    s.len()\n}\n",
        ),
    ]);
    let (reg, report) = checkout.resolved();
    let multer = at(
        &reg,
        &report,
        "crates/web/src/multipart.rs",
        "multer::parse_boundary",
    );
    assert_eq!(multer.band(), Band::External, "{multer:?}");
    assert_never(
        multer,
        &def(&reg, "crates/macros/src/lib.rs", "parse_boundary"),
    );
    let hyper = at(&reg, &report, "crates/web/src/ws.rs", "hyper::upgrade::on");
    assert_eq!(hyper.band(), Band::External, "{hyper:?}");
    assert_never(hyper, &def(&reg, "crates/web/src/routing.rs", "on"));
}

// --- E3: associated functions of different types ------------------------------------

#[test]
fn e3_one_types_associated_function_is_never_anothers_namesake() {
    // `PathDeserializationError::custom` was answered with a free function `custom` of
    // another module. The type's own `custom` comes from an external trait's impl.
    let checkout = Checkout::new(&[
        (
            "Cargo.toml",
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nserde = \"1\"\n",
        ),
        (
            "src/lib.rs",
            "pub mod de;\npub mod errors;\npub mod info;\n",
        ),
        (
            "src/errors.rs",
            "pub struct PathError;\n\nimpl serde::de::Error for PathError {\n    fn custom<T: std::fmt::Display>(_msg: T) -> Self {\n        PathError\n    }\n}\n",
        ),
        ("src/info.rs", "pub fn custom() -> u32 {\n    1\n}\n"),
        (
            "src/de.rs",
            "use crate::errors::PathError;\n\npub fn fail() -> PathError {\n    PathError::custom(\"missing\")\n}\n",
        ),
    ]);
    let (reg, report) = checkout.resolved();
    let got = at(&reg, &report, "src/de.rs", "PathError::custom");
    assert_never(got, &def(&reg, "src/info.rs", "custom"));
    assert_eq!(got.target(), Some(&def(&reg, "src/errors.rs", "custom")));
}

// --- E4: the same name in another crate or a local function ---------------------------

#[test]
fn e4_a_local_function_is_not_another_crates_namesake() {
    // `find_first_not_double` is declared inside the function that calls it; the
    // extraction represents no such function, and `axum-macros` has one of that name.
    // The call is the local function's: unresolved, never drawn to the namesake.
    let checkout = Checkout::new(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/web\", \"crates/macros\"]\n",
        ),
        (
            "crates/web/Cargo.toml",
            "[package]\nname = \"web\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ),
        ("crates/web/src/lib.rs", "pub mod strip;\n"),
        (
            "crates/web/src/strip.rs",
            "pub fn capture(segment: &str) -> usize {\n    let start = find_first_not_double(b'{');\n\n    fn find_first_not_double(needle: u8) -> usize {\n        needle as usize + 1\n    }\n\n    start + segment.len()\n}\n",
        ),
        (
            "crates/macros/Cargo.toml",
            "[package]\nname = \"macros\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ),
        (
            "crates/macros/src/lib.rs",
            "pub fn find_first_not_double(needle: u8) -> usize {\n    needle as usize\n}\n",
        ),
    ]);
    let (reg, report) = checkout.resolved();
    let got = at(
        &reg,
        &report,
        "crates/web/src/strip.rs",
        "find_first_not_double",
    );
    assert_eq!(got.band(), Band::Unresolved, "{got:?}");
    assert!(got.engine().is_none(), "{got:?}");
    assert_never(
        got,
        &def(&reg, "crates/macros/src/lib.rs", "find_first_not_double"),
    );
}

#[test]
fn e4_a_bare_name_unique_by_name_alone_has_no_typed_answer() {
    // A bare name nothing imports, whose one function in the repository is in another
    // crate. Its uniqueness is no evidence that the call reaches it: the engine gives
    // no answer, and Stage 3 resolves the name by its own stages.
    let checkout = Checkout::new(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/one\", \"crates/two\"]\n",
        ),
        (
            "crates/one/Cargo.toml",
            "[package]\nname = \"one\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ),
        (
            "crates/one/src/lib.rs",
            "pub fn caller() -> u32 {\n    unique_helper(1)\n}\n",
        ),
        (
            "crates/two/Cargo.toml",
            "[package]\nname = \"two\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ),
        (
            "crates/two/src/lib.rs",
            "pub fn unique_helper(x: u32) -> u32 {\n    x\n}\n",
        ),
    ]);
    let (reg, report) = checkout.resolved();
    let got = at(&reg, &report, "crates/one/src/lib.rs", "unique_helper");
    assert!(got.engine().is_none(), "{got:?}");
    assert_eq!(got.band(), Band::Exact, "{got:?}");
    assert_eq!(
        got.target(),
        Some(&def(&reg, "crates/two/src/lib.rs", "unique_helper"))
    );
}

// --- E5: a trait's method is not a free function -------------------------------------

#[test]
fn e5_a_trait_qualified_call_is_not_a_free_function_of_its_name() {
    // `Handler::call(handler, req, state)` was answered with the crate's only free
    // function named `call`. `call` is blocklisted (issue 103's test covers it); here a
    // name off the list, so the stages after the engine are seen too, and a free
    // function of the name both in another module and beside the trait.
    let checkout = Checkout::new(&[
        CRATE,
        (
            "src/lib.rs",
            "pub mod handler;\npub mod routing;\npub mod service;\n",
        ),
        (
            "src/handler.rs",
            "pub trait Handler {\n    fn respond(self, x: u32) -> u32;\n}\n\npub struct Doubler;\n\nimpl Handler for Doubler {\n    fn respond(self, x: u32) -> u32 {\n        x * 2\n    }\n}\n\npub struct Tripler;\n\nimpl Handler for Tripler {\n    fn respond(self, x: u32) -> u32 {\n        x * 3\n    }\n}\n\npub fn respond(n: u32) -> u32 {\n    n\n}\n",
        ),
        (
            "src/routing.rs",
            "pub fn respond(n: u32) -> u32 {\n    n + 1\n}\n",
        ),
        (
            "src/service.rs",
            "use crate::handler::Handler;\n\npub fn serve<H: Handler>(h: H) -> u32 {\n    Handler::respond(h, 1)\n}\n",
        ),
    ]);
    let (reg, report) = checkout.resolved();
    let got = at(&reg, &report, "src/service.rs", "Handler::respond");
    // Neither free function of that name: the one elsewhere, nor the one beside the
    // trait in its own module.
    assert_never(got, &def(&reg, "src/routing.rs", "respond"));
    let [beside] = reg.by_qualified_name("src.handler.respond") else {
        panic!("the free function beside the trait");
    };
    assert_never(got, beside);
    assert!(got.engine().is_none(), "{got:?}");
    // The trait's own method, through the import of the trait.
    let [declared] = reg.by_qualified_name("src.handler.Handler.respond") else {
        panic!("the trait's method");
    };
    assert_eq!(got.band(), Band::ImportGuided, "{got:?}");
    assert_eq!(got.target(), Some(declared));
}

// --- E6: a blanket impl ----------------------------------------------------------------

#[test]
fn e6_a_call_a_blanket_impl_answers_is_recorded_as_the_engine_gives_it() {
    // `WithRejection::<TestExtractor, TestRejection>::from_request` reaches the blanket
    // `impl<S, T> FromRequest<S, ViaParts> for T` (its type implements only
    // `FromRequestParts`), not `WithRejection`'s own `FromRequest` impl, whose bound it
    // does not meet. The engine checks no impl's bounds and answers with the type's own
    // impl. Not corrected by issue 104: issue 109 owns it. This records today's answer,
    // so its change is seen.
    let checkout = Checkout::new(&[
        CRATE,
        ("src/lib.rs", "pub mod extract;\npub mod with_rejection;\n"),
        (
            "src/extract.rs",
            "pub struct ViaRequest;\npub struct ViaParts;\n\npub trait FromRequestParts<S> {\n    fn from_request_parts(parts: u32, state: &S) -> Self;\n}\n\npub trait FromRequest<S, M = ViaRequest> {\n    fn from_request(req: u32, state: &S) -> Self;\n}\n\nimpl<S, T> FromRequest<S, ViaParts> for T\nwhere\n    T: FromRequestParts<S>,\n{\n    fn from_request(req: u32, state: &S) -> Self {\n        T::from_request_parts(req, state)\n    }\n}\n",
        ),
        (
            "src/with_rejection.rs",
            "use crate::extract::{FromRequest, FromRequestParts};\n\npub struct WithRejection<E>(pub E);\n\nimpl<E, S> FromRequest<S> for WithRejection<E>\nwhere\n    E: FromRequest<S>,\n{\n    fn from_request(req: u32, state: &S) -> Self {\n        WithRejection(E::from_request(req, state))\n    }\n}\n\npub struct Extractor;\n\nimpl<S> FromRequestParts<S> for Extractor {\n    fn from_request_parts(_parts: u32, _state: &S) -> Self {\n        Extractor\n    }\n}\n\npub fn extract() -> WithRejection<Extractor> {\n    WithRejection::<Extractor>::from_request(1, &())\n}\n",
        ),
    ]);
    let (reg, report) = checkout.resolved();
    let got = at(
        &reg,
        &report,
        "src/with_rejection.rs",
        "WithRejection::<Extractor>::from_request",
    );
    assert_typed(
        got,
        &def(&reg, "src/with_rejection.rs", "from_request"),
        "lsp_ufcs",
    );
}

// --- E7: a pattern binding is not a call ---------------------------------------------

#[test]
fn e7_a_name_a_match_arm_binds_is_not_a_function_of_that_name() {
    // `Ok(Self(params)) => Ok(Some(Self(params)))` passes the arm's own `params`; the
    // engine answered it as a reference to the module's function `params`. The same
    // name called, or passed where no pattern binds it, is the function's.
    let checkout = Checkout::new(&[
        CRATE,
        (
            "src/lib.rs",
            "pub struct Wrap(pub u32);\n\nfn params(n: u32) -> u32 {\n    n\n}\n\npub fn pick(r: Result<Wrap, ()>) -> Option<Wrap> {\n    match r {\n        Ok(Wrap(params)) => Some(Wrap(params)),\n        Err(_) => None,\n    }\n}\n\npub fn direct() -> Wrap {\n    Wrap(params(1))\n}\n\npub fn apply(f: fn(u32) -> u32) -> u32 {\n    f(2)\n}\n\npub fn pass() -> u32 {\n    apply(params)\n}\n",
        ),
    ]);
    let (reg, report) = checkout.resolved();
    let params = def(&reg, "src/lib.rs", "params");
    // The call and the reference outside the arm: typed.
    let named: Vec<&Resolution> = all_at(&reg, &report, "src/lib.rs", "params");
    assert_eq!(named.len(), 2, "{named:?}");
    for got in &named {
        assert_eq!(got.band(), Band::Typed, "{got:?}");
        assert_eq!(got.target(), Some(&params));
    }
    let strategies: Vec<Option<&str>> = named.iter().map(|g| g.engine_strategy()).collect();
    assert!(strategies.contains(&Some("lsp_direct")), "{strategies:?}");
    assert!(
        strategies.contains(&Some("lsp_callable_value_reference")),
        "{strategies:?}"
    );
    // The arm's binding: no answer, so no reference to the function at all.
    let pick_line = "Some(Wrap(params))";
    let source = fs::read_to_string(checkout.root().join("src/lib.rs")).expect("the source");
    let arm = u32::try_from(source.find(pick_line).expect("the arm")).expect("small");
    let end = arm + u32::try_from(pick_line.len()).expect("small");
    let in_arm = |span: Option<pdx_engine::Span>| {
        span.is_some_and(|s| s.start_byte >= arm && s.start_byte < end)
    };
    assert!(
        report
            .unconfirmed
            .iter()
            .filter(|u| u.site_ref.rel_path == "src/lib.rs" && in_arm(u.site.span))
            .all(|u| u.reason == Unconfirmed::Reference && u.engine.is_none()),
        "{:?}",
        report.unconfirmed
    );
}

// --- E8: one qualified name for two definitions ---------------------------------------

#[test]
fn e8_an_answer_whose_name_two_definitions_share_is_withheld() {
    // Two `From` impls of one type give two `from` methods one qualified name. The
    // engine's answer names it; which definition the call reaches is not known from the
    // name, so it is never drawn as typed and neither definition is chosen.
    let checkout = Checkout::new(&[
        CRATE,
        (
            "src/lib.rs",
            "pub struct Wide(pub u32);\n\nimpl From<u8> for Wide {\n    fn from(n: u8) -> Self {\n        Wide(n as u32)\n    }\n}\n\nimpl From<u16> for Wide {\n    fn from(n: u16) -> Self {\n        Wide(n as u32)\n    }\n}\n\npub fn widen() -> Wide {\n    Wide::from(1u8)\n}\n",
        ),
    ]);
    let (reg, report) = checkout.resolved();
    let got = at(&reg, &report, "src/lib.rs", "Wide::from");
    assert_eq!(
        got.engine().map(|e| e.strategy),
        Some(Strategy::LspTyped),
        "{got:?}"
    );
    assert_eq!(got.band(), Band::Candidate, "{got:?}");
    assert_eq!(got.target(), None);
    let froms: Vec<&DefinitionRef> = reg
        .by_name("from")
        .iter()
        .filter(|r| r.path == "src/lib.rs")
        .collect();
    assert_eq!(froms.len(), 2);
    for from in froms {
        assert!(got.candidates().contains(from), "{got:?}");
    }
}

// --- E9: an import-guided `Type::from` --------------------------------------------------

#[test]
fn e9_an_import_guided_from_reaches_the_imported_types_impl() {
    // `Body::from(…)` in `axum-core/src/ext_traits/request.rs` is import-guided: the
    // file imports `Body`, and `Body`'s `from` is the answer. Another type's `from` is
    // never it. The call A0 found reaches an impl a `macro_rules!` invocation writes,
    // which no extraction represents, so the one `from` the extraction had was drawn
    // instead. Since issue 110 the extraction records that the macro writes `Body`'s
    // `from`, and neither call draws the handwritten one: which impl a call of the name
    // means is not told by the members the registry holds.
    let checkout = Checkout::new(&[
        CRATE,
        ("src/lib.rs", "pub mod body;\npub mod ext;\n"),
        (
            "src/body.rs",
            "pub struct Body;\n\nimpl From<()> for Body {\n    fn from(_: ()) -> Self {\n        Body\n    }\n}\n\nmacro_rules! body_from_impl {\n    ($ty:ty) => {\n        impl From<$ty> for Body {\n            fn from(_: $ty) -> Self {\n                Body\n            }\n        }\n    };\n}\n\nbody_from_impl!(&'static str);\n\npub struct Other;\n\nimpl From<()> for Other {\n    fn from(_: ()) -> Self {\n        Other\n    }\n}\n",
        ),
        (
            "src/ext.rs",
            "use crate::body::Body;\nuse crate::body::Other;\n\npub fn empty() -> Body {\n    Body::from(())\n}\n\npub fn text() -> Body {\n    Body::from(\"text\")\n}\n\npub fn other() -> Other {\n    Other::from(())\n}\n",
        ),
    ]);
    let (reg, report) = checkout.resolved();
    let froms: Vec<&DefinitionRef> = reg
        .by_name("from")
        .iter()
        .filter(|r| r.path == "src/body.rs")
        .collect();
    assert_eq!(froms.len(), 2, "Body's and Other's");
    let [empty, text] = all_at(&reg, &report, "src/ext.rs", "Body::from")[..] else {
        panic!("two calls");
    };
    let bodys = reg
        .by_qualified_name("src.body.Body.from")
        .first()
        .expect("Body's from")
        .clone();
    let others = reg
        .by_qualified_name("src.body.Other.from")
        .first()
        .expect("Other's from")
        .clone();
    for (got, what) in [(empty, "Body::from(())"), (text, "Body::from(\"text\")")] {
        assert_eq!(got.band(), Band::Candidate, "{what}: {got:?}");
        assert_eq!(got.target(), None, "{what}");
        assert!(got.candidates().contains(&bodys), "{what}: {got:?}");
        assert!(!got.candidates().contains(&others), "{what}: {got:?}");
    }
    // A type no macro writes for keeps its one `from`.
    let other = at(&reg, &report, "src/ext.rs", "Other::from");
    assert_eq!(other.band(), Band::ImportGuided, "{other:?}");
    assert_eq!(other.target(), Some(&others));
}

// --- E10: true positives stay typed ------------------------------------------------

#[test]
fn e10_typed_answers_with_their_evidence_stay_typed() {
    // A free function, an associated function, a method, a module-qualified function
    // and a trait method each keep their typed answer and strategy.
    let checkout = Checkout::new(&[
        CRATE,
        ("src/lib.rs", "pub mod tp;\npub mod util;\n"),
        (
            "src/util.rs",
            "pub fn helper(x: u32) -> u32 {\n    x + 1\n}\n",
        ),
        (
            "src/tp.rs",
            "use crate::util;\n\npub struct Counter {\n    n: u32,\n}\n\nimpl Counter {\n    pub fn new() -> Self {\n        Counter { n: 0 }\n    }\n\n    pub fn bump(&mut self) -> u32 {\n        self.n += 1;\n        self.n\n    }\n}\n\npub trait Describe {\n    fn describe(&self) -> u32;\n}\n\nimpl Describe for Counter {\n    fn describe(&self) -> u32 {\n        self.n\n    }\n}\n\nfn local(x: u32) -> u32 {\n    x\n}\n\npub fn run() -> u32 {\n    let mut c = Counter::new();\n    let a = c.bump();\n    let b = c.describe();\n    local(a) + util::helper(b)\n}\n",
        ),
    ]);
    let (reg, report) = checkout.resolved();
    let tp = "src/tp.rs";
    let method = |name: &str| -> DefinitionRef {
        let found: Vec<&DefinitionRef> =
            reg.by_name(name).iter().filter(|r| r.path == tp).collect();
        // `describe` is the trait's and the impl's: the impl's is the later one.
        found.last().copied().expect("defined").clone()
    };
    assert_typed(
        at(&reg, &report, tp, "Counter::new"),
        &method("new"),
        "lsp_constructor",
    );
    assert_typed(
        at(&reg, &report, tp, "c.bump"),
        &method("bump"),
        "lsp_method_dispatch",
    );
    assert_typed(
        at(&reg, &report, tp, "c.describe"),
        &method("describe"),
        "lsp_trait_dispatch",
    );
    assert_typed(
        at(&reg, &report, tp, "local"),
        &def(&reg, tp, "local"),
        "lsp_direct",
    );
    assert_typed(
        at(&reg, &report, tp, "util::helper"),
        &def(&reg, "src/util.rs", "helper"),
        "lsp_short_name_unique",
    );
}
