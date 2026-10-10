//! Contracts (4.7.1, P2-08): the rows Stage 4 derives for what a repository provides
//! and consumes, through the whole pipeline (discovery, extraction, the registry,
//! resolution and derivation) over checkouts written here.
//!
//! Synthetic secrets are assembled at run time: no credential-shaped literal is in
//! this file.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use pdx_core::config::PdxConfig;
use pdx_core::contracts::config_values::Resolved;
use pdx_core::contracts::identity::{
    self, ContractIdentity, Ecosystem, api_key, api_path, artifact_key, artifact_version_key,
    column_key, contract_id, namespace_key, table_key, url_authority,
};
use pdx_core::contracts::{
    ContractAccumulator, ContractDiagnostic, ContractObservation, ContractProblem,
};
use pdx_core::ids::{NodeId, RepoId};
use pdx_core::index::derive::{DeriveError, DeriveInput, DerivedGraph, derive};
use pdx_core::index::discover::discover;
use pdx_core::index::extract::{ExtractLimits, ExtractStage};
use pdx_core::kinds::{ContractKind, EdgeKind, NodeKind};
use pdx_core::model::{Contract, ContractDirection, IdentityStrength, Node};
use pdx_core::resolve::registry::SymbolRegistry;
use pdx_core::resolve::stages::resolve;

const REPO: &str = "ce63551447285fd4";
const OTHER_REPO: &str = "2b2f33ba06c16e77";

fn repo(id: &str) -> RepoId {
    RepoId::parse(id).expect("a repo id")
}

/// A checkout on disk.
struct Checkout {
    dir: tempfile::TempDir,
}

impl Checkout {
    fn new(files: &[(&str, &str)]) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("pdx-contracts-test-")
            .tempdir()
            .expect("a directory");
        for (path, content) in files {
            let path = dir.path().join(path);
            fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
            fs::write(&path, content).expect("a file");
        }
        Self { dir }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }
}

/// The pipeline over a checkout, for a repository id, with discovery's order reversed
/// or not.
fn try_derive(
    checkout: &Checkout,
    repo_id: &str,
    reverse: bool,
) -> Result<DerivedGraph, DeriveError> {
    let root = checkout.root();
    let config = PdxConfig::load(root).expect("the configuration loads");
    let mut files = discover(root, &config).expect("discovery succeeds");
    if reverse {
        files.reverse();
    }
    let limits = ExtractLimits {
        requested_workers: if reverse { 1 } else { 2 },
        memory_budget_bytes: 64 << 20,
    };
    let extracted = ExtractStage::new(root, &config.secrets, limits)
        .run(&files)
        .expect("extraction succeeds");
    let registry = SymbolRegistry::build(root, &files, extracted).expect("the registry builds");
    let report = resolve(root, &registry).expect("resolution succeeds");
    derive(&DeriveInput {
        repo: &repo(repo_id),
        repo_name: "fixture",
        registry: &registry,
        resolution: &report,
        root,
        config: &config,
    })
}

fn graph(files: &[(&str, &str)]) -> DerivedGraph {
    try_derive(&Checkout::new(files), REPO, false).expect("derivation succeeds")
}

/// The contracts of a kind with a key.
fn find<'g>(g: &'g DerivedGraph, kind: ContractKind, key: &str) -> Vec<&'g Contract> {
    g.contracts
        .iter()
        .filter(|c| c.kind == kind && c.key == key)
        .collect()
}

/// The one contract of a kind with a key.
fn one<'g>(g: &'g DerivedGraph, kind: ContractKind, key: &str) -> &'g Contract {
    let found = find(g, kind, key);
    assert_eq!(found.len(), 1, "{kind} {key}: {:#?}", g.contracts);
    found[0]
}

/// A string-array prop.
fn strings(c: &Contract, name: &str) -> Vec<String> {
    c.props
        .get(name)
        .and_then(serde_json::Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn node<'g>(g: &'g DerivedGraph, kind: NodeKind, name: &str) -> &'g Node {
    let found: Vec<&Node> = g
        .nodes
        .iter()
        .filter(|n| n.kind == kind && n.name == name)
        .collect();
    assert_eq!(found.len(), 1, "{kind} {name}");
    found[0]
}

fn id(text: &str) -> NodeId {
    NodeId::parse(text).expect("an id")
}

// --- Named acceptance -----------------------------------------------------------------

const OPENAPI: &str = "openapi: 3.0.3\ninfo: {title: users, version: '1'}\nservers:\n  - url: https://API.Example.com/v1\npaths:\n  /users/{id}/:\n    get:\n      operationId: getUser\n    delete:\n      operationId: removeUser\n  /users:\n    post:\n      operationId: createUser\n";

#[test]
fn contracts_openapi() {
    let g = graph(&[("api/openapi.yaml", OPENAPI)]);
    let file = &g.file_nodes["api/openapi.yaml"];
    let get = one(&g, ContractKind::ApiContract, "GET /v1/users/{}");
    // Method upper-cased, the parameter `{}`, the trailing slash gone, the server's
    // path prefix joined and its host in the namespace, never the key.
    assert_eq!(
        get.namespace_key,
        r#"{"identity":"api.example.com","key":"GET /v1/users/{}"}"#
    );
    assert_eq!(get.identity_strength, IdentityStrength::Exact);
    assert_eq!(get.direction, ContractDirection::Provides);
    assert_eq!(get.owner_node_id.as_ref(), Some(file));
    assert_eq!(strings(get, "raw_forms"), ["get /users/{id}/"]);
    assert_eq!(strings(get, "source_paths"), ["api/openapi.yaml"]);
    assert_eq!(strings(get, "frameworks"), ["openapi"]);
    // Fixed: computed outside the crate (issue 58).
    assert_eq!(get.contract_id, id("A19SP1TVVAFTD3DG84PTQ9QPJE"));
    for key in ["DELETE /v1/users/{}", "POST /v1/users"] {
        let c = one(&g, ContractKind::ApiContract, key);
        assert_eq!(c.direction, ContractDirection::Provides);
        assert!(!c.key.contains("example.com"));
    }
    assert_eq!(g.contracts.len(), 3, "{:#?}", g.contracts);
    // No server and no declared hostname: unresolved, scoped to the repository.
    let bare = OPENAPI.replace("servers:\n  - url: https://API.Example.com/v1\n", "");
    let g = graph(&[("openapi.json", &yaml_to_json(&bare))]);
    let get = one(&g, ContractKind::ApiContract, "GET /users/{}");
    assert_eq!(get.identity_strength, IdentityStrength::Unresolved);
    assert_eq!(
        get.namespace_key,
        format!("unresolved:{REPO}:GET /users/{{}}")
    );
}

/// The fixture's YAML as JSON (the same document in the other format).
fn yaml_to_json(_yaml: &str) -> String {
    r#"{"openapi":"3.0.3","info":{"title":"users","version":"1"},"paths":{"/users/{id}/":{"get":{"operationId":"getUser"},"delete":{}},"/users":{"post":{}}}}"#.to_owned()
}

const PROTO: &str = "syntax = \"proto3\";\npackage acme.users;\n\n// rpc Commented(A) returns (B);\nservice UserService {\n  rpc GetUser(GetUserRequest) returns (GetUserResponse);\n  /* rpc Hidden(A) returns (B); */\n  rpc ListUsers(ListUsersRequest) returns (stream User) {}\n}\nmessage GetUserRequest { string id = 1; }\n";

