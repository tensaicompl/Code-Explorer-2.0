//! The command-line interface: `pdx index`, `pdx coverage` and `pdx version` (P2-11),
//! and the hidden engine worker isolated extraction starts this program as.

use std::ffi::OsStr;
use std::process::ExitCode;

mod cli;
mod local;
mod report;
mod repository;
mod resources;

/// Entry point. The hidden engine worker (`pdx_engine::isolate`) is served before the
/// command line is parsed, exactly as before: it is no public command and prints
/// nothing of its own.
fn main() -> ExitCode {
    let first = std::env::args_os().nth(1);
    if first.as_deref() == Some(OsStr::new(pdx_engine::isolate::WORKER_SUBCOMMAND)) {
        return match pdx_engine::isolate::serve_worker() {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        };
    }
    cli::run(std::env::args_os())
}
