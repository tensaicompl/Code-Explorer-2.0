//! Contracts (4.7.1): what a repository provides and consumes across repositories,
//! as rows of the segment's `contracts` table, derived at the end of Stage 4.
//!
//! Each source of evidence is a module that reports what it observed, and nothing
//! else; none of them makes an identity:
//!
//! - [`openapi`]: `OpenAPI` and Swagger operations;
//! - [`routes`]: the `Route` nodes P2-07 derived, and HTTP client calls;
//! - [`proto`]: protobuf services' methods, and calls of their generated stubs;
//! - [`channels`]: the engine's channel facts, listener annotations and `AsyncAPI`;
//! - [`tables`]: DDL, migrations, ORM mappings and SQL literals;
//! - [`artifacts`]: build manifests' coordinates and dependencies.
//!
//! An observation ([`ContractObservation`]) is one sighting: a kind, a normalised key,
//! the identities its namespace may have, a direction, a provider's owner and its raw
//! evidence. One [`ContractAccumulator`] turns them into rows: it alone makes each
//! `namespace_key` and contract id ([`identity`]), merges observations of one contract
//! and sorts the result, so the rows do not depend on the order anything was seen in
//! (issue 59).
//!
//! The rows are the contract record: P2-08 adds no contract node to the graph and no
//! contract edge (`EXPOSES`, `PUBLISHES` and the rest), because the link job (4.7.2)
//! reads the `contracts` table alone and a row already holds its direction, owner and
//! evidence (issue 59).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Value;

use crate::config::PdxConfig;
use crate::ids::{NodeId, RepoId};
use crate::index::derive::{Builder, DeriveError, DeriveInput};
use crate::kinds::ContractKind;
use crate::model::{Contract, ContractDirection, IdentityStrength, Node, Props};
use crate::resolve::registry::SymbolRegistry;
use crate::resolve::stages::ResolveReport;

pub mod annotation;
pub mod artifacts;
pub mod channels;
pub mod config_values;
pub mod document;
pub mod identity;
pub mod openapi;
pub mod proto;
pub mod routes;
pub(crate) mod source;
pub mod tables;

pub use identity::ContractIdentity;

/// One sighting of a contract in the repository's source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractObservation {
    /// Its kind.
    pub kind: ContractKind,
    /// Its normalised key (4.7.1).
    pub key: String,
    /// The identities its namespace has here; none means unresolved. One contract
    /// row is made for each.
    pub identities: Vec<ContractIdentity>,
    /// Which side the repository is on.
    pub direction: ContractDirection,
    /// The node that defines or produces it, for a provider; never a consumer's.
    pub owner: Option<NodeId>,
    /// The repository-relative path of the file that evidences it.
    pub source_path: String,
    /// The evidence as written, normalised for whitespace only.
    pub raw_form: String,
    /// Facts the key itself determines (an artifact's ecosystem, a protobuf method's
    /// package), the same in every observation of the contract.
    pub facts: BTreeMap<&'static str, String>,
    /// Further evidence (a framework, a route's node, a site, an access mode), kept as
    /// a sorted set per name across observations.
    pub evidence: BTreeMap<&'static str, BTreeSet<String>>,
}

impl ContractObservation {
    /// An observation with no identity, owner, facts or evidence yet.
    pub fn new(
        kind: ContractKind,
        key: impl Into<String>,
        direction: ContractDirection,
        source_path: impl Into<String>,
        raw_form: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            key: key.into(),
            identities: Vec::new(),
            direction,
            owner: None,
            source_path: source_path.into(),
            raw_form: raw_form.into(),
            facts: BTreeMap::new(),
            evidence: BTreeMap::new(),
        }
    }

    /// The observation with these identities.
    #[must_use]
    pub fn with_identities(mut self, identities: Vec<ContractIdentity>) -> Self {
        self.identities = identities;
        self
    }

    /// The observation with its provider's owner.
    #[must_use]
    pub fn with_owner(mut self, owner: NodeId) -> Self {
        self.owner = Some(owner);
        self
    }

    /// The observation with a fact the key determines.
    #[must_use]
    pub fn with_fact(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.facts.insert(name, value.into());
        self
    }

    /// The observation with one more piece of evidence.
    #[must_use]
    pub fn with_evidence(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.evidence.entry(name).or_default().insert(value.into());
        self
    }
}

