#!/usr/bin/env bash
# Builds the engine with the address, undefined-behaviour and leak sanitizers and runs
# its tests under them: the interface's tests, and the extraction of every file in the
# sanitizer corpus (bench/corpus, through the test corpus_extracts). Any report fails
# the run: no sanitizer option relaxes a check.
#
# `make check-asan` runs this; `make check-full`, and so the nightly build, runs it too.
set -euo pipefail
cd "$(dirname "$0")/.."

build="${PDX_ASAN_BUILD:-target/engine-asan}"
if command -v clang >/dev/null 2>&1; then
  export CC=clang CXX=clang++
fi
flags="-fsanitize=address,undefined -fno-omit-frame-pointer -fno-sanitize-recover=all"

# The warning policy is stated, not inherited: CMake would keep an earlier configure's
# value for it in a reused build directory.
cmake -S engine -B "$build" -DCMAKE_BUILD_TYPE=Debug -DPDXE_BUILD_TESTS=ON -DPDXE_TEST_SEAMS=ON \
  -DPDXE_VENDORED_WERROR=ON \
  -DCMAKE_C_FLAGS="$flags" -DCMAKE_CXX_FLAGS="$flags" \
  -DCMAKE_EXE_LINKER_FLAGS="-fsanitize=address,undefined" > /dev/null
# Bounded like `make engine`: CMake's own CMAKE_BUILD_PARALLEL_LEVEL, else one job per
# online processor; `-j` alone is unbounded under the Makefile generator.
cmake --build "$build" --parallel "${CMAKE_BUILD_PARALLEL_LEVEL:-$(getconf _NPROCESSORS_ONLN)}"

# The corpus is the point of this run as much as the tests are: refuse to pass a suite
# that does not extract it.
if ! ctest --test-dir "$build" -N -R '^corpus_extracts$' | grep -q 'Total Tests: 1'; then
  echo "check-asan: the sanitizer corpus is not among the tests" >&2
  exit 1
fi

# Reports are written to files as well as failing the test: a test that captures its
# own output streams (abi_no_output) would otherwise swallow the report with them.
# Absolute: each test runs in a directory of its own.
mkdir -p "$build"
logs="$(cd "$build" && pwd)/sanitizer-reports"
rm -rf "$logs"
mkdir -p "$logs"
status=0
ASAN_OPTIONS="detect_leaks=1:halt_on_error=1:abort_on_error=0:log_path=$logs/asan" \
UBSAN_OPTIONS="print_stacktrace=1:halt_on_error=1:log_path=$logs/ubsan" \
  ctest --test-dir "$build" --output-on-failure || status=$?

reports=$(find "$logs" -type f | sort)
if [ -n "$reports" ]; then
  echo "check-asan: sanitizer reports:" >&2
  for report in $reports; do
    echo "--- $report" >&2
    cat "$report" >&2
  done
  exit 1
fi
echo "check-asan: no sanitizer reports"
exit "$status"
