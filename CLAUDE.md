# Code Explorer 2.0 (PDX 2.0) — repository context

Deterministic code-intelligence platform: indexes many repositories into
immutable versioned graphs, resolves relationships with an explicit confidence
band on every edge, links repositories through contracts, and serves the result
to AI agents over MCP and to a GPU-rendered architecture map.

**Remote:** `tensaicompl/Code-Explorer-2.0` (public, `origin`). The predecessor
repo `tensaicompl/praxevia-explorer` is kept as the remote `praxevia-explorer`.

## State: P2 in progress

P0 and P1 are done and gates G0 and G1a have passed; P2 has begun with the graph
model (`pdx_core::ids`, `bands`, `kinds`, `model`), the segment file
(`pdx_core::segment`), the repository configuration (`pdx_core::config`), Stage 1,
discovery (`pdx_core::index::discover`), and Stage 2, extraction with its cache
(`pdx_core::index::extract`) and content secret normalisation (`pdx_core::secrets`),
Stage 3's resolution: the symbol registry (`pdx_core::resolve::registry`), the
generic-name blocklist (`resolve::blocklist`) and the stages that settle every call
site's band and target (`resolve::stages`), and Stage 4, which derives containment,
modules, call sites and edges, tests, routes and entry points
(`pdx_core::index::derive`), the repository's contracts (`pdx_core::contracts`), and
its modules' and classes' layer roles and symbols' metrics (`pdx_core::layers`,
`pdx_core::metrics`), and the whole pipeline, `build_segment` (Stages 1 to 5, with
coverage, `pdx_core::coverage`, and Stage 5's write, `index::write`).
P1 vendored
the engine, built it, gave it its interface, bound it to Rust (`pdx-engine-sys`),
and wrapped it safely (`pdx-engine`: owned extractions, typed resolution with run
health, crash-isolated extraction), built and tested on Linux, macOS and Windows,
and run nightly under the address, undefined-behaviour and leak sanitizers over a
committed corpus; it also transcribed the language matrix (`pdx_core::languages`).
The previous implementation is in `legacy/`, read-only until the retirement task
removes it. The local CLI has `pdx index`, `pdx coverage` and `pdx version` (P2-11), and
`pdx bench` (P2-14); the engine differential (P2-16) closes P2; there is no query
surface (MCP, server, UI) yet. Gate G1 was evaluated on 2026-10-12 and failed on the
Kubernetes performance criterion (issue 82): P3 is halted until it passes.

Always read `docs/plan/PROGRESS.md` for the current position rather than trusting
this paragraph.

| Path | State |
|---|---|
| `crates/` | Ten crates, licence split enforced by `scripts/open-binary-check.sh`. `pdx-engine-sys` builds and links the engine and holds its raw bindings (`make bindgen` regenerates them); `pdx-engine` is the only way above it into the engine; `pdx-core` holds the specification's constants, the language matrix (`languages`) and the graph model's identities, bands, kinds and rows (`ids`, `bands`, `kinds`, `model`), segments (`segment`: writer and reader), `pdx.toml` (`config`), secret normalisation (`secrets`), the pipeline's stages as they arrive (`index`: discover, extract, derive) and resolution (`resolve`: the symbol registry, the blocklist, the stages); `pdx` is the CLI (`index`, `coverage`, `version`, `bench`, and the hidden `engine-worker`); `pdx-bench` is the benchmark harness (the lock files, the golden harness: `pdx-bench golden [--accept]`, the determinism and performance baselines: `pdx-bench bench --determinism --perf`, with its hidden worker, and the engine differential's corpora: `pdx-bench differential-corpora`); the rest are skeletons |
| `engine/` | The vendored extraction and typed-resolution engine, its interface (`include/pdxe.h`, `api/`), patches and tests. Read `engine/README.md` first; never edit a vendored file in place |
| `ui/` | Vite + React + TypeScript scaffold, lint and tests green, no views yet |
| `bench/` | Pinned references, golden and scale repositories, pre-move tree snapshot, the sanitizer corpus (`corpus/`), the goldens (`golden/`), the baselines (`results/<version>/`) and the engine differential's corpora (`engine-differential.toml`) |
| `ada-indexer/`, `deploy/` | Skeleton and placeholder; contents arrive in their phases |
| `legacy/` | The previous implementation. Do not fix, extend or import from it |

## Build

