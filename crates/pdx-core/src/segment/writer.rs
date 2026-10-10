//! Writing a segment (4.3): a temporary build, then `VACUUM INTO` a new file that is
//! made read-only and hashed.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, Statement, params};
use sha2::{Digest, Sha256};

use super::{SCHEMA, SegmentData, SegmentError, queries, verify};
use crate::ids::{NodeId, NodeKey, RepoId, SiteId};
use crate::kinds::LayerRole;
use crate::languages;
use crate::model::{
    CandidateSite, Contract, Coverage, Edge, Evidence, FileRecord, Metric, Node,
    SemanticOccurrence, Site,
};

/// A finished segment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WrittenSegment {
    /// Where it is.
    pub path: PathBuf,
    /// `lowerhex(sha256)` of its bytes, exactly as written: the segment's identity as
    /// an object, stored beside it, never in it.
    pub content_sha256: String,
    /// Its size.
    pub size_bytes: u64,
}

/// Writes segments.
///
/// A segment is built in a temporary directory, by default the system's, and only its
/// finished form reaches the destination. Where the build happens never affects the
/// bytes written.
#[derive(Clone, Debug, Default)]
pub struct SegmentWriter {
    temp_root: Option<PathBuf>,
}

impl SegmentWriter {
    /// A writer that builds in the system's temporary directory.
    pub fn new() -> Self {
        Self::default()
    }

    /// The writer, building in a temporary directory created inside `dir` instead.
    #[must_use]
    pub fn build_in(mut self, dir: &Path) -> Self {
        self.temp_root = Some(dir.to_path_buf());
        self
    }

    /// Writes `data` as a segment at `destination`, which must not exist.
    ///
    /// In order: a temporary database, its settings, the schema, every table's rows in
    /// their key's order in one transaction, the full-text index, a check of the
    /// build, `VACUUM INTO` the destination, every connection closed, the destination
    /// made read-only (mode 0444 on Unix), and its bytes hashed. Nothing is left at the
    /// destination if any step fails.
    ///
    /// # Errors
    ///
    /// [`SegmentError::DestinationExists`] if anything is at `destination`;
    /// [`SegmentError::InvalidDestination`] if its directory is missing or its path is
    /// not UTF-8; [`SegmentError::InvalidRow`] or [`SegmentError::DuplicateRow`] for
    /// rows that cannot be stored as given; SQLite and file errors.
    pub fn write(
        &self,
        data: &SegmentData,
        destination: &Path,
    ) -> Result<WrittenSegment, SegmentError> {
        let destination = std::path::absolute(destination).map_err(|source| SegmentError::Io {
            path: destination.to_path_buf(),
            source,
        })?;
        check_destination(&destination)?;
        let rows = Canonical::of(data)?;

        let temp = match &self.temp_root {
            Some(dir) => tempfile::Builder::new()
                .prefix("pdx-segment-")
                .tempdir_in(dir),
            None => tempfile::Builder::new().prefix("pdx-segment-").tempdir(),
        }
        .map_err(|source| SegmentError::Io {
            path: self.temp_root.clone().unwrap_or_else(std::env::temp_dir),
            source,
        })?;
        let build_path = temp.path().join("build.db");

        let result = build(&rows, &build_path, &destination).and_then(|()| publish(&destination));
        if result.is_err() {
            remove_partial(&destination);
        }
        drop(temp);
        result
    }
}

fn check_destination(destination: &Path) -> Result<(), SegmentError> {
    if destination.symlink_metadata().is_ok() {
        return Err(SegmentError::DestinationExists(destination.to_path_buf()));
    }
    let invalid = |reason: &str| SegmentError::InvalidDestination {
        path: destination.to_path_buf(),
        reason: reason.to_owned(),
    };
    if destination.to_str().is_none() {
        return Err(invalid("the path is not UTF-8"));
    }
    match destination.parent() {
        Some(parent) if parent.is_dir() => Ok(()),
        _ => Err(invalid("its directory does not exist")),
    }
}

