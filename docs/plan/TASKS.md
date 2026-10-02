# Task index — PDX 2.0

Derived from `PDX-2.0-PLAN-v1.3.3.md` (Part 6, Part 7, Appendix J) by
`scripts/plan/extract-plan.py`. Read-only: regenerate rather than edit.
Live status lives in `STATUS.md`; gate results in `GATES.md`.

98 tasks in 10 phases, 9 gates.

`Ord` is the position in the normative cross-phase execution order, which is
not phase order: P7-01…P7-06 run before P8-01…P8-10, then P7-07…P7-12, P7-14,
then P8-11, then P9.

## P0 — Foundations and repository restructuring

6 tasks (S 4 · M 2 · L 0)

| Ord | Task | Title | Size | Depends | Execution | Tests |
|---|---|---|---|---|---|---|
| 1 | `P0-01` | Legal baseline | S | — | single-agent | 1 |
| 2 | `P0-02` | Move as-is code to `legacy/` and lock references | S | — | single-agent | 1 |
| 3 | `P0-03` | Toolchain and workspace skeleton | M | P0-02 | single-agent | 1 |
| 4 | `P0-04` | Scanners and CI | M | P0-03 | single-agent | 1 |
| 5 | `P0-05` | Constants and schema versions | S | P0-03 | single-agent | 2 |
| 6 | `P0-06` | Specification documents | S | P0-03 | fan-out-eligible | 0 |

### P0-01 — Legal baseline