```
make check        # format, lint, build both feature sets and the engine, test, engine tests,
                  # open-binary check, interface lint and tests
make check-full   # adds accuracy, determinism, sanitizers, browser suites; the nightly build
make check-asan   # the engine's tests and the sanitizer corpus (bench/corpus) under the
                  # address, undefined-behaviour and leak sanitizers; `make asan` is the same
```

On a machine with little memory (a WSL VM capped at 10 GB goes down whole when one
process takes about 9 GB), run builds and tests in a capped scope, so a runaway is the
only thing killed: `systemd-run --user --scope -p MemoryMax=6G -p MemorySwapMax=0
make check`, with `CARGO_BUILD_JOBS=6`, `RUST_TEST_THREADS=4` and
`CMAKE_BUILD_PARALLEL_LEVEL=6`. A graph walk without a visited set (P2-05's hierarchy,
before its fix) exhausts memory in seconds. `make engine` and `scripts/check-asan.sh`
build the engine with CMake's own `CMAKE_BUILD_PARALLEL_LEVEL` jobs, else one per
online processor (on a 28-core machine, set it: 28 grammar compilations at once is
several GB); they never pass `-j` alone, which the Makefile generator runs unbounded.
`make check` itself peaks near 600 MB this way.

The language matrix is `pdx_core::languages`, transcribed from the plan's Appendix A:
31 languages, each with its tier, extensions, shebangs, module rule and test rule.
It is not the engine's list: the engine also names a `tsx` grammar, which is how it
parses TypeScript's `.tsx` files, not a language. Assigning a language never asks the
engine. Change the matrix only by a specification change that bumps
`LANGUAGE_MATRIX_VERSION`.

Identities (`pdx_core::ids`) are a stored format: a node, site or edge id is a hash of
the fields 4.2.1 names and never of a line or offset, and the fixed vectors in
`crates/pdx-core/tests/ids.rs` must never be regenerated from the code. Changing an
encoding renames every stored node, so it is a change of stored format: 4.2.1 states
the byte-level rules and reference vectors (approved in issue 33).

Segments are written only by `SegmentWriter` and read only by `SegmentReader`. The
schema is `crates/pdx-core/src/segment/schema.sql`, which must stay 4.3's DDL byte for
byte; every other statement lives, named, in `segment/queries.rs`, and no value is ever
spliced into SQL. A segment must be byte-identical whenever its rows are: nothing
time-, path- or order-dependent may reach it.

Repository content and `pdx.toml` are untrusted. Discovery never follows a symlink,
never reads a secret-pattern or oversized file, reads nothing larger than the limit,
and depends on nothing but the checkout and its root `pdx.toml`: not the environment,
the user's Git settings or the filesystem's listing order. Nothing a repository
configures (precise commands included) is executed outside the precise sandbox.

Stage 2 reads a file's original bytes once, takes its Git `blob_sha` from them, masks
secret values in the same buffer and gives the engine only the normalised bytes:
original bytes never reach the engine, the cache, an error or a log, and a redacted
file is never opened. Masking preserves length and never touches CR or LF (issue 37);
a change to what the detectors match bumps `SECRET_DETECTOR_VERSION` (2 since a
bearer token runs on over its `=` padding, issue 44; 3 since a bearer token before
`://` counts as a URI scheme, issue 72; 4 since a URI scheme is recognised after any
byte that is not an ASCII letter or digit, issue 73). Detectors read the original
bytes once and their ranges are masked once: idempotence must come from detection,
never from rescanning masked output. Coverage is monotonic: a detector change may
mask more, never less than detector 2 did (`uri_detector_never_loses_a_detector_two_password`). The extraction cache key is
`(engine_version, language_matrix_version, secret_policy_digest, language_id,
rel_path, blob_sha)`; only clean extractions (not truncated, no
`extraction_lost`) are cached; with any `pdx_engine::EXTRACTION_SWITCHES` variable set
no cache is used; a hit must be this source's exact extraction (issue 26). An isolated
worker that times out fails the build and is never recorded as `engine_crash` (issue
27). Test secrets are synthetic and assembled at run time: never write a
credential-shaped literal into the tree.

