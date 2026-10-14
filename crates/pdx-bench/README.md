# pdx-bench

The benchmark harness: accuracy, timing and determinism. Today it runs the structural
golden graphs (P2-12) and the determinism and performance baselines (P2-14), and
prepares the engine differential's corpora (P2-16); the oracle suite arrives with its
own task.

Open. Apache-2.0; see `LICENSE` at the repository root.

## Commands

```
pdx-bench golden            # compare: fails on any difference, never writes bench/golden/
pdx-bench golden --accept   # replace bench/golden/ with what the index produces
make golden                 # builds pdx and pdx-bench (release), then pdx-bench golden

pdx bench --determinism     # three builds per golden; the segments must be identical
pdx bench --perf            # three cold samples per golden and scale repository
pdx bench --determinism --perf
make determinism            # builds pdx and pdx-bench (release), then pdx bench --determinism
make perf                   # the same, then pdx bench --perf

pdx-bench differential-corpora   # the engine differential's corpora, checked, as JSON
make engine-differential         # the engine against the reference engine (nightly)

pdx-bench audit-sites --root DIR --out FILE   # A0.1's site dataset (scripts/a0/)
pdx-bench versions               # the versions this executable was built with, as JSON
```

Run from the workspace root (the directory holding `Cargo.toml`, `bench/repos.lock` and
`crates/pdx-bench`). Exit codes: 0 every golden matched or was accepted, or the
benchmarks succeeded; 1 anything else (a usage error and a determinism failure
included).

`pdx bench` runs `pdx-bench bench …` beside the `pdx` executable
(`<dir of pdx>/pdx-bench`, never one on the `PATH`) and passes its exit on as 0 or 1.
`pdx` never links this crate, Criterion or `sysinfo`; `scripts/open-binary-check.sh`
(part of `make check`) fails if its normal dependency tree holds any of them.

`pdx-bench golden`:

1. Refuses to run with any of the engine's extraction switches set
   (`pdx_engine::EXTRACTION_SWITCHES`, copied here and kept equal by a test), naming the
   variable, never its value. `PDX_WORKERS`, `PDX_MEM_BUDGET_MB` and
   `PDX_ENGINE_ISOLATE` are allowed; none reaches a golden.
2. Reads `bench/repos.lock` and checks every `[[golden]]` entry before anything is
   fetched: lock version 1, a language, a full lower-case commit, a permitted licence, a
   scheme-less URL without user information, query, fragment, backslash, whitespace or
   relative segment, path components safe on every platform, one golden file per entry
   (case-insensitively). A rejected URL is never printed. `[[scale]]` entries are never
   processed. The plan is sorted by language, then repository.
3. For each entry, in that order:
   - **Checkout.** `bench/.cache/repos/<lang>/<repo>/` is a Git repository whose
     `origin` is exactly `https://<lock url>`. The pinned commit is fetched by its id
     only (`git fetch --no-tags --depth=1 origin <commit>`) when the checkout lacks it.
     Once every pin is cached, no run needs the network. It is checked out detached and
     forced, reset, cleaned (`git clean -ffdx`), and must then be exactly the pin
     (`HEAD`) and clean (`git status --porcelain`). Branches, tags and default heads are
     never consulted, and a pin is never moved.
   - **Git settings.** Git runs without a shell, with:
     - no prompt (`GIT_TERMINAL_PROMPT=0`);
     - an empty global configuration of the harness's (`GIT_CONFIG_GLOBAL`) and no
       system one (`GIT_CONFIG_NOSYSTEM=1`);
     - `core.hooksPath` pointed at an empty directory on every command;
     - `submodule.recurse=false`, and no submodule command;
     - `core.autocrlf=false` and `core.eol=lf`.

     A checkout with another `origin`, a `.git` that is not a directory, or a symlink
     anywhere in the cache path is refused. The checkout is disposable: delete it and
     run again.
   - **Index.** The `pdx` binary beside `pdx-bench` (never one on the `PATH`) runs
     `pdx index <checkout> --out <bench/.cache/segments/<lang>/<repo>/segment.db>
     --profile structural --json`. The old segment there is removed first, and nothing
     is written into the checkout. Only exit 0 with a `ready` report is a golden.
     Degraded (exit 2) is refused with its reasons, because a degraded graph is no
     baseline, and any other exit is an error.
   - **Golden.** The segment is opened with `SegmentReader` (verified, read-only).
     `nodes_all` and `edges_all` feed `GoldenExpected::from_rows`, and the canonical
     bytes are staged at `bench/.cache/generated/<lang>/<repo>.expected.json`.