- **Size** S · **Depends** — · **Order** 1 · **Execution** single-agent
- **Deliverables** (1) `THIRD_PARTY_NOTICES.md` created with the MIT attribution owed for the dashboard code currently in `frontend/packages/dashboard` (the dashboard was ported from the MIT-licensed `the upstream reference named in the plan` project (attribution details in the plan); the agent fetches that repository's `LICENSE` file, records the copyright line, and adds the entry; if the fetch is impossible offli…
- **Acceptance** `scripts/licence-scan.sh` (P0-04) passes; a test `legal_files_present` checks the four files exist and `THIRD_PARTY_NOTICES.md` has ≥ 1 entry.
- **Named tests** `legal_files_present`

### P0-02 — Move as-is code to `legacy/` and lock references

- **Size** S · **Depends** — · **Order** 2 · **Execution** single-agent
- **Deliverables** `git mv` of `indexer/`, `backend/`, `frontend/`, `mcp-server/`, `e2e/` into `legacy/`; delete `enrichment/`; `legacy/README.md` stating the directory is read-only reference until P9; `bench/references.lock` with name, URL, commit SHA, licence SPDX id, fetch date for every row of Part 3.1 and for the pinned libadalang release (agent fetches each into `/tmp/ref/<name>` and records `git rev-parse HE…
- **Acceptance** `legacy/` tree equals the previous top-level tree (test `legacy_tree_snapshot` compares file lists); lock files parse (`bench/src/lock.rs` test).
- **Named tests** `legacy_tree_snapshot`

### P0-03 — Toolchain and workspace skeleton

- **Size** M · **Depends** P0-02 · **Order** 3 · **Execution** single-agent
- **Deliverables** `rust-toolchain.toml`; root `Cargo.toml` workspace with all crates of 5.2 created as empty libs/bins with `README.md`; `Cargo.lock`; `Makefile` with targets `fmt`, `fmt-check`, `lint`, `build`, `test`, `check`, `check-full`; `.gitattributes` marking `engine/grammars/**/parser.c` as `linguist-generated` and `-diff`; `.editorconfig`; `ui/` scaffold (Vite + React + TS strict, empty app); `ada-indexe…
- **Acceptance** `make check` passes on the empty workspace; `cargo tree -p pdx` (without features) contains no proprietary crate (test `open_binary_has_no_enterprise_deps` implemented as a script in `scripts/`).
- **Named tests** `open_binary_has_no_enterprise_deps`

### P0-04 — Scanners and CI

- **Size** M · **Depends** P0-03 · **Order** 4 · **Execution** single-agent
- **Deliverables** `scripts/provenance-scan.sh` and `scripts/provenance-denylist.txt` (Part 5.10); `scripts/licence-scan.sh`; `deny.toml` for `cargo deny` with the Part 3.4 allow-list; GitHub Actions workflows `ci.yml` (`make check` on Linux and macOS; Windows builds `pdx` only), `nightly.yml` (`make check-full`), `release.yml` (skeleton, completed in P9-03); pre-commit hook script.
- **Acceptance** a fixture file containing a denied string makes `provenance-scan.sh` exit non-zero (test `provenance_scan_detects`); the scan of the current tree passes.
- **Named tests** `provenance_scan_detects`

### P0-05 — Constants and schema versions

- **Size** S · **Depends** P0-03 · **Order** 5 · **Execution** single-agent
- **Deliverables** `crates/pdx-core/src/consts.rs` with every `CAPITALS` constant named in Part 4 and their values (constants are defaults; where Appendix F lists a `PDX_*` key of the same meaning, the environment overrides the default at runtime); `ui/src/consts.ts` mirror; `scripts/consts-sync.py` that fails if the two differ.
- **Acceptance** `consts_sync` script passes; unit test `consts_have_docs` asserts each constant has a doc comment.
- **Named tests** `consts_sync`, `consts_have_docs`

### P0-06 — Specification documents

- **Size** S · **Depends** P0-03 · **Order** 6 · **Execution** fan-out-eligible
- **Gate** G0 is evaluated after this task
- **Deliverables** `docs/spec/` files mirroring 4.2–4.14 (one file per section), copied from this plan and marked "normative; changes require an SCR".
- **Acceptance** `docs/spec/` contains 13 files; the provenance scan passes.

## P1 — Engine vendoring and FFI

7 tasks (S 2 · M 3 · L 2)

| Ord | Task | Title | Size | Depends | Execution | Tests |
|---|---|---|---|---|---|---|
| 7 | `P1-01` | Vendor scripts | M | P0-04 | single-agent | 0 |
| 8 | `P1-02` | Engine build | L | P1-01 | fan-out-eligible | 1 |
| 9 | `P1-03` | ABI layer | L | P1-02 | single-agent | 2 |
| 10 | `P1-04` | `pdx-engine-sys` | M | P1-03 | single-agent | 1 |
| 11 | `P1-05` | `pdx-engine` safe wrapper | M | P1-04 | single-agent | 2 |
| 12 | `P1-06` | Engine ASan job and corpus | S | P1-05 | fan-out-eligible | 0 |
| 13 | `P1-07` | Language matrix registry | S | P1-05 | single-agent | 3 |

### P1-01 — Vendor scripts

- **Size** M · **Depends** P0-04 · **Order** 7 · **Execution** single-agent
- **Deliverables** `scripts/vendor/fetch-engine.sh` (clones `engine-ref` at the locked commit into `/tmp/ref/engine-ref`); `scripts/vendor/copy-engine.sh` (copies only the paths of Appendix I.1 into `engine/src/`, `engine/lsp/`, `engine/grammars/<lang>/` for the Language Matrix languages of Appendix A, `engine/vendored/{ts_runtime,common,...}`, and the upstream `LICENSE` to `engine/LICENSE-ENGINE` and each grammar'…
- **Acceptance** running the four scripts in sequence on a clean tree produces `engine/` with no file containing a denied string (provenance scan passes); `engine/LICENSE-ENGINE` present; every `engine/grammars/*` has `LICENSE`.

### P1-02 — Engine build

- **Size** L · **Depends** P1-01 · **Order** 8 · **Execution** fan-out-eligible
- **Deliverables** `engine/CMakeLists.txt` building `libpdxe.a` from `engine/src`, `engine/lsp`, `engine/grammars/*`, `engine/vendored/*` with the flags of Part 5.1; removal (in `strip-engine.sh`, not by hand) of every source file whose symbols are unreferenced after the strip; `engine/README.md` (build instructions only, no provenance).
- **Acceptance** `cmake --build` succeeds on Linux (clang, gcc) and macOS with `-Werror`; `nm libpdxe.a` contains no symbol starting with the upstream prefix (test `engine_symbols_renamed`).
- **Named tests** `engine_symbols_renamed`

### P1-03 — ABI layer

- **Size** L · **Depends** P1-02 · **Order** 9 · **Execution** single-agent
- **Deliverables** `engine/include/pdxe.h` and `engine/api/pdxe.c` implementing Appendix D exactly: `pdxe_init`, `pdxe_shutdown`, `pdxe_extract_file`, `pdxe_result_free`, `pdxe_resolve_project_begin/add_file/run/end` (typed resolution across files), `pdxe_version`, `pdxe_language_id`; all results as flat, caller-owned arrays of C structs with UTF-8 byte offsets; error codes; no stdout/stderr output. Known from the…
- **Acceptance** C test `engine/tests/abi_smoke.c` extracts one fixture per Language Matrix language and asserts non-zero definitions; `abi_result_build_roundtrip`; `abi_no_output` test runs extraction with stdout/stderr captured and asserts they are empty.
- **Named tests** `abi_result_build_roundtrip`, `abi_no_output`

### P1-04 — `pdx-engine-sys`

- **Size** M · **Depends** P1-03 · **Order** 10 · **Execution** single-agent
- **Deliverables** `build.rs` invoking CMake (via `cmake` crate) and linking `libpdxe.a`; `bindgen` generated bindings for `pdxe.h` committed under `src/bindings.rs` (regenerated by `make bindgen`); feature `regenerate-bindings`.
- **Acceptance** `cargo test -p pdx-engine-sys` runs `abi_smoke` through the bindings on all three OSes in CI.
- **Named tests** `abi_smoke`

### P1-05 — `pdx-engine` safe wrapper

- **Size** M · **Depends** P1-04 · **Order** 11 · **Execution** single-agent
- **Deliverables** `Engine` (one per thread, `!Send`), `FileExtract { definitions, calls, imports, usages, type_refs, throws, read_writes, diagnostics }` as owned Rust structs; `ProjectResolver` wrapping the cross-file typed resolution with `add_file`/`run` returning `TypedResolution { site_ref, target_qn, score, strategy, candidates }`; crash isolation mode: when `PDX_ENGINE_ISOLATE=1`, batches run in a child proc…
- **Acceptance** `engine_isolate_recovers` test feeds a fixture that triggers a deliberate engine abort (`PDXE_TEST_ABORT_ON` env var honoured only in test builds) and asserts the remaining files are extracted; property test `extract_is_deterministic`.
- **Named tests** `engine_isolate_recovers`, `extract_is_deterministic`

### P1-06 — Engine ASan job and corpus

- **Size** S · **Depends** P1-05 · **Order** 12 · **Execution** fan-out-eligible
- **Deliverables** `make check-asan` building the engine with sanitizers and running extraction over `bench/corpus/` (a committed corpus of ≤ 200 small files across the Language Matrix, taken from the golden repos' permissively licensed sources or written by the agent).
- **Acceptance** nightly CI job green.

### P1-07 — Language matrix registry

- **Size** S · **Depends** P1-05 · **Order** 13 · **Execution** single-agent
- **Gate** G1a is evaluated after this task
- **Deliverables** `pdx-core/src/languages.rs` implementing Appendix A: id, extensions, shebangs, tier (`typed` or `structural`), module-naming rule, test-detection rule; `LANGUAGE_MATRIX_VERSION` = 1.
- **Acceptance** table-driven test `language_detection` for every extension in Appendix A; test `every_typed_language_has_engine_support` calls `pdxe_language_id` for each tier-typed language.
- **Named tests** `language_detection`, `every_typed_language_has_engine_support`, `pdxe_language_id`

## P2 — Core graph: identity, resolution, segments, `pdx index`

15 tasks (S 1 · M 10 · L 4)

| Ord | Task | Title | Size | Depends | Execution | Tests |
|---|---|---|---|---|---|---|
| 14 | `P2-01` | Model | M | P0-05, P1-07 | single-agent | 6 |
| 15 | `P2-02` | Segment writer and reader | L | P2-01 | single-agent | 4 |
| 16 | `P2-03` | Discover stage | M | P2-01 | single-agent | 4 |
| 17 | `P2-04` | Extract stage with cache | M | P1-05, P2-03 | single-agent | 4 |
| 18 | `P2-05` | Symbol registry | M | P2-04 | single-agent | 1 |
| 19 | `P2-06` | Resolution stages | L | P2-05 | single-agent | 5 |
| 20 | `P2-07` | Derive stage: containment, modules, routes, tests | L | P2-06 | fan-out-eligible | 5 |
| 21 | `P2-08` | Derive stage: contracts | L | P2-07 | fan-out-eligible | 7 |
| 22 | `P2-09` | Derive stage: layer roles and metrics | M | P2-07 | fan-out-eligible | 3 |
| 23 | `P2-10` | Coverage and write | M | P2-08, P2-09, P2-02 | single-agent | 3 |
| 24 | `P2-11` | `pdx index` CLI | M | P2-10 | single-agent | 3 |
| 25 | `P2-12` | Golden harness v0 | M | P2-11 | fan-out-eligible | 1 |
| 26 | `P2-13` | Legacy comparison (triage only) | M | P2-12 | single-agent | 0 |
| 27 | `P2-14` | Determinism and performance baselines | S | P2-12 | single-agent | 0 |
| 28 | `P2-16` | Engine differential harness (nightly) | M | P2-12, P1-07 | single-agent | 1 |

### P2-01 — Model

- **Size** M · **Depends** P0-05, P1-07 · **Order** 14 · **Execution** single-agent
- **Deliverables** `ids.rs` (4.2.1, with `repo_id_from_url`, `node_id`, `site_id`, `edge_id`), `bands.rs` (4.2.2 enum, ordering, `is_drawn`, `from_engine(score, strategy, candidates)` mapping), `kinds.rs` (4.2.3/4.2.4 enums), `model.rs` (`Node`, `Edge`, `CandidateSite`, `Contract`, `Metric`, `Coverage`).
- **Acceptance** `proptest` `node_id_ignores_lines`, `site_id_ignores_lines`, `site_id_changes_with_ast_path`, `overload_insert_does_not_renumber` (same qn/path/kind, different lines → same id); `band_order_total`; snapshot tests of serialised forms; `from_engine` table test covering every engine strategy string listed in Appendix D.3.
- **Named tests** `node_id_ignores_lines`, `site_id_ignores_lines`, `site_id_changes_with_ast_path`, `overload_insert_does_not_renumber`, `band_order_total`, `from_engine`

### P2-02 — Segment writer and reader

- **Size** L · **Depends** P2-01 · **Order** 15 · **Execution** single-agent
- **Deliverables** `segment/schema.sql` (4.3 verbatim); `SegmentWriter` (temp file, sorted batch inserts, FTS build, `VACUUM INTO`, read-only, `content_sha256`); `SegmentReader` (immutable open, typed queries in `queries.rs`: node by id, children, edges by src/dst with band filter, candidates by src, contracts by kind/key, coverage, fts search, snippet spans); `segment/verify.rs` (schema version check, foreign-key…
- **Acceptance** round-trip test `segment_roundtrip`; `segment_is_byte_identical_across_builds`; `reader_refuses_wrong_schema_version`; `fts_finds_qualified_names`.
- **Named tests** `segment_roundtrip`, `segment_is_byte_identical_across_builds`, `reader_refuses_wrong_schema_version`, `fts_finds_qualified_names`

### P2-03 — Discover stage

- **Size** M · **Depends** P2-01 · **Order** 16 · **Execution** single-agent
- **Deliverables** `index/discover.rs` per 4.5 Stage 1; `pdx.toml` parser (`config.rs`) with the `[discover]`, `[languages]`, `[layers]`, `[rules]`, `[precise.*]`, `[secrets]` sections and defaults.
- **Acceptance** fixture repo tests `discover_honours_gitignore`, `discover_skips_symlinks`, `discover_marks_binary_and_large`, `pdx_toml_defaults`.
- **Named tests** `discover_honours_gitignore`, `discover_skips_symlinks`, `discover_marks_binary_and_large`, `pdx_toml_defaults`

### P2-04 — Extract stage with cache

- **Size** M · **Depends** P1-05, P2-03 · **Order** 17 · **Execution** single-agent
- **Deliverables** `index/extract.rs` running `pdx-engine` over discovered files in `rayon` batches with the memory budget of 4.5; extraction cache trait `ExtractCache` with `FsCache` (local) keyed by `(engine_version, language_matrix_version, secret_policy_digest, blob_sha)` as specification 4.5 requires (the extraction also depends on the file's path and the node budget: issue 26, which P2-04 resolves before the key is final), postcard-serialised `FileExtract`; `blob_sha` computed as git blob hash (`sha1("blob <len>\0" + bytes)`) so it matches `git ls-files -s`.
- **Amended** from the plan's text by specification 4.5; issue 26; decision 19
- **Acceptance** `cache_hit_skips_engine` (engine call counter), `blob_sha_matches_git`, `memory_budget_batches`, `secret_policy_change_invalidates_cache`.
- **Named tests** `cache_hit_skips_engine`, `blob_sha_matches_git`, `memory_budget_batches`, `secret_policy_change_invalidates_cache`

### P2-05 — Symbol registry

- **Size** M · **Depends** P2-04 · **Order** 18 · **Execution** single-agent
- **Deliverables** `resolve/registry.rs`: definitions by name, by qualified name, by file; import graph per file (resolved to files/modules via Appendix A module rules); class hierarchies; module membership; external-name detection (names only defined in imports of external packages).
- **Acceptance** fixture tests per typed language (`registry_<lang>`), `external_detection_python_stdlib`.
- **Named tests** `external_detection_python_stdlib`

### P2-06 — Resolution stages

- **Size** L · **Depends** P2-05 · **Order** 19 · **Execution** single-agent
- **Deliverables** `resolve/stages.rs` implementing 4.5 Stage 3 exactly (blocklist → engine typed → import-guided → inheritance-guided → exact → scoped → candidate/external/unresolved; narrowing semantics); per-site output `Resolution { band, target?, candidates, engine_score, engine_strategy }`; `resolve/blocklist.rs` (Appendix B.4, per-language extensions).
- **Acceptance** table-driven `resolution_stage_matrix` covering all 9 stage outcomes (`typed`, `import-guided`, `inheritance-guided`, `exact`, `scoped`, `candidate`, `external`, `blocked`, `unresolved`; `precise`, `observed`, `contradicted` are produced elsewhere); `narrowing_keeps_narrowest_set`; `blocklist_precedes_all`; `typed_requires_lsp_typed_single_candidate_and_min_score`; `engine_hints_never_restrict_ca…
- **Named tests** `resolution_stage_matrix`, `narrowing_keeps_narrowest_set`, `blocklist_precedes_all`, `typed_requires_lsp_typed_single_candidate_and_min_score`, `engine_hints_never_restrict_candidate_universe`

### P2-07 — Derive stage: containment, modules, routes, tests

- **Size** L · **Depends** P2-06 · **Order** 20 · **Execution** fan-out-eligible
- **Deliverables** `index/derive/{containment,modules,routes,tests}.rs` per Appendix A/B: `Repo/Folder/File/Module` nodes, parent links, `Route` nodes and `DEFINES_ROUTE`, `Test` nodes and `TESTS` edges, entry-point flags (`main`, framework bootstrap classes, `Route` handlers) in `props.is_entry_point`.
- **Acceptance** golden fixture `derive_<lang>` for every typed-tier language and for `ada` (insta snapshots of node kinds and counts); `routes_spring`, `routes_fastapi`, `routes_express`, `tests_junit`, `tests_pytest`.
- **Named tests** `routes_spring`, `routes_fastapi`, `routes_express`, `tests_junit`, `tests_pytest`

### P2-08 — Derive stage: contracts

- **Size** L · **Depends** P2-07 · **Order** 21 · **Execution** fan-out-eligible
- **Deliverables** `contracts/{openapi,routes,proto,channels,tables,artifacts}.rs` implementing 4.7.1 with key normalisation (channel producers/consumers come from the engine's `channels` array, Appendix D.2, supplemented by the annotation rules of 4.7.1); `props` include the raw form; placeholder resolution from `application*.yml|properties` and `.env` keys.
- **Acceptance** fixtures `contracts_openapi`, `contracts_proto`, `contracts_kafka_literal`, `contracts_kafka_placeholder_unresolved`, `contracts_jpa_table`, `contracts_maven_artifact`, `contract_key_normalisation` (table of raw → key).
- **Named tests** `contracts_openapi`, `contracts_proto`, `contracts_kafka_literal`, `contracts_kafka_placeholder_unresolved`, `contracts_jpa_table`, `contracts_maven_artifact`, `contract_key_normalisation`

### P2-09 — Derive stage: layer roles and metrics

- **Size** M · **Depends** P2-07 · **Order** 22 · **Execution** fan-out-eligible
- **Deliverables** `layers/roles.rs` (4.8.3 precedence), `metrics/{complexity,importance,fanio}.rs` (4.12.1; cyclomatic and cognitive from engine AST facts exposed by Appendix D.4).
- **Acceptance** `layer_role_precedence` (config beats annotation beats path), `cyclomatic_known_values` (fixtures with hand-computed values), `importance_formula`.
- **Named tests** `layer_role_precedence`, `cyclomatic_known_values`, `importance_formula`

### P2-10 — Coverage and write

- **Size** M · **Depends** P2-08, P2-09, P2-02 · **Order** 23 · **Execution** single-agent
- **Deliverables** `coverage/mod.rs` producing rows per language with all 11 bands; `index/write.rs` (Stage 5); `index/mod.rs` `build_segment(IndexRequest) -> SegmentReport` orchestrating Stages 1–5 with `tracing` spans and a progress callback.
- **Acceptance** `coverage_counts_sum_to_call_sites`; `build_segment_end_to_end` on a fixture repo; `degraded_when_failed_over_threshold`.
- **Named tests** `coverage_counts_sum_to_call_sites`, `build_segment_end_to_end`, `degraded_when_failed_over_threshold`

### P2-11 — `pdx index` CLI

- **Size** M · **Depends** P2-10 · **Order** 24 · **Execution** single-agent
- **Deliverables** `pdx index [path] [--out .pdx/segment.db] [--profile structural] [--json report]`; `pdx version` (prints `pdx`, engine, schema and matrix versions); `pdx coverage [--json]`; exit codes: 0 ok, 2 degraded, 1 error; progress on stderr when a TTY.
- **Acceptance** `cli_index_fixture` (snapshot of `--json` report); `cli_exit_code_degraded`; `cli_help_snapshot`.
- **Named tests** `cli_index_fixture`, `cli_exit_code_degraded`, `cli_help_snapshot`

### P2-12 — Golden harness v0

- **Size** M · **Depends** P2-11 · **Order** 25 · **Execution** fan-out-eligible
- **Deliverables** `pdx-bench` skeleton: fetch golden repos from `bench/repos.lock` into `bench/.cache/`, run `pdx index`, emit `expected.json` per 5.7 with `--accept`; `bench/golden/` populated for all golden repos.
- **Acceptance** `make golden` passes on a clean checkout; `golden_files_sorted` test.
- **Named tests** `golden_files_sorted`

### P2-13 — Legacy comparison (triage only)

- **Size** M · **Depends** P2-12 · **Order** 26 · **Execution** single-agent
- **Deliverables** `scripts/compare-legacy.py` that runs `legacy/indexer/graph_pass` on the Python golden repo (`flask`) and diffs drawn `calls` edges against the new segment; report committed to `bench/results/legacy-compare.md` with counts of: same, only-legacy, only-new, and 20 sampled explanations written by the agent after reading the evidence lines. The legacy indexer needs PostgreSQL and its embedding model;…
- **Acceptance** the report exists (or the task is `blocked` with an issue); no test threshold (this is triage input for tuning `TYPED_MIN_SCORE` through an SCR if warranted).

### P2-14 — Determinism and performance baselines

- **Size** S · **Depends** P2-12 · **Order** 27 · **Execution** single-agent
- **Deliverables** `pdx bench --determinism` and `--perf` producing `bench/results/<version>/`; `criterion` benches for `resolve` and `segment write`.
- **Acceptance** byte-identical segments on all golden repos; perf numbers recorded (no threshold yet; thresholds are set at G1).

### P2-16 — Engine differential harness (nightly)

- **Size** M · **Depends** P2-12, P1-07 · **Order** 28 · **Execution** single-agent
- **Gate** G1 is evaluated after this task
- **Deliverables** `make engine-differential`: builds the pinned, unmodified reference engine from `/tmp/ref/engine-ref` (never committed; referred to in code and docs only as "the reference engine"), runs it and the vendored engine over the committed fixtures and golden subsets, canonicalises both outputs (sorted rows of definitions, calls, imports, surfaces and typed resolutions with our kind mapping applied to b…
- **Acceptance** `engine_differential_identical` for every typed-tier language fixture; runs in `check-full`, not `check`.
- **Named tests** `engine_differential_identical`

## P3 — Local agent surface

9 tasks (S 0 · M 8 · L 1)

| Ord | Task | Title | Size | Depends | Execution | Tests |
|---|---|---|---|---|---|---|
| 29 | `P3-01` | MCP tool definitions | M | P2-11 | single-agent | 5 |
| 30 | `P3-02` | Local query backend | L | P3-01, P2-02 | single-agent | 2 |
| 31 | `P3-03` | `pdx mcp` stdio server | M | P3-02 | single-agent | 2 |
| 32 | `P3-04` | Working-tree overlay (standalone) | M | P3-03 | single-agent | 2 |
| 33 | `P3-05` | Lexical index (tantivy) | M | P2-10 | single-agent | 2 |
| 34 | `P3-06` | Optional embeddings | M | P3-05 | single-agent | 2 |
| 35 | `P3-07` | `pdx install` / `pdx uninstall` | M | P3-03 | fan-out-eligible | 8 |
| 36 | `P3-08` | Hooks | M | P3-07 | single-agent | 3 |
| 37 | `P3-09` | `pdx serve` (local API) | M | P3-02 | single-agent | 2 |

### P3-01 — MCP tool definitions

- **Size** M · **Depends** P2-11 · **Order** 29 · **Execution** single-agent
- **Deliverables** `pdx-mcp/src/tools/*.rs` with `serde`+`schemars` input/output types for every tool in 4.9.2; generated JSON schemas committed under `docs/spec/mcp-schemas/`; `MCP_TOOLS_VERSION` = 1; budget enforcement (`budget.rs`: byte ceiling, whole-row truncation, `has_more`), cursor encoding/decoding (`cursor.rs`: `(view_handle, query_hash, offset)`); view handles per 4.9.3 (`view.rs`: mint, verify, permissi…
- **Acceptance** `schemas_snapshot`; `budget_drops_whole_rows`; `cursor_rejects_other_view`; `view_handle_rejects_changed_permissions`; `view_handle_expires`.
- **Named tests** `schemas_snapshot`, `budget_drops_whole_rows`, `cursor_rejects_other_view`, `view_handle_rejects_changed_permissions`, `view_handle_expires`

### P3-02 — Local query backend

- **Size** L · **Depends** P3-01, P2-02 · **Order** 30 · **Execution** single-agent
- **Deliverables** `QueryBackend` trait (`manifest`, `search`, `symbol`, `callers`, `callees`, `trace`, `blast_radius`, `coverage`, `candidates`, `snippet`, `definition`, `references`, `implementations`, `contracts`, `architecture` (locally only levels `layer` and `module`; `system`/`context`/`service` return an empty result with `_meta.unsupported_locally = true`), `findings` (empty locally until P8)); locally the…
- **Acceptance** per-tool tests listed in Part 5.7 "MCP conformance" against a fixture segment; `trace_paths_are_shortest_first`; `blast_radius_counts_unresolved_touched`.
- **Named tests** `trace_paths_are_shortest_first`, `blast_radius_counts_unresolved_touched`

### P3-03 — `pdx mcp` stdio server

- **Size** M · **Depends** P3-02 · **Order** 31 · **Execution** single-agent
- **Deliverables** `pdx mcp` using `rmcp` stdio transport; view handles bound to the local segment set (4.9.3); `_meta` fields per 4.9.3; structured logs to stderr only.
- **Acceptance** `mcp_stdio_roundtrip` using an `rmcp` client; `mcp_stdout_is_json_rpc_only`.
- **Named tests** `mcp_stdio_roundtrip`, `mcp_stdout_is_json_rpc_only`

### P3-04 — Working-tree overlay (standalone)

- **Size** M · **Depends** P3-03 · **Order** 32 · **Execution** single-agent
- **Deliverables** `pdx mcp` and `pdx diff` index the working tree (respecting `.gitignore`) into `.pdx/worktree.db` on start and on file change (notify-based watcher, debounced 500 ms, incremental via extraction cache); `pdx diff [--base <ref>]` prints changed symbols and blast radius (single repo) as text or `--json`.
- **Acceptance** `overlay_reflects_uncommitted_edit` (edit a fixture file, query callers, see new edge); `diff_json_snapshot`.
- **Named tests** `overlay_reflects_uncommitted_edit`, `diff_json_snapshot`

### P3-05 — Lexical index (tantivy)

- **Size** M · **Depends** P2-10 · **Order** 33 · **Execution** single-agent
- **Deliverables** `lexical/` building a tantivy index next to the segment (`.pdx/lexical/`) over file contents with camel/snake-aware tokenizer; `pdx_search` falls back to lexical hits (file, line, preview) when FTS has < 5 hits; server uses zoekt instead (P4-12).
- **Acceptance** `lexical_camel_split`, `search_merges_fts_and_lexical`.
- **Named tests** `lexical_camel_split`, `search_merges_fts_and_lexical`

### P3-06 — Optional embeddings

- **Size** M · **Depends** P3-05 · **Order** 34 · **Execution** single-agent
- **Deliverables** `pdx asset install embeddings` downloads the ONNX model listed in Part 3.1 from `PDX_ASSET_BASE_URL` (default: Praxevia release bucket; air-gapped sites set it to an internal mirror) and verifies SHA-256 from a committed manifest; `ort`-based embedding of node `signature + doc + name`; `pdx_search {semantic:true}` uses HNSW (`usearch` crate) with RRF fusion against FTS.
- **Acceptance** `embeddings_absent_degrades_gracefully`; `semantic_search_rrf` on a fixture with a tiny test model committed under `bench/fixtures/tiny-embed.onnx` (generated by the agent with random weights, 8 dimensions).
- **Named tests** `embeddings_absent_degrades_gracefully`, `semantic_search_rrf`

### P3-07 — `pdx install` / `pdx uninstall`

- **Size** M · **Depends** P3-03 · **Order** 35 · **Execution** fan-out-eligible
- **Deliverables** client adapters for Appendix E writing MCP config, hooks and instruction snippets; idempotent; backup of each modified file to `~/.config/pdx/backups/<timestamp>/`; `--dry-run` prints the diff.
- **Acceptance** per-client tests on temp home directories (`install_claude_code`, `install_codex`, `install_cursor`, `install_vscode`, `install_gemini`, `install_opencode`), `uninstall_restores_exact_bytes`, `install_is_idempotent`.
- **Named tests** `install_claude_code`, `install_codex`, `install_cursor`, `install_vscode`, `install_gemini`, `install_opencode`, `uninstall_restores_exact_bytes`, `install_is_idempotent`

### P3-08 — Hooks

- **Size** M · **Depends** P3-07 · **Order** 36 · **Execution** single-agent
- **Deliverables** `pdx hook <event>` subcommand reading the client's JSON on stdin and writing `additionalContext` per 4.9.4; 10-symbol cap; 300 ms internal timeout (returns empty context on timeout).
- **Acceptance** `hook_grep_injects_symbols`, `hook_timeout_is_fail_open`, `hook_read_adds_coverage_note`.
- **Named tests** `hook_grep_injects_symbols`, `hook_timeout_is_fail_open`, `hook_read_adds_coverage_note`

### P3-09 — `pdx serve` (local API)

- **Size** M · **Depends** P3-02 · **Order** 37 · **Execution** single-agent
- **Gate** G2 is evaluated after this task
- **Deliverables** `pdx serve --port 7311` (manifest id `local` in all URLs) exposing `/api/v2/{manifest,search,symbol,callers,callees,trace,blast_radius,coverage,candidates,snippet}` (JSON mirrors of the MCP tools) and `/api/v2/tiles/...` (P7-02) bound to `127.0.0.1`; serves the UI bundle from embedded assets (empty placeholder until P7).
- **Acceptance** `serve_binds_loopback_only`, `serve_api_matches_mcp_outputs` (same fixture, same JSON modulo `_meta`).
- **Named tests** `serve_binds_loopback_only`, `serve_api_matches_mcp_outputs`

## P4 — Enterprise server

14 tasks (S 1 · M 5 · L 8)

| Ord | Task | Title | Size | Depends | Execution | Tests |
|---|---|---|---|---|---|---|
| 38 | `P4-01` | Server skeleton | M | P3-02 | single-agent | 2 |
| 39 | `P4-02` | Migrations and control-plane data access | M | P4-01 | single-agent | 2 |
| 40 | `P4-03` | Segment store | M | P4-02 | single-agent | 1 |
| 41 | `P4-04` | Job runner | L | P4-02 | single-agent | 3 |
| 42 | `P4-05` | Repo sync and webhooks | L | P4-04 | single-agent | 3 |
| 43 | `P4-06` | Index job and publication | L | P4-05, P4-03, P2-10 | single-agent | 5 |
| 44 | `P4-07` | Query service and `RemoteBackend` | L | P4-06, P3-02 | single-agent | 4 |
| 45 | `P4-08` | Identity: OIDC and PATs | L | P4-01 | single-agent | 5 |
| 46 | `P4-09` | Permission sync and enforcement | L | P4-05, P4-08 | single-agent | 4 |
| 47 | `P4-10` | Remote MCP | M | P4-07, P4-08 | single-agent | 4 |
| 48 | `P4-11` | HTTP API v2 and admin API | L | P4-07, P4-09 | single-agent | 3 |
| 49 | `P4-12` | zoekt integration | M | P4-06 | single-agent | 1 |
| 50 | `P4-13` | Helm chart and compose | L | P4-10, P4-12 | single-agent | 0 |
| 51 | `P4-14` | Retention, GC and operational metrics | S | P4-06 | single-agent | 2 |

### P4-01 — Server skeleton

- **Size** M · **Depends** P3-02 · **Order** 38 · **Execution** single-agent
- **Deliverables** `pdx-server` crate with axum app, config from env/`pdx-server.toml` (all keys in Appendix F), `/healthz`, `/readyz`, structured logging, graceful shutdown; `pdx server run|migrate|worker|admin ...` subcommands behind `--features enterprise`.
- **Acceptance** `server_boots_with_postgres` (testcontainers), `readyz_reports_migrations_pending`.
- **Named tests** `server_boots_with_postgres`, `readyz_reports_migrations_pending`

### P4-02 — Migrations and control-plane data access

- **Size** M · **Depends** P4-01 · **Order** 39 · **Execution** single-agent
- **Deliverables** `migrations/0001_init.sql` = 4.4 DDL; `db/` module with typed functions for every table; `sqlx` offline metadata.
- **Acceptance** `migration_0001_applies_clean`; `db_roundtrip_all_tables`.
- **Named tests** `migration_0001_applies_clean`, `db_roundtrip_all_tables`

### P4-03 — Segment store

- **Size** M · **Depends** P4-02 · **Order** 40 · **Execution** single-agent
- **Deliverables** `SegmentStore` trait with `S3Store` (`aws-sdk-s3`, path-style, works with MinIO/Ceph RGW) and `FsStore` (PVC); local read cache with LRU by bytes (`SEGMENT_CACHE_BYTES`, default 20 GiB); `storage_key` convention of 4.3.
- **Acceptance** both implementations pass the same test suite (`store_contract_tests`) using MinIO in testcontainers and a temp dir.
- **Named tests** `store_contract_tests`

### P4-04 — Job runner

- **Size** L · **Depends** P4-02 · **Order** 41 · **Execution** single-agent
- **Deliverables** `jobs/` with `SELECT … FOR UPDATE SKIP LOCKED` claiming, heartbeats (`started_at` refresh every 30 s; jobs with stale heartbeats > 5 min are re-queued up to 3 attempts), per-kind concurrency limits, dedupe keys, cancellation; worker binary entrypoint `pdx server worker --kinds index,precise,...`.
- **Acceptance** `jobs_skip_locked_no_double_claim` (two workers), `jobs_requeue_on_stale_heartbeat`, `jobs_dedupe`.
- **Named tests** `jobs_skip_locked_no_double_claim`, `jobs_requeue_on_stale_heartbeat`, `jobs_dedupe`

### P4-05 — Repo sync and webhooks

- **Size** L · **Depends** P4-04 · **Order** 42 · **Execution** single-agent
- **Deliverables** host adapters (`hosts/{gitlab,github,bitbucket}.rs`) for listing projects, default branches, cloning/fetching (via `git` CLI with credential helper from `settings`), repo registration assigning `repo_id` and maintaining `repo_aliases` on URL change (`GET /api/v2/repos/resolve?url=` for the local binary), webhook receivers (`POST /webhooks/{host}` with secret verification) for push/merge/member ev…
- **Acceptance** adapters tested against recorded HTTP fixtures (`wiremock`); `webhook_push_enqueues_index`; `poll_detects_new_head`, `repo_url_change_keeps_repo_id`.
- **Named tests** `webhook_push_enqueues_index`, `poll_detects_new_head`, `repo_url_change_keeps_repo_id`

### P4-06 — Index job and publication

- **Size** L · **Depends** P4-05, P4-03, P2-10 · **Order** 43 · **Execution** single-agent
- **Deliverables** `index` job: checkout (worktree from mirror), `build_segment` with `ObjectStoreCache` for extraction, upload under the `build_key` path, `segments` row, and publication transaction (4.4) producing a new trunk manifest version; enqueue `links`, `layout`, `estate`, `analytics` jobs; `gc` job for retention.
- **Acceptance** `publish_is_transactional` (failure between steps leaves `manifest_current` unchanged), `manifest_copies_unchanged_repos`, `gc_keeps_retention_window`, `reindex_after_engine_upgrade_does_not_overwrite`.
- **Named tests** `publish_is_transactional`, `manifest_current`, `manifest_copies_unchanged_repos`, `gc_keeps_retention_window`, `reindex_after_engine_upgrade_does_not_overwrite`

### P4-07 — Query service and `RemoteBackend`

- **Size** L · **Depends** P4-06, P3-02 · **Order** 44 · **Execution** single-agent
- **Deliverables** `QueryBackend` implementation over a manifest: opens the permitted segments (LRU-cached handles), fans out per-repo queries, merges and ranks; link-layer and estate queries read `links.db`/`estate.db` (populated in P6; stubs return empty until then); view-handle verification per 4.9.3.
- **Acceptance** `remote_backend_matches_local_on_single_repo`, `view_handle_survives_new_publication`, `fanout_respects_repo_filter`, `strict_mode_omits_denied_counts`.
- **Named tests** `remote_backend_matches_local_on_single_repo`, `view_handle_survives_new_publication`, `fanout_respects_repo_filter`, `strict_mode_omits_denied_counts`

### P4-08 — Identity: OIDC and PATs

- **Size** L · **Depends** P4-01 · **Order** 45 · **Execution** single-agent
- **Deliverables** OIDC Authorization Code + PKCE login for the UI (session cookie, `SameSite=Lax`, secure), JWKS validation for bearer tokens, group claims mapping to `principals(kind='group')`, PAT create/list/revoke API with argon2id hashing, `/.well-known/oauth-protected-resource`, optional DCR proxy, CSRF double-submit token on all cookie-authenticated mutating routes.
- **Acceptance** tests with a mock OIDC provider (`oidc_mock` fixture): `login_flow`, `bearer_jwks_validation`, `pat_scope_enforced`, `expired_pat_rejected`.
- **Named tests** `oidc_mock`, `login_flow`, `bearer_jwks_validation`, `pat_scope_enforced`, `expired_pat_rejected`

### P4-09 — Permission sync and enforcement

- **Size** L · **Depends** P4-05, P4-08 · **Order** 46 · **Execution** single-agent
- **Deliverables** `permsync` job per host (4.10), `permitted_repos(principal)` with group expansion cached 60 s, enforcement in the query planner, link-edge redaction stubs.
- **Acceptance** `permsync_gitlab_inherited_groups` (fixture), `query_filters_unpermitted_repos`, `link_edge_redacted_when_one_side_denied`, `admin_manual_grant`.
- **Named tests** `permsync_gitlab_inherited_groups`, `query_filters_unpermitted_repos`, `link_edge_redacted_when_one_side_denied`, `admin_manual_grant`

### P4-10 — Remote MCP

- **Size** M · **Depends** P4-07, P4-08 · **Order** 47 · **Execution** single-agent
- **Deliverables** `POST /mcp` Streamable HTTP via `rmcp`, bearer auth, view handles (no transport-session state; any instance can serve any request), rate limit, audit rows per call; the local `pdx mcp` forwards estate questions to this endpoint when `[server] url` is configured (overlay merge lands in P6-08; until then forwarding is pass-through).
- **Acceptance** `remote_mcp_end_to_end` with an `rmcp` client over HTTP; `mcp_unauthenticated_401`; `mcp_audit_row_written`; `mcp_stateless_across_two_instances`.
- **Named tests** `remote_mcp_end_to_end`, `mcp_unauthenticated_401`, `mcp_audit_row_written`, `mcp_stateless_across_two_instances`

### P4-11 — HTTP API v2 and admin API

- **Size** L · **Depends** P4-07, P4-09 · **Order** 48 · **Execution** single-agent
- **Deliverables** `/api/v2/*` read endpoints mirroring the tools (as in P3-09) plus `/api/v2/trust`, `/api/v2/manifests`; admin endpoints: repos CRUD and enable/disable, principals and roles, tokens, settings, jobs (list/retry/cancel), annotations review, re-index request, permission manual grants, audit export; OpenAPI document generated with `utoipa` and served at `/api/v2/openapi.json`.
- **Acceptance** `openapi_snapshot`; per-endpoint authz tests (`reader_cannot_admin`); `audit_export_jsonl`.
- **Named tests** `openapi_snapshot`, `reader_cannot_admin`, `audit_export_jsonl`

### P4-12 — zoekt integration

- **Size** M · **Depends** P4-06 · **Order** 49 · **Execution** single-agent
- **Deliverables** `deploy/images/pdx-zoekt` (upstream image pinned), indexer job that points zoekt-indexserver at `REPO_STORE_DIR`; `pdx_search` lexical fallback calls zoekt's JSON API with repo filters derived from permissions.
- **Acceptance** `search_uses_zoekt_when_configured` (zoekt in testcontainers).
- **Named tests** `search_uses_zoekt_when_configured`

### P4-13 — Helm chart and compose

- **Size** L · **Depends** P4-10, P4-12 · **Order** 50 · **Execution** single-agent
- **Deliverables** `deploy/helm/pdx` (server, workers, zoekt, optional MinIO/Postgres subcharts, OpenShift-safe defaults per 4.14.1, values documented, including a `ServiceAccount`/`Role`/`RoleBinding` allowing the worker to create and watch `batch/v1 Jobs` in its namespace for `PDX_PRECISE_MODE=k8s`); `deploy/compose/docker-compose.yml`; `docs/ops/install-openshift.md`, `docs/ops/air-gapped.md`, `docs/ops/permissi…
- **Acceptance** `helm lint`, `helm template` snapshot test, `kubeconform` against Kubernetes 1.29 schemas; compose smoke test in CI (`docker compose up`, index one fixture repo, query MCP).

### P4-14 — Retention, GC and operational metrics

- **Size** S · **Depends** P4-06 · **Order** 51 · **Execution** single-agent
- **Gate** G3 is evaluated after this task
- **Deliverables** `gc` job per 4.4; Prometheus metrics at `/metrics` (jobs, latencies, segment counts, lag per repo); `docs/ops/monitoring.md`; `pdx bench --synthetic-manifest <repos> <files_per_repo>` generating synthetic repositories and publishing them into a running server (used by G3), and `bench/load/mcp.js` (k6) exercising `pdx_callers`, `pdx_search` and `pdx_blast_radius` at a configurable session count.
- **Acceptance** `gc_removes_unreferenced_segments`, `metrics_endpoint_lists_lag`.
- **Named tests** `gc_removes_unreferenced_segments`, `metrics_endpoint_lists_lag`

## P5 — Precise band

8 tasks (S 1 · M 5 · L 2)

| Ord | Task | Title | Size | Depends | Execution | Tests |
|---|---|---|---|---|---|---|
| 52 | `P5-01` | Precise build images | M | P4-04 | fan-out-eligible | 1 |
| 53 | `P5-02` | SCIP loader | M | P2-02 | fan-out-eligible | 1 |
| 54 | `P5-03` | Merge | L | P5-02, P2-10 | single-agent | 10 |
| 55 | `P5-04` | Precise job | M | P5-01, P5-03, P4-06 | single-agent | 2 |
| 56 | `P5-05` | Ada indexer | L | P5-02 | single-agent | 2 |
| 57 | `P5-06` | Oracle benchmark | M | P5-04, P2-12 | single-agent | 1 |
| 58 | `P5-07` | Local `--precise` | S | P5-03, P2-11 | single-agent | 1 |
| 59 | `P5-08` | Navigation tool backends | M | P5-03, P3-02 | single-agent | 4 |

### P5-01 — Precise build images

- **Size** M · **Depends** P4-04 · **Order** 52 · **Execution** fan-out-eligible
- **Deliverables** `deploy/images/precise-{java,ts,python,go,cxx,dotnet,rust}.Dockerfile` with pinned indexer versions and a `pdx-precise-run` entry script that reads `pdx.toml`, runs `build_cmd|install_cmd|compdb_cmd`, then the indexer, and writes `index.scip` to a mounted output dir; no network unless `PDX_PRECISE_ALLOW_NETWORK=1`.
- **Acceptance** each image builds in CI; `precise_java_petclinic` produces a non-empty `index.scip`.
- **Named tests** `precise_java_petclinic`

### P5-02 — SCIP loader

- **Size** M · **Depends** P2-02 · **Order** 53 · **Execution** fan-out-eligible
- **Deliverables** `pdx-precise/src/scip.rs` reading `index.scip` with the `scip` crate into `PreciseDoc { path, occurrences }` with symbol parsing and byte-offset conversion honouring the declared position encoding; `symbol_to_qn(lang, scip_symbol) -> qualified_name` implementing Appendix B.1 per language so that new nodes created during merge get ids consistent with 4.2.1.
- **Acceptance** `scip_load_fixture` on committed small `index.scip` fixtures per language; `scip_symbol_to_qn_<lang>` for every precise-capable language (generated by the agent from `bench/fixtures/precise/<lang>` with the P5-01 images).
- **Named tests** `scip_load_fixture`

### P5-03 — Merge

- **Size** L · **Depends** P5-02, P2-10 · **Order** 54 · **Execution** single-agent
- **Deliverables** `merge.rs` implementing 4.6.4 exactly, including the `semantic_occurrences` writer; new segment with `profile='precise'`.
- **Acceptance** `merge_upgrades_confirmed_edge`, `merge_contradicts_wrong_edge`, `merge_adds_missing_edge`, `merge_replaces_candidate`, `merge_fails_below_mapping_threshold`, `merge_is_idempotent`, `reference_without_call_site_is_not_a_call`, `ambiguous_definition_mapping_is_withheld`, `utf16_positions_map_to_bytes`, `two_providers_conflict_draws_neither`.
- **Named tests** `merge_upgrades_confirmed_edge`, `merge_contradicts_wrong_edge`, `merge_adds_missing_edge`, `merge_replaces_candidate`, `merge_fails_below_mapping_threshold`, `merge_is_idempotent`, `reference_without_call_site_is_not_a_call`, `ambiguous_definition_mapping_is_withheld`, `utf16_positions_map_to_bytes`, `two_providers_conflict_draws_neither`

### P5-04 — Precise job

- **Size** M · **Depends** P5-01, P5-03, P4-06 · **Order** 55 · **Execution** single-agent
- **Deliverables** `precise` job: run the language image(s) for a segment's commit as Kubernetes Jobs (`k8s` mode) or local Docker (`compose` mode) with timeouts from `pdx.toml`; on success run the merge and publish the `precise` segment, replacing the repo's entry in a new manifest version.
- **Acceptance** `precise_job_publishes_new_manifest` (compose mode in CI), `precise_job_timeout_marks_failed`.
- **Named tests** `precise_job_publishes_new_manifest`, `precise_job_timeout_marks_failed`

### P5-05 — Ada indexer

- **Size** L · **Depends** P5-02 · **Order** 56 · **Execution** single-agent
- **Deliverables** `ada-indexer/` per 4.6.3: CLI, GPR project loading, definition and reference occurrences, diagnostics for unresolved, unit tests with a small GPR fixture; `deploy/images/pdx-ada-indexer.Dockerfile`; `precise-ada` handled by P5-04 with language `ada`.
- **Acceptance** `ada_indexer_fixture_scip` (occurrence counts), `ada_merge_end_to_end` on the Ada golden repo subset (`Ada_Drivers_Library/arch/ARM/STM32` only, for time).
- **Named tests** `ada_indexer_fixture_scip`, `ada_merge_end_to_end`

### P5-06 — Oracle benchmark

- **Size** M · **Depends** P5-04, P2-12 · **Order** 57 · **Execution** single-agent
- **Deliverables** `pdx bench --oracle` computing per-band precision/recall per 4.13.1; results committed under `bench/results/<version>/`.
- **Acceptance** numbers exist for every golden repo; `oracle_metrics_formula` unit test on a synthetic case.
- **Named tests** `oracle_metrics_formula`

### P5-07 — Local `--precise`

- **Size** S · **Depends** P5-03, P2-11 · **Order** 58 · **Execution** single-agent
- **Deliverables** `pdx index --precise` detects indexers on `PATH`, runs them, merges; report lists which languages got precise coverage.
- **Acceptance** `cli_precise_reports_missing_indexers`.
- **Named tests** `cli_precise_reports_missing_indexers`

### P5-08 — Navigation tool backends

- **Size** M · **Depends** P5-03, P3-02 · **Order** 59 · **Execution** single-agent
- **Deliverables** `pdx_definition`, `pdx_references`, `pdx_implementations` over the precise-profile segment when present and the structural segment otherwise; provider labelling per 4.9.2; `provider_unavailable` metadata.
- **Acceptance** navigation conformance fixtures for Java/TS/Python/Go/C#/Rust; `references_absent_provider_is_not_zero`; `references_enumerate_semantic_occurrences`; `references_structural_fallback_labelled`; `definition_fallback_never_labelled_precise`.
- **Named tests** `references_absent_provider_is_not_zero`, `references_enumerate_semantic_occurrences`, `references_structural_fallback_labelled`, `definition_fallback_never_labelled_precise`

## P6 — Contracts, links, architecture model, overlay merge

8 tasks (S 2 · M 2 · L 4)

| Ord | Task | Title | Size | Depends | Execution | Tests |
|---|---|---|---|---|---|---|
| 60 | `P6-01` | Link job | L | P4-06, P2-08 | mixed | 8 |
| 61 | `P6-02` | Estate model | L | P6-01 | mixed | 5 |
| 62 | `P6-03` | Annotations API | M | P4-11, P6-02 | single-agent | 2 |
| 63 | `P6-04` | Architecture and contracts tools | M | P4-07, P6-02 | single-agent | 3 |
| 64 | `P6-05` | PR overlays | L | P4-06, P6-04 | single-agent | 2 |
| 65 | `P6-06` | Coverage tool (estate) | S | P4-07 | single-agent | 1 |
| 66 | `P6-07` | Analytics stubs and findings plumbing | S | P4-07 | single-agent | 1 |
| 67 | `P6-08` | Local overlay merge with server | L | P4-10, P3-04 | single-agent | 5 |

### P6-01 — Link job

- **Size** L · **Depends** P4-06, P2-08 · **Order** 60 · **Execution** mixed
- **Deliverables** `pdx-arch/src/links.rs` implementing 4.7.2 over all segments in a manifest; `links.db` writer; unmatched counts in `link_meta`.
- **Acceptance** three-repo fixture (`provider-api`, `consumer-web`, `shared-proto`) tests: `links_http_exact`, `links_http_candidate_when_ambiguous`, `links_rpc_precise_with_shared_artifact`, `links_channel_unresolved_counted`, `links_table_shared`, `links_artifact_versions`, `links_health_path_in_three_repos_not_linked_without_identity`, `links_declared_hostnames_give_import_guided`.
- **Named tests** `links_http_exact`, `links_http_candidate_when_ambiguous`, `links_rpc_precise_with_shared_artifact`, `links_channel_unresolved_counted`, `links_table_shared`, `links_artifact_versions`, `links_health_path_in_three_repos_not_linked_without_identity`, `links_declared_hostnames_give_import_guided`

### P6-02 — Estate model

- **Size** L · **Depends** P6-01 · **Order** 61 · **Execution** mixed
- **Deliverables** `pdx-arch/src/estate.rs` building L0–L2 nodes and `MEMBER_OF`, `DEPLOYED_AS`, `HAS_ROLE`, `LAYER_DEPENDS`, `UPSTREAM_OF` edges per 4.8; `estate.db` writer; `pdx-arch.yaml` parser and validator (`schema/pdx-arch.schema.json`); Backstage import (4.8.2).
- **Acceptance** `estate_service_detection`, `estate_default_context_per_repo`, `estate_arch_yaml_overrides`, `estate_backstage_import`, `estate_deployment_by_image`.
- **Named tests** `estate_service_detection`, `estate_default_context_per_repo`, `estate_arch_yaml_overrides`, `estate_backstage_import`, `estate_deployment_by_image`

### P6-03 — Annotations API

- **Size** M · **Depends** P4-11, P6-02 · **Order** 62 · **Execution** single-agent
- **Deliverables** CRUD and review endpoints for `annotations`; `estate` job applies approved annotations; UI-facing list with filters.
- **Acceptance** `annotation_requires_maintainer_to_approve`, `estate_applies_only_approved`.
- **Named tests** `annotation_requires_maintainer_to_approve`, `estate_applies_only_approved`

### P6-04 — Architecture and contracts tools

- **Size** M · **Depends** P4-07, P6-02 · **Order** 63 · **Execution** single-agent
- **Deliverables** `pdx_architecture` at all levels, `pdx_contracts`, cross-repo `pdx_blast_radius`, `pdx_diff_impact` for a repo in a manifest; aggregated edges carry `worst_band` and counts; drill-down endpoint `/api/v2/edges/{agg_edge_id}/evidence`.
- **Acceptance** `architecture_levels_fixture`, `blast_radius_crosses_repos_via_contract`, `agg_edge_evidence_lists_l5_edges`.
- **Named tests** `architecture_levels_fixture`, `blast_radius_crosses_repos_via_contract`, `agg_edge_evidence_lists_l5_edges`

### P6-05 — PR overlays

- **Size** L · **Depends** P4-06, P6-04 · **Order** 64 · **Execution** single-agent
- **Deliverables** webhook on PR/MR open/update enqueues an `index` job for the PR head with `manifest_name = pr:<repo_id>:<number>`, whose manifest copies trunk and replaces that repo; `pdx_diff_impact {repo, base, head}` resolves against it; PR manifests are GC'd 7 days after close; optional PR comment poster (`settings.pr_comments = true`) summarising blast radius, contracts touched and rule violations.
- **Acceptance** `pr_manifest_created_and_gced`, `pr_comment_snapshot`.
- **Named tests** `pr_manifest_created_and_gced`, `pr_comment_snapshot`

### P6-06 — Coverage tool (estate)

- **Size** S · **Depends** P4-07 · **Order** 65 · **Execution** single-agent
- **Deliverables** `pdx_coverage` over a manifest with per-path detail and link-layer unmatched counts; documentation text embedded in the tool description per 4.9.2.
- **Acceptance** `coverage_paths_report_reason`.
- **Named tests** `coverage_paths_report_reason`

### P6-07 — Analytics stubs and findings plumbing

- **Size** S · **Depends** P4-07 · **Order** 66 · **Execution** single-agent
- **Deliverables** `findings.db` schema (`findings(finding_id, kind, severity, scope_kind, scope_id, repo_id, title, evidence TEXT JSON)`), `pdx_findings` tool reading it (empty until P8).
- **Acceptance** `findings_tool_pagination`.
- **Named tests** `findings_tool_pagination`

### P6-08 — Local overlay merge with server

- **Size** L · **Depends** P4-10, P3-04 · **Order** 67 · **Execution** single-agent
- **Gate** G4 is evaluated after this task
- **Deliverables** 4.9.5 in `pdx mcp`: config `[server] url, token_ref`, merge-base detection, remote view at merge-base (server endpoint `GET /api/v2/manifests/nearest?repo=&sha=` returns the manifest and `overlay_distance`), whole-repo replacement, local recomputation of links touching the active repo, `origin` marking, `overlay_hash` in the view handle, offline fallback; overlay conformance suite (`bench/overlay…
- **Acceptance** `overlay_replaces_active_repo_entirely`, `overlay_no_stale_inbound_edge_to_deleted_symbol`, `overlay_recomputes_links_for_active_repo`, `overlay_offline_falls_back_to_local`, `nearest_manifest_distance`, and the conformance suite green for changed definitions, deleted targets, changed inheritance, new overloads, imports, contracts and config.
- **Named tests** `overlay_replaces_active_repo_entirely`, `overlay_no_stale_inbound_edge_to_deleted_symbol`, `overlay_recomputes_links_for_active_repo`, `overlay_offline_falls_back_to_local`, `nearest_manifest_distance`

## P7 — UI

13 tasks (S 1 · M 9 · L 3)

| Ord | Task | Title | Size | Depends | Execution | Tests |
|---|---|---|---|---|---|---|
| 68 | `P7-01` | UI foundation | M | P0-03, P3-09 | single-agent | 2 |
| 69 | `P7-02` | Layout job and tiles | L | P4-06, P2-02 | single-agent | 6 |
| 70 | `P7-03` | Renderer core | L | P7-01, P7-02 | single-agent | 3 |
| 71 | `P7-04` | Architecture map view | L | P7-03 | fan-out-eligible | 3 |
| 72 | `P7-05` | Search and details | M | P7-04 | fan-out-eligible | 2 |
| 73 | `P7-06` | Neighbourhood view | M | P7-05 | fan-out-eligible | 1 |
| 84 | `P7-07` | DSM view | M | P7-05, P8-03 | fan-out-eligible | 1 |
| 85 | `P7-08` | Hotspot treemap | S | P7-05, P8-05 | fan-out-eligible | 1 |
| 86 | `P7-09` | Contract flow view | M | P6-04, P7-05 | fan-out-eligible | 2 |
| 87 | `P7-10` | Findings and trust views | M | P8-10, P7-05 | fan-out-eligible | 2 |
| 88 | `P7-11` | Chat dock | M | P7-05, P4-11 | single-agent | 2 |
| 89 | `P7-12` | UI e2e fixtures and budgets | M | P7-10 | single-agent | 0 |
| 90 | `P7-14` | Permission-aware tiles and leak test | M | P7-02, P4-09 | single-agent | 2 |

### P7-01 — UI foundation

- **Size** M · **Depends** P0-03, P3-09 · **Order** 68 · **Execution** single-agent
- **Deliverables** `ui/` app shell: routing with URL state (4.11.2), auth handling (cookie session or local mode), API client generated from `/api/v2/openapi.json` (`openapi-typescript`), Zustand stores, theme, layout with left navigation, details panel, band legend component, i18n scaffold.
- **Acceptance** Vitest `router_state_roundtrip`; Playwright `shell_loads_local_mode`.
- **Named tests** `router_state_roundtrip`, `shell_loads_local_mode`

### P7-02 — Layout job and tiles

- **Size** L · **Depends** P4-06, P2-02 · **Order** 69 · **Execution** single-agent
- **Deliverables** `pdx-core/src/layout/{treemap,fa2,aggregate,tiles}.rs` implementing 4.11.5 (Rust ports of the squarified treemap and ForceAtlas2 with Barnes–Hut); `layout` job (server) and `pdx serve` local computation; tiles writer (MessagePack per 4.11.4); `/api/v2/tiles/...` endpoint with ETag; `TILE_FORMAT_VERSION` = 1.
- **Acceptance** `treemap_areas_sum`, `fa2_converges_fixture`, `layout_is_deterministic`, `layout_anchoring_keeps_positions` (moving one node in the input moves only its subtree), `tiles_roundtrip`, `agg_edges_project_to_visible_level`.
- **Named tests** `treemap_areas_sum`, `fa2_converges_fixture`, `layout_is_deterministic`, `layout_anchoring_keeps_positions`, `tiles_roundtrip`, `agg_edges_project_to_visible_level`

### P7-03 — Renderer core

- **Size** L · **Depends** P7-01, P7-02 · **Order** 70 · **Execution** single-agent
- **Deliverables** `ui/src/render/` with `ContainerLayer`, `NodeLayer`, `AggEdgeLayer`, `LabelLayer` (deck.gl custom/instanced layers), tile loader with viewport-driven requests and prefetch of `children`, budget arbitration per 4.11.3, fade transitions, picking; `window.__pdx.stats()`.
- **Acceptance** Vitest `budget_arbitration_drops_deepest_first`, `edges_routed_to_visible_ancestor`; Playwright `map_frame_budget` (fixture with 1M nodes, asserts stats within budgets and fps ≥ 55 in headless with `--use-gl=egl`).
- **Named tests** `budget_arbitration_drops_deepest_first`, `edges_routed_to_visible_ancestor`, `map_frame_budget`

### P7-04 — Architecture map view

- **Size** L · **Depends** P7-03 · **Order** 71 · **Execution** fan-out-eligible
- **Deliverables** default view wired to tiles; click/hover details; fly-to; breadcrumb of containment; band filter; ghost nodes; "why is this here?" drill-down via `/api/v2/edges/{id}/evidence`.
- **Acceptance** Playwright `map_drilldown_to_symbol`, `map_ghost_nodes_visible`, `map_evidence_panel`.
- **Named tests** `map_drilldown_to_symbol`, `map_ghost_nodes_visible`, `map_evidence_panel`

### P7-05 — Search and details

- **Size** M · **Depends** P7-04 · **Order** 72 · **Execution** fan-out-eligible
- **Deliverables** search box with chips (FTS/lexical/semantic), results fly-to, details panel with callers/callees (band chips), coverage note, snippet viewer.
- **Acceptance** Playwright `search_flies_to_node`, `details_shows_bands`.
- **Named tests** `search_flies_to_node`, `details_shows_bands`

### P7-06 — Neighbourhood view

- **Size** M · **Depends** P7-05 · **Order** 73 · **Execution** fan-out-eligible
- **Deliverables** cosmos.gl view for `GET /api/v2/neighbourhood?node=&depth=&bands=` (server computes the subgraph ≤ `NEIGHBOURHOOD_MAX_NODES`), band colouring, pin/unpin, back-to-map.
- **Acceptance** Playwright `neighbourhood_renders_20k` (fixture) with fps ≥ 30.
- **Named tests** `neighbourhood_renders_20k`

### P7-07 — DSM view

- **Size** M · **Depends** P7-05, P8-03 (cycles) · **Order** 84 · **Execution** fan-out-eligible
- **Deliverables** `GET /api/v2/dsm?level=&scope=` returns ordered matrix (rows/cols sorted by SCC and dependency order) as MessagePack; canvas renderer with virtualisation, cell click shows evidence, cycle highlighting.
- **Acceptance** Playwright `dsm_highlights_cycle`.
- **Named tests** `dsm_highlights_cycle`

### P7-08 — Hotspot treemap

- **Size** S · **Depends** P7-05, P8-05 · **Order** 85 · **Execution** fan-out-eligible
- **Deliverables** treemap view with metric selector (`hotspot`, `churn`, `cyclomatic`, `importance`, `bus_factor`).
- **Acceptance** Playwright `treemap_metric_switch`.
- **Named tests** `treemap_metric_switch`

### P7-09 — Contract flow view

- **Size** M · **Depends** P6-04, P7-05 · **Order** 86 · **Execution** fan-out-eligible
- **Deliverables** server-side Sugiyama layout (`layout/sugiyama.rs`, port of the classic layered algorithm: cycle removal, longest-path layering, barycentre ordering, straight-line coordinates) for services and contracts; view with drill-down to evidence.
- **Acceptance** `sugiyama_no_edge_crossings_on_tree_fixture`; Playwright `contract_flow_drilldown`.
- **Named tests** `sugiyama_no_edge_crossings_on_tree_fixture`, `contract_flow_drilldown`

### P7-10 — Findings and trust views

- **Size** M · **Depends** P8-10, P7-05 · **Order** 87 · **Execution** fan-out-eligible
- **Deliverables** findings table with filters and evidence links; trust view with lag per repo, band coverage, degraded segments, benchmark numbers from `/api/v2/trust`.
- **Acceptance** Playwright `findings_filter_and_open_evidence`, `trust_shows_lag`.
- **Named tests** `findings_filter_and_open_evidence`, `trust_shows_lag`

### P7-11 — Chat dock

- **Size** M · **Depends** P7-05, P4-11 · **Order** 88 · **Execution** single-agent
- **Deliverables** `POST /api/v2/chat` streaming endpoint: server-side loop that calls the configured model (endpoint and model name in `settings.model_provider`; the API key only from the deployment secret `PDX_MODEL_API_KEY`, never stored in `settings`; disabled when either is absent) with the MCP tools as functions and returns evidence chips; snippets sent to the model pass the secret filter of 5.12 and are audi…
- **Acceptance** `chat_tool_loop_uses_backend` with a mocked provider; Playwright `chat_disabled_without_provider`.
- **Named tests** `chat_tool_loop_uses_backend`, `chat_disabled_without_provider`

### P7-12 — UI e2e fixtures and budgets

- **Size** M · **Depends** P7-10 · **Order** 89 · **Execution** single-agent
- **Deliverables** `ui/e2e/fixtures/` generator (`pdx bench --ui-fixture`) producing manifests of 10k, 100k and 1M nodes; bundle-size check; accessibility pass (axe) on all views.
- **Acceptance** all Playwright suites green in CI; bundle ≤ 2.5 MB gzipped.

### P7-14 — Permission-aware tiles and leak test

- **Size** M · **Depends** P7-02, P4-09 · **Order** 90 · **Execution** single-agent
- **Gate** G5 is evaluated after this task
- **Deliverables** tile materialisation per `permission_set_hash` (4.11.4), strict-mode omission of denied containers, cache keying and `ETag`; a synthetic estate fixture with three principals of different permissions; `pdx bench --synthetic-manifest` extended to 10M nodes for a nightly server/layout/tile timing report.
- **Acceptance** `tile_bytes_contain_no_denied_repo_strings` (byte-level scan of every tile served to the restricted principal against the denied repos' identifiers and labels); `tile_cache_separates_permission_sets`; 1M browser fixture still ≥ 55 fps; 10M timing recorded.
- **Named tests** `tile_bytes_contain_no_denied_repo_strings`, `tile_cache_separates_permission_sets`

## P8 — Analytics and trust page

11 tasks (S 3 · M 8 · L 0)

| Ord | Task | Title | Size | Depends | Execution | Tests |
|---|---|---|---|---|---|---|
| 74 | `P8-01` | Analytics job and `metrics_ts` | M | P4-06, P6-07 | single-agent | 1 |
| 75 | `P8-02` | Architecture rules | M | P8-01, P6-02 | fan-out-eligible | 3 |
| 76 | `P8-03` | Cycles | S | P8-01 | fan-out-eligible | 2 |
| 77 | `P8-04` | Contract drift | M | P8-01, P6-01 | fan-out-eligible | 4 |
| 78 | `P8-05` | Hotspots and co-change | M | P8-01, P4-05 | fan-out-eligible | 4 |
| 79 | `P8-06` | Ownership | S | P8-05 | fan-out-eligible | 2 |
| 80 | `P8-07` | Duplication | M | P8-01 | fan-out-eligible | 2 |
| 81 | `P8-08` | Observed edges | M | P6-01 | fan-out-eligible | 2 |
| 82 | `P8-09` | Freshness and agent-usage analytics | M | P4-14, P4-10 | single-agent | 3 |
| 83 | `P8-10` | Findings tool and API completion | S | P8-02, P8-03, P8-04, P8-05, P8-06, P8-07, P8-08, P8-09 | single-agent | 1 |
| 91 | `P8-11` | Trust page | M | P5-06, P8-09, P2-14 | single-agent | 1 |

### P8-01 — Analytics job and `metrics_ts`

- **Size** M · **Depends** P4-06, P6-07 · **Order** 74 · **Execution** single-agent
- **Deliverables** `pdx-analytics` job skeleton per manifest: loads segments and links, writes `findings.db` and `metrics_ts` rows; `pdx analyze` local subset (single repo: metrics, cycles, duplication, rules from `pdx.toml`).
- **Acceptance** `analytics_job_writes_metrics_ts`.
- **Named tests** `analytics_job_writes_metrics_ts`

### P8-02 — Architecture rules

- **Size** M · **Depends** P8-01, P6-02 · **Order** 75 · **Execution** fan-out-eligible
- **Deliverables** 4.12.2 evaluator; `VIOLATES` edges in `estate.db`; findings with evidence.
- **Acceptance** `rule_forbid_role_edge`, `rule_forbid_context_cycle`, `rule_require`.
- **Named tests** `rule_forbid_role_edge`, `rule_forbid_context_cycle`, `rule_require`

### P8-03 — Cycles

- **Size** S · **Depends** P8-01 · **Order** 76 · **Execution** fan-out-eligible
- **Deliverables** Tarjan SCC at module/service/context; minimum feedback edge set approximation (greedy by weight).
- **Acceptance** `scc_known_graph`, `feedback_set_breaks_cycle`.
- **Named tests** `scc_known_graph`, `feedback_set_breaks_cycle`

### P8-04 — Contract drift

- **Size** M · **Depends** P8-01, P6-01 · **Order** 77 · **Execution** fan-out-eligible
- **Deliverables** 4.12.5 findings.
- **Acceptance** `drift_undocumented`, `drift_unimplemented`, `drift_orphan_consumer`, `drift_version_skew`.
- **Named tests** `drift_undocumented`, `drift_unimplemented`, `drift_orphan_consumer`, `drift_version_skew`

### P8-05 — Hotspots and co-change

- **Size** M · **Depends** P8-01, P4-05 · **Order** 78 · **Execution** fan-out-eligible
- **Deliverables** 4.12.6 from the server mirror (`git log --numstat`), `CHANGES_WITH` edges written to `links.db` (cross-repo) and per-repo analytics tables; `pdx analyze` computes the single-repo variant from the local clone.
- **Acceptance** `churn_from_numstat_fixture`, `cochange_thresholds`, `hotspot_score_percentiles`, `cross_repo_cochange_by_issue_key`.
- **Named tests** `churn_from_numstat_fixture`, `cochange_thresholds`, `hotspot_score_percentiles`, `cross_repo_cochange_by_issue_key`

### P8-06 — Ownership

- **Size** S · **Depends** P8-05 · **Order** 79 · **Execution** fan-out-eligible
- **Deliverables** 4.12.7 with CODEOWNERS parsing (GitHub and GitLab syntax).
- **Acceptance** `bus_factor_fixture`, `codeowners_parse`.
- **Named tests** `bus_factor_fixture`, `codeowners_parse`

### P8-07 — Duplication

- **Size** M · **Depends** P8-01 · **Order** 80 · **Execution** fan-out-eligible
- **Deliverables** 4.12.8 MinHash/LSH; `SIMILAR_TO` in analytics tables.
- **Acceptance** `minhash_jaccard_estimate_within_0_05`, `lsh_finds_planted_clone`.
- **Named tests** `minhash_jaccard_estimate_within_0_05`, `lsh_finds_planted_clone`

### P8-08 — Observed edges

- **Size** M · **Depends** P6-01 · **Order** 81 · **Execution** fan-out-eligible
- **Deliverables** OTLP/HTTP JSON ingestion (4.12.9), hostname mapping, `observed` flags on link edges and runtime-only link rows, topology diff findings.
- **Acceptance** `otlp_span_creates_observed_edge`, `topology_diff_findings`.
- **Named tests** `otlp_span_creates_observed_edge`, `topology_diff_findings`

### P8-09 — Freshness and agent-usage analytics

- **Size** M · **Depends** P4-14, P4-10 · **Order** 82 · **Execution** single-agent
- **Deliverables** 4.12.10 and 4.12.11 computations into `metrics_ts`; `/api/v2/trust` payload. Deliverables also include the `POST /api/v2/events` endpoint (authenticated, rate-limited, body ≤ 8 KiB) and the `report_hook_events` option in `pdx hook`.
- **Acceptance** `lag_seconds_computation`, `followup_grep_rate`, `events_endpoint_requires_optin`.
- **Named tests** `lag_seconds_computation`, `followup_grep_rate`, `events_endpoint_requires_optin`

### P8-10 — Findings tool and API completion

- **Size** S · **Depends** P8-02..P8-09 · **Order** 83 · **Execution** single-agent
- **Deliverables** `pdx_findings` populated; severity filters; evidence links to `/api/v2/edges/.../evidence` and snippets.
- **Acceptance** `findings_end_to_end_fixture`.
- **Named tests** `findings_end_to_end_fixture`

### P8-11 — Trust page

- **Size** M · **Depends** P5-06, P8-09, P2-14 · **Order** 91 · **Execution** single-agent
- **Gate** G6 is evaluated after this task
- **Deliverables** `scripts/trust-page.py` generating `docs/trust/index.html` from `bench/results`; CI publishes on tags; page sections: per-language per-band precision/recall, timing, determinism, rebuild equivalence, methodology text (4.13).
- **Acceptance** `trust_page_generates_from_fixture_results`.
- **Named tests** `trust_page_generates_from_fixture_results`

## P9 — Retirement, packaging, release

7 tasks (S 3 · M 4 · L 0)

| Ord | Task | Title | Size | Depends | Execution | Tests |
|---|---|---|---|---|---|---|
| 92 | `P9-01` | Delete legacy | S | G5, G6 | single-agent | 0 |
| 93 | `P9-02` | Installers and packaging | M | P3-07 | fan-out-eligible | 0 |
| 94 | `P9-03` | Release pipeline | M | P9-02, P4-13 | single-agent | 0 |
| 95 | `P9-04` | Documentation completion | M | P9-01 | single-agent | 2 |
| 96 | `P9-05` | Upgrade and migration path | M | P9-03 | single-agent | 2 |
| 97 | `P9-06` | Vendor refresh procedure | S | P1-01 | single-agent | 1 |
| 98 | `P9-07` | Final gates and 2.0 release | S | — | single-agent | 1 |

### P9-01 — Delete legacy

- **Size** S · **Depends** G5, G6 · **Order** 92 · **Execution** single-agent
- **Deliverables** remove `legacy/`; update `THIRD_PARTY_NOTICES.md` (the dashboard attribution entry is removed only if no code from it survives; the agent verifies by grepping `ui/` for the original copyright line and distinctive component names from `legacy/frontend`); update `README.md`.
- **Acceptance** provenance and licence scans pass; `legacy/` absent.

### P9-02 — Installers and packaging

- **Size** M · **Depends** P3-07 · **Order** 93 · **Execution** fan-out-eligible
- **Deliverables** `install.sh`, `install.ps1` (checksum verification), Homebrew formula in `deploy/homebrew/`, npm wrapper package `deploy/npm/`, `cargo install` metadata; macOS ad-hoc signing in the installer; Windows unsigned-binary note in docs.
- **Acceptance** installer tests in CI containers (Debian, Alpine musl, macOS), PowerShell test on Windows runner.

### P9-03 — Release pipeline

- **Size** M · **Depends** P9-02, P4-13 · **Order** 94 · **Execution** single-agent
- **Deliverables** `release.yml`: build matrix, SBOMs, SLSA provenance, cosign signatures, `checksums.txt`, container images to the configured registry, Helm chart package, trust page publish; release notes template.
- **Acceptance** dry-run release on a `v0.0.0-test` tag produces all artefacts; `cosign verify` and `gh attestation verify` succeed in a post-release job.

### P9-04 — Documentation completion

- **Size** M · **Depends** P9-01 · **Order** 95 · **Execution** single-agent
- **Deliverables** `docs/user/*` and `docs/ops/*` per Part 5.8 complete; CLI reference generated; `README.md` with quick start for both tiers.
- **Acceptance** `docs_links_valid` (link checker), `cli_reference_matches_help` snapshot.
- **Named tests** `docs_links_valid`, `cli_reference_matches_help`

### P9-05 — Upgrade and migration path

- **Size** M · **Depends** P9-03 · **Order** 96 · **Execution** single-agent
- **Deliverables** `pdx server migrate` handles schema bumps; segment re-index on `SEGMENT_SCHEMA_VERSION` change; `docs/ops/upgrade.md`; `pdx export --team-artifact` writing `.pdx/segment.db.zst` (zstd 9) with `.gitattributes merge=ours` and `pdx index` importing it when present and the blob set matches (extraction cache warm-up from the artifact).
- **Acceptance** `team_artifact_roundtrip`, `migrate_from_v1_fixture`.
- **Named tests** `team_artifact_roundtrip`, `migrate_from_v1_fixture`

### P9-06 — Vendor refresh procedure

- **Size** S · **Depends** P1-01 · **Order** 97 · **Execution** single-agent
- **Deliverables** `docs/dev/vendor-refresh.md`; `make vendor-refresh` running fetch → copy → rename → strip → apply-patches → build → golden; CI job that runs it against the locked commit to prove reproducibility.
- **Acceptance** refresh at the locked commit yields a zero-diff tree (`vendor_refresh_reproducible`).
- **Named tests** `vendor_refresh_reproducible`

### P9-07 — Final gates and 2.0 release

- **Size** S · **Depends** all · **Order** 98 · **Execution** single-agent
- **Gate** G7 is evaluated after this task
- **Deliverables** `pdx bench --release-scale` (generates the 200-repo synthetic estate with the P4-14/P7-14 generator, sized to ≥ 20 M lines with a language mix matching Appendix A's typed tier, publishes it into a compose or Helm deployment, drives 50 merges and the precise subset, runs the k6 MCP load script, and the layout/tile/navigation benchmark; results under `bench/results/<version>/release-scale.json`); `…
- **Acceptance** `release_scale_report_complete` (every Part 1.5 claim has a measured value); G7 passes.
- **Named tests** `release_scale_report_complete`