The symbol registry is read-only once built and owns no graph identity: definitions
are `(path, index)`. Modules come from the matrix's rule and evidence only (a declared
namespace, C++'s qualified names, `__init__.py` packages, `go.mod`, the crate layout),
never a directory guess; a missing fact is `None`. An import is external only by the
language's rules, a relative one never is, and a name is external only when every
import binding it is; ambiguity is kept, never resolved by order. Metadata files are
read only as Stage 2 reads candidates, never a redacted one, and the engine gets the
registry's own metadata, and applies a configuration's path aliases to TypeScript and
JavaScript imports only, as the registry does (issue 43, patch 0009). Rust's `impl
Trait for Type` relations cross the interface (`FileExtract::impl_traits`, issue 42)
and the registry resolves them; supertraits are not recorded, so none is followed. The
extraction cache is format 5 and the worker protocol 6 (issues 40 to 42, 46, 47 and
54): calls carry their node-type path (`ast_path`, computed by the engine since patch
0010) and arguments, and definitions their decorators, parameter types and every route
binding with its declaring node's position and path (`routes`, patch 0011);
`ENGINE_VERSION` is 7. Issue 74 (decision 45, engine 4): the C and C++ expression
evaluator's per-file step budget is 10,000 per started 32 KiB of its source, at least
10,000 and at most 320,000, from the length alone (patch 0012). Running it out is still
lost work and still degrades; a budget that starves real code is fixed in the engine,
never by weakening `degraded`. Issue 75 (decision 47, engine 5): a site's owner (the caller a call names, its
`enclosing_node_id` and the start of its `ast_path`) is the nearest enclosing definition
the extractor represents. A nested callable it does not represent keeps its own lexical
scope and stays the name typed resolution joins on, but owns nothing; no local-function
nodes are added (patch 0013). Issue 76 (decision 48, engine 6): the owner is joined to
its definition by the exact declaration occurrence, the source buffer and the byte span
of the callable's function node. Two independently computed qualified names are never
compared for it. The occurrence is engine-private (`PDXECallableOccurrence`), never in
the interface, a surface, a cache or a segment; `enclosing_func_qn` is unchanged, so
typed resolution joins as before (patch 0014, superseding 0013's join).
- **C, C++ and CUDA** are no longer exempt. Adopted definitions carry occurrences in
  the macro-expanded buffer. A superseded raw callable hands its sites to its
  replacement, and a raw function node stands for an adopted callable only on an
  exact, unique line match with its name on the first line (private aliases).
- **No-position calls.** A call with no position in the file keeps the callable its
  name names, as resolution context only.
- **Rule.** Fix ownership in the engine, never by span lookups in Rust.
- **Open.** Issues 77 to 80 record the residuals: C-family callables only the expanded
  text represents, Kotlin's delegate spans, the walk's frame leak in error-recovered
  regions, and C++ parse artefacts.

Issues 83 and 84 (decisions 51 and 53, engine 7, patches 0015 and 0016 together): the
buffer Stage 2 supplies is the whole input.
- **No file.** C and C++ preprocessing opens none: no `#include` target, and no file
  `__has_include` names, is ever found, wherever the process runs. Include directives
  stay imports, and the macros and conditionals a source defines itself still expand.
- **No clock.** `__DATE__` is `"Jan 01 1970"` and `__TIME__` `"00:00:00"`, in any time
  zone and locale; a source's own `#define` or `#undef` of either still wins.
- **Contract.** An extraction depends on the normalised bytes, the language, the path
  and the metadata only. It never depends on the working directory, a header on disk,
  anything outside the repository, the time, the zone or the locale.
- **Tests.** `engine/tests/c_family_input_isolation.c` (inotify on Linux),
  `engine/tests/c_family_date_time.c` and the `extract` tests prove it.

Resolution (`resolve::stages::resolve`) runs typed resolution over the whole
repository on every build, then settles each site in a fixed precedence: the B.4
blocklist first, even over a typed answer; then the engine (`typed` only through
`bands::from_engine` and a target that is exactly one definition; a target in no file
of the project is `external`, issue 45); then local bindings (`unresolved`) and
external-only bindings (`external`); then import-guided, inheritance-guided, exact and
scoped through `narrow`, which never lets a stage add a candidate or an empty stage
erase one; then `candidate` or `unresolved`. Engine hints seed candidates and never
restrict them; `exact` is the repository's one definition of the name, never a
narrowing to several; `scoped` is the module, never a directory the matrix does not
call a module. `typed_only` sites, references and engine-found sites are call sites
only when typed resolution settles them. A `Resolution` can only be made in a shape
its band means. Nothing here makes a graph id: targets are `DefinitionRef`s.

