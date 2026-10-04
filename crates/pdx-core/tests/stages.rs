//! Stage 3's resolution stages (P2-06): the generic blocklist, the engine's typed and
//! external answers, the lexical facts that stop resolution by name, and the Rust
//! stages that narrow each site's candidates.
//!
//! Every repository here is real: written to a checkout, discovered, extracted by the
//! engine and registered. Where a test needs particular engine evidence it gives the
//! resolver answers of its own making, in the engine's own type, so each stage is seen
//! alone; the tests that run typed resolution itself say so.

use std::fs;
use std::path::Path;

use pdx_core::bands::{Band, EngineVerdict, from_engine};
use pdx_core::config::PdxConfig;
use pdx_core::consts::TYPED_MIN_SCORE;
use pdx_core::index::discover::{DiscoveredFile, discover};
use pdx_core::index::extract::{ExtractLimits, ExtractReport, ExtractStage, FsCache};
use pdx_core::resolve::blocklist;
use pdx_core::resolve::registry::{DefinitionRef, SymbolRegistry};
use pdx_core::resolve::stages::{
    InvalidResolution, Narrowed, NarrowingStage, Resolution, ResolutionError, ResolveReport,
    Unconfirmed, narrow, resolve, resolve_with, typed_resolution,
};
use pdx_engine::{ProjectResolution, RunHealth, RunStatus, SiteRef, Strategy, TypedResolution};

// --- fixtures ----------------------------------------------------------------------

struct Checkout {
    dir: tempfile::TempDir,
}

impl Checkout {
    fn new(files: &[(&str, &str)]) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("pdx-stages-test-")
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

    fn discover(&self) -> Vec<DiscoveredFile> {
        let config = PdxConfig::load(self.root()).expect("the configuration loads");
        discover(self.root(), &config).expect("discovery succeeds")
    }

    fn extract(&self, files: &[DiscoveredFile], cache: Option<&FsCache>) -> ExtractReport {
        let config = PdxConfig::load(self.root()).expect("the configuration loads");
        let limits = ExtractLimits {
            requested_workers: 2,
            memory_budget_bytes: 64 << 20,
        };
        let mut stage = ExtractStage::new(self.root(), &config.secrets, limits);
        if let Some(cache) = cache {
            stage = stage.with_cache(cache);
        }
        stage.run(files).expect("extraction succeeds")
    }

    fn registry(&self) -> SymbolRegistry {
        let files = self.discover();
        let report = self.extract(&files, None);
        SymbolRegistry::build(self.root(), &files, report).expect("the registry builds")
    }
}

fn clean() -> RunHealth {
    RunHealth {
        status: RunStatus::Clean,
        files: 0,
        files_resolved: 0,
        files_untyped: 0,
        files_empty: 0,
        files_over_budget: 0,
        files_source_unavailable: 0,
        files_not_reached: 0,
        pass_failures: 0,
    }
}

/// The stages alone, with no answer from the engine.
fn by_name(reg: &SymbolRegistry) -> ResolveReport {
    with_answers(reg, Vec::new())
}

/// The stages with these answers from the engine.
fn with_answers(reg: &SymbolRegistry, answers: Vec<TypedResolution>) -> ResolveReport {
    resolve_with(
        reg,
        &ProjectResolution {
            resolutions: answers,
            health: clean(),
        },
    )
    .expect("resolution succeeds")
}

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

/// The site of `path` whose callee is spelled `callee`.
fn site_ref(reg: &SymbolRegistry, path: &str, callee: &str) -> SiteRef {
    let calls = &reg.extract(path).expect("extracted").calls;
    let found: Vec<usize> = calls
        .iter()
        .enumerate()
        .filter(|(_, c)| c.callee_text == callee)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(found.len(), 1, "{callee} in {path}: {calls:?}");
    SiteRef {
        rel_path: path.to_owned(),
        call_index: u32::try_from(found[0]).unwrap(),
    }
}

/// What the site of `path` whose callee is spelled `callee` resolved to.
fn at<'r>(report: &'r ResolveReport, path: &str, callee: &str) -> &'r Resolution {
    let found: Vec<&Resolution> = report
        .resolutions
        .iter()
        .filter(|s| s.site_ref.rel_path == path && s.site.callee_text == callee)
        .map(|s| &s.resolution)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "{callee} in {path}: {:?}",
        report
            .resolutions
            .iter()
            .map(|s| (&s.site_ref, &s.site.callee_text))
            .collect::<Vec<_>>()
    );
    found[0]
}

/// An answer of the engine's, for the site of `path` spelled `callee`.
fn answer(
    reg: &SymbolRegistry,
    path: &str,
    callee: &str,
    target: Option<(&str, &str)>,
    strategy: Strategy,
    candidates: u32,
    score: f64,
) -> TypedResolution {
    let site_ref = site_ref(reg, path, callee);
    let site = reg.extract(path).unwrap().calls[site_ref.call_index as usize].clone();
    TypedResolution {
        site_ref,
        site,
        target_qn: target.map_or("builtins.len", |(_, qn)| qn).to_owned(),
        target_rel_path: target.map(|(p, _)| p.to_owned()),
        score,
        strategy,
        engine_strategy: Some(strategy.as_str().to_owned()),
        candidates,
    }
}

fn sorted(mut refs: Vec<DefinitionRef>) -> Vec<DefinitionRef> {
    refs.sort();
    refs
}

/// Python: `compute` defined in two modules, `lib` and `other`; `main` imports it from
/// `lib` and calls it.
fn two_computes() -> Checkout {
    Checkout::new(&[
        ("lib.py", "def compute():\n    return 1\n"),
        ("other.py", "def compute():\n    return 2\n"),
        (
            "main.py",
            "from lib import compute\n\n\ndef run_all():\n    return compute()\n",
        ),
        ("loose.py", "def run_loose():\n    return compute()\n"),
    ])
}

// --- the nine outcomes ------------------------------------------------------------------

/// One row of the stage matrix: a repository, a site, the evidence each stage has, and
/// what the site must resolve to.
struct Row {
    band: Band,
    checkout: Checkout,
    path: &'static str,
    callee: &'static str,
    /// Definitions of the callee's name in the repository: the initial candidates.
    candidates: usize,
    /// What the engine says, as (target file, target qualified name, strategy,
    /// candidates, score); `None` for no answer. The file is `None` for a target in no
    /// file of the project.
    engine: Option<(Option<&'static str>, &'static str, Strategy, u32, f64)>,
    /// The evidence of the import, hierarchy and scope stages, as the fixture sets it.
    imports: &'static str,
    hierarchy: &'static str,
    scope: &'static str,
    /// The target (file, name) of a drawn band, or the candidates (file, name).
    expected: Vec<(&'static str, &'static str)>,
}

/// The matrix's rows, one per outcome, in 4.2.2's order.
fn matrix_rows() -> Vec<Row> {
    let mut rows = drawn_rows();
    rows.extend(settled_rows());
    rows
}

