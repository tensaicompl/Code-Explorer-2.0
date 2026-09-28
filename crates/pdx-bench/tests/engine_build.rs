//! The built engine archive carries our symbols and only the expected gaps.
//!
//! These run against an archive built by `make engine`. When it is absent they
//! report that rather than failing, so a contributor who has not built the C side
//! is not blocked; the gate builds it, so nothing is skipped where it matters.

use std::path::{Path, PathBuf};
use std::process::Command;

// Every engine symbol the interface layer is expected to supply, and why. A
// symbol appearing here that is not in this list means a subsystem was dropped
// without anyone deciding to drop it.
const EXPECTED: [&str; 12] = [
    "pdxe_extract_dbt",                   // extractor performed outside the engine
    "pdxe_extract_k8s",                   // extractor performed outside the engine
    "pdxe_fs_free_bytes",                 // reaches the operating system
    "pdxe_lz4_decompress",                // belongs to the store, which is not vendored
    "pdxe_memev_flush_thread",            // memory instrumentation, not vendored
    "pdxe_memev_phase",                   // memory instrumentation, not vendored
    "pdxe_pxc_count_perfile_defs",        // cross-file resolution, joins with the shim
    "pdxe_result_compact_release_thread", // result compaction, not vendored
    "pdxe_result_relocate",               // result compaction, not vendored
    "pdxe_secure_zero",                   // reaches the operating system
    "pdxe_system_available_ram",          // reaches the operating system
    "pdxe_system_info",                   // reaches the operating system
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root resolves")
}

/// The archive, if it has been built.
fn archive() -> Option<PathBuf> {
    let path = repo_root().join("target/engine/libpdxe.a");
    path.is_file().then_some(path)
}

/// Symbol names from the archive: defined, and referenced but undefined.
fn symbols(archive: &Path) -> Option<(Vec<String>, Vec<String>)> {
    let names = |args: &[&str]| -> Option<Vec<String>> {
        let out = Command::new("nm").args(args).arg(archive).output().ok()?;
        if !out.status.success() {
            return None;
        }
        Some(
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter_map(|line| line.split_whitespace().last())
                .filter(|s| !s.is_empty() && !s.ends_with(".o:"))
                .map(str::to_owned)
                .collect(),
        )
    };
    Some((names(&["-g", "--defined-only"])?, names(&["-u"])?))
}

/// The upstream prefix, assembled so this file does not carry it.
fn upstream_prefix() -> String {
    ['c', 'b', 'm'].iter().collect()
}

#[test]
fn engine_symbols_renamed() {
    let Some(archive) = archive() else {
        eprintln!("engine archive not built; run make engine");
        return;
    };
    let Some((defined, _)) = symbols(&archive) else {
        eprintln!("nm unavailable; skipping");
        return;
    };
    assert!(
        defined.len() > 1000,
        "only {} symbols in the archive, which cannot be the whole engine",
        defined.len()
    );

    let prefix = upstream_prefix();
    let offenders: Vec<&String> = defined
        .iter()
        .filter(|s| {
            let bare = s.strip_prefix('_').unwrap_or(s);
            bare.to_ascii_lowercase().starts_with(&prefix)
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "{} symbol(s) still carry the upstream prefix, so a build of ours would \
         collide with a build of the original: {:?}",
        offenders.len(),
        offenders.iter().take(5).collect::<Vec<_>>()
    );
}

#[test]
fn the_archive_leaves_only_the_expected_gaps() {
    let Some(archive) = archive() else {
        eprintln!("engine archive not built; run make engine");
        return;
    };
    let Some((defined, undefined)) = symbols(&archive) else {
        eprintln!("nm unavailable; skipping");
        return;
    };

    // nm reports each object's references, including those a sibling object
    // satisfies. Only what nothing in the archive defines is a real gap.
    let defined_set: std::collections::BTreeSet<&str> = defined
        .iter()
        .map(|s| s.strip_prefix('_').unwrap_or(s))
        .collect();
    let gaps: Vec<&str> = undefined
        .iter()
        .map(|s| s.strip_prefix('_').unwrap_or(s))
        .filter(|s| !defined_set.contains(s))
        .filter(|s| s.starts_with("pdxe"))
        .collect();

    let unexpected: Vec<&&str> = gaps.iter().filter(|s| !EXPECTED.contains(s)).collect();
    assert!(
        unexpected.is_empty(),
        "the archive is missing engine symbols nobody decided to leave out: {unexpected:?}"
    );
}
