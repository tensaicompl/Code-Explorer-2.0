//! The engine, one per thread.

use std::cell::Cell;
use std::ffi::{CStr, CString};
use std::marker::PhantomData;
use std::ptr::{self, NonNull};

use pdx_engine_sys as sys;

use crate::convert;
use crate::error::{EngineError, check};
use crate::model::{FileExtract, Surface};

/// The environment variable that gives the engine's syntax walk a node budget: a file
/// whose walk would visit more nodes stops early, and its extraction says it was
/// truncated. Unset, there is no budget.
pub const NODE_BUDGET_ENV: &str = "PDX_ENGINE_WALK_MAX_NODES";

/// The engine's environment switches that change what an extraction contains, none
/// of which is set normally: the node budget ([`NODE_BUDGET_ENV`]); the ceiling on the
/// definition walk's stack (`PDX_ENGINE_WALK_DEFS_MAX`); the TypeScript resolver's
/// work budget (`PDX_ENGINE_TS_TYPE_BUDGET`) and walk depth
/// (`PDX_ENGINE_LSP_MAX_WALK_DEPTH`); turning that resolver off
/// (`PDX_ENGINE_LSP_DISABLED`); and a crash-quarantine list, whose files extract as
/// empty (`PDX_ENGINE_INDEX_QUARANTINE_FILE`).
///
/// No extraction cache key names them, and the engine alone interprets their
/// values, so a build with any of them set at all neither reads nor writes an
/// extraction cache (docs/plan/ISSUES.md, issue 26). Every other variable the engine
/// reads leaves extraction as it is, or exists only in test builds; a test holds the
/// engine's sources to that.
pub const EXTRACTION_SWITCHES: [&str; 6] = [
    NODE_BUDGET_ENV,
    "PDX_ENGINE_WALK_DEFS_MAX",
    "PDX_ENGINE_TS_TYPE_BUDGET",
    "PDX_ENGINE_LSP_MAX_WALK_DEPTH",
    "PDX_ENGINE_LSP_DISABLED",
    "PDX_ENGINE_INDEX_QUARANTINE_FILE",
];

/// The first of [`EXTRACTION_SWITCHES`] set in the environment, whatever its value,
/// empty included.
pub fn extraction_switch_set() -> Option<&'static str> {
    EXTRACTION_SWITCHES
        .into_iter()
        .find(|name| std::env::var_os(name).is_some())
}

thread_local! {
    /// Whether this thread has an engine. The engine keeps parser state per thread.
    static ENGINE_ON_THREAD: Cell<bool> = const { Cell::new(false) };
}

/// The extraction engine, for the thread that created it.
///
/// The engine keeps its parser state in thread-local storage, so an `Engine` is
/// neither `Send` nor `Sync`: it cannot be moved to another thread or shared with one.
/// Parallelism belongs to the caller, one engine per thread, and a thread can have only
/// one at a time. Dropping it shuts it down.
///
/// Nothing it returns borrows from it: extractions are copied out whole, and the
/// engine's own result is freed before [`Engine::extract`] returns.
pub struct Engine {
    ctx: NonNull<sys::pdxe_ctx>,
    /// Engine objects alive on behalf of this engine: results and projects not yet
    /// freed. For [`Engine::live_handles`].
    live: Cell<usize>,
    /// Not `Send`, not `Sync`: the context is the creating thread's.
    _thread_bound: PhantomData<*mut ()>,
}

impl Engine {
    /// Starts the engine on this thread.
    ///
    /// # Errors
    ///
    /// [`EngineError::EngineAlreadyOnThread`] when this thread already has one, and
    /// the engine's own failure when it cannot start.
    pub fn new() -> Result<Self, EngineError> {
        if ENGINE_ON_THREAD.get() {
            return Err(EngineError::EngineAlreadyOnThread);
        }
        let mut ctx = ptr::null_mut();
        // SAFETY: `ctx` is a valid place for the engine to write a context pointer.
        check(unsafe { sys::pdxe_init(&raw mut ctx) }, "pdxe_init", None)?;
        let ctx = NonNull::new(ctx)
            .ok_or_else(|| EngineError::Contract("pdxe_init returned no context".into()))?;
        ENGINE_ON_THREAD.set(true);
        Ok(Self {
            ctx,
            live: Cell::new(0),
            _thread_bound: PhantomData,
        })
    }

    /// The engine's version, as extraction caches key on it.
    pub fn version() -> String {
        // SAFETY: the version is a static NUL-terminated string, never NULL.
        unsafe { CStr::from_ptr(sys::pdxe_version()) }
            .to_string_lossy()
            .into_owned()
    }

    /// Whether the engine parses `language`, an engine language: a language matrix
    /// identifier, or a grammar the engine names otherwise than its language (`tsx`).
    pub fn knows_language(language: &str) -> bool {
        language_id(language).is_some()
    }

    /// Extracts one file.
    ///
    /// `rel_path` is the file's path relative to the repository, which qualified names
    /// are built from. The result is complete and owned, surface included.
    ///
    /// # Errors
    ///
    /// The engine's failure, an unknown language, or a path containing a NUL byte.
    pub fn extract(
        &self,
        language: &str,
        rel_path: &str,
        source: &[u8],
    ) -> Result<FileExtract, EngineError> {
        let result = self.extract_raw(language, rel_path, source)?;
        result.to_extract(language, rel_path, source)
    }

