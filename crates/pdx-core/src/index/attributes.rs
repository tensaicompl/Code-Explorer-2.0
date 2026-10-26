//! Which files the repository marks generated or vendored (P2-18, issue 132, decision
//! 85), read from its `.gitattributes` files as Git reads them.
//!
//! Only two attributes matter here, `linguist-generated` and `linguist-vendored`: set
//! (`attr`, `attr=true`), unset (`-attr`, `attr=false`) or unspecified (`!attr`, or no
//! line). The rules are Git's (`gitattributes(5)`):
//!
//! - A pattern is matched relative to the directory of the `.gitattributes` it is in. One
//!   without a `/` (a trailing one aside) matches a file's name at any depth below that
//!   directory; one with a `/` is anchored there (a leading `/` only anchors). `*` never
//!   crosses a `/`; `**` does, as in ignore files.
//! - A pattern that matches a directory does not apply to the files in it, so `vendor/`
//!   (a directory only) marks nothing: `vendor/**` does.
//! - Lines are read in order and a later line wins; a deeper `.gitattributes` wins over a
//!   shallower one.
//!
//! Only the checkout's own `.gitattributes` files are read, never `$GIT_DIR/info/attributes`
//! or a global attributes file: discovery depends on the checkout alone, as it reads
//! ignore files. Negated patterns (`!pattern`) are not allowed in attribute files and are
//! ignored, as Git ignores them; so is any line this reading cannot use. A file that
//! cannot be read adds no rule: discovery never stops on an attribute file.

use std::fs;
use std::path::Path;

use globset::{GlobBuilder, GlobMatcher};

use crate::consts::MAX_FILE_BYTES;

/// What the repository says of one file's provenance: `Some(true)` an attribute set,
/// `Some(false)` unset, `None` unspecified (no line names it, or one resets it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Provenance {
    /// `linguist-generated`.
    pub generated: Option<bool>,
    /// `linguist-vendored`.
    pub vendored: Option<bool>,
}

/// An attribute's state on a line: `attr` or `attr=true`, `-attr` or `attr=false`, and
/// `!attr` or any other value, which leave it unspecified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Set,
    Unset,
    Unspecified,
}

/// One line's patterns and the state it gives each attribute it names (`None`: the
/// line does not name it).
struct Rule {
    matcher: GlobMatcher,
    generated: Option<State>,
    vendored: Option<State>,
}

/// A repository's attribute rules, in precedence order (later wins).
#[derive(Default)]
pub struct Attributes {
    rules: Vec<Rule>,
}

impl Attributes {
    /// The rules of the `.gitattributes` files among `paths` (repository-relative, `/`
    /// separated) below `root`, read in precedence order: shallower first, then by path.
    pub fn read(root: &Path, paths: &[&str]) -> Self {
        let mut files: Vec<&str> = paths
            .iter()
            .copied()
            .filter(|p| p.rsplit('/').next() == Some(".gitattributes"))
            .collect();
        files.sort_by(|a, b| (a.matches('/').count(), *a).cmp(&(b.matches('/').count(), *b)));
        let mut attributes = Self::default();
        for path in files {
            let dir = path.strip_suffix(".gitattributes").unwrap_or("");
            let dir = dir.strip_suffix('/').unwrap_or(dir);
            if let Some(text) = read_small(&root.join(path)) {
                attributes.add(dir, &text);
            }
        }
        attributes
    }

    /// Adds the rules of an attributes file in directory `dir` ("" for the root).
    pub fn add(&mut self, dir: &str, text: &str) {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut fields = line.split_whitespace();
            let Some(pattern) = fields.next() else {
                continue;
            };
            if pattern.starts_with('!') || pattern.starts_with('"') {
                continue; // negated: not allowed; quoted: not read here
            }
            let mut generated = None;
            let mut vendored = None;
            for field in fields {
                match state_of(field) {
                    ("linguist-generated", state) => generated = Some(state),
                    ("linguist-vendored", state) => vendored = Some(state),
                    _ => {}
                }
            }
            if generated.is_none() && vendored.is_none() {
                continue;
            }
            if let Some(matcher) = matcher_of(dir, pattern) {
                self.rules.push(Rule {
                    matcher,
                    generated,
                    vendored,
                });
            }
        }
    }

    /// What the rules say of the file at `path` (repository-relative, `/` separated).
    pub fn of(&self, path: &str) -> Provenance {
        let mut generated = State::Unspecified;
        let mut vendored = State::Unspecified;
        for rule in &self.rules {
            if rule.matcher.is_match(path) {
                generated = rule.generated.unwrap_or(generated);
                vendored = rule.vendored.unwrap_or(vendored);
            }
        }
        let said = |state: State| match state {
            State::Set => Some(true),
            State::Unset => Some(false),
            State::Unspecified => None,
        };
        Provenance {
            generated: said(generated),
            vendored: said(vendored),
        }
    }

    /// Whether no rule names either attribute.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

/// An attribute field's name and the state it gives it.
fn state_of(field: &str) -> (&str, State) {
    if let Some(name) = field.strip_prefix('-') {
        return (name, State::Unset);
    }
    if let Some(name) = field.strip_prefix('!') {
        return (name, State::Unspecified);
    }
    match field.split_once('=') {
        Some((name, "true")) => (name, State::Set),
        Some((name, "false")) => (name, State::Unset),
        Some((name, _)) => (name, State::Unspecified),
        None => (field, State::Set),
    }
}

