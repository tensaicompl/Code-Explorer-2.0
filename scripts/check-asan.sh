#!/usr/bin/env bash
# Builds the engine with the address, undefined-behaviour and leak sanitizers and runs
# its tests under them. Any report fails the run: no sanitizer option relaxes a check.
#
# This covers the engine's own tests. The run over a corpus of real repositories,
# which the first engine gate also asks for, joins this script with the task that
# brings the corpus.
set -euo pipefail
cd "$(dirname "$0")/.."

build="${PDX_ASAN_BUILD:-target/engine-asan}"
if command -v clang >/dev/null 2>&1; then
  export CC=clang CXX=clang++
fi
flags="-fsanitize=address,undefined -fno-omit-frame-pointer -fno-sanitize-recover=all"

cmake -S engine -B "$build" -DCMAKE_BUILD_TYPE=Debug -DPDXE_BUILD_TESTS=ON -DPDXE_TEST_SEAMS=ON \
  -DCMAKE_C_FLAGS="$flags" -DCMAKE_CXX_FLAGS="$flags" \
  -DCMAKE_EXE_LINKER_FLAGS="-fsanitize=address,undefined" > /dev/null
cmake --build "$build" -j
ASAN_OPTIONS="detect_leaks=1:halt_on_error=1:abort_on_error=0" \
UBSAN_OPTIONS="print_stacktrace=1:halt_on_error=1" \
  ctest --test-dir "$build" --output-on-failure