/// Builds the segment at `build_path` and writes it to `destination` with `VACUUM INTO`.
/// Every connection is closed when this returns.
fn build(rows: &Canonical<'_>, build_path: &Path, destination: &Path) -> Result<(), SegmentError> {
    // Opened without URI names, so no path is ever read as anything but a path.
    let mut conn = Connection::open_with_flags(
        build_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.execute_batch(queries::BUILD_PRAGMAS)?;
    conn.execute_batch(SCHEMA)?;

    let transaction = conn.transaction()?;
    insert_all(&transaction, rows)?;
    transaction.execute(queries::FTS_REBUILD, [])?;
    transaction.execute(queries::FTS_INTEGRITY_CHECK, [])?;
    transaction.commit()?;

    verify::verify(&conn)?;
    let destination = destination.to_str().expect("checked to be UTF-8");
    conn.execute(queries::VACUUM_INTO, [destination])?;
    conn.close().map_err(|(_, e)| SegmentError::Sqlite(e))?;
    Ok(())
}

/// Makes the finished file read-only and hashes it.
fn publish(destination: &Path) -> Result<WrittenSegment, SegmentError> {
    let io = |source| SegmentError::Io {
        path: destination.to_path_buf(),
        source,
    };
    make_read_only(destination).map_err(io)?;
    let mut file = File::open(destination).map_err(io)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    let mut size_bytes = 0u64;
    loop {
        let n = file.read(&mut buffer).map_err(io)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        size_bytes += n as u64;
    }
    let digest: [u8; 32] = hasher.finalize().into();
    let content_sha256 = digest.iter().fold(String::with_capacity(64), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    });
    Ok(WrittenSegment {
        path: destination.to_path_buf(),
        content_sha256,
        size_bytes,
    })
}

#[cfg(unix)]
fn make_read_only(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o444))
}

#[cfg(not(unix))]
fn make_read_only(path: &Path) -> std::io::Result<()> {
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(path, permissions)
}

/// Removes whatever a failed write left at the destination, which did not exist when
/// the write began.
fn remove_partial(destination: &Path) {
    if let Ok(metadata) = destination.symlink_metadata() {
        let mut permissions = metadata.permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        let _ = std::fs::set_permissions(destination, permissions);
        let _ = std::fs::remove_file(destination);
    }
}

/// Every table's rows, checked and in their key's order.
struct Canonical<'a> {
    meta: Vec<(&'static str, String)>,
    files: Vec<&'a FileRecord>,
    nodes: Vec<&'a Node>,
    sites: Vec<&'a Site>,
    edges: Vec<&'a Edge>,
    evidence: Vec<&'a Evidence>,
    occurrences: Vec<&'a SemanticOccurrence>,
    candidates: Vec<&'a CandidateSite>,
    contracts: Vec<&'a Contract>,
    metrics: Vec<&'a Metric>,
    coverage: Vec<&'a Coverage>,
}

/// `rows` sorted by `key`, refusing two with the same key.
fn sorted<'a, T, K: Ord + std::fmt::Debug>(
    table: &'static str,
    rows: &'a [T],
    key: impl Fn(&T) -> K,
) -> Result<Vec<&'a T>, SegmentError> {
    let mut sorted: Vec<&T> = rows.iter().collect();
    sorted.sort_by_key(|row| key(row));
    if let Some(pair) = sorted.windows(2).find(|pair| key(pair[0]) == key(pair[1])) {
        return Err(SegmentError::DuplicateRow {
            table,
            key: format!("{:?}", key(pair[0])),
        });
    }
    Ok(sorted)
}

