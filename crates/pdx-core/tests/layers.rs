//! Layer roles (4.8.3, P2-09): each `Module` and `Class` node's `layer_role`, through
//! the whole pipeline (discovery, extraction, the registry, resolution and derivation)
//! over checkouts written here.

use std::collections::BTreeMap;
use std::fs;

use pdx_core::config::PdxConfig;
use pdx_core::ids::RepoId;
use pdx_core::index::derive::tests::is_test_path;
use pdx_core::index::derive::{DeriveInput, DerivedGraph, derive};
use pdx_core::index::discover::discover;
use pdx_core::index::extract::{ExtractLimits, ExtractStage};
use pdx_core::kinds::{LayerRole, NodeKind};
use pdx_core::languages;
use pdx_core::layers::roles::{FRAMEWORK_RULES, FrameworkRule, PATH_ROLES};
use pdx_core::resolve::registry::SymbolRegistry;
use pdx_core::resolve::stages::resolve;

const REPO: &str = "ce63551447285fd4";

/// The pipeline over files written to a fresh checkout, discovery's order reversed or
/// not.
fn derive_files(files: &[(&str, &str)], reverse: bool) -> DerivedGraph {
    let dir = tempfile::Builder::new()
        .prefix("pdx-layers-test-")
        .tempdir()
        .expect("a directory");
    for (path, content) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
        fs::write(&path, content).expect("a file");
    }
    let root = dir.path();
    let config = PdxConfig::load(root).expect("the configuration loads");
    let mut discovered = discover(root, &config).expect("discovery succeeds");
    if reverse {
        discovered.reverse();
    }
    let limits = ExtractLimits {
        requested_workers: if reverse { 1 } else { 2 },
        memory_budget_bytes: 64 << 20,
    };
    let extracted = ExtractStage::new(root, &config.secrets, limits)
        .run(&discovered)
        .expect("extraction succeeds");
    let registry =
        SymbolRegistry::build(root, &discovered, extracted).expect("the registry builds");
    let report = resolve(root, &registry).expect("resolution succeeds");
    derive(&DeriveInput {
        repo: &RepoId::parse(REPO).expect("a repo id"),
        repo_name: "fixture",
        registry: &registry,
        resolution: &report,
        root,
        config: &config,
    })
    .expect("derivation succeeds")
}

fn graph(files: &[(&str, &str)]) -> DerivedGraph {
    derive_files(files, false)
}

/// Every `Module` and `Class` node's role, by `"<Kind> <qualified name>"`.
fn roles(graph: &DerivedGraph) -> BTreeMap<String, &'static str> {
    graph
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Module | NodeKind::Class))
        .map(|n| {
            let role = n.layer_role.expect("a module or class has a role");
            (format!("{} {}", n.kind, n.qualified_name), role.as_str())
        })
        .collect()
}

/// The role of the one class named `name`.
fn class_role(graph: &DerivedGraph, name: &str) -> LayerRole {
    let classes: Vec<_> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Class && n.name == name)
        .collect();
    assert_eq!(classes.len(), 1, "one class {name}: {:#?}", roles(graph));
    classes[0].layer_role.expect("a class has a role")
}

/// The role of the one semantic module named `name` (its qualified name is
/// `<language>:<name>`).
fn module_role(graph: &DerivedGraph, name: &str) -> LayerRole {
    let modules: Vec<_> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Module && n.name == name && n.qualified_name.contains(':'))
        .collect();
    assert_eq!(modules.len(), 1, "one module {name}: {:#?}", roles(graph));
    modules[0].layer_role.expect("a module has a role")
}

fn toml_rules(rules: &[(&str, &str)]) -> String {
    let rules: Vec<String> = rules
        .iter()
        .map(|(glob, role)| {
            format!("{{ match = {{ path_glob = \"{glob}\" }}, role = \"{role}\" }}")
        })
        .collect();
    format!("[layers]\nrules = [\n  {}\n]\n", rules.join(",\n  "))
}

