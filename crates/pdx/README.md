# pdx

The command-line interface.

Open. Apache-2.0; see `LICENSE` at the repository root.

## Commands

```
pdx index [PATH] [--out FILE] [--profile structural] [--json]
pdx coverage [--json]
pdx version
```

- **`pdx index`** builds the structural segment of a Git repository's `HEAD` commit
  through `pdx_core::index::build_segment`. `PATH` (default `.`) is the repository or
  any directory inside it; the repository's top level is indexed.
  - The working tree must be clean: a modified, deleted, staged, or untracked and not
    ignored file refuses the build, since a segment is of a commit (decision 38). Put
    `.pdx/` in `.gitignore`.
  - The repository's identity comes from its `origin` remote, which must be an HTTPS
    clone URL: its canonical form is `repo_url`, and `repo_id` is derived from it
    (4.2.1, decision 36), so the segment records `repo_id_source = "url"` (decision
    40). Any other remote is refused, never guessed into HTTPS, and
    never printed. A repository with a server configured (`pdx.toml [server]`) is
    refused: its identity is the server's registered one, which this build cannot ask
    for.
  - The segment goes to `<repository root>/.pdx/segment.db`; a relative `--out` is below
    the repository root, an absolute one as given. Only the default `.pdx` directory is
    made, never through a symlink. An existing segment is never overwritten.
  - `--profile` accepts `structural` only.
  - Without `--json`, a summary goes to standard output; with it, one JSON report
    (status, repository with its `repo_id_source`, segment, files, rows, resolution,
    coverage, unportable paths, degraded reasons). Neither holds anything about the machine that built it.
  - Progress goes to standard error, one line per stage, only when it is a terminal.
- **`pdx coverage`** prints the coverage rows of the current repository's
  `.pdx/segment.db`, opened read-only and verified by the segment reader; `--json` for
  JSON, with the segment's identity and its `repo_id_source`. It indexes nothing and needs no clean tree.
- **`pdx version`** prints `pdx`, the engine, the segment schema and the language
  matrix versions.

Exit codes: 0 success (an index is ready), 2 an index was built and is degraded, 1 any
error, a usage error included. Errors are one line on standard error, `pdx: …`, with
nothing on standard output.

Stage 2's limits are resolved here (decision 37): workers are the least of the
processors available, the cgroup CPU quota and `PDX_WORKERS`; the memory budget is
`PDX_MEM_BUDGET_MB` MiB, else 60% of the cgroup memory limit, else 512 MiB. An invalid
setting fails the command. The extraction cache is `pdx/extract` below the platform's
cache directory (`$XDG_CACHE_HOME` or `~/.cache`, `~/Library/Caches`,
`%LOCALAPPDATA%\pdx\cache`), never inside the repository.

One more command exists, and is hidden: `pdx engine-worker` serves extraction over
standard input and output, and is how isolated extraction (`PDX_ENGINE_ISOLATE=1`,
`pdx_engine::isolate`) starts this program as its worker. It is served before the
command line is parsed, appears in no help and is not meant to be run by hand.

## Modules

| Module | |
|---|---|
| `cli` | The command definition (`clap`'s builder API), dispatch and exit codes |
| `repository` | Git metadata: top level, `HEAD`, the clean check, `origin` and the identity. Git is run directly, never through a shell, with routing variables cleared, no prompt and bounded output |
| `resources` | Stage 2's worker and memory limits: host facts (`cgroup`) apart from the pure resolution |
| `local` | Output paths, the `.pdx` directory and the extraction cache directory |
| `report` | The JSON reports, the text reports, the versions and the progress lines |

## Tests

```
cargo test -p pdx
```

`tests/cli.rs` runs the built binary against real Git repositories made with fixed
authors, committers, dates and `core.autocrlf` off, with the user's and the system's
Git configuration shut out: `cli_index_fixture` (the JSON report, snapshotted, with no
temporary path in it), `cli_exit_code_degraded`, `cli_help_snapshot` (every help
screen, no hidden or future command), and the ready, usage-error and runtime-error exit
codes, the clean-tree rule, identity and credential redaction, server-configured
refusal, output placement, `.pdx` safety, no machine state in the report, `pdx version`
and `pdx coverage`. Unit tests cover the resource resolution (cgroup v1 and v2 on
synthetic trees), the identity rule, the progress lines and the report shapes.

`tests/rust_generic_calls.rs` is issue 107A end to end: a Rust crate whose calls carry
turbofishes (paths, methods, a chain, a comment before a dot, a block's local function, a
macro's tokens), indexed by the built binary cold into an empty cache and again warm (the
same segment, byte for byte), read back through the segment's queries (a name searched,
its edges, candidate rows and sites): every turbofish call a site, drawn where trusted
evidence allows and weak or candidate otherwise, the call in the macro's tokens no site,
no site twice; and the same crate with each call's turbofish blanked builds the same graph.

`tests/rust_recall.rs` is issue 105 (R5) end to end: a Rust crate of modules, `crate`,
`self` and `super` paths, a `pub use`, a module file named `index.rs`, local, parameter
and field receivers, an associated function, a trait and its impl, a generic bound, a
`Deref` wrapper, `async` and `?`, an external import a parameter shadows, a type a macro
writes and a macro whose tokens read like calls. Indexed cold and warm (the same segment)
and read back: every path and proven receiver drawn `typed` to its target, the bound, the
wrapper, the shared method name and the untyped value never drawn and weak where they
were, the shadowed import's method not `external`, the macro's tokens no sites, every
drawn edge of a drawn band, no site twice.

`engine_isolate_recovers` runs extraction in this binary as a worker, makes the engine
abort on one file through its test switch, and checks that only that file fails, with
reason `engine_crash`, and every other file, before and after it, is extracted exactly
as in process. The others check that isolation changes nothing about what is
extracted, however files are batched, and that a program which is not a worker is
refused.
