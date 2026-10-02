//! Segments: one repository at one commit under one profile, as an immutable SQLite
//! file (specification 4.3).
//!
//! [`SegmentWriter`] turns a complete set of rows, [`SegmentData`], into a finished
//! segment: built in a temporary file, rows inserted in a canonical order, the
//! full-text index built, the result written with `VACUUM INTO` to a new file that is
//! then made read-only and hashed. The same rows always give the same bytes.
//! [`SegmentReader`] opens a segment read-only and immutable, refuses one it cannot
//! trust (`verify`), and answers typed queries. The schema is `schema.sql`, 4.3's DDL
//! verbatim; every other statement is in `queries.rs`.

mod reader;
mod verify;
mod writer;

pub mod queries;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::consts::{ENGINE_VERSION, LANGUAGE_MATRIX_VERSION, SEGMENT_SCHEMA_VERSION};
use crate::ids::RepoId;
use crate::model::{
    CandidateSite, Contract, Coverage, Edge, Evidence, FileRecord, Metric, Node,
    SemanticOccurrence, Site,
};
use crate::vocabulary::vocabulary;

pub use reader::{NodeLocation, SearchHit, SegmentReader};
pub use writer::{SegmentWriter, WrittenSegment};

/// The schema, 4.3's DDL exactly.
pub const SCHEMA: &str = include_str!("schema.sql");

/// Why a segment could not be written, opened or read.
#[derive(Debug, thiserror::Error)]
pub enum SegmentError {
    /// SQLite refused a statement.
    #[error("SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// A file could not be read, written or have its permissions set.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// What went wrong.
        #[source]
        source: std::io::Error,
    },
    /// The segment's schema is another version.
    #[error("the segment's schema version is {found}; this build reads version {expected} only")]
    WrongSchemaVersion {
        /// The version this build reads.
        expected: u32,
        /// The version the segment has.
        found: u32,
    },
    /// A meta key every segment has is missing.
    #[error("the segment has no `{0}` in its meta table")]
    MissingMeta(&'static str),
    /// A meta value is not what its key requires.
    #[error("meta `{key}` is {value:?}: {reason}")]
    MalformedMeta {
        /// The key.
        key: String,
        /// Its value.
        value: String,
        /// What is wrong with it.
        reason: String,
    },
    /// The meta table holds a key no segment of this version has.
    #[error("the meta table holds `{0}`, which no version 1 segment has")]
    UnexpectedMeta(String),
    /// A row refers to a row that is not there.
    #[error("{count} broken reference(s); the first, from {table} row {rowid}, to {parent}")]
    ForeignKeyViolation {
        /// The referring table.
        table: String,
        /// The referring row.
        rowid: i64,
        /// The table referred to.
        parent: String,
        /// How many there are.
        count: usize,
    },
    /// A stored identity is not one.
    #[error("{table}.{column} holds {value:?}, which is not a valid id: {reason}")]
    InvalidStoredId {
        /// The table.
        table: &'static str,
        /// The column.
        column: &'static str,
        /// The value.
        value: String,
        /// Why.
        reason: String,
    },
    /// A stored value is outside its closed vocabulary.
    #[error("{table}.{column} holds {value:?}, which is not one of its values")]
    InvalidStoredVocabulary {
        /// The table.
        table: &'static str,
        /// The column.
        column: &'static str,
        /// The value.
        value: String,
    },
    /// A stored JSON value is malformed or incomplete.
    #[error("{table}.{column} holds {value:?}, which is not what it must be: {reason}")]
    InvalidStoredJson {
        /// The table.
        table: &'static str,
        /// The column.
        column: &'static str,
        /// The value.
        value: String,
        /// Why.
        reason: String,
    },
    /// A stored number or flag is out of range, or NULL where the model needs a value.
    #[error("{table}.{column} holds {value}, which is out of range")]
    InvalidStoredNumber {
        /// The table.
        table: &'static str,
        /// The column.
        column: &'static str,
        /// The value.
        value: String,
    },
    /// A row given to the writer cannot be stored as it is.
    #[error("{table}: {reason}")]
    InvalidRow {
        /// The table.
        table: &'static str,
        /// Why.
        reason: String,
    },
    /// Two rows given to the writer have the same key.
    #[error("{table} has two rows with the key {key}")]
    DuplicateRow {
        /// The table.
        table: &'static str,
        /// The key.
        key: String,
    },
    /// The destination exists; a segment is never overwritten.
    #[error("{} already exists, and a segment is never overwritten", .0.display())]
    DestinationExists(PathBuf),
    /// The destination cannot be written to.
    #[error("{}: {reason}", path.display())]
    InvalidDestination {
        /// The destination.
        path: PathBuf,
        /// Why.
        reason: String,
    },
    /// A full-text query is not valid FTS5 syntax.
    #[error("search {query:?} is not a valid full-text query: {reason}")]
    InvalidSearch {
        /// The query.
        query: String,
        /// SQLite's reason.
        reason: String,
    },
}

