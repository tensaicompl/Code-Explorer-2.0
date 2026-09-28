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

# 1. Rust dependencies.
if command -v cargo-deny >/dev/null 2>&1; then
  if ! cargo deny --log-level error check licenses 2>&1 | sed 's/^/    /'; then
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
  licences_of() {
    (cd ui && pnpm licenses list "$@" --json 2>/dev/null) | python3 -c 'import json,sys
try:
    d = json.load(sys.stdin)
except Exception:
    sys.exit(0)
if isinstance(d, dict):
    for licence, pkgs in sorted(d.items()):
        names = ",".join(sorted({p.get("name", "?") for p in pkgs}))
        print(f"{licence}\t{names}")'
  }

  while IFS=$'\t' read -r id names; do
    [ -n "$id" ] || continue
    allowed "$id" || report "shipped interface dependency licence not allowed: $id ($names)"
  done < <(licences_of --prod)

  while IFS=$'\t' read -r id names; do
    [ -n "$id" ] || continue
    allowed "$id" || echo "licence scan: note, build-only dependency outside the allow list: $id ($names)"
  done < <(licences_of)

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
  if ! grep -q "$dir" THIRD_PARTY_NOTICES.md 2>/dev/null; then
    report "vendored directory absent from the notices file: $dir"
  fi
  # The licence recorded for it in the notices must be one we allow. Vendored code
  # under anything else may not be in the tree at all, whatever the notices say.
  recorded="$(grep -A6 -F "\`${dir}/\`" THIRD_PARTY_NOTICES.md 2>/dev/null \
              | grep -m1 -oE '\*\*Licence\*\*: .*' | sed 's/^\*\*Licence\*\*: //')"
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
