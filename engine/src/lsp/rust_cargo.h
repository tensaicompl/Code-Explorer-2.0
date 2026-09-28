/* rust_cargo.h — Cargo.toml parser for the Rust LSP.
 *
 * Per RUST_LSP_FOLLOWUP §A3: we don't run Cargo or build, but we CAN
 * parse `Cargo.toml` and `[workspace] members` to learn:
 *   - the crate name (`[package].name`)
 *   - declared dependencies (`[dependencies]` + `[dev-dependencies]`)
 *   - workspace members + their relative paths
 *
 * The pipeline uses this to map `other_member::foo` → that member's
 * module QN, and to mark calls into known external deps as "external,
 * not local" rather than fully unresolved.
 *
 * The parser is a tiny hand-written TOML subset: handles `[section]`
 * headers, `key = "value"`, `key = { path = "...", … }` (the relevant
 * subset for our needs), arrays `members = ["a", "b"]`. It IGNORES
 * everything it doesn't understand — that's safe because Cargo.toml
 * is much richer than what we use. */

#ifndef PDXE_LSP_RUST_CARGO_H
#define PDXE_LSP_RUST_CARGO_H

#include "../arena.h"
#include <stdbool.h>

#define PDXE_CARGO_MAX_DEPS    256
#define PDXE_CARGO_MAX_MEMBERS  64

typedef struct {
    const char* name;       /* declared dependency name */
    const char* path;       /* path = "../foo" if local, else NULL */
} PDXECargoDep;

typedef struct {
    const char* member_name;   /* directory name */
    const char* member_path;   /* relative path inside workspace root */
} PDXECargoMember;

typedef struct PDXECargoManifest {
    const char* package_name;    /* [package].name, NULL if missing */
    const char* package_version; /* [package].version, NULL if missing */
    bool is_workspace_root;      /* [workspace] section seen */

    PDXECargoDep deps[PDXE_CARGO_MAX_DEPS];
    int dep_count;

    PDXECargoMember members[PDXE_CARGO_MAX_MEMBERS];
    int member_count;
} PDXECargoManifest;

/* Parse a Cargo.toml-formatted string. The output strings are
 * arena-allocated (so the caller doesn't need to keep `src` alive). */
void pdxe_cargo_parse(PDXEArena* arena, const char* src, int src_len,
    PDXECargoManifest* out);

/* Convenience: does a given path-prefix look like one of the listed
 * dependency names? Used by the resolver to recognise external crate
 * paths. */
bool pdxe_cargo_is_known_dep(const PDXECargoManifest* m, const char* head);

/* Find a workspace member by crate name. Returns NULL if absent. */
const PDXECargoMember* pdxe_cargo_find_member(const PDXECargoManifest* m,
    const char* name);

#endif /* PDXE_LSP_RUST_CARGO_H */