/// The rows of the five drawn bands Stage 3 assigns.
fn drawn_rows() -> Vec<Row> {
    vec![
        Row {
            band: Band::Typed,
            checkout: two_computes(),
            path: "main.py",
            callee: "compute",
            candidates: 2,
            engine: Some((
                Some("lib.py"),
                "lib.compute",
                Strategy::LspTyped,
                1,
                TYPED_MIN_SCORE,
            )),
            imports: "main imports compute from lib",
            hierarchy: "none: a bare call in Python names no member",
            scope: "neither definition is in main",
            expected: vec![("lib.py", "compute")],
        },
        Row {
            band: Band::ImportGuided,
            checkout: two_computes(),
            path: "main.py",
            callee: "compute",
            candidates: 2,
            engine: None,
            imports: "main imports compute from lib: one of the two",
            hierarchy: "none",
            scope: "none",
            expected: vec![("lib.py", "compute")],
        },
        Row {
            band: Band::InheritanceGuided,
            checkout: Checkout::new(&[(
                "shapes.py",
                "class Base:\n    def greet(self):\n        return 1\n\n\nclass Other:\n    def greet(self):\n        return 2\n\n\nclass Child(Base):\n    def go(self):\n        return self.greet()\n",
            )]),
            path: "shapes.py",
            callee: "self.greet",
            candidates: 2,
            engine: None,
            imports: "none",
            hierarchy: "Child's hierarchy is Child, Base: only Base declares greet",
            scope: "both in the same module, which would not decide",
            expected: vec![("shapes.py", "greet@Base")],
        },
        Row {
            band: Band::Exact,
            checkout: Checkout::new(&[
                ("util.py", "def tally():\n    return 1\n"),
                ("main.py", "def run_all():\n    return tally()\n"),
            ]),
            path: "main.py",
            callee: "tally",
            candidates: 1,
            engine: None,
            imports: "none: main imports nothing",
            hierarchy: "none",
            scope: "tally is in util, not main",
            expected: vec![("util.py", "tally")],
        },
        Row {
            band: Band::Scoped,
            checkout: Checkout::new(&[
                (
                    "a.py",
                    "def helper():\n    return 1\n\n\ndef run_all():\n    return helper()\n",
                ),
                ("b.py", "def helper():\n    return 2\n"),
            ]),
            path: "a.py",
            callee: "helper",
            candidates: 2,
            engine: None,
            imports: "none",
            hierarchy: "none",
            scope: "one of the two is in run_all's module, a",
            expected: vec![("a.py", "helper")],
        },
    ]
}

/// The rows of the four bands that are not drawn.
fn settled_rows() -> Vec<Row> {
    vec![
        Row {
            band: Band::Candidate,
            checkout: two_computes(),
            path: "loose.py",
            callee: "compute",
            candidates: 2,
            engine: None,
            imports: "none: loose imports nothing",
            hierarchy: "none",
            scope: "neither is in loose",
            expected: vec![("lib.py", "compute"), ("other.py", "compute")],
        },
        Row {
            band: Band::External,
            checkout: Checkout::new(&[
                (
                    "main.py",
                    "import os\n\n\ndef run_all():\n    return os.getcwd()\n",
                ),
                ("mine.py", "def getcwd():\n    return '.'\n"),
            ]),
            path: "main.py",
            callee: "os.getcwd",
            candidates: 1,
            engine: None,
            imports: "os is bound only by an import of an external package",
            hierarchy: "none",
            scope: "none",
            expected: vec![],
        },
        Row {
            band: Band::Blocked,
            checkout: Checkout::new(&[
                ("store.py", "def get():\n    return 1\n"),
                (
                    "main.py",
                    "from store import get\n\n\ndef run_all():\n    return get()\n",
                ),
            ]),
            path: "main.py",
            callee: "get",
            candidates: 1,
            engine: Some((Some("store.py"), "store.get", Strategy::LspTyped, 1, 1.0)),
            imports: "main imports get, which import-guided would take",
            hierarchy: "none",
            scope: "none",
            expected: vec![],
        },
        Row {
            band: Band::Unresolved,
            checkout: Checkout::new(&[("main.py", "def run_all():\n    return missing_thing()\n")]),
            path: "main.py",
            callee: "missing_thing",
            candidates: 0,
            engine: None,
            imports: "none",
            hierarchy: "none",
            scope: "none",
            expected: vec![],
        },
    ]
}

#[test]
fn resolution_stage_matrix() {
    let rows = matrix_rows();
    let mut covered = Vec::new();
    for row in rows {
        let reg = row.checkout.registry();
        let (_, short) = row.callee.rsplit_once('.').unwrap_or(("", row.callee));
        assert_eq!(
            reg.by_name(short).len(),
            row.candidates,
            "{:?}: initial candidates",
            row.band
        );
        let answers = row
            .engine
            .map(|(file, qn, strategy, candidates, score)| {
                let target = file.map(|f| (f, qn));
                vec![answer(
                    &reg, row.path, row.callee, target, strategy, candidates, score,
                )]
            })
            .unwrap_or_default();
        let report = with_answers(&reg, answers);
        let got = at(&report, row.path, row.callee);
        let context = format!(
            "{:?}: imports {}; hierarchy {}; scope {}",
            row.band, row.imports, row.hierarchy, row.scope
        );
        assert_eq!(got.band(), row.band, "{context}: {got:?}");
        let refs: Vec<DefinitionRef> = row
            .expected
            .iter()
            .map(|(path, name)| match name.split_once('@') {
                // A method, by its type.
                Some((method, ty)) => {
                    let ty = def(&reg, path, ty);
                    reg.by_name(method)
                        .iter()
                        .find(|m| reg.declaring_type(m).as_ref() == Some(&ty))
                        .expect("the method")
                        .clone()
                }
                None => def(&reg, path, name),
            })
            .collect();
        if row.band.is_drawn() {
            assert_eq!(got.target(), refs.first(), "{context}");
            assert!(got.candidates().is_empty());
        } else {
            assert_eq!(got.target(), None, "{context}");
            assert_eq!(got.candidates(), sorted(refs), "{context}");
        }
        covered.push(row.band);
    }
    assert_eq!(
        covered,
        [
            Band::Typed,
            Band::ImportGuided,
            Band::InheritanceGuided,
            Band::Exact,
            Band::Scoped,
            Band::Candidate,
            Band::External,
            Band::Blocked,
            Band::Unresolved,
        ]
    );
}

// --- narrowing ---------------------------------------------------------------------------

