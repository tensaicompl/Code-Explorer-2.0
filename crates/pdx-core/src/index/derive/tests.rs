//! Tests (Appendix A's test rules, Appendix B.5): which files are test files, which
//! definitions are tests, and the `TESTS` edges from a test to what it calls.
//!
//! A file is a test file by its language's rules, read as issue 31 decided: a
//! `.test`/`.spec` stem on any of the file's own language's extensions, a file-name
//! prefix and suffix, or a directory anywhere in its path (`tests/**` matches
//! `packages/api/tests/a.py`; `src/test` needs the two names consecutive; `test*` any
//! directory name beginning `test`, case-sensitively).
//!
//! A definition is a test only on evidence about the definition itself, the way its
//! language's test framework decides: a `JUnit` or `TestNG` annotation; an `xUnit`, `NUnit` or
//! `MSTest` attribute; Rust's `#[test]`; pytest's and unittest's `test` prefix in a test
//! file, at the top of the module or in a class that is a test class; Go's `TestXxx`
//! in a `_test.go` file; `PHPUnit`'s `test` prefix or `@test` in a test file; a `GoogleTest`
//! macro's definition. The engine marks every definition of a test file a test, helpers
//! included, so its flag is the file's context and never makes a helper a test. A
//! language whose tests are anonymous (JavaScript's and TypeScript's `it(...)`) or have
//! no per-definition convention here (C, Perl, Ada) has no `Test` node.

use std::collections::BTreeSet;

use pdx_engine::{Definition, DefinitionKind};

use crate::kinds::EdgeKind;
use crate::languages::{DirectoryPattern, Language, TestRule};
use crate::model::Edge;
use crate::resolve::registry::{DefinitionRef, SymbolRegistry};

use super::calls::DrawnCall;
use super::{Builder, DeriveError};

/// Whether a file of `language` at `path` is a test file by the language's rules.
pub fn is_test_path(language: &Language, path: &str) -> bool {
    let mut parts: Vec<&str> = path.split('/').collect();
    let Some(name) = parts.pop() else {
        return false;
    };
    language.test_rules().iter().any(|rule| match rule {
        TestRule::SourceSuffix(stem) => language.extensions.iter().any(|ext| {
            name.len() > stem.len() + ext.len()
                && name.ends_with(ext)
                && name[..name.len() - ext.len()].ends_with(stem)
        }),
        TestRule::FileName { prefix, suffix } => {
            name.len() >= prefix.len() + suffix.len()
                && name.starts_with(prefix)
                && name.ends_with(suffix)
        }
        TestRule::Under(DirectoryPattern::Path(dir)) => {
            let wanted: Vec<&str> = dir.split('/').collect();
            parts.windows(wanted.len()).any(|w| w == wanted.as_slice())
        }
        TestRule::Under(DirectoryPattern::NamePrefix(prefix)) => {
            parts.iter().any(|p| p.starts_with(prefix))
        }
        TestRule::Annotations(_) | TestRule::Attribute(_) | TestRule::Macros(_) => false,
    })
}

/// A decorator, annotation or attribute's name, without its sigil, brackets,
/// arguments or package: `@org.junit.Test` and `@Test()` are `Test`, `[Fact]` and
/// `Fact(Skip = "x")` are `Fact`, `#[tokio::test]` is `test`.
fn annotation_name(text: &str) -> &str {
    let text = text.trim();
    let text = text.strip_prefix("#[").unwrap_or(text);
    let text = text.trim_start_matches(['@', '[']);
    let end = text.find(['(', ']', ' ']).unwrap_or(text.len());
    let full = &text[..end];
    let after_dot = full.rsplit('.').next().unwrap_or(full);
    after_dot.rsplit("::").next().unwrap_or(after_dot)
}

fn has_annotation(definition: &Definition, names: &[&str]) -> bool {
    definition
        .decorators
        .iter()
        .any(|d| names.contains(&annotation_name(d)))
}

