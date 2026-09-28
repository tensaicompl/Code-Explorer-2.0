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
| 13 | 2026-09-28 | P1-02 | ambiguity | resolved | The reuse map's grammar rule names the files to copy, and misses three kinds the build needs: headers of other names, source fragments a scanner includes, and the per-language wrappers that actually compile the tables | The copy rule now takes the grammar directory whole, and the wrappers only for the matrix's languages. Naming a subset invites the next omission |
| 12 | 2026-09-28 | P1-01 | ambiguity | resolved | The file-level reuse map lists the service-pattern sources under the pipeline directory; they are in the extraction directory, and the wildcard that copies it already takes them | Copied by the wildcard, landing beside the extraction core rather than the resolution sources. Recorded as decision 10; the map's intent is met |
| 11 | 2026-09-28 | P0-04 | blocker | resolved | The deny list's prefix patterns required a word boundary, so the prefix survived in a script where it followed a word character. The scan passed while a stricter test failed | Boundary dropped from the prefix patterns, and the pattern for the type prefix widened. Verified against a planted fixture |
| 10 | 2026-09-28 | P0-04 | blocker | resolved | The provenance scanner used a bash 4 builtin to read its file list, and macOS ships bash 3.2, so every scanner test failed there while passing on Linux and Windows | Replaced with portable read loops and an explicit counter, since an empty array also expands inconsistently under set -u across those versions. Verified against bash 3.2.57 in a container. The non-git exclusions were fixed at the same time |
| 9 | 2026-09-28 | P0-04 | blocker | resolved | The first continuous integration run failed on both platforms: the step activating the pinned package manager ran at the repository root, where there is no manifest declaring it | Step moved to the interface directory in all three workflows. The Rust build passed on all three platforms, including Windows, so only this step was at fault |
| 8 | 2026-09-28 | G0 | blocker | resolved | Gate G0 requires the gate green on Linux and macOS. No macOS host is available, so one criterion of five cannot be evaluated locally | Closed by continuous integration run 36361547926: the gate is green on Linux and macOS, and the binary builds on Windows. Two real defects had to be fixed first, issues 9 and 10 |
| 7 | 2026-09-28 | P0-06 | ambiguity | open | The importance formula's test penalty is given as 0.3 for test nodes; the factor for a non-test node is never stated | The mirror records the source as written rather than filling the silence. To be settled when the metric is implemented, by a specification change request if the answer is not the identity |
| 6 | 2026-09-28 | P0-06 | ambiguity | resolved | The source's band table carries a stray blank line between its first and second rows, which in this markup would split one table into two | The mirror emits one contiguous table of eleven rows, which is what the source's own closing sentence states. The source is not edited |
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

### 13 — The reuse map's grammar rule is not closed

The map lists what to copy from a grammar directory: the parser, the scanner, one
named header and the parser interface headers. Three things the build needs are
outside that list.

A scanner may include a header of any name, and two do. One grammar's scanner
includes further source fragments, which are not headers at all. And the tables are
not compiled directly: each language has a wrapper among the engine sources that
includes its parser and scanner, which is how generated tables declaring static
symbols of the same names are kept in separate translation units. The reference
carries a wrapper for every language it supports, 162 of them, and copying the
extraction core wholesale brought all of them, each reaching for a parser that was
never copied.

The copy rule now takes a grammar directory whole, since these directories hold the
compilation inputs and nothing else, and takes wrappers only for the matrix's
languages. What is compiled is decided by the build definition rather than by what
happens to be present.

The wrappers also address the grammars where the reference nests them. The map puts
them one level higher, so that path is rewritten as they are copied, and the same
normalisation now applies to any include reaching into the vendored tree.

State: resolved.

### 12 — The reuse map misplaces the service-pattern sources

The map is authoritative for what is copied and from where, and it lists the
service-pattern header under the pipeline directory. It is not there: both the
header and its source are in the extraction directory, which the map's first rule
copies wholesale. They are therefore already taken, and land beside the extraction
core rather than beside the resolution sources.

Nothing is lost and nothing needs inventing: the reason the map calls for them, that
the call driver's service-pattern strategy needs them, is satisfied either way.
Recorded rather than corrected in place, because the map is delivered from outside
the repository.

State: resolved.

### 11 — The prefix could hide behind a word character

