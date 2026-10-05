//! The facts derivation needs cross the interface (issues 46 and 47): each call's and
//! callable reference's node-type path from its enclosing definition, definitions'
//! decorators, parameter types and route bindings, and calls' arguments.

mod common;

use pdx_engine::{Call, CallArg, Engine, FileExtract, RouteFact};

fn extract(language: &str, rel_path: &str, source: &str) -> FileExtract {
    Engine::new()
        .unwrap()
        .extract(language, rel_path, source.as_bytes())
        .unwrap_or_else(|e| panic!("{rel_path}: {e}"))
}

fn paths<'e>(e: &'e FileExtract, callee: &str, reference: bool) -> Vec<Vec<&'e str>> {
    let mut found: Vec<&Call> = e
        .calls
        .iter()
        .filter(|c| c.callee_text == callee && c.is_reference == reference)
        .collect();
    found.sort_by_key(|c| c.span.map(|s| s.start_byte));
    found
        .iter()
        .map(|c| c.ast_path.iter().map(String::as_str).collect())
        .collect()
}

const CONTROLLER: &str = "package com.acme.web;\n\nimport org.springframework.web.bind.annotation.*;\n\n@RestController\n@RequestMapping(\"/api\")\npublic class UserController {\n  @GetMapping(\"/users\")\n  public String list(String filter, java.util.List<Integer> ids) {\n    helper();\n    if (filter != null) {\n      helper();\n    }\n    return \"\";\n  }\n  void helper() {}\n}\n";

const APP: &str = "from fastapi import FastAPI\n\napp = FastAPI()\n\n\ndef helper():\n    return 1\n\n\n@app.get(\"/users/{user_id}\", status_code=200)\ndef read_user(user_id: int, q: str = None) -> dict:\n    xs = []\n    xs.append(helper)\n    return {\"id\": helper()}\n";

const SERVER: &str = "const express = require('express');\nconst app = express();\nfunction listUsers(req, res) { res.json([]); }\napp.get('/users', listUsers);\n";

#[test]
fn call_ast_path_crosses_the_boundary() {
    // From the enclosing definition's node to the call's own, by node type.
    let e = extract("java", "src/UserController.java", CONTROLLER);
    assert_eq!(
        paths(&e, "helper", false),
        [
            vec![
                "method_declaration",
                "block",
                "expression_statement",
                "method_invocation"
            ],
            vec![
                "method_declaration",
                "block",
                "if_statement",
                "block",
                "expression_statement",
                "method_invocation"
            ],
        ]
    );
    // At file scope, from the root.
    let e = extract("python", "app/main.py", APP);
    assert_eq!(
        paths(&e, "app.get", false),
        [vec!["module", "decorated_definition", "decorator", "call"]]
    );
}

#[test]
fn reference_ast_path_crosses_the_boundary() {
    let e = extract("python", "app/main.py", APP);
    assert_eq!(
        paths(&e, "helper", true),
        [vec![
            "function_definition",
            "block",
            "expression_statement",
            "call",
            "argument_list",
            "identifier"
        ]]
    );
    let e = extract("javascript", "src/server.js", SERVER);
    assert_eq!(
        paths(&e, "listUsers", true),
        [vec![
            "program",
            "expression_statement",
            "call_expression",
            "arguments",
            "identifier"
        ]]
    );
}

#[test]
fn every_site_in_the_source_has_a_path() {
    // Over every fixture the engine's tests hold: a call or reference with a position
    // in the source always has its path, and no path has an empty step.
    let mut sites = 0;
    for dir in ["resolve", "facts", "smoke", "bases"] {
        let root = common::fixtures().join(dir);
        for (rel, language, source) in common::source_files(&root) {
            let Ok(e) = Engine::new().unwrap().extract(language, &rel, &source) else {
                continue;
            };
            for call in e.calls.iter().filter(|c| c.span.is_some()) {
                assert!(!call.ast_path.is_empty(), "{rel}: {call:?}");
                assert!(
                    call.ast_path.iter().all(|t| !t.is_empty()),
                    "{rel}: {call:?}"
                );
                sites += 1;
            }
        }
    }
    assert!(sites > 100, "{sites}");
}

