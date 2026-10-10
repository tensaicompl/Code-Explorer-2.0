//! Layer-role assignment (4.8.3): every `Module` and `Class` node gets exactly one
//! [`LayerRole`] in `nodes.layer_role`, `unknown` included; every other node none.
//!
//! The tiers, the first that decides winning and nothing combined:
//!
//! 1. **Configuration**: the repository's `pdx.toml [layers]` rules, in order, the
//!    first whose `match.path_glob` matches winning, a configured `unknown` included.
//!    A class or a definition's module is matched by its file's path; a semantic module
//!    by its member files' paths, sorted, the first rule matching any of them winning.
//!    Globs are `globset`'s, as the configuration was validated with, case-sensitive,
//!    against repository-relative POSIX paths. `pdx-arch.yaml`'s rules are the estate
//!    model's (P6-02), not a repository's.
//! 2. **Framework evidence**, each rule needing its framework's provenance (the name
//!    imported from the framework, or written qualified by it), never a name alone:
//!    [`FRAMEWORK_RULES`], in 4.8.3's order, the first a node satisfies deciding.
//! 3. **Path convention**: a path segment, exactly and case-sensitively, in one of
//!    4.8.3's sets ([`PATH_ROLES`]), tried in their order, then `test` for a test path by
//!    its language's rules (issue 31). A module whose member files give different roles
//!    is `unknown` at this tier.
//! 4. `unknown`.
//!
//! No `HAS_ROLE` or `LAYER_DEPENDS` edge is made: `nodes.layer_role` is the segment's
//! record, and those edges are the estate's (P6-02).

use std::collections::{BTreeMap, BTreeSet};

use globset::{Glob, GlobMatcher};
use pdx_engine::DefinitionKind;
use serde_json::Value;

use crate::config::LayersConfig;
use crate::contracts::annotation;
use crate::ids::NodeId;
use crate::index::derive::tests::is_test_path;
use crate::index::derive::{Builder, DeriveError, DeriveInput};
use crate::kinds::{EdgeKind, LayerRole, NodeKind};
use crate::resolve::registry::{BaseResolution, DefinitionRef, SymbolRegistry};

/// 4.8.3's path segments for each role, in the order the path tier tries them; `test`,
/// which follows them, is a test path by its language's rules.
pub const PATH_ROLES: [(LayerRole, &[&str]); 6] = [
    (
        LayerRole::Api,
        &[
            "controllers",
            "controller",
            "api",
            "rest",
            "web",
            "handlers",
            "routes",
            "endpoints",
        ],
    ),
    (
        LayerRole::Service,
        &[
            "services",
            "service",
            "application",
            "usecases",
            "use_cases",
        ],
    ),
    (
        LayerRole::Domain,
        &["domain", "model", "models", "entities", "core"],
    ),
    (
        LayerRole::Persistence,
        &[
            "repository",
            "repositories",
            "persistence",
            "dao",
            "db",
            "storage",
            "migrations",
        ],
    ),
    (
        LayerRole::Adapter,
        &[
            "adapters",
            "adapter",
            "clients",
            "client",
            "gateway",
            "gateways",
            "infra",
            "infrastructure",
        ],
    ),
    (LayerRole::Port, &["ports", "port"]),
];

/// One of 4.8.3's framework rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameworkRule {
    /// Spring `@RestController` or `@Controller`: `api`.
    SpringController,
    /// Spring `@Service`: `service`.
    SpringService,
    /// Spring `@Repository`: `persistence`.
    SpringRepository,
    /// JPA `@Entity` with repository usage: `persistence`. The usage is a repository
    /// interface's base-class type argument, which the facts do not keep (issue 64),
    /// so it is never proven and the rule never decides; a path rule still may.
    JpaEntityWithRepository,
    /// jMolecules `@Port`: `port`.
    JmoleculesPort,
    /// jMolecules `@Adapter`: `adapter`.
    JmoleculesAdapter,
    /// `NestJS` `@Controller`: `api`.
    NestController,
    /// `NestJS` `@Injectable` in a `*.service.ts` file: `service`.
    NestInjectableService,
    /// A `FastAPI` or Flask route handler: its class, else its module, is `api`.
    PythonRouteHandler,
    /// A `SQLAlchemy` model (P2-08's `__tablename__` table evidence): `persistence`.
    SqlAlchemyModel,
    /// ASP.NET `[ApiController]`: `api`.
    AspNetApiController,
    /// An Entity Framework `DbContext`: `persistence`.
    EntityFrameworkDbContext,
}

