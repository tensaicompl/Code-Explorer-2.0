//! Writing a segment (4.3): a temporary build, then `VACUUM INTO` a staged file beside
//! the destination that is made durable and hashed, given the destination's name only
//! if nothing has it, and made read-only.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Read};
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
/// A segment is built in a temporary directory, by default one inside the destination's
/// own directory (the system's may be memory), and only its finished form reaches the
/// destination. Where the build happens never affects the bytes written.
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

    /// Whether a segment could be written at `destination` now: nothing is there, its
    /// directory exists and its path is UTF-8. [`SegmentWriter::write`] checks the same
    /// again before it writes; this lets a caller refuse before any work is done.
    ///
    /// # Errors
    ///
    /// [`SegmentError::DestinationExists`] or [`SegmentError::InvalidDestination`], as
    /// [`SegmentWriter::write`] would report them.
    pub fn check_destination(destination: &Path) -> Result<(), SegmentError> {
        let destination = std::path::absolute(destination).map_err(|source| SegmentError::Io {
            path: destination.to_path_buf(),
            source,
        })?;
        check_destination(&destination)
    }

    /// Writes `data` as a segment at `destination`, which must not exist.
    ///
    /// In order: a temporary database, its settings, the schema, every table's rows in
    /// their key's order in one transaction, the full-text index, a check of the
    /// build, `VACUUM INTO` a staged file of its own beside the destination, every
    /// connection closed, the staged file synced to storage and its bytes hashed, then
    /// given the destination's name by a hard link, which never replaces a file (a
    /// rename, after one more look, where the file system has no hard links), made
    /// read-only (mode 0444 on Unix), and its directory synced. A destination that
    /// appears meanwhile is [`SegmentError::DestinationExists`] and is never touched: if
    /// any step fails, nothing this write did not create is removed, and nothing it
    /// created is left.
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
        let Some(directory) = destination.parent() else {
            return Err(SegmentError::InvalidDestination {
                path: destination.clone(),
                reason: "its directory does not exist".to_owned(),
            });
        };
        let io = |path: &Path| {
            let path = path.to_path_buf();
            move |source| SegmentError::Io { path, source }
        };

        let build_root = self.temp_root.as_deref().unwrap_or(directory);
        let temp = tempfile::Builder::new()
            .prefix(".pdx-segment-")
            .tempdir_in(build_root)
            .map_err(io(build_root))?;
        // Created empty, which `VACUUM INTO` writes into; removed when dropped unless it
        // has become the destination.
        let staged = tempfile::Builder::new()
            .prefix(".pdx-segment-")
            .suffix(".partial")
            .tempfile_in(directory)
            .map_err(io(directory))?
            .into_temp_path();
        build(&rows, &temp.path().join("build.db"), &staged)?;
        drop(temp);
        publish(staged, &destination)
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

/// Builds the segment at `build_path` and writes it to `destination`, an empty file,
/// with `VACUUM INTO`. Every connection is closed when this returns.
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
    let destination = destination
        .to_str()
        .ok_or_else(|| SegmentError::InvalidDestination {
            path: destination.to_path_buf(),
            reason: "the path is not UTF-8".to_owned(),
        })?;
    conn.execute(queries::VACUUM_INTO, [destination])?;
    conn.close().map_err(|(_, e)| SegmentError::Sqlite(e))?;
    Ok(())
}

/// Syncs and hashes the staged file, gives it the destination's name without ever
/// replacing a file there, and makes it read-only.
fn publish(staged: tempfile::TempPath, destination: &Path) -> Result<WrittenSegment, SegmentError> {
    let (content_sha256, size_bytes) = sync_and_hash(&staged)?;
    link_no_clobber(&staged, destination)?;
    // The staged name goes; the destination's stays. A rename has already moved it.
    let _ = staged.close();
    if let Err(source) = make_read_only(destination) {
        // Linked by this write a moment ago: the destination is this write's own.
        let _ = std::fs::remove_file(destination);
        return Err(SegmentError::Io {
            path: destination.to_path_buf(),
            source,
        });
    }
    sync_directory(destination);
    Ok(WrittenSegment {
        path: destination.to_path_buf(),
        content_sha256,
        size_bytes,
    })
}

/// Syncs `path`'s bytes to storage and returns their `lowerhex(sha256)` and length.
fn sync_and_hash(path: &Path) -> Result<(String, u64), SegmentError> {
    let io = |source| SegmentError::Io {
        path: path.to_path_buf(),
        source,
    };
    // Write access, which syncing a file takes on Windows.
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(io)?;
    file.sync_all().map_err(io)?;
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
    Ok((content_sha256, size_bytes))
}

/// Gives `from` the name `to` too, failing if anything has that name: a hard link
/// never replaces a file. A file system without hard links gets a rename once nothing
/// is seen at `to`, the most it offers.
fn link_no_clobber(from: &Path, to: &Path) -> Result<(), SegmentError> {
    let exists = || SegmentError::DestinationExists(to.to_path_buf());
    let io = |source| SegmentError::Io {
        path: to.to_path_buf(),
        source,
    };
    match std::fs::hard_link(from, to) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Err(exists()),
        Err(_) if to.symlink_metadata().is_ok() => Err(exists()),
        Err(_) => std::fs::rename(from, to).map_err(io),
    }
}