#[test]
fn derivation_facts_cross_the_boundary() {
    let e = extract("java", "src/UserController.java", CONTROLLER);
    let list = e.definitions.iter().find(|d| d.name == "list").unwrap();
    assert_eq!(list.decorators, ["@GetMapping(\"/users\")"]);
    assert_eq!(
        list.signature_param_types,
        ["String", "java.util.List<Integer>"]
    );
    assert_eq!(
        routes(&list.routes),
        [("GET", "/api/users", "GetMapping", "@GetMapping(\"/users\")")]
    );
    let route = &list.routes[0];
    assert_eq!(
        route.ast_path,
        ["method_declaration", "modifiers", "annotation"]
    );
    let span = route.span.expect("a position");
    assert_eq!(
        &CONTROLLER[span.start_byte as usize..span.end_byte as usize],
        "@GetMapping(\"/users\")"
    );
    assert_eq!(span.start_line, 8);
    let class = e
        .definitions
        .iter()
        .find(|d| d.name == "UserController")
        .unwrap();
    assert_eq!(
        class.decorators,
        ["@RestController", "@RequestMapping(\"/api\")"]
    );
    assert!(
        class.routes.is_empty(),
        "a class's mapping is its methods' prefix"
    );

    let e = extract("python", "app/main.py", APP);
    let read = e
        .definitions
        .iter()
        .find(|d| d.name == "read_user")
        .unwrap();
    assert_eq!(
        read.decorators,
        ["@app.get(\"/users/{user_id}\", status_code=200)"]
    );
    assert_eq!(read.signature_param_types, ["int", "str"]);
    assert_eq!(
        routes(&read.routes),
        [(
            "GET",
            "/users/{user_id}",
            "app.get",
            "app.get(\"/users/{user_id}\", status_code=200)"
        )]
    );
    assert_eq!(
        read.routes[0].ast_path,
        ["decorated_definition", "decorator", "call"]
    );
    let decorator = e.calls.iter().find(|c| c.callee_text == "app.get").unwrap();
    assert_eq!(
        decorator.args,
        [
            CallArg {
                expr: "\"/users/{user_id}\"".into(),
                value: Some("/users/{user_id}".into()),
                keyword: None,
                index: 0,
            },
            CallArg {
                expr: "200".into(),
                value: None,
                keyword: Some("status_code".into()),
                index: 1,
            },
        ]
    );

    let e = extract("javascript", "src/server.js", SERVER);
    let registration = e.calls.iter().find(|c| c.callee_text == "app.get").unwrap();
    assert_eq!(
        registration.args,
        [
            CallArg {
                expr: "'/users'".into(),
                value: Some("/users".into()),
                keyword: None,
                index: 0,
            },
            CallArg {
                expr: "listUsers".into(),
                value: None,
                keyword: None,
                index: 1,
            },
        ]
    );
}

#[test]
fn site_facts_survive_serialisation() {
    for (language, rel, source) in [
        ("java", "src/UserController.java", CONTROLLER),
        ("python", "app/main.py", APP),
        ("javascript", "src/server.js", SERVER),
    ] {
        let e = extract(language, rel, source);
        let back: FileExtract = postcard::from_bytes(&postcard::to_stdvec(&e).unwrap()).unwrap();
        assert_eq!(back, e, "{rel}");
        assert!(back.calls.iter().any(|c| !c.ast_path.is_empty()), "{rel}");
        assert!(
            back.calls.iter().any(|c| !c.args.is_empty())
                || back.definitions.iter().any(|d| !d.routes.is_empty()),
            "{rel}"
        );
    }
}

