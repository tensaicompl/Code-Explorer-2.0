#!/usr/bin/env bash
# Builds the reference engine from the pinned checkout, outside the repository, and
# prints the path of the binary it built.
#
# The reference is only ever run to produce output that ours is compared with; it is
# never committed, linked or shipped. Its build file, target and binary name are read
# from scripts/vendor/reference-run.txt, because they carry its name.
#
#   PDX_REF_DIR                  where references are fetched (default /tmp/ref)
#   PDX_REFERENCE_BUILD          where to build (default /tmp/pdx-reference-build)
#   PDX_REFERENCE_EXTRA_FLAGS    extra compiler and linker flags, for a machine whose
#                                system headers the reference expects are elsewhere;
#                                the engine differential also adds its observation hook
#                                here (scripts/engine/differential.py)
#   PDX_REFERENCE_JOBS           parallel compile jobs (default: one per online
#                                processor); lower it on a machine with little memory
set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/../vendor/lib.sh"

RUN_DATA="$REPO_ROOT/scripts/vendor/reference-run.txt"
run_field() { grep -vE '^\s*(#|$)' "$RUN_DATA" | awk -F'\t' -v k="$1" '$1 == k { print $2 }'; }

assert_pinned "$ENGINE_REF_NAME"
src="$(ref_path "$ENGINE_REF_NAME")"
build="$(realpath -m "${PDX_REFERENCE_BUILD:-/tmp/pdx-reference-build}")"
binary="$build/$(run_field binary)"

# The checkout must stay exactly as pinned: building writes only under $build, and
# a checkout with any change in it is no reference at all.
[ -z "$(git -C "$src" status --porcelain)" ] || die "the pinned checkout is not clean: $src"
case "$build/" in
  "$REPO_ROOT"/*) die "the reference must be built outside the repository, not in $build" ;;
  "$src"/*) die "the reference must be built outside its checkout, not in $build" ;;
esac
# A build is reused only for the same extra flags it was made with.
flags="${PDX_REFERENCE_EXTRA_FLAGS:-}"
stamp="$build/.pdx-extra-flags"
if [ -x "$binary" ] && [ "$binary" -nt "$src/.git/HEAD" ] && [ -f "$stamp" ] &&
  [ "$(cat "$stamp")" = "$flags" ]; then
  echo "$binary"
  exit 0
fi
mkdir -p "$build"
log="$build/build.log"
if ! make -C "$src" -f "$(run_field build_file)" "$(run_field build_target)" -j"${PDX_REFERENCE_JOBS:-$(nproc 2>/dev/null || echo 4)}" \
    "$(run_field build_dir_variable)=$build" \
    "$(run_field extra_flags_variable)=$flags" >"$log" 2>&1; then
  tail -20 "$log" >&2
  die "the reference engine did not build; the full log is $log"
fi
[ -z "$(git -C "$src" status --porcelain)" ] || die "building changed the pinned checkout"
[ -x "$binary" ] || die "the build finished but produced no binary at $binary"
printf '%s' "$flags" >"$stamp"
echo "$binary"
