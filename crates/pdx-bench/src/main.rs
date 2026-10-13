//! The pdx-bench harness: `pdx-bench golden [--accept]` (P2-12), `pdx-bench bench
//! --determinism --perf` (P2-14), which `pdx bench` launches, and `pdx-bench
//! differential-corpora` (P2-16), which the engine differential reads, and `pdx-bench
//! audit-sites` (A0.1, decision 64), which `scripts/a0/` reads. Its benchmark worker is
//! served before the command line is parsed, when the first argument is
//! `pdx_bench::bench::worker::WORKER`; it is no subcommand and in no help.

use std::ffi::OsString;
use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::{Arg, ArgAction, ArgGroup, Command};
use pdx_bench::audit;
use pdx_bench::bench::area::Area;
use pdx_bench::bench::host::Host;
use pdx_bench::bench::plan::bench_plan;
use pdx_bench::bench::run::{
    self as bench_run, BenchError, GitCheckouts, Modes, Settings, WorkerBuilder,
};
use pdx_bench::bench::worker::{self, WORKER};
use pdx_bench::differential;
use pdx_bench::golden::cache::Cache;
use pdx_bench::golden::git::{Git, Prepared, prepare_checkout};
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
        .subcommand(
            Command::new("bench")
                .about(
                    "Build every golden repository three ways and require identical segments \
                     (--determinism), and time and measure golden and scale repositories \
                     (--perf); results go to bench/results/<version>/",
                )
                .arg(
                    Arg::new("determinism")
                        .long("determinism")
                        .action(ArgAction::SetTrue)
                        .help(
                            "Cold with one worker, cold with many, warm with many: the \
                             segments must be byte-identical",
                        ),
                )
                .arg(
                    Arg::new("perf")
                        .long("perf")
                        .action(ArgAction::SetTrue)
                        .help(
                            "Three cold samples per repository: stage times, files per \
                             second and peak resident memory",
                        ),
                )
                .group(
                    ArgGroup::new("modes")
                        .args(["determinism", "perf"])
                        .required(true)
                        .multiple(true),
                ),
        )
        .subcommand(Command::new("differential-corpora").about(
            "Check bench/engine-differential.toml against the language matrix and \
             bench/repos.lock, fetch the golden repositories it names at their pinned \
             commits, and print every corpus of the engine differential as JSON \
             (scripts/engine/differential.py reads it)",
        ))
        .subcommand(
            Command::new("audit-sites")
                .about(
                    "Write the A0.1 site dataset of a checkout: every call site as Stage 3 \
                     settles it, and the definitions it can name, one JSON object a line \
                     (scripts/a0/ reads it)",
                )
                .arg(
                    Arg::new("root")
                        .long("root")
                        .value_name("DIR")
                        .required(true)
                        .value_parser(clap::value_parser!(std::path::PathBuf))
                        .help("The checkout"),
                )
                .arg(
                    Arg::new("out")
                        .long("out")
                        .value_name("FILE")
                        .required(true)
                        .value_parser(clap::value_parser!(std::path::PathBuf))
                        .help("The dataset to write; an existing file is replaced"),
                ),
        )
}

fn main() -> ExitCode {
    let mut args = std::env::args_os();
    let program = args.next();
    let mut rest = args.peekable();
    if rest.peek().is_some_and(|first| first == WORKER) {
        return serve_worker();
    }
    run_with(program.into_iter().chain(rest))
}

/// The benchmark worker: one build, its answer on standard output.
fn serve_worker() -> ExitCode {
    let mut out = std::io::stdout().lock();
    match worker::serve(std::io::stdin().lock(), &mut out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("pdx-bench worker: {e}");
            ExitCode::FAILURE
        }
    }
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
        })
        .map_err(|e| e.to_string()),
        Some(("bench", m)) => bench(Modes {
            determinism: m.get_flag("determinism"),
            performance: m.get_flag("perf"),
        })
        .map_err(|e| e.to_string()),
        Some(("differential-corpora", _)) => differential_corpora(),
        Some(("audit-sites", m)) => audit_sites(
            m.get_one::<std::path::PathBuf>("root")
                .expect("a required argument"),
            m.get_one::<std::path::PathBuf>("out")
                .expect("a required argument"),
        ),
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

/// `pdx-bench bench`.
fn bench(modes: Modes) -> Result<(), BenchError> {
    bench_run::preflight(|name| std::env::var_os(name).is_some())?;
    let here = std::env::current_dir().map_err(|_| BenchError::Workspace(".".into()))?;
    let root = run::workspace_root(&here).map_err(|_| BenchError::Workspace(here.clone()))?;
    let repos = lock::bench_repos(&root.join("bench/repos.lock"))?;
    let plan = bench_plan(&repos)?;
    let harness = std::env::current_exe().map_err(|e| BenchError::Io {
        path: "pdx-bench".into(),
        source: e,
    })?;
    let cache = Cache::open(&root.join("bench/.cache"))?;
    let area = Area::open(&cache.root().join("bench"))?;
    let settings = Settings::current();
    let mut checkouts = GitCheckouts::new(cache)?;
    let mut builder = WorkerBuilder::new(harness, settings.ceiling);
    let run = bench_run::Run {
        plan: &plan,
        modes,
        settings,
        host: Host::current(settings.perf_workers, settings.memory_budget_bytes),
        area: &area,
        results_root: root.join("bench/results"),
    };
    let mut out = std::io::stdout().lock();
    let mut err = std::io::stderr();
    bench_run::run(&run, &mut checkouts, &mut builder, &mut out, &mut err)
}

