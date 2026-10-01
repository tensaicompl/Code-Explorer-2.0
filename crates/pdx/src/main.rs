//! The command-line interface.

use std::ffi::OsStr;
use std::process::ExitCode;

/// Entry point. The command surface is built in a later task; the one command that
/// exists is the hidden engine worker, which isolated extraction starts this program
/// as (`pdx_engine::isolate`).
fn main() -> ExitCode {
    let first = std::env::args_os().nth(1);
    if first.as_deref() == Some(OsStr::new(pdx_engine::isolate::WORKER_SUBCOMMAND)) {
        return match pdx_engine::isolate::serve_worker() {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        };
    }
    println!("not implemented");
    ExitCode::SUCCESS
}