// --- precedence -------------------------------------------------------------------------

const SPRING_SERVICE: &str = "package com.acme.web;\n\nimport org.springframework.stereotype.Service;\n\n@Service\npublic class Orders {\n    public int count() { return 0; }\n}\n";
const PLAIN_CLASS: &str =
    "package com.acme.web;\n\npublic class Orders {\n    public int count() { return 0; }\n}\n";
const FASTAPI_MODULE: &str = "from fastapi import FastAPI\n\napp = FastAPI()\n\n\n@app.get(\"/orders\")\ndef list_orders():\n    return []\n";
const PLAIN_MODULE: &str = "def list_orders():\n    return []\n";

#[test]
fn layer_role_precedence() {
    // A class whose path says `api` (`web`), whose annotation says `service` (Spring's
    // `@Service`, imported) and whose configuration says `persistence`; and a Python
    // module whose path says `service` (`services`), whose FastAPI handler says `api`
    // and whose configuration says `domain`.
    let java = "src/main/java/com/acme/web/Orders.java";
    let python = "app/services/orders.py";
    let config = toml_rules(&[
        ("src/main/java/com/acme/web/**", "persistence"),
        ("app/services/**", "domain"),
    ]);
    let init = ("app/__init__.py", "");
    let services_init = ("app/services/__init__.py", "");

    // Configuration beats the annotation and the path.
    let g = graph(&[
        ("pdx.toml", &config),
        (java, SPRING_SERVICE),
        init,
        services_init,
        (python, FASTAPI_MODULE),
    ]);
    assert_eq!(class_role(&g, "Orders"), LayerRole::Persistence);
    assert_eq!(module_role(&g, "app.services.orders"), LayerRole::Domain);

    // Without it, the annotation beats the path.
    let g = graph(&[
        (java, SPRING_SERVICE),
        init,
        services_init,
        (python, FASTAPI_MODULE),
    ]);
    assert_eq!(class_role(&g, "Orders"), LayerRole::Service);
    assert_eq!(module_role(&g, "app.services.orders"), LayerRole::Api);

    // Without that, the path beats unknown.
    let g = graph(&[
        (java, PLAIN_CLASS),
        init,
        services_init,
        (python, PLAIN_MODULE),
    ]);
    assert_eq!(class_role(&g, "Orders"), LayerRole::Api);
    assert_eq!(module_role(&g, "app.services.orders"), LayerRole::Service);

    // With none of them: unknown, stored, never left empty.
    let g = graph(&[
        (
            "src/main/java/com/acme/shop/Orders.java",
            &PLAIN_CLASS.replace("web", "shop"),
        ),
        init,
        ("app/shop/__init__.py", ""),
        ("app/shop/orders.py", PLAIN_MODULE),
    ]);
    assert_eq!(class_role(&g, "Orders"), LayerRole::Unknown);
    assert_eq!(module_role(&g, "app.shop.orders"), LayerRole::Unknown);
    assert_eq!(module_role(&g, "com.acme.shop"), LayerRole::Unknown);
}

