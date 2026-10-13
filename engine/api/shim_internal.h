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

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "pdxe.h"
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
/*
 * Issue 90: `source` is the file's source the result was extracted from, `source_len`
 * bytes, which the surface's text ranges are ranges of. Decoding checks every range
 * against the length the surface records and leaves the text it stands for unset in
 * the result, listed in `*texts`, until pdxe_surface_bind fills it from the source the
 * file is resolved with: of that length, or the bind fails with PDXE_E_INVALID. The
 * list is consumed by the bind, or freed with pdxe_surface_texts_free.
 */
typedef struct pdxe_surface_texts pdxe_surface_texts;
int pdxe_surface_encode(const struct PDXEFileResult *r, const char *rel_path, int abi_lang,
                        uint32_t lost, const char *source, size_t source_len, uint8_t **out,
                        size_t *out_len);
int pdxe_surface_decode(const uint8_t *bytes, size_t len, struct PDXEFileResult **out,
                        char **rel_path, int *abi_lang, uint32_t *lost,
                        pdxe_surface_texts **texts);
int pdxe_surface_bind(struct PDXEFileResult *r, pdxe_surface_texts *texts, const char *source,
                      size_t source_len);
void pdxe_surface_texts_free(pdxe_surface_texts *texts);

/*
 * Issue 90: whether `s` is exactly the bytes of `source` that begin at `start` inside a
 * fact's site [start, limit): then `*end` is where they end, and [start, *end) is the
 * text's range. One rule for the interface and the surface.
 */
bool pdxe_text_in_site(const char *source, size_t source_len, const char *s, uint32_t start,
                       uint32_t limit, uint32_t *end);

/*
 * Issue 90: one file's definitions' qualified names, each to the first definition that
 * has it, and the choice of the earlier definition each name is given relative to.
 * pdxe_qn_choose takes `qns`, every definition's qualified name by index, and is called
 * for each definition in order, `self` 0 first. It returns definition `self`'s form:
 * PDXE_QN_MEMBER, with *base, when its name is an earlier definition's, `.`, then
 * `name`; PDXE_QN_EXTENDED, with *base and *rest, when it is an earlier definition's
 * name followed by a rest that is empty or begins with an ASCII byte; PDXE_QN_PREFIX,
 * with *base, *keep and *rest, when it is the first *keep bytes of an earlier
 * definition's name, ending before a `.` that definition adds itself, followed by a
 * rest that is empty or begins with `.`; else PDXE_QN_WHOLE.
 *
 * The base tried first is the definition its name names once its final `.` and `name`
 * are taken off (a nested class's enclosing class, which the engine does not record as
 * its parent), then `parent`, then `module`. A prefix is taken instead only when it
 * leaves strictly fewer bytes to hold: the longest `.`-bounded prefix of the name that
 * an earlier definition added, found by hashing the name's prefixes once, never by
 * copying them. Each name then records the `.`-bounded prefixes it adds beyond its
 * base, so every prefix recorded lies in the bytes its definition adds, and the bytes
 * held over a file are at most the names' segments that are new: linear in the file
 * however deeply its namespaces nest. One rule for the interface and the surface.
 */
typedef struct {
    uint64_t hash;
    const char *text; /* a definition's qualified name; NULL for an empty slot */
    uint32_t len;     /* the prefix: its first `len` bytes */
    uint32_t def;
    bool whole;       /* the prefix is the definition's whole name */
} pdxe_qn_prefix;
typedef struct {
    struct PDXEHashTable *names;
    char *key;
    size_t key_cap;
    pdxe_qn_prefix *prefixes; /* open addressing; a power of two, at most half full */
    size_t prefix_cap;
    size_t n_prefixes;
    uint32_t *bounds; /* the name at hand's `.`-bounded prefix lengths, and its own */
    uint64_t *hashes; /* and their hashes */
    size_t bounds_cap;
} pdxe_qn_index;
bool pdxe_qn_index_init(pdxe_qn_index *x, const char *const *qns, uint32_t n);
void pdxe_qn_index_free(pdxe_qn_index *x);
/* The first definition whose qualified name is `qn`, or UINT32_MAX. */
uint32_t pdxe_qn_index_find(const pdxe_qn_index *x, const char *qn);
int pdxe_qn_choose(pdxe_qn_index *x, const char *const *qns, const char *name, uint32_t self,
                   uint32_t parent, uint32_t module, uint32_t *base, uint32_t *keep,
                   const char **rest);

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

/*
 * Each file's cross-file surface as the reference persists it for its incremental
 * builds: the definitions the cross-file pass registers from the file and the names
 * only the registry serves, as the JSON the engine's own codec writes
 * (resolve/lsp_surface.c), built after the run from the project's results and context
 * exactly as the pass builds them. A run never builds these (the interface gives the
 * pass no pipeline to hand them to). For the engine differential (P2-16), which
 * compares them with the reference's; a file whose results came from surfaces
 * (pdxe_surface_import) is built from what the surface carried. Qualified names keep
 * the project sentinel. Not part of the public interface.
 */
typedef void (*pdxe_lsp_surface_fn)(const char *rel_path, const char *defs_json, void *userdata);
void pdxe_project_debug_lsp_surfaces(struct pdxe_project *p, pdxe_lsp_surface_fn fn,
                                     void *userdata);

/*
 * The engine's own result behind a result extraction returned, with what the engine
 * keeps private: the declaration occurrences that join sites to the definitions owning
 * them (issue 76). NULL for a result rebuilt from a cache. For tests and diagnostics
 * (engine/tests/owner_census.c). Not part of the public interface.
 */
const struct PDXEFileResult *pdxe_result_engine(const pdxe_file_result *r);

#ifdef __cplusplus
}
#endif

#endif /* PDXE_SHIM_INTERNAL_H */
