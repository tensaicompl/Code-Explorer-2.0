//! Issue 117: Stage 3's trusted name stages, `import-guided` and `inheritance-guided`,
//! take a Rust trait impl's member for a call's target only when the impl is proven to
//! apply. Typed resolution answers with such a method only when the impl's header is
//! proven for the call's types (issue 109); a trusted stage reads no type arguments, so
//! it takes one only when the header requires nothing, or, for a call on `self` or
//! `Self` over the same type, when the impl the call is made from requires everything it
//! requires. Unknown is never proven: the site is a `candidate`.
//!
//! Every repository here is real: written to a checkout, discovered, extracted by the
//! engine and registered. The trusted stages are read from Stage 3 alone, with no typed
//! answer: since issue 105 typed resolution answers a call through an imported type in
//! another file, and a call on `self`, first. With it, nothing Stage 3 withholds is drawn
//! (`typed_resolution_draws_nothing_stage_3_withholds`).

use std::fs;
use std::path::Path;

use pdx_core::bands::Band;
use pdx_core::config::PdxConfig;
use pdx_core::index::discover::discover;
use pdx_core::index::extract::{ExtractLimits, ExtractStage, FileOutcome};
use pdx_core::resolve::registry::{DefinitionRef, SymbolRegistry};
use pdx_core::resolve::stages::{Resolution, ResolveReport, resolve, resolve_with};
use pdx_engine::{ExtractRecord, ProjectResolution, RunHealth, RunStatus};

