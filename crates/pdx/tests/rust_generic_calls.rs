//! Issue 107A end to end: a Rust call whose function carries a turbofish (`f::<T>()`,
//! `x.m::<T>()`) is a call site of the graph `pdx index` builds, resolved and drawn as
//! the same call without its type arguments would be.
//!
//! One crate in a real Git repository, indexed by the built binary: discovery,
//! extraction into an empty cache, typed resolution, Stage 3, the segment; indexed again
//! from the warm cache; then read back through the segment's queries (a name searched, its
//! edges, candidates and sites). The same crate with every call's own turbofish replaced
//! by spaces of its length is indexed beside it: its graph must be the same.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use pdx_core::bands::Band;
use pdx_core::ids::{NodeId, SiteId};
use pdx_core::model::Node;
use pdx_core::segment::SegmentReader;

const ORIGIN: &str = "https://git.example/acme/turbo.git";

const FILES: [(&str, &str); 7] = [
    (".gitignore", ".pdx/\n"),
    (
        "Cargo.toml",
        "[package]\nname = \"turbo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    ),
    (
        "src/lib.rs",
        "pub mod app;\npub mod codec;\npub mod pipeline;\npub mod store;\n",
    ),
    (
        "src/codec.rs",
        "pub fn decode_as<T: Default>(raw: u8) -> T {\n    let _ = raw;\n    T::default()\n}\n\npub fn plain_len(raw: &[u8]) -> usize {\n    raw.len()\n}\n\npub fn local_pick<T>() -> u32 {\n    0\n}\n",
    ),
    (
        "src/store.rs",
        r"pub struct Store {
    pub slots: u8,
}

impl Store {
    pub fn open_with<T>(slots: u8) -> Store {
        Store { slots }
    }

    pub fn fetch_as<T: Default>(&self) -> T {
        T::default()
    }

    pub fn widen_into<A, B: Default>(&self, _a: A) -> B {
        B::default()
    }

    pub fn probe_once<T>(&self) -> u8 {
        self.slots
    }
}

pub trait Convert {
    fn convert_to<T: Default>(&self) -> T;
}

impl Convert for Store {
    fn convert_to<T: Default>(&self) -> T {
        self.fetch_as::<T>()
    }
}

pub struct Grid<C>(pub C);

impl<C> Grid<C> {
    pub fn sized<D: Default>() -> D {
        D::default()
    }
}
",
    ),
    (
        "src/pipeline.rs",
        "pub struct Stage(pub u32);\n\npub fn start_at<T>() -> Stage {\n    Stage(0)\n}\n\nimpl Stage {\n    pub fn widen_by<T>(self) -> Stage {\n        Stage(self.0 + 1)\n    }\n\n    pub fn settle_on<T>(self) -> u32 {\n        self.0\n    }\n\n    pub fn step(self, n: u32) -> Stage {\n        Stage(self.0 + n)\n    }\n\n    pub fn finish(self) -> u32 {\n        self.0\n    }\n}\n",
    ),
    (
        "src/app.rs",
        r"use crate::codec;
use crate::codec::decode_as;
use crate::pipeline::{start_at, Stage};
use crate::store::{Convert, Grid, Store};

macro_rules! wrap {
    ($e:expr) => {
        $e
    };
}

pub fn phantom_target<T>() -> u8 {
    0
}

pub fn run(raw: u8) -> u32 {
    let ordinary = codec::plain_len(&[raw]);
    let a: u16 = decode_as::<u16>(raw);
    let b: u32 = codec::decode_as::<u32>(raw);
    let store = Store::open_with::<u8>(4);
    let c: u8 = store.fetch_as::<u8>();
    let d: u64 = store.widen_into::<u8, u64>(raw);
    let e: u8 = store.convert_to::<u8>();
    let f: u8 = Grid::<u8>::sized::<u8>();
    let g = start_at::<u8>().widen_by::<u16>().settle_on::<u32>();
    let h = Stage(1)
        // <- inner stage
        .step(2)
        /* outer stage */
        .finish();
    let i = wrap!(phantom_target::<u8>());
    fn local_pick<T>() -> u32 {
        7
    }
    let j = local_pick::<u8>();
    let _value = decode_as::<u8>;
    ordinary as u32 + a as u32 + b + c as u32 + d as u32 + e as u32 + f as u32 + g + h + i as u32 + j
}

pub fn on_unknown<X: Copy>(x: X) -> u8 {
    x.probe_once::<u8>()
}

pub fn with_store(s: &Store) -> u8 {
    s.fetch_as::<u8>() + s.convert_to::<u8>() + <Store as Convert>::convert_to::<u8>(s)
}

pub fn wrapped() -> Grid<u8> {
    Grid::<u8>(1)
}

pub fn quiet() -> Stage {
    Stage(3)
        // .step(4)
}
",
    ),
];