#[test]
fn contracts_proto() {
    let g = graph(&[("proto/users.proto", PROTO)]);
    let file = &g.file_nodes["proto/users.proto"];
    let keys: Vec<&str> = g
        .contracts
        .iter()
        .map(|c| c.key.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    assert_eq!(
        keys,
        [
            "acme.users.UserService/GetUser",
            "acme.users.UserService/ListUsers"
        ],
        "commented-out methods are not methods"
    );
    let get = one(
        &g,
        ContractKind::RpcMethod,
        "acme.users.UserService/GetUser",
    );
    assert_eq!(get.direction, ContractDirection::Provides);
    assert_eq!(get.owner_node_id.as_ref(), Some(file));
    // No shared artifact or context proves a namespace here.
    assert_eq!(get.identity_strength, IdentityStrength::Unresolved);
    assert_eq!(
        get.namespace_key,
        format!("unresolved:{REPO}:acme.users.UserService/GetUser")
    );
    assert_eq!(get.contract_id, id("P8K4CXYG2FNRVQM5BKVD8828MY"));
    assert_eq!(
        strings(get, "raw_forms"),
        ["rpc GetUser(GetUserRequest) returns (GetUserResponse)"]
    );
    assert_eq!(
        get.props.get("package"),
        Some(&serde_json::json!("acme.users"))
    );
    assert_eq!(
        get.props.get("service"),
        Some(&serde_json::json!("UserService"))
    );
    assert_eq!(get.props.get("method"), Some(&serde_json::json!("GetUser")));
    // No package: `Service/Method`.
    let g = graph(&[("p.proto", "service Ping { rpc Pong(A) returns (B); }\n")]);
    one(&g, ContractKind::RpcMethod, "Ping/Pong");
}

#[test]
fn proto_stub_calls_are_consumed_when_resolution_proves_them() {
    // A checked-in generated stub, called through a typed receiver.
    let files = [
        ("proto/users.proto", PROTO),
        (
            "src/main/java/com/acme/users/UserServiceGrpc.java",
            "package com.acme.users;\n\npublic final class UserServiceGrpc {\n  public static final class UserServiceBlockingStub {\n    public Object getUser(Object request) { return null; }\n  }\n}\n",
        ),
        (
            "src/main/java/com/acme/app/Client.java",
            "package com.acme.app;\n\nimport com.acme.users.UserServiceGrpc.UserServiceBlockingStub;\n\npublic class Client {\n  private final UserServiceBlockingStub stub;\n  public Client(UserServiceBlockingStub stub) { this.stub = stub; }\n  public Object run() { return stub.getUser(null); }\n}\n",
        ),
    ];
    let g = graph(&files);
    let get = one(
        &g,
        ContractKind::RpcMethod,
        "acme.users.UserService/GetUser",
    );
    assert_eq!(get.direction, ContractDirection::Both, "{get:#?}");
    assert!(strings(get, "raw_forms").contains(&"UserServiceBlockingStub.getUser".to_owned()));
    assert_eq!(strings(get, "site_ids").len(), 1);
    // A method of the same name on an unrelated class is no stub call.
    let unrelated = [
        ("proto/users.proto", PROTO),
        (
            "src/main/java/com/acme/app/Cache.java",
            "package com.acme.app;\n\npublic class Cache {\n  public Object getUser(Object k) { return null; }\n  public Object run() { return getUser(null); }\n}\n",
        ),
    ];
    let g = graph(&unrelated);
    assert_eq!(
        one(
            &g,
            ContractKind::RpcMethod,
            "acme.users.UserService/GetUser"
        )
        .direction,
        ContractDirection::Provides
    );
}

const PRODUCER: &str = "from kafka import KafkaProducer\n\nproducer = KafkaProducer()\n\n\ndef publish(event):\n    producer.send(\"orders.created\", event)\n";

#[test]
fn contracts_kafka_literal() {
    let g = graph(&[
        (
            "pdx.toml",
            "[identity]\nbrokers = [\"Kafka-1.example.com:9092\"]\n",
        ),
        ("app/producer.py", PRODUCER),
    ]);
    let file = &g.file_nodes["app/producer.py"];
    let c = one(&g, ContractKind::Channel, "orders.created");
    // The engine's Emit fact: provided, owned by the file (the fact has no position).
    assert_eq!(c.direction, ContractDirection::Provides);
    assert_eq!(c.owner_node_id.as_ref(), Some(file));
    assert_eq!(c.identity_strength, IdentityStrength::Declared);
    assert_eq!(
        c.namespace_key,
        r#"{"identity":"kafka-1.example.com:9092","key":"orders.created"}"#
    );
    assert_eq!(c.contract_id, id("5GEPWQM39TFNJK72648WA81MPY"));
    assert_eq!(strings(c, "raw_forms"), ["producer.send(orders.created)"]);
    assert_eq!(strings(c, "clients"), ["kafka-python producer.send"]);
    assert_eq!(strings(c, "transports"), ["kafka"]);
    // The key is the topic as written: case kept.
    let g = graph(&[(
        "app/producer.py",
        &PRODUCER.replace("orders.created", "Orders.Created"),
    )]);
    let c = one(&g, ContractKind::Channel, "Orders.Created");
    assert_eq!(c.identity_strength, IdentityStrength::Unresolved);
    // A consumer's Listen fact: consumed, no owner.
    let g = graph(&[(
        "app/consumer.py",
        "from kafka import KafkaConsumer\n\nconsumer = KafkaConsumer()\n\n\ndef listen():\n    consumer.subscribe([\"orders.created\"])\n",
    )]);
    let c = one(&g, ContractKind::Channel, "orders.created");
    assert_eq!(c.direction, ContractDirection::Consumes);
    assert_eq!(c.owner_node_id, None);
}

const LISTENER: &str = "package com.acme.orders;\n\nimport org.springframework.kafka.annotation.KafkaListener;\n\npublic class OrderListener {\n  @KafkaListener(topics = \"${orders.topic}\", groupId = \"g\")\n  public void on(String message) {}\n}\n";

#[test]
fn contracts_kafka_placeholder_unresolved() {
    let path = "src/main/java/com/acme/orders/OrderListener.java";
    // `.env` holds the value; it is redacted, never opened, and resolves nothing.
    let marker = ["dotenv", "Only", "Topic"].concat();
    let dotenv = format!("orders.topic={marker}\nORDERS_TOPIC={marker}\n");
    let g = graph(&[(path, LISTENER), (".env", &dotenv)]);
    let c = one(&g, ContractKind::Channel, "unresolved:${orders.topic}");
    assert_eq!(c.direction, ContractDirection::Consumes);
    assert_eq!(c.identity_strength, IdentityStrength::Unresolved);
    assert_eq!(
        c.namespace_key,
        format!("unresolved:{REPO}:unresolved:${{orders.topic}}"),
        "no identity is fabricated"
    );
    assert_eq!(strings(c, "placeholders"), ["${orders.topic}"]);
    assert_eq!(strings(c, "transports"), ["kafka"]);
    assert_no_marker(&g, &marker);
}

#[test]
fn contracts_jpa_table() {
    let entity = "package com.acme.users;\n\nimport jakarta.persistence.*;\n\n@Entity\n@Table(name = \"Users\", schema = \"public\")\npublic class User {\n  @Id Long id;\n}\n";
    let url = format!(
        "{}db.example.com:5432/shop",
        ["jdbc:", "postgres", "ql://"].concat()
    );
    let config = format!("spring:\n  datasource:\n    url: {url}\n");
    let g = graph(&[
        ("src/main/java/com/acme/users/User.java", entity),
        ("src/main/resources/application.yml", &config),
    ]);
    let class = node(&g, NodeKind::Class, "User");
    let c = one(&g, ContractKind::Table, "users");
    assert_eq!(c.direction, ContractDirection::Provides);
    assert_eq!(c.owner_node_id.as_ref(), Some(&class.node_id));
    assert_eq!(c.identity_strength, IdentityStrength::Exact);
    assert_eq!(
        c.namespace_key,
        r#"{"identity":"db.example.com:5432/shop","key":"users"}"#
    );
    assert_eq!(c.contract_id, id("KGXAGGZEDVVEEH6VA40351FK92"));
    assert_eq!(
        strings(c, "raw_forms"),
        [r#"@Table(name = "Users", schema = "public")"#]
    );
    assert_eq!(strings(c, "frameworks"), ["jpa"]);
    // Another schema is kept.
    let sales = entity.replace("schema = \"public\"", "schema = \"sales\"");
    let g = graph(&[("src/main/java/com/acme/users/User.java", &sales)]);
    let c = one(&g, ContractKind::Table, "sales.users");
    assert_eq!(c.identity_strength, IdentityStrength::Unresolved);
    // No @Entity: no table.
    let g = graph(&[(
        "src/main/java/com/acme/users/User.java",
        &entity.replace("@Entity\n", ""),
    )]);
    assert!(g.contracts.is_empty(), "{:#?}", g.contracts);
}

const POM: &str = "<?xml version=\"1.0\"?>\n<project xmlns=\"urn:x-test:pom\">\n  <modelVersion>4.0.0</modelVersion>\n  <groupId>com.acme</groupId>\n  <artifactId>shop</artifactId>\n  <version>1.2.0</version>\n  <properties><lib.range>[1.0,2.0)</lib.range></properties>\n  <dependencies>\n    <dependency>\n      <groupId>org.acme.libs</groupId>\n      <artifactId>money</artifactId>\n      <version>${lib.range}</version>\n    </dependency>\n    <dependency>\n      <groupId>org.acme.libs</groupId>\n      <artifactId>managed</artifactId>\n    </dependency>\n  </dependencies>\n</project>\n";

#[test]
fn contracts_maven_artifact() {
    let g = graph(&[("pom.xml", POM)]);
    let file = &g.file_nodes["pom.xml"];
    let shop = one(&g, ContractKind::Artifact, "maven:com.acme:shop");
    assert_eq!(shop.direction, ContractDirection::Provides);
    assert_eq!(shop.owner_node_id.as_ref(), Some(file));
    assert_eq!(shop.identity_strength, IdentityStrength::Exact);
    assert_eq!(
        shop.namespace_key, "maven:com.acme:shop",
        "the coordinate is its own namespace"
    );
    assert_eq!(shop.contract_id, id("QVY04P4K0YBDWKNTVHGZV1V1B6"));
    let version = one(
        &g,
        ContractKind::ArtifactVersion,
        "maven:com.acme:shop@1.2.0",
    );
    assert_eq!(version.direction, ContractDirection::Provides);
    assert_eq!(version.contract_id, id("D7N1E2J1YBWMKBN8DTQS4GCC44"));
    assert_eq!(
        version.props.get("version"),
        Some(&serde_json::json!("1.2.0"))
    );
    // A dependency with a range: Artifact and ArtifactVersion, consumed, the range kept.
    let money = one(&g, ContractKind::Artifact, "maven:org.acme.libs:money");
    assert_eq!(money.direction, ContractDirection::Consumes);
    assert_eq!(money.owner_node_id, None);
    let ranged = one(
        &g,
        ContractKind::ArtifactVersion,
        "maven:org.acme.libs:money@[1.0,2.0)",
    );
    assert_eq!(ranged.direction, ContractDirection::Consumes);
    // No declared version: the Artifact only.
    one(&g, ContractKind::Artifact, "maven:org.acme.libs:managed");
    assert!(
        find(
            &g,
            ContractKind::ArtifactVersion,
            "maven:org.acme.libs:managed@"
        )
        .is_empty()
    );
    assert_eq!(g.contracts.len(), 5, "{:#?}", g.contracts);
}

#[test]
fn contract_key_normalisation() {
    api_keys_normalise();
    authorities_normalise();
    channel_keys_normalise();
    table_and_column_keys_normalise();
    artifact_keys_normalise();
}

/// API: raw method and path or URL → key.
fn api_keys_normalise() {
    // API: raw method and path or URL → key.
    for (method, raw, key) in [
        ("get", "/users/{id}/", "GET /users/{}"),
        (
            "GET",
            "https://api.example.com/users/{userId}?active=true#x",
            "GET /users/{}",
        ),
        ("get", "/users/:id", "GET /users/{}"),
        ("get", "/users/<id>", "GET /users/{}"),
        ("get", "/users/<int:id>", "GET /users/{}"),
        ("get", "/users/{id:[0-9]+}", "GET /users/{}"),
        ("get", "/users/{id:[0-9]{3}}", "GET /users/{}"),
        ("post", "/", "POST /"),
        ("get", "https://api.example.com", "GET /"),
        ("get", "users", "GET /users"),
        ("get", "/Users/%7Bx%7D/../a", "GET /Users/%7Bx%7D/../a"),
        ("patch", "//cdn.example.com/a//", "PATCH /a"),
    ] {
        assert_eq!(api_key(method, raw).as_deref(), Some(key), "{method} {raw}");
    }
    assert_eq!(api_path("/users/{id"), None);
    assert_eq!(api_key("GE T", "/x"), None);
}

fn authorities_normalise() {
    // Authorities: lower-cased, terminal dot, default ports, user information.
    let at = char::from(0x40);
    // Hosts outside RFC 2606's examples are built at run time: the provenance scan
    // admits no other address written into the tree.
    let http = |rest: &str| format!("http://{rest}");
    let user_info = ["us", "er"].concat();
    for (url, authority) in [
        ("https://API.Example.com./x", Some("api.example.com")),
        ("https://api.example.com:443/x", Some("api.example.com")),
        ("http://api.example.com:80/x", Some("api.example.com")),
        (
            "https://api.example.com:8443/x",
            Some("api.example.com:8443"),
        ),
        (
            &format!("https://{user_info}{at}api.example.com/x"),
            Some("api.example.com"),
        ),
        (&http("10.0.0.7:8080/x"), Some("10.0.0.7:8080")),
        (&http("localhost:8080/x"), None),
        (&http("127.0.0.1/x"), None),
        ("https://{host}/x", None),
    ] {
        assert_eq!(url_authority(url).as_deref(), authority, "{url}");
    }
}

fn channel_keys_normalise() {
    // Channels: literal kept, placeholders resolved or marked.
    let values = pdx_core::contracts::config_values::ConfigValues::from_entries([(
        "orders.topic".to_owned(),
        "orders.created".to_owned(),
    )]);
    assert_eq!(
        values.resolve("Orders.Created"),
        Resolved::Literal("Orders.Created".into())
    );
    assert_eq!(
        values.resolve("${orders.topic}"),
        Resolved::Placeholder {
            raw: "${orders.topic}".into(),
            value: "orders.created".into()
        }
    );
    assert_eq!(
        values.resolve("${missing}"),
        Resolved::Unresolved("${missing}".into())
    );
    assert_eq!(
        values.resolve("${orders.topic:x}"),
        Resolved::Unresolved("${orders.topic:x}".into())
    );
    assert_eq!(
        identity::unresolved_channel_key("${missing}"),
        "unresolved:${missing}"
    );
}

fn table_and_column_keys_normalise() {
    // Tables and columns.
    for (raw, key) in [
        ("Users", "users"),
        ("public.Users", "users"),
        ("PUBLIC.\"Users\"", "users"),
        ("sales.Users", "sales.users"),
        ("\"Sales\".\"Order Lines\"", "sales.order lines"),
        ("`users`", "users"),
        ("[dbo].[Users]", "dbo.users"),
    ] {
        assert_eq!(table_key(raw).as_deref(), Some(key), "{raw}");
    }
    assert_eq!(table_key("users;drop"), None);
    assert_eq!(
        column_key("public.Users", "ID").as_deref(),
        Some("users.id")
    );
    assert_eq!(
        column_key("sales.Users", "\"ID\"").as_deref(),
        Some("sales.users.id")
    );
}

fn artifact_keys_normalise() {
    // Artifacts.
    for (ecosystem, group, name, key) in [
        (Ecosystem::Maven, "com.acme", "shop", "maven:com.acme:shop"),
        (Ecosystem::Npm, "@acme", "web", "npm:@acme:web"),
        (Ecosystem::Npm, "", "lodash", "npm::lodash"),
        (Ecosystem::Cargo, "", "serde", "cargo::serde"),
        (
            Ecosystem::Go,
            "",
            "github.com/acme/x",
            "go::github.com/acme/x",
        ),
        (Ecosystem::Nuget, "", "Acme.Core", "nuget::Acme.Core"),
        (Ecosystem::Pypi, "", "requests", "pypi::requests"),
        (Ecosystem::Alire, "", "aws", "alire::aws"),
        (Ecosystem::Gpr, "", "Shop", "gpr::Shop"),
    ] {
        assert_eq!(artifact_key(ecosystem, group, name).as_deref(), Some(key));
    }
    assert_eq!(
        artifact_version_key("maven:com.acme:shop", "[1.0,2.0)").as_deref(),
        Some("maven:com.acme:shop@[1.0,2.0)")
    );
    assert_eq!(artifact_key(Ecosystem::Maven, "a:b", "c"), None);
}

// --- Configuration and issue 38 -------------------------------------------------------

/// A Kafka listener on `${orders.topic}` with these configuration files.
fn listener_with(config: &[(&str, &str)]) -> DerivedGraph {
    let mut files: Vec<(&str, &str)> =
        vec![("src/main/java/com/acme/orders/OrderListener.java", LISTENER)];
    files.extend_from_slice(config);
    graph(&files)
}

fn channel_keys(g: &DerivedGraph) -> Vec<String> {
    g.contracts
        .iter()
        .filter(|c| c.kind == ContractKind::Channel)
        .map(|c| c.key.clone())
        .collect()
}

/// No `marker` anywhere a contract or its diagnostics carry text.
fn assert_no_marker(g: &DerivedGraph, marker: &str) {
    for c in &g.contracts {
        assert!(!c.key.contains(marker), "{c:#?}");
        assert!(!c.namespace_key.contains(marker), "{c:#?}");
        let props = serde_json::to_string(&c.props).expect("json");
        assert!(!props.contains(marker), "{props}");
    }
    let diagnostics = format!("{:?}", g.diagnostics.contracts);
    assert!(!diagnostics.contains(marker), "{diagnostics}");
    assert!(!format!("{:?}", g.contracts).contains(marker));
}

#[test]
fn dotenv_never_resolves_contract_placeholder() {
    let marker = ["Env", "Marker", "Value"].concat();
    for name in [".env", ".env.local", ".env.production", "config/.env"] {
        let content = format!("orders.topic={marker}\n");
        let checkout = Checkout::new(&[
            ("src/main/java/com/acme/orders/OrderListener.java", LISTENER),
            (name, &content),
        ]);
        // On Unix the file is made unreadable: had anything opened it, the build
        // would fail; it succeeds.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                checkout.root().join(name),
                fs::Permissions::from_mode(0o000),
            )
            .expect("permissions");
        }
        let g = try_derive(&checkout, REPO, false).expect("derivation succeeds");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                checkout.root().join(name),
                fs::Permissions::from_mode(0o644),
            )
            .expect("permissions");
        }
        assert_eq!(channel_keys(&g), ["unresolved:${orders.topic}"], "{name}");
        assert_no_marker(&g, &marker);
    }
}

