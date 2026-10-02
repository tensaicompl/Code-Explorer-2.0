# Code Explorer 2.0

Understand a system by looking at it. Code Explorer indexes every repository in an
estate into an immutable, versioned graph, resolves relationships with an explicit
confidence band on each one, links repositories through the contracts between
them, and serves the result to engineers and to their AI agents.

## What it does

- **Indexes** each repository at each commit into an immutable graph of files,
  symbols, calls, imports, inheritance, contracts, infrastructure and metrics.
- **Builds it deterministically** — no model in the loop, so it runs unattended and
  produces the same graph twice, byte for byte.
- **Draws only what it can justify.** Every edge carries a confidence band.
  Ambiguous call sites are withheld rather than guessed at, and everything
  withheld is counted and published as data, so a gap is visible instead of silent.
- **Links repositories** through their APIs, message channels, database tables and
  build artefacts, producing a system-level graph rather than a per-repository one.
- **Serves the graph to agents** over MCP, with repository permissions mirrored
  from the Git host and every call audited.
- **Shows the system** as an architecture map with semantic zoom and no dataset
  cap, alongside a dependency matrix, hotspots, contract flows and neighbourhoods.
- **Measures it**: architecture-rule violations, cycles, blast radius per change,
  contract drift, hotspots, ownership, freshness.
- **Proves its own accuracy** on a trust page, per band and per language, measured
  against compiler oracles for every release.

## State

Under construction. The 2.0 implementation is being built from the ground up; the
previous implementation has moved to `legacy/`, where it is read-only reference
until it is removed.

Progress, the task breakdown and the gates are in [`docs/plan/`](./docs/plan/) —
start with [`docs/plan/README.md`](./docs/plan/README.md) and
[`docs/plan/PROGRESS.md`](./docs/plan/PROGRESS.md). Repository conventions and the
rules that bite early are in [`CLAUDE.md`](./CLAUDE.md).

There is no quick start yet, because there is not yet anything to start. The build
and verification entry points work today:

```
make check        # format, lint, build, test — green before anything merges
make check-full   # adds the accuracy, determinism and browser suites
make vet          # every third-party crate is covered by supply-chain/ (cargo vet)
```

## Layout

| Path | Contents |
|---|---|
| `crates/` | The Rust workspace: model, pipeline, resolution, segments, agent surface, benchmark, command line, and the enterprise crates |
| `ui/` | The web interface |
| `ada-indexer/` | A separately packaged indexer for Ada, run as its own process |
| `bench/` | Pinned repositories, committed corpus, expected graphs and results |
| `deploy/` | Chart, compose file, indexer images and operational notes |
| `docs/` | Specifications, plan tracking, user and operations documentation |
| `legacy/` | The previous implementation, read-only, awaiting removal |
| `scripts/` | Build, vendoring, scanning and plan tooling |

`engine/` joins this list when the extraction engine is vendored.

## Licence

This repository is licensed in parts, and the split is deliberate:

- The core crates, the interface, the benchmark and the scripts are
  **Apache-2.0** — see [`LICENSE`](./LICENSE), which states exactly what it covers.
- The architecture model, the estate analytics, the server, the chart and the
  operations documentation are **proprietary** — see
  [`LICENSE-ENTERPRISE`](./LICENSE-ENTERPRISE).
- Third-party code, its licences and where those licences live are listed in
  [`THIRD_PARTY_NOTICES.md`](./THIRD_PARTY_NOTICES.md).

Code that has not yet been replaced remains under the enterprise licence. The
repository is private.
