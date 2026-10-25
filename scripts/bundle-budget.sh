#!/usr/bin/env bash
# The interface bundle has a hard size budget, measured gzipped.
set -euo pipefail

BUDGET_BYTES=$((2500 * 1024))
DIST="ui/dist"

# `make bundle-budget` builds the interface first; a missing build is a failure, never
# a pass that measured nothing.
if [ ! -d "$DIST" ]; then
  echo "error: no interface build at $DIST (run make ui-build)" >&2
  exit 1
fi

# Every file the build emits counts: scripts, styles, pages and every asset (fonts,
# images, wasm), each as it would be served, gzipped.
total=0
while IFS= read -r -d '' f; do
  total=$((total + $(gzip -c "$f" | wc -c)))
done < <(find "$DIST" -type f -print0)

printf 'bundle budget: %d bytes gzipped of %d allowed\n' "$total" "$BUDGET_BYTES"
[ "$total" -le "$BUDGET_BYTES" ] || { echo "error: bundle over budget" >&2; exit 1; }
