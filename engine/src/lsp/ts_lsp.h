#ifndef PDXE_LSP_TS_LSP_H
#define PDXE_LSP_TS_LSP_H

#include "type_rep.h"
#include "scope.h"
#include "type_registry.h"
#include "../pdxe_core.h"
#include "go_lsp.h" // for PDXELSPDef, PDXEResolvedCallArray (shared cross-language)

// TSLSPContext holds state for TypeScript / JavaScript / JSX / TSX expression type
// evaluation within a single file. One context per pdxe_run_ts_lsp invocation.
//
// Mode flags choose dialect behaviour:
//   - js_mode:  .js / .jsx — JSDoc inference is the primary type source; missing annotations
//               default to UNKNOWN rather than failing.
//   - jsx_mode: .jsx / .tsx — JSX expressions are recognised; intrinsic elements map to
//               builtin types; component calls register through the registry.
//   - dts_mode: .d.ts ambient declarations — no function bodies; populate registry only,
//               emit no resolved calls.
//
// Modes are independent: a `.tsx` file sets jsx_mode=true with js_mode=false; a `.jsx` file
// sets both true.
typedef struct {
    PDXEArena *arena;
    const char *source;
    int source_len;
    const PDXETypeRegistry *registry;
    PDXEScope *current_scope;

    // Import map: local_name -> module QN (resolved or opaque).
    // Parallel arrays of length import_count.
    const char **import_local_names;
    const char **import_module_qns;
    int import_count;

    // File / surrounding context.
    const char *module_qn;          // QN of this file's module
    const char *enclosing_func_qn;  // current function being walked, NULL at module scope
    const char *enclosing_class_qn; // current class for `this` resolution

    // Output: resolved calls accumulate here.
    PDXEResolvedCallArray *resolved_calls;
    // Optional raw usages owned by the per-file extraction result. The scope
    // pass marks occurrence-exact module/import shadows without emitting a
    // semantic reference record for them.
    PDXEUsageArray *usages;

    // Type-parameter scope (innermost generic function/class).
    // type_param_constraints may be NULL or shorter — entries default to "any".
    const char **type_param_names;
    const PDXEType **type_param_constraints;
    int type_param_count;

    // Mode flags — see comment above.
    bool js_mode;
    bool jsx_mode;
    bool dts_mode;
    bool cross_file_mode; // project-wide registry is available for exact reference proof
    bool strict; // tsconfig "strict": true → fewer implicit-any fallbacks
    bool debug;  // PDXE_LSP_DEBUG env

    // Recursion guard for ts_eval_expr_type (mirrors c_lsp).
    int eval_depth;
    // Expression-type memo: node.id -> evaluated type (lazily created on the
    // first completed eval; see TsEvalMemo in ts_lsp.c). Kills the exponential
    // re-evaluation of shared subexpressions under overload resolution.
    struct TsEvalMemo *eval_memo;
    // Recursion guard for lookup_member_type: cyclic type graphs (mutually
    // recursive unions/wrappers across registered types) otherwise recurse
    // without bound — stack overflow on real repos.
    int member_depth;
} TSLSPContext;

#ifdef PDXE_ENABLE_TEST_SEAMS
// Complexity-regression seam: remaining expression-eval budget / warned flag
// for the calling thread, valid after a pdxe_run_ts_lsp on the same thread.
long pdxe_ts_lsp_test_budget_remaining(void);
bool pdxe_ts_lsp_test_budget_warned(void);
#endif

// --- Initialization ---

// Initialise a TSLSPContext for processing one file. Mode flags select dialect.
void ts_lsp_init(TSLSPContext *ctx, PDXEArena *arena, const char *source, int source_len,
                 const PDXETypeRegistry *registry, const char *module_qn, bool js_mode,
                 bool jsx_mode, bool dts_mode, PDXEResolvedCallArray *out);

// Register an import: local binding name → module QN (or unresolved module specifier).
void ts_lsp_add_import(TSLSPContext *ctx, const char *local_name, const char *module_qn);

// Walk the entire file: bind module-level declarations, then process every function /
// method body. Safe on any tree-sitter root; emits zero calls in dts_mode.
void ts_lsp_process_file(TSLSPContext *ctx, TSNode root);

