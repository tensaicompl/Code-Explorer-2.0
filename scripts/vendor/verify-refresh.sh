#!/usr/bin/env bash
# Re-runs the vendoring at the pinned commit and fails unless the tree comes out
# exactly as committed: the vendored engine is what the scripts produce from the
# pinned reference, and nothing else.
#
# Needs the pinned checkout (scripts/vendor/fetch-engine.sh) and a clean engine/: it
# rewrites the vendored parts in place, and a change it makes is the finding.
set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"
[ -z "$(git status --porcelain -- engine)" ] ||
  die "engine/ has uncommitted changes; commit or stash them first, so the check reads only its own"

"$REPO_ROOT/scripts/vendor/copy-engine.sh" --force >/dev/null || die "copy failed"
"$REPO_ROOT/scripts/vendor/rename-engine.sh" >/dev/null || die "rename failed"
"$REPO_ROOT/scripts/vendor/strip-engine.sh" >/dev/null || die "strip failed"
"$REPO_ROOT/scripts/vendor/apply-patches.sh" >/dev/null || die "patches failed"

changes="$(git status --porcelain -- engine)"
if [ -n "$changes" ]; then
  echo "the refresh does not reproduce the committed engine:" >&2
  echo "$changes" | head -40 >&2
  exit 1
fi
note "the refresh reproduces the committed engine exactly"
