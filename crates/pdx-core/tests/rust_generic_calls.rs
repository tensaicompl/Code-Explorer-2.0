//! Issue 107A: a Rust call whose function carries a turbofish is a call of that function.
//! In the pinned grammar such a call's `function` is a `generic_function` whose own
//! `function` field is the path (`f::<T>()`, `m::f::<T>()`, `T::<A>::f::<B>()`) or the
//! method (`x.m::<T>()`); the engine knew no `generic_function` and dropped the call
//! (patch 0033). The call is now its function's, recorded exactly as the same call
//! without its type arguments, which name no callee.
//!
//! One crate, written twice: as it is, and with every call's own turbofish replaced by
//! spaces of its length, so every other byte stays where it was. Both are discovered and
//! extracted by the engine; A1 to A14 are read from the first, and every call of the first
//! is checked against its twin in the second.

use std::fs;
use std::path::Path;

use pdx_core::config::PdxConfig;
use pdx_core::index::discover::discover;
use pdx_core::index::extract::{ExtractLimits, ExtractStage, FileOutcome};
use pdx_engine::{Call, FileExtract, LexicalFacts, TextRef};

const LIB: &str = "pub mod cases;\npub mod codec;\n";
const CODEC: &str = "pub fn decode<T>(raw: u8) -> u8 {\n    raw\n}\n";
const CASES: &str = r"use crate::codec;

macro_rules! wrap {
    ($e:expr) => {
        $e
    };
}

pub struct Store {
    pub slots: u8,
}

impl Store {
    pub fn fetch<T>(&self) -> u8 {
        self.slots
    }

    pub fn widen<A, B>(&self, _a: A) -> u8 {
        self.slots
    }
}

pub struct Stage(pub u32);

impl Stage {
    pub fn widen_by<T>(self) -> Stage {
        Stage(self.0 + 1)
    }

    pub fn settle_on<T>(self) -> u8 {
        0
    }

    pub fn step(self, n: u32) -> Stage {
        Stage(self.0 + n)
    }

    pub fn finish(self) -> u8 {
        0
    }
}

pub struct Grid<C>(pub C);

impl<C> Grid<C> {
    pub fn sized<D>() -> u8 {
        0
    }
}

pub fn pick<T>() -> u8 {
    0
}

pub fn start<T>() -> Stage {
    Stage(0)
}

pub fn a1() -> u8 {
    pick::<u8>()
}

pub fn a2() -> u8 {
    codec::decode::<u8>(1)
}

pub fn a3() -> u8 {
    Grid::<u8>::sized::<u16>()
}

pub fn a4(store: &Store) -> u8 {
    store.fetch::<u8>()
}

pub fn a5(store: &Store, x: u8) -> u8 {
    store.widen::<u8, Vec<u16>>(x)
}

pub fn a6() -> u8 {
    start::<u8>().widen_by::<u16>().settle_on::<u32>()
}

pub fn a7() -> u8 {
    pick()
}

pub fn a8() {
    let _f = pick::<u8>;
}

pub fn a9() {
    let _m = Store::fetch::<u8>;
}

pub fn a10() -> u8 {
    wrap!(pick::<u8>())
}

pub fn a11() -> u8 {
    Stage(1)
        // <- inner stage
        .step(2)
        .finish()
}

pub fn a12() -> u8 {
    Stage(1)
        /* <- inner stage */
        .step(2)
        .finish()
}

pub fn a13() -> Stage {
    Stage(3)
        // .step(4)
        /* .finish() */
}

pub fn shapes(req: &Request, err: Failure, k: u8) -> u8 {
    let _ = Grid::<u8>(1);
    let _ = <Store as Probe>::probe::<u8>(&k);
    let _ = req.parts.extensions.get::<u8>();
    if let Ok(found) = err.downcast::<u8>() {
        return found;
    }
    for _ in 0..2 {
        pick::<u16>();
    }
    fn inner<T>() -> u8 {
        0
    }
    inner::<u8>()
}
";

/// `source` with each call's own turbofish (`::<…>` right before the call's `(`) replaced
/// by spaces of its length: the same call without its type arguments, at the same bytes.
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

