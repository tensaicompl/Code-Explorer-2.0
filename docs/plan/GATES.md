# Gate evaluations

Criteria derived from `PDX-2.0-PLAN-v1.3.3.md` Part 7 by `scripts/plan/extract-plan.py`.
A gate is evaluated by the driver only, after the task named in *After*.
Every criterion must hold; a failing gate halts progression to the next phase.

Record each evaluation below the gate's checklist with the command output
pasted verbatim, the date, and the resulting verdict.

Performance thresholds in G1 and G4 missed by ≤ 2 points are recorded with the
actual number and an SCR; a larger miss halts. Correctness and security floors
are never redefined — G7 blocks the release until they hold.

| Gate | After | Status | Evaluated |
|---|---|---|---|
| G0 | `P0-06` | passed | 2026-09-28 |
| G1a | `P1-07` | passed | 2026-10-02 |
| G1 | `P2-16` | pending | |
| G2 | `P3-09` | pending | |
| G3 | `P4-14` | pending | |
| G4 | `P6-08` | pending | |
| G5 | `P7-14` | pending | |
| G6 | `P8-11` | pending | |
| G7 | `P9-07` | pending | |

## G0 — after `P0-06`

Status: **passed**. Four criteria were met locally on Linux; the fifth needed
macOS and was closed by a continuous integration run, which found two real defects
before it went green.

Evaluated 2026-09-28 at `ccb711c` on Linux.

Criteria:

- [x] `make check` green on Linux+macOS: green on both, plus the binary built and tested on Windows
- [x] provenance and licence scans green
- [x] `legacy/` snapshot matches
- [x] lock files complete
- [x] `THIRD_PARTY_NOTICES.md` lists the dashboard attribution

Evaluation log:

Evaluated on Linux at `ccb711c`. Commands and their output, verbatim.

```
$ uname -s
Linux

$ make check; echo "exit=$?"
exit=0

$ ./scripts/provenance-scan.sh
provenance scan: clean (110 files)

$ ./scripts/licence-scan.sh
    licenses ok
licence scan: rust dependencies clean
licence scan: note, build-only dependency outside the allow list: BlueOak-1.0.0 (minimatch)
licence scan: note, build-only dependency outside the allow list: CC-BY-4.0 (caniuse-lite)
licence scan: note, build-only dependency outside the allow list: Python-2.0 (argparse)
licence scan: interface dependencies checked
licence scan: no vendored directories yet, skipped
licence scan: clean

$ cargo test -p pdx-bench
test legacy_tree_snapshot ... ok
test legal_files_present ... ok
test lock_files_parse ... ok
test every_pin_is_a_full_commit_sha ... ok
test allow_list_matches_the_one_compiled_in ... ok
test every_vendored_or_ported_licence_is_allowed ... ok
test golden_repositories_are_permissively_licensed ... ok
test a_replacement_records_why ... ok

$ grep -o 'Yuxiang Lin' THIRD_PARTY_NOTICES.md | head -1
Yuxiang Lin
```

Notes on what each criterion means here:

- The three build-only licence notes are informational by decision 3, not failures:
  the allow list governs shipped code, and every shipped dependency is permissive.
- The legacy snapshot test checks all 240 moved paths against the pre-move tree
  recorded at the commit before the restructuring, and asserts the deleted
  directory is gone and no moved directory remains at the top level.
- The notices criterion is met by the attribution entry carrying both upstream
  copyright holders and the full licence text.

macOS, closed by continuous integration. Run 36361547926 at `c5ee14c`:

```
check (ubuntu-24.04): success
check (macos-14):     success
build the binary (windows): success
```

It took three runs, and the two failures were both real:

- The step activating the pinned package manager ran where no manifest declares it,
  so both gate jobs failed before reaching the gate. Issue 9.
- The provenance scanner read its file list with a builtin that arrives in bash 4,
  and macOS ships bash 3.2, so the scanner failed there before reading a file and
  six of its seven tests failed with it. Issue 10. Verified against bash 3.2.57 in a
  container before the fix was pushed.

Neither was findable on this machine: one can only fail on a workflow's first
execution, the other only on a platform not available here. This is the criterion
earning its place in the gate.

Windows is not a criterion of this gate, and is recorded because the run covers it:
the binary builds and its tests pass there, which the next phase depends on.

## G1a — after `P1-07`

Status: **passed**. All four criteria hold. Clang and gcc are both covered by
continuous integration, and locally; the sanitizer corpus was run again from an empty
build directory at the gate head. Checking the warning policy locally found that a
reused build directory could silently drop it (issue 32), fixed before the gate was
recorded.

