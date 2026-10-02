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
(`pdx_core::segment`), the repository configuration (`pdx_core::config`) and Stage 1,
discovery (`pdx_core::index::discover`). P1 vendored the engine, built it,
gave it its interface, bound it to Rust (`pdx-engine-sys`), and wrapped it safely
(`pdx-engine`: owned extractions, typed resolution with run health, crash-isolated
extraction), built and tested on Linux, macOS and Windows, and run nightly under the
address, undefined-behaviour and leak sanitizers over a committed corpus; it also
transcribed the language matrix (`pdx_core::languages`). The previous
implementation is in `legacy/`, read-only until the retirement task removes it. There is no pipeline and no product behaviour yet.

Always read `docs/plan/PROGRESS.md` for the current position rather than trusting
this paragraph.

| Path | State |
|---|---|
| `crates/` | Ten crates, licence split enforced by `scripts/open-binary-check.sh`. `pdx-engine-sys` builds and links the engine and holds its raw bindings (`make bindgen` regenerates them); `pdx-engine` is the only way above it into the engine; `pdx-core` holds the specification's constants, the language matrix (`languages`) and the graph model's identities, bands, kinds and rows (`ids`, `bands`, `kinds`, `model`), segments (`segment`: writer and reader), `pdx.toml` (`config`) and the pipeline's stages as they arrive (`index`); `pdx` has only the hidden `engine-worker` command so far; the rest are skeletons |
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

`bench/corpus/` is the sanitizer corpus: one directory per engine language ID, at most
200 small project-authored files, every one extracted by `make check-asan`. A file
named `recovery_*` may parse partially and `failed_*` must fail; anything else must
parse. Never copy third-party or scale-repository source into it.

The engine's fault-injection switches (crash or skip a named file) exist only in test
builds: CMake option `PDXE_TEST_SEAMS`, Cargo feature `test-seams` of
`pdx-engine-sys`, enabled from `[dev-dependencies]` only, never from
`[dependencies]`. `scripts/no-test-switches.sh <binary>` checks a binary has none.

`make vet` runs `cargo vet --locked` against `supply-chain/` with the cargo-vet version
pinned in `scripts/cargo-vet.sh` (install it with `scripts/cargo-vet.sh install`); CI,
nightly and release run it. A new or changed crate needs a real audit, an owner-approved
import, or a version-specific exemption with an `ISSUES.md` entry; never regenerate the
exemptions to make it pass. The 95 exemptions recorded when vetting began are not
audits (issue 36).

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