    /// Engine objects currently alive on behalf of this engine: extraction results and
    /// projects not yet freed. Zero whenever nothing from it is in use; a value that
    /// stays above zero is a leak.
    pub fn live_handles(&self) -> usize {
        self.live.get()
    }

    pub(crate) fn ctx(&self) -> *mut sys::pdxe_ctx {
        self.ctx.as_ptr()
    }

    pub(crate) fn handle_opened(&self) {
        self.live.set(self.live.get() + 1);
    }

    pub(crate) fn handle_closed(&self) {
        self.live.set(self.live.get() - 1);
    }

    /// Extracts one file and keeps the engine's own result, for a project to resolve
    /// through.
    pub(crate) fn extract_raw(
        &self,
        language: &str,
        rel_path: &str,
        source: &[u8],
    ) -> Result<RawResult<'_>, EngineError> {
        let lang = language_id(language)
            .ok_or_else(|| EngineError::UnknownLanguage(language.to_owned()))?;
        let path = c_string(rel_path, "a path")?;
        let mut out = ptr::null_mut();
        // SAFETY: every pointer is valid for the call; the engine copies the source.
        let status = unsafe {
            sys::pdxe_extract_file(
                self.ctx(),
                lang,
                path.as_ptr(),
                source.as_ptr(),
                source.len(),
                &raw mut out,
            )
        };
        check(status, "pdxe_extract_file", Some(language))?;
        RawResult::adopt(self, out)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: the context came from pdxe_init and is shut down once. Nothing of it
        // is still in use: results and projects borrow the engine, so they are gone.
        unsafe { sys::pdxe_shutdown(self.ctx.as_ptr()) };
        ENGINE_ON_THREAD.set(false);
    }
}

/// The engine's number for a language, if it parses it.
pub(crate) fn language_id(language: &str) -> Option<i32> {
    let name = CString::new(language).ok()?;
    // SAFETY: `name` is a valid C string for the call.
    let id = unsafe { sys::pdxe_language_id(name.as_ptr()) };
    (id > 0).then_some(id)
}

/// `s` as a C string, refusing an interior NUL.
pub(crate) fn c_string(s: &str, what: &str) -> Result<CString, EngineError> {
    CString::new(s).map_err(|_| EngineError::InvalidArgument(format!("{what} contains a NUL byte")))
}

/// An extraction result the engine owns, freed when this is dropped.
pub(crate) struct RawResult<'e> {
    engine: &'e Engine,
    ptr: NonNull<sys::pdxe_file_result>,
}

impl<'e> RawResult<'e> {
    /// Takes ownership of a result the engine returned.
    pub(crate) fn adopt(
        engine: &'e Engine,
        ptr: *mut sys::pdxe_file_result,
    ) -> Result<Self, EngineError> {
        let ptr = NonNull::new(ptr)
            .ok_or_else(|| EngineError::Contract("the engine returned no result".into()))?;
        engine.handle_opened();
        Ok(Self { engine, ptr })
    }

    pub(crate) fn as_ptr(&self) -> *const sys::pdxe_file_result {
        self.ptr.as_ptr()
    }

    /// The result's surface, copied.
    pub(crate) fn surface(&self, rel_path: &str) -> Result<Surface, EngineError> {
        let path = c_string(rel_path, "a path")?;
        let mut bytes = ptr::null_mut();
        let mut len = 0usize;
        // SAFETY: the result is alive and in no project; the out pointers are valid.
        let status = unsafe {
            sys::pdxe_surface_export(self.as_ptr(), path.as_ptr(), &raw mut bytes, &raw mut len)
        };
        check(status, "pdxe_surface_export", None)?;
        if bytes.is_null() {
            return Err(EngineError::Contract(
                "the engine exported no surface".into(),
            ));
        }
        // SAFETY: the engine returned `len` bytes at `bytes`, ours until freed below.
        let copy = unsafe { std::slice::from_raw_parts(bytes, len) }.to_vec();
        // SAFETY: the bytes came from pdxe_surface_export and are freed once.
        unsafe { sys::pdxe_surface_free(bytes) };
        Ok(Surface::new(copy))
    }

    /// The whole result, surface included, as an owned extraction. `source` is the
    /// exact bytes the result was extracted from, which the extraction is identified by.
    pub(crate) fn to_extract(
        &self,
        language: &str,
        rel_path: &str,
        source: &[u8],
    ) -> Result<FileExtract, EngineError> {
        let surface = self.surface(rel_path)?;
        // SAFETY: the result is alive until self is dropped, after this returns.
        unsafe { convert::file_extract(self.ptr.as_ref(), language, rel_path, source, surface) }
    }
}

impl Drop for RawResult<'_> {
    fn drop(&mut self) {
        // SAFETY: the result came from the engine and is freed once, by its engine.
        unsafe { sys::pdxe_result_free(self.engine.ctx(), self.ptr.as_ptr()) };
        self.engine.handle_closed();
    }
}
