//! API contracts from `OpenAPI` 3 and Swagger 2 documents (4.7.1).
//!
//! The files are the families 4.7.1 names, by file name: `openapi*.yaml`,
//! `openapi*.yml`, `openapi*.json`, `swagger*.yaml`, `swagger*.json`. A document must
//! declare its version (`openapi: 3…` or `swagger: "2.0"`); one that does not, or that
//! cannot be read, is reported and gives nothing.
//!
//! Each operation (`get`, `put`, `post`, `delete`, `options`, `head`, `patch`, `trace`
//! of a path item) is an `ApiContract` the document's file provides. Its key is the
//! method and the server's path prefix joined to the operation's path, normalised
//! ([`super::identity::api_path`]). Its identities are the literal authorities of its
//! servers, `exact` (`OpenAPI`'s `servers`, at the operation, path or document level,
//! the nearest that has any; Swagger's `host`, with its `schemes`), and the
//! repository's declared `hostnames`, `declared`; with neither it is unresolved. A
//! server URL holding a template variable gives no authority and no prefix. No `$ref`
//! is followed: a path item that is a reference is reported and gives nothing, and
//! nothing is ever fetched.

use std::collections::BTreeSet;

use crate::index::derive::DeriveError;
use crate::kinds::ContractKind;
use crate::model::ContractDirection;

use super::document::{self, Doc, DocError};
use super::identity::{api_key, host_port, url_authority};
use super::source::{self, Read};
use super::{
    Context, ContractObservation, ContractProblem, DeclaredIdentities, Found, extension, file_name,
};

/// The HTTP methods an `OpenAPI` path item holds operations under.
const METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

/// Whether a file is an `OpenAPI` or Swagger document by its name, and which.
pub fn document_kind(path: &str) -> Option<&'static str> {
    let name = file_name(path);
    let extension = extension(path)?;
    if name.starts_with("openapi") && matches!(extension, "yaml" | "yml" | "json") {
        Some("openapi")
    } else if name.starts_with("swagger") && matches!(extension, "yaml" | "json") {
        Some("swagger")
    } else {
        None
    }
}

/// One server an operation is served from: its authority, if literal, and its path.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Server {
    authority: Option<String>,
    prefix: String,
}

pub(crate) fn observe(context: &Context<'_>, found: &mut Found) -> Result<(), DeriveError> {
    let registry = context.registry;
    for path in registry.files() {
        let Some(family) = document_kind(path) else {
            continue;
        };
        let text = match source::read(context.root, registry, path)? {
            Read::Text(text) => text,
            Read::NotUtf8 => {
                found.diagnose(path, ContractProblem::NotUtf8);
                continue;
            }
            Read::Unread => continue,
        };
        let json = extension(path) == Some("json");
        let parsed = if json {
            document::parse_json(&text).map(|d| vec![d])
        } else {
            document::parse_yaml(&text)
        };
        let doc = match parsed {
            Ok(mut docs) if docs.len() == 1 => docs.remove(0),
            Ok(_) | Err(DocError::Syntax) => {
                let format = if json { "json" } else { "yaml" };
                found.diagnose(path, ContractProblem::Unparseable { format });
                continue;
            }
            Err(DocError::TooDeep) => {
                found.diagnose(path, ContractProblem::TooDeep);
                continue;
            }
        };
        let owner = context.graph.file_node(path)?.clone();
        operations(&doc, path, family, &context.declared, found, |o| {
            o.with_owner(owner.clone())
        });
    }
    Ok(())
}

/// Every operation of a document as an observation, finished by `finish`.
fn operations(
    doc: &Doc,
    path: &str,
    family: &'static str,
    declared: &DeclaredIdentities,
    found: &mut Found,
    finish: impl Fn(ContractObservation) -> ContractObservation,
) {
    let version = doc.get("openapi").or_else(|| doc.get("swagger"));
    let swagger = doc.get("swagger").is_some();
    if version.and_then(Doc::as_str).is_none() {
        found.diagnose(path, ContractProblem::NotADocument { format: family });
        return;
    }
    let root_servers = if swagger {
        Some(swagger_servers(doc))
    } else {
        servers(doc.get("servers"))
    };
    let Some(paths) = doc.get("paths") else {
        return;
    };
    for (template, item) in paths.entries() {
        if item.get("$ref").is_some() {
            found.diagnose(path, ContractProblem::UnfollowedRef);
            continue;
        }
        let item_servers = if swagger {
            None
        } else {
            servers(item.get("servers"))
        };
        for (method, operation) in item.entries() {
            if !METHODS.contains(&method) {
                continue;
            }
            let operation_servers = if swagger {
                None
            } else {
                servers(operation.get("servers"))
            };
            let chosen = operation_servers
                .as_ref()
                .or(item_servers.as_ref())
                .or(root_servers.as_ref());
            let default = vec![Server {
                authority: None,
                prefix: String::new(),
            }];
            for server in chosen.unwrap_or(&default) {
                let Some(key) = api_key(method, &format!("{}{template}", server.prefix)) else {
                    continue;
                };
                let exact: BTreeSet<String> = server.authority.iter().cloned().collect();
                let observation = ContractObservation::new(
                    ContractKind::ApiContract,
                    key,
                    ContractDirection::Provides,
                    path,
                    format!("{method} {template}"),
                )
                .with_identities(DeclaredIdentities::with_exact(&exact, &declared.hostnames))
                .with_evidence("frameworks", family);
                found.observe(finish(observation));
            }
        }
    }
}

/// An `OpenAPI` `servers` list, read: `None` when there is none, so a nearer level's
/// absence defers to the next.
fn servers(list: Option<&Doc>) -> Option<Vec<Server>> {
    let items = list?.items();
    if items.is_empty() {
        return None;
    }
    let mut out = BTreeSet::new();
    for item in items {
        let Some(url) = item.get("url").and_then(Doc::as_str) else {
            continue;
        };
        if url.contains('{') {
            // A templated server names no literal authority or prefix.
            continue;
        }
        let (authority, path) = match url.find("://") {
            Some(at) => {
                let after = &url[at + 3..];
                let end = after.find(['/', '?', '#']).unwrap_or(after.len());
                (url_authority(url), after[end..].to_owned())
            }
            None => (None, url.to_owned()),
        };
        let prefix = path
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .trim_end_matches('/');
        out.insert(Server {
            authority,
            prefix: prefix.to_owned(),
        });
    }
    (!out.is_empty()).then(|| out.into_iter().collect())
}

/// Swagger 2's one server: `host` (with each of `schemes`, for its default port) and
/// `basePath`.
fn swagger_servers(doc: &Doc) -> Vec<Server> {
    let prefix = doc
        .get("basePath")
        .and_then(Doc::as_str)
        .unwrap_or("")
        .trim_end_matches('/')
        .to_owned();
    let host = doc.get("host").and_then(Doc::as_str);
    let schemes: Vec<&str> = doc
        .get("schemes")
        .map(Doc::items)
        .unwrap_or_default()
        .iter()
        .filter_map(Doc::as_str)
        .collect();
    let mut out = BTreeSet::new();
    match host {
        Some(host) if schemes.is_empty() => {
            out.insert(Server {
                authority: host_port(host, None),
                prefix: prefix.clone(),
            });
        }
        Some(host) => {
            for scheme in schemes {
                out.insert(Server {
                    authority: host_port(host, Some(&scheme.to_ascii_lowercase())),
                    prefix: prefix.clone(),
                });
            }
        }
        None => {
            out.insert(Server {
                authority: None,
                prefix,
            });
        }
    }
    out.into_iter().collect()
}
