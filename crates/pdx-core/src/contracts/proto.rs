//! RPC contracts from protobuf service definitions (4.7.1).
//!
//! Every `.proto` file is read with a lexer that knows its comments and strings, and a
//! parser for what a contract needs: `package`, and each `service`'s `rpc` methods.
//! Messages, enums, extensions and options are skipped as balanced blocks. A file the
//! parser cannot read is reported and gives nothing: no method is guessed from a
//! broken one. `protoc` is never run.
//!
//! **Providers.** Each method is an `RpcMethod` the file provides, keyed
//! `package.Service/Method` (`Service/Method` with no package).
//!
//! **Consumers.** A call is a stub call only when resolution names its target: a
//! definition of the repository (its declaring type's name), or the engine's external
//! target's qualified name. The type, less a generated stub's suffix (`BlockingStub`,
//! `FutureStub`, `Stub`, `AsyncClient`, `Client`), must be the name of exactly one
//! service the repository's protobuf files define, and the method, as the language's
//! generator spells it (`GetUser`, Java's `getUser`, C#'s `GetUserAsync`), exactly one
//! of its methods. A name alone is never enough.
//!
//! **Namespace.** None is proven here: a `package` is not a global service identity,
//! and the shared-artifact evidence 4.7.2 names is the link job's (P6-01). Every RPC
//! contract is unresolved.

use std::collections::BTreeMap;

use pdx_engine::SiteRef;

use crate::bands::Band;
use crate::index::derive::DeriveError;
use crate::index::derive::calls::{call_site, ordinals};
use crate::kinds::{ContractKind, SiteKind};
use crate::model::ContractDirection;

use super::source::{self, Read};
use super::{Context, ContractObservation, ContractProblem, Found, extension};

/// A protobuf service.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Service {
    /// Its file's package, if it declares one.
    pub package: Option<String>,
    /// Its name.
    pub name: String,
    /// Its methods, in file order, each with its declaration as written.
    pub methods: Vec<(String, String)>,
}

impl Service {
    /// A method's contract key.
    pub fn key(&self, method: &str) -> String {
        match &self.package {
            Some(package) => format!("{package}.{}/{method}", self.name),
            None => format!("{}/{method}", self.name),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Ident(String),
    Str,
    Punct(char),
}

/// A protobuf file's tokens; `None` for an unterminated comment or string, or a
/// character no protobuf token starts with.
fn tokens(text: &str) -> Option<Vec<Token>> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = '\0';
                loop {
                    let c = chars.next()?;
                    if previous == '*' && c == '/' {
                        break;
                    }
                    previous = c;
                }
            }
            '"' | '\'' => {
                loop {
                    match chars.next()? {
                        '\\' => {
                            chars.next()?;
                        }
                        '\n' => return None,
                        q if q == c => break,
                        _ => {}
                    }
                }
                out.push(Token::Str);
            }
            c if c.is_alphanumeric() || c == '_' || c == '.' => {
                let mut ident = String::from(c);
                while let Some(&n) = chars.peek() {
                    if n.is_alphanumeric() || n == '_' || n == '.' {
                        ident.push(n);
                        chars.next();
                    } else {
                        break;
                    }
                }
                out.push(Token::Ident(ident));
            }
            '{' | '}' | '(' | ')' | ';' | '=' | '<' | '>' | ',' | '[' | ']' | ':' | '-' | '+' => {
                out.push(Token::Punct(c));
            }
            _ => return None,
        }
    }
    Some(out)
}

/// The services of a protobuf file; `None` when the file cannot be read.
pub fn parse(text: &str) -> Option<Vec<Service>> {
    let tokens = tokens(text)?;
    let mut package = None;
    let mut services = Vec::new();
    let mut at = 0;
    let ident = |i: usize| match tokens.get(i) {
        Some(Token::Ident(s)) => Some(s.as_str()),
        _ => None,
    };
    let punct = |i: usize, c: char| tokens.get(i) == Some(&Token::Punct(c));
    while at < tokens.len() {
        match ident(at) {
            Some("package") => {
                package = Some(ident(at + 1)?.to_owned());
                if !punct(at + 2, ';') {
                    return None;
                }
                at += 3;
            }
            Some("syntax" | "edition" | "import" | "option") => {
                at = statement_end(&tokens, at)?;
            }
            Some("message" | "enum" | "extend") => {
                at = block_end(&tokens, at)?;
            }
            Some("service") => {
                let name = ident(at + 1)?.to_owned();
                if name.contains('.') || !punct(at + 2, '{') {
                    return None;
                }
                let end = block_end(&tokens, at)?;
                let methods = rpcs(&tokens[at + 3..end - 1])?;
                services.push(Service {
                    package: None,
                    name,
                    methods,
                });
                at = end;
            }
            None if punct(at, ';') => at += 1,
            _ => return None,
        }
    }
    for service in &mut services {
        service.package.clone_from(&package);
    }
    Some(services)
}

