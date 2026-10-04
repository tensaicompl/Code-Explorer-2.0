//! What the engine records of inheritance and of package and namespace declarations,
//! and that it crosses the safe boundary whole (docs/plan/ISSUES.md, issues 40 and
//! 41): a definition's base classes in the engine's order and spelling, and the
//! package or namespace a file declares, where the engine reads one.

use pdx_engine::{Engine, FileExtract, NamespaceEvidence, ProjectResolver, namespace_evidence};

fn extract(engine: &Engine, language: &str, rel_path: &str, source: &str) -> FileExtract {
    engine
        .extract(language, rel_path, source.as_bytes())
        .unwrap_or_else(|e| panic!("{rel_path}: {e}"))
}

/// The bases of the definition named `name`.
fn bases<'e>(extract: &'e FileExtract, name: &str) -> &'e [String] {
    &extract
        .definitions
        .iter()
        .find(|d| d.name == name)
        .unwrap_or_else(|| panic!("no definition {name}"))
        .base_classes
}

#[test]
fn base_classes_cross_the_safe_boundary() {
    let engine = Engine::new().unwrap();
    // One base.
    let java = extract(
        &engine,
        "java",
        "src/Derived.java",
        "package com.acme;\npublic class Derived extends Base {\n  void run() {}\n}\n",
    );
    assert_eq!(bases(&java, "Derived"), ["Base"]);
    assert!(bases(&java, "run").is_empty(), "a method names no bases");

    // Several, in the engine's order, which is the source's and not alphabetical.
    let python = extract(
        &engine,
        "python",
        "pkg/shapes.py",
        "class Derived(Zeta, Alpha, mixins.Gamma):\n    pass\n\n\nclass Plain:\n    pass\n",
    );
    assert_eq!(bases(&python, "Derived"), ["Zeta", "Alpha", "mixins.Gamma"]);
    // None.
    assert!(bases(&python, "Plain").is_empty());

    let cpp = extract(
        &engine,
        "cpp",
        "src/shapes.cpp",
        "namespace geo {\nclass Circle : public Shape, protected Named<int> {};\n}\n",
    );
    assert_eq!(bases(&cpp, "Circle"), ["Shape", "Named"]);

    let typescript = extract(
        &engine,
        "typescript",
        "src/user.ts",
        "export class User extends Base implements Named, Aged {}\nexport interface Named extends HasName, HasId {}\n",
    );
    assert_eq!(bases(&typescript, "User"), ["Base", "Named", "Aged"]);
    assert_eq!(bases(&typescript, "Named"), ["HasName", "HasId"]);

    // Spelling as the engine records it, unresolved and unedited.
    let javascript = extract(
        &engine,
        "javascript",
        "src/user.js",
        "export class User extends Base {}\n",
    );
    assert_eq!(bases(&javascript, "User"), ["extends Base"]);
}

#[test]
fn bases_and_declarations_survive_serialisation() {
    // The extraction cache and the isolated worker carry `FileExtract` in postcard.
    let engine = Engine::new().unwrap();
    for (language, path, source) in [
        (
            "java",
            "src/Derived.java",
            "package com.acme.shop;\nclass Derived extends Base implements Runnable {}\n",
        ),
        (
            "python",
            "pkg/m.py",
            "class Derived(Zeta, Alpha):\n    pass\n",
        ),
    ] {
        let original = extract(&engine, language, path, source);
        let bytes = postcard::to_stdvec(&original).unwrap();
        let back: FileExtract = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(back, original, "{path}");
        assert!(back.definitions.iter().any(|d| !d.base_classes.is_empty()));
    }
}

