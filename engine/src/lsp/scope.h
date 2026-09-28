#ifndef PDXE_LSP_SCOPE_H
#define PDXE_LSP_SCOPE_H

#include "type_rep.h"
#include "../arena.h"
#include <stdatomic.h> /* relaxed cache for pdxe_lsp_max_walk_depth */
#include <stdlib.h>     /* getenv, atoi (pdxe_lsp_max_walk_depth) */

typedef struct {
    const char* name;
    const PDXEType* type;
    /* Exact callable value carried by this lexical binding, or NULL when the
     * binding is not proven to denote one callable.  This is deliberately
     * identity metadata rather than another PDXEType kind: aliases need both
     * their ordinary type and the graph QN of the value they reference. */
    const char *callable_qn;
} PDXEVarBinding;

#define PDXE_SCOPE_CHUNK_BINDINGS 16

typedef struct PDXEScopeChunk {
    PDXEVarBinding bindings[PDXE_SCOPE_CHUNK_BINDINGS];
    int used;
    struct PDXEScopeChunk* next;
} PDXEScopeChunk;

typedef struct PDXEScope {
    struct PDXEScope* parent;
    PDXEScopeChunk* chunks;
    PDXEArena* arena;        // owning arena, propagated to children at push time
} PDXEScope;

// Bail-to-UNKNOWN depth for type-lookup chains: alias resolution, MRO walks,
// embedded-field/struct-traversal. Exceeding this collapses to pdxe_type_unknown
// rather than recursing — guards against pathological hierarchies.
#define PDXE_LSP_MAX_LOOKUP_DEPTH 16

// Recursion cap for the per-language "resolve calls in AST node" walkers. These
// recurse once per AST nesting level; a deeply-nested or cyclic file can drive
// them into a native stack overflow (SIGSEGV) that takes down the whole index.
// Past this cap the wrapper skips the subtree — those calls stay unresolved,
// which is graceful degradation, not a crash. 512 is far deeper than any
// hand-written source nests; override for pathological/generated repos via the
// PDXE_LSP_MAX_WALK_DEPTH env var (positive integer).
#define PDXE_LSP_MAX_WALK_DEPTH 512

// Resolved walk-depth cap: env override (PDXE_LSP_MAX_WALK_DEPTH, if a positive
// integer) else PDXE_LSP_MAX_WALK_DEPTH. Read once and cached — the walkers call
// this per node, so it must not hit getenv on the hot path. The cache is
// idempotent under multi-threaded indexing (every worker computes the same
// value), but a plain data race is undefined behavior even when the values
// agree, so the slot is a relaxed atomic: on the hot path this is a plain load
// with no fence, and a first-touch double-compute simply stores the same
// value. This keeps the parallel extractor TSan-clean.
static inline int pdxe_lsp_max_walk_depth(void) {
    static _Atomic int cached = -1;
    int value = atomic_load_explicit(&cached, memory_order_relaxed);
    if (value < 0) {
        const char* e = getenv("PDX_ENGINE_LSP_MAX_WALK_DEPTH");
        int v = (e && *e) ? atoi(e) : 0;
        value = (v > 0) ? v : PDXE_LSP_MAX_WALK_DEPTH;
        atomic_store_explicit(&cached, value, memory_order_relaxed);
    }
    return value;
}

PDXEScope* pdxe_scope_push(PDXEArena* a, PDXEScope* current);
PDXEScope* pdxe_scope_pop(PDXEScope* scope);
void pdxe_scope_bind(PDXEScope* scope, const char* name, const PDXEType* type);
/* Checked forms: false when the binding could not be recorded in THIS frame
 * (arena exhaustion). The void forms above discard that and return silently,
 * which lets a caller that then does a scope-CHAIN lookup see a PARENT binding
 * of the same name and believe the child was bound -- fabricating callable
 * proof from a shadow that never took effect. Use these, and read the local
 * result, wherever a failed bind must not be mistaken for success. */
bool pdxe_scope_bind_checked(PDXEScope *scope, const char *name, const PDXEType *type);
bool pdxe_scope_bind_callable_checked(PDXEScope *scope, const char *name, const PDXEType *type,
                                     const char *callable_qn);
/* Bind a value whose identity is one exact callable.  A later ordinary
 * pdxe_scope_bind of the same name clears this identity, so reassignment fails
 * closed instead of leaking a stale alias target. */
void pdxe_scope_bind_callable(PDXEScope *scope, const char *name, const PDXEType *type,
                             const char *callable_qn);
const PDXEType* pdxe_scope_lookup(const PDXEScope* scope, const char* name);
/* True when any lexical frame contains name, even when its type is UNKNOWN. */
bool pdxe_scope_contains(const PDXEScope *scope, const char *name);
/* Return the exact callable QN from the nearest binding.  A nearer ordinary
 * binding shadows a parent's callable and therefore returns NULL. */
const char *pdxe_scope_lookup_callable(const PDXEScope *scope, const char *name);
/* Replace (or clear with NULL) callable identity on the nearest existing
 * lexical binding. Returns false when name is unbound. This is for assignment;
 * declarations should continue to use pdxe_scope_bind[_callable]. */
bool pdxe_scope_update_callable(PDXEScope *scope, const char *name, const char *callable_qn);

#endif // PDXE_LSP_SCOPE_H
