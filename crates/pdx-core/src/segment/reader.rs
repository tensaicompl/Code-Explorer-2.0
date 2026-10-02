//! Reading a segment: opened read-only and immutable (4.3), verified before use, and
//! queried through typed methods only.
//!
//! Every stored value is decoded strictly: an id, vocabulary value, JSON value or
//! number that is not what its column must hold is an error, never a default.

use std::fmt::Write as _;
use std::path::Path;

use rusqlite::{Connection, OpenFlags, Row, params};

use super::{SegmentError, SegmentMeta, queries, verify};
use crate::bands::{Band, CandidateBand};
use crate::ids::{AstFingerprint, EdgeId, IdError, NodeId, SiteId};
use crate::kinds::{ContractKind, EdgeKind, LayerRole, NodeKind, SiteKind};
use crate::model::{
    Authority, BandCounts, CandidateSite, Contract, ContractDirection, Coverage, Edge, Evidence,
    FactKind, FileRecord, FileStatus, IdentityStrength, Metric, Node, OccurrenceRole, Props,
    SemanticOccurrence, Site, Span, Verdict,
};

/// An open segment.
#[derive(Debug)]
pub struct SegmentReader {
    conn: Connection,
    meta: SegmentMeta,
}

/// One full-text match.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    /// The node.
    pub node: Node,
    /// Its BM25 score: lower is a better match.
    pub score: f64,
}

/// Where a node's source is: what a snippet needs to fetch it. A segment holds no
/// source text; the snippet tool reads it from the repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeLocation {
    /// The node.
    pub node_id: NodeId,
    /// Its file's path, when it is in a file.
    pub path: Option<String>,
    /// Its first line, when it has one.
    pub start_line: Option<u32>,
    /// Its last line, when it has one.
    pub end_line: Option<u32>,
}

