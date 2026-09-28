/*
 * pass_lsp_cross.h — Cross-file LSP helpers shared with the parallel
 * resolve pass.
 *
 * Per-file LSP (pdxe_run_X_lsp inside pdxe_extract_file) only sees a single
 * file's defs in its registry, so callees whose receiver type comes from
 * an imported module stay unresolved. The helpers declared here close
 * that gap: they let the parallel resolve worker (pass_parallel.c) build
 * a project-wide PDXELSPDef[] and invoke the language-specific
 * pdxe_run_X_lsp_cross resolver on each file using the file's already-
 * built import map. Resolved calls are appended to result->resolved_calls
 * so the same pdxe_pipeline_find_lsp_resolution path that handles per-
 * file LSP picks them up.
 *
 * Languages covered: Go, C/C++/CUDA, Python, TypeScript/JavaScript/JSX/
 * TSX, PHP, C#, and JVM (Java/Kotlin via the shared filter helper).
 * Anything else short-circuits via pdxe_pxc_has_cross_lsp.
 *
 * Previously this work ran as a separate sequential pipeline pass
 * (pdxe_pipeline_pass_lsp_cross) that re-read every source file from
 * disk and re-parsed each tree-sitter tree on a single thread — a 50×
 * regression vs the parallel extract pass on large repos. The pass was
 * deleted; the resolve worker now invokes these helpers directly using
 * the source bytes retained in result->arena during extract.
 */
#ifndef PDXE_PIPELINE_PASS_LSP_CROSS_H
#define PDXE_PIPELINE_PASS_LSP_CROSS_H

#include "pdxe_core.h"
/* PDXELSPDef historically lives in lsp/go_lsp.h (not lsp/type_rep.h)
 * — type_rep.h covers the type-representation primitives while
 * go_lsp.h was where the project-wide def descriptor landed first. */
#include "lsp/go_lsp.h"
#include "lsp/py_lsp.h"   /* pdxe_py_build_cross_registry / pdxe_run_py_lsp_cross_with_registry */
#include "lsp/c_lsp.h"    /* pdxe_c_build_cross_registry / pdxe_run_c_lsp_cross_with_registry */
#include "lsp/cs_lsp.h"   /* pdxe_cs_build_cross_registry / pdxe_run_cs_lsp_cross_with_registry */
#include "lsp/ts_lsp.h"   /* pdxe_ts_build_cross_registry / pdxe_run_ts_lsp_cross_with_registry */
#include "lsp/java_lsp.h" /* pdxe_java_build_cross_registry / pdxe_run_java_lsp_cross_with_registry */
#include "lsp/rust_lsp.h" /* pdxe_rust_build_cross_registry / pdxe_run_rust_lsp_cross_with_registry */
#include "pipeline/pipeline_internal.h"
#include <stdbool.h>

/* True iff this language has a pdxe_run_X_lsp_cross resolver wired up. */
bool pdxe_pxc_has_cross_lsp(PDXELanguage lang);

/* Collect a project-wide PDXELSPDef[] from every cached file result.
 * def_modules[i] receives the module QN for files[i] (malloc'd; the
 * caller frees each entry then the array). String fields in the
 * returned PDXELSPDef[] are borrowed from cache[i]->arena and from
 * def_modules[i] — caller must keep both alive while the array is in
 * use. Returns the malloc'd array (free() it) and writes the entry
 * count to *out_count. Returns NULL on alloc failure or when no defs
 * exist. out_def_starts (optional, file_count + 1 entries, caller-owned)
 * receives per-file prefix offsets: file i's defs occupy
 * [out_def_starts[i], out_def_starts[i+1]) — the LSP-surface serializer
 * needs the per-file slices, which the flat array does not otherwise
 * record.
 *
 * `ctx` (nullable) enables cross-file base-class resolution: for the
 * languages whose cross registrars read embedded_types as qualified names
 * (Python, JS/TS/TSX), every PDXEDefinition.base_classes spelling is resolved
 * to a project QN through ctx->registry plus the file's import map — the same
 * inputs pass_semantic uses to draw its INHERITS edge, so the LSP's
 * inheritance view and the graph's cannot diverge. Pass NULL to keep the raw
 * source spelling (surface-probe paths that build no registry). */
PDXELSPDef *pdxe_pxc_collect_all_defs(const pdxe_pipeline_ctx_t *ctx, PDXEArena *arena,
                                    PDXEFileResult **cache, const pdxe_file_info_t *files,
                                    int file_count, const char *project_name, char **def_modules,
                                    int *out_count, int *out_def_starts);