Evaluated 2026-10-02 at `f288c52` on Linux, with continuous integration run
36995210643 at the same commit.

Criteria:

- [x] Engine builds with `-Werror` on clang and gcc: gcc 13.3 on Ubuntu and Apple clang
  15 on macOS in continuous integration, gcc 13.3 and clang 18 locally; every
  non-grammar source compiled with `-Wall -Wextra -Werror`, under the exemptions issue
  19 approved for vendored code and none for the interface layer
- [x] `abi_smoke` passes for every typed language: all 31 languages of the matrix and
  the engine's `tsx` grammar, which include the 12 Appendix A declares typed
- [x] ASan corpus run clean: 36 of 36 tests under the address, undefined-behaviour and
  leak sanitizers, the corpus's 65 files in all 32 directories extracted, no report
- [x] no upstream-prefixed symbols: none in the archive, none in the sources

Evaluation log:

Commands and their output, verbatim.

The sanitizer suite, from an empty build directory, at the gate head (clang 18.1.3):

```
$ git rev-parse HEAD
f288c52ac26be2d8bfca712de18a61aa8a5ad2d1

$ rm -rf target/engine-asan-g1a
$ (PDX_ASAN_BUILD=target/engine-asan-g1a make check-asan > asan.log 2>&1; echo "exit=$?" >> asan.log)
$ grep -E 'Test +#|tests passed|sanitizer reports|^exit=' asan.log
 1/36 Test  #1: abi_smoke .................................   Passed    0.08 sec
 2/36 Test  #2: abi_inputs_are_copied .....................   Passed    0.14 sec
 3/36 Test  #3: corpus_extracts ...........................   Passed    0.71 sec
 4/36 Test  #4: abi_result_build_roundtrip ................   Passed    0.46 sec
 5/36 Test  #5: abi_no_output .............................   Passed    2.43 sec
 6/36 Test  #6: abi_no_output_ts_budget ...................   Passed    2.43 sec
 7/36 Test  #7: abi_throws ................................   Passed    0.03 sec
 8/36 Test  #8: abi_truncated .............................   Passed    0.02 sec
 9/36 Test  #9: abi_run_health_clean ......................   Passed    0.03 sec
10/36 Test #10: abi_run_health_untyped_only ...............   Passed    0.02 sec
11/36 Test #11: abi_run_health_clean_without_answers ......   Passed    0.03 sec
12/36 Test #12: abi_run_health_one_file_clean .............   Passed    0.03 sec
13/36 Test #13: abi_run_health_degraded_without_answers ...   Passed    0.02 sec
14/36 Test #14: abi_run_health_degraded_keeps_answers .....   Passed    0.03 sec
15/36 Test #15: abi_run_health_answer_lost ................   Passed    0.03 sec
16/36 Test #16: api_warnings_are_errors ...................   Passed    0.22 sec
17/36 Test #17: c_header_include ..........................   Passed    0.13 sec
18/36 Test #18: cpp_header_include ........................   Passed    0.13 sec
19/36 Test #19: csharp_namespace_using ....................   Passed    0.12 sec
20/36 Test #20: go_import_differential_no_pkgmap ..........   Passed    0.12 sec
21/36 Test #21: go_import_resolves_with_supplied_pkgmap ...   Passed    0.12 sec
22/36 Test #22: java_constructor_cross_file ...............   Passed    0.13 sec
23/36 Test #23: java_package_class_import .................   Passed    0.12 sec
24/36 Test #24: js_import_member_calls ....................   Passed    0.12 sec
25/36 Test #25: python_ambiguous_same_name ................   Passed    0.14 sec
26/36 Test #26: python_cross_file_calls ...................   Passed    0.13 sec
27/36 Test #27: python_locally_bound_call .................   Passed    0.11 sec
28/36 Test #28: python_self_rooted_member_call ............   Passed    0.14 sec
29/36 Test #29: python_unresolvable_import ................   Passed    0.12 sec
30/36 Test #30: python_unresolved_member_call .............   Passed    0.11 sec
31/36 Test #31: rust_workspace_with_supplied_manifest .....   Passed    0.13 sec
32/36 Test #32: rust_workspace_without_manifest ...........   Passed    0.13 sec
33/36 Test #33: ts_import_member_calls ....................   Passed    0.12 sec
34/36 Test #34: ts_path_alias_with_metadata ...............   Passed    0.12 sec
35/36 Test #35: ts_path_alias_without_metadata ............   Passed    0.12 sec
36/36 Test #36: ts_unresolved_member_call .................   Passed    0.11 sec
100% tests passed, 0 tests failed out of 36
check-asan: no sanitizer reports
exit=0

$ ls target/engine-asan-g1a/sanitizer-reports | wc -l
0

$ R=$(pwd)/target/engine-asan-g1a/sanitizer-reports; \
  ASAN_OPTIONS="detect_leaks=1:halt_on_error=1:abort_on_error=0:log_path=$R/asan" \
  UBSAN_OPTIONS="print_stacktrace=1:halt_on_error=1:log_path=$R/ubsan" \
  ./target/engine-asan-g1a/tests/corpus_extracts bench/corpus | tail -1
ok: 65 files extracted, 32 of 32 languages covered, 0 failure(s)
```

