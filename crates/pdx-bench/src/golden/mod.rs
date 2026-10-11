//! The golden harness (plan 5.7, P2-12): each golden repository of `bench/repos.lock`
//! fetched at its pinned commit, indexed by `pdx index`, and its structural graph
//! compared with, or accepted as, `bench/golden/<lang>/<repo>.expected.json`.
//!
//! - [`plan`]: the `[[golden]]` entries, checked and sorted; scale repositories never.
//! - [`cache`]: `bench/.cache/`, the disposable working area, and its safe file
//!   operations.
//! - [`git`]: the pinned checkout, by HTTPS, with no hook, prompt or user setting.
//! - [`index`]: `pdx index` run as a process, and only a ready index accepted.
//! - [`expected`]: the committed format (decision 44), built purely from segment rows.
//! - [`run`]: produce, stage, then compare or accept, all or nothing.
//!
//! Tests never reach the network: the committed goldens are read as files, and the
//! fetch is exercised against local repositories.

pub mod cache;
pub mod expected;
pub mod git;
pub mod index;
pub mod plan;
pub mod run;

pub use expected::{GoldenEdge, GoldenExpected, NodeKindCount};
