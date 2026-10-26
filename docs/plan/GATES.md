# Gate evaluations

Criteria derived from `PDX-2.0-PLAN-v1.4.md` Part 7 by `scripts/plan/extract-plan.py`
(v1.3.3 until 2026-10-26; v1.4's additions are marked in each gate, decision 78).
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
| G1 | `P2-20` | pending | |
| G1b | `P2-22` | pending | |
| G2 | `P3-10` | pending | |
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

## G1 — after `P2-20`

Plan v1.4 (decision 78) moves G1 after P2-20: streaming Stage 2 (P2-17, issue 82), generated
files, data formats and cache retention (P2-18), the extraction bounds' closure (P2-19) and
segment schema 4 (P2-20) are part of P2 now. It also names the machine of the scale
criterion (the local reference profile, decision 80) and adds three criteria, below the
v1.3.3 list. Decision 72 lifts the hold on P3 that the 2026-10-12 verdict placed; G2 is
evaluated only once G1 and G1b have passed.

Status: **pending**. Evaluated by the driver on 2026-10-12 at `2e6f253`: **fail**, and
progression to P3 is halted. The gate records no state between `pending` and `passed`
(`scripts/plan/plan-progress.py` reads `passed` alone), so the status stays `pending`
and the verdict is in the log below. Evaluated again on 2026-10-26 at `48be0ad`, after
P2-20: eight of the ten criteria hold. The scale criterion's two rows meet their thresholds
on the development VM, but decision 80 takes that verdict on the local reference profile
only, so the gate stays `pending` until `make local-scale` passes there (log below).

Criteria:

- [x] `pdx index` succeeds on all 10 golden repos with `status ∈ {ready}` (no `degraded`): all ten ready (`make golden`)
- [x] determinism 100%: ten of ten byte-identical (P2-14)
- [x] golden snapshots committed: ten, in `bench/golden/`
- [ ] performance baseline recorded: ≥ 400 files/s single-node for the structural profile (extraction plus typed resolution) on `kubernetes` (nightly), peak RSS ≤ 4 GiB on `kubernetes`: recorded (`bench/results/0.1.0/kubernetes.json`, 2026-10-26) and the thresholds met on the development VM limited to eight processors, 591.2 files/s and 3,029 MiB; the verdict is the local reference profile's (D7, decision 80)
- [x] coverage rows sum to call sites: the segment writer refuses rows that do not (P2-10)
- [x] `bench/results/legacy-compare.md` exists
- [ ] (v1.4) the scale criterion above measured on the local reference profile (decision 80): **not yet**, `make local-scale` passes on the development VM, which decision 80 lets record numbers only
- [x] (v1.4) issue 131's data repository indexed under 1 GiB peak: 504 MiB cold, `ready` (2026-10-26)
- [x] (v1.4) engine differential identical or ratified: 32 corpora passed, 29 identical, 3 with only ratified rows (2026-10-26)
- [x] (v1.4) sanitizer corpus run clean: `make check-asan`, 62 of 62, no sanitizer report (2026-10-26)

Evaluation log:

2026-10-12, at `2e6f253` (engine 6), by the driver:

```
driver verdict: FAIL — progression to P3 halted
blocking criterion: Kubernetes performance
blocking issue: 82
```

The Kubernetes criterion is not established. It requires at least 400 files/s and at
most 4 GiB peak resident memory, structural profile, on the `kubernetes` nightly
corpus. P2-14's worker exceeded the harness's 6 GiB ceiling in Stage 2, which did not
complete, so no completed throughput or memory result exists. This is no miss of two
points or fewer: there is no measurement. Issue 82 stays open, and is closed only by a
completed Kubernetes measurement that meets the criterion. The other five criteria have
evidence. Commands and their output, verbatim:

```
$ git rev-parse 2e6f253
2e6f253034316f75b822912e6385690c43b91e3a
$ for r in kubernetes linux; do git show 2e6f253:bench/results/0.1.0/$r.json | python3 -c 'import json,sys; d=json.load(sys.stdin); p=d["performance"]; print(d["provenance"]["repository"], "engine", d["provenance"]["engine_version"], p["outcome"], "stage", p["stage_reached"], "peak_rss", p["peak_rss_bytes"], "ceiling", p["memory_ceiling_bytes"], "files_discovered", p["files_discovered"])'; done
kubernetes engine 6 out_of_memory stage extract peak_rss 6443593728 ceiling 6442450944 files_discovered 25930
linux engine 6 out_of_memory stage extract peak_rss 6448656384 ceiling 6442450944 files_discovered 95723
$ git show 2e6f253:bench/results/0.1.0/summary.json | python3 -c 'import json,sys; s=json.load(sys.stdin); d=s["determinism"]; p=s["performance"]; print("determinism_rate", d["determinism_rate"], "identical", d["repositories_identical"], "of", d["repositories_tested"]); print("goldens_completed", p["goldens_completed"], "of", p["goldens_measured"], "all_completed", p["all_completed"])'
determinism_rate 1.0 identical 10 of 10
goldens_completed 10 of 10 all_completed False
$ git ls-tree --name-only 2e6f253 bench/golden/ | wc -l
10
$ git ls-tree --name-only 2e6f253 bench/results/legacy-compare.md
bench/results/legacy-compare.md
```

2026-10-25 and 2026-10-26, the stabilisation round's evidence and decision 72. Evidence,
not an evaluation; the status stays `pending`. At the stabilisation head (engine 14; no product code changed after `f1e406a`): all ten goldens
`ready` (`make golden`); `make determinism` ten of ten byte-identical across a cold
single-worker build, a cold 28-worker build and a warm rebuild; `make check-asan` 61 of 61
with no sanitizer report; the engine differential 32 corpora, 0 unexpected rows. Kubernetes
still passes a 6 GiB ceiling in Stage 2, after 13.6 s at 6.5 GB (issue 82). Decision 72
(2026-10-26, the owner) lifts the hold on P3 this gate's verdict placed: P3 proceeds while
G1 is open, and G2 is evaluated only once G1 and G1b have passed.

2026-10-26, at `48be0ad` (engine 16, schema 4, matrix 3), after P2-20:

```
verdict: PENDING — eight of ten criteria hold; the scale criterion meets its thresholds
on the development VM, whose numbers decision 80 records but never takes as a verdict
open: `make local-scale` on the local reference profile (8 physical cores, 16 GB, NVMe, Linux x86-64)
```

- **Goldens, determinism, snapshots.** All ten goldens index `ready` and match the
  committed goldens; each is byte-identical across a cold single-worker build, a cold
  28-worker build and a warm rebuild.
- **Scale.** Kubernetes (25,930 files), cold, eight workers, the process limited to eight
  processors (`taskset -c 0-7`, as `make local-scale` runs it): 591.2 files/s (median of
  584.6, 658.0 and 591.2) and a peak of 3,029 MiB (the largest of 3,021, 3,029 and 3,017),
  against at least 400 files/s and at most 4,096 MiB. Every sample is `degraded` by one
  file's lost work (issue 134, open), which decision 86 allows for a scale repository;
  the segment's content hash is the same in all three (`b56e8d27…`). The machine is the
  development VM (an i7-14700F's 28 logical processors under a hypervisor, 10 GB of
  memory), which decision 80 names as recording numbers only: it has less memory than the
  profile, and its eight processors are four cores' hardware threads as the VM reports
  them, not eight physical cores. The peak follows from the worker count and the
  repository, not the processor; throughput would have to be 32% lower on the reference
  profile to miss.
- **Issue 131's repository** (`tenderscore`, 1,314 files): `ready`, 82,714 nodes, peak
  504 MiB cold and 462 MiB warm, the cold and warm segments byte-identical.
- **Engine differential:** 32 corpora, 29 identical, `golden:rust/axum` and the two
  `rust_workspace_*` fixtures with only their 64 ratified records, none unexpected or stale.
- **Sanitizers:** the engine's 62 tests and the sanitizer corpus under the address,
  undefined-behaviour and leak sanitizers, no report.

Commands, from the repository root at the commit below, and their output verbatim (the
harness's progress lines, `pdx-bench: …`, and the build and test runners' progress
omitted; the results the harness wrote are committed in `bench/results/0.1.0/`):

```
$ git rev-parse HEAD
48be0ad797f0bd101d13c6432615fa9b391a3542
$ target/release/pdx-bench golden
ada/Ada_Drivers_Library: matches, 18350 nodes, 76 drawn edges, segment 2dc25f373895a29e35ae9177f4fb22b4045ff78ba2ee01578d1a4402a66b6b50
c/jq: matches, 2202 nodes, 3937 drawn edges, segment 017bb0371083aa471db676b92864f2e559cc295fbfa8eff12ba2701918df1785
cpp/fmt: matches, 7823 nodes, 3250 drawn edges, segment e90a526bdbb65cc5bed26be1c95f156f476d765b6bf3c9e02315049e86bd9a5b
csharp/eShop: matches, 6878 nodes, 514 drawn edges, segment 9438ee4788d5e43ba9bfb0442f6d98f992813a303a5add84051ebee6568204dd
go/gin: matches, 2562 nodes, 3960 drawn edges, segment b12cc7086937ee3759abb77167e8cc5745cd57cc921427322ad0e09f52fe369f
java/spring-petclinic: matches, 1636 nodes, 222 drawn edges, segment 2c14eee886dcc6f63c125cf9efcd118f715ff9b20242937d459231af90a88b0e
kotlin/Exposed: matches, 24335 nodes, 11561 drawn edges, segment 0879c9def7458ab203336e08a00b2f309ca35fae056cc582bc45f854e1111856
python/flask: matches, 1816 nodes, 469 drawn edges, segment 4d5697cfc395d18bf92284357d05d894a26850666a77e60cb5ce8f9f269e7b96
rust/axum: matches, 6097 nodes, 5992 drawn edges, segment 1666a2a0a15ad8fab3d7359380e8a161da39a9664ac133a64acc589f52a27adf
typescript/hono: matches, 5474 nodes, 3044 drawn edges, segment 66747210a9dd3b921cf155453aed97b003b69d7bf2af4f26162c2535e40727d2
golden: 10 repositories match
$ target/release/pdx-bench bench --determinism
determinism: ada/Ada_Drivers_Library: identical, 1363 files, cold and warm rebuilds equivalent, segment 2dc25f373895a29e35ae9177f4fb22b4045ff78ba2ee01578d1a4402a66b6b50
determinism: c/jq: identical, 397 files, cold and warm rebuilds equivalent, segment 017bb0371083aa471db676b92864f2e559cc295fbfa8eff12ba2701918df1785
determinism: cpp/fmt: identical, 145 files, cold and warm rebuilds equivalent, segment e90a526bdbb65cc5bed26be1c95f156f476d765b6bf3c9e02315049e86bd9a5b
determinism: csharp/eShop: identical, 1142 files, cold and warm rebuilds equivalent, segment 9438ee4788d5e43ba9bfb0442f6d98f992813a303a5add84051ebee6568204dd
determinism: go/gin: identical, 130 files, cold and warm rebuilds equivalent, segment b12cc7086937ee3759abb77167e8cc5745cd57cc921427322ad0e09f52fe369f
determinism: java/spring-petclinic: identical, 132 files, cold and warm rebuilds equivalent, segment 2c14eee886dcc6f63c125cf9efcd118f715ff9b20242937d459231af90a88b0e
determinism: kotlin/Exposed: identical, 5593 files, cold and warm rebuilds equivalent, segment 0879c9def7458ab203336e08a00b2f309ca35fae056cc582bc45f854e1111856
determinism: python/flask: identical, 236 files, cold and warm rebuilds equivalent, segment 4d5697cfc395d18bf92284357d05d894a26850666a77e60cb5ce8f9f269e7b96
determinism: rust/axum: identical, 498 files, cold and warm rebuilds equivalent, segment 1666a2a0a15ad8fab3d7359380e8a161da39a9664ac133a64acc589f52a27adf
determinism: typescript/hono: identical, 483 files, cold and warm rebuilds equivalent, segment 66747210a9dd3b921cf155453aed97b003b69d7bf2af4f26162c2535e40727d2
bench: wrote 11 file(s) to bench/results/0.1.0/
$ taskset -c 0-7 target/release/pdx-bench bench --perf --only ada/Ada_Drivers_Library --only c/jq --only cpp/fmt --only csharp/eShop --only go/gin --only java/spring-petclinic --only kotlin/Exposed --only python/flask --only rust/axum --only typescript/hono --only kubernetes
perf: ada/Ada_Drivers_Library: 2009.5 files/s (median of 3), 1363 files, peak RSS 130 MiB, median wall 1.26 s
perf: c/jq: 966.8 files/s (median of 3), 397 files, peak RSS 88 MiB, median wall 0.57 s
perf: cpp/fmt: 73.5 files/s (median of 3), 145 files, peak RSS 238 MiB, median wall 2.47 s
perf: csharp/eShop: 2715.9 files/s (median of 3), 1142 files, peak RSS 76 MiB, median wall 0.67 s
perf: go/gin: 441.4 files/s (median of 3), 130 files, peak RSS 84 MiB, median wall 0.49 s
perf: java/spring-petclinic: 1515.4 files/s (median of 3), 132 files, peak RSS 37 MiB, median wall 0.15 s
perf: kotlin/Exposed: 1778.1 files/s (median of 3), 5593 files, peak RSS 317 MiB, median wall 5.50 s
perf: python/flask: 1173.8 files/s (median of 3), 236 files, peak RSS 58 MiB, median wall 0.75 s
perf: rust/axum: 287.6 files/s (median of 3), 498 files, peak RSS 87 MiB, median wall 2.04 s
perf: typescript/hono: 619.2 files/s (median of 3), 483 files, peak RSS 139 MiB, median wall 1.45 s
perf: kubernetes: 591.2 files/s (median of 3), 25930 files, peak RSS 3029 MiB, median wall 77.43 s, degraded (extraction_lost 1 file(s))
bench: wrote 12 file(s) to bench/results/0.1.0/
$ cargo test -p pdx-bench --test local_scale -- --ignored
test kubernetes_local_scale ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
$ PDX_REFERENCE_EXTRA_FLAGS=… PDX_REFERENCE_JOBS=6 make engine-differential
corpora 32: 32 passed, 29 identical, 3 with 64 ratified divergences, 0 unexpected divergences, 0 stale ratifications, 0 differ or failed
mismatches 0
engine differential: passed, 29 identical and 3 with only ratified divergences (decision 66)
$ make check-asan
100% tests passed, 0 tests failed out of 62
check-asan: no sanitizer reports
$ /usr/bin/time -v pdx index <issue 131's repository> --out .pdx/cold.db --json    # cold cache
Elapsed (wall clock) time (h:mm:ss or m:ss): 0:03.24
Maximum resident set size (kbytes): 515956
Exit status: 0
$ /usr/bin/time -v pdx index <issue 131's repository> --out .pdx/warm.db --json    # the same cache, warm
Elapsed (wall clock) time (h:mm:ss or m:ss): 0:02.28
Maximum resident set size (kbytes): 472956
Exit status: 0
```

## G1b — after `P2-22`

Plan v1.4 (decision 78) makes G1b a plan gate after P2-22, so `tasks.json` and the roll-up
carry it, and decision 72 replaces the holds it placed (issue 93's Phase B, issue 82 and P3
now proceed as tasks P2-19, P2-17 and P3). Its floors become per language (decision 73):
for language L, G4's v1.3.3 floor where P2-21's measured structural ceiling allows it,
otherwise that ceiling less 5 points, never below the recall measured at v1.4 (Python
golden 28.82%, Rust golden 81.05%). It now covers every golden with an oracle, each
judged on its own; a product-level structural recall is published beside it and is not a
criterion that can average a weak language away. Criteria from v1.4, replacing the six
golden-specific ones below once P2-21 records the floors:

- [ ] every golden with an oracle: `typed` precision ≥ 97% and drawn-union precision ≥ 95% on oracle-verifiable sites, the oracle-verifiable fraction reported
- [ ] every golden with an oracle: recall at or above its language's floor (table recorded here by P2-21)
- [ ] every miss classified by cause; product-level structural recall published

The v1.3.3-era section follows unchanged: the driver-inserted checkpoint of decision 64.

### Before v1.4: the driver-inserted structural trust checkpoint

Status: **pending**. Inserted by the driver on 2026-10-13 (decision 64, issue 95). G1b
does not replace G1, and is not in the plan's projection (`tasks.json` has no G1b, so
`scripts/plan/plan-progress.py` neither reads nor counts it). It holds issue 93's Phase B,
issue 82 and P3 until the driver releases them after A0.3.

