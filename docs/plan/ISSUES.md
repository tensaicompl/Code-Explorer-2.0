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
| 20 | 2026-09-28 | P1-03 | ambiguity | resolved | Appendix I.2 states the shim's semantics in a line each, and four of those lines differ from what the reference actually does: its store keeps one node per qualified name by a content rule, its short-name lists move a renamed node, the import edges come from its import resolver rather than from extraction's import list, and its language lookup is a table of its own, not the language matrix | The shim reproduces the reference, not the summary, and the language lookup is vendored verbatim rather than rewritten. Differential fixtures against the reference confirm it |
| 19 | 2026-09-28 | P1-03 | scr | open | Part 5.1 requires `-Wall -Wextra -Werror` for the engine, with no exemption. Measured: the vendored typed-resolution layer, which the reference compiles with every warning suppressed, carries dead code and style warnings under both compilers, so a literal reading does not build | Warnings are now errors across the engine by default, replacing decision 11. Nine warning classes that report dead code or style stay visible but non-fatal in vendored code only, plus one gcc false positive in one file. The interface layer has no exemption. Awaiting approval of the exemption list |
| 18 | 2026-09-28 | P1-03 | scr | open | The plan's premise that the cross-file surface can be cached (Part 9.5, D20, 4.5 Stage 2) does not hold for the surface the reference builds: its definition rows are computed through the project's registry and import map, so they depend on other files, and they omit what the resolver reads to resolve the file's own calls | A file's surface is the context-free snapshot of every fact the engine records for it, minus the tree. A cache hit resolves from it with a re-parse and no re-extraction, and gives the same answers as a fresh extraction, which a test checks on every fixture. Awaiting approval of the corrected definition |
| 17 | 2026-09-28 | P1-03 | scr | open | Appendix D.2 and D.3 cannot express what the typed pass answers: calls that exist only for typed resolution, callables passed as values, and call sites the pass itself adds; and nothing tells a caller which answers must not be followed by a guess | Appended fields: `typed_only` and lexical bits on each call, lexical bits on each usage, and the site's description on each resolution. References are reported as calls with `is_reference` set, as D.2 already defines. The reporting rules follow the reference's own calls and usages passes. Awaiting approval |
| 16 | 2026-09-28 | P1-03 | scr | resolved | Appendix D's typed-resolution strategy vocabulary is exhaustive and normalised, while section 4.2.2 rule 1 requires the engine's strategy string to be kept verbatim for calibration. Normalising alone loses which kind of dispatch resolved a call | Owner decision 2026-09-28: keep the normalised `strategy` exactly as Appendix D specifies, and append an `engine_strategy` field carrying the verbatim string. Implemented; the fixtures check the verbatim strategy against the reference's |
| 15 | 2026-09-28 | P1-03 | scr | resolved | Appendix I's copy list omits the import-target resolver, which lives in a pipeline source it does not vendor. The typed resolver builds its import map from the edges that resolver creates, so without it TypeScript, JavaScript, Go, Java and C# resolve with an empty import map, contradicting Appendix D's import-map strategies | Owner decisions 2026-09-28: vendor the smallest exact closure of the resolver, and supply repository metadata (packages, path aliases, the root crate manifest) through the interface, produced in Rust with the registry work in P2-05. Implemented, and the vendored subset gives the reference's answer on every differential fixture. Aliased and module-internal imports need that metadata; until P2-05 supplies it from real manifests they resolve as for a repository that declares none, a staged gap with an owner, not an accuracy limit |
| 14 | 2026-09-28 | P1-02 | ambiguity | resolved | The strip list cannot be built at this task. Deciding what nothing needs requires the interface layer, because that layer is what reaches the extraction entry points and the per-language registration tables; a trial analysis now reports files as unreferenced that demonstrably are not | Decided with the interface in place: a program referring to every interface function, linked against the archive, pulls in all but four objects, which are stripped. A build test now keeps every archive member reachable |
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

### 20 — The shim's semantics, as the reference actually has them

Appendix I.2 gives each function the shim replaces a one-line meaning. Four of those
lines are summaries that differ from the reference once measured against it, and the
resolver's answers depend on the difference:

- **Lookup by qualified name.** "Unique lookup" is right, but the uniqueness comes
  from how nodes are added: the reference's store keeps one node per qualified name
  and, when a second arrives, picks the survivor by content (smaller path, then
  larger start line, then name, then kind), and never lets a module displace a
  directory of the same name. The shim now adds nodes that way. It used to keep the
  first node inserted, which gives a different target whenever two definitions share
  a name, as a C struct and function can.