Stage 4 (`index::derive`) is where facts get persistent identities, from 4.2.1's
fields only: never a line, offset, call index, insertion order or checkout location,
and every path repository-relative POSIX (a host-native path stops the stage, issue
53). One map from `DefinitionRef` to node serves every part. A module is `(Module,
scope, "<language>:<name>", "")`, never keyed or parented by a member file; a spanning
one has no `file_id`, sorted `props.files` and the nearest common folder as parent;
symbols keep their physical parents and carry `props.module` (issue 48). Drawn
resolutions become a site and a `CALLS` or `CALL_REFERENCE` edge, others a site and a
candidate row, with the engine's numbers copied unchanged; unconfirmed sites and sites
with no position in the file are counted, never stored (issue 49). Test files follow
issue 31's rules (`LANGUAGE_MATRIX_VERSION` 2); a definition is a test only on its
framework's evidence, never because the engine flags its file, and `TESTS` edges sit
beside the `CALLS` edges they mirror. Routes (Spring, JAX-RS, FastAPI, Flask, Express)
key on their handler's file and `{"handler","method","path"}` JSON, never a line, and
every `DEFINES_ROUTE` edge has its `route` site (the declaring annotation, decorator or
call); no site, no route (issue 54). Every structural edge has its site: `Builder`
and the segment writer refuse one without (`EdgeKind::requires_site`). An entry point
is Appendix B.2's, decided here (tests, route handlers, each language's conventional
`main`, the framework bootstraps); the engine's own flag is `props.engine_entry_point`
and never makes one (issue 55). Segments are
`SEGMENT_SCHEMA_VERSION` 3: version 2 added `candidates.engine_candidates` and nullable
`blob_sha` and `line_count` for files never read (issues 50 and 51); version 3 added the
required meta key `repo_id_source`, exactly `url` or `registered` (issue 71, decision
40). With `url`, the writer and reader refuse a `repo_id` that is not
`repo_id_from_url(repo_url)`; a `registered` id is never compared with the URL. Older
segments are refused and re-indexed, never migrated. The golden fixtures are `insta`
snapshots in `crates/pdx-core/tests/snapshots/`: review a changed one, never accept it
blind.