#[test]
fn narrowing_keeps_narrowest_set() {
    let make = |name: &str| DefinitionRef {
        path: format!("{name}.py"),
        index: 1,
    };
    let [alpha, beta, gamma, delta] = ["a", "b", "c", "d"].map(make);
    let seen = std::cell::RefCell::new(Vec::new());
    let stage = |band: Band, keep: Vec<DefinitionRef>| NarrowingStage {
        band,
        evidence: Box::new({
            let seen = &seen;
            move |current: &[DefinitionRef]| {
                seen.borrow_mut().push((band, current.to_vec()));
                current
                    .iter()
                    .filter(|x| keep.contains(x))
                    .cloned()
                    .collect()
            }
        }),
    };
    // ABCD; import keeps ABC; inheritance AB; exact has no evidence; scoped keeps B.
    let narrowed = narrow(
        vec![delta.clone(), gamma.clone(), beta.clone(), alpha.clone()],
        vec![
            stage(
                Band::ImportGuided,
                vec![alpha.clone(), beta.clone(), gamma.clone()],
            ),
            stage(Band::InheritanceGuided, vec![alpha.clone(), beta.clone()]),
            stage(Band::Exact, vec![]),
            stage(Band::Scoped, vec![beta.clone()]),
        ],
    );
    assert_eq!(
        narrowed,
        Narrowed::Resolved {
            band: Band::Scoped,
            target: beta.clone()
        }
    );
    // Each stage saw the narrowest set before it; exact's empty answer erased nothing.
    assert_eq!(
        *seen.borrow(),
        [
            (
                Band::ImportGuided,
                vec![alpha.clone(), beta.clone(), gamma.clone(), delta.clone()]
            ),
            (
                Band::InheritanceGuided,
                vec![alpha.clone(), beta.clone(), gamma.clone()]
            ),
            (Band::Exact, vec![alpha.clone(), beta.clone()]),
            (Band::Scoped, vec![alpha.clone(), beta.clone()]),
        ]
    );
    // A stage cannot add a candidate, and the narrowest set is what is left.
    seen.borrow_mut().clear();
    let narrowed = narrow(
        vec![alpha.clone(), beta.clone(), gamma.clone()],
        vec![
            stage(
                Band::ImportGuided,
                vec![alpha.clone(), beta.clone(), delta.clone()],
            ),
            stage(Band::Exact, vec![]),
        ],
    );
    assert_eq!(
        narrowed,
        Narrowed::Remaining(vec![alpha.clone(), beta.clone()])
    );

    // And in a real repository: two bases declare `greet`, a third class elsewhere
    // does too. The hierarchy narrows three to two, which nothing narrows further.
    let checkout = Checkout::new(&[
        ("a.py", "class A:\n    def greet(self):\n        return 1\n"),
        ("b.py", "class B:\n    def greet(self):\n        return 2\n"),
        ("d.py", "class D:\n    def greet(self):\n        return 4\n"),
        (
            "c.py",
            "from a import A\nfrom b import B\n\n\nclass C(A, B):\n    def go(self):\n        return self.greet()\n",
        ),
    ]);
    let reg = checkout.registry();
    let report = by_name(&reg);
    let got = at(&report, "c.py", "self.greet");
    assert_eq!(got.band(), Band::Candidate);
    let greet_of = |path: &str, ty: &str| {
        let ty = def(&reg, path, ty);
        reg.by_file(path)
            .iter()
            .find(|m| reg.declaring_type(m).as_ref() == Some(&ty))
            .unwrap()
            .clone()
    };
    assert_eq!(
        got.candidates(),
        sorted(vec![greet_of("a.py", "A"), greet_of("b.py", "B")])
    );
}

#[test]
fn zero_narrowing_result_keeps_previous_candidates() {
    // The import stage finds no import of `compute`, the hierarchy stage does not
    // apply, exact sees two and scoped sees neither in `loose`: none of them erases a
    // candidate, so both remain.
    let checkout = two_computes();
    let reg = checkout.registry();
    let got = at(&by_name(&reg), "loose.py", "compute").clone();
    assert_eq!(got.band(), Band::Candidate);
    assert_eq!(
        got.candidates(),
        sorted(vec![
            def(&reg, "lib.py", "compute"),
            def(&reg, "other.py", "compute")
        ])
    );
}

// --- the blocklist ---------------------------------------------------------------------

#[test]
fn blocklist_precedes_all() {
    // Appendix B.4's base list, exactly, and Python's documented additions.
    assert_eq!(blocklist::BASE.len(), 57);
    let mut unique = blocklist::BASE.to_vec();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 57);
    for name in ["get", "run", "toString", "forEach", "from_json", "hashCode"] {
        assert!(blocklist::is_blocked("java", name), "{name}");
        assert!(blocklist::is_blocked("python", name), "{name}");
    }
    assert!(!blocklist::is_blocked("java", "Get"), "case matters");
    assert!(!blocklist::is_blocked("go", "Run"), "case matters");
    for name in ["__init__", "__str__", "__repr__", "__enter__", "__exit__"] {
        assert!(blocklist::is_blocked("python", name), "{name}");
        assert!(!blocklist::is_blocked("java", name), "{name} is Python's");
    }
    assert!(blocklist::ADDITIONS.iter().all(|a| a.language == "python"));

    // Before a qualifying typed answer, before an engine-proven external target, and
    // before every stage that would otherwise draw it.
    let checkout = Checkout::new(&[
        ("store.py", "def get():\n    return 1\n"),
        (
            "main.py",
            "from store import get\n\n\ndef run_all():\n    return get()\n",
        ),
    ]);
    let reg = checkout.registry();
    for engine in [
        Some((Some(("store.py", "store.get")), Strategy::LspTyped, 1, 1.0)),
        Some((None, Strategy::LspTyped, 1, 1.0)),
        None,
    ] {
        let answers = engine
            .map(|(target, strategy, candidates, score)| {
                vec![answer(
                    &reg, "main.py", "get", target, strategy, candidates, score,
                )]
            })
            .unwrap_or_default();
        let report = with_answers(&reg, answers);
        let got = at(&report, "main.py", "get");
        assert_eq!(got.band(), Band::Blocked, "{engine:?}");
        assert_eq!(got.target(), None);
        assert!(got.candidates().is_empty());
        // The engine's calibration data is kept all the same.
        assert_eq!(got.engine_score(), engine.map(|e| e.3));
    }
}

// --- the engine's answer ------------------------------------------------------------

#[test]
fn typed_requires_lsp_typed_single_candidate_and_min_score() {
    let checkout = two_computes();
    let reg = checkout.registry();
    let lib = def(&reg, "lib.py", "compute");
    let other = def(&reg, "other.py", "compute");
    let below = TYPED_MIN_SCORE - 1e-9;
    for (strategy, candidates, score, typed) in [
        (Strategy::LspTyped, 1, TYPED_MIN_SCORE, true),
        (Strategy::LspTyped, 1, 1.0, true),
        (Strategy::LspTyped, 1, below, false),
        (Strategy::LspTyped, 2, 0.99, false),
        (Strategy::UniqueName, 1, 1.0, false),
        (Strategy::SameModule, 1, 1.0, false),
        (Strategy::ImportMap, 1, 1.0, false),
        (Strategy::LspTyped, 1, f64::NAN, false),
        (Strategy::LspTyped, 1, -0.5, false),
        (Strategy::LspTyped, 1, 1.5, false),
    ] {
        assert_eq!(
            from_engine(score, strategy.as_str(), candidates) == EngineVerdict::Typed,
            typed,
            "{strategy:?} {candidates} {score}"
        );
        // The same answer through the resolver, for a site no Rust stage settles.
        let a = answer(
            &reg,
            "loose.py",
            "compute",
            Some(("lib.py", "lib.compute")),
            strategy,
            candidates,
            score,
        );
        let report = with_answers(&reg, vec![a]);
        let got = at(&report, "loose.py", "compute");
        if typed {
            assert_eq!(got.band(), Band::Typed, "{strategy:?} {score}");
            assert_eq!(got.target(), Some(&lib));
        } else {
            assert_eq!(
                got.band(),
                Band::Candidate,
                "{strategy:?} {candidates} {score}"
            );
            assert_eq!(got.candidates(), sorted(vec![lib.clone(), other.clone()]));
        }
        assert_eq!(got.engine_candidates(), Some(candidates));
        assert_eq!(got.engine_strategy(), Some(strategy.as_str()));
    }
}

