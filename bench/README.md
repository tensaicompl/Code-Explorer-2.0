# bench

The benchmark harness's data: pinned repositories, committed corpus, expected graphs, reviewed samples and results.

Contents arrive with the task that produces them.

## corpus/

The sanitizer corpus: small source files that `make check-asan` extracts with the
engine built under the address, undefined-behaviour and leak sanitizers, through the
engine test `corpus_extracts`, and that `abi_no_output` resolves with nothing written
to either output stream. The nightly build runs both.

Every file is written for this project. Nothing is copied from the golden or scale
repositories, whose sources are never redistributed, or from anywhere else.

The layout is the contract, and the test enforces it:

- Each top-level directory is named after an engine language ID, and every file
  beneath it is a source in that language. A file at the top level, a directory named
  after no known language, or a hidden entry fails the run; nothing is skipped.
- Every language the engine's tests cover has at least one file: the language
  matrix's 31 (`pdx_core::languages`) and the engine's `tsx` grammar, as listed in
  `engine/tests/matrix_languages.h`, which a Rust test keeps equal to the matrix.
- The corpus holds at most 200 files. It is meant to stay in the tens: one file per
  shape worth exercising, not size for its own sake.
- A file's name declares the status extraction must give it: `recovery_*` is source a
  parser has to recover from and may come back parsed or partial; `failed_*` is source
  the engine refuses by design and must fail with a diagnostic; any other file must
  parse.

The files are meant to reach the code that tends to hide memory and undefined-behaviour
defects: nesting, generics and templates, macros and the preprocessor, annotations and
decorators, escaped, multi-line and non-ASCII strings, inheritance, and source a parser
has to recover from.
