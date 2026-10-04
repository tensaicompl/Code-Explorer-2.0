//! Resolution (specification 4.5, Stage 3): the symbol registry, every definition,
//! module, import, base class and trait implementation of the repository, indexed;
//! the generic-name blocklist; and the stages that settle each call site's band and
//! target from typed resolution and the registry.

pub mod blocklist;
mod imports;
mod metadata;
mod modules;
pub mod registry;
pub mod stages;