/// A contract row as 4.2.1 and 4.7.1 define it, whoever assembled it: the same rules
/// the accumulator applies ([`crate::contracts::row::check`]), against the segment's
/// own repository and nodes.
fn contract_row(
    contract: &Contract,
    repo: &RepoId,
    nodes: &BTreeSet<&NodeId>,
) -> Result<(), SegmentError> {
    crate::contracts::row::check(contract, repo, |id| nodes.contains(id)).map_err(|e| {
        SegmentError::InvalidRow {
            table: "contracts",
            reason: format!("{}: {e}", contract.contract_id),
        }
    })
}

fn finite(table: &'static str, column: &str, value: Option<f64>) -> Result<(), SegmentError> {
    match value {
        Some(v) if !v.is_finite() => Err(SegmentError::InvalidRow {
            table,
            reason: format!("{column} is {v}; only finite numbers are stored"),
        }),
        _ => Ok(()),
    }
}

impl<'a> Canonical<'a> {
    fn of(data: &'a SegmentData) -> Result<Self, SegmentError> {
        let repo = &data.meta.repo_id;
        for file in &data.files {
            let expected = NodeKey::file(repo, &file.path).node_id().map_err(|e| {
                SegmentError::InvalidRow {
                    table: "files",
                    reason: format!("{}: {e}", file.path),
                }
            })?;
            if file.file_id != expected {
                return Err(SegmentError::InvalidRow {
                    table: "files",
                    reason: format!("{} is not the id of the file {}", file.file_id, file.path),
                });
            }
            if file.language != "unknown" && languages::by_id(&file.language).is_none() {
                return Err(SegmentError::InvalidRow {
                    table: "files",
                    reason: format!(
                        "{} is neither a language of the matrix nor unknown",
                        file.language
                    ),
                });
            }
        }
        for edge in &data.edges {
            finite("edges", "engine_score", edge.engine_score)?;
            // 4.2.4: a structural edge references the site that evidences it.
            if edge.site_id.is_none() && edge.kind.requires_site() {
                return Err(SegmentError::InvalidRow {
                    table: "edges",
                    reason: format!(
                        "{} is a {} edge with no evidence site",
                        edge.edge_id, edge.kind
                    ),
                });
            }
        }
        for candidate in &data.candidates {
            finite("candidates", "engine_score", candidate.engine_score)?;
            let unique: BTreeSet<_> = candidate.candidate_ids.iter().collect();
            if unique.len() != candidate.candidate_ids.len() {
                return Err(SegmentError::InvalidRow {
                    table: "candidates",
                    reason: format!("site {} lists a candidate twice", candidate.site_id),
                });
            }
        }
        for metric in &data.metrics {
            finite("metrics", "value", Some(metric.value))?;
        }
        let nodes: BTreeSet<&NodeId> = data.nodes.iter().map(|n| &n.node_id).collect();
        for contract in &data.contracts {
            contract_row(contract, repo, &nodes)?;
        }
        let meta = verify::meta_rows(&data.meta)?;
        Ok(Self {
            meta,
            files: sorted("files", &data.files, |r| r.file_id.clone())?,
            nodes: sorted("nodes", &data.nodes, |r| r.node_id.clone())?,
            sites: sorted("sites", &data.sites, |r| r.site_id.clone())?,
            edges: sorted("edges", &data.edges, |r| r.edge_id.clone())?,
            evidence: sorted("evidence", &data.evidence, |r| r.evidence_id.clone())?,
            occurrences: sorted("semantic_occurrences", &data.semantic_occurrences, |r| {
                r.occ_id.clone()
            })?,
            candidates: sorted("candidates", &data.candidates, |r| r.site_id.clone())?,
            contracts: sorted("contracts", &data.contracts, |r| r.contract_id.clone())?,
            metrics: sorted("metrics", &data.metrics, |r| {
                (r.node_id.clone(), r.metric.clone())
            })?,
            coverage: sorted("coverage", &data.coverage, |r| r.language.clone())?,
        })
    }
}

fn json<T: serde::Serialize>(table: &'static str, value: &T) -> Result<String, SegmentError> {
    serde_json::to_string(value).map_err(|e| SegmentError::InvalidRow {
        table,
        reason: e.to_string(),
    })
}

