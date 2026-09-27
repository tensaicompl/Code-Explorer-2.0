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
| 1 | 2026-09-27 | P0-01 | ambiguity | open | The replaced NOTICE carried a standing directive never to publish and to keep the repository private; the plan's open-core split and release pipeline contradict it | Plan followed (it is the later authority and P0-01 publishes nothing). Needs human confirmation before P9-03 makes it irreversible |
| 2 | 2026-09-27 | P0-01, P0-02 | ambiguity | open | Both tasks' acceptance tests need infrastructure that later tasks create: the Rust test harness (P0-03) and `scripts/licence-scan.sh` (P0-04). The tests cannot fail-then-pass within their own task | Deliverables verified by inspection now; the named tests are written when the harness exists and are a G0 criterion |

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

State: open until confirmed. Blocks nothing before P9-03.

### 2 — First two tasks cannot run their own acceptance tests

P0-01's acceptance names `scripts/licence-scan.sh`, created by P0-04, and the test
`legal_files_present`; P0-02's names `legacy_tree_snapshot` and a test in
`bench/src/lock.rs`. All of them need the Cargo workspace and the scanner scripts,
which P0-03 and P0-04 create. The task protocol's fail-first-then-pass sequence is
therefore not achievable inside these two tasks without implementing later tasks'
deliverables early, which the protocol forbids.

Handling: the deliverables are verified by inspection when the task is done, the
named tests are written as part of the tasks that create the harness, and G0
requires both scans and the snapshot to be green before P1 begins. No
specification change is needed.

State: open until G0.