#[test]
fn typed_target_must_map_uniquely() {
    // A qualifying answer whose file and qualified name match two definitions (a
    // redefinition), or none, draws nothing: its definitions stay candidates.
    let checkout = Checkout::new(&[
        (
            "lib.py",
            "def compute():\n    return 1\n\n\ndef compute():\n    return 2\n",
        ),
        ("loose.py", "def run_loose():\n    return compute()\n"),
    ]);
    let reg = checkout.registry();
    let both = reg.by_qualified_name("lib.compute").to_vec();
    assert_eq!(both.len(), 2);
    let two = answer(
        &reg,
        "loose.py",
        "compute",
        Some(("lib.py", "lib.compute")),
        Strategy::LspTyped,
        1,
        1.0,
    );
    let got = at(&with_answers(&reg, vec![two]), "loose.py", "compute").clone();
    assert_eq!(got.band(), Band::Candidate);
    assert_eq!(got.candidates(), both);
    // A target that names no definition: not typed, and no candidate it could name.
    let none = answer(
        &reg,
        "loose.py",
        "compute",
        Some(("lib.py", "lib.compute_gone")),
        Strategy::LspTyped,
        1,
        1.0,
    );
    let got = at(&with_answers(&reg, vec![none]), "loose.py", "compute").clone();
    assert_eq!(got.band(), Band::Candidate);
    assert_eq!(got.candidates(), both);
}

#[test]
fn engine_external_does_not_fall_back_to_local_name() {
    // The engine says `tally`'s target is in no file of the project (a built-in, say).
    // A definition of the same name in the repository, which exact would otherwise
    // take, does not get the site.
    let checkout = Checkout::new(&[
        ("util.py", "def tally():\n    return 1\n"),
        ("main.py", "def run_all():\n    return tally()\n"),
    ]);
    let reg = checkout.registry();
    assert_eq!(at(&by_name(&reg), "main.py", "tally").band(), Band::Exact);
    let external = answer(&reg, "main.py", "tally", None, Strategy::LspTyped, 1, 0.5);
    let report = with_answers(&reg, vec![external]);
    let got = at(&report, "main.py", "tally");
    assert_eq!(got.band(), Band::External);
    assert_eq!(got.target(), None);
    assert_eq!(got.engine_score(), Some(0.5));
}

#[test]
fn engine_hints_never_restrict_candidate_universe() {
    let checkout = Checkout::new(&[
        ("x.py", "def compute():\n    return 1\n"),
        ("y.py", "def compute():\n    return 2\n"),
        ("z.py", "def other_name():\n    return 3\n"),
        ("loose.py", "def run_loose():\n    return compute()\n"),
    ]);
    let reg = checkout.registry();
    let [x, y, z] = [
        ("x.py", "compute"),
        ("y.py", "compute"),
        ("z.py", "other_name"),
    ]
    .map(|(p, n)| def(&reg, p, n));
    // The engine hints at x: y stays.
    for strategy in [
        Strategy::ImportMap,
        Strategy::UniqueName,
        Strategy::SameModule,
    ] {
        let hint = answer(
            &reg,
            "loose.py",
            "compute",
            Some(("x.py", "x.compute")),
            strategy,
            1,
            0.9,
        );
        let got = at(&with_answers(&reg, vec![hint]), "loose.py", "compute").clone();
        assert_eq!(got.band(), Band::Candidate, "{strategy:?}");
        assert_eq!(
            got.candidates(),
            sorted(vec![x.clone(), y.clone()]),
            "{strategy:?}"
        );
    }
    // The engine hints at z, which the name does not reach: it joins x and y.
    let hint = answer(
        &reg,
        "loose.py",
        "compute",
        Some(("z.py", "z.other_name")),
        Strategy::ImportMap,
        1,
        0.9,
    );
    let got = at(&with_answers(&reg, vec![hint]), "loose.py", "compute").clone();
    assert_eq!(got.band(), Band::Candidate);
    assert_eq!(got.candidates(), sorted(vec![x, y, z]));
}

#[test]
fn one_unvalidated_engine_hint_stays_candidate() {
    // A low-score answer with one candidate the name does not reach: no Rust stage
    // validates it, so it is a candidate, never drawn.
    let checkout = Checkout::new(&[
        ("lib.py", "def compute():\n    return 1\n"),
        ("main.py", "def run_all():\n    return calc()\n"),
    ]);
    let reg = checkout.registry();
    let hint = answer(
        &reg,
        "main.py",
        "calc",
        Some(("lib.py", "lib.compute")),
        Strategy::LspTyped,
        1,
        0.5,
    );
    let got = at(&with_answers(&reg, vec![hint]), "main.py", "calc").clone();
    assert_eq!(got.band(), Band::Candidate);
    assert_eq!(got.candidates(), [def(&reg, "lib.py", "compute")]);
    assert_eq!(got.engine_score(), Some(0.5));
}

#[test]
fn duplicate_engine_answers_are_refused() {
    let checkout = two_computes();
    let reg = checkout.registry();
    let first = answer(
        &reg,
        "loose.py",
        "compute",
        Some(("lib.py", "lib.compute")),
        Strategy::LspTyped,
        1,
        1.0,
    );
    // The same answer twice is one answer.
    let report = resolve_with(
        &reg,
        &ProjectResolution {
            resolutions: vec![first.clone(), first.clone()],
            health: clean(),
        },
    )
    .unwrap();
    assert_eq!(at(&report, "loose.py", "compute").band(), Band::Typed);
    // Two different answers for one site: neither is chosen.
    let mut second = first.clone();
    second.target_rel_path = Some("other.py".into());
    second.target_qn = "other.compute".into();
    let err = resolve_with(
        &reg,
        &ProjectResolution {
            resolutions: vec![first.clone(), second],
            health: clean(),
        },
    )
    .unwrap_err();
    assert!(
        matches!(err, ResolutionError::DuplicateEngineAnswer { .. }),
        "{err}"
    );
    // An answer about a file the registry has no extraction of, or about a call the
    // extraction names otherwise.
    let mut elsewhere = first.clone();
    elsewhere.site_ref.rel_path = "nowhere.py".into();
    let mut renamed = first;
    renamed.site.callee_text = "renamed".into();
    for wrong in [elsewhere, renamed] {
        let err = resolve_with(
            &reg,
            &ProjectResolution {
                resolutions: vec![wrong],
                health: clean(),
            },
        )
        .unwrap_err();
        assert!(
            matches!(err, ResolutionError::ImpossibleEngineSite { .. }),
            "{err}"
        );
    }
}

