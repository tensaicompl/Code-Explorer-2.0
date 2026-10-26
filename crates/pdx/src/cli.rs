//! The public command line (P2-11): `pdx index`, `pdx coverage`, `pdx version`, and
//! `pdx bench` (P2-14), which runs the benchmark harness `pdx-bench` beside this
//! executable. The harness is a separate program: `pdx` never links it.
//!
//! Exit codes: 0 when a command succeeds (an index that is ready), 2 when an index
//! succeeds but its segment is degraded, 1 for every error, a usage error included
//! (the parser's own usage code would be read as "degraded"). Results go to standard
//! output, and only results: errors (one line, `pdx: …`) and progress go to standard
//! error, progress only when it is a terminal.

use std::ffi::OsString;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::{Arg, ArgAction, ArgGroup, ArgMatches, Command, value_parser};
use pdx_core::config::PdxConfig;
use pdx_core::index::extract::{ExtractCache, FsCache};
use pdx_core::index::{IndexError, IndexRequest, Progress, build_segment};
use pdx_core::segment::{RepoIdSource, SegmentError, SegmentReader};

use crate::local::{self, Output};
use crate::report::{self, CoverageReport, IndexReport};
use crate::repository;
use crate::resources::{self, Settings};

/// How a command ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It did what it was asked; an index is ready.
    Success,
    /// An index was built, and its segment is degraded.
    Degraded,
    /// It failed.
    Error,
}

impl Outcome {
    /// The process exit code: 0, 2 or 1.
    pub const fn code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::Degraded => 2,
            Self::Error => 1,
        }
    }
}

/// `pdx cache` and its two commands.
fn cache_command(json: &Arg) -> Command {
    Command::new("cache")
        .about("Report on, or prune, the local extraction cache")
        .after_help(
            "The cache keeps at most `[cache] max_bytes` of the user's pdx/config.toml \
                 (10 GiB by default), least recently used first; a build prunes it when it \
                 ends, and removes the entries of retired formats when it begins.",
        )
        .subcommand_required(true)
        .subcommand(
            Command::new("stats")
                .about("Print the cache's location, size and retired formats")
                .arg(json.clone().help("Print them as JSON")),
        )
        .subcommand(
            Command::new("prune")
                .about("Remove retired formats, then entries least recently used first")
                .arg(
                    Arg::new("max-bytes")
                        .long("max-bytes")
                        .value_name("BYTES")
                        .value_parser(value_parser!(u64))
                        .help("Keep at most this many bytes [default: the configured]"),
                )
                .arg(json.clone().help("Print what was removed as JSON")),
        )
}