G1b brings G4's structural accuracy floors forward, measured by A0.1 with 4.13.1's three
ratios against a semantic oracle independent of the structural engine. Each golden is
judged on its own: no cross-repository averaging, so a strong Python result cannot hide a
weak Rust one. The PDX repository's own index is diagnostic and can neither pass nor fail
G1b.

Criteria:

- [ ] Python golden (`bench/repos.lock`): `typed` precision ≥ 97% on oracle-verifiable sites
- [ ] Python golden: union of drawn bands precision ≥ 95% on oracle-verifiable sites
- [ ] Python golden: recall ≥ 85%
- [ ] Rust golden (`bench/repos.lock`): `typed` precision ≥ 97% on oracle-verifiable sites
- [ ] Rust golden: union of drawn bands precision ≥ 95% on oracle-verifiable sites
- [ ] Rust golden: recall ≥ 85%
- [ ] the oracle-verifiable fraction reported beside every precision

A floor that is missed leaves G1b pending, with the verdict in the log below. It is not
waived, the metric is not redefined and nothing is averaged away (rule D32); the resolver
is not patched within A0.1 or A0.2. What is done about it is A0.3, the driver's decision.

Evaluation log:

_not yet evaluated by the driver._

A0.1's measurement, 2026-10-14. This is evidence for A0.3, not an evaluation, and
the status stays `pending`. It measures the code at `e746b1e`; A0.1 changes no
product code. Each ratio has its numerator and denominator in
`bench/results/a0/python.json` and `rust.json`; `methodology.md` gives the method.