#[test]
fn layer_role_first_config_rule_wins() {
    let java = |package: &str| format!("package com.acme.{package};\n\npublic class Thing {{}}\n");
    // Two rules match: the first wins, a configured `unknown` included, over the path
    // (`api`) and a later rule.
    let files = |config: &str| {
        graph(&[
            ("pdx.toml", config),
            ("src/main/java/com/acme/api/Thing.java", &java("api")),
        ])
    };
    let g = files(&toml_rules(&[
        ("src/**", "domain"),
        ("src/main/**", "service"),
    ]));
    assert_eq!(class_role(&g, "Thing"), LayerRole::Domain);
    assert_eq!(module_role(&g, "com.acme.api"), LayerRole::Domain);
    let g = files(&toml_rules(&[
        ("src/main/**", "service"),
        ("src/**", "domain"),
    ]));
    assert_eq!(class_role(&g, "Thing"), LayerRole::Service);
    let g = files(&toml_rules(&[
        ("**/api/*.java", "unknown"),
        ("src/**", "domain"),
    ]));
    assert_eq!(class_role(&g, "Thing"), LayerRole::Unknown);
    // A rule that matches nothing decides nothing: the path does.
    let g = files(&toml_rules(&[("lib/**", "domain")]));
    assert_eq!(class_role(&g, "Thing"), LayerRole::Api);
    // Globs are matched case-sensitively, against the repository-relative path, never
    // a qualified name or a package.
    let g = files(&toml_rules(&[
        ("SRC/**", "domain"),
        ("com/acme/**", "service"),
    ]));
    assert_eq!(class_role(&g, "Thing"), LayerRole::Api);
    // A module is matched by its member files, sorted: the first rule matching any of
    // them wins, whichever file it matches.
    let g = graph(&[
        (
            "pdx.toml",
            &toml_rules(&[("lib/**", "port"), ("src/**", "adapter")]),
        ),
        (
            "src/main/java/com/acme/x/A.java",
            "package com.acme.x;\n\npublic class A {}\n",
        ),
        (
            "lib/com/acme/x/B.java",
            "package com.acme.x;\n\npublic class B {}\n",
        ),
    ]);
    assert_eq!(module_role(&g, "com.acme.x"), LayerRole::Port);
    assert_eq!(class_role(&g, "A"), LayerRole::Adapter);
    assert_eq!(class_role(&g, "B"), LayerRole::Port);
}

// --- framework evidence -----------------------------------------------------------------

/// One class, module or handler per framework rule, each with its provenance.
const FRAMEWORK_FILES: [(&str, &str); 20] = [
    (
        "src/main/java/com/acme/shop/A.java",
        "package com.acme.shop;\n\nimport org.springframework.web.bind.annotation.RestController;\n@RestController\npublic class A {}\n",
    ),
    (
        "src/main/java/com/acme/shop/B.java",
        "package com.acme.shop;\n\nimport org.springframework.stereotype.Controller;\n@Controller\npublic class B {}\n",
    ),
    (
        "src/main/java/com/acme/shop/C.java",
        "package com.acme.shop;\n\nimport org.springframework.stereotype.Service;\n@Service\npublic class C {}\n",
    ),
    (
        "src/main/java/com/acme/shop/D.java",
        "package com.acme.shop;\n\nimport org.springframework.stereotype.Repository;\n@Repository\npublic class D {}\n",
    ),
    (
        "src/main/java/com/acme/shop/E.java",
        "package com.acme.shop;\n\n\n@org.springframework.stereotype.Service\npublic class E {}\n",
    ),
    (
        "src/main/java/com/acme/shop/F.java",
        "package com.acme.shop;\n\nimport org.springframework.stereotype.*;\n@Repository\npublic class F {}\n",
    ),
    (
        "src/main/java/com/acme/shop/G.java",
        "package com.acme.shop;\n\nimport org.jmolecules.architecture.hexagonal.Port;\n@Port\npublic class G {}\n",
    ),
    (
        "src/main/java/com/acme/shop/H.java",
        "package com.acme.shop;\n\nimport org.jmolecules.architecture.hexagonal.Adapter;\n@Adapter\npublic class H {}\n",
    ),
    (
        "src/main/java/com/acme/shop/I.java",
        "package com.acme.shop;\n\nimport javax.persistence.Entity;\n@Entity\npublic class I {}\n",
    ),
    (
        "src/main/kotlin/com/acme/shop/K.kt",
        "package com.acme.shop\n\nimport org.springframework.stereotype.Service\n\n@Service\nclass K\n",
    ),
    (
        "web-app/src/orders.controller.ts",
        "import { Controller, Get } from '@nestjs/common';\n\n@Controller('orders')\nexport class OrdersController {\n  @Get()\n  list() { return []; }\n}\n",
    ),
    (
        "web-app/src/orders.service.ts",
        "import { Injectable } from '@nestjs/common';\n\n@Injectable()\nexport class OrdersService {}\n",
    ),
    (
        "web-app/src/orders.helper.ts",
        "import { Injectable } from '@nestjs/common';\n\n@Injectable()\nexport class OrdersHelper {}\n",
    ),
    (
        "Shop/OrdersApi.cs",
        "using Microsoft.AspNetCore.Mvc;\n\nnamespace Shop\n{\n    [ApiController]\n    [Route(\"orders\")]\n    public class OrdersApi : ControllerBase {}\n}\n",
    ),
    (
        "Shop/ShopContext.cs",
        "using Microsoft.EntityFrameworkCore;\n\nnamespace Shop\n{\n    public class ShopContext : DbContext {}\n}\n",
    ),
    (
        "Shop/OtherContext.cs",
        "namespace Shop\n{\n    public class OtherContext : Microsoft.EntityFrameworkCore.DbContext {}\n}\n",
    ),
    ("pyapp/__init__.py", ""),
    (
        "pyapp/views.py",
        "from flask import Flask\n\napp = Flask(__name__)\n\n\n@app.route(\"/orders\")\ndef orders():\n    return []\n",
    ),
    (
        "pyapp/models.py",
        "from sqlalchemy.orm import declarative_base\n\nBase = declarative_base()\n\n\nclass Order(Base):\n    __tablename__ = \"orders\"\n",
    ),
    (
        "pyapp/resources.py",
        "from fastapi import APIRouter\n\nrouter = APIRouter()\n\n\nclass OrderViews:\n    @router.get(\"/orders/{order_id}\")\n    def show(self, order_id: int):\n        return order_id\n",
    ),
];

