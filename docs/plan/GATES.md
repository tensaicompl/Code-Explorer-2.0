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
| G1a | `P1-07` | pending | |
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

Status: **pending**

Criteria:

- [ ] Engine builds with `-Werror` on clang and gcc
- [ ] `abi_smoke` passes for every typed language
- [ ] ASan corpus run clean
- [ ] no upstream-prefixed symbols

Evaluation log:

_not yet evaluated_

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