#[test]
fn resolution_shapes_are_enforced() {
    let r = DefinitionRef {
        path: "a.py".into(),
        index: 1,
    };
    let s = DefinitionRef {
        path: "b.py".into(),
        index: 1,
    };
    for drawn in [
        Band::Typed,
        Band::ImportGuided,
        Band::InheritanceGuided,
        Band::Exact,
        Band::Scoped,
    ] {
        assert!(Resolution::new(drawn, Some(r.clone()), vec![], None).is_ok());
        assert_eq!(
            Resolution::new(drawn, None, vec![], None),
            Err(InvalidResolution::Drawn(drawn))
        );
        assert_eq!(
            Resolution::new(drawn, Some(r.clone()), vec![s.clone()], None),
            Err(InvalidResolution::Drawn(drawn))
        );
    }
    assert!(Resolution::new(Band::Candidate, None, vec![r.clone(), s.clone()], None).is_ok());
    assert_eq!(
        Resolution::new(Band::Candidate, Some(r.clone()), vec![s.clone()], None),
        Err(InvalidResolution::Candidate)
    );
    assert_eq!(
        Resolution::new(Band::Candidate, None, vec![], None),
        Err(InvalidResolution::Candidate)
    );
    assert_eq!(
        Resolution::new(Band::Candidate, None, vec![s.clone(), r.clone()], None),
        Err(InvalidResolution::Unsorted)
    );
    for settled in [Band::External, Band::Blocked, Band::Unresolved] {
        assert!(Resolution::new(settled, None, vec![], None).is_ok());
        assert_eq!(
            Resolution::new(settled, Some(r.clone()), vec![], None),
            Err(InvalidResolution::Settled(settled))
        );
        assert_eq!(
            Resolution::new(settled, None, vec![r.clone()], None),
            Err(InvalidResolution::Settled(settled))
        );
    }
    for other in [Band::Precise, Band::Contradicted] {
        assert_eq!(
            Resolution::new(other, Some(r.clone()), vec![], None),
            Err(InvalidResolution::NotAssigned(other))
        );
    }
}

// --- the stages --------------------------------------------------------------------

#[test]
fn aliased_imports_are_import_guided() {
    // Python: `from a import f as g`, then `g()`.
    let checkout = Checkout::new(&[
        ("a.py", "def f():\n    return 1\n"),
        ("b.py", "def f():\n    return 2\n"),
        (
            "main.py",
            "from a import f as g\n\n\ndef run_all():\n    return g()\n",
        ),
    ]);
    let reg = checkout.registry();
    let got = at(&by_name(&reg), "main.py", "g").clone();
    assert_eq!(got.band(), Band::ImportGuided);
    assert_eq!(got.target(), Some(&def(&reg, "a.py", "f")));

    // Kotlin: `import a.b.C as D`, then `D()`.
    let checkout = Checkout::new(&[
        ("src/a/b/C.kt", "package a.b\n\nclass C\n"),
        ("src/x/C.kt", "package x\n\nclass C\n"),
        (
            "src/main/Main.kt",
            "package main\n\nimport a.b.C as D\n\nfun build() = D()\n",
        ),
    ]);
    let reg = checkout.registry();
    let got = at(&by_name(&reg), "src/main/Main.kt", "D").clone();
    assert_eq!(got.band(), Band::ImportGuided);
    assert_eq!(got.target(), Some(&def(&reg, "src/a/b/C.kt", "C")));

    // Rust: `use crate::a::f as g`, then `g()`.
    let checkout = Checkout::new(&[
        (
            "Cargo.toml",
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        ),
        ("src/lib.rs", "pub mod a;\npub mod b;\npub mod c;\n"),
        ("src/a.rs", "pub fn f() -> i32 {\n    1\n}\n"),
        ("src/b.rs", "pub fn f() -> i32 {\n    2\n}\n"),
        (
            "src/c.rs",
            "use crate::a::f as g;\n\npub fn run_all() -> i32 {\n    g()\n}\n",
        ),
    ]);
    let reg = checkout.registry();
    let got = at(&by_name(&reg), "src/c.rs", "g").clone();
    assert_eq!(got.band(), Band::ImportGuided);
    assert_eq!(got.target(), Some(&def(&reg, "src/a.rs", "f")));
}

#[test]
fn module_receiver_imports_are_import_guided() {
    // TypeScript: `import * as ns from './x'`, then `ns.h()`, with an `h` elsewhere.
    let checkout = Checkout::new(&[
        (
            "src/x.ts",
            "export function h(): number {\n  return 1;\n}\n",
        ),
        (
            "src/y.ts",
            "export function h(): number {\n  return 2;\n}\n",
        ),
        (
            "src/main.ts",
            "import * as ns from './x';\n\nexport function runAll(): number {\n  return ns.h();\n}\n",
        ),
    ]);
    let reg = checkout.registry();
    let got = at(&by_name(&reg), "src/main.ts", "ns.h").clone();
    assert_eq!(got.band(), Band::ImportGuided);
    assert_eq!(got.target(), Some(&def(&reg, "src/x.ts", "h")));
}

#[test]
fn relative_unresolved_import_is_not_external() {
    // `from .missing import thing` names the repository, which does not answer: the
    // call is unresolved, never external.
    let checkout = Checkout::new(&[
        ("pkg/__init__.py", ""),
        (
            "pkg/main.py",
            "from .missing import thing\n\n\ndef run_all():\n    return thing()\n",
        ),
    ]);
    let reg = checkout.registry();
    let got = at(&by_name(&reg), "pkg/main.py", "thing").clone();
    assert_eq!(got.band(), Band::Unresolved);
}

#[test]
fn external_binding_is_external_despite_a_namesake() {
    // `from pathlib import Path`: the file's `Path` is the external one, whatever the
    // repository defines elsewhere under that name.
    let checkout = Checkout::new(&[
        ("model.py", "class Path:\n    pass\n"),
        (
            "main.py",
            "from pathlib import Path\n\n\ndef run_all():\n    return Path('.')\n",
        ),
    ]);
    let reg = checkout.registry();
    assert_eq!(at(&by_name(&reg), "main.py", "Path").band(), Band::External);
}

#[test]
fn super_calls_use_the_ancestors() {
    let checkout = Checkout::new(&[(
        "shapes.py",
        "class Base:\n    def greet(self):\n        return 1\n\n\nclass Child(Base):\n    def greet(self):\n        return super().greet() + self.greet_twice()\n\n    def greet_twice(self):\n        return self.greet()\n",
    )]);
    let reg = checkout.registry();
    let report = by_name(&reg);
    let base = def(&reg, "shapes.py", "Base");
    let child = def(&reg, "shapes.py", "Child");
    let greet_of = |ty: &DefinitionRef| {
        reg.by_name("greet")
            .iter()
            .find(|m| reg.declaring_type(m).as_ref() == Some(ty))
            .unwrap()
            .clone()
    };
    // `super().greet()` skips the current type; `self.greet()` takes the nearest.
    let got = at(&report, "shapes.py", "super().greet");
    assert_eq!(got.band(), Band::InheritanceGuided);
    assert_eq!(got.target(), Some(&greet_of(&base)));
    let got = at(&report, "shapes.py", "self.greet");
    assert_eq!(got.band(), Band::InheritanceGuided);
    assert_eq!(got.target(), Some(&greet_of(&child)));
}

