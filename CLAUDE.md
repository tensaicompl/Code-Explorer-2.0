# Code Explorer 2.0 (PDX 2.0) — repository context

Deterministic code-intelligence platform: indexes many repositories into
immutable versioned graphs, resolves relationships with an explicit confidence
band on every edge, links repositories through contracts, and serves the result
to AI agents over MCP and to a GPU-rendered architecture map.

**Remote:** `tensaicompl/Code-Explorer-2.0` (public, `origin`). The predecessor
repo `tensaicompl/praxevia-explorer` is kept as the remote `praxevia-explorer`.

## CURRENT DRIVER HOLD

After issue 94, **A0 / gate G1b is next** (decision 64, issue 95): A0.1, a
measurement-only structural-accuracy audit of the Python and Rust goldens against a
semantic oracle; A0.2, counterfactual re-scoring of that one dataset; A0.3, the
driver's decision. A0 changes no resolver behaviour, band, blocklist order, extraction,
secret detector, contract reader, discovery or output policy, and is not P5-06.

**Do not start issue 93's Phase B, issue 82 or P3 until the driver releases this
hold.** Do not merge `main`. The plan's projection is unchanged (98 tasks, 28 done):
never hand-edit `docs/plan/tasks.json`, `TASKS.md` or `PROGRESS.md` for A0.

A0.1 and A0.2's results are in `bench/results/a0/`; `methodology.md` there explains
them. They are produced by:

- `pdx-bench audit-sites`, which writes the product's own Stage 1 to 3 decisions as a
  site dataset;
- `scripts/a0/`, the stdlib-only audit: a SCIP reader, the language parsers' call
  sites, rules M1 to M6, 4.13.1's measures, the counterfactuals, and `run.py`, which
  reruns everything (`test_a0.py` runs in `make test`);
- `bench/a0/sanity/`, the oracle sanity fixtures.

The oracles (`scip-python`, `rust-analyzer`) run from an isolated work directory
and are never project dependencies. A0 itself never edits resolver behaviour.

**A0.3 is decided (decision 65, 2026-10-14); G1b stays pending, with four of six
floors failed.**

- `typed`, `import-guided` and `inheritance-guided` stay drawn.
- `exact` and `scoped` short-name uniqueness is no longer trusted graph truth
  (issue 102, a specification change to 4.2.2, implemented as R3: they are weak
  evidence, never drawn).
- A valid typed answer comes before the blocklist (issue 103, a specification change
  to 4.5 Stage 3, amended with the implementation).
- Recall is a first-class blocker (issues 104 to 108).
- Rule: draw a `CALLS` edge only from positive target evidence, never because every
  other same-name target disappeared.

Remediation order, **each step only when the driver authorises it**:

| Step | Issue | Work |
|---|---|---|
| R1 | 103 | typed before the blocklist |
| R2 | 104 | Rust typed false positives |
| R3 | 102 | `exact` and `scoped` not drawn |
| R4 | 107A | Rust turbofish and other ordinary site holes |
| R5 | 105 | Rust candidate classification, then fixes |
| R6 | 106 | Python candidate classification, then fixes |
| R7 | 108 | overload identity |
| R8 | 107B | macro token-tree policy |
| R9 | 96 | secrets |
| R10 | 97 | SQL prose contracts |

**R1 is done (issue 103 resolved, 2026-10-14).** A qualifying typed answer now comes
before the blocklist.

**R2 is done (2026-10-14); issue 104 closed with issue 109 (2026-10-15).** Rust typed answers need path
evidence (patch 0028, below). Of the Rust golden's 17 trusted false positives, 15 are
no longer drawn wrong. The last typed one, a blanket impl, is issue 109, and the
`import-guided` one, a `macro_rules!` impl, is issue 110.

**Hotfix 112 is done (2026-10-14).** Patch 0028 changed what Rust extraction records
while `ENGINE_VERSION` stayed 7, so a cache filled by engine 7 before it was served
after it under the same key. `ENGINE_VERSION` became 8: what engine 7 cached is a miss.

**Issue 111 is done (2026-10-14).** Rust `use` declarations are read from the syntax
tree (patch 0029, `ENGINE_VERSION` 9), restoring the typed answers R2 lost to `use`
lists the resolver misread. It found issues 113 (paths through `crate::` and
re-exports; an imported type's impls) and 114 (`use` has no lexical scope). The engine
differential holds one ratified divergence for it (decision 66).

**Hotfix 115 and issues 110 and 114 are done (2026-10-14).** `scripts/a0/run.py`
refuses an executable whose versions are not the source's (115). A member a
`macro_rules!` invocation may write another of its name for is no target (110, patch
0030, `ENGINE_VERSION` 10): the Rust golden's `import-guided` false positive is gone. A
Rust `use` binds only in its lexical scope, in the typed pass and Stage 3 alike (114,
patch 0031, `ENGINE_VERSION` 11, cache format 8, worker protocol 9): a test module's
import no longer draws a module's own call wrong. The engine differential holds a second
ratified divergence for 114 (decision 66).

**Issue 109 is done (2026-10-15).** A Rust `Type::method` is answered with a trait impl's
method only when the impl is proven to apply: extraction records each trait impl's header
and the typed pass proves every predicate for the path's types (patch 0032,
`ENGINE_VERSION` 12, surface 6). The Rust golden's last typed false positive is gone. Its
site (`with_rejection.rs:196`) now falls to Stage 3's `scoped` guess, still the wrong
impl: issue 109 removes the false trusted typed answer, and the weak `scoped` guess is
issue 102's (R3) and known wrong until R3, an R3 acceptance case. The engine differential
holds a third ratified divergence for 109 (decision 66).

**Issue 117 is done (2026-10-15).** Stage 3's trusted name stages take a Rust trait
impl's member only when its impl is proven to apply: `import-guided` when the impl's
header requires nothing, `inheritance-guided` also when the impl the call on `self` is
made from entails it (below). The headers issue 109 records now cross the interface
(cache format 9, worker protocol 10); the engine's extraction is unchanged
(`ENGINE_VERSION` 12, surface 6) and the engine differential with it. The goldens'
trusted bands are unchanged.

