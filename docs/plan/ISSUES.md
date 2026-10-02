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
| 36 | 2026-10-02 | P2-03 | ambiguity | open | Part 5.12 requires `cargo vet` records for new crates in `supply-chain/`, but no task delivers it: there is no `supply-chain/` directory, no audit configuration and no check, and every crate added since P0 is unvetted. Licences are checked (`cargo deny`); provenance of the code itself is not | For the owner to place: a task that sets up `cargo vet`, imports trusted audits and records the existing crates, and a check that fails on an unrecorded one. P2-03 adds crates under the current policy (D1, licence scan) and does not set up vetting on its own |
| 35 | 2026-10-02 | P2-03 | ambiguity | resolved | Stage 1 and Appendix C leave choices open: no binary-file detector; Appendix C mixes defaults with examples; whether `[secrets] patterns` replaces or extends the 5.12 defaults; where `.pdxignore` applies and how it combines with `.gitignore`; which `[precise.<family>]` tables and keys exist; what the hard-coded excludes match | Implemented as recorded in the entry: a file is binary when it holds a NUL byte (owner-approved); example values are not defaults; `patterns` replaces the defaults; the root `.pdxignore` only, as an independent rule set; the four documented families, each with its documented command and a `timeout_minutes` override; excludes match directories by name at any depth |
| 34 | 2026-10-02 | P5-03 | ambiguity | open | 4.3 gives `evidence.evidence_id` as the "sha of (fact_id, provider, provider_version, verdict)" and `semantic_occurrences.occ_id` as the "sha of (provider, symbol, file_id, start_byte, end_byte, role)", without the byte encoding issue 33 fixed for the graph's ids: separators, how numbers are written, and the output's form and length. Both are stored keys | Owned by P5-03, which first writes these rows: fix both encodings, by specification change, before a precise segment is published. P2-02 stores and reads the ids exactly as given and computes neither |
| 33 | 2026-10-02 | P2-01 | scr | resolved | 4.2.1 defines every identity as a hash, but leaves byte-level choices open that two implementations could make differently and so produce different stored ids: how `ast_fingerprint` serialises its node-type path, texts and ordinal, and in what form `site_id` reads it; the case of the base32 output; whether the overload disambiguator is hex text or raw bytes, and how the identical-signature ordinal is written; what an absent enclosing definition or site contributes; which clone URLs `canonical_clone_url` accepts | Approved by the owner on 2026-10-02 exactly as P2-01 implemented them, and written into 4.2.1 with reference vectors. `SEGMENT_SCHEMA_VERSION` stays 1: this completes the initial identity format before the first segment writer, so nothing stored is invalidated |
| 32 | 2026-10-02 | G1a | blocker | resolved | Evaluating G1a found the local engine build compiled without warnings as errors in vendored sources: its build directory had once been configured with `PDXE_VENDORED_WERROR` off, CMake keeps an option's last value, and neither `make engine` nor `make check-asan` stated it. The continuous integration builds were unaffected, but a local check could pass on a weaker policy than it appeared to | Both targets now state the policy on every configure (`make engine PDXE_VENDORED_WERROR=OFF` remains the explicit way to relax it). Rebuilt locally with gcc 13.3 and clang 18: every non-grammar source carries `-Wall -Wextra -Werror`, and every warning printed is in issue 19's exempt set |
| 31 | 2026-10-02 | P2-07 | ambiguity | open | Appendix A gives `javascript` the test rule "same as TS", and TypeScript's file-name patterns are `*.test.ts` and `*.spec.ts`: read literally, no JavaScript file is a test by its name, and neither is a TypeScript `.tsx`, `.mts` or `.cts` file. Nor does it say where a directory pattern such as `tests/**` applies, at the repository root or at any depth | Owned by P2-07, which derives tests from these rules: decide, by specification change if the answer is not the literal reading. The registry records the rules exactly as Appendix A writes them, the JavaScript rule as TypeScript's by reference, so a reading of them changes no data |
| 30 | 2026-10-02 | P1-07 | ambiguity | resolved | Appendix A leaves parts of language detection unsaid: it gives `bash` a shebang without naming a form, writes two patterns over a file's name (`Dockerfile*`, `.env*`) beside the extensions without saying which wins when a name matches both kinds, and says nothing of case | Implemented to the letter where Appendix A speaks and narrowly where it is silent: a shebang is a `#!` first line naming `bash`, directly or through `env`, and nothing else, `sh` included; an entry with `*` is a pattern over the name, any other an extension the name ends with; an extension decides before a name pattern, and a name pattern before a shebang; matching is case-sensitive. Each choice has a test, and any can be changed by a specification change that bumps the matrix version |
| 29 | 2026-10-02 | P1-06 | blocker | resolved | The first sanitizer run over the corpus failed: for a project whose definitions typed resolution keeps none of, building the C# resolver's shared registry takes an offset from a NULL array, which is undefined behaviour in C. A documentation-only project reaches it; no fixture had | Fixed by `engine/patches/0008`, in the C# builder and in the Java one, which has the same shape and no caller yet. The regression `abi_run_health_untyped_only` fails under the sanitizers without the patch. No sanitizer report is suppressed |
| 28 | 2026-10-02 | P1-06 | ambiguity | resolved | The interface promises it writes nothing to standard output or standard error, but the TypeScript resolver prints a line to standard error when one of its work budgets runs out, whatever the logging switches say. The interface's no-output test does not see it because its fixtures never exhaust a budget | The two unconditional prints are removed through the vendored patch mechanism (`engine/patches/0007`); the lost-work count at both sites remains, and evaluation still degrades to an unknown type. A no-output regression starves the budget and proves every TypeScript run of the corpus exhausted it and was degraded for it, with nothing written; the sanitizer corpus is covered by the no-output contract too |
| 27 | 2026-10-02 | P2-04 | ambiguity | open | Isolated extraction survives a crashing engine but not a hanging one: the worker protocol bounds what crosses it and how often a worker is restarted, not how long a batch may take, and 4.5 names no failure reason for a file the engine never finishes | Owned by P2-04, which batches extraction: decide whether a batch has a time limit and how a file that exceeds it is recorded, by specification change if it needs a new reason |
| 26 | 2026-10-02 | P2-04 | ambiguity | open | The extraction cache key in 4.5 is `(engine_version, language_matrix_version, secret_policy_digest, blob_sha)`, but an extraction also depends on the file's path, from which its qualified names and module are built, and on the node budget the environment may set (`PDX_ENGINE_WALK_MAX_NODES`), under which it can come back truncated | Owned by P2-04: the key gains the path, or an entry is used only for the path it names, which every `FileExtract` records; a truncated extraction, which says so, is not cached, or the budget joins the key |
| 25 | 2026-10-02 | P1-05 | scr | resolved | Two facts the engine records about a file did not cross the interface: the exceptions each definition raises, which the `THROWS` edge of 4.2.3 is built from, and that the extractor stopped at its node budget. P1-05's `FileExtract` lists `throws`, which Appendix D.2 does not have, and omits channels and configuration reads, which it does | Approved by the owner on review, 2026-10-02, as implemented: a throw array, positionless like the type references, and a `truncated` flag appended to `pdxe_file_result`; `FileExtract` carries all ten arrays of the interface; `THROWS` stays resolution's to derive |
| 24 | 2026-09-28 | P1-04 | ambiguity | resolved | The engine had never been built for Windows. The reuse map's foundation list omits a Windows-only header the kept sources include, and the vendored sources rely on POSIX names that the reference's Windows runtime provides and Microsoft's, which Rust links against, does not | The header is vendored like the others; the missing names are supplied at the build boundary for Microsoft's runtime only; the libraries a Windows link needs are named; one Windows-only use of the unlinked allocator is put behind its switches by a patch. Found by the first Windows builds of P1-04 |
| 23 | 2026-09-28 | P9-02 | ambiguity | open | `pdx-engine-sys` is marked publishable, but its build script builds the engine from `../../engine`, which a crate packaged on its own does not contain | Owned by P9-02: either the engine sources travel inside the packaged crate, or the crate stops being publishable. Nothing in P1 publishes a crate |
| 22 | 2026-09-28 | P0-04 | blocker | resolved | The nightly run failed on targets that belong to later tasks: the browser suite has no browser installed and the engine differential has no script yet, while four other targets reported success without checking anything. A red nightly could not be told from a regression | Each unfinished target now says it is skipped and names the task that brings it, and succeeds. The progress check fails once that task is done while its target still skips |
| 21 | 2026-09-28 | P1-05 | ambiguity | resolved | The cross-file pass reports success whatever happens inside it: an allocation failure yields no typed answers at all, and a file whose source cannot be supplied is skipped and counted, and its log goes to a sink that discards it. A caller cannot tell no typed answer from a degraded run | A completed run reports its health: clean or degraded, every file counted by what typed resolution did with it, and the work lost without skipping a file, which after a review audit includes every failed allocation and exhausted work budget anywhere in typed resolution, extraction included. Counted on the run's thread and kept in the run's record, never read from a log. Answers and carriers are copied whole or lost and counted |
| 20 | 2026-09-28 | P1-03 | ambiguity | resolved | Appendix I.2 states the shim's semantics in a line each, and four of those lines differ from what the reference actually does: its store keeps one node per qualified name by a content rule, its short-name lists move a renamed node, the import edges come from its import resolver rather than from extraction's import list, and its language lookup is a table of its own, not the language matrix | The shim reproduces the reference, not the summary, and the language lookup is vendored verbatim rather than rewritten. Differential fixtures against the reference confirm it |
| 19 | 2026-09-28 | P1-03 | scr | resolved | Part 5.1 requires `-Wall -Wextra -Werror` for the engine, with no exemption. Measured: the vendored typed-resolution layer, which the reference compiles with every warning suppressed, carries dead code and style warnings under both compilers, so a literal reading does not build | Approved 2026-09-28: warnings are errors by default; a named set of harmless classes stays non-fatal in vendored sources only; the interface layer has none; defect-class diagnostics are fatal everywhere. The exemptions first reached the interface layer through the target; now attached to vendored files only, with a test that each class still fails in `engine/api` |
| 18 | 2026-09-28 | P1-03 | scr | resolved | The plan's premise that the cross-file surface can be cached (Part 9.5, D20, 4.5 Stage 2) does not hold for the surface the reference builds: its definition rows are computed through the project's registry and import map, so they depend on other files, and they omit what the resolver reads to resolve the file's own calls | Approved 2026-09-28 as implemented: a surface holds the file-local facts typed resolution reads and no cross-file state; unchanged files re-parse, never re-extract; cross-file state is recomputed every resolution; fresh, cached and reverse-order resolution are equivalent. Section 4.5 updated |
| 17 | 2026-09-28 | P1-03 | scr | resolved | Appendix D.2 and D.3 cannot express what the typed pass answers: calls that exist only for typed resolution, callables passed as values, and call sites the pass itself adds; and nothing tells a caller which answers must not be followed by a guess | Appended: `typed_only` and lexical facts on calls, lexical facts on usages, the site's description on each resolution. The review found the call facts the reference's weak-call guards read were missing; added as three separate facts and proven against the reference by four fixtures |
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

