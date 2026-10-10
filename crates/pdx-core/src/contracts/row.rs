//! The rules every stored contract row obeys (4.7.1, issues 58 and 59), checked by one
//! pure function both where rows are made (the accumulator) and where they are stored
//! (the segment writer), so a row assembled by hand cannot be more than the format
//! allows.
//!
//! - `key` and `namespace_key` are not empty.
//! - An `Artifact` or `ArtifactVersion` is `exact` and its `namespace_key` is its key.
//! - Any other kind, `unresolved`: `namespace_key` is exactly
//!   `unresolved:<the segment's repo_id>:<key>`.
//! - Any other kind, `exact` or `declared`: `namespace_key` is exactly the compact JSON
//!   `{"identity":…,"key":…}` the namespace helper writes, with a non-empty identity and
//!   the row's own key; an equivalent but differently written object is refused,
//!   because the id is a hash of these bytes.
//! - `contract_id` is 4.2.1's estate id of the kind over `namespace_key`.
//! - `owner_node_id`, when there is one, names a node of the repository.
//! - `owner_ambiguous`, when present, is `true`, the row has no owner, and
//!   `provider_owner_node_ids` lists at least two distinct nodes of the repository,
//!   sorted; the list never appears without the flag.

use std::fmt;

use serde_json::Value;

use crate::ids::{IdError, NodeId, RepoId};
use crate::model::{Contract, IdentityStrength};

use super::identity::{self, ContractIdentity};

/// Why a contract row is not one the format allows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContractRowError {
    /// `key` or `namespace_key` is empty.
    EmptyKey,
    /// An artifact whose identity is not `exact`.
    ArtifactNotExact,
    /// An artifact whose `namespace_key` is not its key.
    ArtifactNamespaceNotKey,
    /// An unresolved identity whose `namespace_key` is not the repository-scoped one.
    UnresolvedNamespace {
        /// What it must be.
        expected: String,
    },
    /// An established identity whose `namespace_key` is not a JSON object of exactly
    /// the members `identity` and `key`, both strings.
    ResolvedNamespaceShape,
    /// An established identity that is empty.
    EmptyIdentity,
    /// An established identity whose `namespace_key` embeds another key than the row's.
    EmbeddedKeyDiffers,
    /// An established identity whose `namespace_key` is not written exactly as the
    /// namespace helper writes it (member order, whitespace, escaping).
    NonCanonicalNamespace,
    /// The id could not be computed.
    Id(IdError),
    /// `contract_id` is not the estate id of the kind over `namespace_key`.
    WrongId {
        /// The id it must be.
        expected: NodeId,
    },
    /// The owner is not a node of the repository.
    UnknownOwner(NodeId),
    /// `owner_ambiguous` is present but not `true`.
    AmbiguityFlag,
    /// `owner_ambiguous` with an owner.
    AmbiguousWithOwner,
    /// `provider_owner_node_ids` missing, not at least two distinct sorted nodes of the
    /// repository, or present without `owner_ambiguous`.
    ProviderOwners,
}

impl fmt::Display for ContractRowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyKey => f.write_str("an empty key or namespace_key"),
            Self::ArtifactNotExact => f.write_str("an artifact's identity is not exact"),
            Self::ArtifactNamespaceNotKey => {
                f.write_str("an artifact's namespace_key is not its key")
            }
            Self::UnresolvedNamespace { expected } => {
                write!(f, "an unresolved namespace_key is not {expected:?}")
            }
            Self::ResolvedNamespaceShape => f.write_str(
                "a resolved namespace_key is not a JSON object of exactly `identity` and `key`",
            ),
            Self::EmptyIdentity => f.write_str("a resolved namespace_key has an empty identity"),
            Self::EmbeddedKeyDiffers => {
                f.write_str("a resolved namespace_key embeds another key than the row's")
            }
            Self::NonCanonicalNamespace => {
                f.write_str("a resolved namespace_key is not in its canonical form")
            }
            Self::Id(e) => write!(f, "the id cannot be computed: {e}"),
            Self::WrongId { expected } => {
                write!(f, "contract_id is not the estate id of its namespace_key ({expected})")
            }
            Self::UnknownOwner(id) => write!(f, "the owner {id} is not a node of the repository"),
            Self::AmbiguityFlag => f.write_str("owner_ambiguous is present but not true"),
            Self::AmbiguousWithOwner => f.write_str("owner_ambiguous with an owner"),
            Self::ProviderOwners => f.write_str(
                "provider_owner_node_ids is not at least two distinct sorted nodes of an ambiguous row",
            ),
        }
    }
}

