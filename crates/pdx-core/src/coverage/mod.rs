//! Coverage (4.3's `coverage` table, P2-10): what indexing accounted for, one row per
//! language, so that no file, definition or call site is silently dropped.
//!
//! Each row counts, for the files of one language (an Appendix A id, or `unknown`):
//!
//! - **`files`**: every file of the repository in the language, whatever became of it,
//!   the files held back for a path no identity can be made from included (issue 53).
//! - **`parsed`, `partial`, `failed`, `skipped`**: the files with that status, as Stage
//!   4 records it ([`file_status`]): the engine's own report for an extracted file
//!   (issue 57), `failed` for an engine error or crash, `skipped` for size, memory, no
//!   language or an unportable path. A binary or redacted file is counted in `files`
//!   and in none of these: nothing was attempted (decision 33).
//! - **`symbols`**: the definitions extracted, each once: never the engine's file-level
//!   module (it is the file), and a test callable once, whatever its node's kind.
//! - **`call_sites`** and **`by_band`**: every call site Stage 3 resolved, once, under
//!   the band it resolved to, in its file's language. Stage 3's report is the truth:
//!   a site Stage 4 could not store (no position in the file, no node-type path) is
//!   still a call site, and a site Stage 3 reports as unconfirmed is none. Every row
//!   holds a count for all eleven bands, and they sum to `call_sites`.
//! - **`observed_links`**: 0; observation is the link layer's (4.12.9).
//!
//! A row exists for each language with at least one file, and no other: the matrix's
//! languages the repository does not use have none (decision 33).

use std::collections::BTreeMap;

use crate::index::derive::containment::{file_status, is_file_definition};
use crate::index::discover::DiscoveredFile;
use crate::model::{BandCounts, Coverage, FileStatus};
use crate::resolve::registry::SymbolRegistry;
use crate::resolve::stages::ResolveReport;

/// Why coverage could not be counted.
#[derive(Debug, thiserror::Error)]
pub enum CoverageError {
    /// A call site in a file the registry does not have: Stage 3 and the registry
    /// disagree.
    #[error("{0}: a call site in a file the registry does not have")]
    SiteOutsideRegistry(String),
}

/// A row with every count 0.
fn empty(language: &str) -> Coverage {
    Coverage {
        language: language.to_owned(),
        files: 0,
        parsed: 0,
        partial: 0,
        failed: 0,
        skipped: 0,
        symbols: 0,
        call_sites: 0,
        by_band: BandCounts::default(),
        observed_links: 0,
    }
}

/// The row of a language, made empty when it is first counted.
fn row<'r>(
    rows: &'r mut BTreeMap<&'static str, Coverage>,
    language: &'static str,
) -> &'r mut Coverage {
    rows.entry(language).or_insert_with(|| empty(language))
}

/// The coverage rows of a repository, by language: from the registry's files (Stage
/// 1's and Stage 2's facts), Stage 3's resolutions, and the files held back for an
/// unportable path.
///
/// # Errors
///
/// [`CoverageError::SiteOutsideRegistry`] when a resolution names a file the registry
/// does not have.
pub fn rows(
    registry: &SymbolRegistry,
    resolution: &ResolveReport,
    unportable: &[DiscoveredFile],
) -> Result<Vec<Coverage>, CoverageError> {
    let mut rows: BTreeMap<&'static str, Coverage> = BTreeMap::new();
    for path in registry.files() {
        let Some(discovered) = registry.discovered(path) else {
            continue;
        };
        let r = row(&mut rows, discovered.language_id());
        r.files += 1;
        if let Some(outcome) = registry.outcome(path) {
            match file_status(outcome).0 {
                FileStatus::Parsed => r.parsed += 1,
                FileStatus::Partial => r.partial += 1,
                FileStatus::Failed => r.failed += 1,
                FileStatus::Skipped => r.skipped += 1,
                FileStatus::Binary | FileStatus::Redacted => {}
            }
        }
        if let Some(extract) = registry.extract(path) {
            let symbols = extract
                .definitions
                .iter()
                .filter(|d| !is_file_definition(d, path))
                .count();
            r.symbols += symbols as u64;
        }
    }
    for file in unportable {
        let r = row(&mut rows, file.language_id());
        r.files += 1;
        r.skipped += 1;
    }
    for site in &resolution.resolutions {
        let path = site.site_ref.rel_path.as_str();
        let discovered = registry
            .discovered(path)
            .ok_or_else(|| CoverageError::SiteOutsideRegistry(path.to_owned()))?;
        let r = row(&mut rows, discovered.language_id());
        r.call_sites += 1;
        r.by_band.add(site.resolution.band(), 1);
    }
    Ok(rows.into_values().collect())
}
