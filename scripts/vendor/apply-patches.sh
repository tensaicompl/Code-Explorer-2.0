#!/usr/bin/env bash
# Applies the local patches to the vendored engine, in order.
#
# Changes to vendored code live here as patches rather than as edits in place, so a
# refresh at a newer commit re-applies them and says plainly which ones no longer
# apply. A patch that fails is a stop, not a warning: silently skipping one leaves
# the engine in a state nobody described.
#
# The patches are a series: each is written against the tree the ones before it
# leave, and a later one may change lines an earlier one wrote (0014 supersedes part
# of 0013's implementation). Such an earlier patch, checked on its own against the
# patched tree, neither applies nor reverses, so what the tree already carries is
# found as a prefix of the series: the longest one that reverses cleanly, last patch
# first, on a scratch copy of the files the series touches. The rest is then applied
# in order.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

ENGINE="$REPO_ROOT/engine"
PATCH_DIR="$ENGINE/patches"
[ -d "$ENGINE" ] || die "engine/ is absent; run scripts/vendor/copy-engine.sh first"

if [ ! -d "$PATCH_DIR" ]; then
  note "no patch directory: nothing to apply"
  exit 0
fi

patches=()
for patch in "$PATCH_DIR"/*.patch; do
  [ -f "$patch" ] && patches+=("$patch")
done
if [ "${#patches[@]}" -eq 0 ]; then
  note "no patches needed applying"
  exit 0
fi

# Whether the tree carries exactly the first $1 patches' changes to the lines they
# touch: they reverse cleanly, last first, on a scratch copy.
carries_prefix() {
  local count="$1" scratch file i ok=0
  scratch="$(mktemp -d)" || die "no scratch directory"
  while IFS= read -r file; do
    if [ ! -f "$REPO_ROOT/$file" ]; then
      ok=1
      break
    fi
    mkdir -p "$scratch/$(dirname "$file")"
    cp "$REPO_ROOT/$file" "$scratch/$file"
  done < <(sed -n 's|^+++ b/||p' "${patches[@]:0:count}" | sort -u)
  for ((i = count - 1; i >= 0 && ok == 0; i--)); do
    (cd "$scratch" && git apply --reverse "${patches[$i]}") 2>/dev/null || ok=1
  done
  rm -rf "$scratch"
  return "$ok"
}

carried=0
for ((k = ${#patches[@]}; k > 0; k--)); do
  if carries_prefix "$k"; then
    carried="$k"
    break
  fi
done
for ((i = 0; i < carried; i++)); do
  note "already applied: $(basename "${patches[$i]}")"
done

applied=0
for ((i = carried; i < ${#patches[@]}; i++)); do
  patch="${patches[$i]}"
  name="$(basename "$patch")"
  git -C "$REPO_ROOT" apply --check "$patch" 2>/dev/null ||
    die "$name does not apply. The reference has moved under it: rebuild the patch against the pinned commit."
  git -C "$REPO_ROOT" apply "$patch" || die "$name failed to apply after passing its own check"
  note "applied $name"
  applied=$((applied + 1))
done

[ "$applied" -gt 0 ] || note "no patches needed applying"
