//! Determinism and performance baselines (4.13, P2-14): `pdx-bench bench --determinism
//! --perf`, which `pdx bench` launches.
//!
//! - [`plan`]: the repositories of `bench/repos.lock`, goldens and scale, checked.
//! - [`area`]: `bench/.cache/bench/`, the benchmark's disposable working area.
//! - [`timer`]: a build's stage durations, from its progress events.
//! - [`measure`]: throughput, medians and the resident-memory peak.
//! - [`worker`]: the hidden worker, one `build_segment` in a process of its own.
//! - [`process`]: the worker run from the harness, its memory sampled, bounded.
//! - [`host`]: what the measuring machine is, without naming it.
//! - [`results`]: the result files, merged, checked and promoted all or nothing.
//! - [`run`]: the determinism and performance runs.
//!
//! Tests never reach the network: builders are synthetic or run on local checkouts.

pub mod area;
pub mod host;
pub mod measure;
pub mod plan;
pub mod process;
pub mod results;
pub mod run;
pub mod timer;
pub mod worker;
