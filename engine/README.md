# engine

The extraction and typed-resolution engine, built as the static library `libpdxe.a`.

This is third-party source under our maintenance, not code developed here. New
capability belongs in Rust. Changes to these sources are limited to four things:
what the vendoring scripts rename, what the strip list removes, the interface layer
in `api/`, and the patches in `patches/`. Editing a vendored file in place is how a
refresh silently loses a fix.

## Building

```
make engine          # target/engine/libpdxe.a and the test programs
make engine-test     # the interface's tests (part of make check)
make check-asan      # the same tests, and the sanitizer corpus, under the address,
                     # undefined-behaviour and leak sanitizers (make asan is the same)
```

or directly:

```
cmake -S engine -B build -DPDXE_BUILD_TESTS=ON
cmake --build build -j
ctest --test-dir build --output-on-failure
```

Requires CMake 3.25 or later and a C17 and C++17 compiler. Built and tested with
gcc 13 and clang 18. One translation unit is C++ (macro preprocessing); everything
else is C.

On Windows the engine is built by clang for the MSVC ABI, through Ninja, as
`crates/pdx-engine-sys` does; Microsoft's own compiler is not supported. `api/windows`
supplies what that runtime lacks, and the project names the system libraries the
engine's Windows code calls (issue 24).

The grammars are not compiled directly. Each language has a wrapper in `src/` that
includes its parser and scanner, which is how the generated tables are kept apart:
they declare static symbols of the same names, so two in one translation unit would
collide. The typed-resolution layer is likewise one translation unit,
`src/lsp_all.c`, because its sources are written to be compiled together.

### Options

| Option | Default | Effect |
|---|---|---|
| `PDXE_VENDORED_WERROR` | `ON` | Warnings are errors in the vendored sources, as everywhere else. A short list of dead-code and style warning classes stays visible but non-fatal in vendored files only; the interface layer has no exemption, which the test `api_warnings_are_errors` checks (issue 19). Turn it off only to let an untested newer compiler finish while its warnings are looked at |
| `PDXE_BUILD_TESTS` | `OFF` | Builds the test programs in `tests/` and registers them with ctest. `make engine` turns it on |
| `PDXE_TEST_SEAMS` | `OFF` | Compiles the engine's fault-injection switches, with which a test makes it abort on a named file (`PDX_ENGINE_TEST_CRASH_ON`) or skip one in typed resolution (`PDX_ENGINE_TEST_LSP_SKIP_ON`). Each reads the environment, so a binary built with them can be crashed by an environment variable: tests only. `make engine` turns it on, and the `test-seams` feature of `pdx-engine-sys`, which only dev-dependencies enable; `scripts/no-test-switches.sh` checks a binary has none |

Whatever the switch, four warning classes are always errors, because each is a
defect rather than style: a call with no declaration in scope, and integer, pointer
and return-type mismatches.

## Layout

| Path | Contents |
|---|---|
| `src/` | Extraction, the typed-resolution layer under `lsp/`, the foundation under `foundation/`, and the cross-file resolution driver under `resolve/` |
| `src/resolve/` | Vendored whole where possible; three sources are extracted subsets of larger upstream files (the import resolver, the path-alias resolver, the language lookup), written by `scripts/vendor/extract-functions.py` from the lists in `scripts/vendor/` |
| `grammars/<language>/` | Generated parser tables, one directory per language, each with its own licence |
| `vendored/<library>/` | Third-party libraries, each with its own licence |
| `api/` | The interface this project presents to the rest of the system, and the shim the resolver's sources call into. Ours, and held to warnings-as-errors with no exemption |
| `api/windows/` | For Microsoft's C runtime only: the POSIX names the vendored sources use, in a header included ahead of every source, and stand-ins for three headers that runtime lacks |
| `include/pdxe.h` | The public header, and the contract: read it before calling anything |
| `patches/` | Local changes, applied after copying. See `patches/README.md` |
| `tests/` | The interface's tests and their fixtures |

## The interface

Everything goes through `include/pdxe.h`. In outline:

