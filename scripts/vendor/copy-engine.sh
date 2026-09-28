#!/usr/bin/env bash
# Copies the extraction engine from the pinned reference into engine/.
#
# The file-level map is authoritative and is reproduced here as data rather than
# prose: what is copied, and nothing else. A required file that has moved stops the
# script, because that means the map and the pinned commit disagree and someone must
# look rather than let the build quietly lose a subsystem.
#
# This step copies only. The upstream names are still present afterwards; the rename
# step removes them and the provenance scan is the check.
set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

assert_pinned "$ENGINE_REF_NAME"
SRC="$(ref_path "$ENGINE_REF_NAME")"
DEST="$REPO_ROOT/engine"
MAP="$REPO_ROOT/scripts/vendor/copy-map.txt"
[ -r "$MAP" ] || die "the path map is missing: ${MAP#$REPO_ROOT/}"

# Upstream paths live in the map, not in this script: a path names the upstream, and
# this script is scanned for exactly that.
upstream() {
  local key="$1" value
  value="$(grep -E "^${key}\b" "$MAP" | head -1 | cut -f2)"
  [ -n "$value" ] || die "the path map has no entry for $key"
  echo "$SRC/$value"
}

CORE_DIR="$(upstream core_dir)"
RESOLVER_DIR="$(upstream resolver_dir)"
RESOLVER_GENERATED_DIR="$(upstream resolver_generated_dir)"
VENDORED_DIR="$(upstream vendored_dir)"
GRAMMARS_DIR="$(upstream grammars_dir)"
TOP_VENDORED_DIR="$(upstream top_vendored_dir)"
FOUNDATION_DIR="$(upstream foundation_dir)"
PIPELINE_DIR="$(upstream pipeline_dir)"

[ "${1:-}" = "--force" ] || [ ! -d "$DEST" ] ||
  die "engine/ exists; pass --force to replace it"
rm -rf "$DEST"
mkdir -p "$DEST"

