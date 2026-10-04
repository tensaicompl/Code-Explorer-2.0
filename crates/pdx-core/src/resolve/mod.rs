//! Resolution (specification 4.5, Stage 3). The symbol registry exists: every
//! definition, module, import and base class of the repository, indexed for the
//! resolution stages, which arrive with their task.

mod imports;
mod metadata;
mod modules;
pub mod registry;