/// `pdx-bench differential-corpora`: the engine differential's corpora, checked, the
/// goldens fetched as `golden` fetches them, as one JSON document on standard output.
fn differential_corpora() -> Result<(), String> {
    let here = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = run::workspace_root(&here).map_err(|e| e.to_string())?;
    let text = std::fs::read_to_string(root.join(differential::MANIFEST))
        .map_err(|e| format!("{}: {e}", differential::MANIFEST))?;
    let manifest = differential::parse(&text).map_err(|e| e.to_string())?;
    let repos = lock::bench_repos(&root.join("bench/repos.lock")).map_err(|e| e.to_string())?;
    let dirs = differential::fixture_dirs(&root).map_err(|e| e.to_string())?;
    differential::check_manifest(&manifest, &repos, &dirs).map_err(|e| e.to_string())?;
    let mut corpora = Vec::new();
    for fixture in &manifest.fixture {
        corpora.push(differential::fixture_corpus(&root, fixture).map_err(|e| e.to_string())?);
    }
    let cache = Cache::open(&root.join("bench/.cache")).map_err(|e| e.to_string())?;
    let git = Git::new(
        cache.no_hooks().map_err(|e| e.to_string())?,
        cache.git_global().map_err(|e| e.to_string())?,
    );
    let goldens = differential::typed_goldens(&repos).map_err(|e| e.to_string())?;
    for (subset, entry) in manifest.golden.iter().zip(&goldens) {
        let checkout = cache.checkout(entry).map_err(|e| e.to_string())?;
        let prepared = prepare_checkout(&git, &checkout, &entry.fetch_url(), &entry.commit)
            .map_err(|e| format!("{}: {e}", entry.key()))?;
        let how = match prepared {
            Prepared::Reused => "checkout reused",
            Prepared::Fetched => "commit fetched",
        };
        eprintln!("pdx-bench: {}: {how}", entry.key());
        corpora.push(
            differential::golden_corpus(subset, entry, &checkout).map_err(|e| e.to_string())?,
        );
    }
    let document = serde_json::json!({ "format": differential::FORMAT, "corpora": corpora });
    println!("{document}");
    Ok(())
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

/// `pdx-bench audit-sites --root DIR --out FILE`: A0.1's site dataset of the checkout
/// at `root` (decision 64), written to `out`; its counts on standard error.
fn audit_sites(root: &std::path::Path, out: &std::path::Path) -> Result<(), String> {
    let file = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut writer = std::io::BufWriter::new(file);
    let counts = audit::write_dataset(root, &mut writer).map_err(|e| e.to_string())?;
    std::io::Write::flush(&mut writer).map_err(|e| format!("{}: {e}", out.display()))?;
    eprintln!(
        "pdx-bench: {} files, {} definitions, {} call sites, {} other sites",
        counts.files, counts.definitions, counts.resolved, counts.unconfirmed
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_line_has_golden_and_bench_only() {
        command().debug_assert();
        let m = command()
            .try_get_matches_from(["pdx-bench", "golden", "--accept"])
            .expect("parses");
        let (name, golden) = m.subcommand().expect("a subcommand");
        assert_eq!(name, "golden");
        assert!(golden.get_flag("accept"));
        let cli = command();
        let names: Vec<&str> = cli.get_subcommands().map(Command::get_name).collect();
        assert_eq!(
            names,
            ["golden", "bench", "differential-corpora", "audit-sites"]
        );
        for args in [
            &["pdx-bench"][..],
            &["pdx-bench", "golden", "--determinism"],
            &["pdx-bench", "perf"],
            &["pdx-bench", "bench"],
            &["pdx-bench", "bench", "kubernetes"],
            &["pdx-bench", "bench", "--perf", "kubernetes"],
            &["pdx-bench", "bench", "--accept"],
            &["pdx-bench", WORKER],
            &["pdx-bench", "audit-sites"],
            &["pdx-bench", "audit-sites", "--root", "."],
        ] {
            assert!(command().try_get_matches_from(args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn bench_takes_determinism_perf_or_both() {
        for (args, determinism, perf) in [
            (&["pdx-bench", "bench", "--determinism"][..], true, false),
            (&["pdx-bench", "bench", "--perf"], false, true),
            (
                &["pdx-bench", "bench", "--perf", "--determinism"],
                true,
                true,
            ),
        ] {
            let m = command().try_get_matches_from(args).expect("parses");
            let (name, bench) = m.subcommand().expect("a subcommand");
            assert_eq!(name, "bench");
            assert_eq!(bench.get_flag("determinism"), determinism, "{args:?}");
            assert_eq!(bench.get_flag("perf"), perf, "{args:?}");
        }
    }

    #[test]
    fn the_worker_is_in_no_help() {
        let mut cli = command();
        let help = cli.render_long_help().to_string();
        assert!(!help.contains(WORKER) && !help.contains("worker"), "{help}");
        let bench = cli.find_subcommand_mut("bench").expect("bench");
        let help = bench.render_long_help().to_string();
        assert!(!help.contains(WORKER), "{help}");
        assert!(help.contains("--determinism") && help.contains("--perf"));
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
        assert_eq!(
            run_with(["pdx-bench", "bench"].map(OsString::from)),
            ExitCode::FAILURE
        );
    }
}
