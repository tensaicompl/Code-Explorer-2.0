# pdx-core

The graph model, the indexing pipeline, resolution, segments and layout.

Open. Apache-2.0; see `LICENSE` at the repository root.

The graph model's vocabulary, identities and rows exist, and so do segments, the
files that store them, a repository's configuration, content secret normalisation,
the pipeline's first two stages, discovery and extraction, and Stage 3's resolution:
the symbol registry and the stages that settle each call site's band and target, and
Stage 4's derivation of the graph, the repository's contracts, its modules' and
classes' layer roles and its symbols' metrics; the later stages arrive with the tasks
that implement them. What exists is below.

## Public API

### `consts`

Every constant the specification names, defined once: schema and format versions,
resolution thresholds, budgets and limits. The interface mirrors the subset it needs,
and `scripts/consts-sync.py` fails the build when the two disagree.

### `ids`

Identities (specification 4.2.1, including its byte-level encoding, approved in
`docs/plan/ISSUES.md`, issue 33). Every id is a SHA-256 hash of the fields 4.2.1 names
and nothing else: no line, column, byte offset or insertion order.

| Item | |
|---|---|
| `RepoId` | A repository's registered identity, 16 lower-case hex characters. **Registered, not derived:** a server assigns it and it never changes |
| `repo_id_from_url`, `canonical_clone_url` | The fallback for a local binary with no server only: `lowerhex(sha256(canonical_clone_url))[0..16]`. Accepts `https` clone URLs only; never use it for a repository that has a registered id |
| `NodeId`, `NodeKey`, `node_id` | `base32-crockford(sha256(repo_id ‖ 0 ‖ kind ‖ 0 ‖ path ‖ 0 ‖ qualified_name ‖ 0 ‖ disambiguator))[0..26]`. `NodeKey::repo`, `folder`, `file`, `definition` and `estate` supply 4.2.1's special values |
| `signature_disambiguator`, `overload_disambiguators` | Overloads by their normalised signatures' hashes, so inserting one renumbers none; identical signatures numbered `-1`, `-2` in file order |
| `AstFingerprint`, `ast_fingerprint` | Where a site is in its definition's syntax tree: node-type path, texts and ordinal |
| `SiteId`, `SiteKey` | A site's identity: file, enclosing definition, site kind, fingerprint |
| `EdgeId`, `edge_id` | An edge's identity: source, target, kind and site |

Ids are 26 upper-case symbols of Crockford's base32, the hash's first 130 bits; each
id type serialises as its string and refuses a malformed one. Every input refuses a
NUL byte, the formulas' separator.

### `bands`

| Item | |
|---|---|
| `Band` | The 11 confidence bands of 4.2.2, spelled `precise` … `contradicted`; ordered by `confidence_rank`, so a higher band is a more confident one; `is_drawn` for the six drawn ones |
| `CandidateBand` | The five non-drawn bands, the only ones a `candidates` row may have |
| `from_engine(score, strategy, candidates)` | What an engine answer contributes (Appendix D.3): `Typed` only for `lsp_typed` with one candidate and a score from `TYPED_MIN_SCORE` to 1; a hint for the import-guided, scoped or exact stage, which decide those bands; or a diagnostic hint. `unnarrowed_band` is `typed` or `candidate`: the engine alone never yields another band |

### `kinds`

`NodeKind` (38: structural, contract and architecture kinds of 4.2.3), `EdgeKind` (37,
4.2.4, with `requires_band` for the kinds 4.2.2 bands and `requires_site` for the
structural kinds a repository segment holds only with their evidence site), `LayerRole`, `SiteKind` and
`ContractKind`. Each vocabulary has exactly one spelling per value, used for display,
parsing and serialisation; anything else is refused.

### `model`