### 36 — `cargo vet` is required but owned by no task

Part 5.12: "Dependencies: pinned via lockfiles; `cargo vet` recorded for new crates in
`supply-chain/`." Lockfiles are pinned and every dependency's licence is checked by
`cargo deny` (`scripts/licence-scan.sh`), but nothing in the repository runs or records
`cargo vet`: there is no `supply-chain/` directory, and no task in the plan delivers
one. Every crate added since P0 (serde, sha2, postcard, rusqlite, and now ignore,
globset and toml) is therefore unvetted.

Setting up vetting is a project-wide decision (which audits to import, how to record
the existing crates, whether the check gates CI), not discovery's. P2-03 adds its
crates under the policy in force, D1 and the licence scan, and leaves this for the owner
to place in the plan.

State: open.

### 35 — Discovery where Stage 1 and Appendix C are silent

Stage 1 (4.5) and Appendix C leave several choices open. P2-03 implements these
readings, each tested in `crates/pdx-core/tests/discover.rs` and `tests/config.rs`:

- **Binary files.** 4.5 says binary files are recorded as `binary` and gives no
  detector. A file is binary when its bytes contain a NUL (0x00): exact, over the
  whole file (which is at most `max_file_bytes`), independent of locale and encoding,
  and not dependent on a sample size. Bytes that are not valid UTF-8 alone do not make a
  file binary. Approved by the owner for P2-03.
