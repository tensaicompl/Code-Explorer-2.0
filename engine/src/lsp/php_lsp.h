#ifndef PDXE_LSP_PHP_LSP_H
#define PDXE_LSP_PHP_LSP_H

#include "type_rep.h"
#include "scope.h"
#include "type_registry.h"
#include "../pdxe.h"
#include "go_lsp.h"  /* PDXELSPDef reused across languages */

/* PHPLSPContext — per-file state for PHP type-aware call resolution.
 * Mirrors GoLSPContext / CLSPContext structure. */
typedef struct {
    PDXEArena *arena;
    const char *source;
    int source_len;
    const PDXETypeRegistry *registry;
    PDXEScope *current_scope;

    /* Namespace state. PHP files declare a single namespace
     * (or use the global namespace if none); empty string means global. */
    const char *current_namespace_qn;

    /* `use` clause map.
     * use_kinds[i] selects whether the local maps a class, function, or const. */
    const char **use_local_names;
    const char **use_target_qns;
    enum { PDXE_PHP_USE_CLASS = 0, PDXE_PHP_USE_FUNCTION, PDXE_PHP_USE_CONST } *use_kinds;
    int use_count;
    int use_cap;

    /* Current function/method/class context. */
    const char *enclosing_func_qn;
    const char *enclosing_class_qn; /* NULL outside class body */
    const char *enclosing_parent_qn; /* parent class QN (for parent::), or NULL */
    const char *module_qn;

    /* Output: resolved calls accumulate here. */
    PDXEResolvedCallArray *resolved_calls;

    /* @phpstan-type alias map (per-file, populated from class docblocks).
     * Used by resolve_phpdoc_type before generic name resolution so user
     * type aliases like `@phpstan-type UserId int|string` and references
     * to `UserId` in @var/@param/@return all resolve to the aliased type.
     */
    const char **phpstan_alias_names;  /* arena-allocated, NULL-terminated */
    const PDXEType **phpstan_alias_types;
    int phpstan_alias_count;
    int phpstan_alias_cap;

    /* Recursion guard for php_eval_expr_type. */
    int eval_depth;

    /* AST-walk recursion depth for php_resolve_calls_in_node (guards stack
     * overflow on deeply-nested/cyclic files; see pdxe_lsp_max_walk_depth).
     * Zero via memset. */
    int walk_depth;

    /* Debug mode (PDXE_LSP_DEBUG env). */
    bool debug;
} PHPLSPContext;

/* Initialize a PHPLSPContext for processing one file. */
void php_lsp_init(PHPLSPContext *ctx, PDXEArena *arena, const char *source, int source_len,
                  const PDXETypeRegistry *registry, const char *module_qn,
                  PDXEResolvedCallArray *out);

/* Add a `use` mapping. */
void php_lsp_add_use(PHPLSPContext *ctx, const char *local_name, const char *target_qn,
                     int use_kind);

/* Process a file's AST: walk top-level decls, then function/method bodies. */
void php_lsp_process_file(PHPLSPContext *ctx, TSNode root);

/* Refine an already-populated registry from local class ASTs: declared and
 * PHPDoc returns, trait flattening, and typed fields. */
void pdxe_php_refine_lsp_registry(PHPLSPContext *ctx, PDXETypeRegistry *reg, TSNode root);

/* Evaluate a PHP expression's type. May return NULL / PDXE_TYPE_UNKNOWN. */
const PDXEType *php_eval_expr_type(PHPLSPContext *ctx, TSNode node);

/* Parse a PHP type-AST node (named_type, primitive_type, union_type, ...) to PDXEType. */
const PDXEType *php_parse_type_node(PHPLSPContext *ctx, TSNode node);

/* Resolve a class name (bare or qualified) using current namespace + use map. */
const char *php_resolve_class_name(PHPLSPContext *ctx, const char *name);

/* Look up a method on a class, walking parent chain (registry-based). */
const PDXERegisteredFunc *php_lookup_method(PHPLSPContext *ctx, const char *class_qn,
                                            const char *method_name);

/* Entry point: build registry from file defs + stdlib + composer (if present),
 * then run resolution. Called from pdxe_extract_file(). */
void pdxe_run_php_lsp(PDXEArena *arena, PDXEFileResult *result, const char *source, int source_len,
                    TSNode root);

/* Register PHP stdlib + curated framework types into a registry. */
void pdxe_php_stdlib_register(PDXETypeRegistry *reg, PDXEArena *arena);

/* --- Cross-file LSP resolution ---
 *
 * Mirrors pdxe_run_py_lsp_cross / pdxe_run_ts_lsp_cross. Caller supplies the
 * combined PDXELSPDef[] (file-local + cross-file) and a resolved import map
 * (use → target QN). Imports are added as CLASS-kind uses; file-internal
 * `use` declarations from the AST are layered on top by process_file.
 *
 * Reuses go_lsp.h's PDXELSPDef so cross-language registration is uniform. */
void pdxe_php_register_lsp_defs(PDXEArena *arena, PDXEArena *idx_arena, PDXETypeRegistry *reg,
                               const PDXELSPDef *defs, int def_count);

void pdxe_run_php_lsp_cross(
    PDXEArena *arena,
    const char *source, int source_len,
    const char *module_qn,
    PDXELSPDef *defs, int def_count,
    const char **import_names, const char **import_qns, int import_count,
    TSTree *cached_tree,           /* NULL = parse internally */
    PDXEResolvedCallArray *out);

/* --- Batch cross-file LSP --- */

/* Per-file input for batch PHP LSP processing. */
typedef struct {
    const char *source;
    int source_len;
    const char *module_qn;
    TSTree *cached_tree;            /* NULL = parse internally */
    PDXELSPDef *defs;                /* combined file-local + cross-file defs */
    int def_count;
    const char **import_names;      /* parallel arrays, import_count long */
    const char **import_qns;
    int import_count;
} PDXEBatchPHPLSPFile;

/* Process multiple PHP files' cross-file LSP in one call. out must point to
 * file_count pre-zeroed PDXEResolvedCallArray structs. */
void pdxe_batch_php_lsp_cross(
    PDXEArena *arena,
    PDXEBatchPHPLSPFile *files, int file_count,
    PDXEResolvedCallArray *out);

#endif /* PDXE_LSP_PHP_LSP_H */