/// `source` with each call's own turbofish (`::<…>` right before the call's `(`) replaced
/// by spaces of its length.
fn without_turbofish(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = bytes.to_vec();
    let mut i = 0;
    while let Some(found) = source[i..].find("::<") {
        let start = i + found;
        let mut end = start + 3;
        let mut depth = 1;
        while depth > 0 {
            match bytes[end] {
                b'<' => depth += 1,
                b'>' => depth -= 1,
                _ => {}
            }
            end += 1;
        }
        if bytes.get(end) == Some(&b'(') {
            out[start..end].fill(b' ');
        }
        i = end;
    }
    String::from_utf8(out).expect("UTF-8")
}

/// A Git repository of `files`, committed once with the user's and the system's
/// configuration shut out, and a home for the binary's caches.
struct Repo {
    root: tempfile::TempDir,
    home: tempfile::TempDir,
}

impl Repo {
    fn new(files: &[(&str, String)]) -> Self {
        let repo = Self {
            root: tempfile::Builder::new()
                .prefix("pdx-generic-calls-")
                .tempdir()
                .expect("a repository"),
            home: tempfile::tempdir().expect("a home"),
        };
        fs::write(repo.home.path().join("gitconfig"), "").expect("an empty configuration");
        for (path, content) in files {
            let path = repo.root.path().join(path);
            fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
            fs::write(path, content).expect("a file");
        }
        for args in [
            &["init", "-q"][..],
            &["config", "core.autocrlf", "false"],
            &["config", "commit.gpgsign", "false"],
            &["add", "-A"],
            &["commit", "-q", "--no-verify", "-m", "fixture"],
            &["remote", "add", "origin", ORIGIN],
        ] {
            let status = repo
                .isolated(&mut Command::new("git"))
                .current_dir(repo.root.path())
                .args(args)
                .status()
                .expect("git runs");
            assert!(status.success(), "git {args:?}");
        }
        repo
    }

    fn isolated<'c>(&self, command: &'c mut Command) -> &'c mut Command {
        let home = self.home.path();
        command
            .env("GIT_CONFIG_GLOBAL", home.join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Fixture Author")
            .env("GIT_AUTHOR_EMAIL", "author@git.example")
            .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00+00:00")
            .env("GIT_COMMITTER_NAME", "Fixture Committer")
            .env("GIT_COMMITTER_EMAIL", "committer@git.example")
            .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00+00:00")
            .env("HOME", home)
            .env("XDG_CACHE_HOME", self.cache())
            .env("LOCALAPPDATA", home.join("local"))
            .env_remove("PDX_WORKERS")
            .env_remove("PDX_MEM_BUDGET_MB")
            .env_remove("PDX_ENGINE_ISOLATE")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
    }

    fn cache(&self) -> PathBuf {
        self.home.path().join("cache")
    }

    /// `pdx index` into `out` (below the repository), ready; the segment's path.
    fn index(&self, out: &str) -> PathBuf {
        let root = self.root.path().to_str().expect("a UTF-8 path");
        let segment = self.root.path().join(out);
        fs::create_dir_all(segment.parent().expect("a parent")).expect("the ignored .pdx/");
        let output = self
            .isolated(&mut Command::new(env!("CARGO_BIN_EXE_pdx")))
            .current_dir(self.home.path())
            .args(["index", root, "--out", out, "--json"])
            .output()
            .expect("pdx runs");
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        segment
    }
}

fn files_in(dir: &Path) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .map(|e| e.expect("an entry").path())
        .map(|p| if p.is_dir() { files_in(&p) } else { 1 })
        .sum()
}