/* Detect TS dialect flags from a relative path. */
void pdxe_pxc_ts_modes(PDXELanguage lang, const char *rel_path, bool *out_js, bool *out_jsx,
                      bool *out_dts);

/* Build the local-name -> semantic import-QN map consumed by cross-file LSPs.
 * Both sequential and parallel drivers use this exact helper so import
 * metadata cannot diverge between pipelines. Values are owned by the returned
 * map (not borrowed from gbuf); release both arrays with
 * pdxe_pxc_free_import_map(). */
int pdxe_pxc_build_import_map(const pdxe_gbuf_t *gbuf, const char *project_name, const char *rel_path,
                             PDXELanguage lang, const PDXEFileResult *result, const char ***out_keys,
                             const char ***out_vals, int *out_count);

void pdxe_pxc_free_import_map(const char **keys, const char **vals, int count);

/* ── Per-module def index (the gopls "package summary" pattern) ──
 *
 * The hot path used to register ALL all_defs[] into a fresh registry
 * per file (~110k defs × 11k files for kubernetes = ~21,000 CPU-s of
 * arena_strdup). Most of those defs are irrelevant to any one file —
 * each file only references defs from its own module + its imported
 * modules. gopls observed the same: it builds per-package summaries
 * and per-file only loads the summaries the file imports.
 *
 * pdxe_pxc_build_module_def_index() builds inverted indexes once (O(D)):
 * def_module_qn → defs and declared namespace/package → defs.
 * pdxe_pxc_filter_defs_for_file() then returns own_module + imp_qns for
 * most languages. For Java/Kotlin callers it additionally returns
 * same-namespace JVM defs so Gradle/Maven mixed source roots
 * (`src/main/java/...` + `src/main/kotlin/...`) resolve same-package
 * references without falling back to a full project registry per file. */
typedef struct PDXEModuleDefIndex PDXEModuleDefIndex;

PDXEModuleDefIndex *pdxe_pxc_build_module_def_index(PDXELSPDef *all_defs, int def_count);

void pdxe_pxc_free_module_def_index(PDXEModuleDefIndex *idx);

/* Return a malloc'd PDXELSPDef[] containing all defs whose
 * def_module_qn matches own_module OR any of imp_qns. For Java/Kotlin
 * callers, also include defs from the same declared package/namespace:
 * JVM same-package references often cross `src/main/java` and
 * `src/main/kotlin` roots without import statements. String fields inside
 * each entry are borrowed from the original all_defs[] arena (caller keeps
 * it alive). Caller frees the returned array with free(). Writes the entry
 * count to *out_count and sets *out_success on every valid selection. A valid
 * empty selection returns NULL with *out_count = 0 and *out_success = true;
 * NULL with *out_success = false means invalid input or allocation failure. */
PDXELSPDef *pdxe_pxc_filter_defs_for_file(const PDXEModuleDefIndex *idx, PDXELSPDef *all_defs,
                                        PDXELanguage caller_lang, const char *caller_namespace,
                                        const char *own_module, const char *const *imp_qns,
                                        int imp_count, int *out_count, bool *out_success);

/* ── Tier 2 full: pre-built per-language cross-LSP registries ─────
 *
 * Each non-NULL registry is built ONCE in pipeline.c (in a dedicated
 * cross_lsp_arena), finalized, and shared READ-ONLY across all
 * resolve workers for files of that language. The worker uses the
 * matching pdxe_run_X_lsp_cross_with_registry variant which skips the
 * per-file registry build entirely. NULL → fall back to the per-file
 * pdxe_pxc_run_one path. */
typedef struct {
    PDXETypeRegistry *go;     /* PDXE_LANG_GO */
    PDXETypeRegistry *c;      /* PDXE_LANG_C, PDXE_LANG_CPP, PDXE_LANG_CUDA */
    PDXETypeRegistry *python; /* PDXE_LANG_PYTHON */
    PDXETypeRegistry *ts;     /* PDXE_LANG_JAVASCRIPT, TYPESCRIPT, TSX */
    PDXETypeRegistry *php;    /* PDXE_LANG_PHP */
    PDXETypeRegistry *cs;     /* PDXE_LANG_CSHARP */
    PDXETypeRegistry *java;   /* PDXE_LANG_JAVA (JVM def universe incl. Kotlin defs) */
    /* PDXE_LANG_RUST: intentionally absent — the shared rust registry is built
     * LAZILY inside pdxe_parallel_resolve (first NULL-filter rust file), not eagerly. */
} PDXECrossLspRegistries;