#[test]
fn layer_roles_framework_annotations() {
    let g = graph(&FRAMEWORK_FILES);
    // Spring, bare with its import, qualified, and through a wildcard.
    assert_eq!(class_role(&g, "A"), LayerRole::Api);
    assert_eq!(class_role(&g, "B"), LayerRole::Api);
    assert_eq!(class_role(&g, "C"), LayerRole::Service);
    assert_eq!(class_role(&g, "D"), LayerRole::Persistence);
    assert_eq!(class_role(&g, "E"), LayerRole::Service);
    assert_eq!(class_role(&g, "F"), LayerRole::Persistence);
    assert_eq!(class_role(&g, "K"), LayerRole::Service);
    // jMolecules.
    assert_eq!(class_role(&g, "G"), LayerRole::Port);
    assert_eq!(class_role(&g, "H"), LayerRole::Adapter);
    // JPA `@Entity` alone is not "with repository usage": the usage is a repository's
    // base-class type argument, which the facts do not keep (issue 64).
    assert_eq!(class_role(&g, "I"), LayerRole::Unknown);
    // NestJS: `@Injectable` makes a service only in `*.service.ts`.
    assert_eq!(class_role(&g, "OrdersController"), LayerRole::Api);
    assert_eq!(class_role(&g, "OrdersService"), LayerRole::Service);
    assert_eq!(class_role(&g, "OrdersHelper"), LayerRole::Unknown);
    // ASP.NET and Entity Framework.
    assert_eq!(class_role(&g, "OrdersApi"), LayerRole::Api);
    assert_eq!(class_role(&g, "ShopContext"), LayerRole::Persistence);
    assert_eq!(class_role(&g, "OtherContext"), LayerRole::Persistence);
    // Flask's top-level handler gives its module `api`; FastAPI's handler in a class
    // gives the class `api`; a SQLAlchemy model is `persistence`. The handlers
    // themselves have no role.
    assert_eq!(module_role(&g, "pyapp.views"), LayerRole::Api);
    assert_eq!(class_role(&g, "OrderViews"), LayerRole::Api);
    assert_eq!(module_role(&g, "pyapp.resources"), LayerRole::Unknown);
    assert_eq!(class_role(&g, "Order"), LayerRole::Persistence);
    for n in &g.nodes {
        if matches!(n.name.as_str(), "orders" | "show" | "list") {
            assert_eq!(n.layer_role, None, "{}", n.qualified_name);
        }
    }
}

