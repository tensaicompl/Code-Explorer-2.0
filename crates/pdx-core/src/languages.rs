//! The language matrix: the languages PDX indexes, how a file is assigned one, and
//! the rules later stages apply to each (specification Appendix A).
//!
//! Everything here is static data and pure functions. Nothing reads a file, the
//! environment or the engine: discovery hands [`detect`] a file's path and bytes and
//! gets back the language Appendix A assigns it. Whether the engine can parse or
//! type-resolve a language is a separate question, answered by the engine; the
//! matrix's [`Tier`] is what the specification promises, and a test holds the engine
//! to it.
//!
//! The matrix's version is [`LANGUAGE_MATRIX_VERSION`]. Changing which language a file
//! gets, or any rule below, changes the matrix, which is a specification change that
//! bumps it.
//!
//! [`LANGUAGE_MATRIX_VERSION`]: crate::consts::LANGUAGE_MATRIX_VERSION

/// How far PDX resolves a language's references.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tier {
    /// The engine extracts the language and resolves its types; the Rust stages run on
    /// what it resolves.
    Typed,
    /// The engine extracts the language (definitions, calls, and imports where the
    /// grammar has them); resolution is the Rust stages' alone.
    Structural,
}

/// The keyword of the declaration that names a file's module.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeclarationKeyword {
    /// `package`.
    Package,
    /// `namespace`.
    Namespace,
}

impl DeclarationKeyword {
    /// The keyword as the source spells it.
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Package => "package",
            Self::Namespace => "namespace",
        }
    }
}

/// How a file's module is named: Appendix A's "Module rule" column.
///
/// The rule says where the name comes from; building `Module` nodes from it is the
/// derive stage's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModuleRule {
    /// The `package` or `namespace` the file declares.
    Declaration(DeclarationKeyword),
    /// The directory the file is in.
    Directory,
    /// The directory the file is in, with the path mappings of the project's `config`
    /// (`tsconfig`) applied.
    DirectoryWithPathMappings {
        /// The configuration whose path mappings apply.
        config: &'static str,
    },
    /// The directory the file is in, beneath the module path its `manifest` (`go.mod`)
    /// declares.
    DirectoryUnderModulePath {
        /// The manifest that declares the module path.
        manifest: &'static str,
    },
    /// The directory the file is in; a header and a source file with the same basename
    /// are paired.
    DirectoryPairingHeaders,
    /// The dotted path from the root of the nearest package, a package being a
    /// directory that holds `package_marker` (`__init__.py`).
    DottedPath {
        /// The file whose presence makes a directory a package.
        package_marker: &'static str,
    },
    /// The `mod` tree, from a crate root: one of `crate_roots`.
    ModTree {
        /// The files a crate's module tree starts from.
        crate_roots: &'static [&'static str],
    },
    /// The unit name of the package specification or body the file holds.
    UnitName,
    /// The nesting of the `module` and `class` declarations around a definition.
    ModuleClassNesting,
    /// The file itself.
    File,
    /// The file itself, its sections becoming `Doc` nodes.
    FileSectionsAsDocs,
    /// The file itself, of which only the keys are kept.
    FileKeysOnly,
}

/// A test framework a rule names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TestFramework {
    /// `JUnit`.
    JUnit,
    /// `TestNG`.
    TestNG,
    /// `GoogleTest`.
    GoogleTest,
}

