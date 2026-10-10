# Decisions

One line per decision taken under the plan's decision table, with the rule
applied. Append only; never rewrite an earlier entry. A decision that changes a
normative specification is not a decision — it is a specification change request
and belongs in `ISSUES.md`.

| # | Date | Task | Rule | Decision taken |
|---|---|---|---|---|
| 1 | 2026-09-27 | P0-02 | D5 | The C golden repository's licence is no longer permissive (tri-licensed RSALv2 / SSPLv1 / AGPLv3, none on the allow list). Replaced with the fallback named for that language in the language matrix, pinned in `bench/repos.lock` with the reason recorded |
| 3 | 2026-09-28 | P0-04 | D17 | The licence allow list is enforced on dependencies that ship and reported on those that only build. The bundle contains production dependencies alone, and "forbidden for inclusion" is about what reaches a user; a build tool that never does is surfaced as a note so an outlier stays visible |
| 4 | 2026-09-28 | P0-04 | D12 | The superseded tree is excluded from the provenance scan. It is itself the third-party-derived code, attributed in the notices file, and the rule exists to keep upstream names out of what we write. The exclusion is removed by the retirement task, which deletes that tree |
| 5 | 2026-09-28 | P0-04 | D1 | Continuous integration uses the runners' own toolchain manager, which honours the pinned toolchain file, rather than a third-party action, keeping the supply chain of the build itself small |
| 8 | 2026-09-28 | P1-01 | D12 | Upstream paths and the clone address live in data files the scan excludes, not in the vendoring scripts, because a path names the upstream and the scripts are scanned like any other source |
| 9 | 2026-09-28 | P1-01 | D17 | The marker-word and address rules apply to code we author, not to the vendored engine. Both exist to catch what we write; editing hundreds of licensed third-party files to remove upstream's own comments would create a diff against the reference for no gain. The deny list still applies there in full |
| 10 | 2026-09-28 | P1-01 | D11 | The file-level map places the service-pattern sources under the pipeline directory, where they are not; the wildcard that copies the extraction core already takes them. Copied by that rule, to the extraction directory rather than the resolution one |
| 11 | 2026-09-28 | P1-02 | D17 | Warnings are fatal for the interface layer we write and visible but not fatal for the vendored sources. Holding third-party code we do not edit to warnings-as-errors means a compiler upgrade can stop the build of something nobody touched. A switch turns it on |
| 12 | 2026-09-28 | P1-02 | D11 | The foundation carries headers whose names match the standard library's, so its directory is kept off the include path and its headers are reached through their prefix. On the path, one of ours shadowed a system header for every file compiled, including the grammars |
| 13 | 2026-09-28 | P1-03 | D11 | Four copied sources nothing in the interface reaches are stripped: a multi-pattern matcher, a string interner, the process-wide allocator setup and the allocator's implementation. The allocator's header stays, because kept sources include it for types behind switches this build leaves off. Issue 14 |
| 14 | 2026-09-28 | P1-03 | D12 | The environment variables the engine reads are named `PDX_ENGINE_*`, as Appendix I.3 specifies; the rename had given them the identifier prefix. Applied by the rename script to every string literal naming one, so a refresh keeps it |
| 15 | 2026-09-28 | P1-03 | D17 | Extraction runs with no time budget. The reference's indexer passes a five-second processor budget, which makes the result for a large file depend on how loaded the machine is; `MAX_FILE_BYTES` already bounds the work per file, so determinism costs nothing here |
| 16 | 2026-09-28 | P1-03 | D11 | Decision 11 is withdrawn: warnings are errors across the engine, as Part 5.1 requires, with a named list of dead-code and style classes non-fatal in vendored code only. The list is specification change request 19, awaiting approval |
| 17 | 2026-09-28 | P1-03 | D2 | Typed resolution is checked against the pinned reference by differential fixtures whose expected answers are recorded from the reference, so the check runs without it; each fixture also asserts, by hand, what it exists to prove, and must give the same answers whatever order its files arrive in and whether they come from a cache |
| 18 | 2026-09-28 | P1-04 | D11 | Stripped at P1-04, once the earlier stripping had removed their last callers: four interface shims nothing called any more (a compression wrapper, a memory-phase marker, two memory queries), the compression library they alone used, two other libraries no kept source includes, and five headers nothing includes. Found while making the interface portable; the archive-reachability test sees objects, not functions, so these were looked for by hand |
| 19 | 2026-10-02 | P1-05 | D1 | The isolated worker's protocol is postcard, not the plan's bincode. bincode's only current release (3.0.0) is a placeholder that fails to compile and states that development has ceased, so D1's latest stable version does not exist; the release before it is unmaintained. postcard (MIT or Apache-2.0) is serde-based with a stable wire format, so the owned model carries serde derives and P2-04's cache can use the same format or another serde one without touching the model |
| 20 | 2026-10-02 | P1-05 | D3 | The abort the isolation test injects is the engine's own test switch, `PDX_ENGINE_TEST_CRASH_ON` (named under decision 14), not a second one called `PDXE_TEST_ABORT_ON` as the plan names it. The engine's test switches compile only under the CMake option `PDXE_TEST_SEAMS`, set by `make engine` and by the `test-seams` feature of `pdx-engine-sys`, which only dev-dependencies enable; `scripts/no-test-switches.sh` checks the Windows build in every CI run, and the release binary, for the switches' names |
| 21 | 2026-10-02 | P1-05 | D2 | When a worker dies on a batch, only the file it dies on is recorded as `failed` with reason `engine_crash`: the batch's files are extracted again one at a time in a new worker. The plan's task text marks the batch's files; 4.5, the more specific statement, records the file the engine crashed on. It also keeps each file's outcome independent of how files were batched |
| 22 | 2026-10-02 | P2-04 | D17 | Stage 2 batches hold at most 16 files (`EXTRACT_BATCH_MAX_FILES`), whatever the memory budget would allow. The budget alone would let one batch take a whole repository of small files, leaving every other worker idle and one isolated exchange as long as the whole stage beside its 120-second timeout; a fixed count keeps batches the same on every machine |
| 23 | 2026-10-02 | P2-04 | D17 | The extraction cache reads no entry larger than 1 GiB (`EXTRACT_CACHE_MAX_ENTRY_BYTES`). An extraction of the largest file the index parses is far smaller, so a larger file is not an entry this program wrote; it is a miss, and the bound keeps a damaged entry from making the reader allocate without limit |
| 2 | 2026-09-27 | P0-02 | D1 | Lock files are TOML: the format is unspecified, TOML is what the rest of the toolchain reads, and both files parse under a standard parser |
| 24 | 2026-10-10 | P2-08 | D1 | Contracts read YAML with `saphyr-parser` 0.1.0 and XML with `roxmltree` 0.21.1, each pinned exactly. No parser of either format was in the dependency graph; `serde_yaml` is deprecated and archived, and a hand-written YAML or XML reader would be the fragile code the brief forbids. `saphyr-parser` is an event parser with no `unsafe` code, so the tree is built here and an alias is never expanded; `roxmltree` is read-only, refuses a document type declaration when asked to, and fetches nothing. Together they add three crates (`arraydeque`, `roxmltree`, `saphyr-parser`), each an exact-version exemption under issue 60 |
| 25 | 2026-10-10 | P2-08 | D17 | `CONTRACT_DOCUMENT_MAX_DEPTH` = 64: a structured document read for contracts nests at most 64 mappings and sequences, and a deeper one is reported and gives no contract. The documents read nest a handful of levels; the limit bounds what a repository can make the reader hold |

