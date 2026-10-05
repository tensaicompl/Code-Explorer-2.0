//! Routes: one `Route` node per route binding, and a `DEFINES_ROUTE` edge from its
//! handler, for Spring, `FastAPI` and Express.
//!
//! The facts are the engine's: a definition's route method and path, which it reads
//! from Spring's mapping annotations (already joined to the controller's prefix) and
//! `FastAPI`'s route decorators; and the arguments of an Express registration call. No
//! source is read again. A route's handler is only ever one the facts identify: the
//! decorated definition, or for Express the definition the handler argument resolves
//! to, through its reference site's resolution or as the one callable of that name
//! defined in the same file. Anything else (an inline function, an expression) gives
//! no route, and is reported.
//!
//! A route's identity is its handler's file as `path` and, as `qualified_name`, compact
//! JSON with the fields `handler` (the handler's qualified name), `method` (in upper
//! case) and `path` (as the facts give it), in that order: no delimiter can be
//! mistaken. Its name is `GET /users`. The route's evidence site, where the source has
//! one the engine records (a `FastAPI` decorator call, an Express registration), is a
//! `route` site; a Spring annotation is not one, so its `DEFINES_ROUTE` edge has no
//! site, and is reported. A handler is always an entry point.

use std::collections::BTreeMap;

use pdx_engine::{Call, Definition, DefinitionKind, SiteRef};
use serde::Serialize;
use serde_json::Value;

use crate::bands::Band;
use crate::ids::{NodeKey, RepoId};
use crate::kinds::{EdgeKind, NodeKind, SiteKind};
use crate::model::{Edge, Node};
use crate::resolve::registry::{DefinitionRef, NameProvenance, SymbolRegistry};
use crate::resolve::stages::{ResolvedSite, split_callee};

use super::calls::{call_site, ordinals};
use super::{Builder, DeriveError, DeriveInput, RouteDiagnostic, RouteProblem};

/// The fields of a route's qualified name, in their order.
#[derive(Serialize)]
struct RouteIdentity<'a> {
    handler: &'a str,
    method: &'a str,
    path: &'a str,
}

/// A route's qualified name: `{"handler":…,"method":…,"path":…}`, compact.
pub fn route_qualified_name(handler_qn: &str, method: &str, path: &str) -> String {
    serde_json::to_string(&RouteIdentity {
        handler: handler_qn,
        method,
        path,
    })
    .unwrap_or_default()
}

/// A route's node key: its handler's file, and [`route_qualified_name`].
pub fn route_node_key(
    repo: &RepoId,
    handler_path: &str,
    handler_qn: &str,
    method: &str,
    path: &str,
) -> NodeKey {
    NodeKey::definition(
        repo,
        NodeKind::Route,
        handler_path,
        &route_qualified_name(handler_qn, method, path),
        "",
    )
}

/// One route binding the facts establish.
struct Binding {
    handler: DefinitionRef,
    method: String,
    route: String,
    framework: &'static str,
    /// The call that evidences it, when the source has one.
    evidence: Option<(SiteRef, Call)>,
    band: Band,
}

/// The HTTP methods Express registers by name.
const EXPRESS_METHODS: [&str; 8] = [
    "get", "post", "put", "patch", "delete", "head", "options", "all",
];

/// Whether a file imports a module, or a path beneath it.
pub(crate) fn imports_module(registry: &SymbolRegistry, path: &str, module: &str) -> bool {
    registry.imports_of(path).iter().any(|i| {
        let text = i.module_text.as_str();
        text == module
            || text
                .strip_prefix(module)
                .is_some_and(|rest| rest.starts_with(['.', '/']))
    })
}

fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