/// A directory, in a rule for the files beneath it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DirectoryPattern {
    /// A directory by its path of one or more names, `tests` or `src/test`: consecutive
    /// directory components anywhere in a file's path.
    Path(&'static str),
    /// A directory whose name begins with the prefix, case-sensitively: `test` for
    /// `test*`, any directory component of a file's path.
    NamePrefix(&'static str),
}

/// One way Appendix A recognises a test.
///
/// Rules are data: matching them against a repository, and drawing `TESTS` edges, is
/// the derive stage's. Where a rule's pattern applies in a path is that stage's to
/// decide; the pattern itself is Appendix A's, exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TestRule {
    /// A source file whose name ends with this stem and then one of its own language's
    /// extensions: `.test` matches `a.test.ts` and `a.test.tsx` in TypeScript, and, for a
    /// language whose rules are TypeScript's ([`TestDetection::SameAs`]), `a.test.js`
    /// and `a.test.mjs` in JavaScript, never `a.test.ts` (issue 31).
    SourceSuffix(&'static str),
    /// A file whose name is `prefix`, then anything, then `suffix`: `test_*.py` is the
    /// prefix `test_` and the suffix `.py`.
    FileName {
        /// What the name begins with; empty for none.
        prefix: &'static str,
        /// What the name ends with.
        suffix: &'static str,
    },
    /// A file beneath the directory, at any depth: `tests/**` matches `tests/a.py` and
    /// `packages/api/tests/a.py` (issue 31).
    Under(DirectoryPattern),
    /// A function or class carrying the framework's test annotations.
    Annotations(TestFramework),
    /// A function or method carrying the attribute of this name: `test` for Rust's
    /// `#[test]`, `Fact` for C#'s `[Fact]`.
    Attribute(&'static str),
    /// A test the framework's macros define.
    Macros(TestFramework),
}

/// How a language's tests are recognised: Appendix A's "Test rule" column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TestDetection {
    /// The language's own rules; none where Appendix A gives none.
    Rules(&'static [TestRule]),
    /// The rules of the language with this id, which Appendix A says are the same.
    SameAs(&'static str),
}

/// One language of the matrix: a row of Appendix A.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Language {
    /// The language's identifier, as Appendix A writes it and as everything stores it.
    pub id: &'static str,
    /// How far it is resolved.
    pub tier: Tier,
    /// Endings that assign a file this language, matched against the file's name and
    /// case-sensitively: `.java`, and `.dockerfile` for Appendix A's `*.dockerfile`.
    pub extensions: &'static [&'static str],
    /// Beginnings that assign a file this language when no extension does, matched
    /// against the file's name and case-sensitively: `Dockerfile` for Appendix A's
    /// `Dockerfile*`, `.env` for its `.env*`.
    pub name_prefixes: &'static [&'static str],
    /// Interpreters a file's `#!` line may name to assign it this language when its
    /// name assigns none.
    pub shebangs: &'static [&'static str],
    /// How a file's module is named.
    pub module_rule: ModuleRule,
    /// How its tests are recognised.
    pub test_detection: TestDetection,
}

impl Language {
    /// The rules that recognise this language's tests, following [`TestDetection::SameAs`].
    pub fn test_rules(&self) -> &'static [TestRule] {
        match self.test_detection {
            TestDetection::Rules(rules) => rules,
            TestDetection::SameAs(id) => by_id(id).map_or(&[], Language::test_rules),
        }
    }

    /// The engine language a file of this language at `path` is extracted as: its own
    /// id, unless an [`EngineDialect`] names another grammar for the file's extension.
    pub fn engine_language(&self, path: &str) -> &'static str {
        let name = file_name(path);
        ENGINE_DIALECTS
            .iter()
            .find(|d| d.language == self.id && name.ends_with(d.extension))
            .map_or(self.id, |d| d.engine_id)
    }
}

/// The one extension two languages share, and Appendix A's rule for which a file
/// gets: "`.h` files are assigned to `cpp` when a sibling `.cpp/.cc/.cxx` with the same
/// basename exists or the file contains `class `, `namespace ` or `template<`;
/// otherwise `c`."
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HeaderRule {
    /// The shared extension.
    pub extension: &'static str,
    /// The language a file gets without evidence of the other.
    pub default: &'static str,
    /// The language a file gets with evidence of it.
    pub alternative: &'static str,
    /// Evidence: a file in the same directory with the same basename and one of these
    /// extensions.
    pub sibling_extensions: &'static [&'static str],
    /// Evidence: the file contains one of these, byte for byte.
    pub content_markers: &'static [&'static str],
}

/// Appendix A's rule for `.h`.
pub const HEADER_RULE: HeaderRule = HeaderRule {
    extension: ".h",
    default: "c",
    alternative: "cpp",
    sibling_extensions: &[".cpp", ".cc", ".cxx"],
    content_markers: &["class ", "namespace ", "template<"],
};

/// A grammar of the engine's that is not named after the language it parses.
///
/// Engine mechanics, not the matrix: the engine parses `.tsx` with a grammar of its
/// own, which it names `tsx`, while in the matrix `.tsx` is a TypeScript extension
/// like any other. A file of `language` ending in `extension` is extracted as
/// `engine_id`; every other file is extracted under its language's own id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EngineDialect {
    /// The matrix language.
    pub language: &'static str,
    /// The extension that needs the other grammar.
    pub extension: &'static str,
    /// The engine's name for that grammar.
    pub engine_id: &'static str,
}

