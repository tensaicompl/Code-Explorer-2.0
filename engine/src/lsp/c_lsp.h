#ifndef PDXE_LSP_C_LSP_H
#define PDXE_LSP_C_LSP_H

#include "type_rep.h"
#include "scope.h"
#include "type_registry.h"
#include "../pdxe_core.h"
#include "go_lsp.h" // for PDXELSPDef, PDXEResolvedCallArray

// CLSPContext holds state for C/C++ expression type evaluation within a file.
typedef struct {
    PDXEArena *arena;
    const char *source;
    int source_len;
    const PDXETypeRegistry *registry;
    /* The writable head of the registry chain: the per-file overlay the
     * dispatcher hands in, or the per-file registry on the non-cross path.
     * Every refinement (lazy add, min_params, template params) goes here
     * via pdxe_registry_*_for_update; nothing behind ->fallback is written. */
    PDXETypeRegistry *registry_head;
    PDXEScope *current_scope;

    // Include map: header_path -> namespace QN prefix
    const char **include_paths;
    const char **include_ns_qns;
    int include_count;

    // Namespace state
    const char *current_namespace; // current namespace QN (e.g., "proj.ns1.ns2")

    // Using namespace directives
    const char **using_namespaces;
    int using_ns_count;
    int using_ns_cap;

    // Using declarations: specific names imported into scope
    const char **using_decl_names;
    const char **using_decl_qns;
    int using_decl_count;
    int using_decl_cap;

    // Namespace aliases: short -> full QN
    const char **ns_alias_names;
    const char **ns_alias_qns;
    int ns_alias_count;
    int ns_alias_cap;

    // Current context
    const char *enclosing_func_qn;
    const char *enclosing_class_qn; // for implicit `this` resolution
    const char *module_qn;
    size_t module_qn_len; // cached strlen(module_qn); for stack-buffer QN building

    // Negative-lookup memo for c_lookup_member_depth (depth==0 misses only).
    // Open-addressing uint64 hash SET; 0 is the empty-slot sentinel. Populated
    // ONLY when the Tier-2 registry is shared+read-only (registry_shared), where
    // the module-prefix/base-class/short-name cascades are pure, stable functions
    // of (type_qn, registry) — so a recorded miss can never turn into a hit. Lets
    // the hot resolve path skip the sprintf("%s.%s") strlen storm + the O(type_count)
    // short-name scan on repeated misses of the same (type_qn, member). malloc-owned;
    // freed at end of c_lsp_process_file.
    uint64_t *neg_memo;
    int neg_memo_cap;   // power-of-two; 0 until first insert
    int neg_memo_count; // live entries (grow by rehash at 70% load)

    // Output
    PDXEResolvedCallArray *resolved_calls;
    PDXESourceOrigin source_origin; // source buffer represented by emitted occurrence spans

    // Function pointer targets: lexical binding -> exact target function QN.
    // A NULL target is an explicitly unknown/ambiguous binding and must shadow
    // any outer exact target of the same name.
    const char **fp_var_names;
    const char **fp_target_qns;
    const PDXEScope **fp_binding_scopes;
    int fp_count;
    int fp_cap;

    // Template parameter defaults for current template scope
    const char **template_param_names;       // e.g., ["T", "U"]
    const PDXEType **template_param_defaults; // e.g., [int_type, NULL]
    int template_param_count;

    // Pending template calls: member calls on TYPE_PARAM inside template functions.
    // At call sites with known arg types, these are resolved retroactively.
    struct {
        const char *func_qn;     // enclosing template function
        const char *type_param;  // e.g., "T"
        const char *method_name; // e.g., "draw"
        int arg_count;
    } *pending_template_calls;
    int pending_tc_count;
    int pending_tc_cap;

    // Flags
    bool cpp_mode;        // C++ features enabled
    bool in_template;     // currently inside template declaration
    bool registry_shared; // ctx->registry is the Tier-2 cross registry, shared
                          // READ-ONLY across resolve workers — never mutate it
                          // (and never store per-worker arena pointers into it)
    bool debug;
    int eval_depth;         // recursion depth for c_eval_expr_type (crash guard)
    int eval_steps;         // total expression eval calls for current file (hang guard)
    int walk_depth;         // c_resolve_calls_in_node self-recursion (AST nesting)
    int control_flow_depth; // if/loop/switch/catch nesting; assignments merge fail-closed
} CLSPContext;