/// The command line's definition.
pub fn command() -> Command {
    let json = Arg::new("json").long("json").action(ArgAction::SetTrue);
    Command::new("pdx")
        .bin_name("pdx")
        .about("Index a Git repository into a code-graph segment, and report on it")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .disable_help_subcommand(true)
        .after_help("Exit codes: 0 success (an index is ready), 2 indexed but degraded, 1 error.")
        .subcommand(
            Command::new("index")
                .about("Build the structural segment of the repository's HEAD commit")
                .after_help(
                    "The working tree must be clean: a segment is of the HEAD commit. The \
                     repository's identity comes from its origin remote, which must be an \
                     HTTPS clone URL. Exit codes: 0 ready, 2 degraded, 1 error.",
                )
                .arg(
                    Arg::new("path")
                        .value_name("PATH")
                        .default_value(".")
                        .value_parser(value_parser!(PathBuf))
                        .help("The repository, or a directory inside it"),
                )
                .arg(
                    Arg::new("out")
                        .long("out")
                        .value_name("FILE")
                        .value_parser(value_parser!(OsString))
                        .help(
                            "Where to write the segment; a relative path is below the \
                             repository root; a segment named here is never overwritten, \
                             the default one is replaced [default: .pdx/segment.db]",
                        ),
                )
                .arg(
                    Arg::new("profile")
                        .long("profile")
                        .value_name("PROFILE")
                        .value_parser(["structural"])
                        .default_value("structural")
                        .help("The build profile"),
                )
                .arg(json.clone().help("Print the index report as JSON")),
        )
        .subcommand(
            Command::new("coverage")
                .about("Print the coverage of the repository's segment, .pdx/segment.db")
                .arg(
                    Arg::new("segment")
                        .long("segment")
                        .value_name("PATH")
                        .value_parser(clap::value_parser!(PathBuf))
                        .help("Read this segment instead (one `pdx index --out` wrote)"),
                )
                .arg(json.clone().help("Print the coverage as JSON")),
        )
        .subcommand(Command::new("version").about(
            "Print the versions of pdx, the engine, the segment schema and the language matrix",
        ))
        .subcommand(cache_command(&json))
        .subcommand(
            Command::new("bench")
                .about("Run the benchmark harness: determinism, performance, or both")
                .after_help(
                    "Runs pdx-bench, which must be beside this pdx, from the workspace root; \
                     results go to bench/results/<version>/. Exit codes: 0 success, 1 error \
                     or segments that are not identical.",
                )
                .arg(
                    Arg::new("determinism")
                        .long("determinism")
                        .action(ArgAction::SetTrue)
                        .help("Build every golden repository three ways; the segments must be identical"),
                )
                .arg(
                    Arg::new("perf")
                        .long("perf")
                        .action(ArgAction::SetTrue)
                        .help("Time golden and scale repositories and measure their peak memory"),
                )
                .group(
                    ArgGroup::new("modes")
                        .args(["determinism", "perf"])
                        .required(true)
                        .multiple(true),
                ),
        )
}

/// The benchmark harness beside `pdx` (this executable), and its arguments.
pub fn bench_invocation(pdx: &Path, determinism: bool, perf: bool) -> (PathBuf, Vec<OsString>) {
    let harness = pdx
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("pdx-bench{}", std::env::consts::EXE_SUFFIX));
    let mut args = vec![OsString::from("bench")];
    if determinism {
        args.push(OsString::from("--determinism"));
    }
    if perf {
        args.push(OsString::from("--perf"));
    }
    (harness, args)
}

/// `pdx bench`: the harness's own output and exit, success or error.
fn bench(m: &ArgMatches) -> Result<Outcome, String> {
    let pdx = std::env::current_exe()
        .map_err(|_| "the location of this pdx cannot be determined".to_owned())?;
    let (harness, args) = bench_invocation(&pdx, m.get_flag("determinism"), m.get_flag("perf"));
    if !harness.is_file() {
        return Err(format!(
            "pdx bench runs the benchmark harness beside pdx, and there is none at {}; build it: cargo build --release -p pdx -p pdx-bench",
            harness.display()
        ));
    }
    let status = std::process::Command::new(&harness)
        .args(args)
        .status()
        .map_err(|e| format!("cannot run {}: {e}", harness.display()))?;
    Ok(if status.success() {
        Outcome::Success
    } else {
        Outcome::Error
    })
}

/// Writes to standard output. A closed pipe (`pdx index --json | head`) is no error;
/// any other failure (a full disk under `> report.json`) is, so a truncated report is
/// never a success. Control characters but newlines and tabs are escaped: a file name
/// in a report never writes escape sequences to a terminal.
fn out(text: &str) -> Result<(), String> {
    let text = printable(text);
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(format!("standard output could not be written: {e}")),
    }
}

/// `text` with every control character but newline and tab escaped as `\u{..}`.
fn printable(text: &str) -> std::borrow::Cow<'_, str> {
    if !text
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return std::borrow::Cow::Borrowed(text);
    }
    std::borrow::Cow::Owned(
        text.chars()
            .map(|c| {
                if c.is_control() && c != '\n' && c != '\t' {
                    c.escape_unicode().to_string()
                } else {
                    c.to_string()
                }
            })
            .collect(),
    )
}