/// Each route as (method, path, callee, source text).
fn routes(facts: &[RouteFact]) -> Vec<(&str, &str, &str, &str)> {
    facts
        .iter()
        .map(|r| {
            (
                r.method.as_str(),
                r.path.as_str(),
                r.callee_text.as_str(),
                r.source_text.as_deref().unwrap_or(""),
            )
        })
        .collect()
}

/// Each route as "METHOD path", for a definition.
fn bindings(e: &FileExtract, name: &str) -> Vec<String> {
    e.definitions
        .iter()
        .find(|d| d.name == name)
        .unwrap_or_else(|| panic!("{name}"))
        .routes
        .iter()
        .map(|r| format!("{} {}", r.method, r.path))
        .collect()
}

#[test]
fn route_facts_cross_the_boundary() {
    // One fact per method and path, joined to the class's prefix, each with the node
    // that declares it; a path or method that is not a literal gives none (issue 54).
    let java = extract(
        "java",
        "src/R.java",
        "package a;\n@RestController\n@RequestMapping(\"/api\")\npublic class R {\n  @GetMapping({\"/a\", \"/b\"})\n  public void paths() {}\n  @RequestMapping(path = {\"/x\", \"/y\"}, method = {RequestMethod.GET, RequestMethod.POST})\n  public void both() {}\n  @RequestMapping(\"/any\")\n  public void any() {}\n  @GetMapping\n  public void root() {}\n  @GetMapping(Paths.USERS)\n  public void constant() {}\n  @GetMapping(value = \"/v\", produces = \"application/json\")\n  public void media() {}\n}\n",
    );
    assert_eq!(bindings(&java, "paths"), ["GET /api/a", "GET /api/b"]);
    assert_eq!(
        bindings(&java, "both"),
        ["GET /api/x", "GET /api/y", "POST /api/x", "POST /api/y"]
    );
    assert_eq!(bindings(&java, "any"), ["ANY /api/any"]);
    assert_eq!(bindings(&java, "root"), ["GET /api"]);
    assert!(bindings(&java, "constant").is_empty(), "no guessed path");
    assert_eq!(bindings(&java, "media"), ["GET /api/v"]);
    // The bindings of one annotation share its node.
    let both = java.definitions.iter().find(|d| d.name == "both").unwrap();
    assert!(both.routes.windows(2).all(|w| w[0].span == w[1].span));

    let kotlin = extract(
        "kotlin",
        "src/K.kt",
        "package a\n@RestController\n@RequestMapping(\"/k\")\nclass K {\n  @GetMapping(value = [\"/y\", \"/z\"])\n  fun yz(): String = \"\"\n  @RequestMapping(path = [\"/m\"], method = [RequestMethod.GET, RequestMethod.PUT])\n  fun m(): String = \"\"\n}\n",
    );
    assert_eq!(bindings(&kotlin, "yz"), ["GET /k/y", "GET /k/z"]);
    assert_eq!(bindings(&kotlin, "m"), ["GET /k/m", "PUT /k/m"]);
    let m = kotlin.definitions.iter().find(|d| d.name == "m").unwrap();
    assert_eq!(
        m.routes[0].ast_path,
        ["function_declaration", "modifiers", "annotation"]
    );

    let python = extract(
        "python",
        "app/f.py",
        "from flask import Flask\napp = Flask(__name__)\n\n@app.route(\"/f\", methods=[\"GET\", \"POST\"])\ndef f():\n    return ''\n\n@app.get(\"/one\")\n@app.post(\"/two\")\ndef stacked():\n    return ''\n\n@app.get(PATH)\ndef dynamic():\n    return ''\n",
    );
    assert_eq!(bindings(&python, "f"), ["GET /f", "POST /f"]);
    assert_eq!(bindings(&python, "stacked"), ["GET /one", "POST /two"]);
    assert!(bindings(&python, "dynamic").is_empty(), "no guessed path");
    let stacked = python
        .definitions
        .iter()
        .find(|d| d.name == "stacked")
        .unwrap();
    assert_ne!(
        stacked.routes[0].span, stacked.routes[1].span,
        "two decorators"
    );
}
