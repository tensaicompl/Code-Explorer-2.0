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
| P1-02 | done | PENDING | The reuse map's grammar rules were incomplete in three ways, all widened in the copy script rather than patched per file: grammar-local headers of any name, source fragments a scanner includes, and the wrappers that exist for every language the reference supports rather than the matrix's. Two foundation headers are taken without their implementations, one inert and one supplied by our layer. Grammars compile through their wrappers, not directly, or every table would be defined twice. Six patches' worth of compile fixes reduced to three patch files; clang rejects three implicit declarations that gcc only warns about |
| P1-03 | todo | | |
| P1-04 | todo | | |
| P1-05 | todo | | |
| P1-06 | todo | | |
| P1-07 | todo | | |
| P2-01 | todo | | |
| P2-02 | todo | | |
| P2-03 | todo | | |
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