#[test]
fn rust_impl_trait_inheritance_guided() {
    // An empty `impl Describe for Square {}` is the only evidence that `self.describe()`
    // in Square's own impl reaches Describe's default method, not Other's (issue 42).
    let checkout = Checkout::new(&[
        (
            "Cargo.toml",
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        ),
        (
            "src/lib.rs",
            "pub trait Describe {\n    fn describe(&self) -> String {\n        String::from(\"thing\")\n    }\n}\n\npub trait Other {\n    fn describe(&self) -> String {\n        String::from(\"other\")\n    }\n}\n\npub struct Square;\n\nimpl Describe for Square {}\n\nimpl Square {\n    pub fn label(&self) -> String {\n        self.describe()\n    }\n}\n",
        ),
    ]);
    let reg = checkout.registry();
    let describe = def(&reg, "src/lib.rs", "Describe");
    let square = def(&reg, "src/lib.rs", "Square");
    assert_eq!(
        reg.implemented_traits(&square),
        std::slice::from_ref(&describe)
    );
    let got = at(&by_name(&reg), "src/lib.rs", "self.describe").clone();
    assert_eq!(got.band(), Band::InheritanceGuided);
    let target = got.target().unwrap();
    assert_eq!(reg.declaring_type(target), Some(describe));
}

#[test]
fn scoped_means_module_not_directory() {
    // Java: `helper` is defined twice. One is in the caller's directory but another
    // package; the other is in the caller's package but another directory. Scoped
    // takes the package's.
    let checkout = Checkout::new(&[
        (
            "src/a/Caller.java",
            "package p1;\n\nclass Caller {\n  void go() { helper(); }\n}\n",
        ),
        (
            "src/a/Neighbour.java",
            "package p2;\n\nclass Neighbour {\n  static void helper() {}\n}\n",
        ),
        (
            "src/b/Kin.java",
            "package p1;\n\nclass Kin {\n  static void helper() {}\n}\n",
        ),
    ]);
    let reg = checkout.registry();
    let got = at(&by_name(&reg), "src/a/Caller.java", "helper").clone();
    assert_eq!(got.band(), Band::Scoped);
    assert_eq!(got.target(), Some(&def(&reg, "src/b/Kin.java", "helper")));
}

#[test]
fn no_candidate_crosses_a_language_family() {
    // A Python call to `compute`, which only Go defines: no candidate.
    let checkout = Checkout::new(&[
        ("go.mod", "module example.com/acme\n"),
        (
            "calc/calc.go",
            "package calc\n\nfunc compute() int { return 1 }\n",
        ),
        ("main.py", "def run_all():\n    return compute()\n"),
    ]);
    let reg = checkout.registry();
    assert_eq!(
        at(&by_name(&reg), "main.py", "compute").band(),
        Band::Unresolved
    );
}

// --- what stops resolution by name ----------------------------------------------------

#[test]
fn lexically_local_binding_is_not_name_resolved() {
    // `cb` is a parameter: the definition of that name elsewhere is not its target.
    let checkout = Checkout::new(&[
        ("hooks.py", "def cb():\n    return 1\n"),
        ("main.py", "def run_all(cb):\n    return cb()\n"),
    ]);
    let reg = checkout.registry();
    let site = site_ref(&reg, "main.py", "cb");
    let call = &reg.extract("main.py").unwrap().calls[site.call_index as usize];
    assert!(
        call.lexical
            .contains(pdx_engine::LexicalFacts::LOCALLY_BOUND)
    );
    let got = at(&by_name(&reg), "main.py", "cb").clone();
    assert_eq!(got.band(), Band::Unresolved);
    assert!(got.candidates().is_empty());
}

#[test]
fn unresolved_member_is_not_unique_name_resolved() {
    // `obj.compute()`: a receiver nothing ties to a type. The one `compute` of the
    // repository is a candidate, never `exact`. And `self.engine.spin()` is a member of
    // something `self` owns, not of the current class, which also defines `spin`.
    let checkout = Checkout::new(&[
        ("lib.py", "def compute():\n    return 1\n"),
        (
            "main.py",
            "class Runner:\n    def spin(self):\n        return 1\n\n    def go(self, obj):\n        return obj.compute() + self.engine.spin()\n",
        ),
    ]);
    let reg = checkout.registry();
    let report = by_name(&reg);
    let got = at(&report, "main.py", "obj.compute");
    assert_eq!(got.band(), Band::Candidate);
    assert_eq!(got.candidates(), [def(&reg, "lib.py", "compute")]);
    let got = at(&report, "main.py", "self.engine.spin");
    assert_eq!(got.band(), Band::Candidate, "{got:?}");
}

#[test]
fn typed_only_site_is_never_name_resolved() {
    // C++'s `->` on a value is a question for typed resolution, which the repository's
    // `operator->` (were there one) or any other name never answers.
    let checkout = Checkout::new(&[(
        "src/a.cpp",
        "struct Ptr { int *operator->(); };\nstruct A { void run(Ptr p) { p->size(); helper(); } void helper() {} };\n",
    )]);
    let reg = checkout.registry();
    let calls = &reg.extract("src/a.cpp").unwrap().calls;
    let typed_only: Vec<usize> = calls
        .iter()
        .enumerate()
        .filter(|(_, c)| c.typed_only)
        .map(|(i, _)| i)
        .collect();
    assert!(!typed_only.is_empty(), "{calls:?}");
    let report = by_name(&reg);
    for index in &typed_only {
        let site = SiteRef {
            rel_path: "src/a.cpp".into(),
            call_index: u32::try_from(*index).unwrap(),
        };
        assert!(report.resolutions.iter().all(|s| s.site_ref != site));
        assert!(
            report
                .unconfirmed
                .iter()
                .any(|u| u.site_ref == site && u.reason == Unconfirmed::TypedOnly)
        );
    }
    // With a typed answer, the same site is a call.
    let index = typed_only[0];
    let call = calls[index].clone();
    let typed = TypedResolution {
        site_ref: SiteRef {
            rel_path: "src/a.cpp".into(),
            call_index: u32::try_from(index).unwrap(),
        },
        site: call,
        target_qn: reg
            .definition(&def(&reg, "src/a.cpp", "Ptr"))
            .unwrap()
            .qualified_name
            .clone(),
        target_rel_path: Some("src/a.cpp".into()),
        score: 1.0,
        strategy: Strategy::LspTyped,
        engine_strategy: Some("lsp_typed".into()),
        candidates: 1,
    };
    let report = with_answers(&reg, vec![typed.clone()]);
    let resolved = report
        .resolutions
        .iter()
        .find(|s| s.site_ref == typed.site_ref)
        .expect("a call now");
    assert_eq!(resolved.resolution.band(), Band::Typed);
}

