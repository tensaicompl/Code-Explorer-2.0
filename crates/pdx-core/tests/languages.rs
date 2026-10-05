//! The language matrix against the specification's Appendix A.
//!
//! Appendix A is transcribed here a second time, by hand, so the registry is checked
//! against the specification rather than against itself: `ROWS` is the table as
//! Appendix A writes it, its file patterns in Appendix A's own notation. Detection
//! is exercised for every one of those patterns, and the engine is asked about every
//! language the matrix declares typed.

use pdx_core::consts::LANGUAGE_MATRIX_VERSION;
use pdx_core::languages::{
    self, DeclarationKeyword, DirectoryPattern, ENGINE_DIALECTS, HEADER_RULE, Language, ModuleRule,
    TestDetection, TestFramework, TestRule, Tier,
};
use pdx_engine::Engine;

/// One row of Appendix A.
struct Row {
    id: &'static str,
    tier: Tier,
    /// The Extensions column, entry by entry, as Appendix A writes it. An entry with a
    /// `*` is a pattern over the file's name; any other is an extension the name ends
    /// with. Appendix A writes bash's shebang in this column too; it is `shebangs` here.
    files: &'static [&'static str],
    /// The interpreters a `#!` line may name.
    shebangs: &'static [&'static str],
    module: ModuleRule,
    tests: TestDetection,
}

use DirectoryPattern::{NamePrefix, Path};
use ModuleRule as M;
use TestRule as T;
use Tier::{Structural, Typed};

const fn name(prefix: &'static str, suffix: &'static str) -> TestRule {
    T::FileName { prefix, suffix }
}

const NONE: TestDetection = TestDetection::Rules(&[]);