/// Why contract evidence in a file gave no contract. Names the file and the kind of
/// problem, never the file's content.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ContractDiagnostic {
    /// The file.
    pub path: String,
    /// What was wrong.
    pub problem: ContractProblem,
}

/// What kept contract evidence from giving a contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContractProblem {
    /// A recognised document whose syntax its parser cannot read: nothing is guessed
    /// from it.
    Unparseable {
        /// The document's format.
        format: &'static str,
    },
    /// A recognised document that is not UTF-8.
    NotUtf8,
    /// A document nesting deeper than [`crate::consts::CONTRACT_DOCUMENT_MAX_DEPTH`].
    TooDeep,
    /// A file named as a contract document that is not one (an `openapi*.yaml` with
    /// no `openapi` or `swagger` version).
    NotADocument {
        /// The format its name promises.
        format: &'static str,
    },
    /// A `$ref` the reader does not follow: remote, or to another file. Nothing is
    /// fetched.
    UnfollowedRef,
    /// A channel fact no broker client call in its file names: the engine's label for
    /// a socket or an in-process event, not a destination (issue 61).
    UnconfirmedChannel,
}

/// The contracts of one repository, as its observations are merged (issue 59).
///
/// Observations of one contract (the same kind and `namespace_key`, so the same id)
/// are merged whatever order they arrive in:
///
/// - direction: `provides` with `consumes` is `both`, `both` with anything `both`;
/// - identity strength: the strongest, `exact` over `declared` over `unresolved`;
/// - owner: the providers' one owner; with several, none, `props.owner_ambiguous` and
///   the owners in `props.provider_owner_node_ids`; a consumer nominates none;
/// - `props.raw_forms` and `props.source_paths`, and every piece of evidence, sorted
///   sets; a fact the key determines, once.
#[derive(Clone, Debug)]
pub struct ContractAccumulator {
    repo: RepoId,
    merged: BTreeMap<(&'static str, String), Merged>,
}

#[derive(Clone, Debug)]
struct Merged {
    kind: ContractKind,
    key: String,
    namespace_key: String,
    strength: IdentityStrength,
    direction: ContractDirection,
    provider_owners: BTreeSet<NodeId>,
    raw_forms: BTreeSet<String>,
    source_paths: BTreeSet<String>,
    facts: BTreeMap<&'static str, String>,
    evidence: BTreeMap<&'static str, BTreeSet<String>>,
}

/// An identity strength's rank: `exact` highest.
const fn rank(strength: IdentityStrength) -> u8 {
    match strength {
        IdentityStrength::Exact => 2,
        IdentityStrength::Declared => 1,
        IdentityStrength::Unresolved => 0,
    }
}

/// Two directions merged.
pub const fn merge_direction(a: ContractDirection, b: ContractDirection) -> ContractDirection {
    match (a, b) {
        (ContractDirection::Provides, ContractDirection::Provides) => ContractDirection::Provides,
        (ContractDirection::Consumes, ContractDirection::Consumes) => ContractDirection::Consumes,
        _ => ContractDirection::Both,
    }
}

const fn provides(direction: ContractDirection) -> bool {
    matches!(
        direction,
        ContractDirection::Provides | ContractDirection::Both
    )
}

impl ContractAccumulator {
    /// An accumulator for a repository's contracts.
    pub fn new(repo: RepoId) -> Self {
        Self {
            repo,
            merged: BTreeMap::new(),
        }
    }