fn int(table: &'static str, value: u64) -> Result<i64, SegmentError> {
    i64::try_from(value).map_err(|_| SegmentError::InvalidRow {
        table,
        reason: format!("{value} is too large to store"),
    })
}

/// Inserts every table in an order that satisfies the declared references: files
/// before the nodes, sites and occurrences in them, sites before the edges and rows
/// that point at them. One prepared statement per table.
fn insert_all(conn: &Connection, rows: &Canonical<'_>) -> Result<(), SegmentError> {
    insert_meta(conn, rows)?;
    insert_files(conn, rows)?;
    insert_nodes(conn, rows)?;
    insert_sites(conn, rows)?;
    insert_edges(conn, rows)?;
    insert_evidence(conn, rows)?;
    insert_occurrences(conn, rows)?;
    insert_candidates(conn, rows)?;
    insert_contracts(conn, rows)?;
    insert_metrics(conn, rows)?;
    insert_coverage(conn, rows)?;
    Ok(())
}

fn insert_meta(conn: &Connection, rows: &Canonical<'_>) -> Result<(), SegmentError> {
    let mut statement = conn.prepare(queries::INSERT_META)?;
    for (key, value) in &rows.meta {
        statement.execute(params![key, value])?;
    }
    Ok(())
}

fn insert_files(conn: &Connection, rows: &Canonical<'_>) -> Result<(), SegmentError> {
    let mut statement = conn.prepare(queries::INSERT_FILE)?;
    for f in &rows.files {
        statement.execute(params![
            f.file_id.as_str(),
            f.path,
            f.language,
            f.status.as_str(),
            f.status_reason,
            f.blob_sha,
            int("files", f.size_bytes)?,
            f.line_count.map(|n| int("files", n)).transpose()?,
        ])?;
    }
    Ok(())
}

fn insert_nodes(conn: &Connection, rows: &Canonical<'_>) -> Result<(), SegmentError> {
    let mut statement = conn.prepare(queries::INSERT_NODE)?;
    for n in &rows.nodes {
        insert_node(&mut statement, n)?;
    }
    Ok(())
}

fn insert_sites(conn: &Connection, rows: &Canonical<'_>) -> Result<(), SegmentError> {
    let mut statement = conn.prepare(queries::INSERT_SITE)?;
    for s in &rows.sites {
        statement.execute(params![
            s.site_id.as_str(),
            s.file_id.as_str(),
            s.enclosing_node_id.as_ref().map(NodeId::as_str),
            s.site_kind.as_str(),
            s.ast_fingerprint.as_str(),
            s.span.start_byte,
            s.span.end_byte,
            s.span.start_line,
            s.span.start_col,
            s.span.end_line,
            s.span.end_col,
            s.callee_text,
            s.receiver_text,
        ])?;
    }
    Ok(())
}

fn insert_edges(conn: &Connection, rows: &Canonical<'_>) -> Result<(), SegmentError> {
    let mut statement = conn.prepare(queries::INSERT_EDGE)?;
    for e in &rows.edges {
        statement.execute(params![
            e.edge_id.as_str(),
            e.src.as_str(),
            e.dst.as_str(),
            e.kind.as_str(),
            e.band.as_str(),
            i64::from(e.observed),
            e.engine_score,
            e.engine_strategy,
            e.engine_candidates,
            e.site_id.as_ref().map(SiteId::as_str),
            e.weight,
            json("edges", &e.props)?,
        ])?;
    }
    Ok(())
}

fn insert_evidence(conn: &Connection, rows: &Canonical<'_>) -> Result<(), SegmentError> {
    let mut statement = conn.prepare(queries::INSERT_EVIDENCE)?;
    for e in &rows.evidence {
        statement.execute(params![
            e.evidence_id,
            e.fact_kind.as_str(),
            e.fact_id,
            e.provider,
            e.provider_version,
            e.authority.as_str(),
            e.verdict.as_str(),
            e.target_node_id.as_ref().map(NodeId::as_str),
            e.file_id.as_ref().map(NodeId::as_str),
            e.start_byte,
            e.end_byte,
            json("evidence", &e.metadata)?,
        ])?;
    }
    Ok(())
}