**Issue 118 is done (2026-10-16).** PDX itself's four false `import-guided` edges
(`Performance::OutOfMemory(..)` drawn to the struct `OutOfMemory`, and three alike) came
from a Rust `use` brace group read as its prefix's module: every name the group binds was
taken for that module, so `T::f` became `prefix::f`. Each binding of a group now leads
where its own path does (below). The four fall to `exact`, still wrong, R3 acceptance
cases; every trusted band of the goldens and PDX itself has no false edge (Rust golden
`import-guided` 213/213, PDX 261/261). Stage 3 only: no version moves.

**R3 is done (2026-10-16): issue 102 resolved.** `exact` and `scoped` are weak evidence:
ordered bands, measured and counted, that never draw an edge (below). The drawn
structural resolution bands are `typed`, `import-guided` and `inheritance-guided`, and
none has a known false edge in the goldens or PDX itself. Drawn recall falls, as decision
65 accepted: correctness first. A declared route's `DEFINES_ROUTE` edge, labelled
`exact`, stops drawing with it; its band is issue 119 (open).

**R4 is done (2026-10-16): issue 107A resolved, with issue 120.** 54 of the Rust
golden's 56 oracle relations with no structural site were turbofish calls
(`f::<T>()`, `x.m::<T>()`) the engine dropped; they are extracted now (patch 0033,
`ENGINE_VERSION` 13). The other 2 had sites all along: A0's matcher read a comment's `<`
as generic syntax (issue 120, `a0-match-2`). All 56 are at a site; 29 are drawn, every
one correct. Issue 107B (macro token trees) and issue 107 stay open.

**R5, the Rust recall package (2026-10-17): issue 105 remediated, 113A, 113B and 116
resolved.** The Rust golden's 3,151 relations at a structural site with no drawn edge
were classified first (`scripts/a0/rust_candidates.py`, committed as
`bench/results/a0/rust-candidate-classification.json`), then recovered by evidence only:
paths read as Rust reads them, declared parameter, field and return types,
constructors, `async` and `?` as they are, impl proofs on every dispatch path (patch
0034, `ENGINE_VERSION` 14, below). Rust golden recall 23.33% → 81.05% with every drawn
band 100% precise; 788 relations left, each classified (issue 105 stays open for them).
113C's calls are answered and its naming left open. Do not start issues 106, 108, 107B,
119, 96, 97, 93B, 82 or P3 until the driver authorises them.

**R5 is saved, not closed (2026-10-25).** Committed as a progress save at the driver's
request, with four driver decisions still open, so `make engine-test` and
`make engine-differential` fail by design until they are made (decision 66: never add
a record to make a run pass):
1. the engine differential's 81 new `typed` rows (47 exact candidate records: 33 issue
   113B caller renames, 7 unresolved rows under the path's identity, 3 rows an unknown
   receiver no longer writes, 2 new answers, 2 `Meter.read`), not ratified;
2. the `rust_workspace_*` resolve fixtures' one extra typed edge, `main →
   alpha.Meter.read` (correct; `expected.tsv` is the reference's output and its
   comparator has no ratification);
3. A0 matching for PDX itself's `peak.peak()` sites in `assert_eq!` (2 drawn edges A0
   calls false, both to the method the oracle names at the method's token), which keeps
   the official three-corpus results (Python 28.82%, Rust 81.05%, PDX itself 88.02%) out
   of `bench/results/a0/`;
4. whether issue 105 stays open for its 788 classified residual relations.

**How work is validated (driver, 2026-10-16).** A change is proved by its targeted
tests (the code it touches, a few focused cases, the dangerous invariants' mutations),
one end-to-end product test (source → discovery → extraction → cache → resolution →
segment → the segment's queries), and a measurement of the real corpus it changes.
Build incrementally; never `cargo clean`. Full certification (`cargo test
--workspace`, full clippy, `make check`, `make vet`, every golden, the full three-corpus
A0, mutation campaigns, every differential corpus, Ubuntu/macOS/Windows CI) runs at
integration checkpoints, gate evaluations and releases, and at once when a change
touches the segment schema, a stored or non-backward-compatible cross-process
contract, secret normalisation, cache correctness, graph trust semantics or shared
multi-language infrastructure, or when targeted tests show cross-subsystem effects.
Prefer a few strong tests across several components to many narrow ones; keep useful
existing tests. One green exact-SHA CI run is enough; work packages group related
fixes into one commit.

Measure every change to resolution or extraction with A0 on the corpus it changes, and
report its typed precision, union precision and recall; `scripts/a0/run.py` (all three
corpora, the committed results) runs at integration checkpoints (above). Build `pdx-bench` first
(`cargo build -p pdx-bench`): the runner never builds it, and refuses before measuring
anything unless `pdx-bench versions` reports the source's engine version, `pdx_core`
constants and worker protocol (issue 115, resolved). A result's `engine_version` is the
executed engine's. A recall gain that draws low-confidence edges again is not accepted.
Not drawing `exact` and `scoped` lowers recall first; that is accepted, correctness
first. Since R3 (`a0-metrics-2`) the union and recall are the drawn bands' (`typed`,
`import-guided`, `inheritance-guided`); `exact` and `scoped` keep their own precision,
measured on the one candidate each keeps, and `union_with_evidence` is the union as it
was before R3, for comparison only.

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
| `crates/` | Ten crates, licence split enforced by `scripts/open-binary-check.sh`. `pdx-engine-sys` builds and links the engine and holds its raw bindings (`make bindgen` regenerates them); `pdx-engine` is the only way above it into the engine; `pdx-core` holds the specification's constants, the language matrix (`languages`) and the graph model's identities, bands, kinds and rows (`ids`, `bands`, `kinds`, `model`), segments (`segment`: writer and reader), `pdx.toml` (`config`), secret normalisation (`secrets`), the pipeline's stages as they arrive (`index`: discover, extract, derive) and resolution (`resolve`: the symbol registry, the blocklist, the stages); `pdx` is the CLI (`index`, `coverage`, `version`, `bench`, and the hidden `engine-worker`); `pdx-bench` is the benchmark harness (the lock files, the golden harness: `pdx-bench golden [--accept]`, the determinism and performance baselines: `pdx-bench bench --determinism --perf`, with its hidden worker, and the engine differential's corpora: `pdx-bench differential-corpora`; A0's site dataset, `pdx-bench audit-sites`, and the versions it was built with, `pdx-bench versions`); the rest are skeletons |
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
extraction cache is format 9 and the worker protocol 10 (issues 40 to 42, 46, 47, 54, 90,
110, 114 and 117): calls carry their node-type path (`ast_path`, computed by the engine since patch
0010) and arguments, and definitions their decorators, parameter types and every route
binding with its declaring node's position and path (`routes`, patch 0011); both hold
records whose text is ranges of the source (issue 90B, below), a Rust file the members
its macro invocations may write (issue 110), Rust calls their `use` scope (issue 114)
and a Rust trait impl's relation and methods its header, the methods their trait
(issue 117, all below); `ENGINE_VERSION` is 14 (issue 105). Issue 74 (decision 45, engine 4): the C and C++ expression
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

Issues 83 to 94 (decisions 51 and 53 to 63, engine 7, patches 0015 to 0027 together):
the buffer Stage 2 supplies is the whole input.
- **No file.** C and C++ preprocessing opens none: no `#include` target, and no file
  `__has_include` names, is ever found, wherever the process runs. Include directives
  stay imports, and the macros and conditionals a source defines itself still expand.
