//! The symbol registry (P2-05): definitions by name, qualified name and file; modules
//! by the language matrix's rules; imports and where they lead; class hierarchies;
//! external names; and the module metadata the engine's resolver takes.
//!
//! Every fixture runs the real pipeline: a checkout, discovered, extracted by the
//! engine, then registered. Nothing here hand-builds a definition or an import.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use pdx_core::config::PdxConfig;
use pdx_core::index::discover::{DiscoveredFile, discover};
use pdx_core::index::extract::{ExtractLimits, ExtractReport, ExtractStage, FileOutcome, FsCache};
use pdx_core::languages::{self, ModuleRule, Tier};
use pdx_core::resolve::registry::{
    BaseResolution, DefinitionRef, ImportRecord, ImportTarget, InternalTarget, ModuleKey,
    NameProvenance, RegistryError, SymbolRegistry,
};

// --- fixtures ----------------------------------------------------------------------

/// A checkout in a temporary directory.
struct Checkout {
    dir: tempfile::TempDir,
}

impl Checkout {
    fn new(files: &[(&str, &str)]) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("pdx-registry-test-")
            .tempdir()
            .expect("a directory");
        let checkout = Self { dir };
        for (path, content) in files {
            checkout.write(path, content);
        }
        checkout
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.root().join(relative);
        fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
        fs::write(&path, content).expect("a file");
    }

    fn discover(&self) -> Vec<DiscoveredFile> {
        let config = PdxConfig::load(self.root()).expect("the configuration loads");
        discover(self.root(), &config).expect("discovery succeeds")
    }

    fn extract(&self, files: &[DiscoveredFile], cache: Option<&FsCache>) -> ExtractReport {
        let config = PdxConfig::load(self.root()).expect("the configuration loads");
        let limits = ExtractLimits {
            requested_workers: 2,
            memory_budget_bytes: 64 << 20,
        };
        let mut stage = ExtractStage::new(self.root(), &config.secrets, limits);
        if let Some(cache) = cache {
            stage = stage.with_cache(cache);
        }
        stage.run(files).expect("extraction succeeds")
    }

    fn registry(&self) -> SymbolRegistry {
        self.try_registry().expect("the registry builds")
    }

    fn try_registry(&self) -> Result<SymbolRegistry, RegistryError> {
        let files = self.discover();
        let report = self.extract(&files, None);
        SymbolRegistry::build(self.root(), &files, report)
    }
}

/// The one definition named `name` in `path`.
fn def(reg: &SymbolRegistry, path: &str, name: &str) -> DefinitionRef {
    let found: Vec<&DefinitionRef> = reg
        .by_name(name)
        .iter()
        .filter(|r| r.path == path)
        .collect();
    assert_eq!(found.len(), 1, "{name} in {path}: {found:?}");
    found[0].clone()
}

fn module(language: &'static str, scope: &str, name: &str) -> ModuleKey {
    ModuleKey {
        language,
        scope: scope.to_owned(),
        name: name.to_owned(),
    }
}

/// The import of `path` whose module text is `text`.
fn import<'r>(reg: &'r SymbolRegistry, path: &str, text: &str) -> &'r ImportRecord {
    reg.imports_of(path)
        .iter()
        .find(|i| i.module_text == text)
        .unwrap_or_else(|| {
            panic!(
                "no import {text} in {path}: {:?}",
                reg.imports_of(path)
                    .iter()
                    .map(|i| &i.module_text)
                    .collect::<Vec<_>>()
            )
        })
}

/// The files an internal import leads to.
fn internal_files(target: &ImportTarget) -> Vec<&str> {
    match target {
        ImportTarget::Internal(t) => t.files.iter().map(String::as_str).collect(),
        other => panic!("not internal: {other:?}"),
    }
}

/// The internal target an import leads to.
fn internal(target: &ImportTarget) -> &InternalTarget {
    match target {
        ImportTarget::Internal(t) => t,
        other => panic!("not internal: {other:?}"),
    }
}

/// The definitions a definition names as bases, resolved internally.
fn base_defs(reg: &SymbolRegistry, r: &DefinitionRef) -> Vec<DefinitionRef> {
    reg.direct_bases(r)
        .iter()
        .filter_map(|b| match &b.resolution {
            BaseResolution::Internal(d) => Some(d.clone()),
            _ => None,
        })
        .collect()
}

/// What every typed fixture shows of its definitions: listed by name, by qualified
/// name and under their file.
fn assert_indexed(reg: &SymbolRegistry, r: &DefinitionRef) {
    let d = reg.definition(r).expect("the definition exists");
    assert!(reg.by_name(&d.name).contains(r), "{} not by name", d.name);
    assert!(
        reg.by_qualified_name(&d.qualified_name).contains(r),
        "{} not by qualified name",
        d.qualified_name
    );
    assert!(
        reg.by_file(&r.path).contains(r),
        "{} not under its file",
        d.name
    );
}

/// What every typed fixture shows of a name declared twice in one file (overloads,
/// conditional or repeated definitions): both are kept, each listed by name, by
/// qualified name and under the file, and a qualified name they share lists every one
/// of them, none chosen.
fn assert_duplicates_kept(path: &str, source: &str, name: &str) {
    let checkout = Checkout::new(&[(path, source)]);
    let reg = checkout.registry();
    let found: Vec<DefinitionRef> = reg
        .by_name(name)
        .iter()
        .filter(|r| r.path == path)
        .cloned()
        .collect();
    let described: Vec<String> = found
        .iter()
        .map(|r| {
            reg.definition(r)
                .expect("the definition exists")
                .qualified_name
                .clone()
        })
        .collect();
    assert!(found.len() >= 2, "{path}: {name} found {described:?}");
    for r in &found {
        assert_indexed(&reg, r);
    }
    let shared: BTreeSet<&String> = described
        .iter()
        .filter(|qn| described.iter().filter(|q| q == qn).count() > 1)
        .collect();
    assert!(
        !shared.is_empty(),
        "{path}: no shared qualified name in {described:?}"
    );
    for qn in shared {
        let sharing: Vec<&DefinitionRef> = found
            .iter()
            .zip(&described)
            .filter(|(_, q)| *q == qn)
            .map(|(r, _)| r)
            .collect();
        let listed: Vec<&DefinitionRef> = reg.by_qualified_name(qn).iter().collect();
        assert_eq!(listed, sharing, "{path}: {qn}");
    }
}

// --- the typed languages -------------------------------------------------------------

/// Every typed language has a fixture below: the list is the matrix's, not one kept
/// by hand.
#[test]
fn every_typed_language_has_a_registry_fixture() {
    let source = include_str!("registry.rs");
    for language in languages::all().iter().filter(|l| l.tier == Tier::Typed) {
        assert!(
            source.contains(&format!("fn registry_{}() {{", language.id)),
            "no registry_{} fixture",
            language.id
        );
    }
}

#[test]
fn registry_java() {
    let checkout = Checkout::new(&[
        (
            "src/main/java/com/acme/base/Base.java",
            "package com.acme.base;\n\npublic class Base {\n    public void hello() {}\n}\n",
        ),
        (
            "src/main/java/com/acme/shop/Shop.java",
            "package com.acme.shop;\n\nimport com.acme.base.Base;\nimport java.util.List;\n\npublic class Shop extends Base implements Runnable {\n    public void run() {}\n    void m(int a) {}\n    void m(String s) {}\n}\n",
        ),
    ]);
    let reg = checkout.registry();
    let shop_file = "src/main/java/com/acme/shop/Shop.java";
    let base_file = "src/main/java/com/acme/base/Base.java";
    let shop = def(&reg, shop_file, "Shop");
    let base = def(&reg, base_file, "Base");
    assert_indexed(&reg, &shop);
    assert_indexed(&reg, &base);
    // Overloads share a qualified name: both are kept.
    let qn = format!("{}.m", reg.definition(&shop).unwrap().qualified_name);
    assert_eq!(reg.by_qualified_name(&qn).len(), 2);

    // The declared package is the module, not the directory.
    let shop_pkg = module("java", "", "com.acme.shop");
    assert_eq!(reg.module_of_file(shop_file), Some(&shop_pkg));
    assert_eq!(reg.module_of_definition(&shop), Some(&shop_pkg));
    assert_eq!(
        reg.files_in_module(&module("java", "", "com.acme.base"))
            .collect::<Vec<_>>(),
        [base_file]
    );

    // An internal import and an external one.
    let i = import(&reg, shop_file, "com.acme.base.Base");
    assert_eq!(internal_files(&i.target), [base_file]);
    assert_eq!(internal(&i.target).member.as_deref(), Some("Base"));
    assert_eq!(
        import(&reg, shop_file, "java.util.List").target,
        ImportTarget::External
    );
    assert!(reg.is_external_name(shop_file, "List"));

    // The base resolves through the import; the interface nothing in the repository
    // declares or imports stays unresolved, not external.
    assert_eq!(base_defs(&reg, &shop), std::slice::from_ref(&base));
    let runnable = reg
        .direct_bases(&shop)
        .iter()
        .find(|b| b.name == "Runnable")
        .unwrap();
    assert_eq!(runnable.resolution, BaseResolution::Unresolved);
    assert_eq!(reg.direct_derived(&base), std::slice::from_ref(&shop));
    // A method's declaring type.
    let run = def(&reg, shop_file, "run");
    assert_eq!(reg.declaring_type(&run), Some(shop));
    assert_duplicates_kept(
        "src/main/java/com/acme/dup/Area.java",
        "package com.acme.dup;\n\npublic class Area {\n    int area(int x) { return x; }\n    int area(int x, int y) { return x * y; }\n}\n",
        "area",
    );
}

