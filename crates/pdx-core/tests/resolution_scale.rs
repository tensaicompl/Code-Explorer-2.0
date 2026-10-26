//! Issue 122: Stage 3 at scale. A `candidate` site holds at most
//! [`MAX_STORED_CANDIDATES`](pdx_core::consts::MAX_STORED_CANDIDATES) candidates, the first by path and position, and says how
//! many survived; Stage 4's row records the count. Without the bound, a name defined
//! thousands of times and called on a receiver no stage can place held every definition
//! at every site: quadratic in time, memory and the segment (8,000 modules: 222 s,
//! 8.5 GB, a 1.95 GB segment; with it 3.9 s, 0.9 GB, 96 MB).
//!
//! One real Python checkout, written to disk and resolved through discovery, extraction,
//! typed resolution and Stage 3, then built to a segment and read back.

use std::fs;
use std::path::Path;

use pdx_core::bands::{Band, CandidateBand};
use pdx_core::config::PdxConfig;
use pdx_core::consts::MAX_STORED_CANDIDATES as MAX_CANDIDATES;
use pdx_core::ids::RepoId;
use pdx_core::index::discover::discover;
use pdx_core::index::extract::{ExtractLimits, ExtractStage};
use pdx_core::index::{IndexRequest, build_segment};
use pdx_core::model::CandidateSite;
use pdx_core::resolve::registry::{DefinitionRef, SymbolRegistry};
use pdx_core::resolve::stages::{self, ResolveReport, resolve};
use pdx_core::segment::{RepoIdSource, SegmentReader};

/// How many modules define the method: more than the bound.
const NAMESAKES: usize = 40;

const LIMITS: ExtractLimits = ExtractLimits {
    requested_workers: 2,
    memory_budget_bytes: 64 << 20,
};

struct Fixture {
    _dir: tempfile::TempDir,
    _out: tempfile::TempDir,
    report: ResolveReport,
    candidates: Vec<CandidateSite>,
}

/// `pkg/m00.py` to `pkg/m39.py`, each a class with `reconcile_item`, and `drive.py`,
/// which calls it on a value nothing types.
fn files() -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = (0..NAMESAKES)
        .map(|i| {
            (
                format!("pkg/m{i:02}.py"),
                format!(
                    "class Handler{i:02}:\n    def reconcile_item(self, item):\n        return item\n"
                ),
            )
        })
        .collect();
    files.push(("pkg/__init__.py".to_owned(), String::new()));
    files.push((
        "drive.py".to_owned(),
        "def drive_batch(x):\n    return x.reconcile_item(x)\n".to_owned(),
    ));
    files
}

fn fixture() -> &'static Fixture {
    static FIXTURE: std::sync::OnceLock<Fixture> = std::sync::OnceLock::new();
    FIXTURE.get_or_init(|| {
        let dir = tempfile::Builder::new()
            .prefix("pdx-resolution-scale-test-")
            .tempdir()
            .expect("a directory");
        for (path, content) in files() {
            let path = dir.path().join(path);
            fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
            fs::write(&path, content).expect("a file");
        }
        let root: &Path = dir.path();
        let config = PdxConfig::load(root).expect("the configuration loads");
        let found = discover(root, &config).expect("discovery succeeds");
        let extracted = ExtractStage::new(root, &config.secrets, LIMITS)
            .run(&found)
            .expect("extraction succeeds");
        let reg = SymbolRegistry::build(root, &found, extracted).expect("the registry builds");
        let report = resolve(root, &reg).expect("resolution succeeds");
        let out = tempfile::tempdir().expect("an output directory");
        let destination = out.path().join("segment.db");
        build_segment(IndexRequest {
            root,
            repo_id: RepoId::parse("ce63551447285fd4").expect("a repo id"),
            repo_id_source: RepoIdSource::Registered,
            repo_url: "https://git.example/acme/fixture.git".to_owned(),
            repo_name: "fixture".to_owned(),
            commit_sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            limits: LIMITS,
            cache: None,
            backend: None,
            destination: destination.clone(),
            build_dir: None,
            progress: None,
        })
        .expect("the build succeeds");
        let segment = SegmentReader::open(&destination).expect("the segment opens");
        let candidates = segment
            .nodes_all()
            .expect("nodes")
            .iter()
            .flat_map(|n| segment.candidates_from(&n.node_id).expect("candidates"))
            .collect();
        Fixture {
            _dir: dir,
            _out: out,
            report,
            candidates,
        }
    })
}

#[test]
fn a_candidate_site_holds_at_most_the_bound_and_counts_the_rest() {
    let fixture = fixture();
    let site = fixture
        .report
        .resolutions
        .iter()
        .find(|r| r.site_ref.rel_path == "drive.py")
        .expect("the call in drive.py");
    let resolution = &site.resolution;
    assert_eq!(resolution.band(), Band::Candidate);
    assert_eq!(resolution.candidates_total(), NAMESAKES);
    assert_eq!(resolution.candidates().len(), MAX_CANDIDATES);
    // The first by path and position: the bound keeps the start of the sorted set.
    let expected: Vec<String> = (0..MAX_CANDIDATES)
        .map(|i| format!("pkg/m{i:02}.py"))
        .collect();
    let kept: Vec<&str> = resolution
        .candidates()
        .iter()
        .map(|c: &DefinitionRef| c.path.as_str())
        .collect();
    assert_eq!(kept, expected);
}

#[test]
fn the_row_says_how_many_survived() {
    let rows: Vec<&CandidateSite> = fixture()
        .candidates
        .iter()
        .filter(|c| c.callee_name.ends_with("reconcile_item"))
        .collect();
    let [row] = rows.as_slice() else {
        panic!("one row for the call: {rows:?}");
    };
    assert_eq!(row.band, CandidateBand::Candidate);
    assert_eq!(row.candidate_ids.len(), MAX_CANDIDATES);
    assert_eq!(
        row.reason,
        format!(
            "several definitions survive every stage: {NAMESAKES}, the first {MAX_CANDIDATES} \
             by path and position kept"
        )
    );
}

#[test]
fn a_set_within_the_bound_is_held_whole() {
    let mut candidates: Vec<DefinitionRef> = (0..3u32)
        .map(|index| DefinitionRef {
            path: "a.py".into(),
            index,
        })
        .collect();
    let whole = stages::Resolution::bounded_candidates(candidates.clone()).expect("a resolution");
    assert_eq!(whole.candidates(), candidates.as_slice());
    assert_eq!(whole.candidates_total(), 3);
    // Past the bound, the first are kept and the count is all of them.
    candidates = (0..u32::try_from(MAX_CANDIDATES + 5).expect("small"))
        .map(|index| DefinitionRef {
            path: "a.py".into(),
            index,
        })
        .collect();
    let bounded = stages::Resolution::bounded_candidates(candidates.clone()).expect("a resolution");
    assert_eq!(bounded.candidates(), &candidates[..MAX_CANDIDATES]);
    assert_eq!(bounded.candidates_total(), MAX_CANDIDATES + 5);
}
