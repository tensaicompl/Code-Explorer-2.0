# pdx-core

The graph model, the indexing pipeline, resolution, segments and layout.

Open. Apache-2.0; see `LICENSE` at the repository root.

The indexing pipeline, the graph model and everything after them are not built yet;
they arrive with the tasks that implement them. What exists is below.

## Public API

### `consts`

Every constant the specification names, defined once: schema and format versions,
resolution thresholds, budgets and limits. The interface mirrors the subset it needs,
and `scripts/consts-sync.py` fails the build when the two disagree.

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
| `consts` | Every constant is documented, and the interface's mirror is real |

The engine tests need the engine built, which `pdx-engine` does through
`pdx-engine-sys`; `pdx-engine` is a dev-dependency only.