| Golden | Floor | Measured | Oracle-verifiable fraction | Floor met |
|---|---|---|---|---|
| Python | `typed` precision ≥ 97% | 310 / 310 = 100.00% | 310 / 310 | yes |
| Python | union precision ≥ 95% | 620 / 653 = 94.95% | 653 / 656 = 99.54% | **no** |
| Python | recall ≥ 85% | 603 / 1,114 = 54.13% | | **no** |
| Rust | `typed` precision ≥ 97% | 821 / 836 = 98.21% | 836 / 868 = 96.31% | yes |
| Rust | union precision ≥ 95% | 1,580 / 2,592 = 60.96% | 2,592 / 2,640 = 98.18% | **no** |
| Rust | recall ≥ 85% | 1,377 / 4,061 = 33.91% | | **no** |

Four of the six floors are not met. Nothing is averaged and no floor is waived or
redefined (rule D32). The PDX repository's diagnostic result (`pdx-self.json`) is
not part of the gate.

A0.3, 2026-10-14, the driver's remediation decision (decision 65). G1b is not
passed; it stays `pending` with the four floors above failed. The decision:

- keeps `typed`, `import-guided` and `inheritance-guided` drawn;
- stops trusting `exact` and `scoped` short-name uniqueness as graph truth (issue
  102);
