# pdx-bench

The benchmark harness: accuracy, timing and determinism. Today it runs the structural
golden graphs (P2-12); the oracle, determinism, performance and engine-differential
suites arrive with their own tasks.

Open. Apache-2.0; see `LICENSE` at the repository root.

## Commands

```
pdx-bench golden            # compare: fails on any difference, never writes bench/golden/
pdx-bench golden --accept   # replace bench/golden/ with what the index produces
make golden                 # builds pdx and pdx-bench (release), then pdx-bench golden
```

Run from the workspace root (the directory holding `Cargo.toml`, `bench/repos.lock` and
`crates/pdx-bench`). Exit codes: 0 every golden matched or was accepted, 1 anything else
(a usage error included).

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

The rest of `tests/` checks the repository itself: the lock files, the vendored
engine, the scanners, the specification documents and the supply chain.
