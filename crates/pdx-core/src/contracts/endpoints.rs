//! API contracts the repository provides through framework declarations that Stage 4
//! makes no `Route` node for (4.7.1's provider frameworks beyond P2-07's): `NestJS`,
//! ASP.NET (attribute routing and minimal APIs), Hono, Go's `net/http`, gin, echo,
//! axum and actix-web. No `Route` node is added; the contract is the record.
//!
//! Each is recognised by its provenance (the file imports the framework) and its shape
//! (a decorator or attribute of the framework on a definition, or one of its
//! registration calls with a literal path), as [`super::calls`] describes:
//!
//! | Framework | Declaration | Owner |
//! |---|---|---|
//! | `NestJS` | `@Get`/`@Post`/…/`@All(path?)` on a method of a class with `@Controller(prefix?)` | the method |
//! | ASP.NET | `[HttpGet("t")]`/…/`[AcceptVerbs]`/`[Route("t")]` on an action, joined to the controller's `[Route]`, `[controller]` and `[action]` replaced | the method |
//! | ASP.NET minimal APIs | `MapGet`/`MapPost`/`MapPut`/`MapDelete`/`MapPatch("t", …)` | the file |
//! | Hono | `get`/`post`/`put`/`delete`/`patch`/`options`/`all("/t", …)` | the file |
//! | `net/http` | `http.HandleFunc`/`Handle("[METHOD ]/t", …)`, or a `ServeMux`'s | the file |
//! | gin, echo | `GET`/`POST`/`PUT`/`DELETE`/`PATCH`/`HEAD`/`OPTIONS`/`Any("/t", …)` | the file |
//! | axum | `route("/t", get(h).post(h))` | the file |
//! | actix-web | `#[get("/t")]`/…/`#[route("/t", method = …)]`; `route("/t", web::get().to(h))`, `web::resource("/t").route(…)` | the function, or the file |
//!
//! A prefix composed at run time (a router mounted under a path, a group, a scope, a
//! nest) is not in the facts. A registration made in a callable that composes such a
//! router, or on a parameter declared as a group, is withheld (`PrefixUnknown`); a
//! router mounted from another callable or file cannot be seen, and its registrations
//! keep the path they declare (issue 65).

use std::collections::BTreeSet;

use pdx_engine::{Call, DefinitionKind};

use crate::index::derive::DeriveError;
use crate::kinds::ContractKind;
use crate::model::ContractDirection;
use crate::resolve::registry::{DefinitionRef, SymbolRegistry};
use crate::resolve::stages::split_callee;

use super::annotation::{self, Arg, string_literal};
use super::calls::{
    factory_literal, imports, imports_any, is_internal, literal, parameter_type, positional,
};
use super::identity::{api_key, http_method};
use super::{Context, ContractIdentity, ContractObservation, ContractProblem, Found};