#[test]
fn registry_kotlin() {
    let checkout = Checkout::new(&[
        (
            "src/main/kotlin/com/acme/base/Base.kt",
            "package com.acme.base\n\nopen class Base {\n    fun hello() {}\n}\n",
        ),
        (
            "src/main/kotlin/com/acme/shop/Shop.kt",
            "package com.acme.shop\n\nimport com.acme.base.Base\nimport kotlin.math.max as biggest\n\nclass Shop : Base() {\n    fun run() = biggest(1, 2)\n}\n",
        ),
    ]);
    let reg = checkout.registry();
    let shop_file = "src/main/kotlin/com/acme/shop/Shop.kt";
    let base_file = "src/main/kotlin/com/acme/base/Base.kt";
    let shop = def(&reg, shop_file, "Shop");
    assert_indexed(&reg, &shop);
    assert_eq!(
        reg.module_of_file(shop_file),
        Some(&module("kotlin", "", "com.acme.shop"))
    );
    assert_eq!(
        internal_files(&import(&reg, shop_file, "com.acme.base.Base").target),
        [base_file]
    );
    // The alias the engine keeps inside the module text binds the name.
    let aliased = import(&reg, shop_file, "kotlin.math.max as biggest");
    assert_eq!(aliased.bindings, ["biggest"]);
    assert_eq!(aliased.target, ImportTarget::External);
    assert_eq!(base_defs(&reg, &shop), [def(&reg, base_file, "Base")]);
    assert_duplicates_kept(
        "src/main/kotlin/com/acme/dup/Area.kt",
        "package com.acme.dup\n\nfun area(x: Int): Int = x\n\nfun area(x: Int, y: Int): Int = x * y\n",
        "area",
    );
}

#[test]
fn registry_typescript() {
    let checkout = Checkout::new(&[
        (
            "tsconfig.json",
            "{\n  // comments and trailing commas, as tsconfig allows\n  \"compilerOptions\": {\n    \"baseUrl\": \".\",\n    \"paths\": { \"@core/*\": [\"src/core/*\"], },\n  },\n}\n",
        ),
        (
            "src/core/base.ts",
            "export class Base {\n  hello(): string { return 'hi'; }\n}\n",
        ),
        (
            "src/util/index.ts",
            "export function helper(): number { return 1; }\n",
        ),
        (
            "src/app/user.ts",
            "import { Base } from '@core/base';\nimport { helper } from '../util';\nimport * as fs from 'fs';\n\nexport class User extends Base {\n  run(): number { return helper(); }\n}\n",
        ),
    ]);
    let reg = checkout.registry();
    let user_file = "src/app/user.ts";
    let user = def(&reg, user_file, "User");
    assert_indexed(&reg, &user);
    // The directory is the module.
    assert_eq!(
        reg.module_of_file(user_file),
        Some(&module("typescript", "", "src/app"))
    );
    // Through the tsconfig alias, to a directory's index, and outside.
    assert_eq!(
        internal_files(&import(&reg, user_file, "@core/base").target),
        ["src/core/base.ts"]
    );
    assert_eq!(
        internal_files(&import(&reg, user_file, "../util").target),
        ["src/util/index.ts"]
    );
    assert_eq!(import(&reg, user_file, "fs").target, ImportTarget::External);
    assert_eq!(
        base_defs(&reg, &user),
        [def(&reg, "src/core/base.ts", "Base")]
    );
    assert_duplicates_kept(
        "src/dup.ts",
        "export function area(x: number): number {\n    return x;\n}\n\nexport function area(x: number, y: number): number {\n    return x * y;\n}\n",
        "area",
    );
}

#[test]
fn registry_javascript() {
    let checkout = Checkout::new(&[
        (
            "lib/base.js",
            "export class Base {\n  hello() { return 1; }\n}\n",
        ),
        (
            "lib/user.js",
            "import { Base } from './base.js';\nconst path = require('path');\n\nexport class User extends Base {\n  run() { return path.sep; }\n}\n",
        ),
    ]);
    let reg = checkout.registry();
    let user = def(&reg, "lib/user.js", "User");
    assert_indexed(&reg, &user);
    assert_eq!(
        reg.module_of_file("lib/user.js"),
        Some(&module("javascript", "", "lib"))
    );
    assert_eq!(
        internal_files(&import(&reg, "lib/user.js", "./base.js").target),
        ["lib/base.js"]
    );
    assert_eq!(
        import(&reg, "lib/user.js", "path").target,
        ImportTarget::External
    );
    // The engine records the base as `extends Base`; the name is looked up.
    let relation = &reg.direct_bases(&user)[0];
    assert_eq!(relation.raw, "extends Base");
    assert_eq!(relation.name, "Base");
    assert_eq!(base_defs(&reg, &user), [def(&reg, "lib/base.js", "Base")]);
    assert_duplicates_kept(
        "src/dup.js",
        "function area(x) {\n    return x;\n}\n\nfunction area(x, y) {\n    return x * y;\n}\n",
        "area",
    );
}

#[test]
fn registry_python() {
    let checkout = Checkout::new(&[
        ("app/__init__.py", ""),
        ("app/models/__init__.py", ""),
        (
            "app/models/base.py",
            "class Base:\n    def hello(self):\n        return 1\n",
        ),
        (
            "app/models/user.py",
            "from .base import Base\nfrom app.models import base\nimport json\n\n\nclass User(Base):\n    def run(self):\n        return json.dumps(self.hello())\n",
        ),
        ("scripts/tool.py", "def main():\n    pass\n"),
    ]);
    let reg = checkout.registry();
    let user_file = "app/models/user.py";
    let user = def(&reg, user_file, "User");
    assert_indexed(&reg, &user);
    // The dotted path from the outermost package; a file outside any package is a
    // top-level module of its own directory.
    assert_eq!(
        reg.module_of_file(user_file),
        Some(&module("python", "", "app.models.user"))
    );
    assert_eq!(
        reg.module_of_file("app/models/__init__.py"),
        Some(&module("python", "", "app.models"))
    );
    assert_eq!(
        reg.module_of_file("scripts/tool.py"),
        Some(&module("python", "scripts", "tool"))
    );
    // Relative and absolute internal imports, and one from outside.
    let relative = import(&reg, user_file, ".base.Base");
    assert_eq!(internal_files(&relative.target), ["app/models/base.py"]);
    assert_eq!(internal(&relative.target).member.as_deref(), Some("Base"));
    assert_eq!(
        internal_files(&import(&reg, user_file, "app.models.base").target),
        ["app/models/base.py"]
    );
    assert_eq!(
        import(&reg, user_file, "json").target,
        ImportTarget::External
    );
    assert_eq!(
        base_defs(&reg, &user),
        [def(&reg, "app/models/base.py", "Base")]
    );
    assert_duplicates_kept(
        "dup.py",
        "def area(x):\n    return x\n\n\ndef area(x, y):\n    return x * y\n",
        "area",
    );
}

#[test]
fn registry_go() {
    let checkout = Checkout::new(&[
        ("go.mod", "module example.com/acme\n\ngo 1.22\n"),
        (
            "pkg/bar/bar.go",
            "package bar\n\ntype Mixin struct{}\n\nfunc Helper() int { return 1 }\n",
        ),
        (
            "pkg/foo/foo.go",
            "package foo\n\nimport (\n\t\"fmt\"\n\t\"example.com/acme/pkg/bar\"\n\t\"example.com/acme/pkg/missing\"\n)\n\ntype Shop struct {\n\tname string\n}\n\nfunc (s *Shop) Run() { fmt.Println(bar.Helper()) }\n",
        ),
    ]);
    let reg = checkout.registry();
    let foo = "pkg/foo/foo.go";
    let shop = def(&reg, foo, "Shop");
    assert_indexed(&reg, &shop);
    // The module path, then the directory below it.
    assert_eq!(
        reg.module_of_file(foo),
        Some(&module("go", "", "example.com/acme/pkg/foo"))
    );
    let bar = import(&reg, foo, "example.com/acme/pkg/bar");
    assert_eq!(internal_files(&bar.target), ["pkg/bar/bar.go"]);
    assert_eq!(bar.bindings, ["bar"]);
    assert_eq!(
        import(&reg, foo, "example.com/acme/pkg/missing").target,
        ImportTarget::UnresolvedInternal
    );
    assert_eq!(import(&reg, foo, "fmt").target, ImportTarget::External);
    // A method's receiver is its declaring type.
    let run = def(&reg, foo, "Run");
    assert_eq!(reg.declaring_type(&run), Some(shop));
    // Go has no base classes: struct embedding is not reported as one.
    assert!(reg.direct_bases(&def(&reg, foo, "Shop")).is_empty());
    assert_duplicates_kept(
        "dup/dup.go",
        "package dup\n\nfunc area(x int) int { return x }\n\nfunc area(x, y int) int { return x * y }\n",
        "area",
    );
}