4. Without `--accept`, each staged golden is compared byte for byte with
   `bench/golden/<lang>/<repo>.expected.json`. A missing, differing, differently
   formatted or stale golden (one for no lock entry), or any other file in
   `bench/golden/`, fails the run with a short account: totals and the first differing
   edges. Nothing is written there.
5. With `--accept`, nothing changes until every repository has been staged. Then every
   staged golden is promoted, stale goldens are removed (and their language directory
   when empty), and one summary line per repository is printed. A failure part-way
   leaves `bench/golden/` exactly as it was.

Nothing in a fetched repository is ever executed: no hook, build, package manager or
script. The source never leaves the cache: a golden holds qualified names, kinds and
bands only.

## Determinism and performance (P2-14, decision 49)

`pdx-bench bench --determinism | --perf` (at least one; no positional argument):

1. **Preflight.** Refuses to run with any engine extraction switch, `PDX_ENGINE_ISOLATE`,
   `PDX_WORKERS` or `PDX_MEM_BUDGET_MB` set (`bench::run::REFUSED_VARIABLES`), naming
   the variable, never its value: a baseline is the engine's defaults, in process, with
   the harness's own limits.
2. **Plan.** `bench/repos.lock` is authoritative. Determinism covers the ten
   `[[golden]]` entries only; performance covers them and the `[[scale]]` entries
   (Kubernetes, Linux). Goldens are checked as `golden` checks them; a scale entry needs
   a safe name and URL, a full commit and `indexed_only = true`. No two entries may
   write one result file, case-insensitively, and none may be named `summary`.
3. **Checkouts.** Fetched exactly as `golden` fetches them (P2-12's Git settings and
   invariants): goldens share `bench/.cache/repos/<lang>/<repo>/` with `make golden`,
   scale repositories live in `bench/.cache/repos/scale/<name>/`.
4. **Builds.** Each build is a fresh `pdx-bench` process, the hidden worker
   (`__pdx-bench-worker`, served before the command line is parsed, in no help): one
   `pdx_core::index::build_segment` with the engine in process, the repository's
   identity derived from its URL exactly as `pdx index` derives it (so the segment is
   the one `pdx index` writes for that commit), a cold or warm extraction cache of the
   harness's, and the segment written below `bench/.cache/bench/`, never into the
   checkout. It answers with one JSON object: status, hash, size, files, each stage's
   count and nanoseconds, and the whole build's wall time, timed apart from the stages.
   Its standard error is kept to 4 KiB.
5. **Determinism**, per golden: A, a cold cache and one Stage 2 worker; B, an
   independent cold cache and as many workers as the machine offers (at least 2); C,
   B's cache, now warm, the same workers. All must be ready, A and B must have read
   nothing from their caches, C must have read every extraction B stored, the three
   segments must be byte-identical (compared in 64 KiB chunks, the first differing byte
   reported) with equal hashes, and each reported hash must be its file's. A failure
   names the repository, the two builds, their sizes, their hashes and the first
   differing offset.
6. **Performance**, per golden and scale repository: three samples, each a fresh
   process with a fresh cold cache, Stage 2 at the machine's available parallelism and
   the default 512 MiB memory budget. Each must be ready, read nothing from its cache
   and have its resident memory observed; the three must build the same segment. The
   baseline is the median throughput, the median of each stage and of the whole build,
   and the largest peak resident memory; every sample is kept.
7. **Results.** Staged under `bench/.cache/bench/results/<version>/`, then promoted
   into `bench/results/<version>/` only when every repository has succeeded. A failure
   part-way leaves every result file as it was.