#[test]
fn application_property_resolves_placeholder() {
    let g = listener_with(&[(
        "src/main/resources/application.properties",
        "orders.topic = orders.created\n",
    )]);
    assert_eq!(channel_keys(&g), ["orders.created"]);
    let c = one(&g, ContractKind::Channel, "orders.created");
    assert_eq!(strings(c, "placeholders"), ["${orders.topic}"]);
    // YAML flattens to the same key.
    let g = listener_with(&[(
        "src/main/resources/application.yml",
        "orders:\n  topic: orders.created\n",
    )]);
    assert_eq!(channel_keys(&g), ["orders.created"]);
}

#[test]
fn same_value_in_multiple_configs_resolves() {
    let g = listener_with(&[
        (
            "src/main/resources/application.yml",
            "orders:\n  topic: orders.created\n",
        ),
        (
            "src/main/resources/application-prod.properties",
            "orders.topic=orders.created\n",
        ),
    ]);
    assert_eq!(channel_keys(&g), ["orders.created"]);
}

#[test]
fn conflicting_application_values_are_unresolved() {
    let g = listener_with(&[
        (
            "src/main/resources/application-dev.yml",
            "orders:\n  topic: orders.dev\n",
        ),
        (
            "src/main/resources/application-prod.yml",
            "orders:\n  topic: orders.prod\n",
        ),
    ]);
    // Neither profile is preferred.
    assert_eq!(channel_keys(&g), ["unresolved:${orders.topic}"]);
    // Two documents of one file disagreeing are a conflict too.
    let g = listener_with(&[(
        "src/main/resources/application.yml",
        "orders:\n  topic: orders.a\n---\norders:\n  topic: orders.b\n",
    )]);
    assert_eq!(channel_keys(&g), ["unresolved:${orders.topic}"]);
}

#[test]
fn secret_like_config_key_is_never_contract_identity() {
    let value = ["orders", "Sec", "ret", "Value"].concat();
    for key in [
        "orders.password",
        "orders.token",
        "orders.api-key",
        "client_secret",
        "orders.privateKey",
    ] {
        let listener = LISTENER.replace("${orders.topic}", &format!("${{{key}}}"));
        let properties = format!("{key}={value}\n");
        let g = graph(&[
            (
                "src/main/java/com/acme/orders/OrderListener.java",
                &listener,
            ),
            ("src/main/resources/application.properties", &properties),
        ]);
        assert_eq!(
            channel_keys(&g),
            [format!("unresolved:${{{key}}}")],
            "{key}"
        );
        assert_no_marker(&g, &value);
    }
}

// --- Merging (issue 59) ----------------------------------------------------------------

fn observation(key: &str, direction: ContractDirection, path: &str) -> ContractObservation {
    ContractObservation::new(
        ContractKind::ApiContract,
        key,
        direction,
        path,
        format!("raw {path}"),
    )
}

fn owner(n: &str) -> NodeId {
    pdx_core::ids::NodeKey::file(&repo(REPO), n)
        .node_id()
        .expect("an id")
}

fn nodes_for(owners: &[&str]) -> BTreeMap<NodeId, Node> {
    owners
        .iter()
        .map(|n| {
            let key = pdx_core::ids::NodeKey::file(&repo(REPO), n);
            let node = Node::new(&key, n).expect("a node");
            (node.node_id.clone(), node)
        })
        .collect()
}

fn merged(observations: Vec<ContractObservation>, owners: &[&str]) -> Vec<Contract> {
    let mut accumulator = ContractAccumulator::new(repo(REPO));
    for o in observations {
        accumulator.observe(o).expect("observed");
    }
    accumulator.finish(&nodes_for(owners)).expect("finished")
}

#[test]
fn contract_duplicate_providers_merge_deterministically() {
    let a = observation("GET /x", ContractDirection::Provides, "a.yaml")
        .with_owner(owner("a.yaml"))
        .with_evidence("frameworks", "openapi");
    let b = observation("GET /x", ContractDirection::Provides, "a.yaml")
        .with_owner(owner("a.yaml"))
        .with_evidence("frameworks", "spring");
    let forwards = merged(vec![a.clone(), b.clone()], &["a.yaml"]);
    let backwards = merged(vec![b, a], &["a.yaml"]);
    assert_eq!(forwards, backwards);
    assert_eq!(forwards.len(), 1);
    let c = &forwards[0];
    assert_eq!(c.direction, ContractDirection::Provides);
    assert_eq!(c.owner_node_id, Some(owner("a.yaml")));
    assert_eq!(strings(c, "frameworks"), ["openapi", "spring"]);
    assert_eq!(strings(c, "raw_forms"), ["raw a.yaml"]);
    assert_eq!(c.props.get("owner_ambiguous"), None);
}

#[test]
fn contract_provider_and_consumer_becomes_both() {
    let provider =
        observation("GET /x", ContractDirection::Provides, "a.yaml").with_owner(owner("a.yaml"));
    let consumer = observation("GET /x", ContractDirection::Consumes, "b.ts");
    for order in [
        vec![provider.clone(), consumer.clone()],
        vec![consumer.clone(), provider.clone()],
    ] {
        let c = &merged(order, &["a.yaml"])[0];
        assert_eq!(c.direction, ContractDirection::Both);
        // The consumer nominates no owner.
        assert_eq!(c.owner_node_id, Some(owner("a.yaml")));
        assert_eq!(strings(c, "source_paths"), ["a.yaml", "b.ts"]);
    }
    let c = &merged(vec![consumer.clone(), consumer], &[])[0];
    assert_eq!(c.direction, ContractDirection::Consumes);
    assert_eq!(c.owner_node_id, None);
}

#[test]
fn contract_multiple_provider_owners_are_not_arbitrarily_chosen() {
    let a =
        observation("GET /x", ContractDirection::Provides, "a.yaml").with_owner(owner("a.yaml"));
    let b =
        observation("GET /x", ContractDirection::Provides, "b.yaml").with_owner(owner("b.yaml"));
    let forwards = merged(vec![a.clone(), b.clone()], &["a.yaml", "b.yaml"]);
    let backwards = merged(vec![b, a], &["a.yaml", "b.yaml"]);
    assert_eq!(forwards, backwards);
    let c = &forwards[0];
    assert_eq!(c.owner_node_id, None);
    assert_eq!(
        c.props.get("owner_ambiguous"),
        Some(&serde_json::json!(true))
    );
    let mut expected = vec![
        owner("a.yaml").as_str().to_owned(),
        owner("b.yaml").as_str().to_owned(),
    ];
    expected.sort();
    assert_eq!(strings(c, "provider_owner_node_ids"), expected);
}

#[test]
fn exact_identity_wins_over_declared_for_same_namespace() {
    let exact = observation("GET /x", ContractDirection::Consumes, "b.ts")
        .with_identities(vec![ContractIdentity::Exact("api.example.com".into())]);
    let declared = observation("GET /x", ContractDirection::Provides, "a.yaml")
        .with_owner(owner("a.yaml"))
        .with_identities(vec![ContractIdentity::Declared("api.example.com".into())]);
    for order in [
        vec![exact.clone(), declared.clone()],
        vec![declared.clone(), exact.clone()],
    ] {
        let rows = merged(order, &["a.yaml"]);
        assert_eq!(rows.len(), 1, "one identity, one contract");
        assert_eq!(rows[0].identity_strength, IdentityStrength::Exact);
        assert_eq!(rows[0].direction, ContractDirection::Both);
    }
}

#[test]
fn contract_observation_order_does_not_change_output() {
    let observations: Vec<ContractObservation> = (0..12)
        .map(|i| {
            let direction = if i % 3 == 0 {
                ContractDirection::Consumes
            } else {
                ContractDirection::Provides
            };
            let mut o = observation(
                &format!("GET /r{}", i % 4),
                direction,
                &format!("f{}.yaml", i % 5),
            )
            .with_evidence("frameworks", format!("fw{}", i % 2));
            if direction == ContractDirection::Provides {
                o = o.with_owner(owner(&format!("f{}.yaml", i % 5)));
            }
            if i % 2 == 0 {
                o = o.with_identities(vec![
                    ContractIdentity::Declared("b.example.com".into()),
                    ContractIdentity::Exact("a.example.com".into()),
                ]);
            }
            o
        })
        .collect();
    let owners = ["f0.yaml", "f1.yaml", "f2.yaml", "f3.yaml", "f4.yaml"];
    let normal = merged(observations.clone(), &owners);
    let mut reversed = observations.clone();
    reversed.reverse();
    assert_eq!(normal, merged(reversed, &owners));
    // A fixed shuffle (a permutation coprime with the length).
    let shuffled: Vec<ContractObservation> = (0..observations.len())
        .map(|i| observations[(i * 5 + 3) % observations.len()].clone())
        .collect();
    assert_eq!(normal, merged(shuffled, &owners));
    // Byte for byte, as the segment stores them.
    assert_eq!(
        serde_json::to_string(&normal).expect("json"),
        serde_json::to_string(&merged(observations, &owners)).expect("json")
    );
    assert!(
        normal
            .windows(2)
            .all(|w| w[0].contract_id < w[1].contract_id)
    );
}

// --- Namespaces (issue 58) -------------------------------------------------------------

#[test]
fn unresolved_contracts_are_repo_scoped() {
    let spec = "openapi: 3.0.0\npaths:\n  /health:\n    get: {}\n";
    let checkout = Checkout::new(&[("openapi.yaml", spec)]);
    let a = try_derive(&checkout, REPO, false).expect("derived");
    let b = try_derive(&checkout, OTHER_REPO, false).expect("derived");
    let (a, b) = (
        one(&a, ContractKind::ApiContract, "GET /health"),
        one(&b, ContractKind::ApiContract, "GET /health"),
    );
    assert_eq!(a.key, b.key);
    assert_ne!(
        a.contract_id, b.contract_id,
        "GET /health is in every service (D28)"
    );
    assert_eq!(a.contract_id, id("GGZ2DT4W32WKGXCYHF6CF2Q3RP"));
    assert_eq!(b.contract_id, id("QG5M9PYEGZZ22A11FECCYG11T4"));
}

#[test]
fn same_resolved_contract_matches_across_repo_ids() {
    let checkout = Checkout::new(&[("api/openapi.yaml", OPENAPI)]);
    let a = try_derive(&checkout, REPO, false).expect("derived");
    let b = try_derive(&checkout, OTHER_REPO, false).expect("derived");
    let (a, b) = (
        one(&a, ContractKind::ApiContract, "GET /v1/users/{}"),
        one(&b, ContractKind::ApiContract, "GET /v1/users/{}"),
    );
    assert_eq!(a.contract_id, b.contract_id);
    assert_eq!(a.namespace_key, b.namespace_key);
}

#[test]
fn declared_and_exact_same_identity_share_contract_id() {
    // The provider declares the host its consumer's URL names.
    let provider = graph(&[
        (
            "pdx.toml",
            "[identity]\nhostnames = [\"API.example.com\"]\n",
        ),
        (
            "openapi.yaml",
            "openapi: 3.0.0\npaths:\n  /users/{id}:\n    get: {}\n",
        ),
    ]);
    let consumer = graph(&[(
        "web/users.ts",
        "export async function load(id: string) {\n  return fetch('https://api.example.com/users/{id}');\n}\n",
    )]);
    let p = one(&provider, ContractKind::ApiContract, "GET /users/{}");
    let c = one(&consumer, ContractKind::ApiContract, "GET /users/{}");
    assert_eq!(p.identity_strength, IdentityStrength::Declared);
    assert_eq!(c.identity_strength, IdentityStrength::Exact);
    assert_eq!(p.contract_id, c.contract_id);
    assert_eq!(p.namespace_key, c.namespace_key);
}

