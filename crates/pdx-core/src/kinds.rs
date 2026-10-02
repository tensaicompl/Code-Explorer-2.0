//! The kinds of node, edge and evidence site the graph holds (specification 4.2.3,
//! 4.2.4), and the vocabularies that go with them.
//!
//! Every kind is here, including those later phases create: the vocabulary is the
//! stored format, and it is fixed before anything is stored.

use crate::vocabulary::vocabulary;

vocabulary! {
    /// A node's kind (4.2.3). Its spelling is part of the node's identity (4.2.1).
    pub enum NodeKind {
        /// A repository.
        Repo => "Repo",
        /// A directory.
        Folder => "Folder",
        /// A file.
        File => "File",
        /// A package, namespace or Ada package.
        Module => "Module",
        /// A class.
        Class => "Class",
        /// An interface.
        Interface => "Interface",
        /// An enumeration.
        Enum => "Enum",
        /// A struct.
        Struct => "Struct",
        /// A trait.
        Trait => "Trait",
        /// A type alias.
        TypeAlias => "TypeAlias",
        /// A function.
        Function => "Function",
        /// A method.
        Method => "Method",
        /// A constructor.
        Constructor => "Constructor",
        /// A field.
        Field => "Field",
        /// A module-level variable.
        Variable => "Variable",
        /// A macro.
        Macro => "Macro",
        /// An HTTP endpoint's binding to its handler.
        Route => "Route",
        /// A test.
        Test => "Test",
        /// An instance of a Kubernetes kind.
        Resource => "Resource",
        /// A configuration key.
        ConfigKey => "ConfigKey",
        /// A section of a Markdown document.
        Doc => "Doc",
        /// An `OpenAPI` operation or a route template.
        ApiContract => "ApiContract",
        /// A protobuf or gRPC method.
        RpcMethod => "RpcMethod",
        /// An `AsyncAPI` channel, a Kafka topic or a queue.
        Channel => "Channel",
        /// A database table.
        Table => "Table",
        /// A database column.
        Column => "Column",
        /// A build coordinate, `group:name`.
        Artifact => "Artifact",
        /// A version of an artifact.
        ArtifactVersion => "ArtifactVersion",
        /// A system.
        System => "System",
        /// A bounded context.
        BoundedContext => "BoundedContext",
        /// A service.
        Service => "Service",
        /// A library.
        Library => "Library",
        /// A database.
        Database => "Database",
        /// A message broker.
        Broker => "Broker",
        /// A frontend.
        Frontend => "Frontend",
        /// A batch job.
        BatchJob => "BatchJob",
        /// A deployment.
        Deployment => "Deployment",
        /// A layer role (its values are [`LayerRole`]).
        LayerRole => "LayerRole",
    }
}

/// The three groups 4.2.3 divides node kinds into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeCategory {
    /// Per repository: from `Repo` to `Doc`.
    Structural,
    /// Estate-level, owned by the producing repository: from `ApiContract` to
    /// `ArtifactVersion`.
    Contract,
    /// Estate-level: from `System` to `LayerRole`.
    Architecture,
}

impl NodeKind {
    /// Which of 4.2.3's groups the kind is in.
    pub const fn category(self) -> NodeCategory {
        match self {
            Self::ApiContract
            | Self::RpcMethod
            | Self::Channel
            | Self::Table
            | Self::Column
            | Self::Artifact
            | Self::ArtifactVersion => NodeCategory::Contract,
            Self::System
            | Self::BoundedContext
            | Self::Service
            | Self::Library
            | Self::Database
            | Self::Broker
            | Self::Frontend
            | Self::BatchJob
            | Self::Deployment
            | Self::LayerRole => NodeCategory::Architecture,
            _ => NodeCategory::Structural,
        }
    }

    /// Whether nodes of this kind belong to the estate rather than one repository, and
    /// so are identified with `repo_id = "estate"` (4.2.1).
    pub const fn is_estate_level(self) -> bool {
        !matches!(self.category(), NodeCategory::Structural)
    }
}

