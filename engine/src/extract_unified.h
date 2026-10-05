#ifndef PDXE_EXTRACT_UNIFIED_H
#define PDXE_EXTRACT_UNIFIED_H

#include "pdxe_core.h"
#include "lang_specs.h"

// Scope kinds for the walk state stack.
#define SCOPE_FUNC 1
#define SCOPE_CLASS 2
#define SCOPE_CALL 3
#define SCOPE_IMPORT 4
#define SCOPE_LOOP 5
#define SCOPE_BRANCH 6
#define SCOPE_LEXICAL 7
#define SCOPE_NAMESPACE 8

#define MAX_SCOPES 64
#define INLINE_LEXICAL_SCOPES 64
#define INLINE_LEXICAL_BINDINGS 64
#define INLINE_PYTHON_DIRECTIVES 16
#define INLINE_PY_PARAM_SLOTS 64
#define INLINE_PY_PARAM_STACK 64

// ObjectScript type map: variable name → class name (for instance_method_call
// resolution). Stack-allocated, per-method scope. Overflow is silent (no crash).
#define OS_TYPE_MAP_CAP 64
typedef struct {
    const char *var_name;
    const char *class_name;
} os_type_entry_t;

typedef struct {
    os_type_entry_t entries[OS_TYPE_MAP_CAP];
    int count;
    int class_base_count; // entries [0,class_base_count) survive method-scope resets
} os_type_map_t;

// A call consumes only the exact AST occurrence that denotes its callee.  The
// rest of the call subtree (receiver, computed key, arguments, callback body)
// remains ordinary expression input and is eligible for USAGE extraction.
typedef enum {
    PDXE_INVOCATION_NONE = 0,
    PDXE_INVOCATION_PRIMARY,
    PDXE_INVOCATION_CALLABLE_REFERENCE,
} PDXEInvocationKind;

typedef struct {
    PDXEInvocationKind kind;
    TSNode site;
    TSNode callee_expr;
    TSNode callee_leaf;
    const char *callee_name;
    bool raw_call_emitted;
} PDXEInvocationDescriptor;

/* One name bound as a function/lambda parameter by a scope currently OPEN on
 * the walk stack. A count, not a flag: `def outer(run): def inner(run):` binds
 * the same name twice and the inner pop must not unbind the outer. */
typedef struct {
    const char *name;
    uint32_t hash;
    int count;
} PDXEParamSlot;

typedef struct {
    const char *qn;
    uint32_t depth;
    uint32_t lexical_scope_id;
    uint8_t kind;
    PDXEInvocationKind invocation_kind;
    TSNode callee_expr;
    TSNode callee_leaf;
    /* The complete walk-state tuple this frame displaced, restored verbatim on
     * pop. Saving the full tuple makes push and pop O(1) and kind-agnostic;
     * the previous design recomputed the state by iterating the WHOLE scope
     * stack on every code-bearing node, which is O(depth) per node and turned
     * the deep-nesting torture tests quadratic (0-1s on main, 39-119s here,
     * suite-budget kills on every non-M4 venue). */
    const char *prev_enclosing_func_qn;
    const char *prev_enclosing_class_qn;
    PDXEInvocationKind prev_invocation_kind;
    TSNode prev_callee_expr;
    TSNode prev_callee_leaf;
    bool prev_inside_import;
    int prev_loop_depth;
    int prev_branch_depth;
    uint32_t prev_enclosing_func_depth;
    /* #1912: py_param_stack height on entry. Pop unwinds back to it, so a
     * frame unbinds exactly the parameters it bound and nothing else. */
    int prev_py_param_stack_count;
} PDXEWalkScope;

typedef enum {
    PDXE_LEXICAL_SCOPE_MODULE = 0,
    PDXE_LEXICAL_SCOPE_CLASS,
    PDXE_LEXICAL_SCOPE_FUNCTION,
    PDXE_LEXICAL_SCOPE_BLOCK,
    PDXE_LEXICAL_SCOPE_COMPREHENSION,
} PDXELexicalScopeKind;

/* Concrete AST scope identity. QNs remain graph-attribution metadata only;
 * overloads, lambdas and sibling blocks therefore never share binding facts. */
typedef struct {
    uint32_t id;
    uint32_t parent_id;
    uint32_t lookup_parent_id;
    uint32_t start_byte;
    uint32_t end_byte;
    uint8_t kind;
} PDXELexicalScope;

/* Deferred binding event. Applying these after the walk represents hoisted
 * and whole-scope rules without depending on traversal order. */
typedef struct {
    uint32_t scope_id;
    uint32_t active_start;
    uint32_t active_end;
    const char *name;
} PDXELexicalBinding;

typedef enum {
    PDXE_PYTHON_DIRECTIVE_GLOBAL = 1,
    PDXE_PYTHON_DIRECTIVE_NONLOCAL,
} PDXEPythonDirectiveKind;

typedef struct {
    uint32_t function_scope_id;
    const char *name;
    uint8_t kind;
} PDXEPythonDirective;

