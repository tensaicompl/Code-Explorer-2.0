#ifndef PDXE_LSP_PERL_LSP_H
#define PDXE_LSP_PERL_LSP_H

#include "type_rep.h"
#include "scope.h"
#include "type_registry.h"
#include "../pdxe_core.h"
#include "go_lsp.h" /* PDXELSPDef reused across languages */

/* PerlLSPContext — per-file state for Perl type-aware call resolution.
 * Mirrors PHPLSPContext / GoLSPContext / CLSPContext structure.
 *
 * Perl differs from PHP in a few ways that shape this context:
 *   - Packages (`package Foo::Bar;`) replace PHP namespaces. A file may
 *     contain several packages; current_package_qn tracks the active one.
 *   - Inheritance is expressed via the `@ISA` array (or `use parent`/
 *     `use base`), not a class keyword, so we keep an @ISA table.
 *   - `bless` associates a reference variable with a class at runtime;
 *     we track a bless var→class map for method dispatch.
 *   - Exporter-style imports (`use Foo qw(bar baz)`) populate an export
 *     import map (local name → target QN) analogous to PHP `use`. */
typedef struct {
    PDXEArena *arena;
    const char *source;
    int source_len;
    const PDXETypeRegistry *registry;
    PDXEScope *current_scope;

    /* Package state. A Perl file may declare multiple packages; this is the
     * QN of the package currently in effect (dotted form). Empty string means
     * the default `main` / file-level package. */
    const char *current_package_qn;

    /* Export import map (Exporter / `use Foo qw(...)`).
     * use_local_names[i] is the symbol as referenced in this file;
     * use_target_qns[i] is the fully-qualified target it resolves to. */
    const char **use_local_names;
    const char **use_target_qns;
    int use_count;
    int use_cap;

    /* @ISA inheritance table: isa_pkg_qns[i] inherits from isa_parent_qns[i].
     * Populated from @ISA assignments and `use parent`/`use base`. */
    const char **isa_pkg_qns;
    const char **isa_parent_qns;
    int isa_count;
    int isa_cap;

    /* bless var→class map: blessed_var_names[i] holds a reference blessed
     * into class blessed_class_qns[i], so $self->method() can dispatch. */
    const char **blessed_var_names;
    const char **blessed_class_qns;
    int blessed_count;
    int blessed_cap;

    /* Current package/sub context. */
    const char *enclosing_package_qn; /* package QN of the enclosing scope */
    const char *enclosing_parent_qn;  /* parent class QN (for SUPER::), or NULL */
    const char *enclosing_func_qn;    /* enclosing sub QN, or NULL */
    const char *module_qn;

    /* Output: resolved calls accumulate here. */
    PDXEResolvedCallArray *resolved_calls;
    PDXEUsageArray *usages;

    /* Recursion guard for perl_eval_expr_type. */
    int eval_depth;

    /* Recursion guard for the AST-walk passes (perl_resolve_calls_in_node /
     * perl_pass1_scan). Bounds stack depth on pathologically nested input. */
    int walk_depth;

    /* Debug mode (PDXE_LSP_DEBUG env). */
    bool debug;
} PerlLSPContext;

/* Initialize a PerlLSPContext for processing one file. */
void perl_lsp_init(PerlLSPContext *ctx, PDXEArena *arena, const char *source, int source_len,
                   const PDXETypeRegistry *registry, const char *module_qn,
                   PDXEResolvedCallArray *out);

/* Add an export/`use` mapping (local name → target QN). */
void perl_lsp_add_use(PerlLSPContext *ctx, const char *local_name, const char *target_qn);

/* Process a file's AST: walk package decls + sub bodies, resolve calls. */
void perl_lsp_process_file(PerlLSPContext *ctx, TSNode root);

/* Evaluate a Perl expression's type. May return NULL / PDXE_TYPE_UNKNOWN. */
const PDXEType *perl_eval_expr_type(PerlLSPContext *ctx, TSNode node);

/* Resolve a package/class name (bare or qualified) using current package +
 * the export import map. */
const char *perl_resolve_package_name(PerlLSPContext *ctx, const char *name);

/* Look up a method on a package, walking the @ISA chain (registry-based). */
const PDXERegisteredFunc *perl_lookup_method(PerlLSPContext *ctx, const char *package_qn,
                                            const char *method_name);

/* Entry point: build registry from file defs + stdlib, then run resolution.
 * Called from pdxe_extract_file() via the language dispatch in pdxe.c. */
void pdxe_run_perl_lsp(PDXEArena *arena, PDXEFileResult *result, const char *source, int source_len,
                      TSNode root);

/* Register Perl stdlib (perlfunc builtins) + curated CPAN types into a
 * registry. */
void pdxe_perl_stdlib_register(PDXETypeRegistry *reg, PDXEArena *arena);

/* --- Cross-file LSP resolution ---
 *
 * Mirrors pdxe_run_php_lsp_cross / pdxe_run_py_lsp_cross. Stub-declared here so
 * a later plan (Phase 23, cross-file) can implement it without touching the
 * wiring. Caller supplies the combined PDXELSPDef[] (file-local + cross-file)
 * and a resolved import map (use → target QN). */
void pdxe_run_perl_lsp_cross(PDXEArena *arena, const char *source, int source_len,
                            const char *module_qn, PDXELSPDef *defs, int def_count,
                            const char **import_names, const char **import_qns, int import_count,
                            TSTree *cached_tree, /* NULL = parse internally */
                            PDXEResolvedCallArray *out);

#endif /* PDXE_LSP_PERL_LSP_H */
