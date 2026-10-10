//! API contracts from code (4.7.1): the routes a repository serves and the HTTP calls
//! it makes.
//!
//! **Providers.** Every `Route` node P2-07 derived is an `ApiContract` it owns: the key
//! is the route's method and path normalised ([`super::identity::api_key`]), read from
//! the node, never from the source again, so a route has one identity whatever
//! framework declared it. The route's site is its `DEFINES_ROUTE` edge's. A route's
//! namespace is the repository's declared `hostnames`; with none it is unresolved.
//!
//! **Consumers.** An HTTP client call is an `ApiContract` consumed when its method is
//! determined and its URL is a literal (or the engine followed a constant to one, or a
//! leading `${KEY}` has one configuration value): `fetch` (`GET` unless its options
//! literally name a method), `axios`, `requests` and `httpx` bound by their imports,
//! Spring's `RestTemplate` and .NET's `HttpClient` methods in a file importing them,
//! `reqwest`, and Go's `net/http` (`http.NewRequest` with a literal or `http.Method…`
//! method, `http.Get`, `http.Head`, `http.Post`, `http.PostForm`). An absolute URL's
//! host is its `exact` identity; a path alone is unresolved. A computed method or URL
//! gives nothing.

use std::collections::{BTreeMap, BTreeSet};

use pdx_engine::{Call, CallArg, SiteRef};

use crate::index::derive::DeriveError;
use crate::index::derive::calls::{call_site, ordinals};
use crate::index::derive::routes::imports_module;
use crate::kinds::{ContractKind, EdgeKind, NodeKind, SiteKind};
use crate::model::ContractDirection;
use crate::resolve::registry::SymbolRegistry;
use crate::resolve::stages::split_callee;

use super::annotation::{split_top, string_literal};
use super::config_values::Resolved;
use super::identity::{api_key, http_method, url_authority};
use super::{Context, ContractIdentity, ContractObservation, Found};

pub(crate) fn observe(context: &Context<'_>, found: &mut Found) -> Result<(), DeriveError> {
    providers(context, found);
    consumers(context, found)
}

/// Every `Route` node as a provided contract.
fn providers(context: &Context<'_>, found: &mut Found) {
    let graph = context.graph;
    let mut sites: BTreeMap<_, BTreeSet<String>> = BTreeMap::new();
    for edge in graph.edges.values() {
        if edge.kind == EdgeKind::DefinesRoute
            && let Some(site) = &edge.site_id
        {
            sites
                .entry(edge.dst.clone())
                .or_default()
                .insert(site.as_str().to_owned());
        }
    }
    let files: BTreeMap<_, &String> = graph.file_nodes.iter().map(|(p, id)| (id, p)).collect();
    for node in graph.nodes.values() {
        if node.kind != NodeKind::Route {
            continue;
        }
        let text = |name: &str| node.props.get(name).and_then(serde_json::Value::as_str);
        let (Some(method), Some(path)) = (text("method"), text("path")) else {
            continue;
        };
        let Some(key) = api_key(method, path) else {
            continue;
        };
        let Some(file) = node.file_id.as_ref().and_then(|id| files.get(id)) else {
            continue;
        };
        let identities = context
            .declared
            .hostnames
            .iter()
            .cloned()
            .map(ContractIdentity::Declared)
            .collect();
        let mut observation = ContractObservation::new(
            ContractKind::ApiContract,
            key,
            ContractDirection::Provides,
            file.as_str(),
            format!("{method} {path}"),
        )
        .with_identities(identities)
        .with_owner(node.node_id.clone())
        .with_evidence("route_node_ids", node.node_id.as_str());
        if let Some(framework) = text("framework") {
            observation = observation.with_evidence("frameworks", framework);
        }
        for site in sites.get(&node.node_id).into_iter().flatten() {
            observation = observation.with_evidence("route_site_ids", site.clone());
        }
        found.observe(observation);
    }
}

/// An HTTP client call understood: its client, method and URL argument.
struct ClientCall<'a> {
    client: &'static str,
    method: String,
    url: &'a CallArg,
}