- **Lookup by short name.** "In insertion order" holds until an update renames a
  node; the reference then swaps the last entry into its old slot and appends it to
  its new list. Reproduced exactly.
- **Import edges.** Not served from extraction's import list: they are the edges the
  vendored import resolver creates (issue 15), deduplicated as the reference
  deduplicates them, by source, target and local name.
- **Language by file name.** Not "the language matrix": the reference's own table,
  which differs from the matrix for C headers, `.m` and four other extensions. The
  import resolver gates a fallback on it and the registry vetoes matches across
  language families with it, so the reference's table is vendored verbatim
  (`src/resolve/language_lookup.c`); the per-user configuration it consults is never
  loaded here, as the language matrix is fixed.

The setup the resolver reads is also built in the reference's order: the project node,
then each file followed by its unseen directories, then every definition, then the
import edges. The differential fixtures (engine/tests/fixtures/resolve) compare the
result with the reference's own graph.

State: resolved.

### 19 — Warnings as errors in the vendored engine

Part 5.1 specifies `-Wall -Wextra -Werror -fno-strict-aliasing` for the engine and
names no exemption; gate G1a asks for a build with warnings as errors on both
compilers. Decision 11 had made warnings non-fatal in vendored code instead, which
does not meet that, and was taken without measuring.

Measured with every warning an error: gcc reports 38 warnings in nine classes and
clang 30 in five, nearly all in the typed-resolution layer. The reference itself
compiles that layer, the macro preprocessor and the grammars with every warning
suppressed, and its other sources with unused parameters and sign comparisons
exempt. What the layer carries is unused static functions, unused parameters and
variables, a comment containing a comment opener, and bounded writes into fixed
buffers. One warning is a known gcc false positive: a freshly allocated block passed
as a const pointer so its usable size can be asked for.

Done, and stricter than the reference: warnings are errors across the whole engine
by default. In vendored code only, these classes stay visible and non-fatal:
`unused-function`, `unused-parameter`, `unused-variable`, `unused-but-set-variable`,
`unused-const-variable`, `unused-value`, `comment`, `sign-compare`, and
`format-truncation`, the last as the reference does; plus `maybe-uninitialized` in
the one file with the false positive. Every other class is an error, and the
interface layer has no exemption at all. Four classes that indicate real defects,
implicit declarations and integer, pointer and return-type mismatches, are errors
everywhere whatever the switch says; two such defects were found this way and fixed.

Builds clean under gcc 13 and clang 18 with this configuration.

The specification change requested: Part 5.1's flags read "`-Wall -Wextra -Werror`,
with the exemptions listed in engine/CMakeLists.txt for vendored code".

State: open, awaiting approval of the exemption list.

### 18 — What a cached surface must hold

The plan caches each file's surface so that resolution never re-extracts an unchanged
file (Part 9.5, D20, 4.5 Stage 2), and describes the surface as the definition rows
each file contributes to the typed registry, noting that the reference already
serialises them. Two measured facts make that surface the wrong thing to cache:

- The reference computes those rows through the project's registry and each file's
  import map (base types are resolved to their qualified names), so a file's rows
  depend on other files. Cached from one build and reused in the next, they would
  make the answer depend on the previous build, which 4.5 forbids. The reference
  can accept that because its incremental path repairs it separately.
- Resolution also resolves the file's own call sites, and for that it reads the
  file's calls, usages, imports, namespace and the answers extraction already found,
  none of which the rows carry.

Implemented instead: a file's surface is the snapshot of every fact the engine
records for the file, every field of every structure, minus only the parse tree, the
retained source and memory bookkeeping. A test reads the engine's structure
definitions and fails if a field is not encoded. A file added with a result rebuilt
from the cache resolves from its surface, parsing the source once more and extracting
nothing. Every resolution fixture is resolved both ways and the answers must be
identical. Rows that depend on other files are recomputed in every build, as the
full-repository rule requires.

The specification change requested, for 4.5 Stage 2: "The cross-file *surface* is a
file's own facts as the typed resolver reads them, independent of every other file,
and is cached with the extraction result, so that an unchanged file is never
extracted again."

State: open, awaiting approval of the corrected definition.

### 17 — Call sites the typed pass answers that Appendix D cannot name

Appendix D.3 identifies a resolved call by its index into the file's calls. Three
kinds of site the reference's typed pass answers do not fit that, and one rule a
caller needs is missing:

- **Typed-only sites.** Some calls exist only as questions for typed resolution: an
  operator the language turns into a method call, a protocol method called
  implicitly. The reference never resolves these by name, because the name is not
  text in the source; a caller that did would bind `int + int` to some project
  `operator+`. D.2 has no way to mark them.
- **Callables passed as values.** The reference resolves these through its usages
  pass. D.2 already has `is_reference` for them, but the engine reports them as
  usages.
- **Sites the pass adds.** Resolving a file can add call sites extraction did not
  report. They have no index in extraction's calls.
- **Answers that settle a site.** For a call, the reference ignores a typed answer
  whose target is not in the graph, or is the calling definition, and resolves by
  name. For a reference, a typed answer settles the site even when its target is
  outside the project.

Implemented, by appending and nothing else: `typed_only` and `lexical` on each call,
`lexical` on each usage; usages that may
be callable references reported as calls with `is_reference` set, and not again as
usages; sites the pass adds numbered after extraction's calls; and each resolution
carrying its site's description. The reporting rules are the reference's calls and
usages passes up to the point where each has a target, using the reference's own
matcher to join an answer to its site. The header states the contract. Positions of
sites found in C-family text after macro expansion are reported as unknown rather
than wrongly, since they are offsets into text the caller never sees.

- **What resolving by name needs.** When typed resolution has no answer, the
  reference's usages pass consults lexical facts the extractor recorded: whether a
  reference was spelled as one, whether a name is the member half of a selector, and
  whether a binding in the same code blocks it, locally or not. The Rust port of that
  pass needs them, so they are carried now, as a `lexical` bit field on calls and
  usages, rather than by a second interface change in P2.

What the Rust stages do after that, the textual fallbacks and when a reference
becomes a call-reference edge rather than a use, is the port of those passes in P2.
The notes for it are in engine/README.md.

State: open, awaiting approval of the additions to Appendix D.

### 16 — Strategy vocabulary and calibration disagree

Appendix D declares the typed-resolution strategy vocabulary exhaustive: a small
normalised set from which the confidence band is derived. Section 4.2.2's first rule
requires the engine's own strategy string to be stored verbatim on every edge, so
that the bands can be calibrated against what actually produced them. The engine's
strings are finer than the vocabulary: several kinds of dispatch all normalise to one
typed value. Normalising alone would satisfy Appendix D and discard exactly what the
calibration rule exists to keep.

Owner decision, 2026-09-28: the normalised `strategy` stays exactly as Appendix D
specifies and is what the band is computed from, and an `engine_strategy` field is
appended carrying the verbatim string. Appending changes a normative structure, so
this is recorded as a specification change rather than a decision rule application.

Implemented: every resolution carries the engine's strategy verbatim beside the
normalised one, and the differential fixtures check it against the strategy the
reference records on its own edges. Appendix D is not mirrored in the repository, so
there is no document to update; the public header carries the field and its reason.

State: resolved.

### 15 — The import-target resolver was left out of the reuse map

The link-level coupling between the cross-file resolver and the reference's pipeline
was measured in advance, and that measurement held: nine functions, now implemented.
What it did not measure is semantic coupling. The resolver builds its import map from
import edges in the graph it reads, and those edges are created by a resolver in a
pipeline source the reuse map does not vendor.

For Python and Kotlin that is survivable: the resolver falls back to the extraction's
own import records, and its comments call the edges best-effort. For TypeScript,
JavaScript, Go, Java and C# there is no fallback. Their import maps come from the
edges alone, so without the resolver they are empty, and every cross-file call that
depends on an import resolves worse, silently. Appendix D's import-map strategies
could not be produced for those languages.

Owner decision, 2026-09-28: a specification correction rather than a workaround.
Vendor the smallest exact source closure of the resolver from the pinned reference,
determined by dependency analysis, not the whole source it lives in. Preserve its
algorithms and their order, including header resolution, module resolution, member
preference, the language-gated sibling and symbol fallbacks, namespace resolution and
deterministic candidate selection. Take no manifest scanning or package discovery. If
the closure reaches into the store, discovery or package scanning, stop and record the
dependency rather than widening what is vendored. Any filesystem read inside the
closure is routed through the in-memory source provider and nothing else about it
changes.

Closed only when differential fixtures show that, for identical inputs, the vendored
subset returns the same target or the same absence of one as the pinned reference.

**Closure, determined by dependency analysis** (a call-graph walk over the pinned
reference, resolving each callee to its caller's own source first so a static helper
is never confused with a same-named function elsewhere): 48 functions.

