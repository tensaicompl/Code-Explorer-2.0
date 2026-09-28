# engine

The extraction and typed-resolution engine, built as the static library `libpdxe.a`.

This is third-party source under our maintenance, not code developed here. New
capability belongs in Rust. Changes to these sources are limited to four things:
what the vendoring scripts rename, what the strip list removes, the interface layer
in `api/`, and the patches in `patches/`. Editing a vendored file in place is how a
refresh silently loses a fix.

## Building

```
cmake -S engine -B build
cmake --build build -j
```

Requires CMake 3.25 or later and a C17 and C++17 compiler. Built and tested with
gcc and clang. One translation unit is C++ (macro preprocessing); everything else
is C.

The grammars are not compiled directly. Each language has a wrapper in `src/` that
includes its parser and scanner, which is how the generated tables are kept apart:
they declare static symbols of the same names, so two in one translation unit would
collide.

### Options

| Option | Default | Effect |
|---|---|---|
| `PDXE_VENDORED_WERROR` | `OFF` | Treat warnings in the vendored sources as errors. Off because those sources are not ours to reshape, and a compiler upgrade should not be able to stop a build of code nobody here edits. |

## Layout

| Path | Contents |
|---|---|
| `src/` | Extraction, the typed resolution layer under `lsp/`, the foundation under `foundation/`, and the cross-file resolution driver under `resolve/` |
| `grammars/<language>/` | Generated parser tables, one directory per language, each with its own licence |
| `vendored/<library>/` | Third-party libraries, each with its own licence |
| `api/` | The interface this project presents to the rest of the system. Ours, and held to warnings-as-errors |
| `include/` | The public header |
| `patches/` | Local changes, applied after copying. See `patches/README.md` |

## Unresolved symbols

The archive deliberately leaves some symbols undefined, for the interface layer to
supply. Two kinds:

- Lookups the cross-file resolution sources expect from a graph store this project
  does not vendor, served instead from our own registry.
- A handful of helpers whose implementations reach for the operating system, or
  belong to subsystems computed outside this engine.

`nm -u` on the archive lists a great many more than that, because it reports every
object's unresolved references including those satisfied by sibling objects. What
matters is the set that nothing in the archive defines.

## Refreshing

`make vendor-refresh` re-runs the whole sequence at the pinned commit: fetch, copy,
rename, strip, patch. It must produce an unchanged tree at the same commit, which a
continuous integration job checks. Moving to a newer commit is a deliberate act: the
pin changes in the reference lock, and any patch that no longer applies stops the
refresh rather than being skipped.