#[test]
fn layer_role_framework_tie_break_follows_4_8_3_order() {
    // 4.8.3 lists the framework rules in an order; a class two of them match takes the
    // first listed, whatever order its annotations are written in.
    assert_eq!(FRAMEWORK_RULES[0], FrameworkRule::SpringController);
    assert_eq!(FRAMEWORK_RULES[1], FrameworkRule::SpringService);
    assert_eq!(FRAMEWORK_RULES[2], FrameworkRule::SpringRepository);
    let java = |annotations: &str, name: &str| {
        format!(
            "package com.acme.shop;\n\nimport org.springframework.stereotype.*;\n\n{annotations}\npublic class {name} {{}}\n"
        )
    };
    let g = graph(&[
        (
            "src/main/java/com/acme/shop/A.java",
            &java("@Repository\n@Service", "A"),
        ),
        (
            "src/main/java/com/acme/shop/B.java",
            &java("@Service\n@Repository", "B"),
        ),
        (
            "src/main/java/com/acme/shop/C.java",
            &java("@Repository\n@Controller", "C"),
        ),
    ]);
    assert_eq!(class_role(&g, "A"), LayerRole::Service);
    assert_eq!(class_role(&g, "B"), LayerRole::Service);
    assert_eq!(class_role(&g, "C"), LayerRole::Api);
}

#[test]
fn layer_role_false_positive_annotations_are_withheld() {
    let g = graph(&[
        // The repository's own `@Service` and `@Controller`, no Spring anywhere.
        (
            "src/main/java/com/acme/shop/Service.java",
            "package com.acme.shop;\n\npublic @interface Service {}\n",
        ),
        (
            "src/main/java/com/acme/shop/Controller.java",
            "package com.acme.shop;\n\npublic @interface Controller {}\n",
        ),
        (
            "src/main/java/com/acme/shop/A.java",
            "package com.acme.shop;\n\n@Service\npublic class A {}\n",
        ),
        (
            "src/main/java/com/acme/shop/B.java",
            "package com.acme.shop;\n\n@Controller\npublic class B {}\n",
        ),
        // Spring's wildcard does not reach a name the repository declares itself, and
        // an annotation qualified by another package is not Spring's.
        (
            "src/main/java/com/acme/shop/C.java",
            "package com.acme.shop;\n\nimport org.springframework.stereotype.*;\n\n@Service\npublic class C {}\n",
        ),
        (
            "src/main/java/com/acme/shop/D.java",
            "package com.acme.shop;\n\nimport org.springframework.stereotype.Component;\n\n@com.acme.shop.Service\npublic class D {}\n",
        ),
        // Importing something else from Spring's package does not import `Service`.
        (
            "src/main/java/com/acme/other/E.java",
            "package com.acme.other;\n\nimport org.springframework.stereotype.Component;\n\n@Repository\npublic class E {}\n",
        ),
        // A local `Port` with no jMolecules.
        (
            "src/main/java/com/acme/other/F.java",
            "package com.acme.other;\n\n@Port\npublic class F {}\n",
        ),
        // A local `@Injectable` in a `*.service.ts`, and a local `@Controller`.
        (
            "web-app/src/di.ts",
            "export function Injectable() { return (t: any) => t; }\nexport function Controller(p: string) { return (t: any) => t; }\n",
        ),
        (
            "web-app/src/orders.service.ts",
            "import { Injectable } from './di';\n\n@Injectable()\nexport class OrdersService {}\n",
        ),
        (
            "web-app/src/orders.ts",
            "import { Controller } from './di';\n\n@Controller('orders')\nexport class OrdersPage {}\n",
        ),
        // A `DbContext` of the repository's own, no Entity Framework; a class named
        // `DbContext`; an `[ApiController]` with no ASP.NET.
        (
            "Shop/DbContext.cs",
            "namespace Shop\n{\n    public class DbContext {}\n}\n",
        ),
        (
            "Shop/ShopContext.cs",
            "namespace Shop\n{\n    public class ShopContext : DbContext {}\n}\n",
        ),
        (
            "Shop/Orders.cs",
            "namespace Shop\n{\n    [ApiController]\n    public class Orders {}\n}\n",
        ),
    ]);
    for name in [
        "A",
        "B",
        "C",
        "D",
        "E",
        "F",
        "OrdersService",
        "OrdersPage",
        "Orders",
        "ShopContext",
        "DbContext",
    ] {
        assert_eq!(class_role(&g, name), LayerRole::Unknown, "{name}");
    }
}