The report directory holds the address and leak sanitizers' reports and the
undefined-behaviour sanitizer's, so 0 files means none of the three reported. The
build was configured with `-fsanitize=address,undefined -fno-omit-frame-pointer
-fno-sanitize-recover=all`, `detect_leaks=1` and `halt_on_error=1`.

Continuous integration, run 36995210643 at `f288c52`, Ubuntu (gcc) and macOS (clang):

```
$ gh run view 36995210643 --log \
    | grep -E 'Show toolchain.*(cc \(Ubuntu|Apple clang version)|test (the_engine_is_compiled_with_its_own_warning_policy|abi_smoke|every_typed_language_has_engine_support|engine_symbols_renamed|the_upstream_symbol_prefix_is_gone) |Test +#1: abi_smoke|100% tests passed' \
    | grep -v '^build the binary' | cut -f1,3 | sed -E 's/\t[^ ]+Z / /'
check (ubuntu-24.04) cc (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0
check (ubuntu-24.04) test engine_symbols_renamed ... ok
check (ubuntu-24.04) test the_upstream_symbol_prefix_is_gone ... ok
check (ubuntu-24.04) test every_typed_language_has_engine_support ... ok
check (ubuntu-24.04) test abi_smoke ... ok
check (ubuntu-24.04) test the_engine_is_compiled_with_its_own_warning_policy ... ok
check (ubuntu-24.04)  1/36 Test  #1: abi_smoke .................................   Passed    0.01 sec
check (ubuntu-24.04) 100% tests passed, 0 tests failed out of 36
check (macos-14) Apple clang version 15.0.0 (clang-1500.3.9.4)
check (macos-14) test engine_symbols_renamed ... ok
check (macos-14) test the_upstream_symbol_prefix_is_gone ... ok
check (macos-14) test every_typed_language_has_engine_support ... ok
check (macos-14) test abi_smoke ... ok
check (macos-14) test the_engine_is_compiled_with_its_own_warning_policy ... ok
check (macos-14)  1/36 Test  #1: abi_smoke .................................   Passed    0.24 sec
check (macos-14) 100% tests passed out of 36
```

`the_engine_is_compiled_with_its_own_warning_policy` reads the compile record of the
engine build each job made and requires `-Wall`, `-Wextra` and `-Werror` on every
non-grammar source, no `-w` outside the generated grammars, no `-Wno-error`
exemption on the interface layer, and the named exemptions on vendored sources. CTest
on the macOS runner (CMake 4.4) words its summary without the failure count.

The same tests locally at the gate head:

```
$ PDX_REQUIRE_ENGINE=1 cargo test -p pdx-bench --test engine_build --test vendored_engine 2>&1 | grep -E "^test "
test a_link_map_names_members_in_either_linkers_form ... ok
test engine_symbols_renamed ... ok
test every_symbol_is_defined_once ... ok
test no_engine_symbol_is_left_for_someone_else_to_define ... ok
test nothing_is_missing_that_the_system_does_not_supply ... ok
test every_archive_member_is_reachable_from_the_interface ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.21s
test the_vendoring_scripts_keep_upstream_paths_out_of_themselves ... ok
test every_grammar_carries_its_licence ... ok
test every_grammar_has_a_parser ... ok
test every_vendored_library_carries_its_licence ... ok
test the_engine_carries_its_licence ... ok
test the_upstream_symbol_prefix_is_gone ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s

$ cargo test -p pdx-engine-sys --test engine_flags --test abi_smoke 2>&1 | grep -E "^test "
test typed_resolution_crosses_the_boundary ... ok
test abi_smoke ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test the_engine_has_test_switches_only_when_asked ... ok
test the_engine_is_compiled_with_its_own_warning_policy ... ok
test the_engine_is_built_from_a_plain_path ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

$ cargo test -p pdx-core --test languages every_typed_language_has_engine_support 2>&1 | grep -E "^test "
test every_typed_language_has_engine_support ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 12 filtered out; finished in 0.00s
```

