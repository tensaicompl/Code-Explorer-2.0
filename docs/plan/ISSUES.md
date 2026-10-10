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
| 68 | 2026-10-10 | P2-09 | third-party | open | The engine's per-definition visibility is evidence only in Go (exported identifiers) and Python (a leading underscore): elsewhere it does not read a declaration's modifiers, so Java and Kotlin `public` methods come back non-public, and Rust, TypeScript, JavaScript, C#, C, C++, PHP, Scala and Ada private declarations public | Stage 4 stores `props.visibility` only for Go and Python and leaves it absent (unknown) elsewhere, never stored wrong (`visibility_props_only_where_engine_evidence`); `importance` takes an unknown visibility as not public (decision 28). Restoring it elsewhere needs an engine fact that reads declaration modifiers, an append-only change that moves `ENGINE_VERSION`, the cache format, the worker protocol and the surface version; no task owns it yet |
| 67 | 2026-10-10 | P2-09 | scr | open | 4.12.1 names `cyclomatic`, `cognitive` and `loop_depth` without defining them, and the engine's facts (Appendix D.4) are not the textbook metrics: its count is of each language table's branching node types, so boolean operators and conditional expressions are no decision, `try`, `with`, `defer` and `go` are, and a `switch` or `match` counts itself as well as each case | Proposed and implemented: the three are the engine's facts, for callables only (decision 29); `cyclomatic` is the McCabe form `1 +` the engine's count, `cognitive` the engine's sum of one plus enclosing branching nodes, `loop_depth` its deepest loop nesting. Hand-computed on six languages where the definitions coincide (`cyclomatic_known_values`, `cognitive_and_loop_depth_known_values`, `complexity_engine_facts_cross_stage_boundary`); the deviations are listed below and in 4.12.1's clarifications. D4 does not apply: the facts exist. Awaiting the owner's approval of the definitions |
| 66 | 2026-10-10 | P6-02 | blocker | open | 4.7.1 also takes namespace identities from sources no repository's Stage 4 has: a provider's Kubernetes `Service` name and `Ingress`/`Route` host, `pdx-arch.yaml`'s hostnames, brokers, data sources and contexts, a consumer's declared alias, and a protobuf file shared as an artifact across repositories | Missing facts: deployment resources (`Service`, `Ingress`) and the estate model are not inputs of Stage 4, and sharing is a cross-repository relation. Contracts stay `unresolved` without `pdx.toml [identity]`, never guessed from a manifest's name (`missing_fact_identity_from_deployment_and_estate_model`). P6-02 (estate model, `DEPLOYED_AS`) supplies them; P6-01 applies them when linking |
| 65 | 2026-10-10 | P6-01 | blocker | open | Two 4.7.1 forms carry their full meaning in a value bound before the call: a JMS producer created for a destination sends with none in its own arguments, and a route registered on a group, scope, nest or mounted router has a prefix composed at run time | Missing fact: local value flow (which value a local or a receiver holds). A JMS send with no destination gives nothing (`missing_fact_destination_bound_before_the_call`); a registration in a callable that composes a prefixed router, or on a parameter declared as a group, is withheld with `PrefixUnknown` (`missing_fact_router_prefixes_withhold_routes`); a router mounted from another callable or file is not visible and keeps the path it declares. P6-01 decides before drawing links whether to add the fact |
| 64 | 2026-10-10 | P6-01 | blocker | open | 4.7.1's table consumers include ORM repository classes bound to entities (`interface UserRepository extends JpaRepository<User, Long>`), but the extracted base class is `JpaRepository` with its type arguments erased, and `User` is a type reference of the whole file, tied to no base clause | Missing fact: a definition's base classes with their type arguments (`JpaRepository<User, Long>` → `[User, Long]`), an append-only engine fact. No consumer is guessed from a repository's, file's or method's name (`missing_fact_repository_entity_type_arguments`). P6-01 decides before drawing `SHARES_TABLE` |
| 63 | 2026-10-10 | P6-01 | blocker | open | Kotlin calls are extracted without their arguments (`kafkaTemplate.send("t", m)` has none), so no call-based 4.7.1 source can read its destination or URL in Kotlin: `KafkaTemplate.send`, producer calls, `RestTemplate`, `WebClient` | Missing fact: the arguments of Kotlin calls, as other languages' calls carry them (`Call::args`). Kotlin's annotations (`@KafkaListener` and the rest, Spring mappings) are read; its calls give nothing (`missing_fact_kotlin_call_arguments`). P6-01 decides before drawing links from Kotlin repositories |
| 62 | 2026-10-10 | P2-08 | scr | resolved | P2-08's contract sources did not cover every source the normative 4.7.1 table names (`WebClient`, `KafkaTemplate.send`, NATS and Redis publish, JMS producers, ORM repositories, and several provider frameworks), and its clarifications read as if a smaller list replaced the table | Every source of 4.7.1 classified (below): all those current facts can prove are implemented and in `contract_source_conformance_4_7_1`; the rest each need a named missing fact, with a fixture proving it, an issue (63 to 66) and an owner. No source is left implementable-now. 4.7's clarifications state that the table is the normative target and nothing in them narrows it |
| 61 | 2026-10-10 | P6-01 | ambiguity | open | The engine's channel facts reach Rust as a name and a direction only: the transport the engine classified them by is dropped at the interface, and socket messages and in-process events are channel facts too | No longer a gap in P2-08's rows (issue 62): every 4.7.1 broker source is read from call, import and annotation facts with its transport. An engine fact is used only when a broker client call in a file importing a Kafka or AMQP library confirms it; others are `UnconfirmedChannel`. What remains is P6-01's calibration: whether engine facts outside the named sources (sockets, emitters) should ever be links, which would need the transport across the interface |
| 60 | 2026-10-10 | P2-08 | third-party | resolved | Contracts must read YAML (OpenAPI, AsyncAPI, application configuration, changelogs) and XML (`pom.xml`, `*.csproj`, changelogs); no parser of either was in the dependency graph, and a hand-written general one is what the brief forbids | `saphyr-parser` 0.1.0 (event parser; the tree is built in `contracts::document`, an alias never expanded) and `roxmltree` 0.21.1 (read-only; a DTD refused), pinned exactly; with `arraydeque` 0.5.1 three new crates, each an owner-authorised exact-version `safe-to-deploy` exemption, not an audit. 104 exemptions, 0 audits, 0 imports. Decision 24 |
| 59 | 2026-10-10 | P2-08 | scr | resolved | `contracts.contract_id` is the primary key, yet several sightings in one repository can be one contract (an OpenAPI operation and its route, a producer and a listener of one topic, two providers of one key); 4.7.1 says nothing of merging them, or of whether contracts are also graph nodes and edges | Merged by `(kind, namespace_key)` only, in any order: `provides` + `consumes` = `both`; the strongest identity; the providers' one owner, or none with `owner_ambiguous` and `provider_owner_node_ids` when they differ (NULL owner = consumed-only or several provider owners); `raw_forms`, `source_paths` and all evidence as sorted sets. The `contracts` table is the record: P2-08 adds no contract node and no `EXPOSES`/`PUBLISHES`/… edge (4.7.2 reads only `contracts`), so 4.2.4's site rule is untouched. No DDL change, comment only |
| 58 | 2026-10-10 | P2-08 | scr | resolved | 4.7.1 says `namespace_key` = identity + key and 4.2.1 hashes it into the contract's id, but neither defines the text for an established identity, the column key, an artifact's coordinate parts or the canonical host; the first stored contract ids depend on it | `{"identity":…,"key":…}` compact JSON (route identity's escaping) for `exact` and `declared`, strength not in it; `unresolved:<repo_id>:<key>` for unresolved; an artifact's key is its own namespace. Hosts lower-cased, one terminal dot and user information dropped, default ports omitted, loopback hosts no identity; several identities, several rows. Column = table key `.` column; `ecosystem:group:name[@version]` with the eight ecosystem names. Fixed vectors for every kind, computed outside the crate. No version moves: no contract was stored before |
| 57 | 2026-10-05 | P2-10 | ambiguity | open | Malformed source can come back `parsed`: the engine marks a file `partial` only when an error region remains after it subtracts the regions definitions were recovered from, so `def broken(:` with a body is `parsed`. Coverage, the degraded status and the published parsed/partial/failed counts depend on what syntax damage means | P2-10 (coverage, degraded status, `build_segment`) decides, before publication: the semantics of parsed, partial and failed for recoverable syntax errors, malformed fixtures for them, and the coverage and degraded thresholds that use them |
| 56 | 2026-10-05 | P2-07 | ambiguity | resolved | P2-07 extended 4.2.1 so that non-callable definitions sharing a kind, path and qualified name take the identical-signature collision scheme (`e3b0c442-1`, `e3b0c442-2`), without bringing it to the owner | Owner decision, 2026-10-05: approved as implemented. Ids must be unique, the extracted facts hold no stronger discriminator, and a line number would break the identity design. The limitation is explicit in 4.2.1: inserting another identical declaration before an existing one may renumber that group. No version moves. `duplicate_noncallables_have_unique_deterministic_ids` |
| 55 | 2026-10-05 | P2-07 | blocker | resolved | P2-07 copied the engine's entry-point flag into `props.is_entry_point`, and the engine sets it on every exported JavaScript and TypeScript declaration: the TypeScript golden showed plain exported functions as entry points, which Appendix B.2 does not make them | `is_entry_point` is decided from evidence of B.2's categories only: tests, route handlers, each language's conventional `main` (JVM, C#, C, C++ global namespace, Go, Rust binary roots), the framework bootstraps. The engine's flag is kept as `props.engine_entry_point`. `exported_typescript_function_is_not_entry_point`, `exported_javascript_function_is_not_entry_point`, `main_entry_points_survive_engine_flag_filter`, `entry_point_categories_are_kept` |
| 54 | 2026-10-05 | P2-07 | blocker | resolved | 4.2.4: every non-containment edge references its `site_id`. P2-07 made Spring's `DEFINES_ROUTE` edges with no site (the engine recorded a route as two strings with no position) and `routes_spring` asserted it; and one route per definition lost a mapping's other paths and methods | Engine patch 0011 records every route binding with its declaring node's position and node-type path (`pdxe_route`, `RouteFact`); one fact per method and path, literals only. Every `DEFINES_ROUTE` edge has a real `route` site, shared by the bindings of one declaration; no site, no route. `Builder` and the segment writer refuse a structural edge without its site. `ENGINE_VERSION` 3, cache format 5, protocol 6, surface 4 |
| 53 | 2026-10-05 | P2-10 | ambiguity | open | A POSIX file name may contain `\`, which Windows reads as a separator: such a file has no identity that is the same on every host. Discovery finds it on Linux and macOS; Stage 4 refuses any path that is not repository-relative POSIX, so today a repository holding one fails its build | P2-10 (coverage and the whole pipeline) decides how such a file is accounted for, for example as skipped with a reason, before Stage 4 sees it. Stage 4's refusal stays: a host-native path never becomes an identity (`backslash_path_never_reaches_an_identity`) |
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
extraction cache format, the worker protocol and the surface version. No task owns it
yet; until one does, those languages' symbols take the non-public factor.

State: open.

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

- **`cyclomatic`** = 1 + the engine's count: the McCabe form, so a straight-line
  callable is 1.
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
change that moves `ENGINE_VERSION`. Awaiting the owner's approval of these definitions,
which 4.12.1's clarifications record.

State: open (proposed; implemented as proposed).

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

State: open.

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

State: open.

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
