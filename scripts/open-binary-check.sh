#!/usr/bin/env bash
# The public binary must not link an enterprise crate. Fails if it does.
set -euo pipefail

ENTERPRISE=(pdx-arch pdx-analytics pdx-server)
tree="$(cargo tree -p pdx --no-default-features --prefix none --no-dedupe 2>/dev/null)"

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
echo "open binary: no enterprise crate linked"