Contracts (`pdx_core::contracts`, P2-08) are Stage 4's last part and its
`DerivedGraph::contracts`: `contracts` rows only, never also nodes or contract edges
(issue 59). Each source (`openapi`, `routes`, `endpoints`, `proto`, `messaging`,
`channels`, `tables`, `artifacts`) reports `ContractObservation`s; only `ContractAccumulator` makes a
`namespace_key` and id, and merges (direction lattice, strongest identity, one
provider owner or `owner_ambiguous`, sorted evidence sets). The id is
`NodeKey::estate(kind, namespace_key)`; `namespace_key` is `{"identity","key"}` JSON
when established, `unresolved:<repo_id>:<key>` otherwise, an artifact's coordinate
itself (issue 58, vectors in 4.7). Routes come from P2-07's `Route` nodes, never a
second reading of the source. Documents are read through `contracts::source` (Stage
2's checks, blob and digest re-verified; a changed checkout stops the stage) and
parsed with `saphyr-parser` (aliases never expanded) and `roxmltree` (no DTD), issue
60; nothing is fetched, run or resolved. `.env*` is never opened: placeholders resolve
only from `application*.yml|properties`, one eligible value, never a credential key's
(issue 38). A library call counts only by provenance (the file imports it; a
parameter's declared type) and shape, never by a receiver's name; a call resolution
draws to the repository is its own. The 4.7.1 table is the target: issue 62 classifies
every source as implemented (`contract_source_conformance_4_7_1`) or needing a named
missing fact (Kotlin call arguments 63, base-class type arguments 64, local value flow
65, deployment/estate identities 66); engine channel facts need a Kafka or AMQP library
import (issue 61). A malformed document is a `ContractDiagnostic`, never a guess. The
accumulator and the segment writer apply one row validator (`contracts::row::check`):
canonical namespaces, artifacts exact, unresolved scoped to the segment's repository.

Layer roles and metrics (P2-09) are Stage 4's last parts, after contracts.

- **Roles** (`layers::roles`) go on `Module` and `Class` nodes only, always `Some`
  (`unknown` when nothing decides). The tiers are `pdx.toml [layers]` rules in order
  (`globset`), then framework evidence with provenance, then whole path segments in
  4.8.3's set order, then a test path via `is_test_path`.
  - Framework provenance: the name imported from the framework, or written qualified
    by it; never a bare name. FastAPI and Flask come from Route nodes, SQLAlchemy from
    P2-08 table-contract owners. JPA `@Entity` never decides (issue 64).
  - No `HAS_ROLE`/`LAYER_DEPENDS` edges and no `pdx-arch.yaml` here: those are
    P6-02's.
- **Metrics** (`metrics`) are `DerivedGraph::metrics`, rows sorted by (node, metric).
  `Rows` refuses duplicates and non-finite values.
  - Symbols only: never the file-level module (the File node) or an inline module.
  - Complexity is the engine's, for callables only, with `cyclomatic` = 1 + the
    engine's count (issue 67).
  - `transitive_loop_depth` is the max reachable over drawn `CALLS`, using an SCC
    walk; recursive symbols get `props.recursive = true`.
  - Fans count drawn `CALLS` weights only.
  - `importance` takes unknown visibility as 0.6. The engine's visibility is
    evidence only in Go and Python, so `props.visibility` is stored only there.
    This is ratified as PDX 2.0's behaviour (issue 68, decision 41, option B): no
    engine change, and the limitation is on the trust page (4.13.2). Visibility in
    another language is an SCR with engine, cache, protocol and surface moves.
  - A static test keeps both modules free of fs, net, process, env, clock and
    randomness.

The whole pipeline (P2-10) is `index::build_segment(IndexRequest) -> SegmentReport`.

- **Composition.** It composes the stages and duplicates none of them. The request
  carries resolved worker and memory limits: `build_segment` never resolves
  `PDX_WORKERS`, `PDX_MEM_BUDGET_MB` or cgroup limits (its caller does). Stage 2's
  existing engine switches (isolation, `EXTRACTION_SWITCHES`) are untouched. Meta and
  destination are checked before Stage 1.
- **Unportable paths (issue 53).** Files whose path is not repository-relative POSIX
  are held back after Stage 1 (`discover::split_unportable`). Discovery may already
  have read them, as it reads any file; Stages 2 to 5 never see them. They have no
  identity and no row, are counted as skipped in coverage, and are listed in the
  report.
- **Coverage** (`coverage::rows`) counts Stage 3's resolutions, never persisted rows.
  Unconfirmed sites are not call sites. All 11 bands are stored, summing to
  `call_sites`. Binary and redacted files count in `files` only; `symbols` excludes
  the file-level module. The writer refuses inconsistent rows.
- **Degraded** is decided once (`index::report::assess`): `20 × (failed +
  skipped(size)) > files`, then engine crash, memory skip, truncation, lost work.
  `partial` is never a reason (issue 57). A worker timeout is fatal.
- **Tracing and progress.** `tracing` spans per stage carry names and counts only,
  never a path or source. The progress callback runs on the building thread. Neither
  changes the bytes (tests prove it).
- **Gotchas.** The repository's `.gitignore` ignores `coverage/` everywhere, so
  `crates/pdx-core/src/coverage/` needs its negation. A tracing test with
  `with_default` races other threads over callsite interest, so it lives in a test
  binary of its own.

The CLI (P2-11) is `crates/pdx`: `cli` (clap builder API, exit codes), `repository`
(Git metadata and identity), `resources` (Stage 2 limits), `local` (output and cache
paths), `report` (JSON and text reports, progress).

- **Ownership.** P2-11 owns CLI parsing, building `IndexRequest`, Git metadata policy,
  resource-limit resolution, the JSON report DTO, exit codes, TTY progress and the
  coverage reader. `build_segment` stays the only pipeline: never re-implement a
  stage in `pdx`.
- **Exit codes** are 0 ready, 2 degraded, 1 for every error. Usage errors are caught
  so they never leak clap's own 2. Results go to stdout; errors (`pdx: …`, one line)
  and progress (only when stderr is a terminal) go to stderr.
- **Clean HEAD only.** `pdx index` indexes HEAD and refuses a dirty tree (untracked,
  not ignored files included; decision 38); `.pdx/` belongs in `.gitignore`.
- **Identity** comes from origin's HTTPS URL via `canonical_clone_url` and
  `repo_id_from_url` (decision 36). SSH and other remotes are refused, never guessed
  or echoed. `pdx.toml [server]` refuses (P4-06 owns registered ids). The name comes
  from the URL, never the checkout directory. The segment records
  `repo_id_source = url` (`IndexRequest::repo_id_source`, decision 40), and both JSON
  reports carry it; `registered` is P4-06's, and there is no switch for it.
- **Git** is run as `git -C <path> …`: no shell, GIT_DIR and the other routing
  variables cleared, no prompt, bounded stdout, stderr never shown.
- **Limits** (decision 37): workers are the min of `available_parallelism`, the
  cgroup quota (floor, at least 1) and `PDX_WORKERS`. Memory is `PDX_MEM_BUDGET_MB`,
  else 60% of the cgroup limit, else `DEFAULT_MEMORY_BUDGET_BYTES` (512 MiB). Bad
  settings are errors.
- **Output.** The default is `<root>/.pdx/segment.db`; a relative `--out` is below the
  root. A `.pdx` symlink or file is refused, and a segment is never overwritten.
- **Reports.** The JSON report carries no machine state (workers, budget, cache,
  paths, time); it is snapshotted in `tests/cli.rs`.
- **The hidden worker** is served before parsing, unchanged.
- **Test fixtures** are real Git repositories with fixed author, committer, dates and
  autocrlf, and with `GIT_CONFIG_GLOBAL` pointed at an empty file.

The legacy comparison (P2-13) is `scripts/compare-legacy.py`, tested network-free by
`scripts/test_compare_legacy.py` (part of `make test`), and its triage report
`bench/results/legacy-compare.md`. It runs `legacy/` through its own Compose file as
the isolated project `pdx-p2-13` and never modifies it; the legacy tree stays
read-only. The comparison is pair-level, legacy endpoints are mapped without guessing,
and the legacy graph is never an oracle (decision 46). It is not part of `make check`
or the nightly build.

The golden harness (P2-12) is `crates/pdx-bench` (`golden::{plan, cache, git, index,
expected, run}`) and `make golden`.

- **Pins.** Only `bench/repos.lock`'s `[[golden]]` entries, never `[[scale]]`, each at
  its exact pinned commit. The commit is fetched by its id over `https://<lock url>`
  into the disposable `bench/.cache/repos/`, and is never followed by a branch.
- **Git.** Runs without a shell, with no prompt, an empty global configuration of the
  harness's, no system configuration, `core.hooksPath` pointed at an empty directory,
  no submodules, and LF checkouts. Nothing in a fetched repository is ever executed.
  Symlinks in the cache or `bench/golden/` are refused.
- **Index.** The real `pdx` beside `pdx-bench` runs
  `pdx index <checkout> --out <bench/.cache/segments/…> --profile structural --json`.
  Only a ready index (exit 0) makes a golden; degraded (exit 2) fails the run.
- **Golden.** Read with `SegmentReader::nodes_all`/`edges_all`, no SQL in the harness.
  The format is decision 44: every `NodeKind` with its count (zeros included) sorted by
  spelling, and every stored drawn edge (`Band::is_drawn`, duplicates kept) as
  `(src_qn, dst_qn, kind, band)`, sorted by that tuple. No ids, paths, source,
  timestamps or machine state.
- **Accept.** Comparison never writes `bench/golden/`. `--accept` stages every golden
  under `bench/.cache/generated/` and promotes them only when all succeeded. A changed
  golden needs a justification in the commit body.
- **Tests.** `cargo test` never touches the network: `golden_files_sorted` reads the
  committed files, and fetch tests use local repositories.
- **Baseline.** `bench/golden/` holds one golden per `[[golden]]` entry, all ten
  ready, generated by engine 6 (issues 74 to 76) and unchanged by engine 7 (issue
  83). A golden is never edited by
  hand: a change is `--accept`ed, reviewed and justified in the commit body.
- **Ownership census.** `scripts/owner-census.py <segment> <checkout>` runs the
  engine's diagnostic `owner_census` (`make engine`) over a segment's files and counts
  their sites by owner from the engine's own declaration occurrences; `orphan` must be
  0 (issue 76).

The determinism and performance baselines (P2-14, decision 49) are `pdx bench
--determinism | --perf`, `make determinism` and `make perf`.

- **Launcher.** `pdx bench` runs `<dir of pdx>/pdx-bench bench …` and passes its exit on
  (0, or 1 for an error or a determinism failure). `pdx` never links `pdx-bench`,
  Criterion or `sysinfo`; `scripts/open-binary-check.sh` fails if its normal dependency
  tree holds one.
- **Worker.** Every build is a fresh `pdx-bench __pdx-bench-worker` process (served
  before clap, in no help): one in-process `build_segment`, its identity derived from the
  lock URL as `pdx index` derives it, so its segment is `pdx index`'s. One JSON answer;
  stage times come from the progress events, five stages once each in order.
- **Refused.** The six extraction switches, `PDX_ENGINE_ISOLATE`, `PDX_WORKERS` and
  `PDX_MEM_BUDGET_MB`, by name, never value. Perf uses `available_parallelism` workers
  and `DEFAULT_MEMORY_BUDGET_BYTES`, recorded.
- **Determinism** is the ten goldens only: cold with 1 worker, an independent cold cache
  with max(2, available) workers, then that cache warm. All three must be ready and
  byte-identical, cold caches unread, the warm one read for every stored extraction.
- **Perf** is goldens and `[[scale]]` (Kubernetes, Linux; `indexed_only` required):
  three fresh-process cold-cache samples. Throughput is `Finished(Extract)` files over
  extract + resolve time; medians, the maximum peak RSS, hashes equal, every sample kept.
- **RSS.** Sampled from the parent with `sysinfo` every 10 ms. Above the 6 GiB ceiling
  the worker is stopped: a golden fails, a scale repository records `out_of_memory`
  (Decision 49's P2-14 measurement policy; issue 82: Stage 2 retains about 330 KB per file, so Kubernetes and
  Linux do not complete on this machine).
- **Results** are `bench/results/<version>/<repo>.json` and `summary.json`, results
  format 1. Canonical JSON, provenance without branch or path, staged then promoted all
  or nothing, mode-merged, unknown fields kept, stale sections dropped, stale unknown
  fields refused.
- **Criterion** benches in `pdx-core` (dev-dependency only): `resolve` and
  `segment_write` on a generated Python fixture. `tests/bench_fixture.rs` keeps their
  guards (2,406 sites, five bands) true under `make check`.
- **CI.** Normal CI does not run network performance benchmarks: `determinism` and
  `perf` are in `check-full` only. On a small machine run them in a capped scope above
  the ceiling (`MemoryMax=7G`).

The engine differential (P2-16, decision 50) is `make engine-differential`: the vendored
engine against the pinned, unmodified reference engine, row for row, over every corpus
of `bench/engine-differential.toml`.

- **Corpora.** Every typed-resolution fixture (Kotlin, PHP and Perl ones were added, so
  every typed-tier language has one) and 2 to 10 related files of every typed-tier
  golden, as committed paths at the lock's pin (C/C++ files at most 32 KiB, where patch
  0012's budget is the reference's). `pdx-bench differential-corpora` checks them against
  the matrix and the lock and fetches the goldens with P2-12's machinery.
- **Observation.** Each file's extraction once the cross-file typed pass has written into
  it: `engine/tests/differential_dump` for ours; for the reference, a hook
  (`scripts/engine/differential_hook.c`) linked into its own build with `--wrap` around
  its calls pass, its names translated by `scripts/vendor/rename-map.txt` inverted, built
  outside the tree by `scripts/engine/reference-build.sh` (`PDX_REFERENCE_JOBS`,
  `PDX_REFERENCE_EXTRA_FLAGS`; on this machine zlib's headers come from an unpacked
  `zlib1g-dev`). One row printer, `engine/tests/differential_rows.h`, common fields only.
- **Surfaces** are the cross-file definitions of the one shared surface codec: the
  reference's as stored, ours from a run whose files came from exported surfaces, via the
  internal test accessor `pdxe_project_debug_lsp_surfaces`.
- **Canonical form.** Only the project wrapper becomes `@`; the kind comes from
  `normalise_kind` in `engine/api/pdxe.c`, read from that file. Multisets, sorted, no
  allowlist, no accept. Both engines get the same bytes, given by absolute path, the
  same language per file (the reference configured through its own extension
  configuration) and the same metadata.
- **Working directory.** Files around an engine are no input (decision 52). Each engine
  runs, per corpus, from a fresh empty directory of the harness's own, outside the stage,
  the corpus and the repository (`hermetic_cwd`). P2-16 ran both in the corpus root,
  which exposed issue 83; a corpus-root run is no longer comparable, since the reference
  still reads included files from there.
- **Tests.** `engine_differential_identical` (crates/pdx-bench) and
  `scripts/engine/test_differential.py` run in `make check`; the live comparison never
  does. All 32 corpora were identical at P2-16; at engine 7, jq's and fmt's differ in
  their macro-expanded rows by the headers the reference still reads, and issue 83's
  closure is held for the driver.

`bench/corpus/` is the sanitizer corpus: one directory per engine language ID, at most
200 small project-authored files, every one extracted by `make check-asan`. A file
named `recovery_*` may parse partially and `failed_*` must fail; anything else must
parse. Never copy third-party or scale-repository source into it.

The engine's fault-injection switches (crash on, hang on or skip a named file) exist
only in test builds: CMake option `PDXE_TEST_SEAMS`, Cargo feature `test-seams` of
`pdx-engine-sys`, enabled from `[dev-dependencies]` only, never from
`[dependencies]`. `scripts/no-test-switches.sh <binary>` checks a binary has none.

`make vet` runs `cargo vet --locked` against `supply-chain/` with the cargo-vet version
pinned in `scripts/cargo-vet.sh` (install it with `scripts/cargo-vet.sh install`); CI,
nightly and release run it. A new or changed crate needs a real audit, an owner-approved
import, or a version-specific exemption with an `ISSUES.md` entry; never regenerate the
exemptions to make it pass. The 95 exemptions recorded when vetting began are not
audits (issue 36), and neither are the four exact-version ones P2-04 added (issue 39),
P2-07's two (issue 52), P2-08's three parser crates (issue 60), P2-10's three
tracing crates (issue 69), P2-11's four clap crates (issue 70) or P2-14's 31 Criterion
and `sysinfo` crates, with `syn` 2.0.119 raised to `safe-to-deploy` (issue 81): 142
exemptions.

Engine-specific targets: `make engine-test` (the interface tests),
`make engine-typed-reference` (re-record the reference engine's answers for the
typed-resolution fixtures; needs the pinned checkout), `make engine-differential` (the
engine against the reference engine, nightly; needs the pinned checkout), `make vendor-verify` (a
refresh at the pinned commit must reproduce `engine/` exactly), `make vendor-refresh`.

Requires Rust 1.98.1 (pinned in `rust-toolchain.toml`), gcc 13 or clang 17+,
cmake 3.25+, Node 22 and pnpm 10. Put `~/.cargo/bin` and
`~/.local/share/pnpm` on PATH.

## The plan is not in the repository

The implementation plan is a single external document, currently at
`! Version 2.0/PDX-2.0-PLAN-v1.3.3.md` and **git-ignored**. It must stay
uncommitted: it names the reference repositories that code is vendored or ported
from, and the provenance scanner (plan Part 5.10, built in P0-04) fails the build
on those names anywhere in the tree except `THIRD_PARTY_NOTICES.md` and the
`LICENSE` files inside vendored directories. Committing the plan would fail the
project's own first gate.

The same rule applies to anything written into the tree: no upstream repository,
product, or author name in code, comments, tests, docs, commit messages, CLI help
or UI strings. When unsure whether a string is provenance, treat it as
provenance.

## Tracking

`docs/plan/` is the trackable projection of the plan. Read `docs/plan/README.md`
first. In short:

- `tasks.json` / `TASKS.md` — derived: 98 tasks across 10 phases, with size,
  dependencies, mandated acceptance-test names, normative execution order, and
  fan-out classification. Regenerate, never edit.
- `STATUS.md`, `GATES.md`, `ISSUES.md`, `DECISIONS.md`, `../lint-exceptions.md` —
  live records. Never regenerated.
- `amendments.json` — live: corrections the extractor applies to a task's projected
  text where a later record supersedes the plan's wording. The way to correct
  `TASKS.md`; never edit the derived files.
- `PROGRESS.md` — derived roll-up: counts per phase, gate readiness, next
  actionable tasks.

```
python3 scripts/plan/extract-plan.py "! Version 2.0/PDX-2.0-PLAN-v1.3.3.md"   # needs the external plan
python3 scripts/plan/plan-progress.py                                          # validates + rolls up
```

The second script also validates: a task missing from `STATUS.md`, an unknown
status, a task done with no commit, a task done before its dependencies or gate,
or a blocked task with no issue entry all fail it. Run it at every phase boundary
and at the start of every session.

Statuses are lower case (`todo`, `in-progress`, `done`, `blocked`) — the
upper-case marker words are on the provenance deny-list.

## Working rules that bite early

- Execution order is not phase order: P7-01…P7-06 → P8-01…P8-10 → P7-07…P7-12 →
  P7-14 → P8-11 → P9. There is no P7-13 and no P2-15.
- One task per branch `task/<task-id>-<slug>`; acceptance tests first; `make check`
  green; update `STATUS.md`; commit as `<task-id>: <task title>`; fast-forward.
- Unfinished work goes in `docs/plan/ISSUES.md` with a task reference, never as a
  marker comment in code.
- A blocked task does not stall the plan: record the issue, continue with the next
  task that does not depend on it.
- Only the driver session evaluates gates, and only the driver edits `STATUS.md`,
  `ISSUES.md`, `DECISIONS.md`, `GATES.md`, the shared constants, the segment
  schema, the migrations, the MCP tool schemas and the notices file.
- The plan's one human artefact is the reviewed precise sample truth set
  (`bench/precise-sample/<lang>/truth.jsonl`). The agent generates the samples and
  opens an issue requesting review; only G7 requires the truth sets.