// --- API ---

void c_lsp_init(CLSPContext *ctx, PDXEArena *arena, const char *source, int source_len,
                const PDXETypeRegistry *registry, const char *module_qn, bool cpp_mode,
                PDXEResolvedCallArray *out);

void c_lsp_add_include(CLSPContext *ctx, const char *header_path, const char *ns_qn);

void c_lsp_process_file(CLSPContext *ctx, TSNode root);

const PDXEType *c_eval_expr_type(CLSPContext *ctx, TSNode node);
const PDXEType *c_parse_type_node(CLSPContext *ctx, TSNode node);
void c_process_statement(CLSPContext *ctx, TSNode node);

// Look up a member (method/field) on a type, traversing base classes.
const PDXERegisteredFunc *c_lookup_member(CLSPContext *ctx, const char *type_qn,
                                         const char *member_name);

// Type simplification: unwrap refs, aliases, pointers (like clangd simplifyType).
const PDXEType *c_simplify_type(CLSPContext *ctx, const PDXEType *t, bool unwrap_pointer);

// --- Entry points ---

// Single-file LSP: build registry from file defs + stdlib, run resolution.
void pdxe_run_c_lsp(PDXEArena *arena, PDXEFileResult *result, const char *source, int source_len,
                   TSNode root, bool cpp_mode, PDXESourceOrigin source_origin);

// Cross-file LSP: build registry from defs + stdlib, re-parse and resolve.
void pdxe_run_c_lsp_cross(PDXEArena *arena, const char *source, int source_len, const char *module_qn,
                         bool cpp_mode, PDXELSPDef *defs, int def_count, const char **include_paths,
                         const char **include_ns_qns, int include_count,
                         TSTree *cached_tree, // NULL = parse internally
                         PDXEResolvedCallArray *out);

// Tier 2: build a project-wide C/C++/CUDA registry ONCE from all defs
// (filters by lang). Shared READ-ONLY across resolve workers. Def-driven
// (no AST field collection) → identical entries to the per-file build.
PDXETypeRegistry *pdxe_c_build_cross_registry(PDXEArena *arena, PDXELSPDef *defs, int def_count);

// Cross-file LSP using a pre-built shared registry (Tier 2). Skips the
// per-file registry build; just parse + resolve.
void pdxe_run_c_lsp_cross_with_registry(PDXEArena *arena, const char *source, int source_len,
                                       const char *module_qn, bool cpp_mode,
                                       PDXETypeRegistry *reg, // pre-built, finalized, READ-ONLY
                                       const char **include_paths, const char **include_ns_qns,
                                       int include_count,
                                       TSTree *cached_tree, // NULL = parse internally
                                       PDXEResolvedCallArray *out);

// Register C stdlib types and functions into a registry.
void pdxe_c_stdlib_register(PDXETypeRegistry *reg, PDXEArena *arena);

// Register C++ stdlib types and functions into a registry.
void pdxe_cpp_stdlib_register(PDXETypeRegistry *reg, PDXEArena *arena);

// --- Batch cross-file LSP ---

// Per-file input for batch C/C++ LSP processing.
typedef struct {
    const char *source;
    int source_len;
    const char *module_qn;
    bool cpp_mode;
    TSTree *cached_tree; // from TSTree caching (NULL = parse internally)
    PDXELSPDef *defs;     // combined file-local + cross-file defs
    int def_count;
    const char **include_paths; // parallel arrays, include_count long
    const char **include_ns_qns;
    int include_count;
} PDXEBatchCLSPFile;

// Process multiple C/C++ files' cross-file LSP in one CGo call.
// out must point to file_count pre-zeroed PDXEResolvedCallArray structs.
void pdxe_batch_c_lsp_cross(PDXEArena *arena, PDXEBatchCLSPFile *files, int file_count,
                           PDXEResolvedCallArray *out);

#endif // PDXE_LSP_C_LSP_H
