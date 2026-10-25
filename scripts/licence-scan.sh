#!/usr/bin/env bash
# Fails when a dependency, a vendored directory or the notices file breaches the
# licence policy.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 2

ALLOW=(MIT ISC BSD-2-Clause BSD-3-Clause Apache-2.0 Zlib MPL-2.0 Unlicense CC0-1.0 0BSD)
status=0
report() { echo "licence scan: $*" >&2; status=1; }

allowed() {
  # A licence carrying an exception is judged on its base identifier.
  local base="${1%% WITH *}"
  local id
  for id in "${ALLOW[@]}"; do [ "$base" = "$id" ] && return 0; done
  return 1
}

# 1. Rust dependencies, by the pinned cargo-deny (the whole policy is `make deny`).
if command -v cargo-deny >/dev/null 2>&1; then
  if ! CARGO_DENY_LOG_LEVEL=error scripts/cargo-deny.sh check licenses 2>&1 | sed 's/^/    /'; then
    report "cargo deny rejected a dependency licence"
  else
    echo "licence scan: rust dependencies clean"
  fi
else
  report "cargo-deny is not installed; cannot check rust dependencies"
fi

# 2. Interface dependencies.
if [ -d ui/node_modules ]; then
  # The allow list governs what we distribute. The bundle contains production
  # dependencies only, so those are enforced; build tooling that never reaches a
  # user is listed instead, so an outlier is visible without failing the build.
  # One line per licence, "<id>\t<packages>". A pnpm that cannot be run, fails or
  # answers with something that is not its JSON is a failure: nothing was checked.
  licences_of() {
    local raw
    raw="$(cd ui && pnpm licenses list "$@" --json 2>/dev/null)" || return 1
    printf '%s' "$raw" | python3 -c 'import json,sys
d = json.load(sys.stdin)
if not isinstance(d, dict):
    sys.exit(1)
for licence, pkgs in sorted(d.items()):
    names = ",".join(sorted({p.get("name", "?") for p in pkgs}))
    print(f"{licence}\t{names}")'
  }

  if ! shipped="$(licences_of --prod)"; then
    report "pnpm could not list the shipped interface dependencies' licences"
  else
    while IFS=$'\t' read -r id names; do
      [ -n "$id" ] || continue
      allowed "$id" || report "shipped interface dependency licence not allowed: $id ($names)"
    done <<< "$shipped"
  fi
  if ! everything="$(licences_of)"; then
    report "pnpm could not list the interface dependencies' licences"
  else
    while IFS=$'\t' read -r id names; do
      [ -n "$id" ] || continue
      allowed "$id" || echo "licence scan: note, build-only dependency outside the allow list: $id ($names)"
    done <<< "$everything"
  fi

  echo "licence scan: interface dependencies checked"
else
  echo "licence scan: interface dependencies absent, skipped (run make ui-install)"
fi

# 3. Vendored directories: each carries its own licence file, and the notices
#    file names every one of them.
shopt -s nullglob
vendored=(engine engine/grammars/* engine/vendored/*)
found=0
for dir in "${vendored[@]}"; do
  [ -d "$dir" ] || continue
  found=1
  licence_file=("$dir"/LICENSE* "$dir"/COPYING*)
  found_licence=""
  for candidate in "${licence_file[@]}"; do
    [ -f "$candidate" ] && { found_licence="$candidate"; break; }
  done
  if [ -z "$found_licence" ]; then
    report "vendored directory without a licence file: $dir"
    continue
  fi
  # Named exactly: `engine/grammars/c/` is not satisfied by `c_sharp` or `cpp`.
  if ! grep -qF "\`${dir}/" THIRD_PARTY_NOTICES.md 2>/dev/null; then
    report "vendored directory absent from the notices file: $dir"
  fi
  # The licence recorded for it in the notices must be one we allow. Vendored code
  # under anything else may not be in the tree at all, whatever the notices say. A
  # grammar is a table row whose last cell is its licence file and whose second cell is
  # the licence; any other component has a heading and a **Licence** line.
  case "$dir" in
    engine/grammars/*)
      recorded="$(grep -F "\`${dir}/" THIRD_PARTY_NOTICES.md 2>/dev/null | head -1 \
                  | awk -F'|' '{gsub(/^[ \t]+|[ \t]+$/, "", $3); print $3}')"
      [ -n "$recorded" ] || report "vendored grammar without a recorded licence: $dir"
      ;;
    *)
      recorded="$(grep -A6 -F "\`${dir}/\`" THIRD_PARTY_NOTICES.md 2>/dev/null \
                  | grep -m1 -oE '\*\*Licence\*\*: .*' | sed 's/^\*\*Licence\*\*: //')"
      ;;
  esac
  if [ -n "$recorded" ]; then
    allowed "$recorded" || report "vendored directory under a licence we do not allow: $dir ($recorded)"
  fi
done
if [ "$found" -eq 0 ]; then
  echo "licence scan: no vendored directories yet, skipped"
fi

# 4. The notices file exists and says something.
if [ ! -s THIRD_PARTY_NOTICES.md ]; then
  report "THIRD_PARTY_NOTICES.md is missing or empty"
elif ! grep -q '^### ' THIRD_PARTY_NOTICES.md; then
  report "THIRD_PARTY_NOTICES.md lists no component"
fi

[ "$status" -eq 0 ] && echo "licence scan: clean"
exit "$status"