#[test]
fn registry_c() {
    let checkout = Checkout::new(&[
        ("src/shop.h", "int shop_total(void);\n"),
        (
            "src/shop.c",
            "#include <stdio.h>\n#include \"shop.h\"\n#include \"../lib/util.h\"\n\nint shop_total(void) { return util_one(); }\n",
        ),
        ("lib/util.h", "int util_one(void);\n"),
        (
            "lib/util.c",
            "#include \"util.h\"\nint util_one(void) { return 1; }\n",
        ),
    ]);
    let reg = checkout.registry();
    let total = reg
        .by_name("shop_total")
        .iter()
        .find(|r| r.path == "src/shop.c")
        .unwrap()
        .clone();
    assert_indexed(&reg, &total);
    assert_eq!(
        reg.module_of_file("src/shop.c"),
        Some(&module("c", "", "src"))
    );
    assert_eq!(reg.paired_file("src/shop.h"), Some("src/shop.c"));
    assert_eq!(reg.paired_file("src/shop.c"), Some("src/shop.h"));
    assert_eq!(
        internal_files(&import(&reg, "src/shop.c", "shop.h").target),
        ["src/shop.h"]
    );
    assert_eq!(
        internal_files(&import(&reg, "src/shop.c", "../lib/util.h").target),
        ["lib/util.h"]
    );
    assert_eq!(
        import(&reg, "src/shop.c", "stdio.h").target,
        ImportTarget::External
    );
    assert_duplicates_kept(
        "dup.c",
        "#ifdef WIDE\nint area(int x) { return 2 * x; }\n#else\nint area(int x) { return x; }\n#endif\n",
        "area",
    );
}

#[test]
fn registry_cpp() {
    let checkout = Checkout::new(&[
        ("src/shapes.hpp", "namespace geo { class Shape {}; }\n"),
        (
            "src/shapes.cpp",
            "#include \"shapes.hpp\"\n#include <vector>\n\nnamespace geo {\nclass Circle : public Shape {\n  public:\n    double area() const { return 1.0; }\n};\nnamespace detail { int helper() { return 1; } }\n}\nnamespace other { void f() {} }\nvoid global_fn() {}\n",
        ),
    ]);
    let reg = checkout.registry();
    let file = "src/shapes.cpp";
    let circle = def(&reg, file, "Circle");
    assert_indexed(&reg, &circle);
    // Namespaces, per definition: one file, several modules.
    assert_eq!(
        reg.module_of_definition(&circle),
        Some(&module("cpp", "", "geo"))
    );
    let area = def(&reg, file, "area");
    assert_eq!(
        reg.module_of_definition(&area),
        Some(&module("cpp", "", "geo"))
    );
    assert_eq!(
        reg.module_of_definition(&def(&reg, file, "helper")),
        Some(&module("cpp", "", "geo::detail"))
    );
    assert_eq!(
        reg.module_of_definition(&def(&reg, file, "global_fn")),
        Some(&module("cpp", "", ""))
    );
    let names: Vec<&str> = reg
        .modules_of_file(file)
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(names, ["", "geo", "geo::detail", "other"]);
    assert_eq!(
        reg.module_of_file(file),
        None,
        "a file of several namespaces has no one module"
    );
    assert_eq!(
        internal_files(&import(&reg, file, "shapes.hpp").target),
        ["src/shapes.hpp"]
    );
    assert_eq!(
        base_defs(&reg, &circle),
        [def(&reg, "src/shapes.hpp", "Shape")]
    );
    assert_eq!(reg.declaring_type(&area), Some(circle));
    assert_duplicates_kept(
        "dup.cpp",
        "namespace geo {\nint area(int x) { return x; }\nint area(int x, int y) { return x * y; }\n}\n",
        "area",
    );
}

#[test]
fn registry_csharp() {
    let checkout = Checkout::new(&[
        (
            "src/Base/Base.cs",
            "namespace Acme.Base\n{\n    public class Base { public void Hello() {} }\n}\n",
        ),
        (
            "src/Shop/Shop.cs",
            "using System;\nusing Acme.Base;\n\nnamespace Acme.Shop\n{\n    public class Shop : Base, IDisposable\n    {\n        public void Dispose() {}\n    }\n}\n",
        ),
        (
            "src/Shop/Cart.cs",
            "namespace Acme.Shop;\n\npublic class Cart { public void Add() {} }\n",
        ),
    ]);
    let reg = checkout.registry();
    let shop = def(&reg, "src/Shop/Shop.cs", "Shop");
    assert_indexed(&reg, &shop);
    // Block and file-scoped declarations of one namespace are one module.
    let ns = module("csharp", "", "Acme.Shop");
    assert_eq!(reg.module_of_file("src/Shop/Shop.cs"), Some(&ns));
    assert_eq!(reg.module_of_file("src/Shop/Cart.cs"), Some(&ns));
    assert_eq!(
        reg.files_in_module(&ns).collect::<Vec<_>>(),
        ["src/Shop/Cart.cs", "src/Shop/Shop.cs"]
    );
    assert_eq!(
        internal_files(&import(&reg, "src/Shop/Shop.cs", "Acme.Base").target),
        ["src/Base/Base.cs"]
    );
    assert_eq!(
        import(&reg, "src/Shop/Shop.cs", "System").target,
        ImportTarget::External
    );
    // A `using` imports a namespace and binds no name of its own, so the base comes
    // from the one type of that name in the repository.
    assert_eq!(
        base_defs(&reg, &shop),
        [def(&reg, "src/Base/Base.cs", "Base")]
    );
    assert_duplicates_kept(
        "Dup.cs",
        "namespace Acme.Dup;\n\npublic class Shapes\n{\n    public int Area(int x) => x;\n    public int Area(int x, int y) => x * y;\n}\n",
        "Area",
    );
}

#[test]
fn registry_rust() {
    let checkout = Checkout::new(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/shop\", \"crates/util\"]\n",
        ),
        (
            "crates/util/Cargo.toml",
            "[package]\nname = \"acme-util\"\nversion = \"0.1.0\"\n",
        ),
        ("crates/util/src/lib.rs", "pub fn helper() -> u32 { 1 }\n"),
        (
            "crates/shop/Cargo.toml",
            "[package]\nname = \"shop\"\nversion = \"0.1.0\"\n\n[dependencies]\nacme-util = { path = \"../util\" }\nserde = \"1\"\n",
        ),
        (
            "crates/shop/src/lib.rs",
            "pub mod models;\nuse std::collections::HashMap;\nuse acme_util::helper;\nuse crate::models::Order;\nuse crate::models::{order::Order as Placed, self as m};\n\npub fn total() -> u32 { helper() }\n",
        ),
        (
            "crates/shop/src/models/mod.rs",
            "pub mod order;\npub use self::order::Order;\n",
        ),
        (
            "crates/shop/src/models/order.rs",
            "pub struct Order { pub id: u32 }\nimpl Order { pub fn new() -> Self { Order { id: 1 } } }\n",
        ),
    ]);
    let reg = checkout.registry();
    let lib = "crates/shop/src/lib.rs";
    let total = def(&reg, lib, "total");
    assert_indexed(&reg, &total);
    // The module tree, scoped by crate.
    assert_eq!(
        reg.module_of_file(lib),
        Some(&module("rust", "crates/shop", "crate"))
    );
    assert_eq!(
        reg.module_of_file("crates/shop/src/models/mod.rs"),
        Some(&module("rust", "crates/shop", "crate::models"))
    );
    assert_eq!(
        reg.module_of_file("crates/shop/src/models/order.rs"),
        Some(&module("rust", "crates/shop", "crate::models::order"))
    );
    assert_eq!(
        reg.module_of_file("crates/util/src/lib.rs"),
        Some(&module("rust", "crates/util", "crate"))
    );
    assert_eq!(
        internal_files(&import(&reg, lib, "acme_util::helper").target),
        ["crates/util/src/lib.rs"]
    );
    assert_eq!(
        internal_files(&import(&reg, lib, "crate::models::Order").target),
        ["crates/shop/src/models/mod.rs"]
    );
    assert_eq!(
        import(&reg, lib, "std::collections::HashMap").target,
        ImportTarget::External
    );
    // A brace group: every name it binds, aliases and `self` included.
    let group = reg
        .imports_of(lib)
        .iter()
        .find(|i| i.module_text.contains('{'))
        .expect("the group import");
    assert_eq!(group.bindings, ["Placed", "m"]);
    assert_eq!(
        internal_files(&group.target),
        ["crates/shop/src/models/mod.rs"]
    );
    // A method's declaring type, through the engine's parent.
    let new = def(&reg, "crates/shop/src/models/order.rs", "new");
    assert_eq!(
        reg.declaring_type(&new),
        Some(def(&reg, "crates/shop/src/models/order.rs", "Order"))
    );
    // A plain repeat shares a qualified name. Alternatives under `#[cfg(..)]` do not:
    // the engine names them apart (`src.lib.area#cfg(unix)]`), so they cannot collide.
    assert_duplicates_kept(
        "src/lib.rs",
        "fn area(x: i32) -> i32 {\n    x\n}\n\nfn area(x: i32, y: i32) -> i32 {\n    x * y\n}\n",
        "area",
    );
}

