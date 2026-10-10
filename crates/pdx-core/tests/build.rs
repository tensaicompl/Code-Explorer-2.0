//! The whole pipeline (4.5, P2-10): `build_segment` over checkouts written here,
//! Stages 1 to 5, its coverage rows (4.3 `coverage`), its degraded status, Stage 5's
//! segment and the issues P2-10 settles (53 and 57).
//!
//! Every expected count is written from the fixture by hand or counted here
//! independently of the code under test, never by calling it.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use pdx_core::bands::Band;
use pdx_core::config::PdxConfig;
use pdx_core::coverage;
use pdx_core::ids::RepoId;
use pdx_core::index::derive::{DeriveInput, DerivedGraph, derive};
use pdx_core::index::discover::{DiscoveredFile, discover, split_unportable};
use pdx_core::index::extract::{
    ExtractBackend, ExtractError, ExtractLimits, ExtractReport, ExtractStage,
};
use pdx_core::index::{
    DegradedReason, IndexError, IndexRequest, Progress, SegmentReport, SegmentStatus, Stage,
    build_segment,
};
use pdx_core::kinds::{EdgeKind, NodeKind};
use pdx_core::model::{Coverage, FileStatus};
use pdx_core::resolve::registry::SymbolRegistry;
use pdx_core::resolve::stages::{ResolveReport, resolve};
use pdx_core::segment::SegmentReader;
use pdx_engine::isolate::{ExtractFailure, ExtractOutcome, Extractor, IsolationError, SourceFile};
use pdx_engine::{Engine, EngineError};
use sha2::{Digest, Sha256};

const REPO: &str = "ce63551447285fd4";
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

const LIMITS: ExtractLimits = ExtractLimits {
    requested_workers: 2,
    memory_budget_bytes: 64 << 20,
};

/// A checkout on disk, and a directory for what is built from it.
struct Checkout {
    dir: tempfile::TempDir,
    out: tempfile::TempDir,
}

impl Checkout {
    fn new(files: &[(&str, &str)]) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("pdx-build-test-")
            .tempdir()
            .expect("a directory");
        for (path, content) in files {
            let path = dir.path().join(path);
            fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
            fs::write(&path, content).expect("a file");
        }
        Self {
            dir,
            out: tempfile::tempdir().expect("an output directory"),
        }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn destination(&self, name: &str) -> PathBuf {
        self.out.path().join(name)
    }

    fn request<'a>(&'a self, destination: &str) -> IndexRequest<'a> {
        IndexRequest {
            root: self.root(),
            repo_id: RepoId::parse(REPO).expect("a repo id"),
            repo_url: "https://git.example/acme/shop.git".to_owned(),
            repo_name: "shop".to_owned(),
            commit_sha: COMMIT.to_owned(),
            limits: LIMITS,
            cache: None,
            backend: None,
            destination: self.destination(destination),
            build_dir: None,
            progress: None,
        }
    }

    fn build(&self) -> SegmentReport {
        build_segment(self.request("segment.db")).expect("the build succeeds")
    }
}

/// Stages 1 to 4 run by hand, for what the report does not carry.
struct Stages {
    discovered: Vec<DiscoveredFile>,
    unportable: Vec<DiscoveredFile>,
    registry: SymbolRegistry,
    resolution: ResolveReport,
    graph: DerivedGraph,
}

fn stages(checkout: &Checkout) -> Stages {
    let root = checkout.root();
    let config = PdxConfig::load(root).expect("the configuration loads");
    let all = discover(root, &config).expect("discovery succeeds");
    let (discovered, unportable) = split_unportable(all);
    let extracted: ExtractReport = ExtractStage::new(root, &config.secrets, LIMITS)
        .run(&discovered)
        .expect("extraction succeeds");
    let registry =
        SymbolRegistry::build(root, &discovered, extracted).expect("the registry builds");
    let resolution = resolve(root, &registry).expect("resolution succeeds");
    let graph = derive(&DeriveInput {
        repo: &RepoId::parse(REPO).expect("a repo id"),
        repo_name: "shop",
        registry: &registry,
        resolution: &resolution,
        root,
        config: &config,
    })
    .expect("derivation succeeds");
    Stages {
        discovered,
        unportable,
        registry,
        resolution,
        graph,
    }
}

fn row<'c>(coverage: &'c [Coverage], language: &str) -> &'c Coverage {
    coverage
        .iter()
        .find(|c| c.language == language)
        .unwrap_or_else(|| panic!("no {language} row: {coverage:#?}"))
}