The C `abi_smoke` over the release build, one line per language:

```
$ ctest --test-dir target/engine -R '^abi_smoke$' -V | grep '^1: ok'
1: ok   java        2 definitions besides the module, 0 calls
1: ok   kotlin      3 definitions besides the module, 4 calls
1: ok   scala       4 definitions besides the module, 4 calls
1: ok   typescript  3 definitions besides the module, 1 calls
1: ok   tsx         2 definitions besides the module, 1 calls
1: ok   javascript  3 definitions besides the module, 1 calls
1: ok   python      3 definitions besides the module, 1 calls
1: ok   go          4 definitions besides the module, 0 calls
1: ok   c           4 definitions besides the module, 3 calls
1: ok   cpp         3 definitions besides the module, 4 calls
1: ok   csharp      2 definitions besides the module, 0 calls
1: ok   rust        4 definitions besides the module, 2 calls
1: ok   php         3 definitions besides the module, 1 calls
1: ok   perl        2 definitions besides the module, 1 calls
1: ok   ada         1 definitions besides the module, 0 calls
1: ok   bash        2 definitions besides the module, 3 calls
1: ok   ruby        3 definitions besides the module, 0 calls
1: ok   swift       3 definitions besides the module, 1 calls
1: ok   objc        3 definitions besides the module, 1 calls
1: ok   groovy      3 definitions besides the module, 1 calls
1: ok   lua         3 definitions besides the module, 0 calls
1: ok   sql         2 definitions besides the module, 0 calls
1: ok   protobuf    3 definitions besides the module, 0 calls
1: ok   graphql     5 definitions besides the module, 0 calls
1: ok   yaml        2 definitions besides the module, 0 calls
1: ok   json        5 definitions besides the module, 0 calls
1: ok   toml        5 definitions besides the module, 0 calls
1: ok   hcl         2 definitions besides the module, 0 calls
1: ok   dockerfile  1 definitions besides the module, 0 calls
1: ok   markdown    2 definitions besides the module, 0 calls
1: ok   xml         3 definitions besides the module, 0 calls
1: ok   properties  3 definitions besides the module, 0 calls
```

The warning policy of both local builds, from their compile records: the release
build by gcc 13.3, reconfigured with issue 32's fix, and the sanitized one by clang
18.1.3, configured fresh above:

```
$ for d in target/engine target/engine-asan-g1a; do python3 -c 'import json,sys; c=[e for e in json.load(open(sys.argv[1]+"/compile_commands.json")) if not e["file"].rsplit("/",1)[-1].startswith("grammar_")]; w=lambda e:all(f in e["command"].split() for f in ("-Wall","-Wextra","-Werror")); print(sys.argv[1], c[0]["command"].split()[0], len(c), "non-grammar sources,", sum(not w(e) for e in c), "without -Wall -Wextra -Werror")' $d; done
target/engine /usr/bin/cc 50 non-grammar sources, 0 without -Wall -Wextra -Werror
target/engine-asan-g1a /usr/bin/clang 50 non-grammar sources, 0 without -Wall -Wextra -Werror
```

Notes on what each criterion means here:

- `-Werror` is read as issue 19 approved it: warnings are errors everywhere, a named set
  of harmless warning classes stays visible but non-fatal in vendored sources only, and
  four defect classes are fatal everywhere. Every warning the gcc build prints is in
  that set. `api_warnings_are_errors` proves each exempt class is still an error in the
  interface layer.
- "Every typed language" is Appendix A's twelve (`java`, `kotlin`, `typescript`,
  `javascript`, `python`, `go`, `c`, `cpp`, `csharp`, `rust`, `php`, `perl`), which P1-07
  transcribed; `every_typed_language_has_engine_support` asks the engine about each.
  `abi_smoke` covers them and the 19 structural languages and the `tsx` grammar.
- The symbol criterion is checked twice: `engine_symbols_renamed` reads the built
  archive's defined symbols with `nm` (over 1,000 of them), and
  `the_upstream_symbol_prefix_is_gone` scans the engine's sources. Both are required,
  never skipped, under `make check` and in continuous integration.
- Issue 32: the local release build had been configured once with
  `PDXE_VENDORED_WERROR` off, and CMake kept it, so 37 of its 50 non-grammar sources had
  compiled without `-Werror`. No continuous integration build was affected. `make engine`
  and `make check-asan` now state the policy on every configure.