/// The matcher of `pattern`, written in an attributes file in directory `dir`.
fn matcher_of(dir: &str, pattern: &str) -> Option<GlobMatcher> {
    if pattern.ends_with('/') {
        return None; // a directory only, and a directory's attributes are not its files'
    }
    let (anchored, pattern) = match pattern.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (pattern.contains('/'), pattern),
    };
    if pattern.is_empty() {
        return None;
    }
    let glob = match (dir.is_empty(), anchored) {
        (true, true) => pattern.to_owned(),
        (true, false) => format!("**/{pattern}"),
        (false, true) => format!("{dir}/{pattern}"),
        (false, false) => format!("{dir}/**/{pattern}"),
    };
    GlobBuilder::new(&glob)
        .literal_separator(true)
        .build()
        .ok()
        .map(|g| g.compile_matcher())
}

/// A small regular file's text, or `None`: an attributes file that cannot be read, is
/// not UTF-8 or is larger than any file discovery reads adds no rule.
fn read_small(path: &Path) -> Option<String> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(files: &[(&str, &str)]) -> Attributes {
        let mut a = Attributes::default();
        for (dir, text) in files {
            a.add(dir, text);
        }
        a
    }

    fn generated(a: &Attributes, path: &str) -> bool {
        a.of(path).generated == Some(true)
    }

    #[test]
    fn a_pattern_without_a_slash_matches_a_name_at_any_depth() {
        let a = rules(&[("", "*.pb.go linguist-generated\n")]);
        assert!(generated(&a, "api.pb.go"));
        assert!(generated(&a, "pkg/deep/api.pb.go"));
        assert!(!generated(&a, "pkg/api.go"));
    }

    #[test]
    fn a_pattern_with_a_slash_is_anchored_to_its_file() {
        let a = rules(&[("engine", "grammars/**/parser.c linguist-generated=true\n")]);
        assert!(generated(&a, "engine/grammars/markdown/parser.c"));
        assert!(!generated(&a, "grammars/markdown/parser.c"));
        assert!(!generated(&a, "engine/src/grammars/x/parser.c"));
        let b = rules(&[("", "/top.c linguist-generated\n")]);
        assert!(generated(&b, "top.c"));
        assert!(!generated(&b, "sub/top.c"));
    }

    #[test]
    fn a_star_does_not_cross_a_slash_and_a_directory_marks_nothing() {
        let a = rules(&[(
            "",
            "api/*.json linguist-generated\nvendor/ linguist-vendored\n",
        )]);
        assert!(generated(&a, "api/spec.json"));
        assert!(!generated(&a, "api/v1/spec.json"));
        assert_eq!(a.of("vendor/x.go").vendored, None);
        assert_eq!(
            a.of("vendor").vendored,
            None,
            "a directory pattern names no file"
        );
        let b = rules(&[("", "vendor/** linguist-vendored\n")]);
        assert_eq!(b.of("vendor/a/x.go").vendored, Some(true));
    }

    #[test]
    fn later_lines_and_deeper_files_win() {
        let a = rules(&[
            (
                "",
                "*.go linguist-generated\n*_test.go -linguist-generated\n",
            ),
            ("pkg", "keep.go linguist-generated=false\n"),
        ]);
        assert!(generated(&a, "main.go"));
        assert!(!generated(&a, "main_test.go"));
        assert!(!generated(&a, "pkg/keep.go"));
        assert!(generated(&a, "pkg/other.go"));
        let reset = rules(&[("", "*.go linguist-generated\nx.go !linguist-generated\n")]);
        assert_eq!(reset.of("x.go").generated, None, "reset: unspecified");
        assert_eq!(a.of("main_test.go").generated, Some(false), "unset is said");
    }

    #[test]
    fn other_lines_add_nothing() {
        let a = rules(&[(
            "",
            "# comment\n\n* text=auto eol=lf\n!*.go linguist-generated\n\"q.go\" linguist-generated\n",
        )]);
        assert!(a.is_empty());
    }

    #[test]
    fn the_checkout_s_own_attribute_files_are_read_in_precedence_order() {
        let dir = tempfile::tempdir().expect("a directory");
        fs::write(
            dir.path().join(".gitattributes"),
            "*.c linguist-generated\n",
        )
        .expect("root");
        fs::create_dir_all(dir.path().join("src")).expect("src");
        fs::write(
            dir.path().join("src/.gitattributes"),
            "*.c -linguist-generated\n",
        )
        .expect("src");
        // A local Git setting is no part of the checkout: never read.
        fs::create_dir_all(dir.path().join(".git/info")).expect("info");
        fs::write(
            dir.path().join(".git/info/attributes"),
            "src/main.c linguist-generated\n",
        )
        .expect("info");
        let a = Attributes::read(dir.path(), &["src/.gitattributes", ".gitattributes", "a.c"]);
        assert!(generated(&a, "a.c"));
        assert!(!generated(&a, "src/lib.c"));
        assert!(!generated(&a, "src/main.c"));
    }
}