fn sha256(path: &Path) -> String {
    let bytes = fs::read(path).expect("the segment");
    Sha256::digest(&bytes)
        .iter()
        .fold(String::new(), |mut hex, b| {
            write!(hex, "{b:02x}").expect("a string");
            hex
        })
}

/// The language of each discovered path, as discovery assigned it: the test's own
/// attribution, not coverage's.
fn languages(discovered: &[DiscoveredFile]) -> BTreeMap<&str, &str> {
    discovered
        .iter()
        .map(|f| (f.path.as_str(), f.language.map_or("unknown", |l| l.id)))
        .collect()
}

/// Every band's name: the vocabulary 4.2.2 fixes, written out.
const BAND_NAMES: [&str; 11] = [
    "precise",
    "typed",
    "import-guided",
    "inheritance-guided",
    "exact",
    "scoped",
    "candidate",
    "external",
    "blocked",
    "unresolved",
    "contradicted",
];

// --- fixtures ---------------------------------------------------------------------------

/// Python: `run` calls `helper` twice and `log_line` once (three call sites); a
/// reference `handler = helper` is no call site. Go: `Run` calls `Helper` once. C: six,
/// the engine's: the macro's invocation `TWICE(helper)`, the written `helper()`, and
/// four `helper` calls from the macro, one placed at the invocation and three found in
/// the expansion with no position in the file (counted, never stored as sites).
/// TypeScript: one call. And files of no language, binary, redacted and too large.
const MIXED: [(&str, &str); 10] = [
    ("pdx.toml", "[discover]\nmax_file_bytes = 4096\n"),
    (
        "app/service.py",
        "def helper():\n    return 1\n\n\ndef log_line(text):\n    return text\n\n\ndef run():\n    log_line(\"x\")\n    return helper() + helper()\n\n\nhandler = helper\n",
    ),
    ("app/go.mod", "module example.com/app\n\ngo 1.22\n"),
    (
        "app/calc.go",
        "package app\n\nfunc Helper() int { return 1 }\n\nfunc Run() int { return Helper() }\n",
    ),
    (
        "native/main.c",
        "#define TWICE(f) f(); f()\n\nstatic int helper(void) { return 1; }\n\nint run(void) {\n  TWICE(helper);\n  helper();\n  return 0;\n}\n",
    ),
    (
        "web/total.ts",
        "export function add(a: number, b: number): number {\n  return a + b;\n}\n\nexport function total(): number {\n  return add(1, 2);\n}\n",
    ),
    ("docs/notes.unknownext", "plain text\n"),
    ("assets/logo.bin", "\0\u{1}binary"),
    ("config/.env.production", "MODE=1\n"),
    ("data/big.py", ""),
];

fn mixed_files() -> Vec<(&'static str, String)> {
    MIXED
        .iter()
        .map(|(path, content)| {
            let content = if *path == "data/big.py" {
                "x = 1\n".repeat(1000)
            } else {
                (*content).to_owned()
            };
            (*path, content)
        })
        .collect()
}

fn mixed() -> Checkout {
    let files = mixed_files();
    let files: Vec<(&str, &str)> = files.iter().map(|(p, c)| (*p, c.as_str())).collect();
    Checkout::new(&files)
}

// --- coverage ---------------------------------------------------------------------------

#[test]
fn coverage_counts_sum_to_call_sites() {
    let checkout = mixed();
    let s = stages(&checkout);
    let rows = coverage::rows(&s.registry, &s.resolution, &s.unportable).expect("coverage");

    // Every row's bands sum to its call sites, and every band is stored, by name.
    for r in &rows {
        assert_eq!(r.by_band.total(), r.call_sites, "{}", r.language);
        let json = serde_json::to_value(r.by_band).expect("by_band serialises");
        let keys: BTreeSet<&str> = json
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, BTreeSet::from(BAND_NAMES), "{}", r.language);
        assert_eq!(r.observed_links, 0);
    }

    // The call sites are Stage 3's: every resolution once, in its file's language,
    // under its band, counted here from the resolutions themselves.
    let language = languages(&s.discovered);
    let mut expected: BTreeMap<&str, BTreeMap<Band, u64>> = BTreeMap::new();
    for r in &s.resolution.resolutions {
        let l = language[r.site_ref.rel_path.as_str()];
        *expected
            .entry(l)
            .or_default()
            .entry(r.resolution.band())
            .or_default() += 1;
    }
    for (l, bands) in &expected {
        let r = row(&rows, l);
        assert_eq!(r.call_sites, bands.values().sum::<u64>(), "{l}");
        for band in Band::ALL {
            assert_eq!(
                r.by_band.get(*band),
                bands.get(band).copied().unwrap_or(0),
                "{l} {band}"
            );
        }
    }
    let total: u64 = rows.iter().map(|r| r.call_sites).sum();
    assert_eq!(total, s.resolution.resolutions.len() as u64);

    // By hand: Python has three calls (`log_line`, `helper` twice); the reference
    // `handler = helper` is not one. Go has one, TypeScript one, C six (above).
    assert_eq!(row(&rows, "python").call_sites, 3);
    assert_eq!(row(&rows, "go").call_sites, 1);
    assert_eq!(row(&rows, "typescript").call_sites, 1);
    assert_eq!(row(&rows, "c").call_sites, 6);
    // Structural indexing has no compiler evidence and observes nothing at run time.
    for r in &rows {
        assert_eq!(r.by_band.get(Band::Precise), 0);
        assert_eq!(r.by_band.get(Band::Contradicted), 0);
    }
}