#[test]
fn untyped_callable_reference_is_not_a_call() {
    // `handler = compute` takes a function as a value. Without a typed answer it is
    // not a call, though `compute` is unique.
    let checkout = Checkout::new(&[
        ("lib.py", "def compute():\n    return 1\n"),
        ("main.py", "from lib import compute\n\nhandler = compute\n"),
    ]);
    let reg = checkout.registry();
    let calls = &reg.extract("main.py").unwrap().calls;
    let index = calls
        .iter()
        .position(|c| c.is_reference && c.callee_text == "compute")
        .unwrap_or_else(|| panic!("a reference: {calls:?}"));
    let site = SiteRef {
        rel_path: "main.py".into(),
        call_index: u32::try_from(index).unwrap(),
    };
    let report = by_name(&reg);
    assert!(report.resolutions.iter().all(|s| s.site_ref != site));
    assert!(
        report
            .unconfirmed
            .iter()
            .any(|u| u.site_ref == site && u.reason == Unconfirmed::Reference)
    );
    // An engine-proven external target settles it as external.
    let external = TypedResolution {
        site_ref: site.clone(),
        site: calls[index].clone(),
        target_qn: "builtins.compute".into(),
        target_rel_path: None,
        score: 1.0,
        strategy: Strategy::LspTyped,
        engine_strategy: Some("lsp_typed".into()),
        candidates: 1,
    };
    let report = with_answers(&reg, vec![external]);
    let resolved = report
        .resolutions
        .iter()
        .find(|s| s.site_ref == site)
        .expect("settled");
    assert_eq!(resolved.resolution.band(), Band::External);
}

#[test]
fn degraded_engine_absence_is_not_evidence() {
    // A degraded run with no answer for a site: the site is resolved by name as it
    // would be with no engine at all, and the run's health is reported.
    let checkout = Checkout::new(&[
        ("util.py", "def tally():\n    return 1\n"),
        ("main.py", "def run_all():\n    return tally()\n"),
    ]);
    let reg = checkout.registry();
    let mut health = clean();
    health.status = RunStatus::Degraded;
    health.files = 2;
    health.files_not_reached = 2;
    let report = resolve_with(
        &reg,
        &ProjectResolution {
            resolutions: Vec::new(),
            health,
        },
    )
    .unwrap();
    assert_eq!(report.engine_health.status, RunStatus::Degraded);
    assert_eq!(at(&report, "main.py", "tally").band(), Band::Exact);
    assert_eq!(
        report,
        ResolveReport {
            engine_health: health,
            ..by_name(&reg)
        }
    );
}

// --- typed resolution itself ---------------------------------------------------------

/// A polyglot repository the engine resolves: Java, Python, Go and TypeScript beside a
/// root `tsconfig.json` with a base URL.
fn polyglot() -> Checkout {
    Checkout::new(&[
        ("go.mod", "module example.com/acme\n"),
        ("pkg/a/a.go", "package a\n\nfunc Hello() int { return 1 }\n"),
        (
            "pkg/b/b.go",
            "package b\n\nimport \"example.com/acme/pkg/a\"\n\nfunc Use() int { return a.Hello() }\n",
        ),
        (
            "tsconfig.json",
            "{ \"compilerOptions\": { \"baseUrl\": \".\", \"paths\": { \"@/*\": [\"src/*\"] } } }\n",
        ),
        (
            "src/lib/format.ts",
            "export function formatName(s: string): string { return s.trim(); }\n",
        ),
        (
            "src/app/show.ts",
            "import { formatName } from '@/lib/format';\nexport function show(): string { return formatName(' x '); }\n",
        ),
        (
            "py/shapes.py",
            "class Base:\n    def greet(self):\n        return 1\n\n\nclass Child(Base):\n    def go(self):\n        return self.greet()\n",
        ),
        (
            "src/main/java/com/acme/Shop.java",
            "package com.acme;\n\npublic class Shop {\n  int total() { return helper(); }\n  int helper() { return 1; }\n}\n",
        ),
    ])
}

#[test]
fn polyglot_ts_alias_does_not_affect_go() {
    // Through typed resolution and the stages together: the engine's own typed answers
    // reach the Go call's and the aliased TypeScript call's targets in one repository
    // (issue 43). A name stage reaching the same target would hide the engine losing
    // Go's answer, so the band must be the engine's.
    let checkout = polyglot();
    let reg = checkout.registry();
    let report = resolve(checkout.root(), &reg).expect("resolution succeeds");
    assert!(
        report.engine_health.is_clean(),
        "{:?}",
        report.engine_health
    );
    let go = at(&report, "pkg/b/b.go", "a.Hello");
    assert_eq!(go.band(), Band::Typed, "{go:?}");
    assert_eq!(go.target(), Some(&def(&reg, "pkg/a/a.go", "Hello")));
    let ts = at(&report, "src/app/show.ts", "formatName");
    assert_eq!(ts.band(), Band::Typed, "{ts:?}");
    assert_eq!(
        ts.target(),
        Some(&def(&reg, "src/lib/format.ts", "formatName"))
    );
}

#[test]
fn engine_run_file_order_is_deterministic() {
    // The registry from files given in any order resolves identically, and every list
    // is in path order.
    let checkout = polyglot();
    let files = checkout.discover();
    let report = checkout.extract(&files, None);
    let expected = {
        let reg = SymbolRegistry::build(checkout.root(), &files, report.clone()).unwrap();
        resolve(checkout.root(), &reg).unwrap()
    };
    assert!(
        expected
            .resolutions
            .windows(2)
            .all(|w| w[0].site_ref < w[1].site_ref)
    );
    let mut reversed_files = files.clone();
    reversed_files.reverse();
    let mut reversed_report = report;
    reversed_report.files.reverse();
    let reg = SymbolRegistry::build(checkout.root(), &reversed_files, reversed_report).unwrap();
    assert_eq!(resolve(checkout.root(), &reg).unwrap(), expected);
}

#[test]
fn fresh_and_cached_inputs_resolve_identically() {
    // Stage 2 from the engine, then from its cache: Stage 3 resolves both the same,
    // impl relations (issue 42) included.
    let checkout = Checkout::new(&[
        (
            "Cargo.toml",
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        ),
        (
            "src/lib.rs",
            "pub trait Describe {\n    fn describe(&self) -> String {\n        String::from(\"thing\")\n    }\n}\n\npub struct Square;\n\nimpl Describe for Square {}\n\nimpl Square {\n    pub fn label(&self) -> String {\n        self.describe()\n    }\n}\n",
        ),
        (
            "py/shapes.py",
            "class Base:\n    def greet(self):\n        return 1\n\n\nclass Child(Base):\n    def go(self):\n        return self.greet()\n",
        ),
    ]);
    let cache_dir = tempfile::tempdir().unwrap();
    let cache = FsCache::at(cache_dir.path());
    let files = checkout.discover();
    let fresh = checkout.extract(&files, Some(&cache));
    let cached = checkout.extract(&files, Some(&cache));
    assert_eq!(cached.stats.cache_hits, fresh.stats.cache_writes);
    assert!(cached.stats.cache_hits > 0);
    let fresh_reg = SymbolRegistry::build(checkout.root(), &files, fresh).unwrap();
    let cached_reg = SymbolRegistry::build(checkout.root(), &files, cached).unwrap();
    assert_eq!(fresh_reg, cached_reg);
    let fresh_report = resolve(checkout.root(), &fresh_reg).unwrap();
    assert_eq!(resolve(checkout.root(), &cached_reg).unwrap(), fresh_report);
    assert_eq!(typed_resolution(checkout.root(), &cached_reg).unwrap(), {
        typed_resolution(checkout.root(), &fresh_reg).unwrap()
    });
    assert!(!fresh_report.resolutions.is_empty());
}

