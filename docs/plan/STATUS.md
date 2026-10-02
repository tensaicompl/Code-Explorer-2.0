# Implementation status

One row per task of the implementation plan, in normative execution order.
Statuses: `todo`, `in-progress`, `done`, `blocked`. Keep them lowercase — the
provenance scanner fails the build on the upper-case marker strings.

Updated by the driver at the end of every task, per the task protocol: mark the
task, record the merge commit, and note any deviation with its justification.
A blocked task needs a numbered entry in `ISSUES.md`; a task that applied a
decision rule needs one in `DECISIONS.md`.

Regenerate the roll-up after editing: `python3 scripts/plan/plan-progress.py`

| Task | Status | Commit | Deviations |
|---|---|---|---|
| P0-01 | done | e0c0159 | Proprietary text scoped to the enterprise components instead of copied verbatim, so it cannot over-claim the Apache-2.0 parts; the replaced NOTICE's dependency list was carried into THIRD_PARTY_NOTICES.md rather than dropped; acceptance tests deferred per issue 2; see issue 1 |
| P0-02 | done | ba0b458 | The superseded stack's own compose file and two example configs moved with it: their build contexts broke on the move and a later task expects the compose file inside `legacy/`. C golden repository replaced under D5, see issue 3. Acceptance tests deferred per issue 2 |
| P0-03 | done | e74f5a6 | The scanner steps are defined as targets but not yet wired into `check`; the scanner task adds them with its scripts. Also carries the acceptance tests the first two tasks could not run, closing issue 2, and a lock-file reader in the benchmark crate to support them |
| P0-04 | done | a9c0a68 | Marker words, the URL allow list and the deny list live in data files excluded from the scan, since a scanner that greps for its own patterns flags itself. Superseded tree excluded per decision 4. Licence policy enforced on shipped dependencies and reported on build-only ones per decision 3, see issue 4. Scanners now wired into `check`. The scan reads tracked and new files alike, so a file cannot escape it by being unstaged; the acceptance tests plant their denied strings from the list at run time rather than carrying them |
| P0-05 | done | 0db9f94 | Two values the specification implies but does not name are defined under D17: the days half of the manifest retention rule, and the engine version integer. The interface mirrors only the subset it uses, declared in `scripts/consts-mirror.txt`, and a value there with no counterpart is an error. Invariants are compile-time assertions, not tests; see issue 5, the gate was briefly red on the trunk |
| P0-06 | done | c84e52f | Fanned out to thirteen partitions, one per section, writing disjoint files in the task branch rather than a worktree each. A verifier that wrote none of them found six issues, all fixed by the driver: two came from my own partition instructions, which wrongly told one partition to omit the benchmark corpus (and stated a false rule about the typed tier) and another to avoid naming a layout algorithm whose parameters are meaningless without it. Three documents number their headings differently, where the source has no subsections. Issues 6 and 7 record two silences in the source |
| P1-01 | done | 6dce2ba | Upstream paths and the clone address moved into data files the scan excludes, since the scripts are scanned. The marker and address rules scoped to code we author, per decision 9; the deny list still applies to the vendored tree in full. The reuse map misplaces the service-pattern sources, issue 12. The deny list's prefix patterns were too lenient, issue 11. Grammar tables are 303 MB in the tree and about 19 MB packed, which is the documented exception to the size rule. The notices file gained 42 components, generated rather than hand-listed, and the licence scanner now verifies a vendored licence against the allow list, which it was not doing |
| P1-02 | done | 64f3137 | The reuse map's grammar rules were incomplete in three ways, all widened in the copy script rather than patched per file: grammar-local headers of any name, source fragments a scanner includes, and the wrappers that exist for every language the reference supports rather than the matrix's. Two foundation headers are taken without their implementations, one inert and one supplied by our layer. Grammars compile through their wrappers, not directly, or every table would be defined twice. Six patches' worth of compile fixes reduced to three patch files; clang rejects three implicit declarations that gcc only warns about. Committed on the trunk rather than a task branch: after the previous task merged I stayed there and never branched. The gate passed on each commit, but the protocol asks for a branch per task. The strip deliverable is deferred to the interface-layer task and is issue 14: what nothing needs cannot be decided before the layer that reaches the extraction entry points exists |
| P1-03 | done | 682f00d, cd47fae, 83ad6dd | Appendix D extended by appending only, pending approval: `typed_only` and lexical bits on calls, lexical bits on usages, and the site's description on each resolution (issue 17); `engine_strategy` as the owner decided (issue 16); repository metadata carried through the interface, including the root crate manifest, which the Rust resolver otherwise reads from disk (issue 15). The surface is every fact the engine records for a file, not the reference's definition rows, which depend on other files (issue 18). The shim reproduces the reference's store and lookups rather than Appendix I.2's summary, and the language lookup is vendored rather than written (issue 20). Warnings are errors across the engine by default, replacing decision 11, with named exemptions for vendored code (issue 19). Two P1-02 deliverables corrected here: the environment variable prefix (decision 14) and the strip list (issue 14, decision 13). Defects found and fixed: an undeclared function returning a pointer, extraction reading past a caller's buffer, and a leak in the resolver's module names, the last two by the sanitizers. Typed resolution is checked against the reference engine by differential fixtures (decision 17); `abi_smoke` covers all 31 matrix languages and `tsx`. macOS is verified by continuous integration, not locally: run 36419361425 is green on Linux, macOS and the Windows binary, with the engine built with warnings as errors on macOS. The first run failed only because the reachability test did not read Apple's link-map form, fixed in cd47fae. Review closure in 83ad6dd (CI run 36425086580 green on all three): the call facts the reference's weak-call guards read added to the interface and proven against the reference (issue 17), the cache surface approved as implemented (issue 18), the warning exemptions confined to vendored files (issue 19); a degraded typed pass is recorded for P1-05 (issue 21) and unfinished nightly targets now skip explicitly (issue 22) |
| P1-04 | done | 0aab795, 44eb234, 2c08d9e, 24e550c, 9a63235, 9945cc5 | Before the crate, the engine was stripped of what the interface no longer reaches and the header readied for bindgen, with its enumerations named and its comments beside what they describe, so the bindings carry the header's documentation (decision 18). The build script builds the engine through the same CMake project `make engine` uses; the `cc` dependency is gone and the project owns every compiler flag. Found here and fixed on every system: the cmake crate's base flags switched all warnings off (`-w`), so an engine built by cargo had never been held to its warning policy; the test `engine_flags` now reads every build's compile commands and fails on `-w`, a missing `-Wall`/`-Wextra`/`-Werror`, an exemption on the interface layer, or a vendored source without its exemptions. On Windows the engine is built by clang for the MSVC ABI through Ninja, from a path built from its parts, since the filesystem's verbatim path breaks the sources' relative includes; the build refuses one and a test checks the compiled paths. The engine had never been built for Windows: a Windows-only header, the POSIX names Microsoft's runtime lacks or keeps in `oldnames`, two system libraries, the random source our layer was always meant to supply, and one unguarded use of the unlinked allocator, put behind its switches by patch 0005 (issue 24). The crate builds only from the repository; packaging is P9-02's (issue 23). CI run 36437093620 is green on Linux, macOS and Windows: `cargo test -p pdx-engine-sys` builds the real engine on all three and passes `abi_smoke`, `typed_resolution_crosses_the_boundary` and `engine_flags`. The stale-bindings check runs in the linter, which enables every feature |
| P1-05 | done | a645b4e, bff87ca | Contract preflight: the task's `FileExtract` named `throws`, which Appendix D.2 lacks, and left out channels and configuration reads, which it has. The engine records throws, so they and the extractor's truncation at its node budget are appended to the interface by specification change (issue 25, approved on review); `FileExtract` carries all ten arrays, and `THROWS` stays resolution's to derive, as the reference derives it in a pass not vendored. Java's declared exceptions are not captured by the engine or the reference (issue 25). The resolution surface travels in `FileExtract`; `ProjectResolver` resolves cached extractions through it with exactly the fresh answers, over every typed-resolution fixture. Issue 21 resolved: a completed run reports clean or degraded with every file accounted for, from a per-project record the pass writes (patch 0006), never the log; the safe wrapper returns answers and health together, and the typed fixtures now refuse a degraded run. Defects found by the wrapper and fixed, each with a test that fails without the fix: extraction left the caller's path in the result, so a caller freeing its path read garbage (`abi_inputs_are_copied`); a file whose extracted text is not valid UTF-8 had no exportable surface, reported as out of memory (round trip over `fixtures/bytes`, and the determinism property, which now requires every input to extract). postcard replaces bincode, whose only current release is a placeholder that does not compile (decision 19). The injected abort is the engine's own `PDX_ENGINE_TEST_CRASH_ON` rather than a new `PDXE_TEST_ABORT_ON`, compiled only by the `test-seams` feature through dev-dependencies; CI checks the Windows build and the release workflow the release binary for the switches (decision 20). A crash fails only the file it happens on, not its batch (decision 21). `engine_isolate_recovers` is in the `pdx` crate, whose binary is the worker. Left to P2-04: the cache key omits the path and the node budget (issue 26), and a hung engine hangs isolated extraction (issue 27). CI run 36943186830 is green on Linux, macOS and Windows, with the wrapper's tests and the isolation tests run on Windows too. Review closure in 0479ad9 (CI run 36951357884 green on all three): `FileExtract` identifies its source by SHA-256 of the exact bytes extracted as well as by length, and resolution refuses any other source, same length included; run health counts every allocation that fails and every work budget that runs out anywhere in typed resolution, extraction included, on the run's thread into the run's record, with answers and call carriers copied whole or lost and counted (issue 21, audit there); the per-file surface rows nothing reads are no longer built; issue 25 approved and resolved; P2-04's task text corrected by amendment to the 4.5 cache key and postcard, and the plan extractor's redaction repaired. Found and left to P1-06: the TypeScript resolver writes to standard error when a budget runs out (issue 28) |
| P1-06 | done | 2bfe5f5 | `make check-asan` is the target; `make asan`, its earlier name, is kept as an alias, and `check-full`, which the nightly build runs, depends on `check-asan` alone, so the sanitized suite runs once and a nightly cannot pass while it or the corpus fails. The corpus, `bench/corpus`, is 65 project-authored files (71,672 bytes, none over 3,330), one directory per engine language ID with all 32 covered; nothing is copied from the golden or scale repositories. `corpus_extracts` extracts every file under the sanitizers and checks the status its name declares (`recovery_` parsed or partial, `failed_` failed with a diagnostic, otherwise parsed); a file of no known language, a language without a file, or more than 200 files fails it. The 32 IDs are one test-local list (`engine/tests/matrix_languages.h`) shared by the tests that already had copies of it; the project's registry stays P1-07's. Reports are also written to files and printed, since `abi_no_output` captures its own output streams and would otherwise swallow them. Issue 28 resolved: patch 0007 removes the TypeScript resolver's two unconditional budget prints and keeps the lost-work count and the degradation; `abi_no_output` covers the corpus, and `abi_no_output_ts_budget` starves the budget and requires all four TypeScript runs to be degraded for it, with nothing written; it fails with the patch reverted. Sanitizer finding, issue 29: an offset taken from a NULL array in the C# shared registry builder, reached by a project whose definitions typed resolution keeps none of; fixed by patch 0008 (and the Java builder of the same shape), regression `abi_run_health_untyped_only`, which fails under the sanitizers without it. No suppression was added and no sanitizer option relaxed. `make vendor-verify` reproduces the committed engine. CI run 36986204068 on main is green on Linux, macOS and Windows; nightly run 36987694968, dispatched on main at 2bfe5f5, is green with the sanitized suite and the corpus reporting nothing |
| P1-07 | done | 93d6f65 | `pdx_core::languages` transcribes Appendix A, read from the external plan: 31 languages (12 typed, 19 structural) with id, tier, extensions and name patterns, shebangs, module rule and test rule as typed static data; the tiers are Appendix A's, never inferred from the engine. `tsx` is not a matrix language: `.tsx` is a TypeScript extension, extracted with the engine's `tsx` grammar, which `ENGINE_DIALECTS` records as engine mechanics. `LANGUAGE_MATRIX_VERSION` stays 1 and is defined only in `consts`. Detection is pure: the longest extension, `.h` by Appendix A's header rule, then a name pattern (`Dockerfile*`, `.env*`), then a `#!` line naming `bash`, directly or through `env`, case-sensitive throughout. Where Appendix A is silent (shebang forms, which of an extension and a name pattern wins, case) the narrow reading is implemented and recorded (issue 30); JavaScript's test rule "same as TS" names only `.ts` files, recorded for P2-07 (issue 31). `language_detection` exercises every extension and file pattern against a second transcription of Appendix A; `every_typed_language_has_engine_support` reaches `pdxe_language_id` through `pdx-engine`, a dev-dependency only. The engine's C test list (`engine/tests/matrix_languages.h`) stays, and a test keeps it equal to the matrix and its dialects. CI run 36993222630 green on Linux, macOS and Windows; the language tests run on Linux and macOS, where `make check` runs the workspace's tests. G1a not evaluated |
| P2-01 | done | 3489623 | `ids`, `bands`, `kinds` and `model` in `pdx-core`, from 4.2.1–4.2.4, 4.3 and Appendix D.3. Identity preflight found 4.2.1's formulas leave byte-level choices open that two implementations could make differently (the AST fingerprint's serialisation, base32 case and bit order, the overload disambiguator's form and ordinal, absent sites, accepted clone URLs): proposed and implemented as issue 33, a specification change the owner approved on review (2026-10-02) and written into 4.2.1 with reference vectors, each locked by fixed vectors computed outside the crate (Python `hashlib` and `base64`). No line, column, offset or insertion order reaches a hash, and every identity input refuses a NUL. `repo_id_from_url` is the no-server fallback only and accepts `https` clone URLs only. `from_engine` returns a verdict, not a band: `Typed` only for `lsp_typed` with one candidate and a score from `TYPED_MIN_SCORE` to 1, otherwise a hint for the import-guided, scoped or exact stage, or a diagnostic hint, so the engine alone yields only `typed` or `candidate`; its test is a table written from D.3, and D.3's list is checked against `pdx_engine::Strategy::ALL` (equal). Model rows mirror the 4.3 columns; `Site` is added because sites carry the identity the edges reference. `pdx-engine` stays a dev-dependency; the crate depends on `serde`, `serde_json`, `sha2`. Test URLs are assembled at run time on reserved `.example` hosts, as the scanner's own test does, so the provenance scan sees no address. Ten deliberate mutations (a line or offset in a hash, src and dst swapped, a Crockford symbol or case, `unique_name` exact, `suffix_match` typed, `candidate` drawn, `import_guided`, declaration-order ordinals) each fail named tests. CI run 37004297135 green on Linux, macOS and Windows |
| P2-02 | done | edb50a0 | `pdx_core::segment`: `schema.sql` is 4.3's DDL byte for byte (a test extracts the fenced block and compares), every other statement named in `segment/queries.rs` with values bound, set filters as one JSON array through `json_each`. Writer: temporary build with page size, encoding, auto-vacuum, journal, sync and foreign keys stated; every table inserted in its key's order through prepared statements in one transaction; duplicates, non-finite numbers, a file id that is not its path's, and precise sources on a structural segment refused; FTS5 rebuilt once and integrity-checked; build verified; `VACUUM INTO` a destination that must not exist (bound, not quoted); connections closed; 0444 on Unix, the read-only attribute on Windows; `content_sha256` of the final bytes. Reader: `file:…?mode=ro&immutable=1`, refuses wrong or malformed meta (`WrongSchemaVersion` names expected and found) and broken references before returning, decodes every stored value strictly. Storage rows added beyond P2-01: `FileRecord`, `Evidence`, `SemanticOccurrence` and their vocabularies; a candidate set is stored sorted. SQLite is bundled (rusqlite 0.40.2, SQLite 3.53.2, MIT). The same rows built in another order and other directories give identical bytes, full-text index included. Ten deliberate mutations each fail named tests. Windows CI now runs the `pdx-core` tests, so the segment's platform behaviour is tested there. Found: evidence and occurrence ids have no byte encoding (issue 34, owned by P5-03); P2-02 stores them as given. CI run 37032409020 green on Linux, macOS and Windows |
| P2-03 | done | 0f8c68b, ac48a36 | `pdx_core::config` (`pdx.toml`, Appendix C) and `pdx_core::index::discover` (4.5 Stage 1). Config: root file only, regular, at most `MAX_FILE_BYTES`, strict (unknown section or key refused); owned sections typed and validated, `[identity]` and `[server]` parsed and kept; `[languages] extra` adds suffixes and never redefines Appendix A's; `[precise]` families `java`, `ts`, `python`, `cxx` with their documented command and a per-family timeout; commands stored, never run. Discovery: its own walk over `read_dir`, using `ignore` only for gitignore pattern semantics, so hidden files, `.ignore`, the global ignore file and `.git/info/exclude` follow PDX's rules rather than a library's defaults. Hard-coded directories, `.gitignore` files, the root `.pdxignore` and `extra_excludes` exclude independently; no symlink followed and the root must be a real directory; secret paths `Redacted` and oversized files `SkippedSize` without being read; a NUL makes a file `Binary`; paths relative, `/`, UTF-8, sorted. Where Stage 1 and Appendix C are silent, the readings are issue 35 (the NUL rule as approved; example values are not defaults; `[secrets] patterns` adds to the 5.12 patterns, which are a mandatory floor (corrected on review); root `.pdxignore` only). Found: `cargo vet` (5.12) is owned by no task (issue 36). Fourteen deliberate mutations each fail named tests. The first branch run failed on two test fixtures that assumed Linux (a Windows path inside a TOML string, two names differing only in case on a case-insensitive filesystem), fixed in ac48a36 with the code unchanged. CI run 37044939782 green on Linux, macOS and Windows, symlinks and the no-read proofs exercised on all three |
| P2-04 | todo | | |
| P2-05 | todo | | |
| P2-06 | todo | | |
| P2-07 | todo | | |
| P2-08 | todo | | |
| P2-09 | todo | | |
| P2-10 | todo | | |
| P2-11 | todo | | |
| P2-12 | todo | | |
| P2-13 | todo | | |
| P2-14 | todo | | |
| P2-16 | todo | | |
| P3-01 | todo | | |
| P3-02 | todo | | |
| P3-03 | todo | | |
| P3-04 | todo | | |
| P3-05 | todo | | |
| P3-06 | todo | | |
| P3-07 | todo | | |
| P3-08 | todo | | |
| P3-09 | todo | | |
| P4-01 | todo | | |
| P4-02 | todo | | |
| P4-03 | todo | | |
| P4-04 | todo | | |
| P4-05 | todo | | |
| P4-06 | todo | | |
| P4-07 | todo | | |
| P4-08 | todo | | |
| P4-09 | todo | | |
| P4-10 | todo | | |
| P4-11 | todo | | |
| P4-12 | todo | | |
| P4-13 | todo | | |
| P4-14 | todo | | |
| P5-01 | todo | | |
| P5-02 | todo | | |
| P5-03 | todo | | |
| P5-04 | todo | | |
| P5-05 | todo | | |
| P5-06 | todo | | |
| P5-07 | todo | | |
| P5-08 | todo | | |
| P6-01 | todo | | |
| P6-02 | todo | | |
| P6-03 | todo | | |
| P6-04 | todo | | |
| P6-05 | todo | | |
| P6-06 | todo | | |
| P6-07 | todo | | |
| P6-08 | todo | | |
| P7-01 | todo | | |
| P7-02 | todo | | |
| P7-03 | todo | | |
| P7-04 | todo | | |
| P7-05 | todo | | |
| P7-06 | todo | | |
| P8-01 | todo | | |
| P8-02 | todo | | |
| P8-03 | todo | | |
| P8-04 | todo | | |
| P8-05 | todo | | |
| P8-06 | todo | | |
| P8-07 | todo | | |
| P8-08 | todo | | |
| P8-09 | todo | | |
| P8-10 | todo | | |
| P7-07 | todo | | |
| P7-08 | todo | | |
| P7-09 | todo | | |
| P7-10 | todo | | |
| P7-11 | todo | | |
| P7-12 | todo | | |
| P7-14 | todo | | |
| P8-11 | todo | | |
| P9-01 | todo | | |
| P9-02 | todo | | |
| P9-03 | todo | | |
| P9-04 | todo | | |
| P9-05 | todo | | |
| P9-06 | todo | | |
| P9-07 | todo | | |
