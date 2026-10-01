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
links it statically, with the C++ runtime its macro preprocessor needs and, on
Windows, the system libraries its Windows code calls (`advapi32`, `bcrypt`); the
project names the same libraries for what it links itself.

The project owns every compiler flag, its warning policy included: the build gives
`CMake` empty base flags, because the cmake crate otherwise passes its own, and those
switch every warning off. The test `engine_flags` reads how the engine this crate
built was compiled and holds it to the policy on every system.

On Windows the engine is compiled by clang for the MSVC ABI, with the target stated,
through Ninja; Microsoft's compiler is not supported for it. The engine's path is
built from its parts rather than asked of the filesystem, because Windows answers
with a verbatim path under which the engine's relative includes do not resolve; the
build refuses one.

The crate builds from the repository, not from a packaged copy of itself (issue 23).

## Test switches

The `test-seams` feature compiles the engine's fault-injection switches, with which a
test makes the engine abort on a file it names or skip one in typed resolution. They
read the environment, so a binary built with them can be crashed by an environment
variable. Enable the feature from `[dev-dependencies]` only, as `pdx-engine` and `pdx`
do; a build without dev-dependencies, which is how a release is built, never has it.
`scripts/no-test-switches.sh <binary>` checks a binary for the switches' names.

## Tests

```
cargo test -p pdx-engine-sys
```

`abi_smoke` runs the C test of the same name through the bindings, over every
language of the matrix. `typed_resolution_crosses_the_boundary` resolves a call
across two files through the project interface. `engine_flags` checks how the
engine was compiled, as above, and that it has the test switches exactly when the
`test-seams` feature asks for them.