- **Defaults and examples.** Appendix C shows every key with a value, and some are
  examples rather than defaults. Defaults are what the specification states:
  `include_vendor = false` (4.5), `max_file_bytes = MAX_FILE_BYTES`, the secret
  patterns of 5.12, `[precise] enabled = false`, `languages = []`,
  `timeout_minutes = 60` (Appendix C, 4.6.2). `extra_excludes`, `[languages] extra`,
  `[layers] rules` and `[rules] architecture` default to empty: `generated/**`,
  `.blade.php`, the `web/**` layer rule and `no-api-to-persistence` are examples.
  `[identity]` and `[server]` are absent by default.
- **Secret patterns.** 5.12 calls the setting `secret_patterns` and gives its default;
  Appendix C names it `[secrets] patterns`, which is the key read. A value given
  replaces the default list, as setting any default does; a repository can therefore
  narrow it, and the effective list is what `secret_policy_digest` (P2-04) records.
  Patterns use gitignore glob syntax without negation, matched against the path below
  the root: a pattern without `/` matches the file name in any directory.
- **`.pdxignore`.** The root `.pdxignore` only, in gitignore syntax, as a rule set of
  its own: a negation in it can undo another of its rules, never a `.gitignore`,
  hard-coded or `extra_excludes` exclusion, and no `.gitignore` negation can undo it. A
  path is excluded when any source excludes it. `extra_excludes` are gitignore patterns
  rooted at the repository root, and a negation among them is refused.
- **Ignore files that are not named.** `.gitignore` files are read at every level
  below the root, never above it; the user's global ignore file, `.git/info/exclude`
  and the generic `.ignore` file are not read, so discovery depends on the checkout and
  `pdx.toml` only. Hidden files are not excluded.
- **Hard-coded excludes.** Directories named `.git`, `node_modules`, `target`, `build`,
  `dist` and `vendor` are excluded at any depth and never entered; `.git` is also
  excluded when it is a file (a worktree's or submodule's pointer). A regular file
  with one of the other names is a file like any other.
- **Precise configuration.** The families are those Appendix C and 4.6 document:
  `java` (`build_cmd`), `ts` (`install_cmd`), `python` (`install_cmd`) and `cxx`
  (`compdb_cmd`). 4.6.2's example places `timeout_minutes = 90` under `[precise.cxx]`,
  so each family may set `timeout_minutes`, overriding `[precise] timeout_minutes` for
  that family. 4.6.1's "Maven/Gradle mirrors configured by `[precise.java]`" names no
  keys, so none are accepted; P5 defines them.

State: resolved.

### 34 — Evidence and occurrence ids have no byte encoding

4.3 describes two more stored identities in comments on the DDL:
`evidence.evidence_id`, the "sha of (fact_id, provider, provider_version, verdict)",
and `semantic_occurrences.occ_id`, the "sha of (provider, symbol, file_id, start_byte,
end_byte, role)". Neither says how the fields are joined, how the byte offsets are
written, or what form and length the result takes, which is what issue 33 settled for
node, site and edge ids. Two implementations could store different keys for the same
verdict.

P2-02 stores both columns and reads them back exactly as given, as strings, and
computes neither: producing these rows is the precise merge's (P5-03), which owns
fixing both encodings, by specification change, before the first precise segment is
published.

State: open.

### 33 — Identity encodings 4.2.1 leaves open

Identities are stored: a segment's rows, a cache's keys and every reference between
them are keyed by them, and a change of encoding renames every node. 4.2.1 gives each
as a formula. Where a formula leaves a byte-level choice open, two correct
implementations could differ, so P2-01 proposes one reading of each, implements it in
`crates/pdx-core/src/ids.rs`, and locks it with fixed vectors computed outside the code
under test. Every string is hashed as its UTF-8 bytes, and no input may contain a NUL
byte, which the formulas use as a separator: the functions refuse one rather than hash
an ambiguous sequence.

- **`base32-crockford(...)[0..26]`.** The alphabet `0123456789ABCDEFGHJKMNPQRSTVWXYZ`,
  in upper case, as Crockford's encoding writes it. The digest is read as a bit string,
  most significant bit first, five bits to a symbol, and the first 26 symbols are kept:
  the digest's first 130 bits, which is what "26 characters, 130 bits" requires. (An
  encoding of the digest as one integer would carry a single bit in its first symbol.)
- **`repo_id` from a URL.** `lowerhex(sha256(canonical_clone_url))[0..16]`: the same
  form as a registered `repo_id`. `canonical_clone_url` accepts only an `https` URL with
  a host and a path: the scheme is matched in any case and written `https://`; user
  information is removed; the host is lowercased; a port is kept as written; the path
  is kept byte for byte, except that one trailing `.git` is removed. Anything else is
  refused: another scheme (`http`, `ssh`, the `git@host:path` form), no host, no path,
  a query or a fragment. 4.2.1 says "the HTTPS clone URL" and nothing about deriving
  one, so a repository whose remote is an SSH URL has no URL-derived id until the
  command that indexes it (P2-11) says where its HTTPS URL comes from.
