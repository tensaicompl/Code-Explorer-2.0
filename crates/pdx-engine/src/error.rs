//! What can go wrong, and how the engine's status codes map to it.

use serde::{Deserialize, Serialize};

use pdx_engine_sys as sys;

/// An engine failure.
///
/// The first six mirror the status codes of the engine's interface; the rest are
/// failures this wrapper detects itself. Serialisable, so an isolated worker can
/// report one to its parent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[non_exhaustive]
pub enum EngineError {
    /// The engine could not allocate memory (`PDXE_E_NOMEM`).
    #[error("the engine ran out of memory")]
    OutOfMemory,
    /// The language is not one this engine parses (`PDXE_E_LANG`).
    #[error("language `{0}` is not known to the engine")]
    UnknownLanguage(String),
    /// The input exceeds what the engine will parse (`PDXE_E_TOOLARGE`).
    #[error("the input is larger than the engine will parse")]
    TooLarge,
    /// The engine refused an argument or a call out of order (`PDXE_E_INVALID`). Says
    /// which call refused it.
    #[error("the engine refused {0}")]
    Invalid(String),
    /// The engine failed in a way the caller cannot act on (`PDXE_E_INTERNAL`).
    #[error("the engine failed internally")]
    Internal,
    /// A status code the interface does not define.
    #[error("the engine returned status {0}, which its interface does not define")]
    UnknownStatus(i32),
    /// The engine broke its interface's contract: a value the contract rules out, such
    /// as a missing required string or an index past its array. A defect in the
    /// interface layer, never a property of the input.
    #[error("the engine broke its interface contract: {0}")]
    Contract(String),
    /// An argument the engine cannot be given at all, such as a path containing a NUL
    /// byte.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    /// This thread already has an engine; the engine allows one per thread.
    #[error("this thread already has an engine")]
    EngineAlreadyOnThread,
    /// A source handed to resolution is not the one the extraction was taken from.
    #[error("the source of `{rel_path}` is {actual} bytes, but it was extracted from {expected}")]
    SourceMismatch {
        /// The file's path.
        rel_path: String,
        /// The length of the source it was extracted from.
        expected: u64,
        /// The length of the source given.
        actual: u64,
    },
}

/// `Ok` for `PDXE_OK`, the matching error otherwise. `call` names the call that
/// returned the status; `language` is the language it was given, if any, for the one
/// status that is about a language.
pub(crate) fn check(status: i32, call: &str, language: Option<&str>) -> Result<(), EngineError> {
    Err(match status {
        sys::PDXE_OK => return Ok(()),
        sys::PDXE_E_NOMEM => EngineError::OutOfMemory,
        sys::PDXE_E_LANG => EngineError::UnknownLanguage(language.unwrap_or_default().to_owned()),
        sys::PDXE_E_TOOLARGE => EngineError::TooLarge,
        sys::PDXE_E_INVALID => EngineError::Invalid(call.to_owned()),
        sys::PDXE_E_INTERNAL => EngineError::Internal,
        other => EngineError::UnknownStatus(other),
    })
}
