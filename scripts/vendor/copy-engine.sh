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

# Replace only what this script produces. Everything else under engine/ is ours:
# the build definition, the interface layer, the patches and the tests. A refresh
# that deleted those would take the engine's only connection to this project with
# it, and would do so silently.
for vendored in src grammars vendored LICENSE-ENGINE; do
  rm -rf "${DEST:?}/$vendored"
done
mkdir -p "$DEST"

# The matrix's identifiers, plus the one extra that the typescript identifier needs
# for its own dialect.
GRAMMARS=(
  ada bash c cpp c_sharp dockerfile go graphql groovy hcl java javascript json
  kotlin lua markdown objc perl php properties protobuf python ruby rust scala sql
  swift toml tsx typescript xml yaml
)

is_matrix_grammar() {
  local want="$1" id
  for id in "${GRAMMARS[@]}"; do [ "$id" = "$want" ] && return 0; done
  return 1
}

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
  # A grammar wrapper exists for every language the reference supports. Taking one
  # for a language we do not carry would include a parser that was never copied.
  case "$base" in
    grammar_*.c)
      wrapper_id="${base#grammar_}"
      wrapper_id="${wrapper_id%.c}"
      is_matrix_grammar "$wrapper_id" || continue
      ;;
  esac
  mkdir -p "$DEST/src"
  case "$base" in
    grammar_*.c)
      # The reference nests the grammars under its own source root; we keep them at
      # the engine root, so the include is rewritten to match the layout the reuse
      # map asks for.
      sed 's|"vendored/grammars/|"grammars/|g' "$f" > "$DEST/src/$base"
      ;;
    *) cp "$f" "$DEST/src/$base" ;;
  esac
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

for id in "${GRAMMARS[@]}"; do
  gsrc="$GRAMMARS_DIR/$id"
  gdest="$DEST/grammars/$id"
  [ -d "$gsrc" ] || die "grammar absent from the reference: $id"
  mkdir -p "$gdest"
  # The whole directory. A grammar's scanner may include a helper of any name and
  # any extension: two include headers, and one includes further source fragments.
  # These directories hold the compilation inputs and nothing else, so naming a
  # subset only invites the next omission. What is compiled is decided by the build
  # definition, not by what is present.
  cp -R "$gsrc/." "$gdest/"
  [ -f "$gdest/parser.c" ] || die "grammar $id has no parser"

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
  # The memory-instrumentation header, without its implementation, which is on the
  # list of sources never to take. Its hooks compile to nothing unless the feature
  # flags are defined, and they are not defined here, so the header is inert and the
  # foundation sources that include it need nothing linked.
  mem_events.h
  # Declarations only, again. This one's two functions are real and are called by
  # the foundation, but its implementation reaches for the operating system in ways
  # this project supplies itself: our own layer defines them, exactly as it does for
  # the lookups the resolution sources expect. Until it does they are undefined
  # symbols in the archive, which is what an archive is for.
  secure_random.h
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