- **The overload disambiguator.** `lowerhex(sha256(normalised_signature))[0..8]`: eight
  hex characters, as text, like the other identities. A callable is overloaded when
  another callable has the same kind, path and qualified name; a callable that is not
  has the disambiguator `""`. When members of such a group have identical normalised
  signatures, every one of them gets `-k` appended, `k` counting from 1 in file order
  among those identical ones only, so `…-1`, `…-2` as 4.2.1 writes it. Inserting an
  overload with a new signature therefore changes no existing id. One consequence follows
  from the formula itself: a callable that was alone and gains an overload changes from
  `""` to its signature hash, once.
- **`ast_fingerprint`.** Stored in `sites.ast_fingerprint` as
  `lowerhex(sha256(n || 0x00 || type_1 || 0x00 || … || type_n || 0x00 || callee_text || 0x00 || receiver_text || 0x00 || ordinal))`,
  64 characters. `n` is the number of node types in the path from the enclosing
  definition to the site and `ordinal` the position of this (path, text) pair among the
  definition's sites with the same path and texts, both in decimal ASCII; the ordinal
  counts from 1, as the overload ordinal does. Node types are non-empty. A site with no
  receiver has `receiver_text` `""`; a site other than a call carries its text as
  `callee_text`, which is the `sites` column it is stored in. The count makes the path's
  end unambiguous whatever the types contain.
- **`site_id`.** Reads `ast_fingerprint` in its stored form, the 64 hex characters. A
  site outside every definition (at the top of a file) has `enclosing_node_id` `""`.
  `site_kind` is one of the `sites` table's values: `call`, `reference`, `import`,
  `type_ref`, `field_rw`, `route`, `contract`.
- **`edge_id`.** Concatenated without separators, as 4.2.1 writes it: node and site ids
  have a fixed length of 26 and the edge kinds are a closed set, so nothing is
  ambiguous. An edge with no site contributes `""`.

Two inputs are not P2-01's to produce: Appendix B.1 does not spell out signature
normalisation beyond 4.2.1's sentence, and the engine interface does not report the
node-type path of a site. `ids.rs` takes both as inputs; the tasks that build
definitions and sites from extraction (P2-04, P2-06) supply them, by extending the
engine interface if they must.

Approved by the owner on 2026-10-02, exactly as implemented, including the accepted
consequence that a callable changes its id once when it gains its first overload.
4.2.1 now states every rule above at byte level, with reference vectors from
`tests/ids.rs`, so a second implementation can reproduce the ids from the
specification alone; the vectors are unchanged.

`SEGMENT_SCHEMA_VERSION` stays 1, and no other version moves: this completes the
previously unspecified byte encoding of the initial version 1 identity format before
P2-02 creates the first segment writer, and no version 1 segment has been emitted to
migrate or invalidate. From P2-02 on, segments store these ids, so an incompatible
change to any of these encodings is a change of stored format and follows the
specification and version process.

State: resolved.

### 32 — A cached build option weakened the local warning policy

Gate G1a asks whether the engine builds with warnings as errors on clang and gcc. The
continuous integration evidence answers it: the engine `pdx-engine-sys` builds, in a
fresh directory, is checked source by source by `the_engine_is_compiled_with_its_own_warning_policy`
on Ubuntu (gcc 13.3) and macOS (Apple clang 15).

Checking the same thing locally found a hole. The build `make engine` makes, in
`target/engine`, had compiled 37 of its 50 non-grammar sources without `-Werror`: its
CMake cache held `PDXE_VENDORED_WERROR=OFF` from an earlier configure, and CMake keeps an
option's last value until told otherwise. `make engine` never told it, nor did
`scripts/check-asan.sh` for its own directory. Nothing in the repository or its
workflows sets the option off, so no continuous integration build was affected; but any
reused build directory could run the local checks under a weaker policy than they
claim, and nothing would say so.

Both now state the policy on every configure: `make engine` passes
`-DPDXE_VENDORED_WERROR=$(PDXE_VENDORED_WERROR)`, `ON` unless the caller asks
otherwise, and the sanitizer build passes `ON`. After the change, rebuilt locally with
gcc 13.3 (Release) and clang 18 (the sanitized build): every non-grammar source carries
`-Wall -Wextra -Werror`, the build succeeds, and every warning it prints is in the exempt
set issue 19 approved (unused functions, parameters and variables, comments, a sign
comparison, format truncation).

State: resolved.

### 31 — JavaScript's test rule is TypeScript's, whose patterns name `.ts`

Appendix A's test rule for `javascript` is "same as TS", and TypeScript's is
`*.test.ts`, `*.spec.ts` and `__tests__/**`. Read literally, the two file-name patterns
match only names ending in `.ts`, so no JavaScript file is a test by its name, only by
its directory; the same reading leaves TypeScript's own `.tsx`, `.mts` and `.cts` files
out. The likelier intent, `*.test.js` and the like, is not what the row says.

Separately, the directory patterns (`src/test/**`, `tests/**`, `test*/**`, `t/**`,
`spec/**`, `__tests__/**`) do not say whether the directory must be at the repository
root or may be at any depth, which decides, for example, whether
`packages/api/tests/x.py` is a test.

P1-07 records the rules as Appendix A writes them: the patterns exactly, and the
JavaScript rule as a reference to TypeScript's (`TestDetection::SameAs`), resolved by
`Language::test_rules`. Applying them is P2-07's, which derives `Test` nodes and
`TESTS` edges (Appendix B.5 defers to Appendix A), so the reading is P2-07's to settle,
by specification change if it is not the literal one.

State: open.

### 30 — Language detection where Appendix A is silent