- 19 in the resolver's own source, 789 lines: the part to vendor.
- 19 in the name computation and the foundation, already vendored.
- 2 graph lookups, already served by the registry shim.
- 4 in discovery (language by extension, user configuration). Not reached: they sit
  behind the language-by-file-name function, which the shim already replaces.
- **4 that depend on package scanning**: 3 resolving build-tool path aliases, and 1
  returning the package map. Both are consulted by one strategy each, inside the
  module resolver, and both are null-guarded, so the resolver compiles and runs with
  neither. What neither can do is produce their data, which is built by scanning
  manifests: `tsconfig` paths, `go.mod`, `package.json`, `Cargo.toml`.

No part of the closure touches the store, and none reads the filesystem, so there is
no input boundary to reroute.

**Stopped here, as instructed**, because the closure reaches package scanning. The
code it reaches is small and inert without data; the data is the dependency. Without
it, two kinds of import resolve differently from a fully indexed reference: aliased
imports through `tsconfig` paths, and module-internal imports that need the module
path declared in a manifest, of which Go's are the main case.

Owner decisions, 2026-09-28, on where package data comes from: keep discovery and
manifest parsing in Rust, and define now the interface that supplies their results.
The engine owns the resolution algorithms; Rust owns finding and reading manifests;
the interface carries already-resolved metadata to the resolver. Vendor only the
algorithm closure, adding the alias resolver's functions only because the resolver
needs them once aliases are supplied. The producer is scheduled with the registry work
in P2-05, reused by P2-08, and is not a permanent limit: the aliased and
module-internal imports that need it are a staged gap with an owner, closed before
the language accuracy gates.

Done:

- `pdxe_resolution_metadata` carries packages (import prefix to entry path), path
  alias scopes and entries, and the root crate manifest. The crate manifest was found
  the same way: the Rust resolver reads the root manifest from disk to learn workspace
  members, so that read is replaced (patch 0004) by the supplied manifest, parsed in
  Rust, with member names derived in the engine as its own reader derives them.
- Nothing in the resolver reads the filesystem. The language lookup the closure
  reaches is vendored verbatim (issue 20), so the earlier note that it sits behind a
  replaced function no longer applies.
- Differential fixtures against the pinned reference, each also asserting what it
  proves: TypeScript and JavaScript imports with member calls, a Java package import
  and a cross-file constructor, C# `using`, C and C++ includes, a Python import that
  names a real module against one that falls back by name with two candidates, and a
  negative case where neither a missing module nor an unknown name gets a target.
- Go, in the two tests the owner named: `go_import_differential_no_pkgmap` (the
  reference's answer without a module declaration; parity only, not module
  resolution) and `go_import_resolves_with_supplied_pkgmap` (the module declared
  through the interface places the import and its calls in the repository). The
  third, `go_mod_metadata_drives_import_resolution`, belongs to P2-05. TypeScript
  aliases and Rust workspaces follow the same pattern.
- With metadata supplied, each fixture gives the answer the reference derives from
  reading the manifests itself, so the interface carries what the manifests mean.
- Adding the subset pulled in no store, discovery or package-scanning code.

State: resolved.

### 14 — The strip list cannot be decided before the interface layer exists

The build task is supposed to remove every copied source whose symbols nothing
references once the excluded subsystems are gone, and the reuse map is explicit that
this is decided from a trial build rather than by reading. The build now exists, so
the analysis can be run. It cannot yet be trusted.

Run against the archive, it reports nine objects as unreferenced. At least one of
them is plainly needed: the Python standard-library table, whose registration
function the Python resolver calls by name. Two others are referenced only from the
cross-file resolution sources, which are not in the build yet. The rest are reached
either from those same sources or from the interface layer, which does not exist:
that layer is what will call the extraction entry points and register the per-language
tables, so from the compiler's point of view they currently have no callers at all.

Stripping on that evidence would delete code the next task wires up, and the deletion
would look correct right until the moment it did not.

Deferred to the interface-layer task, where the picture is complete, and closed
before the phase gate rather than carried past it. The script and its list exist and
are exercised; what is missing is a decision nobody can make yet.

Decided at P1-03, with the interface complete. A program that refers to every
function the public header declares, linked against the archive, pulls in 73 of its
77 objects; the link map is the evidence. The four it never reaches are stripped: a
multi-pattern matcher, a string interner, the process-wide allocator setup, and the
allocator's implementation, whose header stays because kept sources include it for
types. The Python standard-library table and the resolution sources the earlier trial
flagged are all reached.

A build test now links the same program and fails if any archive member is
unreachable, so the list cannot drift. After stripping, the engine rebuilds and every
interface test passes, under the sanitizers too.

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