Windows is not a criterion of this gate, and is recorded because the run covers it:
the engine builds there with clang 20.1.8, `abi_smoke` and the warning-policy test pass
through the bindings, and the binary builds and its tests pass.

## G1 — after `P2-16`

Status: **pending**

Criteria:

- [ ] `pdx index` succeeds on all 10 golden repos with `status ∈ {ready}` (no `degraded`)
- [ ] determinism 100%
- [ ] golden snapshots committed
- [ ] performance baseline recorded: ≥ 400 files/s single-node for the structural profile (extraction plus typed resolution) on `kubernetes` (nightly), peak RSS ≤ 4 GiB on `kubernetes`
- [ ] coverage rows sum to call sites
- [ ] `bench/results/legacy-compare.md` exists

Evaluation log:

_not yet evaluated_

## G2 — after `P3-09`

Status: **pending**

Criteria:

- [ ] MCP conformance suite green for all 18 tools locally
- [ ] `pdx install`/`uninstall` idempotent for the 6 clients
- [ ] hook fail-open verified
- [ ] `pdx serve` loopback-only
- [ ] budget, cursor and view-handle tests green

Evaluation log:

_not yet evaluated_

## G3 — after `P4-14`

Status: **pending**

Criteria:

- [ ] Compose smoke: 3 fixture repos indexed and published within 5 min of webhook
- [ ] `publish_is_transactional`
- [ ] permission filtering tests green
- [ ] remote MCP e2e green with OIDC mock and PAT
- [ ] Helm lint/template/kubeconform green
- [ ] p95 `pdx_callers` latency < 300 ms at 100 concurrent sessions on the compose stack (k6 script `bench/load/mcp.js` against a 20-repo synthetic manifest)

Evaluation log:

_not yet evaluated_

## G4 — after `P6-08`

Status: **pending**

Criteria:

- [ ] Structural accuracy published for all golden repos (4.13.1): `typed` precision ≥ 97% and union of drawn bands precision ≥ 95% on oracle-verifiable sites, oracle-verifiable fraction reported
- [ ] recall ≥ 85% for Java/TS/Python/Go/C#/Rust/Kotlin, ≥ 80% Ada (on the subset), ≥ 70% C/C++
- [ ] ingestion fidelity reported
- [ ] precise sample review ≥ 99% where `truth.jsonl` exists (otherwise reported as `unreviewed`)
- [ ] three-repo link fixture green including the collision fixtures (`/health` in three repos not linked without identity)
- [ ] PR overlay tests green
- [ ] overlay conformance suite green (whole-repo replacement against a clean full index for changed definitions, deleted targets, changed inheritance, new overloads, imports, contracts and config)

Evaluation log:

_not yet evaluated_

## G5 — after `P7-14`

Status: **pending**

Criteria:

- [ ] Playwright suites green
- [ ] `map_frame_budget` on the 1M-node fixture: points ≤ 200k, lines ≤ 300k, labels ≤ 2k, fps ≥ 55
- [ ] bundle ≤ 2.5 MB gzipped
- [ ] axe: no critical violations
- [ ] tile leak test: tiles served to a restricted principal contain no identifier, label or count from a denied repo

Evaluation log:

_not yet evaluated_

## G6 — after `P8-11`

Status: **pending**

Criteria:

- [ ] All analytics fixtures green
- [ ] trust page generated
- [ ] freshness and agent-usage metrics visible in `/api/v2/trust`
- [ ] `docs/trust` published in a dry-run release

Evaluation log:

_not yet evaluated_

## G7 — after `P9-07`

Status: **pending**

Criteria:

- [ ] Release-scale benchmark (P9-07, nightly/release only, on the reference hardware of `bench/README.md`) meets the Part 1.5 claims on a synthetic 200-repo estate of ≥ 20 M lines: structural update → published trunk view p95 ≤ 5 min over 50 merges
- [ ] precise update p95 ≤ 60 min on the precise-enabled subset
- [ ] bounded MCP query p95 < 300 ms at 100 concurrent sessions
- [ ] the 10M-node estate completes layout and tile generation and root→L5 navigation stays within the 4.11.3 budgets
- [ ] `legacy/` removed
- [ ] release dry-run artefacts verified (cosign, attestation, checksums)
- [ ] docs link check green
- [ ] `STATUS.md` complete
- [ ] vendor refresh reproducible
- [ ] `bench/precise-sample/<lang>/truth.jsonl` present and ≥ 99% for every precise-capable language
- [ ] **all G1, G3 and G4 correctness and security floors meet their stated values — no small-miss waiver at release**

Evaluation log:

_not yet evaluated_
