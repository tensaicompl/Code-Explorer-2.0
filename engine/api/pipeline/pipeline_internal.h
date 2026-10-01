/*
 * Types the vendored resolution sources pass around, extracted rather than
 * vendored.
 *
 * The reference declares these beside an indexing pipeline this project replaces:
 * a discovery walk, a store, a job queue. Copying those headers to obtain three
 * structures would bring the subsystems with them, so the structures are
 * reproduced here and nothing else is.
 */

#ifndef PDXE_PIPELINE_INTERNAL_H
#define PDXE_PIPELINE_INTERNAL_H

#include <stdatomic.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "graph_buffer/graph_buffer.h"
#include "pipeline/path_alias.h"
#include "foundation/hash_table.h"
#include "pdxe_core.h"
#include "pipeline/pipeline.h"
#include "store/store.h"

#ifdef __cplusplus
extern "C" {
#endif

/*
 * The context's pipeline field points at the reference's own pipeline object, which
 * this project replaces; nothing in the vendored sources dereferences it, so it stays
 * opaque. The path aliases are different: the import resolver reads them, so their
 * real types come from the vendored alias header below.
 */
typedef struct pdxe_pipeline pdxe_pipeline_t;

/* One file, as the caller describes it when adding it to a project. */
typedef struct {
    char *path;           /* absolute path (heap-allocated) */
    char *rel_path;       /* relative to repo root (heap-allocated) */
    PDXELanguage language; /* detected language */
    int64_t size;         /* file size in bytes */
} pdxe_file_info_t;

/*
 * What the cross-file typed-resolution pass did, in its own words. The reference's
 * pass reports this only to its log; engine/patches/0006 has it written here too, so
 * the interface can say whether a run did all its work without reading a log, which
 * is process-wide while a project is not (docs/plan/ISSUES.md, issue 21). Zero until
 * the pass writes it.
 */
typedef struct {
    bool completed;              /* the pass reached its end rather than stopping early */
    bool definitions_collected;  /* it collected the project's definitions */
    int files_dispatched;        /* files it resolved, or tried to */
    int files_skipped_no_lsp;    /* files in a language it does not resolve */
    int files_skipped_no_source; /* files whose source it did not obtain, or had none */
} pdxe_lsp_cross_record_t;

/*
 * What a pass is given: the project it is working on, the files in it, and the
 * registry and graph built from them.
 */
/* Shared context passed to each pass function.
 * Derived from pdxe_pipeline_t fields during run. */
typedef struct {
    const char *project_name; /* borrowed from pipeline */
    const char *repo_path;    /* borrowed from pipeline */
    pdxe_gbuf_t *gbuf;         /* owned by pipeline */
    pdxe_registry_t *registry; /* owned by pipeline */
    atomic_int *cancelled;    /* pointer to pipeline's cancelled flag */
    pdxe_pipeline_t *pipeline; /* back-pointer for recording per-file skips
                               * (Stage 2 / Track B). May be NULL on paths that
                               * don't record; pdxe_pipeline_add_file_error is
                               * NULL-safe. */
    int mode;                 /* pdxe_index_mode_t (0=full, 1=moderate, 2=fast, 3=advanced) */

    /* Extraction result cache (sequential pipeline optimization).
     * When non-NULL, pass_definitions stores results here instead of freeing,
     * and pass_calls/usages/semantic reuse cached results instead of re-extracting.
     * Indexed by file position in the files[] array. Owned by pipeline.c. */
    PDXEFileResult **result_cache;

    /* Build-tool path aliases (tsconfig/jsconfig today; webpack/vite-style
     * configs are an easy follow-on). NULL when no usable configs were found.
     * Owned by pipeline.c / pipeline_incremental.c. */
    const pdxe_path_alias_collection_t *path_aliases;

    /* Directory subtrees excluded during discovery. Borrowed from pipeline.c. */
    char **excluded_dirs;
    int excluded_count;

    /* Sequential cross-LSP registry arena. The lsp_cross pass builds its
     * shared per-language registries here; resolved_calls entries may BORROW
     * strings owned by these registries, and the later calls pass still
     * reads them — so the arena is OWNED and destroyed by
     * run_sequential_pipeline AFTER all passes, never by the lsp_cross pass
     * itself (destroying at pass end was a use-after-free in pass_calls).
     * Mirrors the parallel path, where cross_lsp_arena outlives the fused
     * resolve. */
    PDXEArena seq_cross_arena;
    bool seq_cross_arena_live;
    /* Sequential lsp_cross only: the per-file module-QN strings the collected
     * defs (and through them the shared cross registries in seq_cross_arena)
     * borrow. The registries outlive the pass so pass_calls can read borrowed
     * strings -- these must too. Ownership transfers here at the end of the
     * pass; released beside the arena. Freeing them at pass end was a
     * use-after-free first observable on the real-repo corpus tier. */
    char **seq_cross_def_modules;
    int seq_cross_def_module_count;
    /* Written by the cross-file pass; see pdxe_lsp_cross_record_t above. */
    pdxe_lsp_cross_record_t lsp_cross;

    /* ObjectScript $$$macro table built from .inc files in the repo (NULL if
     * no ObjectScript include files were found). Owned by pipeline.c. */
    const PDXEMacroTable *macro_table;

    /* ObjectScript method-return-type table built from extracted definitions
     * (NULL until pass_calls builds it). Owned by pipeline.c. */
    const PDXEReturnTypeTable *return_type_table;

    /* Spill / admission control (2026-09-13). spill_mode latches on the first
     * over-budget observation in the extract gate (or on PDXE_MEM_SPILL=1):
     * from then on every compacted result is parked on disk instead of held
     * in the cache, results already cached are swept out, and every later
     * consumer (registry build, def collection, resolve) loads a result only
     * for the moment it reads it. Memory then sits at the floor -- graph +
     * registries + in-flight files -- and the run pays with disk reads.
     * NULL/0 = results stay in memory as always. Owned by pipeline.c. */
    struct pdxe_result_spill *spill;
    _Atomic int spill_mode;
    /* Set by the ONE owner whose every result-cache consumer goes through
     * pdxe_pipeline_result_acquire()/release() and that closes the store
     * (run_parallel_pipeline). An owner that leaves it false never spills:
     * the incremental and probe routes still hand the cache array to passes
     * that index it directly, so they keep results in memory (follow-up). */
    bool spill_allowed;
} pdxe_pipeline_ctx_t;

/*
 * Reading a result from the cache. The reference keeps a result either in memory or
 * parked on disk and reads both through this pair; the filter sees a result before
 * it is handed over and may decline it.
 */
typedef bool (*pdxe_result_want_fn)(const PDXEFileResult *header);
PDXEFileResult *pdxe_pipeline_result_acquire(const pdxe_pipeline_ctx_t *ctx,
                                             PDXEFileResult **cache, int i,
                                             pdxe_result_want_fn want, bool *loaded);
void pdxe_pipeline_result_release(PDXEFileResult *r, bool loaded);

/* Hands the surfaces the resolution built to the pipeline object, which owns them. */
void pdxe_pipeline_set_lsp_surfaces(pdxe_pipeline_t *p, pdxe_lsp_surface_row_t *rows, int count);
void pdxe_store_free_lsp_surfaces(pdxe_lsp_surface_row_t *rows, int count);


/* --- import-target resolution (vendored subset) ---------------------------- */
/*
 * The resolver that turns one import into the graph node it names, and the map of
 * declared namespaces it consults. Declared here as the reference declares them;
 * implemented by the vendored subset in src/resolve/import_resolver.c.
 *
 * The namespace map must be built from every file of the project. A file missing
 * from it does not fail to resolve: it resolves through a looser fallback, so an
 * incomplete map changes the graph rather than merely shrinking it.
 */
const pdxe_gbuf_node_t *pdxe_pipeline_resolve_import_node(const pdxe_pipeline_ctx_t *ctx,
                                                          const char *source_rel,
                                                          const char *source_file_qn,
                                                          const PDXEImport *imp,
                                                          PDXEHashTable *namespace_map);
PDXEHashTable *pdxe_pipeline_namespace_map_build(const char *project_name,
                                                 PDXEFileResult *const *results,
                                                 const char *const *rels, int count);
void pdxe_pipeline_namespace_map_free(PDXEHashTable *map);

/* The package map, consulted by the module resolver. */
PDXEHashTable *pdxe_pipeline_get_pkgmap(void);

/* Cross-file typed resolution: writes each file result's resolved calls. */
int pdxe_pipeline_pass_lsp_cross(pdxe_pipeline_ctx_t *ctx, const pdxe_file_info_t *files,
                                 int file_count, PDXEFileResult **cache);

#ifdef __cplusplus
}
#endif

#endif /* PDXE_PIPELINE_INTERNAL_H */
