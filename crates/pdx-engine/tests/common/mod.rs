//! What the tests share: the engine's fixtures, and how to read them.

#![allow(dead_code)] // each test binary uses only part of this module

use std::path::{Path, PathBuf};
use std::process::Command;

use pdx_engine::{
    AliasScope, CrateDependency, CrateManifest, PackageEntry, PathAlias, ResolutionMetadata,
};

/// The engine's test fixtures.
pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../engine/tests/fixtures")
}

/// The language a fixture file is in, by extension, as the resolution harness maps them.
pub fn language_of(path: &str) -> Option<&'static str> {
    let ext = Path::new(path).extension()?.to_str()?;
    Some(match ext {
        "py" => "python",
        "ts" => "typescript",
        "tsx" => "tsx",
        "js" | "jsx" => "javascript",
        "go" => "go",
        "java" => "java",
        "cs" => "csharp",
        "c" | "h" => "c",
        "cpp" | "hpp" | "cc" => "cpp",
        "rs" => "rust",
        "kt" => "kotlin",
        "php" => "php",
        "md" => "markdown",
        _ => return None,
    })
}

/// A fixture's source files in a language the tests know: relative path, language and
/// source, in path order, as the resolution harness adds them.
pub fn source_files(dir: &Path) -> Vec<(String, &'static str, Vec<u8>)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("a readable fixture") {
            let path = entry.expect("a readable entry").path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path.strip_prefix(root).unwrap().to_string_lossy();
                out.push(rel.replace('\\', "/"));
            }
        }
    }
    let mut paths = Vec::new();
    walk(dir, dir, &mut paths);
    paths.sort();
    paths
        .into_iter()
        .filter_map(|rel| {
            let lang = language_of(&rel)?;
            let source = std::fs::read(dir.join(&rel)).expect("a readable source");
            Some((rel, lang, source))
        })
        .collect()
}

/// The repository metadata a resolution fixture declares in `pdx-metadata.txt`, if any.
/// The format is the resolution harness's (`engine/tests/resolve_dump.c`).
pub fn metadata(dir: &Path) -> Option<ResolutionMetadata> {
    let text = std::fs::read_to_string(dir.join("pdx-metadata.txt")).ok()?;
    let mut m = ResolutionMetadata::default();
    let mut manifest: Option<CrateManifest> = None;
    for line in text.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let field = |i: usize| fields.get(i).copied().unwrap_or("").to_owned();
        match fields[0] {
            "" => {}
            k if k.starts_with('#') => {}
            "package" => m.packages.push(PackageEntry {
                import_prefix: field(1),
                entry_path: field(2),
            }),
            "scope" => m.alias_scopes.push(AliasScope {
                dir_prefix: field(1),
                base_url: Some(field(2)),
                aliases: Vec::new(),
            }),
            "alias" => m
                .alias_scopes
                .last_mut()
                .expect("an alias follows its scope")
                .aliases
                .push(PathAlias {
                    alias_prefix: field(1),
                    alias_suffix: field(2),
                    target_prefix: field(3),
                    target_suffix: field(4),
                    has_wildcard: field(5) == "1",
                }),
            k if k.starts_with("crate-") => {
                let c = manifest.get_or_insert_with(|| CrateManifest {
                    package_name: None,
                    is_workspace_root: false,
                    dependencies: Vec::new(),
                    member_paths: Vec::new(),
                });
                match k {
                    "crate-package" => c.package_name = Some(field(1)),
                    "crate-workspace" => c.is_workspace_root = true,
                    "crate-dependency" => c.dependencies.push(CrateDependency {
                        name: field(1),
                        path: Some(field(2)).filter(|p| !p.is_empty()),
                    }),
                    "crate-member" => c.member_paths.push(field(1)),
                    other => panic!("unknown metadata line {other}"),
                }
            }
            other => panic!("unknown metadata line {other}"),
        }
    }
    m.crate_manifest = manifest;
    Some(m)
}

/// The resolution fixtures, one directory each, in name order.
pub fn resolution_fixtures() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(fixtures().join("resolve"))
        .expect("the resolution fixtures")
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

const CHILD: &str = "PDX_ENGINE_TEST_CHILD";

/// Runs the calling test again in a child process of this test binary, with `env` set
/// in the child only, and fails unless it passes there. The engine reads its test
/// switches from the environment, which is shared by every test in a process; a child
/// keeps a switch from reaching any other test.
///
/// Returns `true` in the parent, which then has nothing more to do, and `false` in the
/// child, which then runs the test's body.
pub fn rerun_in_child(test: &str, env: &[(&str, &str)]) -> bool {
    if std::env::var_os(CHILD).is_some() {
        return false;
    }
    let out = Command::new(std::env::current_exe().expect("the test binary"))
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .envs(env.iter().copied())
        .output()
        .expect("the test binary runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "{test} failed in its child:\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("1 passed"),
        "{test} did not run in its child:\n{stdout}"
    );
    true
}
