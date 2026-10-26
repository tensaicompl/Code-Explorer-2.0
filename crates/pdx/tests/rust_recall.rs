//! Issue 105 end to end (R5): a Rust call is drawn only on positive evidence of its
//! target, and that evidence is read as Rust reads it. A path is anchored where Rust
//! anchors it (`crate`, `self`, `super`, a crate's name, a `pub use`, a module file named
//! `index.rs`); a receiver is typed from what proves its type (an annotation, a
//! constructor, a parameter, a field, a declared return type, `async` and `?` as they
//! are); an impl's method is the target only where the impl is proven to apply. What has
//! no such evidence (a generic bound, a type a macro writes, a name two types share, a
//! value nothing types) is never drawn, and stays the weak evidence it was.
//!
//! One crate in a real Git repository, indexed by the built binary: discovery,
//! extraction into an empty cache, typed resolution, Stage 3, the segment; indexed again
//! from the warm cache, byte for byte the same; then read back through the segment's
//! queries (a name searched, its edges, candidate rows and sites).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use pdx_core::bands::Band;
use pdx_core::ids::{NodeId, SiteId};
use pdx_core::segment::SegmentReader;

const ORIGIN: &str = "https://git.example/acme/recall.git";

const FILES: [(&str, &str); 11] = [
    (".gitignore", ".pdx/\n"),
    (
        "Cargo.toml",
        "[package]\nname = \"recall\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    ),
    (
        "src/lib.rs",
        "pub mod app;\npub mod catalog;\npub mod facade;\npub mod gears;\npub mod made;\npub mod reader;\n",
    ),
    // A module file and its child module file named `index.rs`.
    (
        "src/catalog/mod.rs",
        "pub mod index;\n\npub fn count() -> u32 {\n    self::index::lookup(2)\n}\n",
    ),
    (
        "src/catalog/index.rs",
        "pub fn lookup(key: u32) -> u32 {\n    key\n}\n",
    ),
    (
        "src/gears/mod.rs",
        r"pub mod deep;

pub struct Gear {
    pub teeth: u32,
}

impl Gear {
    pub fn new(teeth: u32) -> Gear {
        Gear { teeth }
    }

    pub fn spin(&self) -> u32 {
        self.teeth
    }

    pub fn twin(&self) -> Gear {
        Gear::new(self.teeth)
    }

    pub async fn later(&self) -> Gear {
        Gear::new(self.teeth)
    }

    pub fn checked(&self) -> Result<Gear, u32> {
        Ok(Gear::new(self.teeth))
    }
}

pub trait Turn {
    fn turn(&self) -> u32;
}

impl Turn for Gear {
    fn turn(&self) -> u32 {
        self.spin()
    }
}

pub struct Guard(pub Gear);

impl std::ops::Deref for Guard {
    type Target = Gear;

    fn deref(&self) -> &Gear {
        &self.0
    }
}

pub fn helper() -> u32 {
    super::catalog::index::lookup(1)
}
",
    ),
    (
        "src/gears/deep.rs",
        "pub struct Cog(pub u32);\n\nimpl Cog {\n    pub fn mesh(&self) -> u32 {\n        self.0\n    }\n}\n\npub fn forge() -> Cog {\n    Cog(1)\n}\n",
    ),
    // A re-export: `facade::Cog` and `facade::forge` are `gears::deep`'s.
    (
        "src/facade.rs",
        "pub use crate::gears::deep::forge;\npub use crate::gears::deep::Cog;\n",
    ),
    // Two types with a method of one name, and a trait a generic bound names.
    (
        "src/reader.rs",
        r"pub struct Reader(pub u32);

pub struct Pager(pub u32);

pub trait Chunks {
    fn chunk_text(&self) -> u32;
}

impl Chunks for Reader {
    fn chunk_text(&self) -> u32 {
        self.0
    }
}

impl Reader {
    pub fn page(&self) -> u32 {
        self.0
    }

    pub fn rewind(&self) -> u32 {
        0
    }
}

impl Pager {
    pub fn page(&self) -> u32 {
        self.0
    }
}
",
    ),
    // A type a macro writes, and a macro whose tokens read like calls.
    (
        "src/made.rs",
        r"macro_rules! make_meter {
    ($name:ident) => {
        pub struct $name(pub u32);

        impl $name {
            pub fn read(&self) -> u32 {
                self.0
            }
        }
    };
}

macro_rules! route_table {
    ($($method:ident => $handler:expr),* $(,)?) => {
        0
    };
}

make_meter!(Meter);

pub fn reading(m: &Meter) -> u32 {
    m.read()
}

pub fn table() -> u32 {
    route_table! {
        get => crate::gears::helper(),
        post => crate::catalog::count(),
    }
}
",
    ),
    (
        "src/app.rs",
        r"use std::io;

use crate::facade::Cog;
use crate::gears;
use crate::gears::{Gear, Guard, Turn};
use crate::reader::Chunks;

pub struct Rig {
    pub main: Gear,
}

impl Rig {
    pub fn drive(&self) -> u32 {
        self.main.spin()
    }
}

pub fn local_typed() -> u32 {
    let g: Gear = Gear::new(3);
    g.spin()
}

pub fn constructed() -> u32 {
    let g = Gear::new(4);
    g.twin().spin()
}

pub fn param(g: &Gear) -> u32 {
    g.turn()
}

pub fn bounded<T: Turn>(t: &T) -> u32 {
    t.turn()
}

pub fn wrapped(guard: &Guard) -> u32 {
    guard.spin()
}

pub async fn awaited(g: &Gear) -> u32 {
    g.later().await.spin()
}

pub fn tried(g: &Gear) -> Result<u32, u32> {
    Ok(g.checked()?.spin())
}

pub fn through_reexport() -> u32 {
    let c = crate::facade::forge();
    c.mesh() + Cog(2).mesh()
}

pub fn paths() -> u32 {
    crate::gears::helper() + crate::catalog::count() + gears::helper()
}

pub fn indexed() -> u32 {
    crate::catalog::index::lookup(7)
}

pub fn shadowed<R: Chunks>(io: R) -> u32 {
    let _ = io::stdout();
    io.chunk_text()
}

pub fn untyped<V>(v: V) -> u32 {
    v.page() + v.rewind()
}
",
    ),
];