/// An error, as one line on standard error.
fn fail(message: &str) -> Outcome {
    let line = printable(&message.replace('\n', " ")).into_owned();
    let _ = writeln!(std::io::stderr().lock(), "pdx: {line}");
    Outcome::Error
}

/// Runs the command line on `args` (the program's name first).
pub fn run(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let outcome = match command().try_get_matches_from(args) {
        Ok(matches) => dispatch(&matches),
        Err(error) => usage(&error),
    };
    ExitCode::from(outcome.code())
}

/// A parse that did not give a command: help is a success, anything else an error,
/// never the parser's own exit code.
fn usage(error: &clap::Error) -> Outcome {
    match error.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
            match out(&error.render().to_string()) {
                Ok(()) => Outcome::Success,
                Err(message) => fail(&message),
            }
        }
        ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
            fail("a command is required: index, coverage, cache, version or bench (see pdx --help)")
        }
        _ => {
            let rendered = error.render().to_string();
            let first = rendered.lines().next().unwrap_or("invalid arguments");
            let message = first.strip_prefix("error: ").unwrap_or(first);
            fail(&format!("{message} (see pdx --help)"))
        }
    }
}

fn dispatch(matches: &ArgMatches) -> Outcome {
    match matches.subcommand() {
        Some(("index", m)) => index(m).unwrap_or_else(|message| fail(&message)),
        Some(("coverage", m)) => coverage(m).unwrap_or_else(|message| fail(&message)),
        Some(("bench", m)) => bench(m).unwrap_or_else(|message| fail(&message)),
        Some(("cache", m)) => cache(m).unwrap_or_else(|message| fail(&message)),
        Some(("version", _)) => match out(&report::version_text()) {
            Ok(()) => Outcome::Success,
            Err(message) => fail(&message),
        },
        _ => {
            fail("a command is required: index, coverage, cache, version or bench (see pdx --help)")
        }
    }
}

/// `pdx index`.
fn index(m: &ArgMatches) -> Result<Outcome, String> {
    let path = m
        .get_one::<PathBuf>("path")
        .cloned()
        .unwrap_or_else(|| ".".into());
    let json = m.get_flag("json");

    // Everything that can refuse the build is checked before anything is written.
    let root = repository::toplevel(&path).map_err(|e| e.to_string())?;
    let config = PdxConfig::load(&root).map_err(|e| e.to_string())?;
    let commit_sha = repository::head_commit(&root).map_err(|e| e.to_string())?;
    repository::require_clean(&root).map_err(|e| e.to_string())?;
    let identity =
        repository::identity(&root, config.server.is_some()).map_err(|e| e.to_string())?;
    let limits = resources::resolve(&Settings::from_env(), &resources::host_facts())
        .map_err(|e| e.to_string())?;
    let output = local::output(&root, m.get_one::<OsString>("out").map(OsString::as_os_str))
        .map_err(|e| e.to_string())?;
    local::prepare(&root, &output).map_err(|e| e.to_string())?;
    let staged = local::staging_path(&output);

    let user = local::user_config()?;
    let cache = local::extract_cache(&root).map(FsCache::at);
    if let Some(cache) = &cache {
        // The entries of formats no build reads again; a cache is never a reason to fail.
        let _ = cache.remove_retired_formats();
    }
    let tty = std::io::stderr().is_terminal();
    let progress = |p: &Progress| report::progress_to(tty, &mut std::io::stderr().lock(), p);
    let request = IndexRequest {
        root: &root,
        repo_id: identity.repo_id.clone(),
        // `repository::identity` refuses a configured server: the identity is the URL's.
        repo_id_source: RepoIdSource::Url,
        repo_url: identity.repo_url.clone(),
        repo_name: identity.name.clone(),
        commit_sha,
        limits,
        cache: cache.as_ref().map(|c| c as &dyn ExtractCache),
        backend: None,
        destination: staged.clone(),
        build_dir: None,
        progress: tty.then_some(&progress as &dyn Fn(&Progress)),
    };
    let built = build_segment(request).map_err(|e| build_error(&e, &output))?;
    local::publish(&staged, &output).map_err(|e| e.to_string())?;
    if let Some(cache) = &cache {
        // Never what this build read or wrote (decision 85, issue 133).
        if let Err(e) = cache.prune(user.cache_max_bytes) {
            eprintln!("pdx: the extraction cache was not pruned: {e}");
        }
    }
    let dto = IndexReport::new(&built, &identity, &output.display);
    if json {
        out(&report::json(&dto))?;
    } else {
        out(&report::index_text(&dto))?;
    }
    Ok(if dto.degraded() {
        Outcome::Degraded
    } else {
        Outcome::Success
    })
}