/// The crate written to a checkout, its `src/cases.rs` extracted by the engine.
fn extract(cases: &str) -> FileExtract {
    let dir = tempfile::Builder::new()
        .prefix("pdx-rust-generic-calls-test-")
        .tempdir()
        .expect("a directory");
    for (path, content) in [
        (
            "Cargo.toml",
            "[package]\nname = \"generic\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ),
        ("src/lib.rs", LIB),
        ("src/codec.rs", CODEC),
        ("src/cases.rs", cases),
    ] {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
        fs::write(&path, content).expect("a file");
    }
    let root: &Path = dir.path();
    let config = PdxConfig::load(root).expect("the configuration loads");
    let files = discover(root, &config).expect("discovery succeeds");
    let limits = ExtractLimits {
        requested_workers: 1,
        memory_budget_bytes: 64 << 20,
    };
    let report = ExtractStage::new(root, &config.secrets, limits)
        .run(&files)
        .expect("extraction succeeds");
    let file = report
        .files
        .into_iter()
        .find(|f| f.path == "src/cases.rs")
        .expect("src/cases.rs is extracted");
    match file.outcome {
        FileOutcome::Extracted { extract, .. } => *extract,
        other => panic!("src/cases.rs: {other:?}"),
    }
}

/// One call as read: its callee's text and range, receiver, caller's name, site, flags,
/// argument texts, depths and `use` scope.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Read {
    callee: String,
    callee_range: Option<(u32, u32)>,
    receiver: Option<String>,
    caller: Option<String>,
    site: Option<(u32, u32)>,
    is_reference: bool,
    typed_only: bool,
    lexical: u16,
    ast_path: Vec<String>,
    args: Vec<String>,
    use_scope: Option<u32>,
}

fn read(extract: &FileExtract, call: &Call) -> Read {
    let caller = call
        .caller
        .map(|i| extract.record().definitions[i as usize].name.clone());
    Read {
        callee: extract.text(&call.callee_text).into_owned(),
        callee_range: match call.callee_text {
            TextRef::Source { start, end } => Some((start, end)),
            TextRef::Owned(_) => None,
        },
        receiver: call.receiver_text.clone(),
        caller,
        site: call.span.map(|s| (s.start_byte, s.end_byte)),
        is_reference: call.is_reference,
        typed_only: call.typed_only,
        lexical: call.lexical.bits(),
        ast_path: call.ast_path.clone(),
        args: call
            .args
            .iter()
            .map(|a| extract.text(&a.expr).into_owned())
            .collect(),
        use_scope: call.use_scope,
    }
}

fn calls(extract: &FileExtract) -> Vec<Read> {
    extract
        .record()
        .calls
        .iter()
        .map(|c| read(extract, c))
        .collect()
}

/// The calls `caller` makes, as sites (not references, not typed-only questions).
fn sites_of(all: &[Read], caller: &str) -> Vec<Read> {
    let mut found: Vec<Read> = all
        .iter()
        .filter(|c| c.caller.as_deref() == Some(caller) && !c.is_reference && !c.typed_only)
        .cloned()
        .collect();
    found.sort_by_key(|c| c.site);
    found
}

/// The text of `range` in `source`.
fn at(source: &str, range: Option<(u32, u32)>) -> &str {
    let (s, e) = range.expect("a range of the source");
    &source[s as usize..e as usize]
}