- puts a valid typed answer before the blocklist (issue 103);
- treats recall as a first-class blocker (issues 104 to 108);
- orders the remediation R1 to R10, each authorised separately, with A0 rerun after
  every change to resolution or extraction.

Not drawing `exact` and `scoped` (R3) will lower recall further before it is
recovered; that is accepted, correctness first. G4's floors are unchanged.

A0 after R1, 2026-10-14 (issue 103: a qualifying typed answer before the
blocklist). This is evidence, not an evaluation; the status stays `pending`. The
same pins, oracles and rules are used, the results are `bench/results/a0/`, and the
baseline is `71bec72`.

| Golden | Floor | Baseline | After R1 | Floor met |
|---|---|---|---|---|
| Python | `typed` precision ≥ 97% | 310 / 310 | 314 / 314 = 100.00% | yes |
| Python | union precision ≥ 95% | 620 / 653 = 94.95% | 624 / 657 = 94.98% | **no** |
| Python | recall ≥ 85% | 603 / 1,114 = 54.13% | 606 / 1,114 = 54.40% | **no** |
| Rust | `typed` precision ≥ 97% | 821 / 836 = 98.21% | 852 / 868 = 98.16% | yes |
| Rust | union precision ≥ 95% | 1,580 / 2,592 = 60.96% | 1,611 / 2,624 = 61.39% | **no** |
| Rust | recall ≥ 85% | 1,377 / 4,061 = 33.91% | 1,408 / 4,061 = 34.67% | **no** |