The deny list matched the upstream symbol prefix only at a word boundary. In a
script that quoted the pattern as a regular expression, the prefix followed a `b`,
so there was no boundary and the scan passed. A test written with a plain substring
check failed on the same file, which is how it came to light.

The boundary is gone from the lowercase prefix pattern and the pattern for the type
prefix is widened, so the prefix is now rejected wherever it appears. Verified by
planting it in a fixture. The literal that triggered this is gone from the script,
which now recognises those patterns by a marker in the deny list rather than by
repeating them.

Worth noting for the rules themselves: a stricter check found what the scan missed,
so the scan is a floor and not a ceiling.

State: resolved.

### 10 — The provenance scanner did not run on macOS

The scanner read its file list with `mapfile`, a builtin introduced in bash 4.
macOS ships bash 3.2, so on that platform the scanner failed before reading a
single file, and six of its seven tests failed with it. Linux and Windows were
unaffected, which is why local work never saw it.

Replaced with a read loop that works on both. An explicit counter is kept rather
than querying the array's length, because an empty array expands inconsistently
under `set -u` between those bash versions, and the scanner would then fail on an
empty tree instead of reporting one.

Verified against bash 3.2.57 in a container, which is the version macOS ships: a
clean fixture passes and a planted denied term is caught, with the same exit codes
as on Linux.

Fixing it surfaced a second defect in the same script. The exclusions are expressed
as git pathspecs, so outside a repository nothing was excluded and the scanner read
its own deny list, reporting every term in it as a finding. The non-git path now
applies the same exclusions by path. This affected the fixture tests' mode of
operation, not the repository scan.

State: resolved.

### 9 — The first continuous integration run failed on a configuration defect

Both gate jobs failed at the step that activates the pinned package manager, with
"Couldn't find a project in the local directory". The pinned version is declared in
the interface's own manifest, and the step ran at the repository root, where there is
no manifest at all. The workspace root deliberately has no package file, so this
would have failed on the first run whatever the platform.

The Rust half was unaffected: the Windows job, which builds and tests the binary and
touches none of this, passed.

This is the class of defect that no local check could have caught, because the task
that wrote the workflows cannot run them; the acceptance tests for that task covered
the scanners it also delivered, not the workflows. The first push is the first
execution, which is why it came now rather than later.

Fixed in all three workflows by running the step in the interface directory.

State: resolved.

### 8 — Gate G0 cannot be fully evaluated without a macOS host

G0's first criterion is the gate green on Linux and macOS. This machine is Linux.
The other four criteria are met and their output is recorded verbatim in
`docs/plan/GATES.md`.

The continuous integration workflow already runs the gate on both platforms, so the
criterion is closed by the first push to the remote rather than by any further work
here. Nothing about the code is in doubt; what is missing is evidence from a platform
this machine cannot provide.

Handling follows the plan's own rule for an environment the agent does not have: the
affected check is recorded as outstanding, work continues where it does not depend on
it, and the criterion remains a gate requirement rather than becoming a waiver. G0 is
not marked passed until macOS is green, and the first phase that depends on
platform-specific behaviour is the one that vendors and compiles the engine, which is
next.

Closed. The gate is green on both platforms, at run 36361547926. Getting there took
three runs and fixed two defects that no local check could have caught, recorded as
issues 9 and 10. G0 is marked passed with the job results recorded in
`docs/plan/GATES.md`.

State: resolved.

### 7 — The importance formula is silent on non-test nodes

The per-symbol importance formula multiplies three factors, and the source states
the third as 0.3 for a test node. It never states the value for a node that is not
a test. Read as a penalty, the factor is the identity for everything else, which
makes 1.0 the only sensible reading, and that is almost certainly what is meant.

A mirror does not settle it. Filling a silence inside a normative document creates
a constant nobody chose, in the one place an implementer will treat as authority.
The document therefore records the factor exactly as the source gives it, and the
question is recorded here instead.

To be settled when the metric is implemented. If the answer is not the identity,
that is a specification change request with a measurement behind it.

State: open. Blocks nothing until the metric is built.

### 6 — The source's band table is split by a stray blank line

In the source, a blank line sits between the first and the second row of the
confidence band table. Rendered, that is two tables rather than one, the second
without a header. The source's own closing sentence states that there are eleven
bands, so the intent is unambiguous.

The mirror emits one contiguous table of eleven rows. The source document is not
edited: it is delivered from outside the repository and is not ours to change.

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