#[test]
fn coverage_counts_sites_stage_4_cannot_place() {
    // Three of the macro's calls resolve, but have no position in the file, so Stage 4
    // stores no site, edge or candidate row for them. They are still call sites.
    let checkout = mixed();
    let s = stages(&checkout);
    let rows = coverage::rows(&s.registry, &s.resolution, &s.unportable).expect("coverage");
    let c_file = &s.graph.file_nodes["native/main.c"];
    let c_nodes: BTreeSet<_> = s
        .graph
        .nodes
        .iter()
        .filter(|n| n.file_id.as_ref() == Some(c_file) || &n.node_id == c_file)
        .map(|n| &n.node_id)
        .collect();
    let persisted_calls = s
        .graph
        .edges
        .iter()
        .filter(|e| matches!(e.kind, EdgeKind::Calls | EdgeKind::CallReference))
        .filter(|e| c_nodes.contains(&e.src))
        .count()
        + s.graph
            .candidates
            .iter()
            .filter(|c| c_nodes.contains(&c.src))
            .count();
    let unplaced = s
        .graph
        .diagnostics
        .unmaterialized
        .iter()
        .filter(|u| u.site_ref.rel_path == "native/main.c")
        .count();
    assert_eq!(unplaced, 3, "{:#?}", s.graph.diagnostics.unmaterialized);
    assert_eq!(persisted_calls, 3);
    assert_eq!(row(&rows, "c").call_sites, 6);
    assert_eq!(
        row(&rows, "c").call_sites,
        (persisted_calls + unplaced) as u64
    );
}

#[test]
fn unconfirmed_sites_are_not_call_sites() {
    // `handler = helper` is a reference typed resolution did not settle: Stage 3
    // reports it as unconfirmed, and coverage counts it nowhere, not as unresolved.
    let checkout = mixed();
    let s = stages(&checkout);
    let rows = coverage::rows(&s.registry, &s.resolution, &s.unportable).expect("coverage");
    let unconfirmed: Vec<_> = s
        .resolution
        .unconfirmed
        .iter()
        .filter(|u| u.site_ref.rel_path == "app/service.py")
        .collect();
    assert_eq!(unconfirmed.len(), 1, "{unconfirmed:#?}");
    let python = row(&rows, "python");
    assert_eq!(python.call_sites, 3);
    let unresolved_resolutions = s
        .resolution
        .resolutions
        .iter()
        .filter(|r| r.site_ref.rel_path == "app/service.py")
        .filter(|r| r.resolution.band() == Band::Unresolved)
        .count() as u64;
    assert_eq!(python.by_band.get(Band::Unresolved), unresolved_resolutions);
}