/// Every engine grammar named otherwise than its language.
pub const ENGINE_DIALECTS: &[EngineDialect] = &[EngineDialect {
    language: "typescript",
    extension: ".tsx",
    engine_id: "tsx",
}];

/// The matrix, in Appendix A's order.
pub fn all() -> &'static [Language] {
    MATRIX
}

/// The language with this id, exactly as Appendix A writes it.
pub fn by_id(id: &str) -> Option<&'static Language> {
    MATRIX.iter().find(|l| l.id == id)
}

/// The language Appendix A assigns a file, if any.
///
/// `path` is the file's path with `/` between directories; only its last component,
/// the file's name, is read. `content` is the file's bytes: the first line is read for
/// a shebang, and a `.h` file is searched whole for C++ (pass the whole file for one).
/// `sibling_exists` answers whether the file's directory holds a file of the given
/// name, and is asked only about a `.h` file's siblings.
///
/// In order, and case-sensitively throughout:
///
/// 1. The longest extension the name ends with. `.h` is then decided by
///    [`HEADER_RULE`].
/// 2. Failing that, a name prefix the name begins with.
/// 3. Failing that, the interpreter the first line names, if it is `#!` followed by
///    an interpreter's path, or `env` and options or assignments and then the
///    interpreter's path; the path's last component is the interpreter.
/// 4. Failing that, none.
///
/// The name decides whenever it can: a shebang never overrides it.
pub fn detect(
    path: &str,
    content: &[u8],
    sibling_exists: impl Fn(&str) -> bool,
) -> Option<&'static Language> {
    let name = file_name(path);
    if let Some((extension, language)) = by_extension(name) {
        if extension == HEADER_RULE.extension {
            return by_id(header_language(name, content, sibling_exists));
        }
        return Some(language);
    }
    if let Some(language) = MATRIX
        .iter()
        .find(|l| l.name_prefixes.iter().any(|p| name.starts_with(p)))
    {
        return Some(language);
    }
    let interpreter = shebang_interpreter(content)?;
    MATRIX
        .iter()
        .find(|l| l.shebangs.iter().any(|s| s.as_bytes() == interpreter))
}

/// The last component of a `/`-separated path.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The longest extension `name` ends with, and the first language that has it.
fn by_extension(name: &str) -> Option<(&'static str, &'static Language)> {
    let mut best: Option<(&'static str, &'static Language)> = None;
    for language in MATRIX {
        for extension in language.extensions {
            if name.ends_with(extension) && best.is_none_or(|(b, _)| extension.len() > b.len()) {
                best = Some((extension, language));
            }
        }
    }
    best
}

/// [`HEADER_RULE`] applied to a header named `name`.
fn header_language(
    name: &str,
    content: &[u8],
    sibling_exists: impl Fn(&str) -> bool,
) -> &'static str {
    let stem = &name[..name.len() - HEADER_RULE.extension.len()];
    let sibling = HEADER_RULE
        .sibling_extensions
        .iter()
        .any(|extension| sibling_exists(&format!("{stem}{extension}")));
    let marked = HEADER_RULE.content_markers.iter().any(|marker| {
        content
            .windows(marker.len())
            .any(|w| w == marker.as_bytes())
    });
    if sibling || marked {
        HEADER_RULE.alternative
    } else {
        HEADER_RULE.default
    }
}

/// The interpreter a `#!` first line names: the last component of its path, after
/// `env` and its options and assignments if the line goes through `env`.
fn shebang_interpreter(content: &[u8]) -> Option<&[u8]> {
    let rest = content.strip_prefix(b"#!")?;
    let line = rest.split(|&b| b == b'\n').next().unwrap_or(rest);
    let mut words = line
        .split(|b| matches!(b, b' ' | b'\t' | b'\r'))
        .filter(|w| !w.is_empty());
    let mut program = last_component(words.next()?);
    if program == b"env" {
        program = last_component(words.find(|w| !w.starts_with(b"-") && !w.contains(&b'='))?);
    }
    Some(program)
}

fn last_component(path: &[u8]) -> &[u8] {
    path.rsplit(|&b| b == b'/').next().unwrap_or(path)
}