Appendix A assigns languages by extension, then shebang (4.5 Stage 1), and leaves four
things unsaid:

- **Shebang forms.** The `bash` row lists "shebang" among its extensions and names no
  form. The registry recognises a first line beginning `#!` whose interpreter is
  `bash`: the last component of the first word, or, when that is `env`, of the first
  word after `env`'s options and assignments. `#!/bin/bash`, `#! /bin/bash -e` and
  `#!/usr/bin/env -S bash -eu` qualify; `#!/bin/sh` does not, because Appendix A does
  not name it, even though it maps `.sh` to `bash`. No other row has a shebang, so a
  `#!/usr/bin/env python3` script without an extension has no language.
- **Patterns over the name.** Most entries are extensions; three contain `*`. Read as
  patterns over the file's name, `*.dockerfile` is an ending, and `Dockerfile*` and
  `.env*` are beginnings: `.env*` is `.env`, `.env.local` and the like, not
  `prod.env`.
- **Which wins.** No extension of Appendix A ends with another, and only `.h` belongs to
  two languages, which its own rule decides. A name can still match an extension and a
  beginning: `Dockerfile.md`, `.env.json`. The extension decides, then a beginning,
  then the shebang; a shebang never overrides a name. Detection takes the longest
  matching extension, which matters only once `pdx.toml` adds extensions such as
  `.blade.php` (P2-03).
- **Case.** Appendix A writes every entry in one case and never says it may vary, so
  matching is exact: `README.MD` and `dockerfile` have no language.

The `.h` rule is followed to the letter too: its markers are matched byte for byte,
so `template <` with a space is not `template<`.

Each choice is tested in `crates/pdx-core/tests/languages.rs`. Any of them can be
changed by a specification change, which bumps `LANGUAGE_MATRIX_VERSION`.

State: resolved.

### 29 — Typed resolution takes an offset from a null array

The first run of the sanitizer corpus under `make check-asan` failed with an
undefined-behaviour report in the C# resolver's builder of its shared registry. The
builder copies the project's C# definitions into an array, types first, and registers
the two halves, the second starting at an offset into the array. When the cross-file
pass has kept no definitions at all, there is no array: the pointer is NULL and the
offset zero. Nothing is read through it, but an offset from NULL is undefined in C, and
the sanitizer, which halts on the first report, rejects it.

The pass builds that registry whenever extraction found any definitions, so a project
whose definitions typed resolution keeps none of reaches it: a documentation-only one,
the corpus's Markdown directory. None of the fixtures was such a project.

`engine/patches/0008-no-offset-from-null.patch` passes NULL on unchanged when there is
no array. The Java builder has the same shape and is not called by the pass today; it
gets the same guard, so the first caller does not meet the defect again. The regression
`abi_run_health_untyped_only` resolves a project of one Markdown file and requires a
clean run with the file counted untyped; without the patch it fails under the
sanitizers. The report was fixed, not suppressed: no sanitizer option or suppression
list was added.

State: resolved.

### 28 — The TypeScript resolver writes to standard error

The interface's header promises that nothing in it writes to standard output or
standard error, because it runs inside a program that speaks a protocol on those
streams; the engine's debugging prints are switched off by default and its log goes to
a sink that discards it. The TypeScript resolver's two work budgets, for parsing type
text and for evaluating expressions, print a line to standard error the first time
either runs out in a file, unconditionally. `abi_no_output` checks the promise over the
fixtures, none of which exhausts a budget, so it has never seen this.

Found while auditing issue 21, which now counts the same moment as lost work; the
print was left alone there because the output contract is a separate question.

Owned by P1-06, whose corpus is where budgets run out: remove the prints by patch, and
run the no-output check over the corpus as well as the fixtures.

Resolved by P1-06. `engine/patches/0007-typescript-budget-silent.patch` replaces both
prints, for the type-text budget and the expression budget, with the count of lost work
the run already relies on (issue 21), through the vendored patch mechanism, so a refresh
keeps it. The budgets are unchanged, evaluation still degrades to an unknown type when
one runs out, and a run that ran one out is degraded.

`abi_no_output` now runs over the sanitizer corpus as well as the fixtures, every
language extracted and resolved both directly and through the cache, and the corpus's
TypeScript runs must come back clean. `abi_no_output_ts_budget` runs the same with
`PDX_ENGINE_TS_TYPE_BUDGET=1`, a budget of one unit of work per file, and does not
take the setting as proof: each of the four TypeScript runs (TypeScript and TSX, direct
and cached) must report a degraded run with work lost, and nothing may reach either
output stream. The runs without the setting must be clean, so the degradation is the
budget's. With the patch reverted the test fails: several kilobytes reach standard
error, and none of the four runs counts the loss.

State: resolved.

### 27 — A hanging engine hangs isolated extraction

`PDX_ENGINE_ISOLATE=1` runs extraction in a worker process so that an engine abort
costs one file and not the indexing run (P1-05). The protocol between the two is
bounded in what it carries: frames have a size limit, an incomplete or malformed
frame is refused, and a dead worker is replaced. It is not bounded in time. An engine
that loops forever on a file leaves the parent waiting on that batch for good, in
isolated mode as in process.

Specification 4.5 names one failure for a file the engine does not survive,
`engine_crash`; it has no name for a file the engine never finishes, and whether a
time limit belongs to a batch, a file, or the whole stage is a question about memory
budgets and batching, which are P2-04's. A wall-clock limit also makes a file's
outcome depend on the machine, which the determinism rules have to allow for
explicitly.

Owned by P2-04. Nothing in P1-05 depends on it.

State: open.

### 26 — The extraction cache key leaves out what an extraction depends on

Specification 4.5 keys the extraction cache by
`(engine_version, language_matrix_version, secret_policy_digest, blob_sha)`. Two
inputs of an extraction are not in it.