/// Every HTTP client call whose method and URL are determined, as a consumed contract.
fn consumers(context: &Context<'_>, found: &mut Found) -> Result<(), DeriveError> {
    let registry = context.registry;
    let ordinals = ordinals(registry);
    for path in registry.files() {
        let (Some(language), Some(extract)) = (registry.language(path), registry.extract(path))
        else {
            continue;
        };
        let uses_reqwest = language.id == "rust"
            && (imports_module(registry, path, "reqwest")
                || extract
                    .calls
                    .iter()
                    .any(|c| c.callee_text.starts_with("reqwest::")));
        for (index, call) in extract.calls.iter().enumerate() {
            if call.is_reference || call.typed_only {
                continue;
            }
            let Some(client) = client_call(registry, language.id, path, call, uses_reqwest) else {
                continue;
            };
            let Some(url) = literal(client.url) else {
                continue;
            };
            let url = match context.config.resolve_leading(&url) {
                Resolved::Literal(url) | Resolved::Placeholder { value: url, .. } => url,
                Resolved::Unresolved(_) => continue,
            };
            let absolute = url.starts_with("http://") || url.starts_with("https://");
            if !absolute && !url.starts_with('/') {
                continue;
            }
            // The receiver-less forms of a generic method (`client.get`) need a URL
            // that is one: an absolute HTTP URL.
            if client.client == "reqwest" && !absolute {
                continue;
            }
            let Some(key) = api_key(&client.method, &url) else {
                continue;
            };
            let identities = url_authority(&url)
                .map(ContractIdentity::Exact)
                .into_iter()
                .collect();
            let mut observation = ContractObservation::new(
                ContractKind::ApiContract,
                key,
                ContractDirection::Consumes,
                path,
                format!("{} {} {}", client.client, client.method, display_url(&url)),
            )
            .with_identities(identities)
            .with_evidence("clients", client.client);
            let site_ref = SiteRef {
                rel_path: path.to_owned(),
                call_index: u32::try_from(index).unwrap_or(u32::MAX),
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
            found.observe(observation);
        }
    }
    Ok(())
}

/// A URL as evidence: user information, query and fragment removed, so no credential
/// or token in a URL reaches a contract's props.
fn display_url(url: &str) -> String {
    let end = url.find(['?', '#']).unwrap_or(url.len());
    let url = &url[..end];
    match url.find("://") {
        Some(at) => {
            let (scheme, rest) = (&url[..at + 3], &url[at + 3..]);
            let authority_end = rest.find('/').unwrap_or(rest.len());
            let authority = &rest[..authority_end];
            let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
            format!("{scheme}{host}{}", &rest[authority_end..])
        }
        None => url.to_owned(),
    }
}

/// An argument's string value: the engine's, or one plain string literal as written.
fn literal(arg: &CallArg) -> Option<String> {
    arg.value
        .clone()
        .filter(|v| !v.is_empty())
        .or_else(|| string_literal(&arg.expr))
}

/// The argument at a position.
fn arg(call: &Call, index: u32) -> Option<&CallArg> {
    call.args
        .iter()
        .find(|a| a.index == index && a.keyword.is_none())
}

/// Whether `receiver` is bound in the file by an import of `module`.
fn bound_to(registry: &SymbolRegistry, path: &str, receiver: &str, module: &str) -> bool {
    registry.imports_of(path).iter().any(|i| {
        i.module_text == module
            && (i.bindings.iter().any(|b| b == receiver)
                || i.alias.as_deref() == Some(receiver)
                || (i.alias.is_none() && i.imported_name.as_deref() == Some(receiver)))
    })
}

/// The HTTP method a lower-case client method name stands for.
fn verb(name: &str) -> Option<&'static str> {
    Some(match name {
        "get" => "GET",
        "post" => "POST",
        "put" => "PUT",
        "patch" => "PATCH",
        "delete" => "DELETE",
        "head" => "HEAD",
        "options" => "OPTIONS",
        _ => return None,
    })
}

fn client_call<'a>(
    registry: &SymbolRegistry,
    language: &str,
    path: &str,
    call: &'a Call,
    uses_reqwest: bool,
) -> Option<ClientCall<'a>> {
    let (receiver, name) = split_callee(&call.callee_text);
    let (client, method, url) = match language {
        "javascript" | "typescript" => script_client(registry, path, call, receiver, name)?,
        "python" => python_client(registry, path, call, receiver?, name)?,
        "java" | "kotlin" => jvm_client(registry, path, call, receiver, name)?,
        "csharp" => dotnet_client(registry, path, call, receiver, name)?,
        "rust" if receiver == Some("reqwest") || (uses_reqwest && receiver.is_some()) => {
            ("reqwest", verb(name)?.to_owned(), arg(call, 0))
        }
        "go" => go_client(registry, path, call, receiver, name)?,
        _ => return None,
    };
    Some(ClientCall {
        client,
        method,
        url: url?,
    })
}

/// A client call's parts: its client, method and URL argument.
type Parts<'a> = (&'static str, String, Option<&'a CallArg>);

fn script_client<'a>(
    registry: &SymbolRegistry,
    path: &str,
    call: &'a Call,
    receiver: Option<&str>,
    name: &str,
) -> Option<Parts<'a>> {
    let global = matches!(receiver, None | Some("window" | "globalThis" | "self"));
    if name == "fetch" && global {
        return Some(("fetch", fetch_method(call)?, arg(call, 0)));
    }
    if receiver == Some("axios") && bound_to(registry, path, "axios", "axios") {
        return Some(("axios", verb(name)?.to_owned(), arg(call, 0)));
    }
    None
}