/// The index after a statement's `;`.
fn statement_end(tokens: &[Token], from: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (i, token) in tokens.iter().enumerate().skip(from) {
        match token {
            Token::Punct('{' | '[' | '(') => depth += 1,
            Token::Punct('}' | ']' | ')') => depth -= 1,
            Token::Punct(';') if depth == 0 => return Some(i + 1),
            _ => {}
        }
        if depth < 0 {
            return None;
        }
    }
    None
}

/// The index after the block that opens at or after `from`.
fn block_end(tokens: &[Token], from: usize) -> Option<usize> {
    let open = from
        + tokens[from..]
            .iter()
            .position(|t| *t == Token::Punct('{'))?;
    let mut depth = 0usize;
    for (i, token) in tokens.iter().enumerate().skip(open) {
        match token {
            Token::Punct('{') => depth += 1,
            Token::Punct('}') => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// A service body's methods, each with its declaration.
fn rpcs(body: &[Token]) -> Option<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut at = 0;
    let ident = |i: usize| match body.get(i) {
        Some(Token::Ident(s)) => Some(s.as_str()),
        _ => None,
    };
    let punct = |i: usize, c: char| body.get(i) == Some(&Token::Punct(c));
    while at < body.len() {
        match ident(at) {
            Some("rpc") => {
                let name = ident(at + 1)?;
                if name.contains('.') || !punct(at + 2, '(') {
                    return None;
                }
                let mut i = at + 3;
                let (request, after) = message_type(body, i)?;
                i = after;
                if ident(i) != Some("returns") || !punct(i + 1, '(') {
                    return None;
                }
                let (response, after) = message_type(body, i + 2)?;
                i = after;
                if punct(i, ';') {
                    i += 1;
                } else if punct(i, '{') {
                    i = block_end(body, i)?;
                } else {
                    return None;
                }
                out.push((
                    name.to_owned(),
                    format!("rpc {name}({request}) returns ({response})"),
                ));
                at = i;
            }
            Some("option") => at = statement_end(body, at)?,
            None if punct(at, ';') => at += 1,
            _ => return None,
        }
    }
    Some(out)
}

/// `[stream] Type )` at `at`: the type as written, and the index after `)`.
fn message_type(body: &[Token], at: usize) -> Option<(String, usize)> {
    match (body.get(at), body.get(at + 1), body.get(at + 2)) {
        (Some(Token::Ident(s)), Some(Token::Ident(t)), Some(Token::Punct(')')))
            if s == "stream" =>
        {
            Some((format!("stream {t}"), at + 3))
        }
        (Some(Token::Ident(t)), Some(Token::Punct(')')), _) => Some((t.clone(), at + 2)),
        _ => None,
    }
}

pub(crate) fn observe(context: &Context<'_>, found: &mut Found) -> Result<(), DeriveError> {
    let registry = context.registry;
    let mut services: BTreeMap<String, Vec<Service>> = BTreeMap::new();
    for path in registry.files() {
        if extension(path) != Some("proto") {
            continue;
        }
        let text = match source::read(context.root, registry, path)? {
            Read::Text(text) => text,
            Read::NotUtf8 => {
                found.diagnose(path, ContractProblem::NotUtf8);
                continue;
            }
            Read::Unread => continue,
        };
        let Some(parsed) = parse(&text) else {
            found.diagnose(path, ContractProblem::Unparseable { format: "proto" });
            continue;
        };
        let owner = context.graph.file_node(path)?.clone();
        for service in parsed {
            for (method, declaration) in &service.methods {
                found.observe(
                    provider(&service, method, declaration, path).with_owner(owner.clone()),
                );
            }
            services
                .entry(service.name.clone())
                .or_default()
                .push(service);
        }
    }
    consumers(context, &services, found)
}

fn provider(service: &Service, method: &str, declaration: &str, path: &str) -> ContractObservation {
    let observation = ContractObservation::new(
        ContractKind::RpcMethod,
        service.key(method),
        ContractDirection::Provides,
        path,
        declaration,
    );
    facts(observation, service, method)
}

fn facts(
    mut observation: ContractObservation,
    service: &Service,
    method: &str,
) -> ContractObservation {
    if let Some(package) = &service.package {
        observation = observation.with_fact("package", package.clone());
    }
    observation
        .with_fact("service", service.name.clone())
        .with_fact("method", method)
}

/// The generated stub and client suffixes, longest first.
const STUB_SUFFIXES: [&str; 6] = [
    "BlockingStub",
    "FutureStub",
    "AsyncClient",
    "AsyncStub",
    "Client",
    "Stub",
];

/// The service a generated stub type stands for.
fn service_of_stub(type_name: &str) -> Option<&str> {
    STUB_SUFFIXES
        .iter()
        .find_map(|suffix| type_name.strip_suffix(suffix))
        .filter(|s| !s.is_empty())
}

/// Whether a stub method is the protobuf method `rpc`, as generators spell it.
fn spells(stub_method: &str, rpc: &str) -> bool {
    let lower_first = |s: &str| {
        let mut c = s.chars();
        c.next()
            .map(|f| f.to_lowercase().chain(c).collect::<String>())
            .unwrap_or_default()
    };
    stub_method == rpc
        || stub_method == lower_first(rpc)
        || stub_method.strip_suffix("Async") == Some(rpc)
}

/// Calls of generated stubs that resolution ties to exactly one protobuf method.
fn consumers(
    context: &Context<'_>,
    services: &BTreeMap<String, Vec<Service>>,
    found: &mut Found,
) -> Result<(), DeriveError> {
    if services.is_empty() {
        return Ok(());
    }
    let registry = context.registry;
    let ordinals = ordinals(registry);
    for resolved in &context.resolution.resolutions {
        let resolution = &resolved.resolution;
        let (type_name, method) = if let Some(target) = resolution.target() {
            let (Some(definition), Some(owner)) =
                (registry.definition(target), registry.declaring_type(target))
            else {
                continue;
            };
            let Some(owner) = registry.definition(&owner) else {
                continue;
            };
            (owner.name.clone(), definition.name.clone())
        } else if resolution.band() == Band::External
            && let Some(answer) = resolution.engine()
        {
            let mut parts = answer
                .target_qn
                .rsplit(['.', ':'])
                .filter(|p| !p.is_empty());
            let (Some(method), Some(owner)) = (parts.next(), parts.next()) else {
                continue;
            };
            (owner.to_owned(), method.to_owned())
        } else {
            continue;
        };
        let Some(name) = service_of_stub(&type_name) else {
            continue;
        };
        let [service] = services.get(name).map(Vec::as_slice).unwrap_or_default() else {
            continue;
        };
        let matching: Vec<&String> = service
            .methods
            .iter()
            .map(|(m, _)| m)
            .filter(|rpc| spells(&method, rpc))
            .collect();
        let [rpc] = matching.as_slice() else {
            continue;
        };
        let path = resolved.site_ref.rel_path.as_str();
        let mut observation = facts(
            ContractObservation::new(
                ContractKind::RpcMethod,
                service.key(rpc),
                ContractDirection::Consumes,
                path,
                format!("{type_name}.{method}"),
            ),
            service,
            rpc,
        );
        if let Some(call) = registry
            .extract(path)
            .and_then(|e| e.calls.get(resolved.site_ref.call_index as usize))
        {
            let site_ref = SiteRef {
                rel_path: path.to_owned(),
                call_index: resolved.site_ref.call_index,
            };
            if let Ok((site, _)) = call_site(
                context.graph,
                registry,
                &ordinals,
                &site_ref,
                call,
                SiteKind::Call,
            )? && context.graph.sites.contains_key(&site.site_id)
            {
                observation = observation.with_evidence("site_ids", site.site_id.as_str());
            }
        }
        found.observe(observation);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn services_methods_and_comments() {
        let text = "syntax = \"proto3\";\n// rpc Fake(A) returns (B);\npackage acme.users;\nimport \"google/protobuf/empty.proto\";\noption java_package = \"x\";\nmessage GetUserRequest { string id = 1; message Inner { int32 x = 1; } }\nservice UserService {\n  /* rpc Hidden(A) returns (B); */\n  rpc GetUser(GetUserRequest) returns (GetUserResponse);\n  rpc Watch(stream A) returns (stream B) { option deprecated = true; }\n  option (x) = \"y\";\n}\n";
        let services = parse(text).expect("parsed");
        assert_eq!(services.len(), 1);
        let s = &services[0];
        assert_eq!(s.package.as_deref(), Some("acme.users"));
        let names: Vec<&str> = s.methods.iter().map(|(m, _)| m.as_str()).collect();
        assert_eq!(names, ["GetUser", "Watch"]);
        assert_eq!(s.key("GetUser"), "acme.users.UserService/GetUser");
        assert_eq!(s.methods[1].1, "rpc Watch(stream A) returns (stream B)");
    }

    #[test]
    fn broken_files_give_nothing() {
        assert_eq!(parse("service S { rpc A(B) returns (C);\n"), None);
        assert_eq!(parse("service S { rpc A(B returns (C); }"), None);
        assert_eq!(parse("/* never closed"), None);
        assert_eq!(parse("package a; \"unterminated\n"), None);
    }

    #[test]
    fn stubs_and_spellings() {
        assert_eq!(
            service_of_stub("UserServiceBlockingStub"),
            Some("UserService")
        );
        assert_eq!(service_of_stub("UserServiceClient"), Some("UserService"));
        assert_eq!(service_of_stub("Stub"), None);
        assert!(spells("getUser", "GetUser"));
        assert!(spells("GetUserAsync", "GetUser"));
        assert!(!spells("getuser", "GetUser"));
    }
}
