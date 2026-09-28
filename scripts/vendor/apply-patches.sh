#!/usr/bin/env bash
# Applies the local patches to the vendored engine, in order.
#
# Changes to vendored code live here as patches rather than as edits in place, so a
# refresh at a newer commit re-applies them and says plainly which ones no longer
# apply. A patch that fails is a stop, not a warning: silently skipping one leaves
# the engine in a state nobody described.
set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

ENGINE="$REPO_ROOT/engine"
PATCH_DIR="$ENGINE/patches"
[ -d "$ENGINE" ] || die "engine/ is absent; run scripts/vendor/copy-engine.sh first"

if [ ! -d "$PATCH_DIR" ]; then
  note "no patch directory: nothing to apply"
  exit 0
fi

applied=0
for patch in "$PATCH_DIR"/*.patch; do
  [ -f "$patch" ] || continue
  name="$(basename "$patch")"
  if git -C "$REPO_ROOT" apply --check "$patch" 2>/dev/null; then
    git -C "$REPO_ROOT" apply "$patch" || die "$name failed to apply after passing its own check"
    note "applied $name"
    applied=$((applied + 1))
  elif git -C "$REPO_ROOT" apply --reverse --check "$patch" 2>/dev/null; then
    note "already applied: $name"
  else
    die "$name does not apply. The reference has moved under it: rebuild the patch against the pinned commit."
  fi
done

[ "$applied" -gt 0 ] || note "no patches needed applying"