- **The path.** The engine builds every qualified name and the module name from the
  file's path relative to the repository, and the resolution surface names the path
  too. Two files with the same content at different paths, or one file moved, have
  different extractions and the same key.
- **The node budget.** The environment can set a node budget for the extractor
  (`PDX_ENGINE_WALK_MAX_NODES`, off by default). A file over it comes back with only
  what the walk reached, and typed resolution skips it. The interface now says so,
  in `truncated` (issue 25).

The safe wrapper is built so that neither can do harm silently: a `FileExtract` names
the path and language it was taken for and the length of its source, resolution adds
a file under the path its extraction names, and a source of another length is
refused. But a cache keyed as 4.5 says would still hand a moved file the extraction of
its old path.

Owned by P2-04: add the path to the key, or use an entry only for the path it names;
and either keep truncated extractions out of the cache or add the budget to the key.
The same holds for an extraction that lost work to a failed allocation or an exhausted
budget (issue 21): its surface records the loss, so every run resolving it is
degraded, but `FileExtract` has no field that says so, and whether such an extraction
is cached, or the interface should say so as it says `truncated`, is P2-04's to
decide. P2-04's task text now names the key of 4.5 and this issue
(`docs/plan/amendments.json`).

State: open.

### 25 — Throws and truncation do not cross the interface

**Throws.** Specification 4.2.3 has a structural `THROWS` edge, and P1-05's
deliverable lists `throws` among `FileExtract`'s fields, but Appendix D.2's result has
no throw array, and P1-03 implemented D.2 as written. The engine does record throws:
every result carries the exception each throw or raise statement names and the
function it is in. The reference turns those into edges in a pipeline pass this
project does not vendor, resolving the exception's name to a definition. So the facts
exist at extraction and the edge is resolution's to build, as this project's
resolution stages in Rust build every other structural edge from extraction facts.

Appended to `pdxe_file_result`, after the existing arrays so no offset moves:
`pdxe_throw { exception_text, scope_index, span }` and its count. The span is all zero:
the engine records no position for a throw, as it records none for type references,
field accesses, channels and configuration reads, which the header now says of all
five. A result rebuilt from a cache has no throws, like its channels and
configuration reads; the cache keeps the owned extraction, which has them.

The engine also reads the exceptions a method declares, but only through a grammar
field named for the declaration, and of the matrix's grammars only Java's is looked
for and it has no such field: Java's `throws` clause is not reported, and the
reference behaves the same. The test that covers Java declares an exception and
counts the throws exactly, so the day the engine reads declarations it fails and
says so. Whether declared exceptions should produce `THROWS` edges is a question for
the resolution stages, not for this interface.

**Truncation.** The extractor can stop walking a file at a node budget the
environment sets, off by default. The file's facts are then those the walk reached and
typed resolution skips it, while its status still read "parsed". Appended a
`truncated` flag after the throws.

**The `FileExtract` shape.** P1-05 names `{ definitions, calls, imports, usages,
type_refs, throws, read_writes, diagnostics }`. The interface's result also carries
channels and configuration reads, which 4.7 and `READS_CONFIG` need, so `FileExtract`
carries every array the interface has: definitions, calls, imports, usages, type
references, throws, reads and writes, channels, configuration reads and diagnostics,
with the file's status, whether it was truncated, and its resolution surface. The
wrapper takes every structure of the interface apart field by field, so an array or
field the interface gains cannot be left out of the safe layer without the build
failing.

Tests: `abi_throws` and `abi_truncated` (engine), and the safe wrapper's extraction
tests, which check every array arrives non-empty from sources that have each fact.

Approved by the owner on review, 2026-10-02, as implemented, including the model of
the safe wrapper carrying every array of the extraction, that `THROWS` is derived by
resolution rather than produced by extraction, and that Java's declared exceptions
stay as described above.

State: resolved.

### 24 — The engine had never been built for Windows

P1-04 is the first task to build the engine on Windows, and the first builds found
three gaps, all invisible to a build on Linux or macOS.

- **A header the reuse map leaves out.** The kept foundation sources include a
  header-only set of UTF-8 path conversions inside their Windows branches. The map's
  foundation list names only what a build elsewhere needs, so the header was never
  copied. It is now copied with the other foundation headers.
- **Names a different C runtime provides.** The reference builds for Windows against
  a runtime that has `strcasecmp`, `strncasecmp`, `strtok_r`, `ssize_t` and the
  headers `<strings.h>`, `<unistd.h>` and `<pthread.h>`. Rust links against
  Microsoft's runtime, which has the functions under other names and neither the
  type nor the headers. Several vendored sources use them outside their Windows
  branches. `engine/api/windows` supplies them for that runtime only: a header
  included ahead of every source maps the names, and stand-ins let the includes
  resolve. The thread and system-call stand-ins declare nothing, since the engine's
  Windows branches use the operating system's own threads; a real use would fail to
  compile.
- **What a Windows link needs.** Once the engine compiled, linking it into a Rust
  program left names unresolved. The POSIX names Microsoft's runtime does declare
  are resolved by its `oldnames` library, which Microsoft's compilers name in every
  object and clang, with CMake choosing the runtime, does not; the objects now name
  it. The foundation's Windows branches call `advapi32` (access control on the
  directories it creates) and need random names for them; the libraries are named
  in the engine's CMake project and in the build script, and the random source is
  defined by our own layer, as the vendoring always intended. The thread source
  releases each exiting thread's heap in the allocator, which is not linked here, and
  reports the thread's end to the memory instrumentation, which is not built;
  `engine/patches/0005-windows-thread-exit.patch` puts the release behind the
  allocator's switches, where every other use of it already is, and our layer answers
  the report as it answers the instrumentation's other call. The same patch makes
  the callback's slot in the loader's table read-only, which clears a linker warning.
  No vendored source is changed in place.