const fn file_name_rule(prefix: &'static str, suffix: &'static str) -> TestRule {
    TestRule::FileName { prefix, suffix }
}

const NO_TESTS: TestDetection = TestDetection::Rules(&[]);

/// Appendix A, row for row.
static MATRIX: &[Language] = &[
    Language {
        id: "java",
        tier: Tier::Typed,
        extensions: &[".java"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::Declaration(DeclarationKeyword::Package),
        test_detection: TestDetection::Rules(&[
            TestRule::Annotations(TestFramework::JUnit),
            TestRule::Annotations(TestFramework::TestNG),
            TestRule::Under(DirectoryPattern::Path("src/test")),
            file_name_rule("", "Test.java"),
        ]),
    },
    Language {
        id: "kotlin",
        tier: Tier::Typed,
        extensions: &[".kt", ".kts"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::Declaration(DeclarationKeyword::Package),
        test_detection: TestDetection::Rules(&[
            TestRule::Under(DirectoryPattern::Path("src/test")),
            file_name_rule("", "Test.kt"),
        ]),
    },
    Language {
        id: "scala",
        tier: Tier::Structural,
        extensions: &[".scala"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::Declaration(DeclarationKeyword::Package),
        test_detection: TestDetection::Rules(&[TestRule::Under(DirectoryPattern::Path(
            "src/test",
        ))]),
    },
    Language {
        id: "typescript",
        tier: Tier::Typed,
        extensions: &[".ts", ".tsx", ".mts", ".cts"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::DirectoryWithPathMappings { config: "tsconfig" },
        // Appendix A's `*.test.ts`, `*.spec.ts`: the `.test` and `.spec` convention, on
        // every extension of the language (issue 31).
        test_detection: TestDetection::Rules(&[
            TestRule::SourceSuffix(".test"),
            TestRule::SourceSuffix(".spec"),
            TestRule::Under(DirectoryPattern::Path("__tests__")),
        ]),
    },
    Language {
        id: "javascript",
        tier: Tier::Typed,
        extensions: &[".js", ".jsx", ".mjs", ".cjs"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::Directory,
        test_detection: TestDetection::SameAs("typescript"),
    },
    Language {
        id: "python",
        tier: Tier::Typed,
        extensions: &[".py", ".pyi"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::DottedPath {
            package_marker: "__init__.py",
        },
        test_detection: TestDetection::Rules(&[
            file_name_rule("test_", ".py"),
            file_name_rule("", "_test.py"),
            TestRule::Under(DirectoryPattern::Path("tests")),
        ]),
    },
    Language {
        id: "go",
        tier: Tier::Typed,
        extensions: &[".go"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::DirectoryUnderModulePath { manifest: "go.mod" },
        test_detection: TestDetection::Rules(&[file_name_rule("", "_test.go")]),
    },
    Language {
        id: "c",
        tier: Tier::Typed,
        extensions: &[".c", ".h"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::DirectoryPairingHeaders,
        test_detection: TestDetection::Rules(&[
            TestRule::Under(DirectoryPattern::NamePrefix("test")),
            file_name_rule("", "_test.c"),
        ]),
    },
    Language {
        id: "cpp",
        tier: Tier::Typed,
        // `.h` by HEADER_RULE only: listed here because Appendix A lists it here.
        extensions: &[".cpp", ".cc", ".cxx", ".hpp", ".hh", ".hxx", ".h"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::Declaration(DeclarationKeyword::Namespace),
        test_detection: TestDetection::Rules(&[
            TestRule::Under(DirectoryPattern::NamePrefix("test")),
            TestRule::Macros(TestFramework::GoogleTest),
        ]),
    },
    Language {
        id: "csharp",
        tier: Tier::Typed,
        extensions: &[".cs"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::Declaration(DeclarationKeyword::Namespace),
        test_detection: TestDetection::Rules(&[
            file_name_rule("", "Tests.cs"),
            TestRule::Attribute("Fact"),
            TestRule::Attribute("Test"),
        ]),
    },
    Language {
        id: "rust",
        tier: Tier::Typed,
        extensions: &[".rs"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::ModTree {
            crate_roots: &["lib.rs", "main.rs"],
        },
        test_detection: TestDetection::Rules(&[
            TestRule::Attribute("test"),
            TestRule::Under(DirectoryPattern::Path("tests")),
        ]),
    },
    Language {
        id: "php",
        tier: Tier::Typed,
        extensions: &[".php"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::Declaration(DeclarationKeyword::Namespace),
        test_detection: TestDetection::Rules(&[file_name_rule("", "Test.php")]),
    },
    Language {
        id: "perl",
        tier: Tier::Typed,
        extensions: &[".pl", ".pm"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::Declaration(DeclarationKeyword::Package),
        test_detection: TestDetection::Rules(&[TestRule::Under(DirectoryPattern::Path("t"))]),
    },
    Language {
        id: "ada",
        tier: Tier::Structural,
        extensions: &[".ads", ".adb", ".ada"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::UnitName,
        test_detection: TestDetection::Rules(&[TestRule::Under(DirectoryPattern::NamePrefix(
            "test",
        ))]),
    },
    Language {
        id: "bash",
        tier: Tier::Structural,
        extensions: &[".sh", ".bash"],
        name_prefixes: &[],
        shebangs: &["bash"],
        module_rule: ModuleRule::File,
        test_detection: NO_TESTS,
    },
    Language {
        id: "ruby",
        tier: Tier::Structural,
        extensions: &[".rb"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::ModuleClassNesting,
        test_detection: TestDetection::Rules(&[
            TestRule::Under(DirectoryPattern::Path("spec")),
            TestRule::Under(DirectoryPattern::Path("test")),
        ]),
    },
    Language {
        id: "swift",
        tier: Tier::Structural,
        extensions: &[".swift"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::Directory,
        test_detection: TestDetection::Rules(&[file_name_rule("", "Tests.swift")]),
    },
    Language {
        id: "objc",
        tier: Tier::Structural,
        extensions: &[".m", ".mm"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::Directory,
        test_detection: NO_TESTS,
    },
    Language {
        id: "groovy",
        tier: Tier::Structural,
        extensions: &[".groovy", ".gradle"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::Declaration(DeclarationKeyword::Package),
        test_detection: NO_TESTS,
    },
    Language {
        id: "lua",
        tier: Tier::Structural,
        extensions: &[".lua"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::File,
        test_detection: NO_TESTS,
    },
    Language {
        id: "sql",
        tier: Tier::Structural,
        extensions: &[".sql"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::File,
        test_detection: NO_TESTS,
    },
    Language {
        id: "protobuf",
        tier: Tier::Structural,
        extensions: &[".proto"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::Declaration(DeclarationKeyword::Package),
        test_detection: NO_TESTS,
    },
    Language {
        id: "graphql",
        tier: Tier::Structural,
        extensions: &[".graphql", ".gql"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::File,
        test_detection: NO_TESTS,
    },
    Language {
        id: "yaml",
        tier: Tier::Structural,
        extensions: &[".yaml", ".yml"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::File,
        test_detection: NO_TESTS,
    },
    Language {
        id: "json",
        tier: Tier::Structural,
        extensions: &[".json"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::File,
        test_detection: NO_TESTS,
    },
    Language {
        id: "toml",
        tier: Tier::Structural,
        extensions: &[".toml"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::File,
        test_detection: NO_TESTS,
    },
    Language {
        id: "hcl",
        tier: Tier::Structural,
        extensions: &[".tf", ".hcl"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::File,
        test_detection: NO_TESTS,
    },
    Language {
        id: "dockerfile",
        tier: Tier::Structural,
        extensions: &[".dockerfile"],
        name_prefixes: &["Dockerfile"],
        shebangs: &[],
        module_rule: ModuleRule::File,
        test_detection: NO_TESTS,
    },
    Language {
        id: "markdown",
        tier: Tier::Structural,
        extensions: &[".md"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::FileSectionsAsDocs,
        test_detection: NO_TESTS,
    },
    Language {
        id: "xml",
        tier: Tier::Structural,
        extensions: &[".xml", ".pom"],
        name_prefixes: &[],
        shebangs: &[],
        module_rule: ModuleRule::File,
        test_detection: NO_TESTS,
    },
    Language {
        id: "properties",
        tier: Tier::Structural,
        extensions: &[".properties"],
        name_prefixes: &[".env"],
        shebangs: &[],
        module_rule: ModuleRule::FileKeysOnly,
        test_detection: NO_TESTS,
    },
];