#[test]
fn coverage_attributes_languages_and_counts_files() {
    let checkout = mixed();
    let s = stages(&checkout);
    let rows = coverage::rows(&s.registry, &s.resolution, &s.unportable).expect("coverage");
    // One row per language with a file, and no other: here python, go, c, typescript,
    // toml (`pdx.toml`), properties (the secret file) and unknown (no language, binary).
    let mut languages: Vec<&str> = rows.iter().map(|r| r.language.as_str()).collect();
    let sorted = {
        let mut l = languages.clone();
        l.sort_unstable();
        l
    };
    assert_eq!(languages, sorted, "rows in language order");
    languages.sort_unstable();
    let present: BTreeSet<&str> = s
        .discovered
        .iter()
        .map(|f| f.language.map_or("unknown", |l| l.id))
        .collect();
    assert_eq!(languages.into_iter().collect::<BTreeSet<_>>(), present);
    // Files and their statuses, by hand: `app/service.py` parsed, `data/big.py`
    // skipped for size (4096 bytes allowed, 6000 written).
    let python = row(&rows, "python");
    assert_eq!(
        (
            python.files,
            python.parsed,
            python.partial,
            python.failed,
            python.skipped
        ),
        (2, 1, 0, 0, 1)
    );
    let c = row(&rows, "c");
    assert_eq!((c.files, c.parsed), (1, 1));
    let go = row(&rows, "go");
    assert_eq!((go.files, go.parsed), (1, 1));
    // Binary and redacted files are files of the repository, counted in `files` and in
    // none of parsed, partial, failed or skipped: nothing was attempted (decision 33).
    let total_files: u64 = rows.iter().map(|r| r.files).sum();
    assert_eq!(total_files, MIXED.len() as u64);
    let attempted: u64 = rows
        .iter()
        .map(|r| r.parsed + r.partial + r.failed + r.skipped)
        .sum();
    let binary_or_redacted = s
        .graph
        .files
        .iter()
        .filter(|f| matches!(f.status, FileStatus::Binary | FileStatus::Redacted))
        .count() as u64;
    assert_eq!(binary_or_redacted, 2);
    assert_eq!(total_files, attempted + binary_or_redacted);
}

#[test]
fn coverage_symbols_are_definitions() {
    // Definitions found, by hand: never the repository, folders, files, semantic
    // modules or routes, and never the engine's file-level module (the file itself).
    // A test callable is one definition, whatever its node's kind; a namespace block
    // is a definition the engine found.
    let checkout = Checkout::new(&[
        // `Shop`, its method `total`, the function `helper`: 3.
        ("shop/__init__.py", ""),
        (
            "shop/orders.py",
            "class Shop:\n    def total(self):\n        return helper()\n\n\ndef helper():\n    return 1\n",
        ),
        // `test_total`: 1, a `Test` node.
        (
            "tests/test_orders.py",
            "from shop.orders import Shop\n\n\ndef test_total():\n    assert Shop().total() == 1\n",
        ),
        // The namespace `Inner` and the function `inside`: 2.
        (
            "web/inner.ts",
            "namespace Inner {\n  export function inside() { return 1; }\n}\n",
        ),
    ]);
    let s = stages(&checkout);
    let rows = coverage::rows(&s.registry, &s.resolution, &s.unportable).expect("coverage");
    assert_eq!(row(&rows, "python").symbols, 4);
    assert_eq!(row(&rows, "typescript").symbols, 2);
    // Not the graph's node count: it has repositories, folders, files and modules too.
    let nodes = s.graph.nodes.len() as u64;
    assert!(nodes > 6, "{nodes}");
    assert!(
        s.graph
            .nodes
            .iter()
            .any(|n| n.kind == NodeKind::Test && n.name == "test_total")
    );
}

// --- the whole pipeline -----------------------------------------------------------------

