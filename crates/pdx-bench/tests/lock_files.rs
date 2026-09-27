//! The lock files parse and every pin they carry is usable.
//!
//! These belong to the task that wrote the lock files, which could not run them:
//! the test harness did not exist yet. Recorded as an issue there and closed here.

use std::path::{Path, PathBuf};

use pdx_bench::lock;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root resolves")
}

#[test]
fn lock_files_parse() {
    let root = repo_root();
    let refs = lock::references(&root.join("bench/references.lock")).expect("references parse");
    let repos = lock::bench_repos(&root.join("bench/repos.lock")).expect("repos parse");

    assert_eq!(refs.lock_version, 1);
    assert_eq!(repos.lock_version, 1);
    assert!(!refs.references.is_empty(), "no references pinned");
    assert!(!refs.grammars.is_empty(), "no grammars pinned");
    assert!(!repos.golden.is_empty(), "no golden repositories pinned");
    assert!(!repos.scale.is_empty(), "no scale repositories pinned");
}

#[test]
fn every_pin_is_a_full_commit_sha() {
    let root = repo_root();
    let refs = lock::references(&root.join("bench/references.lock")).expect("references parse");
    let repos = lock::bench_repos(&root.join("bench/repos.lock")).expect("repos parse");

    let mut commits: Vec<(&str, &str)> = Vec::new();
    for r in &refs.references {
        commits.push((&r.name, &r.commit));
    }
    for g in &refs.grammars {
        commits.push((&g.language, &g.commit));
    }
    for r in repos.golden.iter().chain(repos.scale.iter()) {
        commits.push((&r.url, &r.commit));
    }

    for (what, commit) in commits {
        assert_eq!(
            commit.len(),
            40,
            "{what}: commit is not a full sha: {commit}"
        );
        assert!(
            commit
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "{what}: commit is not lowercase hex: {commit}"
        );
    }
}

#[test]
fn allow_list_matches_the_one_compiled_in() {
    let root = repo_root();
    let refs = lock::references(&root.join("bench/references.lock")).expect("references parse");
    let mut from_file: Vec<&str> = refs.licence_allow_list.iter().map(String::as_str).collect();
    let mut compiled: Vec<&str> = lock::LICENCE_ALLOW_LIST.to_vec();
    from_file.sort_unstable();
    compiled.sort_unstable();
    assert_eq!(
        from_file, compiled,
        "the lock file's allow list and the compiled one disagree"
    );
}

#[test]
fn every_vendored_or_ported_licence_is_allowed() {
    let root = repo_root();
    let refs = lock::references(&root.join("bench/references.lock")).expect("references parse");

    for r in &refs.references {
        assert!(
            lock::licence_allowed(&r.licence),
            "reference {} carries {}, which is not on the allow list",
            r.name,
            r.licence
        );
    }
    for g in &refs.grammars {
        assert!(
            lock::licence_allowed(&g.licence),
            "grammar {} carries {}, which is not on the allow list; \
             such a language leaves the matrix rather than being vendored",
            g.language,
            g.licence
        );
    }
}

#[test]
fn golden_repositories_are_permissively_licensed() {
    let root = repo_root();
    let repos = lock::bench_repos(&root.join("bench/repos.lock")).expect("repos parse");

    // Corpus files are taken from the golden repositories and committed here, so
    // their licences bind us. Scale repositories are only ever indexed.
    for r in &repos.golden {
        let language = r.language.as_deref().unwrap_or("?");
        assert!(
            lock::licence_allowed(&r.licence),
            "golden repository for {language} carries {}, which is not permissive",
            r.licence
        );
    }
    for r in &repos.scale {
        assert!(
            r.indexed_only,
            "scale repository {} must be marked indexed only",
            r.url
        );
    }
}

#[test]
fn a_replacement_records_why() {
    let root = repo_root();
    let repos = lock::bench_repos(&root.join("bench/repos.lock")).expect("repos parse");
    for r in repos.golden.iter().chain(repos.scale.iter()) {
        if r.replaces.is_some() {
            assert!(
                r.replacement_reason.is_some(),
                "{} replaces another repository without recording why",
                r.url
            );
        }
    }
}
