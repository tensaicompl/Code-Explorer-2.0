//! Routes: one `Route` node per route binding, and a `DEFINES_ROUTE` edge from its
//! handler at the binding's evidence site, for Spring, `FastAPI`, Flask and Express.
//!
//! The facts are the engine's: every route binding a definition's annotations or
//! decorators declare (`Definition::routes`), each with the node that declares it, its
//! position and its node-type path (issue 54); and the arguments of an Express
//! registration call. No source is read again. A route's handler is only ever one the
//! facts identify: the annotated or decorated definition, or for Express the definition
//! the handler argument resolves to, through its reference site's resolution or as the
//! one callable of that name defined in the same file. Anything else (an inline
//! function, an expression) gives no route, and is reported.
//!
//! A route's identity is its handler's file as `path` and, as `qualified_name`, compact
//! JSON with the fields `handler` (the handler's qualified name), `method` (in upper
//! case) and `path` (as the facts give it), in that order: no delimiter can be
//! mistaken. Its name is `GET /users`. Every `DEFINES_ROUTE` edge has its evidence site,
//! a `route` site (4.2.4): for a declared route the declaring annotation or decorator,
//! in the handler, keyed by 4.2.1's fingerprint of the fact's node-type path, its
//! callee as resolution splits it and its ordinal among the handler's declaring nodes;
//! for Express the registration call. One annotation that binds several methods or
//! paths is one site with an edge to each route. A binding with no position or path in
//! the source gives no route and no edge, and is reported. A handler is always an entry
//! point.

use std::collections::{BTreeMap, BTreeSet};

use pdx_engine::{Call, DefinitionKind, RouteFact, SiteRef};
use serde::Serialize;
use serde_json::Value;

use crate::bands::Band;
use crate::ids::{NodeKey, RepoId, SiteKey, ast_fingerprint};
use crate::kinds::{EdgeKind, NodeKind, SiteKind};
use crate::model::{Edge, Node, Site};
use crate::resolve::registry::{DefinitionRef, NameProvenance, SymbolRegistry};
use crate::resolve::stages::{ResolvedSite, split_callee};

use super::calls::{call_site, model_span, ordinals};
use super::{Builder, DeriveError, DeriveInput, Missing, RouteDiagnostic, RouteProblem};

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
    /// What evidences it in the source.
    evidence: Evidence,
    band: Band,
}

/// A route binding's evidence.
enum Evidence {
    /// The annotation or decorator that declares it: the handler's route fact at this
    /// index.
    Declared(usize),
    /// The Express registration call.
    Registration(SiteRef, Call),
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

/// The framework a declared route is in, from its language, its callee and its file's
/// imports.
fn framework(registry: &SymbolRegistry, language: &str, path: &str, callee: &str) -> &'static str {
    let (_, name) = split_callee(callee);
    match language {
        "java" | "kotlin" | "scala" if name.ends_with("Mapping") => "spring",
        "java" | "kotlin" | "scala" => "jax-rs",
        "python" if name == "action" => "django-rest-framework",
        "python" if imports_module(registry, path, "fastapi") => "fastapi",
        "python" if imports_module(registry, path, "flask") => "flask",
        _ => "decorator",
    }
}

/// Routes a definition's own decorators or annotations bind it to.
fn declared(registry: &SymbolRegistry, out: &mut Vec<Binding>) {
    for path in registry.files() {
        let (Some(language), Some(extract)) = (registry.language(path), registry.extract(path))
        else {
            continue;
        };
        for (index, definition) in extract.definitions.iter().enumerate() {
            let Ok(index) = u32::try_from(index) else {
                continue;
            };
            for (k, fact) in definition.routes.iter().enumerate() {
                out.push(Binding {
                    handler: DefinitionRef {
                        path: path.to_owned(),
                        index,
                    },
                    method: fact.method.to_uppercase(),
                    route: fact.path.clone(),
                    framework: framework(registry, language.id, path, &fact.callee_text),
                    evidence: Evidence::Declared(k),
                    band: Band::Exact,
                });
            }
        }
    }
}