/// Appendix A, in its order, with its test rules read as issue 31 decided
/// (`LANGUAGE_MATRIX_VERSION` = 2).
const ROWS: &[Row] = &[
    Row {
        id: "java",
        tier: Typed,
        files: &[".java"],
        shebangs: &[],
        module: M::Declaration(DeclarationKeyword::Package),
        tests: TestDetection::Rules(&[
            T::Annotations(TestFramework::JUnit),
            T::Annotations(TestFramework::TestNG),
            T::Under(Path("src/test")),
            name("", "Test.java"),
        ]),
    },
    Row {
        id: "kotlin",
        tier: Typed,
        files: &[".kt", ".kts"],
        shebangs: &[],
        module: M::Declaration(DeclarationKeyword::Package),
        tests: TestDetection::Rules(&[T::Under(Path("src/test")), name("", "Test.kt")]),
    },
    Row {
        id: "scala",
        tier: Structural,
        files: &[".scala"],
        shebangs: &[],
        module: M::Declaration(DeclarationKeyword::Package),
        tests: TestDetection::Rules(&[T::Under(Path("src/test"))]),
    },
    Row {
        id: "typescript",
        tier: Typed,
        files: &[".ts", ".tsx", ".mts", ".cts"],
        shebangs: &[],
        module: M::DirectoryWithPathMappings { config: "tsconfig" },
        tests: TestDetection::Rules(&[
            T::SourceSuffix(".test"),
            T::SourceSuffix(".spec"),
            T::Under(Path("__tests__")),
        ]),
    },
    Row {
        id: "javascript",
        tier: Typed,
        files: &[".js", ".jsx", ".mjs", ".cjs"],
        shebangs: &[],
        module: M::Directory,
        tests: TestDetection::SameAs("typescript"),
    },
    Row {
        id: "python",
        tier: Typed,
        files: &[".py", ".pyi"],
        shebangs: &[],
        module: M::DottedPath {
            package_marker: "__init__.py",
        },
        tests: TestDetection::Rules(&[
            name("test_", ".py"),
            name("", "_test.py"),
            T::Under(Path("tests")),
        ]),
    },
    Row {
        id: "go",
        tier: Typed,
        files: &[".go"],
        shebangs: &[],
        module: M::DirectoryUnderModulePath { manifest: "go.mod" },
        tests: TestDetection::Rules(&[name("", "_test.go")]),
    },
    Row {
        id: "c",
        tier: Typed,
        files: &[".c", ".h"],
        shebangs: &[],
        module: M::DirectoryPairingHeaders,
        tests: TestDetection::Rules(&[T::Under(NamePrefix("test")), name("", "_test.c")]),
    },
    Row {
        id: "cpp",
        tier: Typed,
        files: &[".cpp", ".cc", ".cxx", ".hpp", ".hh", ".hxx", ".h"],
        shebangs: &[],
        module: M::Declaration(DeclarationKeyword::Namespace),
        tests: TestDetection::Rules(&[
            T::Under(NamePrefix("test")),
            T::Macros(TestFramework::GoogleTest),
        ]),
    },
    Row {
        id: "csharp",
        tier: Typed,
        files: &[".cs"],
        shebangs: &[],
        module: M::Declaration(DeclarationKeyword::Namespace),
        tests: TestDetection::Rules(&[
            name("", "Tests.cs"),
            T::Attribute("Fact"),
            T::Attribute("Test"),
        ]),
    },
    Row {
        id: "rust",
        tier: Typed,
        files: &[".rs"],
        shebangs: &[],
        module: M::ModTree {
            crate_roots: &["lib.rs", "main.rs"],
        },
        tests: TestDetection::Rules(&[T::Attribute("test"), T::Under(Path("tests"))]),
    },
    Row {
        id: "php",
        tier: Typed,
        files: &[".php"],
        shebangs: &[],
        module: M::Declaration(DeclarationKeyword::Namespace),
        tests: TestDetection::Rules(&[name("", "Test.php")]),
    },
    Row {
        id: "perl",
        tier: Typed,
        files: &[".pl", ".pm"],
        shebangs: &[],
        module: M::Declaration(DeclarationKeyword::Package),
        tests: TestDetection::Rules(&[T::Under(Path("t"))]),
    },
    Row {
        id: "ada",
        tier: Structural,
        files: &[".ads", ".adb", ".ada"],
        shebangs: &[],
        module: M::UnitName,
        tests: TestDetection::Rules(&[T::Under(NamePrefix("test"))]),
    },
    Row {
        id: "bash",
        tier: Structural,
        files: &[".sh", ".bash"],
        shebangs: &["bash"],
        module: M::File,
        tests: NONE,
    },
    Row {
        id: "ruby",
        tier: Structural,
        files: &[".rb"],
        shebangs: &[],
        module: M::ModuleClassNesting,
        tests: TestDetection::Rules(&[T::Under(Path("spec")), T::Under(Path("test"))]),
    },
    Row {
        id: "swift",
        tier: Structural,
        files: &[".swift"],
        shebangs: &[],
        module: M::Directory,
        tests: TestDetection::Rules(&[name("", "Tests.swift")]),
    },
    Row {
        id: "objc",
        tier: Structural,
        files: &[".m", ".mm"],
        shebangs: &[],
        module: M::Directory,
        tests: NONE,
    },
    Row {
        id: "groovy",
        tier: Structural,
        files: &[".groovy", ".gradle"],
        shebangs: &[],
        module: M::Declaration(DeclarationKeyword::Package),
        tests: NONE,
    },
    Row {
        id: "lua",
        tier: Structural,
        files: &[".lua"],
        shebangs: &[],
        module: M::File,
        tests: NONE,
    },
    Row {
        id: "sql",
        tier: Structural,
        files: &[".sql"],
        shebangs: &[],
        module: M::File,
        tests: NONE,
    },
    Row {
        id: "protobuf",
        tier: Structural,
        files: &[".proto"],
        shebangs: &[],
        module: M::Declaration(DeclarationKeyword::Package),
        tests: NONE,
    },
    Row {
        id: "graphql",
        tier: Structural,
        files: &[".graphql", ".gql"],
        shebangs: &[],
        module: M::File,
        tests: NONE,
    },
    Row {
        id: "yaml",
        tier: Structural,
        files: &[".yaml", ".yml"],
        shebangs: &[],
        module: M::File,
        tests: NONE,
    },
    Row {
        id: "json",
        tier: Structural,
        files: &[".json"],
        shebangs: &[],
        module: M::File,
        tests: NONE,
    },
    Row {
        id: "toml",
        tier: Structural,
        files: &[".toml"],
        shebangs: &[],
        module: M::File,
        tests: NONE,
    },
    Row {
        id: "hcl",
        tier: Structural,
        files: &[".tf", ".hcl"],
        shebangs: &[],
        module: M::File,
        tests: NONE,
    },
    Row {
        id: "dockerfile",
        tier: Structural,
        files: &["Dockerfile*", "*.dockerfile"],
        shebangs: &[],
        module: M::File,
        tests: NONE,
    },
    Row {
        id: "markdown",
        tier: Structural,
        files: &[".md"],
        shebangs: &[],
        module: M::FileSectionsAsDocs,
        tests: NONE,
    },
    Row {
        id: "xml",
        tier: Structural,
        files: &[".xml", ".pom"],
        shebangs: &[],
        module: M::File,
        tests: NONE,
    },
    Row {
        id: "properties",
        tier: Structural,
        files: &[".properties", ".env*"],
        shebangs: &[],
        module: M::FileKeysOnly,
        tests: NONE,
    },
];