#[test]
fn build_segment_end_to_end() {
    let checkout = mixed();
    let report = checkout.build();
    let s = stages(&checkout);

    // A complete segment where it was asked for, hashed as written. Degraded: one file
    // of ten skipped for its size is 10%, over 4.5's 5%, and that is the only reason.
    assert_eq!(report.status, SegmentStatus::Degraded);
    assert_eq!(
        report.degraded,
        vec![DegradedReason::FailedOverThreshold {
            failed: 0,
            skipped_size: 1,
            files: 10
        }]
    );
    let path = checkout.destination("segment.db");
    assert_eq!(report.segment.path, std::path::absolute(&path).unwrap());
    assert_eq!(report.segment.content_sha256, sha256(&path));
    assert_eq!(
        report.segment.size_bytes,
        fs::metadata(&path).expect("the segment").len()
    );

    // Stage 5 wrote Stage 4's rows and the coverage, read back through the reader,
    // which verifies the file on opening.
    let reader = SegmentReader::open(&path).expect("the segment opens");
    let meta = reader.meta();
    assert_eq!(meta.repo_id.as_str(), REPO);
    assert_eq!(meta.commit_sha, COMMIT);
    assert_eq!(reader.coverage_all().expect("coverage"), report.coverage);
    assert_eq!(
        report.coverage,
        coverage::rows(&s.registry, &s.resolution, &s.unportable).expect("coverage")
    );
    for n in &s.graph.nodes {
        assert_eq!(reader.node(&n.node_id).expect("a query").as_ref(), Some(n));
    }
    for f in &s.graph.files {
        assert_eq!(
            reader.file_by_path(&f.path).expect("a query").as_ref(),
            Some(f)
        );
    }
    let counts = report.rows;
    assert_eq!(counts.files, s.graph.files.len() as u64);
    assert_eq!(counts.nodes, s.graph.nodes.len() as u64);
    assert_eq!(counts.sites, s.graph.sites.len() as u64);
    assert_eq!(counts.edges, s.graph.edges.len() as u64);
    assert_eq!(counts.candidates, s.graph.candidates.len() as u64);
    assert_eq!(counts.contracts, s.graph.contracts.len() as u64);
    assert_eq!(counts.metrics, s.graph.metrics.len() as u64);
    assert_eq!(counts.coverage, report.coverage.len() as u64);
    let stored_metrics: usize = s
        .graph
        .nodes
        .iter()
        .map(|n| reader.metrics(&n.node_id).expect("a query").len())
        .sum();
    assert_eq!(stored_metrics, s.graph.metrics.len());

    // The report's totals, by hand: 10 files; parsed: `pdx.toml`, `service.py`,
    // `calc.go`, `main.c`, `total.ts`; skipped: `go.mod` and the unknown extension (no
    // language) and `big.py` (size); one binary, one redacted.
    assert_eq!(report.files.files, 10);
    assert_eq!(report.files.parsed, 5);
    assert_eq!(report.files.partial, 0);
    assert_eq!(report.files.skipped, 3);
    assert_eq!(report.files.binary, 1);
    assert_eq!(report.files.redacted, 1);
    assert_eq!(report.files.skipped_size, 1);
    assert_eq!(report.files.failed, 0);
    assert_eq!(
        report.resolution.call_sites,
        s.resolution.resolutions.len() as u64
    );
    assert_eq!(
        report.resolution.unconfirmed,
        s.resolution.unconfirmed.len() as u64
    );

    // A fatal error publishes nothing and leaves what is there alone.
    let again = build_segment(checkout.request("segment.db"));
    assert!(matches!(again, Err(IndexError::Write(_))), "{again:?}");
    assert_eq!(sha256(&path), report.segment.content_sha256);
}

#[test]
fn degraded_when_failed_over_threshold() {
    // 4.5: degraded when failed + skipped(size) > 5% of files, strictly. Failed files
    // here are genuine: Perl nested past the depth the engine parses it to.
    let failed = format!("my $x = {}1{};\n", "f(".repeat(200), ")".repeat(200));
    let big = "x = 1\n".repeat(1000);
    let fixture = |plain: usize, failing: usize, oversized: usize| {
        let mut files: Vec<(String, String)> = vec![(
            "pdx.toml".to_owned(),
            "[discover]\nmax_file_bytes = 4096\n".to_owned(),
        )];
        for i in 0..plain {
            files.push((
                format!("src/m{i:02}.py"),
                format!("def f{i}():\n    return {i}\n"),
            ));
        }
        for i in 0..failing {
            files.push((format!("deep/d{i}.pl"), failed.clone()));
        }
        for i in 0..oversized {
            files.push((format!("data/big{i}.py"), big.clone()));
        }
        files
    };
    let build = |files: &[(String, String)]| {
        let files: Vec<(&str, &str)> = files
            .iter()
            .map(|(p, c)| (p.as_str(), c.as_str()))
            .collect();
        let checkout = Checkout::new(&files);
        let report = checkout.build();
        (checkout, report)
    };

    // Exactly 5%: 2 of 40 files (pdx.toml, 37 modules, 2 failed). Not degraded.
    let files = fixture(37, 2, 0);
    assert_eq!(files.len(), 40);
    let (_checkout, report) = build(&files);
    assert_eq!(report.files.files, 40);
    assert_eq!(report.files.failed, 2);
    assert_eq!(
        report.status,
        SegmentStatus::Ready,
        "{:#?}",
        report.degraded
    );

    // Just over: 2 of 39 files (5.13%). Degraded, for that reason alone.
    let files = fixture(36, 2, 0);
    assert_eq!(files.len(), 39);
    let (_checkout, report) = build(&files);
    assert_eq!(report.status, SegmentStatus::Degraded);
    assert_eq!(
        report.degraded,
        vec![DegradedReason::FailedOverThreshold {
            failed: 2,
            skipped_size: 0,
            files: 39
        }]
    );

    // Skipped for size counts with failed: 1 + 1 of 40 is 5%, of 39 more.
    let (_checkout, report) = build(&fixture(37, 1, 1));
    assert_eq!(
        report.status,
        SegmentStatus::Ready,
        "{:#?}",
        report.degraded
    );
    let (_checkout, report) = build(&fixture(36, 1, 1));
    assert_eq!(
        report.degraded,
        vec![DegradedReason::FailedOverThreshold {
            failed: 1,
            skipped_size: 1,
            files: 39
        }]
    );
    // The failed files are counted as failed, with the parse as their reason.
    let reader = SegmentReader::open(&report.segment.path).expect("the segment opens");
    let deep = reader
        .file_by_path("deep/d0.pl")
        .expect("a query")
        .expect("the file");
    assert_eq!(
        (deep.status, deep.status_reason.as_deref()),
        (FileStatus::Failed, Some("parse"))
    );
}