The oracle-verifiable fraction is 657 / 660 (Python) and 2,624 / 2,672 (Rust).
Four of the six floors still fail.

The Rust golden's `typed` band gains 31 correct edges and 1 false one,
`Handler::call`, which belongs to issue 104.

A0 after R2, 2026-10-14 (issue 104, open: Rust typed answers need path evidence).
This is evidence, not an evaluation; the status stays `pending`. The same pins,
oracles and rules are used, the results are `bench/results/a0/`, and the baseline is
post-R1 (`d96bb2f`).

| Golden | Floor | Post-R1 | After R2 | Floor met |
|---|---|---|---|---|
| Python | `typed` precision ≥ 97% | 314 / 314 | 314 / 314 = 100.00% | yes |
| Python | union precision ≥ 95% | 624 / 657 = 94.98% | 624 / 657 = 94.98% | **no** |
| Python | recall ≥ 85% | 606 / 1,114 = 54.40% | 606 / 1,114 = 54.40% | **no** |
| Rust | `typed` precision ≥ 97% | 852 / 868 = 98.16% | 843 / 844 = 99.88% | yes |
| Rust | union precision ≥ 95% | 1,611 / 2,624 = 61.39% | 1,610 / 2,559 = 62.92% | **no** |
| Rust | recall ≥ 85% | 1,408 / 4,061 = 34.67% | 1,407 / 4,052 = 34.72% | **no** |

The oracle-verifiable fraction is 657 / 660 (Python) and 2,559 / 2,607 (Rust).
Four of the six floors still fail.

Of the Rust golden's 17 trusted false positives, 15 are no longer drawn wrong; the
blanket impl (issue 109) and the macro-written impl (issue 110) are left. 16 correct
typed edges are `exact` now, and 6 are lost to `candidate` (issue 111).

A0 after issue 111, 2026-10-14 (resolved: Rust `use` declarations read from the syntax
tree, engine 9). This is evidence, not an evaluation; the status stays `pending`. The
same pins, oracles and rules are used, the results are `bench/results/a0/`, and the
baseline is post-R2 (`d14ccd4`).

| Golden | Floor | Post-R2 | After issue 111 | Floor met |
|---|---|---|---|---|
| Python | `typed` precision ≥ 97% | 314 / 314 = 100.00% | 314 / 314 = 100.00% | yes |
| Python | union precision ≥ 95% | 624 / 657 = 94.98% | 624 / 657 = 94.98% | **no** |
| Python | recall ≥ 85% | 606 / 1,114 = 54.40% | 606 / 1,114 = 54.40% | **no** |
| Rust | `typed` precision ≥ 97% | 843 / 844 = 99.88% | 859 / 860 = 99.88% | yes |
| Rust | union precision ≥ 95% | 1,610 / 2,559 = 62.92% | 1,612 / 2,560 = 62.97% | **no** |
| Rust | recall ≥ 85% | 1,407 / 4,052 = 34.72% | 1,409 / 4,056 = 34.74% | **no** |

