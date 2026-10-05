//! The facts derivation needs cross the interface (issues 46 and 47): each call's and
//! callable reference's node-type path from its enclosing definition, definitions'
//! decorators, parameter types and route bindings, and calls' arguments.

mod common;

use pdx_engine::{Call, CallArg, Engine, FileExtract};

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
        (list.route_method.as_deref(), list.route_path.as_deref()),
        (Some("GET"), Some("/api/users"))
    );
    let class = e
        .definitions
        .iter()
        .find(|d| d.name == "UserController")
        .unwrap();
    assert_eq!(
        class.decorators,
        ["@RestController", "@RequestMapping(\"/api\")"]
    );
    assert_eq!((&class.route_method, &class.route_path), (&None, &None));

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
        (read.route_method.as_deref(), read.route_path.as_deref()),
        (Some("GET"), Some("/users/{user_id}"))
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
                || back.definitions.iter().any(|d| d.route_path.is_some()),
            "{rel}"
        );
    }
}