pub(crate) fn observe(context: &Context<'_>, found: &mut Found) -> Result<(), DeriveError> {
    let registry = context.registry;
    for path in registry.files() {
        let Some(language) = registry.language(path) else {
            continue;
        };
        match language.id {
            "typescript" | "javascript" => {
                if imports(registry, path, "@nestjs/common") {
                    nest(context, path, found)?;
                }
                if imports(registry, path, "hono") && !imports(registry, path, "express") {
                    registrations(context, path, "hono", found, hono_call)?;
                }
            }
            "csharp" => {
                if imports(registry, path, "Microsoft.AspNetCore.Mvc") {
                    aspnet(context, path, found)?;
                }
                if minimal_api(registry, path) {
                    registrations(context, path, "aspnet-minimal", found, minimal_call)?;
                }
            }
            "go" => {
                let gin = imports(registry, path, "github.com/gin-gonic/gin");
                let echo = imports(registry, path, "github.com/labstack/echo");
                if gin && !echo {
                    registrations(context, path, "gin", found, upper_call)?;
                } else if echo && !gin {
                    registrations(context, path, "echo", found, upper_call)?;
                }
                if net_http(registry, path) {
                    registrations(context, path, "net/http", found, |registry, path, call| {
                        net_http_call(registry, path, call)
                    })?;
                }
            }
            "rust" => {
                let axum = uses_crate(registry, path, "axum");
                let actix = uses_crate(registry, path, "actix_web");
                if axum && !actix {
                    registrations(context, path, "axum", found, axum_call)?;
                } else if actix && !axum {
                    actix_attributes(context, path, found)?;
                    registrations(context, path, "actix-web", found, actix_call)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// A provided API contract, scoped by the declared host names.
fn provided(
    context: &Context<'_>,
    method: &str,
    route: &str,
    path: &str,
    raw_form: String,
    framework: &'static str,
) -> Option<ContractObservation> {
    let key = api_key(method, route)?;
    let identities = context
        .declared
        .hostnames
        .iter()
        .cloned()
        .map(ContractIdentity::Declared)
        .collect();
    Some(
        ContractObservation::new(
            ContractKind::ApiContract,
            key,
            ContractDirection::Provides,
            path,
            raw_form,
        )
        .with_identities(identities)
        .with_evidence("frameworks", framework),
    )
}

/// Path parts joined with single slashes, under a leading one.
fn join(parts: &[&str]) -> String {
    let inner: Vec<&str> = parts
        .iter()
        .map(|p| p.trim_matches('/'))
        .filter(|p| !p.is_empty())
        .collect();
    format!("/{}", inner.join("/"))
}

/// A Rust file's use of a crate: an import from it, or a call through its path.
fn uses_crate(registry: &SymbolRegistry, path: &str, name: &str) -> bool {
    imports(registry, path, name)
        || registry.extract(path).is_some_and(|e| {
            e.calls
                .iter()
                .any(|c| c.callee_text.starts_with(&format!("{name}::")))
        })
}

/// A registration recognised in a call: its method and path.
type Registration = (String, String);

/// The calls that compose a router under a prefix, and the parameter types of a
/// router known to be one, per framework: a registration on such a router has a path
/// the declaration does not complete.
fn composition(framework: &str) -> (&'static [&'static str], &'static [&'static str]) {
    match framework {
        "gin" => (&["Group"], &["RouterGroup"]),
        "echo" => (&["Group"], &["Group"]),
        "aspnet-minimal" => (&["MapGroup"], &["RouteGroupBuilder"]),
        "hono" => (&["route", "basePath"], &[]),
        "axum" => (&["nest", "nest_service"], &[]),
        "actix-web" => (&["scope"], &["Scope"]),
        _ => (&[], &[]),
    }
}

/// Every registration call of a file a recogniser accepts, as provided contracts the
/// file owns.
fn registrations(
    context: &Context<'_>,
    path: &str,
    framework: &'static str,
    found: &mut Found,
    recognise: impl Fn(&SymbolRegistry, &str, &Call) -> Vec<Registration>,
) -> Result<(), DeriveError> {
    let registry = context.registry;
    let Some(extract) = registry.extract(path) else {
        return Ok(());
    };
    let owner = context.graph.file_node(path)?.clone();
    let (composers, groups) = composition(framework);
    // The callables that compose a prefixed router: which router a registration in
    // them is made on is not in the facts.
    let composing: BTreeSet<Option<u32>> = extract
        .calls
        .iter()
        .filter(|c| composers.contains(&split_callee(&c.callee_text).1))
        .map(|c| c.caller)
        .collect();
    for (index, call) in extract.calls.iter().enumerate() {
        if call.is_reference || is_internal(context, path, index) {
            continue;
        }
        let registrations = recognise(registry, path, call);
        if registrations.is_empty() {
            continue;
        }
        let grouped =
            parameter_type(registry, path, call).is_some_and(|t| groups.contains(&t.as_str()));
        if grouped || composing.contains(&call.caller) {
            found.diagnose(path, ContractProblem::PrefixUnknown);
            continue;
        }
        for (method, route) in registrations {
            let raw = format!("{}({route})", call.callee_text);
            if let Some(o) = provided(context, &method, &route, path, raw, framework) {
                found.observe(o.with_owner(owner.clone()));
            }
        }
    }
    Ok(())
}

/// A literal first argument that is a path (it starts with `/`).
fn path_argument(call: &Call, index: u32) -> Option<String> {
    literal(positional(call, index)?).filter(|p| p.starts_with('/'))
}

fn hono_call(_: &SymbolRegistry, _: &str, call: &Call) -> Vec<Registration> {
    let (receiver, name) = split_callee(&call.callee_text);
    let method = match name {
        "get" | "post" | "put" | "delete" | "patch" | "options" => name.to_ascii_uppercase(),
        "all" => "ANY".to_owned(),
        _ => return Vec::new(),
    };
    match (receiver, path_argument(call, 0)) {
        (Some(_), Some(route)) => vec![(method, route)],
        _ => Vec::new(),
    }
}

/// gin and echo: upper-case method registrations.
fn upper_call(_: &SymbolRegistry, _: &str, call: &Call) -> Vec<Registration> {
    let (receiver, name) = split_callee(&call.callee_text);
    let method = match name {
        "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS" => name.to_owned(),
        "Any" => "ANY".to_owned(),
        _ => return Vec::new(),
    };
    match (receiver, path_argument(call, 0)) {
        (Some(_), Some(route)) => vec![(method, route)],
        _ => Vec::new(),
    }
}

/// Whether a C# file is an ASP.NET minimal-API host: it builds a `WebApplication`, or
/// imports the routing namespaces.
fn minimal_api(registry: &SymbolRegistry, path: &str) -> bool {
    imports_any(
        registry,
        path,
        &[
            "Microsoft.AspNetCore.Builder",
            "Microsoft.AspNetCore.Routing",
            "Microsoft.AspNetCore.Http",
        ],
    ) || registry.extract(path).is_some_and(|e| {
        e.calls.iter().any(|c| {
            matches!(
                c.callee_text.as_str(),
                "WebApplication.CreateBuilder"
                    | "WebApplication.Create"
                    | "WebApplication.CreateSlimBuilder"
            )
        })
    })
}

fn minimal_call(_: &SymbolRegistry, _: &str, call: &Call) -> Vec<Registration> {
    let (receiver, name) = split_callee(&call.callee_text);
    let method = match name {
        "MapGet" => "GET",
        "MapPost" => "POST",
        "MapPut" => "PUT",
        "MapDelete" => "DELETE",
        "MapPatch" => "PATCH",
        _ => return Vec::new(),
    };
    match (receiver, path_argument(call, 0)) {
        (Some(_), Some(route)) => vec![(method.to_owned(), route)],
        _ => Vec::new(),
    }
}

/// Routers other than the standard library's, whose `HandleFunc` is theirs.
const GO_ROUTERS: [&str; 3] = [
    "github.com/gorilla/mux",
    "github.com/go-chi/chi",
    "github.com/julienschmidt/httprouter",
];

fn net_http(registry: &SymbolRegistry, path: &str) -> bool {
    imports(registry, path, "net/http") && !imports_any(registry, path, &GO_ROUTERS)
}

fn net_http_call(registry: &SymbolRegistry, path: &str, call: &Call) -> Vec<Registration> {
    let (receiver, name) = split_callee(&call.callee_text);
    if !matches!(name, "HandleFunc" | "Handle") {
        return Vec::new();
    }
    // `http.HandleFunc`, or a mux the file makes with `http.NewServeMux`.
    let standard = receiver == Some("http")
        || (receiver.is_some()
            && registry
                .extract(path)
                .is_some_and(|e| e.calls.iter().any(|c| c.callee_text == "http.NewServeMux")));
    let Some(pattern) = positional(call, 0).and_then(literal).filter(|_| standard) else {
        return Vec::new();
    };
    // Go 1.22 patterns: `[METHOD ]/path`; a host-qualified pattern names an authority
    // the contract cannot use, and is skipped.
    let (method, route) = match pattern.split_once(' ') {
        Some((method, route)) => (http_method(method), route.trim().to_owned()),
        None => (Some("ANY".to_owned()), pattern.clone()),
    };
    match method {
        Some(method) if route.starts_with('/') => vec![(method, route)],
        _ => Vec::new(),
    }
}

/// The HTTP methods an axum method router or an actix `web::<method>()` names.
fn router_methods(expr: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    let mut depth = 0i32;
    let mut start = 0;
    let mut segments = Vec::new();
    for (i, c) in expr.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            '.' if depth == 0 => {
                segments.push(&expr[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    segments.push(&expr[start..]);
    for segment in segments {
        let Some(open) = segment.find('(') else {
            continue;
        };
        let name = segment[..open].trim().rsplit("::").next().unwrap_or("");
        match name {
            "get" | "post" | "put" | "delete" | "patch" | "head" | "options" | "trace" => {
                out.insert(name.to_ascii_uppercase());
            }
            "any" => {
                out.insert("ANY".to_owned());
            }
            _ => {}
        }
    }
    out.into_iter().collect()
}

fn axum_call(_: &SymbolRegistry, _: &str, call: &Call) -> Vec<Registration> {
    let (_, name) = split_callee(&call.callee_text);
    let (Some(route), Some(router)) = (path_argument(call, 0), positional(call, 1)) else {
        return Vec::new();
    };
    if name != "route" {
        return Vec::new();
    }
    router_methods(&router.expr)
        .into_iter()
        .map(|m| (m, route.clone()))
        .collect()
}

fn actix_call(_: &SymbolRegistry, _: &str, call: &Call) -> Vec<Registration> {
    let (receiver, name) = split_callee(&call.callee_text);
    if name != "route" {
        return Vec::new();
    }
    // `route("/t", web::get().to(h))` on an app, scope or config; or
    // `web::resource("/t").route(web::get().to(h))`.
    let (route, handler) =
        if let (Some(route), Some(handler)) = (path_argument(call, 0), positional(call, 1)) {
            (route, handler)
        } else {
            let Some(route) = receiver
                .and_then(|r| factory_literal(r, &["resource"]))
                .filter(|p| p.starts_with('/'))
            else {
                return Vec::new();
            };
            let Some(handler) = positional(call, 0) else {
                return Vec::new();
            };
            (route, handler)
        };
    let head = handler.expr.split(".to(").next().unwrap_or("");
    router_methods(head)
        .into_iter()
        .map(|m| (m, route.clone()))
        .collect()
}

/// actix-web's route attributes on functions.
fn actix_attributes(
    context: &Context<'_>,
    path: &str,
    found: &mut Found,
) -> Result<(), DeriveError> {
    let registry = context.registry;
    let Some(extract) = registry.extract(path) else {
        return Ok(());
    };
    for (index, definition) in extract.definitions.iter().enumerate() {
        if definition.kind != DefinitionKind::Function && definition.kind != DefinitionKind::Method
        {
            continue;
        }
        for decorator in &definition.decorators {
            let text = decorator.trim();
            let Some(inner) = text.strip_prefix("#[").and_then(|t| t.strip_suffix(']')) else {
                continue;
            };
            let Some((name, args)) = annotation::parse(inner) else {
                continue;
            };
            let Some(Arg::Str(route)) = args.first().filter(|(k, _)| k.is_none()).map(|(_, v)| v)
            else {
                continue;
            };
            let methods: Vec<String> = match name.as_str() {
                "get" | "post" | "put" | "delete" | "patch" | "head" | "options" | "trace"
                | "connect" => vec![name.to_ascii_uppercase()],
                "route" => args
                    .iter()
                    .filter(|(k, _)| k.as_deref() == Some("method"))
                    .filter_map(|(_, v)| match v {
                        Arg::Str(m) => http_method(m),
                        _ => None,
                    })
                    .collect(),
                _ => continue,
            };
            let r = DefinitionRef {
                path: path.to_owned(),
                index: u32::try_from(index).unwrap_or(u32::MAX),
            };
            let owner = context.graph.definition_node(&r)?.clone();
            for method in methods {
                if let Some(o) =
                    provided(context, &method, route, path, text.to_owned(), "actix-web")
                {
                    found.observe(o.with_owner(owner.clone()));
                }
            }
        }
    }
    Ok(())
}

/// The string items of an argument: a string, an array of strings; `None` otherwise.
fn strings(arg: &Arg) -> Option<Vec<String>> {
    arg.strings()
        .map(|s| s.into_iter().map(str::to_owned).collect())
}

/// The paths a `NestJS` decorator's first argument names: none (`[""]`), a string, an
/// array, or an options object's `path`.
fn nest_paths(args: &annotation::Args) -> Option<Vec<String>> {
    let Some((key, first)) = args.first() else {
        return Some(vec![String::new()]);
    };
    if key.is_some() {
        return None;
    }
    if let Some(paths) = strings(first) {
        return Some(paths);
    }
    // `{ path: 'x' }` reads as a one-item list of `path: 'x'`.
    if let Arg::List(items) = first {
        for item in items {
            if let Arg::Other(text) = item
                && let Some((k, v)) = text.split_once(':')
                && k.trim() == "path"
            {
                let v = v.trim();
                return string_literal(v).map(|p| vec![p]).or_else(|| {
                    annotation::parse(&format!("x({v})")).and_then(|(_, a)| strings(&a.first()?.1))
                });
            }
        }
        return Some(vec![String::new()]);
    }
    None
}

fn nest(context: &Context<'_>, path: &str, found: &mut Found) -> Result<(), DeriveError> {
    let registry = context.registry;
    let Some(extract) = registry.extract(path) else {
        return Ok(());
    };
    for (index, definition) in extract.definitions.iter().enumerate() {
        let Some(class) = definition
            .parent
            .and_then(|p| extract.definitions.get(p as usize))
        else {
            continue;
        };
        let Some(prefixes) = class
            .decorators
            .iter()
            .filter_map(|d| annotation::parse(d))
            .find(|(n, _)| n == "Controller")
            .and_then(|(_, args)| nest_paths(&args))
        else {
            continue;
        };
        for decorator in &definition.decorators {
            let Some((name, args)) = annotation::parse(decorator) else {
                continue;
            };
            let method = match name.as_str() {
                "Get" | "Post" | "Put" | "Delete" | "Patch" | "Options" | "Head" => {
                    name.to_ascii_uppercase()
                }
                "All" => "ANY".to_owned(),
                _ => continue,
            };
            let Some(paths) = nest_paths(&args) else {
                continue;
            };
            let r = DefinitionRef {
                path: path.to_owned(),
                index: u32::try_from(index).unwrap_or(u32::MAX),
            };
            let owner = context.graph.definition_node(&r)?.clone();
            for prefix in &prefixes {
                for route in &paths {
                    let full = join(&[prefix, route]);
                    let raw = format!("{decorator} in @Controller({prefix:?})");
                    if let Some(o) = provided(context, &method, &full, path, raw, "nestjs") {
                        found.observe(o.with_owner(owner.clone()));
                    }
                }
            }
        }
    }
    Ok(())
}

/// The verb attributes of ASP.NET and the methods they name.
fn aspnet_verb(name: &str) -> Option<&'static str> {
    Some(match name {
        "HttpGet" => "GET",
        "HttpPost" => "POST",
        "HttpPut" => "PUT",
        "HttpDelete" => "DELETE",
        "HttpPatch" => "PATCH",
        "HttpHead" => "HEAD",
        "HttpOptions" => "OPTIONS",
        _ => return None,
    })
}

/// A template's first positional string argument, if it has one.
fn template(args: &annotation::Args) -> Option<String> {
    match args.first() {
        Some((None, Arg::Str(t))) => Some(t.clone()),
        _ => match annotation::named(args, &["Template"], false) {
            Some(Arg::Str(t)) => Some(t.clone()),
            _ => None,
        },
    }
}

/// An ASP.NET template with `[controller]` and `[action]` replaced; `None` for any other
/// token, whose value the source does not give.
fn tokens(template: &str, controller: &str, action: &str) -> Option<String> {
    let replaced = template
        .replace("[controller]", controller)
        .replace("[action]", action);
    (!replaced.contains('[')).then_some(replaced)
}

fn aspnet(context: &Context<'_>, path: &str, found: &mut Found) -> Result<(), DeriveError> {
    let registry = context.registry;
    let Some(extract) = registry.extract(path) else {
        return Ok(());
    };
    for (index, definition) in extract.definitions.iter().enumerate() {
        if definition.kind != DefinitionKind::Method {
            continue;
        }
        let Some(class) = definition
            .parent
            .and_then(|p| extract.definitions.get(p as usize))
        else {
            continue;
        };
        let controller = class.name.strip_suffix("Controller").unwrap_or(&class.name);
        let prefixes: Vec<String> = class
            .decorators
            .iter()
            .filter_map(|d| annotation::parse(d))
            .filter(|(n, _)| n == "Route")
            .filter_map(|(_, args)| template(&args))
            .collect();
        let attributes: Vec<(String, annotation::Args)> = definition
            .decorators
            .iter()
            .filter_map(|d| annotation::parse(d))
            .collect();
        let method_routes: Vec<String> = attributes
            .iter()
            .filter(|(n, _)| n == "Route")
            .filter_map(|(_, args)| template(args))
            .collect();
        // (method, template) pairs the attributes declare.
        let mut declared: Vec<(String, Option<String>)> = Vec::new();
        for (name, args) in &attributes {
            if let Some(verb) = aspnet_verb(name) {
                declared.push((verb.to_owned(), template(args)));
            } else if name == "AcceptVerbs" {
                for (key, value) in args {
                    if let (None, Arg::Str(m)) = (key, value)
                        && let Some(m) = http_method(m)
                    {
                        declared.push((m, None));
                    }
                }
            }
        }
        if declared.is_empty() && !method_routes.is_empty() {
            declared.push(("ANY".to_owned(), None));
        }
        let mut routes = BTreeSet::new();
        for (verb, own) in declared {
            let templates: Vec<Option<String>> = match own {
                Some(t) => vec![Some(t)],
                None if method_routes.is_empty() => vec![None],
                None => method_routes.iter().cloned().map(Some).collect(),
            };
            for t in templates {
                let candidates: Vec<String> = match &t {
                    Some(t) if t.starts_with("~/") => vec![join(&[&t[2..]])],
                    Some(t) if t.starts_with('/') => vec![join(&[t])],
                    Some(t) if prefixes.is_empty() => vec![join(&[t])],
                    Some(t) => prefixes.iter().map(|p| join(&[p, t])).collect(),
                    None if prefixes.is_empty() => Vec::new(),
                    None => prefixes.iter().map(|p| join(&[p])).collect(),
                };
                for c in candidates {
                    if let Some(c) = tokens(&c, controller, &definition.name) {
                        routes.insert((verb.clone(), c));
                    }
                }
            }
        }
        if routes.is_empty() {
            continue;
        }
        let r = DefinitionRef {
            path: path.to_owned(),
            index: u32::try_from(index).unwrap_or(u32::MAX),
        };
        let owner = context.graph.definition_node(&r)?.clone();
        let raw = definition.decorators.join(" ");
        for (verb, route) in routes {
            if let Some(o) = provided(context, &verb, &route, path, raw.clone(), "aspnet") {
                found.observe(o.with_owner(owner.clone()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_routers_and_joins() {
        assert_eq!(router_methods("get(show).post(create)"), ["GET", "POST"]);
        assert_eq!(router_methods("routing::get(h).layer(x)"), ["GET"]);
        assert_eq!(router_methods("axum::routing::any(h)"), ["ANY"]);
        assert_eq!(router_methods("web::post()"), ["POST"]);
        assert!(router_methods("handler").is_empty());
        assert_eq!(join(&["/api/", "users"]), "/api/users");
        assert_eq!(join(&["", ""]), "/");
        assert_eq!(
            tokens("api/[controller]/[action]", "Users", "List").as_deref(),
            Some("api/Users/List")
        );
        assert_eq!(tokens("[area]/x", "Users", "List"), None);
    }
}
