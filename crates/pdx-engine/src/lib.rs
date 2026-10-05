//! Safe wrapper over the extraction engine, with crash isolation.
//!
//! Everything above this crate reaches the engine through it, without `unsafe`:
//!
//! - [`Engine`] extracts a file into a [`FileExtract`], which is owned, complete,
//!   serialisable, and carries the file's resolution surface. One engine per thread;
//!   it cannot leave the thread that made it.
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

mod convert;
mod engine;
mod error;
pub mod isolate;
mod model;
mod resolver;

pub use engine::{EXTRACTION_SWITCHES, Engine, NODE_BUDGET_ENV, extraction_switch_set};
pub use error::{EngineError, SourceDifference};
pub use model::{
    Call, CallArg, Channel, ChannelDirection, Definition, DefinitionKind, Diagnostic, EnvAccess,
    FileExtract, FileStatus, ImplTrait, Import, LexicalFacts, NamespaceEvidence, ReadWrite,
    SourceDigest, Span, Surface, Throw, TypeRef, Usage, Visibility, namespace_evidence,
};
pub use resolver::{
    AliasScope, CrateDependency, CrateManifest, PackageEntry, PathAlias, ProjectResolution,
    ProjectResolver, ResolutionMetadata, RunHealth, RunStatus, SiteRef, Strategy, TypedResolution,
};