vocabulary! {
    /// The role a module or class plays in its architecture's layers (4.2.3). Stored
    /// on the node itself (`nodes.layer_role`) in a segment.
    pub enum LayerRole {
        /// `api`.
        Api => "api",
        /// `service`.
        Service => "service",
        /// `domain`.
        Domain => "domain",
        /// `persistence`.
        Persistence => "persistence",
        /// `adapter`.
        Adapter => "adapter",
        /// `port`.
        Port => "port",
        /// `infra`.
        Infra => "infra",
        /// `test`.
        Test => "test",
        /// `unknown`.
        Unknown => "unknown",
    }
}

vocabulary! {
    /// An edge's kind (4.2.4). Its spelling is part of the edge's identity (4.2.1).
    pub enum EdgeKind {
        /// The containment tree, Repo → Folder → File → Module → Class → Method and
        /// so on: one parent per node but the repository. Never stored as an edge in a
        /// segment, where it is `nodes.parent_id`.
        Contains => "CONTAINS",
        /// A call.
        Calls => "CALLS",
        /// A callable passed as a value.
        CallReference => "CALL_REFERENCE",
        /// An identifier used, its target not proven.
        Usage => "USAGE",
        /// An import.
        Imports => "IMPORTS",
        /// Inheritance.
        Inherits => "INHERITS",
        /// An implementation of an interface.
        Implements => "IMPLEMENTS",
        /// An override.
        Overrides => "OVERRIDES",
        /// A use of a type.
        UsesType => "USES_TYPE",
        /// A read of a field.
        ReadsField => "READS_FIELD",
        /// A write of a field.
        WritesField => "WRITES_FIELD",
        /// An exception thrown.
        Throws => "THROWS",
        /// A test of a symbol.
        Tests => "TESTS",
        /// From a handler to its `Route`.
        DefinesRoute => "DEFINES_ROUTE",
        /// A read of a configuration key.
        ReadsConfig => "READS_CONFIG",
        /// From a service or route to the contract it exposes.
        Exposes => "EXPOSES",
        /// From a call site to the contract it consumes.
        Consumes => "CONSUMES",
        /// A message published.
        Publishes => "PUBLISHES",
        /// A subscription.
        Subscribes => "SUBSCRIBES",
        /// A read of a table.
        ReadsTable => "READS_TABLE",
        /// A write of a table.
        WritesTable => "WRITES_TABLE",
        /// A table's definition.
        DefinesTable => "DEFINES_TABLE",
        /// An artifact a build produces.
        ProducesArtifact => "PRODUCES_ARTIFACT",
        /// A dependency on an artifact.
        LinksArtifact => "LINKS_ARTIFACT",
        /// An HTTP call between repositories.
        HttpCalls => "HTTP_CALLS",
        /// A remote procedure call between repositories.
        RpcCalls => "RPC_CALLS",
        /// From a producer to a consumer, through a channel.
        Messages => "MESSAGES",
        /// A table two repositories share.
        SharesTable => "SHARES_TABLE",
        /// A dependency between repositories through an artifact.
        DependsOnArtifact => "DEPENDS_ON_ARTIFACT",
        /// Membership: a repository or service in a bounded context, a context in a
        /// system.
        MemberOf => "MEMBER_OF",
        /// What a service is deployed as.
        DeployedAs => "DEPLOYED_AS",
        /// From a module or class to its layer role. Never stored in a segment, where
        /// `nodes.layer_role` is authoritative; materialised in the estate database
        /// only.
        HasRole => "HAS_ROLE",
        /// A dependency between layers, aggregated.
        LayerDepends => "LAYER_DEPENDS",
        /// A context-map relation.
        UpstreamOf => "UPSTREAM_OF",
        /// From an edge to the architecture rule it breaks.
        Violates => "VIOLATES",
        /// Two files that change together.
        ChangesWith => "CHANGES_WITH",
        /// A near-clone.
        SimilarTo => "SIMILAR_TO",
    }
}

/// The groups 4.2.4 divides edge kinds into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EdgeCategory {
    /// `CONTAINS`.
    Containment,
    /// Within a repository's code.
    Structural,
    /// Between code and the contracts it exposes, consumes, publishes or defines.
    Contract,
    /// Between repositories, derived from their contracts.
    DerivedCrossRepo,
    /// Between architecture elements.
    Architecture,
    /// From the repository's history.
    History,
}

