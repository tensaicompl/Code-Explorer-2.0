//! The architecture model's repository-level part (4.8): what Stage 4 records on a
//! repository's own nodes. The estate's components, contexts, `HAS_ROLE` and
//! `LAYER_DEPENDS` edges and `pdx-arch.yaml` are the estate model's (P6-02).
//!
//! - [`roles`]: each `Module` and `Class` node's layer role (4.8.3).

pub mod roles;