- **No clock.** `__DATE__` is `"Jan 01 1970"` and `__TIME__` `"00:00:00"`, in any time
  zone and locale; a source's own `#define` or `#undef` of either still wins.
- **No platform.** `sizeof` in `#if` uses one written-out LP64 table (long 8, long double
  16, pointers 8), never the build's own sizes; the test seam `PDX_ENGINE_TEST_HOST_SIZES`
  simulates another platform's. A one-byte character constant is a signed 8-bit char
  (`'\xff'` is -1), and a `#line` number is read at a fixed width and capped at
  4294967295. `preprocessor_host_independence` builds the library with char signed and
  unsigned.
- **No machine.** Preprocessing is bounded by the source (`preprocessor.h`'s
  `PDXE_PREPROCESS_*`): a forward jump of more than 4,096 lines is one `#line` marker;
  the bytes written and generated and the `#if` folding steps are held to
  `clamp(24 × len, 256 KiB, 8 MiB)`, generated tokens and expansions to 500,000 each, and
  nesting to 256. Passing one throws inside the work, discards the expansion and is lost
  work once (never cached, degraded). `#if` arithmetic is 64-bit two's complement
  written out, and integer literals are read by an explicit reader. No timeout and no
  environment override; a ceiling that starves real code is changed by decision, never
  to fit a test (Linux has 4 files past them, all amplification by design).
- **No path amplification.** A site's node-type path is complete or absent, never cut
  (it is part of the site's identity). One per-file budget (`PDXESitePathBudget`,
  `pdxe_site_path_alloc`) serves every producer: at most 1,024 entries a path and
  500,000 a file, checked before allocating; the first refusal is lost work once and no
  path is computed after it. A site without its path is counted, never stored.
- **No deep syntax.** A tree nested deeper than `PDXE_MAX_SYNTAX_DEPTH` (512, the root at
  1) is not extracted: `pdxe_syntax_depth_scan` runs after the parse and before any
  semantic pass, one cursor step a node; past it the result holds no fact, loses work
  once and skips cross-file resolution (`lsp_skipped`). The expanded C/C++ tree is held
  to the same depth, its pass skipped alone. One ceiling, not a fix per mechanism: the
  algorithms that grow with nesting are bounded by it, not made linear.
- **No parser overrun.** A grammar scanner's state between tokens stays within the
  parser's 1,024-byte buffer, each bound derived from it (YAML 253 indentation entries,
  Markdown 254 open blocks, Kotlin 1,024 string delimiters, Python 2 + delimiters + 2
  per indentation level). Past it the push is refused and its token not claimed, so the
  parse recovers; serializers check each complete entry, deserializers read complete
  entries within the length; no scanner aborts on content. Parsing runs before the
  depth ceiling, so that ceiling cannot be the protection. Vendored grammar changes are
  engine patches too (0023).
- **No depth² usage work.** The unified walk records a frame for every node it enters
  (`PDXEWalkFrame`), and usage classification answers every question about a node's
  ancestors from them, each answer computed once a frame; never `ts_node_parent`, which
  descends from the root, on that path. Each scope frame keeps the lexical scope in
  force. Answers are the climbs' exactly (the test seam `PDX_ENGINE_TEST_CLIMB_ANCESTRY`
  answers by climbing; the 104,730-file census extracts identically). Test builds count
  the ancestors examined and every `ts_node_parent` usage classification makes.
- **Contract.** An extraction depends on the normalised bytes, the language, the path
  and the metadata only. It never depends on the working directory, a header on disk,
  anything outside the repository, the time, the zone, the locale or the platform, and
  whether its preprocessing completes never depends on the machine.
- **Tests.** `engine/tests/c_family_input_isolation.c` (inotify on Linux),
  `engine/tests/c_family_date_time.c`, `engine/tests/c_family_sizeof.c`,
  `engine/tests/c_family_char_literals.c`, `engine/tests/c_family_preprocess_budget.c`
  and `preprocessor_bounds` (every ceiling at its edge), `engine/tests/site_path_budget.c`,
  `engine/tests/syntax_depth.c`, `engine/tests/usage_ancestry.c` (linear counted work,
  frames against climbs) and `engine/tests/scanner_state.c` (the scanners' own code:
  ceilings, guard bytes, every malformed length, end to end in child processes), the
  `extract` and `build` tests and `pdx-engine`'s `type_sizes`, `literals_and_lines`,
  `preprocess_bounds`, `site_path_budget`, `syntax_depth`, `usage_ancestry` and
  `scanner_state` (which run on Windows too) prove it.
