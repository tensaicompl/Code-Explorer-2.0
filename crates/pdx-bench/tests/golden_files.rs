//! The committed goldens (plan 5.7, P2-12): one per golden repository of
//! `bench/repos.lock`, each in the canonical v0 format of decision 44. Reads only the
//! committed files: no fetch, no index, no network.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use pdx_bench::golden::GoldenExpected;
use pdx_bench::lock;
use pdx_core::bands::Band;
use pdx_core::kinds::{EdgeKind, NodeKind};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root resolves")
}

/// `<language>/<repo>` for every golden entry of the lock, the repository being the
/// URL's last path segment without a `.git`.
fn lock_keys(root: &Path) -> BTreeSet<String> {
    let repos = lock::bench_repos(&root.join("bench/repos.lock")).expect("repos parse");
    repos
        .golden
        .iter()
        .map(|r| {
            let language = r.language.clone().expect("a golden has a language");
            let last = r.url.rsplit('/').next().expect("a path");
            let repo = last.strip_suffix(".git").unwrap_or(last);
            format!("{language}/{repo}")
        })
        .collect()
}

/// `<language>/<repo>` for every `*.expected.json` under `bench/golden/`.
fn committed_keys(root: &Path) -> BTreeSet<String> {
    let golden = root.join("bench/golden");
    let mut keys = BTreeSet::new();
    let Ok(languages) = std::fs::read_dir(&golden) else {
        return keys;
    };
    for language in languages {
        let language = language.expect("an entry").path();
        for file in std::fs::read_dir(&language).expect("a language directory") {
            let name = file.expect("an entry").file_name();
            let name = name.to_str().expect("a UTF-8 name");
            let repo = name.strip_suffix(".expected.json").expect("only goldens");
            let language = language
                .file_name()
                .and_then(|n| n.to_str())
                .expect("a name");
            keys.insert(format!("{language}/{repo}"));
        }
    }
    keys
}

#[test]
fn golden_files_sorted() {
    let root = repo_root();
    let keys = lock_keys(&root);
    assert!(!keys.is_empty(), "the lock has no golden repository");
    for key in &keys {
        let path = root
            .join("bench/golden")
            .join(format!("{key}.expected.json"));
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("{key}: no committed golden at {}: {e}", path.display()));
        let text = std::str::from_utf8(&bytes).unwrap_or_else(|e| panic!("{key}: not UTF-8: {e}"));
        assert!(text.ends_with('\n'), "{key}: not newline-terminated");
        assert!(!text.contains('\r'), "{key}: a carriage return");
        let golden = GoldenExpected::parse(&bytes).unwrap_or_else(|e| panic!("{key}: {e}"));

        // Node kinds: every kind exactly once, by spelling.
        let kinds: Vec<&str> = golden.node_counts.iter().map(|c| c.kind.as_str()).collect();
        assert!(
            kinds.windows(2).all(|w| w[0] < w[1]),
            "{key}: node_counts not strictly sorted by kind"
        );
        let mut every: Vec<&str> = NodeKind::ALL.iter().map(|k| k.as_str()).collect();
        every.sort_unstable();
        assert_eq!(
            kinds, every,
            "{key}: node_counts is not every node kind once"
        );

        // Drawn edges: sorted by (src_qn, dst_qn, kind, band), every band drawn,
        // every kind an edge kind.
        assert!(
            golden.drawn_edges.windows(2).all(|w| (
                &w[0].src_qn,
                &w[0].dst_qn,
                &w[0].kind,
                &w[0].band
            ) <= (
                &w[1].src_qn,
                &w[1].dst_qn,
                &w[1].kind,
                &w[1].band
            )),
            "{key}: drawn_edges not sorted by (src_qn, dst_qn, kind, band)"
        );
        for edge in &golden.drawn_edges {
            let band = Band::parse(&edge.band)
                .unwrap_or_else(|| panic!("{key}: {:?} is no band", edge.band));
            assert!(band.is_drawn(), "{key}: band {:?} is not drawn", edge.band);
            assert!(
                EdgeKind::parse(&edge.kind).is_some(),
                "{key}: {:?} is no edge kind",
                edge.kind
            );
        }

        // The file is exactly the canonical serialisation of what it holds.
        assert!(
            golden.to_canonical_bytes() == bytes,
            "{key}: not in the canonical format"
        );
    }
}

#[test]
fn golden_file_set_matches_lock() {
    let root = repo_root();
    let expected = lock_keys(&root);
    let committed = committed_keys(&root);
    assert_eq!(
        committed, expected,
        "bench/golden/ must hold exactly one golden per [[golden]] of bench/repos.lock"
    );
}