impl std::error::Error for ContractRowError {}

/// Checks a contract row of the repository `repo`, whose nodes `is_node` recognises.
///
/// # Errors
///
/// The first rule the row breaks.
pub fn check(
    contract: &Contract,
    repo: &RepoId,
    is_node: impl Fn(&NodeId) -> bool,
) -> Result<(), ContractRowError> {
    if contract.key.is_empty() || contract.namespace_key.is_empty() {
        return Err(ContractRowError::EmptyKey);
    }
    namespace(contract, repo)?;
    let expected = identity::contract_id(contract.kind, &contract.namespace_key)
        .map_err(ContractRowError::Id)?;
    if expected != contract.contract_id {
        return Err(ContractRowError::WrongId { expected });
    }
    if let Some(owner) = &contract.owner_node_id
        && !is_node(owner)
    {
        return Err(ContractRowError::UnknownOwner(owner.clone()));
    }
    ownership(contract, &is_node)
}

/// The namespace rules of 4.7.1 (issue 58).
fn namespace(contract: &Contract, repo: &RepoId) -> Result<(), ContractRowError> {
    let (kind, key, namespace) = (contract.kind, &contract.key, &contract.namespace_key);
    if identity::is_artifact(kind) {
        if contract.identity_strength != IdentityStrength::Exact {
            return Err(ContractRowError::ArtifactNotExact);
        }
        if namespace != key {
            return Err(ContractRowError::ArtifactNamespaceNotKey);
        }
        return Ok(());
    }
    if contract.identity_strength == IdentityStrength::Unresolved {
        let expected = identity::namespace_key(kind, &ContractIdentity::Unresolved, key, repo);
        return if *namespace == expected {
            Ok(())
        } else {
            Err(ContractRowError::UnresolvedNamespace { expected })
        };
    }
    let value: Value =
        serde_json::from_str(namespace).map_err(|_| ContractRowError::ResolvedNamespaceShape)?;
    let Some(object) = value.as_object() else {
        return Err(ContractRowError::ResolvedNamespaceShape);
    };
    let (Some(Value::String(name)), Some(Value::String(embedded)), 2) =
        (object.get("identity"), object.get("key"), object.len())
    else {
        return Err(ContractRowError::ResolvedNamespaceShape);
    };
    if name.is_empty() {
        return Err(ContractRowError::EmptyIdentity);
    }
    if embedded != key {
        return Err(ContractRowError::EmbeddedKeyDiffers);
    }
    // The bytes are the id's input: only the helper's own writing is accepted.
    let canonical =
        identity::namespace_key(kind, &ContractIdentity::Exact(name.clone()), key, repo);
    if canonical != *namespace {
        return Err(ContractRowError::NonCanonicalNamespace);
    }
    Ok(())
}

