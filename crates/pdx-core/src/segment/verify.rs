//! What must hold of a segment before anything reads it: its meta table is complete
//! and well formed, its schema is the version this build reads, and every declared
//! reference resolves. The writer checks a build before publishing it; the reader
//! checks a segment before returning it.

use rusqlite::Connection;

use super::queries;
use super::{PreciseSource, SegmentError, SegmentMeta, SegmentProfile};
use crate::consts::SEGMENT_SCHEMA_VERSION;
use crate::ids::RepoId;

/// Every meta key a segment has, sorted: the order the writer stores them in.
pub(crate) const META_KEYS: [&str; 9] = [
    "commit_sha",
    "engine_version",
    "language_matrix_version",
    "pdx_version",
    "precise_sources",
    "profile",
    "repo_id",
    "repo_url",
    "schema_version",
];

/// Lists every row whose declared reference does not resolve.
const FOREIGN_KEY_CHECK: &str = "PRAGMA foreign_key_check";

/// The meta table as rows, in key order, as the writer stores it.
///
/// # Errors
///
/// [`SegmentError::InvalidRow`] if the meta cannot be stored: another schema version,
/// precise sources on a structural segment, or a malformed commit.
pub(crate) fn meta_rows(meta: &SegmentMeta) -> Result<Vec<(&'static str, String)>, SegmentError> {
    let invalid = |reason: String| SegmentError::InvalidRow {
        table: "meta",
        reason,
    };
    if meta.schema_version != SEGMENT_SCHEMA_VERSION {
        return Err(invalid(format!(
            "this build writes schema version {SEGMENT_SCHEMA_VERSION}, not {}",
            meta.schema_version
        )));
    }
    let mut sources = meta.precise_sources.clone();
    sources.sort();
    check_meta_fields(
        meta.profile,
        &sources,
        &meta.commit_sha,
        &meta.repo_url,
        &meta.pdx_version,
    )
    .map_err(|(_, reason)| invalid(reason))?;
    let sources = serde_json::to_string(&sources).map_err(|e| invalid(e.to_string()))?;
    let rows = vec![
        ("commit_sha", meta.commit_sha.clone()),
        ("engine_version", meta.engine_version.to_string()),
        (
            "language_matrix_version",
            meta.language_matrix_version.to_string(),
        ),
        ("pdx_version", meta.pdx_version.clone()),
        ("precise_sources", sources),
        ("profile", meta.profile.as_str().to_owned()),
        ("repo_id", meta.repo_id.as_str().to_owned()),
        ("repo_url", meta.repo_url.clone()),
        ("schema_version", meta.schema_version.to_string()),
    ];
    debug_assert!(rows.iter().map(|(k, _)| *k).eq(META_KEYS));
    Ok(rows)
}

/// What every meta value must be beyond its own syntax; `Err((key, reason))`.
fn check_meta_fields(
    profile: SegmentProfile,
    sorted_sources: &[PreciseSource],
    commit_sha: &str,
    repo_url: &str,
    pdx_version: &str,
) -> Result<(), (&'static str, String)> {
    if profile == SegmentProfile::Structural && !sorted_sources.is_empty() {
        return Err((
            "precise_sources",
            "a structural segment has no precise sources".to_owned(),
        ));
    }
    if sorted_sources.windows(2).any(|w| w[0] == w[1]) {
        return Err((
            "precise_sources",
            "a precise source is listed twice".to_owned(),
        ));
    }
    let hex = commit_sha
        .bytes()
        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if !hex || !matches!(commit_sha.len(), 40 | 64) {
        return Err((
            "commit_sha",
            "a commit is 40 or 64 lower-case hex characters".to_owned(),
        ));
    }
    if repo_url.is_empty() {
        return Err(("repo_url", "empty".to_owned()));
    }
    if pdx_version.is_empty() {
        return Err(("pdx_version", "empty".to_owned()));
    }
    Ok(())
}

