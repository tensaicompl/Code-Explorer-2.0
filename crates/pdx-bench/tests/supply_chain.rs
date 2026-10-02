//! The supply-chain policy stays in force (plan Part 5.12; docs/plan/ISSUES.md, issue
//! 36): the cargo-vet store exists, the tool's version is pinned in one place, every
//! workflow that builds what ships runs the check, and nothing accepts a crate on its
//! own.
//!
//! `cargo vet --locked` itself runs in CI (`make vet`); this test only guards against
//! the policy around it being removed or weakened by accident.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root resolves")
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|e| panic!("{relative}: {e}"))
}

/// The pinned version, from the one line that declares it.
fn pinned_version() -> String {
    let script = read("scripts/cargo-vet.sh");
    let declarations: Vec<&str> = script
        .lines()
        .filter(|l| l.starts_with("CARGO_VET_VERSION="))
        .collect();
    assert_eq!(
        declarations.len(),
        1,
        "the version is declared exactly once: {declarations:?}"
    );
    declarations[0]
        .trim_start_matches("CARGO_VET_VERSION=")
        .to_owned()
}

#[test]
fn the_cargo_vet_store_exists() {
    for file in [
        "supply-chain/config.toml",
        "supply-chain/audits.toml",
        "supply-chain/imports.lock",
    ] {
        assert!(repo_root().join(file).is_file(), "{file} is missing");
    }
    let config = read("supply-chain/config.toml");
    assert!(
        config.contains("[cargo-vet]"),
        "not a cargo-vet configuration"
    );
    // Trusting another organisation's audits is the owner's decision, recorded when
    // made; until then the store imports none.
    assert!(
        !config.contains("[imports"),
        "an audit import was added without a recorded decision"
    );
}

#[test]
fn the_cargo_vet_version_is_pinned_once() {
    let version = pinned_version();
    let parts: Vec<&str> = version.split('.').collect();
    assert!(
        parts.len() == 3
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())),
        "{version:?} is not an exact version"
    );
    // Nothing else writes the version down: installers and checks read the script.
    for file in [
        "Makefile",
        ".github/workflows/ci.yml",
        ".github/workflows/nightly.yml",
        ".github/workflows/release.yml",
        "CLAUDE.md",
    ] {
        assert!(
            !read(file).contains(&version),
            "{file} repeats the cargo-vet version"
        );
    }
}

#[test]
fn every_shipping_workflow_runs_the_check() {
    let makefile = read("Makefile");
    assert!(
        makefile.contains("\nvet:\n\t@scripts/cargo-vet.sh\n"),
        "make vet does not run the check"
    );
    for workflow in ["ci.yml", "nightly.yml", "release.yml"] {
        let text = read(&format!(".github/workflows/{workflow}"));
        assert!(
            text.contains("run: scripts/cargo-vet.sh install"),
            "{workflow} does not install the pinned cargo-vet"
        );
        assert!(
            text.contains("run: make vet"),
            "{workflow} does not run make vet"
        );
    }
}

#[test]
fn nothing_accepts_a_crate_on_its_own() {
    // The check verifies; it never initialises the store or regenerates exemptions,
    // which would accept whatever the lock file holds.
    for file in [
        "Makefile",
        "scripts/cargo-vet.sh",
        ".github/workflows/ci.yml",
        ".github/workflows/nightly.yml",
        ".github/workflows/release.yml",
    ] {
        let text = read(file);
        for forbidden in [
            "vet init",
            "vet regenerate",
            "regenerate exemptions",
            "vet certify",
            "vet import",
        ] {
            let in_code = text
                .lines()
                .filter(|l| !l.trim_start().starts_with('#'))
                .any(|l| l.contains(forbidden));
            assert!(!in_code, "{file} runs `{forbidden}`");
        }
    }
    let script = read("scripts/cargo-vet.sh");
    assert!(
        script.contains("\ncargo vet --locked\n"),
        "the check is not `cargo vet --locked`"
    );
}