/// The ambiguous-owner props of issue 59.
fn ownership(
    contract: &Contract,
    is_node: &impl Fn(&NodeId) -> bool,
) -> Result<(), ContractRowError> {
    let flag = contract.props.get("owner_ambiguous");
    let owners = contract.props.get("provider_owner_node_ids");
    match flag {
        None if owners.is_some() => Err(ContractRowError::ProviderOwners),
        None => Ok(()),
        Some(Value::Bool(true)) => {
            if contract.owner_node_id.is_some() {
                return Err(ContractRowError::AmbiguousWithOwner);
            }
            let ids: Option<Vec<NodeId>> = owners.and_then(Value::as_array).and_then(|list| {
                list.iter()
                    .map(|v| v.as_str().and_then(|s| NodeId::parse(s).ok()))
                    .collect()
            });
            let valid = ids.is_some_and(|ids| {
                ids.len() >= 2 && ids.windows(2).all(|w| w[0] < w[1]) && ids.iter().all(is_node)
            });
            if valid {
                Ok(())
            } else {
                Err(ContractRowError::ProviderOwners)
            }
        }
        Some(_) => Err(ContractRowError::AmbiguityFlag),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::NodeKey;
    use crate::kinds::{ContractKind, NodeKind};
    use crate::model::{ContractDirection, Props};

    fn repo() -> RepoId {
        RepoId::parse("ce63551447285fd4").expect("a repo")
    }

    fn node(name: &str) -> NodeId {
        NodeKey::file(&repo(), name).node_id().expect("an id")
    }

    fn row(kind: ContractKind, key: &str, identity: &ContractIdentity) -> Contract {
        let namespace = identity::namespace_key(kind, identity, key, &repo());
        Contract {
            contract_id: identity::contract_id(kind, &namespace).expect("an id"),
            kind,
            key: key.to_owned(),
            namespace_key: namespace,
            identity_strength: identity.strength(),
            owner_node_id: None,
            direction: ContractDirection::Provides,
            props: Props::default(),
        }
    }

    fn with_namespace(mut c: Contract, namespace: &str) -> Contract {
        c.namespace_key = namespace.to_owned();
        c.contract_id = identity::contract_id(c.kind, namespace).expect("an id");
        c
    }

    fn checked(c: &Contract) -> Result<(), ContractRowError> {
        let (a, b) = (node("a"), node("b"));
        check(c, &repo(), |id| *id == a || *id == b)
    }

    #[test]
    fn rows_the_helper_writes_pass() {
        let exact = ContractIdentity::Exact("api.example.com".into());
        let declared = ContractIdentity::Declared("api.internal".into());
        assert_eq!(
            checked(&row(ContractKind::ApiContract, "GET /x", &exact)),
            Ok(())
        );
        assert_eq!(
            checked(&row(ContractKind::Channel, "orders", &declared)),
            Ok(())
        );
        assert_eq!(
            checked(&row(
                ContractKind::Table,
                "users",
                &ContractIdentity::Unresolved
            )),
            Ok(())
        );
        let artifact = ContractIdentity::Exact("maven:a:b".into());
        assert_eq!(
            checked(&row(ContractKind::Artifact, "maven:a:b", &artifact)),
            Ok(())
        );
        assert_eq!(NodeKind::Artifact, ContractKind::Artifact.node_kind());
    }

    #[test]
    fn semantic_rules_are_independent_of_the_id() {
        let api = row(
            ContractKind::ApiContract,
            "GET /x",
            &ContractIdentity::Exact("h".into()),
        );
        let cases = [
            (
                with_namespace(api.clone(), r#"{"identity":"h","key":"GET /y"}"#),
                ContractRowError::EmbeddedKeyDiffers,
            ),
            (
                with_namespace(api.clone(), r#"{"identity":"","key":"GET /x"}"#),
                ContractRowError::EmptyIdentity,
            ),
            (
                with_namespace(api.clone(), r#"{"key":"GET /x","identity":"h"}"#),
                ContractRowError::NonCanonicalNamespace,
            ),
            (
                with_namespace(api.clone(), r#"{"identity":"h","key":"GET /x","x":1}"#),
                ContractRowError::ResolvedNamespaceShape,
            ),
            (
                with_namespace(api.clone(), r#"["h","GET /x"]"#),
                ContractRowError::ResolvedNamespaceShape,
            ),
        ];
        for (c, expected) in cases {
            assert_eq!(checked(&c), Err(expected), "{}", c.namespace_key);
        }
        let unresolved = row(ContractKind::Channel, "t", &ContractIdentity::Unresolved);
        let other_repo = with_namespace(unresolved, "unresolved:2b2f33ba06c16e77:t");
        assert!(matches!(
            checked(&other_repo),
            Err(ContractRowError::UnresolvedNamespace { .. })
        ));
        let mut artifact = row(
            ContractKind::ArtifactVersion,
            "npm::a@1",
            &ContractIdentity::Exact("x".into()),
        );
        artifact.identity_strength = IdentityStrength::Declared;
        assert_eq!(checked(&artifact), Err(ContractRowError::ArtifactNotExact));
    }

    #[test]
    fn ambiguous_owners() {
        let mut c = row(
            ContractKind::ApiContract,
            "GET /x",
            &ContractIdentity::Unresolved,
        );
        let (mut a, mut b) = (node("a").as_str().to_owned(), node("b").as_str().to_owned());
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        c.props.insert("owner_ambiguous", Value::Bool(true));
        c.props
            .insert("provider_owner_node_ids", serde_json::json!([a, b]));
        assert_eq!(checked(&c), Ok(()));
        let mut owned = c.clone();
        owned.owner_node_id = Some(node("a"));
        assert_eq!(checked(&owned), Err(ContractRowError::AmbiguousWithOwner));
        let mut unknown = c.clone();
        unknown.props.insert(
            "provider_owner_node_ids",
            serde_json::json!([a, node("z").as_str()]),
        );
        assert_eq!(checked(&unknown), Err(ContractRowError::ProviderOwners));
        let mut duplicate = c;
        duplicate
            .props
            .insert("provider_owner_node_ids", serde_json::json!([a, a]));
        assert_eq!(checked(&duplicate), Err(ContractRowError::ProviderOwners));
    }
}