const FILES: &[(&str, &str)] = &[
    (
        "Cargo.toml",
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    ),
    (
        "src/lib.rs",
        "pub mod body;\npub mod calls;\npub mod cells;\npub mod extract;\npub mod runs;\npub mod selfs;\npub mod target;\npub mod types;\npub mod use_body;\npub mod with_rejection;\n",
    ),
    (
        "src/types.rs",
        "pub trait Show {}\n\npub struct A;\npub struct B;\n\nimpl Show for A {}\n",
    ),
    // The imported types, one shape each.
    (
        "src/target.rs",
        "use crate::types::Show;\n\npub trait Make {\n    fn make() -> u8;\n}\n\npub trait Build {\n    fn build() -> u8;\n}\n\npub trait Pick {\n    fn pick() -> u8;\n}\n\npub trait Pick2 {\n    fn pick() -> u8;\n}\n\npub struct Inherent;\n\nimpl Inherent {\n    pub fn assemble() -> u8 {\n        1\n    }\n}\n\npub struct Open<X>(pub X);\n\nimpl<X> Make for Open<X> {\n    fn make() -> u8 {\n        1\n    }\n}\n\npub struct Bounded<X>(pub X);\n\nimpl<X: Show> Make for Bounded<X> {\n    fn make() -> u8 {\n        1\n    }\n}\n\npub struct Concrete<X>(pub X);\n\nimpl Make for Concrete<u8> {\n    fn make() -> u8 {\n        1\n    }\n}\n\npub struct Two<X>(pub X);\n\nimpl<X> Pick for Two<X> {\n    fn pick() -> u8 {\n        1\n    }\n}\n\nimpl<X: Show> Pick2 for Two<X> {\n    fn pick() -> u8 {\n        2\n    }\n}\n\npub struct Unknowns<X>(pub X);\n\nimpl<X: Show> Pick for Unknowns<X> {\n    fn pick() -> u8 {\n        1\n    }\n}\n\nimpl<X: Clone> Pick2 for Unknowns<X> {\n    fn pick() -> u8 {\n        2\n    }\n}\n\npub struct Mixed<X>(pub X);\n\nimpl<X> Mixed<X> {\n    pub fn build() -> u8 {\n        1\n    }\n}\n\nimpl<X: Show> Build for Mixed<X> {\n    fn build() -> u8 {\n        2\n    }\n}\n",
    ),
    (
        "src/calls.rs",
        "use crate::target::Bounded;\nuse crate::target::Concrete;\nuse crate::target::Inherent;\nuse crate::target::Mixed;\nuse crate::target::Open;\nuse crate::target::Two;\nuse crate::target::Unknowns;\n\npub fn i1() -> u8 {\n    Inherent::assemble()\n}\n\npub fn i2() -> u8 {\n    Open::make()\n}\n\npub fn i3() -> u8 {\n    Bounded::make()\n}\n\npub fn i5() -> u8 {\n    Concrete::make()\n}\n\npub fn i6() -> u8 {\n    Two::pick()\n}\n\npub fn i7() -> u8 {\n    Unknowns::pick()\n}\n\npub fn i8() -> u8 {\n    Mixed::build()\n}\n",
    ),
    // Calls on `self`: the impl the call is made from, and the impls it reaches.
    (
        "src/selfs.rs",
        "pub struct Wrap<H>(pub H);\n\npub trait Dup {\n    fn dup(&self) -> Self;\n    fn dup_again(&self) -> Self;\n}\n\nimpl<H: Copy> Dup for Wrap<H> {\n    fn dup(&self) -> Self {\n        Wrap(self.0)\n    }\n\n    fn dup_again(&self) -> Self {\n        self.dup()\n    }\n}\n\npub trait Greet {\n    fn greet(&self) -> u8 {\n        1\n    }\n}\n\nimpl<H: Copy> Greet for Wrap<H> {}\n\npub trait Twice {\n    fn twice(&self) -> u8;\n}\n\nimpl<H: Copy + Send> Twice for Wrap<H> {\n    fn twice(&self) -> u8 {\n        let _ = self.dup();\n        self.greet()\n    }\n}\n\npub trait Once {\n    fn once(&self) -> u8;\n}\n\nimpl<H> Once for Wrap<H> {\n    fn once(&self) -> u8 {\n        let _ = self.dup();\n        self.greet()\n    }\n}\n\nimpl<H: Copy> Wrap<H> {\n    pub fn plain(&self) -> Self {\n        self.dup()\n    }\n}\n",
    ),
    // Two impls of one trait for one type, their self types not written in their
    // headers (`s:other`): one header for both.
    (
        "src/cells.rs",
        "pub struct Cell<T>(pub T);\n\npub trait Twin {\n    fn one(&self) -> u8 {\n        0\n    }\n    fn two(&self) -> u8;\n}\n\nimpl Twin for Cell<u8> {\n    fn one(&self) -> u8 {\n        1\n    }\n    fn two(&self) -> u8 {\n        self.one()\n    }\n}\n\nimpl Twin for Cell<u16> {\n    fn two(&self) -> u8 {\n        self.one()\n    }\n}\n",
    ),
    // Calls made from the impl that declares their target, the impl's bound one no
    // predicate of another impl can be compared with (`Fn(..)`), and from another impl
    // of the trait (other trait arguments, another header).
    (
        "src/runs.rs",
        "pub struct Call<F>(pub F);\n\npub trait Fire<A> {\n    fn fire(&self) -> u8 {\n        0\n    }\n    fn fire_again(&self) -> u8;\n}\n\nimpl<F: Fn() -> u8> Fire<u8> for Call<F> {\n    fn fire(&self) -> u8 {\n        (self.0)()\n    }\n\n    fn fire_again(&self) -> u8 {\n        self.fire()\n    }\n}\n\nimpl<F> Fire<u16> for Call<F> {\n    fn fire_again(&self) -> u8 {\n        self.fire()\n    }\n}\n",
    ),
    // Issue 117's reproductions.
    (
        "src/extract.rs",
        "pub struct ViaRequest;\npub struct ViaParts;\n\npub trait FromRequestParts<S> {\n    fn from_request_parts(parts: u32, state: &S) -> Self;\n}\n\npub trait FromRequest<S, M = ViaRequest> {\n    fn from_request(req: u32, state: &S) -> Self;\n}\n\nimpl<S, T> FromRequest<S, ViaParts> for T\nwhere\n    T: FromRequestParts<S>,\n{\n    fn from_request(req: u32, state: &S) -> Self {\n        T::from_request_parts(req, state)\n    }\n}\n",
    ),
    (
        "src/with_rejection.rs",
        "use crate::extract::{FromRequest, FromRequestParts};\n\npub struct WithRejection<E>(pub E);\n\nimpl<E, S> FromRequest<S> for WithRejection<E>\nwhere\n    E: FromRequest<S>,\n{\n    fn from_request(req: u32, state: &S) -> Self {\n        WithRejection(E::from_request(req, state))\n    }\n}\n\nimpl<E, S> FromRequestParts<S> for WithRejection<E>\nwhere\n    E: FromRequestParts<S>,\n{\n    fn from_request_parts(req: u32, state: &S) -> Self {\n        WithRejection(E::from_request_parts(req, state))\n    }\n}\n\npub struct Extractor;\n\nimpl<S> FromRequestParts<S> for Extractor {\n    fn from_request_parts(_parts: u32, _state: &S) -> Self {\n        Extractor\n    }\n}\n\nimpl<E: FromRequestParts<()>> WithRejection<E> {\n    pub fn through_self() -> Self {\n        Self::from_request(1, &())\n    }\n}\n",
    ),
    (
        "src/body.rs",
        "pub struct Body<T>(pub T);\n\nimpl<T: Copy> From<()> for Body<T> {\n    fn from(_: ()) -> Self {\n        unimplemented!()\n    }\n}\n\npub struct Plain;\n\nimpl From<()> for Plain {\n    fn from(_: ()) -> Self {\n        Plain\n    }\n}\n",
    ),
    (
        "src/use_body.rs",
        "use crate::body::Body;\nuse crate::body::Plain;\nuse crate::with_rejection::Extractor;\nuse crate::with_rejection::WithRejection;\n\npub fn conditional() -> Body<u8> {\n    Body::from(())\n}\n\npub fn unconditional() -> Plain {\n    Plain::from(())\n}\n\npub fn with_rejection() -> WithRejection<Extractor> {\n    WithRejection::from_request(1, &())\n}\n",
    ),
];

