//! Invariants of the vendored engine.
//!
//! The engine is third-party source we maintain rather than author. Three things
//! must hold for it at all times: its licence and those of everything vendored
//! inside it travel with the code, nothing names where the code came from, and the
//! upstream symbol prefix is gone so that our symbols cannot collide with a build
//! of the original.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root resolves")
}

fn engine() -> PathBuf {
    repo_root().join("engine")
}

/// Directories directly under a path.
fn subdirs(path: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    out.sort();
    out
}

fn carries_a_licence(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|e| {
        let name = e.file_name().to_string_lossy().to_uppercase();
        name.starts_with("LICENSE") || name.starts_with("COPYING") || name.starts_with("NOTICE")
    })
}

#[test]
fn the_engine_carries_its_licence() {
    let licence = engine().join("LICENSE-ENGINE");
    assert!(
        licence.is_file(),
        "the vendored engine has no licence file, which the licence itself requires"
    );
    let text = std::fs::read_to_string(&licence).expect("licence is readable");
    assert!(
        text.contains("Permission is hereby granted"),
        "the engine licence file does not read as a permissive licence"
    );
    assert!(
        text.contains("Copyright"),
        "the engine licence file carries no copyright line"
    );
}

#[test]
fn every_grammar_carries_its_licence() {
    let grammars = subdirs(&engine().join("grammars"));
    assert!(
        grammars.len() >= 30,
        "expected the language matrix's grammars, found {}",
        grammars.len()
    );
    let bare: Vec<String> = grammars
        .iter()
        .filter(|d| !d.join("LICENSE").is_file())
        .map(|d| {
            d.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert!(
        bare.is_empty(),
        "grammars vendored without their licence, which is not permitted: {bare:?}"
    );
}

#[test]
fn every_grammar_has_a_parser() {
    for dir in subdirs(&engine().join("grammars")) {
        let name = dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        assert!(
            dir.join("parser.c").is_file(),
            "grammar {name} has no parser, so its language cannot be read"
        );
    }
}

#[test]
fn every_vendored_library_carries_its_licence() {
    let libs = subdirs(&engine().join("vendored"));
    assert!(!libs.is_empty(), "no vendored libraries found");
    let bare: Vec<String> = libs
        .iter()
        .filter(|d| !carries_a_licence(d))
        .map(|d| {
            d.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert!(
        bare.is_empty(),
        "libraries vendored without a licence file: {bare:?}"
    );
}

#[test]
fn the_upstream_symbol_prefix_is_gone() {
    // Written as a character class so this file does not itself carry the prefix it
    // exists to forbid.
    let prefix = regex_lite_prefix();
    let mut offenders = Vec::new();
    walk(&engine(), &mut |path| {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "c" | "h" | "cpp" | "hpp" | "cc") {
            return;
        }
        // Grammars and vendored libraries never carried the prefix.
        let as_str = path.to_string_lossy();
        if as_str.contains("/grammars/") || as_str.contains("/vendored/") {
            return;
        }
        if let Ok(text) = std::fs::read_to_string(path)
            && (text.contains(&prefix) || text.contains(&prefix.to_uppercase()))
        {
            offenders.push(as_str.into_owned());
        }
    });
    assert!(
        offenders.is_empty(),
        "the upstream symbol prefix survives in {} file(s); the rename step did not \
         finish, and a build of ours would collide with a build of the original: {:?}",
        offenders.len(),
        offenders.iter().take(5).collect::<Vec<_>>()
    );
}

/// The upstream prefix, assembled rather than written.
fn regex_lite_prefix() -> String {
    let parts = ['c', 'b', 'm', '_'];
    parts.iter().collect()
}

fn walk(dir: &Path, f: &mut impl FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, f);
        } else {
            f(&path);
        }
    }
}

#[test]
fn the_vendoring_scripts_keep_upstream_paths_out_of_themselves() {
    // The scripts are scanned like any other source, so the upstream's location
    // lives in data files that are the scanner's input, not in the scripts.
    let root = repo_root();
    for data in [
        "scripts/vendor/copy-map.txt",
        "scripts/vendor/rename-map.txt",
        "bench/references.lock",
    ] {
        assert!(root.join(data).is_file(), "{data} is missing");
    }
    for script in ["copy-engine.sh", "fetch-engine.sh", "rename-engine.sh"] {
        let text = std::fs::read_to_string(root.join("scripts/vendor").join(script))
            .expect("script is readable");
        assert!(
            !text.contains(&regex_lite_prefix()),
            "{script} carries the upstream prefix; move it to a data file"
        );
    }
}