impl SegmentReader {
    /// The SQLite URI a segment at `path` is opened with: read-only, immutable.
    ///
    /// # Errors
    ///
    /// [`SegmentError::Io`] if the path cannot be made absolute.
    pub fn uri(path: &Path) -> Result<String, SegmentError> {
        let absolute = std::path::absolute(path).map_err(|source| SegmentError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let text = absolute.to_string_lossy().replace('\\', "/");
        let mut uri = String::from("file:");
        if text.starts_with('/') {
            uri.push_str("//");
        } else {
            // A drive letter: file:///C:/...
            uri.push_str("///");
        }
        for byte in text.bytes() {
            if byte.is_ascii_alphanumeric() || b"/-._~:".contains(&byte) {
                uri.push(char::from(byte));
            } else {
                let _ = write!(uri, "%{byte:02X}");
            }
        }
        uri.push_str("?mode=ro&immutable=1");
        Ok(uri)
    }

    /// Opens the segment at `path`, read-only and immutable, and verifies it.
    ///
    /// # Errors
    ///
    /// [`SegmentError::WrongSchemaVersion`] for another schema version; a meta error for
    /// missing or malformed meta; [`SegmentError::ForeignKeyViolation`] for a broken
    /// reference; SQLite errors for a file that is not a segment.
    pub fn open(path: &Path) -> Result<Self, SegmentError> {
        if !path.is_file() {
            return Err(SegmentError::Io {
                path: path.to_path_buf(),
                source: std::io::Error::from(std::io::ErrorKind::NotFound),
            });
        }
        let conn = Connection::open_with_flags(
            Self::uri(path)?,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_URI
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let meta = verify::verify(&conn)?;
        Ok(Self { conn, meta })
    }

    /// The segment's meta.
    pub fn meta(&self) -> &SegmentMeta {
        &self.meta
    }

    /// The node with this id.
    ///
    /// # Errors
    ///
    /// SQLite errors, or a stored value that is not what its column must hold.
    pub fn node(&self, id: &NodeId) -> Result<Option<Node>, SegmentError> {
        self.one(queries::NODE_BY_ID, id.as_str(), node)
    }

    /// The node's children in the containment tree, by node id.
    ///
    /// # Errors
    ///
    /// As [`SegmentReader::node`].
    pub fn children(&self, parent: &NodeId) -> Result<Vec<Node>, SegmentError> {
        self.all(queries::CHILDREN, params![parent.as_str()], node)
    }

    /// Edges from `src`, by edge id, of the given bands; every band when `bands` is
    /// empty. No band is left out unless asked: drawing is the caller's policy.
    ///
    /// # Errors
    ///
    /// As [`SegmentReader::node`].
    pub fn edges_from(&self, src: &NodeId, bands: &[Band]) -> Result<Vec<Edge>, SegmentError> {
        self.all(
            queries::EDGES_FROM,
            params![src.as_str(), band_filter(bands)],
            edge,
        )
    }

    /// Edges to `dst`, by edge id, of the given bands; every band when `bands` is empty.
    ///
    /// # Errors
    ///
    /// As [`SegmentReader::node`].
    pub fn edges_to(&self, dst: &NodeId, bands: &[Band]) -> Result<Vec<Edge>, SegmentError> {
        self.all(
            queries::EDGES_TO,
            params![dst.as_str(), band_filter(bands)],
            edge,
        )
    }

    /// The non-drawn call sites in `src`, by site id.
    ///
    /// # Errors
    ///
    /// As [`SegmentReader::node`]; a stored band that is drawn is an error.
    pub fn candidates_from(&self, src: &NodeId) -> Result<Vec<CandidateSite>, SegmentError> {
        self.all(queries::CANDIDATES_FROM, params![src.as_str()], candidate)
    }

    /// Contracts of `kind` with `key`, either `None` for any, by contract id.
    ///
    /// # Errors
    ///
    /// As [`SegmentReader::node`].
    pub fn contracts(
        &self,
        kind: Option<ContractKind>,
        key: Option<&str>,
    ) -> Result<Vec<Contract>, SegmentError> {
        self.all(
            queries::CONTRACTS,
            params![kind.map(ContractKind::as_str), key],
            contract,
        )
    }

    /// A node's metrics, by name.
    ///
    /// # Errors
    ///
    /// As [`SegmentReader::node`].
    pub fn metrics(&self, node_id: &NodeId) -> Result<Vec<Metric>, SegmentError> {
        self.all(queries::METRICS_OF, params![node_id.as_str()], metric)
    }

    /// The coverage of one language.
    ///
    /// # Errors
    ///
    /// As [`SegmentReader::node`]; band counts that are not complete are an error.
    pub fn coverage(&self, language: &str) -> Result<Option<Coverage>, SegmentError> {
        self.one(queries::COVERAGE_FOR, language, coverage)
    }

    /// The coverage of every language, by language.
    ///
    /// # Errors
    ///
    /// As [`SegmentReader::coverage`].
    pub fn coverage_all(&self) -> Result<Vec<Coverage>, SegmentError> {
        self.all(queries::COVERAGE_ALL, params![], coverage)
    }

    /// The file with this id.
    ///
    /// # Errors
    ///
    /// As [`SegmentReader::node`].
    pub fn file(&self, id: &NodeId) -> Result<Option<FileRecord>, SegmentError> {
        self.one(queries::FILE_BY_ID, id.as_str(), file)
    }

    /// The file at this path.
    ///
    /// # Errors
    ///
    /// As [`SegmentReader::node`].
    pub fn file_by_path(&self, path: &str) -> Result<Option<FileRecord>, SegmentError> {
        self.one(queries::FILE_BY_PATH, path, file)
    }

    /// The evidence site with this id, with its span.
    ///
    /// # Errors
    ///
    /// As [`SegmentReader::node`].
    pub fn site(&self, id: &SiteId) -> Result<Option<Site>, SegmentError> {
        self.one(queries::SITE_BY_ID, id.as_str(), site)
    }

    /// Where a node's source is: its file's path and its lines.
    ///
    /// # Errors
    ///
    /// As [`SegmentReader::node`].
    pub fn node_location(&self, id: &NodeId) -> Result<Option<NodeLocation>, SegmentError> {
        self.one(queries::NODE_LOCATION, id.as_str(), |row| {
            Ok(NodeLocation {
                node_id: id_at(row, "nodes", "node_id", 0, NodeId::parse)?,
                path: row.get(1)?,
                start_line: opt_u32(row, "nodes", "start_line", 2)?,
                end_line: opt_u32(row, "nodes", "end_line", 3)?,
            })
        })
    }

    /// The verdicts on one fact, by evidence id.
    ///
    /// # Errors
    ///
    /// As [`SegmentReader::node`].
    pub fn evidence_for(
        &self,
        kind: FactKind,
        fact_id: &str,
    ) -> Result<Vec<Evidence>, SegmentError> {
        self.all(
            queries::EVIDENCE_FOR,
            params![kind.as_str(), fact_id],
            evidence,
        )
    }

    /// The compiler occurrences in one file, by occurrence id.
    ///
    /// # Errors
    ///
    /// As [`SegmentReader::node`].
    pub fn occurrences_in_file(
        &self,
        file_id: &NodeId,
    ) -> Result<Vec<SemanticOccurrence>, SegmentError> {
        self.all(
            queries::OCCURRENCES_IN_FILE,
            params![file_id.as_str()],
            occurrence,
        )
    }

    /// Nodes whose name, qualified name or documentation match the FTS5 query, best
    /// first by BM25 score, equal scores by node id; at most `limit`.
    ///
    /// # Errors
    ///
    /// [`SegmentError::InvalidSearch`] for a query that is not valid FTS5 syntax; as
    /// [`SegmentReader::node`] otherwise.
    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<SearchHit>, SegmentError> {
        let invalid = |e: rusqlite::Error| match e {
            rusqlite::Error::SqliteFailure(_, Some(reason)) => SegmentError::InvalidSearch {
                query: query.to_owned(),
                reason,
            },
            other => SegmentError::Sqlite(other),
        };
        let mut statement = self.conn.prepare(queries::SEARCH)?;
        let mut rows = statement.query(params![query, limit]).map_err(invalid)?;
        let mut hits = Vec::new();
        while let Some(row) = rows.next().map_err(invalid)? {
            hits.push(SearchHit {
                node: node(row)?,
                score: row.get(queries::NODE_COLUMN_COUNT)?,
            });
        }
        Ok(hits)
    }

    fn one<T>(
        &self,
        sql: &str,
        key: &str,
        decode: impl FnOnce(&Row<'_>) -> Result<T, SegmentError>,
    ) -> Result<Option<T>, SegmentError> {
        let mut statement = self.conn.prepare(sql)?;
        let mut rows = statement.query(params![key])?;
        match rows.next()? {
            Some(row) => decode(row).map(Some),
            None => Ok(None),
        }
    }

    fn all<T>(
        &self,
        sql: &str,
        args: &[&dyn rusqlite::ToSql],
        decode: impl Fn(&Row<'_>) -> Result<T, SegmentError>,
    ) -> Result<Vec<T>, SegmentError> {
        let mut statement = self.conn.prepare(sql)?;
        let mut rows = statement.query(args)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(decode(row)?);
        }
        Ok(out)
    }
}

/// NULL for every band, or the bands as one JSON array of their names.
fn band_filter(bands: &[Band]) -> Option<String> {
    (!bands.is_empty()).then(|| {
        let names: Vec<&str> = bands.iter().map(|b| b.as_str()).collect();
        serde_json::to_string(&names).expect("names serialise")
    })
}

// --- decoding ----------------------------------------------------------------------

type Table = &'static str;

fn id_at<T>(
    row: &Row<'_>,
    table: Table,
    column: &'static str,
    index: usize,
    parse: fn(&str) -> Result<T, IdError>,
) -> Result<T, SegmentError> {
    let text: String = row.get(index)?;
    parse(&text).map_err(|e| SegmentError::InvalidStoredId {
        table,
        column,
        value: text,
        reason: e.to_string(),
    })
}

fn opt_id_at<T>(
    row: &Row<'_>,
    table: Table,
    column: &'static str,
    index: usize,
    parse: fn(&str) -> Result<T, IdError>,
) -> Result<Option<T>, SegmentError> {
    let text: Option<String> = row.get(index)?;
    text.map(|text| {
        parse(&text).map_err(|e| SegmentError::InvalidStoredId {
            table,
            column,
            value: text,
            reason: e.to_string(),
        })
    })
    .transpose()
}

fn vocab_at<T>(
    row: &Row<'_>,
    table: Table,
    column: &'static str,
    index: usize,
    parse: fn(&str) -> Option<T>,
) -> Result<T, SegmentError> {
    let text: String = row.get(index)?;
    parse(&text).ok_or(SegmentError::InvalidStoredVocabulary {
        table,
        column,
        value: text,
    })
}

fn json_at<T: serde::de::DeserializeOwned>(
    row: &Row<'_>,
    table: Table,
    column: &'static str,
    index: usize,
) -> Result<T, SegmentError> {
    let text: String = row.get(index)?;
    serde_json::from_str(&text).map_err(|e| SegmentError::InvalidStoredJson {
        table,
        column,
        value: text,
        reason: e.to_string(),
    })
}

fn number<T: TryFrom<i64>>(
    table: Table,
    column: &'static str,
    value: i64,
) -> Result<T, SegmentError> {
    T::try_from(value).map_err(|_| SegmentError::InvalidStoredNumber {
        table,
        column,
        value: value.to_string(),
    })
}

fn u32_at(
    row: &Row<'_>,
    table: Table,
    column: &'static str,
    index: usize,
) -> Result<u32, SegmentError> {
    number(table, column, row.get::<_, i64>(index)?)
}

fn opt_u32(
    row: &Row<'_>,
    table: Table,
    column: &'static str,
    index: usize,
) -> Result<Option<u32>, SegmentError> {
    row.get::<_, Option<i64>>(index)?
        .map(|v| number(table, column, v))
        .transpose()
}

/// A count the model requires: NULL is out of range.
fn count_at(
    row: &Row<'_>,
    table: Table,
    column: &'static str,
    index: usize,
) -> Result<u64, SegmentError> {
    match row.get::<_, Option<i64>>(index)? {
        Some(v) => number(table, column, v),
        None => Err(SegmentError::InvalidStoredNumber {
            table,
            column,
            value: "NULL".to_owned(),
        }),
    }
}

fn node(row: &Row<'_>) -> Result<Node, SegmentError> {
    const T: Table = "nodes";
    Ok(Node {
        node_id: id_at(row, T, "node_id", 0, NodeId::parse)?,
        kind: vocab_at(row, T, "kind", 1, NodeKind::parse)?,
        name: row.get(2)?,
        qualified_name: row.get(3)?,
        file_id: opt_id_at(row, T, "file_id", 4, NodeId::parse)?,
        parent_id: opt_id_at(row, T, "parent_id", 5, NodeId::parse)?,
        start_line: opt_u32(row, T, "start_line", 6)?,
        end_line: opt_u32(row, T, "end_line", 7)?,
        layer_role: row
            .get::<_, Option<String>>(8)?
            .map(|text| {
                LayerRole::parse(&text).ok_or(SegmentError::InvalidStoredVocabulary {
                    table: T,
                    column: "layer_role",
                    value: text,
                })
            })
            .transpose()?,
        signature: row.get(9)?,
        doc: row.get(10)?,
        props: json_at::<Props>(row, T, "props", 11)?,
    })
}

fn edge(row: &Row<'_>) -> Result<Edge, SegmentError> {
    const T: Table = "edges";
    let observed: i64 = row.get(5)?;
    Ok(Edge {
        edge_id: id_at(row, T, "edge_id", 0, EdgeId::parse)?,
        src: id_at(row, T, "src", 1, NodeId::parse)?,
        dst: id_at(row, T, "dst", 2, NodeId::parse)?,
        kind: vocab_at(row, T, "kind", 3, EdgeKind::parse)?,
        band: vocab_at(row, T, "band", 4, Band::parse)?,
        observed: match observed {
            0 => false,
            1 => true,
            other => {
                return Err(SegmentError::InvalidStoredNumber {
                    table: T,
                    column: "observed",
                    value: other.to_string(),
                });
            }
        },
        engine_score: row.get(6)?,
        engine_strategy: row.get(7)?,
        engine_candidates: opt_u32(row, T, "engine_candidates", 8)?,
        site_id: opt_id_at(row, T, "site_id", 9, SiteId::parse)?,
        weight: u32_at(row, T, "weight", 10)?,
        props: json_at::<Props>(row, T, "props", 11)?,
    })
}

fn candidate(row: &Row<'_>) -> Result<CandidateSite, SegmentError> {
    const T: Table = "candidates";
    let ids: Vec<String> = json_at(row, T, "candidate_ids", 4)?;
    let candidate_ids = ids
        .into_iter()
        .map(|text| {
            NodeId::parse(&text).map_err(|e| SegmentError::InvalidStoredId {
                table: T,
                column: "candidate_ids",
                value: text,
                reason: e.to_string(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CandidateSite {
        site_id: id_at(row, T, "site_id", 0, SiteId::parse)?,
        src: id_at(row, T, "src", 1, NodeId::parse)?,
        callee_name: row.get(2)?,
        band: vocab_at(row, T, "band", 3, CandidateBand::parse)?,
        candidate_ids,
        engine_score: row.get(5)?,
        engine_strategy: row.get(6)?,
        reason: row.get(7)?,
    })
}

fn contract(row: &Row<'_>) -> Result<Contract, SegmentError> {
    const T: Table = "contracts";
    Ok(Contract {
        contract_id: id_at(row, T, "contract_id", 0, NodeId::parse)?,
        kind: vocab_at(row, T, "kind", 1, ContractKind::parse)?,
        key: row.get(2)?,
        namespace_key: row.get(3)?,
        identity_strength: vocab_at(row, T, "identity_strength", 4, IdentityStrength::parse)?,
        owner_node_id: opt_id_at(row, T, "owner_node_id", 5, NodeId::parse)?,
        direction: vocab_at(row, T, "direction", 6, ContractDirection::parse)?,
        props: json_at::<Props>(row, T, "props", 7)?,
    })
}

fn metric(row: &Row<'_>) -> Result<Metric, SegmentError> {
    Ok(Metric {
        node_id: id_at(row, "metrics", "node_id", 0, NodeId::parse)?,
        metric: row.get(1)?,
        value: row.get(2)?,
    })
}

fn coverage(row: &Row<'_>) -> Result<Coverage, SegmentError> {
    const T: Table = "coverage";
    Ok(Coverage {
        language: row.get(0)?,
        files: count_at(row, T, "files", 1)?,
        parsed: count_at(row, T, "parsed", 2)?,
        partial: count_at(row, T, "partial", 3)?,
        failed: count_at(row, T, "failed", 4)?,
        skipped: count_at(row, T, "skipped", 5)?,
        symbols: count_at(row, T, "symbols", 6)?,
        call_sites: count_at(row, T, "call_sites", 7)?,
        by_band: json_at::<BandCounts>(row, T, "by_band", 8)?,
        observed_links: count_at(row, T, "observed_links", 9)?,
    })
}

fn file(row: &Row<'_>) -> Result<FileRecord, SegmentError> {
    const T: Table = "files";
    Ok(FileRecord {
        file_id: id_at(row, T, "file_id", 0, NodeId::parse)?,
        path: row.get(1)?,
        language: row.get(2)?,
        status: vocab_at(row, T, "status", 3, FileStatus::parse)?,
        status_reason: row.get(4)?,
        blob_sha: row.get(5)?,
        size_bytes: count_at(row, T, "size_bytes", 6)?,
        line_count: count_at(row, T, "line_count", 7)?,
    })
}

fn site(row: &Row<'_>) -> Result<Site, SegmentError> {
    const T: Table = "sites";
    let fingerprint: String = row.get(4)?;
    Ok(Site {
        site_id: id_at(row, T, "site_id", 0, SiteId::parse)?,
        file_id: id_at(row, T, "file_id", 1, NodeId::parse)?,
        enclosing_node_id: opt_id_at(row, T, "enclosing_node_id", 2, NodeId::parse)?,
        site_kind: vocab_at(row, T, "site_kind", 3, SiteKind::parse)?,
        ast_fingerprint: AstFingerprint::try_from(fingerprint.clone()).map_err(|e| {
            SegmentError::InvalidStoredId {
                table: T,
                column: "ast_fingerprint",
                value: fingerprint,
                reason: e.to_string(),
            }
        })?,
        span: Span {
            start_byte: u32_at(row, T, "start_byte", 5)?,
            end_byte: u32_at(row, T, "end_byte", 6)?,
            start_line: u32_at(row, T, "start_line", 7)?,
            start_col: u32_at(row, T, "start_col", 8)?,
            end_line: u32_at(row, T, "end_line", 9)?,
            end_col: u32_at(row, T, "end_col", 10)?,
        },
        callee_text: row.get(11)?,
        receiver_text: row.get(12)?,
    })
}

fn evidence(row: &Row<'_>) -> Result<Evidence, SegmentError> {
    const T: Table = "evidence";
    Ok(Evidence {
        evidence_id: row.get(0)?,
        fact_kind: vocab_at(row, T, "fact_kind", 1, FactKind::parse)?,
        fact_id: row.get(2)?,
        provider: row.get(3)?,
        provider_version: row.get(4)?,
        authority: vocab_at(row, T, "authority", 5, Authority::parse)?,
        verdict: vocab_at(row, T, "verdict", 6, Verdict::parse)?,
        target_node_id: opt_id_at(row, T, "target_node_id", 7, NodeId::parse)?,
        file_id: opt_id_at(row, T, "file_id", 8, NodeId::parse)?,
        start_byte: opt_u32(row, T, "start_byte", 9)?,
        end_byte: opt_u32(row, T, "end_byte", 10)?,
        metadata: json_at::<Props>(row, T, "metadata", 11)?,
    })
}

fn occurrence(row: &Row<'_>) -> Result<SemanticOccurrence, SegmentError> {
    const T: Table = "semantic_occurrences";
    Ok(SemanticOccurrence {
        occ_id: row.get(0)?,
        provider: row.get(1)?,
        provider_symbol: row.get(2)?,
        target_node_id: opt_id_at(row, T, "target_node_id", 3, NodeId::parse)?,
        file_id: id_at(row, T, "file_id", 4, NodeId::parse)?,
        start_byte: u32_at(row, T, "start_byte", 5)?,
        end_byte: u32_at(row, T, "end_byte", 6)?,
        start_line: u32_at(row, T, "start_line", 7)?,
        start_col: u32_at(row, T, "start_col", 8)?,
        role: vocab_at(row, T, "role", 9, OccurrenceRole::parse)?,
        site_id: opt_id_at(row, T, "site_id", 10, SiteId::parse)?,
        enclosing_node_id: opt_id_at(row, T, "enclosing_node_id", 11, NodeId::parse)?,
    })
}