/// 4.8.3's framework rules, in its order: when a node satisfies several, the first
/// decides (DECISIONS: the framework tier's tie-break).
pub const FRAMEWORK_RULES: [FrameworkRule; 12] = [
    FrameworkRule::SpringController,
    FrameworkRule::SpringService,
    FrameworkRule::SpringRepository,
    FrameworkRule::JpaEntityWithRepository,
    FrameworkRule::JmoleculesPort,
    FrameworkRule::JmoleculesAdapter,
    FrameworkRule::NestController,
    FrameworkRule::NestInjectableService,
    FrameworkRule::PythonRouteHandler,
    FrameworkRule::SqlAlchemyModel,
    FrameworkRule::AspNetApiController,
    FrameworkRule::EntityFrameworkDbContext,
];

impl FrameworkRule {
    /// The role the rule gives.
    pub const fn role(self) -> LayerRole {
        match self {
            Self::SpringController
            | Self::NestController
            | Self::PythonRouteHandler
            | Self::AspNetApiController => LayerRole::Api,
            Self::SpringService | Self::NestInjectableService => LayerRole::Service,
            Self::SpringRepository
            | Self::JpaEntityWithRepository
            | Self::SqlAlchemyModel
            | Self::EntityFrameworkDbContext => LayerRole::Persistence,
            Self::JmoleculesPort => LayerRole::Port,
            Self::JmoleculesAdapter => LayerRole::Adapter,
        }
    }
}

const SPRING_STEREOTYPE: &str = "org.springframework.stereotype";
const SPRING_WEB: &str = "org.springframework.web.bind.annotation";
const JMOLECULES_HEXAGONAL: &str = "org.jmolecules.architecture.hexagonal";
const NEST_COMMON: &str = "@nestjs/common";
const ASPNET_MVC: &str = "Microsoft.AspNetCore.Mvc";
const ENTITY_FRAMEWORK: &str = "Microsoft.EntityFrameworkCore";

/// A configured rule, compiled once.
struct Configured {
    matcher: GlobMatcher,
    role: LayerRole,
}

/// The repository's configured rules, compiled with the semantics they were validated
/// with.
fn compile(layers: &LayersConfig) -> Result<Vec<Configured>, DeriveError> {
    layers
        .rules
        .iter()
        .map(|rule| {
            Glob::new(&rule.path_glob)
                .map(|glob| Configured {
                    matcher: glob.compile_matcher(),
                    role: rule.role,
                })
                .map_err(|e| DeriveError::LayerRule(format!("{}: {e}", rule.path_glob)))
        })
        .collect()
}

/// The first configured rule, in order, matching any of the paths.
fn configured(rules: &[Configured], paths: &[String]) -> Option<LayerRole> {
    rules
        .iter()
        .find(|rule| paths.iter().any(|p| rule.matcher.is_match(p)))
        .map(|rule| rule.role)
}

/// The path tier's role for one file: the first of [`PATH_ROLES`]' sets one of its
/// segments is exactly in, else `test` for a test path by its language's rules; none
/// when neither.
pub fn path_role(registry: &SymbolRegistry, path: &str) -> Option<LayerRole> {
    let segments: Vec<&str> = path.split('/').collect();
    PATH_ROLES
        .iter()
        .find(|(_, names)| segments.iter().any(|s| names.contains(s)))
        .map(|(role, _)| *role)
        .or_else(|| {
            registry
                .language(path)
                .filter(|language| is_test_path(language, path))
                .map(|_| LayerRole::Test)
        })
}

/// The path tier's role for a node's files: the one role those that give one agree
/// on; `unknown` when they give different ones; none when none gives one.
fn paths_role(registry: &SymbolRegistry, paths: &[String]) -> Option<LayerRole> {
    let roles: BTreeSet<&'static str> = paths
        .iter()
        .filter_map(|p| path_role(registry, p))
        .map(LayerRole::as_str)
        .collect();
    match roles.len() {
        0 => None,
        1 => roles.first().and_then(|r| LayerRole::parse(r)),
        _ => Some(LayerRole::Unknown),
    }
}