/// Routes a definition's own decorators or annotations bind it to.
fn decorated(registry: &SymbolRegistry, out: &mut Vec<Binding>) {
    for path in registry.files() {
        let (Some(language), Some(extract)) = (registry.language(path), registry.extract(path))
        else {
            continue;
        };
        for (index, definition) in extract.definitions.iter().enumerate() {
            let (Some(method), Some(route)) = (&definition.route_method, &definition.route_path)
            else {
                continue;
            };
            let Ok(index) = u32::try_from(index) else {
                continue;
            };
            let framework = match language.id {
                "java" | "kotlin" => "spring",
                "python" => "fastapi",
                _ => "decorator",
            };
            let evidence = if language.id == "python" {
                decorator_call(path, definition, &extract.calls, method, route)
            } else {
                None
            };
            out.push(Binding {
                handler: DefinitionRef {
                    path: path.to_owned(),
                    index,
                },
                method: method.to_uppercase(),
                route: route.clone(),
                framework,
                evidence,
                band: Band::Exact,
            });
        }
    }
}

/// The decorator call that binds a Python handler to its route: the call whose name is
/// the method's, whose first argument is the route, and which is the last such call
/// starting at or before the definition's first line.
fn decorator_call(
    path: &str,
    definition: &Definition,
    calls: &[Call],
    method: &str,
    route: &str,
) -> Option<(SiteRef, Call)> {
    let first_line = definition.span?.start_line;
    let wanted = method.to_lowercase();
    calls
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            let (_, name) = split_callee(&c.callee_text);
            !c.is_reference
                && (name == wanted || name == "route" || name == "api_route")
                && c.span.is_some_and(|s| s.start_line <= first_line)
                && c.args
                    .iter()
                    .find(|a| a.index == 0)
                    .and_then(|a| a.value.as_deref())
                    == Some(route)
        })
        .max_by_key(|(_, c)| c.span.map(|s| s.start_byte))
        .and_then(|(i, c)| {
            Some((
                SiteRef {
                    rel_path: path.to_owned(),
                    call_index: u32::try_from(i).ok()?,
                },
                c.clone(),
            ))
        })
}

/// Express registrations: `<router>.<method>('/path', …, handler)` in a file that
/// imports `express`.
fn express(
    registry: &SymbolRegistry,
    resolved: &BTreeMap<&SiteRef, &ResolvedSite>,
    out: &mut Vec<Binding>,
    diagnostics: &mut Vec<RouteDiagnostic>,
) {
    for path in registry.files() {
        let (Some(language), Some(extract)) = (registry.language(path), registry.extract(path))
        else {
            continue;
        };
        if !matches!(language.id, "javascript" | "typescript")
            || !imports_module(registry, path, "express")
        {
            continue;
        }
        for (index, call) in extract.calls.iter().enumerate() {
            let (receiver, name) = split_callee(&call.callee_text);
            if call.is_reference || receiver.is_none() || !EXPRESS_METHODS.contains(&name) {
                continue;
            }
            let route = call
                .args
                .iter()
                .find(|a| a.index == 0)
                .and_then(|a| a.value.as_deref())
                .filter(|v| v.starts_with('/'));
            let (Some(route), Some(handler_arg)) =
                (route, call.args.iter().max_by_key(|a| a.index))
            else {
                continue;
            };
            if handler_arg.index == 0 {
                continue;
            }
            let Ok(call_index) = u32::try_from(index) else {
                continue;
            };
            let site_ref = SiteRef {
                rel_path: path.to_owned(),
                call_index,
            };
            let handler = is_identifier(&handler_arg.expr)
                .then(|| {
                    express_handler(
                        registry,
                        resolved,
                        path,
                        &extract.calls,
                        call,
                        &handler_arg.expr,
                    )
                })
                .flatten();
            match handler {
                Some((handler, band)) => out.push(Binding {
                    handler,
                    method: name.to_uppercase(),
                    route: route.to_owned(),
                    framework: "express",
                    evidence: Some((site_ref, call.clone())),
                    band,
                }),
                None => diagnostics.push(RouteDiagnostic {
                    path: path.to_owned(),
                    method: name.to_uppercase(),
                    route: route.to_owned(),
                    problem: RouteProblem::NoHandler,
                }),
            }
        }
    }
}