fn insert_occurrences(conn: &Connection, rows: &Canonical<'_>) -> Result<(), SegmentError> {
    let mut statement = conn.prepare(queries::INSERT_OCCURRENCE)?;
    for o in &rows.occurrences {
        statement.execute(params![
            o.occ_id,
            o.provider,
            o.provider_symbol,
            o.target_node_id.as_ref().map(NodeId::as_str),
            o.file_id.as_str(),
            o.start_byte,
            o.end_byte,
            o.start_line,
            o.start_col,
            o.role.as_str(),
            o.site_id.as_ref().map(SiteId::as_str),
            o.enclosing_node_id.as_ref().map(NodeId::as_str),
        ])?;
    }
    Ok(())
}

fn insert_candidates(conn: &Connection, rows: &Canonical<'_>) -> Result<(), SegmentError> {
    let mut statement = conn.prepare(queries::INSERT_CANDIDATE)?;
    for c in &rows.candidates {
        // A candidate set is a set: stored sorted, whatever order it was found in.
        let mut ids: Vec<&str> = c.candidate_ids.iter().map(NodeId::as_str).collect();
        ids.sort_unstable();
        statement.execute(params![
            c.site_id.as_str(),
            c.src.as_str(),
            c.callee_name,
            c.band.as_str(),
            json("candidates", &ids)?,
            c.engine_score,
            c.engine_strategy,
            c.engine_candidates,
            c.reason,
        ])?;
    }
    Ok(())
}

fn insert_contracts(conn: &Connection, rows: &Canonical<'_>) -> Result<(), SegmentError> {
    let mut statement = conn.prepare(queries::INSERT_CONTRACT)?;
    for c in &rows.contracts {
        statement.execute(params![
            c.contract_id.as_str(),
            c.kind.as_str(),
            c.key,
            c.namespace_key,
            c.identity_strength.as_str(),
            c.owner_node_id.as_ref().map(NodeId::as_str),
            c.direction.as_str(),
            json("contracts", &c.props)?,
        ])?;
    }
    Ok(())
}

fn insert_metrics(conn: &Connection, rows: &Canonical<'_>) -> Result<(), SegmentError> {
    let mut statement = conn.prepare(queries::INSERT_METRIC)?;
    for m in &rows.metrics {
        statement.execute(params![m.node_id.as_str(), m.metric, m.value])?;
    }
    Ok(())
}

fn insert_coverage(conn: &Connection, rows: &Canonical<'_>) -> Result<(), SegmentError> {
    let mut statement = conn.prepare(queries::INSERT_COVERAGE)?;
    for c in &rows.coverage {
        statement.execute(params![
            c.language,
            int("coverage", c.files)?,
            int("coverage", c.parsed)?,
            int("coverage", c.partial)?,
            int("coverage", c.failed)?,
            int("coverage", c.skipped)?,
            int("coverage", c.symbols)?,
            int("coverage", c.call_sites)?,
            json("coverage", &c.by_band)?,
            int("coverage", c.observed_links)?,
        ])?;
    }
    Ok(())
}

fn insert_node(statement: &mut Statement<'_>, n: &Node) -> Result<(), SegmentError> {
    statement.execute(params![
        n.node_id.as_str(),
        n.kind.as_str(),
        n.name,
        n.qualified_name,
        n.file_id.as_ref().map(NodeId::as_str),
        n.parent_id.as_ref().map(NodeId::as_str),
        n.start_line,
        n.end_line,
        n.layer_role.map(LayerRole::as_str),
        n.signature,
        n.doc,
        json("nodes", &n.props)?,
    ])?;
    Ok(())
}