/// Assigns every `Module` and `Class` node its role.
///
/// # Errors
///
/// [`DeriveError::LayerRule`] for a configured glob that does not compile (the
/// configuration was validated, so never in practice); [`DeriveError::Missing`] for a
/// module or class whose files are not known.
pub(crate) fn assign(graph: &mut Builder, input: &DeriveInput<'_>) -> Result<(), DeriveError> {
    let registry = input.registry;
    let configured_rules = compile(&input.config.layers)?;
    let framework = framework_roles(graph, registry);
    // A definition's node is matched by its own file; a semantic module's by its
    // members', sorted.
    let mut files: BTreeMap<&NodeId, Vec<String>> = graph
        .definition_nodes
        .iter()
        .filter(|(r, _)| !graph.file_definitions.contains(*r))
        .map(|(r, id)| (id, vec![r.path.clone()]))
        .collect();
    for (key, id) in &graph.module_nodes {
        let members: BTreeSet<String> = registry.files_in_module(key).map(str::to_owned).collect();
        files.insert(id, members.into_iter().collect());
    }
    let mut assigned = BTreeMap::new();
    for node in graph.nodes.values() {
        if !matches!(node.kind, NodeKind::Module | NodeKind::Class) {
            continue;
        }
        let paths = files
            .get(&node.node_id)
            .ok_or_else(|| DeriveError::Missing {
                path: node.qualified_name.clone(),
                detail: "a module or class with no files",
            })?;
        let role = configured(&configured_rules, paths)
            .or_else(|| framework.get(&node.node_id).map(|rule| rule.role()))
            .or_else(|| paths_role(registry, paths))
            .unwrap_or(LayerRole::Unknown);
        assigned.insert(node.node_id.clone(), role);
    }
    for (id, role) in assigned {
        if let Some(node) = graph.nodes.get_mut(&id) {
            node.layer_role = Some(role);
        }
    }
    Ok(())
}

/// The framework tier's decision for each node it decides: the first of
/// [`FRAMEWORK_RULES`] the node satisfies.
fn framework_roles(graph: &Builder, registry: &SymbolRegistry) -> BTreeMap<NodeId, FrameworkRule> {
    let mut satisfied: BTreeMap<NodeId, BTreeSet<usize>> = BTreeMap::new();
    let mut note = |id: &NodeId, rule: FrameworkRule| {
        if let Some(order) = FRAMEWORK_RULES.iter().position(|r| *r == rule) {
            satisfied.entry(id.clone()).or_default().insert(order);
        }
    };
    for (r, id) in &graph.definition_nodes {
        if graph
            .nodes
            .get(id)
            .is_some_and(|n| n.kind == NodeKind::Class)
        {
            for rule in class_rules(registry, r) {
                note(id, rule);
            }
        }
    }
    for id in route_handler_owners(graph) {
        note(&id, FrameworkRule::PythonRouteHandler);
    }
    for id in sqlalchemy_models(graph) {
        note(&id, FrameworkRule::SqlAlchemyModel);
    }
    satisfied
        .into_iter()
        .filter_map(|(id, orders)| Some((id, FRAMEWORK_RULES[*orders.first()?])))
        .collect()
}

/// The rules a class's own annotations and bases satisfy.
fn class_rules(registry: &SymbolRegistry, r: &DefinitionRef) -> Vec<FrameworkRule> {
    let (Some(definition), Some(language)) = (registry.definition(r), registry.language(&r.path))
    else {
        return Vec::new();
    };
    let path = r.path.as_str();
    let has = |name: &str, package: &str| {
        definition
            .decorators
            .iter()
            .any(|d| from_framework(registry, path, d, name, package))
    };
    let mut rules = Vec::new();
    match language.id {
        "java" | "kotlin" => {
            if has("RestController", SPRING_WEB) || has("Controller", SPRING_STEREOTYPE) {
                rules.push(FrameworkRule::SpringController);
            }
            if has("Service", SPRING_STEREOTYPE) {
                rules.push(FrameworkRule::SpringService);
            }
            if has("Repository", SPRING_STEREOTYPE) {
                rules.push(FrameworkRule::SpringRepository);
            }
            if has("Port", JMOLECULES_HEXAGONAL) {
                rules.push(FrameworkRule::JmoleculesPort);
            }
            if has("Adapter", JMOLECULES_HEXAGONAL) {
                rules.push(FrameworkRule::JmoleculesAdapter);
            }
        }
        "typescript" => {
            if has("Controller", NEST_COMMON) {
                rules.push(FrameworkRule::NestController);
            }
            if has("Injectable", NEST_COMMON) && path.ends_with(".service.ts") {
                rules.push(FrameworkRule::NestInjectableService);
            }
        }
        "csharp" => {
            if has("ApiController", ASPNET_MVC) {
                rules.push(FrameworkRule::AspNetApiController);
            }
            if db_context(registry, r) {
                rules.push(FrameworkRule::EntityFrameworkDbContext);
            }
        }
        _ => {}
    }
    rules
}

