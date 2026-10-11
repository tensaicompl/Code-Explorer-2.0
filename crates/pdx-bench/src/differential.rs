//! The engine differential's corpora (P2-16, decision 50): `bench/engine-differential.toml`
//! checked against the language matrix and `bench/repos.lock`, and handed to
//! `scripts/engine/differential.py`, which runs both engines over them.
//!
//! A corpus is an explicit list of files, never discovered:
//!
//! - `[[fixture]]`: a typed-resolution fixture under `engine/tests/fixtures/resolve/`,
//!   project-authored and committed, every file of which is listed: a compared source
//!   (`files`), a support file staged for the engines but never compared (`support`:
//!   `pdx-metadata.txt` for the vendored engine, the native manifest it stands for for
//!   the reference), or the fixture's own `assert.tsv` and `expected.tsv`.
//! - `[[golden]]`: a bounded subset of a typed-tier golden repository, its paths at the
//!   commit `bench/repos.lock` pins, fetched as the golden harness fetches (P2-12). No
//!   source is committed; the paths are, so a pin refresh never silently changes what is
//!   compared. A subset stages its sources only, so neither engine receives repository
//!   metadata, and is chosen so that its relations need none.
//!
//! Coverage is checked here, against the matrix and the lock: every typed-tier language
//! has a fixture, every typed-tier golden a subset, and every fixture directory an entry.
//! Each file must be the corpus's language by the matrix's own detection, and each
//! engine language is the matrix's for the path (`.tsx` is `tsx`). A C or C++ file of a
//! golden subset is at most 32 KiB, where patch 0012's expression budget equals the
//! reference's fixed one, so the two engines resolve it under the same budget.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use pdx_core::languages::{self, Tier};
use serde::{Deserialize, Serialize};

use crate::golden::plan::{GoldenEntry, check_component, golden_plan};
use crate::lock::BenchRepos;

/// The manifest format this harness reads.
pub const FORMAT: u32 = 1;

/// The manifest, relative to the workspace root.
pub const MANIFEST: &str = "bench/engine-differential.toml";

/// Where the typed-resolution fixtures are.
pub const FIXTURE_ROOT: &str = "engine/tests/fixtures/resolve";

/// A fixture's own files, which no engine reads.
pub const FIXTURE_RECORDS: [&str; 2] = ["assert.tsv", "expected.tsv"];

/// The fewest and most files a golden subset holds.
pub const SUBSET_FILES: std::ops::RangeInclusive<usize> = 2..=10;

/// The largest C or C++ file a golden subset may hold: one started 32 KiB, for which
/// patch 0012's budget is the reference's 10,000 steps.
pub const C_FAMILY_MAX_BYTES: u64 = 32 * 1024;

/// `bench/engine-differential.toml`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// [`FORMAT`].
    pub format: u32,
    /// The fixtures.
    #[serde(default)]
    pub fixture: Vec<FixtureCorpus>,
    /// The golden subsets.
    #[serde(default)]
    pub golden: Vec<GoldenSubset>,
}

/// One fixture.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FixtureCorpus {
    /// The fixture directory's name.
    pub name: String,
    /// The matrix language every file is.
    pub language: String,
    /// `engine/tests/fixtures/resolve/<name>`.
    pub root: String,
    /// The compared sources, sorted.
    pub files: Vec<String>,
    /// Staged, never compared, sorted.
    #[serde(default)]
    pub support: Vec<String>,
}

/// One golden subset.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GoldenSubset {
    /// `<language>/<repo>`, as the golden harness names it.
    pub key: String,
    /// The pinned commit, which must be the lock's.
    pub commit: String,
    /// Why these files: the relations they exercise.
    pub cluster: String,
    /// The compared sources, sorted.
    pub files: Vec<String>,
}

/// Why the corpora cannot be used.
#[derive(Debug, thiserror::Error)]
pub enum DifferentialError {
    /// The manifest cannot be read or parsed.
    #[error("{MANIFEST}: {0}")]
    Manifest(String),
    /// The manifest breaks a rule.
    #[error("{MANIFEST}: {0}")]
    Rule(String),
    /// A file is not what it must be.
    #[error("{corpus}: {path}: {reason}")]
    File {
        /// The corpus.
        corpus: String,
        /// The file.
        path: String,
        /// Why.
        reason: String,
    },
}