/// A Git repository of `files`, committed once with the user's and the system's
/// configuration shut out, and a home for the binary's caches.
struct Repo {
    root: tempfile::TempDir,
    home: tempfile::TempDir,
}

impl Repo {
    fn new(files: &[(&str, &str)]) -> Self {
        let repo = Self {
            root: tempfile::Builder::new()
                .prefix("pdx-rust-recall-")
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
            .env("XDG_CACHE_HOME", home.join("cache"))
            .env("LOCALAPPDATA", home.join("local"))
            .env_remove("PDX_WORKERS")
            .env_remove("PDX_MEM_BUDGET_MB")
            .env_remove("PDX_ENGINE_ISOLATE")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
    }

    /// Where the binary keeps its extraction cache under this home: XDG's on Linux,
    /// `Library/Caches` on macOS, `LOCALAPPDATA` on Windows.
    fn cache(&self) -> PathBuf {
        let home = self.home.path();
        if cfg!(windows) {
            home.join("local").join("pdx").join("cache")
        } else if cfg!(target_os = "macos") {
            home.join("Library").join("Caches").join("pdx")
        } else {
            home.join("cache").join("pdx")
        }
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

/// One call site as the graph serves it: who makes it, where, what it calls, and what
/// became of it, an edge drawn (`kind band -> target`) or a candidate row (`band:
/// reason [targets]`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Served {
    caller: String,
    line: u32,
    span: (u32, u32),
    callee: String,
    outcome: String,
}

/// Every call site of the segment, read from its queries: each node's edges and
/// candidate rows, and each one's site.
fn served(path: &Path) -> Vec<Served> {
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
            if edge.kind.as_str() != "CALLS" {
                continue;
            }
            let s = site(id);
            out.push(Served {
                caller: node.qualified_name.clone(),
                line: s.span.start_line,
                span: (s.span.start_byte, s.span.end_byte),
                callee: s.callee_text.clone().unwrap_or_default(),
                outcome: format!(
                    "{} -> {}",
                    edge.band.map_or("declared", |b| b.as_str()),
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
                outcome: format!("{} {reason} {targets:?}", row.band.as_str()),
            });
        }
    }
    out.sort();
    out
}

/// The sites `caller` (a qualified name) makes, as `(callee, outcome)` in source order.
fn of<'s>(all: &'s [Served], caller: &str) -> Vec<(&'s str, &'s str)> {
    all.iter()
        .filter(|s| s.caller == caller)
        .map(|s| (s.callee.as_str(), s.outcome.as_str()))
        .collect()
}