Two defects in the build itself surfaced at the same time and are fixed in the build
script (`crates/pdx-engine-sys`): a verbatim engine path that broke the sources'
relative includes, and base flags from the build tooling that switched all warnings
off on every system. A test now holds each engine build to the warning policy.

State: resolved.

### 23 — A publishable crate that builds from outside itself

`pdx-engine-sys` carries `publish = true`, from the workspace skeleton, and its build
script builds the engine from `engine/`, two directories above the crate. A crate
packaged for a registry contains only its own directory, so as it stands a published
copy could not build: the script stops with a message saying it builds from the
repository.

Nothing in P1 publishes anything, so this does not block the bindings. It belongs to
the packaging task: either the engine's sources travel inside the packaged crate
(copied in by the packaging step, with their licences and notices), or the crate is
marked unpublishable and the engine ships only inside the binaries.

State: open, owned by P9-02.

### 22 — The nightly run failed on work not yet due

The nightly workflow runs `make check-full`, whose targets are the plan's long suites.
Most belong to tasks far ahead. On its last run, before P1-03, the browser suite
failed for want of an installed browser, and the engine differential for want of a
script. Four others (golden, oracle, determinism, performance) printed "not
implemented" from the harness and exited successfully, which is worse: green for a
check that checked nothing.

A nightly that is red for work not yet due hides a real regression when one comes. So
none of that work is started early; instead each unfinished target runs
`scripts/pending-target.sh`, which prints that it is skipped and which task brings
it, and succeeds without running anything partial:

| Target | Arrives with |
|---|---|
| `golden` | P2-12 |
| `determinism`, `perf` | P2-14 |
| `engine-differential` | P2-16 |
| `oracle` | P5-06 |
| `e2e` | P7-01 |

`asan` is real: since P1-03 it runs the engine's tests under the sanitizers, and P1-06
adds its corpus. The progress check (`scripts/plan/plan-progress.py`) reads the
Makefile and fails when a task is done while a target it owns still skips, so no
stand-in outlives its task.

State: resolved.

### 21 — Resolution cannot yet say it was degraded

`pdxe_resolve_project_run` runs the vendored cross-file pass and then collects its
answers. The pass reports success whatever happens inside it, and several of its
paths degrade rather than fail: an allocation failure at its start returns with no
typed answers at all; a file whose source the provider cannot supply is skipped and
counted; allocation failures inside a language's resolver shrink its answers. Its
error log goes to the sink the interface installs to keep standard error silent, so
the evidence is discarded too.

To a caller that is indistinguishable from files that simply have no typed answers.
Once the safe wrapper is the authoritative interface, a degraded run would be read as
a clean one.

Owned by P1-05: resolution must report a status that distinguishes a legitimate
absence of typed answers from an engine or provider failure or a degraded run, before
the wrapper becomes the engine interface. Two routes are visible now: capture the
engine's own error events through the log sink during a run, rather than discarding
them, and report them; and count the pass's skipped files, which it already tallies,
through the interface. Either extends the ABI and belongs to that task, not to a patch
here.

**Resolved in P1-05**, by the second route. The log was not used: its sink is
process-wide while a project is not, and in the resolution code it records one failure
of the several that lose work. Instead `engine/patches/0006` has the pass write its own
record into the project's context, which belongs to one project: whether it reached
its end, whether it collected the project's definitions, and its counts of files
resolved, of files in languages it does not resolve, and of files whose source it did
not obtain. The interface keeps its own account beside it, of which files the pass
asked the source of and was given it, and which files extraction marked over its node
budget.

`pdxe_resolve_project_health` reports, for a run that completed:

- **clean** or **degraded**. Degraded when any work was lost; clean otherwise, which
  includes a project whose files have no typed answer to find. A run that could not
  complete is neither: it returns an error, as before.
- every file counted once, under the first of: its language has no typed resolution;
  it is empty; the pass stopped before reaching it; its source could not be obtained;
  it was over the node budget; it was resolved. The third, fourth and fifth are lost
  work.
- `pass_failures`: work lost without skipping a file. See the audit below.

A degraded run reports every answer it found. The safe wrapper returns the answers and
the health together, and checks the health is consistent before handing it out. The
typed-resolution fixtures now refuse a degraded run, so each of them also shows its
run was clean.

**Review closure (P1-05).** The first resolution counted only the definitions not
collected and the two accounts disagreeing as pass failures. Review found answers that
could still disappear inside typed resolution with the run reported clean, starting
with the copy of each answer into the result. The whole path was audited, and the
target is: clean means every piece of typed-resolution work that could contribute
evidence was done, with no output silently lost; it does not mean every optimisation
succeeded.

*How losses are counted.* The vendored resolvers mostly answer a failed allocation or
an exhausted work budget by doing less, and none of that code knows which project it
works for. Every allocation in it goes through the engine's memory core (arenas,
hash tables and result arrays all do) or through the C library directly. So the
memory core counts every failed allocation (`engine/api/lost_work.h`); the direct C
library allocations whose failure loses work go through counting versions of the same
functions; each resolver work budget counts the first time it runs out in a file; and
the run takes the difference between its start and its end into its own record, then
into `pass_failures`. The engine runs a project on one thread from start to end, so
the difference is that run's and no other's: the count is per thread, read per run,
and the authority is the run's record, never a process-wide count or a log. Losses
during extraction count too, because part of typed resolution happens there: each
result keeps what its extraction lost, a file's surface carries it, and a run adds it
for each of its files.

*The audit.* Each path that produces answers, and what it now does when it fails:

| Path | Failure | Can it lose or change evidence? | Handling |
|---|---|---|---|
| Collecting the project's definitions | allocation | yes: every file resolves against them | counted; the record says they were not collected |
| Converting a definition | a required string (qualified name, short name, receiver) not copied | yes, and it left a definition without its identity | dropped whole, counted |
| Converting a definition | an optional part not copied (return types, bases, parameters, decorators, namespace, trait, struct fields) | yes: fewer answers | counted, definition kept |
| Base-class and JVM qualified names, joined lists | allocation | yes | counted |
| Rust definitions and impl relations | allocation | yes, and an impl relation could be left half-built | relation dropped whole, counted |
| Module definition index, its entries and their growth | allocation | yes: definitions missing from the index | counted |
| Registry construction (per language and the project's) | allocation; a full label pool | yes | counted, registration all or nothing |
| Definition filtering per file | allocation; it falls back to every definition in the project | yes: the full set is not the filtered set, so answers can differ | counted, not treated as a safe fallback |
| Import maps and the values in them | allocation | yes: imports missing | counted |
| Qualified names, import and path-alias resolution | allocation | yes | counted |
| Language resolvers' output and internals | allocation | yes | counted at the memory core |
| Language resolvers' work budgets (TypeScript, Python, C, Rust) | the budget runs out | yes: evaluation stops for the rest of the file | counted once per file |
| Language resolvers' depth limits | the depth is exceeded | not as a failure: these bound recursion and end cycles, deterministically, as the reference's do | resolver semantics, not counted |
| Copying an answer into the result | any string of it not copied | yes, and it left answers with a missing caller or target | lost whole, counted |
| Copying a synthetic call into the result | any string or captured argument not copied | yes, and it kept carriers without their arguments | lost whole, counted |
| Pushing an answer or a call | the array cannot grow | yes | counted at the memory core |
| Deduplication table | allocation | yes: duplicates can change what a site resolves to | counted at the memory core |
| Python's ambiguity marking | allocation; it marks every function ambiguous | yes: callable references lost | counted |
| Perl's method table | allocation | yes: method calls stay unresolved | counted |
| C's negative-lookup memo; Python's type cache; TypeScript's evaluation memo | allocation | no: the full evaluation runs | not counted |
| Perl's child-node buffer | allocation | no: an indexed walk gives the same children | not counted |
| The pass reading a file from disk | allocation | not on this path: sources always come from memory | not counted |
| Per-file surface rows | allocation | no: nothing reads them; the interface no longer has them built | not built |
| The registry's resolution caches | allocation | not on this path: never opened here | not counted |
| The rustdoc reader; decoding stored surface rows | allocation (in the JSON library) | not on this path: nothing here calls them | not counted |

Two failures were also defects, fixed with the accounting: the registry could store a
label or a name as NULL when copying failed and dereference it later, and its suffix
search wrote through an unchecked allocation.

Tests: `abi_run_health_*` (engine) and the safe wrapper's `run_health` tests, which
prove that a project with nothing to resolve is clean, that a run made to skip a file
is degraded even when that leaves no answer at all, that a degraded run keeps the
answers it found, and, in `abi_run_health_answer_lost` and
`typed_resolution_internal_failure_degrades_run`, that an answer typed resolution found
and lost while copying it into the result degrades the run, with every other answer
kept: eight answers clean, six with the two lost, `pass_failures` 2. A file is made to
be skipped by the engine's own test switch, which marks it as the node budget would,
and an answer is lost through `PDX_ENGINE_TEST_FAIL_ANSWER_ON`, a test-only switch that
fails its copy as a failed allocation would.

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

Approved 2026-09-28: the policy stands as proposed. The review found one defect in
how it was applied: the exemptions were attached to the whole library target, and a
specific `-Wno-error` outranks a general `-Werror` wherever both appear, so they
reached the interface layer's sources too. They are now attached to the vendored
source files only. `engine/tests/warning_scope.py` (ctest `api_warnings_are_errors`)
compiles a probe for each exempt class with the exact flags an interface source gets,
from the build's compile commands, and requires an error naming the class; with the
flags a vendored source gets it requires the warning and no error. Passes under gcc
13 and clang 18; no interface source's compile command carries an exemption.

State: resolved.

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

Approved 2026-09-28 as implemented, in these terms: the surface contains the
file-local extraction facts typed resolution requires and no cross-file-derived
qualified state; unchanged files may re-parse but never re-extract; cross-file state
is recomputed in every project resolution; fresh, cached and reverse-order resolution
must remain equivalent. Section 4.5 of the specification mirror now says so.

A further test makes the second term executable: for every resolution fixture, each
file's surface is exported, the files are resolved as one project, and the surface
exported again after the project ends must be the same bytes. A surface cannot be
exported at all while a project holds the result.

State: resolved.

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

Review, 2026-09-28: approved, on one condition. The public header promised lexical
facts on calls but gave ordinary calls none, while the reference's calls pass reads
three facts about a call whenever typed resolution has no answer:

- whether it is a member call whose receiver nothing ties down (its weak-member
  guard, Python and the JavaScript and TypeScript family, and its Perl guard);
- whether a bare call's name is a parameter of an enclosing function (its
  local-binding guard, Python);
- whether a member call's receiver is rooted at self or cls (the member guard's one
  exemption, Python).

Each is now its own bit, `PDXE_LEX_UNRESOLVED_MEMBER`, `PDXE_LEX_LOCALLY_BOUND` and
`PDXE_LEX_SELF_ROOTED`, so no fact is folded into another; `lexical` widened to 16
bits while the header can still change. Four fixtures show both halves: the
reference's own textual edges (recorded in the goldens) and the facts on each site.

- `python_unresolved_member_call`, `ts_unresolved_member_call`: a member call on a
  parameter is not bound to the only same-named function in the project, while a
  bare call to it binds by unique name; only the member call carries the fact.
- `python_locally_bound_call`: a call to a parameter is not bound to the
  module-level function of that name; the unshadowed call is.
- `python_self_rooted_member_call`: a self-rooted receiver keeps its unique-name
  binding; a parameter receiver does not; a self-rooted call to a builtin type's
  method name does not either. The facts tell the three apart.

Removing the self-rooted fact from the interface makes the last fixture fail.

State: resolved.

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