#[test]
fn a_changed_checkout_fails_resolution() {
    let checkout = Checkout::new(&[("main.py", "def run_all():\n    return 1\n")]);
    let reg = checkout.registry();
    fs::write(
        checkout.root().join("main.py"),
        "def run_all():\n    return 2\n",
    )
    .unwrap();
    assert!(matches!(
        resolve(checkout.root(), &reg),
        Err(ResolutionError::SourceChanged(_))
    ));
}

#[test]
fn java_static_import_is_internal() {
    // `import static q.Util.helper` imports a member of a type of the repository's
    // package `q`: an internal import, so the bare `helper()` it binds is import-guided
    // to it, never external, though `helper` is defined twice.
    let checkout = Checkout::new(&[
        (
            "src/q/Util.java",
            "package q;\n\npublic class Util {\n  public static int helper() { return 1; }\n}\n",
        ),
        (
            "src/r/Other.java",
            "package r;\n\npublic class Other {\n  public static int helper() { return 2; }\n}\n",
        ),
        (
            "src/p/Main.java",
            "package p;\n\nimport static q.Util.helper;\nimport static org.junit.Assert.assertTrue;\n\nclass Main {\n  int go() { assertTrue(true); return helper(); }\n}\n",
        ),
    ]);
    let reg = checkout.registry();
    let statics: Vec<_> = reg
        .imports_of("src/p/Main.java")
        .iter()
        .map(|i| (i.module_text.as_str(), i.target.is_internal()))
        .collect();
    assert_eq!(
        statics,
        [
            ("q.Util.helper", true),
            ("org.junit.Assert.assertTrue", false)
        ]
    );
    let report = by_name(&reg);
    let got = at(&report, "src/p/Main.java", "helper");
    assert_eq!(got.band(), Band::ImportGuided);
    assert_eq!(got.target(), Some(&def(&reg, "src/q/Util.java", "helper")));
    // A static import from outside the repository is external.
    assert_eq!(
        at(&report, "src/p/Main.java", "assertTrue").band(),
        Band::External
    );
}

#[test]
fn implicit_receiver_bare_calls_use_the_hierarchy() {
    // Java: a bare `helper()` inside a class names a member of its hierarchy first.
    let checkout = Checkout::new(&[
        (
            "src/app/Base.java",
            "package app;\n\nclass Base {\n  int helper() { return 1; }\n}\n",
        ),
        (
            "src/app/Child.java",
            "package app;\n\nclass Child extends Base {\n  int go() { return helper(); }\n}\n",
        ),
        (
            "src/app/Other.java",
            "package app;\n\nclass Other {\n  int helper() { return 2; }\n}\n",
        ),
    ]);
    let reg = checkout.registry();
    let got = at(&by_name(&reg), "src/app/Child.java", "helper").clone();
    assert_eq!(got.band(), Band::InheritanceGuided);
    assert_eq!(
        got.target(),
        Some(&def(&reg, "src/app/Base.java", "helper"))
    );
}

#[test]
fn c_include_reaches_the_paired_source() {
    // `#include "a.h"` makes the functions of `a.c`, the source paired with `a.h`,
    // visible: of two `helper`s, the included one is import-guided.
    let checkout = Checkout::new(&[
        ("lib/a.h", "int helper(int x);\n"),
        (
            "lib/a.c",
            "#include \"a.h\"\nint helper(int x) { return x; }\n",
        ),
        ("other/b.c", "int helper(int x) { return 2 * x; }\n"),
        (
            "app/main.c",
            "#include \"../lib/a.h\"\nint run_all(void) { return helper(1); }\n",
        ),
    ]);
    let reg = checkout.registry();
    assert_eq!(reg.paired_file("lib/a.h"), Some("lib/a.c"));
    let report = by_name(&reg);
    let helper_calls: Vec<&Resolution> = report
        .resolutions
        .iter()
        .filter(|s| s.site_ref.rel_path == "app/main.c" && s.site.callee_text == "helper")
        .map(|s| &s.resolution)
        .collect();
    assert!(!helper_calls.is_empty());
    for got in helper_calls {
        assert_eq!(got.band(), Band::ImportGuided);
        assert_eq!(got.target(), Some(&def(&reg, "lib/a.c", "helper")));
    }
}

#[test]
fn engine_sites_past_the_extraction_need_an_answer() {
    // A site typed resolution found past the extraction's calls is a call only when its
    // answer settles it; a hint leaves it unconfirmed.
    let checkout = Checkout::new(&[
        ("lib.py", "def compute():\n    return 1\n"),
        ("main.py", "def run_all():\n    return 1\n"),
    ]);
    let reg = checkout.registry();
    let past = u32::try_from(reg.extract("main.py").unwrap().calls.len()).unwrap();
    let site = |index: u32| SiteRef {
        rel_path: "main.py".into(),
        call_index: index,
    };
    let engine_site = |index: u32, score: f64| TypedResolution {
        site_ref: site(index),
        site: pdx_engine::Call {
            callee_text: "compute".into(),
            receiver_text: None,
            caller: Some(1),
            span: None,
            is_reference: false,
            typed_only: true,
            lexical: pdx_engine::LexicalFacts::default(),
        },
        target_qn: "lib.compute".into(),
        target_rel_path: Some("lib.py".into()),
        score,
        strategy: Strategy::LspTyped,
        engine_strategy: Some("lsp_typed".into()),
        candidates: 1,
    };
    let report = with_answers(
        &reg,
        vec![engine_site(past, 1.0), engine_site(past + 1, 0.5)],
    );
    let typed = report
        .resolutions
        .iter()
        .find(|s| s.site_ref == site(past))
        .expect("settled");
    assert_eq!(typed.resolution.band(), Band::Typed);
    assert_eq!(
        typed.resolution.target(),
        Some(&def(&reg, "lib.py", "compute"))
    );
    assert!(
        report
            .resolutions
            .iter()
            .all(|s| s.site_ref != site(past + 1))
    );
    assert!(
        report
            .unconfirmed
            .iter()
            .any(|u| u.site_ref == site(past + 1) && u.reason == Unconfirmed::EngineSite)
    );
}

#[test]
fn hierarchy_cycles_terminate() {
    // Bases that name each other (malformed, but source can say it): the hierarchy is
    // walked once per type, so resolution ends, and the cycle proves nothing more than
    // the types it holds.
    let checkout = Checkout::new(&[(
        "loop.py",
        "class A(B):\n    def ping(self):\n        return self.pong()\n\n\nclass B(A):\n    def pong(self):\n        return 1\n\n\nclass C:\n    def pong(self):\n        return 2\n",
    )]);
    let reg = checkout.registry();
    let got = at(&by_name(&reg), "loop.py", "self.pong").clone();
    assert_eq!(got.band(), Band::InheritanceGuided);
    let b = def(&reg, "loop.py", "B");
    assert_eq!(reg.declaring_type(got.target().unwrap()), Some(b));
}