- **Source-backed text (90B, decision 61, patch 0024).** Source evidence is held once:
  the normalised source's bytes plus bounded per-fact references.
  - Text a fact takes from the file crosses as `pdxe_text` (24 bytes): a range of the
    source the caller gave when it is exactly one run of it (callee, argument
    expression, usage name, route's declaring node, parameter list), owned text when
    synthesised or read after C-family macro expansion, never both. Arguments and
    parameter lists are ranges inside the engine too.
  - Qualified names are structural: whole, member, extended, or a `.`-bounded prefix of
    an earlier name the earlier definition adds (C++ functions in nested namespaces),
    then a rest; `pdxe_qualified_name` and `FileExtract::qualified_names` spell them.
  - Rust: `ExtractRecord` (serialised, ranges, no source) and `FileExtract` (a record
    bound to one `Arc<[u8]>`); `bind` checks length, digest, every range and every name,
    refuses rather than clamps. Read text with `FileExtract::text` (borrowed); never
    convert a range to a `String` in a loop. Range coordinates are equality within one
    extraction only.
  - Surface 5 (ranges, `source_len`, definition references, structural names; 6 since a
    trait impl's relation and methods carry its header, issue 109; 7 since a Rust impl
    method carries its declared return type and a function whether it is `async`, issue
    105), cache
    format 6 (records bound to the verified current source; a misfit is a miss),
    protocol 7 (the parent binds the worker's records to the source it sent). Identities
    hash the bytes the whole text or name gives: segment schema stays 3.
  - Tests: `engine/tests/source_text.c`, `pdx-engine`'s `source_text`, `pdx-core`'s
    `derive` identity vectors (`structural_names_…`, `prefix_names_…`,
    `source_ranges_…`) and the cache's and worker's record comparisons.
- **No quadratic lookup (issue 92, decision 62, patch 0025).** Typed resolution and the
  interface's translation find a definition, type, method, overload, import, usage,
  binding or row by an index keyed exactly as the old scan matched, never by scanning a
  file's or a registry's entries per question.
  - Every registry keeps exact-key chains as entries are added (functions and types by
    qualified name, methods by receiver and name, receiver-less functions by short
    name, functions by receiver, types by short name and last name segment), each in
    registration order, so a chain's head is the scan's first match and a walk meets
    the scan's matches in its order: duplicates keep their winners. Read them through
    `pdxe_registry_*_first_by_*` / `_next_by_*`; never loop over `reg->funcs` or
    `reg->types` for a key.
  - The helpers index what they scanned (`lsp/name_index.h`, `lsp/site_index.h` for
    arrays that only grow); import, include and use arrays grow by doubling; the
    interface's `index_of_qn` is gone (`qn_positions`), and answers join through an
    index of the file's rows (`pdxe_pipeline_find_lsp_resolution_in_rows`). Stage 3
    reads `name_provenance` by file and name and C++ nesting by type name.
  - An index that cannot get memory finds nothing, the failure lost work; it never falls
    back to a scan. No answer depends on hash order.
  - Python's registered imports are read by local name through one index
    (`PyLSPContext::imports_by_local`; issue 94, decision 63, patch 0027): replaying each
    import statement, an imported reference's exact candidate and a call's alias check.
    Never loop over `import_count` to find a name; apply the scan's other conditions to
    the chain's entries, which are in registration order.
  - Test builds: `PDX_ENGINE_TEST_LINEAR_LOOKUP_REFERENCE` answers by the old scans,
    `lookup_counts.h` counts the work, `PDX_ENGINE_TEST_REFUSE_LOOKUP_INDEXES` refuses a
    family of indexes. `engine/tests/typed_lookup.c` proves answers equal and work
    linear; never assert a time in a test.
- **No positional navigation (issue 93, patch 0026).** tree-sitter's lookups by
  position cost the position: a child by index walks from the first, a parent descends
  from the root, a sibling finds the parent and walks its children. In an error node the
  parser's recovery builds (one flat run holding the rest of a file) each costs the
  position itself, so a pass doing any of them per child is quadratic in the file.
  - Step through children with `PDXE_FOR_EACH_CHILD` / `PDXE_FOR_EACH_NAMED_CHILD`
    (`node_iter.h`): exactly `ts_node_child`'s and `ts_node_named_child`'s children, in
    order, `it.index` the position, `pdxe_child_iter_field_name` the field; leaving the
    loop by `break`, `return` or `goto` gives the cursor back. Never `ts_node_child(n, i)`
    with a variable `i`, nor `ts_node_field_name_for_child(n, i)`.
  - In reverse, or by position: collect once (`pdxe_collect_children`) and index the
    collection. A whole subtree in source order: `PDXE_TREE_WALK`.
  - Parents and siblings: `pdxe_node_parent` and `pdxe_node_{prev,next}[_named]_sibling`,
    never `ts_node_parent` or tree-sitter's sibling lookups. They answer from the walk's
    registered path (`pdxe_walk_path_*`; the unified walk, the definitions walk, the tree
    walk register theirs) or from a loop in progress, and give tree-sitter's answers. A
    new walker whose handlers ask for parents or siblings registers its path or steps
    through children with the iterator.
  - A collection that cannot get its memory is lost work; never fall back to a positional
    walk.
  - Test builds count navigation (`lookup_counts.h`: children by position weighted by the
    position, cursor steps, parent and sibling lookups weighted by the children passed,
    field lookups) and can refuse collections (`PDX_ENGINE_TEST_REFUSE_LOOKUP_INDEXES=
    children`). `engine/tests/wide_nodes.c` proves the answers tree-sitter's and the work
    linear in every language's wide shapes.
- **Open.** Issues 88, 90, 91, 92 and 94 are resolved. Issues 86, 87 and 89 are fixed,
  and their closure stays held: issue 93 is open. Its extraction side is fixed (patch
  0026), but the vendored parser's own error recovery is quadratic: JavaScript after an
  unclosed parenthesis parses in about 94 s at 2 MiB (`ts_parser__recover` rebuilding
  the error node). Changing the parser is the driver's decision. Engine blockers: issues
  82 and 93.

Issue 104 (decision 65's R2, patch 0028; engine 8 since issue 112): Rust typed answers
need path evidence.
- **Path, not name.** The Rust resolver's fallback took the project's one function with
  a call's last name, whatever the path said, and reported it typed. It now answers only
  with the one function of that name in the module the call's path names, or for a bare
  name the path a `use` or a glob imports it by, read as Rust reads it (`crate`, `self`,
  `super`, an imported head, a crate's name with `-` as `_`, an inline module of the
  file). A name unique by itself gets no answer. The label stays the reference's
  `lsp_short_name_unique`, so the differential compares those answers row for row.
- **Locals.** A `match` arm's pattern binds its names in the arm, and a function a block
  declares is bound in the block; extraction marks a call of such a function
  `callee_is_locally_bound`, so it is `unresolved`, never a namesake.
- **External crates.** Stage 3 makes a Rust path `external` when its first name is a
  crate the extern prelude binds (the standard distribution, or a dependency with no
  repository path) and nothing in the file binds it (4.5, step 4).
- **Resolved since.** A blanket impl the engine could not tell from a type's own is issue
  109's, and items a `macro_rules!` invocation writes issue 110's, both below.
- **Rule (issue 112).** A patch that changes what extraction records, in a file's facts
  or in the per-file typed answers its surface holds, moves `ENGINE_VERSION`, delivered
  or not: the version in the cache key is all that keeps an older engine's entries from
  being served. `cache_entries_of_engine_seven_are_never_served` holds it for 0028,
  `cache_entries_of_engine_eight_are_never_served` for 0029,
  `cache_entries_of_engine_nine_are_never_served` for 0030,
  `cache_entries_of_engine_ten_are_never_served` for 0031,
  `cache_entries_of_engine_eleven_are_never_served` for 0032 and
  `cache_entries_of_engine_twelve_are_never_served` for 0033 and
  `cache_entries_of_engine_thirteen_are_never_served` for 0034 (planted in the current cache
  format, so that the engine version alone keeps it out).

Issue 111 (patch 0029, engine 9): a Rust file's `use` declarations are read from its
syntax tree, never its text.
- **Use trees.** Every `use_declaration`'s `argument`, at the top and in inline modules'
  bodies, in source order: nested and multi-line lists, comments in them, aliases
  (`self as o` binds the prefix), `{self}` (the prefix's last name), globs (their whole
  module), a leading `::`, `_` (binds nothing). A visibility (`pub`, `pub(crate)`,
  `pub(in …)`) is not part of the path. The reference reads the text, so
  `pub(crate) use unknown::Unknown;` is its path `pub(crate) use unknown::Unknown`:
  axum's one typed row that differs is ratified (decision 66).
- **Withheld.** A name the file's own `use` trees bind to two different paths is no
  evidence. Imports a caller bridges in come after them, first match.
- **Lookup.** By local name through `RustLSPContext::uses_by_name` (`name_index.h`),
  never a scan; `PDX_ENGINE_TEST_LINEAR_LOOKUP_REFERENCE` answers by the scan.
- **Alias.** The fallback looks for the function by the last name of the import's path;
  a call by its alias is answered `lsp_import_alias`, joined by the name it spells.
- **Not here.** Paths through `crate::`, re-exports and `index.rs`, and an imported
  type's impls, are issue 113.
- **Tests.** `crates/pdx-core/tests/rust_use_trees.rs` (every form, ambiguity, external,
  no evidence) and the cache test above.

Issue 110 (patch 0030, engine 10, option 110-B): an item a `macro_rules!` invocation
writes is no definition, and a type's members the registry holds are not taken for all
of them.
- **Recorded.** Each Rust file's extraction carries `macro_members`: for every invocation
  of a definition it sees in its own file (textual scope), every `impl` of the
  definition's transcribers, the type by the last name of its path and the name of every
  function directly in its body; a metavariable is its binding in each rule expansion's
  matcher matches, else (no rule matched, or bound in a repetition) any identifier of the
  invocation's arguments; a generated name (`fn [<…>]`) is any member; an invocation
  in an impl body writes that impl's type's members. Read from the definition's tokens,
  nothing expanded: it may over-approximate, never under. One per type and member.
- **Withheld.** `SymbolRegistry::macro_may_write(type, name)`, across the repository by
  type name. A Rust member a macro may write another of its name for (the type the
  call's path names, or the member's declaring type) is no target, typed or by a stage:
  the site is a `candidate` with it (`stages::member_unproven`). Never by blocking a name
  (`from`) or a type, never a library's name in production code.
- **Not represented.** Generated definitions are not added (110-A rejected: no identity
  of their own, the expansion is a subset matcher). Not followed: a definition in another
  file, a macro another macro's transcriber invokes, a macro outside the repository.
- **Versions.** Cache format 7, protocol 8, surface 5 (the fact is not in a surface:
  Stage 3 reads it from the record).
- **Tests.** `engine/tests/rust_macro_members.c`, `crates/pdx-core/tests/
  rust_macro_members.rs` (the issue's shape, bound type and member, a generated name,
  impl-body invocation, uninvoked macro, another language), `e9_…` in `rust_targets.rs`,
  `isolation_carries_macro_members` and the cache test above.

Issue 114 (patch 0031, engine 11): a Rust `use` binds only in its lexical scope.
- **Scopes.** One collector (`rust_collect_uses` in `engine/src/lsp/rust_lsp.c`) records
  the file's module (scope 0), each inline module's body (a module) and each block that
  declares a `use`, in source order, each with its parent; a block that declares none is
  no scope. Each scope holds its own bindings (indexed by local name) and globs. The
  typed pass sets the scope in force as its walk enters a block or an inline module; a
  macro's expansion, parsed from a buffer of its own, keeps its invocation's.
- **Lookup.** From the site's scope out through its parents; the nearest scope that binds
  the name decides, and the first module ends the search: a module is a namespace of its
  own, so an inline module never sees its parent's imports unqualified. It sees them
  through `use super::*` (a link to the enclosing module's bindings, after its own) or
  `use super::x` (the enclosing module's binding of `x`, when it imports one). A name one
  scope binds to two paths is withheld (issue 111's rule, now per scope); one name in two
  scopes is no ambiguity. A nearer glob that brings the name in shadows an outer `use`.
  The work is the site's scopes', never every scope's: no file-wide scan.
- **Extraction.** The same collector exports the scopes (`use_scopes`: parent, module),
  the `use` declarations inside every scope but the file's (`scoped_imports`, recorded
  as the file's own imports are) and each call's scope (`Call::use_scope`), placed by the
  call's exact occurrence; the typed pass places the calls it adds. A table not made
  whole is none (lost work), every call unplaced. The file's own `imports` are unchanged,
  so the surface, the engine's import edges and the cross-file pass are too. Interface
  (`pdxe_use_scope`, `pdxe_scoped_import`, `pdxe_call.use_scope`), cache format 8 and
  protocol 9; the surface stayed 5 (6 since issue 109).
- **Stage 3** (`SymbolRegistry::rust_imports_at`, `rust_name_provenance`): for a Rust
  site, the imports in force in its scope, never `imports_of` the file; a site placed in
  no scope sees none. A scoped import's `self` and a `super` within the file are read
  from the file's module.
- **Rule.** Never fix a missing answer by reading a function's or an inline module's
  imports as the file's, or a parent module's as its child's.
- **Tests.** `crates/pdx-core/tests/rust_use_scopes.rs` (S1 to S15, module boundaries,
  macros, the issue's two reproductions; typed and Stage 3 alone), `engine/tests/rust_use_scopes.c`,
  `typed_lookup` (scoped lookups linear), `pdx-engine`'s `use_scopes` and the worker's
  `isolation_carries_use_scopes`.

Issue 109 (patch 0032, engine 12): a Rust trait impl's method answers `Type::method`
only when the impl is proven to apply.
- **Header.** Extraction records each trait impl's header (`PDXEImplTrait::impl_bounds`,
  the same array on its methods' `PDXEDefinition::impl_bounds`; grammar in `pdxe_core.h`):
  the self type's form (`s:plain`, a path of distinct parameters; `s:blanket`; `s:other`),
  parameters, self and trait arguments, and every predicate's bounds (trait with its
  arguments, relaxed, lifetime, other); a macro or syntax error makes it opaque. It
  crosses the surface (version 6) and the cross-file definitions, and each registry reads
  it once (`PDXERustImplFacts`: `PDXERegisteredFunc::rust_impl`,
  `PDXERegisteredType::rust_impls`), a bound's trait made canonical as an impl's is.
- **Proof.** The path's type arguments (`Type::<A, B>`) stand for the self type's
  parameters. A relaxed or lifetime bound needs nothing (selection erases lifetimes); a
  trait bound on a parameter the path gives holds when the registry holds an impl of the
  trait for that type that requires nothing, its trait arguments the bound's or its own
  free parameters. Everything else is unknown: a parameter the path does not give (the
  trait's `S`), a projection, another bound, an opaque or missing header, a blanket
  impl. Unknown is never proven, and nothing is ever shown false: a macro's, a blanket's
  or another crate's impl may satisfy what looks unsatisfied.
- **Choice.** Several impls of the name: the proven one only when it is the only one
  proven and no other is an impl of its trait (two impls of one trait are chosen by the
  call's arguments, issue 108); an inherent method of the name beside a trait impl's
  leaves no answer. A trait's default reached through the type's one impl of it is proven
  the same way. A derive's synthesised method names no definition and is answered as
  before (`PDXE_FUNC_FLAG_RUST_DERIVED`). A type argument naming a generic parameter in
  scope (`RustLSPContext::generic_names`) is never a type.
- **Not here.** No Stage 3 channel for a withheld answer (driver decision): the issue's
  site falls to `scoped`, issue 102's (R3). Stage 3's trusted stages and an impl's
  header are issue 117's (below); cross-file `Type::method` through an import gets no
  typed answer at all (issue 113). The
  incremental cross-surface codec (`resolve/lsp_surface.c`) is not changed: the engine
  differential compares it key by key, and resolution here reads the definitions.
- **Tests.** `engine/tests/rust_impl_bounds.c` (headers, T1 to T7, the issue's and the
  differential's shapes), `crates/pdx-engine/tests/impl_bounds.rs` (fresh and cached
  alike, the header in the surface, surface 5 refused), `crates/pdx-core/tests/
  rust_impl_bounds.rs` (Stage 3, a warm cache), `rust_targets.rs`'s E6 (the R3 case) and
  E8, and the cache test above.

Issue 102 (R3, decision 65; Stage 3 and 4 only, no version moves): `exact` and `scoped`
are weak evidence, never drawn.
- **Bands.** `Band::is_drawn` is `precise`, `typed`, `import-guided`,
  `inheritance-guided`; `Band::is_weak_evidence` is `exact`, `scoped`. Names, order,
  `confidence_rank`, coverage and A0 are unchanged; nothing is renumbered.
- **Shape.** A `Resolution` of a weak band has no target and exactly one candidate
  (`InvalidResolution::Weak` otherwise); `target()` is for drawn bands only,
  `weak_candidate()` for weak ones. `narrow` returns `Narrowed::Weak` when a non-drawn
  stage leaves one candidate (the zero-result rule is unchanged); issue 110's check
  still turns a weak one into `candidate`.
- **Rows.** No `CALLS`, `CALL_REFERENCE` or `TESTS` edge; a `candidates` row, band
  `candidate`, the one candidate, a reason beginning `exact:` or `scoped:` (4.3; no
  schema change, decision 65 (4)). Coverage counts the band.
- **Elsewhere.** Contracts take only drawn resolutions as internal or as proof of a stub
  call; an Express handler keeps its weak evidence and band (`scoped` for the name in
  the file), so its edge is not drawn; a declared route's edge is `exact`, not drawn
  (issue 119). Goldens hold drawn edges only, so every `exact` and `scoped` tuple left.
- **A0.** `a0-metrics-2`: `DRAWN` is the three, `EVIDENCE` the two, measured on the
  candidate kept; the union and recall are the drawn ones; `union_with_evidence` and
  policy C7 are the pre-R3 view; wrong weak evidence is `wrong_weak_evidence` in
  `mismatches.jsonl`, never a false positive.
- **Rule.** Never draw an edge from a name's uniqueness, in the repository or a module;
  a stronger band needs its own measurement and specification change.
- **Tests.** `crates/pdx-core/tests/weak_evidence.rs` (R3-1 to R3-10 through a built
  segment), `bands.rs`, `stages.rs`, `rust_targets.rs`'s E6 (`with_rejection.rs:196`),
  `rust_import_bindings.rs` (issue 118's shapes), `issue_104_sites.rs` (no tuple in the
  axum golden), `golden_expected.rs`, `audit_sites.rs`, `scripts/a0/test_a0.py`.

Issue 118 (Stage 3 only, no version moves): a name a Rust `use` brace group binds leads
where its own path does.
- **Record.** The engine records one import per `use`, a group's whole text. Stage 3's
  record keeps the group's target, its prefix's module (import edges and provenance read
  it), and holds each binding's own (`ImportRecord::binding_targets`, read through
  `target_of`): `use a::{T, b::f, self as m}` binds `T` to `a::T`, `f` to `a::b::f`, `m`
  to `a`; nested groups under their enclosing path; an alias to the path it renames. A
  single-name `use` has none: its target is its binding's.
- **Import stage.** `import_visible` reads the binding's own target, so `T::f` through a
  group is a member of `T`, never the prefix module's `f`. An enum's variants are no
  definitions: a call of one has no `import-guided` target (it falls to `exact`, R3's).
- **Rule.** Never read a name an import binds as anything but what its own path names;
  never draw a namesake for a target the registry does not represent.
- **Tests.** `crates/pdx-core/tests/rust_import_bindings.rs` (E1 to E6, modules, aliases,
  nested groups, `self`, a macro-written type, the record).

Issue 107A (R4, patch 0033, engine 13): a Rust call whose function carries a turbofish
is its function's call.
- **Grammar.** In the pinned grammar the call's `function` is a `generic_function`, whose
  own `function` field is the path (`call_expression` → `generic_function` →
  `identifier`/`scoped_identifier`) or the method (→ `field_expression`). The callee lookup
  knew no `generic_function`: no callee, no call (the reference engine drops it too).
- **Fix.** `unwrap_rust_turbofish_callee` (`engine/src/extract_calls.c`, Rust only: Scala
  has a `generic_function` too) in the callee lookup, the primary callee expression and
  issue 104's local-binding check. The call is recorded exactly as without the type
  arguments; its callee's range runs from the call's start to the function's name.
- **Not here.** A generic path or method named without a call is no call; a call in a
  macro's tokens (`wrap!(f::<u8>())`) is none: a token tree holds no call expression, and
  macro token trees are issue 107B's. Never recover a call by reading source text.
- **A0 (issue 120, `a0-match-2`).** M1 skips Rust comments and literals: a `<` in one is
  no generic syntax, a `//` in a string no comment. Metrics stay `a0-metrics-2`.
- **Tests.** `engine/tests/rust_generic_calls.c` (every call and usage against its twin
  with the turbofish blanked, field by field), `crates/pdx-core/tests/rust_generic_calls.rs` (A1
  to A14, the golden's shapes, callee ranges), `crates/pdx/tests/rust_generic_calls.rs`
  (end to end through `pdx index`, cold and warm cache, the segment's queries, the
  twin's graph), the cache test above, `scripts/a0/test_a0.py` (the matcher;
  `bench/a0/issue-107a-sites.json`, the 56 relations by class).

Issue 105 (R5, patch 0034, engine 14; with issues 113 and 116): a Rust call's target is
drawn only on positive evidence, and that evidence is read as Rust reads it.
- **Classified first.** `scripts/a0/rust_candidates.py` classifies, from A0's rows, every
  Rust golden relation at a structural site with no drawn edge (16 classes, the primary
  the first that applies; recoverable by structure, propagation or proof, or unknown),
  from rust-analyzer's syntax tree, the oracle's signatures and the engine's typed rows.
  `bench/results/a0/rust-candidate-classification.json` (summaries, engines 13 and 14)
  and `rust-candidate-relations.jsonl` (rows); `test_a0.py` recomputes the summaries.
- **Paths (113A).** The cross-file pass builds one index of the repository's Rust module
  files (`PDXERustModuleIndex`, `pxc_build_rust_modules`, set around the pass like the
  manifest): each file's module, child directory, parent, crate root and `use` scope, its
  items by qualified name, crates by directory name (`-` as `_`). A path naming nothing
  registered as written is read as Rust does (`rust_resolve_path_expr_ex`,
  `rmi_path_at_site`): `crate`, `self`, `super`, the `use` in force (a nearer scope's glob
  before an outer `use`), a child file, an item, a module's own `use`/`pub use` bindings,
  exactly one glob, a crate's name; memoised, depth-bounded. Signature types the same
  (`rmi_type_in_module`). An `index.rs` module's items are found under the names
  extraction gives them; its naming (113C) is unchanged.
- **Receivers.** A field read as a value is its declared type, never a method of the
  name; a method call is its method's declared return type (impl methods record it,
  `rust_return_type`, surface 7; Rust fields cross as `rust_field_defs`); an `async fn`
  returns `core.future.Future<T>`, which `.await` alone unwraps; `?` unwraps `Result` and
  `Option` alone; `(x,)` is a tuple; a turbofish call is its function's. `self`/`Self` in
  `impl<S> T<S>` are `T`; a `Self::` path is read through the impl's type as written.
- **Proof.** Issue 109's proof on every dispatch path: a trait impl's method by method
  call, a trait's default through the type's one impl of the trait, a method a whole path
  names (`lsp_direct`). A `where` predicate bounding the receiver's type by the method's
  trait withholds the impl's method (rustc takes the predicate's). Unproven:
  `impl_unproven`, no answer.
- **Callers (113B).** An impl's methods are callers named as extraction names their
  owner: the file's module and the impl's type as written.
- **Labels.** A free function only the index finds keeps the label of the reading that
  also finds it (`lsp_cross_crate`, `lsp_short_name_unique`, `lsp_import_alias`), else
  `lsp_direct`.
- **Stage 3.** A Rust typed answer whose qualified name a field and a method share names
  the method (`answered_definition`); two callables stay no answer (issue 108). Issue
  116: a method call's receiver is a value, never external by its first name's import.
- **Not here.** A project `Deref` impl's target (not recorded), a generic bound's trait
  method, 113C's naming, macro token trees (107B), same-name identity (108), the route
  band (119). Never type a receiver by a method's name, a naming convention or the only
  definition left.
- **Tests.** `crates/pdx-core/tests/rust_receiver_evidence.rs` (E1 to E9),
  `crates/pdx/tests/rust_recall.rs` (end to end, cold and warm), `rust_trusted_impls`'s
  check that typed resolution draws nothing Stage 3 withholds, `weak_evidence` (sites
  typed resolution cannot answer), `cache_entries_of_engine_thirteen_are_never_served`,
  `test_a0.py`'s classification.

Issue 117 (no patch, engine 12): Stage 3's trusted name stages take a Rust trait impl's
member only when the impl is proven to apply.
- **Exposed.** The facts issue 109 records cross the interface as they are:
  `pdxe_definition` gains `impl_trait`, `impl_bounds`, `n_impl_bounds`, `pdxe_impl_trait`
  `impl_bounds`, `n_impl_bounds` (`Definition::impl_trait`, `impl_bounds`,
  `ImplTrait::impl_bounds`); `pdxe_result_build` validates and copies them. Cache format
  9 and protocol 10 (records and answers carry them); `ENGINE_VERSION` 12 and surface 6
  stay (nothing extraction records changed), schema 3.
- **Read once.** `resolve::impls::ImplHeader` reads the entries in the grammar
  `pdxe_core.h` gives, never source text. The registry keeps each trait impl's method's
  header (`trait_impl_of`: `None` for no trait impl's member, `Unrecorded`, `Header`) and
  each relation's (`relation_headers(type, trait)`).
- **Proof** (`stages::impl_unproven`, Rust, `import-guided` and `inheritance-guided`
  only). A trait impl's member is a target when its header requires nothing (a path of
  distinct parameters, or one, and only relaxed or lifetime bounds); for a call on
  `self` or `Self` over the target's own type (`inheritance-guided`), also when the call
  is made from the same impl (one file, type, trait spelling and header, the header
  writing the self type whole: `s:other` never does) or from a trait impl in the same
  file whose own predicates hold every trait bound of the target's, its parameters
  renamed by the self types' positions (`ImplHeader::entailed_by`). A trait's own
  method reached through the type's impls of it is a target when one of those impls is
  proven so. No header recorded, an opaque one, another kind of bound, a subject no
  parameter maps: unknown, never proven. Unproven is a `candidate` holding the member,
  as issue 110's are. Inherent members are as before (an inherent impl's header is not
  recorded).
- **Rule.** Never by blocking a name, a trait or a type, never by reading the source
  again: a trusted stage takes a trait impl's member on no less evidence than the typed
  pass would (issue 109).
- **Tests.** `crates/pdx-core/tests/rust_trusted_impls.rs` (I1 to I8, the reproductions,
  calls on `self`, the same impl, `Cell<u8>` against `Cell<u16>`, no header recorded),
  `resolve::impls`' unit tests, `engine/tests/rust_impl_bounds.c`'s interface round trip,
  `abi_result_build_roundtrip.c`, `pdx-engine`'s `impl_traits`, the worker's
  `isolation_carries_impl_headers`, and the cache vectors of format 9.

Resolution (`resolve::stages::resolve`) runs typed resolution over the whole
repository on every build, then settles each site in a fixed precedence: a qualifying
typed answer first (`typed` only through `bands::from_engine` and a target that is
exactly one definition, whatever the callee's name, issue 103, or in Rust the one
callable of a field and a method sharing its qualified name, issue 105); then the B.4
blocklist, which no engine hint passes; then the engine's other answers (a target in
no file of the project is `external`, issue 45); then local bindings (`unresolved`) and
external-only bindings (`external`, and a Rust path starting with an extern-prelude
crate, issue 104); then import-guided, inheritance-guided, exact and
scoped through `narrow`, which never lets a stage add a candidate or an empty stage
erase one (a Rust member a macro may write, issue 110, or a trait impl's not proven to
apply, issue 117, is a `candidate`); then `candidate` or `unresolved`. `exact` and
`scoped` are weak evidence (issue 102): `narrow` gives `Narrowed::Weak`, the site keeps
its band and its one candidate (`Resolution::weak_candidate`), never a target, and Stage
4 writes a `candidate` row whose reason begins `exact:` or `scoped:` (4.3's
`candidates.band` names neither), never an edge. Engine hints seed candidates and never
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
  allowlist, no accept (decision 66's exact ratifications aside, below). Both engines get
  the same bytes, given by absolute path, the same language per file (the reference
  configured through its own extension configuration) and the same metadata.
- **Working directory.** Files around an engine are no input (decision 52). Each engine
  runs, per corpus, from a fresh empty directory of the harness's own, outside the stage,
  the corpus and the repository (`hermetic_cwd`). P2-16 ran both in the corpus root,
  which exposed issue 83; a corpus-root run is no longer comparable, since the reference
  still reads included files from there.
- **Ratified divergences (decision 66).** An amendment to decisions 50 and 52, not a
  replacement, and not an allowlist. `bench/engine-differential-divergences.json` holds
  records, each one exact canonical row pair (or one row and an explicit `null`) in one
  category of one corpus, with its id, issue, decision and reason. The rows are captured
  from the live run's `.canonical` files, never written from prose; the file is canonical
  JSON and no run writes it. A corpus passes only when the rows that differ (D) and the
  rows ratified for it (R) are equal multisets: an unratified row fails, a ratification
  that no longer occurs fails, multiplicity counts. The file's `reference_commit` must be
  the lock's pin: a pin change fails the run until every record is revalidated. Nothing
  matches by pattern, prefix, field, category or corpus, and a corpus with a ratified
  divergence is never reported identical. A new difference is a finding for the driver,
  never a record added to make a run pass.
- **Tests.** `engine_differential_identical` (crates/pdx-bench) and
  `scripts/engine/test_differential.py` run in `make check`; the live comparison never
  does. All 32 corpora were identical at P2-16 and are again at engines 7 and 8, each engine
  run from an empty working directory (issue 83, decision 52). At engine 13, 31 are
  identical and `golden:rust/axum` passes with 17 ratified divergences: 3 `typed` (issue
  111's, 114's and 109's) and 14 `calls` (issue 107A's turbofish calls, which the
  reference does not extract; `issue-107a-axum-core-…`, record `issue` 107). At engine
  14 (R5) those 17 still occur, 29 corpora are identical, and 81 new `typed` rows in
  `golden:rust/axum` and the two `rust_workspace_*` fixtures await the driver's
  ratification (above).

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
