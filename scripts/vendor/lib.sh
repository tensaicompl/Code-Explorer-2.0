#!/usr/bin/env bash
# Shared helpers for the vendoring scripts.
#
# No script here names the upstream it fetches from. The reference list is the one
# file permitted to do that, and it is excluded from the provenance scan for that
# reason, so every script reads the name, location, commit and licence from there.
# Hard-coding any of it would fail the scan and, worse, would let the tree drift
# from the pinned commit.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOCK_FILE="$REPO_ROOT/bench/references.lock"
# Where references are fetched to. Never inside the repository.
REF_DIR="${PDX_REF_DIR:-/tmp/ref}"
ENGINE_REF_NAME="engine-ref"

die() { echo "error: $*" >&2; exit 1; }
note() { echo "$*"; }

# Reads one field of one reference from the lock file.
#   lock_field <reference-name> <field>
lock_field() {
  local name="$1" field="$2"
  python3 - "$LOCK_FILE" "$name" "$field" <<'PY'
import sys, tomllib
path, name, field = sys.argv[1], sys.argv[2], sys.argv[3]
with open(path, "rb") as fh:
    data = tomllib.load(fh)
for entry in data.get("reference", []):
    if entry.get("name") == name:
        value = entry.get(field)
        if value is None:
            sys.exit(f"reference {name} has no field {field}")
        print(value)
        break
else:
    sys.exit(f"no reference named {name} in the lock file")
PY
}

# The checkout directory for a reference.
ref_path() { echo "$REF_DIR/$1"; }

# Fails unless the checkout is at the pinned commit.
assert_pinned() {
  local name="$1" dir expected actual
  dir="$(ref_path "$name")"
  expected="$(lock_field "$name" commit)" || die "cannot read the pinned commit"
  [ -d "$dir/.git" ] || die "$name is not fetched; run scripts/vendor/fetch-engine.sh"
  actual="$(git -C "$dir" rev-parse HEAD)"
  [ "$actual" = "$expected" ] ||
    die "$name is at $actual but the lock file pins $expected"
}

# Copies one file, creating the destination directory. Missing source is an error
# unless the third argument is "optional": the upstream layout is pinned, so a
# required file that has moved means the pin and this map disagree.
copy_file() {
  local src="$1" dest="$2" mode="${3:-required}"
  if [ ! -f "$src" ]; then
    [ "$mode" = "optional" ] && return 0
    die "expected file is absent from the reference: ${src#$REF_DIR/}"
  fi
  mkdir -p "$(dirname "$dest")"
  cp "$src" "$dest"
}

# Copies every file matching a glob within a directory, non-recursively.
copy_glob() {
  local src_dir="$1" pattern="$2" dest_dir="$3" count=0 f
  [ -d "$src_dir" ] || die "expected directory is absent: ${src_dir#$REF_DIR/}"
  mkdir -p "$dest_dir"
  for f in "$src_dir"/$pattern; do
    [ -f "$f" ] || continue
    cp "$f" "$dest_dir/"
    count=$((count + 1))
  done
  echo "$count"
}