The oracle-verifiable fraction is 657 / 660 (Python) and 2,560 / 2,608 (Rust).
Four of the six floors still fail.

The 31 typed answers R2 lost to misread `use` lists are correct `typed` again, and one
`scoped` false positive is now correct `typed`. No new wrong trusted edge appears. Seven
correct typed edges are lost to paths the engine still misreads (issue 113).

A0 after issue 110, 2026-10-14 (resolved: no member a macro invocation may write another
of its name for is a target, engine 10; the executable checked by issue 115's guard).
Evidence, not an evaluation; the status stays `pending`. Same pins, oracles and rules;
baseline post-111 (`8c93aba`).

| Golden | Floor | After issue 111 | After issue 110 | Floor met |
|---|---|---|---|---|
| Python | `typed` precision ≥ 97% | 314 / 314 = 100.00% | 314 / 314 = 100.00% | yes |
| Python | union precision ≥ 95% | 624 / 657 = 94.98% | 624 / 657 = 94.98% | **no** |
| Python | recall ≥ 85% | 606 / 1,114 = 54.40% | 606 / 1,114 = 54.40% | **no** |
| Rust | `typed` precision ≥ 97% | 859 / 860 = 99.88% | 859 / 860 = 99.88% | yes |
| Rust | union precision ≥ 95% | 1,612 / 2,560 = 62.97% | 1,612 / 2,558 = 63.02% | **no** |
| Rust | recall ≥ 85% | 1,409 / 4,056 = 34.74% | 1,409 / 4,056 = 34.74% | **no** |

The oracle-verifiable fraction is 657 / 660 (Python) and 2,558 / 2,606 (Rust). Four of
the six floors still fail. The Rust golden's `import-guided` false positive (the
macro-written impl) is withheld, and one false `scoped` self-edge with it; no correct
edge is lost. The one trusted false positive left is issue 109's `typed` blanket impl.

A0 after issue 114, 2026-10-14 (resolved on top of issue 110: a Rust `use` binds only in
its lexical scope, engine 11; guarded by issue 115). Evidence, not an evaluation; the
status stays `pending`. Same pins, oracles and rules; baseline post-110 (`15feb3e`).

| Golden | Floor | After issue 110 | After issue 114 | Floor met |
|---|---|---|---|---|
| Python | `typed` precision ≥ 97% | 314 / 314 = 100.00% | 314 / 314 = 100.00% | yes |
| Python | union precision ≥ 95% | 624 / 657 = 94.98% | 624 / 657 = 94.98% | **no** |
| Python | recall ≥ 85% | 606 / 1,114 = 54.40% | 606 / 1,114 = 54.40% | **no** |
| Rust | `typed` precision ≥ 97% | 859 / 860 = 99.88% | 865 / 866 = 99.88% | yes |
| Rust | union precision ≥ 95% | 1,612 / 2,558 = 63.02% | 1,603 / 2,503 = 64.04% | **no** |
| Rust | recall ≥ 85% | 1,409 / 4,056 = 34.74% | 1,400 / 4,056 = 34.52% | **no** |

Four of the six floors still fail. Every site newly drawn is correct; ten correct `exact`
edges become `external` (issue 116), one `blocked` site becomes correct `typed`. The one
trusted false positive left is issue 109's.

A0 after issue 109, 2026-10-15 (a Rust trait impl's method answers `Type::method` only
when the impl is proven to apply, engine 12; guarded by issue 115). Evidence, not an
evaluation; the status stays `pending`. Same pins, oracles and rules; baseline post-114
(`c8239ba`).

| Golden | Floor | After issue 114 | After issue 109 | Floor met |
|---|---|---|---|---|
| Python | `typed` precision ≥ 97% | 314 / 314 = 100.00% | 314 / 314 = 100.00% | yes |
| Python | union precision ≥ 95% | 624 / 657 = 94.98% | 624 / 657 = 94.98% | **no** |
| Python | recall ≥ 85% | 606 / 1,114 = 54.40% | 606 / 1,114 = 54.40% | **no** |
| Rust | `typed` precision ≥ 97% | 865 / 866 = 99.88% | 860 / 860 = 100.00% | yes |
| Rust | union precision ≥ 95% | 1,603 / 2,503 = 64.04% | 1,601 / 2,501 = 64.01% | **no** |
| Rust | recall ≥ 85% | 1,400 / 4,056 = 34.52% | 1,398 / 4,054 = 34.48% | **no** |

Four of the six floors still fail. The Rust golden has no trusted false positive left
(`typed` 860/860, `import-guided` 109/109, `inheritance-guided` 60/60). The issue's site
is no typed answer, and falls to a wrong `scoped` guess, issue 102's (R3), where the union
still counts it. Five correct typed answers are lost to unknown bounds: three become
correct `scoped`, and two calls only typed resolution found in `assert_eq!` are no sites
(issue 107B). Python and PDX itself are identical.

