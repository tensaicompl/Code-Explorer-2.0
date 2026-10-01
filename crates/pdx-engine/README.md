# pdx-engine

Safe wrapper over the extraction engine, with crash isolation.

Open. Apache-2.0; see `LICENSE` at the repository root.

Everything above this crate reaches the engine through it. Nothing here requires
`unsafe` of a caller, and nothing returned borrows from the engine.

## Public API

### Extraction

`Engine` is the engine for one thread. It is neither `Send` nor `Sync`, because the
engine keeps parser state per thread, and a thread can have only one at a time.
Parallelism belongs to the caller: one engine per thread.

`Engine::extract(language, rel_path, source)` returns a `FileExtract`: everything the
engine found in the file, owned, in the engine's order:

| Field | |
|---|---|
| `language`, `rel_path`, `source_len` | What it was extracted from. Qualified names depend on the path |
| `status` | `Parsed`, `Partial` (the tree has errors) or `Failed` |
| `truncated` | The extractor stopped at its node budget, which is off unless the environment sets one |
| `definitions` | With normalised kind and the engine's own, spans, parent, visibility, test and entry-point flags, and three complexity metrics |
| `calls` | Calls, then callables passed as values (`is_reference`); `typed_only` sites; lexical facts |
| `imports`, `usages` | Usages with their lexical facts |
| `type_refs`, `throws`, `read_writes`, `channels`, `env_accesses` | Positionless: the engine records only their scope |
| `diagnostics` | What the engine reported about the file |
| `surface` | The file's resolution surface: opaque, and what lets the file be resolved again without being extracted again |

A position the engine does not know is `None`; an index of "no enclosing definition"
is `None`. The wrapper takes every structure of the engine's interface apart field by
field, so a field the interface gains cannot go missing here without the build
failing. `FileExtract` is serialisable (serde), for the extraction cache and the
isolated worker.

### Typed resolution

`ProjectResolver::new(&engine)` begins a project. Files are added with
`add_file(&extract, source)`, from an extraction that may have come from a cache or
another process, or with `extract_and_add(language, rel_path, source)`;
`set_metadata` supplies the repository's packages, path aliases and crate manifest.
`run()` consumes the resolver and returns a `ProjectResolution`:

- `resolutions`: one `TypedResolution` per answered site, with the site
  (`site_ref`: file and call index, and the `site` itself), `target_qn`,
  `target_rel_path`, `score`, the normalised `strategy`, the engine's own
  `engine_strategy`, and `candidates`.
- `health`: a `RunHealth`, `Clean` or `Degraded`, with every file counted once by
  what typed resolution did with it. A degraded run lost work, so a site without an
  answer may never have been asked about; it still reports every answer it found.

A run that could not complete is an `Err`. The resolver owns every engine object the
project reads, freeing them only after the project ends, so a caller cannot free or
reuse one under it; dropping the resolver, run or not, frees everything.
`Engine::live_handles` counts what is still alive, for leak checks.

Resolving from cached extractions gives exactly the answers resolving from fresh ones
does.

### Crash isolation

With `PDX_ENGINE_ISOLATE=1`, `isolate::Extractor::from_env` extracts in a worker
process: the same program run with the hidden `engine-worker` subcommand, which
calls `isolate::serve_worker`. If the engine aborts, only the worker dies; the
batch's files are extracted again one at a time, the file that kills the worker on its
own is `Failed(EngineCrash)`, reason `engine_crash`, and every other file is
extracted, exactly as in process. Nothing a dying worker sent is used. Workers are
ordinary child processes on every system.

The protocol is length-prefixed postcard frames (decision 19), with a size limit; the
worker names its protocol and engine version first and is refused if they differ.

### Errors

`EngineError` maps each of the interface's status codes to a variant, and adds the
failures the wrapper detects: an engine contract broken, an argument the engine
cannot take, a second engine on a thread, and a source that is not the one an
extraction was taken from.

## Tests

```
cargo test -p pdx-engine
```

| Test | |
|---|---|
| `extraction` | Every array of the interface arrives; throws name their exception and scope; definitions keep every field; an extraction outlives its engine and survives serialisation |
| `resolution` | Cached extractions resolve exactly as fresh ones, over every typed-resolution fixture, and every run is clean; answers carry their site and both strategies |
| `run_health` | Clean with no answers; degraded even with no answers; a degraded run keeps the answers it found |
| `ownership` | `Engine` and `ProjectResolver` are neither `Send` nor `Sync`; one engine per thread; a project frees everything after an error; answers outlive their project |
| `determinism` | `extract_is_deterministic`: property tests over generated programs and over every matrix language's fixture with arbitrary bytes edited in |

The isolation acceptance test, `engine_isolate_recovers`, is in the `pdx` crate, whose
binary is the worker.