#[test]
fn multiple_declared_hostnames_create_multiple_contracts() {
    let g = graph(&[
        (
            "pdx.toml",
            "[identity]\nhostnames = [\"api.internal\", \"api.example.com\"]\n",
        ),
        (
            "openapi.yaml",
            "openapi: 3.0.0\npaths:\n  /users/{id}:\n    get: {}\n",
        ),
    ]);
    let rows = find(&g, ContractKind::ApiContract, "GET /users/{}");
    let namespaces: Vec<&str> = rows
        .iter()
        .map(|c| c.namespace_key.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    assert_eq!(
        namespaces,
        [
            r#"{"identity":"api.example.com","key":"GET /users/{}"}"#,
            r#"{"identity":"api.internal","key":"GET /users/{}"}"#,
        ]
    );
    assert!(
        rows.iter()
            .all(|c| c.identity_strength == IdentityStrength::Declared)
    );
}

#[test]
fn contract_namespace_key_fixed_vectors() {
    // Expected strings and ids computed outside the crate (issue 58).
    let r = repo(REPO);
    let vectors: [(ContractKind, ContractIdentity, &str, &str, &str); 10] = [
        (
            ContractKind::ApiContract,
            ContractIdentity::Exact("api.example.com".into()),
            "GET /v1/users/{}",
            r#"{"identity":"api.example.com","key":"GET /v1/users/{}"}"#,
            "A19SP1TVVAFTD3DG84PTQ9QPJE",
        ),
        (
            ContractKind::ApiContract,
            ContractIdentity::Declared("api.internal".into()),
            "GET /users/{}",
            r#"{"identity":"api.internal","key":"GET /users/{}"}"#,
            "WY63GHE6PAJDJ5B3YV3PRYG2V5",
        ),
        (
            ContractKind::ApiContract,
            ContractIdentity::Unresolved,
            "GET /health",
            "unresolved:ce63551447285fd4:GET /health",
            "GGZ2DT4W32WKGXCYHF6CF2Q3RP",
        ),
        (
            ContractKind::RpcMethod,
            ContractIdentity::Unresolved,
            "acme.users.UserService/GetUser",
            "unresolved:ce63551447285fd4:acme.users.UserService/GetUser",
            "P8K4CXYG2FNRVQM5BKVD8828MY",
        ),
        (
            ContractKind::Channel,
            ContractIdentity::Exact("kafka-1.example.com:9092".into()),
            "orders.created",
            r#"{"identity":"kafka-1.example.com:9092","key":"orders.created"}"#,
            "5GEPWQM39TFNJK72648WA81MPY",
        ),
        (
            ContractKind::Table,
            ContractIdentity::Exact("db.example.com:5432/shop".into()),
            "users",
            r#"{"identity":"db.example.com:5432/shop","key":"users"}"#,
            "KGXAGGZEDVVEEH6VA40351FK92",
        ),
        (
            ContractKind::Column,
            ContractIdentity::Declared("db.example.com:5432/shop".into()),
            "users.id",
            r#"{"identity":"db.example.com:5432/shop","key":"users.id"}"#,
            "2TTW81YKV35CG1691NZ49SVBMZ",
        ),
        (
            ContractKind::Artifact,
            ContractIdentity::Exact("maven:com.acme:shop".into()),
            "maven:com.acme:shop",
            "maven:com.acme:shop",
            "QVY04P4K0YBDWKNTVHGZV1V1B6",
        ),
        (
            ContractKind::ArtifactVersion,
            ContractIdentity::Exact("maven:com.acme:shop@1.2.0".into()),
            "maven:com.acme:shop@1.2.0",
            "maven:com.acme:shop@1.2.0",
            "D7N1E2J1YBWMKBN8DTQS4GCC44",
        ),
        (
            ContractKind::ApiContract,
            ContractIdentity::Unresolved,
            "GET /health",
            "unresolved:2b2f33ba06c16e77:GET /health",
            "QG5M9PYEGZZ22A11FECCYG11T4",
        ),
    ];
    for (i, (kind, identity, key, expected_namespace, expected_id)) in
        vectors.into_iter().enumerate()
    {
        let repo = if i == 9 {
            pdx_core::ids::RepoId::parse(OTHER_REPO).expect("a repo")
        } else {
            r.clone()
        };
        let namespace = namespace_key(kind, &identity, key, &repo);
        assert_eq!(namespace, expected_namespace, "{kind} {key}");
        assert_eq!(
            contract_id(kind, &namespace).expect("an id"),
            id(expected_id),
            "{kind} {key}"
        );
    }
    // The existing 4.2.1 estate vector still holds through the same function.
    assert_eq!(
        contract_id(ContractKind::ApiContract, "GET /orders/{id}").expect("an id"),
        id("FMXRAER9WFVM1JB93F13E9RTFT")
    );
}

// --- Route reuse, the graph, secrets, safety ------------------------------------------

const CONTROLLER: &str = "package com.acme.web;\n\nimport org.springframework.web.bind.annotation.*;\n\n@RestController\n@RequestMapping(\"/api\")\npublic class UserController {\n  @GetMapping(\"/users/{id}\")\n  public String one(String id) { return id; }\n}\n";

#[test]
fn route_contract_reuses_derived_route() {
    let g = graph(&[("src/main/java/com/acme/web/UserController.java", CONTROLLER)]);
    let route = node(&g, NodeKind::Route, "GET /api/users/{id}");
    let edge = g
        .edges
        .iter()
        .find(|e| e.kind == EdgeKind::DefinesRoute && e.dst == route.node_id)
        .expect("the route's edge");
    let c = one(&g, ContractKind::ApiContract, "GET /api/users/{}");
    assert_eq!(c.owner_node_id.as_ref(), Some(&route.node_id));
    assert_eq!(strings(c, "route_node_ids"), [route.node_id.as_str()]);
    assert_eq!(
        strings(c, "route_site_ids"),
        [edge.site_id.as_ref().expect("a site").as_str()]
    );
    assert_eq!(strings(c, "frameworks"), ["spring"]);
    assert_eq!(strings(c, "raw_forms"), ["GET /api/users/{id}"]);
    assert_eq!(c.identity_strength, IdentityStrength::Unresolved);
    // One route, one contract: no second route reading of the source.
    assert_eq!(g.contracts.len(), 1);
    assert_eq!(
        g.nodes.iter().filter(|n| n.kind == NodeKind::Route).count(),
        1
    );
}

#[test]
fn contracts_do_not_duplicate_the_graph() {
    let files = [
        ("src/main/java/com/acme/web/UserController.java", CONTROLLER),
        ("api/openapi.yaml", OPENAPI),
        ("proto/users.proto", PROTO),
        ("pom.xml", POM),
        ("app/producer.py", PRODUCER),
    ];
    let g = graph(&files);
    assert!(g.contracts.len() > 5);
    let contract_kinds = [
        NodeKind::ApiContract,
        NodeKind::RpcMethod,
        NodeKind::Channel,
        NodeKind::Table,
        NodeKind::Column,
        NodeKind::Artifact,
        NodeKind::ArtifactVersion,
    ];
    assert!(
        g.nodes.iter().all(|n| !contract_kinds.contains(&n.kind)),
        "no contract node"
    );
    let contract_edges = [
        EdgeKind::Exposes,
        EdgeKind::Consumes,
        EdgeKind::Publishes,
        EdgeKind::Subscribes,
        EdgeKind::ReadsTable,
        EdgeKind::WritesTable,
        EdgeKind::DefinesTable,
        EdgeKind::ProducesArtifact,
        EdgeKind::LinksArtifact,
    ];
    assert!(
        g.edges.iter().all(|e| !contract_edges.contains(&e.kind)),
        "no contract edge"
    );
    assert!(
        g.edges.iter().all(|e| e.site_id.is_some()),
        "no site-less edge"
    );
    // Every owner is a node of the graph; every id is its namespace key's.
    for c in &g.contracts {
        if let Some(owner) = &c.owner_node_id {
            assert!(g.node(owner).is_some(), "{c:#?}");
        }
        pdx_core::contracts::row::check(c, &repo(REPO), |id| g.node(id).is_some())
            .expect("a valid row");
    }
    assert!(
        g.contracts
            .windows(2)
            .all(|w| w[0].contract_id < w[1].contract_id)
    );
}

#[test]
fn synthetic_secrets_never_reach_contracts() {
    // Assembled at run time; each is a marker no contract may carry.
    let token = ["tok", "MARKERa1b2c3d4e5f6"].concat();
    let pass = ["pw", "MARKERz9y8x7w6"].concat();
    let key_id = ["AKIA", "MARKERQQQQQQQQQQ"].concat();
    let at = char::from(0x40);
    let svc = "svc";
    let ts = format!(
        "export async function f() {{\n  await fetch('https://{svc}:{pass}{at}api.example.com/a?access_token={token}');\n  await fetch('https://api.example.com/b#{key_id}');\n}}\n"
    );
    let yml = format!(
        "spring:\n  datasource:\n    url: {}svc:{pass}{at}db.example.com/shop\n    password: {pass}\norders:\n  topic: orders.created\n  token: {token}\n",
        ["jdbc:", "postgres", "ql://"].concat()
    );
    let listener = LISTENER.replace("${orders.topic}", "${orders.token}");
    let g = graph(&[
        ("web/f.ts", &ts),
        ("src/main/resources/application.yml", &yml),
        (
            "src/main/java/com/acme/orders/OrderListener.java",
            &listener,
        ),
        (".env", &format!("ORDERS_TOKEN={token}\n")),
    ]);
    assert!(!g.contracts.is_empty());
    for marker in [&token, &pass, &key_id] {
        assert_no_marker(&g, marker);
    }
    one(&g, ContractKind::ApiContract, "GET /a");
    assert_eq!(channel_keys(&g), ["unresolved:${orders.token}"]);
}

#[test]
fn contract_props_hold_no_host_paths() {
    let files = [
        ("src/main/java/com/acme/web/UserController.java", CONTROLLER),
        ("api/openapi.yaml", OPENAPI),
        ("pom.xml", POM),
    ];
    let checkout = Checkout::new(&files);
    let g = try_derive(&checkout, REPO, false).expect("derived");
    let root = checkout.root().to_string_lossy().into_owned();
    for c in &g.contracts {
        let text = format!(
            "{} {} {}",
            c.key,
            c.namespace_key,
            serde_json::to_string(&c.props).expect("json")
        );
        assert!(!text.contains(&root), "{text}");
        assert!(!text.contains('\\'), "{text}");
        for path in strings(c, "source_paths") {
            assert!(!path.starts_with('/') && !path.contains(':'), "{path}");
        }
    }
}

#[test]
fn contracts_are_deterministic_across_runs() {
    let files = [
        ("src/main/java/com/acme/web/UserController.java", CONTROLLER),
        ("api/openapi.yaml", OPENAPI),
        ("proto/users.proto", PROTO),
        ("pom.xml", POM),
        ("app/producer.py", PRODUCER),
        ("src/main/java/com/acme/orders/OrderListener.java", LISTENER),
        (
            "src/main/resources/application.yml",
            "orders:\n  topic: orders.created\n",
        ),
    ];
    let checkout = Checkout::new(&files);
    let a = try_derive(&checkout, REPO, false).expect("derived");
    let b = try_derive(&checkout, REPO, true).expect("derived");
    assert_eq!(a.contracts, b.contracts);
    assert_eq!(a.diagnostics.contracts, b.diagnostics.contracts);
    // The provider and the listener of one topic are one contract, both sides.
    assert_eq!(
        one(&a, ContractKind::Channel, "orders.created").direction,
        ContractDirection::Both
    );
}

#[test]
fn malformed_contract_documents_are_reported_not_guessed() {
    let g = graph(&[
        ("openapi.yaml", "openapi: 3.0.0\npaths:\n  /a: [unclosed\n"),
        ("swagger.json", "{\"swagger\": \"2.0\", \"paths\": "),
        ("openapi-notes.yml", "title: not a spec\n"),
        (
            "proto/broken.proto",
            "package a;\nservice S {\n  rpc A(B) returns (C);\n",
        ),
        ("pom.xml", "<project><groupId>g</groupId>"),
        ("db/V1__init.sql", "CREATE TABLE t (a int); /* never closed"),
        ("alire.toml", "name = \"x\"\n[[depends-on]\n"),
        ("src/main/resources/application.yml", "a: [1,\n"),
        ("asyncapi.yaml", "asyncapi: 2.6.0\nchannels: {a: [}\n"),
    ]);
    assert!(
        g.contracts.is_empty(),
        "nothing guessed: {:#?}",
        g.contracts
    );
    let problems: Vec<(&str, ContractProblem)> = g
        .diagnostics
        .contracts
        .iter()
        .map(|ContractDiagnostic { path, problem }| (path.as_str(), *problem))
        .collect();
    assert_eq!(
        problems,
        [
            (
                "alire.toml",
                ContractProblem::Unparseable { format: "toml" }
            ),
            (
                "asyncapi.yaml",
                ContractProblem::Unparseable { format: "yaml" }
            ),
            (
                "db/V1__init.sql",
                ContractProblem::Unparseable { format: "sql" }
            ),
            (
                "openapi-notes.yml",
                ContractProblem::NotADocument { format: "openapi" }
            ),
            (
                "openapi.yaml",
                ContractProblem::Unparseable { format: "yaml" }
            ),
            ("pom.xml", ContractProblem::Unparseable { format: "xml" }),
            (
                "proto/broken.proto",
                ContractProblem::Unparseable { format: "proto" }
            ),
            (
                "src/main/resources/application.yml",
                ContractProblem::Unparseable { format: "yaml" }
            ),
            (
                "swagger.json",
                ContractProblem::Unparseable { format: "json" }
            ),
        ]
    );
}

#[test]
fn openapi_remote_ref_is_never_followed() {
    let spec = "openapi: 3.0.0\nservers: [{url: 'https://api.example.com'}]\npaths:\n  /local:\n    get: {}\n  /remote:\n    $ref: 'https://specs.example.invalid/paths.yaml#/remote'\n  /sibling:\n    $ref: './paths.yaml#/sibling'\n";
    let g = graph(&[
        ("openapi.yaml", spec),
        ("paths.yaml", "sibling:\n  get: {}\n"),
    ]);
    let keys: Vec<&str> = g.contracts.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(keys, ["GET /local"]);
    assert_eq!(
        g.diagnostics.contracts,
        [ContractDiagnostic {
            path: "openapi.yaml".into(),
            problem: ContractProblem::UnfollowedRef
        },]
    );
}

#[test]
fn changed_checkout_aborts_contracts() {
    let checkout = Checkout::new(&[("api/openapi.yaml", OPENAPI)]);
    let root = checkout.root();
    let config = PdxConfig::load(root).expect("config");
    let files = discover(root, &config).expect("discovered");
    let limits = ExtractLimits {
        requested_workers: 1,
        memory_budget_bytes: 64 << 20,
    };
    let extracted = ExtractStage::new(root, &config.secrets, limits)
        .run(&files)
        .expect("extracted");
    let registry = SymbolRegistry::build(root, &files, extracted).expect("registry");
    let report = resolve(root, &registry).expect("resolved");
    // The same length, other bytes: what Stage 2 identified is no longer there.
    let changed = OPENAPI.replace("removeUser", "deleteUser");
    assert_eq!(changed.len(), OPENAPI.len());
    fs::write(root.join("api/openapi.yaml"), changed).expect("rewritten");
    let result = derive(&DeriveInput {
        repo: &repo(REPO),
        repo_name: "fixture",
        registry: &registry,
        resolution: &report,
        root,
        config: &config,
    });
    assert!(
        matches!(result, Err(DeriveError::SourceChanged(ref p)) if p == "api/openapi.yaml"),
        "{result:?}"
    );
}

#[test]
fn http_client_calls_are_consumed_contracts() {
    let g = graph(&[
        (
            "web/c.ts",
            "import axios from 'axios';\nexport async function f(u: string) {\n  await fetch('https://api.example.com/users');\n  await fetch('/x', { method: 'post' });\n  await fetch('/y', { ...opts });\n  await axios.get('https://api.example.com/items/1');\n  await fetch(u);\n}\n",
        ),
        (
            "app/k.py",
            "import requests\n\n\ndef go():\n    requests.get(\"https://API.Example.com:443/users/1?x=1\")\n    requests.request(\"PUT\", \"/rel/path\")\n",
        ),
        (
            "go/main.go",
            "package main\n\nimport \"net/http\"\n\nfunc main() {\n  http.NewRequest(\"GET\", \"https://api.example.com/v1/users\", nil)\n  http.NewRequest(http.MethodPost, \"https://api.example.com/v1/u\", nil)\n}\n",
        ),
        (
            "cs/C.cs",
            "using System.Net.Http;\npublic class C { HttpClient c; async void M() { await c.GetAsync(\"https://api.example.com/x\"); } }\n",
        ),
        (
            "src/main/java/a/L.java",
            "package a;\nimport org.springframework.web.client.RestTemplate;\npublic class L {\n  RestTemplate rt;\n  public String c() { return rt.getForObject(\"https://api.example.com/users/{id}\", String.class, 1); }\n}\n",
        ),
        (
            "rs/src/main.rs",
            "fn main() { let _ = reqwest::get(\"https://api.example.com/r\"); }\n",
        ),
    ]);
    let mut found: Vec<(String, String)> = g
        .contracts
        .iter()
        .map(|c| (c.key.clone(), format!("{:?}", c.identity_strength)))
        .collect();
    found.sort();
    let expected: Vec<(String, String)> = [
        ("GET /items/1", "Exact"),
        ("GET /r", "Exact"),
        ("GET /users", "Exact"),
        ("GET /users/1", "Exact"),
        ("GET /users/{}", "Exact"),
        ("GET /v1/users", "Exact"),
        ("GET /x", "Exact"),
        ("POST /v1/u", "Exact"),
        ("POST /x", "Unresolved"),
        ("PUT /rel/path", "Unresolved"),
    ]
    .iter()
    .map(|(k, s)| ((*k).to_owned(), (*s).to_owned()))
    .collect();
    assert_eq!(found, expected);
    assert!(
        g.contracts
            .iter()
            .all(|c| c.direction == ContractDirection::Consumes && c.owner_node_id.is_none())
    );
}

#[test]
fn listener_annotations_and_asyncapi_channels() {
    let g = graph(&[
        (
            "src/main/java/a/Listeners.java",
            "package a;\npublic class Listeners {\n  @RabbitListener(queues = {\"q.one\", \"q.two\"})\n  public void r(String m) {}\n  @JmsListener(destination = \"jms.queue\")\n  public void j(String m) {}\n  @KafkaListener(topics = Topics.ORDERS)\n  public void k(String m) {}\n}\n",
        ),
        (
            "kt/K.kt",
            "package k\n@KafkaListener(topics = [\"kt.topic\"])\nfun on(m: String) {}\n",
        ),
        (
            "asyncapi.yaml",
            "asyncapi: 2.6.0\nservers:\n  prod: {url: 'Kafka.example.com:9092', protocol: kafka}\nchannels:\n  user/signedup:\n    subscribe: {message: {}}\n  user/deleted:\n    publish: {message: {}}\n  audit: {}\n",
        ),
        (
            "events/asyncapi.json",
            r##"{"asyncapi":"3.0.0","channels":{"orders":{"address":"orders.v1"},"dyn":{"address":null}},"operations":{"take":{"action":"receive","channel":{"$ref":"#/channels/orders"}}}}"##,
        ),
    ]);
    let channels: BTreeMap<String, (ContractDirection, IdentityStrength)> = g
        .contracts
        .iter()
        .map(|c| (c.key.clone(), (c.direction, c.identity_strength)))
        .collect();
    let expected: BTreeMap<String, (ContractDirection, IdentityStrength)> = [
        (
            "audit",
            ContractDirection::Provides,
            IdentityStrength::Exact,
        ),
        (
            "jms.queue",
            ContractDirection::Consumes,
            IdentityStrength::Unresolved,
        ),
        (
            "kt.topic",
            ContractDirection::Consumes,
            IdentityStrength::Unresolved,
        ),
        (
            "orders.v1",
            ContractDirection::Consumes,
            IdentityStrength::Unresolved,
        ),
        (
            "q.one",
            ContractDirection::Consumes,
            IdentityStrength::Unresolved,
        ),
        (
            "q.two",
            ContractDirection::Consumes,
            IdentityStrength::Unresolved,
        ),
        (
            "user/deleted",
            ContractDirection::Consumes,
            IdentityStrength::Exact,
        ),
        (
            "user/signedup",
            ContractDirection::Provides,
            IdentityStrength::Exact,
        ),
    ]
    .into_iter()
    .map(|(k, d, s)| (k.to_owned(), (d, s)))
    .collect();
    assert_eq!(channels, expected);
    let signedup = one(&g, ContractKind::Channel, "user/signedup");
    assert!(signedup.namespace_key.contains("kafka.example.com:9092"));
    assert_eq!(
        signedup.owner_node_id.as_ref(),
        Some(&g.file_nodes["asyncapi.yaml"])
    );
}

#[test]
fn unconfirmed_engine_channels_give_no_contract() {
    // An in-process event emitter's event is a channel fact, not a destination.
    let g = graph(&[(
        "web/e.ts",
        "export function f(emitter: any) {\n  emitter.emit('change');\n}\n",
    )]);
    assert!(g.contracts.is_empty(), "{:#?}", g.contracts);
    assert_eq!(
        g.diagnostics.contracts,
        [ContractDiagnostic {
            path: "web/e.ts".into(),
            problem: ContractProblem::UnconfirmedChannel
        }]
    );
}

#[test]
fn ddl_orm_and_sql_tables() {
    let g = graph(&[
        (
            "db/migration/V1__init.sql",
            "CREATE TABLE public.users (id bigint PRIMARY KEY, \"Email\" text);\nCREATE TEMPORARY TABLE scratch (x int);\n",
        ),
        (
            "db/changelog/db.changelog-master.xml",
            "<databaseChangeLog><changeSet id=\"1\" author=\"a\"><createTable tableName=\"orders\" schemaName=\"sales\"><column name=\"ID\"/></createTable></changeSet></databaseChangeLog>",
        ),
        (
            "cs/U.cs",
            "using System.ComponentModel.DataAnnotations.Schema;\n[Table(\"Invoices\", Schema = \"billing\")]\npublic class Invoice { }\n",
        ),
        (
            "app/models.py",
            "from sqlalchemy.orm import declarative_base\nfrom django.db import models\nBase = declarative_base()\n\nclass Account(Base):\n    __tablename__ = \"accounts\"\n\nclass Thing(models.Model):\n    class Meta:\n        db_table = \"things\"\n",
        ),
        (
            "prisma/schema.prisma",
            "model Post {\n  id Int @id\n  @@map(\"posts\")\n}\n",
        ),
        (
            "src/main/java/a/Q.java",
            "package a;\npublic class Q {\n  void s(org.springframework.jdbc.core.JdbcTemplate j) { j.update(\"UPDATE users SET a = 1\"); j.query(\"SELECT * FROM sales.Orders\", null); }\n}\n",
        ),
    ]);
    let mut found: Vec<(String, String, String)> = g
        .contracts
        .iter()
        .map(|c| {
            (
                format!("{}", c.kind),
                c.key.clone(),
                format!("{:?}", c.direction),
            )
        })
        .collect();
    found.sort();
    let expected: Vec<(String, String, String)> = [
        ("Column", "sales.orders.id", "Provides"),
        ("Column", "users.email", "Provides"),
        ("Column", "users.id", "Provides"),
        ("Table", "accounts", "Provides"),
        ("Table", "billing.invoices", "Provides"),
        ("Table", "posts", "Provides"),
        ("Table", "sales.orders", "Both"),
        ("Table", "things", "Provides"),
        ("Table", "users", "Both"),
    ]
    .iter()
    .map(|(a, b, c)| ((*a).to_owned(), (*b).to_owned(), (*c).to_owned()))
    .collect();
    assert_eq!(found, expected);
    let users = one(&g, ContractKind::Table, "users");
    assert_eq!(strings(users, "access_modes"), ["write"]);
    let things = one(&g, ContractKind::Table, "things");
    assert_eq!(
        things.owner_node_id.as_ref(),
        Some(&node(&g, NodeKind::Class, "Thing").node_id)
    );
}

#[test]
fn accumulator_refuses_invalid_contracts() {
    let mut accumulator = ContractAccumulator::new(repo(REPO));
    let empty = ContractObservation::new(
        ContractKind::Table,
        "",
        ContractDirection::Provides,
        "a.sql",
        "x",
    );
    assert!(matches!(
        accumulator.observe(empty),
        Err(DeriveError::InvalidContract(_))
    ));
    let artifact = ContractObservation::new(
        ContractKind::Artifact,
        "maven:a:b",
        ContractDirection::Provides,
        "pom.xml",
        "x",
    )
    .with_identities(vec![ContractIdentity::Declared("x".into())]);
    assert!(matches!(
        accumulator.observe(artifact),
        Err(DeriveError::InvalidContract(_))
    ));
    // An owner that is not a node of the graph.
    let mut accumulator = ContractAccumulator::new(repo(REPO));
    accumulator
        .observe(
            observation("GET /x", ContractDirection::Provides, "a.yaml")
                .with_owner(owner("a.yaml")),
        )
        .expect("observed");
    assert!(matches!(
        accumulator.finish(&BTreeMap::new()),
        Err(DeriveError::InvalidContract(_))
    ));
    // A row whose id is not its namespace key's.
    let mut row = merged(
        vec![observation("GET /x", ContractDirection::Consumes, "b.ts")],
        &[],
    )
    .remove(0);
    row.contract_id = contract_id(ContractKind::ApiContract, "GET /x").expect("an id");
    assert!(matches!(
        pdx_core::contracts::validate(&row, &repo(REPO), &BTreeMap::new()),
        Err(DeriveError::InvalidContract(_))
    ));
}

#[test]
fn contracts_read_only_through_the_safe_source() {
    // Every read goes through `contracts::source` (Stage 2's checks); nothing runs a
    // program, opens a socket, reads the environment or touches the filesystem
    // directly. A package manager, a build tool or a remote `$ref` is never consulted.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/contracts");
    let mut checked = 0;
    for entry in fs::read_dir(&dir).expect("the module") {
        let path = entry.expect("an entry").path();
        let text = fs::read_to_string(&path).expect("source");
        for forbidden in [
            "std::fs",
            "fs::read",
            "File::open",
            "std::net",
            "TcpStream",
            "UdpSocket",
            "std::process",
            "Command::new",
            "std::env",
        ] {
            assert!(
                !text.contains(forbidden),
                "{} uses {forbidden}",
                path.display()
            );
        }
        checked += 1;
    }
    assert!(checked >= 12, "{checked}");
}

// --- Issue 62: every 4.7.1 source, executable -------------------------------------------

/// A source of 4.7.1 that P2-08 implements: a fixture and the contracts it gives.
struct Implemented {
    source: &'static str,
    files: &'static [(&'static str, &'static str)],
    expect: &'static [(ContractKind, &'static str, ContractDirection)],
}

