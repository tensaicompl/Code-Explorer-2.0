//! Issue 109: a typed answer for a Rust `Type::method` names a trait impl's method only
//! when the impl is proven to apply. `Type::method` reaches a trait's method through an
//! impl of the trait for the type, and an impl applies to a type only when what its
//! header requires holds for it: the engine took the type's one impl with a method of
//! the name, whatever its header required, and drew it typed.
//!
//! Every repository here is real: written to a checkout, discovered, extracted by the
//! engine, registered and resolved with typed resolution itself, so each answer is the
//! engine's own and each band Stage 3's. The headers and the answers in every shape are
//! `engine/tests/rust_impl_bounds.c`'s; the surface's, `crates/pdx-engine/tests/
//! impl_bounds.rs`'s. Here, what Stage 3 makes of them, and that an extraction cache
//! changes none of it. The issue's own site is `rust_targets.rs`'s E6.

use std::fs;
use std::path::Path;

use pdx_core::bands::Band;
use pdx_core::config::PdxConfig;
use pdx_core::index::discover::discover;
use pdx_core::index::extract::{ExtractLimits, ExtractStage, FsCache};
use pdx_core::resolve::registry::{DefinitionRef, SymbolRegistry};
use pdx_core::resolve::stages::{Resolution, ResolveReport, resolve};

const FILES: &[(&str, &str)] = &[
    (
        "Cargo.toml",
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    ),
    ("src/lib.rs", "pub mod extract;\npub mod make;\n"),
    (
        "src/make.rs",
        "pub trait Show {}\n\npub struct A;\npub struct B;\n\nimpl Show for A {}\n\npub trait Make {\n    fn make() -> u8;\n}\n\npub trait Tag {\n    fn tag() -> u8;\n}\n\npub struct W<X>(pub X);\n\nimpl<X: Show> Make for W<X> {\n    fn make() -> u8 {\n        1\n    }\n}\n\nimpl<X> Tag for W<X> {\n    fn tag() -> u8 {\n        2\n    }\n}\n\npub fn proven() -> u8 {\n    W::<A>::make()\n}\n\npub fn unconditional() -> u8 {\n    W::<B>::tag()\n}\n\npub fn unproven() -> u8 {\n    W::<B>::make()\n}\n\npub fn untold() -> u8 {\n    W::make()\n}\n",
    ),
    // The engine differential's site (axum-core/src/extract/mod.rs:129), its shape.
    (
        "src/extract.rs",
        "pub struct ViaRequest;\npub struct ViaParts;\n\npub trait FromRequestParts<S> {\n    fn from_request_parts(parts: u32, state: &S) -> Self;\n}\n\npub trait FromRequest<S, M = ViaRequest> {\n    fn from_request(req: u32, state: &S) -> Self;\n}\n\nimpl<S, T> FromRequest<S, ViaParts> for T\nwhere\n    S: Send + Sync,\n    T: FromRequestParts<S>,\n{\n    fn from_request(req: u32, state: &S) -> Self {\n        T::from_request_parts(req, state)\n    }\n}\n\nimpl<S, T> FromRequest<S> for Option<T>\nwhere\n    T: FromRequest<S>,\n    S: Send + Sync,\n{\n    fn from_request(req: u32, state: &S) -> Self {\n        Some(T::from_request(req, state))\n    }\n}\n",
    ),
];

struct Checkout {
    dir: tempfile::TempDir,
}

impl Checkout {
    fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("pdx-rust-impl-bounds-test-")
            .tempdir()
            .expect("a directory");
        for (path, content) in FILES {
            let path = dir.path().join(path);
            fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
            fs::write(&path, content).expect("a file");
        }
        Self { dir }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    /// The registry and Stage 3's report, typed resolution included, extracted through
    /// `cache` when given; with how many files the cache served.
    fn resolved(&self, cache: Option<&FsCache>) -> (SymbolRegistry, ResolveReport, u64) {
        let config = PdxConfig::load(self.root()).expect("the configuration loads");
        let files = discover(self.root(), &config).expect("discovery succeeds");
        let limits = ExtractLimits {
            requested_workers: 2,
            memory_budget_bytes: 64 << 20,
        };
        let mut stage = ExtractStage::new(self.root(), &config.secrets, limits);
        if let Some(cache) = cache {
            stage = stage.with_cache(cache);
        }
        let extracted = stage.run(&files).expect("extraction succeeds");
        let hits = extracted.stats.cache_hits;
        let reg =
            SymbolRegistry::build(self.root(), &files, extracted).expect("the registry builds");
        let report = resolve(self.root(), &reg).expect("resolution succeeds");
        (reg, report, hits)
    }
}

