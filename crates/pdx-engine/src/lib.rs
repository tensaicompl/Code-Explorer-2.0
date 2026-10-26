//! Safe wrapper over the extraction engine, with crash isolation.
//!
//! Everything above this crate reaches the engine through it, without `unsafe`:
//!
//! - [`Engine`] extracts a file into a [`FileExtract`], which is owned, complete, bound to
//!   the source it was taken from, and carries the file's resolution surface; its
//!   [`ExtractRecord`], which holds text as ranges of that source and no source, is what
//!   is stored and sent. One engine per thread; it cannot leave the thread that made it.
//! - [`ProjectResolver`] resolves calls across the files of a project, from fresh
//!   extractions or from cached ones, and returns each answer as an owned
//!   [`TypedResolution`] together with the run's [`RunHealth`]: whether typed
//!   resolution did all its work, so that a degraded run is never read as a clean
//!   one with fewer answers.
//! - [`isolate`] runs extraction in a child process when `PDX_ENGINE_ISOLATE=1`, so
//!   that the engine crashing costs only the file it crashed on.
//!
//! Nothing returned borrows from the engine: every string and array is copied out
//! before the engine's own copy is freed, and the engine objects a project depends on
//! are owned by the project.

mod ast_path;
mod convert;
mod engine;
mod error;
pub mod isolate;
mod model;
mod resolver;

pub use ast_path::{AstPath, MAX_INTERNED_NODE_TYPES};
pub use engine::{EXTRACTION_SWITCHES, Engine, NODE_BUDGET_ENV, extraction_switch_set};
pub use error::{EngineError, SourceDifference};
pub use model::{
    BindError, Call, CallArg, Channel, ChannelDirection, Definition, DefinitionKind, Diagnostic,
    EnvAccess, ExtractRecord, FileExtract, FileStatus, ImplTrait, Import, LexicalFacts,
    MacroMember, NamespaceEvidence, QualifiedName, QualifiedNames, ReadWrite, RouteFact,
    ScopedImport, SourceDigest, Span, Surface, TextRef, Throw, TypeRef, Usage, UseScope,
    Visibility, namespace_evidence,
};
pub use resolver::{
    AliasScope, CrateDependency, CrateManifest, PackageEntry, PathAlias, ProjectResolution,
    ProjectResolver, ResolutionMetadata, RunHealth, RunStatus, SiteRef, Strategy, TypedResolution,
};

/// Gives the memory the process has freed back to the operating system (P2-17).
///
/// With glibc, memory freed on a thread stays in that thread's allocation arena: Stage 2's
/// extraction workers, and the engine they run, free nearly everything they allocate,
/// and the arenas kept it, more than a third of the build's resident memory on a cold
/// extraction. A build calls this between its stages. It changes no result; elsewhere
/// it does nothing.
pub fn release_freed_memory() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        unsafe extern "C" {
            fn malloc_trim(pad: usize) -> std::ffi::c_int;
        }
        // SAFETY: glibc's `malloc_trim` takes no pointer and only returns free memory.
        unsafe {
            malloc_trim(0);
        }
    }
}
