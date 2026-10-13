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
engine found in the file, owned, in the engine's order, bound to the source it was
taken from (`extract_shared` takes an `Arc<[u8]>` the caller already holds rather than
copying it):

| Field | |
|---|---|
| `language`, `rel_path` | What it was extracted as. Qualified names depend on the path |
| `source_len`, `source_digest` | The source it was extracted from: its length, and SHA-256 of exactly the bytes extracted. Not a version-control identity, which is computed over other bytes |
| `status` | `Parsed`, `Partial` (a region of the tree has errors the engine could not recover a definition from) or `Failed` |
| `truncated` | The extractor stopped at its node budget, which is off unless the environment sets one |
| `extraction_lost` | Work lost while the file was extracted: allocations that failed and work budgets that ran out. 0 when nothing was lost; otherwise the extraction is degraded, and its surface carries the same count into every project that resolves it |
| `declared_namespace` | The package or namespace the file declares, as written, for the languages whose declaration the engine reads (Java, Kotlin, C#, PHP); `None` otherwise. `namespace_evidence(language)` says where a language's evidence is: this field, C++'s qualified names, or nowhere |
| `definitions` | With normalised kind and the engine's own, spans, parent, visibility, test and entry-point flags, three complexity metrics, and `base_classes`: the bases it names, in the engine's order and spelling, unresolved. `decorators` (annotations, decorators and attributes as written) and `signature_param_types` (the declared parameter types, `?` for an unknown one), as the engine extracted them (issue 47). `routes` (`RouteFact`): every HTTP route binding its annotations or decorators declare (Spring, JAX-RS, FastAPI, Flask, Django REST framework), one per method and path, joined to the class's own mapping, each with its callee as written, the declaration's text, position and node-type path; a path or method that is not a literal gives none (issue 54). The engine's `is_test` flag marks every definition of a test file, helpers included |
| `impl_traits` | Rust's `impl Trait for Type` blocks (`ImplTrait`: the trait and the type as spelt, without type arguments, and the type's qualified name), empty blocks included, in the engine's order; empty for every other language. Supertraits are not recorded (issue 42) |
| `calls` | Calls, then callables passed as values (`is_reference`); `typed_only` sites; lexical facts. `caller`: the innermost enclosing definition the extractor represents, never a nested callable it does not (issue 75), joined to it by its declaration occurrence, never by comparing names (issue 76); for a call with no position in the file, the callable its name names. `ast_path`: the node types from that definition's node (the root's, at file scope) down to the site's own, for every site with a position in the file (issue 46); empty for a site found only in preprocessed text. `args` (`CallArg`): each argument's text, its literal value where the engine resolved one, its keyword, and its position (issue 47). A callee and an argument's expression are `TextRef`s |
| `imports`, `usages` | Usages with their lexical facts; a usage's name is a `TextRef` |
| `type_refs`, `throws`, `read_writes`, `channels`, `env_accesses` | Positionless: the engine records only their scope |
| `diagnostics` | What the engine reported about the file |
| `surface` | The file's resolution surface: opaque, and what lets the file be resolved again without being extracted again |

A position the engine does not know is `None`; an index of "no enclosing definition"
is `None`. The wrapper takes every structure of the engine's interface apart field by
field, so a field the interface gains cannot go missing here without the build
failing.