struct Resolved {
    _dir: tempfile::TempDir,
    reg: SymbolRegistry,
    /// Stage 3 alone: no typed answer.
    report: ResolveReport,
    /// With typed resolution.
    full: ResolveReport,
}

/// `FILES` resolved, each extraction record passed through `edit` before the registry
/// reads it.
fn build(edit: fn(&mut ExtractRecord)) -> Resolved {
    let dir = tempfile::Builder::new()
        .prefix("pdx-rust-trusted-impls-test-")
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
    let mut report = ExtractStage::new(root, &config.secrets, limits)
        .run(&files)
        .expect("extraction succeeds");
    for file in &mut report.files {
        if let FileOutcome::Extracted { extract, .. } = &mut file.outcome {
            let mut record = extract.record().clone();
            edit(&mut record);
            **extract = record
                .bind(extract.shared_source())
                .expect("the record binds");
        }
    }
    let reg = SymbolRegistry::build(root, &files, report).expect("the registry builds");
    let full = resolve(root, &reg).expect("resolution succeeds");
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
    let report = resolve_with(
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
        full,
        report,
    }
}

fn resolved() -> &'static Resolved {
    static RESOLVED: std::sync::OnceLock<Resolved> = std::sync::OnceLock::new();
    RESOLVED.get_or_init(|| build(|_| {}))
}

/// `FILES` as an extraction that recorded no impl header would give them: every trait
/// impl's method and relation with its trait and no header.
fn unrecorded() -> &'static Resolved {
    static RESOLVED: std::sync::OnceLock<Resolved> = std::sync::OnceLock::new();
    RESOLVED.get_or_init(|| {
        build(|record| {
            for definition in &mut record.definitions {
                definition.impl_bounds.clear();
            }
            for relation in &mut record.impl_traits {
                relation.impl_bounds.clear();
            }
        })
    })
}

