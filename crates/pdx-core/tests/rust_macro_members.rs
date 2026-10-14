//! Issue 110: an item a `macro_rules!` invocation writes is no definition, so a type
//! whose members a macro writes in part has only its handwritten ones in the registry,
//! and Stage 3 drew a call to the one member of the name it held: `Body::from("…")` to
//! the handwritten `From<()>` impl when a macro writes `From<&'static str>`. The engine
//! now records the members each invocation may write (patch 0030), and a member a stage
//! settles on is no target while a macro may write another of its name for the type the
//! call names or the member is declared in (decision 65).
//!
//! One real repository, written to a checkout, extracted by the engine, registered and
//! resolved. Nothing here names a library or a type of one: the shapes are the issue's,
//! generic.

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
        "[package]\nname = \"macroimpl\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    ),
    (
        "src/lib.rs",
        "pub mod gadget;\npub mod gizmo;\npub mod other;\npub mod payload;\npub mod plain;\npub mod thing;\npub mod user;\npub mod widget;\n",
    ),
    // The issue's shape: a handwritten impl beside the ones a macro writes, the type
    // written literally in the transcriber.
    (
        "src/payload.rs",
        "pub struct Payload(String);\n\nimpl Payload {\n    pub fn new(s: String) -> Self {\n        Payload(s)\n    }\n\n    pub fn unit() -> Self {\n        Self::from(())\n    }\n\n    pub fn seven() -> Self {\n        Self::from(7u32)\n    }\n}\n\nimpl From<()> for Payload {\n    fn from(_: ()) -> Self {\n        Payload::new(String::new())\n    }\n}\n\nmacro_rules! make_impl {\n    ($ty:ty) => {\n        impl From<$ty> for Payload {\n            fn from(v: $ty) -> Self {\n                Payload::new(v.to_string())\n            }\n        }\n    };\n}\n\nmake_impl!(u32);\nmake_impl!(&'static str);\n",
    ),
    // The type named by a metavariable: the invocation's argument.
    (
        "src/gadget.rs",
        "pub struct Gadget;\n\nimpl From<()> for Gadget {\n    fn from(_: ()) -> Self {\n        Gadget\n    }\n}\n\nmacro_rules! via {\n    ($t:ident) => {\n        impl From<u8> for $t {\n            fn from(_: u8) -> Self {\n                $t\n            }\n        }\n    };\n}\n\nvia!(Gadget);\n",
    ),
    // A member named by a metavariable: what the invocation binds it to.
    (
        "src/widget.rs",
        "pub struct Widget;\n\nimpl Widget {\n    pub fn beta() -> u8 {\n        1\n    }\n}\n\nmacro_rules! named {\n    ($n:ident) => {\n        impl Widget {\n            pub fn $n() -> u8 {\n                0\n            }\n        }\n    };\n}\n\nnamed!(alpha);\n",
    ),
    // A member name the transcriber generates: any member of the type.
    (
        "src/gizmo.rs",
        "pub struct Gizmo;\n\nimpl Gizmo {\n    pub fn shown() -> u8 {\n        1\n    }\n}\n\nmacro_rules! accessors {\n    ($t:ident) => {\n        impl Gizmo {\n            pub fn [<get_ $t>]() -> u8 {\n                0\n            }\n        }\n    };\n}\n\naccessors!(size);\n",
    ),
    // An invocation in an impl body writes the impl type's member.
    (
        "src/thing.rs",
        "pub struct Thing;\n\nmacro_rules! getter {\n    () => {\n        pub fn peek(&self) -> u8 {\n            1\n        }\n    };\n}\n\nimpl Thing {\n    getter!();\n}\n",
    ),
    // The only definitions of `alpha` and `peek`: what `exact` would draw to.
    (
        "src/other.rs",
        "pub struct Other;\n\nimpl Other {\n    pub fn alpha() -> u8 {\n        2\n    }\n\n    pub fn peek(&self) -> u8 {\n        3\n    }\n}\n",
    ),
    // A macro no invocation sees writes nothing.
    (
        "src/plain.rs",
        "pub struct Plain;\n\nimpl From<()> for Plain {\n    fn from(_: ()) -> Self {\n        Plain\n    }\n}\n\nmacro_rules! spare {\n    () => {\n        impl From<u8> for Plain {\n            fn from(_: u8) -> Self {\n                Plain\n            }\n        }\n    };\n}\n",
    ),
    (
        "src/user.rs",
        "use crate::gadget::Gadget;\nuse crate::gizmo::Gizmo;\nuse crate::payload::Payload;\nuse crate::plain::Plain;\nuse crate::thing::Thing;\nuse crate::widget::Widget;\n\npub fn calls() -> u8 {\n    let _ = Payload::from(());\n    let _ = Payload::from(\"x\");\n    let _ = Payload::from(7u32);\n    let _ = Payload::new(String::new());\n    let _ = Gadget::from(1u8);\n    let _ = Plain::from(());\n    Widget::alpha() + Widget::beta() + Thing::peek(&Thing) + Gizmo::shown()\n}\n",
    ),
    // Another language's type of a name a Rust macro writes for.
    (
        "tools/widget.py",
        "class Widget:\n    def alpha(self):\n        return 1\n\n\ndef use_it():\n    return Widget().alpha()\n",
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
            .prefix("pdx-rust-macro-members-test-")
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

fn at(path: &str, callee: &str) -> &'static Resolution {
    let found = all_at(path, callee);
    assert_eq!(found.len(), 1, "{callee} in {path}: {found:?}");
    found[0]
}