The segment's rows (4.3), each field a column of the same name, nullable columns as
`Option`: `Node`, `Site` (with its `Span`), `Edge`, `CandidateSite`, `Contract`,
`Metric`, `Coverage` (with `BandCounts`, one count per band in 4.2.2's order). `props`
is a map kept in key order. Edges and candidate rows keep the engine's score, strategy and candidate count
verbatim beside the band (`CandidateSite::engine_candidates` since schema 2, issue 50),
and an edge's observation is a flag that changes neither band nor id. A `FileRecord`'s
`blob_sha` and `line_count` are `None` for a file the pipeline never read (issue 51).
Constructors compute ids from keys; lines and spans are metadata set beside them.

### `segment`

A segment is one repository at one commit under one profile, as an immutable SQLite
file (specification 4.3). SQLite is the bundled build (`rusqlite` with `bundled`,
SQLite 3.53.2), the same library on every platform.

| Item | |
|---|---|
| `SegmentData` | Every row of a segment, by table: what the pipeline hands the writer. Rows may arrive in any order; keys must be unique |
| `SegmentMeta`, `SegmentProfile`, `PreciseSource` | The `meta` table's nine keys and no others: no build time and no content hash. `SegmentMeta::new` takes the schema, engine and language matrix versions from `consts` |
| `SegmentWriter::write(data, destination)` | Builds in a temporary directory: settings stated (page size, encoding, no auto-vacuum, no journal, no sync, foreign keys on), `schema.sql`, every table in its key's order in one transaction through prepared statements, the full-text index rebuilt once and checked, the build verified, then `VACUUM INTO` the destination, which must not exist; connections closed, the file made read-only (0444 on Unix) and hashed. Returns `WrittenSegment { path, content_sha256, size_bytes }` |
| `SegmentReader::open(path)` | Opens `file:…?mode=ro&immutable=1` and verifies the meta (every key, schema version, well-formed values) and every declared reference before returning |
| Queries | `node`, `children`, `edges_from`/`edges_to` (with a band filter; empty means every band), `candidates_from`, `contracts(kind, key)`, `metrics`, `coverage`/`coverage_all`, `file`/`file_by_path`, `site`, `node_location` (path and lines, for snippets), `evidence_for`, `occurrences_in_file`, and `search` (FTS5 over name, qualified name and documentation; BM25, then node id) |
| `SegmentError` | One error type: SQLite, I/O, wrong schema version, missing or malformed meta, broken references, stored values that are not what their column must hold, rows the writer refuses (a structural edge without its site, and a contract whose id is not its `namespace_key`'s, whose key is empty, whose strength and `unresolved:` prefix disagree or whose owner is no node of the segment, among them), an existing destination, invalid search syntax |

`schema.sql` is 4.3's DDL byte for byte, and a test holds it so. Every other statement
is named in `segment/queries.rs`; no value is ever spliced into SQL, and set filters
bind one JSON array. The same rows always give the same bytes, wherever they are built.

### `config`

A repository's `pdx.toml` (Appendix C), read only from the checkout's root, only if it
is a regular file of at most `MAX_FILE_BYTES`, and parsed strictly: an unknown section
or key is an error. `PdxConfig::load(root)` gives the defaults when there is no file;
`PdxConfig::parse(text, path)` parses one; `PdxConfig::default()` is every documented
default.

| Section | Fields |
|---|---|
| `[discover]` | `include_vendor` (false), `extra_excludes` (none; gitignore patterns, no negation), `max_file_bytes` (`MAX_FILE_BYTES`, above 0) |
| `[languages]` | `extra`: extension → matrix language; adds suffixes Appendix A does not define, never redefines one. `LanguagesConfig::detect` applies them, longest first, before `languages::detect` |
| `[secrets]` | `patterns`: patterns a repository adds to 5.12's mandatory `*.pem`, `*.key`, `.env*`, `*id_rsa*`, which it can never remove. The effective patterns are the sorted, de-duplicated union |
| `[layers]` | `rules`: `{ match = { path_glob }, role }`, with `role` a `LayerRole` |
| `[rules]` | `architecture`: rules with an `id` and exactly one of the four forms of 4.12.2, typed (`RuleForm`) |
| `[precise]` | `enabled` (false), `languages` (none; matrix languages), `timeout_minutes` (60), and `java`, `ts`, `python`, `cxx` with their documented command and a timeout override. Commands are stored, never run here |
| `[identity]`, `[server]` | Parsed and kept for the tasks that use them |

Nothing in it is evaluated here: layer rules, architecture rules and precise commands
are applied by their own stages. Readings where Appendix C is silent: issue 35.

### `index::discover`

Stage 1 of 4.5. `discover(root, &config)` walks the checkout and returns every regular
file no rule excludes, as `DiscoveredFile { path, language, disposition, size_bytes }`,
sorted by path. A path is relative, `/`-separated and UTF-8 (a name that is not is an
error). A file is excluded by a hard-coded directory (`.git`, `node_modules`, `target`,
`build`, `dist`, and `vendor` unless `include_vendor`), the `.gitignore` files, the root
`.pdxignore` or `extra_excludes`, each applied on its own so no negation in one undoes
another; hidden files are files, and no other ignore file is read. Symlinks are never
followed, and the root must be a real directory.

`Disposition` says what discovery found, never that anything was extracted:
`Redacted` (a secret path; never read), `SkippedSize` (over `max_file_bytes`, reason
`size`; never read), `Binary` (holds a NUL byte) or `Candidate`. Only candidates and
binaries are read, at most `max_file_bytes` of them. A file of no language is still
discovered.

### `secrets`

Content secret normalisation (5.12), the layer after discovery's redaction by path.
`normalise_in_place(bytes, language)` masks every secret value the detectors find
with ASCII `X`, byte for byte: the length never changes, no CR or LF is touched, and
keys, separators, quotes, PEM markers and indentation stay (issue 37). It works on
bytes, never requiring UTF-8, with non-backtracking `regex::bytes`.

| Item | |
|---|---|
| `Detector` | `PrivateKey` (PEM blocks, base64 masked between the markers), `BearerToken` (16 or more token bytes, then any run of token bytes and `=`), `CredentialAssignment` (keys ending in one of `CREDENTIAL_KEYS`, words joined by `_`, `-` or nothing; quoted values everywhere, bare values only in `BARE_VALUE_LANGUAGES`), `UriPassword`, `CloudAccessKeyId`; a value that is wholly a placeholder is left alone |
| `secret_ranges`, `secret_ranges_with` | The ranges found, sorted and merged: the same in any detector order |
| `SecretPolicyDigest` | SHA-256 of `{"detector_version":…,"patterns":[…]}`, compact, keys in that order, the effective patterns sorted; `of(&SecretsConfig)` under `SECRET_DETECTOR_VERSION`, `for_policy(version, patterns)` |

### `index::extract`

Stage 2 of 4.5. `ExtractStage::new(root, &secrets, limits)`, optionally
`.with_cache(&cache)` and `.with_backend(&factory)`, then `.run(&discovered)`, returns
an `ExtractReport`: one `ExtractedFile { path, language, outcome, line_count }` per
discovered file (the count of lines in the bytes read, `None` for a file never read), in the order given, with `ExtractStats` (cache hits, misses, unusable entries,
writes, workers, batches, the most source bytes held at once) and `degraded()`.

Only a `Candidate` with a language is extracted. `FileOutcome` is `Extracted
{ extract, blob_sha }` (the same from the engine or the cache), `EngineFailed`,
`EngineCrashed`, `SkippedMemory`, or, carried forward unopened, `Binary`, `Redacted`,
`SkippedSize`, `UnknownLanguage`. A crash, a skip for memory, a truncated extraction or
lost work degrades the stage. A file changed since discovery (gone, a symlink, not a
file, another size, now binary), an engine that cannot start, or an isolated worker
that times out fails it with an `ExtractError`, which names paths and never content.

| Item | |
|---|---|
| `prepare_source(root, file)` | The file read (each path component checked without following a link, never more than discovery's size), `BlobSha` over the original bytes, normalised in place, `SourceDigest` over the result. Stage 3 reads a file again through it |
| `BlobSha` | Git's blob identity of the original bytes, 40 hex; never the `SourceDigest`, which is SHA-256 of the normalised bytes the engine was given |
| `CacheKey`, `CacheObjectId` | `(engine_version, language_matrix_version, secret_policy_digest, language_id, rel_path, blob_sha)`; the object id is SHA-256 over a tagged, length-prefixed encoding of it, 64 hex |
| `ExtractCache`, `FsCache::at(root)` | `load` and `store`, usable from every worker at once. `FsCache` keeps `<root>/v<format>/<2 hex>/<64 hex>` (`v5` today), written to a temporary file and renamed, 0700 directories and 0600 files on Unix |
| `CacheEnvelope` | `{ format_version, key, extract }` in postcard, decoded exactly; another format, key, trailing bytes or a damaged entry is a `CacheDefect`, a miss |
| `ExtractLimits`, `plan_batches`, `BatchPlan` | Workers requested and the memory budget, resolved by the caller. A file over the whole budget is skipped; `effective_workers × largest ≤ budget`; each worker's share is `budget / effective_workers`; batches greedy in path order, at most `MAX_BATCH_FILES` |
| `ExtractBackend`, `default_backend` | One per worker thread: the engine in process, or an isolated worker when `PDX_ENGINE_ISOLATE=1` |

Only clean extractions are stored, and none while an engine extraction switch is set
(`pdx_engine::EXTRACTION_SWITCHES`); a hit is used only if it is exactly the clean
extraction of the source now normalised. The budget bounds the source buffers held at
once; what the engine allocates while extracting is not bounded by it.

### `resolve::registry`

The symbol registry (4.5 Stage 3): built once by `SymbolRegistry::build(root,
&discovered, report)` from Stage 1's files and Stage 2's report, then read-only. It
resolves no call and assigns no band.

| Item | |
|---|---|
| `DefinitionRef`, `ImportRef` | A file's path and an index in its extraction: the registry's identity for definitions and imports. No graph id |
| `by_name`, `by_qualified_name`, `by_file`, `definition`, `extract` | Definitions by short name, exact qualified name and file, in (path, index) order; a qualified name shared by overloads keeps every definition. Names keep their case |
| `ModuleKey`, `modules_of_file`, `module_of_file`, `module_of_definition`, `files_in_module`, `definitions_in_module`, `paired_file` | Modules by the matrix's rule, from evidence only: declared namespace (Java, Kotlin, C#, PHP), C++ namespaces and Ruby nesting per definition, Python dotted paths from `__init__.py` packages, `go.mod` module paths, the crate layout under each `Cargo.toml`, directories, files, Ada unit names. None where evidence is missing (Perl, Scala, Groovy, protobuf). C headers paired with sources by basename |
| `ImportRecord`, `ImportTarget`, `InternalTarget`, `imports_of`, `import` | One record per extracted import with its raw text, the local names it binds and its target: `Internal`, `InternalCandidates`, `External`, `UnresolvedInternal`, `Unclassified`. A relative import is never external |
| `NameProvenance`, `name_provenance`, `is_external_name`, `external_names` | A name is external only when every import binding it leads outside the repository and the file does not define it |
| `BaseRelation`, `BaseResolution`, `direct_bases`, `direct_derived`, `ancestors`, `declaring_type` | Hierarchy from `Definition::base_classes`: exact qualified name, same module, import binding, unique name, in that order; ambiguity kept; external only by import. Ancestors breadth-first, cycle-safe |
| `ImplRelation`, `impl_relations`, `implemented_traits`, `implementors` | Rust's `impl Trait for Type` blocks (`FileExtract::impl_traits`), the type by its recorded qualified name or else as a base is resolved, the trait as a base is resolved; a relation counts only when both are exactly one internal type. Supertraits are not recorded (issue 42) |
| `resolution_metadata` | Go modules, packages from `package.json`, declared packages, `tsconfig.json` alias scopes and the root crate manifest, as `pdx_engine::ResolutionMetadata`: the same the registry resolves with |
| `RegistryError` | Stages that disagree, a definition index or parent an extraction does not have, metadata that cannot be read or parsed, or has changed since Stage 2 |

Metadata files are read only if discovery found them candidates, through `prepare_source`
(checked against Stage 2's blob and digest) or `prepare_candidate`, normalised; a
redacted file is never opened. `tsconfig.json` may hold comments and trailing commas,
and `extends` is followed within the repository.

### `resolve::blocklist`

Appendix B.4's generic-name blocklist: `BASE`, its 57 names unchanged, and
`ADDITIONS`, each a documented constant (Python's `__init__`, `__str__`, `__repr__`,
`__enter__`, `__exit__`); `is_blocked(language, name)`, exact and case-sensitive.

### `resolve::stages`

Stage 3's resolution (4.5): `resolve(root, &registry)` runs typed resolution over the
whole repository (`typed_resolution`: every extracted file in path order, its source
read again and checked against Stage 2, the registry's metadata) and then settles every
site (`resolve_with(&registry, &typed)`).

| Item | |
|---|---|
| `Resolution` | A band, the target of a drawn band, the sorted candidates of `candidate`, and the engine's answer verbatim (`EngineAnswer`: score, normalised and raw strategy, candidate count, target). Made only through `Resolution::new`, which refuses every shape its band does not mean (`InvalidResolution`) |
| `ResolveReport` | `resolutions` (`ResolvedSite`: `SiteRef`, the `Call`, its `Resolution`), `unconfirmed` (`UnconfirmedSite`: `typed_only` questions, references and engine-found sites typed resolution did not settle) and `engine_health`, the run's `RunHealth` |
| `split_callee` | A callee's text as receiver and name (`obj.foo` is `obj` and `foo`): the one reading resolution and site identity share |
| `narrow`, `NarrowingStage`, `Narrowed` | The narrowing core every site goes through: one sorted set, a stage's single survivor resolves, several become the set, none changes nothing |
| `ResolutionError` | Typed resolution's failures, a changed or unreadable checkout, two answers for one site, an answer for a site the extractions do not have, a registry without what a site names |

Precedence per site: the blocklist; the engine's answer (`typed` when `from_engine`
says so and its target is one definition; `external` when its target is in no file of
the project, issue 45); names bound locally (`unresolved`) or only by external imports
(`external`); then import-guided, inheritance-guided, exact and scoped over the
candidates (definitions a call can invoke, of the caller's language family, by name, by
qualified name, through aliasing imports, and the engine's hint); then `candidate` or
`unresolved`. An `UNRESOLVED_MEMBER` call is never resolved by its name alone.

### `index::derive`

Stage 4 of 4.5 (P2-07 to P2-09): `derive(&DeriveInput { repo, repo_name, registry,
resolution, root, config })` turns the registry and Stage 3's report into a
`DerivedGraph`, where facts first get persistent identities. It writes no segment: the
graph's rows are what `SegmentData` holds, sorted by stored identity (files by path,
nodes, sites and edges by id, candidate rows by site, contracts by id, metrics by node
then metric), and the same whatever order the stages produced their facts in. `root`
and `config` are for the contracts, which read documents below the root and scope
themselves by `[identity]`, and for the layer roles' `[layers]` rules.

| Item | |
|---|---|
| `DerivedGraph` | `files`, `nodes` (`Module` and `Class` nodes with their `layer_role`), `sites`, `edges`, `candidates`, `contracts` (the `contracts` rows, by `contract_id`; never also nodes or edges), `metrics` (the `metrics` rows, by node then metric); `repo_node`, `file_nodes`, `definition_nodes` (the one map from `DefinitionRef` to node every part reads), `module_nodes`; `diagnostics` |
| `Diagnostics` | What is counted, never stored: Stage 3's unconfirmed sites, resolved sites with no position in the file or no node-type path (`Unmaterialized`, `Missing`), route evidence with no handler the facts identify or no evidence site (`RouteDiagnostic`), contract evidence that gave no contract (`contracts`: `ContractDiagnostic`, a path and a `ContractProblem`, never the file's content), and metrics a symbol has no row for because a fact is missing (`metrics`: `MetricDiagnostic`) |
| `DeriveError` | A path that is not repository-relative POSIX (`NotRepositoryPath`), an identity that cannot be computed, two different facts with one id, a structural edge without its site or naming one the graph does not have (`EdgeWithoutSite`, `UnknownSite`), a fact naming something with no node, a document contracts read that changed since Stage 2 or cannot be read safely (`SourceChanged`, `SourceRead`), an invalid contract row (`InvalidContract`), a configured layer glob that does not compile (`LayerRule`), a second or non-finite metric value (`DuplicateMetric`, `NonFiniteMetric`), facts about one node that contradict each other (`Invariant`) |
| `containment` | `Repo`, `Folder`, `File` nodes and a `FileRecord` per discovered file (`file_status`: every Stage 2 outcome mapped, an engine failure never `parsed`, a withheld file `redacted`); one node per definition by `node_kind`, parented to its enclosing definition or file; the engine's file-level module is the file node. `normalise_type` and `normalised_signature` for overload disambiguators; `is_repository_path` |
| `modules` | `module_node_key`: `(Module, scope, "<language>:<name>", "")` (issue 48); one-file modules parented to their file, spanning ones to the nearest common folder with sorted `props.files`; `props.module` on symbols, which keep their physical parents |
| `calls` | Resolutions as `call`/`reference` sites (4.2.1's fingerprint over the engine's node-type path, `split_callee`'s texts and the ordinal among same-path, same-text sites of the definition), drawn `CALLS`/`CALL_REFERENCE` edges and non-drawn candidate rows, the engine's numbers copied (issue 49) |
| `tests` | `is_test_path` (issue 31); a definition is a `Test` node only on its framework's evidence, its declared kind in `props.declared_kind`; a `TESTS` edge beside every drawn call a test makes |
| `routes` | `route_qualified_name`, `route_node_key`; every route binding the engine reports (`Definition::routes`: Spring, JAX-RS, `FastAPI`, Flask, Django REST framework) and every Express registration, as `Route` nodes in the handler's file, each with a `DEFINES_ROUTE` edge from the handler at its `route` site (the declaring annotation, decorator or call; one site for all the bindings one declaration makes). No site, no route (issue 54) |
| `entry` | `props.is_entry_point` where Appendix B.2's categories are proven by the facts: tests, route handlers, `is_main` by each language's convention (JVM, C#, C, C++, Go, Rust), Spring Boot applications, `FastAPI` and Flask applications, Express `listen` and `NestFactory.create` callers, ASP.NET top-level statements. The engine's own flag is `props.engine_entry_point` and makes no entry point (issue 55) |

### `contracts`

The contracts of 4.7.1 (P2-08), derived last in Stage 4: what the repository provides
and consumes across repositories, as `contracts` rows. Each source reports
`ContractObservation`s (a kind, a normalised key, its identities, a direction, a
provider's owner, raw form and evidence); one `ContractAccumulator` alone makes each
`namespace_key` and id and merges observations of one contract whatever order they
came in (issue 59). Documents are read through `prepare_source`/`prepare_candidate`
with Stage 2's blob and digest checked again; a redacted file, every `.env*` among
them, is never opened (issue 38).

| Item | |
|---|---|
| `identity` | `namespace_key` (`{"identity","key"}` JSON, `unresolved:<repo_id>:<key>`, an artifact's coordinate; issue 58), `contract_id` (`NodeKey::estate` over it), `ContractIdentity` (`Exact`, `Declared`, `Unresolved`); the key normalisers `api_key`, `api_path`, `http_method`, `url_authority`, `host_port`, `declared_name`, `simple_placeholder`, `unresolved_channel_key`, `table_key`, `column_key`, `identifier_parts`, `artifact_key`, `artifact_version_key`, `Ecosystem` |
| `ContractObservation`, `ContractAccumulator`, `validate` | One sighting; the merge (direction lattice, strongest identity, one owner or `owner_ambiguous` with `provider_owner_node_ids`, sorted evidence sets); `validate` applies `row::check` against the graph's nodes |
| `row` | `check` and `ContractRowError`: the rules every stored row obeys, the same in the accumulator and the segment writer (artifact exact with namespace = key; unresolved namespace exactly `unresolved:<repo>:<key>`; resolved namespace exactly the canonical JSON over the row's key and a non-empty identity; id = estate id of the namespace; owners are nodes; ambiguous-owner props consistent) |
| `calls` | Call evidence shared by library sources: `imports`/`imports_any` (Rust `::` paths too), `parameter_type` (a receiver's declared type from its callable's signature, any language's parameter syntax), `receiver_may_be`, `literal`, `literal_list`, `object_value`, `constructed_first`, `factory_literal` |
| `endpoints` | API providers with no `Route` node: `NestJS`, ASP.NET (attributes, minimal APIs), Hono, `net/http`, gin, echo, axum, actix-web; a registration on a router composed under a prefix is withheld (`PrefixUnknown`, issue 65) |
| `messaging` | `RULES`: broker client operations (`KafkaTemplate`, Kafka producers and consumers, `pika`, `amqplib`, NATS, Redis, JMS) by import provenance, operation and destination position; a call two rules read differently is ambiguous and gives nothing |
| `ContractDiagnostic`, `ContractProblem` | Evidence that gave nothing: `Unparseable`, `NotUtf8`, `TooDeep`, `NotADocument`, `UnfollowedRef`, `UnconfirmedChannel`, `PrefixUnknown` |
| `DeclaredIdentities` | `pdx.toml [identity]`, canonical |
| `config_values` | `ConfigValues`: the eligible values of `application*.yml` and `application*.properties` (never a credential key's, never a detected secret), `resolve`/`resolve_leading` (`${KEY}` with exactly one value), `kafka_cluster`, `rabbit_broker`, `datasource` (`jdbc_identity`); `parse_properties`, `flatten` |
| `document` | `Doc`, `parse_yaml` (events; aliases opaque, never expanded), `parse_json`; nesting bounded by `CONTRACT_DOCUMENT_MAX_DEPTH` |
| `annotation` | `parse`/`parse_in` an annotation's literal arguments; `string_literal`, `script_literal` (a `$` interpolates in Kotlin and Groovy) |
| `openapi` | OpenAPI 3 and Swagger 2 operations (`document_kind`): servers' literal authorities `exact`, their paths prefixes; no `$ref` followed |
| `routes` | Every `Route` node of P2-07 as a provided `ApiContract` it owns; HTTP client calls with a determined method and literal URL (`fetch`, `axios`, `requests`, `httpx`, `RestTemplate`, `WebClient`, `HttpClient`, `reqwest`, `net/http`) consumed |
| `proto` | `parse` (`Service`, a lexer that knows comments and strings); methods provided by their file; stub calls consumed only when resolution names a stub of exactly one service and method; always `unresolved` |
| `channels` | Engine channel facts a Kafka or AMQP client call confirms in a file importing the library (issue 61); `@KafkaListener`, `@RabbitListener`, `@JmsListener`; `AsyncAPI` 2 and 3 (`is_asyncapi`) |
| `tables` | `create_tables`, Liquibase, `prisma_models`, JPA, Entity Framework, `SQLAlchemy`, Django; `sql_access` (`Access::Read`/`Write`) for SQL string arguments |
| `artifacts` | `Coordinate`; `maven`, `gradle`, `npm`, `cargo`, `go_mod`, `nuget`, `pyproject`, `setup_cfg`, `alire`, `gpr`: literal coordinates and dependencies only |

### `layers::roles`

Each `Module` and `Class` node's `layer_role` (4.8.3, P2-09), set in Stage 4 after the
contracts: the repository's `pdx.toml [layers]` rules in order (`globset`, as
validated), then framework evidence with its provenance (`FRAMEWORK_RULES`, in 4.8.3's
order; `FrameworkRule`), then path segments (`PATH_ROLES`, `path_role`; a test path by
`is_test_path`), else `unknown`. Every other node has none; no `HAS_ROLE` or
`LAYER_DEPENDS` edge is made (P6-02's).

### `metrics`

The per-symbol metrics of 4.12.1 (P2-09), set last in Stage 4 from facts already held
(nothing is read again): `Rows` refuses a second value or a non-finite one and gives
the rows by node then metric; `METRICS` names them. A symbol is a definition's node,
not the file-level module or an inline module.

| Item | |
|---|---|
| `complexity` | `loc` from the definition's span (`loc`); the engine's `cyclomatic` (`cyclomatic`: 1 + its count), `cognitive` and `loop_depth` for callables (`is_callable`; issue 67); `transitive_loop_depth`, the deepest reachable along drawn `CALLS` with cycles condensed, and `props.recursive` |
| `fanio` | `count` of drawn `CALLS` edges by weight (`Fans`); `fan_in` and `fan_out` for every symbol, zero included |
| `importance` | `importance(fan_in, public, test)`: `sqrt(fan_in)` × 1.0 public or 0.6 otherwise (unknown included; issue 68) × 0.3 for a `Test` node or 1.0 |

### `languages`

The language matrix of the specification's Appendix A: the 31 languages PDX indexes,
how a file is assigned one, and the rules later stages apply to each. Its version is
`consts::LANGUAGE_MATRIX_VERSION`; any change to which language a file gets, or to a
rule, is a specification change that bumps it.

| Item | |
|---|---|
| `all()` | Every `Language`, in Appendix A's order |
| `by_id(id)` | The language with exactly this id; `None` for any other string, `tsx` included |
| `detect(path, content, sibling_exists)` | The language Appendix A assigns a file, or `None` |
| `Language` | `id`, `tier`, `extensions`, `name_prefixes`, `shebangs`, `module_rule`, `test_detection`; `test_rules()` follows a rule shared with another language, `engine_language(path)` names the engine grammar a file is extracted with |
| `Tier` | `Typed` (the engine resolves types) or `Structural` (the Rust stages alone resolve), as Appendix A declares it; never inferred from the engine |
| `ModuleRule` | Where a file's module name comes from: a `package` or `namespace` declaration, the directory (with `tsconfig` path mappings, or under the `go.mod` module path), a dotted path from the nearest `__init__.py` root, the `mod` tree from `lib.rs`/`main.rs`, and so on |
| `TestDetection`, `TestRule` | How tests are recognised: a `.test`/`.spec` stem on the language's own extensions (`SourceSuffix`), file-name patterns, directories, framework annotations, attributes and macros, or the same rules as another language, translated to its extensions (issue 31) |
| `HEADER_RULE` | Appendix A's rule for `.h`, the one extension two languages share |
| `ENGINE_DIALECTS` | Engine grammars named otherwise than their language: `.tsx` files are TypeScript, extracted with the engine's `tsx` grammar |

Detection is pure: it reads no file, no environment and not the engine. The caller
supplies the file's path, its bytes and whether a sibling exists. In order, and
case-sensitively:

1. the longest extension the file's name ends with, `.h` decided by `HEADER_RULE`;
2. a name prefix (`Dockerfile*`, `.env*`);
3. the interpreter the `#!` first line names, directly or through `env`; Appendix A
   names one, `bash`;
4. otherwise no language.

The rules are data: deriving `Module` nodes and `TESTS` edges from them is the derive
stage's (`index::derive`). Discovery's ignore rules, size limit, binary detection and `pdx.toml`
`[languages] extra` belong to the discover stage, which calls `detect`.

## Tests

```
cargo test -p pdx-core
```

| Test | |
|---|---|
| `languages` | `language_detection`: every extension and file pattern of Appendix A detects as its language, `.h` by the header rule, an extension before a name pattern, case as written, unknown names as none; `shebang_detection`: every form of bash's shebang, other interpreters and non-shebangs as none, a shebang never over an extension; `matrix_is_appendix_a`: every row's tier, patterns, shebangs and rules against a second transcription of Appendix A; `every_typed_language_has_engine_support`: the engine, through `pdx-engine`, knows every typed language and the grammars it needs; `every_language_has_an_engine_grammar`; `tsx_is_typescript_through_the_tsx_grammar`; integrity of ids, patterns and the header rule; the engine's C test list equals the matrix and its dialects |
| `ids` | Fixed vectors, computed outside the crate, for every identity; `node_id_ignores_lines`, `site_id_ignores_lines`, `site_id_changes_with_ast_path` and `overload_insert_does_not_renumber` as properties; URL canonicalisation and refusals; NUL refused everywhere |
| `bands` | `band_order_total`; drawn bands; spellings; `from_engine` over every Appendix D.3 strategy, score edge and candidate count, against a table written from D.3, and D.3's list against the engine's |
| `model` | Every vocabulary's complete spelling, and the exact JSON of every row type |
| `segment` | `schema_sql_is_4_3_verbatim`; `segment_roundtrip` over every table; `segment_is_byte_identical_across_builds` from reordered rows in different directories; `reader_refuses_wrong_schema_version` (version 1 and 3 refused), malformed meta and broken references on damaged copies; `fts_finds_qualified_names` and search semantics; read-only files, untouched by reading; the content hash against the file's bytes; hostile text and paths stay data |
| `discover` | `discover_honours_gitignore` (every exclusion source, their independence, hard excludes, `vendor`, `.ignore` and hidden files); `discover_skips_symlinks` (file, directory, outside the root, a loop); `discover_marks_binary_and_large` (the limit inclusive, the file over it never read, NUL is binary, non-UTF-8 is not); secret paths redacted unread; unknown languages kept; configured and header languages; sorted relative paths; root and ignore-file refusals |
| `secrets` | `secret_policy_digest_fixed_vector` and `detector_version_enters_secret_policy_digest` against vectors computed outside the crate; every detector's matches and what it keeps; what is not a credential left alone; non-UTF-8; `secret_detector_overlap_is_order_independent` over all 120 orders; `secret_normalisation_preserves_offsets` as a property |
| `extract` | `cache_hit_skips_engine`, `cache_format_4_is_a_miss`, `cached_extractions_keep_site_paths_and_derivation_facts`, `blob_sha_matches_git` (against `git hash-object` and fixed vectors), `memory_budget_batches`, `secret_policy_change_invalidates_cache`; the key's path and language; `cache_object_id_fixed_vector`; independence of the checkout's location; the node budget and every other extraction switch bypassing the cache; truncated, lossy and unclean entries; wrong digests, corruption and atomic private writes; failures, crashes and timeouts; files carried forward unopened; a changed checkout; order under any workers and batches; `secrets_do_not_reach_engine_or_cache` |
| `registry` | `registry_<lang>` for every typed language (a test holds the list to the matrix), each through discovery, extraction and the registry; `external_detection_python_stdlib`; duplicates and case kept; input order irrelevant; every module rule; imports internal, external, unresolved and ambiguous; Python packages and relative imports, tsconfig aliases and `extends`, Go modules nested, Rust path dependencies and the standard crates, C pairing; hierarchy transitive, cycle-safe, ambiguity unforced; fresh and cached extractions the same registry; Rust `impl` relations resolved, an ambiguous trait kept ambiguous; the engine resolving with the registry's metadata to the registry's files, a root `tsconfig.json` beside a Go module included (`polyglot_ts_alias_does_not_affect_go`); refusals of disagreeing stages, malformed extractions, bad and changed metadata; redacted and symlinked metadata never read |
| `stages` | `resolution_stage_matrix` (all nine outcomes, each row's evidence stated); `narrowing_keeps_narrowest_set`; `blocklist_precedes_all`; `typed_requires_lsp_typed_single_candidate_and_min_score`; `engine_hints_never_restrict_candidate_universe`; typed targets mapped uniquely, engine-proven external targets, unvalidated hints; aliases, module receivers, Java static imports and C includes import-guided; `super`, implicit receivers and Rust `impl` traits through the hierarchy; scoped by module, not directory; no cross-family candidate; lexical bindings, unresolved members, `typed_only`, references and engine-found sites never resolved by name; degraded runs; duplicate and impossible answers refused; resolution shapes; typed resolution over a polyglot repository, in any file order, fresh or cached |
| `derive` | Through the whole pipeline: `derive_<lang>` golden snapshots for every typed language and Ada (a test holds the list to the matrix); `routes_spring`, `routes_fastapi`, `routes_express`, `tests_junit`, `tests_pytest`; the review closure's `every_non_containment_edge_has_a_site`, `spring_route_has_real_site`, `spring_multiple_paths_create_multiple_routes`, `spring_request_mapping_multiple_methods`, `spring_multiple_routes_have_deterministic_ids`, `route_line_shift_keeps_route_site_id`, `exported_typescript_function_is_not_entry_point`, `exported_javascript_function_is_not_entry_point`, `main_entry_points_survive_engine_flag_filter`, `entry_point_categories_are_kept`, `duplicate_noncallables_have_unique_deterministic_ids`; `test_rules_follow_issue_31`; the Part U module tests and `module_id_fixed_vectors`; overload, route and site identities held over line insertions, moves, unrelated definitions and calls, and changed by nesting, method and path; no identity depending on the checkout; host-native paths refused; faithful file records; the engine's numbers copied to edges and candidate rows; unconfirmed and unplaced sites counted, never stored; output independent of input order; the derived graph written and read back as a segment |
| `layers` | `layer_role_precedence` (configuration over framework over path over `unknown`, for a class and a module), `layer_role_first_config_rule_wins`, `layer_roles_framework_annotations` (every framework rule with its provenance; JPA `@Entity` alone withheld, issue 64), `layer_role_framework_tie_break_follows_4_8_3_order`, `layer_role_false_positive_annotations_are_withheld`, `layer_roles_path_conventions`, `layer_role_path_conflict_follows_4_8_3_order`, `layer_role_test_paths_reuse_language_rules`, `layer_role_multifile_module_is_deterministic`, `layer_role_only_module_and_class`, `layer_roles_are_written_into_no_props` |
| `metrics` | Hand-computed values on Java, Python, Go, TypeScript, Rust and C: `cyclomatic_known_values`, `cognitive_and_loop_depth_known_values`, `complexity_engine_facts_cross_stage_boundary`; `complexity_metrics_only_on_callables`, `loc_uses_definition_span`, `fan_in_out_drawn_calls_only`, `fan_counts_edge_weight`, `self_call_counts_both_fans`, `transitive_loop_depth_acyclic`, `transitive_loop_depth_recursive_scc`, `importance_formula`, `visibility_props_only_where_engine_evidence`, `metrics_exclude_file_level_module`, `metrics_are_deterministic`, `derive_returns_layer_roles_and_metrics`, and the static guard `roles_and_metrics_are_pure` |
| `config` | `pdx_toml_defaults` field by field; the Appendix C reference with all four rule forms and every precise family, commands never run; global and family timeouts; the `[languages] extra` rules; every refusal, each naming the file and key |
| `consts` | Every constant is documented, and the interface's mirror is real |

The crate's dependencies are `pdx-engine` (Stage 2 extracts through it), `serde`,
`serde_json`, `sha2`, `rusqlite` (bundled SQLite), `tempfile`, `thiserror`, `ignore`
and `globset` (gitignore and glob semantics), `toml`, `postcard` (cache entries),
`rayon` (Stage 2's workers), `regex` (the secret detectors) and `sha1` (the Git blob
identity only); for its tests only, `insta` (snapshots; issue 52).