#[test]
fn malformed_sources_follow_engine_status() {
    // Issue 57: a file's status is the engine's report, never a second parse. A
    // syntax error inside a definition the engine recovered leaves no error region:
    // `parsed`. One it could not recover leaves one: `partial`. A file it cannot parse
    // at all: `failed`. Only `failed` counts toward the degraded threshold.
    let deep = format!("my $x = {}1{};\n", "f(".repeat(200), ")".repeat(200));
    let checkout = Checkout::new(&[
        ("src/parsed_malformed.py", "def broken(:\n    return\n"),
        (
            "src/partial.py",
            "def fine():\n    return 1\n\n\ndef broken(x:\n    return x\n",
        ),
        ("src/failed.pl", deep.as_str()),
    ]);
    let report = checkout.build();
    let reader = SegmentReader::open(&report.segment.path).expect("the segment opens");
    let status = |path: &str| {
        let f = reader
            .file_by_path(path)
            .expect("a query")
            .expect("the file");
        (f.status, f.status_reason)
    };
    assert_eq!(
        status("src/parsed_malformed.py"),
        (FileStatus::Parsed, None)
    );
    assert_eq!(status("src/partial.py"), (FileStatus::Partial, None));
    assert_eq!(
        status("src/failed.pl"),
        (FileStatus::Failed, Some("parse".to_owned()))
    );
    let python = row(&report.coverage, "python");
    assert_eq!((python.parsed, python.partial, python.failed), (1, 1, 0));
    let perl = row(&report.coverage, "perl");
    assert_eq!((perl.parsed, perl.partial, perl.failed), (0, 0, 1));
    // 1 failed of 3 files is over 5%: degraded by the threshold, and by nothing else.
    // A partial file is no reason.
    assert_eq!(
        report.degraded,
        vec![DegradedReason::FailedOverThreshold {
            failed: 1,
            skipped_size: 0,
            files: 3
        }]
    );
}

#[test]
fn partial_unknown_binary_and_redacted_do_not_degrade() {
    let checkout = Checkout::new(&[
        (
            "src/partial.py",
            "def fine():\n    return 1\n\n\ndef broken(x:\n    return x\n",
        ),
        ("docs/notes.unknownext", "text\n"),
        ("assets/logo.bin", "\0\u{1}binary"),
        ("config/.env.production", "MODE=1\n"),
    ]);
    let report = checkout.build();
    assert_eq!(
        report.status,
        SegmentStatus::Ready,
        "{:#?}",
        report.degraded
    );
    assert_eq!(
        (
            report.files.partial,
            report.files.binary,
            report.files.redacted
        ),
        (1, 1, 1)
    );
}

/// A backend over the in-process engine that crashes on, or is refused, named files.
struct Twisted {
    engine: Extractor,
    crash: &'static [&'static str],
    refuse: &'static [&'static str],
}

impl ExtractBackend for Twisted {
    fn extract_batch(
        &mut self,
        files: &[SourceFile],
    ) -> Result<Vec<ExtractOutcome>, IsolationError> {
        let mut outcomes = self.engine.extract_batch(files)?;
        for (file, outcome) in files.iter().zip(&mut outcomes) {
            if self.crash.contains(&file.rel_path.as_str()) {
                *outcome = ExtractOutcome::Failed(ExtractFailure::EngineCrash);
            } else if self.refuse.contains(&file.rel_path.as_str()) {
                *outcome = ExtractOutcome::Failed(ExtractFailure::Engine(EngineError::Internal));
            }
        }
        Ok(outcomes)
    }
}

fn twisted(
    crash: &'static [&'static str],
    refuse: &'static [&'static str],
) -> impl Fn() -> Result<Box<dyn ExtractBackend>, ExtractError> + Sync {
    move || {
        let engine = Engine::new().map_err(ExtractError::Engine)?;
        Ok(Box::new(Twisted {
            engine: Extractor::InProcess(engine),
            crash,
            refuse,
        }) as Box<dyn ExtractBackend>)
    }
}