A0 after issue 117, 2026-10-15 (Stage 3's `import-guided` and `inheritance-guided` take a
Rust trait impl's member only when its impl is proven to apply; engine 12, cache format 9,
protocol 10; guarded by issue 115). Evidence, not an evaluation; the status stays
`pending`. Same pins, oracles and rules; baseline post-109 (`83fbb45`).

| Golden | Floor | After issue 109 | After issue 117 | Floor met |
|---|---|---|---|---|
| Python | `typed` precision ≥ 97% | 314 / 314 = 100.00% | 314 / 314 = 100.00% | yes |
| Python | union precision ≥ 95% | 624 / 657 = 94.98% | 624 / 657 = 94.98% | **no** |
| Python | recall ≥ 85% | 606 / 1,114 = 54.40% | 606 / 1,114 = 54.40% | **no** |
| Rust | `typed` precision ≥ 97% | 860 / 860 = 100.00% | 860 / 860 = 100.00% | yes |
| Rust | union precision ≥ 95% | 1,601 / 2,501 = 64.01% | 1,601 / 2,501 = 64.01% | **no** |
| Rust | recall ≥ 85% | 1,398 / 4,054 = 34.48% | 1,398 / 4,054 = 34.48% | **no** |

Four of the six floors still fail. Every measure of the three corpora is identical and no
site changes: the goldens' trusted bands hold no trait impl's member a trusted stage
cannot prove (`import-guided` 109/109, none a trait impl's; `inheritance-guided` 60/60,
its four conditional trait impls' members proven by the calling impls). The issue's
reproductions are withheld by tests, not by A0.

A0 after issue 118, 2026-10-16 (a name a Rust `use` brace group binds leads where its own
path does; Stage 3 only, engine 12; guarded by issue 115). Evidence, not an evaluation;
the status stays `pending`. Same pins, oracles and rules; baseline post-117 (`27f7d5b`).

| Golden | Floor | After issue 117 | After issue 118 | Floor met |
|---|---|---|---|---|
| Python | `typed` precision ≥ 97% | 314 / 314 = 100.00% | 314 / 314 = 100.00% | yes |
| Python | union precision ≥ 95% | 624 / 657 = 94.98% | 624 / 657 = 94.98% | **no** |
| Python | recall ≥ 85% | 606 / 1,114 = 54.40% | 606 / 1,114 = 54.40% | **no** |
| Rust | `typed` precision ≥ 97% | 860 / 860 = 100.00% | 860 / 860 = 100.00% | yes |
| Rust | union precision ≥ 95% | 1,601 / 2,501 = 64.01% | 1,648 / 2,541 = 64.86% | **no** |
| Rust | recall ≥ 85% | 1,398 / 4,054 = 34.48% | 1,445 / 4,054 = 35.64% | **no** |

Four of the six floors still fail. Trusted bands, false edges: Rust golden `typed` 0
(860/860), `import-guided` 0 (213/213, from 109/109), `inheritance-guided` 0 (60/60); PDX
itself `typed` 0 (5,911/5,911), `import-guided` 0 (261/261, from 52/56),
`inheritance-guided` 0 (76/76). The four sites PDX itself drew wrong are `exact` now, still
wrong, R3 acceptance cases (issue 102). Python is identical.

A0 after R3, issue 102, 2026-10-16 (`exact` and `scoped` are weak evidence, never drawn;
Stage 3 and 4 only, engine 12; guarded by issue 115; `a0-metrics-2`: the union and
recall count drawn bands only). Evidence, not an evaluation; the status stays `pending`.
Same pins, oracles and rules; baseline post-118 (`1feb3d7`).

| Golden | Floor | After issue 118 | After R3 | Floor met |
|---|---|---|---|---|
| Python | `typed` precision ≥ 97% | 314 / 314 = 100.00% | 314 / 314 = 100.00% | yes |
| Python | union precision ≥ 95% | 624 / 657 = 94.98% | 339 / 339 = 100.00% | yes |
| Python | recall ≥ 85% | 606 / 1,114 = 54.40% | 321 / 1,114 = 28.82% | **no** |
| Rust | `typed` precision ≥ 97% | 860 / 860 = 100.00% | 860 / 860 = 100.00% | yes |
| Rust | union precision ≥ 95% | 1,648 / 2,541 = 64.86% | 1,133 / 1,133 = 100.00% | yes |
| Rust | recall ≥ 85% | 1,445 / 4,054 = 35.64% | 930 / 4,054 = 22.94% | **no** |

Two of the six floors still fail, both recall's. The union is now the drawn bands'
(`typed`, `import-guided`, `inheritance-guided`) and has no false edge in either golden or
in PDX itself (6,248 / 6,248); before R3 it held `exact` and `scoped`, which carried every
remaining false edge. Recall falls by exactly the correct weak evidence (285 and 515
relations), which recall work recovers with positive evidence (issues 105 to 108, 113,
116), never by drawing weak evidence again. G1b stays pending: recall is a first-class
blocker (decision 65), and a precision gain alone passes no gate.

A0 after R4, issue 107A and issue 120, 2026-10-16 (Rust turbofish calls extracted, patch
0033, engine 13; A0's matcher skips Rust comments and literals, `a0-match-2`). Measured on
the Rust golden only, the corpus the change touches (the driver's method from R4: the
three-corpus A0 runs at the next integration checkpoint). Evidence, not an evaluation;
the status stays `pending`. Baseline post-R3 (`b3dd5d1`).

| Golden | Floor | After R3 | After R4 | Floor met |
|---|---|---|---|---|
| Rust | `typed` precision ≥ 97% | 860 / 860 = 100.00% | 874 / 874 = 100.00% | yes |
| Rust | union precision ≥ 95% | 1,133 / 1,133 = 100.00% | 1,162 / 1,162 = 100.00% | yes |
| Rust | recall ≥ 85% | 930 / 4,054 = 22.94% | 959 / 4,110 = 23.33% | **no** |

The Rust golden's 56 relations with no site are all at one now (`site_missing` 56 → 0);
29 are drawn, all correct, and no drawn edge is wrong. The 345 inside macro token trees
are issue 107B's. Python's floors are unchanged (no Python extraction changed). Two of
the six floors still fail, both recall's.

A0 at the stabilisation checkpoint, 2026-10-25: the three-corpus run (`a0-match-3`,
`a0-metrics-2`, engine 14) measuring R4, R5 (issues 105, 113A, 113B, 116) and the
stabilisation round (issues 96, 97, 99, 121 to 128) together, PDX itself at `f1e406a`.
Results in `bench/results/a0/` (methodology's Runs). Evidence, not an evaluation; the
status stays `pending`.

| Golden | Floor | After R4 | Stabilisation | Floor met |
|---|---|---|---|---|
| Python | `typed` precision ≥ 97% | 314 / 314 = 100.00% | 314 / 314 = 100.00% | yes |
| Python | union precision ≥ 95% | 339 / 339 = 100.00% | 339 / 339 = 100.00% | yes |
| Python | recall ≥ 85% | 321 / 1,114 = 28.82% | 321 / 1,114 = 28.82% | **no** |
| Rust | `typed` precision ≥ 97% | 874 / 874 = 100.00% | 3,520 / 3,520 = 100.00% | yes |
| Rust | union precision ≥ 95% | 1,162 / 1,162 = 100.00% | 3,581 / 3,581 = 100.00% | yes |
| Rust | recall ≥ 85% | 959 / 4,110 = 23.33% | 3,371 / 4,159 = 81.05% | **no** |

PDX itself (diagnostic): union 8,849 / 8,849, recall 8,810 / 10,040 = 87.75%. Every drawn
band is precise in every corpus. Two of the six floors still fail, both recall's: Rust
is 3.95 points short (788 relations classified in issue 105, 296 more in macro token
trees, issue 107B); Python needs issue 106's recall work.

## G2 — after `P3-10`

Status: **pending**

Criteria:

- [ ] (v1.4) G1 and G1b passed (decision 72)
- [ ] (v1.4) agent-outcome benchmark recorded, the PDX arm at least as correct as the baseline arm on every task class (4.13.4)
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
- [ ] (v1.4) server scale on the server reference profile: the `linux` scale repository indexed by one index-job worker at ≥ 400 files/s with peak RSS ≤ 8 GiB (decision 78)

Evaluation log:

_not yet evaluated_

## G4 — after `P6-08`

Status: **pending**

Criteria:

- [ ] Structural accuracy published for all golden repos (4.13.1): `typed` precision ≥ 97% and union of drawn bands precision ≥ 95% on oracle-verifiable sites, oracle-verifiable fraction reported
- [ ] recall at or above each language's floor (v1.4, decision 73: ≥ 85% for Java/TS/Python/Go/C#/Rust/Kotlin, ≥ 80% Ada on the subset, ≥ 70% C/C++, or the measured structural ceiling less 5 points where lower, never below the recall at v1.4); product-level structural recall not below G1b's
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
- [ ] trust page generated, with the agent-outcome results (v1.4, 4.13.4)
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
- [ ] (v1.4) comparative benchmark (P9-10) published
- [ ] (v1.4) agent-outcome PDX arm at least as correct as the baseline on every task class
- [ ] (v1.4) the shipped dependency closure audited (decision 83)
- [ ] **all G1, G1b, G3 and G4 correctness and security floors meet their stated values — no small-miss waiver at release**

Evaluation log:

_not yet evaluated_