/// The language a file gets, with no shebang and no sibling.
fn by_name(path: &str) -> Option<&'static str> {
    languages::detect(path, b"", |_| false).map(|l| l.id)
}

/// The extensions and name prefixes an Appendix A entry stands for.
fn split(files: &[&'static str]) -> (Vec<&'static str>, Vec<&'static str>) {
    let mut extensions = Vec::new();
    let mut prefixes = Vec::new();
    for entry in files {
        if let Some(suffix) = entry.strip_prefix('*') {
            extensions.push(suffix);
        } else if let Some(prefix) = entry.strip_suffix('*') {
            prefixes.push(prefix);
        } else {
            extensions.push(*entry);
        }
    }
    (extensions, prefixes)
}

/// A file name that exercises one Appendix A entry, in a directory, so detection
/// never sees a bare name only.
fn exercise(entry: &str) -> Vec<String> {
    if let Some(suffix) = entry.strip_prefix('*') {
        vec![
            format!("build/api{suffix}"),
            format!("build/api.v2{suffix}"),
        ]
    } else if let Some(prefix) = entry.strip_suffix('*') {
        vec![
            format!("deploy/{prefix}"),
            format!("deploy/{prefix}.local"),
            format!("{prefix}-prod"),
        ]
    } else {
        vec![
            format!("src/pkg/inventory{entry}"),
            format!("src/pkg/inventory.min{entry}"),
        ]
    }
}

#[test]
fn language_detection() {
    let mut cases = 0;
    for row in ROWS {
        for entry in row.files {
            // `.h` is both languages'; the header rule decides, tested on its own.
            if *entry == ".h" {
                continue;
            }
            for file in exercise(entry) {
                assert_eq!(
                    by_name(&file),
                    Some(row.id),
                    "{entry} ({file}) expected {}",
                    row.id
                );
                cases += 1;
            }
        }
    }

    // A header with no evidence of C++ is C; any of the three kinds of evidence makes it
    // C++, and nothing about it matters for any other extension.
    let header = "include/buffer.h";
    assert_eq!(by_name(header), Some("c"));
    for sibling in ["buffer.cpp", "buffer.cc", "buffer.cxx"] {
        let found = languages::detect(header, b"int f(void);\n", |n| n == sibling);
        assert_eq!(found.map(|l| l.id), Some("cpp"), ".h beside {sibling}");
    }
    for marker in [
        "class Buffer;",
        "namespace io {",
        "template<typename T> T f();",
    ] {
        let found = languages::detect(header, marker.as_bytes(), |_| false);
        assert_eq!(found.map(|l| l.id), Some("cpp"), ".h containing {marker:?}");
    }
    for unrelated in ["buffer.c", "buffer.hpp", "other.cpp", "buffer.CPP"] {
        let found = languages::detect(header, b"struct buffer;\n", |n| n == unrelated);
        assert_eq!(found.map(|l| l.id), Some("c"), ".h beside only {unrelated}");
    }
    // The rule's markers are exact: `template <` with a space is not one of them.
    assert_eq!(
        languages::detect(header, b"template <int N>", |_| false).map(|l| l.id),
        Some("c")
    );
    assert_eq!(
        languages::detect("lib.cpp", b"", |_| false).map(|l| l.id),
        Some("cpp")
    );
    cases += 13;

    // A name with several dots ends in the extension that counts.
    for (file, id) in [
        ("web/app.test.ts", "typescript"),
        ("types/index.d.ts", "typescript"),
        ("vendor/jquery.min.js", "javascript"),
        ("archive.tar.json", "json"),
    ] {
        assert_eq!(by_name(file), Some(id), "{file}");
        cases += 1;
    }

    // An extension outranks a name pattern: these are the extension's language.
    for (file, id) in [
        ("Dockerfile.md", "markdown"),
        (".env.json", "json"),
        (".env.sh", "bash"),
    ] {
        assert_eq!(by_name(file), Some(id), "{file}");
        cases += 1;
    }

    // Case is as Appendix A writes it, and a name Appendix A does not cover is no
    // language.
    for file in [
        "Main.JAVA",
        "README.MD",
        "dockerfile",
        "DOCKERFILE",
        "app.Dockerfile",
        "Makefile",
        "notes.txt",
        "image.png",
        "src/",
        "",
        ".java.bak",
        "env",
    ] {
        assert_eq!(by_name(file), None, "{file:?} has no language");
        cases += 1;
    }
    assert!(cases > 100, "{cases} cases");
}

#[test]
fn shebang_detection() {
    // Every form of the one shebang Appendix A names: bash's, with the interpreter
    // named directly or through `env`.
    for first_line in [
        "#!/bin/bash",
        "#!/usr/bin/bash",
        "#!/usr/local/bin/bash",
        "#!bash",
        "#! /bin/bash",
        "#!\t/bin/bash",
        "#!/bin/bash -eu",
        "#!/bin/bash\r",
        "#!/usr/bin/env bash",
        "#!/usr/bin/env  bash -e",
        "#!/usr/bin/env -S bash -eu",
        "#!/usr/bin/env LC_ALL=C bash",
        "#!/usr/bin/env -i PATH=/bin bash",
    ] {
        let content = format!("{first_line}\necho hello\n");
        let found = languages::detect("scripts/deploy", content.as_bytes(), |_| false);
        assert_eq!(found.map(|l| l.id), Some("bash"), "{first_line:?}");
    }

    // Anything else names no language: other interpreters, Appendix A's other
    // languages included, and lines that are not a shebang.
    for content in [
        &b"#!/bin/sh\n"[..],
        b"#!/usr/bin/env python3\n",
        b"#!/usr/bin/env node\n",
        b"#!/usr/bin/perl\n",
        b"#!/bin/bashful\n",
        b"#!/bin/BASH\n",
        b"#!/usr/bin/env\n",
        b"#!\n",
        b"#!",
        b"# !/bin/bash\n",
        b" #!/bin/bash\n",
        b"\xef\xbb\xbf#!/bin/bash\n",
        b"echo hello\n#!/bin/bash\n",
        b"",
    ] {
        let found = languages::detect("scripts/run", content, |_| false);
        assert_eq!(
            found.map(|l| l.id),
            None,
            "{:?}",
            String::from_utf8_lossy(content)
        );
    }

    // Only the first line is read, as bytes: what follows need not be text.
    let mut binary_tail = b"#!/bin/bash\n".to_vec();
    binary_tail.extend_from_slice(&[0xff, 0xfe, 0x00, 0x80]);
    assert_eq!(
        languages::detect("run", &binary_tail, |_| false).map(|l| l.id),
        Some("bash")
    );

    // The extension comes first: a shebang never overrides it.
    let bash = b"#!/usr/bin/env bash\n";
    assert_eq!(
        languages::detect("tool.py", bash, |_| false).map(|l| l.id),
        Some("python")
    );
    assert_eq!(
        languages::detect("Dockerfile", bash, |_| false).map(|l| l.id),
        Some("dockerfile")
    );
    // An unknown extension with a known shebang is the shebang's language.
    assert_eq!(
        languages::detect("tool.cgi", bash, |_| false).map(|l| l.id),
        Some("bash")
    );
}

#[test]
fn matrix_is_appendix_a() {
    let all = languages::all();
    let ids: Vec<&str> = all.iter().map(|l| l.id).collect();
    let expected: Vec<&str> = ROWS.iter().map(|r| r.id).collect();
    assert_eq!(
        ids, expected,
        "the matrix's languages, in Appendix A's order"
    );
    // Appendix A: "Grammars vendored: exactly the ids above (31)".
    assert_eq!(all.len(), 31);

    for (language, row) in all.iter().zip(ROWS) {
        let id = row.id;
        assert_eq!(language.tier, row.tier, "{id}: tier");
        let (extensions, prefixes) = split(row.files);
        assert_eq!(
            language.extensions,
            extensions.as_slice(),
            "{id}: extensions"
        );
        assert_eq!(
            language.name_prefixes,
            prefixes.as_slice(),
            "{id}: name patterns"
        );
        assert_eq!(language.shebangs, row.shebangs, "{id}: shebangs");
        assert_eq!(language.module_rule, row.module, "{id}: module rule");
        assert_eq!(language.test_detection, row.tests, "{id}: test rule");
    }
}

#[test]
fn ids_are_unique_and_canonical() {
    let all = languages::all();
    for (i, language) in all.iter().enumerate() {
        assert!(!language.id.is_empty());
        assert!(
            language.id.bytes().all(|b| b.is_ascii_lowercase()),
            "{}",
            language.id
        );
        assert!(
            all[..i].iter().all(|other| other.id != language.id),
            "{} twice",
            language.id
        );
        assert!(!language.extensions.is_empty() || !language.name_prefixes.is_empty());
        assert_eq!(languages::by_id(language.id), Some(language));
    }
    // An engine grammar is not a matrix language, and ids are exact.
    for id in [
        "tsx", "Java", "JAVA", " java", "java ", "", "sh", "c++", "unknown",
    ] {
        assert_eq!(languages::by_id(id), None, "{id:?}");
    }
}

#[test]
fn every_pattern_names_one_language() {
    let all = languages::all();
    let mut extensions: Vec<(&str, &str)> = Vec::new();
    for language in all {
        for extension in language.extensions {
            assert!(
                extension.starts_with('.') && extension.len() > 1,
                "{}: {extension}",
                language.id
            );
            extensions.push((extension, language.id));
        }
    }
    for (i, (extension, id)) in extensions.iter().enumerate() {
        for (other, other_id) in &extensions[..i] {
            if extension == other {
                // The one extension two languages share, decided by the header rule.
                assert_eq!(
                    *extension, HEADER_RULE.extension,
                    "{extension}: {id} and {other_id}"
                );
                let mut pair = [*id, *other_id];
                pair.sort_unstable();
                assert_eq!(pair, ["c", "cpp"]);
            } else {
                // No extension ends with another, so no file has two that compete.
                assert!(
                    !extension.ends_with(other) && !other.ends_with(extension),
                    "{extension} {other}"
                );
            }
        }
    }

    let prefixes: Vec<&str> = all
        .iter()
        .flat_map(|l| l.name_prefixes.iter().copied())
        .collect();
    for (i, prefix) in prefixes.iter().enumerate() {
        for other in &prefixes[..i] {
            assert!(
                !prefix.starts_with(other) && !other.starts_with(prefix),
                "{prefix} {other}"
            );
        }
    }

    let shebangs: Vec<&str> = all
        .iter()
        .flat_map(|l| l.shebangs.iter().copied())
        .collect();
    for (i, shebang) in shebangs.iter().enumerate() {
        assert!(!shebangs[..i].contains(shebang), "{shebang} twice");
    }
    assert_eq!(shebangs, ["bash"], "Appendix A names one shebang, bash's");
}

#[test]
fn header_rule_is_appendix_a() {
    // "`.h` files are assigned to `cpp` when a sibling `.cpp/.cc/.cxx` with the same
    // basename exists or the file contains `class `, `namespace ` or `template<`;
    // otherwise `c`."
    assert_eq!(HEADER_RULE.extension, ".h");
    assert_eq!(HEADER_RULE.default, "c");
    assert_eq!(HEADER_RULE.alternative, "cpp");
    assert_eq!(HEADER_RULE.sibling_extensions, [".cpp", ".cc", ".cxx"]);
    assert_eq!(
        HEADER_RULE.content_markers,
        ["class ", "namespace ", "template<"]
    );
}

#[test]
fn module_rules_by_strategy() {
    // Which languages share each strategy, matched on the typed rule, as a later stage
    // will match on it.
    let with = |wanted: fn(&ModuleRule) -> bool| -> Vec<&str> {
        languages::all()
            .iter()
            .filter(|l| wanted(&l.module_rule))
            .map(|l| l.id)
            .collect()
    };
    assert_eq!(
        with(|r| matches!(r, M::Declaration(DeclarationKeyword::Package))),
        ["java", "kotlin", "scala", "perl", "groovy", "protobuf"]
    );
    assert_eq!(
        with(|r| matches!(r, M::Declaration(DeclarationKeyword::Namespace))),
        ["cpp", "csharp", "php"]
    );
    assert_eq!(
        with(|r| matches!(r, M::Directory)),
        ["javascript", "swift", "objc"]
    );
    assert_eq!(
        with(|r| matches!(r, M::File)),
        [
            "bash",
            "lua",
            "sql",
            "graphql",
            "yaml",
            "json",
            "toml",
            "hcl",
            "dockerfile",
            "xml"
        ]
    );
    assert_eq!(DeclarationKeyword::Package.keyword(), "package");
    assert_eq!(DeclarationKeyword::Namespace.keyword(), "namespace");
}

#[test]
fn test_rules_resolve() {
    let javascript = languages::by_id("javascript").expect("javascript");
    let typescript = languages::by_id("typescript").expect("typescript");
    assert_eq!(
        javascript.test_detection,
        TestDetection::SameAs("typescript")
    );
    assert_eq!(javascript.test_rules(), typescript.test_rules());
    assert!(!typescript.test_rules().is_empty());
    for language in languages::all() {
        if let TestDetection::SameAs(other) = language.test_detection {
            let target = languages::by_id(other).expect("a language of the matrix");
            assert!(
                matches!(target.test_detection, TestDetection::Rules(_)),
                "{other}"
            );
        }
    }
    assert!(
        languages::by_id("bash")
            .expect("bash")
            .test_rules()
            .is_empty()
    );
}

#[test]
fn every_typed_language_has_engine_support() {
    let typed: Vec<&Language> = languages::all()
        .iter()
        .filter(|l| l.tier == Tier::Typed)
        .collect();
    let ids: Vec<&str> = typed.iter().map(|l| l.id).collect();
    assert_eq!(
        ids,
        [
            "java",
            "kotlin",
            "typescript",
            "javascript",
            "python",
            "go",
            "c",
            "cpp",
            "csharp",
            "rust",
            "php",
            "perl"
        ]
    );
    for language in typed {
        // The language's own id, and any grammar of the engine's its files may need.
        assert!(
            Engine::knows_language(language.id),
            "typed language {} has no engine language",
            language.id
        );
        for dialect in ENGINE_DIALECTS.iter().filter(|d| d.language == language.id) {
            assert!(
                Engine::knows_language(dialect.engine_id),
                "{}: {}",
                language.id,
                dialect.engine_id
            );
        }
    }
}

#[test]
fn every_language_has_an_engine_grammar() {
    // Both tiers are extracted by the engine (Appendix A); the tier says only whether it
    // also resolves types. So every language needs an engine grammar.
    for language in languages::all() {
        assert!(
            Engine::knows_language(language.id),
            "{} has no engine grammar",
            language.id
        );
    }
}

#[test]
fn tsx_is_typescript_through_the_tsx_grammar() {
    // `.tsx` is a TypeScript extension in Appendix A, not a language of its own. The
    // engine parses it with a grammar of its own, which is an engine detail.
    assert_eq!(languages::by_id("tsx"), None);
    let typescript = languages::detect("ui/Widget.tsx", b"", |_| false).expect("a language");
    assert_eq!(typescript.id, "typescript");
    assert_eq!(typescript.engine_language("ui/Widget.tsx"), "tsx");
    assert!(Engine::knows_language("tsx"));
    for file in [
        "ui/widget.ts",
        "ui/widget.mts",
        "ui/widget.cts",
        "ui/widget.tsx.ts",
    ] {
        assert_eq!(typescript.engine_language(file), "typescript", "{file}");
    }
    let javascript = languages::by_id("javascript").expect("javascript");
    assert_eq!(javascript.engine_language("ui/Widget.jsx"), "javascript");

    assert_eq!(ENGINE_DIALECTS.len(), 1);
    assert_eq!(
        (
            ENGINE_DIALECTS[0].language,
            ENGINE_DIALECTS[0].extension,
            ENGINE_DIALECTS[0].engine_id
        ),
        ("typescript", ".tsx", "tsx")
    );
}

#[test]
fn engine_test_languages_are_the_matrix_and_its_dialects() {
    // The engine's C tests keep their own list of what the engine must parse
    // (engine/tests/matrix_languages.h). It must be the matrix's languages and the
    // engine grammars they need, nothing more or less.
    let header = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../engine/tests/matrix_languages.h"
    );
    let text = std::fs::read_to_string(header).expect("the engine's test language list");
    let list = text
        .split("MATRIX_LANGUAGES[] = {")
        .nth(1)
        .expect("the list")
        .split("};")
        .next()
        .expect("its end");
    let mut c_ids: Vec<&str> = list.split('"').skip(1).step_by(2).collect();
    let mut expected: Vec<&str> = languages::all().iter().map(|l| l.id).collect();
    expected.extend(ENGINE_DIALECTS.iter().map(|d| d.engine_id));
    c_ids.sort_unstable();
    expected.sort_unstable();
    assert_eq!(c_ids, expected);
}

#[test]
fn version_is_two() {
    // 2 since issue 31: the test rules' semantics changed, the languages did not.
    assert_eq!(LANGUAGE_MATRIX_VERSION, 2);
}
