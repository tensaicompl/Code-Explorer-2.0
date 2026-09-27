#!/usr/bin/env bash
# Fails when the tree names an upstream it must not name, carries a marker word
# where work should have been recorded as an issue, or contains a URL that is not
# on the allow list.
#
# Usage: provenance-scan.sh [root]   (default: the repository root)
set -uo pipefail

ROOT="${1:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
SCRIPTS="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

DENYLIST="${PDX_DENYLIST:-$SCRIPTS/provenance-denylist.txt}"
MARKERS="${PDX_MARKERS:-$SCRIPTS/marker-words.txt}"
URLS="${PDX_URL_ALLOWLIST:-$SCRIPTS/url-allowlist.txt}"

for f in "$DENYLIST" "$MARKERS" "$URLS"; do
  [ -r "$f" ] || { echo "provenance scan: cannot read $f" >&2; exit 2; }
done

# Reads a data file: strips comments and blank lines.
patterns_of() { grep -vE '^\s*(#|$)' "$1"; }

# Paths the scan does not read, and why.
#
#   THIRD_PARTY_NOTICES.md      the legal document; naming upstreams is its purpose
#   engine/**/LICENSE*          vendored licence texts, kept verbatim
#   bench/references.lock       the pinned reference list; naming them is its purpose
#   scripts/*-denylist.txt etc  this scanner's own inputs
#   legacy/                     third-party-derived code awaiting removal, already
#                               attributed in the notices file; the exclusion goes
#                               away when the retirement task deletes it
EXCLUDES=(
  ':(exclude)THIRD_PARTY_NOTICES.md'
  ':(exclude)engine/**/LICENSE*'
  ':(exclude)engine/grammars/**/LICENSE*'
  ':(exclude)bench/references.lock'
  ':(exclude)scripts/provenance-denylist.txt'
  ':(exclude)scripts/marker-words.txt'
  ':(exclude)scripts/url-allowlist.txt'
  ':(exclude)legacy/**'
)

cd "$ROOT" || exit 2

# The file set: inside a repository, tracked files plus new ones that are not
# ignored, so that a file escapes the scan only by being ignored, never by being
# unstaged. Outside a repository, every file, so a fixture directory works too.
if git -C "$ROOT" rev-parse --git-dir >/dev/null 2>&1; then
  mapfile -d '' FILES < <(git -C "$ROOT" ls-files -z --cached --others \
    --exclude-standard -- . "${EXCLUDES[@]}")
else
  mapfile -d '' FILES < <(find . -type f -not -path './.git/*' -print0)
fi

[ "${#FILES[@]}" -gt 0 ] || { echo "provenance scan: no files to scan" >&2; exit 2; }

# Text files only: a grep over a parser table or a model is noise.
TEXT=()
for f in "${FILES[@]}"; do
  [ -f "$f" ] || continue
  if LC_ALL=C grep -qI '' "$f" 2>/dev/null; then TEXT+=("$f"); fi
done

status=0
report() { echo "provenance scan: $*" >&2; status=1; }

# 1. Upstream names, URLs, product names, handles and symbol prefixes.
while IFS= read -r pattern; do
  hits="$(grep -rniHE -- "$pattern" "${TEXT[@]}" 2>/dev/null | head -20)"
  if [ -n "$hits" ]; then
    report "denied string /$pattern/ found:"
    sed 's/^/    /' <<<"$hits" >&2
  fi
done < <(patterns_of "$DENYLIST")

# 2. Marker words, in non-test files only.
while IFS= read -r word; do
  hits="$(grep -rnHE -- "\\b${word}\\b" "${TEXT[@]}" 2>/dev/null \
          | grep -vE '(^|/)(tests?|__tests__)/|_test\.|\.test\.|\.spec\.' | head -20)"
  if [ -n "$hits" ]; then
    report "marker word ${word} in non-test code; record it in docs/plan/ISSUES.md instead:"
    sed 's/^/    /' <<<"$hits" >&2
  fi
done < <(patterns_of "$MARKERS")

# 3. URLs that are not on the allow list.
mapfile -t ALLOWED < <(patterns_of "$URLS")
while IFS= read -r line; do
  [ -n "$line" ] || continue
  file="${line%%:*}"
  url="${line#*:}"
  ok=0
  for prefix in "${ALLOWED[@]}"; do
    case "$url" in "$prefix"*) ok=1; break ;; esac
  done
  [ "$ok" -eq 1 ] || report "URL not on the allow list: $url ($file)"
done < <(grep -roHE 'https?://[A-Za-z0-9._~:/?#@!$&()*+,;=%-]+' "${TEXT[@]}" 2>/dev/null)

if [ "$status" -eq 0 ]; then
  echo "provenance scan: clean (${#TEXT[@]} files)"
fi
exit "$status"