const TYPED: &str = "typed -> ";

fn typed(target: &str) -> String {
    format!("{TYPED}{target}")
}

#[test]
fn rust_calls_are_drawn_on_proven_evidence_only() {
    let repo = Repo::new(&FILES);

    // Cold, into an empty cache, then warm from it: the same segment, byte for byte.
    let cold = repo.index(".pdx/cold.db");
    let cached = files_in(&repo.cache());
    assert!(cached >= 9, "{cached} extractions cached");
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

    let segment = SegmentReader::open(&warm).expect("the segment opens");
    let hits = segment.search("through_reexport", 10).expect("a search");
    assert!(
        hits.iter()
            .any(|h| h.node.qualified_name == "src.app.through_reexport"),
        "`through_reexport` is not found"
    );
    let all = served(&warm);
    proven_receivers(&all);
    paths_as_rust_reads_them(&all);
    no_evidence_no_edge(&all);
}

/// Receivers typed from what proves their type.
fn proven_receivers(all: &[Served]) {
    let spin = typed("src.gears.mod.Gear.spin");
    let new = typed("src.gears.mod.Gear.new");

    // A field's type, an annotation, a constructor, a declared return type.
    assert_eq!(of(all, "src.app.Rig.drive"), [("spin", spin.as_str())]);
    assert_eq!(
        of(all, "src.app.local_typed"),
        [("new", new.as_str()), ("spin", spin.as_str())]
    );
    assert_eq!(
        of(all, "src.app.constructed"),
        [
            ("new", new.as_str()),
            ("twin", typed("src.gears.mod.Gear.twin").as_str()),
            ("spin", spin.as_str()),
        ]
    );
    // A parameter's type, and a trait impl's method whose impl requires nothing.
    assert_eq!(
        of(all, "src.app.param"),
        [("turn", typed("src.gears.mod.Gear.turn").as_str())]
    );
    // A project `Deref` impl's target is not recorded by extraction: the call through
    // the wrapper stays the weak evidence of the method's one definition, never drawn.
    assert_eq!(
        of(all, "src.app.wrapped"),
        [("spin", "candidate exact [\"src.gears.mod.Gear.spin\"]")]
    );
    // `async` wraps the declared return type in a future `.await` unwraps; `?` unwraps a
    // `Result`.
    assert_eq!(
        of(all, "src.app.awaited"),
        [
            ("later", typed("src.gears.mod.Gear.later").as_str()),
            ("spin", spin.as_str()),
        ]
    );
    let tried = of(all, "src.app.tried");
    assert!(
        tried.contains(&("checked", typed("src.gears.mod.Gear.checked").as_str())),
        "{tried:?}"
    );
    assert!(tried.contains(&("spin", spin.as_str())), "{tried:?}");
}