Stage times come from the pipeline's own progress events: each of the five stages must
start once and finish once, in order, or the build is an error. Throughput is the files
given to extraction (`Finished(Extract)`) per second of extraction and resolution
together, computed in integers and divided once; a zero duration is an error.

Resident memory is sampled from this process (`sysinfo`, system information only)
every 10 ms, plus a sample immediately after the worker starts; the last is taken just
before its exit is seen. A worker whose sample exceeds the 6 GiB ceiling
(`bench::process::MEMORY_CEILING_BYTES`) is stopped at once: a golden then fails the
run, and a scale repository records `out_of_memory`, with the peak seen, the stage it
had reached, the stages it finished with their counts and durations, and the clean
extractions its cold cache held, and takes no further sample (Decision 49's P2-14
measurement policy, issue 82). `bench/README.md` states the methodology for readers of
the results.

### Result files

`bench/results/<version>/<repo>.json` (`<repo>` a golden's repository or a scale
repository's name) and `summary.json`, results format 1, canonical JSON (keys sorted at
every level, two-space indentation, a final newline):

- `provenance`: `pdx_version`, `engine_version`, `segment_schema_version`,
  `language_matrix_version`, `repository` (the key), `kind` (`golden` or `scale`),
  `language` (goldens) and `commit`. Never a branch, a path, a host name, a user or a
  time.
- `determinism`: `cold_single_hash`, `cold_parallel_hash`, `warm_parallel_hash`,
  `cold_builds_byte_identical`, `cold_warm_byte_identical`, `rebuild_equivalent`,
  `workers_single`, `workers_parallel`, the segment's size and files, and the cache
  traffic that proves B cold and C warm.
- `performance`: `outcome` `completed` (the host, the sampling interval and ceiling,
  the hash, files, `median_files_per_second`, `median_<stage>_ns`,
  `median_total_wall_ns`, `peak_rss_bytes`, and every sample with its files, workers,
  memory budget, stage nanoseconds, wall time, throughput, peak and hash) or
  `out_of_memory` (as above).
- The summary: `determinism` (`repositories_tested`, `repositories_identical`,
  `determinism_rate`, `cold_warm_equivalent`, `rebuild_equivalence_rate`) and
  `performance` (`repositories_measured`, `goldens_measured`, `goldens_completed`,
  `scale_repositories_measured`, `scale_repositories_completed`, `all_completed`, and
  each repository's outcome by key, scale repositories under `scale`).

A run writes the sections it measured and keeps the other from the file there, so a
determinism run keeps a performance baseline and the reverse. A top-level field this
format does not know is kept. A file whose provenance is not this run's has stale
sections, which are dropped; if it also holds unknown fields it is refused, never
rewritten. A symlink where a result belongs is refused.

The host is recorded as its operating system, architecture, logical processors, total
memory, the benchmark's workers and memory budget, and the processor's name when every
processor reports the same plain one. Nothing identifies the machine or its user.

The benchmark's working area, `bench/.cache/bench/`, is disposable. A build directory
is emptied before use by removing only regular files and real directories below that
area, never following a link; a symlink or anything else there stops the run.

## The engine differential's corpora (P2-16, decision 50)

`pdx-bench differential-corpora` reads `bench/engine-differential.toml` and prints every
corpus as one JSON document (`{"format": 1, "corpora": [...]}`): its name, kind,
language, root, a golden's commit, its files with the engine language each is read in,
and its support files. `scripts/engine/differential.py` runs both engines over them.
Before printing anything it checks (`differential::check_manifest`):

- every fixture directory under `engine/tests/fixtures/resolve` is listed, and nothing
  else; every file of a fixture is compared, support, or its `assert.tsv` or
  `expected.tsv`;
- every typed-tier language of the matrix has a fixture, and every typed-tier golden of
  `bench/repos.lock` exactly one subset, at the commit the lock pins, of 2 to 10 files;
- names and paths are sorted, unique and safe on every platform;
- every file is a regular file reached through real directories, of the corpus's
  language by the matrix's own detection (a header's content and siblings included),
  each extension of one engine language (`.tsx` is `tsx`), and a golden's C or C++ file
  at most 32 KiB.