/// One call site as the graph serves it: where it is, what it calls, and what became of
/// it, an edge drawn (`kind band -> target`) or a candidate row (`band: reason [targets]`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Served {
    caller: String,
    line: u32,
    span: (u32, u32),
    callee: String,
    receiver: Option<String>,
    outcome: String,
}

/// Every call site of the segment, read from its queries: each node's edges and
/// candidate rows, and each one's site.
fn served(path: &Path) -> (Vec<Node>, Vec<Served>) {
    let segment = SegmentReader::open(path).expect("the segment opens");
    let nodes = segment.nodes_all().expect("nodes");
    let qn = |id: &NodeId| {
        segment
            .node(id)
            .expect("a node")
            .expect("the node exists")
            .qualified_name
    };
    let mut out = Vec::new();
    for node in &nodes {
        let site = |id: &SiteId| segment.site(id).expect("a site").expect("the site exists");
        for edge in segment.edges_from(&node.node_id, Band::ALL).expect("edges") {
            let Some(id) = &edge.site_id else { continue };
            let s = site(id);
            if edge.kind.as_str() != "CALLS" {
                continue;
            }
            out.push(Served {
                caller: node.qualified_name.clone(),
                line: s.span.start_line,
                span: (s.span.start_byte, s.span.end_byte),
                callee: s.callee_text.clone().unwrap_or_default(),
                receiver: s.receiver_text.clone(),
                outcome: format!(
                    "{} {} -> {}",
                    edge.kind.as_str(),
                    edge.band.as_str(),
                    qn(&edge.dst)
                ),
            });
        }
        for row in segment.candidates_from(&node.node_id).expect("candidates") {
            let s = site(&row.site_id);
            let targets: Vec<String> = row.candidate_ids.iter().map(qn).collect();
            let reason = row.reason.split(':').next().unwrap_or_default().to_owned();
            out.push(Served {
                caller: node.qualified_name.clone(),
                line: s.span.start_line,
                span: (s.span.start_byte, s.span.end_byte),
                callee: s.callee_text.clone().unwrap_or_default(),
                receiver: s.receiver_text.clone(),
                outcome: format!("{} {reason} {targets:?}", row.band.as_str()),
            });
        }
    }
    out.sort();
    (nodes, out)
}

/// The sites `caller` (a qualified name) makes, as `(line, callee, outcome)`.
fn of<'s>(all: &'s [Served], caller: &str) -> Vec<(u32, &'s str, &'s str)> {
    let mut found: Vec<(u32, &str, &str)> = all
        .iter()
        .filter(|s| s.caller == caller)
        .map(|s| (s.line, s.callee.as_str(), s.outcome.as_str()))
        .collect();
    found.sort_unstable();
    found
}

/// What `src.app.run`'s sites must be served as, in line order.
fn expected_run() -> Vec<(u32, &'static str, String)> {
    let typed = "CALLS typed -> ";
    let import = "CALLS import-guided -> ";
    let exact = |target: &str| format!("candidate exact [\"{target}\"]");
    let unresolved = "unresolved no definition the name can reach []".to_owned();
    vec![
        (17, "plain_len", format!("{typed}src.codec.plain_len")),
        (18, "decode_as", format!("{typed}src.codec.decode_as")),
        (19, "decode_as", format!("{typed}src.codec.decode_as")),
        (20, "open_with", format!("{import}src.store.Store.open_with")),
        (21, "fetch_as", exact("src.store.Store.fetch_as")),
        (22, "widen_into", exact("src.store.Store.widen_into")),
        (
            23,
            "convert_to",
            "candidate several definitions survive every stage [\"src.store.Convert.convert_to\", \"src.store.Store.convert_to\"]".to_owned(),
        ),
        (24, "sized", exact("src.store.Grid.sized")),
        (25, "settle_on", exact("src.pipeline.Stage.settle_on")),
        (25, "start_at", format!("{typed}src.pipeline.start_at")),
        (25, "widen_by", exact("src.pipeline.Stage.widen_by")),
        (26, "Stage", format!("{import}src.pipeline.Stage")),
        (26, "finish", exact("src.pipeline.Stage.finish")),
        (26, "step", exact("src.pipeline.Stage.step")),
        (31, "wrap", unresolved.clone()),
        (35, "local_pick", unresolved),
    ]
}

