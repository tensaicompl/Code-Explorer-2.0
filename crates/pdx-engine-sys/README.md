# pdx-engine-sys

Raw bindings to the extraction engine, and its build.

Open. Apache-2.0; see `LICENSE` at the repository root.

## Public API

Everything in `engine/include/pdxe.h`, as bindgen reads it: the functions, the
structures, and the constants, each carrying the header's documentation. Nothing is
translated or made safe; that is `pdx-engine`'s job.

The bindings are committed in `src/bindings.rs`, so building this crate needs no
libclang. After changing the header, regenerate them:

```
make bindgen
```

Any other build with the `regenerate-bindings` feature (for example the linter,
which enables every feature) compares the regenerated bindings with the committed
ones and fails if they differ.

## Build

`build.rs` builds the engine with the same `CMake` project `make engine` uses and
links it statically, with the C++ runtime its macro preprocessor needs. On Windows
the engine is compiled by clang for the MSVC ABI, through Ninja; Microsoft's
compiler is not supported for it.

## Tests

```
cargo test -p pdx-engine-sys
```

`abi_smoke` runs the C test of the same name through the bindings, over every
language of the matrix. `typed_resolution_crosses_the_boundary` resolves a call
across two files through the project interface.