/// Syncs the directory holding `path`, so that its new name survives a crash. Only
/// Unix opens a directory to sync it; a failure leaves a name the next sync keeps.
fn sync_directory(path: &Path) {
    #[cfg(unix)]
    if let Some(directory) = path.parent()
        && let Ok(directory) = std::fs::File::open(directory)
    {
        let _ = directory.sync_all();
    }
    #[cfg(not(unix))]
    let _ = path;
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

/// A coverage row as 4.3 defines it: a language of the matrix or `unknown`, bands
/// that sum to its call sites, and no more files with a status than files.
fn coverage_row(row: &Coverage) -> Result<(), SegmentError> {
    let invalid = |reason: String| SegmentError::InvalidRow {
        table: "coverage",
        reason: format!("{}: {reason}", row.language),
    };
    if row.language != "unknown" && languages::by_id(&row.language).is_none() {
        return Err(invalid(
            "neither a language of the matrix nor unknown".to_owned(),
        ));
    }
    if row.by_band.total() != row.call_sites {
        return Err(invalid(format!(
            "its bands sum to {}, not its {} call sites",
            row.by_band.total(),
            row.call_sites
        )));
    }
    let with_status = [row.parsed, row.partial, row.failed, row.skipped]
        .iter()
        .try_fold(0u64, |sum, n| sum.checked_add(*n));
    if with_status.is_none_or(|n| n > row.files) {
        return Err(invalid(format!(
            "more files parsed, partial, failed or skipped than its {} files",
            row.files
        )));
    }
    Ok(())
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
        for row in &data.coverage {
            coverage_row(row)?;
        }
        let nodes: BTreeSet<&NodeId> = data.nodes.iter().map(|n| &n.node_id).collect();
        for contract in &data.contracts {
            contract_row(contract, repo, &nodes)?;
        }
        let meta = verify::meta_rows(&data.meta)?;
        Ok(Self {
            meta,
            files: sorted("files", &data.files, |r| r.file_id)?,
            nodes: sorted("nodes", &data.nodes, |r| r.node_id)?,
            sites: sorted("sites", &data.sites, |r| r.site_id)?,
            edges: sorted("edges", &data.edges, |r| r.edge_id)?,
            evidence: sorted("evidence", &data.evidence, |r| r.evidence_id.clone())?,
            occurrences: sorted("semantic_occurrences", &data.semantic_occurrences, |r| {
                r.occ_id.clone()
            })?,
            candidates: sorted("candidates", &data.candidates, |r| r.site_id)?,
            contracts: sorted("contracts", &data.contracts, |r| r.contract_id)?,
            metrics: sorted("metrics", &data.metrics, |r| (r.node_id, r.metric.clone()))?,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A staged file holding `bytes`, in `dir`.
    fn staged(dir: &Path, bytes: &[u8]) -> tempfile::TempPath {
        let file = tempfile::Builder::new()
            .prefix(".pdx-segment-")
            .suffix(".partial")
            .tempfile_in(dir)
            .expect("a staged file");
        std::fs::write(file.path(), bytes).expect("written");
        file.into_temp_path()
    }

    #[test]
    fn publishing_never_replaces_a_file_that_appeared_meanwhile() {
        // Another run's segment reaches the destination after the check and before the
        // publication: it is refused, left exactly as it was, and the staged file goes.
        let dir = tempfile::tempdir().expect("a directory");
        let destination = dir.path().join("segment.db");
        let ours = staged(dir.path(), b"ours");
        let ours_path = ours.to_path_buf();
        std::fs::write(&destination, b"theirs").expect("written");
        let refused = publish(ours, &destination);
        assert!(matches!(refused, Err(SegmentError::DestinationExists(_))));
        assert_eq!(std::fs::read(&destination).expect("theirs"), b"theirs");
        assert!(!ours_path.exists(), "the staged file is removed");
    }

    #[test]
    fn publishing_gives_the_staged_bytes_the_destination_name_only() {
        let dir = tempfile::tempdir().expect("a directory");
        let destination = dir.path().join("segment.db");
        let ours = staged(dir.path(), b"segment bytes");
        let ours_path = ours.to_path_buf();
        let written = publish(ours, &destination).expect("published");
        assert_eq!(std::fs::read(&destination).expect("ours"), b"segment bytes");
        assert_eq!(written.size_bytes, 13);
        // sha256sum of the 13 bytes, computed independently.
        assert_eq!(
            written.content_sha256,
            "560e67b96c49975a2cf9adba2682e00cc6b74a879f2b6897e03b63d7434098e8"
        );
        assert!(
            destination
                .metadata()
                .expect("metadata")
                .permissions()
                .readonly()
        );
        assert!(!ours_path.exists());
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .expect("a listing")
            .map(|e| e.expect("an entry").file_name())
            .collect();
        assert_eq!(names, ["segment.db"]);
    }
}