/// What the call sites of `path` spelled `callee` resolved to, in source order.
fn all_at(path: &str, callee: &str) -> Vec<&'static Resolution> {
    all_in(resolved(), path, callee)
}

/// As [`all_at`], in `r`.
fn all_in(r: &'static Resolved, path: &str, callee: &str) -> Vec<&'static Resolution> {
    let mut found: Vec<_> = r
        .report
        .resolutions
        .iter()
        .filter(|s| {
            s.site_ref.rel_path == path
                && r.reg
                    .text(path, &s.call(&r.reg).expect("a call").callee_text)
                    .is_some_and(|t| t == callee)
        })
        .collect();
    found.sort_by_key(|s| {
        s.call(&r.reg)
            .and_then(|c| c.span)
            .map(|span| span.start_byte)
    });
    found.into_iter().map(|s| &s.resolution).collect()
}

fn at(path: &str, callee: &str) -> &'static Resolution {
    at_in(resolved(), path, callee)
}

/// As [`at`], in `r`.
fn at_in(r: &'static Resolved, path: &str, callee: &str) -> &'static Resolution {
    let found = all_in(r, path, callee);
    assert_eq!(found.len(), 1, "{callee} in {path}: {found:?}");
    found[0]
}

/// The definitions named `name` declared in the type `ty` of `path`.
fn members(path: &str, ty: &str, name: &str) -> Vec<DefinitionRef> {
    let reg = &resolved().reg;
    reg.by_name(name)
        .iter()
        .filter(|r| {
            r.path == path
                && reg
                    .declaring_type(r)
                    .and_then(|t| reg.definition(&t))
                    .is_some_and(|d| d.name == ty)
        })
        .cloned()
        .collect()
}

fn one(path: &str, ty: &str, name: &str) -> DefinitionRef {
    let found = members(path, ty, name);
    assert_eq!(found.len(), 1, "{ty}.{name} in {path}: {found:?}");
    found[0].clone()
}

fn assert_drawn(got: &Resolution, band: Band, target: &DefinitionRef) {
    assert_eq!(got.band(), band, "{got:?}");
    assert_eq!(got.target(), Some(target), "{got:?}");
}

/// No trusted edge: a `candidate` holding `target`, never drawn to it.
fn assert_withheld(got: &Resolution, target: &DefinitionRef) {
    assert_eq!(got.band(), Band::Candidate, "{got:?}");
    assert_eq!(got.target(), None, "{got:?}");
    assert!(got.candidates().contains(target), "{got:?}");
}

const TARGET: &str = "src/target.rs";
const CALLS: &str = "src/calls.rs";

#[test]
fn i1_an_imported_types_inherent_method_stays_import_guided() {
    assert_drawn(
        at(CALLS, "Inherent::assemble"),
        Band::ImportGuided,
        &one(TARGET, "Inherent", "assemble"),
    );
}

#[test]
fn i2_an_unconditional_trait_impls_method_stays_import_guided() {
    assert_drawn(
        at(CALLS, "Open::make"),
        Band::ImportGuided,
        &one(TARGET, "Open", "make"),
    );
}

#[test]
fn i3_i4_a_conditional_impls_method_is_not_import_guided() {
    // `impl<X: Show> Make for Bounded<X>`: whether `X: Show` holds is the call's type
    // argument's question, which a trusted stage does not read. Satisfied or not, the
    // bound is unknown here.
    assert_withheld(at(CALLS, "Bounded::make"), &one(TARGET, "Bounded", "make"));
}

#[test]
fn i5_an_impl_for_a_concrete_argument_is_not_import_guided() {
    // `impl Make for Concrete<u8>` applies to one instance of `Concrete` only.
    assert_withheld(
        at(CALLS, "Concrete::make"),
        &one(TARGET, "Concrete", "make"),
    );
}

#[test]
fn i6_i7_two_impls_of_the_name_are_no_trusted_edge() {
    // One impl requires nothing and the other a bound (I6), or both bounds (I7): two
    // members of one name, one qualified name; no trusted stage chooses between them.
    for (callee, ty) in [("Two::pick", "Two"), ("Unknowns::pick", "Unknowns")] {
        let got = at(CALLS, callee);
        assert_eq!(got.band(), Band::Candidate, "{callee}: {got:?}");
        for member in members(TARGET, ty, "pick") {
            assert!(got.candidates().contains(&member), "{callee}: {got:?}");
        }
    }
}

#[test]
fn i8_an_inherent_method_beside_a_conditional_impls_is_no_trusted_edge() {
    let got = at(CALLS, "Mixed::build");
    assert_eq!(got.band(), Band::Candidate, "{got:?}");
    assert_eq!(members(TARGET, "Mixed", "build").len(), 2);
}

#[test]
fn the_reproductions_are_no_trusted_edge() {
    // `use crate::with_rejection::WithRejection;` then `WithRejection::from_request`:
    // the type's own `FromRequest` impl needs `E: FromRequest<S>`; and
    // `impl<T: Copy> From<()> for Body<T>` needs `T: Copy`. Both were drawn
    // `import-guided` before issue 117.
    let use_body = "src/use_body.rs";
    assert_withheld(
        at(use_body, "WithRejection::from_request"),
        &one("src/with_rejection.rs", "WithRejection", "from_request"),
    );
    assert_withheld(
        at(use_body, "Body::from"),
        &one("src/body.rs", "Body", "from"),
    );
    // An impl that requires nothing is still `import-guided`.
    assert_drawn(
        at(use_body, "Plain::from"),
        Band::ImportGuided,
        &one("src/body.rs", "Plain", "from"),
    );
}

#[test]
fn a_call_on_self_is_inheritance_guided_only_when_its_impl_entails_the_targets() {
    let selfs = "src/selfs.rs";
    let dup = one(selfs, "Wrap", "dup");
    let greet = {
        let found: Vec<DefinitionRef> = resolved()
            .reg
            .by_name("greet")
            .iter()
            .filter(|r| r.path == selfs)
            .cloned()
            .collect();
        assert_eq!(found.len(), 1, "{found:?}");
        found[0].clone()
    };
    // `impl<H: Copy + Send> Twice for Wrap<H>` requires `H: Copy`, which `Dup`'s impl
    // and `Greet`'s (the trait's default, through the relation) require: proven.
    let in_twice = all_at(selfs, "self.dup");
    let greets = all_at(selfs, "self.greet");
    assert_eq!((in_twice.len(), greets.len()), (4, 2));
    // Source order: `dup_again`'s, `twice`'s, `once`'s, `plain`'s.
    assert_drawn(in_twice[0], Band::InheritanceGuided, &dup);
    assert_drawn(in_twice[1], Band::InheritanceGuided, &dup);
    assert_drawn(greets[0], Band::InheritanceGuided, &greet);
    // `impl<H> Once for Wrap<H>` requires nothing of `H`: `H: Copy` is unknown there.
    assert_withheld(in_twice[2], &dup);
    assert_withheld(greets[1], &greet);
    // An inherent impl's header is not recorded: unknown.
    assert_withheld(in_twice[3], &dup);
}

#[test]
fn self_through_an_unproven_own_impl_is_no_trusted_edge() {
    // Inside `impl<E: FromRequestParts<()>> WithRejection<E>`, `Self::from_request`
    // reaches the blanket impl through `FromRequestParts`; the type's own `FromRequest`
    // impl needs `E: FromRequest<S>`, unknown there. The typed pass gives no answer
    // (before issue 109 as after); `inheritance-guided` must not draw it.
    let got = at("src/with_rejection.rs", "Self::from_request");
    assert!(got.engine().is_none(), "{got:?}");
    assert_withheld(
        got,
        &one("src/with_rejection.rs", "WithRejection", "from_request"),
    );
}

#[test]
fn a_trait_impls_member_with_no_recorded_header_is_no_trusted_edge() {
    // The same repository, its headers not recorded: nothing proves an impl applies,
    // not even one that requires nothing, nor the impl a call on `self` is made from.
    let r = unrecorded();
    let [plain] = all_in(r, "src/use_body.rs", "Plain::from")[..] else {
        panic!("one site");
    };
    assert_withheld(plain, &one("src/body.rs", "Plain", "from"));
    let dup = one("src/selfs.rs", "Wrap", "dup");
    for got in all_in(r, "src/selfs.rs", "self.dup") {
        assert_withheld(got, &dup);
    }
    // A trait's own method, through a relation with no header.
    for got in all_in(r, "src/selfs.rs", "self.greet") {
        assert_eq!(got.band(), Band::Candidate, "{got:?}");
        assert_eq!(got.target(), None, "{got:?}");
    }
    // An inherent member keeps its band: it has no trait impl's header to prove.
    assert_drawn(
        at_in(r, CALLS, "Inherent::assemble"),
        Band::ImportGuided,
        &one(TARGET, "Inherent", "assemble"),
    );
}

#[test]
fn impls_whose_headers_do_not_write_their_self_types_are_not_one_impl() {
    // `self.one()` in `impl Twin for Cell<u16>` is the trait's default; `Cell<u8>`'s
    // `one` is another impl's, with the same header. Neither call is proven to be
    // made from the impl that declares `Cell`'s one `one`.
    let one = one("src/cells.rs", "Cell", "one");
    let found = all_at("src/cells.rs", "self.one");
    assert_eq!(found.len(), 2, "{found:?}");
    for got in found {
        assert_withheld(got, &one);
    }
}

#[test]
fn a_call_from_the_impl_that_declares_its_target_is_inheritance_guided() {
    // `F: Fn() -> u8` is no bound entailment compares; the impl the call is made from
    // is the target's own, which applies wherever its methods run. From `impl<F>
    // Fire<u16> for Call<F>`, the same trait spelled alike, the header is another:
    // `Fire<u8>`'s impl is unknown there.
    let fire = one("src/runs.rs", "Call", "fire");
    let found = all_at("src/runs.rs", "self.fire");
    assert_eq!(found.len(), 2, "{found:?}");
    assert_drawn(found[0], Band::InheritanceGuided, &fire);
    assert_withheld(found[1], &fire);
}

#[test]
fn typed_resolution_draws_nothing_stage_3_withholds() {
    // With typed resolution, a site is drawn to what Stage 3 alone draws it to, or to a
    // member typed resolution proves; a member Stage 3 withholds as unproven is never
    // drawn (issue 109's proof is the typed pass's too). Every withheld site of this
    // repository is checked, and the fixture holds some of each.
    let r = resolved();
    let full: std::collections::HashMap<_, _> = r
        .full
        .resolutions
        .iter()
        .map(|s| (&s.site_ref, &s.resolution))
        .collect();
    let (mut withheld, mut drawn) = (0, 0);
    for site in &r.report.resolutions {
        let alone = &site.resolution;
        let with_typed = full[&site.site_ref];
        if let Some(target) = alone.target() {
            drawn += 1;
            assert_eq!(with_typed.target(), Some(target), "{:?}", site.site_ref);
        } else if alone.band() == Band::Candidate {
            withheld += 1;
            assert!(
                with_typed
                    .target()
                    .is_none_or(|t| !alone.candidates().contains(t)),
                "{:?}: {with_typed:?}",
                site.site_ref
            );
        }
    }
    assert!(
        withheld >= 8 && drawn >= 6,
        "{withheld} withheld, {drawn} drawn"
    );
}
