# pdx-core

The graph model, the indexing pipeline, resolution, segments and layout.

Open. Apache-2.0; see `LICENSE` at the repository root.

The graph model's vocabulary, identities and rows exist, and so do segments, the
files that store them, a repository's configuration and the pipeline's first stage,
discovery; the later stages arrive with the tasks that implement them. What exists is
below.

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
4.2.4, with `requires_band` for the kinds 4.2.2 bands), `LayerRole`, `SiteKind` and
`ContractKind`. Each vocabulary has exactly one spelling per value, used for display,
parsing and serialisation; anything else is refused.

### `model`

The segment's rows (4.3), each field a column of the same name, nullable columns as
`Option`: `Node`, `Site` (with its `Span`), `Edge`, `CandidateSite`, `Contract`,
`Metric`, `Coverage` (with `BandCounts`, one count per band in 4.2.2's order). `props`
is a map kept in key order. Edges keep the engine's score, strategy and candidate count
verbatim beside the band, and observation as a flag that changes neither band nor id.
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
| `SegmentError` | One error type: SQLite, I/O, wrong schema version, missing or malformed meta, broken references, stored values that are not what their column must hold, rows the writer refuses, an existing destination, invalid search syntax |

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
| `TestDetection`, `TestRule` | How tests are recognised: file-name patterns, directories, framework annotations, attributes and macros, or the same rules as another language |
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
stage's. Discovery's ignore rules, size limit, binary detection and `pdx.toml`
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
| `segment` | `schema_sql_is_4_3_verbatim`; `segment_roundtrip` over every table; `segment_is_byte_identical_across_builds` from reordered rows in different directories; `reader_refuses_wrong_schema_version`, malformed meta and broken references on damaged copies; `fts_finds_qualified_names` and search semantics; read-only files, untouched by reading; the content hash against the file's bytes; hostile text and paths stay data |
| `discover` | `discover_honours_gitignore` (every exclusion source, their independence, hard excludes, `vendor`, `.ignore` and hidden files); `discover_skips_symlinks` (file, directory, outside the root, a loop); `discover_marks_binary_and_large` (the limit inclusive, the file over it never read, NUL is binary, non-UTF-8 is not); secret paths redacted unread; unknown languages kept; configured and header languages; sorted relative paths; root and ignore-file refusals |
| `config` | `pdx_toml_defaults` field by field; the Appendix C reference with all four rule forms and every precise family, commands never run; global and family timeouts; the `[languages] extra` rules; every refusal, each naming the file and key |
| `consts` | Every constant is documented, and the interface's mirror is real |

The engine tests need the engine built, which `pdx-engine` does through
`pdx-engine-sys`; `pdx-engine` is a dev-dependency only. The crate's own dependencies
are `serde`, `serde_json`, `sha2`, `rusqlite` (bundled SQLite), `tempfile`,
`thiserror`, `ignore` and `globset` (gitignore and glob semantics) and `toml`.
