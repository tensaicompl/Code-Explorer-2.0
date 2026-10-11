#!/usr/bin/env bash
# The public binary must not link an enterprise crate, nor the benchmark harness or
# the crates only it and the benchmarks use. Fails if it does.
set -euo pipefail

ENTERPRISE=(pdx-arch pdx-analytics pdx-server)
# `pdx bench` runs the harness as a separate program beside pdx (P2-14): pdx never
# links it, its resident-memory sampler or the benchmark framework.
BENCH_ONLY=(pdx-bench sysinfo criterion)
tree="$(cargo tree -p pdx --no-default-features --prefix none --no-dedupe 2>/dev/null)"
product="$(cargo tree -p pdx --edges normal,build --prefix none --no-dedupe 2>/dev/null)"

found=0
for crate in "${ENTERPRISE[@]}"; do
  if grep -qE "^${crate} v" <<<"$tree"; then
    echo "error: the public pdx binary links the enterprise crate ${crate}" >&2
    found=1
  fi
done

if [ "$found" -ne 0 ]; then
  echo "the licence split forbids this; move the behaviour behind the enterprise feature" >&2
  exit 1
fi

for crate in "${BENCH_ONLY[@]}"; do
  if grep -qE "^${crate} v" <<<"$product"; then
    echo "error: the pdx binary links ${crate}, which belongs to the benchmark harness" >&2
    found=1
  fi
done

if [ "$found" -ne 0 ]; then
  echo "pdx bench launches pdx-bench as a separate program; keep its crates out of pdx" >&2
  exit 1
fi
echo "open binary: no enterprise or benchmark crate linked"