## Rule reference (extracted from `PDX-2.0-PLAN-v1.3.3.md` Part 8)

When something is not specified, apply the first matching rule, record it above,
and continue.

| Rule | Situation | Decision |
|---|---|---|
| D1 | A named third-party crate/package version is not given | Use the latest stable version compatible with the pinned toolchain; pin it in the lockfile |
| D2 | Two Part 4 statements conflict | The more specific one wins; if equal, the one appearing later in Part 4 wins; record an SCR |
| D3 | A test in a task cannot be written as named because the interface differs | Keep the test *intent*, adjust the name to `<given_name>_v2`, note in STATUS |
| D4 | The engine reference does not expose a fact the plan assumes (e.g. cognitive complexity) | Implement it in Rust from the AST facts the engine does expose (Appendix D.4); if impossible, drop the metric, record an SCR, keep the metric name reserved |
| D5 | A golden repo cannot be fetched or its licence is not permissive | Replace with the next public repo of the same language and comparable size listed in Appendix A "fallback golden repos"; update `bench/repos.lock` |
| D6 | A language in the Language Matrix has no engine typed support | It is tier `structural`; resolution uses the Rust stages only |
| D7 | Performance thresholds cannot be met on CI hardware | Record numbers; CI enforces only the non-regression rule (10%); absolute thresholds are evaluated on the reference machine described in `bench/README.md` |
| D8 | An MCP client's config format changed since Appendix E | Follow the client's current documentation; keep the adapter's tests aligned; record the change |
| D9 | The OIDC provider lacks Dynamic Client Registration | Use the static client id from settings; document per provider |
| D10 | Object storage is unavailable in a deployment | Use `FsStore` on a `ReadWriteMany` PVC; document the trade-off |
| D11 | A vendored file fails to compile after rename/strip | Prefer deleting the file if nothing references it; else patch minimally in `engine/patches/` |
| D12 | Unsure whether a string is "provenance" | Treat it as provenance and remove it; the scanner's deny-list may be extended, never shortened |
| D13 | A feature would need an outbound network call not listed in 4.14.4 | Do not implement the call; make the feature depend on a configured internal endpoint |
| D14 | The plan's estimate says S but the work is L | Split into subtasks `P<phase>-<nn>a/b/c` in STATUS; do not skip deliverables |
| D15 | Uncertain whether a band applies | Choose the lower (less confident) band; never the higher |
| D16 | A UI interaction is unspecified | Follow the pattern of the architecture map view (click selects, double-click drills down, hover previews, `Esc` clears) |
| D17 | A limit/threshold is unspecified | Define a constant in `consts.rs` with the most conservative plausible value and a doc comment; it becomes part of the spec |
| D18 | Where to put a new module | Follow Part 5.2; enterprise-only behaviour never goes into open crates |
| D19 | A vendored grammar's licence is not on the allow-list | Do not vendor it; the language is removed from the matrix by an SCR (it becomes `unknown` in discovery) |
| D20 | The engine's typed resolution needs data the extraction cache does not hold | Extend the ABI and cache with the JSON surface export/import (P1-03); re-parsing for typed resolution is parse-only and permitted, silent re-extraction is not |
| D21 | A task's acceptance test needs a large public repository that CI cannot fetch | Use a committed sub-tree fixture (≤ 5 MB) with the same shape and mark the full-repo test nightly-only |
| D22 | Gate G1a fails because the vendored typed-resolution layer cannot be isolated from its upstream pipeline within two further attempts (each attempt recorded as an issue) | Fall back to *Plan B*: keep the vendored engine for extraction only (`pdxe_extract_file`), drop D.3 typed resolution for 2.0, treat every language as tier `structural`, and record an SCR that removes the `typed` band's engine source (the Rust stages remain). Gate G4's `typed` precision criterion is then waived and the `precise` band becomes the only compiler-grade source. This is a development fallback only: G7 still requires the `typed` floor of G4, so Plan B cannot ship as 2.0 without an SCR approved by a human |
| D23 | Two precise providers disagree on one site | Retain both as `conflict` evidence; draw neither as `precise`; the structural band stands |
| D24 | An optional provider (precise indexer, OTel, zoekt, embeddings) is absent | Return `provider_unavailable` in `_meta`; never report absence as zero results |
| D25 | A compiler occurrence cannot be mapped uniquely to a site or node | No `precise` fact; record the ambiguity in coverage/evidence |
| D26 | Runtime evidence disagrees with the static graph | Keep both; observation never changes a band; report in topology findings |
| D28 | A cross-repo contract lacks authority/broker/datasource identity | Keep it `candidate` and unmatched, even if the key is unique in the estate |
| D29 | The active local repo differs from remote trunk | The full local segment replaces the remote repo; row-level mixing is forbidden |
| D30 | An aggregate would reveal denied-repo metadata | Omit it in strict mode; only `aggregate` mode may return anonymous counts |
| D32 | A correctness or security floor in G1/G3/G4 is missed | Continue independent development; G7 blocks release; the agent may not waive or redefine the floor |
| D34 | A precise/build job needs the sandbox and the cluster cannot provide it | Mark the job `blocked` with reason `sandbox_unavailable`; structural indexing continues |