#[test]
fn a_cached_extraction_with_bases_resolves() {
    // Resolving from a cached extraction rebuilds the engine's result from it, bases
    // included; the answers are the fresh ones.
    let engine = Engine::new().unwrap();
    let source = b"class Base:\n    def hello(self):\n        return 1\n\n\nclass Derived(Base):\n    def run(self):\n        return self.hello()\n";
    let fresh_extract = engine.extract("python", "app/m.py", source).unwrap();
    assert_eq!(bases(&fresh_extract, "Derived"), ["Base"]);
    let mut fresh = ProjectResolver::new(&engine).unwrap();
    fresh.extract_and_add("python", "app/m.py", source).unwrap();
    let fresh = fresh.run().unwrap();

    let cached_extract: FileExtract =
        postcard::from_bytes(&postcard::to_stdvec(&fresh_extract).unwrap()).unwrap();
    let mut cached = ProjectResolver::new(&engine).unwrap();
    cached.add_file(&cached_extract, source).unwrap();
    assert_eq!(cached.run().unwrap(), fresh);
}

#[test]
fn declared_namespaces_cross_the_safe_boundary() {
    let engine = Engine::new().unwrap();
    for (language, path, source, declared) in [
        (
            "java",
            "src/main/java/x/Shop.java",
            "package com.acme.shop;\nclass Shop {}\n",
            Some("com.acme.shop"),
        ),
        ("java", "Default.java", "class Default {}\n", None),
        (
            "kotlin",
            "src/Shop.kt",
            "package com.acme.shop\nclass Shop\n",
            Some("com.acme.shop"),
        ),
        (
            "csharp",
            "src/Block.cs",
            "namespace Acme.Shop\n{\n  class Block {}\n}\n",
            Some("Acme.Shop"),
        ),
        (
            "csharp",
            "src/Scoped.cs",
            "namespace Acme.Shop;\nclass Scoped {}\n",
            Some("Acme.Shop"),
        ),
        ("csharp", "src/Global.cs", "class Global {}\n", None),
        (
            "php",
            "src/Shop.php",
            "<?php\nnamespace Acme\\Shop;\nclass Shop {}\n",
            Some("Acme\\Shop"),
        ),
    ] {
        assert_eq!(
            namespace_evidence(language),
            NamespaceEvidence::FileDeclaration
        );
        let e = extract(&engine, language, path, source);
        assert_eq!(e.declared_namespace.as_deref(), declared, "{path}");
    }
}

#[test]
fn namespace_evidence_is_what_the_engine_records() {
    let engine = Engine::new().unwrap();
    // C++ records its namespaces in qualified names, as written, and declares none.
    assert_eq!(namespace_evidence("cpp"), NamespaceEvidence::QualifiedNames);
    let cpp = extract(
        &engine,
        "cpp",
        "src/n.cpp",
        "namespace geo { namespace detail { int f() { return 1; } } }\nnamespace a::b { struct S {}; }\n",
    );
    assert_eq!(cpp.declared_namespace, None);
    let qn = |name: &str| {
        cpp.definitions
            .iter()
            .find(|d| d.name == name)
            .unwrap()
            .qualified_name
            .clone()
    };
    assert_eq!(qn("f"), "src.n.geo.detail.f");
    assert_eq!(qn("S"), "src.n.a::b.S");

    // Every other language declares nothing at the boundary, even when its source
    // has a package or namespace.
    for (language, path, source) in [
        (
            "perl",
            "lib/Acme/Shop.pm",
            "package Acme::Shop;\nsub run { 1 }\n1;\n",
        ),
        ("scala", "src/Shop.scala", "package acme.shop\nclass Shop\n"),
        (
            "groovy",
            "src/Shop.groovy",
            "package acme.shop\nclass Shop {}\n",
        ),
        (
            "protobuf",
            "proto/shop.proto",
            "syntax = \"proto3\";\npackage acme.shop;\nmessage Order {}\n",
        ),
        ("go", "pkg/foo/foo.go", "package foo\nfunc F() {}\n"),
        ("python", "pkg/m.py", "def f():\n    pass\n"),
        (
            "typescript",
            "src/m.ts",
            "namespace Acme { export const x = 1; }\n",
        ),
        ("rust", "src/lib.rs", "pub mod shop { pub fn f() {} }\n"),
    ] {
        assert_eq!(
            namespace_evidence(language),
            NamespaceEvidence::Unrecorded,
            "{language}"
        );
        let e = extract(&engine, language, path, source);
        assert_eq!(e.declared_namespace, None, "{language} declared one");
    }
}