    /// Adds an observation: one contract per identity it has, or one unresolved.
    ///
    /// # Errors
    ///
    /// [`DeriveError::InvalidContract`] for an empty key, an identity of an artifact
    /// that is not `exact`, or a fact that differs from the one an earlier observation
    /// of the contract gave; [`DeriveError::Id`] for a key no id can hash.
    pub fn observe(&mut self, observation: ContractObservation) -> Result<(), DeriveError> {
        let ContractObservation {
            kind,
            key,
            identities,
            direction,
            owner,
            source_path,
            raw_form,
            facts,
            evidence,
        } = observation;
        if key.is_empty() {
            return Err(DeriveError::InvalidContract(format!(
                "{source_path}: a {kind} with an empty key"
            )));
        }
        let identities = if identities.is_empty() {
            vec![ContractIdentity::Unresolved]
        } else {
            identities
        };
        if identity::is_artifact(kind)
            && identities
                .iter()
                .any(|i| !matches!(i, ContractIdentity::Exact(_)))
        {
            return Err(DeriveError::InvalidContract(format!(
                "{source_path}: an artifact's identity is its coordinate"
            )));
        }
        for identity in identities {
            let namespace_key = identity::namespace_key(kind, &identity, &key, &self.repo);
            identity::contract_id(kind, &namespace_key)?;
            let slot = (kind.as_str(), namespace_key.clone());
            let entry = self.merged.entry(slot).or_insert_with(|| Merged {
                kind,
                key: key.clone(),
                namespace_key,
                strength: identity.strength(),
                direction,
                provider_owners: BTreeSet::new(),
                raw_forms: BTreeSet::new(),
                source_paths: BTreeSet::new(),
                facts: BTreeMap::new(),
                evidence: BTreeMap::new(),
            });
            if rank(identity.strength()) > rank(entry.strength) {
                entry.strength = identity.strength();
            }
            entry.direction = merge_direction(entry.direction, direction);
            if provides(direction)
                && let Some(owner) = &owner
            {
                entry.provider_owners.insert(owner.clone());
            }
            entry.raw_forms.insert(raw_form.clone());
            entry.source_paths.insert(source_path.clone());
            for (name, value) in &facts {
                match entry.facts.get(name) {
                    Some(existing) if existing != value => {
                        return Err(DeriveError::InvalidContract(format!(
                            "{source_path}: two values of `{name}` for one {kind} contract"
                        )));
                    }
                    Some(_) => {}
                    None => {
                        entry.facts.insert(name, value.clone());
                    }
                }
            }
            for (name, values) in &evidence {
                entry
                    .evidence
                    .entry(name)
                    .or_default()
                    .extend(values.iter().cloned());
            }
        }
        Ok(())
    }

    /// The contracts, sorted by id, each validated: its id is 4.2.1's over its
    /// `namespace_key`, its key and `namespace_key` are not empty, and its owners are
    /// nodes of `nodes`.
    ///
    /// # Errors
    ///
    /// [`DeriveError::InvalidContract`] for an owner that is not a node of the graph,
    /// [`DeriveError::Id`] for a key no id can hash.
    pub fn finish(self, nodes: &BTreeMap<NodeId, Node>) -> Result<Vec<Contract>, DeriveError> {
        let mut contracts = Vec::with_capacity(self.merged.len());
        for merged in self.merged.into_values() {
            for owner in &merged.provider_owners {
                if !nodes.contains_key(owner) {
                    return Err(DeriveError::InvalidContract(format!(
                        "a {} contract's owner {owner} is not a node of the graph",
                        merged.kind
                    )));
                }
            }
            let mut props = Props::default();
            let set = |values: &BTreeSet<String>| {
                Value::Array(values.iter().cloned().map(Value::String).collect())
            };
            props.insert("raw_forms", set(&merged.raw_forms));
            props.insert("source_paths", set(&merged.source_paths));
            for (name, value) in &merged.facts {
                props.insert(name, Value::String(value.clone()));
            }
            for (name, values) in &merged.evidence {
                props.insert(name, set(values));
            }
            let owner_node_id = match merged.provider_owners.len() {
                1 => merged.provider_owners.first().cloned(),
                0 => None,
                _ => {
                    props.insert("owner_ambiguous", Value::Bool(true));
                    props.insert(
                        "provider_owner_node_ids",
                        Value::Array(
                            merged
                                .provider_owners
                                .iter()
                                .map(|o| Value::String(o.as_str().to_owned()))
                                .collect(),
                        ),
                    );
                    None
                }
            };
            let contract = Contract {
                contract_id: identity::contract_id(merged.kind, &merged.namespace_key)?,
                kind: merged.kind,
                key: merged.key,
                namespace_key: merged.namespace_key,
                identity_strength: merged.strength,
                owner_node_id,
                direction: merged.direction,
                props,
            };
            validate(&contract)?;
            contracts.push(contract);
        }
        contracts.sort_by(|a, b| a.contract_id.cmp(&b.contract_id));
        Ok(contracts)
    }
}

/// Checks a contract row as stored: its id is 4.2.1's estate id of its kind over its
/// `namespace_key`, its key and `namespace_key` are not empty, and an unresolved
/// identity's `namespace_key`, and only that, is `unresolved:`-prefixed.
///
/// # Errors
///
/// [`DeriveError::InvalidContract`] naming what is wrong.
pub fn validate(contract: &Contract) -> Result<(), DeriveError> {
    let invalid = |why: &str| {
        Err(DeriveError::InvalidContract(format!(
            "{} {}: {why}",
            contract.kind, contract.contract_id
        )))
    };
    if contract.key.is_empty() || contract.namespace_key.is_empty() {
        return invalid("an empty key");
    }
    if identity::contract_id(contract.kind, &contract.namespace_key)? != contract.contract_id {
        return invalid("its id is not the estate id of its namespace_key");
    }
    let unresolved = contract.identity_strength == IdentityStrength::Unresolved;
    if unresolved != contract.namespace_key.starts_with("unresolved:") {
        return invalid("its namespace_key does not match its identity strength");
    }
    Ok(())
}

/// The repository's declared identities (`pdx.toml [identity]`), canonical.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeclaredIdentities {
    /// `hostnames`: API providers' authorities.
    pub hostnames: Vec<String>,
    /// `brokers`: channels' broker.
    pub brokers: Vec<String>,
    /// `datasources`: tables' data source.
    pub datasources: Vec<String>,
}

