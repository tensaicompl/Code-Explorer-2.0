//! Every SQL statement the segment code runs, by name (plan Part 5.6).
//!
//! The schema itself is `schema.sql`; the verification pragmas are in `verify.rs`.
//! Nothing else in the crate writes SQL. Every value is bound as a parameter: no
//! statement here is ever assembled from data, and the filters that take a set of
//! values take it as one JSON array, expanded by `json_each`, so even they are fixed
//! text.

// --- building ------------------------------------------------------------------------

/// The build connection's settings, before the schema exists.
///
/// The page size, text encoding and vacuum mode are written into the file, so they are
/// stated rather than left to the library's defaults. Journaling and syncing are off
/// for the build (4.3); the build file is temporary and rebuilt on failure. Foreign
/// keys are enforced as rows arrive, so a broken reference fails the insert that makes
/// it.
pub(crate) const BUILD_PRAGMAS: &str = "\
PRAGMA page_size = 4096;
PRAGMA encoding = 'UTF-8';
PRAGMA auto_vacuum = NONE;
PRAGMA journal_mode = OFF;
PRAGMA synchronous = OFF;
PRAGMA foreign_keys = ON;
";

pub(crate) const INSERT_META: &str = "INSERT INTO meta (key, value) VALUES (?1, ?2)";

pub(crate) const INSERT_FILE: &str = "INSERT INTO files \
    (file_id, path, language, status, status_reason, blob_sha, size_bytes, line_count) \
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)";

pub(crate) const INSERT_NODE: &str = "INSERT INTO nodes \
    (node_id, kind, name, qualified_name, file_id, parent_id, start_line, end_line, \
     layer_role, signature, doc, props) \
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)";

pub(crate) const INSERT_SITE: &str = "INSERT INTO sites \
    (site_id, file_id, enclosing_node_id, site_kind, ast_fingerprint, start_byte, end_byte, \
     start_line, start_col, end_line, end_col, callee_text, receiver_text) \
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)";

pub(crate) const INSERT_EDGE: &str = "INSERT INTO edges \
    (edge_id, src, dst, kind, band, observed, engine_score, engine_strategy, \
     engine_candidates, site_id, weight, props) \
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)";

pub(crate) const INSERT_EVIDENCE: &str = "INSERT INTO evidence \
    (evidence_id, fact_kind, fact_id, provider, provider_version, authority, verdict, \
     target_node_id, file_id, start_byte, end_byte, metadata) \
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)";

pub(crate) const INSERT_OCCURRENCE: &str = "INSERT INTO semantic_occurrences \
    (occ_id, provider, provider_symbol, target_node_id, file_id, start_byte, end_byte, \
     start_line, start_col, role, site_id, enclosing_node_id) \
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)";

pub(crate) const INSERT_CANDIDATE: &str = "INSERT INTO candidates \
    (site_id, src, callee_name, band, candidate_ids, engine_score, engine_strategy, \
    engine_candidates, reason) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)";

pub(crate) const INSERT_CONTRACT: &str = "INSERT INTO contracts \
    (contract_id, kind, key, namespace_key, identity_strength, owner_node_id, direction, props) \
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)";

pub(crate) const INSERT_METRIC: &str =
    "INSERT INTO metrics (node_id, metric, value) VALUES (?1, ?2, ?3)";

pub(crate) const INSERT_COVERAGE: &str = "INSERT INTO coverage \
    (language, files, parsed, partial, failed, skipped, symbols, call_sites, by_band, \
     observed_links) \
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)";

/// Builds the external-content full-text index from `nodes`, in one pass, once every
/// node is in (the documented FTS5 rebuild).
pub(crate) const FTS_REBUILD: &str = "INSERT INTO fts(fts) VALUES ('rebuild')";

/// Checks the full-text index against `nodes`; fails if they differ.
pub(crate) const FTS_INTEGRITY_CHECK: &str =
    "INSERT INTO fts(fts, rank) VALUES ('integrity-check', 1)";

/// Writes the finished database, compacted, to a new file named by the parameter.
pub(crate) const VACUUM_INTO: &str = "VACUUM INTO ?1";

// --- reading -------------------------------------------------------------------------

pub(crate) const META_ALL: &str = "SELECT key, value FROM meta ORDER BY key";

macro_rules! node_select {
    ($tail:literal) => {
        concat!(
            "SELECT node_id, kind, name, qualified_name, file_id, parent_id, start_line, ",
            "end_line, layer_role, signature, doc, props FROM nodes ",
            $tail
        )
    };
}

pub(crate) const NODE_BY_ID: &str = node_select!("WHERE node_id = ?1");

pub(crate) const CHILDREN: &str = node_select!("WHERE parent_id = ?1 ORDER BY node_id");

macro_rules! edge_select {
    ($tail:literal) => {
        concat!(
            "SELECT edge_id, src, dst, kind, band, observed, engine_score, engine_strategy, ",
            "engine_candidates, site_id, weight, props FROM edges ",
            $tail
        )
    };
}

/// Edges from `?1`; `?2` is NULL for every band, or a JSON array of the bands wanted.
pub(crate) const EDGES_FROM: &str = edge_select!(
    "WHERE src = ?1 AND (?2 IS NULL OR band IN (SELECT value FROM json_each(?2))) ORDER BY edge_id"
);

/// Edges to `?1`; `?2` as for [`EDGES_FROM`].
pub(crate) const EDGES_TO: &str = edge_select!(
    "WHERE dst = ?1 AND (?2 IS NULL OR band IN (SELECT value FROM json_each(?2))) ORDER BY edge_id"
);