# --- extraction core -------------------------------------------------------
#
# Storage, export, semantic similarity, and the extractors done in Rust are not
# taken. Neither is the upstream unity-build file, which exists only to compile
# everything as one translation unit.
EXCLUDE_CORE=(
  sqlite_writer.c iris_export_xml.c lz4_store.c zstd_store.c
  extract_semantic.c extract_dbt.c extract_k8s.c result_compact.c lsp_all.c
)
core_copied=0
for f in "$CORE_DIR"/*.c "$CORE_DIR"/*.h; do
  [ -f "$f" ] || continue
  base="$(basename "$f")"
  skip=0
  for x in "${EXCLUDE_CORE[@]}"; do [ "$base" = "$x" ] && skip=1 && break; done
  [ "$skip" -eq 1 ] && continue
  mkdir -p "$DEST/src"
  cp "$f" "$DEST/src/$base"
  core_copied=$((core_copied + 1))
done
note "extraction core: $core_copied files"

# --- typed resolution layer and its generated tables -----------------------
n=$(copy_glob "$RESOLVER_DIR" '*.c' "$DEST/src/lsp")
m=$(copy_glob "$RESOLVER_DIR" '*.h' "$DEST/src/lsp")
note "typed resolution: $((n + m)) files"
n=$(copy_glob "$RESOLVER_GENERATED_DIR" '*.c' "$DEST/src/lsp/generated")
m=$(copy_glob "$RESOLVER_GENERATED_DIR" '*.h' "$DEST/src/lsp/generated")
note "standard library tables: $((n + m)) files"

# --- the one C++ translation unit ------------------------------------------
copy_file "$(upstream preprocessor_source)" "$DEST/src/preprocessor.cpp"
copy_file "$(upstream preprocessor_header)" "$DEST/src/preprocessor.h"

# --- vendored libraries, each keeping its own licence ----------------------
copy_vendored_tree() {
  local src="$1" name="$2"
  [ -d "$src" ] || die "vendored library absent from the reference: $name"
  mkdir -p "$DEST/vendored/$name"
  cp -R "$src/." "$DEST/vendored/$name/"
  # Its licence travels with it, which is what the licence requires and what the
  # licence scanner checks for.
  local found=0 c
  for c in LICENSE LICENSE.txt LICENSE.md COPYING NOTICE; do
    [ -f "$DEST/vendored/$name/$c" ] && found=1 && break
  done
  [ "$found" -eq 1 ] || note "  warning: vendored/$name carries no licence file"
}
copy_vendored_tree "$VENDORED_DIR/simplecpp" simplecpp
copy_vendored_tree "$VENDORED_DIR/ts_runtime" ts_runtime
copy_vendored_tree "$VENDORED_DIR/common" common
for lib in verstable wyhash lz4 zstd; do
  [ -d "$VENDORED_DIR/$lib" ] && copy_vendored_tree "$VENDORED_DIR/$lib" "$lib"
done
for lib in yyjson mimalloc; do
  [ -d "$TOP_VENDORED_DIR/$lib" ] && copy_vendored_tree "$TOP_VENDORED_DIR/$lib" "$lib"
done
note "vendored libraries: $(ls "$DEST/vendored" | wc -l) directories"

# --- grammars --------------------------------------------------------------
#
# The matrix's identifiers, plus the one extra that the typescript identifier needs
# for its own dialect. Parser tables only: no queries, no bindings, no tests.
GRAMMARS=(
  ada bash c cpp c_sharp dockerfile go graphql groovy hcl java javascript json
  kotlin lua markdown objc perl php properties protobuf python ruby rust scala sql
  swift toml tsx typescript xml yaml
)
for id in "${GRAMMARS[@]}"; do
  gsrc="$GRAMMARS_DIR/$id"
  gdest="$DEST/grammars/$id"
  [ -d "$gsrc" ] || die "grammar absent from the reference: $id"
  mkdir -p "$gdest"
  copy_file "$gsrc/parser.c" "$gdest/parser.c"
  copy_file "$gsrc/scanner.c" "$gdest/scanner.c" optional
  copy_file "$gsrc/_common_scanner.h" "$gdest/_common_scanner.h" optional
  if [ -d "$gsrc/tree_sitter" ]; then
    copy_glob "$gsrc/tree_sitter" '*.h' "$gdest/tree_sitter" >/dev/null
  fi
  licence_found=0
  for c in LICENSE LICENSE.txt LICENSE.md COPYING; do
    if [ -f "$gsrc/$c" ]; then cp "$gsrc/$c" "$gdest/LICENSE"; licence_found=1; break; fi
  done
  [ "$licence_found" -eq 1 ] || die "grammar $id carries no licence file; it may not be vendored"
done
note "grammars: ${#GRAMMARS[@]} directories"

# --- foundation ------------------------------------------------------------
#
# The minimum the engine needs. The rule for growing this list is the linker: add a
# file only when an undefined symbol demands it, never on the assumption it is
# wanted. These are the ones that must never be taken, whatever the linker says,
# because they reach for the operating system in ways the pipeline owns instead.
FOUNDATION=(
  arena hash_table compat compat_thread compat_fs mem_core mem sha256 platform log
  str_util str_intern
)
FOUNDATION_HEADERS=(
  constants.h dyn_array.h limits.h sanitized.h recursion_whitelist.h
  platform_internal.h compat_fs_internal.h
)
for unit in "${FOUNDATION[@]}"; do
  copy_file "$FOUNDATION_DIR/$unit.c" "$DEST/src/foundation/$unit.c" optional
  copy_file "$FOUNDATION_DIR/$unit.h" "$DEST/src/foundation/$unit.h" optional
done
for header in "${FOUNDATION_HEADERS[@]}"; do
  copy_file "$FOUNDATION_DIR/$header" "$DEST/src/foundation/$header" optional
done
note "foundation: $(ls "$DEST/src/foundation" | wc -l) files"

# --- cross-file typed resolution -------------------------------------------
#
# These four drive resolution across files and are the reason the engine is
# vendored rather than only read. They call into the upstream pipeline in a handful
# of places; those calls are served by our own shim.
copy_file "$PIPELINE_DIR/registry.c" "$DEST/src/resolve/registry.c"
copy_file "$PIPELINE_DIR/registry.h" "$DEST/src/resolve/registry.h" optional
copy_file "$PIPELINE_DIR/lsp_surface.c" "$DEST/src/resolve/lsp_surface.c"
copy_file "$PIPELINE_DIR/lsp_surface.h" "$DEST/src/resolve/lsp_surface.h" optional
copy_file "$PIPELINE_DIR/pass_lsp_cross.c" "$DEST/src/resolve/pass_lsp_cross.c"
copy_file "$PIPELINE_DIR/pass_lsp_cross.h" "$DEST/src/resolve/pass_lsp_cross.h" optional
copy_file "$PIPELINE_DIR/lsp_resolve.h" "$DEST/src/resolve/lsp_resolve.h"
copy_file "$PIPELINE_DIR/fqn.c" "$DEST/src/resolve/fqn.c"
note "cross-file resolution: $(ls "$DEST/src/resolve" | wc -l) files"

# --- licence ---------------------------------------------------------------
copy_file "$(upstream licence_file)" "$DEST/LICENSE-ENGINE"

total=$(find "$DEST" -type f | wc -l)
note "copied $total files into engine/"
note "upstream names are still present: run scripts/vendor/rename-engine.sh next"