/// Whether a decorator is the framework's annotation `name` from `package`: written
/// qualified by the package (or by a namespace the file imports it as), or written
/// bare with the file importing that name from the package. A bare name brought in
/// only by a wildcard or a namespace `using` counts unless the repository declares a
/// type of that name itself.
fn from_framework(
    registry: &SymbolRegistry,
    path: &str,
    decorator: &str,
    name: &str,
    package: &str,
) -> bool {
    let Some((found, _)) = annotation::parse(decorator) else {
        return false;
    };
    if found != name {
        return false;
    }
    let written = decorator.trim().trim_start_matches(['@', '[']);
    let head = written.split(['(', ']']).next().unwrap_or(written).trim();
    if let Some((qualifier, _)) = head.rsplit_once('.') {
        return qualifier == package
            || registry
                .imports_of(path)
                .iter()
                .any(|i| i.module_text == package && i.bindings.iter().any(|b| b == qualifier));
    }
    let language = registry.language(path).map_or("", |l| l.id);
    registry.imports_of(path).iter().any(|i| {
        let text = i.module_text.as_str();
        let Some(rest) = text.strip_prefix(package) else {
            return false;
        };
        // `package.Name` (Java, Kotlin) names it exactly.
        if rest.strip_prefix('.') == Some(name) {
            return true;
        }
        // `{ Name } from "package"` (TypeScript) binds it.
        if rest.is_empty() && i.bindings.iter().any(|b| b == name) {
            return true;
        }
        // A wildcard (Java's, recorded as the package; Kotlin's `package.*`) and C#'s
        // namespace `using` bring the package's names in unlisted.
        let unlisted = match language {
            "java" => rest.is_empty() || rest == ".*",
            "kotlin" => rest == ".*",
            "csharp" => rest.is_empty(),
            _ => false,
        };
        unlisted && !declared_type(registry, name)
    })
}

/// Whether the repository declares a type named `name`, which an unlisted import would
/// not shadow.
fn declared_type(registry: &SymbolRegistry, name: &str) -> bool {
    registry.by_name(name).iter().any(|r| {
        registry.definition(r).is_some_and(|d| {
            matches!(
                d.kind,
                DefinitionKind::Class
                    | DefinitionKind::Interface
                    | DefinitionKind::Enum
                    | DefinitionKind::Struct
            )
        })
    })
}

/// Whether a C# class derives from Entity Framework's `DbContext`: a base written
/// `DbContext` that is no type of the repository, with the file using the namespace,
/// or written qualified by it.
fn db_context(registry: &SymbolRegistry, r: &DefinitionRef) -> bool {
    registry.direct_bases(r).iter().any(|base| {
        let raw = base.raw.trim();
        let qualified = raw
            .strip_prefix(ENTITY_FRAMEWORK)
            .is_some_and(|rest| rest == ".DbContext");
        let bare = raw == "DbContext"
            && registry
                .imports_of(&r.path)
                .iter()
                .any(|i| i.module_text == ENTITY_FRAMEWORK);
        let repository = matches!(
            base.resolution,
            BaseResolution::Internal(_) | BaseResolution::Ambiguous(_)
        );
        (qualified || bare) && !repository
    })
}

/// The owners a `FastAPI` or Flask route gives the `api` role: its handler's class, else
/// the handler's semantic module. A handler with neither gives no role: a file has none.
fn route_handler_owners(graph: &Builder) -> BTreeSet<NodeId> {
    let mut owners = BTreeSet::new();
    for edge in graph.edges.values() {
        if edge.kind != EdgeKind::DefinesRoute {
            continue;
        }
        let framework = graph
            .nodes
            .get(&edge.dst)
            .and_then(|route| route.props.get("framework"))
            .and_then(Value::as_str);
        if !matches!(framework, Some("fastapi" | "flask")) {
            continue;
        }
        let Some(handler) = graph.nodes.get(&edge.src) else {
            continue;
        };
        let class = handler
            .parent_id
            .as_ref()
            .and_then(|p| graph.nodes.get(p))
            .filter(|p| p.kind == NodeKind::Class);
        let module = || {
            handler
                .props
                .get("module")
                .and_then(Value::as_str)
                .and_then(|m| NodeId::parse(m).ok())
                .filter(|m| {
                    graph
                        .nodes
                        .get(m)
                        .is_some_and(|n| n.kind == NodeKind::Module)
                })
        };
        if let Some(owner) = class.map(|c| c.node_id.clone()).or_else(module) {
            owners.insert(owner);
        }
    }
    owners
}

/// The classes P2-08 proved `SQLAlchemy` models: the owner of a table contract whose
/// evidence is `SQLAlchemy`'s (a `__tablename__` in a file importing it). A contract
/// whose owner is ambiguous names no class.
fn sqlalchemy_models(graph: &Builder) -> BTreeSet<NodeId> {
    graph
        .contracts
        .iter()
        .filter(|c| {
            c.props
                .get("frameworks")
                .and_then(Value::as_array)
                .is_some_and(|f| f.iter().any(|v| v.as_str() == Some("sqlalchemy")))
        })
        .filter_map(|c| c.owner_node_id.clone())
        .filter(|owner| {
            graph
                .nodes
                .get(owner)
                .is_some_and(|n| n.kind == NodeKind::Class)
        })
        .collect()
}