pub(crate) const CANDIDATES_FROM: &str = "SELECT site_id, src, callee_name, band, \
    candidate_ids, engine_score, engine_strategy, engine_candidates, reason FROM candidates \
    WHERE src = ?1 ORDER BY site_id";

/// Contracts of kind `?1` and key `?2`, either NULL for any.
pub(crate) const CONTRACTS: &str = "SELECT contract_id, kind, key, namespace_key, \
    identity_strength, owner_node_id, direction, props FROM contracts \
    WHERE (?1 IS NULL OR kind = ?1) AND (?2 IS NULL OR key = ?2) ORDER BY contract_id";

pub(crate) const METRICS_OF: &str =
    "SELECT node_id, metric, value FROM metrics WHERE node_id = ?1 ORDER BY metric";

macro_rules! coverage_select {
    ($tail:literal) => {
        concat!(
            "SELECT language, files, parsed, partial, failed, skipped, symbols, call_sites, ",
            "by_band, observed_links FROM coverage ",
            $tail
        )
    };
}

pub(crate) const COVERAGE_FOR: &str = coverage_select!("WHERE language = ?1");

pub(crate) const COVERAGE_ALL: &str = coverage_select!("ORDER BY language");

macro_rules! file_select {
    ($tail:literal) => {
        concat!(
            "SELECT file_id, path, language, status, status_reason, blob_sha, size_bytes, ",
            "line_count FROM files ",
            $tail
        )
    };
}

pub(crate) const FILE_BY_ID: &str = file_select!("WHERE file_id = ?1");

pub(crate) const FILE_BY_PATH: &str = file_select!("WHERE path = ?1");

pub(crate) const SITE_BY_ID: &str = "SELECT site_id, file_id, enclosing_node_id, site_kind, \
    ast_fingerprint, start_byte, end_byte, start_line, start_col, end_line, end_col, \
    callee_text, receiver_text FROM sites WHERE site_id = ?1";

/// Where a node is: its file's path and its lines.
pub(crate) const NODE_LOCATION: &str = "SELECT n.node_id, f.path, n.start_line, n.end_line \
    FROM nodes AS n LEFT JOIN files AS f ON f.file_id = n.file_id WHERE n.node_id = ?1";

pub(crate) const EVIDENCE_FOR: &str = "SELECT evidence_id, fact_kind, fact_id, provider, \
    provider_version, authority, verdict, target_node_id, file_id, start_byte, end_byte, \
    metadata FROM evidence WHERE fact_kind = ?1 AND fact_id = ?2 ORDER BY evidence_id";

pub(crate) const OCCURRENCES_IN_FILE: &str = "SELECT occ_id, provider, provider_symbol, \
    target_node_id, file_id, start_byte, end_byte, start_line, start_col, role, site_id, \
    enclosing_node_id FROM semantic_occurrences WHERE file_id = ?1 ORDER BY occ_id";

/// Nodes matching the full-text query `?1`, best first by BM25 and then by node id, so
/// equal scores come back in one order; at most `?2` of them.
pub(crate) const SEARCH: &str = "SELECT n.node_id, n.kind, n.name, n.qualified_name, \
    n.file_id, n.parent_id, n.start_line, n.end_line, n.layer_role, n.signature, n.doc, \
    n.props, bm25(fts) AS score \
    FROM fts JOIN nodes AS n ON n.rowid = fts.rowid \
    WHERE fts MATCH ?1 ORDER BY score, n.node_id LIMIT ?2";

/// How many columns a node query returns before any others (`node_select!`'s list).
pub(crate) const NODE_COLUMN_COUNT: usize = 12;

// --- for tests -----------------------------------------------------------------------

/// Statements the tests run against a writable copy of a segment, to damage it in one
/// precise way and prove that verification notices, or to look at its schema. Nothing
/// in the crate runs them; they live here so that no SQL is written anywhere else.
#[doc(hidden)]
pub mod testing {
    /// Sets a meta key's value.
    pub const SET_META: &str = "UPDATE meta SET value = ?2 WHERE key = ?1";
    /// Removes a meta key.
    pub const DELETE_META: &str = "DELETE FROM meta WHERE key = ?1";
    /// Adds a meta key.
    pub const ADD_META: &str = "INSERT INTO meta (key, value) VALUES (?1, ?2)";
    /// Turns enforcement of foreign keys off, so a broken reference can be planted.
    pub const FOREIGN_KEYS_OFF: &str = "PRAGMA foreign_keys = OFF";
    /// Turns CHECK constraints off, so a value they forbid can be planted.
    pub const CHECKS_OFF: &str = "PRAGMA ignore_check_constraints = ON";
    /// Points a node at a file that does not exist.
    pub const SET_NODE_FILE: &str = "UPDATE nodes SET file_id = ?2 WHERE node_id = ?1";
    /// Sets a node's kind.
    pub const SET_NODE_KIND: &str = "UPDATE nodes SET kind = ?2 WHERE node_id = ?1";
    /// Sets a candidate row's band.
    pub const SET_CANDIDATE_BAND: &str = "UPDATE candidates SET band = ?2 WHERE site_id = ?1";
    /// Sets a coverage row's band counts.
    pub const SET_COVERAGE_BANDS: &str = "UPDATE coverage SET by_band = ?2 WHERE language = ?1";
    /// Every schema object: its type and name, by name.
    pub const SCHEMA_OBJECTS: &str =
        "SELECT type, name FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY name";
}