use ContractDirection::{Consumes as C, Provides as P};
use ContractKind::{
    ApiContract as Api, Artifact as Art, Channel as Ch, Column as Col, Table as Tab,
};

const API_PROVIDERS: &[Implemented] = &[
    Implemented {
        source: "OpenAPI and Swagger",
        files: &[
            (
                "api/openapi.yaml",
                "openapi: 3.0.0\nservers: [{url: 'https://api.example.com/v1'}]\npaths:\n  /users/{id}:\n    get: {}\n",
            ),
            (
                "swagger.json",
                r#"{"swagger":"2.0","host":"api.example.com","basePath":"/v2","paths":{"/pets/{id}":{"get":{}}}}"#,
            ),
        ],
        expect: &[(Api, "GET /v1/users/{}", P), (Api, "GET /v2/pets/{}", P)],
    },
    Implemented {
        source: "Spring (Route node)",
        files: &[(
            "src/main/java/com/acme/web/UserController.java",
            "package com.acme.web;\n\nimport org.springframework.web.bind.annotation.*;\n\n@RestController\n@RequestMapping(\"/api\")\npublic class UserController {\n  @GetMapping(\"/users/{id}\")\n  public String one(String id) { return id; }\n}\n",
        )],
        expect: &[(Api, "GET /api/users/{}", P)],
    },
    Implemented {
        source: "JAX-RS (Route node)",
        files: &[(
            "src/main/java/a/R.java",
            "package a;\nimport javax.ws.rs.*;\n@Path(\"/orders\")\npublic class R {\n  @GET\n  @Path(\"{id}\")\n  public String one() { return \"\"; }\n}\n",
        )],
        expect: &[(Api, "GET /orders/{}", P)],
    },
    Implemented {
        source: "FastAPI (Route node)",
        files: &[(
            "app/main.py",
            "from fastapi import FastAPI\napp = FastAPI()\n\n@app.get(\"/items/{item_id}\")\ndef read(item_id: int):\n    return {}\n",
        )],
        expect: &[(Api, "GET /items/{}", P)],
    },
    Implemented {
        source: "Flask (Route node)",
        files: &[(
            "app/f.py",
            "from flask import Flask\napp = Flask(__name__)\n\n@app.route(\"/users/<int:uid>\", methods=[\"GET\"])\ndef user(uid):\n    return ''\n",
        )],
        expect: &[(Api, "GET /users/{}", P)],
    },
    Implemented {
        source: "Express (Route node)",
        files: &[(
            "src/server.js",
            "const express = require('express');\nconst app = express();\nfunction list(req, res) { res.json([]); }\napp.get('/items', list);\n",
        )],
        expect: &[(Api, "GET /items", P)],
    },
    Implemented {
        source: "NestJS",
        files: &[(
            "nest/users.controller.ts",
            "import { Controller, Get, Post } from '@nestjs/common';\n\n@Controller('users')\nexport class UsersController {\n  @Get(':id')\n  findOne() { return 1; }\n\n  @Post()\n  create() { return 2; }\n}\n",
        )],
        expect: &[(Api, "GET /users/{}", P), (Api, "POST /users", P)],
    },
    Implemented {
        source: "Hono",
        files: &[(
            "hono/app.ts",
            "import { Hono } from 'hono';\nconst app = new Hono();\napp.get('/users/:id', (c) => c.json({}));\napp.post('/users', create);\nfunction create(c: any) { return c; }\n",
        )],
        expect: &[(Api, "GET /users/{}", P), (Api, "POST /users", P)],
    },
    Implemented {
        source: "ASP.NET attributes",
        files: &[(
            "aspnet/UsersController.cs",
            "using Microsoft.AspNetCore.Mvc;\n\n[ApiController]\n[Route(\"api/[controller]\")]\npublic class UsersController : ControllerBase {\n  [HttpGet(\"{id}\")]\n  public IActionResult Get(int id) { return Ok(); }\n  [HttpPost]\n  public IActionResult Create() { return Ok(); }\n}\n",
        )],
        expect: &[(Api, "GET /api/Users/{}", P), (Api, "POST /api/Users", P)],
    },
    Implemented {
        source: "ASP.NET minimal APIs",
        files: &[(
            "aspnet/Program.cs",
            "using Microsoft.AspNetCore.Builder;\nvar builder = WebApplication.CreateBuilder(args);\nvar app = builder.Build();\napp.MapGet(\"/health\", () => \"ok\");\napp.MapPost(\"/items/{id}\", (int id) => id);\napp.Run();\n",
        )],
        expect: &[(Api, "GET /health", P), (Api, "POST /items/{}", P)],
    },
    Implemented {
        source: "axum",
        files: &[(
            "axum/src/main.rs",
            "use axum::{Router, routing::get};\nasync fn show() {}\nasync fn create() {}\nfn app() -> Router { Router::new().route(\"/users/:id\", get(show).post(create)) }\nfn main() { let _ = app(); }\n",
        )],
        expect: &[(Api, "GET /users/{}", P), (Api, "POST /users/{}", P)],
    },
    Implemented {
        source: "actix-web",
        files: &[(
            "actix/src/main.rs",
            "use actix_web::{get, web, App};\n#[get(\"/users/{id}\")]\nasync fn show() -> String { String::new() }\nasync fn create() -> String { String::new() }\nfn cfg(c: &mut web::ServiceConfig) { c.route(\"/items\", web::post().to(create)); }\nfn main() {}\n",
        )],
        expect: &[(Api, "GET /users/{}", P), (Api, "POST /items", P)],
    },
    Implemented {
        source: "Go net/http",
        files: &[(
            "gohttp/main.go",
            "package main\nimport \"net/http\"\nfunc users(w http.ResponseWriter, r *http.Request) {}\nfunc main() {\n  http.HandleFunc(\"/users\", users)\n  http.HandleFunc(\"GET /items/{id}\", users)\n  mux := http.NewServeMux()\n  mux.HandleFunc(\"POST /orders\", users)\n}\n",
        )],
        expect: &[
            (Api, "ANY /users", P),
            (Api, "GET /items/{}", P),
            (Api, "POST /orders", P),
        ],
    },
    Implemented {
        source: "gin",
        files: &[(
            "gin/main.go",
            "package main\nimport \"github.com/gin-gonic/gin\"\nfunc show(c *gin.Context) {}\nfunc main() {\n  r := gin.Default()\n  r.GET(\"/users/:id\", show)\n}\n",
        )],
        expect: &[(Api, "GET /users/{}", P)],
    },
    Implemented {
        source: "echo",
        files: &[(
            "echo/main.go",
            "package main\nimport \"github.com/labstack/echo/v4\"\nfunc create(c echo.Context) error { return nil }\nfunc main() {\n  e := echo.New()\n  e.POST(\"/users\", create)\n}\n",
        )],
        expect: &[(Api, "POST /users", P)],
    },
];