The golden checkouts are prepared exactly as `golden` prepares them (P2-12's Git
settings and invariants); nothing else here fetches.

## A0's site dataset and the executable's versions (decision 64, issue 115)

`pdx-bench audit-sites` writes A0.1's site dataset (`audit::write_dataset`): Stages 1
to 3 run in this process, as the pipeline runs them, and every call site is written as
Stage 3 settled it. `scripts/a0/run.py` runs it over each corpus.

`pdx-bench versions` prints one JSON object (`audit::versions`, format 1): the version
the engine it links reports at run time (`engine_version`, the engine's own string),
the `pdx_core` constants compiled into it (`ENGINE_VERSION`,
`EXTRACT_CACHE_FORMAT_VERSION`, `LANGUAGE_MATRIX_VERSION`, `SECRET_DETECTOR_VERSION`,
`SEGMENT_SCHEMA_VERSION`) and the worker protocol it speaks (`worker_protocol`). Before
it measures anything, `run.py` refuses an executable whose report is not the checkout's
own source. The engine's surface version is not in the engine's interface, so it is
not reported.

## The golden format (plan 5.7, decision 44)

```json
{
  "node_counts": [{ "kind": "ApiContract", "count": 0 }, …],
  "drawn_edges": [{ "src_qn": "a.A", "dst_qn": "b.B", "kind": "CALLS", "band": "typed" }, …]
}
```

- **`node_counts`**: every `NodeKind`, zero included, sorted by its spelling, so a new
  kind is a reviewed golden change.
- **`drawn_edges`**: one entry per stored edge whose band `Band::is_drawn`, duplicates
  kept, as its endpoints' qualified names, the edge kind and the band, sorted by
  `(src_qn, dst_qn, kind, band)`.
- **What it never holds**: an id, path, line, source, signature, prop, timestamp,
  machine state, repository URL, commit or version. The lock and the version constants
  already record those.
- **Bytes**: UTF-8, pretty-printed with two spaces, `\n` line endings, a final `\n`.
- **Changes**: a golden is never edited by hand. Changing one means
  `pdx-bench golden --accept` and a justification in the commit body.

## Public API

| Module | |
|---|---|
| `lock` | `bench/references.lock` and `bench/repos.lock`, and the licence allow list |
| `golden::plan` | `golden_plan(&BenchRepos)`: the checked, sorted `[[golden]]` entries (`GoldenEntry`: key, HTTPS fetch URL, golden path); `check_url`, `check_component` |
| `golden::cache` | `Cache` (`bench/.cache/` and its layout); `ensure_dir`, `write_file`, `remove_owned_file`, which never follow a symlink or delete outside their root |
| `golden::git` | `Git` (the isolated command), `prepare_checkout(git, dir, remote, commit)` |
| `golden::index` | `run_pdx`, `index_arguments`, `sibling_pdx`, `interpret` (ready only) |
| `golden::expected` | `GoldenExpected`, `NodeKindCount`, `GoldenEdge`: `from_rows` (pure), `to_canonical_bytes`, `parse` |
| `golden::run` | `preflight`, `workspace_root`, `run(golden_dir, cache, plan, mode, producer, out, err)`, the `Producer` trait and `PdxProducer` |
| `bench::plan` | `bench_plan(&BenchRepos)`: `BenchPlan` (`determinism()`, `performance()`) of `BenchEntry` (golden or scale) |
| `bench::area` | `Area` (`bench/.cache/bench/`): fresh determinism, sample and staging directories |
| `bench::timer` | `StageTimer` (progress events to stage durations, strictly in order), `StageTimes`, `MonotonicClock` |
| `bench::measure` | `files_per_second`, `median_u64`, `median_f64`, `maximum`, `Peak` |
| `bench::worker` | `WORKER`, `WorkerRequest`, `WorkerResponse`, `serve`, `replay` (a progress file) |
| `bench::process` | `run_monitored` (input, bounded output, resident-memory sampling, the ceiling), `SystemProbe`, `MEMORY_CEILING_BYTES`, `SAMPLE_INTERVAL` |
| `bench::host` | `Host` (the machine, never named), `available_workers` |
| `bench::results` | `ResultFile`, `SummaryFile`, `merge`, `summarise`, `canonical_bytes`, `stage_and_promote` |
| `audit` | `write_dataset`, `FORMAT`: A0.1's site dataset; `versions`, `VERSIONS_FORMAT`: what the executable was built with |
| `differential` | `parse`, `check_manifest`, `fixture_corpus`, `golden_corpus`, `check_files`, `typed_languages`, `typed_goldens`: the engine differential's corpora |
| `bench::run` | `preflight`, `Settings`, `determinism_specs`, `sample_spec`, `check_determinism`, `aggregate`, `run(run, checkouts, builder, out, err)`, the `Builder` and `Checkouts` traits, `WorkerBuilder`, `GitCheckouts` |

## Tests

```
cargo test -p pdx-bench
```

`cargo test` never reaches the network.

- `golden_files_sorted` and `golden_file_set_matches_lock` read the committed goldens
  and nothing else. Each must be valid UTF-8 JSON in the format, newline-terminated,
  every node kind once and sorted, edges sorted by the tuple with drawn bands only,
  and byte-equal to its canonical re-serialisation. There must be exactly one golden
  per lock entry.
- `golden_expected` builds goldens from rows, every expectation written out:
  - drawn bands only;
  - duplicates kept;
  - zero counts;
  - spelling order;
  - tuple order;
  - exact canonical bytes;
  - unknown fields refused;
  - a segment the pipeline built.
- `golden_plan` covers the lock rules and that scale repositories are excluded.
- `golden_fetch` runs local repositories with fixed identities:
  - exact pin;
  - offline reuse;
  - LF line endings;
  - foreign origin and foreign directory refused;
  - submodules not populated;
  - the isolated command;
  - on Unix: no hook runs, and a symlinked checkout is refused.
- `golden_run` drives synthetic producers:
  - comparison never writes;
  - acceptance is explicit and all-or-nothing;
  - stale and stray files are caught;
  - links are refused;
  - the switch preflight, and the switch list kept equal to the engine's;
  - ready-only interpretation;
  - only owned segments are removed.

- `bench_run` drives synthetic builders that write the segments they are told to and
  model a cache:
  - the plan (goldens for determinism, scale added for performance, `indexed_only`,
    result-name collisions, the committed lock's ten goldens and two scale entries);
  - the preflight, its list kept equal to the engine's;
  - the three builds' workers and caches;
  - each determinism check made to fail alone (different bytes behind equal reported
    hashes, a differing warm build, a warm build that reads nothing, a cold build that
    reads a cache, a degraded build);
  - medians and the maximum peak (throughput 100, 300, 200 to 200; peaks 10, 40, 20 to
    40), fresh cold caches per sample, samples that disagree, unobserved memory, a zero
    duration, a golden over the ceiling, a scale repository recorded `out_of_memory`;
  - all-or-nothing promotion, mode merging, a later format's field kept, stale
    sections dropped, a stale file with unknown fields refused, canonical bytes, and
    refused links.
- `bench_worker` runs the real worker on a project-authored Python fixture: two cold
  builds and a warm one byte-identical, three measured samples, the checkout untouched;
  the worker served before parsing and absent from help; a refused variable named,
  its value never shown.
- Unit tests in `src/bench/` cover the timer, throughput, medians, the bounded reader,
  the resident-memory probe and ceiling, the host record, the result format and the
  safe emptying of the area.

- `audit_sites` writes site datasets of small fixtures, and runs `pdx-bench versions`:
  its report is the linked engine's version and the compiled constants.
- `engine_differential` holds `engine_differential_identical`: the differential's
  canonical comparison, on a committed synthetic pair of row dumps
  (`tests/data/engine-differential/`), gives the same bytes for the same facts whatever
  their order and project name, maps a `Property` to a field on both sides, and fails
  with a bounded account when one call changes. It also checks the committed manifest:
  its rules, a fixture for every typed-tier language, a subset for every typed-tier
  golden, every fixture file its corpus's language, and `resolve_dump`'s extension
  table against the matrix.

The rest of `tests/` checks the repository itself: the lock files, the vendored
engine, the scanners, the specification documents and the supply chain.
