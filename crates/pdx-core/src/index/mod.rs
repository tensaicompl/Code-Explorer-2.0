//! The indexing pipeline (specification 4.5), stage by stage: discovery, extraction
//! and derivation exist (resolution is `crate::resolve`); the later stages arrive with
//! their tasks.

pub mod derive;
pub mod discover;
pub mod extract;