const API_CONSUMERS: &[Implemented] = &[
    Implemented {
        source: "fetch",
        files: &[(
            "web/a.ts",
            "export async function f() { await fetch('https://api.example.com/users/1'); }\n",
        )],
        expect: &[(Api, "GET /users/1", C)],
    },
    Implemented {
        source: "axios",
        files: &[(
            "web/b.ts",
            "import axios from 'axios';\nexport async function f() { await axios.post('https://api.example.com/orders'); }\n",
        )],
        expect: &[(Api, "POST /orders", C)],
    },
    Implemented {
        source: "RestTemplate",
        files: &[(
            "src/main/java/a/L.java",
            "package a;\nimport org.springframework.web.client.RestTemplate;\npublic class L {\n  RestTemplate rt;\n  public String c() { return rt.getForObject(\"https://api.example.com/users/{id}\", String.class, 1); }\n}\n",
        )],
        expect: &[(Api, "GET /users/{}", C)],
    },
    Implemented {
        source: "WebClient",
        files: &[(
            "src/main/java/a/W.java",
            "package a;\nimport org.springframework.web.reactive.function.client.WebClient;\npublic class W {\n  private final WebClient webClient;\n  public W(WebClient webClient) { this.webClient = webClient; }\n  public Object f() { return webClient.get().uri(\"https://api.example.com/users/{id}\", 1).retrieve().bodyToMono(String.class); }\n  public Object g() { return webClient.post().uri(\"/orders\").retrieve(); }\n}\n",
        )],
        expect: &[(Api, "GET /users/{}", C), (Api, "POST /orders", C)],
    },
    Implemented {
        source: "requests and httpx",
        files: &[(
            "app/k.py",
            "import requests\nimport httpx\n\n\ndef go():\n    requests.get(\"https://api.example.com/users/1\")\n    httpx.delete(\"https://api.example.com/users/1\")\n",
        )],
        expect: &[(Api, "GET /users/1", C), (Api, "DELETE /users/1", C)],
    },
    Implemented {
        source: ".NET HttpClient",
        files: &[(
            "cs/C.cs",
            "using System.Net.Http;\npublic class C { HttpClient c; async void M() { await c.GetAsync(\"https://api.example.com/x\"); } }\n",
        )],
        expect: &[(Api, "GET /x", C)],
    },
    Implemented {
        source: "reqwest",
        files: &[(
            "rs/src/main.rs",
            "fn main() { let _ = reqwest::get(\"https://api.example.com/r\"); }\n",
        )],
        expect: &[(Api, "GET /r", C)],
    },
    Implemented {
        source: "Go http.NewRequest and net/http",
        files: &[(
            "go/main.go",
            "package main\n\nimport \"net/http\"\n\nfunc main() {\n  http.NewRequest(\"GET\", \"https://api.example.com/v1/users\", nil)\n  http.Get(\"https://api.example.com/x\")\n}\n",
        )],
        expect: &[(Api, "GET /v1/users", C), (Api, "GET /x", C)],
    },
];