/// The fixture with each Rust file's call turbofishes blanked.
fn twin_files() -> Vec<(&'static str, String)> {
    FILES
        .iter()
        .map(|(p, c)| {
            let rust = Path::new(p).extension().is_some_and(|e| e == "rs");
            (
                *p,
                if rust {
                    without_turbofish(c)
                } else {
                    (*c).to_owned()
                },
            )
        })
        .collect()
}

#[test]
fn turbofish_calls_are_sites_of_the_graph_pdx_index_builds() {
    let files: Vec<(&str, String)> = FILES.iter().map(|(p, c)| (*p, (*c).to_owned())).collect();
    let repo = Repo::new(&files);

    // Cold, into an empty cache, then warm from it: the same segment, byte for byte.
    let cold = repo.index(".pdx/cold.db");
    let cached = files_in(&repo.cache());
    assert!(cached >= 5, "{cached} extractions cached");
    let warm = repo.index(".pdx/warm.db");
    assert_eq!(
        files_in(&repo.cache()),
        cached,
        "the warm build added cache entries"
    );
    assert_eq!(
        fs::read(&cold).expect("the cold segment"),
        fs::read(&warm).expect("the warm segment")
    );

    // Read back: the caller found by name, its sites by its edges and candidate rows.
    let segment = SegmentReader::open(&warm).expect("the segment opens");
    let hits = segment.search("run", 10).expect("a search");
    assert!(
        hits.iter().any(|h| h.node.qualified_name == "src.app.run"),
        "`run` is not found"
    );
    let (nodes, all) = served(&warm);
    let exact = |target: &str| format!("candidate exact [\"{target}\"]");
    let run = of(&all, "src.app.run");
    let expected = expected_run();
    let expected: Vec<(u32, &str, &str)> = expected
        .iter()
        .map(|(l, c, o)| (*l, *c, o.as_str()))
        .collect();
    assert_eq!(run, expected);
    assert_eq!(
        of(&all, "src.store.Store.convert_to"),
        [(29, "fetch_as", "CALLS typed -> src.store.Store.fetch_as")]
    );
    assert_eq!(
        of(&all, "src.app.wrapped"),
        [(49, "Grid", "CALLS import-guided -> src.store.Grid")]
    );
    assert_eq!(
        of(&all, "src.app.on_unknown"),
        [(
            41,
            "probe_once",
            exact("src.store.Store.probe_once").as_str()
        )]
    );
    // A13: a comment holding `.step(4)` is no call.
    assert_eq!(
        of(&all, "src.app.quiet"),
        [(53, "Stage", "CALLS import-guided -> src.pipeline.Stage")]
    );
    // The call in the macro's tokens is no site (issue 107B), though its target exists;
    // the local function's call is drawn to no namesake.
    assert!(
        nodes
            .iter()
            .any(|n| n.qualified_name == "src.app.phantom_target")
    );
    assert!(
        !all.iter()
            .any(|s| s.callee.contains("phantom") || s.outcome.contains("phantom"))
    );
    assert!(
        !all.iter()
            .any(|s| s.outcome.contains("src.codec.local_pick"))
    );
    // No site twice: one edge or one candidate row each.
    let spans: BTreeSet<(&str, (u32, u32))> =
        all.iter().map(|s| (s.caller.as_str(), s.span)).collect();
    assert_eq!(spans.len(), all.len(), "a site served twice");

    // The twin: the same graph, every site where it was, served the same way. A chain's
    // receiver spells the turbofish of the link before it, and nothing else differs.
    let twin = Repo::new(&twin_files());
    let (twin_nodes, twin_all) = served(&twin.index(".pdx/segment.db"));
    assert_eq!(nodes, twin_nodes);
    let blanked: Vec<Served> = all
        .iter()
        .cloned()
        .map(|mut s| {
            s.receiver = s.receiver.as_deref().map(without_turbofish);
            s
        })
        .collect();
    assert_eq!(blanked, twin_all);
}