fn rule(message: impl Into<String>) -> DifferentialError {
    DifferentialError::Rule(message.into())
}

/// Parses the manifest.
///
/// # Errors
///
/// [`DifferentialError::Manifest`] for anything but this format.
pub fn parse(text: &str) -> Result<Manifest, DifferentialError> {
    let manifest: Manifest =
        toml::from_str(text).map_err(|e| DifferentialError::Manifest(e.to_string()))?;
    if manifest.format != FORMAT {
        return Err(DifferentialError::Manifest(format!(
            "format {} is not {FORMAT}",
            manifest.format
        )));
    }
    Ok(manifest)
}

/// Whether `items` are strictly increasing: sorted, no duplicate.
fn strictly_sorted(items: &[String]) -> bool {
    items.windows(2).all(|w| w[0] < w[1])
}

/// A repository-relative POSIX path, every component safe on every platform.
fn check_path(path: &str) -> Result<(), String> {
    if path.is_empty() || path.starts_with('/') {
        return Err("not a relative path".to_owned());
    }
    for part in path.split('/') {
        check_component(part).map_err(|reason| format!("a component is unsafe ({reason})"))?;
    }
    Ok(())
}

/// The typed-tier languages of the matrix, sorted.
pub fn typed_languages() -> Vec<&'static str> {
    let mut found: Vec<&'static str> = languages::all()
        .iter()
        .filter(|l| l.tier == Tier::Typed)
        .map(|l| l.id)
        .collect();
    found.sort_unstable();
    found
}

/// The lock's golden entries whose language is in the typed tier.
///
/// # Errors
///
/// When the lock makes no golden plan.
pub fn typed_goldens(repos: &BenchRepos) -> Result<Vec<GoldenEntry>, DifferentialError> {
    let typed = typed_languages();
    Ok(golden_plan(repos)
        .map_err(|e| rule(e.to_string()))?
        .into_iter()
        .filter(|g| typed.contains(&g.language.as_str()))
        .collect())
}

/// Checks the manifest alone: order, uniqueness, safe paths, languages, pins, sizes,
/// and coverage of every typed-tier language, typed-tier golden and fixture directory.
/// `fixture_dirs` names the directories under [`FIXTURE_ROOT`].
///
/// # Errors
///
/// [`DifferentialError::Rule`] for the first rule broken.
pub fn check_manifest(
    manifest: &Manifest,
    repos: &BenchRepos,
    fixture_dirs: &[String],
) -> Result<(), DifferentialError> {
    let names: Vec<String> = manifest.fixture.iter().map(|f| f.name.clone()).collect();
    if !strictly_sorted(&names) {
        return Err(rule(
            "fixtures are not sorted by name, or one is listed twice",
        ));
    }
    let mut dirs = fixture_dirs.to_vec();
    dirs.sort();
    if names != dirs {
        return Err(rule(format!(
            "the fixtures listed are not every fixture under {FIXTURE_ROOT}: listed {names:?}, present {dirs:?}"
        )));
    }
    for fixture in &manifest.fixture {
        let at = |m: String| rule(format!("fixture {}: {m}", fixture.name));
        if fixture.root != format!("{FIXTURE_ROOT}/{}", fixture.name) {
            return Err(at(format!("root is not {FIXTURE_ROOT}/{}", fixture.name)));
        }
        let language = languages::by_id(&fixture.language)
            .ok_or_else(|| at(format!("{} is no language of the matrix", fixture.language)))?;
        if language.tier != Tier::Typed {
            return Err(at(format!("{} is not in the typed tier", fixture.language)));
        }
        check_list(&fixture.files, &fixture.support).map_err(at)?;
    }
    let covered: BTreeSet<&str> = manifest
        .fixture
        .iter()
        .map(|f| f.language.as_str())
        .collect();
    let uncovered: Vec<&str> = typed_languages()
        .into_iter()
        .filter(|l| !covered.contains(l))
        .collect();
    if !uncovered.is_empty() {
        return Err(rule(format!(
            "no fixture for the typed-tier language(s) {}",
            uncovered.join(", ")
        )));
    }

    let keys: Vec<String> = manifest.golden.iter().map(|g| g.key.clone()).collect();
    if !strictly_sorted(&keys) {
        return Err(rule(
            "golden subsets are not sorted by key, or one is listed twice",
        ));
    }
    let goldens = typed_goldens(repos)?;
    let expected: Vec<String> = goldens.iter().map(GoldenEntry::key).collect();
    if keys != expected {
        return Err(rule(format!(
            "the golden subsets are not one for each typed-tier golden of bench/repos.lock: listed {keys:?}, expected {expected:?}"
        )));
    }
    for (subset, entry) in manifest.golden.iter().zip(&goldens) {
        let at = |m: String| rule(format!("golden {}: {m}", subset.key));
        if subset.commit != entry.commit {
            return Err(at(format!(
                "commit {} is not the pinned {}",
                subset.commit, entry.commit
            )));
        }
        if !SUBSET_FILES.contains(&subset.files.len()) {
            return Err(at(format!(
                "{} files; a subset holds {} to {}",
                subset.files.len(),
                SUBSET_FILES.start(),
                SUBSET_FILES.end()
            )));
        }
        if subset.cluster.trim().is_empty() {
            return Err(at("says nothing of why these files".to_owned()));
        }
        check_list(&subset.files, &[]).map_err(at)?;
    }
    Ok(())
}