vocabulary! {
    /// Which build a segment holds (4.3): structural, or with precise sources merged.
    pub enum SegmentProfile {
        /// Extraction, typed resolution and the Rust stages.
        Structural => "structural",
        /// The structural build with compiler-backed sources merged in.
        Precise => "precise",
    }
}

/// A compiler-backed source merged into a precise segment: one entry of `meta`'s
/// `precise_sources`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreciseSource {
    /// The provider's id.
    pub provider: String,
    /// Its version.
    pub version: String,
    /// The digest of the container it ran in.
    pub container_digest: String,
}

/// A segment's `meta` table: every key 4.3 names, and no other.
///
/// There is no build time and no content hash in it: a segment built twice must be
/// the same bytes, and its content hash is of those bytes, so it is kept outside.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentMeta {
    /// `schema_version`: [`SEGMENT_SCHEMA_VERSION`] for every segment this build writes.
    pub schema_version: u32,
    /// `repo_id`.
    pub repo_id: RepoId,
    /// `repo_url`.
    pub repo_url: String,
    /// `commit_sha`: the commit, in lower-case hex.
    pub commit_sha: String,
    /// `profile`.
    pub profile: SegmentProfile,
    /// `engine_version`.
    pub engine_version: u32,
    /// `pdx_version`: the version of PDX that built it.
    pub pdx_version: String,
    /// `language_matrix_version`.
    pub language_matrix_version: u32,
    /// `precise_sources`: empty for a structural segment. Stored sorted, whatever
    /// order it is given in.
    pub precise_sources: Vec<PreciseSource>,
}

impl SegmentMeta {
    /// The meta of a segment this build writes now: the schema, engine and language
    /// matrix versions are this build's, from `consts`, and so is the PDX version.
    pub fn new(
        repo_id: RepoId,
        repo_url: &str,
        commit_sha: &str,
        profile: SegmentProfile,
        precise_sources: Vec<PreciseSource>,
    ) -> Self {
        Self {
            schema_version: SEGMENT_SCHEMA_VERSION,
            repo_id,
            repo_url: repo_url.to_owned(),
            commit_sha: commit_sha.to_owned(),
            profile,
            engine_version: ENGINE_VERSION,
            pdx_version: env!("CARGO_PKG_VERSION").to_owned(),
            language_matrix_version: LANGUAGE_MATRIX_VERSION,
            precise_sources,
        }
    }
}

/// Everything a segment holds, as rows: what the pipeline hands the writer.
///
/// Rows may arrive in any order; the writer stores each table in its key's order, so
/// the order here never reaches the file. Keys must be unique: rows that share one
/// are an error, never merged or replaced.
#[derive(Clone, Debug, PartialEq)]
pub struct SegmentData {
    /// The `meta` table.
    pub meta: SegmentMeta,
    /// The `files` table.
    pub files: Vec<FileRecord>,
    /// The `nodes` table.
    pub nodes: Vec<Node>,
    /// The `sites` table.
    pub sites: Vec<Site>,
    /// The `edges` table.
    pub edges: Vec<Edge>,
    /// The `evidence` table.
    pub evidence: Vec<Evidence>,
    /// The `semantic_occurrences` table.
    pub semantic_occurrences: Vec<SemanticOccurrence>,
    /// The `candidates` table.
    pub candidates: Vec<CandidateSite>,
    /// The `contracts` table.
    pub contracts: Vec<Contract>,
    /// The `metrics` table.
    pub metrics: Vec<Metric>,
    /// The `coverage` table.
    pub coverage: Vec<Coverage>,
}

impl SegmentData {
    /// A segment with this meta and no rows.
    pub fn new(meta: SegmentMeta) -> Self {
        Self {
            meta,
            files: Vec::new(),
            nodes: Vec::new(),
            sites: Vec::new(),
            edges: Vec::new(),
            evidence: Vec::new(),
            semantic_occurrences: Vec::new(),
            candidates: Vec::new(),
            contracts: Vec::new(),
            metrics: Vec::new(),
            coverage: Vec::new(),
        }
    }
}
