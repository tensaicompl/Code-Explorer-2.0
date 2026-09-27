#!/usr/bin/env bash
# The fast subset of the gate, run before a commit is written: formatting, the
# provenance rules and the licence policy. The slow parts (build, tests, the
# interface suites) belong to make check, which runs in continuous integration and
# before a task is merged.
#
# Install with: scripts/install-hooks.sh
set -uo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT" || exit 1

fail=0
step() {
  local name="$1"; shift
  if "$@" >/tmp/pdx-pre-commit.log 2>&1; then
    echo "  ok      $name"
  else
    echo "  FAILED  $name"
    sed 's/^/          /' /tmp/pdx-pre-commit.log
    fail=1
  fi
}

echo "pre-commit:"
if command -v cargo >/dev/null 2>&1; then
  step "formatting" cargo fmt --all -- --check
else
  echo "  skipped formatting: cargo not on PATH"
fi
step "provenance" ./scripts/provenance-scan.sh
step "licences" ./scripts/licence-scan.sh

if [ "$fail" -ne 0 ]; then
  echo "pre-commit: refused. Fix the above, or commit with --no-verify and record why." >&2
  exit 1
fi
echo "pre-commit: clean"
