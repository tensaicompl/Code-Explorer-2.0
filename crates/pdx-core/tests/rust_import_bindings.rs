//! Issue 118: a name a Rust `use` brace group binds is the item its own path names, not
//! the group's module. Stage 3 read every name of `use a::{T, U};` as the module `a`, so
//! `T::f` was taken for `a::f`: `Performance::OutOfMemory(..)` was drawn `import-guided`
//! to the struct `OutOfMemory` beside the enum, and `Gauge::assemble()` to the module's
//! free `assemble`. Each binding of a group now leads where its own path does, as a
//! `use` of that one path would; `self` the group's module.
//!
//! An enum's variants are no definitions: a call of one has no target among the
//! repository's definitions, and is never `import-guided` to a namesake. What it falls
//! to instead is the same as for a single-name `use` of the enum: the repository's one
//! definition of the name as `exact` weak evidence, which since issue 102 (R3) draws no
//! edge. Those calls are R3's acceptance cases.
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
use pdx_core::resolve::registry::{DefinitionRef, ImportTarget, SymbolRegistry};
use pdx_core::resolve::stages::{Resolution, ResolveReport, resolve};
use pdx_engine::DefinitionKind;

const FILES: &[(&str, &str)] = &[
    (
        "Cargo.toml",
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    ),
    (
        "src/lib.rs",
        "#[macro_use]\npub mod macros;\n\npub mod e1;\npub mod e2;\npub mod e2_single;\npub mod e3;\npub mod e4;\npub mod e5;\npub mod e6;\npub mod kinds;\npub mod levels;\npub mod modules;\npub mod nested;\npub mod outer;\npub mod use_levels;\n",
    ),
    (
        "src/kinds.rs",
        "pub struct Stopped {\n    pub n: u8,\n}\n\npub struct Point(pub u8, pub u8);\n\npub enum Outcome {\n    Stopped(Stopped),\n    Halted(u8),\n}\n\npub struct Gauge {\n    pub n: u8,\n}\n\nimpl Gauge {\n    pub fn assemble(n: u8) -> Gauge {\n        Gauge { n }\n    }\n\n    pub fn reading(&self) -> u8 {\n        self.n\n    }\n}\n\npub fn assemble(n: u8) -> Gauge {\n    Gauge { n }\n}\n\n#[allow(non_snake_case)]\npub fn Halted(n: u8) -> u8 {\n    n\n}\n",
    ),
    (
        "src/e1.rs",
        "use crate::kinds::Outcome::Stopped;\nuse crate::kinds::Stopped as Payload;\n\npub fn e1() -> crate::kinds::Outcome {\n    Stopped(Payload { n: 1 })\n}\n",
    ),
    (
        "src/e2.rs",
        "use crate::kinds::{Outcome, Stopped};\n\npub fn e2() -> Outcome {\n    Outcome::Stopped(Stopped { n: 1 })\n}\n",
    ),
    (
        "src/e2_single.rs",
        "use crate::kinds::Outcome;\nuse crate::kinds::Stopped;\n\npub fn e2_single() -> Outcome {\n    Outcome::Stopped(Stopped { n: 1 })\n}\n",
    ),
    (
        "src/e3.rs",
        "use crate::kinds::{assemble, Gauge};\n\npub fn associated() -> Gauge {\n    Gauge::assemble(1)\n}\n\npub fn free() -> Gauge {\n    assemble(1)\n}\n",
    ),
    (
        "src/e4.rs",
        "use crate::kinds::{Gauge};\n\npub fn by_value(g: &Gauge) -> u8 {\n    g.reading()\n}\n\npub fn by_path(g: &Gauge) -> u8 {\n    Gauge::reading(g)\n}\n",
    ),
    (
        "src/e5.rs",
        "use crate::kinds::{Halted, Outcome};\n\npub fn variant() -> Outcome {\n    Outcome::Halted(1)\n}\n\npub fn function() -> u8 {\n    Halted(1)\n}\n",
    ),
    (
        "src/e6.rs",
        "use crate::kinds::{Point};\n\npub fn tuple() -> Point {\n    Point(1, 2)\n}\n",
    ),
    (
        "src/macros.rs",
        "#[macro_export]\nmacro_rules! level_enum {\n    ($name:ident) => {\n        pub enum $name {\n            Low,\n            High,\n        }\n    };\n}\n",
    ),
    (
        "src/levels.rs",
        "level_enum!(Level);\n\nimpl Level {\n    pub fn rank(&self) -> u8 {\n        match self {\n            Level::Low => 0,\n            Level::High => 1,\n        }\n    }\n}\n\npub fn rank(n: u8) -> u8 {\n    n\n}\n",
    ),
    (
        "src/use_levels.rs",
        "use crate::levels::{self, Level};\n\npub fn through_the_type(l: &Level) -> u8 {\n    Level::rank(l)\n}\n\npub fn through_the_module() -> u8 {\n    levels::rank(1)\n}\n",
    ),
    (
        "src/outer.rs",
        "pub mod inner;\n\npub fn assemble() -> u8 {\n    0\n}\n",
    ),
    (
        "src/outer/inner.rs",
        "pub struct Meter;\n\nimpl Meter {\n    pub fn assemble() -> Meter {\n        Meter\n    }\n}\n",
    ),
    (
        "src/nested.rs",
        "use crate::outer::{inner::{Meter}, self as o};\n\npub fn nested_group() -> Meter {\n    Meter::assemble()\n}\n\npub fn through_self() -> u8 {\n    o::assemble()\n}\n",
    ),
    (
        "src/modules.rs",
        "use crate::{kinds, kinds::Gauge as G};\n\npub fn through_the_module() -> kinds::Gauge {\n    kinds::assemble(1)\n}\n\npub fn through_an_alias() -> G {\n    G::assemble(1)\n}\n",
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
            .prefix("pdx-rust-import-bindings-test-")
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

/// What the one call site of `path` spelled `callee` resolved to.
fn at(path: &str, callee: &str) -> &'static Resolution {
    let r = resolved();
    let found: Vec<&Resolution> = r
        .report
        .resolutions
        .iter()
        .filter(|s| {
            s.site_ref.rel_path == path
                && r.reg
                    .text(path, &s.site.callee_text)
                    .is_some_and(|t| t == callee)
        })
        .map(|s| &s.resolution)
        .collect();
    assert_eq!(found.len(), 1, "{callee} in {path}: {found:?}");
    found[0]
}

/// The one definition of `kinds.rs` named `name`, declared in the type `ty` (`None`: at
/// the top of the module).
fn def(ty: Option<&str>, name: &str) -> DefinitionRef {
    let reg = &resolved().reg;
    let found: Vec<DefinitionRef> = reg
        .by_name(name)
        .iter()
        .filter(|r| {
            r.path == "src/kinds.rs"
                && reg
                    .declaring_type(r)
                    .and_then(|t| reg.definition(&t))
                    .map(|d| d.name.as_str())
                    == ty
        })
        .cloned()
        .collect();
    assert_eq!(found.len(), 1, "{ty:?}.{name}: {found:?}");
    found[0].clone()
}

fn assert_drawn(got: &Resolution, band: Band, target: &DefinitionRef) {
    assert_eq!(got.band(), band, "{got:?}");
    assert_eq!(got.target(), Some(target), "{got:?}");
}

/// Weak evidence (issue 102): `band` with `candidate` its one candidate, no target.
fn assert_weak(got: &Resolution, band: Band, candidate: &DefinitionRef) {
    assert_eq!(got.band(), band, "{got:?}");
    assert!(!band.is_drawn());
    assert_eq!(got.target(), None, "{got:?}");
    assert_eq!(got.weak_candidate(), Some(candidate), "{got:?}");
}

#[test]
fn e1_a_variant_imported_by_its_path_is_not_import_guided() {
    // `use crate::kinds::Outcome::Stopped;` binds the variant, which is no definition:
    // the import leads to no module's member. The struct `Stopped` is the variant's
    // payload, not its constructor. R3 acceptance case: `exact` evidence, no edge.
    let got = at("src/e1.rs", "Stopped");
    assert_ne!(got.band(), Band::ImportGuided, "{got:?}");
    assert_weak(got, Band::Exact, &def(None, "Stopped"));
}

#[test]
fn e2_a_variant_through_a_brace_group_is_not_the_modules_namesake() {
    // `use crate::kinds::{Outcome, Stopped};` binds `Outcome` to the enum: the path
    // `Outcome::Stopped` names one of its variants, never the module's struct of the
    // name. The issue's shape (`Performance::OutOfMemory`, `BuildOutcome::Stopped`).
    let got = at("src/e2.rs", "Outcome::Stopped");
    assert_ne!(got.band(), Band::ImportGuided, "{got:?}");
    // R3 acceptance case: as with a single-name `use` of the enum, the repository's one
    // `Stopped` is `exact` evidence and draws no edge.
    assert_weak(got, Band::Exact, &def(None, "Stopped"));
    assert_weak(
        at("src/e2_single.rs", "Outcome::Stopped"),
        Band::Exact,
        &def(None, "Stopped"),
    );
}

#[test]
fn e3_a_types_associated_function_through_a_brace_group_is_its_own() {
    // `Gauge::assemble` is `Gauge`'s associated function, never the module's free
    // `assemble`, which the group also imports and a bare call reaches.
    assert_drawn(
        at("src/e3.rs", "Gauge::assemble"),
        Band::ImportGuided,
        &def(Some("Gauge"), "assemble"),
    );
    let free = at("src/e3.rs", "assemble");
    assert_eq!(free.target(), Some(&def(None, "assemble")), "{free:?}");
}

#[test]
fn e4_an_inherent_method_through_a_brace_group_is_its_own() {
    assert_drawn(
        at("src/e4.rs", "Gauge::reading"),
        Band::ImportGuided,
        &def(Some("Gauge"), "reading"),
    );
    // A method called on a value: only the name's uniqueness, `exact` evidence, no edge
    // (issue 102).
    assert_weak(
        at("src/e4.rs", "g.reading"),
        Band::Exact,
        &def(Some("Gauge"), "reading"),
    );
}

#[test]
fn e5_a_variant_beside_a_function_of_its_name_is_not_the_function() {
    // `Outcome::Halted(1)` builds the variant; the module's `fn Halted` is reached by a
    // bare call only.
    let variant = at("src/e5.rs", "Outcome::Halted");
    assert_ne!(variant.band(), Band::ImportGuided, "{variant:?}");
    // R3 acceptance case: the repository's one `Halted` is `exact` evidence, no edge.
    assert_weak(variant, Band::Exact, &def(None, "Halted"));
    let function = at("src/e5.rs", "Halted");
    assert_eq!(
        function.target(),
        Some(&def(None, "Halted")),
        "{function:?}"
    );
    assert_ne!(function.band(), Band::Exact, "{function:?}");
}

#[test]
fn e6_a_tuple_struct_constructor_through_a_brace_group_stays_import_guided() {
    assert_drawn(
        at("src/e6.rs", "Point"),
        Band::ImportGuided,
        &def(None, "Point"),
    );
}

#[test]
fn a_module_a_group_binds_leads_to_its_own_items() {
    // `use crate::{kinds, kinds::Gauge as G};`: `kinds` is the module (the engine
    // answers its function), `G` its type.
    let through_the_module = at("src/modules.rs", "kinds::assemble");
    assert_eq!(
        through_the_module.target(),
        Some(&def(None, "assemble")),
        "{through_the_module:?}"
    );
    assert_drawn(
        at("src/modules.rs", "G::assemble"),
        Band::ImportGuided,
        &def(Some("Gauge"), "assemble"),
    );
}

/// The one definition of `path` named `name`, declared in the type `ty`.
fn def_in(path: &str, ty: Option<&str>, name: &str) -> DefinitionRef {
    let reg = &resolved().reg;
    let found: Vec<DefinitionRef> = reg
        .by_name(name)
        .iter()
        .filter(|r| {
            r.path == path
                && reg
                    .declaring_type(r)
                    .and_then(|t| reg.definition(&t))
                    .map(|d| d.name.as_str())
                    == ty
        })
        .cloned()
        .collect();
    assert_eq!(found.len(), 1, "{path}: {ty:?}.{name}: {found:?}");
    found[0].clone()
}

#[test]
fn a_nested_groups_item_leads_under_the_enclosing_groups_path() {
    // `use crate::outer::{inner::{Meter}, self as o};`: `Meter` is `outer::inner::Meter`,
    // `o` the module `outer` itself.
    assert_drawn(
        at("src/nested.rs", "Meter::assemble"),
        Band::ImportGuided,
        &def_in("src/outer/inner.rs", Some("Meter"), "assemble"),
    );
    let through_self = at("src/nested.rs", "o::assemble");
    assert_eq!(
        through_self.target(),
        Some(&def_in("src/outer.rs", None, "assemble")),
        "{through_self:?}"
    );
}

#[test]
fn a_member_of_a_type_no_definition_holds_is_no_import_guided_target() {
    // `Level` is written by a macro another file defines: no definition holds it, so its
    // members are not all known (issue 110), and `rank` beside it is the module's free
    // function, not the type's. Neither is drawn `import-guided` for `Level::rank`.
    let reg = &resolved().reg;
    let rank = |kind: DefinitionKind| {
        let found: Vec<&DefinitionRef> = reg
            .by_name("rank")
            .iter()
            .filter(|r| reg.definition(r).is_some_and(|d| d.kind == kind))
            .collect();
        assert_eq!(found.len(), 1, "{kind:?}: {found:?}");
        found[0].clone()
    };
    let (method, function) = (rank(DefinitionKind::Method), rank(DefinitionKind::Function));
    assert_eq!(
        reg.declaring_type(&method),
        None,
        "the type is no definition"
    );
    let got = at("src/use_levels.rs", "Level::rank");
    assert_ne!(got.band(), Band::ImportGuided, "{got:?}");
    assert_ne!(got.target(), Some(&function), "{got:?}");
    let free = at("src/use_levels.rs", "levels::rank");
    assert_eq!(free.target(), Some(&function), "{free:?}");
}

#[test]
fn a_groups_record_leads_each_binding_where_its_own_path_does() {
    let reg = &resolved().reg;
    let member = |t: &ImportTarget| match t {
        ImportTarget::Internal(t) => (t.files.clone(), t.member.clone()),
        other => panic!("{other:?}"),
    };
    let kinds = vec!["src/kinds.rs".to_owned()];
    let [e2] = reg.imports_of("src/e2.rs") else {
        panic!("one import");
    };
    assert_eq!(e2.bindings, ["Outcome", "Stopped"]);
    // The record's own target stays the group's module.
    assert_eq!(member(&e2.target), (kinds.clone(), None));
    for name in ["Outcome", "Stopped"] {
        assert_eq!(
            member(e2.target_of(name)),
            (kinds.clone(), Some(name.to_owned()))
        );
    }
    let [nested] = reg.imports_of("src/nested.rs") else {
        panic!("one import");
    };
    assert_eq!(nested.bindings, ["Meter", "o"]);
    assert_eq!(
        member(nested.target_of("Meter")),
        (
            vec!["src/outer/inner.rs".to_owned()],
            Some("Meter".to_owned())
        )
    );
    assert_eq!(
        member(nested.target_of("o")),
        (vec!["src/outer.rs".to_owned()], None)
    );
    // A single-name `use` has no per-binding target: its own is its binding's.
    for import in reg.imports_of("src/e2_single.rs") {
        assert!(import.binding_targets.is_empty(), "{import:?}");
        assert_eq!(import.target_of(&import.bindings[0]), &import.target);
    }
}
