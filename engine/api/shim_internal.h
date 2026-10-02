/*
 * The shim's own construction interface, used by the engine interface layer.
 *
 * The resolution sources only ever read the registry. Something has to build it,
 * and that something is ours: this header is how the interface layer adds the
 * definitions and imports of each file before resolution runs. Nothing outside the
 * interface layer includes it.
 */

#ifndef PDXE_SHIM_INTERNAL_H
#define PDXE_SHIM_INTERNAL_H

#include <stdint.h>

#include "graph_buffer/graph_buffer.h"
#include "store/store.h"

#ifdef __cplusplus
extern "C" {
#endif

pdxe_gbuf_t *pdxe_gbuf_new(void);
void pdxe_gbuf_free(pdxe_gbuf_t *gb);

/*
 * Adds a node, or updates the one already under that qualified name, and returns its
 * identifier; 0 on failure or when `qualified_name` is NULL. Identifiers start at 1
 * and are handed out to new nodes in order, so 0 is never a valid one. Every string
 * is copied: the registry owns its storage and outlives nothing it borrowed.
 *
 * The reference's store keeps one node per qualified name, and when a second
 * arrives it decides which survives by content alone, so the answer does not depend
 * on the order files were processed in. This follows it exactly:
 *
 *   - a `Module` never displaces a `Project` or `Folder` node (a package-module
 *     language gives a directory and its package the same qualified name);
 *   - otherwise the arrival replaces the existing node unless the existing one
 *     comes first by: smaller file path, then larger start line, then larger name,
 *     then larger label. A full tie is the same entity, refreshed in place.
 *
 * An update keeps the node's identifier. When it changes the name, the node moves
 * between short-name lists the way the reference moves it (see find_by_name).
 */
int64_t pdxe_gbuf_upsert_node(pdxe_gbuf_t *gb, const char *label, const char *name,
                              const char *qualified_name, const char *file_path, int start_line,
                              int end_line, const char *properties_json);

/*
 * Adds a relationship, unless one with the same source, target and type is already
 * there, which the reference's store collapses into one; for imports, the local name
 * is part of that identity. Returns 0 on success.
 */
int pdxe_gbuf_add_edge(pdxe_gbuf_t *gb, int64_t source_id, int64_t target_id, const char *type,
                       const char *target_qualified_name, const char *properties_json);

uint32_t pdxe_gbuf_node_count(const pdxe_gbuf_t *gb);


/*
 * Sources come from memory, not disk: installed before resolution runs, removed
 * after. The provider returns a malloc'd buffer the caller frees, padded past the
 * end with zeros, because the parser reads ahead of the last byte.
 */
typedef char *(*pdxe_pxc_source_fn)(const char *path, int *out_len, void *userdata);
void pdxe_pxc_set_source_provider(pdxe_pxc_source_fn fn, void *userdata);

/*
 * The root crate manifest the cross-file pass uses in place of reading one from the
 * repository, added by engine/patches/0004; NULL for none. Thread-local, like the
 * source provider.
 */
struct PDXECargoManifest;
void pdxe_pxc_set_supplied_rust_manifest(const struct PDXECargoManifest *m);

/*
 * The package map the module resolver reads. The reference keeps one per process;
 * here it is per thread, set for the length of one project's resolution, since each
 * thread resolves its own project with its own engine context.
 */
struct PDXEHashTable;
void pdxe_pipeline_set_pkgmap(struct PDXEHashTable *map);


/*
 * A file's surface, encoded and decoded (engine/api/surface.c). Encoding is canonical:
 * equal results give equal bytes. Decoding gives a result of its own, freed with
 * pdxe_free_result, and the path and interface language the surface was made for, and
 * how much work extraction lost on it (lost_work.h), which a run counts as its own.
 */
struct PDXEFileResult;
int pdxe_surface_encode(const struct PDXEFileResult *r, const char *rel_path, int abi_lang,
                        uint32_t lost, uint8_t **out, size_t *out_len);
int pdxe_surface_decode(const uint8_t *bytes, size_t len, struct PDXEFileResult **out,
                        char **rel_path, int *abi_lang, uint32_t *lost);

/*
 * Every resolution the typed pass wrote, before the interface filters and attributes
 * them: resolved or not, matched to a call site or not. For tests, which must be able
 * to prove that no answer the resolver gave is dropped without a reason. Not part of
 * the public interface.
 */
typedef void (*pdxe_raw_resolution_fn)(const char *rel_path, const char *callee_qn,
                                       const char *strategy, const char *reason, int kind,
                                       uint32_t site_start, uint32_t site_end, void *userdata);
/*
 * Every import edge the project built: the importing file, the local name the import
 * binds, and the qualified name of the node it resolved to, with the project name
 * removed. For tests that compare import resolution with the reference's. Not part
 * of the public interface.
 */
typedef void (*pdxe_import_edge_fn)(const char *rel_path, const char *local_name,
                                    const char *target_qn, const char *target_label,
                                    void *userdata);
struct pdxe_project;
void pdxe_project_debug_raw_resolutions(struct pdxe_project *p, pdxe_raw_resolution_fn fn,
                                        void *userdata);
void pdxe_project_debug_imports(struct pdxe_project *p, pdxe_import_edge_fn fn, void *userdata);

#ifdef __cplusplus
}
#endif

#endif /* PDXE_SHIM_INTERNAL_H */
