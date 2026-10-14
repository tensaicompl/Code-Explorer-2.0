//! The benchmarks' fixture (P2-14), shared by `benches/resolve.rs`,
//! `benches/segment_write.rs` and `tests/bench_fixture.rs`, which keeps its guards
//! true under `make check`.
//!
//! A project-authored Python package, generated: modules that import and call each
//! other's functions, call their own, call a method every helper class defines, call
//! names nothing defines, and call builtins. So resolution meets four bands, and the
//! segment holds the rows a structural build writes. No third-party source.

#![allow(dead_code)] // each benchmark uses part of it

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use pdx_core::config::PdxConfig;
use pdx_core::coverage;
use pdx_core::ids::repo_id_from_url;
use pdx_core::index::derive::{DeriveInput, derive};
use pdx_core::index::discover::{discover, split_unportable};
use pdx_core::index::extract::{ExtractLimits, ExtractStage};
use pdx_core::index::write::segment_data;
use pdx_core::resolve::registry::SymbolRegistry;
use pdx_core::resolve::stages::ResolveReport;
use pdx_core::segment::{RepoIdSource, SegmentData, SegmentMeta, SegmentProfile};

/// Modules in the generated package.
pub const MODULES: usize = 48;

/// Functions per module.
pub const FUNCTIONS: usize = 6;

/// The fixture's URL; no part of it is fetched.
pub const URL: &str = "https://git.example/bench/fixture";

/// The fixture's commit.
pub const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

/// The resolved call sites the fixture must produce: eight calls in each of the 288
/// functions, two in each of the 48 classes, one in each of the six tests.
pub const SITES: usize = 2_406;

/// Its call sites by band, as resolution settles them at engine 8: per function, the
/// imported function (import-guided), a sibling, a class, a module attribute and
/// `h.run`, whose name the generic-name blocklist holds but whose typed answer comes
/// first (issue 103) (typed), `len` and `str` (external) and a name nothing defines
/// (unresolved); per class `self.step` (typed) and `sorted` (unresolved); per test the
/// imported function (typed).
pub const BANDS: [(&str, usize); 4] = [
    ("external", 576),
    ("import-guided", 288),
    ("typed", 1_206),
    ("unresolved", 336),
];

/// Writes the package below `root`.
pub fn write_project(root: &Path) {
    let package = root.join("pkg");
    fs::create_dir_all(&package).expect("the package directory");
    fs::write(package.join("__init__.py"), "").expect("__init__.py");
    for i in 0..MODULES {
        let next = (i + 1) % MODULES;
        let after = (i + 2) % MODULES;
        let mut text = format!(
            "from pkg.m{next} import f{next}_0, Helper{next}\nfrom pkg import m{after}\n\n\n"
        );
        for j in 0..FUNCTIONS {
            let sibling = (j + 1) % FUNCTIONS;
            let _ = write!(
                text,
                "def f{i}_{j}(x):\n    y = f{next}_0(x)\n    z = f{i}_{sibling}(y) if x > 0 else y\n    \
                 h = Helper{next}()\n    h.run(z)\n    m{after}.f{after}_1(z)\n    \
                 unknown_{j}(z)\n    return len(str(z))\n\n\n"
            );
        }
        let _ = write!(
            text,
            "class Helper{i}:\n    def run(self, v):\n        return self.step(v)\n\n    \
             def step(self, v):\n        return sorted([v])\n"
        );
        fs::write(package.join(format!("m{i}.py")), text).expect("a module");
    }
    let mut tests = String::from("from pkg.m0 import f0_0\n\n\n");
    for j in 0..FUNCTIONS {
        let _ = write!(tests, "def test_f{j}():\n    assert f0_0({j}) >= 0\n\n\n");
    }
    fs::create_dir_all(root.join("tests")).expect("the tests directory");
    fs::write(root.join("tests/test_m0.py"), tests).expect("a test module");
}

/// The fixture's configuration, files and symbol registry: Stages 1 and 2, then the
/// registry.
pub fn registry(root: &Path) -> SymbolRegistry {
    let config = PdxConfig::load(root).expect("the configuration");
    let (files, unportable) = split_unportable(discover(root, &config).expect("discovery"));
    assert!(unportable.is_empty());
    let limits = ExtractLimits {
        requested_workers: 2,
        memory_budget_bytes: 64 << 20,
    };
    let extracted = ExtractStage::new(root, &config.secrets, limits)
        .run(&files)
        .expect("extraction");
    SymbolRegistry::build(root, &files, extracted).expect("the registry")
}

/// Call sites by band.
pub fn bands(report: &ResolveReport) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for site in &report.resolutions {
        *counts
            .entry(site.resolution.band().as_str().to_owned())
            .or_default() += 1;
    }
    counts
}

/// Panics unless resolution settled the fixture as it is known to.
pub fn guard_resolution(report: &ResolveReport) {
    assert_eq!(report.resolutions.len(), SITES, "the fixture's call sites");
    let expected: BTreeMap<String, usize> = BANDS
        .iter()
        .map(|(band, count)| ((*band).to_owned(), *count))
        .collect();
    assert_eq!(bands(report), expected, "the fixture's bands");
}

/// The segment a structural build writes for the fixture: Stages 3 and 4 and the
/// coverage rows.
pub fn segment(root: &Path, registry: &SymbolRegistry, report: &ResolveReport) -> SegmentData {
    let config = PdxConfig::load(root).expect("the configuration");
    let repo = repo_id_from_url(URL).expect("an id");
    let graph = derive(&DeriveInput {
        repo: &repo,
        repo_name: "fixture",
        registry,
        resolution: report,
        root,
        config: &config,
    })
    .expect("derivation");
    let coverage = coverage::rows(registry, report, &[]).expect("coverage");
    let meta = SegmentMeta::new(
        repo,
        RepoIdSource::Url,
        URL,
        COMMIT,
        SegmentProfile::Structural,
        Vec::new(),
    );
    segment_data(meta, graph, coverage)
}