// --- Internals exposed for tests and stdlib data ---

// Evaluate the type of a TS/JS expression node. Returns pdxe_type_unknown() on miss.
const PDXEType *ts_eval_expr_type(TSLSPContext *ctx, TSNode node);

// Parse a TS/JS type-position AST node into a PDXEType.
const PDXEType *ts_parse_type_node(TSLSPContext *ctx, TSNode node);

// Process a single statement node, binding any variables it declares into ctx->current_scope.
void ts_process_statement(TSLSPContext *ctx, TSNode node);

// --- Entry points ---

// Single-file LSP. Builds a registry from result->defs + the TS stdlib subset, runs
// resolution, and writes resolved calls into result->resolved_calls.
void pdxe_run_ts_lsp(PDXEArena *arena, PDXEFileResult *result, const char *source, int source_len,
                    TSNode root, bool js_mode, bool jsx_mode, bool dts_mode);

// Cross-file LSP. Caller passes in cross-file definitions (PDXELSPDef list) and the
// resolved import → module-QN map. Re-parses source if cached_tree is NULL.
void pdxe_run_ts_lsp_cross(PDXEArena *arena, const char *source, int source_len,
                          const char *module_qn, bool js_mode, bool jsx_mode, bool dts_mode,
                          PDXELSPDef *defs, int def_count, const char **import_names,
                          const char **import_qns, int import_count, TSTree *cached_tree,
                          PDXEResolvedCallArray *out);

// Tier 2: build a project-wide TS/JS/TSX registry ONCE from all defs
// (filters by lang). Shared READ-ONLY base; per-file overlays chain to
// it via the registry fallback pointer.
PDXETypeRegistry *pdxe_ts_build_cross_registry(PDXEArena *arena, PDXELSPDef *defs, int def_count);

// Tier 2 per-file resolve. Builds a small per-file overlay (the file's
// own-module defs, AST-refined) that chains to the shared base `reg`.
// `defs`/`def_count` are the file's relevant defs (own + imports); only
// own-module ones are registered into the overlay.
void pdxe_run_ts_lsp_cross_with_registry(PDXEArena *arena, const char *source, int source_len,
                                        const char *module_qn, bool js_mode, bool jsx_mode,
                                        bool dts_mode, PDXETypeRegistry *reg, PDXELSPDef *defs,
                                        int def_count, const char **import_names,
                                        const char **import_qns, int import_count,
                                        TSTree *cached_tree, PDXEResolvedCallArray *out);

// Register the TypeScript / JavaScript stdlib subset (Promise, Array<T>, Map<K,V>, Set<T>,
// Object, Function, console, JSON) into a registry. v1 is hand-curated; a generator script
// will replace this in v1.3.
void pdxe_ts_stdlib_register(PDXETypeRegistry *reg, PDXEArena *arena);

// TEST HOOKS: count of full per-file cross-registry builds (the quadratic the
// shared-registry dispatch eliminates; must stay 0 on the shared path).
long pdxe_ts_full_registry_builds(void);
void pdxe_ts_full_registry_builds_reset(void);

// --- Batch cross-file LSP ---

// Per-file input for batch TS LSP processing.
typedef struct {
    const char *source;
    int source_len;
    const char *module_qn;
    bool js_mode;
    bool jsx_mode;
    bool dts_mode;
    TSTree *cached_tree; // from TSTree caching (NULL = parse internally)
    PDXELSPDef *defs;     // combined file-local + cross-file defs
    int def_count;
    const char **import_names; // parallel arrays, import_count long
    const char **import_qns;
    int import_count;
} PDXEBatchTSLSPFile;

// Process multiple TS/JS files' cross-file LSP in one CGo call.
// out must point to file_count pre-zeroed PDXEResolvedCallArray structs.
// Project-scope declaration merging happens here (per plan §17 finding #4).
void pdxe_batch_ts_lsp_cross(PDXEArena *arena, PDXEBatchTSLSPFile *files, int file_count,
                            PDXEResolvedCallArray *out);

#endif // PDXE_LSP_TS_LSP_H
