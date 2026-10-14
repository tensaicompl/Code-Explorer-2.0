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
| 115 | 2026-10-14 | P2-06 | blocker | resolved | Found in issue 114's A0 run: `scripts/a0/run.py` measures with `target/debug/pdx-bench`, which it never builds, and took the `engine_version` it records from `consts.rs`, the source. Nothing tied a result to the executable that produced it: with `ENGINE_VERSION` moved to 10 in the working tree and the executable built before, at 9, the run measured engine 9 and labelled every document engine 10. That run was discarded and measured again after a rebuild; no committed result came from it | Resolved 2026-10-14 (hotfix): `pdx-bench versions` prints, as JSON, the version the linked engine reports at run time, the `pdx_core` constants compiled in (`ENGINE_VERSION`, `EXTRACT_CACHE_FORMAT_VERSION`, `LANGUAGE_MATRIX_VERSION`, `SECRET_DETECTOR_VERSION`, `SEGMENT_SCHEMA_VERSION`) and the worker protocol. Before anything runs, `run.py` compares each with the source and refuses, naming both values and writing nothing, on any difference, a missing executable or one that cannot report. The executable's bytes are held for the run (checked before each dataset and the results), the results are staged and promoted together, and a document's `engine_version` is the executed engine's. Tests A1 to A5 in `scripts/a0/test_a0.py`, now part of `make test`; 14 mutations killed. No resolution, extraction, version or A0 rule (`a0-match-1`, `a0-metrics-1`) changes |
| 114 | 2026-10-14 | P2-06 | blocker | open | Found by issue 111: a Rust `use` has no lexical scope in PDX. The typed pass reads a file's top-level and inline-module `use` declarations as the whole file's, so a test module's `use crate::other::fetch;` makes the module's own call of its own `fetch` a typed answer to `other::fetch` (engine 8 with a plain `use`; engine 9 also with a list it now reads); neither pass reads a `use` in a function body, so 34 calls of PDX itself imported that way have no typed answer; and Stage 3 reads a file's top-level imports only, so an inline module's call is drawn `import-guided` to the module's import. The measured corpora hold no wrong trusted edge from it | Open. Give each `use` its lexical scope (module, inline module, block, function body) and bind its names only for the sites in it, in the typed pass and Stage 3 alike. Never by reading a function's or an inline module's imports as the file's: that broadening is what draws the wrong edge now. |
| 113 | 2026-10-14 | P2-06 | blocker | open | Found by issue 111, three causes: (A) a type path through `crate::` and a re-export names no definition: the typed pass keeps `crate::extract::NestedPath` as written, neither anchoring `crate::` nor following `pub use self::nested_path::NestedPath;`, so 5 `NestedPath::as_str` calls of the Rust golden have no typed answer; (B) an impl of an imported type names its methods' caller under the import (`http.header.HeaderMap.into_response_parts`) while extraction names the owner by the file's module, so 2 correct answers (`headers_mut`, `extensions_mut`) join no site and are drawn `exact`; (C) a module file named `index.rs` is named as its parent (`golden/index.rs`'s functions as `golden`'s), so a path through `golden::index` names no such function: 4 calls of PDX itself. All predate issue 111 (engine 8 with a plain `use` does the same) and draw no wrong edge | Open. (A) Anchor `crate::`, `self::` and `super::` in a type path and follow re-exports to the definition by identity, never a last-segment guess. (B) Name an impl's methods' caller as extraction names their owner. (C) Name a module file by its stem. Separate fixes. |
| 112 | 2026-10-14 | P2-16 | blocker | resolved | Found after issue 104 (R2): patch 0028 changed what Rust extraction records while `ENGINE_VERSION` stayed 7, and the extraction cache is keyed by the engine version, so an entry engine 7 wrote before the patch was served after it for the same source, path, language and policy. What differs: a bare call of a function its block declares is now `LOCALLY_BOUND`, and a Rust file's per-file typed answers, which its surface holds, need path evidence and leave a `match` arm's and a block's names alone. Shown with both builds: the same 5 object ids, 3 entries' bytes different, and the post-patch build, served the pre-patch entries, drawing `a::expand` to the crate root's `expand` and the arm's `params` as a typed reference | Resolved 2026-10-14 (hotfix): `ENGINE_VERSION` 7 to 8, in `consts.rs` and the engine (`pdxe_version`). Engine 7 began with patch 0015; 0028 is engine 8. Every other version is unchanged. `cache_entries_of_engine_seven_are_never_served`: a faithful engine 7 entry is a miss, the file is extracted again with engine 8's facts, the old entry is left alone, an engine 8 entry is written and served warm; with the version back at 7 it fails. Goldens, A0 and the differential are unchanged but for the version |
| 111 | 2026-10-14 | P2-06 | blocker | resolved | Found by issue 104 (R2): Rust typed resolution reads a file's `use` declarations as text and splits a brace list at every comma, so a nested or multi-line list (`use crate::{routing::{on, MethodFilter}, Router};`) gives some names no entry and others a wrong path (`on` as `crate::on`), and `pub use` none. Since issue 104 an imported name is a typed answer only through its import's path, so in the Rust golden 16 correct typed edges become `exact` (same targets), 6 are lost to `candidate` (`on`, `on_service`) and 9 unmaterialised typed answers are lost; PDX itself loses 38 unmaterialised | Resolved 2026-10-14 (patch 0029, engine 9): `use` declarations are read from the syntax tree, nested and multi-line lists, comments, aliases, `self`, globs and `pub`, `pub(crate)` and `pub(in …)` included; a name the file's own use trees bind to two paths is withheld; lookups by local name through an index; an alias reaches its function (`lsp_import_alias`). All 31 Rust golden sites are correct `typed` again (16 from `exact`, 6 from `candidate`, 9 unmaterialised), and one `scoped` false positive is `typed` and right. A0: Rust golden `typed` 843/844 → 859/860 = 99.88%, union 62.92% → 62.97%, recall 1,407/4,052 → 1,409/4,056; PDX itself `typed` 5,904/5,904, 43 sites strengthened and none lost; Python unchanged; no new wrong trusted edge. Not restored: 5 `NestedPath::as_str` and 2 impl sites (issue 113), 34 PDX calls through a function body's `use` (issue 114). The engine differential's one changed row (`pub(crate) use unknown::Unknown;`, which the reference reads as a path) is ratified by decision 66. `ENGINE_VERSION` 8 → 9; `cache_entries_of_engine_eight_are_never_served` |
| 110 | 2026-10-14 | P2-04 | blocker | open | Found by issue 104 (R2): an item a `macro_rules!` invocation writes is no definition. `axum-core/src/body.rs` writes `impl From<&'static str> for Body` (and others) through `body_from_impl!`, so the only `Body::from` the registry has is the handwritten `From<()>` one, and `Body::from("…")` (`axum-core/src/ext_traits/request.rs:355`) is drawn `import-guided` to it: the Rust golden's one `import-guided` false positive | Open. Decide whether item-position macro invocations are expanded for definitions (the typed pass already expands `macro_rules!` bodies for calls), or the definitions a macro may write make a name ambiguous; never block `from` by name. |
| 109 | 2026-10-14 | P2-06 | blocker | open | Found by issue 104 (R2): Rust typed resolution chooses a trait impl for `Type::method` without its bounds. `WithRejection::<TestExtractor, TestRejection>::from_request` (`axum-extra/src/extract/with_rejection.rs:196`) reaches the blanket `impl<S, T> FromRequest<S, ViaParts> for T` (its type implements only `FromRequestParts`), but is answered `typed` (`lsp_ufcs`) with `WithRejection`'s own `FromRequest` impl, whose bound `E: FromRequest<S>` it does not meet: the Rust golden's one remaining typed false positive | Open. Find whether the engine can withhold a `Type::method` answer when a blanket impl of the same trait may supply the method, without scanning the registry per call (issue 92), or check an impl's bounds; never special-case the trait. |
| 108 | 2026-10-14 | P2-06 | blocker | open | Child of issue 95 (decision 65, R7): target identity among definitions that share a qualified name. In the Rust golden, 89 oracle relations target such a definition; 83 have a structural site, 52 are drawn, 49 to the right definition and 3 to a wrong one, and 2 typed answers are withheld because their qualified name maps to several definitions. A0 cannot adjudicate Python's `@overload` stubs (28 calls without a verdict) | Open. Investigate whether the extraction's normalised signature, the definition's disambiguator, the call's argument count and types, or the engine's occurrence identity can select the definition a call reaches. Overloads are never collapsed because their qualified names are equal |
| 107 | 2026-10-14 | P2-04 | blocker | open | Child of issue 95 (decision 65, R4 and R8): Rust call sites the oracle sees and Stage 3 does not. 107A (R4): 56 oracle relations at ordinary Rust syntax with no structural site: 54 turbofish calls, 30 of a path (`assert_from_request::<_, ()>()`) and 24 of a method (`parts.extract_with_state::<S, T>(&state)`), and 2 member calls in a chain with a line comment before the dot (`examples/error-handling/src/main.rs`): a grammar or extraction gap. 107B (R8): 338 oracle relations written inside macro invocations' token trees with no site, out of 1,696 call texts there, of which 90 are already extraction sites (all drawn correctly) and 9 are typed sites Stage 4 cannot persist | Open. 107A: find why these calls are no extraction sites and make them ordinary call sites. 107B: classify token-tree calls (known standard and assertion macros, expression-position arguments, format-like macros, arbitrary macro DSLs, `macro_rules!` bodies) and define a semantics-safe policy before recovering any; no call is invented from DSL tokens, and macros are not expanded |
| 106 | 2026-10-14 | P2-06 | blocker | open | Child of issue 95 (decision 65, R6): Python recall. 470 of the Python golden's 1,114 oracle relations end as `candidate` (36 more are blocked and 5 drawn wrong), most of them member calls on a local value (`bp.route(…)` on a `Blueprint`) or package re-exports (`flask.make_response(…)`) | Open. Classify all 470 first, by the evidence that could resolve them: local-variable receiver, parameter receiver, factory or call-result receiver, attribute receiver, package re-export, import alias, inheritance or base method, decorator-produced binding, fixture or other untyped external value, other; and separate what is structurally recoverable from what needs a compiler or precise provider and from where the oracle itself is weak or untyped. Then design evidence fixes from the counts. 85% structural recall is not promised from information the structure does not hold |
| 105 | 2026-10-14 | P2-06 | blocker | open | Child of issue 95 (decision 65, R5), probably the largest recall workstream: 2,079 of the Rust golden's 4,061 oracle relations at structural call sites end as `candidate` (389 more are blocked, 190 drawn wrong and 17 external) | Open. Classify all 2,079 before any fix, by the evidence that could resolve them, with counts: receiver local-variable type, receiver field type, receiver call-result type, associated or static path type, import or `use` binding, trait bound, impl relation, generic type parameter, method through `Deref`, method of an external type, `crate`/`self`/`super` path, re-export, macro-generated type or item, other. Mark what no structural evidence can resolve, so the driver can judge what G4's structural recall floor assumes. Then design evidence-strengthening work from the counts; no fix is guessed before the classification |
| 104 | 2026-10-14 | P2-06 | blocker | open | Child of issue 95 (decision 65, R2): the Rust golden's 15 typed false positives (16 since issue 103 draws `Handler::call`), each a typed answer naming the wrong definition: the module path ignored (`from_request::expand`, `debug_handler::expand`, `typed_path::expand` to `lib.rs`'s `expand`, 5); an external crate's function taken for an internal look-alike (`multer::parse_boundary`, `hyper::upgrade::on`, 4); another type's associated function (`PathDeserializationError::custom` to `connect_info`'s `custom`, 2); a same-named function of another crate (`find_first_not_double` in `axum-macros`, 2); a blanket trait impl (`WithRejection::<…>::from_request`, 1); and a pattern binding answered as a call (`Ok(Self(params))`, 1). Also the one import-guided false positive (`Body::from` to another `From` impl's `from`) and the one false typed answer the blocklist now hides (`Handler::call`, `axum/src/handler/service.rs:171`) | Open. R2 (2026-10-14, patch 0028; 4.5 amended): a Rust typed answer needs path evidence, never a name's uniqueness alone; a `match` arm's bindings and a block's functions are locals; a Rust path starting with an extern-prelude crate is `external`. 15 of the 17 are no longer drawn wrong; the Rust golden's `typed` is 843 / 844 = 99.88% (from 852 / 868), PDX's 5,870 / 5,870, Python unchanged. Left: the blanket impl (issue 109) and the macro-written impl (issue 110); the 6 typed edges lost to `candidate` are issue 111 |
| 103 | 2026-10-14 | P2-06 | scr | resolved | Child of issue 95 (decision 65, R1): the generic-name blocklist settles a site before a valid typed answer is considered (4.5 Stage 3, "`blocked`, even with a qualifying typed answer", and "Names on the blocklist are `blocked` before any stage runs"), so stronger semantic evidence is discarded for names such as `get`, `run`, `parse` or `update`. A0 measured the typed answers it suppresses: 4 in the Python golden (all correct), 32 in the Rust golden (31 correct, 1 false) and 412 in PDX (409 correct); accepting them (A0.2's C4) recovers 3, 31 and 409 relations | Resolved 2026-10-14 (R1). A qualifying unique typed answer (`from_engine` says typed and its target is exactly one definition of the repository: Stage 3's existing `typed_target`) is `typed` first, whatever the callee's name; otherwise a blocklisted name is `blocked`, before the engine's external answer and every name-based stage. An engine hint, a low-score or multi-candidate `lsp_typed` answer, an out-of-range score and an ambiguous mapping never pass the blocklist; a typed-only site, a reference or an engine site without a qualifying answer stays unconfirmed. No other precedence changed. 4.5 Stage 3 amended (both statements, steps renumbered). Tests in `crates/pdx-core/tests/stages.rs`; four mutations killed. A0 rerun (same pins, tools and rules): typed answers suppressed by the blocklist 4, 32 and 412 → 0; Python golden union 624/657 = 94.98%, recall 54.40%; Rust golden typed 852/868 = 98.16%, union 61.39%, recall 34.67%. The one false typed answer it exposes (`Handler::call`) is issue 104's. Nine goldens gain 303 typed tuples, none removed. No version change |
| 102 | 2026-10-14 | P2-06 | scr | open | Child of issue 95 (decision 65, R3): 4.2.2 draws `exact` (the callee's short name has one definition in the repository) and `scoped` (one definition in the same module or package), but A0 measured them at 284/310 = 91.61% and 1/8 = 12.50% (Python golden), 509/1,168 = 43.58% and 155/492 = 31.50% (Rust golden), and 37.7% and 13.2% (PDX itself), against G4's 95% union floor: uniqueness of a short name does not establish which definition a call reaches | Open; approved by decision 65, implementation R3. Specification change to 4.2.2: `exact` and `scoped` remain ordered bands, numbered and named as now, but under their current semantics neither can by itself produce a drawn edge; a stronger variant may be drawn only once the harness (4.13) measures it at G4's floor. Implementation keeps their evidence for candidate ranking, explanation, coverage, trust diagnostics and calibration, keeps candidate ordering deterministic, avoids a schema or version change unless one is truly required, updates the affected tests and goldens deliberately, amends 4.2.2's table, and reruns A0. A correctness change, not a threshold |
| 101 | 2026-10-13 | P0-02 | ambiguity | open | Raised by an independent review before the oracle work: the provenance records disagree about the engine reference's part in the Rust resolver. `bench/references.lock` gives the engine reference (`engine-ref`) the mode "vendor+refactor, and reference for the Rust ports" and lands it in `engine/` and `crates/pdx-core/src/resolve/`, while `THIRD_PARTY_NOTICES.md`'s section of algorithms and designs consulted ("Re-implementations written from scratch in this repository, with the reference whose algorithm or design was consulted") says "No entries yet" | Open. To be settled from the actual derivation history of `crates/pdx-core/src/resolve/`, never by removing an attribution to make the records agree. No legal conclusion is drawn here |
| 100 | 2026-10-13 | P0-04 | third-party | open | Raised by an independent review: supply-chain hardening has gaps. The cargo-vet store holds 142 exemptions and 0 audits (`supply-chain/config.toml`, `supply-chain/audits.toml`): every crate is accepted by exemption, none by a recorded audit, and `make vet` passing means no more than that. `cargo deny` runs its licence check alone (`scripts/licence-scan.sh`: `check licenses`); the advisory policy `deny.toml` configures (`[advisories]`) is not run as a check by any workflow. Every workflow step uses a mutable major tag (`actions/checkout@v4`, `actions/cache@v4`, `actions/setup-node@v4`, `actions/upload-artifact@v4`) rather than an immutable commit | Open; not fixed in this round |
| 99 | 2026-10-13 | P2-11 | blocker | open | Raised by an independent review: the local output's lifecycle can block the next run. `pdx index` refuses a tree `git status` does not report clean, `.pdx/` included (decision 38); it writes its segment to `.pdx/segment.db` by default and never overwrites a segment; and it does not make `.pdx/` ignored. In a repository whose ignore files do not already cover `.pdx/`, one successful index leaves the tree dirty, and the next run is refused | Open; not fixed in this round |
| 98 | 2026-10-13 | P2-03 | scr | open | Raised by an independent review: discovery excludes every directory named `build`, `target`, `dist` or `vendor` at any depth (`index::discover::HARD_EXCLUDES`, as 4.5 specifies; `vendor` unless `[discover] include_vendor`), whatever is under it. Legitimate source below such a component is not discovered, not indexed and not represented in coverage: no coverage row records that it was left out | Open. The rule is 4.5's, so a change is a specification change; not fixed in this round |
| 97 | 2026-10-13 | P2-08 | blocker | open | Raised by an independent review: the SQL consumer reader (`contracts::tables::sql_access`) reads a call's string argument as SQL when its first word is `SELECT`, `INSERT`, `UPDATE`, `DELETE`, `WITH` or `MERGE` in any case, so natural-language text can mint a false table contract. `t("Select an item from the list")` begins with `Select`, its `from the` reads as a `FROM` clause, and the call consumes `Table("the")` | Open; not fixed in this round. Reproduced by A0 (2026-10-14); decision 65 makes it remediation R10, an early trust fix after A0.3, not release-time work |
| 96 | 2026-10-13 | P2-04 | blocker | open | Raised by an independent review: content secret normalisation (detector 4) has credible false negatives and one destructive mask. A standalone JWT-shaped credential and a standalone GitHub personal-access-token-shaped credential match no detector (the detectors are a PEM private key, a bearer token, a credential assignment, a URI password and a cloud access-key id); a password in a semicolon-separated key-value connection string is not masked; and the credential-assignment detector masks across `"password=" + decrypt(pw) + "..."`, destroying code that is not a secret | Open; not fixed in this round. A fix must prove secrets masked, non-secret executable syntax preserved, byte length preserved and CR and LF preserved, with fixtures whose synthetic values are built at run time; a change to what a detector matches bumps `SECRET_DETECTOR_VERSION`. Reproduced by A0 (2026-10-14); decision 65 makes it remediation R9, an early trust fix after A0.3, not release-time work |
| 95 | 2026-10-13 | P2-06 | blocker | open | Structural accuracy and trust. An independent adversarial review raised low precision and recall of the drawn structural bands before P5 and P6's oracle work; its percentages are not accepted as authoritative. The driver's inspection of `1eb801d` confirmed six mechanisms: the generic-name blocklist settles a site before a valid typed answer is accepted; `exact` draws from repository-wide short-name uniqueness; `scoped` from module or package short-name uniqueness; a Rust call on an unknown receiver has none of the unresolved-member protection Python, JavaScript, TypeScript and Perl have, so it falls through to `exact` and `scoped`; overloads sharing a qualified name cannot be told apart by signature; and a call written only inside a Rust macro's token tree is not a parsed `call_expression` site | Open. A0.1's measurement and A0.2's counterfactuals (decision 64, gate G1b) are required before any architectural change; what follows is A0.3, the driver's decision. A0.1 and A0.2 done 2026-10-14 (`bench/results/a0/`), against `scip-python` 0.6.6 and `rust-analyzer` 1.98.1: Python golden `typed` 310/310, union 620/653 = 94.95%, recall 603/1,114 = 54.13%; Rust golden `typed` 821/836 = 98.21%, union 1,580/2,592 = 60.96%, recall 1,377/4,061 = 33.91%. Four of G1b's six floors are not met. A0.3 (decision 65, 2026-10-14): `typed`, `import-guided` and `inheritance-guided` stay drawn; `exact` and `scoped` short-name uniqueness is not evidence to draw (issue 102, specification change); a valid typed answer precedes the blocklist (issue 103, specification change); recall is a first-class blocker. Split into issues 102 to 108, remediation order R1 to R10 (103, 104, 102, 107A, 105, 106, 108, 107B, 96, 97), each authorised separately; stays open as the umbrella until G1b's floors hold. R1 done 2026-10-14 (issue 103 resolved): Python union 94.98%, recall 54.40%; Rust typed 98.16%, union 61.39%, recall 34.67% |
| 94 | 2026-10-13 | P2-16 | blocker | resolved | Found by issue 93's battery: Python's per-file resolver replays each import statement by scanning every import of the file for its local name (`py_replay_import_local`), a lookup by key that issue 92's indexes do not reach, so a file's typed resolution is quadratic in its imports: a file of `from m import a` lines takes 0.14, 0.47, 1.8 and 6.7 s of per-file typed resolution at 0.25 to 2 MiB, and the project's run as long again (16 s at 2 MiB in all). Navigation stays linear. As long at `5915669` | Fixed by indexing, nothing limited (patch 0027, decision 63, engine 7): replay, an imported reference's exact candidate and a call's alias check read the file's registered imports through one index by exact local name (`imports_by_local`, issue 92's `PDXENameIndex`), each name's chain in registration order, so the import each statement takes, the conflicts and the imports consumed are the scans'; an index that cannot get memory finds nothing, never scans. The 2 MiB file's per-file typed resolution now takes 0.15 s (6.7 s) and the project's run 0.31 s (7.7 s); the imports examined double with the file at 1,000 to 16,000 imports; the 104,730-file census resolves and extracts identically; four mutations killed |
| 93 | 2026-10-13 | P2-16 | blocker | open | Found by issue 92's resource audit, the final engine-7 audit: when a parse fails early, the parser's recovery can leave one error node holding the rest of the file as a flat run of children, and every pass that steps through it by position, or asks for a sibling or parent inside it, costs the position: the parser's own recovery, extraction's docstring, decorator and ancestor lookups, channel extraction and the Kotlin resolver's top-level walks. A Kotlin file of one-line nested classes takes 195 s at 1 MiB and 764 s at 2 MiB; JavaScript after an unclosed parenthesis 40 s and 160 s. As long at `8b3ed42` | Extraction fixed (patch 0026, engine 7), nothing limited: children are visited by cursor through one iterator, in reverse or by position from a collection made once, and parents and siblings are found from the walk's own path or the loop in progress, never by descending from the root; the answers are tree-sitter's on every node of the 104,730-file census and it extracts and resolves identically; the navigation work is linear in 75 wide shapes in 32 languages; nine mutations killed. The Kotlin file now extracts in 1.75 s at 2 MiB (764 s), 8,000 decorated TypeScript methods in one class in 0.55 s (68 s). Open: the vendored parser's recovery is quadratic in an error node's width (`ts_parser__recover` rebuilds the node for each token it takes): the JavaScript file parses in 1.5, 5.8, 22.9 and 88.7 s at 0.25 to 2 MiB, the rest of its extraction in 1.0 s. Changing the parser is the driver's decision; the battery also found issue 94 |
| 92 | 2026-10-12 | P2-16 | blocker | resolved | Found by issue 90's resource audit: per-file typed resolution and the interface's translation are quadratic in how many definitions a file holds, in every typed language, whatever its nesting. 16,000 one-method classes take 62 s in Python (677 KB), 18 s in Java (549 KB), 15 s in TypeScript, 13 s in C++ and 8 s in C# and Go, about four times as long for each doubling; 2 MiB of distinct nested Java classes take 237 s. Within the depth ceiling, JavaScript calls nested 200 deep cost the TypeScript resolver more the more of them a file holds: 2 MiB, 441 s. Past an isolated worker's 120 s timeout a file fails the build, and whether it gets there depends on the machine. Generated code (protocol buffers, models, clients) holds thousands of classes a file | Fixed by indexing, nothing limited (patch 0025, decision 62, engine 7, and Stage 3's registry): every registry keeps exact-key chains in registration order as entries are added, the resolvers' helpers, the cross-file pass and the interface's translation index what they scanned (`index_of_qn` is gone), Stage 3 reads a name's provenance by file and name and C++ nesting by type name; an index that cannot get memory finds nothing, never scans. 16,000 classes now take 0.50 to 0.87 s in every typed language (Python 63.9 s before), each doubling about twice the last; the 2 MiB nested JavaScript 21 s (406 s) and nested Java 5.8 s (401 s). The 104,730-file census resolves and extracts identically; ten mutations killed. The final audit found issue 93 |
| 91 | 2026-10-12 | P2-16 | third-party | resolved | Found by issue 90's resource audit: grammar scanners of the vendored parser keep state that grows with nesting and do not keep it within the parser's 1,024-byte state buffer. YAML's writes two bytes past it from 253 nested block mappings (a 41 KB file) and Markdown's copies its open blocks past it without a bound, so the parser's own state is overwritten: a release build, where the bound is only an assertion, crashes in the parser (a null dereference), and the address sanitizer stops at the assertion. Kotlin's calls `abort()` at 1,024 open string templates (about 6 KB). Python's can write one byte past it, not reproduced. All of it happens during the parse, before any extraction pass and before the depth ceiling. Extraction is in process unless isolation is asked for, so one such file stops `pdx index`. The same at `582b58a` and on main | Resolved. Patch 0023, decision 60, engine 7: each affected scanner holds what the buffer holds, the bound derived from it (YAML 253 indentation entries, Markdown 254 open blocks, Kotlin 1,024 delimiters, Python 2 + delimiters + 2 per level at most 1,024); a push past it is refused and its token not claimed, so the parse recovers; serializers check each complete entry, deserializers read complete entries within the length. No abort remains reachable from content. All 23 scanners audited, the other 19 already bounded. Direct canary, sweep, round-trip and malformed-length tests, end-to-end children, corpus files; release and sanitizers clean; mutations killed; the census extracts identically |
| 90 | 2026-10-12 | P2-16 | blocker | resolved | Found by the closure audit of issues 88 and 89, with both in force: within issue 89's 512-depth admission, usage handling still climbs to the root with `ts_node_parent`, which descends from the root again, for every usage, so each usage costs depth², and a file holds as many deep constructs as its size allows. One admitted 400-link Python member chain (about 800 bytes) takes 3.1 s, 8 of them 25 s, a 13 KB file about 50 s; Java classes 250 deep take 0.85 s each. A call's argument text, proportional to its depth, makes 2 MiB of admitted 250-deep call chains peak at 1.33 GB in the engine | Resolved. 90A, patch 0022, decision 59: a usage's questions about its ancestors are answered from the unified walk's own frames, each frame's answer computed once. The 400-link chain takes 2 ms (3.71 s before), 16 of them 31 ms (54.9 s), Java classes 250 deep 15 ms (0.93 s); the counted work is linear in links and chains, with no `ts_node_parent`; all 104,730 census files extract identically; mutations killed. 90B, patch 0024, decision 61 (the driver's redesign of the interface, model, cache and protocol): text a fact takes from the file is a range of the source (`pdxe_text`, `TextRef`), owned only when synthesised or macro-expanded; qualified names are structural (member, extended, or a `.`-bounded prefix of an earlier name); records hold ranges and bind to the verified source (cache format 6, protocol 7, surface 5); identities hash the same bytes, segment schema 3. An audit of every extracted string over 108 shapes in 29 languages found five fields growing with the square of the nesting (argument expressions, callees, usage names, qualified names, parameter lists); none does now. At 2 MiB the record without its surface shrinks 6 to 26 times (Python chains 443 to 49 MB) and peak RSS by 3 to 45%; all 104,730 census files extract identically; mutations killed. Closure of issues 86, 87 and 89 stays held for the final engine-7 audit; see issues 91 and 92 |
| 89 | 2026-10-12 | P2-16 | blocker | open | Found by issue 88's extraction audit: extraction's memory and time are superlinear in syntax nesting, in many places and every language, beyond `ast_path`. Memory: each call stores its argument text and its callee text whole, so nested calls store about 1.5 n² bytes of arguments and a method chain about 2 n² bytes of callee text; nested C++ namespaces, JavaScript closures and Java `new` expressions grow ×3 to ×4 per doubling. Time: usage handling climbs to the root with `ts_node_parent`, itself a descent from the root, for every usage: O(n³). 1,000 nested Java classes (13 KB) take 56 s, 1,000 nested JavaScript functions 45 s and a 1,000-link Python member chain 50 s. Whether such a file extracts, or finishes within a worker's timeout, depends on the machine | Fixed in part, closure held. Patch 0021, decision 58, engine 7: after the parse and before any semantic pass, a cursor scan (every node, the root at depth 1) refuses a tree deeper than 512: no fact, no semantic pass, lost work once, no cross-file resolution, never cached, degraded; the expanded C and C++ tree alike, its pass alone skipped. From a census of 104,730 files (p99.9 65): the 13 past 512 are parser recovery, generated Go or expanded text. The issue's 1,000-deep reproducers take under 0.01 s; mutations killed. Held on issue 90: within the admitted depth, a file of many deep constructs still costs depth² a usage. Issue 90 is resolved; the final engine-7 audit (issue 92's) found issue 93, so closure is still held |
| 88 | 2026-10-12 | P2-16 | blocker | resolved | Found by issue 87's security test: extraction's memory is quadratic in how deeply call sites nest, in every language. Each call site stores the node types from its owner down to it (`ast_path`, patch 0010), so n calls nested n deep store about n² entries: a 30 KB C or Python file of 10,000 nested calls stores 100,030,000, peaking at 954 MB in the engine and 6.7 GB in `build_segment`, which fails the file under an 8 GB address-space limit and is killed under a 6 GB cap; a 10,000-call method chain takes 1.6 GB and 39 s. Whether such a file extracts depends on the machine | Resolved in engine 7 (decision 57, patch 0020): a site's node-type path is complete or absent, never truncated; one per-file budget shared by every producer holds a path to 1,024 entries and the file to 500,000, checked before allocating; the first refusal is lost work once and no path is computed after it; a site without its path is counted, never stored. Of 104,730 census files only `lib/test_bpf.c` (parser recovery) passed it, and issue 89 now refuses it first; surfaces byte-identical otherwise. Mutations killed |
| 87 | 2026-10-12 | P2-16 | blocker | open | Found by issue 86's audit: C and C++ preprocessing's work and memory are unbounded by the source. A forward `#line` within one file pads the expanded text with one line per line skipped (`#line 400000000` in a three-line source: 400 MB of text, 2.7 GB peak), and doubling macros grow exponentially (437 bytes, 22 levels: 1.3 GB, 9.8 s). Whether such a file extracts depends on the host's memory and speed, and a repository can exhaust the indexer's memory | Fixed on the branch, closure held. Patch 0019, decision 56, engine 7 with issues 83 to 86: a forward jump of more than 4,096 lines is one `#line` marker; what one file's preprocessing writes and generates, and its `#if` folding, are held to `clamp(24 × len, 256 KiB, 8 MiB)` (16 until the driver's review of five Linux files admitted `stackinit_kunit.c`), generated tokens and expansions to 500,000 each and nesting to 256, counted inside the work; passing one discards the expansion and is lost work once, never cached, degraded. `#if` arithmetic is 64-bit two's complement written out, literals read explicitly. The closest normal corpus file uses 10.9% of a ceiling; 4 of Linux's 64,432 pass one, all amplification by design. Mutations killed but one equivalent. Held: its security test found issue 88 (resolved), and the closure audit issues 89 and 90 (90 resolved); the final engine-7 audit (issue 92's) found issue 93, so closure is still held |
| 86 | 2026-10-12 | P2-16 | blocker | open | Found by issue 85's audit: two more host data-model dependences in C and C++ preprocessing. A single-byte character constant in `#if` is evaluated as plain `char`, signed on four of the five specified targets and unsigned on linux-aarch64, so `#if '\xff' < 0` (valid C) takes opposite branches; and `#line` numbers are read with `atol`, a C `long`, so a number above 2147483647 (invalid C) gives a different `__LINE__` on Windows | Fixed on the branch, closure held. Patch 0018, decision 55, engine 7 with issues 83 to 85: a one-byte character constant is a signed 8-bit char, written out, and a multi-character one a 32-bit int; every `#line` number is its leading decimal digits read at 64 bits and clamped to 4294967295. Tests build the library with char signed and unsigned and assert the same values on Linux, macOS and Windows; mutations killed. Held because the same audit found issue 87, and the driver set its closure condition as no known host property changing extraction. Issue 87 is fixed (patch 0019) and issue 88 resolved (patch 0020); closure is held on issues 89 and 90 (90 resolved) and, after the final engine-7 audit (issue 92's), on issue 93 |
| 85 | 2026-10-12 | P2-16 | blocker | resolved | Found by issue 84's audit: C and C++ preprocessing evaluates `sizeof(type)` inside `#if` with the type sizes of the engine binary's own build (`long` is 8 bytes on Linux and macOS, 4 on Windows), so such a source can extract differently on different platforms under one `ENGINE_VERSION`. No ambient input (one binary, one answer) and inert in practice: no C or C++ compiler accepts the construct, and jq, fmt and the Linux checkout hold none | Resolved in engine 7 with issues 83 and 84 (decision 54, patch 0017): the library's twenty recognised spellings use one written-out LP64 table on every platform (char 1, short 2, int 4, long 8, long long 8, float 4, double 8, long double 16, pointers 8); a caller's table and the build's own sizes are ignored, unrecognised spellings stay unevaluated. Host ABI is no extraction input and not in the cache key. Tests simulate other platforms through a test-build seam and assert the same sizes on Linux, macOS and Windows; mutations killed. The audit found issue 86 |
| 84 | 2026-10-12 | P2-16 | blocker | resolved | Found by issue 83's environment audit: C-family preprocessing predefined `__DATE__` and `__TIME__` from the wall clock in the process's local time zone (`localtime`, which reads `TZ` and, without it, the system's zone file) and with the locale's month name. A source macro that expanded either put the date or time into the extraction (a call's arguments and first string argument), which no cache key names | Resolved in engine 7 with issue 83 (decision 53, patch 0016): `__DATE__` is `"Jan 01 1970"` and `__TIME__` `"00:00:00"`, written out, so no clock, zone or locale is read; a source's own definitions keep precedence. The only time-derived predefined macros; `ENGINE_VERSION` stays 7. Tests fail on the clock-reading engine and pass now; mutations killed |
| 83 | 2026-10-12 | P2-16 | blocker | resolved | Found by P2-16's engine differential: the vendored engine's C, C++ and CUDA preprocessing reads the files a source includes from disk, relative to the process working directory (the preprocessor's file loading in `engine/src/preprocessor.cpp`), so a C-family extraction is not a function of the bytes Stage 2 gives the engine. Through `pdxe_extract_file`, the interface Stage 2 uses, the sites jq's `src/jv_alloc.c` has in its macro-expanded text are at lines 133 and 134 with the workspace root as working directory and at 397 and 398 in the repository's root, where `src/jv.h` is found and expanded. The reference engine does the same, so the differential runs both in the corpus root, where they agree | Resolved (decisions 51 and 52). Production engine: patch 0015, engine 7, opens no file, so no ambient include is consumed; specification 4.5 clarified; tests fail on engine 6 and pass on 7, mutations killed, goldens unchanged. P2-16 harness: both engines execute from a fresh empty working directory of the harness's own, an execution rule of the differential, not the fix; `make engine-differential` is 32 of 32 identical, jq and fmt included |
| 82 | 2026-10-11 | P2-14 | blocker | open | P2-14's performance run cannot complete either scale repository on the measuring machine (a 10 GB VM, 28 logical processors). Stage 2 keeps every file's extraction resident until the stage ends, at about 300 to 360 KB of resident memory per extraction completed, so a worker passes the harness's 6 GiB ceiling during extraction: Kubernetes (`6c1c770`, 25,930 files discovered) after 9.6 s with 21,211 extractions cached, Linux (`72d3fcf`, 95,723 files) after 14.2 s with 17,625. Not a defect of the harness: the goldens' peaks are 50 to 1,262 MiB. Blocks G1's scale criterion (peak RSS ≤ 4 GiB and ≥ 400 files/s on `kubernetes`), not P2-14 | Open. Under Decision 49's P2-14 measurement policy, P2-14 records each as `out_of_memory` data (peak, stage reached, stages finished, extractions cached) and its summary marks scale incomplete; nothing is optimised in P2-14. Needs Stage 2 and the registry to hold less per file (or stream into Stage 3) before G1 |
| 81 | 2026-10-11 | P2-14 | third-party | resolved | P2-14's deliverables need a benchmark framework for the `resolve` and `segment write` benchmarks and a way to read a benchmark worker's resident memory and the host's processors and memory on Linux, macOS and Windows; neither was in the dependency graph | `criterion` 0.8.2 (a dev-dependency of `pdx-core` only, default features off but `cargo_bench_support`) with 13 crates of its own, `safe-to-run`; `sysinfo` 0.39.6 (a dependency of `pdx-bench` only, default features off but `system`) with 16 crates of its own, `safe-to-deploy`: 31 exact-version exemptions added by hand, each naming this issue, and the existing `syn` 2.0.119 exemption raised from `safe-to-run` to `safe-to-deploy`, because `sysinfo`'s Windows dependency builds two procedural macros with it. Exemptions, not audits: 142 exemptions, 0 audits, 0 imports. `pdx` links none of them (`scripts/open-binary-check.sh`) |
| 80 | 2026-10-11 | P2-13 | ambiguity | open | Found closing issue 76: error recovery in C++ makes callables of what is not one, and they own the sites they span. Examples in fmt are a `Function` named `namespace` over lines 928–1755 of `include/fmt/core.h`, and a `Method` `GoogleTestFailureException` over lines 10986–11713 of gtest's header. Overloads and templates inside such regions, or in class bodies, get no definition: `basic_specs::set_fill(basic_string_view<Char>)`, `count<B1, B2, Tail...>`, `EqHelper::Compare` behind `enable_if`. Under engine 5 the name join gave their sites to another definition with the same name. Since issue 76 they belong to the representation as it is: the enclosing artefact, or the file | Open: an extraction-quality question for C and C++, not ownership's |
| 79 | 2026-10-11 | P2-13 | ambiguity | open | Found closing issue 76: the unified walk keeps a function's frame open past its node. Expiry waits for the next node that is not trivia, and an error-recovered region the parser marks as an extra is walked at a greater depth, so it never comes. Lexical facts in the region (the callable a call is found in, which typed resolution joins on) name the earlier function. In jq, 8 sites in `src/jq_test.c` inside `run_jq_pthread_tests`, which has no definition, are found in `run_jq_exhaust_and_reuse_tests`. Since issue 76 graph ownership is guarded: no site is owned by a callable whose declaration does not hold it, so these are the file's | Open: an engine change to lexical facts, with typed resolution's answers to review |
| 78 | 2026-10-11 | P2-13 | ambiguity | open | Found closing issue 76: the Kotlin resolver positions its synthetic property-delegate `getValue` calls at the wrong bytes. The span has the property name's length but starts before it, crossing nodes. All 35 such sites in the Kotlin golden are affected, so their positions, paths and site ids are wrong. Their owner is right since issue 76: the late-site search starts at the node the bytes fall in | Open: an engine change to the Kotlin resolver's site spans |
| 77 | 2026-10-11 | P2-13 | ambiguity | open | Found closing issue 76: in C, C++ and CUDA, a callable only the macro-expanded text represents, with no raw function node that can stand for it, leaves its raw sites to the file. The preprocessor maps lines, not bytes, and decision 48 aliases a raw node only on an exact, unique line match with the name on the first line. jq has 149 stored sites, in split-brace functions whose raw parse has no function node: `strptime` 89, `bind_bytecoded_builtins` 34, `gen_last_1` 26. fmt has 4, where the line match is not unique: gmock's `NaggyMock` 2 and gtest's `Matcher` specialisations 2. They were file-owned under engine 5 too | Open: needs a byte or token map from the preprocessor, or the adoption recording the raw region it rescued |
| 76 | 2026-10-11 | P2-13 | scr | resolved | Found closing issue 75: a represented callable whose unified-walk qualified name differs from its definition's still loses ownership of its sites. The walk names a frame by `compute_func_qn`, the definition extractor by its own rules; when they disagree, no definition carries the frame's name and the call's caller is the file. Examples: Rust trait methods on a generic type (`impl<F> Debug for WebSocketUpgrade<F>`, whose definition drops `<F>`), a Rust test whose name carries its `cfg` attribute, minified JavaScript, Ada specification files, C and C++ template and macro forms. Under engine 5, the call sites inside a represented callable's span with no enclosing node number: ada 475, c 190, cpp 9,200, kotlin repository 7,081 (6,651 in its documentation JavaScript), rust 1,756, typescript 6; csharp, go, java and python 0. The same sites had no owner under engine 4 | Resolved by decision 48 (patch 0014). Graph ownership no longer depends on qualified-name equality: the exact private declaration occurrence (source buffer and the byte span of the callable's function node) is authoritative, and qualified names stay lexical and semantic data. Typed resolution still joins on `enclosing_func_qn`; typed answers are unchanged at every site of the ten goldens. C, C++ and CUDA use the final adopted definitions, including the preprocessed supersession path, through private aliases; what exact bytes and the adoption's own evidence cannot join is issue 77. `ENGINE_VERSION` 5 → 6; schema 3, cache format 5, protocol 6 and surface 4 unchanged. Byte-precise census of represented-callable orphans, engine 5 → 6: ada 475 → 0, c 41 → 0, cpp 9,153 → 0, kotlin 430 → 0, rust 1,756 → 0, csharp, go, java, python and typescript 0 → 0. Every represented-callable orphan on the ten goldens is 0. Five goldens refreshed after review (ada, c, cpp, kotlin, rust), five byte-identical |
| 75 | 2026-10-11 | P2-13 | scr | resolved | Found by P2-13's legacy comparison: a call inside a definition nested in a function body (a decorated inner view, a decorator's wrapper, a closure) has no enclosing definition in the segment. The segment has no node for the nested definition, and the site's `enclosing_node_id` is empty rather than the outer definition's, so a drawn edge from it has the File node as its source. 4.2.1 reserves an empty `enclosing_node_id` for a site outside every definition. In the Python golden, 449 of 3,840 call sites lie inside a definition's span with no enclosing node (119 drawn `CALLS` rows from the File), against 236 genuinely at module level | Resolved by decision 47 (patch 0013). The driver selected the nearest already-modelled enclosing function rather than minting local-definition nodes. Modelled function boundaries are identified from the definitions extracted before the unified walk, by declaration occurrence (qualified name, first and last line), never the name alone. Unmodelled nested callables keep their lexical scope (and the name typed resolution joins on) but are not graph-ownership frames, so `Call.caller` and `ast_path` agree on the represented enclosing definition. C, C++ and CUDA keep ownership by name, because definitions are adopted after their walk. `ENGINE_VERSION` 4 → 5; schema 3, cache format 5, protocol 6 and surface 4 unchanged. All ten goldens are ready. Python golden, engine 4 → 5: call sites inside a callable's span with no enclosing node 441 → 0; File-sourced `CALLS` from them 118 → 0; module-level sites 244 → 244; nodes, sites, candidates and `CALLS` rows unchanged; coverage call sites 3,861 unchanged. Five goldens refreshed after review (csharp, kotlin, python, rust, typescript), five byte-identical. The residual class of issue 76 is separate |
| 74 | 2026-10-11 | P2-12 | scr | resolved | P2-12's first `pdx-bench golden --accept` over the ten pinned goldens cannot complete: `cpp/fmt` indexes degraded (`pdx index` exit 2), reason `extraction_lost` on `test/gtest/gmock-gtest-all.cc`, a 533 KB, 14,442-line amalgamated test framework. The vendored engine's C/C++ typed resolver has a compiled-in per-file expression-evaluation step budget (`C_EVAL_MAX_STEPS_PER_FILE`, 10,000). It runs out on that file and counts as lost work (issue 21), so the segment is degraded. Deterministic: the same segment under one worker, isolation, a 4 GiB budget and every extraction switch raised. The other nine goldens are ready. Nothing was accepted; no golden is committed | Option A selected by the driver; B (making the budget run health) and C (another golden) rejected: a budget running out is real lost evidence, and replacing the golden would hide a G1 engine limitation. The C/C++ expression evaluation budget changed from a fixed 10,000 threshold to a deterministic source-size-aware budget: 10,000 per started 32 KiB, minimum 10,000, hard maximum 320,000 (patch 0012, decision 45, 4.5). Budget exhaustion remains lost work and remains a degraded reason. `ENGINE_VERSION` 3 → 4. The extraction cache format remains 5, because `engine_version` is already in the key. Resolved: all ten goldens index ready with engine 4. The file (532,839 bytes, budget 170,000) extracts with `extraction_lost` 0 (engine 3: 2), and `cpp/fmt` is ready with 7,633 nodes (unchanged) and 7,547 drawn edges (was 7,544), every change in that file. The other nine are row-for-row identical to engine 3. `make check-asan` is clean, `make vendor-verify` is exact, and the initial baselines are accepted |
| 73 | 2026-10-11 | P2-04 | scr | resolved | Issue 72's detector version 3 fixed cross-detector idempotence but narrowed URI-password protection relative to detector 2: by treating `+`, `-`, `.` and the other scheme bytes as forbidden before an embedded scheme, it stopped masking passwords detector 2 masked (`-mysql://u:p@h`, `+mysql://…`, `.mysql://…`, `1.mysql://…`). Rejected by independent review before P2-12's golden baselines: a secret detector may mask more, never less, without an approved SCR | Decision 43: a letter-led scheme is recognised at the input's start or after any byte that is not an ASCII letter or digit, which keeps every detector-2 start (its word boundary allowed punctuation) and adds `_`; the mask `X` is a letter, so masking never opens a boundary. Issue 72's bearer-before-`://` clause, shared `BEARER_TOKEN`, `@` continuation and one-pass normalisation are kept. `SECRET_DETECTOR_VERSION` 3 → 4; the cache format stays 5. Monotonicity against detector 2's pattern as a test oracle: `uri_detector_preserves_detector_two_coverage`, `uri_detector_covers_detector_two_on_the_corpus`, the property `uri_detector_never_loses_a_detector_two_password` (200,000 cases); lost ranges 0 |
| 72 | 2026-10-11 | P2-04 | scr | resolved | CI run 38098740924 (P2-11 review closure), attempt 1, failed on Windows in P2-04's property `secret_normalisation_preserves_offsets`: minimal YAML input `Authorization: Bearer <token>_mysql://u:<password>@h`. The same input fails identically on Linux; the defect predates the P2-11 closure: the word-boundary URI pattern dates from P2-04's first commit, 8f89f38, and the secrets code last changed in P2-08. Normalisation was not idempotent: the bearer token ran over `_mysql`, the URI detector, starting a scheme at a regex word boundary, missed `mysql` after `_`, and the masked token, a run of `X`, read as a scheme on a second pass, which masked the password then. The password reached the engine. Issue 44's claim that no other detector had the defect held per detector; this is one detector's mask completing another's match | URI password schemes follow URI grammar: a letter, then letters, digits, `+`, `-` and `.`, starting at the input's start or after a byte no scheme holds (`_` included), never at a regex word boundary. That alone left a class open (`Bearer <tok>_9mysql://…`, `<tok>_://…`, `<tok>=://…`, `Bearer 0000000000000000://…`: the masked token is always a scheme, the original need not be one), so by the driver's decision a bearer token before `://`, in the bearer detector's exact shape, also counts (decision 42). Every detector still reads the original bytes once, the ranges still merge, and masking still happens once. `SECRET_DETECTOR_VERSION` 2 → 3: the policy digest and every extraction cache key move (an older entry is a miss); `EXTRACT_CACHE_FORMAT_VERSION` stays 5, as do the segment schema, matrix, engine, protocol and surface. Regressions `bearer_adjacent_uri_password_is_idempotent`, `uri_scheme_boundary_uses_scheme_grammar_not_word_boundary`, `cross_detector_boundaries_are_idempotent`, `uri_boundary_change_leaves_ordinary_sources_alone`; the property, its pieces widened, passes 20,000 and 200,000 cases |
| 71 | 2026-10-11 | P2-11 | scr | resolved | 4.2.1 says a local binary with no server "records `repo_id_source = \"url\"` in `meta`", but 4.3's `meta` keys do not include it and the reader refuses a key it does not know, so no segment can record it without a schema change | Resolved by the P2-11 review closure on 2026-10-11 (decision 40): 4.3 gains the required meta key `repo_id_source`, `url` or `registered` (`segment::RepoIdSource`), and `SEGMENT_SCHEMA_VERSION` moves 2 → 3 (no other version moves). With `url` the writer and the reader refuse a `repo_id` that is not `repo_id_from_url(repo_url)`; a `registered` id is never derived from or compared with the URL. A schema-2 segment is refused and re-indexed, never migrated. `pdx index` writes `url`, reports it in its JSON and `pdx coverage`'s, and still refuses a configured server |
| 70 | 2026-10-11 | P2-11 | third-party | resolved | `pdx`'s command line (P2-11) needs an argument parser with subcommands, typed values and help; none was in the dependency graph | `clap` 4.6.7, pinned exactly, builder API, default features off but `std`, `help`, `usage`, `error-context` (no colour, suggestions, wrapping, environment arguments or derive macros), with `clap_builder` 4.6.7, `clap_lex` 1.1.1 and `anstyle` 1.0.14: four exact-version `safe-to-deploy` exemptions added by hand, each naming this issue. Exemptions, not audits: 111 exemptions, 0 audits, 0 imports |
| 69 | 2026-10-10 | P2-10 | third-party | resolved | P2-10's deliverable puts `build_segment`'s stages in `tracing` spans (Part 9's logging rule), and no tracing crate was in the dependency graph | `tracing` 0.1.44, pinned exactly, without default features (no attribute macros) but `std`, with `tracing-core` 0.1.36 and `pin-project-lite` 0.2.17: three exact-version `safe-to-deploy` exemptions added by hand, each naming this issue. Exemptions, not audits: 107 exemptions, 0 audits, 0 imports. No subscriber is installed by the library; spans carry stage names and counts only (decision 35) |
| 68 | 2026-10-10 | P2-09 | third-party | resolved | The engine's per-definition visibility is evidence only in Go (exported identifiers) and Python (a leading underscore): elsewhere it does not read a declaration's modifiers, so Java and Kotlin `public` methods come back non-public, and Rust, TypeScript, JavaScript, C#, C, C++, PHP, Scala and Ada private declarations public | Stage 4 stores `props.visibility` only for Go and Python and leaves it absent (unknown) elsewhere, never stored wrong (`visibility_props_only_where_engine_evidence`); `importance` takes an unknown visibility as not public (decision 28). Restoring it elsewhere needs an engine fact that reads declaration modifiers, an append-only change that moves `ENGINE_VERSION`, the cache format, the worker protocol and the surface version. Resolved at the pre-P2-12 checkpoint by the P2-11 review closure on 2026-10-11, option B (decision 41): Go and Python are PDX 2.0's only evidence-bearing visibility languages, every other language's visibility is unknown (`props.visibility` absent) and `importance` takes 0.6 for it; no engine change. Stated in 4.12.1 and on the trust page (4.13.2). Visibility in another language is a later SCR |
| 67 | 2026-10-10 | P2-09 | scr | resolved | 4.12.1 names `cyclomatic`, `cognitive` and `loop_depth` without defining them, and the engine's facts (Appendix D.4) are not the textbook metrics: its count is of each language table's branching node types, so boolean operators and conditional expressions are no decision, `try`, `with`, `defer` and `go` are, and a `switch` or `match` counts itself as well as each case | Ratified by the P2-09 driver review on 2026-10-10 (decision 32). For PDX 2.0, for callables only (decision 29): `cyclomatic` = 1 + the engine's branching-node count, `cognitive` = the engine's cognitive count, `loop_depth` = the engine's deepest loop nesting. The documented engine-specific deviations (below) are intentional product semantics; the metrics are not identical to every textbook definition. The acceptance fixtures on Java, Python, Go, TypeScript, Rust and C lock the definition (`cyclomatic_known_values`, `cognitive_and_loop_depth_known_values`, `complexity_engine_facts_cross_stage_boundary`). A later change of these semantics is a specification change, with the compatibility and build-identity treatment it needs before published segments can coexist with it |
| 66 | 2026-10-10 | P6-02 | blocker | open | 4.7.1 also takes namespace identities from sources no repository's Stage 4 has: a provider's Kubernetes `Service` name and `Ingress`/`Route` host, `pdx-arch.yaml`'s hostnames, brokers, data sources and contexts, a consumer's declared alias, and a protobuf file shared as an artifact across repositories | Missing facts: deployment resources (`Service`, `Ingress`) and the estate model are not inputs of Stage 4, and sharing is a cross-repository relation. Contracts stay `unresolved` without `pdx.toml [identity]`, never guessed from a manifest's name (`missing_fact_identity_from_deployment_and_estate_model`). P6-02 (estate model, `DEPLOYED_AS`) supplies them; P6-01 applies them when linking |
| 65 | 2026-10-10 | P6-01 | blocker | open | Two 4.7.1 forms carry their full meaning in a value bound before the call: a JMS producer created for a destination sends with none in its own arguments, and a route registered on a group, scope, nest or mounted router has a prefix composed at run time | Missing fact: local value flow (which value a local or a receiver holds). A JMS send with no destination gives nothing (`missing_fact_destination_bound_before_the_call`); a registration in a callable that composes a prefixed router, or on a parameter declared as a group, is withheld with `PrefixUnknown` (`missing_fact_router_prefixes_withhold_routes`); a router mounted from another callable or file is not visible and keeps the path it declares. P6-01 decides before drawing links whether to add the fact |
| 64 | 2026-10-10 | P6-01 | blocker | open | 4.7.1's table consumers include ORM repository classes bound to entities (`interface UserRepository extends JpaRepository<User, Long>`), but the extracted base class is `JpaRepository` with its type arguments erased, and `User` is a type reference of the whole file, tied to no base clause | Missing fact: a definition's base classes with their type arguments (`JpaRepository<User, Long>` → `[User, Long]`), an append-only engine fact. No consumer is guessed from a repository's, file's or method's name (`missing_fact_repository_entity_type_arguments`). P6-01 decides before drawing `SHARES_TABLE` |
| 63 | 2026-10-10 | P6-01 | blocker | open | Kotlin calls are extracted without their arguments (`kafkaTemplate.send("t", m)` has none), so no call-based 4.7.1 source can read its destination or URL in Kotlin: `KafkaTemplate.send`, producer calls, `RestTemplate`, `WebClient` | Missing fact: the arguments of Kotlin calls, as other languages' calls carry them (`Call::args`). Kotlin's annotations (`@KafkaListener` and the rest, Spring mappings) are read; its calls give nothing (`missing_fact_kotlin_call_arguments`). P6-01 decides before drawing links from Kotlin repositories |
| 62 | 2026-10-10 | P2-08 | scr | resolved | P2-08's contract sources did not cover every source the normative 4.7.1 table names (`WebClient`, `KafkaTemplate.send`, NATS and Redis publish, JMS producers, ORM repositories, and several provider frameworks), and its clarifications read as if a smaller list replaced the table | Every source of 4.7.1 classified (below): all those current facts can prove are implemented and in `contract_source_conformance_4_7_1`; the rest each need a named missing fact, with a fixture proving it, an issue (63 to 66) and an owner. No source is left implementable-now. 4.7's clarifications state that the table is the normative target and nothing in them narrows it |
| 61 | 2026-10-10 | P6-01 | ambiguity | open | The engine's channel facts reach Rust as a name and a direction only: the transport the engine classified them by is dropped at the interface, and socket messages and in-process events are channel facts too | No longer a gap in P2-08's rows (issue 62): every 4.7.1 broker source is read from call, import and annotation facts with its transport. An engine fact is used only when a broker client call in a file importing a Kafka or AMQP library confirms it; others are `UnconfirmedChannel`. What remains is P6-01's calibration: whether engine facts outside the named sources (sockets, emitters) should ever be links, which would need the transport across the interface |
| 60 | 2026-10-10 | P2-08 | third-party | resolved | Contracts must read YAML (OpenAPI, AsyncAPI, application configuration, changelogs) and XML (`pom.xml`, `*.csproj`, changelogs); no parser of either was in the dependency graph, and a hand-written general one is what the brief forbids | `saphyr-parser` 0.1.0 (event parser; the tree is built in `contracts::document`, an alias never expanded) and `roxmltree` 0.21.1 (read-only; a DTD refused), pinned exactly; with `arraydeque` 0.5.1 three new crates, each an owner-authorised exact-version `safe-to-deploy` exemption, not an audit. 104 exemptions, 0 audits, 0 imports. Decision 24 |
| 59 | 2026-10-10 | P2-08 | scr | resolved | `contracts.contract_id` is the primary key, yet several sightings in one repository can be one contract (an OpenAPI operation and its route, a producer and a listener of one topic, two providers of one key); 4.7.1 says nothing of merging them, or of whether contracts are also graph nodes and edges | Merged by `(kind, namespace_key)` only, in any order: `provides` + `consumes` = `both`; the strongest identity; the providers' one owner, or none with `owner_ambiguous` and `provider_owner_node_ids` when they differ (NULL owner = consumed-only or several provider owners); `raw_forms`, `source_paths` and all evidence as sorted sets. The `contracts` table is the record: P2-08 adds no contract node and no `EXPOSES`/`PUBLISHES`/… edge (4.7.2 reads only `contracts`), so 4.2.4's site rule is untouched. No DDL change, comment only |
| 58 | 2026-10-10 | P2-08 | scr | resolved | 4.7.1 says `namespace_key` = identity + key and 4.2.1 hashes it into the contract's id, but neither defines the text for an established identity, the column key, an artifact's coordinate parts or the canonical host; the first stored contract ids depend on it | `{"identity":…,"key":…}` compact JSON (route identity's escaping) for `exact` and `declared`, strength not in it; `unresolved:<repo_id>:<key>` for unresolved; an artifact's key is its own namespace. Hosts lower-cased, one terminal dot and user information dropped, default ports omitted, loopback hosts no identity; several identities, several rows. Column = table key `.` column; `ecosystem:group:name[@version]` with the eight ecosystem names. Fixed vectors for every kind, computed outside the crate. No version moves: no contract was stored before |
| 57 | 2026-10-05 | P2-10 | ambiguity | resolved | Malformed source can come back `parsed`: the engine marks a file `partial` only when an error region remains after it subtracts the regions definitions were recovered from, so `def broken(:` with a body is `parsed`. Coverage, the degraded status and the published parsed/partial/failed counts depend on what syntax damage means | Settled at P2-10 (decision 34): a file's status is the engine's report, never a second parse: `Parsed` → `parsed`, `Partial` → `partial`, `Failed` → `failed` (reason `parse`), as Stage 4 already maps it. A syntax defect the engine reports no status for is not a pipeline fact. `partial` counts in coverage and is no degraded reason; `failed` counts toward the `failed + skipped(size) > 5%` threshold. Fixtures for all three (`malformed_sources_follow_engine_status`) |
| 56 | 2026-10-05 | P2-07 | ambiguity | resolved | P2-07 extended 4.2.1 so that non-callable definitions sharing a kind, path and qualified name take the identical-signature collision scheme (`e3b0c442-1`, `e3b0c442-2`), without bringing it to the owner | Owner decision, 2026-10-05: approved as implemented. Ids must be unique, the extracted facts hold no stronger discriminator, and a line number would break the identity design. The limitation is explicit in 4.2.1: inserting another identical declaration before an existing one may renumber that group. No version moves. `duplicate_noncallables_have_unique_deterministic_ids` |
| 55 | 2026-10-05 | P2-07 | blocker | resolved | P2-07 copied the engine's entry-point flag into `props.is_entry_point`, and the engine sets it on every exported JavaScript and TypeScript declaration: the TypeScript golden showed plain exported functions as entry points, which Appendix B.2 does not make them | `is_entry_point` is decided from evidence of B.2's categories only: tests, route handlers, each language's conventional `main` (JVM, C#, C, C++ global namespace, Go, Rust binary roots), the framework bootstraps. The engine's flag is kept as `props.engine_entry_point`. `exported_typescript_function_is_not_entry_point`, `exported_javascript_function_is_not_entry_point`, `main_entry_points_survive_engine_flag_filter`, `entry_point_categories_are_kept` |
| 54 | 2026-10-05 | P2-07 | blocker | resolved | 4.2.4: every non-containment edge references its `site_id`. P2-07 made Spring's `DEFINES_ROUTE` edges with no site (the engine recorded a route as two strings with no position) and `routes_spring` asserted it; and one route per definition lost a mapping's other paths and methods | Engine patch 0011 records every route binding with its declaring node's position and node-type path (`pdxe_route`, `RouteFact`); one fact per method and path, literals only. Every `DEFINES_ROUTE` edge has a real `route` site, shared by the bindings of one declaration; no site, no route. `Builder` and the segment writer refuse a structural edge without its site. `ENGINE_VERSION` 3, cache format 5, protocol 6, surface 4 |
| 53 | 2026-10-05 | P2-10 | ambiguity | resolved | A POSIX file name may contain `\`, which Windows reads as a separator: such a file has no identity that is the same on every host. Discovery finds it on Linux and macOS; Stage 4 refuses any path that is not repository-relative POSIX, so today a repository holding one fails its build | Settled at P2-10 (decision 33): `build_segment` holds such a file back at the end of Stage 1 (`discover::split_unportable`, by Stage 4's own `is_repository_path`): discovery may have read it as it reads any file, but Stages 2 to 5 never process it and no identity is made from its path: no file row, node or site. It is counted in its language's coverage row as a file and as skipped, listed in `SegmentReport::unportable` with the reason `unportable_path`, and counted in the degraded threshold's file total. Stage 4's refusal is unchanged. `unportable_path_is_accounted_never_identified`, `split_unportable_keeps_identities_portable` |
| 52 | 2026-10-05 | P2-07 | third-party | resolved | P2-07's golden fixtures use snapshot tests; the ratchet refuses `insta` 1.49.0 and its diffing dependency `similar` 2.7.0 until they are recorded | Owner-authorised exact-version exemptions, `safe-to-run` (test-only), added by hand with notes naming this issue; no audit claimed, nothing regenerated. 101 exemptions. Owner confirmed as implemented, 2026-10-05: exemptions, not audits |
| 51 | 2026-10-05 | P2-07 | ambiguity | resolved | 4.3's `files` row requires a `blob_sha` and a `line_count` for every file, but Stage 2 reads only extraction candidates: a redacted, binary, oversized or unknown file is never read, so neither exists, and Stage 2 kept no line count for any file | The facts are kept where they exist and absent where they do not: `files.blob_sha` and `files.line_count` are nullable, NULL for a file never read; Stage 2 counts lines in the bytes it already reads. Part of `SEGMENT_SCHEMA_VERSION` 2 (with issue 50). Proven by `file_records_are_faithful`. Owner confirmed as implemented, 2026-10-05 |
| 50 | 2026-10-05 | P2-07 | scr | resolved | 4.2.2 stores the engine's score, strategy and candidate count verbatim on an edge or a candidate row, but the `candidates` table and `CandidateSite` had no candidate count: the first derived candidate rows would have lost it | `candidates.engine_candidates INTEGER` and `CandidateSite::engine_candidates`; `SEGMENT_SCHEMA_VERSION` 1 → 2, and a reader refuses version 1. Proven by `segment_roundtrip`, `reader_refuses_wrong_schema_version` and `candidate_rows_copy_engine_calibration` |
| 49 | 2026-10-05 | P2-07 | ambiguity | resolved | P2-06 settles every call site as a `Resolution`, and no task owned turning resolutions into persistent sites, `CALLS` and `CALL_REFERENCE` edges and candidate rows; `TESTS` edges depend on them | Owner decision: P2-07 owns it. Drawn bands give a site and an edge with the engine's numbers copied unchanged; other bands a site and a candidate row; unconfirmed sites and sites with no position in the file are counted, never rows. Proven by `drawn_edges_copy_engine_calibration`, `candidate_rows_copy_engine_calibration`, `unconfirmed_sites_are_counted_never_drawn` and `site_without_raw_position_is_counted_not_placed` |
| 48 | 2026-10-05 | P2-07 | scr | resolved | `ModuleKey` was not a graph identity, and Appendix B.3 has a module spanning files be one node with no `file_id`: no member file can stably identify or parent it | Owner decision, written into 4.2.1: `(Module, path = scope, qualified_name = "<language>:<name>", "")`; one file → that file is `file_id` and parent; several → no `file_id`, sorted `props.files`, nearest common folder (or the repository) as parent; symbols keep their physical parents and gain `props.module`. Fixed vectors in 4.2.1; `module_id_does_not_depend_on_file_count` |
| 47 | 2026-10-05 | P2-07 | ambiguity | resolved | Facts Stage 4 needs exist inside the engine and were dropped at the safe boundary: definitions' decorators, declared parameter types and route bindings, and calls' arguments | Exposed, append-only, deep-copied by `pdxe_result_build`; `Definition::{decorators, signature_param_types, route_path, route_method}` and `Call::args`. Existing facts passed through: no `ENGINE_VERSION` move of their own. Shares issue 46's cache format 4 and worker protocol 5. Fresh, cached and isolated-worker extractions agree |
| 46 | 2026-10-05 | P2-07 | ambiguity | resolved | 4.2.1's `site_id` needs the engine's node-type path from the enclosing definition to the site, but `Call` did not carry one, and nothing else (a line, an offset, a call index, a constant path) may stand in for it | The engine did not compute one: `engine/patches/0010-site-node-type-paths.patch` records it during the walk, so `ENGINE_VERSION` 1 → 2, `EXTRACT_CACHE_FORMAT_VERSION` 3 → 4 and the worker protocol 4 → 5. `Call::ast_path` for calls and callable references. Proven by the site-path tests listed in the entry |
| 45 | 2026-10-04 | P2-06 | ambiguity | resolved | An engine answer whose target is in no file of the project (`target_rel_path` absent: a built-in, say) settles its site, the safe model says, and must not be resolved by name; but no band says what such a site is, and the Rust stages, run on its name, would bind a built-in call to any definition of the repository that shares it | Owner decision, implemented: after the generic blocklist, such a site is `external`, target none, with the engine's score, strategy and candidate count kept; no Rust stage runs on its name. Proven by `engine_external_does_not_fall_back_to_local_name`, and for references by `untyped_callable_reference_is_not_a_call` |
| 44 | 2026-10-04 | P2-04 | blocker | resolved | P2-04's acceptance property `secret_normalisation_preserves_offsets` fails about one run in three (13 of 40 local runs, and P2-05's CI on Windows, run 37229601107): a bearer token followed by `=` and more token bytes (`Bearer <token>=A`) is masked only to the `=`, so the bytes after it reach the engine, and a second pass, reading the masked `=` as a token byte, masks them then. Normalisation was not idempotent, and part of a secret was not masked | The bearer token runs on over `=` and the token bytes after it in the same run: `[A-Za-z0-9\-._~+/]{16,}[A-Za-z0-9\-._~+/=]*`, so padding never ends a match early. It masks more, never less. `SECRET_DETECTOR_VERSION` 1 → 2, as a change to what a detector matches requires, which moves the policy digest and so every extraction cache key (an older entry is a miss); fixed vectors recomputed independently. Regression `bearer_masking_is_idempotent_after_padding`; the property passes 60 random runs and 20,000 cases. No other detector has the defect: each masks to `X` inside a class `X` already belongs to |
| 43 | 2026-10-04 | P2-06 | third-party | resolved | The engine applies a `tsconfig.json`'s path aliases and base URL to every file below the configuration, whatever its language: in a repository with a root `tsconfig.json` whose `baseUrl` is set, a Go import such as `example.com/acme/pkg/a` contains `/`, takes the base-URL fallback, becomes `./example.com/acme/pkg/a`, and its typed resolution is lost. The registry applies aliases to TypeScript and JavaScript only, so the two would read one repository differently | Confirmed against the pinned reference, whose import resolver tries the alias step for every language too. `engine/patches/0009-script-path-aliases-only.patch` applies the alias step only to an importing file the engine's own lookup classes as TypeScript, TSX or JavaScript; relative imports, the package map and Go's module mapping are unchanged. `polyglot_ts_alias_does_not_affect_go` (registry and stages) holds a root `tsconfig.json` with a base URL, an aliased TypeScript import and a Go module importing its own package in one repository: it failed before the patch and passes after, the Go answer the same with and without the configuration, and engine and registry agree |
| 42 | 2026-10-04 | P2-06 | ambiguity | resolved | Rust has no base classes: a trait's supertraits and an `impl Trait for Type` are what inheritance-guided resolution needs. The engine records `impl Trait for Type` pairs internally (`impl_traits`) and supertraits not at all; neither crosses the safe boundary, and `base_classes` is empty for every Rust definition | Owner decision, implemented: `pdxe_impl_trait { trait_name, struct_name, struct_qn }` and `pdxe_file_result.impl_traits`/`n_impl_traits` appended; `pdxe_result_build` takes and deep-copies them, refusing a counted array that is missing or a NULL string; `ImplTrait` and `FileExtract::impl_traits`; `EXTRACT_CACHE_FORMAT_VERSION` 2 → 3, worker protocol 3 → 4. The registry resolves each relation (`implemented_traits`, `implementors`) and inheritance-guided resolution follows a type's direct internal traits. Supertraits stay unrecorded: Rust's inheritance-guided support is the direct `impl` relations only, a stated coverage limit, never a guess |
| 41 | 2026-10-04 | P2-05 | scr | resolved | The language matrix makes Java's, Kotlin's, C#'s, PHP's, Perl's, Scala's, Groovy's and protobuf's module the package or namespace a file declares, but qualified names are built from the file's path, and no declaration crossed the safe boundary. The engine records the declaration for Java, Kotlin, C# and PHP (`namespace_name`) and drops it at the interface; for Perl, Scala, Groovy and protobuf it records none | Exposed as issue 40 exposed bases; owner confirmed 2026-10-04: `pdxe_file_result` appends `declared_namespace`, carried into `FileExtract::declared_namespace`; `pdx_engine::namespace_evidence` says where each language's evidence is (the file's declaration; C++'s qualified names; nowhere). Cache format 2 and worker protocol 3 cover it with issue 40. Perl, Scala, Groovy and protobuf get no module: none is invented from the directory, and no parser is added to manufacture one |
| 40 | 2026-10-04 | P2-05 | scr | resolved | Class hierarchies (P2-05) and inheritance-guided resolution (P2-06) need each definition's bases. The engine records them (`PDXEDefinition.base_classes`) and the surface codec keeps them, but `pdxe.c` dropped them, so neither `pdxe_definition` nor `FileExtract` had them, and the opaque surface is not to be decoded in `pdx-core` | Owner decision 2026-10-03, implemented: `pdxe_definition` appends `base_classes` and `n_base_classes`, the engine's strings in its order, spelling and case, NULL and 0 for none; `pdxe_result_build` deep-copies them and refuses a counted array that is missing or holds NULL; `Definition::base_classes`. `EXTRACT_CACHE_FORMAT_VERSION` 1 → 2 (a format-1 entry is a miss) and the worker protocol 2 → 3 (a protocol-2 worker is refused). No graph, schema, matrix, engine or secret-detector version moves |
| 39 | 2026-10-02 | P2-04 | third-party | resolved | P2-04 adds four packages the supply-chain ratchet refuses until they are recorded: `rayon` 1.12.0, with `rayon-core` 1.13.0 and `either` 1.18.0, for Stage 2's worker pool, and `sha1` 0.11.0 for the Git blob identity. The other packages P2-04 uses directly, `regex` 1.13.1 and `postcard` 1.1.3, and rayon's `crossbeam-*` dependencies, were already locked and recorded | Owner-authorised exact-version exemptions, `safe-to-deploy` because all four ship, each with a note naming this issue: no audit is claimed, none is imported, no version is a wildcard, nothing else changed. `cargo vet --locked` passes with 99 exemptions, 0 audits and 0 imports; CI run 37069384925's supply-chain job proved it, after a local check that dropping one exemption fails the ratchet |
| 38 | 2026-10-02 | P2-08 | ambiguity | resolved | 5.12 redacts `.env*` files and says `.env` files are parsed for keys only, never values; 4.7.1 resolves a contract's placeholders from `.env` when a value exists there. Read literally, both cannot hold: resolving a placeholder from `.env` reads the value 5.12 forbids reading | Owner decision, 2026-10-10: the security boundary wins. `.env*` stays redacted and unopened, is parsed for neither keys nor values, resolves no placeholder and strengthens no identity. Placeholders resolve only from non-redacted `application*.yml` and `application*.properties`, read as every file is, when exactly one eligible value exists; never a credential key's value. 4.7.1 corrected; P2-08's projection amended. `dotenv_never_resolves_contract_placeholder` |
| 37 | 2026-10-02 | P2-04 | scr | resolved | 5.12 replaces each secret value with `<REDACTED_SECRET>`, but spans, site identities and, later, the merging of compiler occurrences rely on exact byte offsets: a fixed-length marker in place of a value of any other length shifts every offset after it | Approved by the owner on 2026-10-02: normalisation preserves length, masking each byte of a secret value with ASCII `X` and never a carriage return or line feed, and keeps keys, separators, quotes, PEM marker lines and indentation; `<REDACTED_SECRET>` describes a masked value and is never substituted. Written into 4.5; proven by `secret_normalisation_preserves_offsets` and the leak regressions. No stored format changes |
| 36 | 2026-10-02 | P0-04 | ambiguity | resolved | Part 5.12 requires `cargo vet` records for new crates in `supply-chain/`, but no task delivered it: there was no `supply-chain/` directory, no audit configuration and no check, and every crate added since P0 was unvetted. Licences were checked (`cargo deny`); provenance of the code itself was not | P0-04 ownership restored by amendment. cargo-vet 0.10.2 is pinned (`scripts/cargo-vet.sh`). `supply-chain/` records the current `Cargo.lock`: its 95 crates are one-time bootstrap exemptions, explicitly not claimed as audited; no local audits, no imported ones. Normal CI runs `cargo vet --locked` (`make vet`), as do nightly and release, so a new or changed crate is refused until it is audited, imported by an owner's decision, or exempted on the record |
| 35 | 2026-10-02 | P2-03 | ambiguity | resolved | Stage 1 and Appendix C leave choices open: no binary-file detector; Appendix C mixes defaults with examples; whether `[secrets] patterns` replaces or extends the 5.12 defaults; where `.pdxignore` applies and how it combines with `.gitignore`; which `[precise.<family>]` tables and keys exist; what the hard-coded excludes match | Accepted as implemented: a file is binary when it holds a NUL byte; example values are not defaults; the root `.pdxignore` only, as an independent rule set; ignore sources 4.5 does not name are not read; excludes match directories by name at any depth; the four documented precise families, each with a `timeout_minutes` override. Overridden by the owner on review: `[secrets] patterns` extends a mandatory floor and never replaces it; the effective patterns are the sorted, de-duplicated union of the 5.12 defaults and the configured ones |
| 34 | 2026-10-02 | P5-03 | ambiguity | open | 4.3 gives `evidence.evidence_id` as the "sha of (fact_id, provider, provider_version, verdict)" and `semantic_occurrences.occ_id` as the "sha of (provider, symbol, file_id, start_byte, end_byte, role)", without the byte encoding issue 33 fixed for the graph's ids: separators, how numbers are written, and the output's form and length. Both are stored keys | Owned by P5-03, which first writes these rows: fix both encodings, by specification change, before a precise segment is published. P2-02 stores and reads the ids exactly as given and computes neither |
| 33 | 2026-10-02 | P2-01 | scr | resolved | 4.2.1 defines every identity as a hash, but leaves byte-level choices open that two implementations could make differently and so produce different stored ids: how `ast_fingerprint` serialises its node-type path, texts and ordinal, and in what form `site_id` reads it; the case of the base32 output; whether the overload disambiguator is hex text or raw bytes, and how the identical-signature ordinal is written; what an absent enclosing definition or site contributes; which clone URLs `canonical_clone_url` accepts | Approved by the owner on 2026-10-02 exactly as P2-01 implemented them, and written into 4.2.1 with reference vectors. `SEGMENT_SCHEMA_VERSION` stays 1: this completes the initial identity format before the first segment writer, so nothing stored is invalidated |
| 32 | 2026-10-02 | G1a | blocker | resolved | Evaluating G1a found the local engine build compiled without warnings as errors in vendored sources: its build directory had once been configured with `PDXE_VENDORED_WERROR` off, CMake keeps an option's last value, and neither `make engine` nor `make check-asan` stated it. The continuous integration builds were unaffected, but a local check could pass on a weaker policy than it appeared to | Both targets now state the policy on every configure (`make engine PDXE_VENDORED_WERROR=OFF` remains the explicit way to relax it). Rebuilt locally with gcc 13.3 and clang 18: every non-grammar source carries `-Wall -Wextra -Werror`, and every warning printed is in issue 19's exempt set |
| 31 | 2026-10-02 | P2-07 | ambiguity | resolved | Appendix A gives `javascript` the test rule "same as TS", and TypeScript's file-name patterns are `*.test.ts` and `*.spec.ts`: read literally, no JavaScript file is a test by its name, and neither is a TypeScript `.tsx`, `.mts` or `.cts` file. Nor does it say where a directory pattern such as `tests/**` applies, at the repository root or at any depth | Owner decision, 2026-10-05: `.test` and `.spec` on every TypeScript extension; JavaScript takes that convention on its own extensions, never `.test.ts`; directory patterns match at any depth on whole components, several components consecutively; `test*` is any component beginning `test`, case-sensitively. `LANGUAGE_MATRIX_VERSION` 1 → 2. Proven by `test_rules_follow_issue_31` |
| 30 | 2026-10-02 | P1-07 | ambiguity | resolved | Appendix A leaves parts of language detection unsaid: it gives `bash` a shebang without naming a form, writes two patterns over a file's name (`Dockerfile*`, `.env*`) beside the extensions without saying which wins when a name matches both kinds, and says nothing of case | Implemented to the letter where Appendix A speaks and narrowly where it is silent: a shebang is a `#!` first line naming `bash`, directly or through `env`, and nothing else, `sh` included; an entry with `*` is a pattern over the name, any other an extension the name ends with; an extension decides before a name pattern, and a name pattern before a shebang; matching is case-sensitive. Each choice has a test, and any can be changed by a specification change that bumps the matrix version |
| 29 | 2026-10-02 | P1-06 | blocker | resolved | The first sanitizer run over the corpus failed: for a project whose definitions typed resolution keeps none of, building the C# resolver's shared registry takes an offset from a NULL array, which is undefined behaviour in C. A documentation-only project reaches it; no fixture had | Fixed by `engine/patches/0008`, in the C# builder and in the Java one, which has the same shape and no caller yet. The regression `abi_run_health_untyped_only` fails under the sanitizers without the patch. No sanitizer report is suppressed |
| 28 | 2026-10-02 | P1-06 | ambiguity | resolved | The interface promises it writes nothing to standard output or standard error, but the TypeScript resolver prints a line to standard error when one of its work budgets runs out, whatever the logging switches say. The interface's no-output test does not see it because its fixtures never exhaust a budget | The two unconditional prints are removed through the vendored patch mechanism (`engine/patches/0007`); the lost-work count at both sites remains, and evaluation still degrades to an unknown type. A no-output regression starves the budget and proves every TypeScript run of the corpus exhausted it and was degraded for it, with nothing written; the sanitizer corpus is covered by the no-output contract too |
| 27 | 2026-10-02 | P2-04 | ambiguity | resolved | Isolated extraction survives a crashing engine but not a hanging one: the worker protocol bounds what crosses it and how often a worker is restarted, not how long a batch may take, and 4.5 names no failure reason for a file the engine never finishes | Decided by the owner on 2026-10-02 and implemented: every exchange with a worker, its introduction included, is bounded (120 s by default, another only through a constructor, no environment variable); a worker past it is killed and reaped, everything it sent is discarded, and Stage 2 fails with a typed timeout, so the build publishes no segment. A timeout is not `engine_crash`, no file status records it, and crashes keep P1-05's semantics. Written into 4.5; proven by `engine_isolate_times_out_and_reaps_worker` |
| 26 | 2026-10-02 | P2-04 | ambiguity | resolved | The extraction cache key in 4.5 is `(engine_version, language_matrix_version, secret_policy_digest, blob_sha)`, but an extraction also depends on the file's path, from which its qualified names and module are built, and on the node budget the environment may set (`PDX_ENGINE_WALK_MAX_NODES`), under which it can come back truncated | Decided by the owner on 2026-10-02 and implemented: the key is `(engine_version, language_matrix_version, secret_policy_digest, language_id, rel_path, blob_sha)`, `blob_sha` of the original bytes; only a clean extraction is cached (not truncated, and no lost work, which the interface now reports as `extraction_lost`); while the node budget is set no cache is used at all, a rule P2-04 found had to cover five more engine switches, the extension owner-approved on 2026-10-03; a hit is used only if it is exactly this source's clean extraction. Written into 4.5; no version bumped |
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
| 7 | 2026-09-28 | P0-06 | ambiguity | resolved | The importance formula's test penalty is given as 0.3 for test nodes; the factor for a non-test node is never stated | Settled at P2-09, 2026-10-10: the identity, 1.0, as the P2-09 task brief directs (`test_penalty = 1.0 otherwise`); no specification change request is needed. A test is a `Test` node. `importance_formula` |
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

### 115 — The A0 runner can record the source's `ENGINE_VERSION` while running a stale executable

Found during issue 114's A0 run. `scripts/a0/run.py` measures with
`target/debug/pdx-bench` and never builds it, rightly: a run must not change what it
measures. But it took the `engine_version` it records from
`crates/pdx-core/src/consts.rs`, the source, so nothing tied a result to the executable
that produced it. Issue 114's working tree had moved `ENGINE_VERSION` to 10, the
executable had been built before that change, at engine 9, and the run measured engine
9 and labelled every document engine 10. The numbers showed none of the expected
change; the run was discarded and measured again after a rebuild. No committed result
came from it.

**Resolution (hotfix, 2026-10-14).** The executable reports its versions and the runner
refuses to measure with one that is not the source's.

- **The report.** `pdx-bench versions` prints one JSON object (`audit::versions`,
  format 1): `engine_version`, the linked engine's own answer at run time
  (`pdxe_version`, a string); `pdx_core`, the constants compiled in
  (`ENGINE_VERSION`, `EXTRACT_CACHE_FORMAT_VERSION`, `LANGUAGE_MATRIX_VERSION`,
  `SECRET_DETECTOR_VERSION`, `SEGMENT_SCHEMA_VERSION`); and `worker_protocol`, the
  protocol its isolated extraction speaks (`pdx_engine::isolate::PROTOCOL`, now public).
  The engine's `SURFACE_VERSION` is private to the engine's interface code and the
  executable cannot report it, so it is not checked.
- **The check.** First, before any directory is made or any tool runs, `run.py` reads
  each constant from the source (`pub const NAME: u32 = n;`, exactly once) and compares
  it with the report: `ENGINE_VERSION` against both the engine's answer and the
  compiled constant, the others against their compiled values, `PROTOCOL` against
  `worker_protocol`. Any difference refuses the run, naming every one:
  `A0 refused: source ENGINE_VERSION=10 but pdx-bench reports engine_version=9; source
  ENGINE_VERSION=10 but pdx-bench reports pdx_core.ENGINE_VERSION=9. Rebuild the
  executable (cargo build -p pdx-bench) before running the audit.` A missing
  executable, one whose `versions` fails (any executable built before this fix), one
  that prints no JSON or another format are refused the same way.
- **Held.** The executable's SHA-256 is taken at the check. Each dataset and the
  result set check it unchanged; an executable rebuilt during the run refuses it before
  any result is written.
- **Results.** Written to a stage in the work directory and promoted into
  `bench/results/a0/` together, only once all are written; a copy that fails leaves the
  directory as it was.
- **Provenance.** A document's `engine_version` is the executed engine's answer,
  checked equal to the source's, never the source read again.

Tests (`scripts/a0/test_a0.py`, `ExecutableGuard`, now run by `make test`), each with a
checkout and an executable of its own: A1, an executable built from the source passes
and the run goes on to the oracles; A2, an executable reporting engine 9 against source
10 is refused with the message above, and only `pdx-bench versions` ran; A3, a refusal
(a stale schema version, a failing `versions`, another format, no executable) writes no
work directory and leaves the results as they were; A4, the provenance's version is the
executed engine's, unchanged when the source moves after the check, and an executable
rebuilt after it runs no dataset; A5, a source move of each of the six constants without
a rebuild is refused naming it. Also: results promoted only when every report is
written and the executable is unchanged, and promotion all or nothing.
`versions_report_what_the_executable_was_built_with` (`crates/pdx-bench/tests/
audit_sites.rs`) runs the built executable and compares its report with the engine and
the constants.

Mutations, each killed: the check removed, a difference not refused, the check after
the work directory, a missing executable accepted, a constant or the protocol dropped
from the comparison, no check before a dataset, before the reports or before promotion,
the provenance or the executed version read from the source, results written in place,
promoted one by one, and staging files left after a failed copy.

Live: the executable left by issue 114's parked work (no `versions`) is refused; with
`ENGINE_VERSION` moved to 10 in `consts.rs` and no rebuild, the run is refused with the
message above; rebuilt, the check passes. `bench/results/a0/` was byte-identical
throughout.

No resolution, extraction, band or version changes, and no A0 rule: `a0-match-1` and
`a0-metrics-1` are unchanged and the committed results are not regenerated.

State: resolved.

### 114 — Rust `use` declarations have no lexical scope

Found by issue 111. A Rust `use` binds its names in the module or block it is written
in, and nowhere else. Neither of PDX's readings models that scope:

- **The typed pass's use map is the file's.** It reads the `use` declarations at the
  top of a file and in its inline modules' bodies, as the reference does, and answers
  every site of the file from all of them. A test module's `use crate::other::fetch;`
  is evidence for the module's own `call()`, which calls the module's own `fetch`.
- **A `use` in a function body is read by neither pass.** Its names are no evidence for
  the calls in its function, so a call through it has no typed answer, and Stage 3
  finds no import for it.
- **Stage 3 reads a file's top-level imports only.** An inline module's `use` is not
  in the file's import map, so a call in a test module that imports another `fetch` is
  drawn `import-guided` to the module's.

Shown with engine 8 (`48c8d7e`) and engine 9 (issue 111) on one project-authored crate:
a module that defines `fetch` and calls it from `call()`, and a test module that
imports `crate::other::fetch` and calls it from `probe()`, once with a plain `use`
(`plain.rs`) and once in a list (`use crate::{other::{fetch}};`, `listed.rs`); and a
function that imports `crate::other::on` in its body (`fnscope.rs`).

| Site | Rust's target | Engine 8 | Engine 9 |
|---|---|---|---|
| `plain::call` → `fetch` | `plain::fetch` | `typed` `other::fetch` (wrong) | `typed` `other::fetch` (wrong) |
| `plain::tests::probe` → `fetch` | `other::fetch` | `typed` `other::fetch` | `typed` `other::fetch` |
| `listed::call` → `fetch` | `listed::fetch` | `typed` `listed::fetch` | `typed` `other::fetch` (wrong) |
| `listed::tests::probe` → `fetch` | `other::fetch` | `typed` `listed::fetch` (wrong) | `typed` `other::fetch` |
| `fnscope::inner` → `on` | `other::on` | `exact` | `exact` |
| `fnscope::outer` → `on` | none (does not compile) | `exact` | `exact` |

The file-wide map is the reference's and predates issue 111: engine 8 draws
`plain::call` wrong. Engine 8 read no `use` list, so for `listed.rs` it drew the test
module's call wrong instead; issue 111 reads the list, and the wrong edge moves to the
module's call. Issue 111 withholds a name the file's own `use` trees bind to two
different paths, which covers the conflicting case but not a module's own definition.
The measured corpora hold no such site: A0 after issue 111 finds no new wrong trusted
edge in the Rust golden or PDX itself.

What it costs: PDX itself has 34 calls imported by a function body's `use` that lost
their typed answers to issue 104 (R2): 31 in `crates/pdx-core/tests/derive.rs` (`is_test_path` 18
times, `normalise_type`, `overload_disambiguators`, `is_repository_path`, `file_status`,
`edge_id`) and 3 in `crates/pdx-core/tests/ids.rs` (`module_node_key`,
`route_qualified_name`), each inside `assert_eq!` or `assert!`, imported by a test
function's own `use`. Issue 111 does not restore them; they have no site. `crates/pdx-core/tests/rust_use_trees.rs` records the Stage 3 side
(`a_name_bound_to_two_paths_is_no_evidence`: both `fetch` calls `import-guided` to the
module's) and that a function's `use` is no evidence outside it
(`a_functions_use_is_no_evidence_outside_it`).

To fix: give every `use` its lexical scope (the file's module, an inline module, a
block, a function body) and bind its names only for the sites in that scope, Rust's
shadowing applied, in the typed pass and in Stage 3 alike. Not a fix: reading a
function's or an inline module's imports as the file's. That broadening is what draws
`plain::call` wrong; it must not be extended to function bodies, and the inline
modules' share of it is this issue's to remove.

State: open.

### 113 — Rust paths through `crate::`, re-exports and `index.rs`, and an imported type's impls

Found by issue 111, which reads the `use` lists that hid the first two. Three causes,
recorded together because each makes a Rust path name the wrong module or caller, and to
be fixed separately.

**113A: a path through `crate::` and a re-export names no definition.** In
`axum/src/extract/nested_path.rs`, the test module imports `crate::{extract::{NestedPath,
Request}, …}`. `axum::extract` re-exports the type (`pub use
self::nested_path::NestedPath;`). The typed pass keeps the path as written, as
`crate.extract.NestedPath`: `crate::` is not read as the crate's root module, and no
lookup follows the re-export to `axum.src.extract.nested_path.NestedPath`. So the type
of `nested_path: NestedPath` is unknown and `nested_path.as_str()` gets no answer
(`method_not_found`): 5 sites, lines 129 to 199. Engine 8 answered them only because it
could not read that nested `use` list and the name fell to the file's own definition.
With a plain `use crate::extract::NestedPath;`, engine 8 draws the same call `exact`
(shown with the crate above). To fix: anchor `crate::`, `self::` and `super::` in a
type path as Rust does, and follow `pub use` re-exports to the definition, by
identity, never by a last-segment guess.

**113B: an impl of an imported type names its methods' caller under the import.**
`axum-core/src/response/into_response_parts.rs` imports `HeaderMap` from `http` and
implements `IntoResponseParts for HeaderMap`. The typed pass names that impl's
`into_response_parts` `http.header.HeaderMap.into_response_parts`, the self type read
through the import; the extraction names the site's owner by the file's module,
`…response.into_response_parts.HeaderMap.into_response_parts`. A typed answer joins its
site by the exact caller, so the correct answers (`ResponseParts::headers_mut` and
`extensions_mut`, lines 151 and 278) join no site, and Stage 3 draws both `exact`, same
targets. With a plain `use http::header::HeaderMap;` engine 8 does the same. This is
not 113A's cause: the target is found, and the caller's name is wrong. To fix: name an
impl's methods' caller as the extraction names their owner (the file's module and the
type as written), whatever the self type resolves to.

**113C: a module file named `index.rs` is named as its parent.** The engine gives the
functions of `crates/pdx-bench/src/golden/index.rs` the qualified names of
`golden/mod.rs`'s module (`crates.pdx-bench.src.golden.sibling_pdx`), its file stem left
out as for `mod.rs`. Rust names that module `golden::index`, so the path a call or a `use`
spells (`index::run_pdx`, `pdx_bench::golden::index::sibling_pdx`) names a module with no
such function and the typed pass has no answer. In PDX itself, 4 calls that had typed
answers before issue 104 (R2), when a unique last name was enough, have none:
`crates/pdx-bench/src/golden/run.rs:213` and `crates/pdx-bench/src/main.rs:223` are drawn
`import-guided` to the right targets, and `crates/pdx-bench/tests/golden_run.rs:384` and
`397`, inside `assert_eq!`, have no site. The name is also a definition's identity: a
`golden/mod.rs` and a `golden/index.rs` defining one name would share a qualified name.
To fix: name a module file by its stem, `mod.rs` alone standing for its directory.

All three predate issue 111 and draw no wrong edge. In the Rust golden, where several
types define `as_str`, 113A's five calls draw no edge (in the crate above, where `as_str`
is unique, `exact` on engines 8 and 9); 113B's two are `exact` with the right targets;
113C's are `import-guided` and right, or have no site.

State: open.

### 112 — Patch 0028 changed extraction output without moving `ENGINE_VERSION`

Found after issue 104 (R2). The extraction cache keys an entry by
`(EXTRACT_CACHE_FORMAT_VERSION, ENGINE_VERSION, LANGUAGE_MATRIX_VERSION, secret
policy digest, language, path, blob)` (`index::extract::CacheKey`), and
`ENGINE_VERSION` is "bumped when its output can change". Patch 0028 changed what
Rust extraction records and left it at 7, so for the same source, path, language,
policy and matrix an entry written by engine 7 before the patch has the same key as
one written after it. Engine 7 had been pushed and its caches written, on this
machine and wherever the branch was built; R2's first golden run here read such
entries and drew wrong edges.

What an extraction record holds that the patch changed:

- `calls[].lexical`: a bare Rust call of a function its block declares is
  `LOCALLY_BOUND` (`callee_is_locally_bound`, also in the surface's calls), which
  Stage 3 settles `unresolved`.
- The surface's `resolved_calls`: Rust's per-file typed pass runs during extraction,
  and a cached file is resolved from its surface. Its answers need path evidence now (a
  qualified path, or a bare name's import), and it gives none for a name a `match`
  arm's pattern or a block binds. So the answers it stored differ: no
  `lsp_short_name_unique` answer by uniqueness alone or against a disagreeing path,
  and no `lsp_callable_value_reference` for an arm's binding. The cross-file pass runs
  on every build and is not cached.

Shown with the two builds, on one repository: pre-0028 engine 7 (`d96bb2f`) and
post-0028 engine 7 (`d14ccd4`). Both store its 5 files under the same 5 object ids,
and 3 entries' bytes differ: `src/lib.rs` (a module-qualified call), `src/strip.rs` (a
block's function) and `src/pick.rs` (an arm's binding). Run over the pre-0028 cache,
the post-0028 build wrote no entry, so it served them all, and drew
`first -> lib.expand` (`scoped`) instead of `first -> a.expand` (`typed`), and the
arm's `params` as a `typed` `CALL_REFERENCE`: two of R2's corrections undone.

**Resolution (hotfix, 2026-10-14).** `ENGINE_VERSION` moves from 7 to 8, where the
contract puts it: `crates/pdx-core/src/consts.rs` and the engine's own
`pdxe_version()` (`engine/api/pdxe.c`), which `engine_version_is_the_engines` keeps
equal. Engine 7 began with patch 0015 (issue 83); 0028 is engine 8, and the patch
README says so. No other version moves:

| Constant | Value | Why not |
|---|---|---|
| `EXTRACT_CACHE_FORMAT_VERSION` | 6 | the envelope and its layout are unchanged |
| worker protocol | 7 | the messages are unchanged |
| `SURFACE_VERSION` | 5 | the surface's fields are unchanged; only values differ |
| `SEGMENT_SCHEMA_VERSION` | 3 | the segment's tables are unchanged |
| `LANGUAGE_MATRIX_VERSION` | 2 | the matrix is unchanged |
| `SECRET_DETECTOR_VERSION` | 4 | the detectors are unchanged |

Nothing is migrated or deleted: an engine 7 entry is a miss, the file is extracted
again, and an engine 8 entry is written beside the old one.

`cache_entries_of_engine_seven_are_never_served` (`crates/pdx-core/tests/extract.rs`)
plants, under engine 7's key, the extraction of a source with a block's function
called, the call bound to nothing as engine 7 recorded it. Cold, the entry is a miss:
the file is extracted again, the call is `LOCALLY_BOUND`, the old entry is unchanged
and an engine 8 entry is written. Warm, that entry is served and equals the cold
extraction. With `ENGINE_VERSION` back at 7 (and the engine's own), it fails: the
engine 7 entry is served. `cache_object_id_fixed_vector` and
`cache_key_includes_engine_version` pin engine 8's key
(`5d5e1d9a…0b68`, independently computed), beside engines 3 to 7.

With the builds above, engine 8 run over the pre-0028 cache wrote 5 new entries,
changed none, and produced a segment byte-identical to its own cold and warm runs.
Its graph is the post-0028 engine 7 graph: every table equal, only
`meta.engine_version` differs.

`scripts/a0/run.py` wrote `engine_version: 7` into A0's provenance whatever engine it
measured. It now reads `ENGINE_VERSION` from `consts.rs`. The committed post-R2
results keep 7: they were measured at `d14ccd4`, whose engine then called itself 7
and is this engine 8.

The audit since engine 7 began (patch 0015):

| Patch | Extraction output | Note |
|---|---|---|
| 0016–0019 | changed (C-family preprocessing values and bounds) | under cache format 5 |
| 0020 | changed (site paths complete or absent) | under cache format 5 |
| 0021 | changed (a tree deeper than 512 is lost work) | under cache format 5 |
| 0022 | unchanged (the climbs' answers, from frames) | performance |
| 0023 | changed only past a scanner's bound, which overran its buffer | under cache format 5 |
| 0024 | values unchanged; texts held as ranges | moved the cache to format 6 |
| 0025–0027 | unchanged (each lookup meets the scan's answers in its order) | performance |
| 0028 | changed (above) | this issue |

0016 to 0021 and 0023 also changed what engine 7 extracted without moving it, the same
defect at the time. Their entries are format 5's (`v5/`), which no build since 0024
reads (`cache_formats_before_6_are_a_miss`). Within format 6, 0028 is the only change,
so engine 8 at 0028 suffices.

State: resolved.

### 111 — Rust typed resolution misreads nested `use` lists

Found by issue 104 (R2). Rust typed resolution builds a file's `use` map from each
declaration's text, splitting a brace list at every comma:

- `use crate::{routing::{on, MethodFilter}, Router};` gives `on` no entry, or the
  path `crate::on`;
- a list spread over lines keeps the line breaks in its paths;
- `pub use` is read with its visibility, so none of its names has an entry.

Since issue 104 an imported bare name is a typed answer only through the path its
import gives, so these imports are no evidence. In the Rust golden, 16 correct `typed`
edges become `exact` with the same targets (`parse_attrs`, `from_fn`, `any`, …), 6 are
lost to `candidate` (`on`, `on_service`), and 9 unmaterialised typed answers are lost;
PDX itself loses 38 unmaterialised ones. None is drawn wrong.

To read `use` trees from the syntax tree (nested lists, aliases, `self`, globs,
visibility), and to measure the typed answers that recovers and any new one that is
wrong. Stage 3's own reading (`resolve::imports`) already handles nested lists.

**Resolution (2026-10-14, patch 0029, engine 9).** A file's `use` declarations are read
from its syntax tree, never its text (`rust_collect_uses`, `engine/src/lsp/rust_lsp.c`):

- every `use_declaration`'s `argument`, at the top of the file and in its inline
  modules' bodies, in source order, through the pinned grammar's use trees:
  `scoped_use_list` (a nested list's prefix joins its parent's), `use_list` (its
  comments bind nothing), `use_as_clause` (the alias; `self as o` binds the prefix),
  `use_wildcard` (the whole module), `scoped_identifier`, `identifier`, `crate`,
  `self` (`{self}` binds the prefix's last name) and `super`; a leading `::` is dropped
  and `_` binds nothing;
- the visibility is not part of the path: `pub use`, `pub(crate) use` and
  `pub(in path) use` bind their names as `use` does;
- a name the file's own `use` trees bind to two different paths is withheld: no import
  is evidence for it. Imports a caller bridges in after the file's own keep their
  first-match order;
- an import is found by its local name through an index (`RustLSPContext::uses_by_name`,
  `lsp/name_index.h`), never by scanning the file's imports; a refused index finds
  nothing;
- an alias reaches its function: the fallback looks for the function by the last name
  of the import's path, and a call by its alias is answered `lsp_import_alias`, joined
  by the name it spells, as Python's imported aliases are.

The reference reads the text. `pub(crate) use unknown::Unknown;` is to it the path
`pub(crate) use unknown::Unknown`: a malformed path, the visibility and the whole
declaration read as the imported path. Engine 9 records the path Rust imports,
`unknown::Unknown`. In the engine differential that is one typed row of
`golden:rust/axum` (`axum-core/src/body.rs`, `Unknown::new()`, both unresolved), whose
callee the reference spells `pub(crate) use unknown.Unknown.new` and engine 9
`unknown.Unknown.new`: a ratified semantic divergence from a known-wrong reference
behaviour (decision 66, record `issue-111-axum-core-body-unknown-new`), captured from
the live run. The other 31 corpora are identical.

`ENGINE_VERSION` moves from 8 to 9 (issue 112's rule): a Rust file's per-file typed
answers, which its surface holds, change for an unchanged source, and so do the calls
those answers add inside macros. `EXTRACT_CACHE_FORMAT_VERSION` stays 6, and no other
version moves. `cache_entries_of_engine_eight_are_never_served` plants engine 8's
extraction of a file importing through a nested multi-line list: cold, a miss with the
call the answer adds present, the old entry unchanged and an engine 9 entry written;
warm, a hit.

Tests: `crates/pdx-core/tests/rust_use_trees.rs` (13 tests over a crate of
24 source files: simple, nested, deep, multi-line with comments, aliases, `self`,
globs, visibility, `super` and an inline module, a name bound to two paths, an external
crate, a function body's `use`, and no or wrong evidence) and `engine/tests/typed_lookup.c`
(Rust `use` lookups: the scan's answers, linear work, the scan quadratic, the index
refused). Sixteen mutations, all killed, every file restored
byte-identical: nested groups flattened, lists split at raw commas, `pub use` ignored,
aliases dropped, `{self}` and `self as` bound wrongly, a glob's prefix dropped,
visibility-wrapped `use` ignored, the `crate`/`self`/`super` anchor lost, an external
crate taken for the project's, the project-wide unique short name restored, the first
of two bindings taken, the file's own trees taken for bridged imports, an alias answered
by the path's label, a function body's `use` read as the file's, and `ENGINE_VERSION`
left at 8. The glob-prefix mutation first survived: the fixture's nested glob read the
same without its prefix, so a nested glob whose prefix changes the target was added.

A0 (same pins, tools, `a0-match-1`, `a0-metrics-1`; baseline post-R2):

| Golden | | Post-R2 | Post-111 |
|---|---|---|---|
| Rust | `typed` | 843 / 844 = 99.88% | 859 / 860 = 99.88% |
| Rust | `import-guided` | 24 / 25 | 24 / 25 |
| Rust | `exact` | 525 / 1,150 = 45.65% | 511 / 1,136 = 44.98% |
| Rust | `scoped` | 155 / 477 = 32.49% | 155 / 476 = 32.56% |
| Rust | union | 1,610 / 2,559 = 62.92% | 1,612 / 2,560 = 62.97% |
| Rust | recall | 1,407 / 4,052 = 34.72% | 1,409 / 4,056 = 34.74% |
| PDX | `typed` | 5,870 / 5,870 = 100.00% | 5,904 / 5,904 = 100.00% |
| PDX | `import-guided` | 64 / 68 = 94.12% | 39 / 43 = 90.70% |
| PDX | union | 6,591 / 7,753 = 85.01% | 6,599 / 7,761 = 85.03% |
| PDX | recall | 6,560 / 7,816 = 83.93% | 6,568 / 7,825 = 83.94% |

Python's numbers are identical, site for site. Compared site by site with the post-R2
run, the Rust golden changes at 47 sites:

- 16 correct `exact` edges are correct `typed` (`parse_attrs` 9 times,
  `parse_assignment_attribute`, `any` 3 times, `from_fn` 3 times);
- 6 `candidate` sites are correct `typed` (`on` and `on_service`:
  `axum-extra/src/routing/mod.rs:458`, `axum-extra/src/routing/resource.rs:120`,
  `axum/src/form.rs:256`, `axum/src/routing/tests/mod.rs:145`, `186`, `305`);
- 9 typed answers no site held (`path_for_nested_route` in `assert_eq!`,
  `axum/src/routing/tests/mod.rs:1150` to `1160`) are correct `typed`, still
  unmaterialised;
- 1 `scoped` false positive is correct `typed` (`Unknown::new()` in
  `axum-core/src/body.rs`, drawn to `Body::new` before);
- 8 typed-only sites appear, answers inside macros (`load`, `from_secs`, `file_name`,
  `new`), none drawn;
- 5 correct `typed` edges are lost: `NestedPath::as_str` in `assert_eq!`
  (`axum/src/extract/nested_path.rs:129` to `199`), whose type path the test module's
  nested `use` now gives as `crate::extract::NestedPath`, a re-export the typed pass
  does not follow (issue 113A); engine 8 answered them only because it could not read
  that list;
- 2 correct `typed` edges are correct `exact`, same targets: `res.headers_mut` and
  `res.extensions_mut` (`axum-core/src/response/into_response_parts.rs:151`, `278`),
  whose answers name their caller under `HeaderMap`'s and `Extensions`' import path, now
  read (issue 113B).

PDX itself changes at 43 sites, every one to correct `typed`: 25 from `import-guided`, 7
typed-only, 5 `blocked`, 3 `candidate`, 2 with no site before and 1 `exact`; none is lost.
Of the 38 unmaterialised typed answers R2 cost PDX, 2 are back; 34 are calls through a
`use` in a function body (issue 114) and 2 go through `golden/index.rs`, whose module
the engine names without its stem (issue 113C).

No site of either Rust corpus becomes a false positive: the `typed` false positive is
still issue 109's blanket impl and the `import-guided` one issue 110's macro-written impl,
and PDX's 4 `import-guided` false positives are the ones before.

The axum golden replaces 32 of its 3,659 drawn edges, node counts unchanged: the 16
`exact` and 1 `scoped` edges above drawn `typed`, the 6 new `typed` edges, the 5
`NestedPath::as_str` and 2 impl edges, and the `TESTS` edges beside them. The other nine
goldens are unchanged.

The closure conditions:

1. The 31 sites restored to strong evidence: met. All 31 are correct `typed`, 22 drawn
   and 9 unmaterialised.
2. No new wrong trusted edge: met in the Rust golden and PDX itself. The crate issue 114
   records shows how a test module's import can draw a module's own call wrong, as
   engine 8 already did with a plain `use`; neither corpus holds such a site.
3. What it found, recorded: issues 113 and 114.
4. The differential passes under decision 66: met.
5. Validation and CI: see the commit.

State: resolved.

### 110 — Items a `macro_rules!` invocation writes are no definitions

Found by issue 104 (R2). `axum-core/src/body.rs` writes `impl From<&'static str> for
Body`, and others, through `body_from_impl!`. The extraction represents only the
handwritten `impl From<()> for Body`, so its `from` is the only one the registry has.
`Body::from("…")` (`axum-core/src/ext_traits/request.rs:355`) is then drawn
`import-guided` to it: the Rust golden's one `import-guided` false positive.
`e9_an_import_guided_from_reaches_the_imported_types_impl` reproduces it.

To decide whether item-position macro invocations are expanded for definitions (the
typed pass already expands `macro_rules!` bodies for calls), or whether the
definitions a macro may write make a name ambiguous. `from` is never blocked by name.

State: open.

### 109 — A trait impl chosen for `Type::method` without its bounds

Found by issue 104 (R2). `WithRejection::<TestExtractor, TestRejection>::from_request`
(`axum-extra/src/extract/with_rejection.rs:196`) reaches the blanket
`impl<S, T> FromRequest<S, ViaParts> for T`, because its type implements only
`FromRequestParts`. The engine answers it `typed` (`lsp_ufcs`) with `WithRejection`'s
own `FromRequest` impl, whose bound `E: FromRequest<S>` the type does not meet. It is
the Rust golden's one remaining typed false positive.
`e6_a_call_a_blanket_impl_answers_is_recorded_as_the_engine_gives_it` records it.

The registry holds the blanket impl's method under a receiver named after the impl's
type parameter, indistinguishable from an impl for an external type. To find whether
the engine can withhold a `Type::method` answer when a blanket impl of the same trait
may supply the method, without scanning the registry per call (issue 92), or check an
impl's bounds. Never special-case the trait.

State: open.

### 108 — Target identity among definitions sharing a qualified name

A child of issue 95 (decision 65, remediation R7). Several definitions can share one
qualified name in a file: overloads, a trait's method implemented for several types
under one name, a function declared twice under different `cfg` attributes. A typed
answer naming such a name maps to several definitions and is withheld (4.5 Stage 3),
and the name stages see each as a distinct candidate of one name.

A0 measured, in the Rust golden:

| | Relations |
|---|---|
| target shares its qualified name | 89 |
| with a structural site | 83 |
| with a typed answer | 2 |
| withheld (the name maps to several definitions) | 2 |
| drawn | 52 |
| drawn to the right definition | 49 |
| drawn to a wrong one | 3 |

scip-python gives Python's `@overload` stubs and their implementation one symbol, so
A0 has no verdict for those 28 calls of the Python golden.

To investigate whether the extraction's normalised signature, the definition's
disambiguator, the call's argument count and types, or the engine's occurrence
identity can select the definition a call reaches. Overloads are never collapsed
because their qualified names are equal.

State: open.

### 107 — Rust call sites the oracle sees and Stage 3 does not

A child of issue 95 (decision 65, remediations R4 and R8). Of the Rust golden's 4,455
oracle relations, 394 have no structural call site at all: site loss, apart from
resolution loss.

**107A, ordinary syntax (R4).** 56 relations at ordinary Rust syntax:

- 30 turbofish calls of a path (`assert_from_request::<_, ()>()`);
- 24 turbofish calls of a method (`parts.extract_with_state::<S, T>(&state)`);
- 2 member calls in a chain with a line comment before the dot
  (`examples/error-handling/src/main.rs`).

These should be ordinary call sites. To find whether the grammar or the extractor
loses them, and to make them sites.

**107B, macro token trees (R8).** The Rust golden has 1,696 call-shaped texts inside
macro invocations' token trees:

- 437 are oracle relations;
- 90 are already extraction sites (`assert_eq!`, `format!` and `assert!` arguments)
  and are all drawn correctly;
- 9 are sites typed resolution found by itself, which Stage 4 cannot persist;
- 338 relations have no site at all.

A macro's tokens are not Rust expressions in general. Before recovering any, the
calls are classified: known standard and assertion macros, expression-position
arguments, format-like macros, arbitrary macro DSLs and `macro_rules!` bodies (405
call texts, no oracle relation). A semantics-safe policy is then defined. No call is
invented from DSL tokens, and macros are not expanded.

State: open.

### 106 — Python candidate resolution

A child of issue 95 (decision 65, remediation R6). Of the Python golden's 1,114 oracle
relations, 603 are drawn correctly, 470 end as `candidate`, 36 are blocked and 5 are
drawn wrong. Most candidates are:

- member calls on a local value: `bp.route(…)` on a `Blueprint` the function made;
- package re-exports: `flask.make_response(…)` through `flask/__init__.py`.

Before any fix, all 470 are classified by the evidence that could resolve them:

- local-variable receiver;
- parameter receiver;
- factory or call-result receiver;
- attribute receiver;
- package re-export;
- import alias;
- inheritance or base method;
- decorator-produced binding;
- fixture or other untyped external value;
- other.

Each class is also marked by where its evidence lies:

- structurally recoverable;
- needs a compiler or precise provider;
- the oracle itself is weak or untyped.

Evidence fixes are then designed from the counts. 85% structural recall is not
promised from information the structure does not hold. What cannot be recovered
structurally is reported so that the driver can judge what G4's structural recall
floor assumes; the floor does not change here.

State: open.

### 105 — Rust candidate resolution

A child of issue 95 (decision 65, remediation R5), and probably the largest recall
workstream. Of the Rust golden's 4,061 oracle relations at structural call sites:

| | Relations |
|---|---|
| drawn correctly | 1,377 |
| end as `candidate` | 2,079 |
| blocked | 389 |
| drawn wrong | 190 |
| `external` | 17 |
| typed but not materialised | 9 |

Not drawing `exact` and `scoped` alone (A0.2's C3) would leave recall at 17.80%.

Before any fix, all 2,079 candidates are classified, with counts, by the evidence
that could resolve them:

- receiver local-variable type;
- receiver field type;
- receiver call-result type;
- associated or static path type;
- import or `use` binding;
- trait bound;
- impl relation;
- generic type parameter;
- method through `Deref`;
- method of an external type;
- `crate`/`self`/`super` path;
- re-export;
- macro-generated type or item;
- other.

What no structural evidence can resolve is marked as such, for the driver's later
judgement of G4's structural recall floor. Evidence-strengthening work is designed
from the counts; no fix is guessed before the classification.

State: open.

### 104 — Rust typed false positives

A child of issue 95 (decision 65, remediation R2). The Rust golden's `typed` band is
821/836 = 98.21% precise, above G1b's 97% floor. Its 15 false positives are each a
typed answer naming the wrong definition:

| Class | Sites | Example (`file:line`, call, wrong target) |
|---|---|---|
| module path ignored | 5 | `axum-macros/src/lib.rs:377` `from_request::expand` → `lib.rs`'s `expand`, not `from_request::expand` (also `debug_handler::expand`, `typed_path::expand`) |
| an external crate's function taken for an internal look-alike | 4 | `axum/src/extract/multipart.rs:76` `multer::parse_boundary` → `axum-extra`'s `parse_boundary`; `axum/src/extract/ws.rs:1261` `hyper::upgrade::on` → `routing::method_routing::on` |
| another type's associated function | 2 | `axum/src/extract/path/de.rs:265` `PathDeserializationError::custom` → `connect_info`'s `custom` |
| a same-named function of another crate | 2 | `axum/src/routing/strip_prefix.rs:191` `find_first_not_double` → `axum-macros`'s |
| a blanket trait impl | 1 | `axum-extra/src/extract/with_rejection.rs:196` `WithRejection::<…>::from_request` |
| a pattern binding answered as a call | 1 | `axum/src/extract/path/inner_path.rs:168` `params` in `Ok(Self(params))` |

Within this issue too:

- the one `import-guided` false positive: `axum-core/src/ext_traits/request.rs:355`
  `Body::from` is drawn to another `From` impl's `from`;
- the one false typed answer the blocklist hid: `axum/src/handler/service.rs:171`,
  `Handler::call`. Since issue 103 (R1) it is drawn, so the Rust golden's typed
  false positives are 16.
  - The engine answers this trait-qualified call through its short-name fallback,
    `lsp_short_name_unique`, reported as `lsp_typed`, with the crate's only free
    function named `call`.
  - `a_typed_answer_the_blocklist_hid_is_drawn_as_the_engine_gives_it`
    (`crates/pdx-core/tests/stages.rs`) reproduces it in the smallest repository
    and records today's answer; this issue changes it.

To investigate:

- the engine's evidence;
- path and import identity;
- the project-prefix translation;
- the registry's mapping of a typed answer to a definition;
- duplicate qualified names.

`TYPED_MIN_SCORE` is not moved blindly: true-positive coverage is kept and the
mis-mapping removed. Every location is in `bench/results/a0/mismatches.jsonl`.

**R2, 2026-10-14.** The 17 were classified before any change. None is the registry
mapping a right engine answer to a wrong definition, and none a duplicate qualified
name. Sixteen are wrong engine answers, and one an import resolution over an
incomplete set of definitions:

| Sites | Engine strategy | Where identity is lost | Layer | Fix | After R2 |
|---|---|---|---|---|---|
| 5: `axum-macros/src/lib.rs:377`, `426` `from_request::expand`; `582`, `638` `debug_handler::expand`; `665` `typed_path::expand` | `lsp_short_name_unique` | the Rust resolver's fallback (`rust_resolve_call_expression_inner`) keeps the path's last segment and drops its qualifier | engine | the fallback needs path evidence | `typed`, right target |
| 4: `axum/src/extract/multipart.rs:76`, `94` `multer::parse_boundary`; `axum/src/extract/ws.rs:1261`, `examples/http-proxy/src/main.rs:91` `hyper::upgrade::on` | `lsp_short_name_unique` | the same fallback: an external crate's path, answered with a namesake anywhere in the project | engine | path evidence; Stage 3's extern prelude | `external` |
| 2: `axum/src/extract/path/de.rs:265`, `296` `PathDeserializationError::custom` | `lsp_short_name_unique` | the type's path misses its definition (`path/mod.rs`), and the fallback takes a free test function `custom` | engine | path evidence | `candidate` |
| 2: `axum/src/routing/strip_prefix.rs:191`, `193` `find_first_not_double` | `lsp_short_name_unique` | a function declared in the caller's body is bound nowhere, and has no definition | engine and extraction | block-local functions bound; the call `LOCALLY_BOUND` | `unresolved` |
| 1: `axum/src/handler/service.rs:171` `Handler::call` | `lsp_short_name_unique` | the fallback takes the free function `call` for a trait's method | engine | path evidence | `blocked` |
| 1: `axum/src/extract/path/inner_path.rs:168` `params` | `lsp_callable_value_reference` | a `match` arm's pattern binds nothing, so `Ok(Self(params)) => …` passes the module's function | engine | arm patterns bound | not a call (`reference`, no answer) |
| 1: `axum-extra/src/extract/with_rejection.rs:196` `WithRejection::<…>::from_request` | `lsp_ufcs` | `Type::method` takes the type's own trait impl, whose bound the type does not meet; the call reaches a blanket impl | engine | not fixed: issue 109 | `typed`, wrong |
| 1 `import-guided`: `axum-core/src/ext_traits/request.rs:355` `Body::from` | none | the impl the call reaches is written by `body_from_impl!`, which no extraction represents; import-guided finds the one `from` there is | extraction | not fixed: issue 110 | `import-guided`, wrong |

What changed (patch `0028-rust-path-evidence.patch`; Stage 3; specification 4.5):

- **Path evidence, not name uniqueness.** The fallback answered a call whose path named
  no registered function with the project's one function of the path's last name.
  It now answers only with the one function of that name in the module the call's
  path names, or a child module it re-exports from. For a bare name, the path a
  `use` or a glob imports it by stands in. The path is read as Rust reads it:
  `crate`, `self` and `super`; a head the file imports; a crate's name, `-` as `_`;
  an inline module of the file, which qualified names leave out. The member-crate
  route (`lsp_cross_crate`) applies the same check. A bare name nothing imports gets
  no answer. The label stays the reference's, `lsp_short_name_unique`, so both
  typed-resolution differentials compare those answers row for row; the interface is
  unchanged. The engine differential (`make engine-differential`, all 32 corpora, the
  Rust fixtures and the axum cluster among them) is identical: no row changes, so no
  divergence needs ratifying.
- **Locals.** A `match` arm's pattern binds its names in the arm. A function a block
  declares is bound in the block, and extraction marks a bare call of it
  `callee_is_locally_bound` (the walk's table Python's parameters use), which Stage 3
  settles `unresolved`.
- **External crates.** A Rust path whose first name nothing in the file binds and the
  extern prelude binds to an external crate is `external` (Stage 3, step 4): a crate of
  the standard distribution, or a dependency of the caller's crate with no repository
  path, classified as a `use` of it is (`resolve::imports::rust_external_crates`). Only
  a path: a method called on a value it names may be the repository's trait's. 4.5 is
  amended to say so, in the name-provenance rule and step 4.

Nothing here moves `TYPED_MIN_SCORE`, adds a threshold, names a crate, package or
golden name, or reads the oracle. `ENGINE_VERSION` stayed 7 (patch README): engine 7 had
not been merged or released. An extraction cached before this patch by a build of this
branch is stale (the cache key is the engine version), so this machine's goldens were
re-recorded with an empty cache; CI caches none. Issue 112 corrected this: patch 0028
is engine 8.

Tests (`crates/pdx-core/tests/rust_targets.rs`, through typed resolution itself):

| Test | Class |
|---|---|
| `e1_a_module_qualified_call_reaches_the_module_its_path_names` | E1: `a::expand`, `b::expand` (a directory module), the root's `expand` |
| `e1_paths_through_crate_names_inline_modules_and_imports_are_path_evidence` | E1: a crate's name, an inline module, an imported module, a re-exported bare import |
| `e2_an_external_crates_function_is_external_not_a_namesake` | E2: `multer::…`, `hyper::upgrade::on` with namesakes |
| `e3_one_types_associated_function_is_never_anothers_namesake` | E3: a type's external trait impl against a free namesake |
| `e4_a_local_function_is_not_another_crates_namesake` | E4: a block-local function against another crate's |
| `e4_a_bare_name_unique_by_name_alone_has_no_typed_answer` | E4: uniqueness alone is no answer |
| `e5_a_trait_qualified_call_is_not_a_free_function_of_its_name` | E5: `Handler::respond` against free namesakes in another module and beside the trait |
| `e6_a_call_a_blanket_impl_answers_is_recorded_as_the_engine_gives_it` | E6: today's wrong answer, issue 109 |
| `e7_a_name_a_match_arm_binds_is_not_a_function_of_that_name` | E7: an arm's binding, a call, a callable passed |
| `e8_an_answer_whose_name_two_definitions_share_is_withheld` | E8: two `From` impls, one qualified name: `candidate` |
| `e9_an_import_guided_from_reaches_the_imported_types_impl` | E9: `Body::from` against another type's `from`; the macro case, issue 110 |
| `e10_typed_answers_with_their_evidence_stay_typed` | E10: free, associated, method, module-qualified and trait calls |

`crates/pdx-bench/tests/issue_104_sites.rs` holds the 17 against the committed axum
golden. `a_typed_answer_the_blocklist_hid_is_drawn_as_the_engine_gives_it` now finds
`Handler::call` blocked with no engine answer.

Eight mutations are each killed:

| Mutation | Killed by |
|---|---|
| a bare name promoted by uniqueness alone | E4 |
| the qualifier ignored | E1, E2, E3 |
| an external crate's path resolved by name | E2 |
| a type's path dropped from the qualifier | E3 |
| a trait's path read as its module's | E5 |
| the first of two definitions sharing a name chosen | E8 |
| an import-guided member taken from any type of the module | E5, E9 |
| an arm's pattern left unbound | E7 |

**A0 after R2** (same pins, tools, `a0-match-1`, `a0-metrics-1`; baseline post-R1):

| Golden | | Post-R1 | Post-R2 |
|---|---|---|---|
| Rust | `typed` | 852 / 868 = 98.16% | 843 / 844 = 99.88% |
| Rust | `import-guided` | 32 / 33 | 24 / 25 |
| Rust | `exact` | 509 / 1,168 = 43.58% | 525 / 1,150 = 45.65% |
| Rust | `scoped` | 155 / 492 = 31.50% | 155 / 477 = 32.49% |
| Rust | union | 1,611 / 2,624 = 61.39% | 1,610 / 2,559 = 62.92% |
| Rust | recall | 1,408 / 4,061 = 34.67% | 1,407 / 4,052 = 34.72% |
| PDX | `typed` | 5,720 / 5,765 = 99.22% | 5,870 / 5,870 = 100.00% |
| PDX | recall | 6,473 / 7,844 = 82.52% | 6,560 / 7,816 = 83.93% |

Python's numbers are identical. Of the 17, 15 are no longer drawn wrong; no new wrong
trusted edge appears in either Rust corpus.

The Rust golden's correct `typed` edges go from 852 to 843:

- 16 bare names imported through `use` lists the resolver misreads (issue 111) are
  `exact` now, same targets;
- 6 are lost to `candidate`: `on` and `on_service` (`axum-extra/src/routing/mod.rs:458`,
  `axum-extra/src/routing/resource.rs:120`, `axum/src/form.rs:256`,
  `axum/src/routing/tests/mod.rs:145`, `186`, `305`), imported through nested or
  multi-line `use` lists the resolver misreads (issue 111);
- 8 `import-guided` edges become `typed`, and the 5 module-qualified calls are right.

Nine unmaterialised typed answers (`path_for_nested_route` in `assert_eq!`) are lost
for the same reason, and 38 in PDX itself. The external rule settles 340 unresolved
and 110 candidate sites of the Rust golden, and removes 34 `exact` and 15 `scoped`
false positives; the oracle confirms every site it made external that it has an
answer for.

The axum golden loses 149 drawn edges and gains 40, node counts unchanged. Every
`CALLS` and `CALL_REFERENCE` change is a site above: 64 wrong edges removed, 29
re-drawn to the same or the right target, 6 lost. Every `TESTS` change follows its
call.

The closure conditions are not all met, so this issue stays open:

1. Every typed false positive fixed or withheld: no, the blanket impl (issue 109).
2. The `import-guided` one fixed or withheld: no, the macro-written impl (issue 110).
3. No new class of trusted false positive: met.
4. Typed coverage not materially degraded: met, with the 6 losses above (issue 111).
5. A0 confirms: met.
6. The fixtures pass: met.
7. Validation and CI: see the R2 commit.

State: open.

### 103 — Typed before the blocklist (specification change)

A child of issue 95 (decision 65, remediation R1). 4.5 Stage 3 settles a blocklisted
name first. "Names on the blocklist are `blocked` before any stage runs", and the
precedence list's first step is "`blocked`, even with a qualifying typed answer". So
a valid typed answer for a call named `get`, `run`, `parse` or `update` is
discarded.

The blocklist exists to stop weak guessing by a generic name, not to override
semantic evidence. A0 measured the typed answers it suppresses:

| Corpus | Suppressed | Correct | False | No verdict | Relations recovered by C4 |
|---|---|---|---|---|---|
| Python golden | 4 | 4 | 0 | 0 | 3 |
| Rust golden | 32 | 31 | 1 | 0 | 31 |
| PDX itself | 412 | 409 | 0 | 3 | 409 |

**Change, approved by decision 65.**

- A valid unique typed answer is `typed`. Valid means `from_engine` says typed and
  its target is exactly one definition of the repository, as Stage 3 maps a typed
  answer.
- Otherwise a blocklisted name is `blocked`.
- An engine hint never passes the blocklist.

4.5's two sentences quoted above are amended with the implementation (R1). 4.5 is
not changed by A0.3.

**Acceptance:**

- the correct typed generic-name calls are recovered;
- name-based generic false edges stay suppressed;
- the one false Rust typed answer (`Handler::call`, issue 104) is tested on its own,
  not hidden in an aggregate;
- A0 is rerun.

**Resolution (R1, 2026-10-14).** `Resolver::site`
(`crates/pdx-core/src/resolve/stages.rs`) now settles a site in this order:

1. a qualifying unique typed answer is `typed`, whatever the callee's name. That is
   Stage 3's existing `typed_target`: `from_engine` says typed and the answer's file
   and qualified name are exactly one definition;
2. otherwise a blocklisted name is `blocked`;
3. the engine's answer naming a target in no file of the project is `external`;
4. bindings and the name-based stages, unchanged.

The blocklist and the external answer keep their relative order. Nothing else moved:

- the unconfirmed sites (typed-only, references, engine sites) still need a typed or
  external answer;
- `exact` and `scoped` are still drawn (issue 102).

4.5 Stage 3 is amended: "A qualifying unique typed answer is accepted first. If no
such typed answer exists, a blocklisted generic name is blocked before any
name-based resolver stage runs." The steps after it are renumbered, and its
cross-reference for unconfirmed sites follows.

Tests in `crates/pdx-core/tests/stages.rs`:

- `blocklist_precedes_all`, P2-06's named test, updated: all but a qualifying typed
  answer;
- `a_qualifying_typed_answer_passes_the_blocklist`: `get`, `run`, `create`,
  `update`, `parse`, with a resolution identical to a name off the list;
- `only_a_qualifying_typed_answer_passes_the_blocklist`: low and out-of-range
  scores, candidate counts 0 and 2, every other strategy;
- `an_ambiguous_typed_target_does_not_pass_the_blocklist`;
- `blocklisted_sites_that_are_no_call_need_a_qualifying_answer`;
- `issue_103_leaves_exact_and_scoped_drawn`;
- `a_typed_answer_the_blocklist_hid_is_drawn_as_the_engine_gives_it`: the
  `Handler::call` class through typed resolution itself;
- the stage matrix's `blocked` row, now with an engine hint.

Four mutations are each killed: any answer passes the blocklist; the blocklist
before typed; a weak `lsp_typed` answer passes; an ambiguous mapping passes.

The benchmark fixture's 288 `h.run(z)` calls now resolve as `typed`. Nine goldens
gain 303 tuples, all typed edges at blocklisted names (221 `CALLS`, 5
`CALL_REFERENCE`, 77 derived `TESTS`), with none removed and node counts unchanged.

**A0 rerun** (same pins, tools and rules; `bench/results/a0/`, baseline `71bec72`).
Only blocked → typed transitions occur:

| | Python golden | Rust golden | PDX itself |
|---|---|---|---|
| Sites changed | 4 | 32 | 412 |
| Of which correct | 4 | 31 | 409 |
| Of which false | 0 | 1 (`Handler::call`) | 0 |
| Of which without a verdict | 0 | 0 | 3 |
| `typed` | 314/314 | 852/868 = 98.16% | 99.22% |
| Union | 624/657 = 94.98% | 1,611/2,624 = 61.39% | 83.74% |
| Recall | 606/1,114 = 54.40% | 1,408/4,061 = 34.67% | 82.52% |
| Typed answers suppressed by the blocklist | 0 | 0 | 0 |

These are A0.2's C4 prediction exactly. No version constant changes.

State: resolved.

### 102 — `exact` and `scoped` are not drawn on uniqueness alone (specification change)

A child of issue 95 (decision 65, remediation R3). 4.2.2 draws two bands:

- `exact`: the callee's short name has exactly one definition in the repository;
- `scoped`: exactly one definition in the same module or package.

Both assume that a unique short name identifies the definition a call reaches. A0's
measurements invalidate that assumption:

| Band | Python golden | Rust golden | PDX itself |
|---|---|---|---|
| `exact` | 284/310 = 91.61% | 509/1,168 = 43.58% | 548/1,452 = 37.7% |
| `scoped` | 1/8 = 12.50% | 155/492 = 31.50% | 47/357 = 13.2% |

G4's floor for the union of drawn bands is 95%. Most false positives are an external
target taken for an internal one, a self-edge, or a constructor collision
(`bench/results/a0/`, `exact_scoped_false_positives`).

**Change, approved by decision 65.**

- `exact` and `scoped` remain ordered evidence bands. Their order, names and
  history are kept, and no band is renumbered.
- Under their current semantics, neither can by itself produce a drawn edge. They
  narrow candidates and may stay candidate evidence.
- A stronger variant may be drawn only once the harness (4.13) measures it at G4's
  floor.
- The principle: PDX draws a `CALLS` edge only from positive target evidence, never
  merely because every other same-name target disappeared.

**Implementation (R3):**

- their evidence is kept for candidate ranking and explanation, coverage, trust
  diagnostics and calibration;
- candidate ordering stays deterministic;
- no schema or version change, unless the existing candidate and evidence rows
  truly cannot express them;
- the affected tests and goldens are updated deliberately;
- 4.2.2's table is amended;
- A0 is rerun.

Not drawing them alone is not the remedy. With both off, recall falls to 28.55%
(Python) and 17.80% (Rust); recall is recovered by issues 105 to 108. R3 may still
land first: correctness comes first.

This is a correctness change, not a threshold. 4.2.2 records the approved change in
a note; its table changes with the implementation.

State: open.

### 101 — Provenance records disagree about the Rust resolver's derivation

Raised by an independent review before the oracle work. `bench/references.lock` records
the engine reference (`engine-ref`) with the mode "vendor+refactor, and reference for
the Rust ports" and `lands_in = "engine/, crates/pdx-core/src/resolve/"`: it says the
reference also informed the Rust resolver. `THIRD_PARTY_NOTICES.md`'s section
"Algorithms and designs consulted", which lists re-implementations written from scratch
with the reference consulted, says "No entries yet". (Its first "Vendored components"
section also still says "No entries yet", above the generated section that lists the
vendored engine.)

Both cannot be complete as written. Which one is wrong is a question of fact, the actual
derivation history of `crates/pdx-core/src/resolve/`, and is settled from that history.
No attribution is removed to make the records agree, and no legal conclusion is drawn
here.

State: open.

### 100 — Supply-chain hardening has gaps

Raised by an independent review.

- **cargo-vet.** The store holds 142 exemptions (`supply-chain/config.toml`) and 0
  audits (`supply-chain/audits.toml`'s `[audits]` is empty). Every crate is accepted by
  an exemption, none by a recorded audit; `make vet` passing says no more than that.
  The two are kept distinct: exemptions are not audits.
- **cargo deny.** `scripts/licence-scan.sh` runs `cargo deny check licenses` alone.
  `deny.toml` configures an advisory policy (`[advisories]`, yanked crates denied), but
  no workflow runs it as a check.
- **Workflow actions.** Every step uses a mutable major tag (`actions/checkout@v4`,
  `actions/cache@v4`, `actions/setup-node@v4`, `actions/upload-artifact@v4`) rather than
  an immutable commit.

Not fixed in this round.

State: open.

### 99 — One local index can block the next

Raised by an independent review. `pdx index` refuses a working tree `git status` does
not report clean, and an untracked `.pdx/` counts (decision 38). It writes its segment
to `.pdx/segment.db` unless `--out` names another path, never overwrites a segment, and
does not make `.pdx/` ignored (its refusal suggests adding `.pdx/` to `.gitignore`). In
a repository whose ignore files do not already cover `.pdx/`, a first successful index
leaves the tree dirty, and the next run is refused.

Not fixed in this round.

State: open.

### 98 — Source below a build-named directory is hidden from coverage

Raised by an independent review. Discovery never enters a directory named `build`,
`target`, `dist` or `vendor`, at any depth (`index::discover::HARD_EXCLUDES`; `vendor`
unless `[discover] include_vendor = true`), with `.git` and `node_modules`. Source
legitimately below such a component, a package or module that happens to have one of
those names, is therefore not discovered, not indexed and not represented in coverage:
no row records that it was left out, so nothing tells a reader that it exists.

The exclusion is 4.5's ("Hard-coded excludes"), so changing it is a specification
change. Not fixed in this round.

State: open.

### 97 — Natural-language strings can mint a table contract

Raised by an independent review. The SQL consumer reader (`contracts::tables`,
`sql_access`) takes a call's string argument as SQL when its first word is `SELECT`,
`INSERT`, `UPDATE`, `DELETE`, `WITH` or `MERGE`, compared in any case, and then reads
every `FROM`, `JOIN`, `INTO` and `UPDATE` it finds. A user-interface string such as

```
t("Select an item from the list")
```

begins with `Select`; `from the` reads as a `FROM` clause; and the call consumes a
`Table` named `the`, a contract nothing provides and no reader should see.

**Reproduced (A0, 2026-10-14), not fixed.** `contracts::tables::sql_access`, called
in a local test that was never committed:

| Text | Tables read |
|---|---|
| `"Select an item from the list"` | `("the", Read)` |
| `"SELECT id FROM users"` (control) | `("users", Read)` |

Decision 65 sequences this as remediation R10, an early trust fix after A0.3 rather
than release-time work. It is authorised separately.

Not fixed in this round.

State: open.

### 96 — Secret masking has false negatives and one destructive mask

Raised by an independent review of content secret normalisation (`pdx_core::secrets`,
detector 4). Its detectors are a PEM private key, a bearer token, a credential
assignment, a URI password and a cloud access-key id. The classes identified:

1. **A standalone JWT-shaped credential** (three base64url segments, the first a JSON
   header), outside an assignment or a bearer header: no detector matches it.
2. **A standalone GitHub personal-access-token-shaped credential** (the documented
   personal-token prefix and its fixed-length body), outside an assignment: no detector
   matches it.
3. **A semicolon-separated key-value connection string** inside a string literal of
   source code, its password a key of its own (`…;Password=…;`): the value is neither
   quoted nor in a language whose bare values are literals, so it is not masked.
4. **A destructive mask.** In `"password=" + decrypt(pw) + "..."` the credential
   assignment detector reads the closing quote of `"password="` as the start of a quoted
   value and masks across ` + decrypt(pw) + `, which is code, not a secret.

No real credential may be committed: every fixture's value is synthetic and assembled at
run time. A resolution must prove that secrets are masked, that non-secret executable
syntax is preserved, that byte length is preserved and that carriage returns and line
feeds are preserved. A change to what a detector matches bumps
`SECRET_DETECTOR_VERSION`, and coverage stays monotonic (issue 73).

**Reproduced (A0, 2026-10-14), not fixed.** `secrets::normalise_in_place` was run on
fixtures whose values were assembled at run time, in a local test that was never
committed.

| Class | Value masked | Length | CR and LF |
|---|---|---|---|
| 1. Python call argument | no | kept | kept |
| 2. Python call argument | no | kept | kept |
| 3. C# `"Server=…;User Id=…;Password=…;"` string, ending CRLF | no | kept | kept |
| 4. Python `url = "password=" + decrypt(pw) + "&mode=x"` | — | kept | kept |

In class 4, ` + decrypt(pw) + ` became 17 `X` bytes: the code between the two
literals is masked.

Decision 65 sequences this as remediation R9, an early trust fix after A0.3 rather
than release-time work. It is authorised separately.

Not fixed in this round.

State: open.

### 95 — Structural accuracy and trust

An independent adversarial review raised low precision and recall of the drawn
structural bands, before P5 and P6's oracle work. Its preliminary percentages are not
accepted as ground truth: they were obtained by ad-hoc matching, not by 4.13.1's
measures against a semantic oracle.

The driver's inspection of the branch at `1eb801d` confirmed that these mechanisms are
present (`crates/pdx-core/src/resolve/stages.rs` unless named):

1. **The blocklist before a typed answer.** `Resolver::site` settles a site whose
   callee's short name is on the generic-name blocklist as `blocked` (step 1) before
   it looks at the engine's answer (step 2), so a valid typed answer for such a site is
   never drawn. 4.5 orders it so; whether that is right is what A0 measures.
2. **`exact` from repository-wide short-name uniqueness.** `exact` holds when the
   callee's short name has exactly one callable definition in the repository (of the
   caller's language family), whatever the receiver.
3. **`scoped` from module or package short-name uniqueness.** `scoped` holds when
   exactly one remaining candidate is in the caller's module.
4. **Rust's unknown receivers.** `exact` and `scoped` are skipped for a call whose
   receiver the extractor could not tie to anything (`UNRESOLVED_MEMBER`, from the
   extraction's `is_method`), but the extractor sets it for Python, JavaScript,
   TypeScript and Perl only (`engine/src/extract_calls.c`). A Rust method call on a
   value of unproven type, or a path call on an unproven type, falls through to
   `exact` and `scoped`.
5. **Overloads.** Definitions sharing one qualified name (overloads, and a trait's
   method implemented for several types) cannot be told apart by signature: a typed
   answer naming that qualified name maps to several definitions and is not drawn, and
   the name stages see the overloads as distinct candidates of one name.
6. **Rust macro token trees.** A call written only inside a macro invocation's token
   tree (`assert_eq!(a.f(), 1)`, `vec![g()]`) is not a parsed `call_expression`, so it
   is not an extraction call site; only the macro invocation is. Typed resolution walks
   some token trees and may report sites past the extraction's calls, which are drawn
   only with a typed answer.

**Resolution.** No architectural change before evidence. A0.1 measures structural
accuracy with 4.13.1's measures against a semantic oracle on the Python and Rust
goldens (the PDX repository diagnostic only), and A0.2 re-scores the same dataset under
counterfactual policies (decision 64, gate G1b). What is changed, if anything, is A0.3,
the driver's decision.

**A0.1 and A0.2 (2026-10-14).** The results are in `bench/results/a0/`, with
`methodology.md`, measured at `e746b1e`. The oracles are `scip-python` 0.6.6 and
`rust-analyzer` 1.98.1 (SCIP), and 4.13.1's measures are applied exactly. The oracle
mapping passes all 25 sanity cases (`bench/a0/sanity/`).

| Band | Python golden | Rust golden | PDX itself (diagnostic) |
|---|---|---|---|
| `typed` | 310 / 310 = 100.00% | 821 / 836 = 98.21% | 5,311 / 5,356 = 99.16% |
| `import-guided` | 11 / 11 | 32 / 33 = 96.97% | 115 / 119 = 96.64% |
| `inheritance-guided` | 14 / 14 | 63 / 63 | 74 / 74 |
| `exact` | 284 / 310 = 91.61% | 509 / 1,168 = 43.58% | 548 / 1,452 = 37.74% |
| `scoped` | 1 / 8 = 12.50% | 155 / 492 = 31.50% | 47 / 357 = 13.17% |
| union | 620 / 653 = 94.95% | 1,580 / 2,592 = 60.96% | 6,095 / 7,358 = 82.84% |
| oracle-verifiable fraction | 653 / 656 = 99.54% | 2,592 / 2,640 = 98.18% | 7,358 / 7,369 = 99.85% |
| recall | 603 / 1,114 = 54.13% | 1,377 / 4,061 = 33.91% | 6,064 / 7,844 = 77.31% |

What the evidence says of each mechanism above:

1. **The blocklist before a typed answer.** The blocklist suppresses valid typed
   answers in all three corpora: 4 in the Python golden (all 4 correct), 32 in the
   Rust golden (31 correct, 1 false) and 412 in PDX (409 correct, 3 without a
   verdict). Accepting them first (A0.2's C4) recovers 3, 31 and 409 relations, and
   adds 1 false positive, in the Rust golden.
2. **`exact`.** Its false positives are mostly an external target taken for an
   internal one: 16 of 26 in the Python golden and 481 of 659 in the Rust golden
   (`x.field(…)` on `fmt::DebugTuple` drawn to a repository `field`).
3. **`scoped`.** Its false positives are mostly self-edges and constructor
   collisions: 7 of 7 in the Python golden; in the Rust golden 137 self-edges and 84
   constructor collisions of 337 (`TestClient::new(app)` drawn to the module's own
   `new`).
4. **Rust receivers.**
   - `exact` and `scoped` are 345 / 1,017 = 33.9% precise on a value receiver, and
     48 / 228 = 21.1% on a path receiver nothing binds.
   - Without them (C5) the Rust golden's union is 1,187 / 1,347 = 88.1% precise and
     its recall 24.2%.
5. **Overloads.**
   - In the Rust golden, 89 relations target a definition sharing its qualified name.
     83 have a site and 52 are drawn: 49 to the right definition and 3 to a wrong one.
     2 typed answers are withheld because their name maps to several definitions.
   - scip-python cannot tell `@overload` stubs from their implementation, so in the
     Python golden those 28 calls have no verdict.
6. **Rust macro token trees.**
   - The Rust golden has 1,696 call texts inside macro invocations' token trees. 437
     are oracle relations; 338 of those have no structural site.
   - 90 calls inside `assert_eq!` and `format!` arguments are extraction sites and
     are drawn correctly.
   - 9 are typed by the engine's own token-tree walk, but Stage 4 has no site to
     draw them on.

Neither `exact` nor `scoped` alone explains the Rust golden. With both removed (C3),
its union is 916 / 932 = 98.3% precise, but its recall falls to 723 / 4,061 = 17.8%.
Recall loss is mostly resolution, not extraction:

| Golden | Resolution loss | Extraction loss | Largest resolution bucket |
|---|---|---|---|
| Python | 511 | 0 | `candidate`, 470 |
| Rust | 2,684 | 394 | `candidate`, 2,079 |

The Rust golden's extraction loss is 338 token-tree calls and 56 turbofish calls,
such as `f::<T>()`, which have no site.

**A0.3, the driver's decision (decision 65, 2026-10-14).** The evidence above is
accepted.

- **Kept drawn:** `typed`, `import-guided` and `inheritance-guided`. Their false
  positives are defects (issue 104).
- **Not evidence enough to draw:** the current semantics of `exact` and `scoped`.
  They stay ordered bands and candidate evidence (issue 102, a specification change
  to 4.2.2).
- **Principle:** PDX draws a `CALLS` edge only from positive target evidence, never
  merely because every other same-name target disappeared.
- **Blocklist:** a valid typed answer is considered before it (issue 103, a
  specification change to 4.5 Stage 3).

Demotion alone is not the remedy. Without `exact` and `scoped`:

| Golden | Precision | Recall |
|---|---|---|
| Python | 335/335 | 28.55% |
| Rust | 916/932 = 98.28% | 17.80% |

Recall is a first-class blocker: remediation must recover positive evidence.

This issue stays the umbrella and is split into its children:

| Issue | Subject |
|---|---|
| 102 | `exact` and `scoped` not drawn on uniqueness alone |
| 103 | typed before the blocklist |
| 104 | Rust typed false positives |
| 105 | Rust candidate resolution |
| 106 | Python candidate resolution |
| 107 | Rust site coverage: 107A ordinary syntax, 107B macro token trees |
| 108 | target identity among definitions sharing a qualified name |
| 109 | a trait impl chosen without its bounds (found by 104) |
| 110 | items a `macro_rules!` invocation writes (found by 104) |
| 111 | nested `use` lists the Rust resolver misreads (found by 104) |
| 113 | Rust paths through `crate::`, re-exports and `index.rs`, and an imported type's impls (found by 111) |
| 114 | Rust `use` without lexical scope (found by 111) |

The preferred order, each step authorised separately by the driver:

| Step | Issue |
|---|---|
| R1 | 103 |
| R2 | 104 |
| R3 | 102 |
| R4 | 107A |
| R5 | 105 |
| R6 | 106 |
| R7 | 108 |
| R8 | 107B |
| R9 | 96 |
| R10 | 97 |

R3 may land before recall is recovered, though G1b's recall then falls further. A
graph with missing edges is incomplete; a graph with invented edges is
untrustworthy. After every remediation that changes resolution or extraction, A0 is
rerun and reported. No change may raise recall by drawing low-confidence edges
again.

This issue closes only when G1b's floors hold. G4 is not weakened.

**R1 (2026-10-14).** Issue 103 is resolved: a qualifying typed answer comes before
the blocklist.

| Golden | `typed` | Union | Recall |
|---|---|---|---|
| Python | 314/314 | 94.98% | 54.40% |
| Rust | 98.16% | 61.39% | 34.67% |

PDX itself is diagnostic: `typed` 99.22%, recall 82.52%. Four of G1b's six floors
still fail. R2 (issue 104) is next, when the driver authorises it.

**R2 (2026-10-14).** Issue 104 stays open: Rust typed answers need path evidence, and
15 of the Rust golden's 17 trusted false positives are no longer drawn wrong. It found
three more children: issues 109 (a blanket impl), 110 (macro-written items) and 111
(`use` lists the Rust resolver misreads).

| Golden | `typed` | Union | Recall |
|---|---|---|---|
| Python | 314/314 | 94.98% | 54.40% |
| Rust | 843/844 = 99.88% | 62.92% | 34.72% |

PDX itself: `typed` 100.00%, recall 83.93%. Four of G1b's six floors still fail. R3
(issue 102) is next, when the driver authorises it.

**Issue 111 (2026-10-14).** Resolved: Rust `use` declarations are read from the syntax
tree (patch 0029, engine 9), and the typed answers R2 lost to `use` lists the resolver
misread are back. It found two more children: issues 113 (paths through `crate::`,
re-exports and `index.rs`, and an imported type's impls) and 114 (`use` without lexical
scope). The engine differential holds one ratified divergence for it (decision 66).

| Golden | `typed` | Union | Recall |
|---|---|---|---|
| Python | 314/314 | 94.98% | 54.40% |
| Rust | 859/860 = 99.88% | 62.97% | 34.74% |

PDX itself: `typed` 100.00%, recall 83.94%. Four of G1b's six floors still fail. Issue
109 is next, then 110, then R3 (issue 102), each when the driver authorises it.

State: open.

### 94 — Python's typed resolution is quadratic in a file's imports

Found by issue 93's battery of wide shapes, with patch 0026 in force. Before resolving a
module's calls, Python's per-file resolver replays each import statement in source
order (`py_replay_import_statement`), and for each imported name it scans every import
of the file for one with that local name (`py_replay_import_local`). That is a lookup by
key, not a step through a node, so it is issue 92's class, and none of issue 92's
indexes covers this loop. A file holding only `from mN import aN` lines, CPU seconds:

| Phase | 0.25 MiB | 0.5 MiB | 1 MiB | 2 MiB |
|---|---|---|---|---|
| per-file typed resolution | 0.14 | 0.47 | 1.77 | 6.73 |
| the project's typed-resolution run | 0.18 | 0.58 | 2.08 | 7.66 |

Parse and navigation stay linear (0.17 s and the counters doubling). The same at
`5915669`. Under an isolated worker's 120 s at 2 MiB, but quadratic.

**The driver's decision** (decision 63): fix it by indexing. No import is limited, no
replay skipped, no work quota added and no timeout changed, and it is not part of issue 93.

**The scans.** The resolver registers a file's imports from extraction's import list, in
its order (the source's), before the file is processed (`py_lsp_add_import`), as three
arrays: the local name each binds (its alias, or the name it imports), its module's
qualified name and its kind. The local names are never rewritten afterwards. Three
lookups by local name read every registered import, in that order:

- Replay (`py_replay_import_local`). The module's top-level statements are executed in
  source order, and each import statement replays each name it binds; a wildcard
  statement looks nothing up, but resets the module bindings a wildcard may change. The
  name is first bound to unknown; with imports registered but no record of those consumed
  (its memory refused), it stays unknown. Otherwise every registered import is examined
  and skipped when it is consumed, has no local name or module, or its local name is not
  exactly the name (`strcmp`). The statement classifies each remaining one from its own
  syntax and the import's module (`py_import_kind_from_statement`, a from-import's target
  canonicalised); only `import X`, `import X as Y` and `from X import Y` replay. The
  first replayable import is chosen; a later replayable one with another target or kind
  is a conflict, and a conflict or no choice leaves the name unknown. The choice takes
  its canonical target and kind, is consumed (a later statement binding the same name
  takes the next import) and binds the name. An alias is the local name itself; the
  module and the statement's form are conditions on the entries a name matches, never
  part of the match.
- An imported reference's exact candidate (`py_exact_imported_reference_candidate`): the
  target every from-binding import of the name agrees on, or none.
- A call's alias check: whether a from-binding import of the name with that target exists.

Each lookup examined every import, and replay looks up once per name a statement binds,
so N imports examined N² of them.

**The index.** One index of the registered imports by local name
(`PyLSPContext::imports_by_local`), issue 92's `PDXENameIndex` (`lsp/name_index.h`, as
Perl's @ISA and Rust's macro rules use it): keyed by exactly the local name the scans
compared, each name's chain in registration order, and brought up to date with the
arrays before each lookup by placing the imports added since. All three lookups read it
and apply the scans' every other condition to the imports a chain gives, so each meets
the scan's matching imports in the scan's order: the import a statement takes, the
conflicts and the imports consumed are the scan's, and no answer depends on hash order.
Issue 92's index of import items by local name (`PyImportKeys`) is not reused: it holds
the module's import syntax for classification (a plain import under each dotted prefix of
its name, by source range), is discarded once the imports are classified, and names no
registered import. The registered arrays had no index.

**Allocation.** An index that cannot get its memory finds nothing from then on: replay
leaves the name unknown, as for a name no registered import binds, and the candidate and
the alias check find none. The failure is lost work, counted once; it never scans.

**Work.** Counted in test builds (`lookup_counts.h`: registered imports examined, name
index probes), extraction and the project's run together:

| Imports | 1,000 | 2,000 | 4,000 | 8,000 | 16,000 |
|---|---|---|---|---|---|
| imports, half aliased, each called from a function: imports examined | 2,500 | 5,000 | 10,000 | 20,000 | 40,000 |
| index probes | 9,344 | 19,118 | 36,682 | 79,836 | 151,192 |
| `from mN import aN` lines: imports examined | 2,000 | 4,000 | 8,000 | 16,000 | 32,000 |
| index probes | 8,158 | 16,352 | 31,402 | 68,760 | 145,954 |

The scans examined 562,750 imports at 500 and 9,001,000 at 2,000. The issue's file, CPU
seconds of one extraction and one project run, single-threaded, at `735aacf` and now:

| | 0.25 MiB | 0.5 MiB | 1 MiB | 2 MiB |
|---|---|---|---|---|
| imports | 10,937 | 21,019 | 41,184 | 81,356 |
| per-file typed resolution, before | 0.139 | 0.469 | 1.768 | 6.731 |
| per-file typed resolution, now | 0.019 | 0.037 | 0.068 | 0.154 |
| the project's run, before | 0.177 | 0.581 | 2.077 | 7.656 |
| the project's run, now | 0.039 | 0.077 | 0.144 | 0.306 |
| extraction in all, before | 0.238 | 0.688 | 2.296 | 8.150 |
| extraction in all, now | 0.104 | 0.206 | 0.395 | 0.816 |
| imports examined, each pass, by the scans | 119,617,969 | 441,798,361 | 41,184² | 81,356² |
| imports examined, each pass, now | 10,937 | 21,019 | 41,184 | 81,356 |
| index probes, each pass | 51,377 | 111,870 | 222,957 | 419,308 |

(The scans at 1 and 2 MiB are the square, not run.) The 2 MiB file took about 16 s in
all and takes 1.1 s.

**Tests.** `engine/tests/typed_lookup.c`:

- Eleven fixtures of imports a lookup by local name must tell apart: the same import
  twice; one name from two modules; one alias for two members of two modules, and of one;
  an alias for a module and for a member; an import, then a wildcard; a wildcard, then an
  import; wildcards repeated; an import shadowed by a later one; relative, dotted and
  member imports of one module; one local name for a module and a member. Each calls at
  module level and from a function, and resolves to the same facts, answers (target,
  file, score, strategy, engine strategy, candidates, site), health and decision trace
  with the index as with the scans. Six get typed answers (an alias of two members takes
  the later, a wildcard then an import takes the import); the other five fail closed,
  alike. The test requires at least five, so that no comparison is of two empty sets.
- The decision trace (test builds, `pdxe_lookup_trace_read`) mixes in, in order, each
  replay's chosen import, conflict and kind, so implementations that agree on every
  answer but take different imports differ.
- 300 imports, half aliased, called from a function: 300 answers, the same both ways.
- The imports examined double with the file at 1,000 to 16,000 in both shapes, and the
  scans' grow with its square. With the resolver's indexes refused, no import is
  examined one by one, the refusal is lost work, and the work stays linear.

**Mutations.** Each compiled, was killed by failing assertions and was restored byte for
byte (sha256). A, every lookup scanning again: the imports examined grow fourfold per
doubling (2,250,500 to 9,001,000), 1,250,500 examined with the index refused. B, the
name index's chains newest first: the same import twice takes the other import (the
trace differs), as does Rust's one macro name in two modules. C, the imports keyed by
module instead of local name: 18 failures, among them every alias fixture (a target lost:
`pkg.two.f`, `pkg.one.h`), the wildcard and relative fixtures, the resolve fixtures and
both guards against empty answers. D, a refused index falling back to the scan: 1,250,500
imports examined one by one, the refused work quadratic.

**Census.** All 104,730 census files, as 10,928 directory projects, resolve to the same
answers (target, file, score, strategy, engine strategy, candidates, site) and health as
at `735aacf`, and extract to the same public facts, texts, qualified names, surfaces and
lost counts. The goldens' segments are unchanged.

**Issue 93.** With the index in force, `wide_nodes` passes, with collections and with
them refused, and the issue's files' navigation counts are unchanged. The Kotlin file
extracts in 0.21, 0.41, 0.81 and 1.66 s at 0.25 to 2 MiB. The JavaScript file's
extraction after the parse takes 0.13, 0.24, 0.48 and 0.98 s, and its parse is still
1.6, 5.9, 20.9 and 88.9 s (issue 93, open). The parser is untouched.

State: resolved.

### 93 — An error-recovered file costs superlinear work in its wide error node

Found by issue 92's resource audit, the final engine-7 audit, with patch 0025 in force.
When a parse fails early, the parser's recovery can leave one error node holding the rest
of the file as a flat run of children. A list a grammar's repetition builds is a balanced
tree of hidden nodes, so positional access into it costs the logarithm of the position;
an error node's children are one flat array, so `ts_node_child(n, i)`,
`ts_node_prev_sibling` and `ts_node_parent` below it each cost the position. Every pass
that steps through such a node by position, or asks for a sibling or parent within it,
is then quadratic in the file. CPU time of one extraction, single-threaded, at `5915669`:

| Shape | 0.25 MiB | 0.5 MiB | 1 MiB | 2 MiB |
|---|---|---|---|---|
| Kotlin: `class O { class I { fun m() { I() } } }`, one class a line (the grammar does not take a nested class body on one line, and the error node holds the rest of the file) | 13.1 s | 51.4 s | 195 s | 764 s |
| JavaScript: `let x = (` before a file of functions | 2.9 s | 10.5 s | 39.6 s | 160 s |

The same files took as long at `8b3ed42`. Each is admitted (within the file-size limit
and the depth ceiling, which an error node's width does not cross), and past an isolated
worker's 120 s timeout (issue 27) whether it extracts depends on the machine: issue 87's
class. It predates this hotfix and is on main.

**Extraction (patch 0026, engine 7).** The driver's decision: remove every quadratic
traversal of wide nodes the engine owns, with no ceiling on an error node's width and no
parse timeout, then measure the parser again. Of 1,302 positional calls in the engine's
own sources, 649 loops stepped through a node's children by index and 203 sites asked
tree-sitter for a parent or a sibling; inside a wide node every one is quadratic,
wherever the node comes from. Now:

- Children are visited by cursor through one iterator (`node_iter.h`), which gives
  exactly tree-sitter's children, named children and fields. A pass that needs them in
  reverse or by position collects them once (`pdxe_collect_children`; the four
  collectors that had grown up became that one). A whole subtree is walked in source
  order by `PDXE_TREE_WALK`.
- Parents and siblings are found from the walk's registered path (the unified walk, the
  definitions walk, the tree walk, C#'s collectors) or from a loop in progress, each
  anchor answering its own node's parent, previous sibling and position and its node's
  other children from a collection made once, never by descending from the root.
  Their answers are tree-sitter's: the next sibling is the first child after the node
  whose end is past the node's (an empty child at its end passed over), and the previous
  sibling of an empty node is tree-sitter's own lookup.
- The sites a resolver adds after the walk are located by one forward cursor pass over
  them in source order, and given their paths and owners in their own order, as the
  file's path budget is spent.
- A collection that cannot get its memory is lost work, never a positional walk.

What remains positional is a constant index (366 sites), a field lookup or a node's own
last child read once per visit (1,259), or test-only code (31). Helpers of languages the
interface does not admit (Dart, LLVM IR, NASM, ObjectScript) still scan back over
siblings to a label or a signature; no file reaches them.

The answers are tree-sitter's: on every node of the 104,730 census files the iterator,
the field names, the parent and the four sibling lookups agree with tree-sitter's (293.8
million nodes, 4.6 billion comparisons), and the census extracts (every field, the
surface, counts, lost work) and resolves (10,928 projects' answers and health)
identically to `5915669`. The work is linear: in test builds the navigation counters
(children by position weighted by the position, cursor steps, parent and sibling lookups
weighted by the children passed, field lookups) double with the file in 75 wide shapes
in 32 languages at 1,000 to 8,000 items and in the issue's two files
(`engine/tests/wide_nodes.c`), and with every collection refused they still do while
the loss is counted; nine mutations are killed. The shapes found more than the issue
named: 8,000 decorated TypeScript methods in one class took 68 s, C# 48 s and Java
16.5 s (every member's doc comment looked up through the class body) and now take
0.55 s, 0.49 s and 0.36 s; a PHP file of 8,000 documented functions 3.4 s, now 0.25 s.
CPU seconds after patch 0026, one file:

| Phase | 0.25 MiB | 0.5 MiB | 1 MiB | 2 MiB |
|---|---|---|---|---|
| Kotlin: the parse alone | 0.08 | 0.14 | 0.29 | 0.55 |
| Kotlin: extraction, all of it | 0.21 | 0.43 | 0.88 | 1.75 |
| Kotlin: the project's typed-resolution run | 0.06 | 0.11 | 0.22 | 0.46 |
| JavaScript: the parse alone | 1.55 | 5.76 | 22.9 | 88.7 |
| JavaScript: extraction, all of it | 1.66 | 6.03 | 21.8 | 85.4 |
| JavaScript: extraction less its parse | 0.13 | 0.25 | 0.50 | 1.02 |
| JavaScript: the project's typed-resolution run | 0.02 | 0.04 | 0.09 | 0.20 |

The admission scan takes 8 to 64 ms in both, and fetching the run's answers no
measurable time. (The engine parses with its own allocator, faster than the parse
alone: 75.8 s at 2 MiB.)

**The parser (open).** The Kotlin file's parse is linear. The JavaScript file's is not:
the `program` holds two error nodes, one of 8,810, 17,077, 33,461 and 66,101 children
(from byte 10 to the end) among 141,000 to 1,058,000 nodes, and its parse quadruples
as the file doubles. Sampled at 1 MiB, 73% of the parse is the parser's recovery
(`ts_parser__recover`), which builds the error node anew for each token it takes
(`ts_subtree_new_error_node`: `ts_subtree_summarize_children` 30% and `ts_subtree_retain`
36% over every child so far, `ts_stack_remove_version` releasing the last, 21%). The
grammar's external scanner is 0.1% of it and lexing below 1%. At 2 MiB the parse alone
takes 89 s, under an isolated worker's 120 s on this machine and over it on one a third
slower. Generic error shapes in the other 31 languages parse linearly (Ada's mildly
superlinear). Changing the parser's recovery changes the vendored runtime, which the
driver decides; nothing here limits an error node's width or a parse's time.

State: open. Extraction fixed (patch 0026); the parser's recovery quadratic.

### 92 — Typed resolution is quadratic in a file's definitions

Found by issue 90's resource audit, with patch 0022 in force: after the usage climbs, the
largest costs left are in the per-file typed resolvers and the interface's translation,
and they grow with how many definitions a file holds, not with its depth. Flat files of
one-method classes (or Go types with a method, Rust structs with an `impl`), CPU time
per extraction:

| Classes | Python | Java | TypeScript | C++ | C# | Go | Rust | PHP | Kotlin |
|---|---|---|---|---|---|---|---|---|---|
| 4,000 | 3.5 s | 0.88 s | 0.86 s | 0.69 s | 0.54 s | 0.45 s | 0.35 s | 0.31 s | 1.3 s |
| 8,000 | 14.8 s | 3.7 s | 3.6 s | 3.0 s | 1.9 s | 1.8 s | 1.1 s | 0.97 s | 4.9 s |
| 16,000 | 61.8 s | 17.9 s | 15.3 s | 13.1 s | 7.9 s | 7.5 s | 3.9 s | 3.6 s | — |

Each doubling costs 3.5 to 4.8 times as long: quadratic. Sampled, the mechanisms are:

- the Python resolver, for each class, scans the module's top level for a class of that
  name (`py_bind_external_module_classes`, `py_root_defines_class_named`);
- the resolvers' registries look a name up by scanning while they register each
  definition (`register_local_func_or_type_from_file`, `lookup_type_self`,
  `lookup_func_self`);
- the interface's translation finds a definition by qualified name by scanning
  (`index_of_qn` in `engine/api/pdxe.c`, for parents and for calls without an owner).

Within the depth ceiling, JavaScript calls nested 200 deep (`f(a, f(a, ...))`) cost the
TypeScript resolver per call more the more of them a file holds (`ts_lookup_symbol_for_call`,
the registry's lookups by arguments and types): 64 such nests 2.9 s, 128 7.0 s, 256
18.5 s, 512 54 s, and a 2 MiB file 441 s; flat calls and calls nested 10 deep are linear.
2 MiB of distinct Java classes nested 200 deep take 237 s, most of it the Java
registry's.

Each of these files is admitted and extracts whole; on the measuring machine some take
longer than an isolated worker's 120 s, which fails the build (issue 27), and how long
each takes depends on the machine: issue 87's class. Generated code holds such files:
protocol-buffer outer classes, models, API clients. The goldens and the census do not
come near it. It predates this hotfix and is on main.

The driver decided it must be fixed before engine 7 reaches main, by indexing every lookup
by the key it matches and never by a limit: no file-size limit, depth, budget, quota or
timeout changed, and typed resolution is skipped for no file (decision 62). The fix is
patch 0025 and, for Stage 3, `pdx-core`'s registry and module rules.

**The audit.** Every loop on the path that grows with a file was read and classified: the
per-file resolvers and their registry (`engine/src/lsp/`), the language helpers, the
cross-file pass, the interface's translation (`engine/api/pdxe.c`) and Stage 3 (`pdx-core`'s
symbol registry and stages). A is an exact key an index answers, B a bucket of values one
key holds, C a candidate set the semantics examine whole, D bounded by a small constant, E
linear already or not on the path. Every A and B on a path a file of many definitions,
imports, usages or sites reaches is now indexed:

| Where | What scanned, per question | Now |
|---|---|---|
| Registries | every function or type per self-lookup (`lookup_type_self`, `lookup_func_self`, `lookup_method_self`), overload lookup and iterator, in a registry never finalized (every per-file resolver's), and the entries a finalized one gained later | chains kept on every add: functions by qualified name, types by qualified name, methods by receiver and name, receiver-less functions by short name, functions by receiver, types by short name and by last name segment |
| Java (`register_local_func_or_type_from_file`) | every type per type definition, every function per method to patch, every type and every field per field | the registry's chains; a declaration's fields appended once |
| Python | the module's top level per class (`py_root_defines_class_named`), every statement per import, every binding per wildcard import | the module's root class names; import items by local name; the bindings written since the last wildcard |
| TypeScript, JavaScript | every function per symbol or overload lookup, every usage per lexical argument, every import per identifier, every function and type per declaration patched | the registry's chains; usages by site; imports by local name |
| Kotlin, Go, C#, PHP, Rust, Perl, C | sole definers, registration and patch passes, free-function fallbacks, extension and operator candidates, macro rules and carriers, function-pointer bindings, @ISA, docblock siblings, the Go cross builder's type table (and its scan when it could not be allocated) | the registry's chains, or an index of each by its key |
| Every resolver | import, include and use arrays copied whole on each add, or grown by 32 | grown by doubling |
| Extraction | every definition per call, for the innermost enclosing one | one sweep |
| Cross-file pass | every definition per Go struct, every import and key per import | indexes by owner, by local name and by key |
| Interface | `index_of_qn` per parent, usage, type, write, throw and caller; every resolver row per call and per reference | a position index per file; invocation rows by site, by caller and leaf, by caller's tail and leaf; references by occurrence |
| Stage 3 | every definition and import of the file per site (`name_provenance`), every C++ definition per definition (`nesting`) | by file and name; by type name |

Kept, C or D: the matcher's rows without a site of one caller and one callee leaf, which
the join compares whole (16,000 calls of one method in one C++ function: 0.9 s in the
project run, about 3 s at 2 MiB); JavaScript's re-evaluation of a degraded nested call up
to the evaluator's depth cap of 64, linear in the calls by that constant; bounded stacks,
searches and caps.

**Answers.** Each chain is in the scan's order, so the first entry a scan found heads it
and a walk meets the scan's matches in its order: every duplicate qualified name, overload
and short name keeps the winner it had, first in a registry never finalized, the finalized
prefix's own in one finalized. The interface's index skips synthetic definitions, compares
names without the project's prefix and gives a name to its first definition, as
`index_of_qn` did. No answer comes from hash order.

**Work.** CPU time of one extraction, single-threaded, at `8b3ed42` and now, with each
doubling of the file from 1,000 classes on costing 1.9 to 2.2 times the last:

| 16,000 classes | Python | Java | TypeScript | C++ | C# | Go | Rust | PHP | Kotlin |
|---|---|---|---|---|---|---|---|---|---|
| `8b3ed42` | 63.9 s | 18.0 s | 16.7 s | 13.6 s | 8.1 s | 7.2 s | 4.4 s | 3.7 s | 20.4 s |
| now | 0.57 s | 0.59 s | 0.67 s | 0.60 s | 0.72 s | 0.76 s | 0.87 s | 0.50 s | 0.82 s |

JavaScript calls nested 200 deep: 64, 128, 256 and 512 groups 2.7, 7.0, 18.9 and 53.7 s
then, 0.78, 1.57, 3.18 and 6.30 s now; the 2 MiB file 406 s then, 21.3 s now (the
evaluator's depth cap of 64 is the constant). Distinct Java classes nested 200 deep: the
2 MiB file 401 s then, 5.8 s now (0.25, 0.5 and 1 MiB: 0.71, 1.40 and 2.79 s). A
project's run over each flat file takes at most 0.24 s at 16,000. The whole pipeline
(`pdx index`, a cold cache, one worker) over one 16,000-class file takes 1.6 to 2.5 s in
every language; before Stage 3's change C++ took 7.1 s, 38% of it in `nesting` and 43%
in `name_provenance`.

Counted (test builds), the indexes examine no registry entry one by one, and every
count doubles with the file. The scans, for comparison (the reference switch), at 8,000
classes: registry entries examined one by one, qualified-name comparisons and the
interface's definition entries were 297.5 million, 297.5 million and 192.1 million in
Python (now 0, 8,096 and 51,131), 761 million, 204 million and 192 million in Java (0,
56,001 and 48,729), 622 million, 490 million and 256 million in TypeScript (0, 24,255 and
62,945), and 1.84 billion, 1.71 billion and 256 million in Kotlin (0, 24,001 and 70,481);
Python's module-class scans 32.0 million (25,970). At 512 nested JavaScript groups the
scans examined 14.1 billion registry entries; the indexes examine none, with 1,280
qualified-name comparisons. Peak memory at 16,000 classes is 6 to 12% above `8b3ed42`'s
(Python 179 MB then, 200 MB now), linear in the file.

**Allocation.** An index that cannot get memory finds nothing from then on: a lookup misses,
an iterator yields nothing, the translation or a run's collection fails with
`PDXE_E_NOMEM`; the memory core counts the failure as lost work. No index falls back to the
scan it replaced. Indexes are owned by their registry, context or run and freed with it;
no index is static or shared between threads (the test builds' counters are per thread).

**Tests.** `engine/tests/typed_lookup.c`:

- Every typed-resolution fixture, issue 92's own cases (duplicate qualified names and
  short names, overloads, one class name in two modules, a nested class beside a root
  class of its name, imported aliases and wildcards, one function in several files,
  receiver member lookups, ambiguous TypeScript symbols, C function pointers and their
  shadows, Rust macros and impls, PHP docblocks, Perl @ISA) in eleven languages, a
  fixture where the first and the last of two duplicates lead to different answers,
  generated code (protocol-buffer modules, nested builders, API clients) and the issue's
  shapes extract to the same facts, each file's resolver to the same rows, and each
  project to the same answers and health, with the indexes and with the scans.
- The work is linear: flat files of 1,000 to 16,000 classes in nine languages, 64 to 512
  groups of nested JavaScript calls, 4,000 to 16,000 definitions translated and resolved,
  and generated code; the scans' work, for comparison, grows more than sixfold when the
  file quadruples.
- A duplicated qualified name scopes to its first definition, found without the project's
  prefix.
- Each family of indexes refused its memory (`registry`, `scope`, `resolver`,
  `interface`, `all`): extraction and resolution complete or fail with `PDXE_E_NOMEM`,
  the refusal is lost work or a pass failure, no registry is scanned, and the work stays
  linear.

`crates/pdx-core/tests/registry.rs`: `name_provenance_keeps_every_definition_and_import_in_order`
and `cpp_definitions_take_the_namespace_of_their_outermost_type`, which pass with the old
scans and the new indexes alike.

**Mutations.** Each compiled, was killed by a failing assertion and was restored byte for
byte (sha256): the interface's `index_of_qn` scan restored (definition entries quadruple);
the last duplicate keeping the name (the indexes and the scans disagree on 13 fixtures);
names compared with the project's prefix (a usage scoped to no definition);
`lookup_type_self` and `lookup_func_self` scanning (qualified-name comparisons triple per
doubling); Python's top-level scan restored (module-class scans quadruple); the method key
without its receiver (overload candidates quadruple); chains in newest-first order (the
first and last duplicates' answers differ); TypeScript's global symbol scan and its
per-call argument and type scans restored (comparisons triple per doubling).

**Census.** All 104,730 census files, as 10,928 directory projects, resolve to the same
answers (target, file, score, strategy, engine strategy, candidates, site) and health as
at `8b3ed42`, and extract to the same public facts, texts, qualified names, surfaces and
lost counts. The goldens' segments are unchanged.

State: resolved.

### 91 — Grammar scanners overrun or abort on deeply nested state

Found by issue 90's resource audit. The parser keeps an external scanner's state between
tokens in a 1,024-byte buffer (`lexer.debug_buffer`, `TREE_SITTER_SERIALIZATION_BUFFER_SIZE`),
and asserts, in debug builds only, that a scanner's serialised state fits. Several of the
vendored grammars' scanners keep state that grows with nesting:

- **YAML** writes a 10-byte header, then four bytes for each open indentation level
  while its size is below 1,024: at 253 levels it writes two bytes past the buffer and
  returns 1,026. A release build then crashes in the parser, a null dereference in
  `ts_stack_pop_count` (253 nested block mappings, a 41 KB file; 252 parse).
- **Markdown** copies its open blocks into the buffer with no bound at all: nested
  lists overrun it by as much as they nest (400 levels: the sanitizer's assertion).
- **Kotlin** calls `abort()` when a 1,025th string delimiter would open (1,100 nested
  string templates, about 6 KB, abort the process in every build; 600 do not).
- **Python** writes two bytes for each indentation level while its size is below 1,024;
  its header is one byte plus one for each open string delimiter, so with an odd number
  of those and more than 510 indentation levels it would write one byte past. Not
  reproduced.

The other scanners' serialisers were read: Bash, C#, C++, Dockerfile, HCL, Ruby, Scala,
SQL and XML bound what they write, Perl caps its quotes, and JavaScript, TypeScript,
TSX, TOML, Lua, Rust, Swift and properties write a fixed few bytes; PHP's delegates to a
serialiser this audit did not read. Under the address sanitizer every battery shape
(about 70, nested 50 to 400 deep, every language with a grammar here) passed but YAML's
and Markdown's.

All of it happens during the parse, before any extraction pass, so the depth ceiling
(issue 89) does not reach it. Extraction runs in process unless isolation is asked for
(`PDX_ENGINE_ISOLATE`), so one such file in a repository stops `pdx index`; isolated, it
costs its file. The overruns are memory safety from repository content, the abort a
denial of service. The same at `582b58a` and on main.

The driver decided it must be fixed before merge, in the scanners: not by isolating
extraction, not by an assertion, with no setting, every bound derived from the buffer.

#### Resolution (patch 0023, decision 60)

The census, every external scanner the engine builds (the other nine grammars, Ada, C,
Go, GraphQL, Groovy, Java, JSON, Objective-C and protocol buffers, have none):

| Scanner | Variable state | Serialized, at most | Safe before | Changed | Why |
|---|---|---|---|---|---|
| YAML | indentation pairs | was 1,026; now 1,022 | no | yes | `size < 1024` before a 4-byte pair |
| Markdown | open blocks | was unbounded; now 1,021 | no | yes | no bound on the copy |
| Python | string delimiters, indentation | was 1,025; now 1,024 | no (latent) | yes | `size < 1024` before a 2-byte entry |
| Kotlin | string delimiters | 1,024, by `abort()`; now 1,024 | no (abort) | yes | aborted at 1,025 |
| Bash | heredocs | under 1,024 | yes | no | checks each whole entry; past the buffer, keeps no state |
| C# | interpolations | 1,022 | yes | no | past 255 entries, keeps no state; reads guarded |
| C++ | raw-string delimiter | 64 | yes | no | at most 16 characters |
| Dockerfile | heredocs | 1,024 | yes | no | checks each whole string; at most 10 |
| HCL | template contexts | under 1,024 | yes | no | checks each whole entry; pops guarded |
| Perl | quotes, heredoc queue | 1,024 | yes | no | caps quotes after reserving the full queue |
| PHP | heredocs | under 1,024 | yes | no | checks each whole entry; reads guarded |
| Ruby | literals, heredocs | under 1,024 | yes | no | checks each whole entry; reads guarded |
| Scala | indentation | 1,024 | yes | no | past the buffer, keeps no state; pops guarded |
| SQL | dollar-quote tag | 1,023 | yes | no | past the buffer, keeps no state |
| XML | tag names | under 1,024 | yes | no | checks each whole name; pops guarded |
| JavaScript, TypeScript, TSX, TOML | none | 0 | yes | no | no state |
| Lua, Rust, Swift, properties | fixed | 1 to 4 | yes | no | fixed fields |

Besides the serializers: `abort()` remains only where a scanner's own creation cannot
allocate (Kotlin, XML), HCL's growth checks its allocation only by an `assert`, some
scanners' creation does not check it (Python, Dockerfile, Markdown), and tree-sitter's
array growth assumes allocation succeeds while the engine's allocator returns null. Allocation failure inside the parser is outside this issue: a residual,
not reachable from repository content within the memory budget. `assert(size ==
length)` in several deserializers is a debug check of the scanner's own format; Scala's
and Python's `assert(false)` are unreachable defaults of exhaustive switches; every
stack read and pop is guarded by a size check, or, in Perl, by the error-recovery exit
and the quote pushed at every quote-like start.

The four affected scanners now hold what the buffer holds, each bound derived from the
buffer (`TREE_SITTER_SERIALIZATION_BUFFER_SIZE`, 1,024), its header and an entry:

- **YAML:** a 10-byte header (five `int16_t`) and 4 bytes an indentation entry: (1,024 -
  10) / 4 = 253 entries above the root. `PUSH_IND`, `PUSH_BGN_IND` and
  `MAY_PUSH_IMP_IND` refuse a push past it and return no token, as they already did for
  a tab. The serializer writes complete pairs only; the deserializer reads a complete
  header and complete pairs within the length, and nothing from a length under the
  header or past the buffer.
- **Markdown:** a 5-byte header and `sizeof(Block)` (4) a block: 254 open blocks, which
  also keeps the count within the 8-bit `matched` it is compared with. `push_block`
  refuses past it, and when its growth cannot allocate, and each of the 14 block-opening
  tokens is then not claimed. Serializer and deserializer are bounded the same way.
- **Kotlin:** one byte a delimiter, no header: 1,024 open strings. The push refuses past
  it and the string start is not claimed; popping an empty stack returns 0; the
  serializer copies at most the buffer and the deserializer reads at most it.
- **Python:** 2 bytes, then a byte for each open string delimiter (at most 255, the
  count's width) and 2 for each indentation level above the first, all at most 1,024. A
  delimiter or level past it is not pushed: the string start is not claimed, the indent
  not emitted. The serializer checks each complete 2-byte entry; the deserializer clamps
  the delimiter count to the bytes present.

Past a bound the construct is not opened and the parser recovers: 254 nested YAML
entries now parse partially (the old 510-byte crash), Markdown quotes past 254 are text,
and Kotlin templates and Python levels past theirs are already past the depth ceiling,
which refuses them after the parse. Below the bounds nothing changes: all 104,730
census files extract to the same public result, surface and lost count as at
`94003e2`.

Tests:

- `engine/tests/scanner_state.c` compiles the four scanners' own code into the test,
  entry points renamed. For each: the ceiling exactly (253, 254, 1,024; Python's for 0,
  1, 2, 3, 254 and 255 open delimiters); every state size from empty to 16 past the
  ceiling, filled past it the way only the old code could, serialized between 64 guard
  bytes either side of a 1,024-byte buffer (length within the buffer, guards
  untouched, complete entries only, round trip identical); every length from 0 to 1,032
  handed to the deserializer in a block of exactly that size; Kotlin's string start
  refused at the ceiling and its empty pop. End to end, each in a child process so an
  abort or crash is reported: YAML mappings and compact sequences at 252, 253, 254 and
  400, Markdown quotes and lists at 253, 254, 255 and 300, Kotlin templates at 1,023,
  1,024, 1,025 and 1,100, Python blocks at 510, 511, 512 and 520, each the same on two
  runs.
- `crates/pdx-engine/tests/scanner_state.rs`: the 510-byte reproducer is a partial parse
  with no work lost; every shape above extracts, the same on every run.
- The sanitizer corpus holds `yaml/recovery_deep_sequences.yaml` and
  `markdown/recovery_deep_quotes.md`.
- Release and `make check-asan` (address, undefined-behaviour and leak sanitizers): the
  test and the corpus pass with no report. Before, the YAML and Markdown shapes stopped
  the sanitizer's assertion and crashed a release build, and the Kotlin one aborted.
- Mutations killed: BP (YAML's `size < 1024` restored), BQ (YAML writes half a pair),
  BR (Markdown's copy unbounded again), BS (Kotlin's `abort()` restored: reported as
  signal 6 from the child processes), BT (Kotlin claims a push it did not record), BU
  (Python's `size < 1024` restored).

`ENGINE_VERSION` stays 7: patch 0023 is part of engine 7. No stored format changes.

State: resolved.

### 90 — Within the admitted depth, usage handling still costs depth² per usage

Found by the closure audit of issues 88 and 89, with both in force. Issue 89's ceiling
refuses any file deeper than 512, so no single construct costs what 1,000 levels did.
Usage handling still climbs to the root with `ts_node_parent` for every usage, and
`ts_node_parent` descends from the root again: depth² per usage. A file may hold as many
deep constructs as its size allows, and their costs add up.

| Source, all admitted (depth under 512) | Size | Time | Engine peak |
|---|---|---|---|
| one Python member chain `a.b.b...`, 100 links | small | 0.06 s | 9 MB |
| the same, 200 links | small | 0.41 s | 9 MB |
| the same, 400 links | about 800 B | 3.08 s | 9 MB |
| 2, 4 and 8 such 400-link chains in one function | 1.6 to 6.4 KB | 6.1, 15.8 and 25.0 s | 10 to 13 MB |
| Java classes nested 250 deep | about 4 KB | 0.85 s | 9 MB |
| JavaScript functions nested 250 deep | about 4 KB | 0.70 s | 8 MB |
| 2,700 C call chains 250 deep (argument text) | 2.0 MB | 8.1 s | 1.33 GB |

- **Time.** Sampling the multi-chain Python file puts 98% of samples in
  `handle_usages_at_node`, under `ts_node_parent` and `ts_node_child_with_descendant`:
  the same mechanism issue 89 found. At about 3 s per 800-byte chain, a 13 KB file of
  them takes about 50 s, and a 2 MiB one hours. Whether it extracts within an isolated
  worker's timeout (issue 27) depends on the machine.
- **Memory.** A call stores its argument text whole, so nested calls store text
  proportional to their depth. That is linear in the source within the ceiling, but at
  about 124 bytes of argument text, and about 650 bytes of engine memory, per source
  byte: 1.33 GB for a 2 MiB file, before the extraction is copied into Rust.

The normal corpora do not come near it: every census file extracted under a 1.2 GB cap,
and the goldens and the differential are unchanged. It predates this hotfix and is on
main.

The rest of the audit, every representation an extraction holds, after issues 87 to 89:

- **Bounded by the file-size limit:** the parse tree and the depth scan over it (at most
  961,482 nodes in the census), and the preprocessor's own token list (about 600 to 700
  MB for a 2 MiB source, linear; issue 87's residual).
- **Bounded by the preprocessing ceilings (issue 87):** macro expansion, `#line`
  padding, `#if` folding.
- **Bounded by the path budget (issue 88):** site node-type paths.
- **Bounded by the syntax depth (issue 89), linear in the source within it:**
  - argument and callee text;
  - the qualified names of nested definitions and namespaces;
  - nested closures and `new` expressions;
  - type references to nested generics;
  - per-node climbs.
- **Linear in the source or its evidence:** decorators, base classes, signature
  parameter types, docstrings and signatures, route facts (each path at most 64
  entries), throws, read and write facts, imports, usage names, channels, environment
  accesses, `impl` relations, error ranges (capped).
- **Bounded by the walk's node budget:** nothing by default; it is an operator switch.
- **Defect:** usage handling's depth² per usage (time), and argument text times depth
  (memory), within the admitted depth: this issue.

The driver decided it must be fixed before engine 7 reaches main, in two parts that
must both close: 90A, the repeated ancestry (time), by carrying the walk's own ancestry;
90B, the overlapping retained text (memory), without an interface change or not at all
(the interface stop condition below). No depth change, no clock, no generic work cap.

#### 90A: fixed (patch 0022, decision 59)

The audit of every `ts_node_parent` reachable in extraction (173 sites; the languages
with a grammar here are the 34 the engine builds):

- **A, per node or usage in the unified walk:** usage classification in
  `extract_usages.c`. The reference test (a Python attribute's callable site, a Rust
  path's parent), argument labels, the binding and write climbs, direct-argument and
  callable-site climbs (JavaScript, Python, Go, Rust, C#, Kotlin, the parenthesised
  form), Perl code references and declarations, HCL attributes, the Python `global` and
  `nonlocal` and default-value climbs, `record_lexical_binding`'s parameter, declared
  name and JavaScript `var` climbs, and the import-binding climbs. Also not a
  `ts_node_parent` but O(depth) for every node: finding the lexical scope in force by
  scanning the scope stack.
- **B, per definition or rare:** definition extraction (`is_js_exported`, docstrings,
  decorators, function-name resolution, route paths), qualified-name computation for a
  function frame, channels, URL builders, the C# and Kotlin resolvers' collections,
  late-site and owner paths, the PHP route group prefix.
- **C, a constant number of hops:** the Kotlin index write test, a callable
  reference's branch test, YAML's root mapping, an arrow function's declarator.
- **D, tests only:** none reachable.
- **E, no grammar here:** the TLA+, CFML, Tcl, CMake, Fish, Puppet, Vim script, OCaml,
  SCSS, Lisp, Elixir, Julia, Wolfram, Typst, COBOL, Lean, Pascal, Teal, MATLAB, VHDL,
  Pine, Pkl, Agda, LLVM, Dart and the rest of the usage rules, and the legacy
  `walk_usages`, which nothing calls.

Profiling before: 3,086 of 3,089 samples of the multi-chain Python file in
`handle_usages_at_node`, 3,037 under `ts_node_parent`; Java nested classes and
JavaScript nested functions spent theirs in `record_lexical_binding`'s
`declared_function_name_owner`, `binding_is_parameter` and `declared_class_name_owner`.

The fix: the unified walk records a frame for every node it enters (`PDXEWalkFrame`:
the node and its field, by cursor depth), so the current node's ancestors are the
frames below it. Every A site is answered from them. A nearest-of-a-kind question is
kept by each frame once computed, from the frame below; a climb (binding, write, Python
default value) keeps each frame's answer for its own node, its parent's verdict or,
undecided, the frame below's, which is every positive-width node's answer, since a
child containing such a node by bytes is the node's own ancestor (a node of no width,
a parser's missing one, climbs the frames step by step, at most the depth). The other
climbs step over the frames. Each scope frame keeps the lexical scope in force. B and C
sites are unchanged (below).

| Reproducer (CPU time per extraction) | 582b58a | patch 0022 |
|---|---|---|
| Python member chain, 100 / 200 / 400 links | 0.072 / 0.511 / 3.71 s | 0.8 / 1.8 / 2.1 ms |
| 1 / 2 / 4 / 8 / 16 chains of 400 links | 3.72 / 7.45 / 14.1 / 27.3 / 54.9 s | 2.2 / 4.4 / 7.7 / 16 / 31 ms |
| Java classes nested 250 deep | 0.93 s | 15 ms |
| JavaScript functions nested 250 deep | 0.77 s | 66 ms |

- **Counted, not timed** (`engine/tests/usage_ancestry.c`, test builds' counters):
  ancestors examined for a member chain of 100, 200 and 400 links 1,575, 3,075 and
  6,075 (climbing: 67,541, 254,941, 989,741); for 1 to 16 chains of 400 links 6,075 to
  96,420; nested classes, functions, definitions and calls likewise linear; no
  `ts_node_parent` anywhere, the 400-link chain included.
- **Same answers.** The test switch `PDX_ENGINE_TEST_CLIMB_ANCESTRY` answers by
  climbing: every fixture extracts identically both ways, in every language with a rule
  the frames serve and with parser recovery. Every one of the 104,730 census files
  extracts to the same public result, surface and lost count as at `582b58a`.
- **Mutations killed:** BG (Python's directive and default-value questions climb), BH
  (`binding_is_parameter` climbs), BI (the JavaScript `var` question climbs), BJ (the
  nearest import found by the old walk), BK (frame answers recomputed from the root for
  every question).

Not fixed by 90A, measured on 2 MiB files of the deepest admitted constructs (at a fixed
depth each time is proportional to the file, a cost per byte that grows with the depth):

- **Definition extraction, per definition:** nested JavaScript functions 23 s (the
  export test's four `ts_node_parent` hops, complexity and fingerprint walks over each
  function's subtree); nested Java and C# classes 12 s (docstring and decorator
  climbs) when the copies repeat their names, 237 s for Java when every class is
  distinct, where the typed resolver's registry dominates (issue 92).
- **Typed evaluation of nested and chained calls:** JavaScript call chains 19 s, Go 22
  s, Java 13 s; 200-deep JavaScript nested calls 441 s, past the isolated worker's 120 s
  (issue 92).
- **Import climbs** in Rust `use` lists nested 200 deep: about 6 ms each, linear in
  their own nesting per binding.
- **Kotlin classes nested 400 deep** parse as a flat recovered tree: no 2 MiB result in
  15 minutes (a width cost; issue 92).

#### 90B: stopped at the interface condition

The text an extraction retains is linear in the file at a fixed depth and grows with the
depth, as the issue says; measured on 0.25, 0.5, 1 and 2 MiB of four admitted shapes
(retained bytes for 2 MiB):

| Shape | Field that grows with depth | 2 MiB: that field | arena | peak RSS |
|---|---|---|---|---|
| C calls nested 250 deep | argument `expr` | 254 MB | 861 MB | 1.16 GB |
| JavaScript call chains 200 deep | callee text | 204 MB | 1.16 GB | 1.49 GB |
| Python member chains, 400 links | usage names | 407 MB | 1.10 GB | 1.37 GB |
| Java classes nested 200 deep | qualified names | 99 MB | 722 MB | 1.23 GB |

Each doubling of the file doubles each figure. Besides those fields the arena holds
per-fact records (a call's eight-argument block, its record, the C resolver's type
names), linear in the facts. The engine's own copies that nothing exports (receiver
classification, environment-key probes, the resolvers' node text) add 1 to 4 times the
exported text in some languages, also proportional to it.

The four fields are exported whole, and read whole:

- **Argument `expr`** (`pdxe_call_arg.expr`, "its expression as written", issue 47):
  Rust contracts read it entire: `string_literal(expr)`, `expr[..expr.find('(')]` of
  a nested call argument, `handler.expr.split(".to(")`, `a.expr.contains(quoted)` (any
  substring), object and list literals parsed from it; it crosses the worker protocol
  (6) and the extraction cache (format 5). Which calls a contract reads is decided in
  Rust by provenance, so the engine cannot leave any out.
- **Callee text** (`pdxe_call.callee_text`): hashed into every site's
  `ast_fingerprint` (4.2.1) and stored in `sites.callee_text` (4.3); split into receiver
  and name by Stage 3 and matched by contracts.
- **Usage names** (`pdxe_usage.name`): a Python `attribute` usage's name is its whole
  text, so each link of a chain names its whole prefix (the audit above listed usage
  names as linear in the source; they are, at a fixed depth, like the rest).
- **Qualified names** of nested definitions: identity (4.2.1).

A null-terminated string per field cannot share bytes with an overlapping one, and
interning finds no duplicates among them. So with the interface, the cache and the
identities as they are, retaining every one of them is what the specification asks: the
stop condition holds, and nothing was implemented for 90B. Changing it is the driver's
decision. The smallest change found, in two steps. (1) Extraction: the interface gives
each argument expression, callee and usage name as a byte span into the source it was
given (`pdxe_call_arg`, `pdxe_call`, `pdxe_usage`), Rust keeps the normalised source once
per extraction and reads the spans, and the extraction cache (format 6) and the worker
protocol (7) carry spans; `ENGINE_VERSION` moves with the interface. That makes the
engine, the worker and the cache linear. (2) Stored graph: a site's callee text is
stored in `sites.callee_text` and hashed into its identity, and a definition's qualified
name is its identity, so a linear segment needs a spelling that does not repeat a
chain's receivers or a nest's enclosing names: a change to 4.2.1 and 4.3 (identity
vectors, `SEGMENT_SCHEMA_VERSION` 4, every chained site and nested definition renamed,
every segment re-indexed).

#### 90B: fixed (patch 0024, decision 61)

The driver decided the stop condition above: 90B is an interface, model, cache and
protocol redesign, provided every identity is kept. The invariant: source-backed
evidence is held once, as the normalised source's bytes and bounded per-fact
references to them.

- **Interface.** Text a fact takes from the file crosses as `pdxe_text` (a pointer and
  four 32-bit fields, 24 bytes, layout asserted): `SOURCE`, the bytes [start, end) of the
  source the caller passed, when the text is exactly one contiguous run of it, nothing
  trimmed; `OWNED`, when the engine assembled or rewrote it or read it after C-family
  macro expansion; `NONE`. Never a range and a copy both. A range is decided by
  verification: the text's bytes equal the source at the fact's site, inside it, and
  the fact has raw origin. A call's callee, an argument's expression, a usage's name, a
  route's declaring node and a definition's parameter list are texts.
- **Inside the engine** (patch 0024): an argument's expression and a definition's
  parameter list read in the raw source are ranges (`expr_start`/`expr_end`,
  `signature_start`/`signature_end`), never copied; the environment-key probe and the
  JavaScript receiver classifier read in place. A call's argument array is sized to the
  arguments it captures, a splat after them taking a position and no slot.
- **Qualified names** are structural (`qn_form`, `qn_base`, `qn_text`, `qn_keep`):
  whole; a member of an earlier definition (its name, `.`, the definition's own name);
  an earlier name extended by a rest beginning with an ASCII byte; or the first
  `qn_keep` bytes of an earlier name, ending before a `.` that definition adds itself,
  then a rest empty or beginning with `.` (C++ functions in nested namespaces, which
  are no definitions). Chosen in order: the definition the name names without its
  final `.name`, then the parent, then the module, and a prefix only when it holds
  strictly fewer bytes, found by hashing the name's `.`-bounded prefixes once against an
  index of the prefixes earlier names added beyond their bases. Every byte a prefix
  form holds is a segment new to the file, so the bytes held are linear in it.
  `pdxe_qualified_name` spells one name, linear in its chain.
- **Rust.** `TextRef { Source { start, end }, Owned }`, `QualifiedName { Whole, Member,
  Extended, Prefix }`; `ExtractRecord` is what is serialised (ranges, no source) and
  `FileExtract` a record bound to one `Arc<[u8]>`. `ExtractRecord::bind` checks the
  length, the digest, every range (start ≤ end ≤ length) and every name (bases earlier;
  a prefix's cut a `.` in its base's own bytes, from lengths computed once, so the check
  is linear) and returns a `BindError`, never clamping; conversion runs the same checks
  and never spells a range out. `FileExtract::text` reads a text borrowed;
  `qualified_name`, `qualified_name_parts` and the per-file memo `QualifiedNames` spell
  names. Range coordinates are equality within one extraction only; consumers compare
  text.
- **Versions.** Extraction cache format 6: records only, no source; a hit binds the
  record to the verified current normalised source, and a range or name that does not
  fit is a miss (`MalformedText`), replaced. Worker protocol 7: the worker answers with
  records and the parent binds each to the source it sent; protocol 6 is refused.
  Surface 5 (the layout changed: `source_len`, texts as `[start, end]`, definition
  references as indexes, names as a string, `[base]`, `[base, "rest"]` or
  `[base, "rest", keep]`, parameter-list ranges), validated on decode and bound to the
  source when a project runs. `ENGINE_VERSION` stays 7 (unreleased); language matrix 2,
  secret detector 4.
- **Identities.** Node ids and site fingerprints hash exactly the bytes the whole name
  or text gives: fixed vectors (`structural_names_give_the_identities_their_whole_names_give`,
  `prefix_names_give_the_identities_their_whole_names_give`,
  `source_ranges_give_the_site_identities_their_whole_text_gave`), computed
  independently over 4.2.1's fields. `SEGMENT_SCHEMA_VERSION` stays 3: a node's row
  stores its whole name and parameter list, spelled out once where the row is made.

The audit of every string an extraction holds (the brief's §42): 108 shapes in 29
languages, each nested at 10 to 160 levels, every field's bytes compared across a
doubling of the depth, relative to the source's own growth (2 is linear, 4 the square):

| Field | Class | At `8538528` | Now |
|---|---|---|---|
| `Call.callee_text` | source range; owned when synthesised or expanded | 2.1 (3 call chains) | at most 1.03 |
| `CallArg.expr` | source range; owned when expanded | 2.0 to 2.1 (15 shapes: nested calls, closures, lambdas, `new`, decorators' arguments) | no owned bytes |
| `Usage.name` | source range | 2.1 (Python attribute chains) | no owned bytes |
| `Definition.qualified_name` | structural identity | 1.6 to 1.8 (11 shapes: nested classes, modules, objects, types, TypeScript and C++ namespaces) | at most 1.00 |
| `Definition.signature` | source range; owned when expanded | 2.0 (JavaScript functions as defaults) | no owned bytes |
| `RouteFact.source_text` | source range | linear | linear |
| `Definition.name`, `engine_kind`, `kind` | small, inherently bounded | 1.09 | 1.09 |
| `Definition.doc`, `base_classes`, `decorators`, `signature_param_types` | inherently bounded (a comment, a base's spelling, a decorator, a type; class expressions as bases and decorators measured) | at most 1.02 | at most 1.02 |
| `RouteFact.method`, `path`, `callee_text` | small, inherently bounded | linear | linear |
| `Call.receiver_text` | always none from extraction | — | — |
| `CallArg.value`, `keyword` | inherently bounded (a literal's or a constant's value, an identifier; 4,000 uses of one 200-byte constant measured) | 1.01 | 1.01 |
| `Import.*`, `TypeRef.type_text`, `ReadWrite.field_text`, `Channel.channel_text`, `EnvAccess.key`, `Throw.exception_text`, `ImplTrait.*`, diagnostics | inherently bounded (nested generics, `use` trees, attribute writes, `throw new` chains, `impl` blocks in nested modules measured) | at most 1.01 | at most 1.01 |
| the surface | the engine's per-fact encoding, linear in the facts | linear | linear |

Measured through the Rust wrapper, one extraction a process, at `8538528` and now
(retained: Rust heap the extraction holds; record: its serialised size without the
surface; the surface itself is the engine's linear per-fact encoding):

| 2 MiB of | Peak RSS | Retained | Record without surface |
|---|---|---|---|
| C calls nested in arguments | 3.20 GB → 2.42 GB | 1.32 GB → 832 MB | 282 MB → 35 MB |
| JavaScript call chains | 2.96 GB → 2.16 GB | 807 MB → 398 MB | 225 MB → 24 MB |
| Python attribute chains | 4.05 GB → 2.95 GB | 1.55 GB → 744 MB | 443 MB → 49 MB |
| Java classes nested in classes | 1.34 GB → 1.26 GB | 339 MB → 149 MB | 105 MB → 7.3 MB |
| JavaScript functions as defaults | 997 MB → 545 MB | 449 MB → 139 MB | 165 MB → 6.4 MB |
| C++ functions in nested namespaces | 447 MB → 432 MB | 92 MB → 67 MB | 18 MB → 3.2 MB |

The same at 0.25, 0.5 and 1 MiB scales linearly in both. At a fixed 0.5 MiB, the record
without its surface no longer grows with the depth: Python chains of 50, 100, 200 and
400 links 9.9, 11.1, 11.8 and 12.2 MB (17.9, 31.5, 58.0 and 110.5 MB before); C calls
nested 50, 100 and 200 deep 14.1, 14.3 and 14.4 MB (24.4, 37.6, 63.7). Text read
borrowed costs nothing per use; a qualified name is spelled once a file by the memo
that needs it.

- **Same results.** All 104,730 census files extract identically to `8538528`: the
  public result with every text and name spelled out, the surface rewritten in version
  4's form, and lost work (17 files, as before). Their surfaces total 33.99 GB, against
  41.23 GB (17.6% smaller); 54.9 million texts are ranges and 31.6 million owned (354
  MB); names are 104,716 whole, 7,218,322 members, 10,442 extended and 3,702 prefixes,
  holding 8.0 MB. Goldens, the differential and the typed fixtures are unchanged.
- **Tests.** `engine/tests/source_text.c` (layout, ranges and owned text, names and
  their vectors, truncation and refusals, the build's and the surface's checks, retained
  text at depths 40, 80 and 160 in six shapes), `pdx-engine`'s `source_text` (the same
  through Rust, binding and its refusals), `isolate` (protocol 6 refused, the worker's
  answer the in-process record), `pdx`'s
  `isolation_carries_text_as_ranges_bound_to_the_source_sent`, `pdx-core`'s cache tests
  (rebinding a hit, another source, a range or name that does not fit, format 5 a miss,
  the stored record the extraction's own) and the identity vectors above.
- **Mutations killed:** BV (eager argument copy), BW (eager callee copy), BX (eager
  Python attribute name), BY (texts spelled out when cached), BZ (spelled out in
  conversion), CA (a whole name a definition), CB (the member's bytes reversed), CC (the
  separator, in Rust and in C), CD (an extension taken as a member), CE (a range past
  the source accepted from the cache), CF (format 5 read as 6), CG (binding without the
  digest), CH (protocol 6 accepted), CI (the worker spelling texts out), CJ (eager
  parameter-list copy), CK (no prefix search), CL (a prefix one byte long), CM (any
  prefix bound), CN (a prefix spelled unchecked).

Residuals, none of them retained by an extraction:

- **Engine working copies.** The engine still copies callee names, usage names and
  qualified names for its own passes while it extracts; the copies go with the engine's
  result, which conversion frees, but they are in the peak (the Python chains' peak at
  0.5 MiB is 560, 620, 675 and 741 MB at 50 to 400 links).
- **Typed resolution** holds text for the duration of a project: the resolvers' own
  chain text (the TypeScript resolver's, 366 MB on the JavaScript chains) belongs to
  issue 92's mechanism, and a project decodes and binds every file's surface.
- **The surface** is linear in the facts but large per fact (445 MB of the 2 MiB C
  file's record), issue 82's retention.
- **Node-type paths** grow with a site's depth, bounded by issue 88's budget of 500,000
  entries a file; a nested class's 4 KB arena stack is linear in the classes.
- **The stored graph** keeps whole names and parameter lists in its node rows (4.3);
  so does Stage 4's graph that writes them. Holding them otherwise is a change of
  stored format, not this issue's.

State: resolved. 90A is fixed by patch 0022 and 90B by patch 0024. The closure of
issues 86, 87 and 89 stays held for the final engine-7 audit; issue 92 is open.

### 89 — Extraction's work and memory are superlinear in syntax nesting

Found by issue 88's audit of every collection an extraction stores (its brief's §41),
with that issue's path budget in force. Each shape below is a few KB of source,
measured at nesting n of 1,000, 2,000 and 4,000:

| Shape | Language | Peak memory | Time | Growth |
|---|---|---|---|---|
| `f(f(...f(0)...))`, argument text | C | 12, 19, 40 MB | — | 1.5 n² bytes of argument text |
| `x.f().f()...`, callee text | Python, TypeScript | 20, 45, 143 MB (Python) | 0.4, 1.5, 6.0 s (Python) | 2 n² bytes of callee text |
| nested namespaces | C++ | 20, 53, 189 MB | 0.09, 0.35, 1.38 s | ×4 |
| nested closures as arguments | JavaScript | 16, 32, 90 MB | 0.2, 0.8, 3.3 s | ×3 to ×4 |
| nested `new E(...)` thrown | Java | 15, 27, 71 MB | — | ×2.6 |
| nested classes | Java | 28 MB | 56 s at 1,000; over 120 s at 2,000 | time cubic |
| nested functions | JavaScript | 12 MB | 45 s at 1,000; over 120 s at 2,000 | time cubic |
| member chain `a.b.b...` | Python | 11 MB | 50 s at 1,000; over 120 s at 2,000 | time cubic |
| member chains, generic types, nested `if`, `+` chains | TypeScript, Java, C, Go | small | 1.4 to 2.7 s at 4,000 | time ×4 |

- **Memory.** A call stores its argument text and its callee text as written. Every
  inner call is inside its outer call's argument, and every link of a chain inside the
  next link's callee, so the stored text is quadratic in the nesting.
- **Time.** Sampling the Java and Python cases puts 89% to 90% of samples in
  `ts_node_child_with_descendant`, under `ts_node_parent`, under
  `handle_usages`, `record_lexical_binding` and `usage_lexical_scope_id_for_node`. For
  every usage, the lexical-scope lookup climbs to the root one `ts_node_parent` at a
  time, and each `ts_node_parent` descends from the root again: depth² per usage,
  O(n³) in all. 400 levels take 3.1 to 3.4 s, 1,000 levels 50 to 56 s.

Whether such a file extracts on a given machine, or within an isolated worker's
timeout (issue 27: a timeout fails the build), depends on the machine: issue 87's and
88's class, outside both. It predates this hotfix and is on main. The census of 104,730 files that issue 88 ran
extracted every one under a 1.2 GB memory cap; it did not record per-file times.

The driver decided it must be fixed before engine 7 reaches main, by one deterministic
admission ceiling rather than a fix per mechanism (decision 58). The fix is commit
`e1eb0c6`, patch 0021:

- **The scan.** `pdxe_syntax_depth_scan` walks the parsed tree once with a cursor, depth
  moving by one each step (every node counted, the root at depth 1), and stops at the
  first node deeper than `PDXE_MAX_SYNTAX_DEPTH`, 512. It inspects every node whatever
  the walk's operator budget: one cursor step a node, in a tree linear in the source the
  file-size limit admits.
- **Where.** In `extract_file_ex_body`, right after the parse and the path-only module
  name and test flag. Every semantic pass comes after it: definitions, imports, the
  unified walk, channels, k8s, dbt, the per-file resolvers, the late-site stamps and the
  expanded pass.
- **Past the ceiling.** The result is valid and holds no fact: no definition, call,
  usage, import, resolved call or path. It counts no semantic pass, records lost work
  once, sets `lsp_skipped` so cross-file resolution skips it, releases its tree and is
  counted done. It is not a parse failure. It is never cached and degrades its segment.
- **The expanded tree** of C, C++ and CUDA gets the same scan right after its parse;
  past the ceiling, that pass alone is skipped, losing work once.
- **The ceiling.** Of the 104,730 census files, p99 is 38 and p99.9 is 65. The deepest
  admitted files are 262 for ordinary source (`hono/src/utils/headers.ts`), 452
  generated and 470 parser recovery. The 13 files past 512:
  - parser recovery: `intel-ucode-defs.h`, `test_bpf.c`, `adf_admin.c` (an unexpanded
    `__aligned(1024)` turns a 1,024-byte table into a 1,023-deep comma expression; the
    same table without it is 7 deep), `hal_pwr_seq.h`, `jz4780-cgu.c`, two `pwrseq.h`
    headers and `enums_abi.autogen.h`;
  - generated Go: two Unicode tables and two protobuf files, each a long string
    concatenation;
  - expanded text: `test-arm.c`, whose expanded pass alone is skipped.

  Exported surfaces are byte-identical to `e7865a0` for 104,716 files. The 14 that
  differ are those 13 and `stackinit_kunit.c`, which issue 87's factor now admits.
- **Before and after**, by structure, never by timing in a test:
  - Java classes 1,000 deep: 56 s before, under 0.01 s after.
  - JavaScript functions 1,000 deep: 45 s before, under 0.01 s after.
  - A Python member chain 1,000 long: 50 s before, under 0.01 s after.
  - A Python method chain 10,000 long: 38 s and 1.6 GB before, 0.01 s and 11 MB after.
- **Tests.**
  - `syntax_depth` (engine):
    - the scan: the root alone is 1, `int v;` is 3, k parentheses are k + 4, every node
      visited once, stopping at the first node past a depth or count, recovery on the
      path;
    - depths 511 and 512 extracted, 513 refused at the 513th level with no semantic
      pass, fact or path;
    - the four shapes, refused at 300 levels and extracted at 150;
    - the expanded tree refused alone;
    - each way with issue 88.
  - `pdx-engine`'s `syntax_depth`: the same on every platform.
  - `pdx-core`: never cached, cold/cold/warm equal; the segment degraded by lost work
    alone.
- **Mutations.** Each restored byte for byte; all killed:
  - no preflight (AX), an unbounded ceiling (AY), the check after the unified walk (AZ)
    or between the imports and the walk (BA: "semantic work ran (3 passes, 2
    definitions)"), no lost work (BB), a refusal by the clock in place of the depth
    (BC), and the passes run on after the marker (BD).
  - Each was killed by `syntax_depth`, and all but BA by `site_path_budget`'s "255
    nested calls were not refused before any path" too.

Closure is held. The closure audit found issue 90. Within the admitted depth, usage
handling still costs depth² per usage, and a file holds as many deep constructs as it
likes, so a few KB can still take tens of seconds. The ceiling bounds each construct,
not the file.

The final engine-7 audit, issue 92's (2026-10-13), found issue 93: an error-recovered
file whose parse leaves one wide error node costs superlinear work, past an isolated
worker's timeout at 1 MiB. Closure is still held.

State: open.

### 88 — Extraction's memory is quadratic in how deeply call sites nest

Found by issue 87's security test, which first nested a macro call 10,000 deep. The
preprocessing stopped at its depth ceiling, as it should. Extracting the file without
the expanded pass then took 6.7 GB in `build_segment`. Under an 8 GB address-space
limit the file failed on an allocation; under a 6 GB memory cap the build was killed.

It is not preprocessing, and not C. Every call site stores the node types from its owner
down to its own node (`ast_path`, patch 0010, issue 46), so a call n levels deep stores
about 2n entries. n calls nested inside one another store about n² entries:

| Source | Calls | Stored entries | Longest | Engine peak |
|---|---|---|---|---|
| `f(f(...f(0)...))`, C, 2,500 deep | 2,500 | 6,257,500 | 5,002 | 70 MB |
| the same, 5,000 deep | 5,000 | 25,015,000 | 10,002 | 248 MB |
| the same, 10,000 deep (30 KB) | 10,000 | 100,030,000 | 20,002 | 954 MB |
| Python, 10,000 deep | 10,000 | — | — | 956 MB |

Nesting without calls is cheap: 10,000 parentheses in C, or 10,000 nested Python lists,
take 10 to 15 MB. The cost is per site. Two more shapes show it:

- `f(0)+f(0)+...` 10,000 times, a left-deep expression, takes 418 MB.
- A 10,000-call Python method chain takes 1.6 GB and 39 s, and loses work to the type
  evaluator's budget.

The pipeline peaks several times higher than the engine (6.7 GB against 954 MB),
consistent with the entries being copied again as the extraction crosses into Rust.

Whether such a file extracts therefore depends on the machine's memory, from repository
bytes alone: issue 87's class, in extraction rather than preprocessing. The node budget
does not reach it, since 10,000 nested calls are about 50,000 nodes. No corpus here has
it: across the 175 C-family files of the fixtures, the sanitizer corpus, the jq and fmt
goldens and Kubernetes, the most any file stores is 96,362 entries (fmt's
`gmock-gtest-all.cc`, 12,498 calls), and the longest path is 214. Other languages and
Linux were not measured.

It predates this hotfix: it is on main, in every language, since patch 0010. Patch 0019
does not touch it.

The driver decided it must be fixed before engine 7 reaches main, by deterministic
evidence budgets rather than shared prefixes, which would reach the interface, the cache
and surface codecs and the Rust model (decision 57). The fix is commit `f3300e0` on
`hotfix/issue-83-c-family-input-isolation`, patch 0020:

- **Complete or absent.** A path is never cut, since it is part of a site's identity;
  a site whose path is refused has none, is counted (`Missing::AstPath`) and never given
  a stored identity or row. No empty path is ever hashed (`IdError::EmptyAstPath`).
- **One budget per file.** `PDXESitePathBudget`, in the result, serves every producer
  through `pdxe_site_path_alloc`: the walk's calls and possible callable references
  (`current_site_path`), sites added after the walk (`late_site_path`,
  `owner_site_path`) and route bindings (`route_site_path`). Owner aliases re-root an
  existing path and allocate nothing.
- **Ceilings.** At most 1,024 node types a path, and 500,000 kept across the file,
  terminators not counted, checked before anything is allocated.
- **Exhaustion.** The first refusal records lost work once. The budget is then spent:
  no path is computed or allocated for the file after it (each producer asks
  `pdxe_site_paths_exhausted` first, and a late path's sizing climb stops one node
  past the deepest path allowed). The sites themselves remain.
- **A late allocation removed.** A late site whose owner was undecided was given a path
  by a nearest-function climb that `stamp_late_site_owners` always replaced before
  anything read it: never observable, serialized, hashed or resolved. That allocation is
  gone, so the budget counts what is kept. The Kotlin delegate's late `getValue` path is
  pinned in a test exactly as engine 7 gave it before.
- **Evidence.** The census covered 104,730 files (engine fixtures, sanitizer corpus,
  the ten goldens, Kubernetes, Linux). Only Linux's `lib/test_bpf.c` passes a ceiling:
  2,265,760 entries and a 2,100-deep path, from tree-sitter error recovery turning a
  large initializer into a 2,094-deep `comma_expression`. Since issue 89 it is refused
  for its depth before any path is asked for. The closest below:

  | File | Measure | Share of its ceiling |
  |---|---|---|
  | `hal_pwr_seq.h` | 948-entry path, a recovery artefact | 93% |
  | Exposed's minified `main.js` | 339,520 entries | 68% |
  | Kubernetes' `generated.pb.go` | 294,692 entries | 59% |

  p99.9 is 66,773 entries and a 53-entry path. Exported surfaces were byte-identical
  to `e7865a0` for 104,729 of the 104,730 files, `test_bpf.c` alone excepted.
- **Memory.** `plain10000.c` fell from 954 MB to 174 MB; the rest was issue 89, and
  since it, 12 MB.
- **Tests.**
  - `site_path_budget` (engine):
    - the builder at 1,024 and 1,025 entries, and at 499,999, 500,000 and 500,001,
      with no request after exhaustion;
    - a total crossed exactly where the grammar puts it (499,948 entries in 4,873
      paths), no path after the first absent one, every kept path complete;
    - Python routes, calls and references on one counter;
    - 254 nested calls kept whole, 255 refused for their depth before any path.
  - `pdx-engine`'s `site_path_budget`: C, Python, Java and JavaScript, exactly one loss
    more than the same function under the budget; the late path pinned.
  - `pdx-core`: never cached, cold/cold/warm equal; the segment degraded by lost work,
    every path-less site counted and none stored.
- **Mutations.** Each restored byte for byte (checksums compared); all killed:
  - no total-entry check (AQ): the 500,001st entry is given;
  - no per-path check (AR): a 1,025-entry path is given;
  - the check after allocation (AS): the arena is asked once more than it gives;
  - a counter of its own for routes (AT): the counter and the paths kept disagree;
  - asking again after exhaustion (AU): a path follows the first absent one;
  - a cut path in place of none (AV): an incomplete path is found;
  - no `pdxe_lost_work` (AW): no work is lost.

State: resolved.

### 87 — C-family preprocessing's work and memory are unbounded by the source

Found by issue 86's audit. Two ways a few hundred bytes of C or C++ make the
preprocessing pass take as much memory and time as they like:

- **`#line` padding.** The library writes its expanded text with one newline for every
  line a forward `#line` skips within one file (`TokenList::stringify`). The engine then
  maps every expanded line and parses the text. A three-line source with
  `#line 400000000` expands to 400 MB, peaking at 2.7 GB in 4.3 s; with
  `#line 50000000` it is 50 MB and 345 MB, and with 1,000,000, 1 MB and 10 MB. These were
  measured under a 3 GB cap. The largest number valid C allows, 2147483647, would need
  about 14 GB, and issue 86's clamp makes 4294967295 reachable. A `#line` that names
  another file, or moves backwards, is written as a marker instead, and costs nothing.
- **Macro expansion.** Doubling macros (`#define A1 A0 A0`, ...) grow exponentially. A
  437-byte source of 22 levels expands to 8 MB of text and peaks at 1.3 GB in 9.8 s.
  Every two further levels multiply both by four.

Whether such a file extracts therefore depends on the machine's memory and speed. One
machine finishes; another runs out of memory. An extraction in the indexing process ends
the process, while an isolated worker crashes or times out, failing or degrading the
build. That is also a denial of service through repository content, which Stage 2
treats as untrusted.

It predates this hotfix: the reference engine and engine 6 behave alike. No corpus
here has it: the largest `#line` in 66,008 C-family files is 3921.

The driver decided it must be fixed before engine 7 reaches main, as untrusted-input
denial-of-service prevention, with no timeout, no environment override and no new
status (decision 56). The fix is commit `52c19c6` on
`hotfix/issue-83-c-family-input-isolation`, patch 0019:

- **`#line`.** A forward jump of more than 4,096 lines within one file is written as one
  `#line N "file"` marker, as a jump back already was; the line map maps the marker to 0
  and the next line to N, so it is never a fact. A shorter jump is padded as before.
  `#line 400000000`, 2147483647 and 4294967295 now expand to 76 to 78 bytes in 4 lines.
- **Ceilings.** The library carries a `Budget` per thread, charged inside the work. The
  byte budget is `clamp(16 × len, 256 KiB, 8 MiB)`, compared before multiplying. It bounds
  the bytes written (padding and markers included), the token text expansion generates
  (temporaries included) and the `#if` folding steps. Expansion may generate at most
  500,000 tokens; the source's own tokens are never charged, only those created while a
  top-level expansion runs. Three ceilings were added in implementation, for amplifiers
  the first two do not reach: at most 500,000 expansions (doubling macros that end in an
  empty macro create nothing and took exponential time), nesting at most 256 deep
  (`F(F(...))` 10,000 deep overflowed the stack), and a ternary's restart counted as a
  folding step (a long `#if` was quadratic).
- **Exhaustion.** Passing a ceiling throws at its first step past it, from inside the
  expansion, the fold or `stringify`, before the next allocation; none of the library's
  handlers catches it. The engine returns no expansion (never a truncated one), counts
  `pdxe_lost_work` once, and extracts the file from its whole normalised source without
  the expanded pass. `extraction_lost` is 1, so the extraction is never cached and
  degrades its segment, by the existing rules.
- **Arithmetic.** `#if` arithmetic is 64-bit two's complement on unsigned values: `+`,
  `-`, `*` and unary minus wrap, `INT64_MIN / -1` is `INT64_MIN` and `% -1` is 0, a shift
  of 64 or more is 0, a negative value shifts right arithmetically (written out), and a
  negative count fails the condition as a division by zero does. Unary minus negates the
  number: `-(-5)` was the text `--5`, read as 0. Integer literals are read by an explicit
  reader with libstdc++'s results (equal on 2,940,896 fuzzed strings); the stream it
  replaces is the C++ library's own (Microsoft's on Windows), and left its result
  unassigned for empty text.
- **Tests.** Each fails or crashes before the fix and passes after. None needs a variable
  or switch: the bytes alone.
  - `preprocessor_bounds` (C++) checks every library ceiling at its use minus one, at it
    and plus one. A doubling bomb stops one token past its ceiling, with nothing yet
    written; padding is checked at 11 and 10 lines; and the arithmetic model.
  - `c_family_preprocess_budget` checks the formula at its boundaries, and against a
    64-bit reference at every length up to 600,000.
    - `#line` 400000000, 2147483647 and 4294967295: one marker, mapped exactly, finding
      the facts a near `#line` finds.
    - Jumps of 4095, 4096 and 4097 lines.
    - The engine's own ceilings at minus one, at and plus one: bytes written, bytes
      generated, tokens generated, expansions and depth.
    - Bombs below and above; above, lost work exactly once, the same each run.
    - A near-2-MiB normal source preprocesses, at 71% of the token ceiling.
  - `pdx-engine`'s `preprocess_bounds` runs on Linux, macOS and Windows: nesting 256 and
    257 deep, 80,000 parentheses, and the arithmetic, including out-of-range literals.
  - `pdx-core` covers the cache and the build:
    - an exhausted extraction is never cached, and is equal cold, cold again with
      another cache, and warm;
    - a build with one is degraded by lost work alone;
    - `repository_bytes_alone_cannot_exhaust_the_indexer` is the security test.
- **Mutations.** Every one was restored byte for byte (checksums compared). All are
  killed but AJ:
  - full padding (AA), no marker path (AB) and a huge threshold (AC): OOM-killed under
    a 2 GB cap, or the far-`#line` tests fail;
  - no byte budget (AD), no token budget (AE), limits checked only after `stringify`
    (AF), a truncated expansion (AG) and no `pdxe_lost_work` (AH): the exact-edge,
    stop-point and lost-work tests;
  - native signed overflow (AI): UBSan;
  - shifts of 64 or more native (AK): the boundary tests and UBSan;
  - `INT64_MIN / -1` native (AL): SIGFPE, and UBSan;
  - the added ceilings, removed: no expansion count (AM; the empty bomb times out), no
    ternary charge (AN) and no depth ceiling (AP), all killed.

  AJ, a native right shift of a negative value, survives as an equivalent mutant. gcc,
  clang and Microsoft's compiler all shift negatives arithmetically, and C++20 requires
  it, so no behavioural test can tell it from the written-out form.
- **Headroom.** No C-family file passes a ceiling or needs a marker among the fixtures,
  the sanitizer corpus, the jq and fmt goldens and Kubernetes; the other goldens and
  fixtures hold none.
  - Largest source 532,839 bytes; largest expansion 235,614 bytes; largest ratio 1.74.
  - Most generated tokens 6,343, expansions 902 and folding steps 968; deepest nesting
    7; largest jump 1,515 lines.
  - The closest file to a ceiling is fmt's `printf-test.cc`, at 10.9%.

  Of Linux's 64,432 C-family files, 5 pass a ceiling. `net/mac80211/airtime.c`,
  `lib/tests/stackinit_kunit.c`, `tools/testing/selftests/bpf/uprobe_multi.c` and
  `arch/s390/lib/test_modules.h` pass on generated bytes, `arch/powerpc/xmon/ppc-opc.c`
  on generated tokens. 11 files get a marker, for jumps of 4,239 to 33,345 lines. Those
  5 files would lose work and degrade a Linux index; the limits were not retuned.
- **Residual.** The raw token list of the source itself is not charged, as specified,
  and grows linearly. 1.2 MB of parentheses peaked at 420 MB before stopping at its
  budget; a 2 MiB source is about 600 to 700 MB.
- **Audit.** Every amplifier after patch 0019:
  - recursion: expansion nests at most 256 deep, under 512 KiB of stack; the other
    loops are iterative;
  - token cloning, temporaries, pasting and stringification: charged as generated
    tokens and bytes;
  - padding: each jump is at most 4,096 lines, and the whole is in the written-byte
    budget;
  - macro cycles: the library's expansion set, then the ceilings;
  - conditional nesting: a stack linear in the source, and folding counted;
  - integer behaviour: no signed overflow or native shift remains in `#if`, and
    UBSan is clean;
  - data model, files, clock, cwd and environment: patches 0015 to 0018, and the only
    variable read is the test-build seam;
  - locale: numbers are formatted in the classic locale and read explicitly; the C
    locale functions run in the C locale, since neither the engine nor its host calls
    `setlocale`.

  The unmodified library's own deterministic quirks remain: `- -5 == 5` and `!00` are
  false.

- **The factor, reviewed.** The driver reviewed the five Linux files and changed one
  constant before engine 7 was delivered: the byte budget is 24 bytes per source byte,
  not 16 (decision 56, amended; commit `291e435`, patch 0019 carrying 24).
  - `lib/tests/stackinit_kunit.c` is ordinary KUnit test source. It generates 398,632
    bytes, 17.9 times its 18,591, and carries 215 callees nothing else gives. Its
    budget is now 446,184, and it expands.
  - The other four stay bounded, and are amplification by design:
    - `airtime.c`: constant tables, 232 times its size;
    - `uprobe_multi.c`: 50,000 generated functions;
    - `test_modules.h`: a 10,000-fold repetition macro;
    - `ppc-opc.c`: past the token ceiling, which stays at 500,000. Its 1,782,252
      tokens and about 432 MB buy 18 calls and no new callee.
  - Across Linux's 64,432 C-family files those four are the only ones past a ceiling.
    The normal corpora have none.
  - The budget's comparison is now exact for a factor that does not divide 8 MiB: a
    length above 8 MiB / 24, rounded down, has the maximum. The 64-bit reference sweep
    caught the old form at 349,525 bytes.
  - The mutations were re-run after the change. All are killed but AJ, the equivalent mutant, with each restored byte for byte:
    - AA to AN and AP again;
    - the factor back to 16 (BE): the recorded ceiling;
    - the comparison back to `>=` (BF): "a source of 349525 bytes has a budget of
      8388608, not 8388600".

Closure is held. Its security test found issue 88, which is resolved, and the closure
audit found issue 89 and then issue 90: machine-dependent extraction outside
preprocessing.

The final engine-7 audit, issue 92's (2026-10-13), found issue 93: an error-recovered
file whose parse leaves one wide error node costs superlinear work, past an isolated
worker's timeout at 1 MiB. Closure is still held.

State: open.

### 86 — Preprocessing keeps two more host data-model dependences

Found by issue 85's audit of every preprocessing value that depends on the platform the
engine is built for. Neither is a type size.

- **Plain `char` signedness.** In `#if` and `#elif`, the library evaluates a single-byte
  character constant as `static_cast<char>`. So `'\xff'` is -1 where plain `char` is
  signed and 255 where it is unsigned. It is signed on linux-x86_64, darwin-arm64,
  darwin-x86_64 and windows-x86_64, and unsigned on linux-aarch64, one of the five
  targets specification 4.14.1 names. `#if '\xff' < 0` is valid C and C++, whose value
  is implementation-defined, and it takes opposite branches on the two. This was shown
  by building the library twice, once with `-funsigned-char`.
- **`#line` numbers wider than 31 bits.** `#line N` and `# N "file"` read the number
  with `atol`, which returns a C `long`: 64 bits on Linux and macOS, 32 on Windows.
  `#line 4294967297` makes the following `__LINE__` 1 on LP64, the value cut to the
  library's 32-bit line. Microsoft's `atol` returns `LONG_MAX` on overflow, so on
  Windows it would be 2147483647. That is from its documentation; it was not run here.
  C limits `#line` to 2147483647, so only an invalid directive reaches this.

Same input, same engine version, different extraction on two specified targets: the
determinism claim of issues 83 to 85 does not hold on all five until both are fixed. The
census that issue 85 ran finds neither construct in jq, fmt, Kubernetes, Linux, the
engine's fixtures or the sanitizer corpus. The fix is a fixed choice in the engine, for
example plain `char` signed as on four of the five targets and `#line` read at a fixed
64-bit width as on LP64. It is an engine change and, at the driver's choice, part of
engine 7.

Also found, and not a host dependence in practice: `#if` arithmetic that overflows, and
shifts by 64 or more, are undefined in the library's C++. They give the same result on
x86-64 and AArch64 (two's complement; shift counts taken modulo 64).

The driver decided both must be fixed before engine 7 reaches main (decision 55). The fix
is commit `b14a194` on `hotfix/issue-83-c-family-input-isolation`, patch 0018:

- **Character constants.** A one-byte narrow constant is a signed 8-bit char, written
  out: 0x00 to 0x7f are 0 to 127, and 0x80 to 0xff are -128 to -1. No `char`, `CHAR_MIN`
  or compiler flag decides it.
  - **Multi-character constants.** These are a 32-bit two's complement int, also written
    out. They had those values on every target, since `int` is 32 bits on all five and
    their compilers wrap, but by implementation-defined conversion.
  - **Code units.** A narrow code unit is 8 bits, where it was `CHAR_BIT`.
  - **Unchanged.** `u`, `u8`, `U` and `L` constants keep their unsigned code points. `L`
    was always the library's 32-bit rule, never the host's `wchar_t`. Escapes are read
    through `strtoull`, 64 bits everywhere. Bytes are read as unsigned char, and values
    are built by shifts, never memory order.
- **`#line` numbers.** One helper, `lineDirectiveNumber`, reads every `#line N`,
  `#line N "file"` and `# N "file"` number. It takes the token's leading decimal digits,
  as `atol` read them, and accumulates them at 64 bits. A value above 4294967295, the
  largest line a location holds, is clamped to 4294967295. The library had no rule of
  its own: `atol` gave LP64 the value modulo 2^32 (or `LONG_MAX`'s, 4294967295, beyond
  2^63) and Windows `LONG_MAX`, 2147483647. Values up to 4294967295 now read exactly,
  so `#line 2147483648` is that line everywhere, and `#line 4294967296` is 4294967295
  everywhere.
- **Tests.** Each failed before the fix and passes after.
  - `preprocessor_host_independence` builds the library twice, with `-fsigned-char` and
    with `-funsigned-char`. The second is the Linux AArch64 case on any machine. Both
    builds check:
    - 28 constants: every one-byte boundary in hex, octal and raw bytes, multi-character
      constants, and the other kinds;
    - `#if` and `#elif` branches;
    - 15 `#line` numbers in all three forms, read from token locations.

    The unsigned build failed 14 checks before the fix, and the signed one 4.
  - `c_family_char_literals` runs full C and C++ extractions whose calls are chosen by
    character constants, and by `#line 2147483648` and `#line 4294967296`. Those two
    values separated the old Windows and LP64 readings. Each directive names a file of
    its own, so the expanded text is never padded (issue 87).
  - `pdx-engine`'s `literals_and_lines` asserts the same expectations on Linux, macOS and
    Windows, with no platform conditional.
- **Mutations.** All killed:
  - native `char` (V): the unsigned-char build;
  - unsigned constants (W): every build and the Rust test;
  - `atol` (X), a 32-bit `long` limit (Y) and a 32-bit wrap (Z): every build, the full
    extraction and the Rust test.
- **Census.** The text census found two `#if` lines with a character constant, both in
  jq and neither with a high byte. It found 488 `#line` directives, the largest 3921.
  The census covered 66,008 C-family files in jq, fmt, Kubernetes, Linux, the engine
  fixtures and the sanitizer corpus.

Closure is held. The same audit found issue 87: how much work and memory preprocessing
takes is unbounded by the source, so whether an extraction completes depends on the
host. The driver set the closure condition that no known host property may change
extraction for the same inputs.

Issue 87 is fixed (patch 0019, decision 56, its factor 24 since `291e435`). Its
security test found issue 88, now resolved (patch 0020). Issue 89's depth admission is
delivered (patch 0021), but the closure audit found issue 90 within it, so closure is
still held.

The final engine-7 audit, issue 92's (2026-10-13), found issue 93: an error-recovered
file whose parse leaves one wide error node costs superlinear work, past an isolated
worker's timeout at 1 MiB. Closure is still held.

State: open.

### 85 — Preprocessing evaluates `sizeof` in `#if` with the engine's own type sizes

Found by issue 84's audit of everything C and C++ preprocessing predefines or evaluates.
The preprocessing library evaluates `sizeof(type)` inside `#if` and `#elif` from the
size each type has in the build of the engine binary itself:

- `long` is 8 bytes in the Linux and macOS builds and 4 in the Windows one.
- `long double` is 16 bytes on x86-64 Linux and 8 on Apple silicon.

So a source whose `#if` asks one can extract differently on the three platforms under
one `ENGINE_VERSION`. No build key or cache key names the platform (specifications 4.3
and 4.5).

It is no ambient input: one engine binary always gives one answer and reads nothing at
run time to make it. It is also inert in practice. `sizeof` in `#if` is invalid C and
C++, which no compiler accepts, and jq, fmt and all 95,723 files of the Linux scale
checkout hold none (the one match is in a comment). Nothing else in the preprocessing
depends on the build target: absolute-path syntax does, but it only ever served include
lookup, which issue 83 removed.

Whether an extraction must be identical across build targets is unspecified. Making it
so is a fixed type-size table in the engine, an engine change. The driver's decision.

The driver decided it must be fixed before engine 7 reaches main (decision 54). Commit
`ef6a367` on `hotfix/issue-83-c-family-input-isolation`:

- **Root cause.** The library's preprocessing builds its `sizeof` table from a caller's
  table (`TokenList::sizeOfType`, which PDX never fills) and then the native `sizeof` of
  twenty spellings: `char`, `short`, `short int`, `int`, `long`, `long int`, `long long`,
  `float`, `double`, `long double`, and a pointer to each. `simplifySizeof` drops
  `signed` or `unsigned` before a name and a second `*`, joins the rest with spaces and
  looks the spelling up. A spelling it does not find is left unevaluated, and the
  condition then fails to evaluate: `void *`, `wchar_t`, `size_t`, `ptrdiff_t`, `bool`,
  `_Bool`, a lone `signed` or `unsigned`, `long long int`, qualified, array and struct
  types. It holds for every platform alike, and is unchanged.
- **Old values.** char 1, short 2, int 4, long long 8, float 4, double 8 and pointers 8
  on all five targets. Long was 8 on Linux and macOS and 4 on Windows. Long double was
  16 on Linux (x86-64 and AArch64) and Intel macOS, and 8 on Apple silicon and Windows.
- **Model.** LP64 as 64-bit Linux has it, written out: char 1, short and short int 2,
  int 4, long and long int 8, long long 8, float 4, double 8, long double 16, every
  pointer 8. Patch 0017 adds the setting `fixedSizeOfTypes`, which the engine always
  sets and which replaces the whole table, so neither the build's sizes nor a caller's
  can enter. `ENGINE_VERSION` stays 7: patches 0015 to 0017 together are engine 7.
- **Cache.** Host ABI is no extraction input and is not in the key, so a cache filled on
  any platform is right on every other. The key vectors do not change: they hold
  versions and file identity only.
- **Tests.** `engine/tests/c_family_sizeof.c` runs under this build's sizes and four
  simulated ones: 64-bit Windows, Apple silicon, a 32-bit build, and every size wrong.
  The test seam `PDX_ENGINE_TEST_HOST_SIZES` simulates them, in test builds only, by
  handing the library another platform's sizes as a caller's table. The test covers:
  - the fixed size of all 41 spellings and folded forms, in C and C++;
  - unrecognised spellings staying unevaluated;
  - a bare `sizeof` giving one expansion;
  - a full extraction whose `#if` and `#elif` choose calls by `sizeof`.

  It failed 156 checks with the seam and without the fix.

  `pdx-engine`'s `type_sizes` asserts the fixed model's branches, with the same
  expectations on every platform. Simulated platforms in child processes change no
  extraction. `pdx-core`'s `c_family_preprocessor_sizeof_is_platform_independent` runs
  Stage 2 cold, warm and fresh, and requires the fixed model's branch. The Windows CI job
  runs both Rust tests, so Windows must select `long` 8 and `long double` 16, as Linux and
  macOS do. On Apple silicon `long double` was 8, so the macOS job proves that too.
- **Mutations.** All killed: a Windows-style `long` of 4 (Q), native pointer sizes (R), a
  table conditional on the platform (S), the setting off (T), and `long double` left to
  the build (U).
- **Census.** A text scan of every C-family file in the cached golden and scale
  checkouts, the engine's fixtures and the sanitizer corpus found no `sizeof` in `#if`.
  That is 64,685 files in jq, fmt, Kubernetes and Linux, and 16 more; the one match
  earlier counted in Linux is in a comment.
- **Correction.** "Nothing else in the preprocessing depends on the build target" above
  was wrong. Issue 86 records two more dependences.

State: resolved.

### 84 — C-family preprocessing expands `__DATE__` and `__TIME__` from the clock

Found by issue 83's audit of what `pdxe_extract_file` reads. The preprocessing library
predefines `__DATE__` and `__TIME__` for every C, C++ and CUDA source it expands, from
`time()` and `localtime()`. `localtime` reads `TZ`, and without it the system's zone
file, so the values depend on the clock and the process's environment.

A macro the source defines that expands either puts the value into the extraction.
`#define STAMP() record_stamp(__DATE__, __TIME__)` and `void f(void) { STAMP(); }`
give a call `record_stamp` (`origin` 1) whose first string argument and arguments are
`"Oct 12 2026"` and `"09:15:31"`. The same bytes therefore extract differently on
another day, at another time or in another zone. No cache key names those inputs, so a
cached extraction keeps the day it was made, while a fresh one does not. That also
contradicts specification 4.5, which issue 83 clarified: an extraction depends on the
normalised bytes, the language and the path alone. It reads no source content, so it is
outside issue 83's scope and was not fixed there.

A fix predefines both with fixed values in the engine, an engine change that moves
`ENGINE_VERSION`. It is the driver's decision, as is whether it rides with issue 83's
engine 7.

The driver decided both (decision 53), and the fix is on
`hotfix/issue-83-c-family-input-isolation`, commit `fe536c8`:

- **Root cause.** The library's preprocessing inserted `__DATE__` and `__TIME__` from
  `time` and `localtime_r` (`localtime_s` on Windows) through `strftime`'s `%b %d %Y` and
  `%H:%M:%S`, so from the clock, the zone (`TZ`, else the system's zone file) and, for
  the month name, `LC_TIME`.
- **Audit.** Of everything the library predefines, only `__DATE__` and `__TIME__` were
  time-derived. The rest are `__FILE__` (the path given), `__LINE__` and `__COUNTER__`
  (from the source), `__STDC_VERSION__` or `__cplusplus` (from the configured standard),
  `__STRICT_ANSI__` and `__has_include`. It predefines no `__TIMESTAMP__`,
  `__BASE_FILE__` or `__INCLUDE_LEVEL__`, reads no file time, and takes nothing from a
  hostname, user, process id, the working directory or the environment. One
  build-target dependence was found: issue 85.
- **Patch 0016.** A preprocessor setting, `fixedDateTime`, which the engine always sets:
  `"Jan 01 1970"` and `"00:00:00"`, the start of the Unix epoch in the library's own
  formats, written out rather than formatted. Nothing replaces the clock: no
  `SOURCE_DATE_EPOCH`, Git or file time is read. A source's own `#define` or `#undef` of
  either keeps its precedence, and an `#ifdef` still sees both defined. `ENGINE_VERSION`
  stays 7: issues 83 and 84 together are engine 7, never delivered before both. The
  cache-key vectors are unchanged, since a key holds versions and file identity only.
  Specification 4.5 now says an extraction depends on no clock, time zone or locale.
- **Tests.** Both failed on the clock-reading engine and pass now.
  `engine/tests/c_family_date_time.c` failed 20 times there, with `"Oct 12 2026"` and
  `"10:15:35"`. It requires the exact expansion in C and C++; rows and surface
  byte-identical under four POSIX TZ rules (UTC, New Zealand, US Pacific, +05:45) and
  every installed locale; a source's own definitions winning; and `__TIMESTAMP__`
  staying a name. The `pdx-core` `extract` test
  `date_and_time_cannot_change_a_cached_extraction` requires a cold cache under
  `TZ=UTC0`, a warm hit under `NZST-12` and a fresh run under `PST8` to give one
  extraction, with the call's arguments `"Jan 01 1970"` and `"00:00:00"`.
- **Locale.** The first macOS CI run (38225693037), on a machine with German, French
  and Polish locales, failed the engine test. The cause was the test's own row printer,
  which writes a score with `printf` and so used the decimal comma under them. The
  engine's extraction was byte-identical under all three: reproduced on Linux with those
  locales built under `LOCPATH`, holding `LC_NUMERIC` at C changes nothing else. The
  rows are now printed in the C numeric locale while the engine runs under the locale
  being tested. A `__DATE__` formatted from a fixed epoch through `%b` fails the test
  under the French and Polish locales (mutation P). PDX never sets a locale, so its
  processes run in the C locale whatever `LC_*` says.
- **Mutations.** All killed: the clock `__DATE__` (K) and `__TIME__` (L); the epoch
  through `localtime`, so the zone moves it (M); a clock `__TIMESTAMP__` (N); the
  setting off at engine 7 (O); and a fixed date formatted through the locale's month
  name (P).

State: resolved.

### 83 — C-family preprocessing reads included files from disk

P2-16's engine differential first ran the vendored engine with the workspace root as
its working directory and the reference engine in the staged corpus root. Every
golden subset agreed but jq's and fmt's, where the sites found in the macro-expanded
text (`origin` 1) were the same calls, callers and arguments at other lines and bytes:
`abort` in `memory_exhausted` of jq's `src/jv_alloc.c` at line 134 for the vendored
engine and 398 for the reference. Run in the corpus root too, the vendored engine gives
398: the sides then agree row for row (decision 50).

The cause is in both engines. C, C++ and CUDA sources with preprocessor work are
expanded by the vendored preprocessing library (`engine/src/preprocessor.cpp`), whose
include loading opens a quoted include relative to the including file's path, which
the interface gives as the repository-relative path, so it is resolved against the
process's working directory. `src/jv_alloc.c` includes `jv.h`: run from the repository's
root, `src/jv.h` is read from disk and expanded, 264 lines ahead of the first function;
run elsewhere it is not found. `pdxe_extract_file`, which Stage 2 calls, is the path
measured.

What this breaks, for C-family files only:

- **Stage 2's inputs.** The engine reads files Stage 2 never gave it: not normalised by
  the secret detectors, not checked against the redaction patterns, and, for an include
  that climbs out of the repository or a working directory elsewhere, possibly not the
  repository's at all.
- **The cache.** An extraction is cached by its own file's bytes and the versions; a
  change to a header it includes does not change its key.
- **Reproducibility.** The same commit indexed from two working directories can give
  two segments. The goldens and the P2-14 baselines were built from the workspace root
  (`pdx-bench` runs `pdx` there, and its builds run there), consistently, but not in the
  repository's root.

Nothing else differs: with one working directory the two engines agree on every
corpus, so this is no fidelity defect of the vendoring, and P2-16 changes no engine
behaviour. The fix is an engine change: preprocess from the source alone (no include
loading), or from includes Stage 2 supplies, masked and confined to the repository. It
moves `ENGINE_VERSION` and is the driver's decision.

The driver chose the source alone (decision 51), and the hotfix is on
`hotfix/issue-83-c-family-input-isolation`, from `2e6f253`, not merged:

- **Root cause.** The preprocessing library's include loading (its file cache's
  lookup, for every `#include` and forced include) and its `__has_include` (two
  call sites) open files by a path relative to the including file, which is the
  repository-relative path, or by an absolute one, so against the working directory
  or anywhere. `pdxe_preprocess_with_map`, called by the second pass of
  `pdxe_engine_extract_file` for C, C++ and CUDA, was the only route.
- **Patch 0015.** A preprocessor setting under which no file is opened, checked
  first in both places; the engine always sets it. An include, forced include or
  `__has_include` target is never found, as a missing file is not. Include
  directives stay imports, and the source's own macros and conditionals still
  expand. `ENGINE_VERSION` 6 → 7; no other version and no interface moves.
- **Contract.** An extraction depends on the normalised bytes, the language and the
  path (and on the extraction switches, under which nothing is cached). It does not
  depend on the working directory, a file on disk, or anything outside the repository
  (specification 4.5).
- **Tests.** Each fails on engine 6 and passes on 7.
  `engine/tests/c_family_input_isolation.c` covers working-directory independence for
  C and C++ (with `__has_include`), a secret header (with inotify on Linux: no file
  opened), headers outside the repository by relative and absolute path, the
  supplied buffer over a self-including file on disk, and include facts and
  source-local preprocessing kept. The `pdx-core` `extract` tests are
  `c_family_extraction_is_independent_of_the_working_directory`,
  `ambient_header_change_cannot_change_a_cached_extraction` (a cache hit equals a
  fresh extraction after a header changes), `cache_entries_of_engine_six_are_never_served`
  and `c_family_engine_reads_only_the_normalised_bytes`; on engine 6, from the
  repository root, a masked URI password came back through a self-include, and a
  redacted key header's value came back through an include. CUDA shares the code
  but cannot be extracted here (no grammar, no matrix language). Mutations
  (disk loading restored, `__has_include` from the working directory, the source
  read from disk, absolute includes allowed, no version move, no preprocessing) are
  all killed.
- **Audit.** From `pdxe_extract_file`, the only file reads left are the quarantine
  file an extraction switch names (configuration, no cache) and the marker file's
  appends (output only). The foundation's directory, spill, process and path helpers
  and the JSON library's file reader are unreachable. No compiler is run, and no
  system include path is read. Environment: the six extraction switches, which
  bypass the cache; logging, debugging and marker switches, which are output only;
  test seams in test builds only. One input materially changes extraction and is
  named by no key: the clock and time zone behind `__DATE__` and `__TIME__`, issue 84.
- **Goldens.** `make golden` matches all ten, unchanged: the harness indexes from the
  workspace root, where none of the goldens' relative includes resolved.
- **Differential.** `make engine-differential`: 30 of 32 corpora identical.
  `golden:c/jq` differs in `src/jv_alloc.c` (calls and references: 30 rows only the
  reference has, 28 only ours; typed: 15 and 14). `golden:cpp/fmt` differs in
  `test/util.cc` and `test/gtest-extra.cc` (calls and references: 44 and 21; typed:
  10 and 3). Every differing row has `origin` 1. Definitions, imports, surfaces and
  file rows agree. The cause: the reference, run in the corpus root, still expands
  `src/jv.h`, `test/util.h` and `test/gtest-extra.h`. Ours at engine 7 gives exactly
  the rows engine 6 gives with no file reachable. Running both engines from an empty
  directory (an experiment outside the tree) makes all 32 identical. P2-16 defines
  fidelity to the reference, so the harness is left as it is, and closure is held for
  the driver.

The driver's decision on the held differential (decision 52), commit `fe536c8`:

- **Production engine.** Patch 0015 is the fix: an extraction consumes no ambient
  include file, from any working directory.
- **P2-16 harness.** The engine differential compares the explicit engine contract, and
  ambient corpus files are no input. Both engines execute, for every corpus, from a fresh
  empty working directory of the harness's own (`hermetic_cwd` in
  `scripts/engine/differential.py`), outside the stage, the corpus and the repository,
  and are given every input by absolute path. This is the differential's execution rule,
  not the production fix: the canonical form, the categories and the comparison are
  unchanged, and no row is filtered.
- **Result.** `make engine-differential` from the committed harness is 32 of 32
  identical, with 0 mismatches. jq (definitions 127, calls 261, imports 28, surfaces 79,
  typed 161) and fmt (171, 645, 9, 133 and 322) are identical. The totals on each side
  are, for the goldens, 1,505, 3,490, 413, 1,447 and 1,875, and for the fixtures, 366,
  111, 36, 365 and 96. A live run observed the vendored engine in a fresh, empty
  `pdx-differential-vendored-cwd-*` directory.
- **Tests.** The harness's `HermeticRuns` tests use stand-in engines that record where
  they run. Neither engine runs in the corpus root; files around the caller are no
  input; every run starts in a fresh, empty directory that is removed afterwards; both
  engines are isolated alike; the inputs are absolute and unchanged. All five failed on
  the corpus-root harness.
- **Mutations.** All killed. Issue 83's six (disk includes, `__has_include` from the
  working directory, the source read from disk, absolute includes, no version move, no
  preprocessing) were re-run with issue 84 in place. Four more cover the harness: the
  reference in the corpus root (G), one working directory shared and never cleaned (H),
  the engines under different rules (I), and both back in the corpus root, where the
  live differential differs on jq and fmt again (J).
- **Audit.** It was repeated after issue 84; see issue 84 for time-derived state. Normal
  extraction reads no ambient source or header and no undeclared ambient state.

State: resolved.

### 82 — The scale repositories exhaust memory in Stage 2

P2-14's performance run (`make perf`, decision 49) measured all ten goldens to
completion and neither scale repository. Each scale worker was stopped at the harness's
6 GiB resident-memory ceiling during Stage 2, before extraction finished. Measured on a
WSL 2 VM with 10 GB of memory and 28 logical processors (Intel Core i7-14700F), Stage 2
at 28 workers and the default 512 MiB budget, engine 6:

| Repository | Commit | Files discovered | Discovery | Stopped after | Peak resident | Extractions cached |
|---|---|---|---|---|---|---|
| `kubernetes` | `6c1c7702cf20` | 25,930 | 1.78 s | 9.58 s | 6,443,593,728 B (6,145 MiB) | 21,211 |
| `linux` | `72d3fcf802c4` | 95,723 | 8.85 s | 14.19 s | 6,448,656,384 B (6,149 MiB) | 17,625 |

Resident memory grows with the extractions completed: about 304 KB per cached extraction
for Kubernetes and 366 KB for Linux. These are upper estimates per file, since the
resident set also holds the engine, the 28 workers' working memory and source in flight
(at most the 512 MiB budget), and the cache holds only clean extractions, a lower bound
on the files extracted. Stage 2 returns every file's `FileExtract` (definitions, calls
with their `ast_path` node-type strings, imports, routes) to be held until the registry
is built, so the whole repository's extraction is resident at once. An earlier probe
of Kubernetes outside the harness, four Stage 2 workers included, saw the same linear
growth, to about 7.1 GB by the end of extraction, and more in Stage 3.

The harness behaved as specified. The goldens peak at 50 MiB (spring-petclinic) to
1,262 MiB (Exposed), and the ceiling stopped each scale worker within one sampling
interval of crossing it (1 and 5 MiB over).

**Decision 49's P2-14 measurement policy.** The outcome is recorded as data, rather than
the ceiling raised or anything optimised inside P2-14. `bench/results/0.1.0/kubernetes.json` and `linux.json` hold
`outcome: out_of_memory`, the peak, the stage reached (`extract`), the stages finished
(discovery, with its count and duration), the files discovered and the extractions
cached at the stop. No further sample is taken once one is stopped. The summary has
`scale_repositories_completed: 0` and `all_completed: false`.

**Consequence.** G1's performance criterion ("≥ 400 files/s … on `kubernetes`, peak
RSS ≤ 4 GiB on `kubernetes`") cannot be met until Stage 2 and Stage 3 hold far less per
file, for example by storing extractions compactly (the `ast_path` strings repeat
across sites) or by streaming extractions into the registry instead of holding them
all. That is a pipeline change for a later task, decided at G1, not in P2-14.

State: open.

### 81 — P2-14 benchmark dependencies

P2-14 adds two Criterion benchmarks to `pdx-core` and makes `pdx-bench` sample a
benchmark worker's resident memory and record the host's processors and memory, on
Linux, macOS and Windows. Neither a benchmark framework nor a cross-platform process
and system reader was in the dependency graph, and the workspace forbids `unsafe`, so
the operating systems' process interfaces cannot be called directly. Chosen under D1,
pinned exactly. Reported separately:

**Criterion**, a dev-dependency of `pdx-core` for `benches/resolve.rs` and
`benches/segment_write.rs` only, so `safe-to-run`. Default features off but
`cargo_bench_support`: no plotting backend, HTML report, async runtime or thread pool.

| Package | Version | Criterion | Direct | Why |
|---|---|---|---|---|
| `criterion` | 0.8.2 | `safe-to-run` | yes | The benchmark framework |
| `criterion-plot` | 0.8.2 | `safe-to-run` | no | Its plot types; no backend is enabled |
| `alloca` | 0.4.0 | `safe-to-run` | no | Its stack-allocated measurement buffers |
| `page_size` | 0.6.0 | `safe-to-run` | no | The page size `alloca` rounds to |
| `anes` | 0.1.6 | `safe-to-run` | no | Its terminal escape sequences |
| `cast` | 0.3.0 | `safe-to-run` | no | Its checked numeric casts |
| `ciborium` | 0.2.2 | `safe-to-run` | no | The CBOR encoding of its saved baselines |
| `ciborium-io` | 0.2.2 | `safe-to-run` | no | `ciborium`'s input and output traits |
| `ciborium-ll` | 0.2.2 | `safe-to-run` | no | `ciborium`'s low-level codec |
| `half` | 2.7.1 | `safe-to-run` | no | `ciborium`'s 16-bit floats |
| `crunchy` | 0.2.4 | `safe-to-run` | no | `half`'s loop unrolling, at build time |
| `itertools` | 0.13.0 | `safe-to-run` | no | Its iterator helpers |
| `oorandom` | 11.1.5 | `safe-to-run` | no | Its resampling generator |
| `tinytemplate` | 1.2.1 | `safe-to-run` | no | Its report templates; no report is enabled |

**sysinfo**, a dependency of `pdx-bench` (published, so `safe-to-deploy`), never of
`pdx`. Default features off but `system`: no disk, network, component or user
inspection. It reads one process's resident set (the worker's, every 10 ms) and the
host's logical processors, memory and processor name. The Windows and macOS crates
build only on those platforms but are vetted for every target.

| Package | Version | Criterion | Direct | Why |
|---|---|---|---|---|
| `sysinfo` | 0.39.6 | `safe-to-deploy` | yes | The worker's resident memory and the host's facts |
| `ntapi` | 0.4.3 | `safe-to-deploy` | no | Its Windows native API bindings |
| `winapi` | 0.3.9 | `safe-to-deploy` | no | `ntapi`'s Windows API bindings |
| `winapi-i686-pc-windows-gnu` | 0.4.0 | `safe-to-deploy` | no | `winapi`'s import libraries, 32-bit GNU Windows |
| `winapi-x86_64-pc-windows-gnu` | 0.4.0 | `safe-to-deploy` | no | `winapi`'s import libraries, 64-bit GNU Windows |
| `windows` | 0.62.2 | `safe-to-deploy` | no | Its Windows API bindings |
| `windows-core` | 0.62.2 | `safe-to-deploy` | no | The `windows` crate's core types |
| `windows-collections` | 0.3.2 | `safe-to-deploy` | no | Its collection types |
| `windows-future` | 0.3.2 | `safe-to-deploy` | no | Its asynchronous types |
| `windows-implement` | 0.60.2 | `safe-to-deploy` | no | `windows-core`'s `implement` procedural macro |
| `windows-interface` | 0.59.3 | `safe-to-deploy` | no | `windows-core`'s `interface` procedural macro |
| `windows-numerics` | 0.3.1 | `safe-to-deploy` | no | Its numeric types |
| `windows-result` | 0.4.1 | `safe-to-deploy` | no | Its error types |
| `windows-strings` | 0.5.1 | `safe-to-deploy` | no | Its string types |
| `windows-threading` | 0.2.1 | `safe-to-deploy` | no | Its thread-pool types |
| `objc2-core-foundation` | 0.3.2 | `safe-to-deploy` | no | Its macOS Core Foundation bindings |
| `objc2-io-kit` | 0.3.2 | `safe-to-deploy` | no | Its macOS IOKit bindings |

`windows-implement` and `windows-interface` are procedural macros built with `syn`, so
`syn` 2.0.119, already exempted at `safe-to-run` (issue 36's baseline), must now be
`safe-to-deploy`: that existing exemption's criterion was raised and a note naming this
issue added; no other exemption changed. `sha2` 0.11.0, which `pdx-bench` now uses
directly to re-hash segments, was already vetted.

Each new exemption is for its exact version, carries a note naming this issue and was
added by hand to `supply-chain/config.toml`. **These are exemptions, not audits.**
Nothing was regenerated and no audit was imported: 142 exemptions (111 before and these
31), 0 audits, 0 imports. Licences: MIT or Apache-2.0 for all but `alloca`, `crunchy`,
`oorandom` and `sysinfo` (MIT), the `ciborium` crates (Apache-2.0) and the `objc2`
crates (Zlib, Apache-2.0 or MIT), all on the allow list.

`pdx` links none of them: `scripts/open-binary-check.sh`, part of `make check`, fails if
`cargo tree -p pdx --edges normal,build` holds `pdx-bench`, `sysinfo` or `criterion`.

State: resolved.

### 76 — A callable named otherwise than its definition loses its sites

Closing issue 75 left a second class of site with no owner. The callable is
represented, but the unified walk and the definition extractor name it differently.
The walk qualifies a frame by `compute_func_qn`, the extractor a definition by its own
rules. The interface found a call's caller by looking the walk's name up among the
definitions' names, so a disagreement made the caller the file. Examples:

- a Rust method of a generic type (`impl<F> Debug for WebSocketUpgrade<F>`, whose
  definition drops `<F>`);
- a Rust function under a `cfg` attribute, whose definition's name carries the
  predicate;
- an Ada subprogram in a package specification, which the walk qualifies by the package;
- a C++ test macro or an out-of-line template member;
- the minified JavaScript and C macro forms the issue's row lists.

The same join gave a site to the first definition with its name, so the second of two
overloads, a method named like its struct, or a function named like a macro, lost its
sites to the other.

#### Disposition (decision 48)

The driver chose the exact declaration occurrence as the private join: the source
buffer and the byte span of the callable's function node, never a name. Patch 0014
supersedes the ownership part of 0013 and keeps its lexical and typed semantics.
`enclosing_func_qn` is unchanged, so typed resolution joins as before.

C, C++ and CUDA are no longer exempt. The preprocessed walk records occurrences in the
expanded buffer, which the adopted definitions match exactly. A superseded raw callable
hands its sites to its replacement. For callables only the expanded text represents,
the driver chose an alias from the adoption's own evidence (exact original lines, the
name on the first line, unique), because the preprocessor maps lines only; what that
cannot join is issue 77.

A call with no position in the file is never a site, and keeps the callable its name
names as resolution context (the driver's choice).

#### Result

The recorded counts were line-based, so a site on the same line as a callable counted
as inside it. Engine 5's counts reproduced exactly. Counted by bytes against the
engine's own declaration occurrences (`scripts/owner-census.py`, the engine's
`owner_census`):

| Golden | Engine 5, by lines | Engine 5, by bytes | Engine 6, by bytes |
|---|---|---|---|
| ada | 475 | 475 | 0 |
| c | 190 | 41 | 0 |
| cpp | 9,200 | 9,153 | 0 |
| kotlin repository | 7,081 | 430 (all Kotlin) | 0 |
| rust | 1,756 | 1,756 | 0 |
| typescript | 6 | 0 | 0 |
| csharp, go, java, python | 0 | 0 | 0 |

Why the line and byte counts differ:

- **JavaScript and TypeScript.** The 6,651 sites in the Kotlin repository's minified
  documentation JavaScript, and TypeScript's 6, only share lines with callables.
- **C.** 149 of jq's 190 lie in functions only the macro-expanded text defines, with
  no raw function node (issue 77). Their owner was the file under engine 5 too.
- **C++.** The 47 in fmt lie in template headers, outside the function node (as
  decorators do), or in 4 adopted callables with no unique raw node (issue 77).

The goldens were reviewed against the engine-5 segments, site by site, matched by
byte position and callee:

- **Unchanged everywhere:** node counts, site counts, coverage call sites, and file
  statuses and blobs.
- **Typed answers:** the same at all 7,896 typed sites, with none changed, lost or
  new.
- **Owner corrections.** Sites move from the file to the represented callable:
  ada 475, c 41, rust 1,756 (191 of them to tests), kotlin 430.
- **cpp test macros.** fmt's `TEST` bodies, 7,818 sites, now belong to their Test
  nodes, adding 3,598 `TESTS` edges.
- **Same-name corrections.** Sites move to the right one of several definitions of one
  kind and one name, such as overloads, which the golden's tuples cannot show.
  - Goldens whose tuples changed: ada 2,239, cpp 1,173, kotlin 3,427, c 61, rust 40.
  - Goldens byte-identical: csharp 252, java 8, python 39, typescript 1,069 and
    go 91.
  - Across kinds, sites move from a struct, class, field, variable or macro named like
    their callable: go's `Negotiate`, 18; jq's lexer macros, 55; and a few elsewhere.
- **Stage 3, now that the caller's class or module is known.** Bands move with the
  same target: exact or scoped → inheritance-guided, 49 in rust and 28 in kotlin.
  Candidates are drawn: 11 in rust, 13 in kotlin, 219 in cpp. Four Kotlin
  `super.prepareSQL` calls, which engine 5 drew to the overriding method itself, now
  resolve through the caller's class hierarchy: two to the parent's method, two to
  its candidates.
- **Back to the file:** sites that no represented callable holds.
  - jq's leaked frame (issue 79).
  - fmt's template headers, its overloads with no definition, and its parse
    artefacts (issue 80).
  - 5 minified ui-kit sites.

Drawn edges went ada 906 → 906, c 5,900 → 5,900, cpp 7,547 → 11,360, kotlin
28,268 → 28,279 and rust 3,649 → 3,709. Five goldens are byte-identical, and all ten
are ready.

Validation:

- 16 `occurrence_ownership` tests (11 failing on engine 5), and 4 `derive` tests (all
  failing on engine 5).
- `engine/tests/owner_occurrence`: the index, and the corpus invariant over 735 sites.
- 14 deliberate mutations, all killed. The late-site search is guarded by the census
  of the Kotlin golden, whose two misplaced `getValue` spans (issue 78) depend on it.
- `make check`, `make check-asan` (38 tests, no report) and `make vendor-verify`
  (exact): all green.
- `make golden`: ten match, from a clean checkout and an empty cache.

State: resolved.

### 75 — Calls inside nested definitions have no enclosing definition

P2-13 compared the legacy graph's calls with PDX's on the Python golden. One class of
difference was not about resolution at all: where a call sits.

Both pipelines see these calls. The legacy graph makes a node of every definition,
nested ones included, and draws the call from the innermost one. The segment has no
node for a definition nested in a function body, and its site has no enclosing
definition at all: `enclosing_node_id` is empty. The drawn edge therefore comes from the
File node, although the call is inside a definition the segment does have. Examples
from the pinned commit:

- **A decorator's wrapper.** In `src/flask/sansio/scaffold.py:46`, the call inside the
  wrapper nested in the decorator `setupmethod` is drawn from the file, not from
  `setupmethod`.
- **Views nested in tests.** In `tests/test_blueprints.py:917` and the five lines like
  it, the calls are inside `before_request` functions nested in a test function, and
  are drawn from the test file.
- **A view nested in a fixture.** In `tests/test_async.py:56`, the call is inside an
  async view nested in a fixture, and is drawn from the test file.

A class nested in a test function behaves differently: calls in its body are
attributed to the enclosing test (`tests/test_cli.py:49`).

Across the golden there are 3,840 call sites:

| Call sites | Count |
|---|---|
| Inside a definition's span, with an enclosing node | 3,155 (498 drawn `CALLS`) |
| Inside a definition's span, with no enclosing node | 449 (119 drawn `CALLS`, 330 not drawn) |
| At module level, with no enclosing node (as 4.2.1 intends) | 236 (22 drawn `CALLS`) |

4.2.1 defines an empty `enclosing_node_id` as a site outside every definition, which
these sites are not.

For the driver:

- **A.** Make nested definitions nodes.
- **B.** Attribute their sites to the nearest enclosing definition that is a node.
- **C.** Ratify the current behaviour and say so in 4.2.1.

A and B change `enclosing_node_id`, and with it `site_id` and every affected edge's
source: a stored-identity change, so an SCR. The engine's node-type path (`ast_path`)
starts at the innermost enclosing function, which matters for B. P2-13 changes nothing;
this is triage input.

#### Disposition: B (decision 47)

Independent review chose B: the nearest enclosing definition already modelled. A
(local-definition nodes) would have widened the catalogue. It would have meant
deciding qualified names, ids, containment, collisions, metrics, search, MCP and
visibility for local functions in every language, which is more than this defect
asks. C would have kept a mismatch with 4.2.1.

The correction is in the engine, because `ast_path` comes from there and must start
at the same definition `enclosing_node_id` names; a Rust span lookup would have fixed
one and not the other.

Definitions are extracted before the unified walk. A function boundary therefore
pushes a function frame, and so owns the caller and the path origin, only when its
exact declaration occurrence (qualified name, first line, last line) is among them.
The occurrence is looked up in a set built once per walk, with a scan of the same
definitions if the set cannot be allocated. The name alone would let the nested
`helper` of a function borrow the top-level `helper`.

A boundary that is not represented still gets its frame, as before. Its name stays the
callable each fact under it is found in (typed resolution joins its answers to calls by
that name), and its parameters and locals bind inside it. It just owns nothing: the
path keeps starting at the nearest represented callable, and calls and callable
references record that callable as an internal owner, which the interface reports as
their caller. A first implementation that moved the name itself was rejected during
review of the goldens: typed answers for calls in nested Kotlin and Rust functions
stopped joining.

Where definitions are adopted after the walk (C, C++ and CUDA, whose preprocessed
pass rescues definitions an error-recovered raw tree hides; in one golden that tree
nested five functions inside another), the walk cannot know which callables will be
represented, so ownership stays by name, as before.

4.2.1 and 4.5 state the rule. `ENGINE_VERSION` 4 → 5; nothing else moves.

#### Result

Engine 5 removes the defect from every golden where it is the cause. Counted the same
way before and after: call and reference sites inside a Function, Method, Test or
Constructor span with no enclosing node.

| Golden | Engine 4 | Engine 5 |
|---|---|---|
| python | 441 | 0 |
| csharp | 15 | 0 |
| go, java | 0 | 0 |
| typescript, rust, kotlin repository | 20, 1,993, 7,632 | 6, 1,756, 7,081 |
| ada, c, cpp | 475, 190, 9,200 | unchanged |

What remains is issue 76's class, not a nested callable. P2-13 counted 449 sites
because it also took class spans.

On the Python golden:

| Measure | Engine 4 | Engine 5 |
|---|---|---|
| `CALLS` rows | 639 | 639 |
| Unique `CALLS` pairs | 468 | 506 |
| File-sourced `CALLS` | 141 | 23, all at module level |
| `TESTS` edges | 229 | 325 (+96) |

- **Unchanged:** nodes 1,727, sites 3,892, candidates 3,205 and `CALL_REFERENCE` 17.
- **`fan_out`** changes on 85 nodes, while `fan_in`, `importance` and
  `transitive_loop_depth` are unchanged.
- **Coverage:** 3,861 call sites, unchanged; 11 sites move to `inheritance-guided`
  from `exact` (10) and `scoped` (1), with the same targets, now that Stage 3 knows the
  caller's class.
- **Discovery:** files, statuses and blobs are identical.

Across the five changed goldens every tuple was reviewed against the engine-4
segments, matched by site position and callee. The changes are:

- **Source attribution** moves to the represented callable. In the Kotlin repository
  a few move away from a different declaration that only shared the qualified name.
- **`TESTS` edges** follow calls now owned by tests.
- **Stage 3 caller effects:**
  - same-target band changes;
  - one TypeScript candidate resolving inside its own former set;
  - two Kotlin bare calls in a test class withheld by the implicit-receiver rule;
  - two Kotlin candidate sets gaining one overload node.

Typed answers are unchanged.

During review a first implementation, which moved the caller's name itself, was
rejected because typed answers for calls in nested functions stopped joining. Two
tests now guard that: `typed_answers_still_join_calls_in_unmodelled_nested_callables`
and `nested_typed_answers_are_the_same_from_a_cached_extraction`.

Validation:

- 11 deliberate mutations, all killed.
- `make check-asan`: 37 tests, no report.
- `make vendor-verify`: exact.
- `make check`: green.
- `make golden`: ten match, also from a clean checkout.

State: resolved.

### 74 — The fmt golden indexes degraded

P2-12's harness is implemented: fetch at the pin, `pdx index`, golden built from the
segment, compare or accept, all or nothing. Its first `pdx-bench golden --accept` over
the ten `[[golden]]` entries of `bench/repos.lock` stopped at `cpp/fmt`, with nothing
promoted:

```text
cpp/fmt: pdx index produced a degraded segment, which is no golden baseline:
  extraction_lost (1 file(s): test/gtest/gmock-gtest-all.cc)
```

A diagnostic run of the same `pdx index` on every pin (outside the harness, nothing
committed) gave:

| Golden | Status | Files (parsed / skipped) | Nodes | Edges |
|---|---|---|---|---|
| ada | ready | 1,363 (1,134 / 216) | 18,332 | 906 |
| c | ready | 397 (65 / 310) | 2,840 | 5,900 |
| cpp | **degraded**: `extraction_lost`, one file | 145 (70 / 36) | 7,633 | 7,544 |
| csharp | ready | 1,142 (680 / 230) | 8,453 | 1,205 |
| go | ready | 130 (119 / 8) | 2,419 | 5,516 |
| java | ready | 132 (85 / 32) | 1,556 | 531 |
| kotlin | ready | 5,593 (1,056 / 4,476) | 42,405 | 28,198 |
| python | ready | 236 (106 / 123) | 1,727 | 916 |
| rust | ready | 498 (419 / 66) | 5,312 | 3,618 |
| typescript | ready | 483 (441 / 27) | 5,763 | 4,944 |

The file is `test/gtest/gmock-gtest-all.cc`: 533 KB, 14,442 lines, the amalgamated
test framework the repository vendors. The engine's C/C++ typed resolver evaluates
expression types under a per-file step budget, `C_EVAL_MAX_STEPS_PER_FILE` (10,000).
The budget is compiled into the vendored resolver, a guard so that a pathological
expression degrades to an unknown type rather than hanging. Its exhaustion is counted
as lost work since issue 21, and lost work is a degraded reason (4.5, decision 34). The
engine's own debug output confirms the budget running out inside that file.

The result is deterministic: the same segment, `041d066d…`, as is, with one worker,
with isolated extraction, with a 4 GiB memory budget, and with
`PDX_ENGINE_WALK_DEFS_MAX`, `PDX_ENGINE_WALK_MAX_NODES` and
`PDX_ENGINE_LSP_MAX_WALK_DEPTH` raised. No setting lifts the constant.

P2-12 accepts only ready goldens, so as not to freeze a defect as the expected answer,
and G1 requires every golden repository to index ready. The baseline therefore waits.
Nothing was accepted, the harness was not loosened, and no repository or pin was
changed.

For the driver:

- **A. An engine change.** Raise, or scale with file size, the C/C++ step budget. This
  is an engine behaviour change: `ENGINE_VERSION` moves (invalidating the extraction
  cache), a vendored patch is recorded, and the sanitizer run applies.
- **B. A specification change.** Make a typed-resolution budget a run-health fact
  instead of a degraded reason. Extraction itself is complete here; only some
  expression types in one file stayed unknown. This changes what degraded means
  (4.5, decision 34, issue 21).
- **C. Another disposition.** For example, a different golden for C++ under D5's rule,
  which today covers only fetch and licence failures.

#### Disposition: option A

The driver chose A. B is rejected: `pdxe_lost_work` means a work budget ran out, so
work that could have produced evidence was not done. Issue 21 made that observable on
purpose, and decision 34 makes it a degraded reason. Relabelling it healthy would also
leave this file not clean enough to cache but clean enough to publish as ready. C is
rejected: the repository is fetchable and permissively licensed, and it exposes a real
engine scalability limit that G1, which needs every golden ready, must not hide. G1,
decision 34, `index::report::assess`, the degraded reasons, Stage 2's lost-work
propagation and the cache's clean-entry rule are all unchanged.

The fixed budget becomes a bounded, source-size-aware one (patch
`0012-c-lsp-eval-budget.patch`, decision 45):

```text
chunks = max(1, ceil(L / 32768))      L = length of the source the evaluator runs over
budget = min(320000, chunks * 10000)
```

The rule has these properties:

- **Small files** keep the old 10,000-step protection.
- **Larger files** receive proportionally more work.
- **Deterministic.** The cost depends only on the source length: never the machine,
  CPU or worker count, memory, clock, path or environment. No switch sets it.
- **Bounded.** The budget never exceeds 320,000, so an adversarial input still has a
  hard finite ceiling. The recursion-depth guard (256) stays independently active.
- **Overflow-safe.** It is computed once per evaluation context, by quotient and
  remainder, so no intermediate can overflow up to `INT_MAX`.
- **Still observable.** Running it out is still lost work, counted once per file. The
  first exhaustion is counted by `0006`'s unchanged lines.

The file here is 532,839 bytes, 17 started tranches, so its budget is 170,000.
`ENGINE_VERSION` moves 3 → 4: Rust's constant and the engine's `pdxe_version`, which
`engine_version_is_the_engines` keeps equal. Extraction-cache keys move with it (fixed
vector `ba0eb494…1508`, engine 3's `088819a1…0d81`), so an engine-3 entry is a miss,
and the format stays 5. The schema, matrix, detector, protocol and surface versions do
not move. A segment's bytes move with its `engine_version` meta value even where its
rows do not.

#### Result

All ten goldens index ready with engine 4, under the P2-12 harness itself (`pdx-bench
golden`, staging only; then `--accept`; then `make golden`, all ten matching):

- **The file.** 532,839 bytes, 17 started tranches, budget 170,000. It extracts with
  `extraction_lost` 0. Engine 3 reported 2, one for each evaluator pass (the source,
  and its macro-expanded text), and the engine's debug output, which then showed the
  budget running out twice, shows it no more.
- **`cpp/fmt`.** Ready (exit 0, no degraded reason), segment `0f5f9228…c108`, the same
  in three repeated runs, with one worker and with an empty extraction cache.
  - Nodes stay 7,633. Drawn edges are 7,547, from 7,544.
  - The whole change is in that one file. 29 `exact` edges are now `typed`, with the
    same source, target and kind: typed resolution confirms what the textual stage
    found.
  - Two `candidate` sites are now drawn `typed` edges, and one more call site is
    settled by typed resolution. So `typed` gains 32 and `exact` loses 29.
- **The other nine.** Row-for-row identical to engine 3 (nodes, edges, candidates),
  `c/jq` included: its files never reached the old budget. Their segment hashes move
  only with `meta.engine_version`.
- **Sanitizers.** `make check-asan` (clang): 37 tests, among them `c_eval_budget` at
  `INT_MAX` and `INT_MIN`. No ASan, UBSan or leak report.
- **Vendoring.** `make vendor-verify`: the refresh with patches 0001 to 0012
  reproduces the committed engine exactly.
- **Mutations.** Seven, all killed and restored:
  - a fixed 10,000 budget;
  - no cap;
  - a minimum below 10,000;
  - no `pdxe_lost_work`;
  - Rust's version left at 3;
  - the engine's string left at 3;
  - cache keys built as engine 3.

State: resolved.

### 73 — Detector 3 narrowed URI-password coverage

Issue 72's resolution (decision 42) made the URI password detector start a scheme only
at the start of the input or after a byte that cannot belong to a scheme: anything but
a letter, a digit, `+`, `-` or `.`. That fixed the idempotence defect, but it also
stopped masking passwords detector 2 masked. Detector 2's regex word boundary already
let a letter-led scheme start after any punctuation, `+`, `-` and `.` included.
Detector 3 lost, among others:

```text
-mysql://u:<password>@h
+mysql://u:<password>@h
.mysql://u:<password>@h
1.mysql://u:<password>@h
```

Independent review rejected the narrowing before P2-12's golden baselines. A secret
detector's job is to keep a password in a connection-string-shaped substring from
reaching the engine, the cache, the segment, the lexical index, embeddings, snippets
and model requests. It does not need the surrounding text to be one formally valid
URI. Detection must be monotonic: what an earlier version masked, a later one masks,
unless an approved specification change accepts the loss. None was.

Resolution (decision 43, superseding only decision 42's ordinary delimiter):

- **The final ordinary delimiter.** A letter-led scheme (a letter, then letters,
  digits, `+`, `-` and `.`) is recognised at the start of the input or right after any
  byte that is not an ASCII letter or digit. Every byte detector 2's word boundary
  accepted before a letter is accepted, and so is `_`. After a letter or digit no
  scheme starts (`9mysql://u:p@h` holds no URI password), and since the mask `X` is a
  letter, masking never opens a boundary. At the start of the input `xmysql://` and
  `abc9mysql://` are schemes of their own, masked as detector 2 masked them.
- **Kept from issue 72.** The bearer-before-`://` clause (a masked bearer token is a
  scheme even where the original is not one: `Bearer 0000000000000000://…`,
  `<token>_://…`, `<token>=://…`), the shared `BEARER_TOKEN` pattern, resuming the
  search on a match's `@` (`a://u:p@b://u:q@h` masks both), and one pass over the
  original bytes with one mask. Issue 72 stays resolved; its root cause and bearer
  rule stand.
- **Versions.** `SECRET_DETECTOR_VERSION` 3 → 4. The default policy digest moves from
  `de4ad6db…7776` to `05f97f16…6904`, the custom vector from `e4eeb594…75d0` to
  `92d35bc7…3be7`, and the cache object vector from `c1c95f71…8e28` to
  `088819a1…0d81` (`v5/08/…`). Detector-3 entries are misses, needing no migration.
  The cache format stays 5, and nothing else moves.
- **Monotonicity.** Detector 2's URI pattern and placeholder exemption are kept in
  the tests as an oracle, never used to compute the detector's own ranges.
  - `uri_detector_preserves_detector_two_coverage`: the lost cases, issue 72's
    additions, and every byte value before a scheme after nothing, a digit, letters
    and `x.` (1,024 sources).
  - `uri_detector_covers_detector_two_on_the_corpus`: replaces
    `uri_boundary_change_leaves_ordinary_sources_alone`, asserting a superset rather
    than equality. The 65 sanitizer-corpus files hold no URI password, so 0 and 0
    ranges; over all 1,171 tracked files, a one-off run gave detector 2 34 ranges and
    detector 4 37, lost 0, added 3, the `_mysql` cases.
  - The property `uri_detector_never_loses_a_detector_two_password`: 200,000 cases,
    lost 0.
  - `uri_scheme_boundary_uses_scheme_grammar_not_word_boundary` is renamed
    `uri_password_boundary_is_security_conservative`, with a table over every class
    of preceding byte.
  - The idempotence property still passes 200,000 cases.
- **4.5** now describes the boundary as a secret detector's, not a URI lexer's, with
  the security reason, and the bearer rule as the cross-detector fixed point.

State: resolved.

### 72 — A bearer token's mask completed a URI password

Found by the P2-11 review closure's main CI, run 38098740924, attempt 1, on Windows: P2-04's
property `secret_normalisation_preserves_offsets` failed. Minimal input, as YAML:

```text
Authorization: Bearer <bearer token>_mysql://u:<uri password>@h
```

The case is not Windows': replayed from the persisted seed on Linux it fails the same
way. The P2-11 closure did not introduce it: the word-boundary URI pattern dates from
P2-04's first commit (8f89f38), and the secrets code last changed in P2-08. The
property found it by chance, as it found issue 44.

What happened, on one pass over the original bytes:

- the bearer detector's token runs over `_mysql`, since `_` is a token byte;
- the URI password detector started its scheme at a regex word boundary (`\b`). `_` is
  a word character, so there is no boundary before `mysql`, and it found nothing;
- masking turned the token into `XXXX…`, so the output held `XXXX…://u:<password>@h`.

A second pass read `XXXX…` as a scheme and masked the password: normalisation was not
idempotent, and the password reached the engine on the first pass.

Issue 44's conclusion, that no other detector had its defect because each masks to `X`
inside a class `X` already belongs to, was right for each detector by itself. This
defect is between two: one detector's mask completes another's match.

Resolution (decision 42):

- **The scheme follows URI grammar.** A letter, then letters, digits, `+`, `-` and `.`,
  starting at the start of the input or right after a byte that cannot belong to a
  scheme (`_` included). A regex word boundary is not the rule. The byte before the
  scheme is matched, never captured or masked; the search resumes on a match's `@`, so
  a scheme right after it still starts there.
- **A bearer token before `://` counts as a scheme.** The grammar rule alone left a
  class open. A masked bearer token is always a valid scheme, but the original need
  not be: `<token>_9mysql://` (no scheme starts at `9`), `<token>_-mysql://`,
  `<token>_://` and `<token>=://` (empty), `Bearer 0000000000000000://` (a digit starts
  none). Each was proven non-idempotent. By the driver's decision, a bearer token
  before `://`, in the bearer detector's exact shape (one shared pattern), also stands
  for a scheme, so the password is found on the original bytes. Every other detector
  was checked: none can mask the bytes right before `://` and leave `://` whole (a base64
  run masks its `//`, an assignment value runs over it or ends at a quote or delimiter,
  an access key id starts with a letter and holds only scheme bytes).
- **One pass, unchanged.** Every detector reads the original bytes once, the ranges
  merge, and masking happens once. Idempotence comes from detection; normalisation is
  never repeated until it stops changing, which would read masks as source. No
  detector order matters.
- **What it costs.** `SECRET_DETECTOR_VERSION` 2 → 3, so the secret-policy digest moves
  (default `de4ad6db…7776`, was `2d31e6c1…07e8`) and with it every extraction cache key
  (the fixed vector `c1c95f71…8e28`, was `6ff3be1c…d177`): an entry cached under
  detector 2 is a miss, never read, and needs no migration. The cache format stays 5,
  since the entry's layout did not change. Source the change does not touch normalises
  as before (`uri_boundary_change_leaves_ordinary_sources_alone`: detector 2's matches
  kept wherever its word boundary was a scheme boundary, the sanitizer corpus
  included). A scheme that would start with a digit, or right after `+`, `-` or `.`,
  with no letter-led scheme before it, is no scheme, as URI grammar has it.
- **Tests.** `bearer_adjacent_uri_password_is_idempotent` (CI's minimal bytes: both
  ranges found on the original, both masked once, a second pass identical),
  `uri_scheme_boundary_uses_scheme_grammar_not_word_boundary`,
  `cross_detector_boundaries_are_idempotent` (bearer and URI, assignment and bearer,
  assignment and URI, access key id and URI, in every detector order), and
  `uri_boundary_change_leaves_ordinary_sources_alone`. The property's pieces gained
  short runs of bearer-token bytes and a URI without a scheme, so one secret runs into
  another often; it passes 20,000 and 200,000 cases. 4.5 states the one-pass rule and
  the URI detector's.

State: resolved.

### 71 — `repo_id_source` has no place in a segment's meta

4.2.1 says that with no server the local binary derives `repo_id` from the canonical
clone URL "and records `repo_id_source = "url"` in `meta`". 4.3 lists a segment's
`meta` keys (`schema_version`, `repo_id`, `repo_url`, `commit_sha`, `profile`,
`engine_version`, `pdx_version`, `language_matrix_version`, `precise_sources`), the
writer stores exactly those, and the reader refuses a segment holding any other key
(`SegmentError::UnexpectedMeta`). The two cannot both hold: recording the source is a
change of the stored format.

P2-11 derives the URL identity (decision 36) and records nothing more: no meta key is
invented and `SEGMENT_SCHEMA_VERSION` stays 2. `pdx_core::ids::repo_id_from_url`'s
documentation repeats 4.2.1's clause and is left as it is until this is decided.

For the owner: either 4.3 gains `repo_id_source` (a schema change, version 3, with the
writer, reader and verifier), or 4.2.1 drops the clause (a local segment's identity is
URL-derived exactly when no server is configured, which the binary decides, and the
re-keying 4.2.1 describes is the local cache's, not the segment's).

Resolution (P2-11 review closure, 2026-10-11, decision 40): the first. A segment
records where its identity came from, because a registered id and a URL-derived one
look alike (16 hex characters) and only the source says whether the id must follow
the URL.

- `pdx_core::segment::RepoIdSource` is a closed vocabulary, stored exactly as `url` or
  `registered`, with no alias. `SegmentMeta::repo_id_source` is a first-class field
  and `SegmentMeta::new(repo_id, repo_id_source, repo_url, commit_sha, profile,
  precise_sources)` has no default for it.
- The meta table has ten keys: `commit_sha`, `engine_version`,
  `language_matrix_version`, `pdx_version`, `precise_sources`, `profile`, `repo_id`,
  `repo_id_source`, `repo_url`, `schema_version`. The reader requires every one and
  refuses any other key and any other value.
- With `url`, the writer and the reader refuse a segment whose `repo_id` is not
  `repo_id_from_url(repo_url)`. With `registered`, `repo_id` is the server's: never
  derived from the URL and never compared with it, so it survives the repository
  moving.
- `SEGMENT_SCHEMA_VERSION` 2 → 3; the language matrix (2), engine (3), cache format
  (5), worker protocol (6) and surface (4) versions are unchanged. A schema-2 segment
  is refused as another schema version (`schema_two_segment_is_refused`) and indexed
  again; there is no migration. The source is no input to any node, site or edge id
  (`repo_id_source_is_no_part_of_any_identity`).
- `IndexRequest::repo_id_source` is passed through by `build_segment` to the meta and
  changes nothing else. `pdx index` sets `url`, has no switch for the other and still
  refuses a configured server; its JSON report's `repository` and `pdx coverage`'s
  JSON carry `repo_id_source`. `pdx version` prints `schema 3`.
- 4.2.1 (rule 5) and 4.3 (section 5, the DDL's meta comment) say so; the
  `UnexpectedMeta` message no longer names a schema version.

State: resolved.

### 70 — P2-11 command-line parser

`pdx`'s command line has subcommands, a positional path with a default, typed options,
help and usage errors, which a parser should own rather than a hand-written loop. No
parser was in the dependency graph. Chosen under D1 (decision 39), pinned exactly:

| Package | Version | Criterion | Direct | Why |
|---|---|---|---|---|
| `clap` | 4.6.7 | `safe-to-deploy` | yes | The command line, through the builder API. Default features off; `std`, `help`, `usage`, `error-context` on: no colour, suggestions, terminal-width wrapping, environment-backed arguments or derive macros, so help and errors are the same on every terminal and no procedural macro is built |
| `clap_builder` | 4.6.7 | `safe-to-deploy` | no | `clap`'s builder and parser |
| `clap_lex` | 1.1.1 | `safe-to-deploy` | no | `clap`'s argument lexer |
| `anstyle` | 1.0.14 | `safe-to-deploy` | no | `clap_builder`'s style types; no styling is enabled |

Usage errors are intercepted: the parser's own exit code (2) would read as "degraded",
so every usage error exits 1. Each exemption is for its exact version, carries a note
naming this issue and was added by hand to `supply-chain/config.toml`. **These are
exemptions, not audits.** Nothing was regenerated and no audit was imported: 111
exemptions (107 before and these 4), 0 audits, 0 imports. Licences: MIT OR Apache-2.0
(all four).

State: resolved.

### 69 — P2-10 tracing dependency

P2-10's deliverable runs `build_segment`'s stages in `tracing` spans, as Part 9's
logging rule requires of the pipeline. No tracing crate was in the dependency graph.
Chosen under D1 (decision 35), pinned exactly:

| Package | Version | Criterion | Direct | Why |
|---|---|---|---|---|
| `tracing` | 0.1.44 | `safe-to-deploy` | yes | Spans and events. Default features off (no `tracing-attributes`, so no procedural macro and no `syn`), `std` on |
| `tracing-core` | 0.1.36 | `safe-to-deploy` | no | `tracing`'s dispatcher and callsite registry |
| `pin-project-lite` | 0.2.17 | `safe-to-deploy` | no | `tracing`'s instrumented futures, unused here but compiled |

No subscriber is installed by `pdx-core`: with none, a span costs a check and records
nothing. Span fields are stage names and counts only: never a source, a secret or the
checkout's location (`tracing_spans_cover_every_stage_and_change_nothing`). Each
exemption is for its exact version, carries a note naming this issue and was added by
hand to `supply-chain/config.toml`. **These are exemptions, not audits.** Nothing was
regenerated and no audit was imported: 107 exemptions (104 before and these 3), 0
audits, 0 imports. Licences: `tracing` and `tracing-core` MIT, `pin-project-lite`
Apache-2.0 OR MIT.

State: resolved.

### 68 — The engine's visibility is evidence only in Go and Python

Found by P2-09, whose `importance` reads visibility. The engine's per-definition
visibility follows a naming convention it applies in two languages: Go's exported
identifiers and Python's leading underscore. Elsewhere it does not read the declaration's
modifiers. Probed on 2026-10-10: Java and Kotlin methods declared `public` are
non-public; C#'s `private` methods, Rust's private functions and methods, TypeScript's
and JavaScript's unexported functions and private methods, C's `static` functions, C++'s
`private:` members, and PHP's, Scala's and Ada's private declarations are all public.

Stage 4 stores `props.visibility` (`public` or `non_public`) only for Go and Python
(`containment::visibility_is_evidence`), and leaves it absent elsewhere: unknown, never
stored wrong. `importance` takes an unknown visibility as not public, the
non-inflating reading (decision 28). Proven by
`visibility_props_only_where_engine_evidence` and `importance_formula`.

Restoring visibility in the other languages needs an engine fact that reads each
declaration's modifiers. That is an append-only change: it moves `ENGINE_VERSION`, the
extraction cache format, the worker protocol and the surface version.

The P2-09 driver review (2026-10-10) approved the current behaviour. In Go and Python
the engine's visibility is evidence. In every other language visibility is unknown:
`props.visibility` is absent and `importance` takes the 0.6 factor. This is preferable
to persisting known-false visibility.

Review checkpoint: before P2-12's golden snapshots are accepted. At that checkpoint
the driver chooses one of:

- **A.** Implement trustworthy modifier-derived visibility as a versioned engine fact,
  with the required `ENGINE_VERSION`, cache, protocol and surface moves and rebuilds.
- **B.** Explicitly ratify unknown visibility and 0.6 as the PDX 2.0 behaviour, and
  keep the limitation in the product and trust documentation.

This does not make P2-12 responsible for an engine change. It makes P2-12 the point,
before golden output is frozen, where the limitation is consciously dispositioned.
Until then the conservative behaviour stays correct and blocks neither P2-10 nor
P2-11.

Resolution (P2-11 review closure, 2026-10-11, before P2-12; decision 41): **B**. Go
and Python are the only languages whose visibility is evidence; in every other
language visibility is unknown, `props.visibility` is absent, and `importance` takes
the 0.6 factor. There is no engine change. Why:

- It is conservative: a known-false visibility is never persisted, and an unknown one
  never inflates `importance`.
- The alternative is a new engine fact across many languages, which moves the engine's
  interface, `ENGINE_VERSION`, the extraction cache format, the worker protocol and the
  surface version, and rebuilds every segment: too much for a secondary factor.
- Visibility is a secondary factor of `importance` (1.0 against 0.6), beside `fan_in`
  and the test penalty.
- Improving it later is a specification change (an SCR) with those version moves, not
  a tuning change.

4.12.1 states the rule and its reasons; 4.13.2 puts the limitation on the trust page.
`visibility_props_only_where_engine_evidence` and `importance_formula` hold it.

State: resolved.

### 67 — What `cyclomatic`, `cognitive` and `loop_depth` are

4.12.1 names the three metrics without defining them; Appendix D.4 says the engine
computes them. P2-09 is the first task to store them, so their meaning becomes product
data here.

The engine (`pdxe_compute_complexity`) walks a definition's syntax tree, testing each
node's type against its language's table of branching node types:

- its cyclomatic count is the number of branching nodes;
- its cognitive count adds, for each branching node, one plus the number of branching
  nodes enclosing it;
- its loop depth is the deepest nesting of named loop nodes, a separate table.

Proposed, and implemented by P2-09 (`metrics::complexity`):

- **`cyclomatic`** = 1 + the engine's count, so a straight-line callable is 1. This
  is McCabe's `1 + decisions` shape over the engine's count, not textbook McCabe.
- **`cognitive`** = the engine's cognitive count.
- **`loop_depth`** = the engine's loop depth.

All three are stored for callables only (function, method, constructor, a test among
them), as decision 29 records: the engine measures nothing for any other kind.

Where the definitions coincide, the values are hand-computed and proven on Java,
Python, Go, TypeScript, Rust and C: a straight line, one `if`, two sequential `if`s, a
`for`, a `while`, a `for` holding an `if`, and two nested `for`s holding an `if`
(`cyclomatic_known_values`, `cognitive_and_loop_depth_known_values`,
`complexity_engine_facts_cross_stage_boundary`).

Known deviations from the textbook metrics, which these definitions accept:

- **Boolean operators** (`&&`, `||`, `and`, `or`) and **conditional expressions**
  (`?:`, Python's `x if c else y`) are no decision, and add nothing to either count.
- **Non-decisions that are counted**: the tables list some statements that are not
  decisions. Python's `try` and `with`, Go's `defer` and `go`, and the `try` of Java,
  JavaScript and others each add one.
- **`switch` and `match`** count themselves and each case or arm wherever the table lists
  both (JavaScript, C, Go, Rust, Java). That is one more than McCabe's count.
- **Java's `do … while`** is a loop but no decision in Java's table.
- **Nesting** for `cognitive` counts enclosing branching nodes, `try` and `with`
  included, rather than the standard's nesting increments. An `else if` or `elif`
  nested in its `if` adds two; `else`, labelled jumps and recursion add nothing.
- **Very large bodies**: the walk stacks at most 4096 nodes (`BRANCHING_STACK_CAP`),
  so an exceptionally large body can be undercounted.

D4 does not apply: the facts exist and are used as the engine gives them, never
recomputed or re-read from the source. A change to what a table counts is an engine
change that moves `ENGINE_VERSION`.

Ratified by the P2-09 driver review on 2026-10-10 (decision 32). For PDX 2.0:

- `cyclomatic` = 1 + the engine's branching-node count;
- `cognitive` = the engine's cognitive count;
- `loop_depth` = the engine's deepest loop nesting;
- the engine-specific deviations documented above are intentional product semantics.

These are PDX 2.0's definitions, sourced from the engine's Appendix D.4 facts:
deterministic and documented. Because of the engine's per-language branching tables,
they are not identical to every external or textbook definition of McCabe or cognitive
complexity. They apply to callable definitions only, as implemented (decision 29).

The acceptance fixtures on Java, Python, Go, TypeScript, Rust and C lock the
definition. Changing these semantics later is a specification change. It must come
with the appropriate compatibility and build-identity treatment before existing
published segments can coexist with the new semantics.

State: resolved.

### 66 — Identity sources outside a repository's Stage 4

4.7.1's namespace column takes identities from places a repository's own derivation
does not have: a provider's Kubernetes `Service` name and `Ingress`/`Route` host; the
hostnames, brokers, data sources and contexts `pdx-arch.yaml` declares; a consumer's
declared alias for a host; and, for RPC, the `.proto` file shared as an artifact between
two repositories or one package declared under one context.

Missing facts: deployment resources are not derived at Stage 4 (no task before the
estate model reads them), `pdx-arch.yaml` is estate-level configuration parsed by P6-02,
and sharing is a relation between repositories. P2-08 uses `pdx.toml [identity]` and
literal configuration, and otherwise leaves the contract `unresolved`, never deriving
an identity from a manifest's, file's or repository's name. The fixture
`missing_fact_identity_from_deployment_and_estate_model` has a `Service`, an `Ingress`
and a `pdx-arch.yaml` and an unresolved contract. Owner: P6-02 (estate model,
`DEPLOYED_AS`) supplies the identities; P6-01 applies them when it groups contracts.

State: open.

### 65 — Values bound before the call

Two 4.7.1 forms keep part of their meaning in a value bound earlier:

- A JMS producer created for a destination (`session.createProducer(d)`) sends with no
  destination in its own arguments (`p.send(m)`). The inline form,
  `producer.send(session.createQueue("q"), m)`, is read.
- A route registered on a router composed under a prefix at run time (gin's and echo's
  `Group`, ASP.NET's `MapGroup`, Hono's `route` and `basePath`, axum's `nest`,
  actix-web's `scope`, Express's `use`) has a path its declaration does not complete.

Missing fact: local value flow (which value a local variable or a receiver holds).
Without it nothing is guessed: the bound JMS send gives no contract
(`missing_fact_destination_bound_before_the_call`); a registration in a callable that
composes a prefixed router, or on a parameter declared as a group type, is withheld
with `PrefixUnknown` (`missing_fact_router_prefixes_withhold_routes`). A router mounted
from another callable or file is not visible, so its registrations keep the path they
declare, the same limit as P2-07's Express routes. Owner: P6-01 decides, before it
draws links, whether the value-flow fact is needed.

State: open.

### 64 — Base classes keep no type arguments

4.7.1's table consumers include ORM repository classes bound to entities. For
`interface UserRepository extends JpaRepository<User, Long>`, the extraction's base
class is `JpaRepository`, its type arguments erased, and `User` and `Long` are type
references of the file, tied to no base clause (`missing_fact_repository_entity_type_arguments`
proves both). The entity is never guessed from the repository's, file's or methods'
names, so no table is consumed. Missing fact: a definition's base classes with their
type arguments, an append-only engine fact (it would move `ENGINE_VERSION`, the cache
format, the worker protocol and the surface version). Owner: P6-01, before it draws
`SHARES_TABLE`.

State: open.

### 63 — Kotlin calls carry no arguments

The engine extracts Kotlin calls without their arguments:
`kafkaTemplate.send("kt.orders", "x")` is a call with none
(`missing_fact_kotlin_call_arguments`). No call-based source can therefore read a
destination or a URL in Kotlin: `KafkaTemplate.send`, Kafka producers and consumers,
`RestTemplate`, `WebClient`, JMS and Redis calls. Kotlin's annotations are extracted and
read (`@KafkaListener`, `@RabbitListener`, `@JmsListener`, Spring mappings, JPA). Missing
fact: Kotlin call arguments (`Call::args`), as the other languages' calls carry them.
Owner: P6-01, before it draws links from Kotlin repositories.

State: open.

### 62 — Contract-source coverage of 4.7.1

The review of P2-08 found sources the normative 4.7.1 table names that the
implementation did not read, and clarifications in 4.7 that read as if a smaller list
replaced the table. The table stays the target: 4.7 now says so, and every source is
classified here. A source is read only by provenance (the file imports the framework or
library; a receiver's declared type, where the facts give it) and shape (the
framework's decorator or registration, the library's operation with the value where it
takes it); a receiver's name is never evidence, and a call resolution sends to a
definition of the repository is its own.

**API providers**

| Source | Evidence | Route node | Classification | Proof |
|---|---|---|---|---|
| OpenAPI / Swagger | `openapi*.{yaml,yml,json}`, `swagger*.{yaml,json}` | none needed | IMPLEMENTED | `contracts_openapi`, conformance |
| Spring | mapping annotations | P2-07, reused | IMPLEMENTED | `route_contract_reuses_derived_route`, conformance |
| JAX-RS | `@Path` and verb annotations | P2-07, reused | IMPLEMENTED | conformance |
| `FastAPI` | route decorators | P2-07, reused | IMPLEMENTED | conformance |
| Flask | `route` decorators | P2-07, reused | IMPLEMENTED | conformance |
| Express | `<router>.<method>(path, handler)`, `express` imported | P2-07, reused | IMPLEMENTED | conformance |
| `NestJS` | `@Controller` + `@Get`/…/`@All`, `@nestjs/common` imported | none; owner the method | IMPLEMENTED | conformance |
| Hono | `get`/`post`/…/`all("/t", …)`, `hono` imported | none; owner the file | IMPLEMENTED | conformance |
| ASP.NET | `[Route]` + `[HttpGet]`/…/`[AcceptVerbs]`, `[controller]`/`[action]`; minimal `MapGet`/… | none | IMPLEMENTED | conformance |
| axum | `route("/t", get(h).post(h))`, `axum` used | none | IMPLEMENTED | conformance |
| actix-web | `#[get("/t")]`/`#[route]`; `route("/t", web::get().to(h))`, `web::resource` | none | IMPLEMENTED | conformance |
| Go `net/http` | `http.HandleFunc`/`Handle`, a `ServeMux`'s, Go 1.22 method patterns | none | IMPLEMENTED | conformance |
| gin | `GET`/…/`Any("/t", …)`, gin imported | none | IMPLEMENTED | conformance |
| echo | `GET`/…/`Any("/t", …)`, echo imported | none | IMPLEMENTED | conformance |
| Prefixes composed at run time (groups, scopes, nests, mounts) | local value flow | — | REQUIRES_MISSING_FACT | `missing_fact_router_prefixes_withhold_routes`; issue 65; P6-01 |

**API consumers**

| Source | Evidence | Classification | Proof |
|---|---|---|---|
| `fetch` | global `fetch`, literal or no options | IMPLEMENTED | conformance, `http_client_calls_are_consumed_contracts` |
| `axios` | `axios.<verb>`, bound by its import | IMPLEMENTED | conformance |
| `RestTemplate` | its operations, `org.springframework.web.client` imported | IMPLEMENTED | conformance, `configured_identities_reach_contracts` |
| `WebClient` | `<client>.<verb>().uri("…")` or `.method(HttpMethod.X).uri(…)`, imported; a parameter of another type refused | IMPLEMENTED | conformance |
| `requests`, `httpx` | module calls, bound by their import | IMPLEMENTED | conformance |
| `HttpClient` | `GetAsync`/…, `System.Net.Http` imported | IMPLEMENTED | conformance |
| `reqwest` | `reqwest::get`, a client's `get`/… with an absolute URL | IMPLEMENTED | conformance |
| Go `http.NewRequest` | `NewRequest`, `NewRequestWithContext`, `Get`, `Head`, `Post`, `PostForm` | IMPLEMENTED | conformance |
| Any of these in Kotlin | Kotlin calls carry no arguments | REQUIRES_MISSING_FACT | `missing_fact_kotlin_call_arguments`; issue 63; P6-01 |

**RPC**

| Source | Classification | Proof |
|---|---|---|
| `.proto` service definitions | IMPLEMENTED | `contracts_proto` |
| Generated-stub calls resolved to a stub of one service | IMPLEMENTED | `proto_stub_calls_are_consumed_when_resolution_proves_them` |

**Channels**

| Source | Evidence | Classification | Proof |
|---|---|---|---|
| `AsyncAPI` | 2.x and 3.x documents | IMPLEMENTED | conformance, `listener_annotations_and_asyncapi_channels` |
| `KafkaTemplate.send` (Java) | `org.springframework.kafka` imported, `send("t", …)` | IMPLEMENTED | conformance |
| `producer.send` | `kafka-python`/`aiokafka` `send`, `confluent-kafka` `produce`, `kafkajs` `send({topic})`, Java `KafkaProducer.send(new ProducerRecord<>("t", …))`; the engine's facts with a Kafka library imported | IMPLEMENTED | `contracts_kafka_literal`, conformance |
| `channel.basic_publish` | `pika` (`exchange`, else `routing_key`); `amqplib` `publish`/`sendToQueue` | IMPLEMENTED | conformance |
| NATS publish | `nats` (JavaScript, Python), `nats.go` `Publish` | IMPLEMENTED | conformance |
| Redis publish | `redis-py`, `ioredis`, go-redis, Jedis, Lettuce, Spring `RedisTemplate.convertAndSend` | IMPLEMENTED | conformance |
| JMS producer calls | Spring `JmsTemplate.convertAndSend`/`send`; `MessageProducer`/`JMSProducer.send(session.createQueue("q"), …)` | IMPLEMENTED | conformance |
| JMS producer whose destination was bound earlier | local value flow | REQUIRES_MISSING_FACT | `missing_fact_destination_bound_before_the_call`; issue 65; P6-01 |
| `@KafkaListener` | Java and Kotlin annotations | IMPLEMENTED | `contracts_kafka_placeholder_unresolved`, conformance |
| `consumer.subscribe` | `kafka-python`, `confluent-kafka`, `kafkajs` `subscribe({topic(s)})`, Java `KafkaConsumer.subscribe(List.of(…))` | IMPLEMENTED | conformance |
| `@RabbitListener`, `@JmsListener` | annotations | IMPLEMENTED | conformance |
| Producer and consumer calls in Kotlin | Kotlin calls carry no arguments | REQUIRES_MISSING_FACT | `missing_fact_kotlin_call_arguments`; issue 63; P6-01 |
| Placeholders from `application*.yml/properties` | one eligible value | IMPLEMENTED | `application_property_resolves_placeholder` and the rest |

**Tables and columns**

| Source | Classification | Proof |
|---|---|---|
| DDL, Flyway SQL migrations | IMPLEMENTED | conformance |
| Liquibase (XML, YAML, JSON) | IMPLEMENTED | conformance |
| JPA `@Table`, `SQLAlchemy` `__tablename__`, Django `Meta.db_table`, Prisma, EF `[Table]` | IMPLEMENTED | `contracts_jpa_table`, conformance |
| SQL literals with `FROM`, `JOIN`, `INTO`, `UPDATE` | IMPLEMENTED | conformance, `ddl_orm_and_sql_tables` |
| ORM repository classes bound to entities | REQUIRES_MISSING_FACT | `missing_fact_repository_entity_type_arguments`; issue 64; P6-01 |

**Artifacts**: `pom.xml`, `build.gradle`, `build.gradle.kts`, `package.json`,
`Cargo.toml`, `go.mod`, `*.csproj`, `pyproject.toml`, `setup.cfg`, `alire.toml`, `*.gpr`
and their dependency sections: each IMPLEMENTED (`contracts_maven_artifact`,
conformance). Literal declarations only; no package manager, registry or network.

**Namespace identities**

| Source | Classification | Proof |
|---|---|---|
| `pdx.toml [identity]` hostnames, brokers, data sources | IMPLEMENTED | `multiple_declared_hostnames_create_multiple_contracts`, `contracts_kafka_literal`, `configured_identities_reach_contracts` |
| A consumer URL's host; a configuration value resolving one | IMPLEMENTED | `declared_and_exact_same_identity_share_contract_id`, `configured_identities_reach_contracts` |
| Literal OpenAPI and `AsyncAPI` servers; a literal Kafka cluster or `RabbitMQ` broker; a literal JDBC URL | IMPLEMENTED | `contracts_openapi`, `configured_identities_reach_contracts`, `contracts_jpa_table` |
| Kubernetes `Service` name, `Ingress`/`Route` host; `pdx-arch.yaml`; a consumer's declared alias; a shared `.proto` artifact or one context | REQUIRES_MISSING_FACT | `missing_fact_identity_from_deployment_and_estate_model`; issue 66; P6-02 and P6-01 |

`.env` is not a source: the owner removed it from 4.7.1 by issue 38's correction.

Nothing is IMPLEMENTABLE_NOW: every source the current facts prove is implemented, and
each other names its missing fact. No engine fact was added, so no version moved.

State: resolved.

### 61 — Engine channel facts carry no transport

The engine classifies each publish or subscribe it finds by transport (Kafka, AMQP,
Socket.IO, an in-process event emitter, a raw socket, STOMP) and names the channel by
the call's argument. Across the interface (`pdxe_channel`, `Channel`) only the name
and the direction remain. Two things follow. A channel fact may be a socket's message
or an in-process event (`emitter.emit('change')`), not a destination any broker
carries. And for a socket with no named channel the engine uses the enclosing
function's name, or `(websocket)`, as the channel.

P2-08 does not widen the interface for this (its brief: no ABI change to make props
richer). It uses an engine channel fact only when a call in the same file confirms
it: a receiver whose last part is `producer`, `consumer` or `channel` (the receivers
the engine classifies as Kafka and AMQP clients), an operation of the fact's
direction (`send`, `sendBatch`, `produce`, `publish`, `basic_publish`, `sendToQueue`;
`subscribe`, `poll`, `consume`, `basic_consume`), and an argument that is the
channel's name. Every other fact is reported as `UnconfirmedChannel` and gives no
contract. No transport is claimed for an engine fact; listener annotations, whose
transport the annotation names, carry `props.transports`. Proven by
`unconfirmed_engine_channels_give_no_contract` and `contracts_kafka_literal`.

Open for P6-01, which draws `MESSAGES` from these rows: whether the engine interface
must carry the transport (and a position) before broker identity can be applied to
engine facts beyond the declared `brokers`. That is an append-only interface change
and moves `ENGINE_VERSION`, the cache format, the worker protocol and the surface
version.

Updated in the P2-08 review closure (issue 62), outcome A: the transport is no longer
needed for any P2-08 contract row. Every broker source 4.7.1 names is read from call,
import and annotation facts that prove the library, the direction and the destination
(`contracts::messaging`: `KafkaTemplate.send`, Kafka's Java, Python and JavaScript
producers and consumers, `pika` and `amqplib`, NATS, Redis publish, JMS; the listener
annotations), each with its transport in `props.transports`. The engine fact path
remains for the facts those rules do not read, and now also requires the file to import
a Kafka or AMQP client library: a receiver's name is never evidence on its own. What
issue 61 still covers is P6-01's calibration of the engine's other channel facts
(sockets, in-process emitters, STOMP): whether any should become a link, which would
need the transport across the interface. It no longer represents missing contract rows.

State: open.

### 60 — P2-08 parser dependencies

Contracts read YAML and XML documents. Neither a YAML nor an XML parser was in the
dependency graph (the graph had `serde_json` and `toml`). Writing a general parser of
either by hand is what the brief forbids, and the YAML crate most often used is
deprecated and archived. Chosen under D1 (decision 24), pinned exactly:

| Package | Version | Criterion | Direct | Why |
|---|---|---|---|---|
| `saphyr-parser` | 0.1.0 | `safe-to-deploy` | yes | YAML events; the tree is built in `contracts::document`, so an alias stays opaque (never expanded), tags are ignored and nesting is bounded (decision 25). No `unsafe` code |
| `arraydeque` | 0.5.1 | `safe-to-deploy` | no | `saphyr-parser`'s fixed-capacity buffer |
| `roxmltree` | 0.21.1 | `safe-to-deploy` | yes | XML, read-only; `allow_dtd` is off, so a document type declaration (and with it any entity) is refused; nothing is fetched. Default features off but `std`; its `memchr` was already vetted |

`thiserror`, which `saphyr-parser` also uses, was already in the graph. Each
exemption is for its exact version, carries a note naming this issue and was added by
hand to `supply-chain/config.toml`; the brief authorised them. **These are
exemptions, not audits.** Nothing was regenerated and no audit was imported: 104
exemptions (95 of issue 36, 4 of issue 39, 2 of issue 52 and these 3), 0 audits, 0
imports. Licences: MIT OR Apache-2.0 (all three).

State: resolved.

### 59 — Duplicate contract observations

`contracts.contract_id` is the table's primary key, and one contract can be sighted
more than once in a repository: an OpenAPI operation and the route that serves it,
two routes of one method and path, a producer and a listener of one topic, a table
declared by a migration and mapped by an entity, a dependency declared by two
manifests. 4.7.1 did not say how such rows become one; first-wins or last-wins would
make a row depend on the order files are read in.

Resolved (owner brief, 2026-10-10). Observations merge only when their kind and
`namespace_key` are equal, which is exactly when their ids are; unresolved keys hold
the repository id, so nothing merges across repositories. The merge is commutative:

- direction: `provides` with `provides` stays, `consumes` with `consumes` stays, any
  other pair and anything with `both` is `both`;
- identity strength: `exact` over `declared` over `unresolved` (unresolved
  observations have their own `namespace_key`, so they do not meet resolved ones);
- owner: a consumed-only observation nominates none. The providers' owners, when they
  are one node, are the row's `owner_node_id`; when they are several, the row has none,
  `props.owner_ambiguous` is `true` and `props.provider_owner_node_ids` lists them,
  sorted. `owner_node_id` NULL therefore means consumed-only or several provider
  owners (4.3's comment says so; no DDL change, no version moves);
- `props.raw_forms` and `props.source_paths` are sorted sets of every observation's,
  and every piece of evidence (`frameworks`, `route_node_ids`, `route_site_ids`,
  `site_ids`, `clients`, `transports`, `placeholders`, `access_modes`) a sorted set;
  facts the key determines (an artifact's `ecosystem`, `group`, `name`, `version`; a
  protobuf method's `package`, `service`, `method`) appear once, and two different
  values of one are refused.

Owner ratification, 2026-10-10 (P2-08 review closure): evidence props are always
sorted arrays with plural names (`route_node_ids`, `route_site_ids`, `site_ids`,
`frameworks`, `clients`, `transports`, `placeholders`, `access_modes`), even with one
value; a shape never depends on the data.

The writer and the accumulator check the same rules (`contracts::row::check`): an
artifact is `exact` with its key as namespace; an unresolved namespace is exactly
`unresolved:<the segment's repo_id>:<key>`; a resolved one is exactly the canonical
compact JSON over the row's own key and a non-empty identity; the id is then the estate
id of that namespace; `owner_ambiguous` is `true` with no owner and at least two sorted
distinct provider owners that are nodes (`writer_refuses_malformed_contract_rows`).

Scope: the `contracts` rows are the record of contract participation. P2-08 adds no
contract node to `nodes` and no `EXPOSES`, `CONSUMES`, `PUBLISHES`, `SUBSCRIBES`,
`READS_TABLE`, `WRITES_TABLE`, `DEFINES_TABLE`, `PRODUCES_ARTIFACT` or
`LINKS_ARTIFACT` edge: the link job (4.7.2) reads only the `contracts` tables, and a
row already holds the direction, owner and evidence such an edge would repeat. Those
kinds stay in the vocabulary for views that need site-backed edges; 4.2.4's site rule
is not weakened. Proven by `contract_duplicate_providers_merge_deterministically`,
`contract_provider_and_consumer_becomes_both`,
`contract_multiple_provider_owners_are_not_arbitrarily_chosen`,
`exact_identity_wins_over_declared_for_same_namespace`,
`contract_observation_order_does_not_change_output` and
`contracts_do_not_duplicate_the_graph`.

State: resolved.

### 58 — Contract identity encoding

4.7.1 defines `namespace_key` as the identity and the key, and 4.2.1 hashes it into
the contract's estate-level id, but neither defined its text for an established
identity. Nor were the column key, an artifact coordinate's parts per ecosystem or
the canonical form of a host defined. Contract ids are a stored format, so this had
to be settled before the first contract row.

Resolved (owner brief, 2026-10-10), written into 4.7.1:

- `exact` or `declared`: `{"identity":"<identity>","key":"<key>"}`, compact JSON,
  members in that order, escaped as a route's qualified name (4.2.1, rule 12). The
  strength is not in it, so an `exact` and a `declared` observation of one identity
  are one contract; nor are the repository, the kind (already in the id) or anything
  positional.
- `unresolved`: `unresolved:<repo_id>:<key>`, the repository's stable `RepoId`.
- `Artifact` and `ArtifactVersion`: the key itself, `exact`; the coordinate is
  unique in its registry.
- The id is `NodeKey::estate(kind, namespace_key)`, never another formula; owner,
  direction, raw form, file and line are not in it.
- Several identities (two declared host names, two literal servers) give one row
  each.
- Strength: `exact` for an identity the source or configuration describing the
  endpoint names (a URL's host, a literal OpenAPI server, a literal JDBC host and
  database, a literal broker); `declared` for `pdx.toml [identity]`; otherwise
  `unresolved`. Never a repository's or directory's name.
- Hosts: lower-cased, one terminal dot removed, user information dropped, the
  scheme's default port (`http` 80, `https` 443) omitted, any other kept, IP literals
  as written, no DNS. A loopback or unspecified host (`localhost`, `127.0.0.0/8`,
  `::1`, `0.0.0.0`) names no shared service and gives no identity.
- Keys: API `METHOD path` (4.7.1, with every parameter syntax `{}`); a table's key
  lower-cased and unquoted, `public.` dropped and any other schema kept; a column's,
  its table's key, `.`, and the column lower-cased and unquoted; an artifact's
  `ecosystem:group:name` (`maven`, `npm`, `cargo`, `go`, `nuget`, `pypi`, `alire`,
  `gpr`; the group empty where the ecosystem has none, `@scope` for a scoped npm
  package) and `…@version`, a range verbatim.

Fixed vectors, computed outside the crate, for an exact, a declared and an unresolved
API contract (and the same unresolved key in another repository), an RPC method, a
channel, a table, a column, an artifact and an artifact version
(`contract_namespace_key_fixed_vectors`). No version moves: no contract row existed
before this.

Owner ratification, 2026-10-10 (P2-08 review closure): the loopback and unspecified
hosts (`localhost`, `*.localhost`, `127.0.0.0/8`, `::1`, `::`, `0.0.0.0`) never give an
`exact` identity, because a runtime-local endpoint is no service another repository can
share and treating it as one would join unrelated repositories. A local name declared
in `pdx.toml [identity]` stays `declared`: it is the owner's statement
(`local_hosts_are_never_exact_identities`).

The contract fixtures use hosts reserved for examples by RFC 2606, as issue 33's
vectors do; their prefixes (`https://api.example.com`, `http://api.example.com`,
`https://API.Example.com`, `https://specs.example.invalid/`) are added to
`scripts/url-allowlist.txt`, and every other address a test needs (a loopback or
private host, user information) is built at run time.

State: resolved.

### 57 — Malformed source can be reported as parsed

The engine reports a file `failed` when it could not parse it at all and `partial`
when an error region of the tree remains after it subtracts the regions it recovered a
definition from. A small syntax error inside a definition it still recovered leaves no
such region, so the file is `parsed`. Observed in P2-07:

```python
def broken(:
    return
```

is `parsed`, while

```python
def fine():
    return 1


def broken(x:
    return x
```

is `partial` (`file_records_are_faithful` uses the second).

Stage 4 maps the engine's status faithfully and is not the place to redefine it. What
syntax damage means for coverage, the degraded status and the published parsed,
partial and failed counts is P2-10's (coverage, degraded status, `build_segment`), and
must be decided before anything is published: the semantics of `parsed`, `partial` and
`failed` for a recoverable syntax error, malformed fixtures for them, and coverage and
degraded thresholds that use those semantics.

Settled at P2-10 (decision 34). The published status is the engine's report, mapped as
Stage 4 maps it, and nothing else decides it: no second parser and no heuristic over
the source text in the pipeline.

| Engine status | `files.status` | Coverage counter | Degraded |
|---|---|---|---|
| `Parsed` | `parsed` | `parsed` | no |
| `Partial` | `partial` | `partial` | no: no normative rule makes it a reason |
| `Failed` | `failed`, reason `parse` | `failed` | counts toward `failed + skipped(size) > 5%` |

A human-visible syntax defect the engine reports no different status for (the first
example above) stays `parsed`: the pipeline has no fact that says otherwise.
`malformed_sources_follow_engine_status` builds all three cases through
`build_segment`: `def broken(:` is `parsed`, the second example `partial`, and Perl
nested past the depth the engine parses it to `failed`; only the last counts toward
the threshold.

State: resolved.

### 56 — Repeated non-callables share the collision scheme

4.2.1's disambiguator rule names callables. P2-07 found definitions that are not
callables yet share a kind, path and qualified name (a Python name assigned twice at
module level, a class declared twice), and gave them the identical-signature scheme:
their signatures are empty, so they are `e3b0c442-1`, `e3b0c442-2`, in file order. It
wrote that into 4.2.1 without bringing it to the owner.

Owner decision, 2026-10-05: approved as implemented. Persistent node ids must stay
unique; the extracted facts hold nothing stronger that tells such declarations apart; a
line number would break the identity design; deterministic file order is better than a
collision or a position. The limitation is now explicit in 4.2.1: unlike distinct
overloads, inserting another identical declaration before an existing one may renumber
that collision group. No version moves: the rule was in the identity format before any
segment was published. Proven by `duplicate_noncallables_have_unique_deterministic_ids`
(two variables and two classes, each pair two ids, the same on every run, equal to
4.2.1's rule computed by hand).

State: resolved.

### 55 — The engine's entry-point flag is not Appendix B.2's

P2-07 set `props.is_entry_point` to the engine's own flag or a test, and `entry.rs`
kept the engine's flag unconditionally. The engine sets that flag far more broadly than
Appendix B.2: on every exported JavaScript and TypeScript declaration, and on any
`main` (a `func main()` in any Go package, a `main` inside a C++ namespace). The
TypeScript golden proved it: plain exported `show` and `formatName` were entry points.

Resolved. `props.is_entry_point` is a PDX graph fact decided from evidence of B.2's
categories (`index::derive::entry`): every test and route handler; a program's `main` by
its language's convention (`entry::is_main`: Java's and Groovy's `main(String[])`,
Scala's `main(Array[String])`, Kotlin's `main()` or `main(Array<String>)`, C#'s
`Main()` or `Main(string[])`, C's `main` at file scope, C++'s in the global namespace,
Go's `func main()` at file scope, Rust's `fn main()` at file scope in `main.rs` or under
`bin`); a class annotated `@SpringBootApplication`; a module-level `FastAPI` or Flask
application; the definition calling `listen` on an Express application or
`NestFactory.create`; an ASP.NET `Program.cs` with top-level statements. The engine's
flag is kept as `props.engine_entry_point`, engine evidence only, and never makes an
entry point; the engine's own semantics are unchanged. Command-line subcommand handlers
are not supported: nothing in the facts ties a handler to its registration. One
approximation remains: Go's package clause is not extracted, so a `func main()` in a
package other than `main` is taken for one.

Proven by `exported_typescript_function_is_not_entry_point` and
`exported_javascript_function_is_not_entry_point` (the engine flags each; the graph does
not), `main_entry_points_survive_engine_flag_filter` (Java, Kotlin, C, C++, Go, Rust and
C# mains are; a Java `main(int)`, a C++ namespace's `main`, a Rust library's `main` and a
Python `main` are not), `entry_point_categories_are_kept` (Spring Boot, FastAPI
application and handler, a TypeScript Express handler and `listen` caller, a test) and
the goldens: the TypeScript golden's entry points went from 2 to 0.

State: resolved.

### 54 — A route's `DEFINES_ROUTE` edge had no site

4.2.4 says every non-containment edge references its `site_id`, and `DEFINES_ROUTE` is
one. The engine recorded a definition's route as two strings, `route_path` and
`route_method`, with no position and no node, so P2-07 made Spring's `DEFINES_ROUTE`
edges with `site_id` none, reported them, and `routes_spring` asserted the missing site.
The same two strings held one route per definition: the engine took the first mapping
annotation or decorator and the first path in it, so `@GetMapping({"/a", "/b"})` lost
`/b`, a `@RequestMapping` listing two methods lost one, and a function under two route
decorators lost the second.

Resolved without weakening 4.2.4. Engine patch 0011 records, beside the old strings, a
`PDXERouteFact` per method and path every route annotation or decorator on a definition
declares (Spring mappings, JAX-RS verbs with `@Path`, FastAPI, Flask with `methods=`,
Django REST framework's `@action`), each with the declaring node's span and its
node-type path; several paths or methods in one mapping are one fact each, joined to
each of the class's mapping paths; a path or method that is not a literal gives no fact.
The interface carries them as `pdxe_route` (append-only on `pdxe_definition`, validated
and deep-copied by `pdxe_result_build`, encoded in the surface), and Rust as
`Definition::routes` (`RouteFact`), in place of the two strings. Stage 4 makes each
binding's `route` site from its fact (4.2.1's fingerprint over the path, the callee as
resolution splits it and the ordinal among the handler's declaring nodes); one
declaration is one site for the edges of all its bindings; a binding without a position
or path gives no route and no edge, only a diagnostic. Express keeps its registration
call as its site. `Builder::add_edge` refuses a structural edge without its site or
naming one it does not have, and the segment writer refuses a structural edge without
its site, so hand-assembled rows cannot bypass derive (`EdgeKind::requires_site`; the
contract group's sites are P2-08's, and history and estate edges are outside the
repository's code).

`ENGINE_VERSION` 2 → 3 (the engine computes new facts), `EXTRACT_CACHE_FORMAT_VERSION`
4 → 5 and the worker protocol 5 → 6 (`FileExtract` changed shape), the surface codec
3 → 4. Segment schema, language matrix and secret detector are unchanged. The accepted
Route identity is unchanged; its vectors stand.

Proven by `routes_spring` (a real site at the annotation's line, kept under line
insertion), `spring_route_has_real_site`, `spring_multiple_paths_create_multiple_routes`,
`spring_request_mapping_multiple_methods`, `spring_multiple_routes_have_deterministic_ids`,
`route_line_shift_keeps_route_site_id`, `every_non_containment_edge_has_a_site`,
`route_facts_cross_the_boundary`, `cached_extractions_keep_site_paths_and_derivation_facts`,
`isolation_carries_site_paths_and_derivation_facts`, the C round trip's route coverage
and refusals, `a_siteless_defines_route_is_refused` and
`writer_refuses_a_structural_edge_without_its_site`.

The P2-02 segment fixture had a `USES_TYPE` edge and an observed precise `CALLS` edge
without sites, which the writer now refuses; both were given sites.

State: resolved.

### 53 — A file name with a backslash has no host-independent identity

A file name on Linux or macOS may contain `\`. Windows reads the same character as a
path separator, so `src\a.py` there is a file `a.py` in a folder `src`: no identity
built from such a name is the same on every host, which 4.2.1 requires of every path
in one. Discovery builds paths from names joined by `/` and finds such a file on Linux
and macOS like any other.

P2-07 makes Stage 4 refuse every path that is not repository-relative POSIX (a `\`, a
drive letter, an absolute path, an empty, `.` or `..` component) with
`DeriveError::NotRepositoryPath`, rather than give it an identity:
`backslash_path_never_reaches_an_identity` and `host_native_paths_are_refused`. A
repository holding such a file therefore fails its build today.

How the file is accounted for instead (skipped by discovery with a reason, and counted
by coverage) is the whole pipeline's decision, P2-10's, which owns coverage and the
build. The refusal in Stage 4 stays whatever is decided: it is what keeps a host-native
path out of every identity.

Settled at P2-10 (decision 33). Stage 1 owns paths, so the classification happens at
its boundary: `build_segment` divides discovery's files with
`discover::split_unportable`, which applies Stage 4's own rule (`is_repository_path`).
A file whose path fails it (a name holding `\`, or a first name Windows reads as a
drive, such as `c:x.py`) is held back from Stages 2 to 5. Stage 1 discovers it under
its ordinary rules, which may read it (to tell binary from text, or a language from
its first line), before `split_unportable` holds it back. After that:

- Stages 2 to 5 never process it, so it has no blob, extraction, definition or call
  site;
- no identity is made from it: no `files` row, no node, no site;
- it is counted in its language's coverage row, in `files` and `skipped`;
- `SegmentReport::unportable` lists it, with the reason `unportable_path`
  (`discover::UNPORTABLE_PATH`);
- it counts in the file total of the degraded threshold, and is not itself a reason.

A file row is not forced for it: identity correctness wins over a row for every
file. Every other path is unchanged, and Stage 4's refusal stays as the guard behind
this. Proven end to end on Linux and macOS, where such a name can exist
(`unportable_path_is_accounted_never_identified`), and on every host on synthetic
paths (`split_unportable_keeps_identities_portable`).

State: resolved.

### 52 — P2-07 dependency exemptions

The golden fixtures of P2-07 (Part S of its brief) are snapshot tests, written with
`insta`, which brings `similar` for its diffs. Both are development dependencies of
`pdx-core` only and never ship:

| Package | Version | Criterion | Why |
|---|---|---|---|
| `insta` | 1.49.0 | `safe-to-run` | Snapshot assertions for `derive_<language>`; default features off |
| `similar` | 2.7.0 | `safe-to-run` | `insta`'s diffing |

As for issue 39, each exemption is for its exact version, carries a note naming this
issue, and was added by hand to `supply-chain/config.toml`. **These are exemptions,
not audits.** No exemption was regenerated and nothing else changed: 101 exemptions
(95 of issue 36, 4 of issue 39 and these 2).

Owner confirmation, 2026-10-05: approved as implemented. `insta` 1.49.0 and `similar`
2.7.0 are exact-version, test-only cargo-vet exemptions: exemptions, not audits.

State: resolved.

### 51 — File records for files that were never read

4.3's `files` row has a `blob_sha` and a `line_count` for every file, and the brief for
P2-07 requires both to be the pipeline's own facts, never a second read. Stage 2 reads
only extraction candidates. A redacted file is never opened, a binary one is recognised
and never extracted, an oversized one or one of no language is never read: there is no
blob identity for any of them. Stage 2 also kept no line count at all.

Resolved within `SEGMENT_SCHEMA_VERSION` 2 (issue 50): both columns are nullable,
`NULL` for a file the pipeline never read, and stated so in 4.3's DDL. Stage 2 counts
lines (newlines, and one more for a last line without one) in the bytes it already reads
for every candidate, and the registry keeps the count. A file whose status is
`redacted`, `binary` or `skipped` has neither fact; a `parsed`, `partial` or `failed`
one has both. Proven by `file_records_are_faithful`, with Git's own blob ids as the
expected values.

Owner confirmation, 2026-10-05: approved as implemented. `files.blob_sha` and
`files.line_count` are NULL when Stage 1 or 2 deliberately never read the file
(redacted, binary, too large, of no language or otherwise never read); the pipeline
must not open such a file only to fill them. `SEGMENT_SCHEMA_VERSION` stays 2.

State: resolved.

### 50 — Candidate rows lose the engine's candidate count

4.2.2 keeps the engine's score, strategy and candidate count verbatim wherever a
resolution is stored. `edges` had all three; `candidates` and `CandidateSite` had no
candidate count, so a non-drawn site the engine answered would have lost it in the
first segment P2-07's rows reach.

Resolved: `engine_candidates INTEGER` in `candidates` (4.3's DDL and `schema.sql`
together), `CandidateSite::engine_candidates`, the writer and reader. A changed table
is a changed segment layout, so `SEGMENT_SCHEMA_VERSION` is 2, mirrored in
`ui/src/consts.ts`, and a reader refuses version 1 as it refuses any other. No
identity changes. Proven by `segment_roundtrip`,
`segment_is_byte_identical_across_builds`, `reader_refuses_wrong_schema_version` and
`candidate_rows_copy_engine_calibration`.

State: resolved.

### 49 — Resolutions had no owner to become graph rows

P2-06's stages settle every call site as a `Resolution`, with its unconfirmed sites
apart, and stop there. No later task named turning them into persistent sites,
`CALLS` and `CALL_REFERENCE` edges and candidate rows, and P2-07's `TESTS` edges are
made from those calls.

Owner decision, implemented in P2-07 (`index::derive::calls`):

- A resolution with a drawn band (`typed`, `import-guided`, `inheritance-guided`,
  `exact`, `scoped`) gives one site (`call`, or `reference` for a callable passed as a
  value) and one edge, `CALLS` or `CALL_REFERENCE`, from the definition the engine
  attributes the site to (or from the file, at file scope) to the target's node, with
  the band, the site, `observed` false, and the engine's score, strategy and candidate
  count copied unchanged.
- Any other band gives one site and one candidate row, its candidates' nodes sorted
  and once each, with the same engine numbers, and no edge.
- Unconfirmed sites (`typed_only` with no typed answer, an unconfirmed reference, an
  engine-found site) are carried in the stage's diagnostics for counting, never as a
  row.
- A resolved site with no position in the file (a call found only in preprocessed
  text) or no node-type path is counted in the diagnostics, never stored at a
  position it does not have.

Proven by `drawn_edges_copy_engine_calibration`, `candidate_rows_copy_engine_calibration`,
`unconfirmed_sites_are_counted_never_drawn` and
`site_without_raw_position_is_counted_not_placed`.

State: resolved.

### 48 — Module node identity and containment

`ModuleKey` (language, scope, name) was explicitly not a graph identity, and Appendix
B.3 says both that containment runs `Repo → Folder → File → Module` and that a module
spanning files is one node with no `file_id`. A member file can neither identify such a
module (which file would be first?) nor be its parent.

Owner decision, written into 4.2.1 (a bullet and rule 11, with fixed vectors):

- Identity: `kind` `Module`, `path` the module's scope, `qualified_name`
  `<language>:<name>`, disambiguator `""`. It depends on no member file, their number
  or their order: adding a file to a package changes no id.
- A module in one file has that file as `file_id` and parent. A module spanning files
  has no `file_id`, its member paths sorted in `props.files`, and the nearest folder
  holding them all, or the repository, as parent.
- Symbols keep their physical parents (`File → Class → Method`); their module is
  `props.module`, so a file joining a package moves nothing.
- A language whose module is the file itself has no module node: the file node is the
  module.

Proven by the Part U tests (`single_file_module_has_file_parent` to
`symbols_reference_semantic_module`, over a Java package in two files, one Python name
under two source roots, one `crate::` name in two crates and TypeScript and JavaScript
in one directory) and `module_id_fixed_vectors`.

State: resolved.

### 47 — Derivation facts dropped at the safe boundary

The engine extracts, for each definition, its decorators or annotations, its declared
parameter types and, for a Spring or `FastAPI` handler, its route's method and path
(Spring's joined to the controller's prefix); and for each call its arguments, with
the literal value and keyword where it has them. None crossed the interface, and Stage
4 needs them for tests, routes, entry points and overload identity. Reading them again
from source would duplicate the engine's work and could disagree with it.

Exposed, append-only: `pdxe_definition` gains `decorators`, `signature_param_types`,
`route_path` and `route_method`, `pdxe_call` gains `args` (`pdxe_call_arg`: `expr`,
`value`, `keyword`, `index`); `pdx_engine::Definition` and `Call` carry them owned,
in the engine's order and spelling. `pdxe_result_build` validates every counted array
and deep-copies it, so a rebuilt result holds nothing of its caller's; the C test
`abi_result_build_roundtrip` covers the new fields and five refusals. Passing existing
facts through moves no `ENGINE_VERSION`; the change to `FileExtract` shares issue 46's
cache format 4 and worker protocol 5. Fresh, cached and isolated-worker extractions
agree: `derivation_facts_cross_the_boundary`,
`cached_extractions_keep_site_paths_and_derivation_facts` and
`isolation_carries_site_paths_and_derivation_facts`.

State: resolved.

### 46 — Call sites had no node-type path

4.2.1's `ast_fingerprint` hashes the engine's node-type path from the enclosing
definition to the site, with the callee and receiver texts and an ordinal, so a site's
identity survives line shifts. `pdx_engine::Call` carried texts, a position and a
caller, but no path, and nothing else may stand in for one: not a line, an offset, a
call index, file order or a constant path.

Preflight evidence that the path is new: the engine's call and usage structures had no
such field, its walk kept a stack of scopes (kind, name, depth) and never the node
types above the current node, and the pinned reference contains no computation of an
ancestry path anywhere. So the path is computed, not exposed:
`engine/patches/0010-site-node-type-paths.patch` records the node types of the walk's
current branch and the depth of the innermost enclosing function, and stamps every
call and callable reference the walk adds with the slice between them; sites added
after the walk are stamped from the tree. Extraction output changed, so
`ENGINE_VERSION` is 2; `FileExtract` changed, so `EXTRACT_CACHE_FORMAT_VERSION` is 4
and the worker protocol 5, with no migration: format 3 entries are misses and a
protocol-4 worker is refused.

Proven: `call_ast_path_crosses_the_boundary`, `reference_ast_path_crosses_the_boundary`,
`every_site_in_the_source_has_a_path`, `cached_extractions_keep_site_paths_and_derivation_facts`,
`isolation_carries_site_paths_and_derivation_facts`, `line_insertion_keeps_node_and_site_ids`,
`moving_a_call_without_restructuring_keeps_its_fingerprint`,
`changing_ast_nesting_changes_fingerprint`, `identical_sites_get_deterministic_ordinals`,
`unrelated_call_and_definition_keep_identities`, `cache_format_3_is_a_miss` and
`a_worker_of_another_protocol_is_refused`.

State: resolved.

### 45 — Typed engine target with no project file

`pdx_engine::TypedResolution` says a target in no file of the project
(`target_rel_path` absent, such as a built-in) settles its site all the same, which
must not then be resolved by name. Nothing said which band that is: the import-based
`external` rule needs import provenance, which a built-in call has none of, and the
Rust stages, run on the name, would draw a built-in call to any definition of the
repository that shares it.

Owner decision, implemented in P2-06: after the generic blocklist (a blocklisted name
is still `blocked`), such a site is `external`, with no target and no candidates, and
the engine's score, strategy and candidate count kept for calibration. No Rust stage
runs on its name. A `typed_only` question or a reference settled this way is an
external call site. Proven by `engine_external_does_not_fall_back_to_local_name`,
`blocklist_precedes_all` and `untyped_callable_reference_is_not_a_call`.

State: resolved.

### 44 — Bearer-token masking stops at padding

Found on P2-05's continuous integration (run 37229601107, Windows): P2-04's property
`secret_normalisation_preserves_offsets` failed, minimal input
`Authorization: Bearer <token>=A` as YAML. The bearer detector matched
`<token>=*`, so `=` padding ended the token and the `A` after it was left unmasked:
a byte of what is plausibly one credential reached the engine. Masking then turned the
`=` into `X`, a token byte, so normalising the output again masked the `A` too, which
the property's idempotence check refuses. The property generates its source at random
and the case needs a bearer piece directly followed by random bytes that start with a
token byte, so it failed in about a third of runs (13 of 40 locally) and had passed
P2-04's own runs by chance.

Fixed: the token is at least 16 token bytes, then any run of token bytes and `=`,
so padding and whatever follows it in the same run are masked. The change masks more,
never less, and no other detector has the defect (each masks to `X` inside a class `X`
belongs to, so a second pass finds the same ranges). What a detector matches changed,
so `SECRET_DETECTOR_VERSION` moves from 1 to 2: the secret-policy digest and with it
every extraction cache key change, and an entry cached under version 1 is a miss.
The fixed vectors (`secret_policy_digest_fixed_vector`, `cache_object_id_fixed_vector`)
were recomputed outside the crate with Python `hashlib`, which first reproduced the
version-1 values. Regression: `bearer_masking_is_idempotent_after_padding`; the
property passes 60 random runs and one of 20,000 cases.

State: resolved.

### 43 — The engine applies path aliases to every language

`pdxe_pipeline_resolve_module`, in the vendored import resolver, tries a file's
nearest alias scope before the package map, for an import of any language. A scope
with a base URL rewrites any import that has a `/` and does not start with `.` or `@`
to a path below the base URL. Go's import paths have that shape, so in a repository
whose root `tsconfig.json` sets `baseUrl`, a Go import of the repository's own module
becomes a path that names nothing, and the call it carries goes unresolved. Found by
P2-05's consistency test: the same Go call resolves with the registry's metadata
alone and is lost once a root alias scope with a base URL is added.

The registry applies aliases to TypeScript and JavaScript only, which is
TypeScript's own rule, so engine and registry read such a repository differently.

Resolved in P2-06. The pinned reference's import resolver tries the alias step for an
import of any language too, so this is upstream behaviour, changed here by a patch so
that a refresh keeps it: `engine/patches/0009-script-path-aliases-only.patch` applies
the alias step only when the engine's own language lookup classes the importing file
(by its name) as TypeScript, TSX or JavaScript. Relative imports, the package map and
Go's module mapping are unchanged. `polyglot_ts_alias_does_not_affect_go` puts a root
`tsconfig.json` with a base URL and an alias, an aliased TypeScript import and a Go
module importing its own package in one repository; before the patch the engine lost
the Go call's answer, after it the Go answer is the one it gives without the
configuration, the TypeScript import resolves through the alias, and the registry's
import targets are the engine's. The test is in both `registry` and `stages`, so it
runs in continuous integration with the rest.

State: resolved.

### 42 — Rust's trait relations do not cross the safe boundary

Rust has no base classes. What inheritance-guided resolution needs in Rust is which
traits a type implements and which traits a trait requires. The engine records
`impl Trait for Type` as pairs (`PDXEImplTrait`: trait, type, the type's qualified
name) and does not record supertraits; the interface carries neither, and every Rust
definition's `base_classes` is empty (`pub trait Shape: Drawable` included).

P2-05 does not invent them from names: its Rust hierarchy is empty, which the
registry's tests state.

Resolved in P2-06, as owner-decided:

- `pdxe_impl_trait { trait_name, struct_name, struct_qn }` and, appended to
  `pdxe_file_result`, `impl_traits` and `n_impl_traits`: the engine's relations in its
  order and spelling, the type's qualified name without the project prefix every
  interface qualified name loses, NULL and 0 for none. A relation the engine recorded
  without a string (only a failed allocation leaves one) is not reported.
- `pdxe_result_build` takes the relations and deep-copies them, refusing a counted
  array that is missing or one with a NULL string; `ProjectResolver::rebuild` passes
  them.
- `pdx_engine::ImplTrait` and `FileExtract::impl_traits`, copied while the result is
  alive.
- `EXTRACT_CACHE_FORMAT_VERSION` 2 → 3: a format-1 or format-2 entry is a miss
  (`cache_format_2_is_a_miss`). The worker protocol 3 → 4: a protocol-3 worker is
  refused (`a_worker_of_another_protocol_is_refused`). No segment, matrix, engine or
  detector version moves: the engine recorded the fact already.
- The registry resolves each relation (`impl_relations`): the type by its recorded
  qualified name, else as a base is resolved; the trait as a base is resolved. Only a
  relation whose type and trait are each exactly one internal type counts
  (`implemented_traits`, `implementors`); an ambiguous trait stays ambiguous.
- Inheritance-guided resolution adds a type's direct internal traits to its hierarchy.

Supertraits (`trait Child: Parent`) are not recorded by the engine and are not parsed
or inferred here: Rust's inheritance-guided support is the direct `impl Trait for
Type` relations only. That is a stated coverage limit.

Proven by `impl_traits_cross_the_safe_boundary` (none, one from an empty block,
several), `impl_traits_survive_serialisation`, `a_cached_extraction_with_impl_traits_resolves`,
`cached_extractions_keep_impl_relations`, `isolation_carries_impl_relations`,
`abi_result_build_roundtrip` (none, one and several, deep-copied, refusals),
`rust_impl_relations_resolve_internal_traits`, `ambiguous_trait_remains_ambiguous` and
`rust_impl_trait_inheritance_guided`.

State: resolved.

### 41 — Declared packages and namespaces do not cross the safe boundary

The language matrix gives Java, Kotlin, Scala, C++, C#, PHP, Perl, Groovy and protobuf
the module rule `Declaration`: a file's module is the package or namespace it
declares. The engine builds qualified names from paths, never from declarations
(`src/main/java/com/acme/Shop.java` gives `src.main.java.com.acme.Shop`), so the
registry cannot read a package from them, and P2-05 may not infer one from the
directory.

What the engine records:

- Java, Kotlin, C# and PHP: the file's first `package` or `namespace` declaration, in
  `PDXEFileResult.namespace_name`, which `pdxe.c` did not pass on.
- C++: its namespaces, inside the qualified names (`src.shapes.geo.detail.helper`;
  `a::b` as one part for a declaration written `namespace a::b`).
- Perl, Scala, Groovy and protobuf: nothing.

Resolved for the first group as issue 40 was resolved: `pdxe_file_result` appends `const char *declared_namespace`, NULL when
the file declares none, and `FileExtract::declared_namespace` carries it;
`pdx_engine::namespace_evidence(language)` says which of the three cases a language is,
and a test holds it to real extractions. Cache format 2 and protocol 3 (issue 40) cover
the change. C++'s modules come from its qualified names. Perl, Scala, Groovy and
protobuf have no module in the registry: none is invented, and their imports are
unclassified. Recording their declarations would take a change to the vendored
extractor; no task needs it before P2-07, which derives `Module` nodes.

Owner confirmed, 2026-10-04, as the final rule: Java, Kotlin, C# and PHP use
`FileExtract::declared_namespace`; C++ derives namespace membership from its qualified
names; for Perl, Scala, Groovy and protobuf the engine exposes no reliable declaration,
so their module stays absent and is never inferred from the directory. No parser is
added to manufacture a missing declaration, and resolution does not wait for one.

State: resolved.

### 40 — Class hierarchy facts are dropped at the safe extraction boundary

P2-05 builds class hierarchies and P2-06 resolves calls through them. The engine
records each definition's bases (`PDXEDefinition.base_classes`), and the surface codec
keeps them, but the translation in `pdxe.c` dropped them: `pdxe_definition` had no
field for them, nor `pdx_engine::Definition`, and the surface is opaque to `pdx-core`.

Owner decision 2026-10-03, implemented:

- `pdxe_definition` appends `const char **base_classes; uint32_t n_base_classes;`.
  An extracted result borrows the engine's array; none is NULL and 0. The strings are
  the engine's, in its order, spelling and case, unresolved (`extends Base` for
  JavaScript, a repeat for some Java declarations: what the engine wrote).
- `pdxe_result_build` copies the array and each string into the rebuilt result, and
  refuses a definition that counts bases with no array or holds a NULL one.
- `Definition::base_classes: Vec<String>`, copied while the result is alive.
- `EXTRACT_CACHE_FORMAT_VERSION` 1 → 2: a format-1 entry is never decoded, in the
  directory format 1 used or at the current path; it is a miss and is replaced.
- The worker protocol 2 → 3: a worker of another protocol is refused at its
  introduction.
- No other version moves: the engine extracts exactly as before; only what crosses the
  interface grew.

Proven by `base_classes_cross_the_safe_boundary`, `bases_and_declarations_survive_serialisation`,
`a_cached_extraction_with_bases_resolves`, `cached_extractions_keep_bases_and_namespaces`,
`cache_format_1_is_a_miss` (since issue 42 `cache_format_2_is_a_miss`, which plants
format 1 and format 2), `a_worker_of_another_protocol_is_refused`, and
`abi_result_build_roundtrip`, which rebuilds from a copy it then overwrites and frees
and requires definitions with no base, one, and several (`tests/fixtures/bases`).

State: resolved.

### 39 — P2-04 dependency exemptions

The supply-chain ratchet of issue 36 refuses a new package until it is audited,
imported by the owner's decision or exempted on the record. P2-04 added four:

| Package | Version | Criterion | Why P2-04 needs it |
|---|---|---|---|
| `rayon` | 1.12.0 | `safe-to-deploy` | Stage 2's worker pool, the executor 4.5 and the task name |
| `rayon-core` | 1.13.0 | `safe-to-deploy` | `rayon`'s thread pool |
| `either` | 1.18.0 | `safe-to-deploy` | A dependency of `rayon` |
| `sha1` | 0.11.0 | `safe-to-deploy` | The Git blob identity, `blob_sha`; never a security hash. 0.11 shares `digest` 0.11 with the `sha2` already in use, so it brings nothing else |

`regex` 1.13.1, which the secret detectors use, and `postcard` 1.1.3, which the cache
entries use, were already in the lock file and recorded, and `rayon-core`'s
`crossbeam-deque`, `crossbeam-epoch` and `crossbeam-utils` too; P2-04 uses the locked
versions. All four new packages ship in the binary, so each is `safe-to-deploy`.

The owner authorised narrow exemptions for the task's own dependency graph. Each is
for its exact version, carries a note naming this issue, and was added by hand to
`supply-chain/config.toml`: no exemption was regenerated, no audit imported or
certified, no wildcard version used, and no other entry changed. **These are
exemptions, not audits**: nobody is claimed to have reviewed these versions. After
them `cargo vet --locked` passes, with 99 exemptions (the 95 of issue 36 and these
four), no local audits and no imports.

The ratchet holds with them: removing the `sha1` exemption makes `make vet` fail
("1 unvetted dependencies: sha1:0.11.0 missing safe-to-deploy"), and CI run
37069384925's supply-chain job passed with the four recorded.

State: resolved.

### 38 — `.env` is both redacted and required for contract placeholders

5.12 makes every file matching `.env*` `redacted`, its content never read, and says
`.env` files are parsed for keys only, never values. 4.7.1 resolves a contract's
placeholders from, among other sources, `.env` when a value exists there. Read
literally, they cannot both be implemented: resolving a placeholder from `.env`
means reading the value 5.12 says is never read.

P2-04 does not decide it. In P2-04 a redacted `.env` stays as discovery left it: never
opened, not hashed, not searched for secrets, not given to the engine and not stored
in the extraction cache, and no `.env` value is read anywhere.

Owned by P2-08, which links contracts and must decide, by specification change, what
a placeholder may take from `.env`, if anything, without a secret value reaching the
index.

Owner decision, 2026-10-10: the security boundary wins. `.env*` files stay inside the
mandatory secret-path floor: redacted, never opened, never parsed for keys or values.
They resolve no placeholder and strengthen no identity, and no repository can make
their content reach a contract key, a namespace, props, an error or a log. 4.7.1 is
corrected accordingly. A placeholder resolves only from non-redacted
`application*.yml` and `application*.properties` files, read through the same checks
as every other file (and so normalised, a secret value already masked); only the
simple `${KEY}` form resolves, when the eligible values of `KEY` across every such
file and document are exactly one distinct non-empty value, with no file, profile or
order preferred; a credential key's value (`password`, `secret`, `token`, `api_key`,
`client_secret`, `private_key` and the rest of 5.12's list), a value a secret detector
still recognises, or one holding a placeholder itself is never eligible. P2-08's
projected task is amended (`amendments.json`) so it no longer says `.env` keys are
read. Proven by `dotenv_never_resolves_contract_placeholder` (`.env`, `.env.local`,
`.env.production`, `config/.env`, made unreadable on Unix, so any open would fail the
build) and `contracts_kafka_placeholder_unresolved`.

State: resolved.

### 37 — Secret replacement must preserve byte offsets

5.12 says a secret value found by the content detectors is replaced with
`<REDACTED_SECRET>`. Everything downstream of extraction is positioned by byte
offset: definition and call spans, site identities, and, from P5, the intersection of
compiler occurrences with sites. A fixed marker in place of a value of any other
length moves every offset after it, so a file with a secret would be indexed at
positions that are not the file's.

Approved by the owner on 2026-10-02 and written into 4.5:

- normalisation preserves length: the normalised bytes are as long as the original,
  and every byte after a secret keeps its offset;
- each byte of a secret value is masked with ASCII `X`, and no carriage return or line
  feed is ever masked, so lines keep their numbers and their endings;
- a key, its separator and quotes, a PEM block's marker lines, line endings and
  indentation are kept, as each detector's shape allows;
- `<REDACTED_SECRET>` remains the description of a masked value, never a substitution
  in the engine's input.

Implemented in `pdx_core::secrets` under `SECRET_DETECTOR_VERSION` 1, on bytes, with
non-backtracking `regex::bytes` matching: private-key blocks, bearer tokens, credential
assignments (values masked inside their quotes, and unquoted ones only in YAML,
properties, shell, Dockerfile and Markdown, where an unquoted value is a literal and
not code), URI passwords and cloud access key ids. Overlapping ranges are merged
before masking, so the order detectors run in changes nothing. Proven by
`secret_normalisation_preserves_offsets` (a property over arbitrary bytes and planted
secrets, with either line ending), `secret_detector_overlap_is_order_independent`
over every order of the five detectors, and `secrets_do_not_reach_engine_or_cache`.
No stored format changes.

State: resolved.

### 36 — `cargo vet` is required but owned by no task

Part 5.12: "Dependencies: pinned via lockfiles; `cargo vet` recorded for new crates in
`supply-chain/`." Lockfiles were pinned and every dependency's licence was checked by
`cargo deny` (`scripts/licence-scan.sh`), but nothing in the repository ran or recorded
`cargo vet`: there was no `supply-chain/` directory, and no task in the plan delivered
one. Every crate added since P0 (serde, sha2, postcard, rusqlite, ignore, globset,
toml and the rest) was therefore unvetted.

Resolved by the P2-03 review closure, at the owner's direction, before P2-04 adds more
dependencies:

- **Ownership.** Repository-wide scanning and CI policy are P0-04's, so P0-04's
  deliverables are amended (`docs/plan/amendments.json`) to own the store and its
  enforcement. No task is added; the plan keeps 98.
- **The tool.** cargo-vet 0.10.2, installed with the pinned toolchain
  (`cargo install --locked --version 0.10.2 cargo-vet`). `scripts/cargo-vet.sh` is the
  one place the version is written; `make vet` checks the installed version and runs
  `cargo vet --locked`, and installs nothing.
- **The store.** `supply-chain/config.toml`, `audits.toml` and `imports.lock`, created
  by `cargo vet init` against the `Cargo.lock` of the time: 95 crates from crates.io,
  each a bootstrap exemption at the exact version locked (83 `safe-to-deploy`, 12 used
  only to build or test, `safe-to-run`). **These are exemptions, not audits.** Nobody is
  claimed to have reviewed those versions; they are the baseline accepted as debt when
  vetting began. There are no local audits and no imported audits: importing another
  organisation's audits is a trust decision the owner has not made.
- **Enforcement.** A dedicated Linux job in `ci.yml` installs the pinned cargo-vet and
  runs `make vet` on every push and pull request; `nightly.yml` and `release.yml` run the
  same two steps, so no workflow has a policy of its own. Nothing ever runs
  `cargo vet init`, regenerates exemptions or certifies automatically, which a test in
  `crates/pdx-bench/tests/supply_chain.rs` holds, along with the store's presence, the
  single pinned version and the absence of imports.
- **The ratchet.** Proven on a scratch change: adding `hex` 0.4.3 to a crate, with its
  lock file updated, made `cargo vet --locked` fail ("1 unvetted dependencies: hex:0.4.3
  missing safe-to-deploy"); reverted, it passed.

From now on a new or changed crate is accepted only through one of: a real local audit
(`cargo vet certify`) by someone who reviewed it; an import of another organisation's
audits, decided and recorded by the owner; or an exemption for that exact version with
an entry in this file saying why it is accepted for now. Regenerating the exemptions to
make a failing check pass is not one of them; the bootstrap exemptions above were the
one exception.

State: resolved.

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
  Appendix C names it `[secrets] patterns`, which is the key read. P2-03 first read a
  value given as replacing the default list. The owner rejected that on review: a
  repository's `pdx.toml` is untrusted, so it must not be able to weaken the baseline,
  and an empty list would otherwise let a repository's own `.env` and private keys
  reach extraction, the full-text index, snippets, embeddings and model requests. The
  four patterns of 5.12 (`*.pem`, `*.key`, `.env*`, `*id_rsa*`) are a mandatory floor,
  and configured patterns only add to it:
  `effective = sorted_unique(DEFAULT_SECRET_PATTERNS ∪ configured)`. Since a negation is
  refused and any one match redacts a file, order and repetition mean nothing, so the
  effective patterns are a set in byte order, which `secret_policy_digest` (P2-04) can
  hash as it stands. No repository mechanism removes a mandatory pattern; an exception
  would need its own security design and specification change. Patterns use gitignore
  glob syntax without negation, matched against the path below the root: a pattern
  without `/` matches the file name in any directory, and a directory pattern covers
  everything below it.
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

Review, 2026-10-02: the owner accepted every reading above but one, the secret
patterns, which the P2-03 review closure corrected to the mandatory floor described
there (`repository_cannot_disable_default_secret_patterns`,
`secret_pattern_order_and_duplicates_do_not_change_effective_policy`).

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

Owner decision, 2026-10-05, implemented in P2-07:

- TypeScript's file-name rule is the `.test` and `.spec` convention on every
  TypeScript extension of the matrix: `.test.ts`, `.test.tsx`, `.test.mts`,
  `.test.cts`, and the same with `.spec` (`TestRule::SourceSuffix`).
- JavaScript's "same as TypeScript" is that convention on JavaScript's own extensions
  (`.test.js`, `.test.jsx`, `.test.mjs`, `.test.cjs`, and `.spec`); a JavaScript file is
  never matched against `.test.ts`.
- A directory rule (`tests/**`, `__tests__/**`, `src/test/**`, `t/**`, `spec/**`)
  matches that directory at any depth, on whole path components: `tests/a.py` and
  `packages/api/tests/a.py` match, `packages/api/mytests/a.py` does not. A rule of
  several components matches them consecutively: `service/src/test/java/A.java`
  matches `src/test/**`, `src/main/test/A.java` does not.
- `test*` (`NamePrefix("test")`) is any directory component beginning `test`,
  case-sensitively.

This changes the matrix's test-detection column, so `LANGUAGE_MATRIX_VERSION` is 2;
language detection is unchanged. Proven by `test_rules_follow_issue_31` and the
matrix tests.

State: resolved.

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

Decided by the owner on 2026-10-02, implemented by P2-04 and written into 4.5:

- **The bound.** One request and its response, the request's writing included, must
  finish within 120 seconds by default. Tests choose a shorter timeout through
  `IsolatedExtractor::with_timeout`; there is no environment variable, since Appendix
  F names none. The worker's introduction is bounded the same way.
- **On timeout.** The worker is killed and reaped; everything it sent for the
  exchange is discarded with it; `extract_batch` returns `IsolationError::Timeout`;
  Stage 2 fails with it, storing and returning nothing of the batch, so the build
  publishes no segment.
- **Not a crash.** A crash still costs the file that causes it (`engine_crash`), the
  batch's other files extracted again in a fresh worker. A timeout is not recorded as
  `engine_crash` or as any file status: a wall-clock threshold depends on the
  machine's load, and a segment that held a file's failure on one machine and its
  extraction on another would break the build's determinism. It is a liveness guard,
  failing the build rather than changing the graph.
- **How.** Each exchange runs on a thread of its own, scoped to the exchange, while
  the caller waits for its answer with the timeout. Past it the caller kills and reaps
  the worker, which closes the pipes the thread is blocked on, writing or reading; the
  thread ends and is joined before the error is returned. Nothing outlives the call: no
  process and no thread.

Proven by `engine_isolate_times_out_and_reaps_worker` (a worker spinning on a file
under `PDX_ENGINE_TEST_HANG_ON`, which exists only in test builds; the timeout
reported, its process gone, no crash counted, the extractor working afterwards), on
Linux, macOS and Windows; `a_worker_that_never_introduces_itself_times_out`; and, for
the stage, `an_isolated_worker_timeout_fails_the_stage` with this program's own worker.

State: resolved.

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

Decided by the owner on 2026-10-02, implemented by P2-04 and written into 4.5:

- **The key** is `(engine_version, language_matrix_version, secret_policy_digest,
  language_id, rel_path, blob_sha)`. `blob_sha` is the Git blob identity of the
  original bytes; `language_id` is there because a repository's `[languages]` can
  extract the same path and bytes as another language without any version changing;
  `rel_path` because the extraction is built from it.
- **The node budget** does not join the key: while `PDX_ENGINE_WALK_MAX_NODES` is set
  to anything, no cache is read or written. The engine alone interprets the value, the
  budget is off normally, and it changes what an extraction contains on purpose.
- **Only clean extractions are cached**: not truncated, and having lost no work. The
  loss was in the surface and nowhere a caller could read; `pdxe_file_result` now
  carries it, appended as `extraction_lost`, and `FileExtract` with it, the same count
  the surface carries. An engine error or crash is never cached; an extraction
  returned with status `failed` is, being the engine's deterministic answer.
- **A hit validates itself**: it is used only if its entry decodes exactly, is of the
  current format, was stored under the same key, is clean, and was taken as the same
  language at the same path from source of the same length and `source_digest`.
  Anything else is a miss, extracted afresh and replaced.

P2-04 found that the node budget is not the only switch of the engine's that changes
what it extracts. Five more do: the definition walk's ceiling
(`PDX_ENGINE_WALK_DEFS_MAX`), the TypeScript resolver's work budget and walk depth
(`PDX_ENGINE_TS_TYPE_BUDGET`, `PDX_ENGINE_LSP_MAX_WALK_DEPTH`), turning that resolver
off (`PDX_ENGINE_LSP_DISABLED`), and a crash-quarantine list whose files extract as
empty (`PDX_ENGINE_INDEX_QUARANTINE_FILE`). With only the node budget bypassed, a
build run with the resolver off, or with a file quarantined, would cache an extraction
missing what a normal build finds, neither truncated nor reporting a loss, and later
builds would be served it. The owner's rule is applied to all six, for its own
reasons: `pdx_engine::EXTRACTION_SWITCHES` names them, the node budget's name is
exposed as `NODE_BUDGET_ENV`, and with any of them set no cache is used. Every other
variable the engine reads is classified as leaving extraction unchanged or test-only,
and `every_engine_variable_is_classified` fails on one that is not, so a refreshed
engine cannot add a switch unnoticed.

No version is bumped: the cache had never been released or persisted, the segment
format and the engine's extraction are unchanged, and this completes the cache's
contract before its first implementation. Proven by `cache_key_includes_path`,
`cache_key_includes_language`, `secret_policy_change_invalidates_cache`,
`node_budget_disables_cache`, `extraction_switches_disable_cache`,
`truncated_extraction_is_not_cached`, `lost_work_extraction_is_not_cached`,
`cache_rejects_unclean_entries`, `cache_rejects_wrong_source_digest`,
`cache_rejects_corruption` and `extraction_lost_is_what_the_surface_carries`.

Owner review confirmation, 2026-10-03: the extension of the node-budget cache-bypass
rule to all six `EXTRACTION_SWITCHES` is approved as implemented. Each of the six can
change what a `FileExtract` contains, some while it still looks clean (not truncated,
no work lost), so with any of them set no extraction cache is read or written;
`every_engine_variable_is_classified` stays, so a refreshed engine cannot add such a
switch unnoticed.

State: resolved.

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

Settled at P2-09, on 2026-10-10. The P2-09 task brief directs the identity:
`test_penalty` is 0.3 for a test node and 1.0 otherwise. A test is a `Test` node, the
stricter semantics of Appendix B.5, and its `props.is_test` must agree. No
specification change request is needed. Proven by `importance_formula`.

State: resolved.

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