- **Extraction.** `pdxe_extract_file` turns one file's bytes into a result the
  caller owns: definitions, calls, imports, usages and the rest, positioned in the
  file. The result says whether the walk stopped at the node budget (`truncated`) and
  how much work extraction lost (`extraction_lost`: failed allocations and exhausted
  budgets, the count the file's surface carries), so a cache never keeps a degraded
  extraction (issue 26). The C and C++ expression evaluator's budget is per file and
  grows with the source, 10,000 steps per started 32 KiB, from 10,000 up to 320,000
  (`patches/0012-c-lsp-eval-budget.patch`, issue 74): a large file gets the work it
  needs, every input still has a ceiling, and running it out is still lost work. A result rebuilt from a cache has neither. Each definition
  carries the bases it names, as the engine recorded them (issue 40), and the result
  the package or namespace the file declares, where the engine reads one (issue 41),
  and Rust's `impl Trait for Type` relations, empty blocks included (issue 42). Each
  definition also carries its decorators, declared parameter types and, for a Spring
  or `FastAPI` handler, its route; each call its arguments (issue 47) and, for a site
  positioned in the file, its node-type path from the innermost enclosing definition
  the result represents, which `patches/0010-site-node-type-paths.patch` records
  during the walk (issue 46). A nested callable the extractor does not represent owns
  no site: its sites belong to the nearest represented one around it, while it keeps
  its own lexical scope (`patches/0013-nested-modeled-call-ownership.patch`,
  issue 75). Which definition represents a callable is decided by its declaration
  occurrence (its buffer and the byte span of its function node), never by comparing
  the walk's name for it with the definition's
  (`patches/0014-exact-callable-owner-occurrence.patch`, issue 76). The occurrence is
  private to the engine and its translation: no part of the interface, a surface or a
  cache.
  Every route binding a definition's annotations or decorators declare crosses as a
  `pdxe_route`, with the declaring node's position and node-type path
  (`patches/0011-route-facts.patch`, issue 54).
  The bytes given are the whole input. C and C++ preprocessing expands the macros and
  conditionals the source defines itself and opens no file: an `#include` target, or a
  file `__has_include` names, is never found, wherever the process runs, and the
  directive stays an import (`patches/0015-source-only-preprocessing.patch`, issue 83).
  Nor does it read a clock: `__DATE__` and `__TIME__` are `"Jan 01 1970"` and
  `"00:00:00"` in any time zone and locale (`patches/0016-fixed-date-time-macros.patch`,
  issue 84). Nor does the platform it was built for matter: `sizeof` in `#if` has one
  fixed LP64 data model (`patches/0017-fixed-preprocessor-sizeof-model.patch`, issue 85),
  a one-byte character constant is a signed 8-bit char and a `#line` number is read at
  a fixed width and capped at 4294967295
  (`patches/0018-host-independent-literals-and-line.patch`, issue 86). Nor does the
  machine's memory or speed: a forward jump of more than 4,096 lines is one `#line`
  marker, what one file's preprocessing writes and generates is held to ceilings counted
  from its length, passing one discards the expansion and is lost work, and `#if`
  arithmetic is 64-bit two's complement, written out
  (`patches/0019-bounded-deterministic-preprocessing.patch`, issue 87). A site's
  node-type path is complete or absent, at most 1,024 entries, and one file's paths at
  most 500,000 entries together, the first refusal lost work
  (`patches/0020-bounded-site-node-type-paths.patch`, issue 88). A tree nested deeper
  than 512 is not extracted at all, the scan running before any semantic pass, and is
  lost work; the expanded tree of C and C++ is held to the same depth
  (`patches/0021-bounded-syntax-depth.patch`, issue 89). Within that depth, a usage's
  questions about its ancestors are answered from the walk's own path, each answer
  computed once a node, so that work is linear in the nesting
  (`patches/0022-linear-admitted-depth-extraction.patch`, issue 90). Before any of it,
  the parse itself is safe: each grammar scanner keeps its state within the parser's
  1,024-byte buffer, refusing a construct that would need more, and none ends the
  process (`patches/0023-bounded-external-scanner-state.patch`, issue 91). Text a fact
  takes from the file crosses as a `pdxe_text`: the byte range of the source the caller
  gave when the text is exactly a run of it, owned text when the engine assembled it or
  read it after C-family macro expansion; a definition's parameter list is one. A
  definition's qualified name crosses by its relation to an earlier definition's
  (`qn_form`, `qn_base`, `qn_text`, and `qn_keep` for a prefix of an earlier name, as
  C++ functions in nested namespaces share), spelled out with `pdxe_qualified_name`;
  the surface holds ranges, definition references and structural names the same way,
  and is bound to the file's source when a project runs (surface version 5). Argument
  expressions and parameter lists are ranges inside the engine too, never copied
  (`patches/0024-source-backed-extraction-text.patch`, issue 90). Typed resolution and
  the interface's translation look things up by index, never by scanning a file's or a
  registry's definitions, imports, usages or sites per question: each index keeps its
  entries in the scan's order, so the answers are the scans' and the work is linear in
  the file, and an index that cannot get memory finds nothing rather than scan
  (`patches/0025-indexed-typed-resolution.patch`, issue 92).
- **Typed resolution across files.** A project collects files
  (`pdxe_resolve_project_add_file`), optionally the repository's metadata
  (`pdxe_resolve_project_set_metadata`), runs once, and reports one resolution per
  call site the typed pass answers.
- **Repository metadata.** Packages, path aliases and the root crate manifest,
  already found and parsed by the caller. The engine resolves imports; it never
  reads a manifest or the filesystem.
- **Run health.** `pdxe_resolve_project_health` says whether a completed run did
  all its work, and counts every file by what typed resolution did with it, so that a
  run that lost work is never read as one with fewer answers. What a run loses (failed
  allocations, exhausted work budgets) is counted where it is lost, on the run's
  thread (`api/lost_work.h`), and kept in the run's record.
- **Caching.** `pdxe_result_build` rebuilds a result from the parts a cache keeps,
  and `pdxe_surface_export` / `pdxe_surface_import` carry everything the resolver
  reads from a file, so an unchanged file is never extracted again and resolves
  exactly as it would fresh.

Nothing in the interface writes to standard output or standard error. The engine's
own debugging prints stay behind their switches, which are off unless a debugging
environment variable turns them on.

## Tests

| Test | Proves |
|---|---|
| `abi_smoke` | Every language of the matrix extracts, parses and yields definitions of its own |
| `abi_inputs_are_copied` | A result keeps nothing of the caller's: extracted from a path and source freed as soon as extraction returns, it equals one extracted from buffers kept alive |
| `corpus_extracts` | Every file of the sanitizer corpus (`bench/corpus`) extracts with the status its name declares; every language is covered, nothing in the corpus is of no known language, and it holds at most 200 files |
| `abi_no_output` | Extraction, resolution and the cache path write nothing to either output stream, with the engine's log level raised as far as it goes, over the fixtures and every language of the corpus; the corpus's TypeScript resolves clean |
| `abi_no_output_ts_budget` | The same with the TypeScript resolver's work budget starved: every TypeScript run of the corpus runs it out, is degraded for it, and still writes nothing (issue 28); the files' results report the work extraction lost in `extraction_lost`, which the run counts, and a rebuilt result reports none; unstarved, no result reports any |
| `abi_throws`, `abi_truncated` | Throws cross the interface with their exception and definition; a walk cut short by the node budget says so; a rebuilt result carries no throws, truncation or lost work |
| `abi_run_health_*` | Whether a run did all its work: clean with no answers to find, and with definitions typed resolution keeps none of; degraded by a skipped file or a lost answer, even with no answers, keeping the answers it found |
| `abi_result_build_roundtrip` | A rebuilt result equals the original field by field, base classes, `impl Trait for Type` relations, node-type paths, call arguments, decorators, parameter types, routes and route facts included and copied (rebuilt from copies that are then overwritten and freed); definitions with no base, one and several, files with no relation, one and several, each kind of derivation fact, and definitions with one route fact and with several all occur (`fixtures/bases`, `fixtures/facts`), and every call and route fact positioned in the file has a path; a surface decodes and re-encodes to the same bytes; malformed surfaces, bases counted without an array or with a NULL, relations counted without an array or with a NULL string, decorators counted without an array, parameter types holding a NULL, a node-type path counted without an array or holding a NULL, arguments counted without an array or one without its text, and route facts counted without an array, missing a method, or with a path counted without an array or holding a NULL, are refused; resolving a project leaves every file's surface as it was |
| `owner_occurrence` | Sites are joined to their owners by declaration occurrence (issue 76): the index finds a definition by its exact occurrence, buffer included, so a raw and a macro-expanded occurrence at the same numeric span are two; two definitions with one occurrence are ambiguous, never the first; a scan answers as the table does. Over the corpus, no two callables share an occurrence, no site inside a callable is the file's, and no site is owned by a raw callable that does not hold it |
| `c_eval_budget` | The C and C++ expression evaluator's budget for a source's length: zero, negative and the largest int, the boundaries of a tranche and of the cap, and every length to past the cap against a 64-bit reference; a generated source four tranches long, taking about twice the old fixed 10,000 steps, extracts losing nothing, and one under a tranche that runs its budget out still reports lost work (issue 74) |
| `owner_census` (a diagnostic, not a test) | Counts a list of files' sites by owner from the engine's own occurrences: `scripts/owner-census.py <segment> <checkout>` runs it over a segment's files; `orphan` must be 0 |
| `differential_dump` (the engine differential's vendored side, not a test) | Prints every row of the engine differential for a list of files: each file's extraction once the cross-file typed pass has written into it, and each file's cross-file surface from a second run in which every file comes from its exported surface; refuses a run that is not clean (P2-16, below) |
| `typed_lookup` | Typed resolution and the interface's translation look up by index (issue 92): every resolve fixture, duplicate, overload, shadowing, alias and ambiguity cases in eleven languages, generated code and the issue's shapes extract and resolve to the same facts and answers with the indexes as with the scans (`PDX_ENGINE_TEST_LINEAR_LOOKUP_REFERENCE`); counted work doubles with the file for 1,000 to 16,000 flat classes in nine languages, 64 to 512 nested JavaScript groups, 16,000 definitions translated and generated code, never examining a registry entry by entry, while the scans' work grows with its square; a duplicated qualified name scopes to its first definition, found without the project's prefix; every family of indexes refused its memory (`PDX_ENGINE_TEST_REFUSE_LOOKUP_INDEXES`) completes or fails as the interface says, as lost work, the work still linear |
| `api_warnings_are_errors` | Each warning class exempt in vendored code is still an error in the interface layer, compiled with the build's own flags |
| one per `tests/fixtures/resolve/<name>` | Typed resolution on that fixture matches the reference engine's recorded answer, the fixture's own assertions hold, and neither the order files are added in nor resolving them from a cache changes the answer |

The Rust tests in `crates/pdx-bench/tests/` check the archive: no symbol carries the
upstream prefix, each is defined once, the whole archive links against the system
libraries alone, every object is reachable from the interface, and the surface codec
covers every field the engine records.

### The reference differential

Each resolution fixture's `expected.tsv` is the reference engine's answer for it,
recorded by `make engine-typed-reference`: its typed edges, which ours must equal,
and its textual edges, which this project's own stages must reproduce and which the
fixtures' assertions read. That builds the pinned, unmodified
reference outside the tree (`scripts/engine/reference-build.sh`), indexes the fixture
with it, and reads the typed edges from the graph it builds. The recorded files let
the comparison run anywhere without the reference; re-recording is how a change on
either side is caught. The details are in `scripts/engine/typed-differential.py`.

### The engine differential (P2-16)

`make engine-differential`, nightly only (`check-full`), compares the whole of what
the engine produces with the pinned, unmodified reference engine, on the same inputs:
every corpus of `bench/engine-differential.toml` (every resolution fixture, at least
one per typed-tier language, and a bounded subset of every typed-tier golden), row for
row. The rows (`tests/differential_rows.h`, decision 50) are each file's definitions,
call sites and callable references, imports and typed answers, as the cross-file typed
pass leaves them, and each file's cross-file surface: the definitions both engines'
surface codec (`src/resolve/lsp_surface.c`) writes. Only fields both engines' structures
have are read. The vendored side is `tests/differential_dump`, which builds its surfaces
through `pdxe_project_debug_lsp_surfaces` (internal, `api/shim_internal.h`, tests only)
after a run whose files all came from their exported surfaces. The reference side is a
build of its pinned checkout outside the tree with an observation hook linked in
(`scripts/engine/differential_hook.c`, translated to the reference's names by the rename
map, inverted), and the surfaces it stores. `scripts/engine/differential.py` stages
each corpus, runs both, canonicalises the rows (the project name wrapping qualified
names becomes `@`, and each engine label gets the kind `api/pdxe.c` maps it to) and
fails on any difference; nothing can be accepted.

The differential compares the explicit engine contract; files around an engine are no
input. Each engine runs, for each corpus, from a fresh empty working directory of the
harness's own, outside the stage, the corpus and the repository, and is given every
input by absolute path (decision 52). P2-16 ran both in the corpus root, which is how
issue 83 was found: both engines' C and C++ preprocessing read the files a source
includes relative to the working directory. Normal CI runs the comparison's own tests
(`scripts/engine/test_differential.py`, and `engine_differential_identical` in
`crates/pdx-bench`), never the reference.

## For the Rust stages

The interface reports what typed resolution decided, and stops where the reference's
calls and usages passes stop using typed answers and start guessing. Porting those
passes (P2) means reproducing what they do next. From the reference:

- **A call with a typed answer** is drawn to that target. A call without one goes to
  the textual stages, unless it is `typed_only`, in which case it is dropped: it has
  no text to match.
- **The weak-call guards.** A textual match by a short name alone (`suffix_match`,
  `unique_name`, `field_type_hint`, `fuzzy`) is not drawn as a call when:
  the call has `PDXE_LEX_UNRESOLVED_MEMBER` in Python, JavaScript or TypeScript,
  unless it is Python, has `PDXE_LEX_SELF_ROOTED`, matched by `unique_name`, and its
  name is not a method of a builtin type (the reference's list, which the port takes
  over); or the call has `PDXE_LEX_LOCALLY_BOUND` (Python). In Perl, a call with
  `PDXE_LEX_UNRESOLVED_MEMBER` or to a builtin keeps only same-module and import
  matches. The fixtures `*_unresolved_member_call`, `python_locally_bound_call` and
  `python_self_rooted_member_call` hold the reference's answers for each case.
- **A reference** (`is_reference`) with a typed answer is a call reference when its
  target is callable, meaning a definition whose engine kind is `Function`, `Method`,
  `Constructor` or `Class`, and a plain use otherwise. A typed answer whose target is
  outside the project settles the site: no edge, and no guess. Without a typed answer
  the reference resolves a reference by name as a plain use, never as a call
  reference; for Java and Kotlin, when the reference is spelled as one
  (`PDXE_LEX_EXPLICIT_REFERENCE`, and for Kotlin any possible reference), it refuses
  its two project-wide guesses there (`unique_name`, `suffix_match`) unless a Kotlin
  import names it.
- **Resolving any use by name** respects the lexical bits: `PDXE_LEX_BLOCKED_LOCALLY`
  means no target at all, `PDXE_LEX_BLOCKED` means no callable target, and in Go a
  bare name (`PDXE_LEX_MEMBER_ACCESS` clear) is never a struct field. A use is never
  resolved by name to a definition in another language family.
- **No edge from a definition to itself** is ever drawn, by either pass.
- **A call or reference at file scope** is attributed to the file. In a language
  whose modules are directories (Go, Java), a class-level site names the directory's
  module, and the reference attributes it to the file instead.
- **Files are processed in path order.** The answers here do not depend on the order
  files are added, but the order of results does.

## Unresolved symbols

The archive leaves nothing undefined that the system's C, mathematics, thread and C++
runtime libraries do not supply, and defines nothing twice; the build tests link the
whole archive to prove it. The lookups the resolution sources expect from a graph
store and pipeline this project does not vendor are defined in `api/`.

`nm -u` on the archive lists far more than that, because it reports every object's
unresolved references, including those satisfied by sibling objects. What matters is
the set nothing in the archive defines.

## Refreshing

`make vendor-refresh` re-runs the whole sequence at the pinned commit: fetch, copy,
rename, strip, patch. At the same commit it must reproduce the committed tree
exactly, which `make vendor-verify` checks; it needs the pinned checkout, so it runs
where that is available rather than in continuous integration. Moving to a newer
commit is a deliberate act: the pin changes in the reference lock, and any patch that
no longer applies stops the refresh rather than being skipped.