/// `pdx cache stats | prune`.
fn cache(m: &ArgMatches) -> Result<Outcome, String> {
    let dir = local::extract_cache_here().ok_or_else(|| {
        "no extraction cache directory is known (set HOME, or XDG_CACHE_HOME)".to_owned()
    })?;
    let cache = FsCache::at(&dir);
    let user = local::user_config()?;
    let failed = |e: std::io::Error| format!("{}: {e}", dir.display());
    match m.subcommand() {
        Some(("stats", m)) => {
            let stats = cache.stats().map_err(failed)?;
            let text = if m.get_flag("json") {
                report::json(&report::CacheStatsReport::new(
                    &dir,
                    &stats,
                    user.cache_max_bytes,
                ))
            } else {
                report::cache_stats_text(&dir, &stats, user.cache_max_bytes)
            };
            out(&text)?;
        }
        Some(("prune", m)) => {
            let max = m
                .get_one::<u64>("max-bytes")
                .copied()
                .unwrap_or(user.cache_max_bytes);
            let retired = cache.remove_retired_formats().map_err(failed)?;
            let pruned = cache.prune(max).map_err(failed)?;
            let text = if m.get_flag("json") {
                report::json(&report::CachePruneReport::new(&retired, &pruned, max))
            } else {
                report::cache_prune_text(&retired, &pruned, max)
            };
            out(&text)?;
        }
        _ => return Err("a cache command is required: stats or prune".to_owned()),
    }
    Ok(Outcome::Success)
}

/// A build error in words, naming the output as the user did.
fn build_error(error: &IndexError, output: &Output) -> String {
    match error {
        IndexError::Write(SegmentError::DestinationExists(_)) => format!(
            "{} already exists, and a segment --out names is never overwritten",
            output.display
        ),
        IndexError::Write(SegmentError::InvalidDestination { reason, .. }) => {
            format!("{}: {reason}", output.display)
        }
        other => other.to_string(),
    }
}