// --- path conventions -------------------------------------------------------------------

#[test]
fn layer_roles_path_conventions() {
    let ts = |name: &str| format!("export class {name} {{}}\n");
    let g = graph(&[
        ("src/api/user.ts", &ts("User")),
        ("src/capitalize/x.ts", &ts("Capitalized")),
        ("src/API/y.ts", &ts("Upper")),
        ("src/services/x.ts", &ts("Svc")),
        ("src/use_cases/x.ts", &ts("UseCase")),
        ("src/model/x.ts", &ts("Model")),
        ("src/db/x.ts", &ts("Db")),
        ("src/clients/x.ts", &ts("Client")),
        ("src/ports/x.ts", &ts("Ports")),
        ("src/webhooks/x.ts", &ts("Webhooks")),
        ("src/core.ts", &ts("NamedCore")),
    ]);
    assert_eq!(class_role(&g, "User"), LayerRole::Api);
    // Whole segments only, case-sensitively: `capitalize` holds `api`, `API` is not
    // `api`, `webhooks` is not `web`, and a file named `core.ts` is not `core`.
    assert_eq!(class_role(&g, "Capitalized"), LayerRole::Unknown);
    assert_eq!(class_role(&g, "Upper"), LayerRole::Unknown);
    assert_eq!(class_role(&g, "Webhooks"), LayerRole::Unknown);
    assert_eq!(class_role(&g, "NamedCore"), LayerRole::Unknown);
    assert_eq!(class_role(&g, "Svc"), LayerRole::Service);
    assert_eq!(class_role(&g, "UseCase"), LayerRole::Service);
    assert_eq!(class_role(&g, "Model"), LayerRole::Domain);
    assert_eq!(class_role(&g, "Db"), LayerRole::Persistence);
    assert_eq!(class_role(&g, "Client"), LayerRole::Adapter);
    assert_eq!(class_role(&g, "Ports"), LayerRole::Port);
    // TypeScript's module is its directory: the same convention.
    assert_eq!(module_role(&g, "src/api"), LayerRole::Api);
    assert_eq!(module_role(&g, "src/capitalize"), LayerRole::Unknown);
    // The sets are 4.8.3's, in its order.
    let order: Vec<&str> = PATH_ROLES.iter().map(|(r, _)| r.as_str()).collect();
    assert_eq!(
        order,
        ["api", "service", "domain", "persistence", "adapter", "port"]
    );
}

#[test]
fn layer_role_path_conflict_follows_4_8_3_order() {
    // A path with segments of several sets takes the set 4.8.3 lists first, wherever
    // the segments are in the path; `test` comes after every set.
    let ts = |name: &str| format!("export class {name} {{}}\n");
    let g = graph(&[
        ("src/api/services/a.ts", &ts("ApiFirst")),
        ("src/services/api/b.ts", &ts("ApiSecond")),
        ("src/db/model/c.ts", &ts("Domain")),
        ("src/ports/adapters/d.ts", &ts("Adapter")),
        ("src/services/e.test.ts", &ts("ServiceTest")),
    ]);
    assert_eq!(class_role(&g, "ApiFirst"), LayerRole::Api);
    assert_eq!(class_role(&g, "ApiSecond"), LayerRole::Api);
    assert_eq!(class_role(&g, "Domain"), LayerRole::Domain);
    assert_eq!(class_role(&g, "Adapter"), LayerRole::Adapter);
    assert_eq!(class_role(&g, "ServiceTest"), LayerRole::Service);
    // The same answer whatever order the files are found in.
    let reversed = derive_files(
        &[
            ("src/api/services/a.ts", &ts("ApiFirst")),
            ("src/services/api/b.ts", &ts("ApiSecond")),
            ("src/db/model/c.ts", &ts("Domain")),
            ("src/ports/adapters/d.ts", &ts("Adapter")),
            ("src/services/e.test.ts", &ts("ServiceTest")),
        ],
        true,
    );
    assert_eq!(roles(&reversed), roles(&g));
}

