#!/usr/bin/env bash
# Fetches the extraction engine reference at the commit pinned in the lock file,
# into a directory outside the repository, and verifies its licence.
#
# Nothing is copied into the tree here. That is copy-engine.sh.
set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

name="$ENGINE_REF_NAME"
dir="$(ref_path "$name")"
clone_url="$(lock_field "$name" clone_url)" || die "cannot read the reference location"
commit="$(lock_field "$name" commit)" || die "cannot read the pinned commit"
expected_licence="$(lock_field "$name" licence)" || die "cannot read the pinned licence"

if [ -d "$dir/.git" ]; then
  actual="$(git -C "$dir" rev-parse HEAD)"
  if [ "$actual" = "$commit" ]; then
    note "reference already at the pinned commit"
  else
    note "reference is at $actual, fetching the pinned commit"
    git -C "$dir" fetch --depth 1 origin "$commit" ||
      die "cannot fetch the pinned commit"
    git -C "$dir" checkout -q --detach FETCH_HEAD || die "cannot check out the commit"
  fi
else
  mkdir -p "$dir"
  git -C "$dir" init -q
  git -C "$dir" remote add origin "$clone_url" 2>/dev/null ||
    git -C "$dir" remote set-url origin "$clone_url"
  # One commit, no history: the pin is what matters and the reference is large.
  git -C "$dir" fetch --depth 1 origin "$commit" || die "cannot fetch the pinned commit"
  git -C "$dir" checkout -q --detach FETCH_HEAD || die "cannot check out the commit"
fi

assert_pinned "$name"

# The licence is verified against the pin on every fetch, not only the first: an
# upstream that relicenses must stop the build rather than be vendored quietly.
licence_file=""
for candidate in LICENSE LICENSE.txt LICENSE.md COPYING; do
  [ -f "$dir/$candidate" ] && { licence_file="$dir/$candidate"; break; }
done
[ -n "$licence_file" ] || die "the reference has no licence file"

case "$expected_licence" in
  MIT)
    grep -q "MIT License" "$licence_file" ||
      grep -q "Permission is hereby granted, free of charge" "$licence_file" ||
      die "the reference's licence file does not read as the pinned licence ($expected_licence)"
    ;;
  *)
    note "licence $expected_licence is not checked by text here; verify it by hand"
    ;;
esac

note "reference fetched at $(git -C "$dir" rev-parse --short HEAD), licence $expected_licence verified"
note "location: $dir"
