# pdx-core

The graph model, the indexing pipeline, resolution, segments and layout.

Open. Apache-2.0; see `LICENSE` at the repository root.

The graph model's vocabulary, identities and rows exist; the indexing pipeline,
segments and everything after them are not built yet and arrive with the tasks that
implement them. What exists is below.

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
| `consts` | Every constant is documented, and the interface's mirror is real |

The engine tests need the engine built, which `pdx-engine` does through
`pdx-engine-sys`; `pdx-engine` is a dev-dependency only. The crate's own dependencies
are `serde`, `serde_json` and `sha2`.