#[test]
fn registry_php() {
    let checkout = Checkout::new(&[
        (
            "src/Base/Base.php",
            "<?php\nnamespace Acme\\Base;\n\nclass Base { public function hello() {} }\n",
        ),
        (
            "src/Shop/Shop.php",
            "<?php\nnamespace Acme\\Shop;\n\nuse Acme\\Base\\Base;\nuse Vendor\\Lib\\Thing as T;\nrequire_once 'lib/legacy.php';\n\nclass Shop extends Base { public function run() {} }\n",
        ),
        ("lib/legacy.php", "<?php\nfunction legacy() { return 1; }\n"),
    ]);
    let reg = checkout.registry();
    let shop = def(&reg, "src/Shop/Shop.php", "Shop");
    assert_indexed(&reg, &shop);
    assert_eq!(
        reg.module_of_file("src/Shop/Shop.php"),
        Some(&module("php", "", "Acme\\Shop"))
    );
    assert_eq!(
        internal_files(&import(&reg, "src/Shop/Shop.php", "Acme\\Base\\Base").target),
        ["src/Base/Base.php"]
    );
    let thing = import(&reg, "src/Shop/Shop.php", "Vendor\\Lib\\Thing");
    assert_eq!(thing.target, ImportTarget::External);
    assert!(reg.is_external_name("src/Shop/Shop.php", "T"));
    assert_eq!(
        internal_files(&import(&reg, "src/Shop/Shop.php", "lib/legacy.php").target),
        ["lib/legacy.php"]
    );
    assert_eq!(
        base_defs(&reg, &shop),
        [def(&reg, "src/Base/Base.php", "Base")]
    );
    assert_duplicates_kept(
        "dup.php",
        "<?php\nnamespace Acme\\Dup;\n\nif (PHP_OS === 'Linux') {\n    function area($x) { return $x; }\n} else {\n    function area($x) { return 2 * $x; }\n}\n",
        "area",
    );
}

#[test]
fn registry_perl() {
    let checkout = Checkout::new(&[
        (
            "lib/Acme/Util.pm",
            "package Acme::Util;\nuse strict;\nsub helper { return 1; }\n1;\n",
        ),
        (
            "lib/Acme/Shop.pm",
            "package Acme::Shop;\nuse strict;\nuse Acme::Util;\nsub run { return Acme::Util::helper(); }\nsub run_twice { run(); run(); }\n1;\n",
        ),
    ]);
    let reg = checkout.registry();
    let run = def(&reg, "lib/Acme/Shop.pm", "run");
    assert_indexed(&reg, &run);
    // Perl's module is the package it declares, which the engine does not record
    // (docs/plan/ISSUES.md, issue 41): no module is invented from the directory.
    assert_eq!(reg.modules_of_file("lib/Acme/Shop.pm"), []);
    assert_eq!(reg.module_of_definition(&run), None);
    // So its imports cannot be placed either way: not internal, not external.
    let util = import(&reg, "lib/Acme/Shop.pm", "Acme::Util");
    assert_eq!(util.target, ImportTarget::Unclassified);
    assert!(!reg.is_external_name("lib/Acme/Shop.pm", "Util"));
    assert_duplicates_kept(
        "Dup.pm",
        "package Acme::Dup;\nsub area { return $_[0]; }\nsub area { return 2 * $_[0]; }\n1;\n",
        "area",
    );
}

#[test]
fn external_detection_python_stdlib() {
    // No local `os`: `import os` and `from pathlib import Path` bind external names;
    // a name nothing imports is not external merely because nothing defines it.
    let plain = Checkout::new(&[(
        "main.py",
        "import os\nfrom pathlib import Path\n\n\ndef run(items):\n    return len(items), os.getcwd(), Path('.')\n",
    )]);
    let reg = plain.registry();
    assert_eq!(import(&reg, "main.py", "os").target, ImportTarget::External);
    assert!(matches!(
        reg.name_provenance("main.py", "os"),
        NameProvenance::ExternalImport(_)
    ));
    assert_eq!(
        import(&reg, "main.py", "pathlib.Path").target,
        ImportTarget::External
    );
    assert!(reg.is_external_name("main.py", "Path"));
    assert_eq!(
        reg.name_provenance("main.py", "len"),
        NameProvenance::Unknown
    );
    assert!(!reg.is_external_name("main.py", "len"));
    assert_eq!(
        reg.external_names("main.py"),
        BTreeSet::from(["Path", "os"])
    );
    // The file's own definitions are its own.
    assert!(matches!(
        reg.name_provenance("main.py", "run"),
        NameProvenance::Defined(_)
    ));

    // A local `os.py` shadows the standard library: the same import is internal.
    let shadowed = Checkout::new(&[
        ("os.py", "def getcwd():\n    return '.'\n"),
        (
            "main.py",
            "import os\n\n\ndef run():\n    return os.getcwd()\n",
        ),
    ]);
    let reg = shadowed.registry();
    assert_eq!(
        internal_files(&import(&reg, "main.py", "os").target),
        ["os.py"]
    );
    assert!(!reg.is_external_name("main.py", "os"));
    assert!(matches!(
        reg.name_provenance("main.py", "os"),
        NameProvenance::InternalImport(_)
    ));
}

// --- regressions ---------------------------------------------------------------------

#[test]
fn duplicate_qn_preserved() {
    let checkout = Checkout::new(&[(
        "app/dup.py",
        "def handler():\n    return 1\n\n\ndef handler():\n    return 2\n",
    )]);
    let reg = checkout.registry();
    let refs = reg.by_qualified_name("app.dup.handler");
    assert_eq!(refs.len(), 2, "{refs:?}");
    assert!(refs[0].index < refs[1].index);
    assert_eq!(reg.by_name("handler"), refs);
}

#[test]
fn names_keep_their_case() {
    let checkout = Checkout::new(&[(
        "app/m.py",
        "class Widget:\n    pass\n\n\nclass widget:\n    pass\n",
    )]);
    let reg = checkout.registry();
    assert_eq!(reg.by_name("Widget").len(), 1);
    assert_eq!(reg.by_name("widget").len(), 1);
    assert_ne!(reg.by_name("Widget"), reg.by_name("widget"));
}

#[test]
fn shuffled_input_same_registry() {
    // Names several files define, so every list the registry keeps has more than one
    // entry whose order could follow the input.
    let checkout = Checkout::new(&[
        ("a/__init__.py", ""),
        (
            "a/x.py",
            "class X:\n    pass\n\n\ndef helper():\n    pass\n",
        ),
        (
            "a/y.py",
            "from .x import X\n\n\nclass Y(X):\n    pass\n\n\ndef helper():\n    pass\n",
        ),
        (
            "b/z.ts",
            "export class Z {}\nexport function helper(): void {}\n",
        ),
        (
            "b/w.ts",
            "import { Z } from './z';\nexport class W extends Z {}\nexport function helper(): void {}\n",
        ),
        ("c/x.py", "class X:\n    pass\n"),
    ]);
    let files = checkout.discover();
    let report = checkout.extract(&files, None);
    let ordered = SymbolRegistry::build(checkout.root(), &files, report.clone()).unwrap();
    // The lists are in (path, index) order, whatever order the files came in.
    let paths: Vec<&str> = ordered
        .by_name("helper")
        .iter()
        .map(|r| r.path.as_str())
        .collect();
    assert_eq!(paths, ["a/x.py", "a/y.py", "b/w.ts", "b/z.ts"]);
    for (rotate, reverse) in [(1, true), (2, false), (4, true)] {
        let mut shuffled_files = files.clone();
        if reverse {
            shuffled_files.reverse();
        }
        let mut shuffled_report = report.clone();
        shuffled_report.files.rotate_left(rotate);
        if reverse {
            shuffled_report.files.reverse();
        }
        let shuffled =
            SymbolRegistry::build(checkout.root(), &shuffled_files, shuffled_report).unwrap();
        assert_eq!(ordered, shuffled, "rotated {rotate}, reversed {reverse}");
    }
}

#[test]
fn class_hierarchy_transitive_and_cycle_safe() {
    let checkout = Checkout::new(&[(
        "h.py",
        "class A(B):\n    pass\n\n\nclass B(C):\n    pass\n\n\nclass C(A):\n    pass\n\n\nclass D(A):\n    pass\n",
    )]);
    let reg = checkout.registry();
    let [a, b, c, d] = ["A", "B", "C", "D"].map(|n| def(&reg, "h.py", n));
    // Transitive, nearest first, each once, and finite despite the cycle.
    assert_eq!(reg.ancestors(&d), [a.clone(), b.clone(), c.clone()]);
    assert_eq!(reg.ancestors(&a), [b.clone(), c.clone()]);
    assert_eq!(reg.direct_derived(&a), [c, d]);
}

