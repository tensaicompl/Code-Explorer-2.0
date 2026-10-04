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
| `language`, `rel_path` | What it was extracted as. Qualified names depend on the path |
| `source_len`, `source_digest` | The source it was extracted from: its length, and SHA-256 of exactly the bytes extracted. Not a version-control identity, which is computed over other bytes |
| `status` | `Parsed`, `Partial` (the tree has errors) or `Failed` |
| `truncated` | The extractor stopped at its node budget, which is off unless the environment sets one |
| `extraction_lost` | Work lost while the file was extracted: allocations that failed and work budgets that ran out. 0 when nothing was lost; otherwise the extraction is degraded, and its surface carries the same count into every project that resolves it |
| `declared_namespace` | The package or namespace the file declares, as written, for the languages whose declaration the engine reads (Java, Kotlin, C#, PHP); `None` otherwise. `namespace_evidence(language)` says where a language's evidence is: this field, C++'s qualified names, or nowhere |
| `definitions` | With normalised kind and the engine's own, spans, parent, visibility, test and entry-point flags, three complexity metrics, and `base_classes`: the bases it names, in the engine's order and spelling, unresolved |
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

### The engine's switches

The engine reads a few environment variables. Six change what an extraction contains,
and none is set normally: `EXTRACTION_SWITCHES` names them, the node budget
(`NODE_BUDGET_ENV`, `PDX_ENGINE_WALK_MAX_NODES`) among them, and
`extraction_switch_set()` says whether any is set, to any value. No extraction cache
key names them, so a build with one set uses no cache at all
(`docs/plan/ISSUES.md`, issue 26). Every other variable the engine reads leaves its
output as it is or exists only in test builds; a test holds the engine's sources to
that classification.

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

A source is checked against the extraction's length and digest before it is added:
any other source, even of the same length, is `SourceMismatch`, which says how it
differs and never what either contains.

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

The protocol is length-prefixed postcard frames (decision 19), with a size limit, each
encoded straight into the pipe so a batch's sources are not copied on the way; the
worker names its protocol (3: since a definition carries its bases and an extraction its
declared namespace) and engine version first and is refused if they differ.

Every exchange with a worker, its introduction included, is bounded in time: the
request written and the response read within `DEFAULT_EXCHANGE_TIMEOUT` (120 s), or
the timeout given to `IsolatedExtractor::with_timeout`. The exchange runs on a thread
of its own while the caller waits; past the timeout the worker is killed and reaped,
which ends the thread, the thread is joined, and whatever the worker sent is
discarded with it. `extract_batch` then returns `IsolationError::Timeout`, naming the
timeout and the worker's process id. A timeout is not a crash: nothing is retried and
no file is recorded as `engine_crash`, because a wall-clock limit depends on the
machine, and a build that met one fails rather than publish what another machine would
not. The extractor starts a fresh worker for its next batch.

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
| `resolution` | Cached extractions resolve exactly as fresh ones, over every typed-resolution fixture, and every run is clean; answers carry their site and both strategies; `same_length_different_source_is_rejected`; the digest is SHA-256 of the extracted bytes |
| `run_health` | Clean with no answers; degraded even with no answers; a degraded run keeps the answers it found; `typed_resolution_internal_failure_degrades_run`: an answer lost inside typed resolution degrades the run and the rest remain |
| `ownership` | `Engine` and `ProjectResolver` are neither `Send` nor `Sync`; one engine per thread; a project frees everything after an error; answers outlive their project |
| `determinism` | `extract_is_deterministic`: property tests over generated programs and over every matrix language's fixture with arbitrary bytes edited in |
| `declarations` | `base_classes_cross_the_safe_boundary` (one base, several in the engine's order, none, the engine's spelling); they and declared namespaces survive postcard; a cached extraction with bases resolves as a fresh one; `declared_namespace` for each language that has one, block and file-scoped C# alike; `namespace_evidence` held to what the engine records |
| `budgets` | Nothing is lost or truncated normally; a starved TypeScript budget is reported in `extraction_lost`; `NODE_BUDGET_ENV` is the variable the engine reads; a switch set to any value counts as set; `every_engine_variable_is_classified` over the engine's sources |

The isolation tests are in the `pdx` crate, whose binary is the worker:
`engine_isolate_recovers`, `isolation_does_not_change_what_is_extracted`,
`engine_isolate_times_out_and_reaps_worker` (a worker spinning on a file is stopped
and reaped, the error is a timeout and not a crash, and the extractor works
afterwards), `a_worker_that_never_introduces_itself_times_out` (Unix), and
`extraction_lost_is_what_the_surface_carries` (a worker's lost work, resolved in the
parent, is the run's lost work).