/// Twenty-five small Python modules: one failed file of them is under 5%.
fn many_modules() -> Vec<(String, String)> {
    (0..25)
        .map(|i| {
            (
                format!("src/m{i:02}.py"),
                format!("def f{i}():\n    return {i}\n"),
            )
        })
        .collect()
}

#[test]
fn explicit_stage_2_degradation_propagates() {
    let files = many_modules();
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, c)| (p.as_str(), c.as_str()))
        .collect();
    let checkout = Checkout::new(&files);

    // A crash on one file of 25 (4%): under the threshold, and degraded all the same.
    let factory = twisted(&["src/m03.py"], &[]);
    let mut request = checkout.request("crash.db");
    request.backend = Some(&factory);
    let report = build_segment(request).expect("the build succeeds");
    assert_eq!(report.status, SegmentStatus::Degraded);
    assert_eq!(
        report.degraded,
        vec![DegradedReason::EngineCrash {
            paths: vec!["src/m03.py".to_owned()]
        }]
    );
    assert_eq!(report.files.failed, 1);

    // An engine error on one file of 25 is only a failed file: under 5%, ready.
    let factory = twisted(&[], &["src/m04.py"]);
    let mut request = checkout.request("refused.db");
    request.backend = Some(&factory);
    let report = build_segment(request).expect("the build succeeds");
    assert_eq!(
        report.status,
        SegmentStatus::Ready,
        "{:#?}",
        report.degraded
    );
    assert_eq!(report.files.failed, 1);

    // A file larger than the whole memory budget is skipped for memory: degraded.
    let mut request = checkout.request("memory.db");
    request.limits = ExtractLimits {
        requested_workers: 1,
        memory_budget_bytes: 24,
    };
    let report = build_segment(request).expect("the build succeeds");
    let DegradedReason::SkippedMemory { paths } = &report.degraded[0] else {
        panic!("{:#?}", report.degraded);
    };
    assert!(!paths.is_empty());
    assert_eq!(report.status, SegmentStatus::Degraded);
}

#[test]
fn a_worker_timeout_is_fatal() {
    struct Hung;
    impl ExtractBackend for Hung {
        fn extract_batch(
            &mut self,
            _files: &[SourceFile],
        ) -> Result<Vec<ExtractOutcome>, IsolationError> {
            Err(IsolationError::Timeout {
                after: std::time::Duration::from_millis(1),
                worker: 0,
            })
        }
    }
    let factory = || Ok(Box::new(Hung) as Box<dyn ExtractBackend>);
    let checkout = Checkout::new(&[("src/a.py", "def a():\n    return 1\n")]);
    let mut request = checkout.request("timeout.db");
    request.backend = Some(&factory);
    let result = build_segment(request);
    assert!(matches!(result, Err(IndexError::Extract(_))), "{result:?}");
    assert!(!checkout.destination("timeout.db").exists());
}

// --- issue 53 ---------------------------------------------------------------------------

#[test]
fn split_unportable_keeps_identities_portable() {
    // Discovery's paths that no host-independent identity can be made from are held
    // back; every other path passes unchanged and in order.
    let file = |path: &str| DiscoveredFile {
        path: path.to_owned(),
        language: pdx_core::languages::by_id("python"),
        disposition: pdx_core::index::discover::Disposition::Candidate,
        size_bytes: 1,
    };
    let (portable, unportable) = split_unportable(vec![
        file("a.py"),
        file("src\\b.py"),
        file("c:d.py"),
        file("dir\\x/e.py"),
        file("src/f.py"),
    ]);
    let paths = |files: &[DiscoveredFile]| -> Vec<String> {
        files.iter().map(|f| f.path.clone()).collect()
    };
    assert_eq!(paths(&portable), ["a.py", "src/f.py"]);
    assert_eq!(paths(&unportable), ["src\\b.py", "c:d.py", "dir\\x/e.py"]);
}