#[test]
fn rust_impl_relations_resolve_internal_traits() {
    // `impl Trait for Type` (issue 42): the trait through the file's `use`, the type
    // by its recorded qualified name; an empty block counts as much as one with
    // methods, and a trait imported from outside the repository is external.
    let checkout = Checkout::new(&[
        (
            "Cargo.toml",
            "[package]\nname = \"shapes\"\nversion = \"0.1.0\"\n",
        ),
        ("src/lib.rs", "pub mod shape;\npub mod square;\n"),
        (
            "src/shape.rs",
            "pub trait Shape {\n    fn area(&self) -> f64;\n}\n\npub trait Marker {}\n",
        ),
        (
            "src/square.rs",
            "use crate::shape::{Marker, Shape};\nuse std::fmt;\n\npub struct Square;\n\nimpl Marker for Square {}\n\nimpl Shape for Square {\n    fn area(&self) -> f64 {\n        1.0\n    }\n}\n\nimpl fmt::Display for Square {\n    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {\n        write!(f, \"square\")\n    }\n}\n",
        ),
    ]);
    let reg = checkout.registry();
    let square = def(&reg, "src/square.rs", "Square");
    let shape = def(&reg, "src/shape.rs", "Shape");
    let marker = def(&reg, "src/shape.rs", "Marker");
    let relations = reg.impl_relations("src/square.rs");
    let named: Vec<&str> = relations.iter().map(|r| r.trait_name.as_str()).collect();
    assert_eq!(named, ["Marker", "Shape", "fmt::Display"]);
    for relation in relations {
        assert_eq!(
            relation.implementing_type,
            BaseResolution::Internal(square.clone())
        );
        assert_eq!(relation.struct_qn, "src.square.Square");
    }
    assert_eq!(
        relations[0].implemented_trait,
        BaseResolution::Internal(marker.clone())
    );
    assert_eq!(
        relations[1].implemented_trait,
        BaseResolution::Internal(shape.clone())
    );
    assert!(matches!(
        relations[2].implemented_trait,
        BaseResolution::External(_)
    ));
    let mut expected = vec![shape.clone(), marker.clone()];
    expected.sort();
    assert_eq!(reg.implemented_traits(&square), expected);
    assert_eq!(reg.implementors(&shape), std::slice::from_ref(&square));
    assert_eq!(reg.implementors(&marker), std::slice::from_ref(&square));
    // No supertrait is invented, and the class hierarchy is still empty for Rust.
    assert!(reg.implemented_traits(&shape).is_empty());
    assert!(reg.ancestors(&square).is_empty());
}

#[test]
fn ambiguous_trait_remains_ambiguous() {
    // Two traits of one name, and an impl whose file names neither through an import:
    // nothing chooses between them, so the type implements no internal trait.
    let checkout = Checkout::new(&[
        (
            "Cargo.toml",
            "[package]\nname = \"shapes\"\nversion = \"0.1.0\"\n",
        ),
        ("src/lib.rs", "pub mod a;\npub mod b;\npub mod c;\n"),
        ("src/a.rs", "pub trait Shape {}\n"),
        ("src/b.rs", "pub trait Shape {}\n"),
        (
            "src/c.rs",
            "pub struct Square;\n\nimpl Shape for Square {}\n",
        ),
    ]);
    let reg = checkout.registry();
    let square = def(&reg, "src/c.rs", "Square");
    let [relation] = reg.impl_relations("src/c.rs") else {
        panic!("one relation: {:?}", reg.impl_relations("src/c.rs"));
    };
    let mut both = vec![
        def(&reg, "src/a.rs", "Shape"),
        def(&reg, "src/b.rs", "Shape"),
    ];
    both.sort();
    assert_eq!(relation.implemented_trait, BaseResolution::Ambiguous(both));
    assert_eq!(
        relation.implementing_type,
        BaseResolution::Internal(square.clone())
    );
    assert!(reg.implemented_traits(&square).is_empty());
}