/// What the one call site of `path` spelled `callee` resolved to.
fn at<'r>(
    reg: &SymbolRegistry,
    report: &'r ResolveReport,
    path: &str,
    callee: &str,
) -> &'r Resolution {
    let found: Vec<&Resolution> = report
        .resolutions
        .iter()
        .filter(|s| {
            s.site_ref.rel_path == path
                && reg
                    .text(path, &s.site.callee_text)
                    .is_some_and(|t| t == callee)
        })
        .map(|s| &s.resolution)
        .collect();
    assert_eq!(found.len(), 1, "{callee} in {path}: {found:?}");
    found[0]
}

/// The definitions named `name` in `path`, in their order: a trait's declaration of a
/// method before an impl's.
fn defs(reg: &SymbolRegistry, path: &str, name: &str) -> Vec<DefinitionRef> {
    reg.by_name(name)
        .iter()
        .filter(|r| r.path == path)
        .cloned()
        .collect()
}

/// Not typed, by no engine answer, and settled with these candidates.
fn assert_withheld(got: &Resolution, candidates: &[DefinitionRef]) {
    assert_eq!(got.band(), Band::Candidate, "{got:?}");
    assert!(got.engine().is_none(), "{got:?}");
    assert_eq!(got.target(), None, "{got:?}");
    assert_eq!(got.candidates(), candidates, "{got:?}");
}

fn check(reg: &SymbolRegistry, report: &ResolveReport) {
    let make = "src/make.rs";
    let makes = defs(reg, make, "make");
    let tags = defs(reg, make, "tag");
    assert_eq!((makes.len(), tags.len()), (2, 2), "{makes:?} {tags:?}");

    // T2: `X: Show` is proven for `A`, by its unconditional impl.
    let got = at(reg, report, make, "W::<A>::make");
    assert_eq!(got.band(), Band::Typed, "{got:?}");
    assert_eq!(got.target(), Some(&makes[1]), "{got:?}");
    assert_eq!(got.engine_strategy(), Some("lsp_ufcs"), "{got:?}");
    // T1: an impl that requires nothing applies to every `W`.
    let got = at(reg, report, make, "W::<B>::tag");
    assert_eq!(got.band(), Band::Typed, "{got:?}");
    assert_eq!(got.target(), Some(&tags[1]), "{got:?}");
    // T4: `B` implements no `Show`. T5: the path gives no type argument.
    assert_withheld(at(reg, report, make, "W::<B>::make"), &makes);
    assert_withheld(at(reg, report, make, "W::make"), &makes);

    // The engine differential's site: the impl's `T` is not the blanket impl's by its
    // name; no answer, the reference's `lsp_ufcs` ratified away (decision 66). The three
    // `from_request`s stay its candidates.
    let extract = "src/extract.rs";
    assert_withheld(
        at(reg, report, extract, "T::from_request"),
        &defs(reg, extract, "from_request"),
    );
}

#[test]
fn proven_impls_are_typed_and_unproven_ones_withheld() {
    let checkout = Checkout::new();
    let (reg, report, _) = checkout.resolved(None);
    check(&reg, &report);
}

#[test]
fn a_warm_extraction_cache_resolves_as_a_cold_one() {
    // The second run reads every extraction, its surface included, from the cache: the
    // headers the proofs need come from surface 6 there. Both runs answer alike.
    let checkout = Checkout::new();
    let cache_dir = tempfile::Builder::new()
        .prefix("pdx-rust-impl-bounds-cache-")
        .tempdir()
        .expect("a directory");
    let cache = FsCache::at(cache_dir.path().join("extract"));
    let (cold_reg, cold, cold_hits) = checkout.resolved(Some(&cache));
    let (warm_reg, warm, warm_hits) = checkout.resolved(Some(&cache));
    assert_eq!(cold_hits, 0);
    assert!(warm_hits >= 3, "the cache served {warm_hits} files");
    assert_eq!(warm, cold, "a warm cache resolves otherwise");
    check(&cold_reg, &cold);
    check(&warm_reg, &warm);
}