fn check_list(files: &[String], support: &[String]) -> Result<(), String> {
    if files.is_empty() {
        return Err("no file".to_owned());
    }
    if !strictly_sorted(files) || !strictly_sorted(support) {
        return Err("files or support files are not sorted, or one is listed twice".to_owned());
    }
    for path in files.iter().chain(support) {
        check_path(path).map_err(|reason| format!("{path}: {reason}"))?;
    }
    if let Some(both) = files.iter().find(|f| support.contains(f)) {
        return Err(format!("{both} is both compared and support"));
    }
    Ok(())
}

/// One file of a corpus as the harness receives it.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct CorpusFile {
    /// Below the corpus root.
    pub path: String,
    /// The engine language the vendored engine is given, and the reference configured
    /// with for the file's extension.
    pub engine_language: String,
}

/// One corpus as the harness receives it.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Corpus {
    /// The fixture's name, or `golden:<key>`.
    pub name: String,
    /// `fixture` or `golden`.
    pub kind: &'static str,
    /// The matrix language.
    pub language: String,
    /// The directory the paths are below.
    pub root: PathBuf,
    /// A golden's pinned commit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// The compared files.
    pub files: Vec<CorpusFile>,
    /// The support files.
    pub support: Vec<String>,
}

/// `root/path`, reached through real directories, a regular file.
fn real_file(root: &Path, path: &str) -> Result<PathBuf, String> {
    let mut at = root.to_path_buf();
    for part in path.split('/') {
        at.push(part);
        let metadata = fs::symlink_metadata(&at).map_err(|_| "does not exist".to_owned())?;
        if metadata.file_type().is_symlink() {
            return Err("passes through a symlink".to_owned());
        }
    }
    if !at.is_file() {
        return Err("is not a regular file".to_owned());
    }
    Ok(at)
}

/// Checks one corpus's files on disk and gives each its engine language: each a
/// regular file reached through real directories, of the corpus's language by the
/// matrix's detection (content and siblings included), each extension of one engine
/// language, and, for a golden subset, a C or C++ file no larger than
/// [`C_FAMILY_MAX_BYTES`].
///
/// # Errors
///
/// [`DifferentialError::File`] for the first file that is not.
pub fn check_files(
    name: &str,
    language: &str,
    root: &Path,
    files: &[String],
    support: &[String],
    golden: bool,
) -> Result<Vec<CorpusFile>, DifferentialError> {
    let bad = |path: &str, reason: String| DifferentialError::File {
        corpus: name.to_owned(),
        path: path.to_owned(),
        reason,
    };
    let mut by_extension: BTreeMap<String, String> = BTreeMap::new();
    let mut out = Vec::with_capacity(files.len());
    for path in files {
        let at = real_file(root, path).map_err(|r| bad(path, r))?;
        let content = fs::read(&at).map_err(|e| bad(path, e.to_string()))?;
        let dir = at.parent().map(Path::to_path_buf).unwrap_or_default();
        let detected = languages::detect(path, &content, |sibling| dir.join(sibling).is_file())
            .ok_or_else(|| bad(path, "is of no language of the matrix".to_owned()))?;
        if detected.id != language {
            return Err(bad(
                path,
                format!("is {} by the matrix, not {language}", detected.id),
            ));
        }
        if golden && matches!(language, "c" | "cpp") && content.len() as u64 > C_FAMILY_MAX_BYTES {
            return Err(bad(
                path,
                format!("is larger than {C_FAMILY_MAX_BYTES} bytes"),
            ));
        }
        let engine = detected.engine_language(path).to_owned();
        let extension = path
            .rsplit('/')
            .next()
            .and_then(|n| n.rfind('.').map(|i| n[i..].to_owned()))
            .ok_or_else(|| bad(path, "has no extension".to_owned()))?;
        if by_extension
            .entry(extension)
            .or_insert_with(|| engine.clone())
            != &engine
        {
            return Err(bad(
                path,
                "shares its extension with a file of another engine language".to_owned(),
            ));
        }
        out.push(CorpusFile {
            path: path.clone(),
            engine_language: engine,
        });
    }
    for path in support {
        real_file(root, path).map_err(|r| bad(path, r))?;
    }
    Ok(out)
}

