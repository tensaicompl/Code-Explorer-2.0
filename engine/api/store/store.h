/*
 * The store the reference writes its results to is not vendored: results here
 * live only as long as the caller holds them. One type escapes into a header the
 * resolution sources include, so it is declared, and nothing more.
 */
#ifndef PDXE_STORE_FORWARD_H
#define PDXE_STORE_FORWARD_H

#include <stddef.h>
#include <stdint.h>

/* Opaque: no source here dereferences one. */
typedef struct pdxe_store pdxe_store;

/*
 * One definition a file contributes to the cross-file registry.
 *
 * This is the surface: the rows a file publishes so that other files can resolve
 * against it without being re-parsed. The reference keeps the type beside its
 * store because that is where the rows were written; here they are exported as
 * JSON and cached by the caller, so only the shape is needed.
 */
/* One file's persisted LSP surface: the serialized cross-file definition set
 * (exactly what pass_lsp_cross registration consumes) plus the metadata the
 * closure-repair incremental route needs to decide and bound its work. The
 * store treats defs_json/ref_bloom as opaque; the codec lives with
 * pass_lsp_cross, which is the only writer and reader of their contents. */
typedef struct {
    const char *project;
    const char *rel_path;
    const char *surface_sha; /* sha256 hex of defs_json (the early-cutoff key) */
    const char *defs_json;   /* canonical JSON array of the file's LSP defs */
    const void *ref_bloom;   /* referenced-identifier bloom blob (may be NULL) */
    int ref_bloom_len;
    const char *config_ctx; /* governing-config context hash ("" = none) */
} pdxe_lsp_surface_row_t;

#endif /* PDXE_STORE_FORWARD_H */