Text a fact takes from the file is a `TextRef` (issue 90, decision 61): `Source { start,
end }`, the bytes of the source it was taken from, or `Owned`, text the engine assembled
or read after C-family macro expansion. A range is never resolved on the way in, out or
through serialisation; `FileExtract::text` reads it, borrowed (bytes that are not UTF-8
replaced as the engine's strings always were). A definition's `qualified_name` is a
`QualifiedName`: `Whole`, `Member { base }` (an earlier definition's name, `.`, its own
name), `Extended { base, rest }` or `Prefix { base, keep, rest }` (the first `keep`
bytes of an earlier definition's name, ending before a `.` it adds, then `rest`: C++
functions in nested namespaces, which are no definitions), never a whole name repeated
for each nested definition; a definition's `signature`, its parameter list, is a
`TextRef` too; `qualified_name(i)` spells one out, `qualified_name_parts(i)` gives its pieces
in order, and `qualified_names()` spells every name of a file once, for a consumer that
needs them all (node identities, the registry's index). A range's coordinates are
equality only within one extraction: compare text across extractions by what `text`
gives.

What is stored and sent is the `ExtractRecord` (`record()`, `into_record()`):
serialisable (serde, postcard), with no source in it. `ExtractRecord::bind(source)`
binds a record to the bytes it was taken from, checking their length and digest and that
every range lies within them, every qualified name's base is an earlier definition and
every prefix ends before a `.` its base adds (in the base's own bytes, so the check is
linear); anything else is a `BindError`, never clamped. The engine's own records are
checked the same way when converted.

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
worker names its protocol (7: since the worker answers with records whose text is ranges
of the source sent, which the parent binds to that source, never receiving it back,
after 6 for every route binding with its declaring node, 5 for sites' node-type paths,
arguments, decorators, parameter types and one route, 4 for `impl Trait for Type`
relations and 3 for bases and declared namespaces) and engine version first and is
refused if they differ. An answer that does not bind to the source sent for it is no
answer of the protocol: the exchange fails as a worker dying does.

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
| `impl_traits` | `impl_traits_cross_the_safe_boundary` (none, one from an empty block, several in the source's order, a module-qualified trait kept whole, type arguments left off, Rust only); they survive postcard; a cached extraction with them resolves as a fresh one |
| `site_facts` | `call_ast_path_crosses_the_boundary` and `reference_ast_path_crosses_the_boundary` (exact paths, in a method, under an `if`, at file scope); `every_site_in_the_source_has_a_path` over every fixture; `derivation_facts_cross_the_boundary` (Spring, `FastAPI` and Express: decorators, parameter types, routes with their positions and paths, positional and keyword arguments with their values); `route_facts_cross_the_boundary` (several paths and methods in one Spring or Kotlin mapping, a class prefix, a marker annotation, stacked and Flask decorators, a constant path giving none); they survive postcard |
| `occurrence_ownership` | Issue 76: a callable the walk names otherwise than its definition owns its sites, its path starting at its node: a Rust method of a generic type (`rust_method_of_a_generic_type_owns_its_calls`, `..._owns_its_callable_references`), `cfg` twins (`rust_cfg_twins_each_own_their_calls`), functions on one line (`functions_on_one_line_each_own_their_calls`, the file's own call after them included), an Ada specification's aspect (`ada_specification_aspect_is_owned_by_its_subprogram`), a C statement misread as a function (`c_statement_misread_as_a_function_is_owned_by_its_function`), a C++ test macro and an out-of-line template member (`cpp_test_macro_owns_its_calls`, `cpp_template_member_defined_out_of_line_owns_its_calls`), a C++ method only the macro-expanded text gave (`cpp_method_adopted_from_expanded_text_owns_its_raw_calls`, its expanded call joined by occurrence) and a superseded definition's sites going to its replacement (`cpp_superseded_definition_hands_its_sites_to_its_replacement`), a Kotlin secondary constructor the walk cannot name (`kotlin_secondary_constructor_owns_its_delegation_calls`); typed answers still join in a generic type's method, and cached extractions answer as fresh ones (`typed_answers_join_calls_in_a_method_of_a_generic_type`, `typed_answers_in_a_method_of_a_generic_type_are_the_same_from_a_cached_extraction`, `typed_answers_in_a_cpp_test_macro_are_the_same_from_a_cached_extraction`) |
| `nested_ownership` | Issue 75: a call or callable reference in a nested callable the extractor does not represent is owned by the outer definition, its path running from the outer node through the nested definition (`unmodelled_nested_call_is_owned_by_the_outer_definition`, `site_path_starts_at_the_modelled_owner`, `callable_reference_in_nested_callable_is_owned_by_the_outer_definition`); a nested `helper` never borrows a top-level `helper` (`nested_same_name_does_not_borrow_a_top_level_occurrence`); nested parameters stay in the nested scope (`nested_parameters_stay_in_the_nested_lexical_scope`); typed answers still join calls in a nested Kotlin function (`typed_answers_still_join_calls_in_unmodelled_nested_callables`); decorator wrappers and methods of a class in a function (`decorator_wrapper_and_local_class_methods_are_owned_by_the_outer_definition`); a represented nested function keeps its ownership (`modelled_nested_function_keeps_its_own_ownership`, TypeScript); file scope stays the file's (`module_level_call_stays_file_scoped`) |
| `budgets` | Nothing is lost or truncated normally; a starved TypeScript budget is reported in `extraction_lost`; the C++ expression evaluator's budget grows with the source (`cpp_expression_budget_scales_with_source`: a generated four-tranche source, twice the old fixed 10,000 steps, loses nothing) and running it out is still reported (`cpp_expression_budget_running_out_is_lost_work`), issue 74; `NODE_BUDGET_ENV` is the variable the engine reads; a switch set to any value counts as set; `every_engine_variable_is_classified` over the engine's sources |
| `type_sizes` | Issue 85: `sizeof` in `#if` takes the fixed LP64 model's branches on every platform, with the same expectations on Linux, macOS and Windows (`sizeof_in_if_takes_the_fixed_models_branches_on_every_platform`); a 64-bit Windows, an Apple silicon and a 32-bit build's sizes, simulated in child processes through `PDX_ENGINE_TEST_HOST_SIZES`, change no extraction (`host_type_sizes_cannot_change_an_extraction`) |
| `syntax_depth` | Issue 89, the same on every platform and asserted by structure, never time: C at depth 511 and 512 is extracted, 513 is refused, lost work once and no fact (`depths_511_and_512_are_extracted_and_513_is_not`); Java classes, JavaScript functions, a Python member chain and C calls nested 300 deep are refused alike, the same on every run, and 150 deep extracted (`nested_classes_functions_member_chains_and_calls_past_the_depth_are_refused`); the issue's 1,000-deep reproducers are refused (`a_thousand_levels_are_refused_by_structure`) |
| `usage_ancestry` | Issue 90, asserted by structure, never time: a 400-link Python member chain, eight of them in one function, Java classes nested 250 deep and JavaScript functions nested 240 deep extract whole, with no work lost and every fact present, the same on every run (`the_issues_fixtures_extract_whole`); answering from the walk's frames gives exactly what climbing gave, the test seam `PDX_ENGINE_TEST_CLIMB_ANCESTRY` making a child process climb, for the issue's shapes and Python's, JavaScript's and Rust's usage rules (`the_frames_answer_what_climbing_answered`). The work is counted in `engine/tests/usage_ancestry.c` |
| `scanner_state` | Issue 91, the same on every platform: 510 bytes of compact YAML sequences, which wrote past the parser's state buffer and crashed the process, are a partial parse with no work lost (`the_old_crash_is_a_partial_parse`); YAML mappings and sequences, Markdown quotes and lists, Kotlin string templates and Python blocks nested either side of their scanners' bounds extract, the same on every run (`each_scanner_extracts_either_side_of_its_ceiling_the_same_on_every_run`). The bounds themselves are tested on the scanners' own code in `engine/tests/scanner_state.c` |
| `site_path_budget` | Issue 88, the same on every platform: statements of 100 nested calls in C, Python, Java and JavaScript pass the file's 500,000-entry path budget; every call remains, every path kept is complete (from the owner's node type to the call's), none follows the first absent one, and the budget loses work exactly once more than the same function under it (`a_files_paths_are_bounded_complete_or_absent_in_every_language`); 40 statements keep every path, 412,000 entries (`a_file_under_the_budget_keeps_every_path`); a site added after the walk keeps the path its owner gives it, exactly as before (`a_late_sites_path_is_the_one_its_owner_gives`) |
| `preprocess_bounds` | Issue 87, with the same expectations on Linux, macOS and Windows and no variable set: `#line` 400000000, 2147483647 and 4294967295 lose nothing and find exactly what a near `#line` does (`a_far_line_jump_is_a_marker_and_never_a_fact`); doubling macros expand below their ceilings (`a_doubling_macro_expands_below_its_ceilings`), and above them lose work once, keep the source's own facts and are the same on every run (`a_doubling_macro_above_its_ceilings_is_lost_work_once`), as do 2^40 expansions of a macro that expands to nothing (`doubling_macros_that_create_nothing_are_lost_work_once`); a macro nested 256 deep expands and 257 deep is lost work (`nesting_stops_at_its_ceiling_on_every_platform`); 80,000 nested parentheses in an `#if` stop at the folding steps (`nested_parentheses_in_an_if_are_lost_work_once`); `#if` arithmetic wraps, shifts and divides by the 64-bit model and out-of-range literals saturate (`if_arithmetic_is_the_same_on_every_platform`); a negative shift count and a division by zero fail the condition (`an_if_that_cannot_be_evaluated_fails_on_every_platform`) |
| `source_text` | Issue 90B, decision 61: a call's callee and argument and a Python attribute chain are ranges of the source, macro-expanded C text is owned (`ranges_are_the_text_and_expanded_text_is_owned`); a nested definition's name is a `Member` of its parent's and spells out byte for byte; a record serialises without its source and binds to a fresh copy as the same extraction (`a_record_survives_serialisation_and_binds_to_its_source_again`); another length, other bytes of the same length and a forged digest are refused (`a_record_binds_only_to_its_own_source`); a range past the source or starting past its end, in a callee, a usage or an argument, and a name based on itself or a later definition are refused, never clamped (`a_record_whose_ranges_or_names_do_not_fit_is_refused`); in the four shapes issue 90 measured and the two its audit found, nested 40, 80 and 160 deep, owned text and the encoded record grow with the source (`retained_text_grows_with_the_source_not_the_nesting`); C++ functions in nested namespaces hold prefixes of earlier names, spelled out and bound again byte for byte (`names_in_nested_namespaces_share_their_prefix`), and a prefix ending at no `.`, at or past its base's length, in its base's own base's bytes, of nothing, or with a rest after no `.` is refused (`a_prefix_that_is_not_one_its_base_adds_is_refused`); parameter lists are ranges, a JavaScript function given as a default included, and an adopted C++ method's is owned (`parameter_lists_are_ranges_and_expanded_ones_owned`) |
| `literals_and_lines` | Issue 86: character constants and `#line` numbers take the fixed rules with the same expectations on Linux, macOS and Windows: `#if '\xff' < 0` takes the signed branch, `'\x80'` is -128 and a multi-character constant a 32-bit int (`character_constants_take_the_signed_char_rule_on_every_platform`); `#line 2147483648` gives that line and `#line 4294967296` gives 4294967295, reached in files of their own so that nothing is padded (`line_numbers_take_the_fixed_rule_on_every_platform`) |

The isolation tests are in the `pdx` crate, whose binary is the worker:
`engine_isolate_recovers`, `isolation_does_not_change_what_is_extracted`,
`isolation_carries_site_paths_and_derivation_facts`,
`isolation_carries_text_as_ranges_bound_to_the_source_sent` (a worker's answer holds
its texts as ranges, bound in the parent to the source it sent: the same extraction
as in process), `engine_isolate_times_out_and_reaps_worker` (a worker spinning on a file is stopped
and reaped, the error is a timeout and not a crash, and the extractor works
afterwards), `a_worker_that_never_introduces_itself_times_out` (Unix), and
`extraction_lost_is_what_the_surface_carries` (a worker's lost work, resolved in the
parent, is the run's lost work). The worker's protocol is tested in `isolate.rs` itself:
`a_worker_of_another_protocol_is_refused` (protocol 6 among them) and
`a_worker_answers_in_proportion_to_the_facts` (an answer's size, for Python attribute
chains 40, 80 and 160 deep, grows with the facts and not the chains' text).
