//! Repository-level invariants of the restructuring and the legal baseline.
//!
//! These belong to the two tasks that established them, neither of which could
//! run a test: the harness did not exist yet.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

// The directories that moved wholesale, and the files that moved with them
// because they only ever configured that stack.
const MOVED_DIRS: [&str; 5] = ["indexer/", "backend/", "frontend/", "mcp-server/", "e2e/"];
const MOVED_FILES: [&str; 3] = ["docker-compose.yml", ".env.example", ".mcp.json.example"];
// Deliberately deleted rather than moved.
const DELETED: [&str; 1] = ["enrichment/"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root resolves")
}

#[test]
fn legal_files_present() {
    let root = repo_root();
    for name in [
        "LICENSE",
        "LICENSE-ENTERPRISE",
        "NOTICE",
        "THIRD_PARTY_NOTICES.md",
    ] {
        let path = root.join(name);
        assert!(path.is_file(), "{name} is missing");
        assert!(
            std::fs::metadata(&path).expect("metadata").len() > 0,
            "{name} is empty"
        );
    }

    let notices = std::fs::read_to_string(root.join("THIRD_PARTY_NOTICES.md")).expect("notices");
    let entries = notices.lines().filter(|l| l.starts_with("### ")).count();
    assert!(entries >= 1, "the notices file lists no component");

    let licence = std::fs::read_to_string(root.join("LICENSE")).expect("licence");
    assert!(
        licence.contains("Apache License"),
        "the root licence is not the Apache licence"
    );
    assert!(
        licence.contains("LICENSE-ENTERPRISE"),
        "the root licence does not point at the enterprise licence"
    );

    let notice = std::fs::read_to_string(root.join("NOTICE")).expect("notice");
    assert!(
        notice.contains("Copyright"),
        "the notice carries no copyright line"
    );
}

#[test]
fn legacy_tree_snapshot() {
    let root = repo_root();
    let snapshot = std::fs::read_to_string(root.join("bench/legacy-tree.txt"))
        .expect("the pre-restructuring file list is committed");

    let mut expected: BTreeSet<String> = BTreeSet::new();
    for line in snapshot.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if DELETED.iter().any(|d| line.starts_with(d)) {
            assert!(
                !root.join(line).exists(),
                "{line} was deleted by the restructuring but is present"
            );
            continue;
        }
        if MOVED_DIRS.iter().any(|d| line.starts_with(d)) || MOVED_FILES.contains(&line) {
            expected.insert(format!("legacy/{line}"));
        }
    }

    assert!(
        expected.len() > 200,
        "the snapshot lists only {} moved paths, which is too few to be the real tree",
        expected.len()
    );

    let missing: Vec<&String> = expected.iter().filter(|p| !root.join(p).exists()).collect();
    assert!(
        missing.is_empty(),
        "{} paths did not survive the move, first few: {:?}",
        missing.len(),
        missing.iter().take(5).collect::<Vec<_>>()
    );

    assert!(
        root.join("legacy/README.md").is_file(),
        "legacy/README.md does not state the directory is read-only reference"
    );
    for dir in MOVED_DIRS {
        assert!(
            !root.join(dir).exists(),
            "{dir} is still at the top level after the move"
        );
    }
}