// WalkState tracks scope context during the unified cursor walk.
// Replaces parent-chain walks for enclosing_func_qn, import context, etc.
typedef struct {
    const char *enclosing_func_qn;      // current function QN (module_qn at top level)
    const char *enclosing_class_qn;     // current class QN (NULL outside class)
    const TSTreeCursor *current_cursor; // unified walk cursor at the current node
    TSTreeCursor *occurrence_cursor;    // reusable parent-preserving classifier cursor
    PDXEInvocationKind invocation_kind;  // exact active invocation/reference role
    TSNode callee_expr;                 // exact active callee expression, if any
    TSNode callee_leaf;                 // exact active terminal callee, if any
    bool inside_import;                 // within an import_node_types subtree
    int loop_depth;                     // count of enclosing loop scopes (for bottleneck metrics)
    int branch_depth;                   // count of enclosing branch scopes
    /* A site's node-type path: node_types holds the type of the current node and of
     * each ancestor, by cursor depth; enclosing_func_depth is the depth of the
     * innermost function frame (0, the root, at file scope). The path of a site at
     * current_depth is node_types[enclosing_func_depth..current_depth]. */
    const char **node_types;
    uint32_t node_types_capacity;
    uint32_t current_depth;
    uint32_t enclosing_func_depth;
    bool node_types_failed;

    PDXEArena *arena;
    PDXEWalkScope *scopes;
    PDXEWalkScope inline_scopes[MAX_SCOPES];
    int scope_capacity;
    int scope_top;

    PDXELexicalScope *lexical_scopes;
    PDXELexicalScope inline_lexical_scopes[INLINE_LEXICAL_SCOPES];
    int lexical_scope_capacity;
    int lexical_scope_count;
    uint32_t root_lexical_scope_id;
    uint32_t split_function_scope_id;
    uint32_t split_signature_start_byte;
    uint32_t split_signature_end_byte;
    const char *split_function_qn;
    uint32_t flat_function_scope_id;
    uint32_t flat_anchor_start_byte;
    uint32_t flat_anchor_end_byte;
    const char *flat_function_qn;

    PDXELexicalBinding *lexical_bindings;
    PDXELexicalBinding inline_lexical_bindings[INLINE_LEXICAL_BINDINGS];
    int lexical_binding_capacity;
    int lexical_binding_count;
    int usage_start_index;
    bool lexical_binding_tracking_failed;
    PDXEPythonDirective *python_directives;
    PDXEPythonDirective inline_python_directives[INLINE_PYTHON_DIRECTIVES];
    int python_directive_capacity;
    int python_directive_count;
    /* #1912 -- Python bare-call shadowing. A name bound as a parameter by ANY
     * enclosing function or lambda shadows every project function, so a bare
     * `run()` under `def outer(run)` cannot honestly resolve by short name.
     *
     * Maintained as a live name->count map pushed and popped BY THE WALK, not
     * recomputed per call. Walking ancestors (either ts_node_parent or a
     * cursor) and scanning the frame stack are both O(depth) per call, and
     * since every level of f(f(f(...))) is itself a bare call that is
     * quadratic across the file -- the same trap PDXEWalkScope records above.
     * Lookup here is O(1), so no hop cap and no fail-open cap are needed.
     *
     * The other binding table (PDXELexicalBinding) cannot serve this: it is
     * qsort-ed in pdxe_finalize_lexical_usages AFTER the walk and its
     * active_end stays 0 until then, so its binary search is invalid from
     * handle_calls, which runs mid-walk and before handle_usages. */
    PDXEParamSlot *py_param_slots;
    PDXEParamSlot inline_py_param_slots[INLINE_PY_PARAM_SLOTS];
    int py_param_slot_capacity;
    int py_param_slot_used;
    const char **py_param_stack;
    const char *inline_py_param_stack[INLINE_PY_PARAM_STACK];
    int py_param_stack_capacity;
    int py_param_stack_count;
    /* Allocation failure: stop tracking and answer "not bound" forever after,
     * which can only cost a suppression, never a true edge. */
    bool py_param_tracking_failed;

    PDXELanguage language;

    os_type_map_t os_type_map; // ObjectScript variable → type mapping
} WalkState;

/* #1912: is `name` bound as a parameter by a Python function or lambda scope
 * currently open on the walk stack? O(1). Answers false on any failure, so a
 * caller can only ever lose a suppression, never a true edge. */
bool pdxe_walk_python_param_is_bound(const WalkState *state, const char *name);

// Per-node handler prototypes. Each is called once per node during the
// unified cursor walk, replacing the old recursive walk_* functions.
PDXEInvocationDescriptor handle_calls(PDXEExtractCtx *ctx, TSNode node, const PDXELangSpec *spec,
                                     WalkState *state);
void handle_usages(PDXEExtractCtx *ctx, TSNode node, const PDXELangSpec *spec, WalkState *state);
/* Gives every call pushed since calls_before, and every possible callable reference
 * pushed since usages_before, the current node's node-type path. */
void pdxe_walk_stamp_site_paths(PDXEExtractCtx *ctx, WalkState *state, int calls_before,
                               int usages_before);
void pdxe_finalize_lexical_usages(PDXEExtractCtx *ctx, WalkState *state);
void handle_throws(PDXEExtractCtx *ctx, TSNode node, const PDXELangSpec *spec, WalkState *state);
void handle_readwrites(PDXEExtractCtx *ctx, TSNode node, const PDXELangSpec *spec, WalkState *state);
void handle_type_refs(PDXEExtractCtx *ctx, TSNode node, const PDXELangSpec *spec, WalkState *state);
void handle_env_accesses(PDXEExtractCtx *ctx, TSNode node, const PDXELangSpec *spec,
                         WalkState *state);
void handle_type_assigns(PDXEExtractCtx *ctx, TSNode node, const PDXELangSpec *spec,
                         WalkState *state);

// Single-pass extraction using TSTreeCursor. Visits every node once,
// dispatching to all handlers per node. Replaces the 7 separate walk_*
// functions for calls/usages/throws/readwrites/type_refs/env_accesses/type_assigns.
// Definitions and imports stay as separate passes (different recursion patterns).
void pdxe_extract_unified(PDXEExtractCtx *ctx);

#endif // PDXE_EXTRACT_UNIFIED_H
