//! The golden plan: `[[golden]]` entries of the lock, checked before any fetch, sorted,
//! and never a scale repository.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use pdx_bench::golden::plan::{GoldenEntry, PlanError, check_component, check_url, golden_plan};
use pdx_bench::lock::{self, BenchRepos};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

fn lock_text(golden: &[(&str, &str)], scale: bool) -> String {
    let mut text = String::from("lock_version = 1\nfetch_date = \"2026-01-01\"\n");
    if !scale {
        // Before any table, or it would be a key of the last one.
        text.push_str("scale = []\n");
    }
    for (language, url) in golden {
        let _ = write!(
            text,
            "\n[[golden]]\nlanguage = \"{language}\"\nurl = \"{url}\"\ndefault_branch = \"main\"\n\
             commit = \"{COMMIT}\"\nlicence = \"MIT\"\nfetch_date = \"2026-01-01\"\n"
        );
    }
    if scale {
        text.push_str(
            "\n[[scale]]\nname = \"big\"\nurl = \"git.example/acme/big\"\ndefault_branch = \"main\"\n\
             commit = \"fedcba9876543210fedcba9876543210fedcba98\"\nlicence = \"GPL-2.0-only\"\n\
             fetch_date = \"2026-01-01\"\nindexed_only = true\n",
        );
    }
    text
}

fn parse(text: &str) -> BenchRepos {
    toml::from_str(text).expect("a lock")
}

#[test]
fn golden_plan_selects_golden_entries_only() {
    let repos = parse(&lock_text(&[("python", "git.example/acme/shop")], true));
    assert_eq!(repos.scale.len(), 1);
    let plan = golden_plan(&repos).expect("a plan");
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].key(), "python/shop");
    assert!(plan.iter().all(|e| !e.url.ends_with("/big")));
}

#[test]
fn golden_plan_is_sorted_by_language_then_repository() {
    let repos = parse(&lock_text(
        &[
            ("rust", "git.example/acme/zeta"),
            ("go", "git.example/acme/beta"),
            ("rust", "git.example/acme/alpha"),
            ("ada", "git.example/acme/omega.git"),
        ],
        false,
    ));
    let keys: Vec<String> = golden_plan(&repos)
        .expect("a plan")
        .iter()
        .map(GoldenEntry::key)
        .collect();
    assert_eq!(keys, ["ada/omega", "go/beta", "rust/alpha", "rust/zeta"]);
}

#[test]
fn golden_entry_paths_and_fetch_url() {
    let repos = parse(&lock_text(
        &[("python", "git.example/acme/shop.git")],
        false,
    ));
    let entry = golden_plan(&repos).expect("a plan").remove(0);
    assert_eq!(entry.repo, "shop");
    assert_eq!(entry.file_name(), "shop.expected.json");
    assert_eq!(
        entry.relative_path(),
        PathBuf::from("python").join("shop.expected.json")
    );
    // The only fetch URL is HTTPS over the checked lock text.
    let scheme = ["ht", "tps://"].concat();
    assert_eq!(
        entry.fetch_url(),
        format!("{scheme}git.example/acme/shop.git")
    );
    assert_eq!(entry.commit, COMMIT);
}

