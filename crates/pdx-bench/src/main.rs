//! The pdx-bench harness: `pdx-bench golden [--accept]` (P2-12).

use std::ffi::OsString;
use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::{Arg, ArgAction, Command};
use pdx_bench::golden::cache::Cache;
use pdx_bench::golden::index::sibling_pdx;
use pdx_bench::golden::plan::golden_plan;
use pdx_bench::golden::run::{self, GoldenError, Mode, PdxProducer};
use pdx_bench::lock;

/// The command line.
fn command() -> Command {
    Command::new("pdx-bench")
        .about("The benchmark harness")
        .subcommand_required(true)
        .subcommand(
            Command::new("golden")
                .about(
                    "Index every golden repository of bench/repos.lock at its pinned commit \
                     and compare its structural graph with bench/golden/",
                )
                .arg(
                    Arg::new("accept")
                        .long("accept")
                        .action(ArgAction::SetTrue)
                        .help(
                            "Replace bench/golden/ with what the index produces, once every \
                             repository has succeeded; justify the change in the commit body",
                        ),
                ),
        )
}

fn main() -> ExitCode {
    run_with(std::env::args_os())
}

fn run_with(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let matches = match command().try_get_matches_from(args) {
        Ok(matches) => matches,
        Err(e) if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) => {
            print!("{e}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            // A usage error is an error like any other: exit 1, never clap's 2.
            eprint!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let result = match matches.subcommand() {
        Some(("golden", m)) => golden(if m.get_flag("accept") {
            Mode::Accept
        } else {
            Mode::Compare
        }),
        _ => unreachable!("a subcommand is required"),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("pdx-bench: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `pdx-bench golden`.
fn golden(mode: Mode) -> Result<(), GoldenError> {
    run::preflight(|name| std::env::var_os(name).is_some())?;
    let here = std::env::current_dir().map_err(|_| GoldenError::Workspace(".".into()))?;
    let root = run::workspace_root(&here)?;
    let repos = lock::bench_repos(&root.join("bench/repos.lock"))?;
    let plan = golden_plan(&repos)?;
    let harness = std::env::current_exe().map_err(|_| GoldenError::NoPdx("pdx".into()))?;
    let pdx = sibling_pdx(&harness).ok_or_else(|| GoldenError::NoPdx(harness.clone()))?;
    let cache = Cache::open(&root.join("bench/.cache"))?;
    let mut producer = PdxProducer::new(pdx, cache.clone())?;
    let mut out = std::io::stdout().lock();
    let mut err = std::io::stderr();
    run::run(
        &root.join("bench/golden"),
        &cache,
        &plan,
        mode,
        &mut producer,
        &mut out,
        &mut err,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_line_has_golden_and_accept_only() {
        command().debug_assert();
        let m = command()
            .try_get_matches_from(["pdx-bench", "golden", "--accept"])
            .expect("parses");
        let (name, golden) = m.subcommand().expect("a subcommand");
        assert_eq!(name, "golden");
        assert!(golden.get_flag("accept"));
        let cli = command();
        let names: Vec<&str> = cli.get_subcommands().map(Command::get_name).collect();
        assert_eq!(names, ["golden"]);
        for args in [
            &["pdx-bench"][..],
            &["pdx-bench", "golden", "--determinism"],
            &["pdx-bench", "perf"],
        ] {
            assert!(command().try_get_matches_from(args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn usage_errors_exit_one() {
        assert_eq!(
            run_with(["pdx-bench", "golden", "--nope"].map(OsString::from)),
            ExitCode::FAILURE
        );
        assert_eq!(
            run_with(["pdx-bench", "--help"].map(OsString::from)),
            ExitCode::SUCCESS
        );
    }
}