/// Not drawn: a candidate, `target` among the candidates.
fn withheld(got: &Resolution, target: &DefinitionRef, what: &str) {
    assert_eq!(got.band(), Band::Candidate, "{what}: {got:?}");
    assert_eq!(got.target(), None, "{what}");
    assert!(got.candidates().contains(target), "{what}: {got:?}");
}

#[test]
fn the_registry_knows_what_macros_may_write() {
    let reg = &resolved().reg;
    assert!(reg.macro_may_write("Payload", "from"));
    assert!(!reg.macro_may_write("Payload", "new"));
    assert!(reg.macro_may_write("Gadget", "from"));
    assert!(reg.macro_may_write("Widget", "alpha"));
    assert!(!reg.macro_may_write("Widget", "beta"));
    assert!(reg.macro_may_write("Thing", "peek"));
    assert!(!reg.macro_may_write("Thing", "put"));
    assert!(
        !reg.macro_may_write("Plain", "from"),
        "an uninvoked macro writes nothing"
    );
    assert!(!reg.macro_may_write("Other", "alpha"));
    assert!(
        reg.macro_may_write("Gizmo", "shown"),
        "a generated name is any member"
    );
}

#[test]
fn the_one_handwritten_member_is_no_target_beside_macro_written_ones() {
    // The issue's sites: `Payload::from("x")` and `Payload::from(7u32)` mean the impls
    // the macro writes, and were drawn `import-guided` to the handwritten one.
    let from = def("src/payload.rs", "from");
    let sites = all_at("src/user.rs", "Payload::from");
    assert_eq!(sites.len(), 3);
    withheld(sites[1], &from, "Payload::from(\"x\")");
    withheld(sites[2], &from, "Payload::from(7u32)");
    // `Payload::from(())` does mean the handwritten impl, and nothing proves it: which
    // impl a call of the name means is not told by the registry's members. Withheld too.
    withheld(sites[0], &from, "Payload::from(())");
    // Inside the type, typed resolution answers `Self::from(7u32)` with the one `from`
    // it knows too: no proven target either, whatever answered.
    let inside = all_at("src/payload.rs", "Self::from");
    assert_eq!(inside.len(), 2);
    for (got, what) in inside
        .into_iter()
        .zip(["Self::from(())", "Self::from(7u32)"])
    {
        assert!(
            got.engine_score()
                .is_some_and(|score| score >= pdx_core::consts::TYPED_MIN_SCORE),
            "{what}: typed resolution answered: {got:?}"
        );
        withheld(got, &from, what);
    }
}

#[test]
fn a_member_no_macro_writes_is_still_drawn() {
    let got = at("src/user.rs", "Payload::new");
    assert_eq!(got.band(), Band::ImportGuided, "{got:?}");
    assert_eq!(got.target(), Some(&def("src/payload.rs", "new")));
    // A macro no invocation sees writes nothing: the handwritten impl is drawn.
    let got = at("src/user.rs", "Plain::from");
    assert_eq!(got.band(), Band::ImportGuided, "{got:?}");
    assert_eq!(got.target(), Some(&def("src/plain.rs", "from")));
}

#[test]
fn a_type_or_member_named_by_a_metavariable() {
    withheld(
        at("src/user.rs", "Gadget::from"),
        &def("src/gadget.rs", "from"),
        "Gadget::from",
    );
    // `Widget::alpha` is written by the macro; the repository's one `alpha` is
    // another type's, which `exact` drew to.
    withheld(
        at("src/user.rs", "Widget::alpha"),
        &def("src/other.rs", "alpha"),
        "Widget::alpha",
    );
    // The macro writes `alpha`, what its metavariable is bound to: `Widget`'s
    // handwritten `beta` is still its one `beta`.
    let got = at("src/user.rs", "Widget::beta");
    assert_eq!(got.band(), Band::ImportGuided, "{got:?}");
    assert_eq!(got.target(), Some(&def("src/widget.rs", "beta")));
}

#[test]
fn a_generated_member_name_leaves_every_member_unproven() {
    withheld(
        at("src/user.rs", "Gizmo::shown"),
        &def("src/gizmo.rs", "shown"),
        "Gizmo::shown",
    );
}

#[test]
fn an_invocation_in_an_impl_body_writes_the_impl_types_member() {
    withheld(
        at("src/user.rs", "Thing::peek"),
        &def("src/other.rs", "peek"),
        "Thing::peek",
    );
}

#[test]
fn another_language_is_untouched() {
    // A Rust macro writes `Widget.alpha`; Python's `Widget.alpha` is no Rust member.
    let got = at("tools/widget.py", "Widget().alpha");
    assert!(got.band().is_drawn(), "{got:?}");
    assert_eq!(got.target(), Some(&def("tools/widget.py", "alpha")));
}