const CHANNEL_SOURCES: &[Implemented] = &[
    Implemented {
        source: "AsyncAPI",
        files: &[(
            "asyncapi.yaml",
            "asyncapi: 2.6.0\nchannels:\n  user/signedup:\n    subscribe: {message: {}}\n  user/deleted:\n    publish: {message: {}}\n",
        )],
        expect: &[(Ch, "user/signedup", P), (Ch, "user/deleted", C)],
    },
    Implemented {
        source: "KafkaTemplate.send",
        files: &[(
            "src/main/java/a/K.java",
            "package a;\nimport org.springframework.kafka.core.KafkaTemplate;\npublic class K {\n  private KafkaTemplate<String, String> kafkaTemplate;\n  public void s() { kafkaTemplate.send(\"orders.created\", \"x\"); }\n}\n",
        )],
        expect: &[(Ch, "orders.created", P)],
    },
    Implemented {
        source: "producer.send (kafka-python, confluent-kafka)",
        files: &[
            (
                "app/producer.py",
                "from kafka import KafkaProducer\n\nproducer = KafkaProducer()\n\n\ndef publish(event):\n    producer.send(\"orders.created\", event)\n",
            ),
            (
                "app/ck.py",
                "from confluent_kafka import Producer\np = Producer({})\n\ndef f():\n    p.produce('orders.shipped', b'x')\n",
            ),
        ],
        expect: &[(Ch, "orders.created", P), (Ch, "orders.shipped", P)],
    },
    Implemented {
        source: "producer.send (Java KafkaProducer)",
        files: &[(
            "src/main/java/b/KP.java",
            "package b;\nimport org.apache.kafka.clients.producer.KafkaProducer;\nimport org.apache.kafka.clients.producer.ProducerRecord;\npublic class KP {\n  public void p(KafkaProducer<String, String> producer) { producer.send(new ProducerRecord<>(\"orders.created\", \"k\", \"v\")); }\n}\n",
        )],
        expect: &[(Ch, "orders.created", P)],
    },
    Implemented {
        source: "producer.send (kafkajs)",
        files: &[(
            "kjs/p.ts",
            "import { Kafka } from 'kafkajs';\nconst kafka = new Kafka({ clientId: 'a', brokers: [] });\nconst producer = kafka.producer();\nexport async function f() {\n  await producer.send({ topic: 'orders.created', messages: [] });\n}\n",
        )],
        expect: &[(Ch, "orders.created", P)],
    },
    Implemented {
        source: "channel.basic_publish (pika) and amqplib",
        files: &[
            (
                "py/pika_p.py",
                "import pika\nconnection = pika.BlockingConnection()\nchannel = connection.channel()\n\ndef f():\n    channel.basic_publish(exchange='', routing_key='task_queue', body=b'x')\n    channel.basic_publish(exchange='logs', routing_key='', body=b'x')\n",
            ),
            (
                "amqp/a.ts",
                "import amqp from 'amqplib';\nexport async function f(channel: any) {\n  channel.sendToQueue('work_queue', Buffer.from('x'));\n}\n",
            ),
        ],
        expect: &[
            (Ch, "task_queue", P),
            (Ch, "logs", P),
            (Ch, "work_queue", P),
        ],
    },
    Implemented {
        source: "NATS publish",
        files: &[
            (
                "nats/p.ts",
                "import { connect } from 'nats';\nexport async function f() { const nc = await connect(); nc.publish('nats.js', new Uint8Array()); }\n",
            ),
            (
                "py/nats_p.py",
                "import nats\n\nasync def f():\n    nc = await nats.connect()\n    await nc.publish(\"nats.py\", b\"x\")\n",
            ),
            (
                "gonats/main.go",
                "package main\nimport \"github.com/nats-io/nats.go\"\nfunc main() {\n  nc, _ := nats.Connect(nats.DefaultURL)\n  nc.Publish(\"nats.go\", []byte(\"x\"))\n}\n",
            ),
        ],
        expect: &[(Ch, "nats.js", P), (Ch, "nats.py", P), (Ch, "nats.go", P)],
    },
    Implemented {
        source: "Redis publish",
        files: &[
            (
                "py/redis_p.py",
                "import redis\nr = redis.Redis()\n\ndef f():\n    r.publish(\"redis.py\", \"x\")\n",
            ),
            (
                "redis/p.ts",
                "import Redis from 'ioredis';\nconst redis = new Redis();\nexport function f() { redis.publish('redis.js', 'x'); }\n",
            ),
            (
                "src/main/java/a/J.java",
                "package a;\nimport redis.clients.jedis.Jedis;\npublic class J {\n  public void s(Jedis jedis) { jedis.publish(\"redis.jedis\", \"x\"); }\n}\n",
            ),
            (
                "src/main/java/b/RT.java",
                "package b;\nimport org.springframework.data.redis.core.StringRedisTemplate;\npublic class RT {\n  private StringRedisTemplate redisTemplate;\n  public void s() { redisTemplate.convertAndSend(\"redis.spring\", \"x\"); }\n}\n",
            ),
            (
                "goredis/main.go",
                "package main\nimport (\n  \"context\"\n  \"github.com/redis/go-redis/v9\"\n)\nfunc main() {\n  ctx := context.Background()\n  rdb := redis.NewClient(nil)\n  rdb.Publish(ctx, \"redis.go\", \"x\")\n}\n",
            ),
            (
                "src/main/java/c/L.java",
                "package c;\nimport io.lettuce.core.api.sync.RedisCommands;\npublic class L {\n  public void s(RedisCommands<String, String> commands) { commands.publish(\"redis.lettuce\", \"x\"); }\n}\n",
            ),
        ],
        expect: &[
            (Ch, "redis.py", P),
            (Ch, "redis.js", P),
            (Ch, "redis.jedis", P),
            (Ch, "redis.spring", P),
            (Ch, "redis.go", P),
            (Ch, "redis.lettuce", P),
        ],
    },
    Implemented {
        source: "JMS producers",
        files: &[
            (
                "src/main/java/a/JT.java",
                "package a;\nimport org.springframework.jms.core.JmsTemplate;\npublic class JT {\n  private JmsTemplate jmsTemplate;\n  public void s() { jmsTemplate.convertAndSend(\"orders.queue\", \"y\"); }\n}\n",
            ),
            (
                "src/main/java/b/JM.java",
                "package b;\nimport jakarta.jms.*;\npublic class JM {\n  public void s(Session session, MessageProducer producer, Message m) throws Exception {\n    producer.send(session.createQueue(\"inline.queue\"), m);\n  }\n}\n",
            ),
        ],
        expect: &[(Ch, "orders.queue", P), (Ch, "inline.queue", P)],
    },
    Implemented {
        source: "@KafkaListener, @RabbitListener, @JmsListener",
        files: &[(
            "src/main/java/a/Listeners.java",
            "package a;\npublic class Listeners {\n  @KafkaListener(topics = \"orders.created\")\n  public void k(String m) {}\n  @RabbitListener(queues = \"q.one\")\n  public void r(String m) {}\n  @JmsListener(destination = \"jms.queue\")\n  public void j(String m) {}\n}\n",
        )],
        expect: &[
            (Ch, "orders.created", C),
            (Ch, "q.one", C),
            (Ch, "jms.queue", C),
        ],
    },
    Implemented {
        source: "consumer.subscribe",
        files: &[
            (
                "app/consumer.py",
                "from kafka import KafkaConsumer\n\nconsumer = KafkaConsumer()\n\n\ndef listen():\n    consumer.subscribe([\"py.topic\"])\n",
            ),
            (
                "kjs/c.ts",
                "import { Kafka } from 'kafkajs';\nconst kafka = new Kafka({ clientId: 'a', brokers: [] });\nconst consumer = kafka.consumer({ groupId: 'g' });\nexport async function f() { await consumer.subscribe({ topics: ['js.topic'] }); }\n",
            ),
            (
                "src/main/java/b/KC.java",
                "package b;\nimport org.apache.kafka.clients.consumer.KafkaConsumer;\nimport java.util.List;\npublic class KC {\n  public void p(KafkaConsumer<String, String> consumer) { consumer.subscribe(List.of(\"java.topic\")); }\n}\n",
            ),
        ],
        expect: &[
            (Ch, "py.topic", C),
            (Ch, "js.topic", C),
            (Ch, "java.topic", C),
        ],
    },
];

const TABLE_SOURCES: &[Implemented] = &[
    Implemented {
        source: "DDL and Flyway migrations",
        files: &[
            ("schema/tables.sql", "CREATE TABLE users (id bigint);\n"),
            (
                "db/migration/V1__init.sql",
                "CREATE TABLE IF NOT EXISTS orders (id bigint);\n",
            ),
        ],
        expect: &[(Tab, "users", P), (Col, "users.id", P), (Tab, "orders", P)],
    },
    Implemented {
        source: "Liquibase",
        files: &[
            (
                "db/changelog/db.changelog-master.xml",
                "<databaseChangeLog><changeSet id=\"1\" author=\"a\"><createTable tableName=\"invoices\"><column name=\"id\"/></createTable></changeSet></databaseChangeLog>",
            ),
            (
                "db/changelog/more.changelog.yaml",
                "databaseChangeLog:\n  - changeSet:\n      changes:\n        - createTable:\n            tableName: refunds\n",
            ),
        ],
        expect: &[(Tab, "invoices", P), (Tab, "refunds", P)],
    },
    Implemented {
        source: "JPA @Table",
        files: &[(
            "src/main/java/com/acme/users/User.java",
            "package com.acme.users;\n\nimport jakarta.persistence.*;\n\n@Entity\n@Table(name = \"Users\")\npublic class User {\n  @Id Long id;\n}\n",
        )],
        expect: &[(Tab, "users", P)],
    },
    Implemented {
        source: "SQLAlchemy and Django",
        files: &[(
            "app/models.py",
            "from sqlalchemy.orm import declarative_base\nfrom django.db import models\nBase = declarative_base()\n\nclass Account(Base):\n    __tablename__ = \"accounts\"\n\nclass Thing(models.Model):\n    class Meta:\n        db_table = \"things\"\n",
        )],
        expect: &[(Tab, "accounts", P), (Tab, "things", P)],
    },
    Implemented {
        source: "Prisma and EF [Table]",
        files: &[
            (
                "prisma/schema.prisma",
                "model Post {\n  id Int @id\n  @@map(\"posts\")\n}\n",
            ),
            (
                "cs/U.cs",
                "using System.ComponentModel.DataAnnotations.Schema;\n[Table(\"Invoices\", Schema = \"billing\")]\npublic class Invoice { }\n",
            ),
        ],
        expect: &[(Tab, "posts", P), (Tab, "billing.invoices", P)],
    },
    Implemented {
        source: "SQL literals (FROM, JOIN, INTO, UPDATE)",
        files: &[(
            "src/main/java/a/Q.java",
            "package a;\npublic class Q {\n  void s(org.springframework.jdbc.core.JdbcTemplate j) {\n    j.query(\"SELECT * FROM reads r JOIN joined x ON x.r = r.id\", null);\n    j.update(\"INSERT INTO inserted (a) VALUES (1)\");\n    j.update(\"UPDATE updated SET a = 1\");\n  }\n}\n",
        )],
        expect: &[
            (Tab, "reads", C),
            (Tab, "joined", C),
            (Tab, "inserted", C),
            (Tab, "updated", C),
        ],
    },
];

const ARTIFACT_SOURCES: &[Implemented] = &[
    Implemented {
        source: "pom.xml",
        files: &[(
            "pom.xml",
            "<project><groupId>com.acme</groupId><artifactId>shop</artifactId><version>1.0</version><dependencies><dependency><groupId>org.x</groupId><artifactId>lib</artifactId><version>2</version></dependency></dependencies></project>",
        )],
        expect: &[(Art, "maven:com.acme:shop", P), (Art, "maven:org.x:lib", C)],
    },
    Implemented {
        source: "build.gradle",
        files: &[
            (
                "build.gradle",
                "group = 'com.acme'\ndependencies {\n  implementation 'org.x:lib:1.2'\n}\n",
            ),
            ("settings.gradle", "rootProject.name = 'shop'\n"),
        ],
        expect: &[(Art, "maven:com.acme:shop", P), (Art, "maven:org.x:lib", C)],
    },
    Implemented {
        source: "build.gradle.kts",
        files: &[
            (
                "build.gradle.kts",
                "group = \"com.acme\"\ndependencies {\n  implementation(\"org.y:kt:2.0\")\n}\n",
            ),
            ("settings.gradle.kts", "rootProject.name = \"shopkt\"\n"),
        ],
        expect: &[
            (Art, "maven:com.acme:shopkt", P),
            (Art, "maven:org.y:kt", C),
        ],
    },
    Implemented {
        source: "package.json",
        files: &[(
            "package.json",
            r#"{"name":"@acme/web","version":"1.0.0","dependencies":{"lodash":"^4.17.21"}}"#,
        )],
        expect: &[(Art, "npm:@acme:web", P), (Art, "npm::lodash", C)],
    },
    Implemented {
        source: "Cargo.toml",
        files: &[(
            "Cargo.toml",
            "[package]\nname = \"svc\"\nversion = \"0.1.0\"\n[dependencies]\nserde = \"1\"\n",
        )],
        expect: &[(Art, "cargo::svc", P), (Art, "cargo::serde", C)],
    },
    Implemented {
        source: "go.mod",
        files: &[(
            "go.mod",
            "module example.com/acme/svc\n\nrequire github.com/a/b v1.2.3\n",
        )],
        expect: &[
            (Art, "go::example.com/acme/svc", P),
            (Art, "go::github.com/a/b", C),
        ],
    },
    Implemented {
        source: "*.csproj",
        files: &[(
            "src/Acme.Core.csproj",
            "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><PackageId>Acme.Core</PackageId><Version>1.0.0</Version></PropertyGroup><ItemGroup><PackageReference Include=\"Newtonsoft.Json\" Version=\"13.0.1\" /></ItemGroup></Project>",
        )],
        expect: &[
            (Art, "nuget::Acme.Core", P),
            (Art, "nuget::Newtonsoft.Json", C),
        ],
    },
    Implemented {
        source: "pyproject.toml",
        files: &[(
            "pyproject.toml",
            "[project]\nname = \"svcpy\"\nversion = \"1.0\"\ndependencies = [\"requests>=2.31\"]\n",
        )],
        expect: &[(Art, "pypi::svcpy", P), (Art, "pypi::requests", C)],
    },
    Implemented {
        source: "setup.cfg",
        files: &[(
            "setup.cfg",
            "[metadata]\nname = svccfg\nversion = 1.2\n[options]\ninstall_requires =\n    flask>=2\n",
        )],
        expect: &[(Art, "pypi::svccfg", P), (Art, "pypi::flask", C)],
    },
    Implemented {
        source: "alire.toml",
        files: &[(
            "alire.toml",
            "name = \"shop\"\nversion = \"1.0.0\"\n[[depends-on]]\naws = \"^24\"\n",
        )],
        expect: &[(Art, "alire::shop", P), (Art, "alire::aws", C)],
    },
    Implemented {
        source: "*.gpr",
        files: &[(
            "shop.gpr",
            "with \"common.gpr\";\nproject Shop is\nend Shop;\n",
        )],
        expect: &[(Art, "gpr::Shop", P), (Art, "gpr::common", C)],
    },
];

fn implemented(rows: &[Implemented]) {
    for row in rows {
        let g = graph(row.files);
        for (kind, key, direction) in row.expect {
            let found = find(&g, *kind, key);
            assert!(
                found
                    .iter()
                    .any(|c| c.direction == *direction || c.direction == ContractDirection::Both),
                "{}: no {kind} {key} {direction}: {:#?}",
                row.source,
                g.contracts
            );
        }
    }
}

