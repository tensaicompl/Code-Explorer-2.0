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

# The deny list may carry a replacement after a tab, used by the vendoring rename
# step. Only the pattern concerns the scan.
denied_patterns() { patterns_of "$1" | cut -f1; }

# Paths the scan does not read, and why.
#
#   THIRD_PARTY_NOTICES.md      the legal document; naming upstreams is its purpose
#   engine/**/LICENSE*          vendored licence texts, kept verbatim
#   bench/references.lock       the pinned reference list; naming them is its purpose
#   scripts/*-denylist.txt etc  this scanner's own inputs
#   engine/LICENSE*             the vendored engine's own licence, kept verbatim
#   scripts/vendor/*-map.txt    the vendoring scripts' own inputs
#   legacy/                     third-party-derived code awaiting removal, already
#                               attributed in the notices file; the exclusion goes
#                               away when the retirement task deletes it
EXCLUDES=(
  ':(exclude)THIRD_PARTY_NOTICES.md'
  ':(exclude)engine/**/LICENSE*'
  ':(exclude)engine/grammars/**/LICENSE*'
  ':(exclude)bench/references.lock'
  ':(exclude)engine/LICENSE*'
  ':(exclude)scripts/provenance-denylist.txt'
  ':(exclude)scripts/vendor/rename-map.txt'
  ':(exclude)scripts/vendor/copy-map.txt'
  ':(exclude)scripts/marker-words.txt'
  ':(exclude)scripts/url-allowlist.txt'
  ':(exclude)legacy/**'
)

cd "$ROOT" || exit 2

# The file set: inside a repository, tracked files plus new ones that are not
# ignored, so that a file escapes the scan only by being ignored, never by being
# unstaged. Outside a repository, every file, so a fixture directory works too.
#
# Read with a loop rather than mapfile: mapfile arrives in bash 4, and macOS still
# ships bash 3.2. A counter is kept alongside because an empty array expands
# inconsistently under set -u across those versions.
FILES=()
file_count=0
collect() {
  while IFS= read -r -d '' entry; do
    FILES+=("$entry")
    file_count=$((file_count + 1))
  done
}

if git -C "$ROOT" rev-parse --git-dir >/dev/null 2>&1; then
  collect < <(git -C "$ROOT" ls-files -z --cached --others \
    --exclude-standard -- . "${EXCLUDES[@]}")
else
  # Outside a repository the git pathspecs above do not apply, so the same
  # exclusions are applied by path here. Without this the scanner reads its own
  # deny list and reports every term in it.
  collect < <(find . -type f -not -path './.git/*' \
    -not -path './legacy/*' \
    -not -name 'THIRD_PARTY_NOTICES.md' \
    -not -name 'references.lock' \
    -not -name 'provenance-denylist.txt' \
    -not -name 'marker-words.txt' \
    -not -name 'url-allowlist.txt' \
    -not -path '*/engine/*/LICENSE*' \
    -print0)
fi

[ "$file_count" -gt 0 ] || { echo "provenance scan: no files to scan" >&2; exit 2; }

# Text files only: a grep over a parser table or a model is noise.
TEXT=()
text_count=0
for f in "${FILES[@]}"; do
  [ -f "$f" ] || continue
  if LC_ALL=C grep -qI '' "$f" 2>/dev/null; then
    TEXT+=("$f")
    text_count=$((text_count + 1))
  fi
done

[ "$text_count" -gt 0 ] || { echo "provenance scan: no text files to scan" >&2; exit 2; }

status=0
report() { echo "provenance scan: $*" >&2; status=1; }

# 1. Upstream names, URLs, product names, handles and symbol prefixes.
while IFS= read -r pattern; do
  hits="$(grep -rniHE -- "$pattern" "${TEXT[@]}" 2>/dev/null | head -20)"
  if [ -n "$hits" ]; then
    report "denied string /$pattern/ found:"
    sed 's/^/    /' <<<"$hits" >&2
  fi
done < <(denied_patterns "$DENYLIST")

# 2. Marker words, in non-test files we author.
while IFS= read -r word; do
  hits="$(grep -rnHE -- "\\b${word}\\b" "${TEXT[@]}" 2>/dev/null \
          | grep -vE '(^|/)(tests?|__tests__)/|_test\.|\.test\.|\.spec\.' \
          | grep -vE '^\.?/?engine/' | head -20)"
  if [ -n "$hits" ]; then
    report "marker word ${word} in non-test code; record it in docs/plan/ISSUES.md instead:"
    sed 's/^/    /' <<<"$hits" >&2
  fi
done < <(patterns_of "$MARKERS")

# 3. URLs that are not on the allow list.
#
# The vendored engine is exempt from this rule and from the marker rule, but never
# from the deny list. Both of those rules govern code we author: an address we wrote
# is either documentation or an undeclared network call, and a marker we wrote is
# work that belongs in the issue log. Neither is true of third-party source we keep
# as close to upstream as we can, where a comment citing a library's own
# documentation is part of the file we are licensed to redistribute, and editing
# hundreds of files to remove upstream's markers would create a diff against the
# reference for no gain. What may never appear there, and is still checked, is
# anything naming where the code came from.
vendored_path() {
  case "$1" in
    ./engine/*|engine/*) return 0 ;;
    *) return 1 ;;
  esac
}
ALLOWED=()
while IFS= read -r allowed_prefix; do
  [ -n "$allowed_prefix" ] && ALLOWED+=("$allowed_prefix")
done < <(patterns_of "$URLS")
while IFS= read -r line; do
  [ -n "$line" ] || continue
  file="${line%%:*}"
  url="${line#*:}"
  vendored_path "$file" && continue
  ok=0
  for prefix in "${ALLOWED[@]}"; do
    case "$url" in "$prefix"*) ok=1; break ;; esac
  done
  [ "$ok" -eq 1 ] || report "URL not on the allow list: $url ($file)"
done < <(grep -roHE 'https?://[A-Za-z0-9._~:/?#@!$&()*+,;=%-]+' "${TEXT[@]}" 2>/dev/null)

if [ "$status" -eq 0 ]; then
  echo "provenance scan: clean ($text_count files)"
fi
exit "$status"