fn python_client<'a>(
    registry: &SymbolRegistry,
    path: &str,
    call: &'a Call,
    receiver: &str,
    name: &str,
) -> Option<Parts<'a>> {
    let client = match receiver {
        "requests" => "requests",
        "httpx" => "httpx",
        _ => return None,
    };
    if !bound_to(registry, path, receiver, receiver) {
        return None;
    }
    if name == "request" {
        let method = http_method(&literal(arg(call, 0)?)?)?;
        return Some((client, method, arg(call, 1)));
    }
    Some((client, verb(name)?.to_owned(), arg(call, 0)))
}

fn jvm_client<'a>(
    registry: &SymbolRegistry,
    path: &str,
    call: &'a Call,
    receiver: Option<&str>,
    name: &str,
) -> Option<Parts<'a>> {
    let resttemplate = imports_module(
        registry,
        path,
        "org.springframework.web.client.RestTemplate",
    ) || imports_module(registry, path, "org.springframework.web.client");
    if !resttemplate || receiver.is_none() {
        return None;
    }
    let method = match name {
        "getForObject" | "getForEntity" => "GET",
        "postForObject" | "postForEntity" | "postForLocation" => "POST",
        "put" => "PUT",
        "delete" => "DELETE",
        "patchForObject" => "PATCH",
        "headForHeaders" => "HEAD",
        "optionsForAllow" => "OPTIONS",
        "exchange" => {
            let method = arg(call, 1)?.expr.rsplit('.').next()?.trim();
            return Some(("RestTemplate", http_method(method)?, arg(call, 0)));
        }
        _ => return None,
    };
    Some(("RestTemplate", method.to_owned(), arg(call, 0)))
}

fn dotnet_client<'a>(
    registry: &SymbolRegistry,
    path: &str,
    call: &'a Call,
    receiver: Option<&str>,
    name: &str,
) -> Option<Parts<'a>> {
    if receiver.is_none()
        || !(imports_module(registry, path, "System.Net.Http")
            || imports_module(registry, path, "System.Net.Http.Json"))
    {
        return None;
    }
    let method = match name {
        "GetAsync" | "GetStringAsync" | "GetByteArrayAsync" | "GetStreamAsync"
        | "GetFromJsonAsync" => "GET",
        "PostAsync" | "PostAsJsonAsync" => "POST",
        "PutAsync" | "PutAsJsonAsync" => "PUT",
        "PatchAsync" | "PatchAsJsonAsync" => "PATCH",
        "DeleteAsync" | "DeleteFromJsonAsync" => "DELETE",
        _ => return None,
    };
    Some(("HttpClient", method.to_owned(), arg(call, 0)))
}

fn go_client<'a>(
    registry: &SymbolRegistry,
    path: &str,
    call: &'a Call,
    receiver: Option<&str>,
    name: &str,
) -> Option<Parts<'a>> {
    if receiver != Some("http") || !bound_to(registry, path, "http", "net/http") {
        return None;
    }
    Some(match name {
        "NewRequest" => ("net/http", go_method(arg(call, 0)?)?, arg(call, 1)),
        "NewRequestWithContext" => ("net/http", go_method(arg(call, 1)?)?, arg(call, 2)),
        "Get" => ("net/http", "GET".to_owned(), arg(call, 0)),
        "Head" => ("net/http", "HEAD".to_owned(), arg(call, 0)),
        "Post" | "PostForm" => ("net/http", "POST".to_owned(), arg(call, 0)),
        _ => return None,
    })
}

/// A Go request's method: a literal, or one of `net/http`'s `Method…` constants.
fn go_method(arg: &CallArg) -> Option<String> {
    if let Some(value) = literal(arg) {
        return http_method(&value);
    }
    let name = arg.expr.strip_prefix("http.Method")?;
    http_method(name)
}

/// `fetch`'s method: `GET` with no options, the options' literal `method`, `GET` for
/// options that name none; `None` for options that are not an object literal, a
/// computed method, or a spread that could set one.
fn fetch_method(call: &Call) -> Option<String> {
    let Some(options) = arg(call, 1) else {
        return Some("GET".to_owned());
    };
    let inner = options.expr.trim().strip_prefix('{')?.strip_suffix('}')?;
    let mut method = None;
    for part in split_top(inner)? {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if part.starts_with("...") {
            return None;
        }
        let Some((key, value)) = part.split_once(':') else {
            // A shorthand `{ method }`: computed.
            if part.trim_matches(['"', '\'']) == "method" {
                return None;
            }
            continue;
        };
        if key.trim().trim_matches(['"', '\'']) == "method" {
            method = Some(http_method(&string_literal(value)?)?);
        }
    }
    Some(method.unwrap_or_else(|| "GET".to_owned()))
}