/// A version number as the writer stores it: decimal digits, no sign, no leading zero.
fn version(key: &'static str, value: &str) -> Result<u32, SegmentError> {
    let canonical = !value.is_empty()
        && value.bytes().all(|b| b.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'));
    canonical
        .then(|| value.parse::<u32>().ok())
        .flatten()
        .ok_or_else(|| SegmentError::MalformedMeta {
            key: key.to_owned(),
            value: value.to_owned(),
            reason: "not a version number".to_owned(),
        })
}

/// The segment's meta, read and checked: every key present and no other, the schema
/// version this build reads, and every value well formed.
///
/// # Errors
///
/// [`SegmentError::MissingMeta`], [`SegmentError::UnexpectedMeta`],
/// [`SegmentError::WrongSchemaVersion`] or [`SegmentError::MalformedMeta`].
pub(crate) fn read_meta(conn: &Connection) -> Result<SegmentMeta, SegmentError> {
    let mut statement = conn.prepare(queries::META_ALL)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let value = |key: &'static str| -> Result<&str, SegmentError> {
        rows.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .ok_or(SegmentError::MissingMeta(key))
    };
    let malformed = |key: &str, value: &str, reason: String| SegmentError::MalformedMeta {
        key: key.to_owned(),
        value: value.to_owned(),
        reason,
    };

    // The schema version first: nothing else can be trusted under another version.
    let schema_version = version("schema_version", value("schema_version")?)?;
    if schema_version != SEGMENT_SCHEMA_VERSION {
        return Err(SegmentError::WrongSchemaVersion {
            expected: SEGMENT_SCHEMA_VERSION,
            found: schema_version,
        });
    }
    if let Some((key, _)) = rows.iter().find(|(k, _)| !META_KEYS.contains(&k.as_str())) {
        return Err(SegmentError::UnexpectedMeta(key.clone()));
    }

    let repo_id_text = value("repo_id")?;
    let repo_id = RepoId::parse(repo_id_text)
        .map_err(|e| malformed("repo_id", repo_id_text, e.to_string()))?;
    let profile_text = value("profile")?;
    let profile = SegmentProfile::parse(profile_text)
        .ok_or_else(|| malformed("profile", profile_text, "not a profile".to_owned()))?;
    let sources_text = value("precise_sources")?;
    let precise_sources: Vec<PreciseSource> = serde_json::from_str(sources_text)
        .map_err(|e| malformed("precise_sources", sources_text, e.to_string()))?;
    if !precise_sources.is_sorted() {
        return Err(malformed(
            "precise_sources",
            sources_text,
            "not in sorted order".to_owned(),
        ));
    }
    let meta = SegmentMeta {
        schema_version,
        repo_id,
        repo_url: value("repo_url")?.to_owned(),
        commit_sha: value("commit_sha")?.to_owned(),
        profile,
        engine_version: version("engine_version", value("engine_version")?)?,
        pdx_version: value("pdx_version")?.to_owned(),
        language_matrix_version: version(
            "language_matrix_version",
            value("language_matrix_version")?,
        )?,
        precise_sources,
    };
    check_meta_fields(
        meta.profile,
        &meta.precise_sources,
        &meta.commit_sha,
        &meta.repo_url,
        &meta.pdx_version,
    )
    .map_err(|(key, reason)| {
        malformed(
            key,
            rows.iter().find(|(k, _)| k == key).map_or("", |(_, v)| v),
            reason,
        )
    })?;
    Ok(meta)
}

/// Every declared reference resolves.
///
/// # Errors
///
/// [`SegmentError::ForeignKeyViolation`] naming the first broken reference and how many
/// there are.
pub(crate) fn check_foreign_keys(conn: &Connection) -> Result<(), SegmentError> {
    let mut statement = conn.prepare(FOREIGN_KEY_CHECK)?;
    let violations = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    match violations.first() {
        None => Ok(()),
        Some((table, rowid, parent)) => Err(SegmentError::ForeignKeyViolation {
            table: table.clone(),
            rowid: rowid.unwrap_or(0),
            parent: parent.clone(),
            count: violations.len(),
        }),
    }
}

/// Everything a segment must satisfy before it is read.
///
/// # Errors
///
/// As [`read_meta`] and [`check_foreign_keys`].
pub(crate) fn verify(conn: &Connection) -> Result<SegmentMeta, SegmentError> {
    let meta = read_meta(conn)?;
    check_foreign_keys(conn)?;
    Ok(meta)
}
