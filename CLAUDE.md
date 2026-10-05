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
site's band and target (`resolve::stages`), and Stage 4's first part, which derives
containment, modules, call sites and edges, tests, routes and entry points
(`pdx_core::index::derive`). P1 vendored
the engine, built it, gave it its interface, bound it to Rust (`pdx-engine-sys`),
and wrapped it safely (`pdx-engine`: owned extractions, typed resolution with run
health, crash-isolated extraction), built and tested on Linux, macOS and Windows,
and run nightly under the address, undefined-behaviour and leak sanitizers over a
committed corpus; it also transcribed the language matrix (`pdx_core::languages`).
The previous implementation is in `legacy/`, read-only until the retirement task
removes it. There is no whole pipeline (`build_segment`) and no product behaviour
yet.

Always read `docs/plan/PROGRESS.md` for the current position rather than trusting
this paragraph.

| Path | State |
|---|---|
| `crates/` | Ten crates, licence split enforced by `scripts/open-binary-check.sh`. `pdx-engine-sys` builds and links the engine and holds its raw bindings (`make bindgen` regenerates them); `pdx-engine` is the only way above it into the engine; `pdx-core` holds the specification's constants, the language matrix (`languages`) and the graph model's identities, bands, kinds and rows (`ids`, `bands`, `kinds`, `model`), segments (`segment`: writer and reader), `pdx.toml` (`config`), secret normalisation (`secrets`), the pipeline's stages as they arrive (`index`: discover, extract, derive) and resolution (`resolve`: the symbol registry, the blocklist, the stages); `pdx` has only the hidden `engine-worker` command so far; the rest are skeletons |
| `engine/` | The vendored extraction and typed-resolution engine, its interface (`include/pdxe.h`, `api/`), patches and tests. Read `engine/README.md` first; never edit a vendored file in place |
| `ui/` | Vite + React + TypeScript scaffold, lint and tests green, no views yet |
| `bench/` | Pinned references, golden and scale repositories, pre-move tree snapshot, the sanitizer corpus (`corpus/`) |
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
bearer token runs on over its `=` padding, issue 44). The extraction cache key is
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
`ENGINE_VERSION` is 3.

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
`SEGMENT_SCHEMA_VERSION` 2 (`candidates.engine_candidates`, nullable `blob_sha` and
`line_count` for files never read; issues 50 and 51). The golden fixtures are `insta`
snapshots in `crates/pdx-core/tests/snapshots/`: review a changed one, never accept it
blind.

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
audits (issue 36), and neither are the four exact-version ones P2-04 added (issue 39).

Engine-specific targets: `make engine-test` (the interface tests),
`make engine-typed-reference` (re-record the reference engine's answers for the
typed-resolution fixtures; needs the pinned checkout), `make vendor-verify` (a
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
