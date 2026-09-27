#!/usr/bin/env bash
# The interface bundle has a hard size budget, measured gzipped.
set -euo pipefail

BUDGET_BYTES=$((2500 * 1024))
DIST="ui/dist"

if [ ! -d "$DIST" ]; then
  echo "bundle budget: no build present, skipping (run make ui-build)"
  exit 0
fi

total=0
while IFS= read -r -d '' f; do
  total=$((total + $(gzip -c "$f" | wc -c)))
done < <(find "$DIST" -type f \( -name '*.js' -o -name '*.css' -o -name '*.html' \) -print0)

printf 'bundle budget: %d bytes gzipped of %d allowed\n' "$total" "$BUDGET_BYTES"
[ "$total" -le "$BUDGET_BYTES" ] || { echo "error: bundle over budget" >&2; exit 1; }