#[test]
fn golden_plan_validates_every_entry_before_any_fetch() {
    let base = lock_text(&[("python", "git.example/acme/shop")], false);
    // Another lock format.
    let other = base.replace("lock_version = 1", "lock_version = 2");
    assert!(matches!(
        golden_plan(&parse(&other)),
        Err(PlanError::LockVersion(2))
    ));
    // No language.
    let no_language = base.replace("language = \"python\"\n", "");
    assert!(matches!(
        golden_plan(&parse(&no_language)),
        Err(PlanError::NoLanguage(1))
    ));
    // A short, an upper-case, or a non-hex commit.
    for commit in [&COMMIT[..39], &COMMIT.to_ascii_uppercase(), &"g".repeat(40)] {
        let text = base.replace(COMMIT, commit);
        assert!(
            matches!(golden_plan(&parse(&text)), Err(PlanError::Commit(_))),
            "{commit}"
        );
    }
    // A licence off the allow list.
    let gpl = base.replace("licence = \"MIT\"", "licence = \"GPL-3.0-only\"");
    assert!(matches!(
        golden_plan(&parse(&gpl)),
        Err(PlanError::Licence { .. })
    ));
    // Two entries writing one file, on a case-insensitive file system too.
    for pair in [
        [
            ("python", "git.example/a/shop"),
            ("python", "git.example/b/shop"),
        ],
        [
            ("python", "git.example/a/shop"),
            ("Python", "git.example/b/Shop"),
        ],
    ] {
        assert!(matches!(
            golden_plan(&parse(&lock_text(&pair, false))),
            Err(PlanError::Duplicate(..))
        ));
    }
    // An unsafe language component.
    for language in ["", ".", "..", "py/thon", "py\\thon", "con", "py ", "py."] {
        let text = lock_text(&[(language, "git.example/acme/shop")], false);
        assert!(
            matches!(
                golden_plan(&parse(&text)),
                Err(PlanError::UnsafeComponent { .. })
            ),
            "{language:?}"
        );
    }
}

#[test]
fn golden_plan_refuses_urls_and_never_quotes_them() {
    let secret = ["s3c", "r3t"].concat();
    let scheme = ["ht", "tps://"].concat();
    for url in [
        format!("{scheme}git.example/acme/shop"),
        format!("user:{secret}@git.example/acme/shop"),
        format!("git.example/acme/shop?token={secret}"),
        format!("git.example/acme/shop#{secret}"),
        "git.example\\acme\\shop".to_owned(),
        "git.example/acme/sh op".to_owned(),
        "git.example/acme/shop\u{0}".to_owned(),
        "/acme/shop".to_owned(),
        "git.example".to_owned(),
        "git.example/".to_owned(),
        "git.example//shop".to_owned(),
        "git.example/acme/./shop".to_owned(),
        "git.example/acme/../shop".to_owned(),
        "git.example:x/acme/shop".to_owned(),
        "git.example/acme/sh$op".to_owned(),
    ] {
        let text = lock_text(&[("python", "PLACEHOLDER")], false).replace(
            "PLACEHOLDER",
            &url.replace('\\', "\\\\").replace('\u{0}', "\\u0000"),
        );
        match golden_plan(&parse(&text)) {
            Err(e @ PlanError::Url { .. }) => {
                let message = e.to_string();
                assert!(!message.contains(&secret), "{message}");
                assert!(!message.contains("git.example"), "{message}");
            }
            other => panic!("{url:?}: expected a refused URL, got {other:?}"),
        }
    }
    // A port is allowed, as are the usual repository names.
    assert!(check_url("git.example:8443/acme/shop").is_ok());
    assert!(check_url("git.example/acme/my-repo_2.0~x").is_ok());
}

#[test]
fn unsafe_components_are_refused_never_rewritten() {
    for name in [
        "",
        ".",
        "..",
        "a/b",
        "a\\b",
        "a\u{1}",
        "a:b",
        "a*b",
        "a?b",
        "a\"b",
        "a<b",
        "a|b",
        "trailing ",
        "trailing.",
        "NUL",
        "com1.txt",
        "Lpt9",
    ] {
        assert!(check_component(name).is_err(), "{name:?}");
    }
    for name in [
        "python",
        "Ada_Drivers_Library",
        "c",
        "spring-petclinic",
        "x.y",
    ] {
        assert!(check_component(name).is_ok(), "{name:?}");
    }
}

#[test]
fn the_committed_lock_makes_a_plan_of_its_golden_entries() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let repos = lock::bench_repos(&root.join("bench/repos.lock")).expect("the lock");
    let plan = golden_plan(&repos).expect("the committed lock is valid");
    assert_eq!(plan.len(), repos.golden.len());
    let keys: Vec<String> = plan.iter().map(GoldenEntry::key).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    for scale in &repos.scale {
        assert!(plan.iter().all(|e| e.url != scale.url), "{}", scale.url);
    }
}
