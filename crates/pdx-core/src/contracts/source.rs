//! Reading repository files for contracts, safely: the same checks and bound as every
//! other read of the checkout, and the same bytes the graph was derived from.
//!
//! A file is read only if discovery found it a candidate: a redacted file (every
//! `.env*` among them, issue 38), a binary or oversized one, and one skipped for memory
//! is never opened. A file Stage 2 extracted is read again through [`prepare_source`]
//! and must still be the bytes Stage 2 identified, its `blob_sha` and the extraction's
//! digest both; one the engine failed on must still have its `blob_sha`; one of no
//! language through [`prepare_candidate`]. Either way the text parsed is the
//! normalised source, never the original, so a secret value reaches no contract.
//! Nothing is fetched, installed or run.

use std::path::Path;

use crate::index::derive::DeriveError;
use crate::index::discover::Disposition;
use crate::index::extract::{FileOutcome, prepare_candidate, prepare_source};
use crate::resolve::registry::SymbolRegistry;

/// What a read found.
pub(crate) enum Read {
    /// The file's normalised text.
    Text(String),
    /// The file was read and is not UTF-8: no document of a contract format.
    NotUtf8,
    /// The file is not one to read: absent, not a candidate, or skipped for memory.
    Unread,
}

/// A repository file's normalised text, read safely.
///
/// # Errors
///
/// [`DeriveError::SourceRead`] when the file is no longer what discovery found (the
/// checkout changed, a symlink appeared, the size changed) or cannot be read, and
/// [`DeriveError::SourceChanged`] when its bytes are not the ones Stage 2 identified.
pub(crate) fn read(
    root: &Path,
    registry: &SymbolRegistry,
    path: &str,
) -> Result<Read, DeriveError> {
    let (Some(discovered), Some(outcome)) = (registry.discovered(path), registry.outcome(path))
    else {
        return Ok(Read::Unread);
    };
    if discovered.disposition != Disposition::Candidate {
        return Ok(Read::Unread);
    }
    let read_error = |source| DeriveError::SourceRead {
        path: path.to_owned(),
        source: Box::new(source),
    };
    let prepared = match outcome {
        FileOutcome::Extracted { extract, blob_sha } => {
            let prepared = prepare_source(root, discovered).map_err(read_error)?;
            if prepared.blob_sha != *blob_sha || prepared.digest != extract.source_digest {
                return Err(DeriveError::SourceChanged(path.to_owned()));
            }
            prepared
        }
        FileOutcome::EngineFailed { blob_sha, .. } | FileOutcome::EngineCrashed { blob_sha } => {
            let prepared = prepare_source(root, discovered).map_err(read_error)?;
            if prepared.blob_sha != *blob_sha {
                return Err(DeriveError::SourceChanged(path.to_owned()));
            }
            prepared
        }
        FileOutcome::UnknownLanguage => prepare_candidate(root, discovered).map_err(read_error)?,
        // Skipped for memory, redacted, binary or too large: never opened.
        FileOutcome::SkippedMemory
        | FileOutcome::Redacted
        | FileOutcome::Binary
        | FileOutcome::SkippedSize => return Ok(Read::Unread),
    };
    Ok(String::from_utf8(prepared.bytes).map_or(Read::NotUtf8, Read::Text))
}
