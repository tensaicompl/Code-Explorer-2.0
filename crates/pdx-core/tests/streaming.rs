//! P2-17 (issue 82): a build keeps each extraction's surface in a spill and sheds the
//! facts only the engine reads, the engine reads each surface whole only for its own
//! file, and Stage 3 resolves files in parallel. None of it may change an answer: the
//! report and the graph are the ones the whole extractions, held in memory, give.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use pdx_core::bands::Band;
use pdx_core::config::PdxConfig;
use pdx_core::ids::RepoId;
use pdx_core::index::derive::{DeriveInput, DerivedGraph, derive, derive_pending};
use pdx_core::index::discover::discover;
use pdx_core::index::extract::{
    ExtractLimits, ExtractReport, ExtractStage, FileOutcome, SurfaceSpill,
};
use pdx_core::resolve::registry::SymbolRegistry;
use pdx_core::resolve::stages::{ResolveReport, resolve};

const FILES: &[(&str, &str)] = &[
    (
        "app/models.py",
        "class User:\n    def save(self):\n        return 1\n\n\ndef make_user():\n    return User()\n",
    ),
    (
        "app/service.py",
        "from app.models import make_user\n\n\ndef create():\n    user = make_user()\n    user.save()\n    return user\n",
    ),
    ("go.mod", "module example.com/shop\n\ngo 1.22\n"),
    (
        "cart/cart.go",
        "package cart\n\ntype Cart struct{ n int }\n\nfunc (c *Cart) Add() { c.n++ }\n\nfunc New() *Cart { return &Cart{} }\n",
    ),
    (
        "cart/use.go",
        "package cart\n\nfunc Fill() *Cart {\n\tc := New()\n\tc.Add()\n\treturn c\n}\n",
    ),
    (
        "web/util.ts",
        "export function slug(s: string): string { return s.trim(); }\n",
    ),
    (
        "web/page.ts",
        "import { slug } from './util';\n\nexport function title(s: string): string { return slug(s); }\n",
    ),
];

fn checkout() -> tempfile::TempDir {
    let dir = tempfile::Builder::new()
        .prefix("pdx-streaming-test-")
        .tempdir()
        .expect("a directory");
    for (path, content) in FILES {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
        fs::write(&path, content).expect("a file");
    }
    dir
}

/// Stage 2 over `root`, with a spill or without.
fn extract(
    root: &Path,
    spill: Option<Arc<SurfaceSpill>>,
) -> (
    Vec<pdx_core::index::discover::DiscoveredFile>,
    ExtractReport,
) {
    let config = PdxConfig::load(root).expect("the configuration loads");
    let files = discover(root, &config).expect("discovery succeeds");
    let limits = ExtractLimits {
        requested_workers: 2,
        memory_budget_bytes: 64 << 20,
    };
    let mut stage = ExtractStage::new(root, &config.secrets, limits);
    if let Some(spill) = spill {
        stage = stage.with_surface_spill(spill);
    }
    let report = stage.run(&files).expect("extraction succeeds");
    (files, report)
}

/// Stages 3 and 4 over Stage 2's report.
fn resolve_and_derive(
    root: &Path,
    files: &[pdx_core::index::discover::DiscoveredFile],
    report: ExtractReport,
) -> (ResolveReport, DerivedGraph, DerivedGraph) {
    let config = PdxConfig::load(root).expect("the configuration loads");
    let registry = SymbolRegistry::build(root, files, report).expect("the registry builds");
    let resolution = resolve(root, &registry).expect("resolution succeeds");
    let repo = RepoId::parse("ce63551447285fd4").expect("a repo id");
    let input = DeriveInput {
        repo: &repo,
        repo_name: "fixture",
        registry: &registry,
        resolution: &resolution,
        root,
        config: &config,
    };
    let graph = derive(&input).expect("derivation succeeds");
    let pending = derive_pending(&input)
        .expect("derivation succeeds")
        .finish();
    (resolution, graph, pending)
}

#[test]
fn streaming_build_is_byte_identical() {
    let dir = checkout();
    let root = dir.path();
    let (files, whole) = extract(root, None);
    let (expected, graph, _) = resolve_and_derive(root, &files, whole);
    assert!(
        expected
            .resolutions
            .iter()
            .any(|r| r.resolution.band() == Band::Typed),
        "the fixture exercises typed resolution"
    );
    let spill = Arc::new(SurfaceSpill::create().expect("a spill"));
    let (files, kept) = extract(root, Some(Arc::clone(&spill)));
    let (report, streamed, pending) = resolve_and_derive(root, &files, kept);
    assert_eq!(report, expected);
    assert_eq!(streamed, graph);
    assert_eq!(pending, graph, "a graph finished later is the same graph");
}

#[test]
fn stage2_holds_no_full_extraction() {
    let dir = checkout();
    let root = dir.path();
    let (_, whole) = extract(root, None);
    let usages: usize = whole
        .files
        .iter()
        .filter_map(|f| match &f.outcome {
            FileOutcome::Extracted { extract, .. } => Some(extract.usages.len()),
            _ => None,
        })
        .sum();
    assert!(usages > 0, "the fixture has facts to shed");
    let spill = Arc::new(SurfaceSpill::create().expect("a spill"));
    let (_, kept) = extract(root, Some(Arc::clone(&spill)));
    let mut extracted = 0;
    for file in &kept.files {
        let FileOutcome::Extracted { extract, .. } = &file.outcome else {
            continue;
        };
        extracted += 1;
        assert!(
            extract.surface.is_empty(),
            "{}: its surface is resident",
            file.path
        );
        assert!(
            extract.usages.is_empty()
                && extract.type_refs.is_empty()
                && extract.read_writes.is_empty()
                && extract.throws.is_empty()
                && extract.env_accesses.is_empty(),
            "{}: it holds facts only the engine reads",
            file.path
        );
        assert!(
            spill
                .get(&file.path)
                .expect("the spill reads")
                .is_some_and(|s| !s.is_empty()),
            "{}: its surface is in the spill",
            file.path
        );
    }
    assert_eq!(spill.len(), extracted);
    assert!(kept.surfaces.is_some());
}