/// `pdx coverage`: the coverage of a segment, `.pdx/segment.db` of the repository the
/// command runs in unless `--segment` names one (a segment `pdx index --out` wrote),
/// read through the segment reader, which opens it read-only and verifies it. Nothing
/// is indexed or written.
fn coverage(m: &ArgMatches) -> Result<Outcome, String> {
    let (path, shown) = if let Some(path) = m.get_one::<PathBuf>("segment") {
        (path.clone(), path.display().to_string())
    } else {
        let root = repository::toplevel(std::path::Path::new(".")).map_err(|e| e.to_string())?;
        (
            root.join(".pdx").join("segment.db"),
            local::DEFAULT_OUTPUT.to_owned(),
        )
    };
    if !path.is_file() {
        return Err(format!("no segment at {shown}; run `pdx index` first"));
    }
    let reader = SegmentReader::open(&path).map_err(|e| format!("{shown} cannot be read: {e}"))?;
    let rows = reader
        .coverage_all()
        .map_err(|e| format!("{shown} cannot be read: {e}"))?;
    let dto = CoverageReport::new(reader.meta(), rows);
    if m.get_flag("json") {
        out(&report::json(&dto))?;
    } else {
        out(&report::coverage_text(&dto))?;
    }
    Ok(Outcome::Success)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<ArgMatches, clap::Error> {
        command().try_get_matches_from(std::iter::once("pdx").chain(args.iter().copied()))
    }

    #[test]
    fn the_command_line_is_well_formed() {
        command().debug_assert();
    }

    #[test]
    fn exit_codes_are_zero_two_and_one() {
        assert_eq!(Outcome::Success.code(), 0);
        assert_eq!(Outcome::Degraded.code(), 2);
        assert_eq!(Outcome::Error.code(), 1);
    }

    #[test]
    fn json_takes_no_value_and_the_profile_is_structural() {
        let m = parse(&["index", "--json"]).expect("parses");
        let (_, index) = m.subcommand().expect("index");
        assert!(index.get_flag("json"));
        assert_eq!(
            index.get_one::<String>("profile").map(String::as_str),
            Some("structural")
        );
        assert_eq!(index.get_one::<PathBuf>("path"), Some(&PathBuf::from(".")));
        // `--json` takes no value: `report` after it is the PATH, and with a PATH given
        // it is an argument too many.
        let m = parse(&["index", "--json", "report"]).expect("parses");
        let (_, index) = m.subcommand().expect("index");
        assert_eq!(
            index.get_one::<PathBuf>("path"),
            Some(&PathBuf::from("report"))
        );
        assert!(index.get_flag("json"));
        assert!(parse(&["index", ".", "--json", "report"]).is_err());
        assert!(parse(&["index", "--profile", "precise"]).is_err());
        assert!(parse(&["coverage", "--json"]).is_ok());
        assert!(parse(&["version"]).is_ok());
        // Usage errors never end with the parser's exit code.
        let error = parse(&["index", "--nope"]).expect_err("an error");
        assert_eq!(usage(&error), Outcome::Error);
    }

    #[test]
    fn the_hidden_worker_is_not_a_public_command() {
        let cli = command();
        let names: Vec<&str> = cli.get_subcommands().map(Command::get_name).collect();
        assert_eq!(names, ["index", "coverage", "version", "cache", "bench"]);
        assert!(parse(&[pdx_engine::isolate::WORKER_SUBCOMMAND]).is_err());
    }

    #[test]
    fn bench_takes_determinism_perf_or_both_and_nothing_else() {
        for (args, determinism, perf) in [
            (&["bench", "--determinism"][..], true, false),
            (&["bench", "--perf"], false, true),
            (&["bench", "--determinism", "--perf"], true, true),
        ] {
            let m = parse(args).expect("parses");
            let (_, bench) = m.subcommand().expect("bench");
            assert_eq!(bench.get_flag("determinism"), determinism, "{args:?}");
            assert_eq!(bench.get_flag("perf"), perf, "{args:?}");
        }
        for args in [
            &["bench"][..],
            &["bench", "linux"],
            &["bench", "--perf", "kubernetes"],
            &["bench", "--accept"],
            &["bench", "--json", "--perf"],
        ] {
            assert!(parse(args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn bench_runs_the_harness_beside_pdx() {
        let pdx = Path::new("/opt/pdx/bin").join(format!("pdx{}", std::env::consts::EXE_SUFFIX));
        let (harness, args) = bench_invocation(&pdx, true, false);
        assert_eq!(
            harness,
            Path::new("/opt/pdx/bin").join(format!("pdx-bench{}", std::env::consts::EXE_SUFFIX))
        );
        assert_eq!(args, ["bench", "--determinism"]);
        assert_eq!(bench_invocation(&pdx, false, true).1, ["bench", "--perf"]);
        assert_eq!(
            bench_invocation(&pdx, true, true).1,
            ["bench", "--determinism", "--perf"]
        );
    }
}