/// Paths read as Rust reads them.
fn paths_as_rust_reads_them(all: &[Served]) {
    // Paths read as Rust reads them: through a `pub use`, `crate`, `self`, `super`, and
    // a module file named `index.rs`, which is its own module.
    assert_eq!(
        of(all, "src.app.through_reexport"),
        [
            ("forge", typed("src.gears.deep.forge").as_str()),
            ("mesh", typed("src.gears.deep.Cog.mesh").as_str()),
            // A tuple struct's constructor: Stage 3 does not follow the `pub use`, and
            // the type's one definition of the name is weak evidence.
            ("Cog", "candidate exact [\"src.gears.deep.Cog\"]"),
            ("mesh", typed("src.gears.deep.Cog.mesh").as_str()),
        ]
    );
    let helper = typed("src.gears.mod.helper");
    assert_eq!(
        of(all, "src.app.paths"),
        [
            ("helper", helper.as_str()),
            ("count", typed("src.catalog.mod.count").as_str()),
            ("helper", helper.as_str()),
        ]
    );
    let lookup = typed("src.catalog.lookup");
    assert_eq!(of(all, "src.app.indexed"), [("lookup", lookup.as_str())]);
    assert_eq!(
        of(all, "src.catalog.mod.count"),
        [("lookup", lookup.as_str())]
    );
    assert_eq!(
        of(all, "src.gears.mod.helper"),
        [("lookup", lookup.as_str())]
    );
}

/// What has no evidence is never drawn, and stays the weak evidence it was.
fn no_evidence_no_edge(all: &[Served]) {
    // No evidence, no edge. A generic bound: the trait's method and the impl's both
    // survive. A value nothing types: the one method of the name stays weak, two stay a
    // candidate. A type a macro writes: no definition, no target.
    assert_eq!(
        of(all, "src.app.bounded"),
        [(
            "turn",
            "candidate several definitions survive every stage [\"src.gears.mod.Gear.turn\", \"src.gears.mod.Turn.turn\"]"
        )]
    );
    assert_eq!(
        of(all, "src.app.untyped"),
        [
            (
                "page",
                "candidate several definitions survive every stage [\"src.reader.Pager.page\", \"src.reader.Reader.page\"]"
            ),
            ("rewind", "candidate exact [\"src.reader.Reader.rewind\"]"),
        ]
    );
    let reading = of(all, "src.made.reading");
    assert!(
        reading.iter().all(|(_, o)| !o.starts_with(TYPED)),
        "{reading:?}"
    );
    // A local binding shadows the external import of its name (issue 116): the method
    // called on it is the repository's, never `external`; the import itself still is.
    let shadowed = of(all, "src.app.shadowed");
    assert!(
        shadowed.contains(&("stdout", "external the target is outside the repository []")),
        "{shadowed:?}"
    );
    let chunk = shadowed
        .iter()
        .find(|(c, _)| *c == "chunk_text")
        .expect("the method call is a site");
    assert!(
        chunk.1.starts_with("candidate") && chunk.1.contains("src.reader.Reader.chunk_text"),
        "{chunk:?}"
    );

    // A macro's tokens are no calls (issue 107B): the paths they spell are not sites;
    // the invocation is, and names nothing the repository defines.
    assert_eq!(
        of(all, "src.made.table"),
        [(
            "route_table",
            "unresolved no definition the name can reach []"
        )]
    );

    // Nothing drawn but by a drawn band, no site twice.
    for s in all {
        if !s.outcome.contains("->") {
            continue;
        }
        let band = s.outcome.split(' ').next().unwrap_or_default();
        assert!(
            ["typed", "import-guided", "inheritance-guided"].contains(&band),
            "{s:?}"
        );
    }
    let spans: BTreeSet<(&str, (u32, u32))> =
        all.iter().map(|s| (s.caller.as_str(), s.span)).collect();
    assert_eq!(spans.len(), all.len(), "a site served twice");
}
