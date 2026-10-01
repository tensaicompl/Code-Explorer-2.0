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
make asan            # the same tests under the address, undefined-behaviour and leak sanitizers
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
  file.
- **Typed resolution across files.** A project collects files
  (`pdxe_resolve_project_add_file`), optionally the repository's metadata
  (`pdxe_resolve_project_set_metadata`), runs once, and reports one resolution per
  call site the typed pass answers.
- **Repository metadata.** Packages, path aliases and the root crate manifest,
  already found and parsed by the caller. The engine resolves imports; it never
  reads a manifest or the filesystem.
- **Run health.** `pdxe_resolve_project_health` says whether a completed run did
  all its work, and counts every file by what typed resolution did with it, so that a
  run that lost work is never read as one with fewer answers.
- **Caching.** `pdxe_result_build` rebuilds a result from the parts a cache keeps,
  and `pdxe_surface_export` / `pdxe_surface_import` carry everything the resolver
  reads from a file, so an unchanged file is never extracted again and resolves
  exactly as it would fresh.

Nothing in the interface writes to standard output or standard error.

## Tests

| Test | Proves |
|---|---|
| `abi_smoke` | Every language of the matrix extracts, parses and yields definitions of its own |
| `abi_no_output` | Extraction, resolution and the cache path write nothing to either output stream, with the engine's log level raised as far as it goes |
| `abi_result_build_roundtrip` | A rebuilt result equals the original field by field; a surface decodes and re-encodes to the same bytes; malformed surfaces are refused; resolving a project leaves every file's surface as it was |
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