/// Each declaring node's ordinal among a handler's: its route facts' nodes, by
/// position, grouped by node-type path and callee as resolution splits it (4.2.1). The
/// facts one node declares share it.
fn declared_ordinals(facts: &[RouteFact]) -> Vec<Option<u32>> {
    type Group<'a> = (&'a [String], &'a str, &'a str);
    let mut nodes: BTreeMap<Group<'_>, BTreeSet<(u32, u32)>> = BTreeMap::new();
    for fact in facts {
        let (Some(span), false) = (fact.span, fact.ast_path.is_empty()) else {
            continue;
        };
        let (receiver, name) = split_callee(&fact.callee_text);
        nodes
            .entry((fact.ast_path.as_slice(), name, receiver.unwrap_or("")))
            .or_default()
            .insert((span.start_byte, span.end_byte));
    }
    facts
        .iter()
        .map(|fact| {
            let span = fact.span?;
            let (receiver, name) = split_callee(&fact.callee_text);
            let group = nodes.get(&(fact.ast_path.as_slice(), name, receiver.unwrap_or("")))?;
            let position = group
                .iter()
                .position(|&s| s == (span.start_byte, span.end_byte))?;
            u32::try_from(position + 1).ok()
        })
        .collect()
}

/// The `route` site of a handler's declared route, or what it lacks to be one.
fn declared_site(
    graph: &Builder,
    handler: &DefinitionRef,
    fact: &RouteFact,
    ordinal: Option<u32>,
) -> Result<Result<Site, Missing>, DeriveError> {
    let Some(span) = fact.span else {
        return Ok(Err(Missing::Position));
    };
    let (Some(ordinal), false) = (ordinal, fact.ast_path.is_empty()) else {
        return Ok(Err(Missing::AstPath));
    };
    let (receiver, name) = split_callee(&fact.callee_text);
    let node_types: Vec<&str> = fact.ast_path.iter().map(String::as_str).collect();
    let fingerprint = ast_fingerprint(&node_types, name, receiver.unwrap_or(""), ordinal)?;
    let enclosing = graph.definition_node(handler)?.clone();
    let key = SiteKey::new(&handler.path, Some(enclosing), SiteKind::Route, fingerprint);
    let file = graph.file_node(&handler.path)?.clone();
    Ok(Ok(
        Site::new(&key, file, model_span(span))?.with_texts(Some(name), receiver)
    ))
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
                    evidence: Evidence::Registration(site_ref, call.clone()),
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

/// Adds every route the facts establish with its evidence site: its `Route` node, the
/// site and the `DEFINES_ROUTE` edge, and marks its handler an entry point. A binding
/// with no site in the source adds nothing and is reported.
pub(crate) fn build(graph: &mut Builder, input: &DeriveInput<'_>) -> Result<(), DeriveError> {
    let registry = input.registry;
    let resolved: BTreeMap<&SiteRef, &ResolvedSite> = input
        .resolution
        .resolutions
        .iter()
        .map(|r| (&r.site_ref, r))
        .collect();
    let mut bindings = Vec::new();
    declared(registry, &mut bindings);
    express(
        registry,
        &resolved,
        &mut bindings,
        &mut graph.diagnostics.routes,
    );
    let call_ordinals = ordinals(registry);
    let mut fact_ordinals: BTreeMap<DefinitionRef, Vec<Option<u32>>> = BTreeMap::new();
    for binding in bindings {
        let handler = registry
            .definition(&binding.handler)
            .ok_or(DeriveError::Missing {
                path: binding.handler.path.clone(),
                detail: "a route handler the registry does not have",
            })?;
        let handler_path = binding.handler.path.as_str();
        let site = match &binding.evidence {
            Evidence::Declared(k) => {
                let ordinals = fact_ordinals
                    .entry(binding.handler.clone())
                    .or_insert_with(|| declared_ordinals(&handler.routes));
                let ordinal = ordinals.get(*k).copied().flatten();
                declared_site(graph, &binding.handler, &handler.routes[*k], ordinal)?
            }
            Evidence::Registration(site_ref, call) => call_site(
                graph,
                registry,
                &call_ordinals,
                site_ref,
                call,
                SiteKind::Route,
            )?
            .map(|(site, _)| site),
        };
        let Ok(site) = site else {
            graph.diagnostics.routes.push(RouteDiagnostic {
                path: handler_path.to_owned(),
                method: binding.method.clone(),
                route: binding.route.clone(),
                problem: RouteProblem::NoSite,
            });
            continue;
        };
        let handler_node = graph.definition_node(&binding.handler)?.clone();
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
        let site_id = site.site_id.clone();
        graph.add_site(site)?;
        graph.add_edge(Edge::new(
            handler_node.clone(),
            route_node,
            EdgeKind::DefinesRoute,
            binding.band,
            Some(site_id),
        ))?;
        graph.set_prop(&handler_node, "is_entry_point", Value::from(true));
    }
    Ok(())
}
