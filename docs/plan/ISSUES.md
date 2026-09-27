# Issues

Numbered log of blockers, ambiguities, specification change requests and
third-party problems. Unfinished work is recorded here with a task reference,
never as a marker comment in code — the provenance scanner fails the build on
those markers.

Open an entry when: an acceptance test cannot pass without changing a normative
specification; a required third-party component is unavailable, unlicensed for
our allow-list, or fails the provenance scan; or a gate fails. Then continue with
the next task that does not depend on the halted one.

Types: `blocker`, `ambiguity`, `scr` (specification change request),
`third-party`. States: `open`, `resolved`, `withdrawn`.

| # | Date | Task | Type | State | Summary | Resolution |
|---|---|---|---|---|---|---|
| 5 | 2026-09-28 | P0-05 | blocker | resolved | The constants task merged with the gate red: its invariant assertions were rejected by the linter under all targets, which the test run does not exercise | Invariants converted to compile-time assertions, which are stronger than the tests they replace. The linter added to the pre-commit subset so the class is caught before a commit exists |
| 1 | 2026-09-27 | P0-01 | ambiguity | resolved | The replaced NOTICE carried a standing directive never to publish and to keep the repository private; the plan's open-core split and release pipeline contradict it | Owner confirmed the open-core split on 2026-09-28: the plan stands, the old directive is superseded, publishing at P9 is intended |
| 4 | 2026-09-28 | P0-04 | third-party | resolved | Three build-only interface dependencies carry licences outside the allow list: a permissive model licence, a documentation licence on a browser-support data file, and an old permissive licence on a small parser | Owner confirmed on 2026-09-28 that the allow list governs shipped code only. Enforcement stays strict on shipped dependencies and reports build-only ones; no specification change |
| 3 | 2026-09-27 | P0-02 | third-party | resolved | The C golden repository relicensed away from BSD-3: Redis 8 and later are tri-licensed RSALv2 / SSPLv1 / AGPLv3, and the allow list forbids all three | Replaced with the language matrix's stated fallback under D5; decision 1 |
| 2 | 2026-09-27 | P0-01, P0-02 | ambiguity | resolved | Both tasks' acceptance tests need infrastructure that later tasks create: the Rust test harness (P0-03) and `scripts/licence-scan.sh` (P0-04). The tests cannot fail-then-pass within their own task | Tests written in P0-03, the task that creates the harness, and verified to fail when their invariant is broken |

## Entries

### 1 — Publishing directive in the replaced NOTICE contradicts the plan

The `NOTICE` file that P0-01 replaces stated, in its own words: keep the
repository private, never add a publish step, no npm, no marketplace, no public
container registry, no public release workflow, and no copy to anyone without
written approval. It also claimed the whole web dashboard as first-party work,
which is the attribution gap P0-01 exists to close.

The plan supersedes that: it licenses the core crates and the interface under
Apache-2.0, and its release tasks add installers, an npm wrapper, a Homebrew
formula, public container images and signed releases.

Both cannot hold. The plan is the later and more specific authority and is
followed. P0-01 itself publishes nothing and is reversible: it only writes licence
files inside a private repository. The point of no return is the release pipeline,
so a human decision is recorded as required before that task runs.

Resolved. The owner confirmed on 2026-09-28 that the open-core split is intended:
the core crates and the interface are Apache-2.0, the architecture model, estate
analytics, server, chart and operations documentation are proprietary, and the
public packaging at the release phase is deliberate. The directive in the replaced
file is superseded rather than overlooked.

Nothing needs revisiting: the split as built matches the confirmed intent.

State: resolved.

### 5 — A commit with a red gate reached the trunk

The constants task asserted its invariants in unit tests. Those tests pass under
the test runner, but the linter rejects an assertion whose operands are all
constants, and the linter only sees it when run across all targets. So the task's
own verification was green and the gate was red.

It merged anyway, which is the part that matters. The command that ran the gate and
the command that committed were joined by a newline rather than by a conjunction,
so the commit ran despite the gate having failed. The task protocol says the gate
passes before a merge; here the check ran and its result was discarded.

Fixed on both counts. The invariants are now compile-time assertions, which is
strictly stronger: a violation stops any build anywhere rather than waiting for a
test run, and each one names the rule it broke. Verified by setting a value that
breaks one and watching the build refuse it. The linter is now part of the
pre-commit subset, so a lint failure cannot reach a commit, let alone the trunk.

The gate is green on the trunk again as of the fixing commit.

State: resolved.

### 4 — Build-only dependency licences sit outside the allow list

The allow list names ten licences. Three dependencies of the interface toolchain
carry others: a permissive model licence on a path-matching library, a
documentation licence on a browser-support data table, and an old permissive
licence on a small argument parser. None is copyleft and none carries a
redistribution obligation we would breach.

What matters is that none of them ships. The bundle contains production
dependencies only, and every one of those is permissively licensed; these three
exist to build and lint, and never reach a user. The licence scanner therefore
fails on a shipped dependency outside the list and prints a note for a build-only
one, so an outlier stays visible without stopping work on a question that is not
a legal risk.

Resolved. The owner confirmed on 2026-09-28 that the allow list governs shipped
code only, which is the reading the scanner already implements: an outlying licence
on a dependency that reaches a user fails the build, one on a tool that merely
builds it is reported. No specification change, and the list does not grow every
time the toolchain gains a transitive dependency.

State: resolved.

### 3 — C golden repository is no longer permissively licensed

The plan names a repository as the C golden corpus that was BSD-3 when the plan
was written. Its licence has since changed: release 8 and later are tri-licensed
under a source-available licence, a server-side public licence and a strong
copyleft licence. The allow list forbids all three, and the benchmark corpus that
is committed to this repository takes files from the golden repositories, so this
is not merely a question of what we index.

Resolved by the decision rule for exactly this case: replaced with the fallback
repository the language matrix names for that language, which is permissively
licensed (its code under MIT; its manual and two bundled components under other
permissive terms). Pinned in `bench/repos.lock` with the replacement and the
reason recorded next to it.

Recall and precision numbers for C are therefore measured against a different
corpus than the plan assumed. The gate threshold for C is the lowest of any
language, and nothing in the plan depends on the identity of that repository
beyond its language and comparable size.

State: resolved.

### 2 — First two tasks cannot run their own acceptance tests

P0-01's acceptance names `scripts/licence-scan.sh`, created by P0-04, and the test
`legal_files_present`; P0-02's names `legacy_tree_snapshot` and a test in
`bench/src/lock.rs`. All of them need the Cargo workspace and the scanner scripts,
which P0-03 and P0-04 create. The task protocol's fail-first-then-pass sequence is
therefore not achievable inside these two tasks without implementing later tasks'
deliverables early, which the protocol forbids.

Handling: the deliverables were verified by inspection when each task was done,
and the tests were written in the task that created the test harness, which is
the first point at which they can exist. They live in the benchmark crate:
`tests/lock_files.rs` covers the lock files, and `tests/repository_invariants.rs`
covers the legal baseline and the move. Both were checked against a broken tree —
removing the notice file and removing a moved directory each make them fail — so
they are not passing vacuously. No specification change was needed.

State: resolved.
