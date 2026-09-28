#!/usr/bin/env bash
# Removes copied engine sources that nothing needs.
#
# The copy step already leaves out whole subsystems. This step removes what survives
# copying but turns out to have no remaining caller once those are gone. That cannot
# be decided by reading: it is decided by the linker, so the list is built from a
# trial build's undefined symbols and committed, and this script applies it.
#
#   strip-engine.sh            apply the committed list
#   strip-engine.sh --report   list copied files nothing in the tree references
#
# The report is advice, not an instruction: a file referenced only from a header
# macro, or only under a platform the report did not build for, will look unused.
# Deletions are reviewed and added to the list by hand.
set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

ENGINE="$REPO_ROOT/engine"
LIST="$REPO_ROOT/scripts/vendor/strip-list.txt"
[ -d "$ENGINE" ] || die "engine/ is absent; run scripts/vendor/copy-engine.sh first"

if [ "${1:-}" = "--report" ]; then
  note "files whose symbols nothing else appears to reference:"
  found=0
  while IFS= read -r -d '' file; do
    case "$file" in */vendored/*|*/grammars/*) continue ;; esac
    base="$(basename "$file" .c)"
    # A crude proxy for the linker: does any other source mention this name?
    hits="$(grep -rlF "$base" "$ENGINE/src" 2>/dev/null | grep -vF "$file" | wc -l)"
    if [ "$hits" -eq 0 ]; then
      echo "  ${file#$REPO_ROOT/}"
      found=$((found + 1))
    fi
  done < <(find "$ENGINE/src" -name '*.c' -print0)
  note "$found candidate(s). Review before adding any to ${LIST#$REPO_ROOT/}."
  exit 0
fi

if [ ! -r "$LIST" ] || [ -z "$(grep -vE '^\s*(#|$)' "$LIST")" ]; then
  note "no strip list yet: nothing removed"
  note "the list is built once the engine builds and its undefined symbols can be read"
  exit 0
fi

removed=0
while IFS= read -r entry; do
  case "$entry" in ''|'#'*) continue ;; esac
  target="$REPO_ROOT/$entry"
  if [ -e "$target" ]; then
    rm -rf "$target"
    removed=$((removed + 1))
  fi
done < "$LIST"
note "removed $removed path(s) listed in ${LIST#$REPO_ROOT/}"