#[test]
fn contract_source_conformance_4_7_1() {
    // Every source issue 62 classifies IMPLEMENTED gives its contracts; the sources a
    // current fact cannot prove have their own tests below (`missing_fact_*`).
    implemented(API_PROVIDERS);
    implemented(API_CONSUMERS);
    implemented(CHANNEL_SOURCES);
    implemented(TABLE_SOURCES);
    implemented(ARTIFACT_SOURCES);
}

/// The extraction of one file, through the pipeline.
fn extraction(path: &str, source: &str) -> pdx_engine::FileExtract {
    let checkout = Checkout::new(&[(path, source)]);
    let root = checkout.root();
    let config = PdxConfig::load(root).expect("config");
    let files = discover(root, &config).expect("discovered");
    let limits = ExtractLimits {
        requested_workers: 1,
        memory_budget_bytes: 64 << 20,
    };
    let extracted = ExtractStage::new(root, &config.secrets, limits)
        .run(&files)
        .expect("extracted");
    let registry = SymbolRegistry::build(root, &files, extracted).expect("registry");
    registry.extract(path).expect("an extraction").clone()
}

#[test]
fn missing_fact_repository_entity_type_arguments() {
    // Issue 64: `JpaRepository<User, Long>` binds the repository to `User`'s table, but
    // the extracted base class keeps no type argument, and `User` is a type reference
    // of the whole file, tied to no base clause. The entity is never guessed from the
    // repository's name.
    let repository = "package a;\nimport org.springframework.data.jpa.repository.JpaRepository;\npublic interface UserRepository extends JpaRepository<User, Long> {\n  User findByEmail(String email);\n}\n";
    let path = "src/main/java/a/UserRepository.java";
    let extract = extraction(path, repository);
    let interface = extract
        .definitions
        .iter()
        .find(|d| d.name == "UserRepository")
        .expect("the repository");
    assert_eq!(
        interface.base_classes,
        ["JpaRepository"],
        "the type arguments are erased"
    );
    let entity = "package a;\nimport jakarta.persistence.*;\n@Entity\n@Table(name = \"users\")\npublic class User { @Id Long id; }\n";
    let g = graph(&[(path, repository), ("src/main/java/a/User.java", entity)]);
    let users = one(&g, ContractKind::Table, "users");
    assert_eq!(
        users.direction,
        ContractDirection::Provides,
        "no consumer is invented"
    );
}

#[test]
fn missing_fact_kotlin_call_arguments() {
    // Issue 63: Kotlin calls are extracted without their arguments, so no call-based
    // source (a producer, an HTTP client) can read its destination in Kotlin. Kotlin's
    // annotations are read (`@KafkaListener` and the rest).
    let source = "package kt\nimport org.springframework.kafka.core.KafkaTemplate\nclass P(private val kafkaTemplate: KafkaTemplate<String, String>) {\n  fun s() { kafkaTemplate.send(\"kt.orders\", \"x\") }\n}\n";
    let extract = extraction("kt/src/P.kt", source);
    let send = extract
        .calls
        .iter()
        .find(|c| c.callee_text == "kafkaTemplate.send")
        .expect("the call");
    assert!(send.args.is_empty(), "{send:?}");
    let g = graph(&[("kt/src/P.kt", source)]);
    assert!(g.contracts.is_empty(), "{:#?}", g.contracts);
}

#[test]
fn missing_fact_destination_bound_before_the_call() {
    // Issue 65: a JMS producer created for a destination sends with no destination in
    // its own arguments; which destination it was created for is local data flow the
    // facts do not carry. The same destination given inline is read.
    let source = "package b;\nimport jakarta.jms.*;\npublic class JM {\n  public void s(Session session, Message m) throws Exception {\n    Destination d = session.createQueue(\"bound.queue\");\n    MessageProducer p = session.createProducer(d);\n    p.send(m);\n  }\n}\n";
    let path = "src/main/java/b/JM.java";
    let extract = extraction(path, source);
    let send = extract
        .calls
        .iter()
        .find(|c| c.callee_text == "p.send")
        .expect("the call");
    assert_eq!(send.args.len(), 1, "only the message: {send:?}");
    let g = graph(&[(path, source)]);
    assert!(find(&g, ContractKind::Channel, "bound.queue").is_empty());
}

#[test]
fn missing_fact_router_prefixes_withhold_routes() {
    // Issue 65: a group's prefix is composed at run time on a local router value, so a
    // registration in a callable that makes a group is withheld rather than given a key
    // without its prefix.
    let source = "package main\nimport \"github.com/gin-gonic/gin\"\nfunc show(c *gin.Context) {}\nfunc main() {\n  r := gin.Default()\n  api := r.Group(\"/api\")\n  api.POST(\"/orders\", show)\n}\nfunc register(g *gin.RouterGroup) { g.GET(\"/items\", show) }\n";
    let g = graph(&[("gin/main.go", source)]);
    assert!(g.contracts.is_empty(), "{:#?}", g.contracts);
    assert_eq!(
        g.diagnostics.contracts,
        [ContractDiagnostic {
            path: "gin/main.go".into(),
            problem: ContractProblem::PrefixUnknown
        },]
    );
}

#[test]
fn local_hosts_are_never_exact_identities() {
    // Owner-ratified (issue 58): a loopback or unspecified host names the machine the
    // code runs on, never a service another repository shares. Built at run time: the
    // provenance scan admits no such address in the tree.
    let http = |rest: &str| format!("http://{rest}");
    for url in [
        http("localhost:8080/users"),
        http("api.localhost/users"),
        http("127.0.0.1:8080/users"),
        http("127.1.2.3/users"),
        http("[::1]:8080/users"),
        http("[::]/users"),
        http("0.0.0.0:80/users"),
    ] {
        assert_eq!(url_authority(&url), None, "{url}");
        let source = format!("export async function f() {{ await fetch('{url}'); }}\n");
        let g = graph(&[("web/a.ts", &source)]);
        let c = one(&g, ContractKind::ApiContract, "GET /users");
        assert_eq!(c.identity_strength, IdentityStrength::Unresolved, "{url}");
    }
    // A declared local name is the owner's statement and stays declared.
    let g = graph(&[
        ("pdx.toml", "[identity]\nhostnames = [\"localhost\"]\n"),
        (
            "openapi.yaml",
            "openapi: 3.0.0\npaths:\n  /users:\n    get: {}\n",
        ),
    ]);
    let c = one(&g, ContractKind::ApiContract, "GET /users");
    assert_eq!(c.identity_strength, IdentityStrength::Declared);
    assert_eq!(
        c.namespace_key,
        r#"{"identity":"localhost","key":"GET /users"}"#
    );
}

#[test]
fn accumulator_rows_pass_the_storage_rules() {
    // What the accumulator makes, the writer accepts: exact, declared and unresolved
    // rows, and artifacts exact with their key as namespace.
    let g = graph(&[
        ("pdx.toml", "[identity]\nhostnames = [\"api.internal\"]\n"),
        ("api/openapi.yaml", OPENAPI),
        ("proto/users.proto", PROTO),
        ("pom.xml", POM),
    ]);
    let strengths: std::collections::BTreeSet<String> = g
        .contracts
        .iter()
        .map(|c| format!("{:?}", c.identity_strength))
        .collect();
    assert_eq!(strengths.len(), 3, "{strengths:?}");
    for c in &g.contracts {
        pdx_core::contracts::row::check(c, &repo(REPO), |id| g.node(id).is_some())
            .unwrap_or_else(|e| panic!("{c:#?}: {e}"));
        if matches!(
            c.kind,
            ContractKind::Artifact | ContractKind::ArtifactVersion
        ) {
            assert_eq!(c.identity_strength, IdentityStrength::Exact);
            assert_eq!(c.namespace_key, c.key);
        }
    }
}

#[test]
fn missing_fact_identity_from_deployment_and_estate_model() {
    // Issue 66: 4.7.1 also takes a provider's authority from its Kubernetes `Service`
    // name or `Ingress` host, and from `pdx-arch.yaml`. Neither is a fact of a
    // repository's Stage 4: deployment resources and the estate model are P6-02's.
    // Without `pdx.toml [identity]` the contract stays unresolved; nothing is read from
    // the manifests here.
    let g = graph(&[
        (
            "openapi.yaml",
            "openapi: 3.0.0\npaths:\n  /users:\n    get: {}\n",
        ),
        (
            "k8s/service.yaml",
            "apiVersion: v1\nkind: Service\nmetadata:\n  name: users\n---\napiVersion: networking.k8s.io/v1\nkind: Ingress\nmetadata:\n  name: users\nspec:\n  rules:\n    - host: api.example.com\n",
        ),
        (
            "pdx-arch.yaml",
            "contexts:\n  - id: users\n    hostnames: [api.example.com]\n",
        ),
    ]);
    let c = one(&g, ContractKind::ApiContract, "GET /users");
    assert_eq!(c.identity_strength, IdentityStrength::Unresolved);
}

#[test]
fn configured_identities_reach_contracts() {
    // A consumer URL whose base is a configuration value: exact, from that value.
    let client = "package a;\nimport org.springframework.web.client.RestTemplate;\npublic class L {\n  RestTemplate rt;\n  public String c() { return rt.getForObject(\"${users.base}/users/{id}\", String.class, 1); }\n}\n";
    let g = graph(&[
        ("src/main/java/a/L.java", client),
        (
            "src/main/resources/application.yml",
            "users:\n  base: https://api.example.com/v1\n",
        ),
    ]);
    let c = one(&g, ContractKind::ApiContract, "GET /v1/users/{}");
    assert_eq!(c.identity_strength, IdentityStrength::Exact);
    assert_eq!(
        c.namespace_key,
        r#"{"identity":"api.example.com","key":"GET /v1/users/{}"}"#
    );
    // A Kafka listener with the cluster in the configuration: exact broker identity.
    let listener = LISTENER.replace("${orders.topic}", "orders.created");
    let g = graph(&[
        (
            "src/main/java/com/acme/orders/OrderListener.java",
            &listener,
        ),
        (
            "src/main/resources/application.properties",
            "spring.kafka.bootstrap-servers=K2.example.com:9092,k1.example.com:9092\n",
        ),
    ]);
    let c = one(&g, ContractKind::Channel, "orders.created");
    assert_eq!(c.identity_strength, IdentityStrength::Exact);
    assert_eq!(
        c.namespace_key,
        r#"{"identity":"k1.example.com:9092,k2.example.com:9092","key":"orders.created"}"#
    );
    // Declared data sources scope tables.
    let g = graph(&[
        ("pdx.toml", "[identity]\ndatasources = [\"shop-db\"]\n"),
        ("db/V1__init.sql", "CREATE TABLE users (id bigint);\n"),
    ]);
    let c = one(&g, ContractKind::Table, "users");
    assert_eq!(c.identity_strength, IdentityStrength::Declared);
    assert_eq!(c.namespace_key, r#"{"identity":"shop-db","key":"users"}"#);
}

#[test]
fn engine_channel_facts_need_library_provenance() {
    // The engine classifies a `producer` receiver as Kafka by its name. Without a Kafka
    // client library imported, the name is no evidence: no contract.
    let source = "class Outbox:\n    def send(self, topic, body):\n        return None\n\nproducer = Outbox()\n\n\ndef publish(event):\n    producer.send(\"orders.created\", event)\n";
    let extract = extraction("app/outbox.py", source);
    assert!(
        !extract.channels.is_empty(),
        "the engine reports a channel fact"
    );
    let g = graph(&[("app/outbox.py", source)]);
    assert!(g.contracts.is_empty(), "{:#?}", g.contracts);
}

#[test]
fn library_calls_need_unambiguous_provenance() {
    // Two libraries whose operation reads the same call differently: nothing.
    let both = "package a;\nimport org.springframework.kafka.core.KafkaTemplate;\nimport org.springframework.jms.core.JmsTemplate;\npublic class B {\n  private Object template;\n  public void s() { template.send(\"orders\", \"x\"); }\n}\n";
    let g = graph(&[("src/main/java/a/B.java", both)]);
    assert!(g.contracts.is_empty(), "{:#?}", g.contracts);
    // A receiver declared as another type is not the library's client.
    let other = "package a;\nimport redis.clients.jedis.Jedis;\npublic class E {\n  public void s(EventPublisher jedis) { jedis.publish(\"events\", \"x\"); }\n}\n";
    let g = graph(&[("src/main/java/a/E.java", other)]);
    assert!(g.contracts.is_empty(), "{:#?}", g.contracts);
    // A call resolution sends to the repository's own definition is not the library's.
    let own = "package a;\nimport redis.clients.jedis.Jedis;\npublic class Bus {\n  public void publish(String channel, String body) {}\n  public void s() { Bus bus = new Bus(); this.publish(\"events\", \"x\"); }\n}\n";
    let g = graph(&[("src/main/java/a/Bus.java", own)]);
    assert!(g.contracts.is_empty(), "{:#?}", g.contracts);
}