#[test]
fn ambiguous_base_is_not_forced() {
    let checkout = Checkout::new(&[
        ("one/base.py", "class Base:\n    pass\n"),
        ("two/base.py", "class Base:\n    pass\n"),
        ("three/derived.py", "class Derived(Base):\n    pass\n"),
    ]);
    let reg = checkout.registry();
    let derived = def(&reg, "three/derived.py", "Derived");
    match &reg.direct_bases(&derived)[0].resolution {
        BaseResolution::Ambiguous(candidates) => {
            assert_eq!(
                candidates
                    .iter()
                    .map(|r| r.path.as_str())
                    .collect::<Vec<_>>(),
                ["one/base.py", "two/base.py"]
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(reg.ancestors(&derived).is_empty());
}

#[test]
fn external_base_needs_external_import() {
    let checkout = Checkout::new(&[(
        "app.py",
        "from django.db import models\n\n\nclass Order(models.Model):\n    pass\n\n\nclass Loose(Unknown):\n    pass\n",
    )]);
    let reg = checkout.registry();
    let order = def(&reg, "app.py", "Order");
    assert!(matches!(
        reg.direct_bases(&order)[0].resolution,
        BaseResolution::External(_)
    ));
    let loose = def(&reg, "app.py", "Loose");
    assert_eq!(
        reg.direct_bases(&loose)[0].resolution,
        BaseResolution::Unresolved
    );
}

#[test]
fn relative_missing_import_is_not_external() {
    let checkout = Checkout::new(&[
        ("pkg/__init__.py", ""),
        (
            "pkg/a.py",
            "from .missing import thing\nfrom . import gone\n",
        ),
        ("web/a.ts", "import { x } from './missing';\n"),
    ]);
    let reg = checkout.registry();
    // A module the package does not have.
    assert_eq!(
        import(&reg, "pkg/a.py", ".missing.thing").target,
        ImportTarget::UnresolvedInternal
    );
    // A name the package itself may define: the package, asked for that member.
    let gone = import(&reg, "pkg/a.py", "..gone");
    assert_eq!(internal_files(&gone.target), ["pkg/__init__.py"]);
    assert_eq!(internal(&gone.target).member.as_deref(), Some("gone"));
    for record in reg.imports_of("pkg/a.py") {
        assert!(record.target.is_internal(), "{record:?}");
    }
    assert_eq!(
        import(&reg, "web/a.ts", "./missing").target,
        ImportTarget::UnresolvedInternal
    );
    assert!(!reg.is_external_name("pkg/a.py", "thing"));
}

#[test]
fn unimported_missing_name_is_not_external() {
    let checkout = Checkout::new(&[("a.py", "def f():\n    return undefined_thing()\n")]);
    let reg = checkout.registry();
    assert_eq!(
        reg.name_provenance("a.py", "undefined_thing"),
        NameProvenance::Unknown
    );
    assert!(reg.external_names("a.py").is_empty());
}

#[test]
fn python_local_module_shadows_stdlib() {
    let checkout = Checkout::new(&[
        ("json.py", "def dumps(x):\n    return str(x)\n"),
        ("main.py", "import json\n"),
    ]);
    let reg = checkout.registry();
    assert_eq!(
        internal_files(&import(&reg, "main.py", "json").target),
        ["json.py"]
    );
}

#[test]
fn python_relative_imports_follow_packages() {
    let checkout = Checkout::new(&[
        ("pkg/__init__.py", ""),
        ("pkg/util.py", "def u():\n    pass\n"),
        (
            "pkg/sub/__init__.py",
            "from . import tool\nfrom .. import util\n",
        ),
        ("pkg/sub/tool.py", "from ..util import u\n"),
    ]);
    let reg = checkout.registry();
    let init = "pkg/sub/__init__.py";
    assert_eq!(
        internal_files(&import(&reg, init, "..tool").target),
        ["pkg/sub/tool.py"]
    );
    assert_eq!(
        internal_files(&import(&reg, init, "...util").target),
        ["pkg/util.py"]
    );
    let u = import(&reg, "pkg/sub/tool.py", "..util.u");
    assert_eq!(internal_files(&u.target), ["pkg/util.py"]);
    assert_eq!(internal(&u.target).member.as_deref(), Some("u"));
}

#[test]
fn tsconfig_path_alias_internal() {
    let checkout = Checkout::new(&[
        (
            "web/tsconfig.base.json",
            "{ \"compilerOptions\": { \"baseUrl\": \"./src\", \"paths\": { \"~lib/*\": [\"lib/*\"] } } }\n",
        ),
        (
            "web/tsconfig.json",
            "{\n  \"extends\": \"./tsconfig.base.json\", // inherits the aliases\n}\n",
        ),
        (
            "web/src/lib/format.ts",
            "export function format(): string { return ''; }\n",
        ),
        (
            "web/src/app/show.ts",
            "import { format } from '~lib/format';\nimport { other } from 'lib/format';\n",
        ),
        ("elsewhere/x.ts", "import { format } from '~lib/format';\n"),
    ]);
    let reg = checkout.registry();
    assert_eq!(
        internal_files(&import(&reg, "web/src/app/show.ts", "~lib/format").target),
        ["web/src/lib/format.ts"]
    );
    // The base URL resolves a path-like specifier too.
    assert_eq!(
        internal_files(&import(&reg, "web/src/app/show.ts", "lib/format").target),
        ["web/src/lib/format.ts"]
    );
    // A file outside the configuration's directory takes none of its aliases.
    assert_eq!(
        import(&reg, "elsewhere/x.ts", "~lib/format").target,
        ImportTarget::External
    );
    let scope = &reg.resolution_metadata().alias_scopes[0];
    assert_eq!(scope.dir_prefix, "web");
    assert_eq!(scope.base_url.as_deref(), Some("web/src"));
    assert_eq!(scope.aliases[0].alias_prefix, "~lib/");
    assert_eq!(scope.aliases[0].target_prefix, "web/src/lib/");
}

#[test]
fn ambiguous_internal_import_stays_ambiguous() {
    let checkout = Checkout::new(&[
        ("src/a.h", "int a(void);\n"),
        ("lib/a.h", "int b(void);\n"),
        (
            "tools/main.c",
            "#include \"a.h\"\nint main(void) { return 0; }\n",
        ),
        ("web/thing.ts", "export const t = 1;\n"),
        ("web/thing.js", "export const t = 1;\n"),
        ("web/use.ts", "import { t } from './thing';\n"),
    ]);
    let reg = checkout.registry();
    match &import(&reg, "tools/main.c", "a.h").target {
        ImportTarget::InternalCandidates(c) => assert_eq!(c.len(), 2),
        other => panic!("{other:?}"),
    }
    match &import(&reg, "web/use.ts", "./thing").target {
        ImportTarget::InternalCandidates(c) => assert_eq!(c.len(), 2),
        other => panic!("{other:?}"),
    }
}

#[test]
fn go_same_module_import_internal() {
    let checkout = Checkout::new(&[
        ("go.mod", "module example.com/acme\n"),
        (
            "internal/store/store.go",
            "package store\n\nfunc Get() int { return 1 }\n",
        ),
        (
            "cmd/app/main.go",
            "package main\n\nimport (\n\t\"example.com/acme/internal/store\"\n\t\"example.com/other/lib\"\n)\n\nfunc main() { store.Get(); lib.X() }\n",
        ),
        ("tools/go.mod", "module example.com/acme/tools\n"),
        ("tools/gen/gen.go", "package gen\n\nfunc Gen() {}\n"),
        (
            "tools/run.go",
            "package tools\n\nimport \"example.com/acme/tools/gen\"\n\nfunc Run() { gen.Gen() }\n",
        ),
    ]);
    let reg = checkout.registry();
    let store = import(&reg, "cmd/app/main.go", "example.com/acme/internal/store");
    assert_eq!(internal_files(&store.target), ["internal/store/store.go"]);
    assert_eq!(
        import(&reg, "cmd/app/main.go", "example.com/other/lib").target,
        ImportTarget::External
    );
    // A nested module owns its own prefix.
    assert_eq!(
        reg.module_of_file("tools/gen/gen.go"),
        Some(&module("go", "tools", "example.com/acme/tools/gen"))
    );
    assert_eq!(
        internal_files(&import(&reg, "tools/run.go", "example.com/acme/tools/gen").target),
        ["tools/gen/gen.go"]
    );
}

#[test]
fn rust_path_dependency_internal_and_std_external() {
    let checkout = Checkout::new(&[
        (
            "Cargo.toml",
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[workspace]\nmembers = [\"libs/core\"]\n\n[dependencies]\ncore-lib = { path = \"libs/core\" }\nanyhow = \"1\"\n",
        ),
        (
            "libs/core/Cargo.toml",
            "[package]\nname = \"core-lib\"\nversion = \"0.1.0\"\n",
        ),
        ("libs/core/src/lib.rs", "pub mod math;\n"),
        (
            "libs/core/src/math.rs",
            "pub fn add(a: u32, b: u32) -> u32 { a + b }\n",
        ),
        (
            "src/main.rs",
            "use core_lib::math::add;\nuse core::fmt;\nuse alloc::vec::Vec;\nuse anyhow::Result;\n\nfn main() { add(1, 2); }\n",
        ),
    ]);
    let reg = checkout.registry();
    let add = import(&reg, "src/main.rs", "core_lib::math::add");
    assert_eq!(internal_files(&add.target), ["libs/core/src/math.rs"]);
    assert_eq!(internal(&add.target).member.as_deref(), Some("add"));
    for external in ["core::fmt", "alloc::vec::Vec", "anyhow::Result"] {
        assert_eq!(
            import(&reg, "src/main.rs", external).target,
            ImportTarget::External,
            "{external}"
        );
    }
    let manifest = reg.resolution_metadata().crate_manifest.as_ref().unwrap();
    assert_eq!(manifest.package_name.as_deref(), Some("app"));
    assert!(manifest.is_workspace_root);
    assert_eq!(manifest.member_paths, ["libs/core"]);
    let dependency = |name: &str| {
        manifest
            .dependencies
            .iter()
            .find(|d| d.name == name)
            .unwrap()
            .path
            .clone()
    };
    assert_eq!(dependency("core-lib").as_deref(), Some("libs/core"));
    assert_eq!(dependency("anyhow"), None);
}

#[test]
fn c_header_source_pairing() {
    let checkout = Checkout::new(&[
        ("src/a.h", "int a(void);\n"),
        ("src/a.c", "#include \"a.h\"\nint a(void) { return 1; }\n"),
        ("src/lone.h", "int lone(void);\n"),
        ("other/a.c", "int other(void) { return 2; }\n"),
    ]);
    let reg = checkout.registry();
    assert_eq!(reg.paired_file("src/a.h"), Some("src/a.c"));
    assert_eq!(reg.paired_file("src/a.c"), Some("src/a.h"));
    assert_eq!(reg.paired_file("src/lone.h"), None);
    assert_eq!(
        reg.paired_file("other/a.c"),
        None,
        "pairs never cross directories"
    );
}

#[test]
fn fresh_and_cached_extractions_build_same_registry() {
    let checkout = Checkout::new(&[
        ("go.mod", "module example.com/acme\n"),
        ("pkg/a/a.go", "package a\n\nfunc A() {}\n"),
        (
            "pkg/b/b.go",
            "package b\n\nimport \"example.com/acme/pkg/a\"\n\nfunc B() { a.A() }\n",
        ),
        ("app/__init__.py", ""),
        ("app/base.py", "class Base:\n    pass\n"),
        (
            "app/user.py",
            "from .base import Base\n\n\nclass User(Base):\n    pass\n",
        ),
        (
            "src/com/acme/Shop.java",
            "package com.acme;\nclass Shop extends Thing implements Runnable { public void run() {} }\n",
        ),
        (
            "src/com/acme/Thing.java",
            "package com.acme;\nclass Thing {}\n",
        ),
        (
            "src/geo.cpp",
            "namespace geo { class S {}; class C : public S {}; }\n",
        ),
    ]);
    let files = checkout.discover();
    let cache_dir = tempfile::tempdir().unwrap();
    let cache = FsCache::at(cache_dir.path().join("extract"));
    let fresh_report = checkout.extract(&files, Some(&cache));
    assert_eq!(fresh_report.stats.cache_hits, 0);
    let cached_report = checkout.extract(&files, Some(&cache));
    assert!(cached_report.stats.cache_hits > 0);
    assert_eq!(cached_report.stats.engine_files, 0, "not from the cache");
    let fresh = SymbolRegistry::build(checkout.root(), &files, fresh_report).unwrap();
    let cached = SymbolRegistry::build(checkout.root(), &files, cached_report).unwrap();
    assert_eq!(fresh, cached);
    // And what the cache had to carry is there.
    let shop = def(&cached, "src/com/acme/Shop.java", "Shop");
    assert_eq!(
        base_defs(&cached, &shop),
        [def(&cached, "src/com/acme/Thing.java", "Thing")]
    );
    assert_eq!(
        cached.module_of_file("src/com/acme/Shop.java"),
        Some(&module("java", "", "com.acme"))
    );
    let user = def(&cached, "app/user.py", "User");
    assert_eq!(
        cached.ancestors(&user),
        [def(&cached, "app/base.py", "Base")]
    );
}

/// Resolves the files of `paths` in the engine with the registry's metadata, and
/// returns the file each named callee resolves to.
fn engine_targets(
    checkout: &Checkout,
    reg: &SymbolRegistry,
    paths: &[&str],
    calls: &[(&str, &str)],
) -> Vec<String> {
    let engine = pdx_engine::Engine::new().unwrap();
    let mut project = pdx_engine::ProjectResolver::new(&engine).unwrap();
    project.set_metadata(reg.resolution_metadata()).unwrap();
    for path in paths {
        let source = fs::read(checkout.root().join(path)).unwrap();
        project
            .add_file(reg.extract(path).unwrap(), &source)
            .unwrap();
    }
    let run = project.run().unwrap();
    calls
        .iter()
        .map(|(file, callee)| {
            run.resolutions
                .iter()
                .find(|r| r.site_ref.rel_path == *file && r.site.callee_text.ends_with(callee))
                .and_then(|r| r.target_rel_path.clone())
                .unwrap_or_else(|| {
                    panic!("the engine did not resolve {callee}: {:?}", run.resolutions)
                })
        })
        .collect()
}

#[test]
fn engine_metadata_matches_registry_modules() {
    let checkout = Checkout::new(&[
        ("go.mod", "module example.com/acme\n"),
        ("pkg/a/a.go", "package a\n\nfunc Hello() int { return 1 }\n"),
        (
            "pkg/b/b.go",
            "package b\n\nimport \"example.com/acme/pkg/a\"\n\nfunc Use() int { return a.Hello() }\n",
        ),
        (
            "packages/lib/package.json",
            "{ \"name\": \"@acme/lib\", \"main\": \"index.ts\" }\n",
        ),
        ("packages/lib/index.ts", "export const lib = 1;\n"),
        ("app/z.ts", "import { lib } from '@acme/lib';\n"),
        (
            "src/main/java/com/acme/Shop.java",
            "package com.acme;\nclass Shop {}\n",
        ),
        (
            "Cargo.toml",
            "[package]\nname = \"root\"\nversion = \"0.1.0\"\n",
        ),
        ("src/lib.rs", "pub fn f() {}\n"),
    ]);
    let reg = checkout.registry();
    let metadata = reg.resolution_metadata();
    let entry = |prefix: &str| -> Vec<&str> {
        metadata
            .packages
            .iter()
            .filter(|p| p.import_prefix == prefix)
            .map(|p| p.entry_path.as_str())
            .collect()
    };
    // The Go module the registry keys packages under, at its root.
    assert_eq!(entry("example.com/acme"), [""]);
    assert_eq!(
        reg.module_of_file("pkg/a/a.go").map(|m| m.name.as_str()),
        Some("example.com/acme/pkg/a")
    );
    // The package `package.json` names, at the entry the registry resolves it to.
    assert_eq!(entry("@acme/lib"), ["packages/lib/index.ts"]);
    assert_eq!(
        internal_files(&import(&reg, "app/z.ts", "@acme/lib").target),
        ["packages/lib/index.ts"]
    );
    // The declared Java package, at the directory of the files declaring it.
    assert_eq!(entry("com.acme"), ["src/main/java/com/acme"]);
    assert!(
        reg.modules()
            .any(|m| m.language == "java" && m.name == "com.acme")
    );
    // The root crate manifest.
    assert_eq!(
        metadata
            .crate_manifest
            .as_ref()
            .and_then(|m| m.package_name.as_deref()),
        Some("root")
    );
    // The engine, given that metadata, resolves the call through the Go import to the
    // file the registry's import leads to.
    assert_eq!(
        engine_targets(
            &checkout,
            &reg,
            &["pkg/a/a.go", "pkg/b/b.go"],
            &[("pkg/b/b.go", "Hello")]
        ),
        internal_files(&import(&reg, "pkg/b/b.go", "example.com/acme/pkg/a").target)
    );

    // The alias scope the registry resolves with is the one the engine gets, and both
    // take an aliased import to the same file. Both mechanisms in one repository are
    // `polyglot_ts_alias_does_not_affect_go` (issue 43).
    let ts = Checkout::new(&[
        (
            "tsconfig.json",
            "{ \"compilerOptions\": { \"baseUrl\": \".\", \"paths\": { \"@/*\": [\"src/*\"] } } }\n",
        ),
        (
            "src/lib/format.ts",
            "export function formatName(s: string): string { return s.trim(); }\n",
        ),
        (
            "src/app/show.ts",
            "import { formatName } from '@/lib/format';\nexport function show(): string { return formatName(' x '); }\n",
        ),
    ]);
    let reg = ts.registry();
    let scope = &reg.resolution_metadata().alias_scopes[0];
    assert_eq!(
        (scope.dir_prefix.as_str(), scope.base_url.as_deref()),
        ("", Some("."))
    );
    assert_eq!(scope.aliases[0].target_prefix, "src/");
    assert_eq!(
        engine_targets(
            &ts,
            &reg,
            &["src/lib/format.ts", "src/app/show.ts"],
            &[("src/app/show.ts", "formatName")]
        ),
        internal_files(&import(&reg, "src/app/show.ts", "@/lib/format").target)
    );
}

#[test]
fn polyglot_ts_alias_does_not_affect_go() {
    // One repository with a root `tsconfig.json` that sets a base URL and an alias, a
    // TypeScript file importing through the alias, and a Go module importing its own
    // package (issue 43). The alias resolves TypeScript's import; Go's import is not
    // rewritten by it; and the engine and the registry agree on both, with the
    // configuration present or not.
    let go_files = [
        ("go.mod", "module example.com/acme\n"),
        ("pkg/a/a.go", "package a\n\nfunc Hello() int { return 1 }\n"),
        (
            "pkg/b/b.go",
            "package b\n\nimport \"example.com/acme/pkg/a\"\n\nfunc Use() int { return a.Hello() }\n",
        ),
    ];
    let ts_files = [
        (
            "tsconfig.json",
            "{ \"compilerOptions\": { \"baseUrl\": \".\", \"paths\": { \"@/*\": [\"src/*\"] } } }\n",
        ),
        (
            "src/lib/format.ts",
            "export function formatName(s: string): string { return s.trim(); }\n",
        ),
        (
            "src/app/show.ts",
            "import { formatName } from '@/lib/format';\nexport function show(): string { return formatName(' x '); }\n",
        ),
    ];
    let go_paths = ["pkg/a/a.go", "pkg/b/b.go"];
    let go_call = [("pkg/b/b.go", "Hello")];

    // Go alone: the baseline.
    let alone = Checkout::new(&go_files);
    let reg = alone.registry();
    let baseline = engine_targets(&alone, &reg, &go_paths, &go_call);
    assert_eq!(baseline, ["pkg/a/a.go"]);

    // The same Go module beside a root configuration with a base URL.
    let files: Vec<(&str, &str)> = go_files.iter().chain(&ts_files).copied().collect();
    let polyglot = Checkout::new(&files);
    let reg = polyglot.registry();
    let scope = &reg.resolution_metadata().alias_scopes[0];
    assert_eq!(
        (scope.dir_prefix.as_str(), scope.base_url.as_deref()),
        ("", Some("."))
    );
    let paths = [
        "pkg/a/a.go",
        "pkg/b/b.go",
        "src/app/show.ts",
        "src/lib/format.ts",
    ];
    let calls = [("pkg/b/b.go", "Hello"), ("src/app/show.ts", "formatName")];
    let engine = engine_targets(&polyglot, &reg, &paths, &calls);
    assert_eq!(engine, ["pkg/a/a.go", "src/lib/format.ts"]);
    assert_eq!(
        engine[0], baseline[0],
        "the configuration changed Go's answer"
    );
    // The registry reads both imports as the engine does.
    assert_eq!(
        internal_files(&import(&reg, "pkg/b/b.go", "example.com/acme/pkg/a").target),
        [engine[0].as_str()]
    );
    assert_eq!(
        internal_files(&import(&reg, "src/app/show.ts", "@/lib/format").target),
        [engine[1].as_str()]
    );
}

#[test]
fn registry_handles_every_matrix_language() {
    // One file of every language of the matrix: each registers, and its module
    // follows its rule, or is absent where the rule's evidence is.
    let smoke = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../engine/tests/fixtures/smoke");
    let mut files = Vec::new();
    for entry in fs::read_dir(smoke).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        files.push((format!("m/{name}"), fs::read_to_string(&path).unwrap()));
    }
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, c)| (p.as_str(), c.as_str()))
        .collect();
    let checkout = Checkout::new(&refs);
    let reg = checkout.registry();
    let mut rules_seen = BTreeSet::new();
    for path in reg.files().map(str::to_owned).collect::<Vec<_>>() {
        let Some(language) = reg.language(&path) else {
            continue;
        };
        rules_seen.insert(format!(
            "{:?}",
            std::mem::discriminant(&language.module_rule)
        ));
        let modules = reg.modules_of_file(&path);
        match language.module_rule {
            ModuleRule::Directory
            | ModuleRule::DirectoryWithPathMappings { .. }
            | ModuleRule::DirectoryPairingHeaders => {
                assert_eq!(modules, [module(language.id, "", "m")], "{path}");
            }
            ModuleRule::File | ModuleRule::FileSectionsAsDocs | ModuleRule::FileKeysOnly => {
                assert_eq!(modules, [module(language.id, "", &path)], "{path}");
            }
            // No go.mod, no crate manifest: no module path, no crate.
            ModuleRule::DirectoryUnderModulePath { .. } | ModuleRule::ModTree { .. } => {
                assert!(modules.is_empty(), "{path}: {modules:?}");
            }
            ModuleRule::DottedPath { .. } => {
                assert_eq!(modules.len(), 1, "{path}");
            }
            ModuleRule::Declaration(_) | ModuleRule::ModuleClassNesting | ModuleRule::UnitName => {
                // Deterministic, whatever the evidence: built twice, the same.
                assert_eq!(
                    modules,
                    checkout.registry().modules_of_file(&path),
                    "{path}"
                );
            }
        }
    }
    let all_rules: BTreeSet<String> = languages::all()
        .iter()
        .map(|l| format!("{:?}", std::mem::discriminant(&l.module_rule)))
        .collect();
    assert_eq!(
        rules_seen, all_rules,
        "a module rule of the matrix was not exercised"
    );
}

#[test]
fn every_module_rule_variant() {
    // Each rule the matrix uses, with its evidence present.
    let checkout = Checkout::new(&[
        // Declaration from the file, and from qualified names.
        ("j/A.java", "package a.b;\nclass A {}\n"),
        ("c/n.cpp", "namespace x::y { struct S {}; }\n"),
        // Declaration the engine does not record.
        ("s/A.scala", "package a.b\nclass A\n"),
        // Module and class nesting.
        (
            "r/shop.rb",
            "module Acme\n  class Shop\n    def run; end\n  end\nend\n",
        ),
        // Unit name, from a package specification.
        (
            "ada/acme-shop.ads",
            "package Acme.Shop is\n   procedure Run;\nend Acme.Shop;\n",
        ),
        // File-level rules.
        ("docs/guide.md", "# Guide\n"),
        ("conf/app.properties", "key=value\n"),
    ]);
    let reg = checkout.registry();
    assert_eq!(reg.modules_of_file("j/A.java"), [module("java", "", "a.b")]);
    assert_eq!(reg.modules_of_file("c/n.cpp"), [module("cpp", "", "x::y")]);
    assert_eq!(reg.modules_of_file("s/A.scala"), []);
    let run = def(&reg, "r/shop.rb", "run");
    assert_eq!(
        reg.module_of_definition(&run),
        Some(&module("ruby", "", "Acme::Shop"))
    );
    assert_eq!(
        reg.module_of_definition(&def(&reg, "r/shop.rb", "Shop")),
        Some(&module("ruby", "", "Acme"))
    );
    assert_eq!(
        reg.modules_of_file("ada/acme-shop.ads"),
        [module("ada", "", "Acme.Shop")]
    );
    assert_eq!(
        reg.modules_of_file("docs/guide.md"),
        [module("markdown", "", "docs/guide.md")]
    );
    assert_eq!(
        reg.modules_of_file("conf/app.properties"),
        [module("properties", "", "conf/app.properties")]
    );
}

// --- what the registry reads, and refuses ---------------------------------------------

#[test]
fn stages_that_disagree_are_refused() {
    let checkout = Checkout::new(&[("a.py", "x = 1\n"), ("b.py", "y = 2\n")]);
    let files = checkout.discover();
    let report = checkout.extract(&files, None);
    // A file Stage 2 never saw.
    let mut fewer = report.clone();
    fewer.files.pop();
    assert!(matches!(
        SymbolRegistry::build(checkout.root(), &files, fewer),
        Err(RegistryError::StageMismatch { .. })
    ));
    // A file Stage 1 never found.
    assert!(matches!(
        SymbolRegistry::build(checkout.root(), &files[..1], report.clone()),
        Err(RegistryError::StageMismatch { .. })
    ));
    // An extraction of another file under this one's path.
    let mut swapped = report;
    let other = swapped.files[1].outcome.clone();
    swapped.files[0].outcome = other;
    assert!(matches!(
        SymbolRegistry::build(checkout.root(), &files, swapped),
        Err(RegistryError::StageMismatch { .. })
    ));
}

#[test]
fn malformed_extractions_are_refused() {
    let checkout = Checkout::new(&[("a.py", "class A:\n    def m(self):\n        return f()\n")]);
    let files = checkout.discover();
    let report = checkout.extract(&files, None);
    let corrupt = |change: &dyn Fn(&mut pdx_engine::FileExtract)| {
        let mut r = report.clone();
        if let FileOutcome::Extracted { extract, .. } = &mut r.files[0].outcome {
            change(extract);
        }
        SymbolRegistry::build(checkout.root(), &files, r)
    };
    let n = u32::try_from(match &report.files[0].outcome {
        FileOutcome::Extracted { extract, .. } => extract.definitions.len(),
        _ => panic!("not extracted"),
    })
    .unwrap();
    // A parent past the end, a parent cycle, a call's caller past the end.
    assert!(matches!(
        corrupt(&|e| e.definitions[1].parent = Some(n)),
        Err(RegistryError::InvalidParent { .. })
    ));
    assert!(matches!(
        corrupt(&|e| {
            e.definitions[1].parent = Some(2);
            e.definitions[2].parent = Some(1);
        }),
        Err(RegistryError::InvalidParent { .. })
    ));
    assert!(matches!(
        corrupt(&|e| e.calls[0].caller = Some(n + 5)),
        Err(RegistryError::InvalidDefinitionIndex { .. })
    ));
}

#[test]
fn bad_metadata_is_an_error() {
    for (path, content) in [
        ("go.mod", "go 1.22\n"),
        ("tsconfig.json", "{ \"compilerOptions\": [ }\n"),
        ("Cargo.toml", "[package\n"),
    ] {
        let checkout = Checkout::new(&[(path, content), ("a.go", "package a\n")]);
        assert!(
            matches!(
                checkout.try_registry(),
                Err(RegistryError::MetadataParse { .. })
            ),
            "{path}"
        );
    }
}

#[test]
fn metadata_changed_since_extraction_is_an_error() {
    let checkout = Checkout::new(&[
        (
            "tsconfig.json",
            "{ \"compilerOptions\": { \"baseUrl\": \".\" } }\n",
        ),
        ("a.ts", "export const a = 1;\n"),
    ]);
    let files = checkout.discover();
    let report = checkout.extract(&files, None);
    // Same size, other bytes.
    checkout.write(
        "tsconfig.json",
        "{ \"compilerOptions\": { \"baseUrl\": \"/\" } }\n",
    );
    assert!(matches!(
        SymbolRegistry::build(checkout.root(), &files, report),
        Err(RegistryError::SourceChanged(_))
    ));
}

#[test]
fn redacted_metadata_is_never_opened() {
    // A repository can make a metadata file a secret: it is then absent to the
    // registry, never read. Deleting it after discovery proves it is not opened.
    let checkout = Checkout::new(&[
        ("pdx.toml", "[secrets]\npatterns = [\"tsconfig.json\"]\n"),
        (
            "tsconfig.json",
            "{ \"compilerOptions\": { \"paths\": { \"@/*\": [\"src/*\"] } } }\n",
        ),
        ("src/x.ts", "export const x = 1;\n"),
        ("src/y.ts", "import { x } from '@/x';\n"),
        (".env", "TOKEN=never-read\n"),
    ]);
    let files = checkout.discover();
    let report = checkout.extract(&files, None);
    fs::remove_file(checkout.root().join("tsconfig.json")).unwrap();
    fs::remove_file(checkout.root().join(".env")).unwrap();
    let reg = SymbolRegistry::build(checkout.root(), &files, report).unwrap();
    assert!(reg.resolution_metadata().alias_scopes.is_empty());
    assert_eq!(
        import(&reg, "src/y.ts", "@/x").target,
        ImportTarget::External
    );
}

#[cfg(unix)]
#[test]
fn metadata_symlinks_are_not_followed() {
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("go.mod"), "module example.com/evil\n").unwrap();
    let checkout = Checkout::new(&[
        ("go.mod", "module example.com/acme\n"),
        ("a/a.go", "package a\n"),
    ]);
    let files = checkout.discover();
    let report = checkout.extract(&files, None);
    fs::remove_file(checkout.root().join("go.mod")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("go.mod"),
        checkout.root().join("go.mod"),
    )
    .unwrap();
    assert!(matches!(
        SymbolRegistry::build(checkout.root(), &files, report),
        Err(RegistryError::MetadataRead { .. })
    ));
}

/// Every source file of the crate, with its text.
fn crate_sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<(String, String)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push((
                    path.display().to_string(),
                    fs::read_to_string(&path).unwrap(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
    assert!(out.len() > 10, "the scan found nothing");
    out
}

#[test]
fn the_crate_never_decodes_the_surface_or_runs_a_program() {
    // The surface is the engine's opaque encoding: `pdx-core` reads facts from the
    // extraction's own fields, never from it. And nothing here learns about a
    // repository by running its tools (`cargo`, `go`, `node`, a compiler): metadata
    // is read from files.
    for (path, text) in crate_sources() {
        for forbidden in [".surface", "Surface", "process::Command", "Command::new"] {
            assert!(!text.contains(forbidden), "{path} uses `{forbidden}`");
        }
    }
}