/// Every file below a fixture directory, as sorted relative paths; a symlink anywhere
/// is refused.
///
/// # Errors
///
/// [`DifferentialError::File`] for a symlink or an unreadable directory.
pub fn fixture_contents(name: &str, dir: &Path) -> Result<Vec<String>, DifferentialError> {
    fn walk(
        name: &str,
        root: &Path,
        dir: &Path,
        out: &mut Vec<String>,
    ) -> Result<(), DifferentialError> {
        let bad = |p: &Path, reason: &str| DifferentialError::File {
            corpus: name.to_owned(),
            path: p.display().to_string(),
            reason: reason.to_owned(),
        };
        for entry in fs::read_dir(dir).map_err(|_| bad(dir, "cannot be read"))? {
            let path = entry.map_err(|_| bad(dir, "cannot be read"))?.path();
            let kind = fs::symlink_metadata(&path)
                .map_err(|_| bad(&path, "cannot be read"))?
                .file_type();
            if kind.is_symlink() {
                return Err(bad(&path, "is a symlink"));
            } else if kind.is_dir() {
                walk(name, root, &path, out)?;
            } else {
                let rel = path
                    .strip_prefix(root)
                    .map_err(|_| bad(&path, "is outside the fixture"))?;
                out.push(
                    rel.components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/"),
                );
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(name, dir, dir, &mut out)?;
    out.sort();
    Ok(out)
}

/// A fixture as the harness receives it, every file of its directory accounted for.
///
/// # Errors
///
/// [`DifferentialError`] for a file unaccounted for or not what it must be.
pub fn fixture_corpus(
    workspace: &Path,
    fixture: &FixtureCorpus,
) -> Result<Corpus, DifferentialError> {
    let root = workspace.join(&fixture.root);
    let present = fixture_contents(&fixture.name, &root)?;
    for path in &present {
        let accounted = fixture.files.contains(path)
            || fixture.support.contains(path)
            || FIXTURE_RECORDS.contains(&path.as_str());
        if !accounted {
            return Err(DifferentialError::File {
                corpus: fixture.name.clone(),
                path: path.clone(),
                reason: "is in the fixture but neither compared nor support".to_owned(),
            });
        }
    }
    let files = check_files(
        &fixture.name,
        &fixture.language,
        &root,
        &fixture.files,
        &fixture.support,
        false,
    )?;
    Ok(Corpus {
        name: fixture.name.clone(),
        kind: "fixture",
        language: fixture.language.clone(),
        root,
        commit: None,
        files,
        support: fixture.support.clone(),
    })
}

/// A golden subset as the harness receives it, from a checkout at its pin.
///
/// # Errors
///
/// [`DifferentialError`] for a file that is not what it must be.
pub fn golden_corpus(
    subset: &GoldenSubset,
    entry: &GoldenEntry,
    checkout: &Path,
) -> Result<Corpus, DifferentialError> {
    let name = format!("golden:{}", subset.key);
    let files = check_files(&name, &entry.language, checkout, &subset.files, &[], true)?;
    Ok(Corpus {
        name,
        kind: "golden",
        language: entry.language.clone(),
        root: checkout.to_path_buf(),
        commit: Some(subset.commit.clone()),
        files,
        support: Vec::new(),
    })
}

/// The directories under [`FIXTURE_ROOT`], sorted.
///
/// # Errors
///
/// When it cannot be read.
pub fn fixture_dirs(workspace: &Path) -> Result<Vec<String>, DifferentialError> {
    let root = workspace.join(FIXTURE_ROOT);
    let mut out = Vec::new();
    for entry in fs::read_dir(&root).map_err(|e| rule(format!("{FIXTURE_ROOT}: {e}")))? {
        let entry = entry.map_err(|e| rule(format!("{FIXTURE_ROOT}: {e}")))?;
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            out.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repos() -> BenchRepos {
        let entry = |language: &str, repo: &str, commit: char| {
            format!(
                "[[golden]]\nlanguage = \"{language}\"\nurl = \"git.example/acme/{repo}\"\n\
                 default_branch = \"main\"\ncommit = \"{}\"\nlicence = \"MIT\"\nfetch_date = \"2026-01-01\"\n",
                commit.to_string().repeat(40)
            )
        };
        let text = format!(
            "lock_version = 1\nfetch_date = \"2026-01-01\"\nscale = []\n{}{}{}",
            entry("ada", "lib", 'a'),
            entry("go", "gin", 'b'),
            entry("python", "flask", 'c'),
        );
        toml::from_str(&text).expect("a lock")
    }

    fn manifest() -> Manifest {
        let fixture = |name: &str, language: &str| FixtureCorpus {
            name: name.to_owned(),
            language: language.to_owned(),
            root: format!("{FIXTURE_ROOT}/{name}"),
            files: vec!["a.x".to_owned(), "b.x".to_owned()],
            support: Vec::new(),
        };
        let mut fixtures: Vec<FixtureCorpus> = typed_languages()
            .iter()
            .map(|l| fixture(&format!("{l}_fixture"), l))
            .collect();
        fixtures.sort_by(|a, b| a.name.cmp(&b.name));
        let subset = |key: &str, commit: char| GoldenSubset {
            key: key.to_owned(),
            commit: commit.to_string().repeat(40),
            cluster: "a caller and its callee".to_owned(),
            files: vec!["a.x".to_owned(), "b.x".to_owned()],
        };
        Manifest {
            format: FORMAT,
            fixture: fixtures,
            golden: vec![subset("go/gin", 'b'), subset("python/flask", 'c')],
        }
    }

    fn dirs(m: &Manifest) -> Vec<String> {
        m.fixture.iter().map(|f| f.name.clone()).collect()
    }

    fn refused(m: &Manifest, d: &[String], needle: &str) {
        match check_manifest(m, &repos(), d) {
            Err(e) => assert!(e.to_string().contains(needle), "{e}"),
            Ok(()) => panic!("accepted; expected {needle}"),
        }
    }

    #[test]
    fn a_complete_manifest_is_accepted() {
        let m = manifest();
        check_manifest(&m, &repos(), &dirs(&m)).expect("accepted");
    }

    #[test]
    fn every_typed_language_needs_a_fixture() {
        let mut m = manifest();
        m.fixture.retain(|f| f.language != "perl");
        let d = dirs(&m);
        refused(&m, &d, "typed-tier language(s) perl");
    }

    #[test]
    fn every_typed_golden_needs_a_subset_and_ada_is_not_one() {
        let mut m = manifest();
        m.golden.remove(0);
        let d = dirs(&m);
        refused(&m, &d, "one for each typed-tier golden");
        let mut m = manifest();
        m.golden.insert(
            0,
            GoldenSubset {
                key: "ada/lib".to_owned(),
                commit: "a".repeat(40),
                cluster: "x".to_owned(),
                files: vec!["a.adb".to_owned(), "b.ads".to_owned()],
            },
        );
        let d = dirs(&m);
        refused(&m, &d, "one for each typed-tier golden");
    }

    #[test]
    fn a_subset_must_be_at_the_lock_s_pin() {
        let mut m = manifest();
        m.golden[0].commit = "f".repeat(40);
        let d = dirs(&m);
        refused(&m, &d, "is not the pinned");
    }

    #[test]
    fn lists_are_sorted_unique_safe_and_bounded() {
        let d = dirs(&manifest());
        let mut m = manifest();
        m.fixture[0].files = vec!["b.x".to_owned(), "a.x".to_owned()];
        refused(&m, &d, "not sorted");
        let mut m = manifest();
        m.fixture[0].files = vec!["a.x".to_owned(), "a.x".to_owned()];
        refused(&m, &d, "not sorted");
        let mut m = manifest();
        m.fixture[0].files = vec!["../a.x".to_owned()];
        refused(&m, &d, "unsafe");
        let mut m = manifest();
        m.fixture[0].support = vec!["a.x".to_owned()];
        refused(&m, &d, "both compared and support");
        let mut m = manifest();
        m.golden[0].files = (0..11).map(|i| format!("f{i:02}.go")).collect();
        refused(&m, &d, "a subset holds 2 to 10");
        let mut m = manifest();
        m.fixture.swap(0, 1);
        refused(&m, &d, "not sorted by name");
    }

    #[test]
    fn every_fixture_directory_is_listed_and_nothing_else() {
        let m = manifest();
        let mut d = dirs(&m);
        d.push("zz_unlisted".to_owned());
        refused(&m, &d, "not every fixture");
        let mut m = manifest();
        m.fixture[0].root = "elsewhere".to_owned();
        let d = dirs(&m);
        refused(&m, &d, "root is not");
    }

    #[test]
    fn a_fixture_language_must_be_typed() {
        let mut m = manifest();
        m.fixture[0].language = "ada".to_owned();
        let d = dirs(&m);
        refused(&m, &d, "not in the typed tier");
    }

    #[test]
    fn files_are_detected_as_the_corpus_language_with_their_engine_dialect() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        fs::create_dir(root.join("src")).expect("mkdir");
        fs::write(root.join("src/a.ts"), "export const a = 1;\n").expect("write");
        fs::write(root.join("src/b.tsx"), "export const b = <div/>;\n").expect("write");
        fs::write(root.join("src/c.py"), "x = 1\n").expect("write");
        let files = check_files(
            "t",
            "typescript",
            root,
            &["src/a.ts".to_owned(), "src/b.tsx".to_owned()],
            &[],
            false,
        )
        .expect("typescript");
        assert_eq!(files[0].engine_language, "typescript");
        assert_eq!(files[1].engine_language, "tsx");
        let wrong = check_files(
            "t",
            "typescript",
            root,
            &["src/c.py".to_owned()],
            &[],
            false,
        );
        assert!(wrong.is_err_and(|e| e.to_string().contains("is python by the matrix")));
        let missing = check_files("t", "python", root, &["src/none.py".to_owned()], &[], false);
        assert!(missing.is_err());
    }

    #[test]
    fn a_header_s_language_is_the_matrix_s() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        fs::write(root.join("plain.h"), "int f(void);\n").expect("write");
        fs::write(root.join("shape.h"), "class Shape {};\n").expect("write");
        assert!(check_files("t", "c", root, &["plain.h".to_owned()], &[], false).is_ok());
        assert!(check_files("t", "cpp", root, &["shape.h".to_owned()], &[], false).is_ok());
        assert!(check_files("t", "cpp", root, &["plain.h".to_owned()], &[], false).is_err());
    }

    #[test]
    fn a_golden_c_family_file_is_at_most_32_kib() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let big = "int x;\n".repeat(5000);
        fs::write(root.join("big.c"), &big).expect("write");
        fs::write(root.join("small.c"), "int y;\n").expect("write");
        assert!(check_files("t", "c", root, &["big.c".to_owned()], &[], false).is_ok());
        let refused = check_files("t", "c", root, &["big.c".to_owned()], &[], true);
        assert!(refused.is_err_and(|e| e.to_string().contains("larger than")));
        assert!(check_files("t", "c", root, &["small.c".to_owned()], &[], true).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_never_followed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("root");
        fs::create_dir(&root).expect("mkdir");
        fs::write(tmp.path().join("outside.py"), "x = 1\n").expect("write");
        std::os::unix::fs::symlink(tmp.path().join("outside.py"), root.join("a.py"))
            .expect("symlink");
        let refused = check_files("t", "python", &root, &["a.py".to_owned()], &[], false);
        assert!(refused.is_err_and(|e| e.to_string().contains("symlink")));
        assert!(fixture_contents("t", &root).is_err());
    }
}