#[test]
fn a1_to_a14_a_turbofish_call_is_its_functions_call() {
    let turbo = extract(CASES);
    let all = calls(&turbo);
    let chain = "start::<u8>().widen_by";
    let line = "Stage(1)\n        // <- inner stage\n        .step";
    let block = "Stage(1)\n        /* <- inner stage */\n        .step";
    let expected: &[(&str, &[&str])] = &[
        ("a1", &["pick"]),              // A1: f::<T>()
        ("a2", &["codec::decode"]),     // A2: m::f::<T>()
        ("a3", &["Grid::<u8>::sized"]), // A3: T::<A>::f::<B>()
        ("a4", &["store.fetch"]),       // A4: x.m::<T>()
        ("a5", &["store.widen"]),       // A5: x.m::<A, B>(arg)
        ("a7", &["pick"]),              // A7: the ordinary call
        ("a8", &[]),                    // A8: a generic path, not called
        ("a9", &[]),                    // A9: a generic method, not called
        ("a10", &["wrap"]),             // A10: a macro's tokens are no call
        ("a13", &["Stage"]),            // A13: comments are no call
        // A6: each link of a chain of turbofish calls, once.
        (
            "a6",
            &["start", chain, "start::<u8>().widen_by::<u16>().settle_on"],
        ),
        // A11, A12: a comment before the dot.
        (
            "a11",
            &[
                "Stage",
                line,
                "Stage(1)\n        // <- inner stage\n        .step(2)\n        .finish",
            ],
        ),
        (
            "a12",
            &[
                "Stage",
                block,
                "Stage(1)\n        /* <- inner stage */\n        .step(2)\n        .finish",
            ],
        ),
        // The shapes of the Rust golden's calls that had no site: a tuple struct's
        // constructor, a qualified path, a field chain, a branch, a loop, a local function.
        (
            "shapes",
            &[
                "Grid",
                "<Store as Probe>::probe",
                "req.parts.extensions.get",
                "err.downcast",
                "pick",
                "inner",
            ],
        ),
    ];
    for (caller, callees) in expected {
        let found = sites_of(&all, caller);
        let texts: Vec<&str> = found.iter().map(|c| c.callee.as_str()).collect();
        assert_eq!(&texts, callees, "{caller}");
        for c in &found {
            // The callee is the function's own text, from the call's start to the end of
            // its name: never the type arguments after it (a macro's name is followed by
            // its `!`).
            assert_eq!(at(CASES, c.callee_range), c.callee, "{caller}");
            assert_eq!(c.callee_range.unwrap().0, c.site.unwrap().0, "{caller}");
            let after = &CASES[c.callee_range.unwrap().1 as usize..];
            assert!(
                after.starts_with("::<") || after.starts_with(['(', '!']),
                "{caller}: {} is followed by {:?}",
                c.callee,
                &after[..after.len().min(8)]
            );
        }
    }
    // A5: the arguments are the call's.
    assert_eq!(sites_of(&all, "a5")[0].args, ["x"]);
    // A local function's call stays locally bound through its turbofish (issue 104);
    // the module's function of another name does not.
    let shapes = sites_of(&all, "shapes");
    let bound = LexicalFacts::LOCALLY_BOUND.bits();
    assert_eq!(shapes[5].lexical & bound, bound, "inner::<u8>()");
    assert_eq!(shapes[4].lexical & bound, 0, "pick::<u16>()");
    // A8 to A10: nothing of `pick` there at all, no reference and no typed question.
    for caller in ["a8", "a9", "a10"] {
        assert!(
            !all.iter()
                .any(|c| c.caller.as_deref() == Some(caller) && c.callee.contains("pick")),
            "{caller}"
        );
    }
    // A14: no site twice.
    let mut sites: Vec<(u32, u32)> = all
        .iter()
        .filter(|c| !c.is_reference && !c.typed_only)
        .filter_map(|c| c.site)
        .collect();
    let count = sites.len();
    sites.sort_unstable();
    sites.dedup();
    assert_eq!(sites.len(), count, "a site recorded twice");
}

#[test]
fn a_turbofish_call_reads_as_the_same_call_without_it() {
    // Every call of the crate against its twin's at the same bytes: the same callee
    // (its text the twin's once the turbofish is blanked), caller, site, flags,
    // arguments and scope. A link of a chain differs from its twin only where the source
    // does: its receiver's text spells the turbofish, and its syntax path passes through
    // the `generic_function` that holds the link below it.
    let turbo = calls(&extract(CASES));
    let plain = calls(&extract(&without_turbofish(CASES)));
    assert_eq!(turbo.len(), plain.len());
    for (t, p) in turbo.iter().zip(&plain) {
        let mut t = t.clone();
        t.callee = without_turbofish(&t.callee);
        t.receiver = t.receiver.as_deref().map(without_turbofish);
        t.ast_path.retain(|n| n != "generic_function");
        assert_eq!(&t, p);
    }
}