/// The definition an Express handler argument names, and how sure that is: what its
/// reference site resolved to, when that is drawn; else the one callable of that name
/// the file itself defines.
fn express_handler(
    registry: &SymbolRegistry,
    resolved: &BTreeMap<&SiteRef, &ResolvedSite>,
    path: &str,
    calls: &[Call],
    call: &Call,
    name: &str,
) -> Option<(DefinitionRef, Band)> {
    let span = call.span?;
    for (i, c) in calls.iter().enumerate() {
        let inside = c
            .span
            .is_some_and(|s| s.start_byte >= span.start_byte && s.end_byte <= span.end_byte);
        if !(c.is_reference && c.callee_text == name && inside) {
            continue;
        }
        let site_ref = SiteRef {
            rel_path: path.to_owned(),
            call_index: u32::try_from(i).ok()?,
        };
        if let Some(found) = resolved.get(&site_ref)
            && found.resolution.band().is_drawn()
            && let Some(target) = found.resolution.target()
        {
            return Some((target.clone(), found.resolution.band()));
        }
    }
    let NameProvenance::Defined(defined) = registry.name_provenance(path, name) else {
        return None;
    };
    let callables: Vec<&DefinitionRef> = defined
        .iter()
        .filter(|r| {
            registry.definition(r).is_some_and(|d| {
                matches!(d.kind, DefinitionKind::Function | DefinitionKind::Method)
            })
        })
        .collect();
    match callables.as_slice() {
        [one] => Some(((*one).clone(), Band::Scoped)),
        _ => None,
    }
}

/// Adds every route the facts establish, its `DEFINES_ROUTE` edge and evidence site,
/// and marks its handler an entry point.
pub(crate) fn build(graph: &mut Builder, input: &DeriveInput<'_>) -> Result<(), DeriveError> {
    let registry = input.registry;
    let resolved: BTreeMap<&SiteRef, &ResolvedSite> = input
        .resolution
        .resolutions
        .iter()
        .map(|r| (&r.site_ref, r))
        .collect();
    let mut bindings = Vec::new();
    decorated(registry, &mut bindings);
    express(
        registry,
        &resolved,
        &mut bindings,
        &mut graph.diagnostics.routes,
    );
    let ordinals = ordinals(registry);
    for binding in bindings {
        let handler_node = graph.definition_node(&binding.handler)?.clone();
        let handler = registry
            .definition(&binding.handler)
            .ok_or(DeriveError::Missing {
                path: binding.handler.path.clone(),
                detail: "a route handler the registry does not have",
            })?;
        let handler_path = binding.handler.path.as_str();
        let key = route_node_key(
            input.repo,
            handler_path,
            &handler.qualified_name,
            &binding.method,
            &binding.route,
        );
        let mut node = Node::new(&key, &format!("{} {}", binding.method, binding.route))?;
        let file = graph.file_node(handler_path)?.clone();
        node.file_id = Some(file.clone());
        node.parent_id = Some(file);
        node.props
            .insert("framework", Value::from(binding.framework));
        node.props
            .insert("method", Value::from(binding.method.as_str()));
        node.props
            .insert("path", Value::from(binding.route.as_str()));
        node.props
            .insert("handler", Value::from(handler_node.as_str()));
        let route_node = node.node_id.clone();
        graph.add_node(node)?;

        let site_id = match &binding.evidence {
            Some((site_ref, call)) => {
                match call_site(graph, registry, &ordinals, site_ref, call, SiteKind::Route)? {
                    Ok((site, _)) => {
                        let id = site.site_id.clone();
                        graph.add_site(site)?;
                        Some(id)
                    }
                    Err(_) => None,
                }
            }
            None => None,
        };
        if site_id.is_none() {
            graph.diagnostics.routes.push(RouteDiagnostic {
                path: handler_path.to_owned(),
                method: binding.method.clone(),
                route: binding.route.clone(),
                problem: RouteProblem::NoSite,
            });
        }
        graph.add_edge(Edge::new(
            handler_node.clone(),
            route_node,
            EdgeKind::DefinesRoute,
            binding.band,
            site_id,
        ))?;
        graph.set_prop(&handler_node, "is_entry_point", Value::from(true));
    }
    Ok(())
}
