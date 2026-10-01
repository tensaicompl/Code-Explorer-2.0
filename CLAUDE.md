# Code Explorer 2.0 (PDX 2.0) — repository context

Deterministic code-intelligence platform: indexes many repositories into
immutable versioned graphs, resolves relationships with an explicit confidence
band on every edge, links repositories through contracts, and serves the result
to AI agents over MCP and to a GPU-rendered architecture map.

**Remote:** `tensaicompl/Code-Explorer-2.0` (public, `origin`). The predecessor
repo `tensaicompl/praxevia-explorer` is kept as the remote `praxevia-explorer`.

## State: P1 in progress

P0 is done and gate G0 has passed; P1 has vendored the engine, built it, given it
its interface, bound it to Rust (`pdx-engine-sys`), and wrapped it safely
(`pdx-engine`: owned extractions, typed resolution with run health, crash-isolated
extraction), built and tested on Linux, macOS and Windows. The previous implementation
is in `legacy/`, read-only until the
retirement task removes it. There is no pipeline and no product behaviour yet.

Always read `docs/plan/PROGRESS.md` for the current position rather than trusting
this paragraph.

| Path | State |
|---|---|
| `crates/` | Ten crates, licence split enforced by `scripts/open-binary-check.sh`. `pdx-engine-sys` builds and links the engine and holds its raw bindings (`make bindgen` regenerates them); `pdx-engine` is the only way above it into the engine; `pdx` has only the hidden `engine-worker` command so far; the rest are skeletons |
| `engine/` | The vendored extraction and typed-resolution engine, its interface (`include/pdxe.h`, `api/`), patches and tests. Read `engine/README.md` first; never edit a vendored file in place |
| `ui/` | Vite + React + TypeScript scaffold, lint and tests green, no views yet |
| `bench/` | Pinned references, golden and scale repositories, pre-move tree snapshot |
| `ada-indexer/`, `deploy/` | Skeleton and placeholder; contents arrive in their phases |
| `legacy/` | The previous implementation. Do not fix, extend or import from it |

## Build

```
make check        # format, lint, build both feature sets and the engine, test, engine tests,
                  # open-binary check, interface lint and tests
make check-full   # adds accuracy, determinism, sanitizers, browser suites
make asan         # the engine's tests under the address, undefined-behaviour and leak sanitizers
```

The engine's fault-injection switches (crash or skip a named file) exist only in test
builds: CMake option `PDXE_TEST_SEAMS`, Cargo feature `test-seams` of
`pdx-engine-sys`, enabled from `[dev-dependencies]` only, never from
`[dependencies]`. `scripts/no-test-switches.sh <binary>` checks a binary has none.

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