#[test]
fn layer_role_test_paths_reuse_language_rules() {
    let files: [(&str, &str); 7] = [
        (
            "src/test/java/com/acme/shop/OrdersTest.java",
            "package com.acme.shop;\n\npublic class OrdersTest {}\n",
        ),
        (
            "src/main/java/com/acme/latest/Latest.java",
            "package com.acme.latest;\n\npublic class Latest {}\n",
        ),
        ("tests/test_orders.py", "class TestOrders:\n    pass\n"),
        ("pkg/orders_test.py", "class OrdersCheck:\n    pass\n"),
        ("lib/contest.py", "class Contest:\n    pass\n"),
        ("web-app/orders.spec.ts", "export class OrdersSpec {}\n"),
        ("web-app/testing.ts", "export class Testing {}\n"),
    ];
    let g = graph(&files);
    // The role is `test` exactly where the language's own test rule (issue 31) says
    // the file is a test file: the stage's one test-path rule, not another.
    for (path, _) in files {
        let language = languages::detect(path, b"", |_| false).expect("a language");
        let expected = if is_test_path(language, path) {
            LayerRole::Test
        } else {
            LayerRole::Unknown
        };
        let class = g
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Class && n.file_id.as_ref() == g.file_nodes.get(path))
            .expect("the file's class");
        assert_eq!(class.layer_role, Some(expected), "{path}");
    }
    assert_eq!(class_role(&g, "OrdersTest"), LayerRole::Test);
    assert_eq!(class_role(&g, "TestOrders"), LayerRole::Test);
    assert_eq!(class_role(&g, "Latest"), LayerRole::Unknown);
    assert_eq!(class_role(&g, "Contest"), LayerRole::Unknown);
    assert_eq!(class_role(&g, "Testing"), LayerRole::Unknown);
}

#[test]
fn layer_role_multifile_module_is_deterministic() {
    // One Java package across three files: under `web` (api), under `services`
    // (service) and under neither. Its members disagree, so the module is `unknown`
    // at the path tier; each class keeps its own file's role.
    let class = |name: &str| format!("package com.acme.mixed;\n\npublic class {name} {{}}\n");
    let files = [
        ("src/main/java/web/com/acme/mixed/A.java", class("A")),
        ("src/main/java/services/com/acme/mixed/B.java", class("B")),
        ("src/main/java/plain/com/acme/mixed/C.java", class("C")),
        (
            "src/main/java/web/com/acme/agree/D.java",
            class("D").replace("mixed", "agree"),
        ),
        (
            "src/main/java/plain/com/acme/agree/E.java",
            class("E").replace("mixed", "agree"),
        ),
        (
            "src/main/java/web/com/acme/agree/more/F.java",
            class("F").replace("mixed", "agree"),
        ),
    ];
    let files: Vec<(&str, &str)> = files.iter().map(|(p, c)| (*p, c.as_str())).collect();
    let g = graph(&files);
    assert_eq!(module_role(&g, "com.acme.mixed"), LayerRole::Unknown);
    assert_eq!(class_role(&g, "A"), LayerRole::Api);
    assert_eq!(class_role(&g, "B"), LayerRole::Service);
    assert_eq!(class_role(&g, "C"), LayerRole::Unknown);
    // Members that give a role all agree (a member that gives none does not object).
    assert_eq!(module_role(&g, "com.acme.agree"), LayerRole::Api);
    // The same graph, roles included, whatever order discovery finds the files in.
    let reversed = derive_files(&files, true);
    assert_eq!(reversed, g);
    // Configuration still decides a disagreeing module.
    let mut configured = files.clone();
    let config = toml_rules(&[("src/main/java/plain/**", "infra")]);
    configured.push(("pdx.toml", &config));
    let g = graph(&configured);
    assert_eq!(module_role(&g, "com.acme.mixed"), LayerRole::Infra);
    assert_eq!(class_role(&g, "A"), LayerRole::Api);
}