/* Return the appropriate pre-built registry for a language, or NULL
 * if none was built (or language has no cross-LSP entrypoint). */
/* Per-file registry-build cost (#1669): how many defs the per-file cross-LSP
 * path actually registered, and how often the module filter failed. */
/* Count defs an overlay registered for one file (complexity-gate telemetry). */
void pdxe_pxc_count_perfile_defs(uint64_t defs);

void pdxe_pxc_filter_stats(uint64_t *defs_registered, uint64_t *build_files, uint64_t *filter_files,
                          uint64_t *filter_failed);

static inline PDXETypeRegistry *pdxe_pxc_registry_for_lang(const PDXECrossLspRegistries *r,
                                                         PDXELanguage lang) {
    if (!r) {
        return NULL;
    }
    switch (lang) {
    case PDXE_LANG_GO:
        return r->go;
    case PDXE_LANG_C:   /* fallthrough */
    case PDXE_LANG_CPP: /* fallthrough */
    case PDXE_LANG_CUDA:
        return r->c;
    case PDXE_LANG_PYTHON:
        return r->python;
    case PDXE_LANG_JAVASCRIPT: /* fallthrough */
    case PDXE_LANG_TYPESCRIPT: /* fallthrough */
    case PDXE_LANG_TSX:
        return r->ts;
    case PDXE_LANG_PHP:
        return r->php;
    case PDXE_LANG_CSHARP:
        return r->cs;
    case PDXE_LANG_JAVA:
        return r->java;
    default:
        return NULL; /* incl. PDXE_LANG_RUST — its shared registry is built lazily */
    }
}

/* Build and borrow the Rust Cargo manifest used for cross-crate (#56) routing.
 * The manifest owns strings in manifest_arena; callers keep that arena alive
 * until every resolver worker has joined.  Each worker must install the shared
 * immutable pointer in its own TLS slot before dispatch and clear it afterward. */
struct PDXECargoManifest;
bool pdxe_pxc_build_rust_manifest(const pdxe_pipeline_ctx_t *ctx, PDXEArena *manifest_arena,
                                 struct PDXECargoManifest *out_manifest);
void pdxe_pxc_set_rust_manifest(const struct PDXECargoManifest *manifest);
/* A resolve worker keeps its per-file cross-LSP scratch arenas between files
 * from _begin to _end; _end must run on the same thread before it exits. */
void pdxe_pxc_thread_scratch_begin(void);
void pdxe_pxc_thread_scratch_end(void);
const struct PDXECargoManifest *pdxe_pxc_get_rust_manifest(void);

/* Run the cross-file LSP resolver for non-TS languages. Appends
 * resolved CALLS into r->resolved_calls (lives in r->arena). Caller
 * owns source, module_qn, defs, imp_names, imp_qns.
 * NOTE: defs is read-only in practice but typed non-const to match
 * the existing pdxe_run_X_lsp_cross callee signatures. */
void pdxe_pxc_run_one(PDXELanguage lang, PDXEFileResult *r, const char *source, int source_len,
                     const char *module_qn, PDXELSPDef *defs, int def_count, const char **imp_names,
                     const char **imp_qns, int imp_count);

/* TS / JS / JSX / TSX variant with explicit dialect flags. */
void pdxe_pxc_run_one_ts(PDXEFileResult *r, const char *source, int source_len, const char *module_qn,
                        PDXELSPDef *defs, int def_count, const char **imp_names,
                        const char **imp_qns, int imp_count, bool js_mode, bool jsx_mode,
                        bool dts_mode);

/* Per-file cross-LSP dispatch shared by the parallel resolve worker AND the
 * sequential driver (one path = one semantics): module-def-index filter →
 * shared prebuilt registry (overlay pattern, no per-file registry build) →
 * per-file fallback with FILTERED defs for languages without a shared
 * variant. rust_shared_get (nullable) supplies the lazily-built shared Rust
 * registry for NULL-filter rust files. */
void pdxe_pxc_dispatch_file(PDXELanguage lang, PDXEFileResult *result, const char *source,
                           int source_len, const char *rel, const char *def_module,
                           const PDXECrossLspRegistries *cross_registries,
                           const PDXEModuleDefIndex *module_def_index, PDXELSPDef *all_defs,
                           int all_def_count, const char **imp_keys, const char **imp_vals,
                           int imp_count, PDXETypeRegistry *(*rust_shared_get)(void *),
                           void *rust_shared_ctx);

#endif /* PDXE_PIPELINE_PASS_LSP_CROSS_H */