const JUNIT: [&str; 5] = [
    "Test",
    "ParameterizedTest",
    "RepeatedTest",
    "TestFactory",
    "TestTemplate",
];
const DOTNET: [&str; 7] = [
    "Fact",
    "Theory",
    "Test",
    "TestMethod",
    "DataTestMethod",
    "TestCase",
    "TestCaseSource",
];
const GOOGLE_TEST: [&str; 4] = ["TEST_", "TEST_F_", "TEST_P_", "TYPED_TEST_"];

/// Whether a definition is a test, on evidence about the definition itself.
fn is_test(
    registry: &SymbolRegistry,
    language: &Language,
    path: &str,
    r: &DefinitionRef,
    definition: &Definition,
) -> bool {
    if !matches!(
        definition.kind,
        DefinitionKind::Function | DefinitionKind::Method
    ) {
        return false;
    }
    let name = definition.name.as_str();
    let test_file = is_test_path(language, path);
    let declaring = registry
        .declaring_type(r)
        .and_then(|t| registry.definition(&t));
    match language.id {
        "java" | "kotlin" | "scala" | "groovy" => has_annotation(definition, &JUNIT),
        "csharp" => has_annotation(definition, &DOTNET),
        "rust" => has_annotation(definition, &["test"]),
        // pytest: `test` functions at the top of a test module, and `test` methods of a
        // `Test` class; unittest: `test` methods of a `TestCase`.
        "python" => {
            test_file
                && name.starts_with("test")
                && declaring.is_none_or(|t| {
                    t.name.starts_with("Test")
                        || t.base_classes.iter().any(|b| b.ends_with("TestCase"))
                })
        }
        // `go test`: `TestXxx` at the top of a `_test.go` file, `Xxx` not lower case.
        "go" => {
            path.ends_with("_test.go")
                && declaring.is_none()
                && name
                    .strip_prefix("Test")
                    .is_some_and(|rest| !rest.starts_with(|c: char| c.is_lowercase()))
        }
        // PHPUnit: `test` methods, or methods documented `@test`, in a test file.
        "php" => {
            test_file
                && declaring.is_some()
                && (name.starts_with("test")
                    || definition
                        .doc
                        .as_deref()
                        .is_some_and(|d| d.contains("@test")))
        }
        // GoogleTest: the definitions its `TEST` macros expand to.
        "cpp" => GOOGLE_TEST.iter().any(|p| name.starts_with(p)),
        _ => false,
    }
}

/// Every definition of the repository that is a test.
pub(crate) fn test_definitions(registry: &SymbolRegistry) -> BTreeSet<DefinitionRef> {
    let mut out = BTreeSet::new();
    for path in registry.files() {
        let (Some(language), Some(extract)) = (registry.language(path), registry.extract(path))
        else {
            continue;
        };
        for (index, definition) in extract.definitions.iter().enumerate() {
            let Ok(index) = u32::try_from(index) else {
                continue;
            };
            let r = DefinitionRef {
                path: path.to_owned(),
                index,
            };
            if is_test(registry, language, path, &r, definition) {
                out.insert(r);
            }
        }
    }
    out
}

/// A `TESTS` edge beside every drawn call a test makes: the same target, band and
/// site. The `CALLS` edge stays; `TESTS` adds what the call means.
pub(crate) fn link(
    graph: &mut Builder,
    tests: &BTreeSet<DefinitionRef>,
    drawn: &[DrawnCall],
) -> Result<(), DeriveError> {
    for call in drawn {
        let Some(caller) = call.caller.as_ref().filter(|c| tests.contains(*c)) else {
            continue;
        };
        let test = graph.definition_node(caller)?.clone();
        let mut edge = Edge::new(
            test,
            call.edge.dst.clone(),
            EdgeKind::Tests,
            call.edge.band,
            call.edge.site_id.clone(),
        );
        edge.engine_score = call.edge.engine_score;
        edge.engine_strategy.clone_from(&call.edge.engine_strategy);
        edge.engine_candidates = call.edge.engine_candidates;
        graph.add_edge(edge)?;
    }
    Ok(())
}