#[test]
fn layer_role_only_module_and_class() {
    let g = graph(&[
        ("pyapp/__init__.py", ""),
        ("pyapp/api/__init__.py", ""),
        (
            "pyapp/api/views.py",
            "from fastapi import FastAPI\n\napp = FastAPI()\n\n\nclass Thing:\n    def method(self):\n        return 1\n\n\n@app.get(\"/things\")\ndef things():\n    return helper()\n\n\ndef helper():\n    return Thing().method()\n",
        ),
        (
            "tests/test_views.py",
            "from pyapp.api.views import helper\n\n\ndef test_helper():\n    assert helper() == 1\n",
        ),
        (
            "src/main/java/com/acme/web/Api.java",
            "package com.acme.web;\n\npublic class Api {\n    private int field;\n    public Api() {}\n    public interface Inner {}\n    public enum Kind { A }\n}\n",
        ),
        (
            "crate/Cargo.toml",
            "[package]\nname = \"crate\"\nversion = \"0.1.0\"\n",
        ),
        (
            "crate/src/lib.rs",
            "pub mod inner {\n    pub fn f() {}\n}\npub struct S;\npub trait T {}\n",
        ),
    ]);
    let mut seen = BTreeMap::new();
    for n in &g.nodes {
        let role_bearing = matches!(n.kind, NodeKind::Module | NodeKind::Class);
        assert_eq!(
            n.layer_role.is_some(),
            role_bearing,
            "{} {}",
            n.kind,
            n.qualified_name
        );
        *seen.entry(n.kind.as_str()).or_insert(0) += 1;
    }
    // The fixture has the kinds the rule must leave alone (the engine records a Java
    // constructor as a method and a Rust trait as an interface).
    for kind in [
        "Repo",
        "Folder",
        "File",
        "Function",
        "Method",
        "Field",
        "Variable",
        "Interface",
        "Enum",
        "Struct",
        "Route",
        "Test",
        "Module",
        "Class",
    ] {
        assert!(seen.contains_key(kind), "no {kind} node: {seen:?}");
    }
    // No HAS_ROLE or LAYER_DEPENDS edge, and no role-bearing node of the estate's.
    assert!(
        g.edges
            .iter()
            .all(|e| !matches!(e.kind.as_str(), "HAS_ROLE" | "LAYER_DEPENDS"))
    );
    assert!(g.nodes.iter().all(|n| n.kind != NodeKind::LayerRole));
}

#[test]
fn layer_roles_are_written_into_no_props() {
    // The role is `nodes.layer_role`, nothing else: no prop names it, and no source
    // text, annotation or path beyond what the stage already stored reaches a prop.
    let g = graph(&[(
        "src/main/java/com/acme/web/A.java",
        "package com.acme.web;\n\nimport org.springframework.stereotype.Service;\n\n@Service\npublic class A {}\n",
    )]);
    for n in &g.nodes {
        for key in [
            "layer_role",
            "role",
            "framework_rule",
            "decorators",
            "annotations",
        ] {
            assert!(n.props.get(key).is_none(), "{} has {key}", n.qualified_name);
        }
        let text = serde_json::to_string(&n.props).expect("props serialise");
        assert!(!text.contains("@Service"), "{}: {text}", n.qualified_name);
        assert!(
            !text.contains("pdx-layers-test-"),
            "{}: {text}",
            n.qualified_name
        );
    }
}