impl EdgeKind {
    /// Which of 4.2.4's groups the kind is in.
    pub const fn category(self) -> EdgeCategory {
        match self {
            Self::Contains => EdgeCategory::Containment,
            Self::Calls
            | Self::CallReference
            | Self::Usage
            | Self::Imports
            | Self::Inherits
            | Self::Implements
            | Self::Overrides
            | Self::UsesType
            | Self::ReadsField
            | Self::WritesField
            | Self::Throws
            | Self::Tests
            | Self::DefinesRoute
            | Self::ReadsConfig => EdgeCategory::Structural,
            Self::Exposes
            | Self::Consumes
            | Self::Publishes
            | Self::Subscribes
            | Self::ReadsTable
            | Self::WritesTable
            | Self::DefinesTable
            | Self::ProducesArtifact
            | Self::LinksArtifact => EdgeCategory::Contract,
            Self::HttpCalls
            | Self::RpcCalls
            | Self::Messages
            | Self::SharesTable
            | Self::DependsOnArtifact => EdgeCategory::DerivedCrossRepo,
            Self::MemberOf
            | Self::DeployedAs
            | Self::HasRole
            | Self::LayerDepends
            | Self::UpstreamOf
            | Self::Violates => EdgeCategory::Architecture,
            Self::ChangesWith | Self::SimilarTo => EdgeCategory::History,
        }
    }

    /// Whether 4.2.2 requires every edge of this kind to carry a confidence band.
    pub const fn requires_band(self) -> bool {
        matches!(
            self,
            Self::Calls
                | Self::UsesType
                | Self::Inherits
                | Self::Implements
                | Self::Overrides
                | Self::Imports
                | Self::HttpCalls
                | Self::RpcCalls
                | Self::Publishes
                | Self::Subscribes
                | Self::ReadsTable
                | Self::WritesTable
                | Self::LinksArtifact
        )
    }
}

vocabulary! {
    /// What an evidence site is (the `sites` table of 4.3). Part of the site's identity
    /// (4.2.1).
    pub enum SiteKind {
        /// A call.
        Call => "call",
        /// A reference to a callable, or another use of a name.
        Reference => "reference",
        /// An import.
        Import => "import",
        /// A reference to a type.
        TypeRef => "type_ref",
        /// A read or write of a field.
        FieldRw => "field_rw",
        /// A route's binding.
        Route => "route",
        /// A contract's use or definition.
        Contract => "contract",
    }
}

vocabulary! {
    /// A contract's kind (the `contracts` table of 4.3): exactly the contract node
    /// kinds of 4.2.3, spelled the same.
    pub enum ContractKind {
        /// `ApiContract`.
        ApiContract => "ApiContract",
        /// `RpcMethod`.
        RpcMethod => "RpcMethod",
        /// `Channel`.
        Channel => "Channel",
        /// `Table`.
        Table => "Table",
        /// `Column`.
        Column => "Column",
        /// `Artifact`.
        Artifact => "Artifact",
        /// `ArtifactVersion`.
        ArtifactVersion => "ArtifactVersion",
    }
}

impl ContractKind {
    /// The node kind of the contract's node.
    pub const fn node_kind(self) -> NodeKind {
        match self {
            Self::ApiContract => NodeKind::ApiContract,
            Self::RpcMethod => NodeKind::RpcMethod,
            Self::Channel => NodeKind::Channel,
            Self::Table => NodeKind::Table,
            Self::Column => NodeKind::Column,
            Self::Artifact => NodeKind::Artifact,
            Self::ArtifactVersion => NodeKind::ArtifactVersion,
        }
    }

    /// The contract kind of a contract node kind; `None` for any other kind.
    pub const fn from_node_kind(kind: NodeKind) -> Option<Self> {
        match kind {
            NodeKind::ApiContract => Some(Self::ApiContract),
            NodeKind::RpcMethod => Some(Self::RpcMethod),
            NodeKind::Channel => Some(Self::Channel),
            NodeKind::Table => Some(Self::Table),
            NodeKind::Column => Some(Self::Column),
            NodeKind::Artifact => Some(Self::Artifact),
            NodeKind::ArtifactVersion => Some(Self::ArtifactVersion),
            _ => None,
        }
    }
}
