//! The public command line (P2-11): `pdx index`, `pdx coverage`, `pdx version`.
//!
//! Exit codes: 0 when a command succeeds (an index that is ready), 2 when an index
//! succeeds but its segment is degraded, 1 for every error, a usage error included
//! (the parser's own usage code would be read as "degraded"). Results go to standard
//! output, and only results: errors (one line, `pdx: …`) and progress go to standard
//! error, progress only when it is a terminal.

use std::ffi::OsString;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};
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
                             repository root; never overwritten [default: .pdx/segment.db]",
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
                .arg(json.help("Print the coverage as JSON")),
        )
        .subcommand(Command::new("version").about(
            "Print the versions of pdx, the engine, the segment schema and the language matrix",
        ))
}

/// Writes to standard output, ignoring a closed pipe.
fn out(text: &str) {
    let _ = std::io::stdout().lock().write_all(text.as_bytes());
}

/// An error, as one line on standard error.
fn fail(message: &str) -> Outcome {
    let line = message.replace('\n', " ");
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
            out(&error.render().to_string());
            Outcome::Success
        }
        ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
            fail("a command is required: index, coverage or version (see pdx --help)")
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
        Some(("version", _)) => {
            out(&report::version_text());
            Outcome::Success
        }
        _ => fail("a command is required: index, coverage or version (see pdx --help)"),
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

    let cache = local::extract_cache(&root).map(FsCache::at);
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
        destination: output.path.clone(),
        build_dir: None,
        progress: tty.then_some(&progress as &dyn Fn(&Progress)),
    };
    let built = build_segment(request).map_err(|e| build_error(&e, &output))?;
    let dto = IndexReport::new(&built, &identity, &output.display);
    if json {
        out(&report::json(&dto));
    } else {
        out(&report::index_text(&dto));
    }
    Ok(if dto.degraded() {
        Outcome::Degraded
    } else {
        Outcome::Success
    })
}

/// A build error in words, naming the output as the user did.
fn build_error(error: &IndexError, output: &Output) -> String {
    match error {
        IndexError::Write(SegmentError::DestinationExists(_)) => format!(
            "{} already exists, and a segment is never overwritten",
            output.display
        ),
        IndexError::Write(SegmentError::InvalidDestination { reason, .. }) => {
            format!("{}: {reason}", output.display)
        }
        other => other.to_string(),
    }
}

/// `pdx coverage`: the coverage of `.pdx/segment.db`, read through the segment reader,
/// which opens it read-only and verifies it. Nothing is indexed or written.
fn coverage(m: &ArgMatches) -> Result<Outcome, String> {
    let root = repository::toplevel(std::path::Path::new(".")).map_err(|e| e.to_string())?;
    let path = root.join(".pdx").join("segment.db");
    if !path.is_file() {
        return Err(format!(
            "no segment at {}; run `pdx index` first",
            local::DEFAULT_OUTPUT
        ));
    }
    let reader = SegmentReader::open(&path)
        .map_err(|e| format!("{} cannot be read: {e}", local::DEFAULT_OUTPUT))?;
    let rows = reader
        .coverage_all()
        .map_err(|e| format!("{} cannot be read: {e}", local::DEFAULT_OUTPUT))?;
    let dto = CoverageReport::new(reader.meta(), rows);
    if m.get_flag("json") {
        out(&report::json(&dto));
    } else {
        out(&report::coverage_text(&dto));
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
        assert_eq!(names, ["index", "coverage", "version"]);
        assert!(parse(&[pdx_engine::isolate::WORKER_SUBCOMMAND]).is_err());
    }
}