#[cfg(unix)]
#[test]
fn unportable_path_is_accounted_never_identified() {
    // Issue 53: a file name holding `\` (and one Windows reads as a drive) has no
    // identity the same on every host. The build holds it back from Stages 2 to 5,
    // counts it as skipped in its language, names it in the report, and makes no
    // node, file row or site from it. Every other file is indexed as before.
    let checkout = Checkout::new(&[
        (
            "src/a.py",
            "def a():\n    return b()\n\n\ndef b():\n    return 1\n",
        ),
        ("src/back\\slash.py", "def hidden():\n    return 1\n"),
        ("c:drive.py", "def drive():\n    return 1\n"),
    ]);
    let report = checkout.build();
    assert_eq!(
        report.unportable,
        vec!["c:drive.py".to_owned(), "src/back\\slash.py".to_owned()]
    );
    let python = row(&report.coverage, "python");
    assert_eq!(
        (python.files, python.parsed, python.skipped),
        (3, 1, 2),
        "{python:#?}"
    );
    assert_eq!(report.files.files, 3);
    assert_eq!(report.files.skipped, 2);
    let reader = SegmentReader::open(&report.segment.path).expect("the segment opens");
    assert!(reader.file_by_path("src/a.py").expect("a query").is_some());
    for path in ["src/back\\slash.py", "c:drive.py"] {
        assert!(
            reader.file_by_path(path).expect("a query").is_none(),
            "{path}"
        );
    }
    let hits = reader.search("hidden", 10).expect("a search");
    assert!(hits.is_empty(), "{hits:?}");
    let hits = reader.search("drive", 10).expect("a search");
    assert!(hits.is_empty(), "{hits:?}");
    // Not degraded: 2 skipped (not for size) is not a degradation reason.
    assert_eq!(report.status, SegmentStatus::Ready);
    // The same as a checkout without them, but for the coverage that counts them.
    let clean = Checkout::new(&[(
        "src/a.py",
        "def a():\n    return b()\n\n\ndef b():\n    return 1\n",
    )]);
    let clean_report = clean.build();
    assert_eq!(clean_report.rows, report.rows);
}

// --- progress and determinism (tracing: `tests/build_tracing.rs`) ------------------------

#[test]
fn progress_never_changes_the_segment() {
    let checkout = mixed();
    let plain = build_segment(checkout.request("plain.db")).expect("the build succeeds");

    // With a progress callback: every stage started and finished, in order, from the
    // building thread, with the same segment.
    let events = RefCell::new(Vec::new());
    let callback = |p: &Progress| events.borrow_mut().push(p.clone());
    let mut request = checkout.request("progress.db");
    request.progress = Some(&callback);
    let with_progress = build_segment(request).expect("the build succeeds");
    let events = events.into_inner();
    let stages = [
        Stage::Discover,
        Stage::Extract,
        Stage::Resolve,
        Stage::Derive,
        Stage::Write,
    ];
    let started: Vec<Stage> = events
        .iter()
        .filter_map(|e| match e {
            Progress::Started(stage) => Some(*stage),
            Progress::Finished { .. } => None,
        })
        .collect();
    assert_eq!(started, stages);
    assert_eq!(events.len(), 10);
    for pair in events.chunks(2) {
        let (Progress::Started(a), Progress::Finished { stage: b, .. }) = (&pair[0], &pair[1])
        else {
            panic!("{events:?}");
        };
        assert_eq!(a, b);
    }
    // Their counts are the build's: 10 files discovered, the same 10 given to
    // extraction (no unportable path here), the call sites, the nodes, the bytes.
    let finished = |stage: Stage| {
        events
            .iter()
            .find_map(|e| match e {
                Progress::Finished { stage: s, items } if *s == stage => Some(*items),
                _ => None,
            })
            .expect("finished")
    };
    assert_eq!(finished(Stage::Discover), 10);
    assert_eq!(finished(Stage::Extract), 10);
    assert_eq!(finished(Stage::Resolve), plain.resolution.call_sites);
    assert_eq!(finished(Stage::Derive), plain.rows.nodes);
    assert_eq!(finished(Stage::Write), plain.segment.size_bytes);

    // The same bytes with a callback as without.
    assert_eq!(
        with_progress.segment.content_sha256,
        plain.segment.content_sha256
    );
}

#[test]
fn repeated_builds_are_deterministic() {
    // The same checkout twice, and the same files in another directory, built with a
    // different number of workers: the same segment, byte for byte.
    let first = mixed();
    let second = mixed();
    let a = first.build();
    let b = build_segment(first.request("again.db")).expect("the build succeeds");
    let mut request = second.request("other.db");
    request.limits = ExtractLimits {
        requested_workers: 1,
        memory_budget_bytes: 64 << 20,
    };
    let c = build_segment(request).expect("the build succeeds");
    assert_eq!(a.segment.content_sha256, b.segment.content_sha256);
    assert_eq!(a.segment.content_sha256, c.segment.content_sha256);
    assert_eq!(a.coverage, c.coverage);
    assert_eq!(a.rows, c.rows);
}