impl DeclaredIdentities {
    /// The declared identities of a configuration, each canonical, sorted and once.
    pub fn of(config: &PdxConfig) -> Self {
        let canonical = |names: &[String]| -> Vec<String> {
            let set: BTreeSet<String> = names
                .iter()
                .filter_map(|n| identity::declared_name(n))
                .collect();
            set.into_iter().collect()
        };
        config
            .identity
            .as_ref()
            .map_or_else(Self::default, |i| Self {
                hostnames: canonical(&i.hostnames),
                brokers: canonical(&i.brokers),
                datasources: canonical(&i.datasources),
            })
    }

    /// `exact` identities, then these declared ones; none (unresolved) when both are
    /// empty.
    pub fn with_exact(exact: &BTreeSet<String>, declared: &[String]) -> Vec<ContractIdentity> {
        exact
            .iter()
            .cloned()
            .map(ContractIdentity::Exact)
            .chain(declared.iter().cloned().map(ContractIdentity::Declared))
            .collect()
    }
}

/// What every source of evidence reads.
pub(crate) struct Context<'a> {
    pub(crate) root: &'a Path,
    pub(crate) registry: &'a SymbolRegistry,
    pub(crate) resolution: &'a ResolveReport,
    pub(crate) graph: &'a Builder,
    pub(crate) config: config_values::ConfigValues,
    pub(crate) declared: DeclaredIdentities,
}

/// What the sources of evidence found.
#[derive(Default)]
pub(crate) struct Found {
    pub(crate) observations: Vec<ContractObservation>,
    pub(crate) diagnostics: Vec<ContractDiagnostic>,
}

impl Found {
    pub(crate) fn observe(&mut self, observation: ContractObservation) {
        self.observations.push(observation);
    }

    pub(crate) fn diagnose(&mut self, path: &str, problem: ContractProblem) {
        self.diagnostics.push(ContractDiagnostic {
            path: path.to_owned(),
            problem,
        });
    }
}

/// The file name of a repository-relative path.
pub(crate) fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// A path's extension, as written (case-sensitive, as 4.7.1's file families are).
pub(crate) fn extension(path: &str) -> Option<&str> {
    file_name(path).rsplit_once('.').map(|(_, e)| e)
}

/// Derives the repository's contracts into the graph, after every other part of
/// Stage 4 (they read its route, class and file nodes).
///
/// # Errors
///
/// [`DeriveError`] when a file is no longer what Stage 1 and 2 found (the checkout
/// changed, a symlink appeared, a read failed), or a contract would be invalid.
pub(crate) fn build(graph: &mut Builder, input: &DeriveInput<'_>) -> Result<(), DeriveError> {
    let mut found = Found::default();
    let config = config_values::ConfigValues::read(input.root, input.registry, &mut found)?;
    let context = Context {
        root: input.root,
        registry: input.registry,
        resolution: input.resolution,
        graph,
        config,
        declared: DeclaredIdentities::of(input.config),
    };
    openapi::observe(&context, &mut found)?;
    routes::observe(&context, &mut found)?;
    proto::observe(&context, &mut found)?;
    channels::observe(&context, &mut found)?;
    tables::observe(&context, &mut found)?;
    artifacts::observe(&context, &mut found)?;
    let mut accumulator = ContractAccumulator::new(input.repo.clone());
    for observation in found.observations {
        accumulator.observe(observation)?;
    }
    let contracts = accumulator.finish(&graph.nodes)?;
    graph.contracts = contracts;
    graph.diagnostics.contracts.extend(found.diagnostics);
    Ok(())
}
