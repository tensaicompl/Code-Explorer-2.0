#include "pdxe_core.h"
#include "helpers.h"
#include "lang_specs.h"
#include "extract_unified.h"
#include "tree_sitter/api.h" // TSNode, ts_node_*
#include "foundation/constants.h"
#include "extract_node_stack.h"

enum { LAST_IDX = 1 };
#include <stdint.h> // uint32_t
#include <stdlib.h> // qsort
#include <string.h>
#include <strings.h>
#include <ctype.h>

/* Test builds count the ancestors usage classification examines (cursor steps, walk
 * frame steps and frame answers computed: issue 90) and the climbs that fell back to
 * ts_node_parent, which descends from the root. Release builds count nothing. */
#if (defined(PDXE_CALL_REFERENCE_LOOKUP_TEST_API) && PDXE_CALL_REFERENCE_LOOKUP_TEST_API) ||     \
    defined(PDXE_ENABLE_TEST_SEAMS)
#include <stdatomic.h>

static _Atomic uint64_t g_usage_field_lookup_work = 0;
static _Atomic uint64_t g_usage_slow_parent_fallbacks = 0;

void pdxe_usage_field_lookup_test_reset(void) {
    atomic_store_explicit(&g_usage_field_lookup_work, 0, memory_order_relaxed);
    atomic_store_explicit(&g_usage_slow_parent_fallbacks, 0, memory_order_relaxed);
}

uint64_t pdxe_usage_field_lookup_test_work(void) {
    return atomic_load_explicit(&g_usage_field_lookup_work, memory_order_relaxed);
}

uint64_t pdxe_usage_slow_parent_fallback_test_count(void) {
    return atomic_load_explicit(&g_usage_slow_parent_fallbacks, memory_order_relaxed);
}

static void usage_field_lookup_test_note_work(void) {
    atomic_fetch_add_explicit(&g_usage_field_lookup_work, 1, memory_order_relaxed);
}

static void usage_slow_parent_fallback_test_note(void) {
    atomic_fetch_add_explicit(&g_usage_slow_parent_fallbacks, 1, memory_order_relaxed);
}

/* Every ts_node_parent usage classification makes counts as a slow fallback too, so
 * a climb that comes back counts whether or not it notes itself. */
static TSNode usage_counted_ts_node_parent(TSNode node) {
    usage_slow_parent_fallback_test_note();
    return ts_node_parent(node);
}
#define ts_node_parent usage_counted_ts_node_parent
#else
static void usage_field_lookup_test_note_work(void) {}
static void usage_slow_parent_fallback_test_note(void) {}
#endif

// Forward declaration
static void walk_usages(PDXEExtractCtx *ctx, TSNode root, const PDXELangSpec *spec);
static bool is_direct_argument_value(TSNode node);
static TSNode python_direct_callable_attribute_site(TSNode node);
static bool walk_depth_of(const WalkState *state, TSNode node, uint32_t *depth);
static TSNode walk_parent_of(const WalkState *state, TSNode node);
static TSNode walk_python_direct_callable_attribute_site(WalkState *state, uint32_t depth);

// Is this an identifier-like node that represents a reference? `state` is the walk's,
// NULL outside it.
static bool is_reference_node(TSNode node, PDXELanguage lang, WalkState *state) {
    const char *kind = ts_node_type(node);

    /* Python's attribute node and its terminal identifier describe the same
     * source occurrence. For a direct callable-value argument, keep the leaf
     * as the raw carrier (so unresolved cases retain a useful short-name
     * USAGE) and stamp it with the full attribute span below. Receivers remain
     * independent value references. */
    uint32_t depth;
    if (lang == PDXE_LANG_PYTHON && strcmp(kind, "attribute") == 0 &&
        !ts_node_is_null(walk_depth_of(state, node, &depth)
                             ? walk_python_direct_callable_attribute_site(state, depth)
                             : python_direct_callable_attribute_site(node))) {
        return false;
    }

    /* A Rust scoped_identifier owns the complete path occurrence. Its nested
     * path/identifier children are grammar structure, not additional value
     * references; keeping them would let a short-name fallback bind a decoy. */
    if (lang == PDXE_LANG_RUST &&
        (strcmp(kind, "identifier") == 0 || strcmp(kind, "scoped_identifier") == 0)) {
        TSNode parent = walk_parent_of(state, node);
        if (!ts_node_is_null(parent) && strcmp(ts_node_type(parent), "scoped_identifier") == 0) {
            return false;
        }
    }

    /* Some grammars expose the sigil/scope as a named wrapper around a generic
     * identifier. Keep exactly one occurrence: the wrapper carries the source
     * spelling that resolution needs (`$watched`, `a:watched`).
     *
     * The language gate MUST precede the parent fetch: ts_node_parent descends
     * from the root (O(depth)), and fetching it for every identifier in every
     * language made deep-nesting extraction quadratic across the board (the
     * stack_overflow deep tests went 0-1s -> 39-119s and blew the suite
     * budget on every non-M4 venue). */
    if ((lang == PDXE_LANG_PUPPET || lang == PDXE_LANG_VIMSCRIPT) &&
        strcmp(kind, "identifier") == 0) {
        TSNode parent = ts_node_parent(node);
        if (!ts_node_is_null(parent) &&
            ((lang == PDXE_LANG_PUPPET && strcmp(ts_node_type(parent), "variable") == 0) ||
             (lang == PDXE_LANG_VIMSCRIPT && strcmp(ts_node_type(parent), "argument") == 0))) {
            return false;
        }
    }

    // Common identifier types across languages
    if (strcmp(kind, "identifier") == 0 || strcmp(kind, "simple_identifier") == 0 ||
        strcmp(kind, "type_identifier") == 0) {
        return true;
    }

    // Language-specific reference types
    switch (lang) {
    case PDXE_LANG_JAVASCRIPT:
    case PDXE_LANG_TYPESCRIPT:
    case PDXE_LANG_TSX:
    case PDXE_LANG_ARKTS:
    case PDXE_LANG_QML:
    case PDXE_LANG_CFSCRIPT:
        return strcmp(kind, "property_identifier") == 0 ||
               strcmp(kind, "private_property_identifier") == 0;
    case PDXE_LANG_GO:
        return strcmp(kind, "field_identifier") == 0 || strcmp(kind, "package_identifier") == 0;
    case PDXE_LANG_PYTHON:
        return strcmp(kind, "attribute") == 0;
    case PDXE_LANG_RUST:
        return strcmp(kind, "field_identifier") == 0 || strcmp(kind, "scoped_identifier") == 0;
    case PDXE_LANG_C:
    case PDXE_LANG_CPP:
    case PDXE_LANG_CUDA:
        return strcmp(kind, "field_identifier") == 0;
    case PDXE_LANG_PHP:
        return strcmp(kind, "name") == 0 || strcmp(kind, "variable_name") == 0;
    case PDXE_LANG_HASKELL:
        return strcmp(kind, "variable") == 0 || strcmp(kind, "constructor") == 0;
    case PDXE_LANG_OCAML:
        return strcmp(kind, "value_path") == 0 || strcmp(kind, "constructor_path") == 0;
    case PDXE_LANG_ERLANG:
        return strcmp(kind, "atom") == 0 || strcmp(kind, "var") == 0;
    case PDXE_LANG_CSS:
        /* Custom-property reads inside `var(...)` are exposed as plain_value;
         * declarations use the separate property_name alias. */
        return strcmp(kind, "plain_value") == 0;
    case PDXE_LANG_SCSS:
        /* SCSS value occurrences retain their `$` spelling in variable_value;
         * declaration parameters use the distinct variable_name node. */
        return strcmp(kind, "variable_value") == 0;
    case PDXE_LANG_LLVM_IR:
        return strcmp(kind, "local_var") == 0 || strcmp(kind, "global_var") == 0;
    case PDXE_LANG_PUPPET:
    case PDXE_LANG_POWERSHELL:
        /* Puppet keeps the sigil in a dedicated `variable` node (`$watched`). */
        return strcmp(kind, "variable") == 0;
    case PDXE_LANG_BASH:
    case PDXE_LANG_FISH:
    case PDXE_LANG_ZSH:
        /* Expansion wrappers retain `$`; their named leaf is the bare variable name. */
        return strcmp(kind, "variable_name") == 0;
    case PDXE_LANG_PERL:
        /* Perl's named scalar node retains its sigil (`$watched`). */
        return strcmp(kind, "scalar") == 0;
    case PDXE_LANG_CLOJURE:
    case PDXE_LANG_COMMONLISP:
        return strcmp(kind, "sym_lit") == 0;
    case PDXE_LANG_VIMSCRIPT:
        return strcmp(kind, "scoped_identifier") == 0 || strcmp(kind, "argument") == 0;
    case PDXE_LANG_ELM:
        return strcmp(kind, "lower_case_identifier") == 0;
    case PDXE_LANG_COBOL:
        return strcmp(kind, "qualified_word") == 0;
    case PDXE_LANG_EMACSLISP:
    case PDXE_LANG_SCHEME:
    case PDXE_LANG_FENNEL:
    case PDXE_LANG_RACKET:
    case PDXE_LANG_CHIALISP:
    case PDXE_LANG_LINKERSCRIPT:
        return strcmp(kind, "symbol") == 0;
    case PDXE_LANG_MAKEFILE:
        return strcmp(kind, "variable_reference") == 0;
    case PDXE_LANG_CMAKE:
        /* `${watched}` contains a named `variable` leaf spanning only `watched`. */
        return strcmp(kind, "variable") == 0;
    case PDXE_LANG_WOLFRAM:
        return strcmp(kind, "user_symbol") == 0;
    case PDXE_LANG_TYPST:
        return strcmp(kind, "ident") == 0;
    case PDXE_LANG_TCL:
        /* Tcl's value spelling includes the sigil, so retain the substitution wrapper. */
        return strcmp(kind, "variable_substitution") == 0;
    case PDXE_LANG_TLAPLUS:
        /* Definition-side names/parameters are `identifier`; expression-side
         * values and operator arguments use the distinct `identifier_ref`. */
        return strcmp(kind, "identifier_ref") == 0;
    case PDXE_LANG_AGDA:
        /* `_qid` is hidden and aliases its public expression leaf to `qid`. */
        return strcmp(kind, "qid") == 0;
    case PDXE_LANG_RESCRIPT:
        return strcmp(kind, "value_identifier") == 0;
    case PDXE_LANG_PURESCRIPT:
        return strcmp(kind, "variable") == 0;
    case PDXE_LANG_NICKEL:
        return strcmp(kind, "ident") == 0;
    case PDXE_LANG_JSONNET:
        return strcmp(kind, "id") == 0;
    case PDXE_LANG_CFML:
        return strcmp(kind, "property_identifier") == 0;
    case PDXE_LANG_OBJECTSCRIPT_UDL:
    case PDXE_LANG_OBJECTSCRIPT_ROUTINE:
        return strcmp(kind, "objectscript_identifier") == 0 ||
               strcmp(kind, "objectscript_identifier_special") == 0;
    case PDXE_LANG_PLSQL:
        return strcmp(kind, "identifier") == 0;
    default:
        return false;
    }
}

static bool node_contains(TSNode outer, TSNode inner) {
    return !ts_node_is_null(outer) && !ts_node_is_null(inner) &&
           ts_node_start_byte(outer) <= ts_node_start_byte(inner) &&
           ts_node_end_byte(outer) >= ts_node_end_byte(inner);
}

static bool vhdl_forward_callee_wrapper(TSNode node) {
    const char *kind = ts_node_type(node);
    return strcmp(kind, "identifier") == 0 || strcmp(kind, "simple_identifier") == 0 ||
           strcmp(kind, "library_function") == 0 || strcmp(kind, "name") == 0 ||
           strcmp(kind, "simple_name") == 0;
}

static TSNode terminal_vhdl_identifier(TSNode node, int remaining_depth) {
    if (ts_node_is_null(node) || remaining_depth < 0) {
        return (TSNode){0};
    }
    uint32_t count = ts_node_named_child_count(node);
    for (uint32_t i = count; i > 0; i--) {
        TSNode found =
            terminal_vhdl_identifier(ts_node_named_child(node, i - 1), remaining_depth - 1);
        if (!ts_node_is_null(found)) {
            return found;
        }
    }
    const char *kind = ts_node_type(node);
    return strcmp(kind, "identifier") == 0 || strcmp(kind, "simple_identifier") == 0 ? node
                                                                                     : (TSNode){0};
}

// Dart and VHDL represent some calls as a callee node immediately followed by
// the invocation node. The unified pre-order walk has already visited that
// callee when the call descriptor is created, so suppress only the exact
// language-specific preceding occurrence. Do not generalize this to arbitrary
// identifiers: a sibling selector/parenthesis group is the grammar contract.
static bool is_forward_sibling_callee(PDXELanguage language, TSNode node) {
    if (language == PDXE_LANG_DART && strcmp(ts_node_type(node), "identifier") == 0) {
        TSNode next = ts_node_next_named_sibling(node);
        return !ts_node_is_null(next) && strcmp(ts_node_type(next), "selector") == 0;
    }

    if (language != PDXE_LANG_VHDL) {
        return false;
    }

    TSNode owner = node;
    for (int depth = 0; depth < 4 && vhdl_forward_callee_wrapper(owner); depth++) {
        TSNode next = ts_node_next_named_sibling(owner);
        if (!ts_node_is_null(next) && strcmp(ts_node_type(next), "parenthesis_group") == 0) {
            TSNode leaf = terminal_vhdl_identifier(owner, 8);
            return !ts_node_is_null(leaf) && ts_node_eq(node, leaf);
        }
        TSNode parent = ts_node_parent(owner);
        if (ts_node_is_null(parent) || !vhdl_forward_callee_wrapper(parent)) {
            break;
        }
        owner = parent;
    }
    return false;
}

static const char *field_name_for_node(TSNode parent, TSNode child) {
    const char *field = NULL;
    TSTreeCursor cursor = ts_tree_cursor_new(parent);
    if (ts_tree_cursor_goto_first_child(&cursor)) {
        do {
            usage_field_lookup_test_note_work();
            if (ts_node_eq(ts_tree_cursor_current_node(&cursor), child)) {
                field = ts_tree_cursor_current_field_name(&cursor);
                break;
            }
        } while (ts_tree_cursor_goto_next_sibling(&cursor));
    }
    ts_tree_cursor_delete(&cursor);
    return field;
}

/* The unified walk already owns the exact cursor path to the current node.
 * Reset one reusable scratch cursor to that path before each occurrence
 * classifier, then walk its parents without restarting a sibling scan at
 * every level. Legacy extraction callers retain the node-based fallback. */
static TSTreeCursor *reset_occurrence_cursor(WalkState *state, TSNode node) {
    if (!state || !state->current_cursor || !state->occurrence_cursor ||
        !ts_node_eq(ts_tree_cursor_current_node(state->current_cursor), node)) {
        return NULL;
    }
    ts_tree_cursor_reset_to(state->occurrence_cursor, state->current_cursor);
    return state->occurrence_cursor;
}

static bool occurrence_parent(TSTreeCursor *cursor, TSNode current, TSNode *parent,
                              const char **field) {
    if (cursor) {
        *field = ts_tree_cursor_current_field_name(cursor);
        usage_field_lookup_test_note_work();
        if (!ts_tree_cursor_goto_parent(cursor)) {
            *parent = (TSNode){0};
            return false;
        }
        *parent = ts_tree_cursor_current_node(cursor);
        return true;
    }

    *parent = ts_node_parent(current);
    if (ts_node_is_null(*parent)) {
        *field = NULL;
        return false;
    }
    *field = field_name_for_node(*parent, current);
    return true;
}

static const char *field_name_for_walk_node(WalkState *state, TSNode parent, TSNode node) {
    if (state && state->current_cursor &&
        ts_node_eq(ts_tree_cursor_current_node(state->current_cursor), node)) {
        usage_field_lookup_test_note_work();
        return ts_tree_cursor_current_field_name(state->current_cursor);
    }
    return field_name_for_node(parent, node);
}

static bool is_value_field(const char *field) {
    return field && (strcmp(field, "value") == 0 || strcmp(field, "right") == 0 ||
                     strcmp(field, "initializer") == 0 || strcmp(field, "default") == 0 ||
                     strcmp(field, "default_value") == 0 || strcmp(field, "body") == 0 ||
                     strcmp(field, "arguments") == 0 || strcmp(field, "condition") == 0 ||
                     strcmp(field, "consequence") == 0 || strcmp(field, "alternative") == 0 ||
                     strcmp(field, "expression") == 0 || strcmp(field, "result") == 0);
}

static bool kind_in_exact_set(const char *kind, const char *const *set) {
    if (!kind || !set) {
        return false;
    }
    for (const char *const *candidate = set; *candidate; candidate++) {
        if (strcmp(kind, *candidate) == 0) {
            return true;
        }
    }
    return false;
}

static bool field_contains_node(TSNode parent, const char *field, TSNode node) {
    TSNode value = ts_node_child_by_field_name(parent, field, (uint32_t)strlen(field));
    return node_contains(value, node);
}

typedef enum {
    PDXE_OCCURRENCE_STANDARD = 0,
    PDXE_OCCURRENCE_LISP_DEF,
    PDXE_OCCURRENCE_COMMONLISP_DEFUN,
    PDXE_OCCURRENCE_FENNEL_FN,
    PDXE_OCCURRENCE_ELIXIR_DEF,
    PDXE_OCCURRENCE_JULIA_FUNCTION,
    PDXE_OCCURRENCE_WOLFRAM_SET,
    PDXE_OCCURRENCE_TYPST_LET,
    PDXE_OCCURRENCE_AGDA_FUNCTION,
    PDXE_OCCURRENCE_TLAPLUS_OPERATOR,
    PDXE_OCCURRENCE_COBOL_MOVE,
    PDXE_OCCURRENCE_HCL_ATTRIBUTE,
    PDXE_OCCURRENCE_ELM_VALUE,
    PDXE_OCCURRENCE_RESCRIPT_LET,
    PDXE_OCCURRENCE_PURESCRIPT_LHS,
    PDXE_OCCURRENCE_NICKEL_LET,
    PDXE_OCCURRENCE_ERLANG_CLAUSE,
    PDXE_OCCURRENCE_NIX_FUNCTION,
    PDXE_OCCURRENCE_MATLAB_ARGUMENTS,
    PDXE_OCCURRENCE_LEAN_BINDER,
    PDXE_OCCURRENCE_PASCAL_PROC,
    PDXE_OCCURRENCE_TEAL_FUNCTION,
    PDXE_OCCURRENCE_VHDL_INTERFACE,
    PDXE_OCCURRENCE_PINE_FUNCTION,
    PDXE_OCCURRENCE_LLVM_FUNCTION,
    PDXE_OCCURRENCE_PKL_DECLARATION,
} PDXEOccurrencePolicy;

typedef struct {
    const char *const *whole_binding_nodes;
    const char *const *write_nodes;
    PDXEOccurrencePolicy policy;
    bool first_named_child_is_write;
} PDXEOccurrenceSpec;

/* These are exact grammar roles, not name fragments.  Default/value/body
 * fields are barriers, so an initializer nested below one of these containers
 * remains a read. */
static const char *const common_whole_binding_nodes[] = {"formal_parameter",
                                                         "formal_parameters",
                                                         "parameter",
                                                         "parameters",
                                                         "parameter_list",
                                                         "parameter_declaration",
                                                         "parameter_specification",
                                                         "required_parameter",
                                                         "optional_parameter",
                                                         "default_parameter",
                                                         "typed_parameter",
                                                         "function_value_parameter",
                                                         "function_value_parameters",
                                                         "lambda_parameter",
                                                         "lambda_parameters",
                                                         "function_parameter_declaration",
                                                         "closure_parameters",
                                                         "block_parameters",
                                                         "receiver",
                                                         NULL};

static const char *const field_binding_nodes[] = {"variable_declarator",
                                                  "init_declarator",
                                                  "variable_declaration",
                                                  "const_declaration",
                                                  "lexical_declaration",
                                                  "short_var_declaration",
                                                  "local_variable_declaration",
                                                  "property_declaration",
                                                  "field_declaration",
                                                  "value_declaration",
                                                  "val_definition",
                                                  "var_definition",
                                                  "let_declaration",
                                                  "local_bind",
                                                  "let_binding",
                                                  "data_declaration",
                                                  "net_declaration",
                                                  "object_declaration",
                                                  "number_declaration",
                                                  "typed_binding",
                                                  "variable_assignment",
                                                  NULL};

static const char *const binding_fields[] = {"name",       "pattern", "declarator", "parameter",
                                             "parameters", "left",    "variable",   "variables",
                                             "key",        NULL};

static const char *const sql_binding_nodes[] = {"function_argument", NULL};
static const char *const haskell_binding_nodes[] = {"patterns", NULL};
static const char *const fsharp_binding_nodes[] = {
    "function_declaration_left", "value_declaration_left", "argument_patterns", NULL};
static const char *const crystal_binding_nodes[] = {"param_list", NULL};
static const char *const awk_binding_nodes[] = {"param_list", NULL};
static const char *const teal_binding_nodes[] = {"function_signature", NULL};
static const char *const systemverilog_binding_nodes[] = {"tf_port_item", "tf_port_item1", NULL};
static const char *const rescript_binding_nodes[] = {"formal_parameters", "labeled_parameter",
                                                     "parameter", NULL};
static const char *const purescript_binding_nodes[] = {"bind_pattern", "pattern", "patterns", NULL};
static const char *const nickel_binding_nodes[] = {"pattern_fun", NULL};
static const char *const jsonnet_binding_nodes[] = {"param", NULL};
static const char *const llvm_binding_nodes[] = {"function_header", NULL};
static const char *const linkerscript_write_nodes[] = {"assignment", NULL};
static const char *const meson_write_nodes[] = {"operatorunit", NULL};
static const char *const gn_write_nodes[] = {"assignment_statement", NULL};
static const char *const objectscript_binding_nodes[] = {"argument", "tag_parameter", NULL};
static const char *const objectscript_write_nodes[] = {"set_argument", NULL};

/* Parallel to lang_specs: occurrence semantics evolve without adding fields to
 * the positional PDXELangSpec initializer used by every language. */
static const PDXEOccurrenceSpec occurrence_specs[PDXE_LANG_COUNT] = {
    [PDXE_LANG_SQL] = {sql_binding_nodes, NULL, PDXE_OCCURRENCE_STANDARD, false},
    [PDXE_LANG_CLOJURE] = {NULL, NULL, PDXE_OCCURRENCE_LISP_DEF, false},
    [PDXE_LANG_SCHEME] = {NULL, NULL, PDXE_OCCURRENCE_LISP_DEF, false},
    [PDXE_LANG_RACKET] = {NULL, NULL, PDXE_OCCURRENCE_LISP_DEF, false},
    [PDXE_LANG_CHIALISP] = {NULL, NULL, PDXE_OCCURRENCE_LISP_DEF, false},
    [PDXE_LANG_COMMONLISP] = {NULL, NULL, PDXE_OCCURRENCE_COMMONLISP_DEFUN, false},
    [PDXE_LANG_FENNEL] = {NULL, NULL, PDXE_OCCURRENCE_FENNEL_FN, false},
    [PDXE_LANG_ELIXIR] = {NULL, NULL, PDXE_OCCURRENCE_ELIXIR_DEF, false},
    [PDXE_LANG_JULIA] = {NULL, NULL, PDXE_OCCURRENCE_JULIA_FUNCTION, false},
    [PDXE_LANG_WOLFRAM] = {NULL, NULL, PDXE_OCCURRENCE_WOLFRAM_SET, false},
    [PDXE_LANG_TYPST] = {NULL, NULL, PDXE_OCCURRENCE_TYPST_LET, false},
    [PDXE_LANG_AGDA] = {NULL, NULL, PDXE_OCCURRENCE_AGDA_FUNCTION, false},
    [PDXE_LANG_TLAPLUS] = {NULL, NULL, PDXE_OCCURRENCE_TLAPLUS_OPERATOR, false},
    [PDXE_LANG_COBOL] = {NULL, NULL, PDXE_OCCURRENCE_COBOL_MOVE, false},
    [PDXE_LANG_HCL] = {NULL, NULL, PDXE_OCCURRENCE_HCL_ATTRIBUTE, false},
    [PDXE_LANG_ELM] = {NULL, NULL, PDXE_OCCURRENCE_ELM_VALUE, false},
    [PDXE_LANG_RESCRIPT] = {rescript_binding_nodes, NULL, PDXE_OCCURRENCE_RESCRIPT_LET, false},
    [PDXE_LANG_PURESCRIPT] = {purescript_binding_nodes, NULL, PDXE_OCCURRENCE_PURESCRIPT_LHS, false},
    [PDXE_LANG_NICKEL] = {nickel_binding_nodes, NULL, PDXE_OCCURRENCE_NICKEL_LET, false},
    [PDXE_LANG_JSONNET] = {jsonnet_binding_nodes, NULL, PDXE_OCCURRENCE_STANDARD, false},
    [PDXE_LANG_HASKELL] = {haskell_binding_nodes, NULL, PDXE_OCCURRENCE_STANDARD, false},
    [PDXE_LANG_ERLANG] = {NULL, NULL, PDXE_OCCURRENCE_ERLANG_CLAUSE, false},
    [PDXE_LANG_FSHARP] = {fsharp_binding_nodes, NULL, PDXE_OCCURRENCE_STANDARD, false},
    [PDXE_LANG_NIX] = {NULL, NULL, PDXE_OCCURRENCE_NIX_FUNCTION, false},
    [PDXE_LANG_MATLAB] = {NULL, NULL, PDXE_OCCURRENCE_MATLAB_ARGUMENTS, false},
    [PDXE_LANG_LEAN] = {NULL, NULL, PDXE_OCCURRENCE_LEAN_BINDER, false},
    [PDXE_LANG_PASCAL] = {NULL, NULL, PDXE_OCCURRENCE_PASCAL_PROC, false},
    [PDXE_LANG_VERILOG] = {systemverilog_binding_nodes, NULL, PDXE_OCCURRENCE_STANDARD, false},
    [PDXE_LANG_AWK] = {awk_binding_nodes, NULL, PDXE_OCCURRENCE_STANDARD, false},
    [PDXE_LANG_CRYSTAL] = {crystal_binding_nodes, NULL, PDXE_OCCURRENCE_STANDARD, false},
    [PDXE_LANG_TEAL] = {teal_binding_nodes, NULL, PDXE_OCCURRENCE_TEAL_FUNCTION, false},
    [PDXE_LANG_VHDL] = {NULL, NULL, PDXE_OCCURRENCE_VHDL_INTERFACE, false},
    [PDXE_LANG_SYSTEMVERILOG] = {systemverilog_binding_nodes, NULL, PDXE_OCCURRENCE_STANDARD, false},
    [PDXE_LANG_PINE] = {NULL, NULL, PDXE_OCCURRENCE_PINE_FUNCTION, false},
    [PDXE_LANG_PUPPET] = {NULL, NULL, PDXE_OCCURRENCE_STANDARD, true},
    [PDXE_LANG_LLVM_IR] = {llvm_binding_nodes, NULL, PDXE_OCCURRENCE_LLVM_FUNCTION, false},
    [PDXE_LANG_PKL] = {NULL, NULL, PDXE_OCCURRENCE_PKL_DECLARATION, false},
    [PDXE_LANG_MESON] = {NULL, meson_write_nodes, PDXE_OCCURRENCE_STANDARD, true},
    [PDXE_LANG_GN] = {NULL, gn_write_nodes, PDXE_OCCURRENCE_STANDARD, true},
    [PDXE_LANG_LINKERSCRIPT] = {NULL, linkerscript_write_nodes, PDXE_OCCURRENCE_STANDARD, true},
    [PDXE_LANG_OBJECTSCRIPT_UDL] = {objectscript_binding_nodes, objectscript_write_nodes,
                                   PDXE_OCCURRENCE_STANDARD, true},
    [PDXE_LANG_OBJECTSCRIPT_ROUTINE] = {objectscript_binding_nodes, objectscript_write_nodes,
                                       PDXE_OCCURRENCE_STANDARD, true},
};

static bool text_equals(PDXEExtractCtx *ctx, TSNode node, const char *expected) {
    char *text = pdxe_node_text(ctx->arena, node, ctx->source);
    return text && strcmp(text, expected) == 0;
}

static bool named_child_contains(TSNode parent, uint32_t index, TSNode node) {
    return ts_node_named_child_count(parent) > index &&
           node_contains(ts_node_named_child(parent, index), node);
}

/* ── Issue 90: the walk's own path ───────────────────────────────────────────────
 *
 * Classifying a usage asks about its ancestors: the nearest function or import above
 * it, whether a global statement holds it, the first ancestor whose role decides a
 * binding. Asked by climbing with ts_node_parent, which tree-sitter answers by
 * descending from the root, each question cost depth² a usage; a file of deep
 * constructs took seconds per construct. The unified walk records the frame of every
 * node it enters (record_walk_frame), so the current node's ancestors are its frames:
 * a parent is an index away. A nearest-of-kind answer is kept by each frame, and a
 * climb's answer too, each computed when first asked, from the frame below it, so a
 * frame costs a bounded amount of work however many usages ask: linear in the file.
 *
 * Nothing about what is answered changes. A frame's node is ts_node_parent of the
 * frame above it, its field the cursor's, and each question asks at each ancestor
 * exactly what the climb asked. Outside the walk (the frames do not hold the node, or
 * could not be recorded) the climbs are made as before. */

static bool is_labeled_argument_kind(const char *kind);
static bool is_argument_container_kind(const char *kind);
static bool import_kind_matches(TSNode node, const PDXELangSpec *spec);

/* Whether `node` is the walk's current node, and its depth on the path. */
static bool walk_depth_of(const WalkState *state, TSNode node, uint32_t *depth) {
    if (!state || state->frames_failed || !state->frames ||
        state->frame_depth >= state->frames_capacity ||
        !ts_node_eq(state->frames[state->frame_depth].node, node)) {
        return false;
    }
    *depth = state->frame_depth;
    return true;
}

/* The parent of `node`: its frame's predecessor when `node` is the walk's current node,
 * otherwise ts_node_parent. */
static TSNode walk_parent_of(const WalkState *state, TSNode node) {
    uint32_t depth;
    if (walk_depth_of(state, node, &depth)) {
        usage_field_lookup_test_note_work();
        return depth > 0 ? state->frames[depth - 1].node : (TSNode){0};
    }
    usage_slow_parent_fallback_test_note();
    return ts_node_parent(node);
}

static bool walk_node_in_set(const WalkState *state, unsigned set, TSNode node) {
    const PDXELangSpec *spec = state->spec;
    const char *kind = ts_node_type(node);
    switch (set) {
    case PDXE_NEAREST_PY_GLOBAL:
        return strcmp(kind, "global_statement") == 0;
    case PDXE_NEAREST_PY_NONLOCAL:
        return strcmp(kind, "nonlocal_statement") == 0;
    case PDXE_NEAREST_FUNCTION:
        return spec && spec->function_node_types &&
               pdxe_kind_in_set(node, spec->function_node_types);
    case PDXE_NEAREST_CLASS:
        return spec && spec->class_node_types && pdxe_kind_in_set(node, spec->class_node_types);
    case PDXE_NEAREST_PARAMETER:
        return kind_in_exact_set(kind, common_whole_binding_nodes) ||
               kind_in_exact_set(kind, occurrence_specs[state->language].whole_binding_nodes) ||
               (spec && spec->function_node_types &&
                pdxe_kind_in_set(node, spec->function_node_types));
    case PDXE_NEAREST_JS_DECLARATION:
        return strcmp(kind, "variable_declaration") == 0 ||
               strcmp(kind, "lexical_declaration") == 0;
    case PDXE_NEAREST_IMPORT:
        return spec && import_kind_matches(node, spec);
    case PDXE_NEAREST_ARGUMENT_LABEL:
        return is_labeled_argument_kind(kind) || is_argument_container_kind(kind);
    case PDXE_NEAREST_PERL_CODEREF:
        return strcmp(kind, "refgen_expression") == 0 || is_argument_container_kind(kind);
    case PDXE_NEAREST_PERL_DECLARATION:
        return strcmp(kind, "variable_declaration") == 0 ||
               strcmp(kind, "assignment_expression") == 0;
    case PDXE_NEAREST_HCL_ATTRIBUTE:
        return strcmp(kind, "attribute") == 0;
    default:
        return false;
    }
}

/* Depth + 1 of the nearest frame at or above `depth` whose node is of a kind in `set`,
 * 0 for none. Computed down to the nearest frame that already knows, each frame once. */
static uint32_t walk_nearest(WalkState *state, uint32_t depth, unsigned set) {
    uint32_t bit = 1U << set;
    uint32_t first = depth + 1;
    while (first > 0 && !(state->frames[first - 1].known & bit)) {
        first--;
    }
    for (uint32_t k = first; k <= depth; k++) {
        PDXEWalkFrame *frame = &state->frames[k];
        uint32_t below = k > 0 ? state->frames[k - 1].nearest[set] : 0;
        frame->nearest[set] = walk_node_in_set(state, set, frame->node) ? k + 1 : below;
        frame->known |= bit;
        usage_field_lookup_test_note_work();
    }
    return state->frames[depth].nearest[set];
}

/* The nearest strict ancestor of the node at `depth` of a kind in `set`: its depth + 1,
 * 0 for none. */
static uint32_t walk_nearest_above(WalkState *state, uint32_t depth, unsigned set) {
    return depth > 0 ? walk_nearest(state, depth - 1, set) : 0;
}

/* One step of a climb: what an ancestor (`parent`, which the node below on the path
 * occupies as `field`) says about `node`. */
typedef enum {
    PDXE_STEP_CONTINUE = 0,
    PDXE_STEP_FALSE = 1,
    PDXE_STEP_TRUE = 2,
} PDXEClimbStep;

typedef PDXEClimbStep (*PDXEClimbStepFn)(PDXEExtractCtx *ctx, const PDXELangSpec *spec,
                                         TSNode parent, const char *field, TSNode node);

/* A climb from the node at `depth`, the walk's current node: each ancestor in turn, the
 * nearest first, until one decides; reaching the root decides false.
 *
 * Where a step asks whether an ancestor's child contains `node`, it asks it of byte
 * ranges. For a node of positive width, that child contains it exactly when the child
 * is the node's own ancestor on the path (siblings' ranges do not overlap, and each
 * holds its descendants'), so every step says of it what it says of the frame below the
 * ancestor: the frames' answers serve every such node, the frame's own computed once
 * from the frame below. An empty node (a parser's missing one) can sit in a sibling's
 * range at its edge: its climb is made step by step, over the frames, never more than
 * the depth. */
static bool walk_climb(PDXEExtractCtx *ctx, const PDXELangSpec *spec, WalkState *state,
                       TSNode node, uint32_t depth, unsigned climb, PDXEClimbStepFn step) {
    if (ts_node_end_byte(node) <= ts_node_start_byte(node)) {
        for (uint32_t k = depth; k > 0; k--) {
            usage_field_lookup_test_note_work();
            PDXEClimbStep decided =
                step(ctx, spec, state->frames[k - 1].node, state->frames[k].field, node);
            if (decided != PDXE_STEP_CONTINUE) {
                return decided == PDXE_STEP_TRUE;
            }
        }
        return false;
    }
    uint32_t bit = 1U << (PDXE_NEAREST_COUNT + climb);
    uint32_t first = depth + 1;
    while (first > 0 && !(state->frames[first - 1].known & bit)) {
        first--;
    }
    for (uint32_t k = first; k <= depth; k++) {
        PDXEWalkFrame *frame = &state->frames[k];
        uint8_t answer = PDXE_STEP_FALSE;
        if (k > 0) {
            PDXEClimbStep decided =
                step(ctx, spec, state->frames[k - 1].node, frame->field, frame->node);
            answer = decided != PDXE_STEP_CONTINUE ? (uint8_t)decided
                                                   : state->frames[k - 1].climb[climb];
        }
        frame->climb[climb] = answer;
        frame->known |= bit;
        usage_field_lookup_test_note_work();
    }
    return state->frames[depth].climb[climb] == PDXE_STEP_TRUE;
}

/* is_direct_argument_value over the frames, for the node at `depth`. */
static bool walk_is_direct_argument_value(WalkState *state, uint32_t depth) {
    for (;;) {
        usage_field_lookup_test_note_work();
        if (depth == 0) {
            return false;
        }
        TSNode node = state->frames[depth].node;
        TSNode parent = state->frames[depth - 1].node;
        if (is_labeled_argument_kind(ts_node_type(parent))) {
            TSNode value = ts_node_child_by_field_name(parent, TS_FIELD("value"));
            if (ts_node_is_null(value) || !ts_node_eq(value, node)) {
                return false;
            }
            depth--;
            continue;
        }
        TSNode direct_arguments = ts_node_child_by_field_name(parent, TS_FIELD("arguments"));
        if (!ts_node_is_null(direct_arguments) && ts_node_eq(direct_arguments, node)) {
            return true;
        }
        if (is_argument_container_kind(ts_node_type(parent))) {
            return true;
        }
        const char *parent_kind = ts_node_type(parent);
        if (strcmp(parent_kind, "list_expression") == 0) {
            if (depth < 2) {
                return false;
            }
            TSNode arguments =
                ts_node_child_by_field_name(state->frames[depth - 2].node, TS_FIELD("arguments"));
            return !ts_node_is_null(arguments) && ts_node_eq(arguments, parent);
        }
        if (strcmp(parent_kind, "argument") != 0 && strcmp(parent_kind, "value_argument") != 0) {
            return false;
        }
        return depth >= 2 && is_argument_container_kind(ts_node_type(state->frames[depth - 2].node));
    }
}

/* is_direct_argument_value_from over the frames: the node below the frame at
 * `parent_depth` occupies `field` of it. */
static bool walk_is_direct_argument_value_from(WalkState *state, uint32_t parent_depth,
                                               const char *field) {
    for (;;) {
        usage_field_lookup_test_note_work();
        const char *parent_kind = ts_node_type(state->frames[parent_depth].node);
        if (is_labeled_argument_kind(parent_kind)) {
            if (!field || strcmp(field, "value") != 0) {
                return false;
            }
            /* The labeled argument now has to be the argument value itself. */
            if (parent_depth == 0) {
                return false;
            }
            field = state->frames[parent_depth].field;
            parent_depth--;
            continue;
        }
        if (field && strcmp(field, "arguments") == 0) {
            return true;
        }
        if (is_argument_container_kind(parent_kind)) {
            return true;
        }
        if (strcmp(parent_kind, "list_expression") == 0) {
            const char *list_field = state->frames[parent_depth].field;
            return parent_depth > 0 && list_field && strcmp(list_field, "arguments") == 0;
        }
        if (strcmp(parent_kind, "argument") != 0 && strcmp(parent_kind, "value_argument") != 0) {
            return false;
        }
        return parent_depth > 0 &&
               is_argument_container_kind(ts_node_type(state->frames[parent_depth - 1].node));
    }
}

/* is_direct_argument_value_cursor over the frames, for the node at `depth`. */
static bool walk_is_direct_argument_value_cursor(WalkState *state, uint32_t depth) {
    usage_field_lookup_test_note_work();
    return depth > 0 &&
           walk_is_direct_argument_value_from(state, depth - 1, state->frames[depth].field);
}

static bool lisp_def_head(const char *text) {
    static const char *const heads[] = {"defn",
                                        "defn-",
                                        "def",
                                        "defmacro",
                                        "defmulti",
                                        "defmethod",
                                        "defprotocol",
                                        "defrecord",
                                        "deftype",
                                        "definterface",
                                        "defonce",
                                        "define",
                                        "define-syntax",
                                        "define-values",
                                        "define-struct",
                                        "define-record-type",
                                        "define/contract",
                                        "struct",
                                        NULL};
    return kind_in_exact_set(text, heads);
}

/* Chialisp heads whose THIRD form is a parameter list, so the symbols in it
 * bind rather than refer: `(defun NAME (params) body)`. Deliberately not
 * `defconstant` — its third form is the VALUE expression, whose symbols are
 * genuine usages — and not `mod`, whose binder is the second form and is
 * already covered by the shared named_child(1) rule below. */
static bool chialisp_head_binds_params_at_2(const char *head) {
    return head && (strcmp(head, "defun") == 0 || strcmp(head, "defun-inline") == 0 ||
                    strcmp(head, "defmacro") == 0 || strcmp(head, "defmac") == 0);
}

static bool is_lisp_def_binding(PDXEExtractCtx *ctx, TSNode node) {
    bool chialisp = (ctx->language == PDXE_LANG_CHIALISP);
    for (TSNode form = ts_node_parent(node); !ts_node_is_null(form); form = ts_node_parent(form)) {
        const char *kind = ts_node_type(form);
        if ((strcmp(kind, "list") != 0 && strcmp(kind, "list_lit") != 0) ||
            ts_node_named_child_count(form) < 2) {
            continue;
        }
        TSNode head_node =
            chialisp ? pdxe_lisp_named_child_skip_comments(form, 0) : ts_node_named_child(form, 0);
        if (ts_node_is_null(head_node)) {
            continue;
        }
        char *head = pdxe_node_text(ctx->arena, head_node, ctx->source);
        if (!(chialisp ? pdxe_chialisp_is_def_head(head) : lisp_def_head(head))) {
            continue;
        }
        if (node_contains(head_node, node) || named_child_contains(form, 1, node)) {
            return true;
        }
        if ((ctx->language == PDXE_LANG_CLOJURE ||
             (chialisp && chialisp_head_binds_params_at_2(head))) &&
            ts_node_named_child_count(form) > 2 && named_child_contains(form, 2, node)) {
            return true;
        }
        return false;
    }
    return false;
}

static bool is_fennel_fn_binding(PDXEExtractCtx *ctx, TSNode node) {
    for (TSNode form = ts_node_parent(node); !ts_node_is_null(form); form = ts_node_parent(form)) {
        if (strcmp(ts_node_type(form), "list") != 0 || ts_node_named_child_count(form) < 2 ||
            !text_equals(ctx, ts_node_named_child(form, 0), "fn")) {
            continue;
        }
        TSNode first = ts_node_named_child(form, 1);
        const char *first_kind = ts_node_type(first);
        bool anonymous = strcmp(first_kind, "sequence") == 0 || strcmp(first_kind, "table") == 0 ||
                         strcmp(first_kind, "vector") == 0;
        if (anonymous) {
            return named_child_contains(form, 0, node) || node_contains(first, node);
        }
        return named_child_contains(form, 0, node) || node_contains(first, node) ||
               (ts_node_named_child_count(form) > 2 && named_child_contains(form, 2, node));
    }
    return false;
}

static bool is_elixir_def_binding(PDXEExtractCtx *ctx, TSNode node) {
    for (TSNode form = ts_node_parent(node); !ts_node_is_null(form); form = ts_node_parent(form)) {
        if (strcmp(ts_node_type(form), "call") != 0 || ts_node_named_child_count(form) < 2) {
            continue;
        }
        TSNode head = ts_node_named_child(form, 0);
        if (!text_equals(ctx, head, "def") && !text_equals(ctx, head, "defp") &&
            !text_equals(ctx, head, "defmacro")) {
            continue;
        }
        TSNode arguments = ts_node_child_by_field_name(form, TS_FIELD("arguments"));
        if (ts_node_is_null(arguments) || ts_node_named_child_count(arguments) == 0) {
            arguments = ts_node_named_child(form, 1);
        }
        TSNode signature = ts_node_named_child_count(arguments) > 0
                               ? ts_node_named_child(arguments, 0)
                               : arguments;
        return node_contains(head, node) || node_contains(signature, node);
    }
    return false;
}

static bool is_first_named_part_of(TSNode node, const char *container_kind) {
    for (TSNode parent = ts_node_parent(node); !ts_node_is_null(parent);
         parent = ts_node_parent(parent)) {
        if (strcmp(ts_node_type(parent), container_kind) == 0) {
            return named_child_contains(parent, 0, node);
        }
    }
    return false;
}

static bool is_wolfram_lhs(TSNode node) {
    static const char *const set_nodes[] = {
        "set",     "set_top",     "set_delayed",     "set_delayed_top",
        "tag_set", "tag_set_top", "tag_set_delayed", "tag_set_delayed_top",
        "up_set",  "up_set_top",  "up_set_delayed",  "up_set_delayed_top",
        NULL};
    for (TSNode parent = ts_node_parent(node); !ts_node_is_null(parent);
         parent = ts_node_parent(parent)) {
        if (kind_in_exact_set(ts_node_type(parent), set_nodes)) {
            return named_child_contains(parent, 0, node);
        }
    }
    return false;
}

static bool any_field_contains_node(TSNode parent, const char *field, TSNode node);

static bool is_tlaplus_binding(TSNode node) {
    for (TSNode parent = ts_node_parent(node); !ts_node_is_null(parent);
         parent = ts_node_parent(parent)) {
        const char *kind = ts_node_type(parent);
        if (strcmp(kind, "operator_definition") == 0) {
            /* `name:` is the callable declaration, while every repeated
             * `parameter:` field is a function-wide lexical binder. */
            return any_field_contains_node(parent, "parameter", node);
        }
        if (strcmp(kind, "function_definition") == 0) {
            /* `F[x \in S] == ...`: only quantifier_bound.intro binds. The set
             * expression S remains an ordinary identifier_ref usage. */
            for (TSNode bound = ts_node_parent(node);
                 !ts_node_is_null(bound) && !ts_node_eq(bound, parent);
                 bound = ts_node_parent(bound)) {
                if (strcmp(ts_node_type(bound), "quantifier_bound") == 0) {
                    return any_field_contains_node(bound, "intro", node);
                }
            }
            return false;
        }
        if (strcmp(kind, "bounded_quantification") == 0) {
            for (TSNode bound = ts_node_parent(node);
                 !ts_node_is_null(bound) && !ts_node_eq(bound, parent);
                 bound = ts_node_parent(bound)) {
                if (strcmp(ts_node_type(bound), "quantifier_bound") == 0) {
                    return any_field_contains_node(bound, "intro", node);
                }
            }
            return false;
        }
        if (strcmp(kind, "unbounded_quantification") == 0) {
            return any_field_contains_node(parent, "intro", node);
        }
    }
    return false;
}

static bool is_cobol_move_destination(TSNode node) {
    for (TSNode parent = ts_node_parent(node); !ts_node_is_null(parent);
         parent = ts_node_parent(parent)) {
        if (strcmp(ts_node_type(parent), "move_statement") != 0) {
            continue;
        }
        if (field_contains_node(parent, "destination", node) ||
            field_contains_node(parent, "target", node)) {
            return true;
        }
        uint32_t count = ts_node_named_child_count(parent);
        return count > 1 && named_child_contains(parent, count - 1, node);
    }
    return false;
}

/* Some grammars expose declaration roles as repeated/inherited fields. The
 * public child-by-field helper returns only one match, so inspect every direct
 * child and preserve the exact field role while allowing nested wrappers. */
static bool any_field_contains_node(TSNode parent, const char *field, TSNode node) {
    uint32_t count = ts_node_child_count(parent);
    for (uint32_t i = 0; i < count; i++) {
        const char *child_field = ts_node_field_name_for_child(parent, i);
        if (child_field && strcmp(child_field, field) == 0 &&
            node_contains(ts_node_child(parent, i), node)) {
            return true;
        }
    }
    return false;
}

static bool is_perl_lexical_declaration_binding(TSNode node) {
    for (TSNode parent = ts_node_parent(node); !ts_node_is_null(parent);
         parent = ts_node_parent(parent)) {
        if (strcmp(ts_node_type(parent), "variable_declaration") == 0) {
            /* Perl permits repeated `variables:` fields for `my ($a, $b)`;
             * inspect all direct field instances, not only the first. */
            return any_field_contains_node(parent, "variables", node);
        }
        if (strcmp(ts_node_type(parent), "assignment_expression") == 0) {
            return false;
        }
    }
    return false;
}

static bool is_cmake_function_parameter(TSNode node) {
    if (strcmp(ts_node_type(node), "unquoted_argument") != 0) {
        return false;
    }
    TSNode argument = ts_node_parent(node);
    TSNode arguments = ts_node_parent(argument);
    TSNode command = ts_node_parent(arguments);
    if (ts_node_is_null(argument) || strcmp(ts_node_type(argument), "argument") != 0 ||
        ts_node_is_null(arguments) || strcmp(ts_node_type(arguments), "argument_list") != 0 ||
        ts_node_is_null(command) ||
        (strcmp(ts_node_type(command), "function_command") != 0 &&
         strcmp(ts_node_type(command), "macro_command") != 0)) {
        return false;
    }
    uint32_t count = ts_node_named_child_count(arguments);
    for (uint32_t i = 1; i < count; i++) {
        if (node_contains(ts_node_named_child(arguments, i), node)) {
            return true;
        }
    }
    return false;
}

static bool fish_argument_marker(PDXEExtractCtx *ctx, TSNode node) {
    char *text = pdxe_node_text(ctx->arena, node, ctx->source);
    return text && (strcmp(text, "-a") == 0 || strcmp(text, "--argument") == 0 ||
                    strcmp(text, "--argument-names") == 0);
}

static bool is_fish_function_parameter(PDXEExtractCtx *ctx, TSNode node, WalkState *state) {
    if (strcmp(ts_node_type(node), "word") != 0) {
        return false;
    }
    TSNode definition = ts_node_parent(node);
    const char *field =
        ts_node_is_null(definition) ? NULL : field_name_for_walk_node(state, definition, node);
    if (!field || strcmp(field, "option") != 0 ||
        strcmp(ts_node_type(definition), "function_definition") != 0) {
        return false;
    }
    for (TSNode previous = ts_node_prev_named_sibling(node); !ts_node_is_null(previous);
         previous = ts_node_prev_named_sibling(previous)) {
        char *text = pdxe_node_text(ctx->arena, previous, ctx->source);
        if (!text || text[0] != '-') {
            continue;
        }
        return fish_argument_marker(ctx, previous);
    }
    return false;
}

static bool is_tcl_procedure_parameter(TSNode node) {
    TSNode argument = ts_node_parent(node);
    if (ts_node_is_null(argument) || strcmp(ts_node_type(argument), "argument") != 0 ||
        !any_field_contains_node(argument, "name", node)) {
        return false;
    }
    TSNode arguments = ts_node_parent(argument);
    TSNode procedure = ts_node_parent(arguments);
    return !ts_node_is_null(arguments) && strcmp(ts_node_type(arguments), "arguments") == 0 &&
           !ts_node_is_null(procedure) && strcmp(ts_node_type(procedure), "procedure") == 0 &&
           any_field_contains_node(procedure, "arguments", node);
}

static bool cfml_argument_tag_name_binding(PDXEExtractCtx *ctx, TSNode node) {
    if (strcmp(ts_node_type(node), "attribute_value") != 0) {
        return false;
    }
    TSNode quoted = ts_node_parent(node);
    TSNode attribute = ts_node_parent(quoted);
    if (ts_node_is_null(attribute) || strcmp(ts_node_type(attribute), "cf_attribute") != 0) {
        return false;
    }
    TSNode attribute_name = pdxe_find_child_by_kind(attribute, "cf_attribute_name");
    char *name_text = pdxe_node_text(ctx->arena, attribute_name, ctx->source);
    if (!name_text || strcasecmp(name_text, "name") != 0) {
        return false;
    }
    TSNode tag = ts_node_parent(attribute);
    if (ts_node_is_null(tag) || strcmp(ts_node_type(tag), "cf_selfclose_tag") != 0) {
        return false;
    }
    char *tag_text = pdxe_node_text(ctx->arena, tag, ctx->source);
    static const char argument_tag[] = "<cfargument";
    size_t prefix_length = sizeof(argument_tag) - 1U;
    if (!tag_text || strncasecmp(tag_text, argument_tag, prefix_length) != 0 ||
        (tag_text[prefix_length] != '\0' && !isspace((unsigned char)tag_text[prefix_length]) &&
         tag_text[prefix_length] != '>' && tag_text[prefix_length] != '/')) {
        return false;
    }
    for (TSNode parent = ts_node_parent(tag); !ts_node_is_null(parent);
         parent = ts_node_parent(parent)) {
        if (strcmp(ts_node_type(parent), "cf_function_tag") == 0) {
            return true;
        }
    }
    return false;
}

static bool is_exact_language_binding(PDXEExtractCtx *ctx, TSNode node, WalkState *state) {
    const char *kind = ts_node_type(node);
    switch (ctx->language) {
    case PDXE_LANG_OCAML: {
        if (strcmp(kind, "value_pattern") != 0) {
            return false;
        }
        TSNode parameter = ts_node_parent(node);
        return !ts_node_is_null(parameter) && strcmp(ts_node_type(parameter), "parameter") == 0 &&
               field_contains_node(parameter, "pattern", node);
    }
    case PDXE_LANG_SCSS: {
        if (strcmp(kind, "variable_name") != 0) {
            return false;
        }
        TSNode parameter = ts_node_parent(node);
        return !ts_node_is_null(parameter) && strcmp(ts_node_type(parameter), "parameter") == 0;
    }
    case PDXE_LANG_FORM: {
        if (strcmp(kind, "parameter") != 0) {
            return false;
        }
        TSNode parameters = ts_node_parent(node);
        return !ts_node_is_null(parameters) &&
               strcmp(ts_node_type(parameters), "parameter_list") == 0;
    }
    case PDXE_LANG_FUNC: {
        if (strcmp(kind, "parameter") != 0) {
            return false;
        }
        TSNode declaration = ts_node_parent(node);
        return !ts_node_is_null(declaration) &&
               strcmp(ts_node_type(declaration), "parameter_declaration") == 0 &&
               field_contains_node(declaration, "name", node);
    }
    case PDXE_LANG_PERL: {
        uint32_t depth;
        if (walk_depth_of(state, node, &depth)) {
            uint32_t nearest = walk_nearest_above(state, depth, PDXE_NEAREST_PERL_DECLARATION);
            if (nearest == 0) {
                return false;
            }
            TSNode declaration = state->frames[nearest - 1].node;
            /* Perl permits repeated `variables:` fields for `my ($a, $b)`; inspect all
             * direct field instances, not only the first. */
            return strcmp(ts_node_type(declaration), "variable_declaration") == 0 &&
                   any_field_contains_node(declaration, "variables", node);
        }
        usage_slow_parent_fallback_test_note();
        return is_perl_lexical_declaration_binding(node);
    }
    case PDXE_LANG_CMAKE:
        return is_cmake_function_parameter(node);
    case PDXE_LANG_FISH:
        return is_fish_function_parameter(ctx, node, state);
    case PDXE_LANG_TCL:
        return is_tcl_procedure_parameter(node);
    case PDXE_LANG_CFML:
        return cfml_argument_tag_name_binding(ctx, node);
    default:
        return false;
    }
}

static bool ancestor_field_binds(TSNode node, const char *container_kind,
                                 const char *const *fields) {
    for (TSNode parent = ts_node_parent(node); !ts_node_is_null(parent);
         parent = ts_node_parent(parent)) {
        if (strcmp(ts_node_type(parent), container_kind) != 0) {
            continue;
        }
        for (const char *const *field = fields; *field; field++) {
            if (any_field_contains_node(parent, *field, node)) {
                return true;
            }
        }
        return false;
    }
    return false;
}

static bool is_erlang_clause_binding(TSNode node) {
    static const char *const fields[] = {"args", NULL};
    return ancestor_field_binds(node, "function_clause", fields);
}

static bool is_nix_function_binding(TSNode node) {
    /* tree-sitter-nix names a simple `x: body` binder `universal`; set
     * destructuring uses the distinct `formals` field. */
    static const char *const fields[] = {"universal", "formals", NULL};
    return ancestor_field_binds(node, "function_expression", fields);
}

static bool is_lean_binder_name(TSNode node) {
    static const char *const binder_kinds[] = {"explicit_binder", "implicit_binder",
                                               "instance_binder", NULL};
    for (TSNode parent = ts_node_parent(node); !ts_node_is_null(parent);
         parent = ts_node_parent(parent)) {
        if (kind_in_exact_set(ts_node_type(parent), binder_kinds)) {
            return any_field_contains_node(parent, "name", node);
        }
    }
    return false;
}

static bool is_pascal_proc_binding(TSNode node) {
    static const char *const fields[] = {"args", NULL};
    return ancestor_field_binds(node, "declProc", fields) ||
           ancestor_field_binds(node, "defProc", fields);
}

static bool is_teal_function_binding(TSNode node) {
    TSNode arguments = {0};
    for (TSNode current = ts_node_parent(node); !ts_node_is_null(current);
         current = ts_node_parent(current)) {
        const char *kind = ts_node_type(current);
        if (ts_node_is_null(arguments) && strcmp(kind, "arguments") == 0) {
            arguments = current;
            continue;
        }
        if (strcmp(kind, "function_signature") == 0) {
            return !ts_node_is_null(arguments) &&
                   any_field_contains_node(current, "arguments", node);
        }
        if (strcmp(kind, "function_statement") == 0) {
            if (ts_node_is_null(arguments)) {
                return false;
            }
            TSNode signature = ts_node_child_by_field_name(current, TS_FIELD("signature"));
            return node_contains(signature, arguments);
        }
    }
    return false;
}

static bool is_commonlisp_defun_binding(TSNode node) {
    for (TSNode parent = ts_node_parent(node); !ts_node_is_null(parent);
         parent = ts_node_parent(parent)) {
        if (strcmp(ts_node_type(parent), "defun_header") != 0) {
            continue;
        }
        return field_contains_node(parent, "function_name", node) ||
               field_contains_node(parent, "lambda_list", node);
    }
    return false;
}

static bool is_hcl_attribute_binding(TSNode node) {
    for (TSNode parent = ts_node_parent(node); !ts_node_is_null(parent);
         parent = ts_node_parent(parent)) {
        if (strcmp(ts_node_type(parent), "attribute") == 0) {
            /* HCL's attribute production exposes no fields. The first named
             * child is its key; the expression is the second named child. */
            return named_child_contains(parent, 0, node);
        }
    }
    return false;
}

static bool is_matlab_argument_binding(TSNode node) {
    for (TSNode parent = ts_node_parent(node); !ts_node_is_null(parent);
         parent = ts_node_parent(parent)) {
        const char *kind = ts_node_type(parent);
        if (strcmp(kind, "function_arguments") == 0 || strcmp(kind, "lambda_arguments") == 0) {
            return true;
        }
    }
    return false;
}

static bool is_vhdl_interface_binding(TSNode node) {
    static const char *const interface_kinds[] = {
        "interface_constant_declaration", "interface_signal_declaration",
        "interface_variable_declaration", "interface_declaration", NULL};
    for (TSNode parent = ts_node_parent(node); !ts_node_is_null(parent);
         parent = ts_node_parent(parent)) {
        if (kind_in_exact_set(ts_node_type(parent), interface_kinds)) {
            /* FIELD_COUNT does not name this role; every interface production
             * starts with its identifier_list before mode/type/default. */
            return named_child_contains(parent, 0, node);
        }
    }
    return false;
}

static bool is_pine_function_binding(TSNode node) {
    static const char *const fields[] = {"argument", NULL};
    return ancestor_field_binds(node, "function_declaration_statement", fields);
}

/* Pkl declares names positionally: no production labels the declared identifier
 * with a `name` field, so the generic declared-container rule never binds them
 * and every method name, parameter name, and property name would be re-emitted
 * as an ordinary read of itself. Each container below holds its declared name
 * as named child 0; annotations, defaults, and bodies follow it and stay reads.
 * Resolve against the NEAREST container so a nested declaration's own name is
 * the only occurrence its parent can bind. */
static bool is_pkl_declaration_binding(TSNode node) {
    static const char *const declaration_kinds[] = {"methodHeader",
                                                    "typedIdentifier",
                                                    "classProperty",
                                                    "objectProperty",
                                                    "clazz",
                                                    "typeAlias",
                                                    NULL};
    for (TSNode parent = ts_node_parent(node); !ts_node_is_null(parent);
         parent = ts_node_parent(parent)) {
        if (kind_in_exact_set(ts_node_type(parent), declaration_kinds)) {
            return named_child_contains(parent, 0, node);
        }
    }
    return false;
}

static bool is_policy_binding(PDXEExtractCtx *ctx, TSNode node,
                              const PDXEOccurrenceSpec *occurrence, WalkState *state) {
    switch (occurrence->policy) {
    case PDXE_OCCURRENCE_LISP_DEF:
        return is_lisp_def_binding(ctx, node);
    case PDXE_OCCURRENCE_COMMONLISP_DEFUN:
        return is_commonlisp_defun_binding(node);
    case PDXE_OCCURRENCE_FENNEL_FN:
        return is_fennel_fn_binding(ctx, node);
    case PDXE_OCCURRENCE_ELIXIR_DEF:
        return is_elixir_def_binding(ctx, node);
    case PDXE_OCCURRENCE_JULIA_FUNCTION:
        return is_first_named_part_of(node, "function_definition");
    case PDXE_OCCURRENCE_WOLFRAM_SET:
        return is_wolfram_lhs(node);
    case PDXE_OCCURRENCE_TYPST_LET:
        return is_first_named_part_of(node, "let");
    case PDXE_OCCURRENCE_AGDA_FUNCTION:
        for (TSNode parent = ts_node_parent(node); !ts_node_is_null(parent);
             parent = ts_node_parent(parent)) {
            if (strcmp(ts_node_type(parent), "lhs") == 0) {
                return true;
            }
        }
        return false;
    case PDXE_OCCURRENCE_TLAPLUS_OPERATOR:
        return is_tlaplus_binding(node);
    case PDXE_OCCURRENCE_COBOL_MOVE:
        return is_cobol_move_destination(node);
    case PDXE_OCCURRENCE_HCL_ATTRIBUTE: {
        uint32_t depth;
        if (walk_depth_of(state, node, &depth)) {
            uint32_t nearest = walk_nearest_above(state, depth, PDXE_NEAREST_HCL_ATTRIBUTE);
            return nearest != 0 && named_child_contains(state->frames[nearest - 1].node, 0, node);
        }
        usage_slow_parent_fallback_test_note();
        return is_hcl_attribute_binding(node);
    }
    case PDXE_OCCURRENCE_ELM_VALUE:
        return is_first_named_part_of(node, "value_declaration");
    case PDXE_OCCURRENCE_RESCRIPT_LET:
        return is_first_named_part_of(node, "let_binding");
    case PDXE_OCCURRENCE_PURESCRIPT_LHS:
        return is_first_named_part_of(node, "function");
    case PDXE_OCCURRENCE_NICKEL_LET:
        return is_first_named_part_of(node, "let_binding") ||
               is_first_named_part_of(node, "pattern_fun");
    case PDXE_OCCURRENCE_ERLANG_CLAUSE:
        return is_erlang_clause_binding(node);
    case PDXE_OCCURRENCE_NIX_FUNCTION:
        return is_nix_function_binding(node);
    case PDXE_OCCURRENCE_MATLAB_ARGUMENTS:
        return is_matlab_argument_binding(node);
    case PDXE_OCCURRENCE_LEAN_BINDER:
        return is_lean_binder_name(node);
    case PDXE_OCCURRENCE_PASCAL_PROC:
        return is_pascal_proc_binding(node);
    case PDXE_OCCURRENCE_TEAL_FUNCTION:
        return is_teal_function_binding(node);
    case PDXE_OCCURRENCE_VHDL_INTERFACE:
        return is_vhdl_interface_binding(node);
    case PDXE_OCCURRENCE_PINE_FUNCTION:
        return is_pine_function_binding(node);
    case PDXE_OCCURRENCE_PKL_DECLARATION:
        return is_pkl_declaration_binding(node);
    case PDXE_OCCURRENCE_LLVM_FUNCTION:
        for (TSNode parent = ts_node_parent(node); !ts_node_is_null(parent);
             parent = ts_node_parent(parent)) {
            if (strcmp(ts_node_type(parent), "function_header") == 0) {
                return field_contains_node(parent, "arguments", node);
            }
        }
        return false;
    case PDXE_OCCURRENCE_STANDARD:
    default:
        return false;
    }
}

static bool elixir_binary_operator_binds(PDXEExtractCtx *ctx, TSNode node) {
    if (ctx->language != PDXE_LANG_ELIXIR || strcmp(ts_node_type(node), "binary_operator") != 0) {
        return false;
    }
    TSNode operator_node = ts_node_child_by_field_name(node, TS_FIELD("operator"));
    return text_equals(ctx, operator_node, "=") || text_equals(ctx, operator_node, "<-") ||
           text_equals(ctx, operator_node, "->") || text_equals(ctx, operator_node, "\\\\");
}

/* One step of is_binding_occurrence's climb: what `parent`, which the node below on the
 * path occupies as `field`, says about `node`. */
static PDXEClimbStep binding_step(PDXEExtractCtx *ctx, const PDXELangSpec *spec, TSNode parent,
                                  const char *field, TSNode node) {
    const PDXEOccurrenceSpec *occurrence = &occurrence_specs[ctx->language];
    if (is_value_field(field)) {
        return PDXE_STEP_FALSE;
    }
    /* A declaration container binds its name/pattern, not the symbols used by its type
     * annotation.  Both TypeScript (`cfg: Config`) and Go (`cfg Config`) expose that
     * annotation through the exact `type` field; stop before the whole-parameter
     * binding rule can swallow `Config`. The emitted occurrence remains an ordinary
     * USAGE, never a callable reference merely because the target happens to be a
     * type. */
    if (field && strcmp(field, "type") == 0) {
        return PDXE_STEP_FALSE;
    }

    const char *kind = ts_node_type(parent);
    /* PL/SQL: `parameter` is a ref_call ARGUMENT wrapper (upstream grammar naming), not
     * a declaration; definition-side bindings use the distinct parameter_declaration
     * kind. Skip it so call arguments stay ordinary value usages. */
    if (ctx->language == PDXE_LANG_PLSQL && strcmp(kind, "parameter") == 0) {
        return PDXE_STEP_CONTINUE;
    }
    if (kind_in_exact_set(kind, common_whole_binding_nodes) ||
        kind_in_exact_set(kind, occurrence->whole_binding_nodes)) {
        return PDXE_STEP_TRUE;
    }

    bool variable_container =
        spec->variable_node_types && pdxe_kind_in_set(parent, spec->variable_node_types);
    if (ctx->language == PDXE_LANG_ELIXIR && strcmp(kind, "binary_operator") == 0) {
        variable_container = elixir_binary_operator_binds(ctx, parent);
    }
    bool declared_container =
        kind_in_exact_set(kind, field_binding_nodes) ||
        (spec->function_node_types && pdxe_kind_in_set(parent, spec->function_node_types)) ||
        (spec->class_node_types && pdxe_kind_in_set(parent, spec->class_node_types)) ||
        (spec->field_node_types && pdxe_kind_in_set(parent, spec->field_node_types)) ||
        variable_container;
    if (declared_container) {
        for (const char *const *binding_field = binding_fields; *binding_field; binding_field++) {
            if (field_contains_node(parent, *binding_field, node)) {
                return PDXE_STEP_TRUE;
            }
        }
    }
    return PDXE_STEP_CONTINUE;
}

static bool is_binding_occurrence(PDXEExtractCtx *ctx, TSNode node, const PDXELangSpec *spec,
                                  WalkState *state) {
    const PDXEOccurrenceSpec *occurrence = &occurrence_specs[ctx->language];
    if (is_exact_language_binding(ctx, node, state)) {
        return true;
    }
    if (is_policy_binding(ctx, node, occurrence, state)) {
        return true;
    }

    uint32_t depth;
    if (walk_depth_of(state, node, &depth)) {
        return walk_climb(ctx, spec, state, node, depth, PDXE_CLIMB_BINDING, binding_step);
    }
    TSNode current = node;
    TSTreeCursor *cursor = reset_occurrence_cursor(state, node);
    while (!ts_node_is_null(current)) {
        TSNode parent;
        const char *field;
        if (!occurrence_parent(cursor, current, &parent, &field)) {
            break;
        }
        PDXEClimbStep decided = binding_step(ctx, spec, parent, field, node);
        if (decided != PDXE_STEP_CONTINUE) {
            return decided == PDXE_STEP_TRUE;
        }
        current = parent;
    }
    return false;
}

static bool assignment_reads_target(TSNode assignment) {
    static const char *const read_write_nodes[] = {"augmented_assignment",
                                                   "augmented_assignment_expression",
                                                   "compound_assignment_expr",
                                                   "compound_assignment_expression",
                                                   "operator_assignment",
                                                   "operator_assign",
                                                   "update_exp",
                                                   "postfix_unary_expression",
                                                   "prefix_unary_expression",
                                                   NULL};
    if (kind_in_exact_set(ts_node_type(assignment), read_write_nodes)) {
        return true;
    }
    static const char *const read_write_operators[] = {"+=",
                                                       "-=",
                                                       "*=",
                                                       "/=",
                                                       "%=",
                                                       "&=",
                                                       "|=",
                                                       "^=",
                                                       "<<=",
                                                       ">>=",
                                                       "?"
                                                       "?=",
                                                       "++",
                                                       "--",
                                                       NULL};
    uint32_t count = ts_node_child_count(assignment);
    for (uint32_t i = 0; i < count; i++) {
        if (kind_in_exact_set(ts_node_type(ts_node_child(assignment, i)), read_write_operators)) {
            return true;
        }
    }
    return false;
}

/* One step of is_write_occurrence's climb, as binding_step. */
static PDXEClimbStep write_step(PDXEExtractCtx *ctx, const PDXELangSpec *spec, TSNode parent,
                                const char *field, TSNode node) {
    const PDXEOccurrenceSpec *occurrence = &occurrence_specs[ctx->language];
    if (is_value_field(field)) {
        return PDXE_STEP_FALSE;
    }
    bool assignment =
        (spec->assignment_node_types && pdxe_kind_in_set(parent, spec->assignment_node_types)) ||
        kind_in_exact_set(ts_node_type(parent), occurrence->write_nodes);
    if (ctx->language == PDXE_LANG_ELIXIR && strcmp(ts_node_type(parent), "binary_operator") == 0) {
        assignment = elixir_binary_operator_binds(ctx, parent);
    }
    if (!assignment) {
        return PDXE_STEP_CONTINUE;
    }
    if (assignment_reads_target(parent)) {
        return PDXE_STEP_FALSE;
    }
    if (ctx->language == PDXE_LANG_LINKERSCRIPT && strcmp(ts_node_type(parent), "assignment") == 0) {
        /* This grammar's assignment production has no field map. Its first direct child
         * is the lhs, even when that child is the anonymous location-counter token `.`. */
        return ts_node_child_count(parent) > 0 && node_contains(ts_node_child(parent, 0), node)
                   ? PDXE_STEP_TRUE
                   : PDXE_STEP_FALSE;
    }
    TSNode left = ts_node_child_by_field_name(parent, TS_FIELD("left"));
    if (node_contains(left, node) || field_contains_node(parent, "target", node) ||
        field_contains_node(parent, "destination", node)) {
        return PDXE_STEP_TRUE;
    }
    return occurrence->first_named_child_is_write && named_child_contains(parent, 0, node)
               ? PDXE_STEP_TRUE
               : PDXE_STEP_FALSE;
}

static bool is_write_occurrence(PDXEExtractCtx *ctx, TSNode node, const PDXELangSpec *spec,
                                WalkState *state) {
    uint32_t depth;
    if (walk_depth_of(state, node, &depth)) {
        return walk_climb(ctx, spec, state, node, depth, PDXE_CLIMB_WRITE, write_step);
    }
    TSNode current = node;
    TSTreeCursor *cursor = reset_occurrence_cursor(state, node);
    while (!ts_node_is_null(current)) {
        TSNode parent;
        const char *field;
        if (!occurrence_parent(cursor, current, &parent, &field)) {
            break;
        }
        PDXEClimbStep decided = write_step(ctx, spec, parent, field, node);
        if (decided != PDXE_STEP_CONTINUE) {
            return decided == PDXE_STEP_TRUE;
        }
        current = parent;
    }
    return false;
}

static bool is_argument_container_kind(const char *kind) {
    return kind && (strcmp(kind, "arguments") == 0 || strcmp(kind, "argument_list") == 0 ||
                    strcmp(kind, "value_arguments") == 0);
}

static bool is_labeled_argument_kind(const char *kind) {
    return kind && (strcmp(kind, "keyword_argument") == 0 || strcmp(kind, "named_argument") == 0 ||
                    strcmp(kind, "labeled_argument") == 0);
}

/* A named/labeled argument's key describes the callee parameter; it is not a
 * value read in the caller. Keep this separate from binding detection: the
 * label must neither emit USAGE nor create a caller-local lexical binding. */
static bool is_call_argument_label(TSNode node) {
    usage_slow_parent_fallback_test_note();
    for (TSNode current = node; !ts_node_is_null(current);) {
        TSNode parent = ts_node_parent(current);
        if (ts_node_is_null(parent)) {
            return false;
        }
        const char *parent_kind = ts_node_type(parent);
        if (is_labeled_argument_kind(parent_kind)) {
            return field_contains_node(parent, "name", node) ||
                   field_contains_node(parent, "label", node) ||
                   field_contains_node(parent, "key", node);
        }
        if (is_argument_container_kind(parent_kind)) {
            return false;
        }
        current = parent;
    }
    return false;
}

static bool is_call_argument_label_walk(TSNode node, WalkState *state) {
    uint32_t depth;
    if (walk_depth_of(state, node, &depth)) {
        /* The nearest labeled argument or argument list decides, by the field the
         * node's own ancestor below it occupies. */
        uint32_t nearest = walk_nearest_above(state, depth, PDXE_NEAREST_ARGUMENT_LABEL);
        if (nearest == 0) {
            return false;
        }
        if (!is_labeled_argument_kind(ts_node_type(state->frames[nearest - 1].node))) {
            return false;
        }
        const char *field = state->frames[nearest].field;
        return field && (strcmp(field, "name") == 0 || strcmp(field, "label") == 0 ||
                         strcmp(field, "key") == 0);
    }
    TSTreeCursor *cursor = reset_occurrence_cursor(state, node);
    if (!cursor)
        return is_call_argument_label(node);
    TSNode current = node;
    while (!ts_node_is_null(current)) {
        TSNode parent;
        const char *field;
        if (!occurrence_parent(cursor, current, &parent, &field))
            return false;
        const char *parent_kind = ts_node_type(parent);
        if (is_labeled_argument_kind(parent_kind)) {
            return field && (strcmp(field, "name") == 0 || strcmp(field, "label") == 0 ||
                             strcmp(field, "key") == 0);
        }
        if (is_argument_container_kind(parent_kind))
            return false;
        current = parent;
    }
    return false;
}

static bool is_direct_argument_value(TSNode node) {
    usage_slow_parent_fallback_test_note();
    TSNode parent = ts_node_parent(node);
    if (ts_node_is_null(parent)) {
        return false;
    }
    if (is_labeled_argument_kind(ts_node_type(parent))) {
        TSNode value = ts_node_child_by_field_name(parent, TS_FIELD("value"));
        return !ts_node_is_null(value) && ts_node_eq(value, node) &&
               is_direct_argument_value(parent);
    }
    TSNode direct_arguments = ts_node_child_by_field_name(parent, TS_FIELD("arguments"));
    if (!ts_node_is_null(direct_arguments) && ts_node_eq(direct_arguments, node)) {
        return true;
    }
    if (is_argument_container_kind(ts_node_type(parent))) {
        return true;
    }
    const char *parent_kind = ts_node_type(parent);
    if (strcmp(parent_kind, "list_expression") == 0) {
        TSNode call = ts_node_parent(parent);
        if (!ts_node_is_null(call)) {
            TSNode arguments = ts_node_child_by_field_name(call, TS_FIELD("arguments"));
            return !ts_node_is_null(arguments) && ts_node_eq(arguments, parent);
        }
        return false;
    }
    if (strcmp(parent_kind, "argument") != 0 && strcmp(parent_kind, "value_argument") != 0) {
        return false;
    }
    TSNode grandparent = ts_node_parent(parent);
    return !ts_node_is_null(grandparent) && is_argument_container_kind(ts_node_type(grandparent));
}

/* The body of the walk above, entered one level in: for a caller that has
 * ALREADY stepped the cursor onto `parent` and knows which `field` of it the
 * node below occupies. A caller that climbed to find its site has that pair in
 * hand, and re-deriving it would mean either stepping the cursor twice or
 * paying ts_node_parent for what the cursor just told us. */
static bool is_direct_argument_value_from(TSNode parent, const char *field, TSTreeCursor *cursor) {
    for (;;) {
        const char *parent_kind = ts_node_type(parent);
        if (is_labeled_argument_kind(parent_kind)) {
            if (!field || strcmp(field, "value") != 0)
                return false;
            /* The labeled argument now has to be the argument value itself. */
            if (!occurrence_parent(cursor, parent, &parent, &field))
                return false;
            continue;
        }
        if (field && strcmp(field, "arguments") == 0)
            return true;
        if (is_argument_container_kind(parent_kind))
            return true;
        if (strcmp(parent_kind, "list_expression") == 0) {
            TSNode call;
            const char *list_field;
            return occurrence_parent(cursor, parent, &call, &list_field) && list_field &&
                   strcmp(list_field, "arguments") == 0;
        }
        if (strcmp(parent_kind, "argument") != 0 && strcmp(parent_kind, "value_argument") != 0) {
            return false;
        }
        TSNode grandparent;
        const char *argument_field;
        return occurrence_parent(cursor, parent, &grandparent, &argument_field) &&
               is_argument_container_kind(ts_node_type(grandparent));
    }
}

/* Cursor-backed counterpart for the unified walker. `cursor` must currently
 * point at `node`; it is consumed while walking toward the argument owner. */
static bool is_direct_argument_value_cursor(TSNode node, TSTreeCursor *cursor) {
    TSNode parent;
    const char *field;
    if (!occurrence_parent(cursor, node, &parent, &field))
        return false;
    return is_direct_argument_value_from(parent, field, cursor);
}

static bool is_direct_argument_value_walk(TSNode node, WalkState *state) {
    TSTreeCursor *cursor = reset_occurrence_cursor(state, node);
    return cursor ? is_direct_argument_value_cursor(node, cursor) : is_direct_argument_value(node);
}

static TSNode python_direct_callable_attribute_site(TSNode node) {
    usage_slow_parent_fallback_test_note();
    if (ts_node_is_null(node) || strcmp(ts_node_type(node), "attribute") != 0) {
        return (TSNode){0};
    }
    TSNode site = node;
    TSNode parent = ts_node_parent(site);
    while (!ts_node_is_null(parent) &&
           strcmp(ts_node_type(parent), "parenthesized_expression") == 0) {
        TSNode inner = ts_node_child_by_field_name(parent, TS_FIELD("expression"));
        if (ts_node_is_null(inner) && ts_node_named_child_count(parent) == 1) {
            inner = ts_node_named_child(parent, 0);
        }
        if (ts_node_is_null(inner) || !ts_node_eq(inner, site)) {
            return (TSNode){0};
        }
        site = parent;
        parent = ts_node_parent(site);
    }
    return is_direct_argument_value(site) ? site : (TSNode){0};
}

/* python_direct_callable_attribute_site over the frames, for the node at `depth`. */
static TSNode walk_python_direct_callable_attribute_site(WalkState *state, uint32_t depth) {
    TSNode node = state->frames[depth].node;
    if (strcmp(ts_node_type(node), "attribute") != 0) {
        return (TSNode){0};
    }
    TSNode site = node;
    uint32_t site_depth = depth;
    while (site_depth > 0) {
        usage_field_lookup_test_note_work();
        TSNode parent = state->frames[site_depth - 1].node;
        if (strcmp(ts_node_type(parent), "parenthesized_expression") != 0) {
            break;
        }
        TSNode inner = ts_node_child_by_field_name(parent, TS_FIELD("expression"));
        if (ts_node_is_null(inner) && ts_node_named_child_count(parent) == 1) {
            inner = ts_node_named_child(parent, 0);
        }
        if (ts_node_is_null(inner) || !ts_node_eq(inner, site)) {
            return (TSNode){0};
        }
        site = parent;
        site_depth--;
    }
    return walk_is_direct_argument_value(state, site_depth) ? site : (TSNode){0};
}

static bool language_may_stamp_exact_callable_value_candidate(PDXELanguage language) {
    switch (language) {
    case PDXE_LANG_JAVASCRIPT:
    case PDXE_LANG_TYPESCRIPT:
    case PDXE_LANG_TSX:
    case PDXE_LANG_ARKTS:
    case PDXE_LANG_GO:
    case PDXE_LANG_PYTHON:
    case PDXE_LANG_C:
    case PDXE_LANG_CPP:
    case PDXE_LANG_CUDA:
    case PDXE_LANG_RUST:
    case PDXE_LANG_CSHARP:
    case PDXE_LANG_KOTLIN:
        return true;
    default:
        return false;
    }
}

/* Direct identifiers and direct, statically-named TS member values passed as
 * arguments are narrow syntactic candidates only. A language LSP must still
 * prove one target at this exact occurrence before the graph upgrades USAGE to
 * CALL_REFERENCE; unresolved, reassigned, and composite expressions stay USAGE. */
/* One step up from `site`, on the walk cursor when the caller has one. Every
 * other language branch in call_reference_candidate_site climbs this way; C#
 * used ts_node_parent, which tree-sitter answers by descending from the ROOT,
 * so each step costs O(depth) with a child scan at every level. On the
 * generated .NET JIT tests — single expressions megabytes deep — that was
 * 18-48 us per visited node and ~85% of the whole unified walk (sampled
 * 2026-09-19: stamp_usage_site -> ts_node_parent ->
 * ts_node_child_with_descendant). Those files are also the ones the old CPU
 * deadline used to cut off mid-walk, so the slow path and the nondeterminism
 * it forced were the same defect seen from two ends. */
static bool csharp_site_parent(TSTreeCursor *cursor, TSNode site, TSNode *parent,
                               const char **field) {
    if (!cursor) {
        usage_slow_parent_fallback_test_note();
    }
    return occurrence_parent(cursor, site, parent, field);
}

static TSNode csharp_callable_value_site(TSNode node, TSNode parent, const char *parent_field,
                                         TSTreeCursor *cursor) {
    const char *kind = ts_node_type(node);
    if (strcmp(kind, "identifier") != 0 && strcmp(kind, "simple_identifier") != 0) {
        return (TSNode){0};
    }

    /* The caller has already resolved node's parent (and the field it occupies)
     * with the cursor, so the climb starts from that pair instead of asking for
     * it again. Invariant below: `parent`/`field` always describe `site`. */
    TSNode site = node;
    const char *field = parent_field;
    if (!ts_node_is_null(parent) && strcmp(ts_node_type(parent), "generic_name") == 0) {
        TSNode name = ts_node_child_by_field_name(parent, TS_FIELD("name"));
        if (ts_node_is_null(name) && ts_node_named_child_count(parent) > 0) {
            name = ts_node_named_child(parent, 0);
        }
        if (ts_node_is_null(name) || !ts_node_eq(name, node)) {
            return (TSNode){0};
        }
        site = parent;
        if (!csharp_site_parent(cursor, site, &parent, &field)) {
            return (TSNode){0};
        }
    }

    if (!ts_node_is_null(parent) && strcmp(ts_node_type(parent), "member_access_expression") == 0) {
        TSNode member = ts_node_child_by_field_name(parent, TS_FIELD("name"));
        if (ts_node_is_null(member) || !ts_node_eq(member, site)) {
            return (TSNode){0};
        }
        site = parent;
        if (!csharp_site_parent(cursor, site, &parent, &field)) {
            return (TSNode){0};
        }
    }

    while (!ts_node_is_null(parent) &&
           strcmp(ts_node_type(parent), "parenthesized_expression") == 0) {
        TSNode inner = ts_node_child_by_field_name(parent, TS_FIELD("expression"));
        if (ts_node_is_null(inner) && ts_node_named_child_count(parent) == 1) {
            inner = ts_node_named_child(parent, 0);
        }
        if (ts_node_is_null(inner) || !ts_node_eq(inner, site)) {
            return (TSNode){0};
        }
        site = parent;
        if (!csharp_site_parent(cursor, site, &parent, &field)) {
            return (TSNode){0};
        }
    }

    /* The wrapper chain proves admission, but the semantic row is keyed to the
     * terminal method-name identifier. Keep the raw carrier on that leaf. */
    if (ts_node_is_null(parent)) {
        return (TSNode){0};
    }
    return is_direct_argument_value_from(parent, field, cursor) ? node : (TSNode){0};
}

/* Climb out of the parentheses wrapping a direct argument and return the
 * outermost wrapper, or null when the node is not a parenthesised direct
 * argument. Mirrors python_direct_callable_attribute_site so the bare
 * identifier and bound method forms agree on one occurrence. */
static TSNode paren_wrapped_direct_argument_site(TSNode node, TSNode parent, TSTreeCursor *cursor,
                                                 WalkState *state) {
    /* The caller already resolved `parent`; checking its kind first keeps the
     * common case (an identifier that is not parenthesised at all) free of any
     * extra parent resolution. Walking up with ts_node_parent here instead
     * costs one slow fallback per identifier in the file, which the linearity
     * guard in test_extraction.c rightly rejects. */
    if (ts_node_is_null(parent) || strcmp(ts_node_type(parent), "parenthesized_expression") != 0) {
        return (TSNode){0};
    }
    TSNode site = node;
    TSNode wrapper = parent;
    for (int depth = 0; depth < 64; depth++) {
        TSNode inner = ts_node_child_by_field_name(wrapper, TS_FIELD("expression"));
        if (ts_node_is_null(inner) && ts_node_named_child_count(wrapper) == 1) {
            inner = ts_node_named_child(wrapper, 0);
        }
        if (ts_node_is_null(inner) || !ts_node_eq(inner, site)) {
            return (TSNode){0};
        }
        site = wrapper;
        TSNode next = {0};
        const char *field = NULL;
        if (cursor) {
            if (!occurrence_parent(cursor, site, &next, &field)) {
                next = (TSNode){0};
            }
        } else {
            usage_slow_parent_fallback_test_note();
            next = ts_node_parent(site);
        }
        if (ts_node_is_null(next) || strcmp(ts_node_type(next), "parenthesized_expression") != 0) {
            break;
        }
        wrapper = next;
    }
    return is_direct_argument_value_walk(site, state) ? site : (TSNode){0};
}

/* csharp_callable_value_site over the frames: `node` is the walk's current node, at
 * `depth`. */
static TSNode walk_csharp_callable_value_site(WalkState *state, TSNode node, uint32_t depth) {
    const char *kind = ts_node_type(node);
    if (strcmp(kind, "identifier") != 0 && strcmp(kind, "simple_identifier") != 0) {
        return (TSNode){0};
    }
    /* Invariant: the site is the frame at site_depth, its parent the frame below. */
    TSNode site = node;
    uint32_t site_depth = depth;
    TSNode parent = site_depth > 0 ? state->frames[site_depth - 1].node : (TSNode){0};
    if (!ts_node_is_null(parent) && strcmp(ts_node_type(parent), "generic_name") == 0) {
        TSNode name = ts_node_child_by_field_name(parent, TS_FIELD("name"));
        if (ts_node_is_null(name) && ts_node_named_child_count(parent) > 0) {
            name = ts_node_named_child(parent, 0);
        }
        if (ts_node_is_null(name) || !ts_node_eq(name, node)) {
            return (TSNode){0};
        }
        site = parent;
        site_depth--;
        usage_field_lookup_test_note_work();
        if (site_depth == 0) {
            return (TSNode){0};
        }
        parent = state->frames[site_depth - 1].node;
    }
    if (!ts_node_is_null(parent) && strcmp(ts_node_type(parent), "member_access_expression") == 0) {
        TSNode member = ts_node_child_by_field_name(parent, TS_FIELD("name"));
        if (ts_node_is_null(member) || !ts_node_eq(member, site)) {
            return (TSNode){0};
        }
        site = parent;
        site_depth--;
        usage_field_lookup_test_note_work();
        if (site_depth == 0) {
            return (TSNode){0};
        }
        parent = state->frames[site_depth - 1].node;
    }
    while (!ts_node_is_null(parent) &&
           strcmp(ts_node_type(parent), "parenthesized_expression") == 0) {
        TSNode inner = ts_node_child_by_field_name(parent, TS_FIELD("expression"));
        if (ts_node_is_null(inner) && ts_node_named_child_count(parent) == 1) {
            inner = ts_node_named_child(parent, 0);
        }
        if (ts_node_is_null(inner) || !ts_node_eq(inner, site)) {
            return (TSNode){0};
        }
        site = parent;
        site_depth--;
        usage_field_lookup_test_note_work();
        if (site_depth == 0) {
            return (TSNode){0};
        }
        parent = state->frames[site_depth - 1].node;
    }
    if (ts_node_is_null(parent)) {
        return (TSNode){0};
    }
    return walk_is_direct_argument_value_from(state, site_depth - 1,
                                              state->frames[site_depth].field)
               ? node
               : (TSNode){0};
}

/* paren_wrapped_direct_argument_site over the frames: `node` is the walk's current
 * node, at `depth`. */
static TSNode walk_paren_wrapped_direct_argument_site(WalkState *state, uint32_t depth) {
    if (depth == 0 ||
        strcmp(ts_node_type(state->frames[depth - 1].node), "parenthesized_expression") != 0) {
        return (TSNode){0};
    }
    uint32_t site_depth = depth;
    uint32_t wrapper_depth = depth - 1;
    for (int hops = 0; hops < 64; hops++) {
        usage_field_lookup_test_note_work();
        TSNode wrapper = state->frames[wrapper_depth].node;
        TSNode inner = ts_node_child_by_field_name(wrapper, TS_FIELD("expression"));
        if (ts_node_is_null(inner) && ts_node_named_child_count(wrapper) == 1) {
            inner = ts_node_named_child(wrapper, 0);
        }
        if (ts_node_is_null(inner) || !ts_node_eq(inner, state->frames[site_depth].node)) {
            return (TSNode){0};
        }
        site_depth = wrapper_depth;
        if (wrapper_depth == 0 ||
            strcmp(ts_node_type(state->frames[wrapper_depth - 1].node),
                   "parenthesized_expression") != 0) {
            break;
        }
        wrapper_depth--;
    }
    return walk_is_direct_argument_value(state, site_depth) ? state->frames[site_depth].node
                                                            : (TSNode){0};
}

/* call_reference_candidate_site over the frames: `node` is the walk's current node, at
 * `depth`. Each branch asks what the cursor-backed one below asks. */
static TSNode walk_call_reference_candidate_site(PDXEExtractCtx *ctx, TSNode node,
                                                 WalkState *state, uint32_t depth) {
    const char *kind = ts_node_type(node);
    usage_field_lookup_test_note_work();
    TSNode parent = depth > 0 ? state->frames[depth - 1].node : (TSNode){0};
    bool ts_family = ctx->language == PDXE_LANG_JAVASCRIPT || ctx->language == PDXE_LANG_TYPESCRIPT ||
                     ctx->language == PDXE_LANG_TSX || ctx->language == PDXE_LANG_ARKTS;
    if (ts_family && strcmp(kind, "property_identifier") == 0 && !ts_node_is_null(parent) &&
        strcmp(ts_node_type(parent), "member_expression") == 0) {
        TSNode property = ts_node_child_by_field_name(parent, TS_FIELD("property"));
        TSNode arguments = depth >= 2 ? state->frames[depth - 2].node : (TSNode){0};
        return !ts_node_is_null(property) && ts_node_eq(property, node) &&
                       !ts_node_is_null(arguments) &&
                       strcmp(ts_node_type(arguments), "arguments") == 0
                   ? node
                   : (TSNode){0};
    }
    if (ctx->language == PDXE_LANG_PYTHON && strcmp(kind, "identifier") == 0 &&
        !ts_node_is_null(parent) && strcmp(ts_node_type(parent), "attribute") == 0) {
        TSNode attribute = ts_node_child_by_field_name(parent, TS_FIELD("attribute"));
        return !ts_node_is_null(attribute) && ts_node_eq(attribute, node)
                   ? walk_python_direct_callable_attribute_site(state, depth - 1)
                   : (TSNode){0};
    }
    if (ctx->language == PDXE_LANG_GO && strcmp(kind, "field_identifier") == 0 &&
        !ts_node_is_null(parent) && strcmp(ts_node_type(parent), "selector_expression") == 0) {
        TSNode field = ts_node_child_by_field_name(parent, TS_FIELD("field"));
        return !ts_node_is_null(field) && ts_node_eq(field, node) &&
                       walk_is_direct_argument_value_cursor(state, depth - 1)
                   ? parent
                   : (TSNode){0};
    }
    if (ctx->language == PDXE_LANG_RUST && strcmp(kind, "scoped_identifier") == 0) {
        return walk_is_direct_argument_value_cursor(state, depth) ? node : (TSNode){0};
    }
    if (ctx->language == PDXE_LANG_CSHARP) {
        return walk_csharp_callable_value_site(state, node, depth);
    }
    if (strcmp(kind, "identifier") != 0 && strcmp(kind, "simple_identifier") != 0) {
        return (TSNode){0};
    }
    if (ctx->language == PDXE_LANG_KOTLIN && !ts_node_is_null(parent) &&
        strcmp(ts_node_type(parent), "navigation_suffix") == 0) {
        return node;
    }
    if (ctx->language == PDXE_LANG_PYTHON && !ts_node_is_null(parent) &&
        strcmp(ts_node_type(parent), "assignment") == 0) {
        TSNode right = ts_node_child_by_field_name(parent, TS_FIELD("right"));
        return !ts_node_is_null(right) && ts_node_eq(right, node) ? node : (TSNode){0};
    }
    if (walk_is_direct_argument_value_cursor(state, depth)) {
        return node;
    }
    return walk_paren_wrapped_direct_argument_site(state, depth);
}

static TSNode call_reference_candidate_site(PDXEExtractCtx *ctx, TSNode node, const char *name,
                                            WalkState *state) {
    if (!ctx || !name || !language_may_stamp_exact_callable_value_candidate(ctx->language)) {
        return (TSNode){0};
    }
    uint32_t depth;
    if (walk_depth_of(state, node, &depth)) {
        return walk_call_reference_candidate_site(ctx, node, state, depth);
    }
    const char *kind = ts_node_type(node);
    TSTreeCursor *cursor = reset_occurrence_cursor(state, node);
    TSNode parent = {0};
    const char *parent_field = NULL;
    if (!cursor) {
        usage_slow_parent_fallback_test_note();
    }
    /* Resolve the parent AND the field node occupies in it, on both paths. The
     * C# site walk below carries that field into the argument test, and
     * ts_node_parent on its own would leave it NULL there — silently losing the
     * "this node IS the arguments" case whenever no cursor is available. */
    (void)occurrence_parent(cursor, node, &parent, &parent_field);
    bool ts_family = ctx->language == PDXE_LANG_JAVASCRIPT || ctx->language == PDXE_LANG_TYPESCRIPT ||
                     ctx->language == PDXE_LANG_TSX || ctx->language == PDXE_LANG_ARKTS;
    if (ts_family && strcmp(kind, "property_identifier") == 0 && !ts_node_is_null(parent) &&
        strcmp(ts_node_type(parent), "member_expression") == 0) {
        TSNode property = ts_node_child_by_field_name(parent, TS_FIELD("property"));
        TSNode arguments = {0};
        const char *member_field = NULL;
        if (cursor) {
            (void)occurrence_parent(cursor, parent, &arguments, &member_field);
        } else {
            usage_slow_parent_fallback_test_note();
            arguments = ts_node_parent(parent);
        }
        return !ts_node_is_null(property) && ts_node_eq(property, node) &&
                       !ts_node_is_null(arguments) &&
                       strcmp(ts_node_type(arguments), "arguments") == 0
                   ? node
                   : (TSNode){0};
    }
    if (ctx->language == PDXE_LANG_PYTHON && strcmp(kind, "identifier") == 0 &&
        !ts_node_is_null(parent) && strcmp(ts_node_type(parent), "attribute") == 0) {
        TSNode attribute = ts_node_child_by_field_name(parent, TS_FIELD("attribute"));
        return !ts_node_is_null(attribute) && ts_node_eq(attribute, node)
                   ? python_direct_callable_attribute_site(parent)
                   : (TSNode){0};
    }
    if (ctx->language == PDXE_LANG_GO && strcmp(kind, "field_identifier") == 0 &&
        !ts_node_is_null(parent) && strcmp(ts_node_type(parent), "selector_expression") == 0) {
        TSNode field = ts_node_child_by_field_name(parent, TS_FIELD("field"));
        return !ts_node_is_null(field) && ts_node_eq(field, node) &&
                       (cursor ? is_direct_argument_value_cursor(parent, cursor)
                               : is_direct_argument_value(parent))
                   ? parent
                   : (TSNode){0};
    }
    if (ctx->language == PDXE_LANG_RUST && strcmp(kind, "scoped_identifier") == 0) {
        return is_direct_argument_value_walk(node, state) ? node : (TSNode){0};
    }
    if (ctx->language == PDXE_LANG_CSHARP) {
        return csharp_callable_value_site(node, parent, parent_field, cursor);
    }
    if (strcmp(kind, "identifier") != 0 && strcmp(kind, "simple_identifier") != 0) {
        return (TSNode){0};
    }
    /* Kotlin: a navigation member read (`obj.member`, and `value::member`,
     * which the vendored grammar also parses as navigation) is a candidate at
     * the member occurrence, so a receiver-typed LSP row can claim it instead
     * of the name-only registry fallback (maintainer decision, option B). With
     * no row at the occurrence the join finds nothing and behavior is exactly
     * as before. */
    if (ctx->language == PDXE_LANG_KOTLIN && !ts_node_is_null(parent) &&
        strcmp(ts_node_type(parent), "navigation_suffix") == 0) {
        return node;
    }
    /* Python: the right-hand side of `callback = handler` is a genuine value
     * occurrence of handler, and when the LSP proves its exact callable
     * identity it becomes a CALL_REFERENCE like any proven argument value
     * (maintainer decision on the local-alias fixture). Bare identifier RHS
     * only, in lockstep with the row py_lsp.c emits -- a candidate the LSP
     * never matches would change nothing, but the asymmetry would be a trap. */
    if (ctx->language == PDXE_LANG_PYTHON && !ts_node_is_null(parent) &&
        strcmp(ts_node_type(parent), "assignment") == 0) {
        TSNode right = ts_node_child_by_field_name(parent, TS_FIELD("right"));
        return !ts_node_is_null(right) && ts_node_eq(right, node) ? node : (TSNode){0};
    }
    if (is_direct_argument_value_walk(node, state)) {
        return node;
    }
    /* `f((handler))` passes handler just as directly as `f(handler)`. The
     * semantic row is recorded on the outermost parenthesised wrapper -- the
     * site python_direct_callable_attribute_site already returns for the bound
     * method form -- so climb to that same wrapper here. A bare identifier
     * previously stopped at the parentheses and became no candidate at all, so
     * the occurrence-exact join had nothing to join and a parenthesised
     * callable argument produced no reference. */
    return paren_wrapped_direct_argument_site(node, parent, cursor, state);
}

static char *reference_name(PDXEExtractCtx *ctx, TSNode node) {
    char *name = pdxe_node_text(ctx->arena, node, ctx->source);
    if (!name || ctx->language != PDXE_LANG_MAKEFILE ||
        strcmp(ts_node_type(node), "variable_reference") != 0) {
        return name;
    }

    /* Make exposes `$(watched)` as one named reference node.
     * Normalize only that grammar wrapper; the recorded site still spans the
     * exact source occurrence for occurrence-sensitive resolution. */
    size_t length = strlen(name);
    bool parenthesized = length > 3 && name[0] == '$' && name[1] == '(' && name[length - 1] == ')';
    return parenthesized ? pdxe_arena_strndup(ctx->arena, name + 2, length - 3) : name;
}

static bool lexical_identifier_case_insensitive(PDXELanguage language) {
    switch (language) {
    case PDXE_LANG_POWERSHELL:
    case PDXE_LANG_FORTRAN:
    case PDXE_LANG_ADA:
    case PDXE_LANG_PASCAL:
    case PDXE_LANG_COBOL:
    case PDXE_LANG_VHDL:
        return true;
    default:
        return false;
    }
}

static const char *lexical_binding_key(PDXEExtractCtx *ctx, WalkState *state, const char *name) {
    if (!ctx || !name) {
        return name;
    }
    /* Preserve language namespaces: Perl sigils and Vim prefixes distinguish
     * different variables. Tcl's grammar is asymmetric (`name` versus
     * `$name`), so unwrap only that language-defined reference sigil. */
    if (ctx->language == PDXE_LANG_TCL && name[0] == '$') {
        name++;
    }
    if (!lexical_identifier_case_insensitive(ctx->language)) {
        return name;
    }
    size_t n = strlen(name);
    char *folded = (char *)pdxe_arena_alloc(ctx->arena, n + 1U);
    if (!folded) {
        if (state) {
            state->lexical_binding_tracking_failed = true;
        }
        return NULL;
    }
    for (size_t i = 0; i < n; i++) {
        folded[i] = (char)tolower((unsigned char)name[i]);
    }
    folded[n] = '\0';
    return folded;
}

static void stamp_usage_site(PDXEExtractCtx *ctx, PDXEUsage *usage, TSNode node, const char *name,
                             WalkState *state) {
    TSNode candidate_site = call_reference_candidate_site(ctx, node, name, state);
    TSNode site = ts_node_is_null(candidate_site) ? node : candidate_site;
    usage->site_start_byte = ts_node_start_byte(site);
    usage->site_end_byte = ts_node_end_byte(site);
    usage->may_be_call_reference = !ts_node_is_null(candidate_site);
}

static const PDXELexicalScope *usage_lexical_scope(const WalkState *state, uint32_t id) {
    return state && id > 0 && id <= (uint32_t)state->lexical_scope_count
               ? &state->lexical_scopes[id - 1U]
               : NULL;
}

/* The nearest open frame's lexical scope, the root's when none: kept by each frame
 * (issue 90), not found by scanning the stack. */
static uint32_t active_lexical_scope_id(const WalkState *state) {
    if (!state) {
        return 0;
    }
    return state->scope_top > 0 ? state->scopes[state->scope_top - 1].active_lexical_scope_id
                                : state->root_lexical_scope_id;
}

static uint32_t lexical_ancestor_of_kind(const WalkState *state, uint32_t start_id,
                                         bool want_function, bool want_block);

/* One step of python_default_value_reference's climb, as binding_step. */
static PDXEClimbStep python_default_step(PDXEExtractCtx *ctx, const PDXELangSpec *spec,
                                         TSNode parent, const char *field, TSNode node) {
    (void)ctx;
    (void)spec;
    (void)field;
    const char *kind = ts_node_type(parent);
    /* A nested executable scope inside the default owns its own lookups; only a direct
     * default expression is evaluated in the declaring function's structural parent
     * namespace. */
    if (strcmp(kind, "lambda") == 0 || strcmp(kind, "list_comprehension") == 0 ||
        strcmp(kind, "set_comprehension") == 0 || strcmp(kind, "dictionary_comprehension") == 0 ||
        strcmp(kind, "generator_expression") == 0) {
        return PDXE_STEP_FALSE;
    }
    if ((strcmp(kind, "default_parameter") == 0 || strcmp(kind, "typed_default_parameter") == 0) &&
        (field_contains_node(parent, "value", node) || field_contains_node(parent, "default", node))) {
        return PDXE_STEP_TRUE;
    }
    return PDXE_STEP_CONTINUE;
}

static bool python_default_value_reference(PDXEExtractCtx *ctx, WalkState *state, TSNode node) {
    uint32_t depth;
    if (walk_depth_of(state, node, &depth)) {
        return walk_climb(ctx, state->spec, state, node, depth, PDXE_CLIMB_PY_DEFAULT,
                          python_default_step);
    }
    usage_slow_parent_fallback_test_note();
    for (TSNode parent = ts_node_parent(node); !ts_node_is_null(parent);
         parent = ts_node_parent(parent)) {
        PDXEClimbStep decided = python_default_step(ctx, NULL, parent, NULL, node);
        if (decided != PDXE_STEP_CONTINUE) {
            return decided == PDXE_STEP_TRUE;
        }
    }
    return false;
}

static uint32_t usage_lexical_scope_id_for_node(PDXEExtractCtx *ctx, WalkState *state,
                                                TSNode node) {
    uint32_t scope_id = active_lexical_scope_id(state);
    if (!ctx || ctx->language != PDXE_LANG_PYTHON ||
        !python_default_value_reference(ctx, state, node)) {
        return scope_id;
    }
    uint32_t function_id = lexical_ancestor_of_kind(state, scope_id, true, false);
    const PDXELexicalScope *function_scope = usage_lexical_scope(state, function_id);
    /* Method bodies intentionally skip their class lookup parent, but Python
     * evaluates the method's default expressions while the class namespace is
     * executing. Use structural ownership here, not body lookup ownership. */
    return function_scope ? function_scope->parent_id : scope_id;
}

static uint32_t lexical_ancestor_of_kind(const WalkState *state, uint32_t start_id,
                                         bool want_function, bool want_block) {
    uint32_t id = start_id;
    int remaining = state ? state->lexical_scope_count : 0;
    while (id != 0 && remaining-- > 0) {
        const PDXELexicalScope *scope = usage_lexical_scope(state, id);
        if (!scope) {
            return 0;
        }
        bool function_scope = scope->kind == PDXE_LEXICAL_SCOPE_FUNCTION ||
                              scope->kind == PDXE_LEXICAL_SCOPE_COMPREHENSION;
        bool block_scope = scope->kind == PDXE_LEXICAL_SCOPE_BLOCK;
        if ((want_function && function_scope) || (want_block && block_scope)) {
            return id;
        }
        id = scope->parent_id;
    }
    return 0;
}

static uint32_t python_nearest_namespace(const WalkState *state, uint32_t start_id) {
    uint32_t id = start_id;
    int remaining = state ? state->lexical_scope_count : 0;
    while (id != 0 && remaining-- > 0) {
        const PDXELexicalScope *scope = usage_lexical_scope(state, id);
        if (!scope) {
            return 0;
        }
        if (scope->kind == PDXE_LEXICAL_SCOPE_FUNCTION ||
            scope->kind == PDXE_LEXICAL_SCOPE_COMPREHENSION ||
            scope->kind == PDXE_LEXICAL_SCOPE_CLASS || scope->kind == PDXE_LEXICAL_SCOPE_MODULE) {
            return id;
        }
        id = scope->parent_id;
    }
    return 0;
}

static bool ensure_lexical_binding_capacity(WalkState *state) {
    if (!state->lexical_bindings) {
        state->lexical_bindings = state->inline_lexical_bindings;
        state->lexical_binding_capacity = INLINE_LEXICAL_BINDINGS;
        memset(state->inline_lexical_bindings, 0, sizeof(state->inline_lexical_bindings));
    }
    if (state->lexical_binding_count < state->lexical_binding_capacity) {
        return true;
    }
    if (state->lexical_binding_capacity > INT32_MAX / PAIR_LEN) {
        state->lexical_binding_tracking_failed = true;
        return false;
    }
    int new_capacity = state->lexical_binding_capacity * PAIR_LEN;
    PDXELexicalBinding *grown =
        (PDXELexicalBinding *)pdxe_arena_alloc(state->arena, (size_t)new_capacity * sizeof(*grown));
    if (!grown) {
        state->lexical_binding_tracking_failed = true;
        return false;
    }
    memcpy(grown, state->lexical_bindings, (size_t)state->lexical_binding_count * sizeof(*grown));
    state->lexical_bindings = grown;
    state->lexical_binding_capacity = new_capacity;
    return true;
}

/* Whether an ancestor of `node` is a `global_statement` (or, with `nonlocal`, a
 * `nonlocal_statement`): the nearest of that kind among the frames when `node` is the
 * walk's current node. */
static bool python_directive_ancestor(WalkState *state, TSNode node, bool nonlocal) {
    uint32_t depth;
    if (walk_depth_of(state, node, &depth)) {
        return walk_nearest_above(state, depth,
                                  nonlocal ? PDXE_NEAREST_PY_NONLOCAL : PDXE_NEAREST_PY_GLOBAL) != 0;
    }
    usage_slow_parent_fallback_test_note();
    const char *kind = nonlocal ? "nonlocal_statement" : "global_statement";
    for (TSNode parent = ts_node_parent(node); !ts_node_is_null(parent);
         parent = ts_node_parent(parent)) {
        if (strcmp(ts_node_type(parent), kind) == 0) {
            return true;
        }
    }
    return false;
}

/* The nearest ancestor of `node` of a kind in `set` (PDXE_NEAREST_FUNCTION or
 * PDXE_NEAREST_CLASS) when it declares `node` as its name, else null. */
static TSNode walk_declared_name_owner(WalkState *state, uint32_t depth, TSNode node,
                                       unsigned set) {
    uint32_t nearest = walk_nearest_above(state, depth, set);
    if (nearest == 0) {
        return (TSNode){0};
    }
    TSNode owner = state->frames[nearest - 1].node;
    return field_contains_node(owner, "name", node) ? owner : (TSNode){0};
}

static TSNode declared_function_name_owner(TSNode node, const PDXELangSpec *spec,
                                           WalkState *state) {
    uint32_t depth;
    if (walk_depth_of(state, node, &depth)) {
        return walk_declared_name_owner(state, depth, node, PDXE_NEAREST_FUNCTION);
    }
    usage_slow_parent_fallback_test_note();
    for (TSNode parent = ts_node_parent(node); !ts_node_is_null(parent);
         parent = ts_node_parent(parent)) {
        if (spec->function_node_types && pdxe_kind_in_set(parent, spec->function_node_types)) {
            return field_contains_node(parent, "name", node) ? parent : (TSNode){0};
        }
    }
    return (TSNode){0};
}

static TSNode declared_class_name_owner(TSNode node, const PDXELangSpec *spec,
                                        WalkState *state) {
    uint32_t depth;
    if (walk_depth_of(state, node, &depth)) {
        return walk_declared_name_owner(state, depth, node, PDXE_NEAREST_CLASS);
    }
    usage_slow_parent_fallback_test_note();
    for (TSNode parent = ts_node_parent(node); !ts_node_is_null(parent);
         parent = ts_node_parent(parent)) {
        if (spec->class_node_types && pdxe_kind_in_set(parent, spec->class_node_types)) {
            return field_contains_node(parent, "name", node) ? parent : (TSNode){0};
        }
    }
    return (TSNode){0};
}

static bool ensure_python_directive_capacity(WalkState *state) {
    if (!state->python_directives) {
        state->python_directives = state->inline_python_directives;
        state->python_directive_capacity = INLINE_PYTHON_DIRECTIVES;
        memset(state->inline_python_directives, 0, sizeof(state->inline_python_directives));
    }
    if (state->python_directive_count < state->python_directive_capacity) {
        return true;
    }
    if (state->python_directive_capacity > INT32_MAX / PAIR_LEN) {
        state->lexical_binding_tracking_failed = true;
        return false;
    }
    int new_capacity = state->python_directive_capacity * PAIR_LEN;
    PDXEPythonDirective *grown =
        (PDXEPythonDirective *)pdxe_arena_alloc(state->arena, (size_t)new_capacity * sizeof(*grown));
    if (!grown) {
        state->lexical_binding_tracking_failed = true;
        return false;
    }
    memcpy(grown, state->python_directives, (size_t)state->python_directive_count * sizeof(*grown));
    state->python_directives = grown;
    state->python_directive_capacity = new_capacity;
    return true;
}

static void record_python_directive(WalkState *state, uint32_t function_scope_id, const char *name,
                                    PDXEPythonDirectiveKind kind) {
    if (!function_scope_id || !name || !ensure_python_directive_capacity(state)) {
        return;
    }
    for (int i = 0; i < state->python_directive_count; i++) {
        PDXEPythonDirective *directive = &state->python_directives[i];
        if (directive->function_scope_id == function_scope_id &&
            strcmp(directive->name, name) == 0) {
            directive->kind = (uint8_t)kind;
            return;
        }
    }
    PDXEPythonDirective *directive = &state->python_directives[state->python_directive_count++];
    directive->function_scope_id = function_scope_id;
    directive->name = name;
    directive->kind = (uint8_t)kind;
}

static PDXEPythonDirectiveKind python_directive_for(const WalkState *state,
                                                   uint32_t function_scope_id, const char *name) {
    for (int i = state ? state->python_directive_count - 1 : -1; i >= 0; i--) {
        const PDXEPythonDirective *directive = &state->python_directives[i];
        if (directive->function_scope_id == function_scope_id && directive->name &&
            strcmp(directive->name, name) == 0) {
            return (PDXEPythonDirectiveKind)directive->kind;
        }
    }
    return 0;
}

static uint32_t python_enclosing_function_namespace(const WalkState *state,
                                                    uint32_t inner_function_id) {
    const PDXELexicalScope *inner = usage_lexical_scope(state, inner_function_id);
    uint32_t id = inner ? inner->parent_id : 0;
    int remaining = state ? state->lexical_scope_count : 0;
    while (id != 0 && remaining-- > 0) {
        const PDXELexicalScope *scope = usage_lexical_scope(state, id);
        if (!scope) {
            return 0;
        }
        if (scope->kind == PDXE_LEXICAL_SCOPE_FUNCTION ||
            scope->kind == PDXE_LEXICAL_SCOPE_COMPREHENSION) {
            return id;
        }
        id = scope->parent_id;
    }
    return 0;
}

static bool binding_is_parameter(PDXEExtractCtx *ctx, TSNode node, const PDXELangSpec *spec,
                                 WalkState *state) {
    const PDXEOccurrenceSpec *occurrence = &occurrence_specs[ctx->language];
    uint32_t depth;
    if (walk_depth_of(state, node, &depth)) {
        /* The nearest whole-binding container or function decides. */
        uint32_t nearest = walk_nearest_above(state, depth, PDXE_NEAREST_PARAMETER);
        if (nearest == 0) {
            return false;
        }
        TSNode parent = state->frames[nearest - 1].node;
        const char *kind = ts_node_type(parent);
        if (kind_in_exact_set(kind, common_whole_binding_nodes) ||
            kind_in_exact_set(kind, occurrence->whole_binding_nodes)) {
            return true;
        }
        return field_contains_node(parent, "parameter", node) ||
               field_contains_node(parent, "parameters", node);
    }
    usage_slow_parent_fallback_test_note();
    for (TSNode parent = ts_node_parent(node); !ts_node_is_null(parent);
         parent = ts_node_parent(parent)) {
        const char *kind = ts_node_type(parent);
        if (kind_in_exact_set(kind, common_whole_binding_nodes) ||
            kind_in_exact_set(kind, occurrence->whole_binding_nodes)) {
            return true;
        }
        if (spec->function_node_types && pdxe_kind_in_set(parent, spec->function_node_types)) {
            return field_contains_node(parent, "parameter", node) ||
                   field_contains_node(parent, "parameters", node);
        }
    }
    return false;
}

static bool js_var_binding(TSNode node, WalkState *state) {
    uint32_t depth;
    if (walk_depth_of(state, node, &depth)) {
        uint32_t nearest = walk_nearest_above(state, depth, PDXE_NEAREST_JS_DECLARATION);
        return nearest != 0 &&
               strcmp(ts_node_type(state->frames[nearest - 1].node), "variable_declaration") == 0;
    }
    usage_slow_parent_fallback_test_note();
    for (TSNode parent = ts_node_parent(node); !ts_node_is_null(parent);
         parent = ts_node_parent(parent)) {
        const char *kind = ts_node_type(parent);
        if (strcmp(kind, "variable_declaration") == 0) {
            return true;
        }
        if (strcmp(kind, "lexical_declaration") == 0) {
            return false;
        }
    }
    return false;
}

static bool import_kind_matches(TSNode node, const PDXELangSpec *spec) {
    return (spec->import_node_types && pdxe_kind_in_set(node, spec->import_node_types)) ||
           (spec->import_from_types && pdxe_kind_in_set(node, spec->import_from_types));
}

/* The ancestors of a node, nearest first: its frames' nodes when it is the walk's
 * current node, otherwise ts_node_parent each step. */
typedef struct {
    const WalkState *state;
    uint32_t depth;
    TSNode node;
    bool frames;
} PDXEWalkAncestors;

static PDXEWalkAncestors walk_ancestors(const WalkState *state, TSNode node) {
    PDXEWalkAncestors it = {state, 0, node, false};
    it.frames = walk_depth_of(state, node, &it.depth);
    if (!it.frames) {
        usage_slow_parent_fallback_test_note();
    }
    return it;
}

/* The next ancestor, null past the root. */
static TSNode walk_ancestors_next(PDXEWalkAncestors *it) {
    if (it->frames) {
        usage_field_lookup_test_note_work();
        if (it->depth == 0) {
            return (TSNode){0};
        }
        it->depth--;
        return it->state->frames[it->depth].node;
    }
    it->node = ts_node_is_null(it->node) ? it->node : ts_node_parent(it->node);
    return it->node;
}

static TSNode nearest_import_ancestor(TSNode node, const PDXELangSpec *spec, WalkState *state) {
    uint32_t depth;
    if (walk_depth_of(state, node, &depth)) {
        uint32_t nearest = walk_nearest_above(state, depth, PDXE_NEAREST_IMPORT);
        return nearest != 0 ? state->frames[nearest - 1].node : (TSNode){0};
    }
    usage_slow_parent_fallback_test_note();
    for (TSNode parent = ts_node_parent(node); !ts_node_is_null(parent);
         parent = ts_node_parent(parent)) {
        if (import_kind_matches(parent, spec)) {
            return parent;
        }
    }
    return (TSNode){0};
}

static TSNode first_named_leaf(TSNode node) {
    while (!ts_node_is_null(node) && ts_node_named_child_count(node) > 0) {
        node = ts_node_named_child(node, 0);
    }
    return node;
}

static bool import_alias_contains(TSNode node, TSNode boundary, const WalkState *state) {
    PDXEWalkAncestors ancestors = walk_ancestors(state, node);
    for (TSNode parent = walk_ancestors_next(&ancestors);
         !ts_node_is_null(parent) && !ts_node_eq(parent, boundary);
         parent = walk_ancestors_next(&ancestors)) {
        if (field_contains_node(parent, "alias", node)) {
            return true;
        }
    }
    return false;
}

static bool rust_use_list_has_direct_self(TSNode list) {
    uint32_t count = ts_node_named_child_count(list);
    for (uint32_t i = 0; i < count; i++) {
        if (strcmp(ts_node_type(ts_node_named_child(list, i)), "self") == 0) {
            return true;
        }
    }
    return false;
}

/* Import subtrees are excluded from ordinary usage emission, but their local
 * names still participate in lexical lookup. Keep this classifier narrow: it
 * identifies only the binding side, never a module path or imported source
 * name. */
static bool is_import_binding_occurrence(PDXEExtractCtx *ctx, TSNode node, const PDXELangSpec *spec,
                                         WalkState *state) {
    TSNode boundary = nearest_import_ancestor(node, spec, state);
    if (ts_node_is_null(boundary)) {
        return false;
    }
    /* Rust's extern-crate alias is a direct field of the import boundary,
     * unlike the nested alias containers used by Python and ES imports. */
    if (field_contains_node(boundary, "alias", node)) {
        return true;
    }
    if (import_alias_contains(node, boundary, state)) {
        return true;
    }

    switch (ctx->language) {
    case PDXE_LANG_PYTHON: {
        const char *boundary_kind = ts_node_type(boundary);
        if (strcmp(boundary_kind, "future_import_statement") == 0) {
            return false;
        }
        PDXEWalkAncestors aliased = walk_ancestors(state, node);
        for (TSNode parent = walk_ancestors_next(&aliased);
             !ts_node_is_null(parent) && !ts_node_eq(parent, boundary);
             parent = walk_ancestors_next(&aliased)) {
            /* In an aliased import, only the alias binds. */
            if (strcmp(ts_node_type(parent), "aliased_import") == 0) {
                return false;
            }
        }
        bool from_import = strcmp(boundary_kind, "import_from_statement") == 0;
        if (from_import) {
            if (field_contains_node(boundary, "module_name", node) ||
                field_contains_node(boundary, "module", node)) {
                return false;
            }
            /* Older Python grammar variants omit the module field. Their first
             * named child is still the source module, never a local binding. */
            if (ts_node_named_child_count(boundary) > 1 &&
                node_contains(ts_node_named_child(boundary, 0), node)) {
                return false;
            }
            return strcmp(ts_node_type(node), "identifier") == 0;
        }

        TSNode entry = node;
        PDXEWalkAncestors up = walk_ancestors(state, node);
        TSNode parent = walk_ancestors_next(&up);
        while (!ts_node_is_null(parent) && !ts_node_eq(parent, boundary)) {
            entry = parent;
            parent = walk_ancestors_next(&up);
        }
        TSNode first = first_named_leaf(entry);
        return !ts_node_is_null(first) && ts_node_eq(first, node);
    }
    case PDXE_LANG_JAVASCRIPT:
    case PDXE_LANG_TYPESCRIPT:
    case PDXE_LANG_TSX:
    case PDXE_LANG_ARKTS: {
        if (strcmp(ts_node_type(boundary), "import_statement") != 0) {
            return false;
        }
        PDXEWalkAncestors clause = walk_ancestors(state, node);
        for (TSNode parent = walk_ancestors_next(&clause);
             !ts_node_is_null(parent) && !ts_node_eq(parent, boundary);
             parent = walk_ancestors_next(&clause)) {
            const char *kind = ts_node_type(parent);
            if (strcmp(kind, "import_specifier") == 0) {
                TSNode alias = ts_node_child_by_field_name(parent, TS_FIELD("alias"));
                if (!ts_node_is_null(alias)) {
                    return false;
                }
                return field_contains_node(parent, "name", node) ||
                       ts_node_eq(first_named_leaf(parent), node);
            }
            if (strcmp(kind, "namespace_import") == 0) {
                return field_contains_node(parent, "name", node) ||
                       ts_node_eq(first_named_leaf(parent), node);
            }
            if (strcmp(kind, "import_clause") == 0) {
                /* A direct identifier is the default import. Named and
                 * namespace imports were handled by their inner containers. */
                return ts_node_eq(first_named_leaf(parent), node);
            }
            if (strcmp(kind, "import_require_clause") == 0) {
                /* TypeScript `import local = require("module")` and
                 * `import local = Namespace.member` put the binding first.
                 * Current grammar variants expose only the source field, so
                 * retain the first-leaf fallback as the exact role check. */
                return field_contains_node(parent, "name", node) ||
                       ts_node_eq(first_named_leaf(parent), node);
            }
        }
        return false;
    }
    case PDXE_LANG_RUST: {
        TSNode boundary_alias = ts_node_child_by_field_name(boundary, TS_FIELD("alias"));
        if (!ts_node_is_null(boundary_alias) && field_contains_node(boundary, "name", node)) {
            /* `extern crate source as local` binds only local. */
            return false;
        }
        bool terminal = false;
        PDXEWalkAncestors use = walk_ancestors(state, node);
        for (TSNode parent = walk_ancestors_next(&use);
             !ts_node_is_null(parent) && !ts_node_eq(parent, boundary);
             parent = walk_ancestors_next(&use)) {
            if (field_contains_node(parent, "path", node)) {
                /* `use foo::{self}` imports the module itself under the final
                 * segment of the prefix (`foo`). An aliased `self as x` is a
                 * use_as_clause child rather than a direct self entry and is
                 * already handled by the alias rule above. */
                if (strcmp(ts_node_type(parent), "scoped_use_list") == 0 &&
                    strcmp(ts_node_type(node), "identifier") == 0) {
                    TSNode path = ts_node_child_by_field_name(parent, TS_FIELD("path"));
                    TSNode list = ts_node_child_by_field_name(parent, TS_FIELD("list"));
                    bool terminal_path = terminal || ts_node_eq(path, node);
                    if (terminal_path && !ts_node_is_null(list) &&
                        rust_use_list_has_direct_self(list)) {
                        return true;
                    }
                }
                return false;
            }
            if (field_contains_node(parent, "name", node)) {
                terminal = true;
            }
        }
        return terminal || strcmp(ts_node_type(node), "identifier") == 0;
    }
    default:
        return false;
    }
}

static uint32_t lexical_import_scope(const WalkState *state, uint32_t start_id) {
    uint32_t id = start_id;
    int remaining = state ? state->lexical_scope_count : 0;
    while (id != 0 && remaining-- > 0) {
        const PDXELexicalScope *scope = usage_lexical_scope(state, id);
        if (!scope) {
            return 0;
        }
        if (scope->kind == PDXE_LEXICAL_SCOPE_MODULE || scope->kind == PDXE_LEXICAL_SCOPE_FUNCTION ||
            scope->kind == PDXE_LEXICAL_SCOPE_COMPREHENSION ||
            scope->kind == PDXE_LEXICAL_SCOPE_BLOCK) {
            return id;
        }
        id = scope->parent_id;
    }
    return 0;
}

static void record_lexical_binding(PDXEExtractCtx *ctx, WalkState *state, TSNode node,
                                   const PDXELangSpec *spec, const char *raw_name,
                                   bool import_binding) {
    if (!ctx || !state || !raw_name || !raw_name[0] || state->lexical_binding_tracking_failed) {
        return;
    }
    bool parameter = binding_is_parameter(ctx, node, spec, state);
    if (ctx->language == PDXE_LANG_VIMSCRIPT && parameter && !strchr(raw_name, ':')) {
        raw_name = pdxe_arena_sprintf(ctx->arena, "a:%s", raw_name);
        if (!raw_name) {
            state->lexical_binding_tracking_failed = true;
            return;
        }
    }
    const char *name = lexical_binding_key(ctx, state, raw_name);
    if (!name || !name[0]) {
        return;
    }

    uint32_t current_id = active_lexical_scope_id(state);
    uint32_t scope_id = 0;
    bool whole_scope = false;
    TSNode function_declaration = declared_function_name_owner(node, spec, state);
    TSNode class_declaration = declared_class_name_owner(node, spec, state);

    if (ctx->language == PDXE_LANG_PYTHON && (python_directive_ancestor(state, node, false) ||
                                             python_directive_ancestor(state, node, true))) {
        uint32_t function_id = lexical_ancestor_of_kind(state, current_id, true, false);
        PDXEPythonDirectiveKind directive = python_directive_ancestor(state, node, false)
                                               ? PDXE_PYTHON_DIRECTIVE_GLOBAL
                                               : PDXE_PYTHON_DIRECTIVE_NONLOCAL;
        record_python_directive(state, function_id, name, directive);
        return;
    }

    if (!ts_node_is_null(function_declaration)) {
        uint32_t function_id = lexical_ancestor_of_kind(state, current_id, true, false);
        const PDXELexicalScope *function_scope = usage_lexical_scope(state, function_id);
        uint32_t parent_id = function_scope ? function_scope->parent_id : 0;
        if (ctx->language == PDXE_LANG_PYTHON) {
            scope_id = python_nearest_namespace(state, parent_id);
            const PDXELexicalScope *owner = usage_lexical_scope(state, scope_id);
            if (!owner || (owner->kind != PDXE_LEXICAL_SCOPE_FUNCTION &&
                           owner->kind != PDXE_LEXICAL_SCOPE_COMPREHENSION)) {
                return;
            }
        } else {
            scope_id = lexical_ancestor_of_kind(state, parent_id, true, true);
        }
        /* Top-level/class callable declarations are themselves semantic
         * targets, not local-value blockers. Nested declarations still bind
         * their enclosing executable scope and must block raw-name fallback. */
        if (scope_id == 0) {
            return;
        }
        whole_scope = true;
    } else if (ctx->language == PDXE_LANG_PYTHON && !ts_node_is_null(class_declaration)) {
        uint32_t class_id = python_nearest_namespace(state, current_id);
        const PDXELexicalScope *class_scope = usage_lexical_scope(state, class_id);
        scope_id = python_nearest_namespace(state, class_scope ? class_scope->parent_id : 0);
        const PDXELexicalScope *owner = usage_lexical_scope(state, scope_id);
        if (!owner || (owner->kind != PDXE_LEXICAL_SCOPE_FUNCTION &&
                       owner->kind != PDXE_LEXICAL_SCOPE_COMPREHENSION)) {
            return;
        }
        whole_scope = true;
    } else if (parameter) {
        scope_id = lexical_ancestor_of_kind(state, current_id, true, false);
        whole_scope = true;
    } else if (ctx->language == PDXE_LANG_PYTHON) {
        uint32_t namespace_id = python_nearest_namespace(state, current_id);
        const PDXELexicalScope *namespace_scope = usage_lexical_scope(state, namespace_id);
        uint32_t function_id =
            namespace_scope && (namespace_scope->kind == PDXE_LEXICAL_SCOPE_FUNCTION ||
                                namespace_scope->kind == PDXE_LEXICAL_SCOPE_COMPREHENSION)
                ? namespace_id
                : lexical_ancestor_of_kind(state, current_id, true, false);
        PDXEPythonDirectiveKind directive = python_directive_for(state, function_id, name);
        if (directive == PDXE_PYTHON_DIRECTIVE_GLOBAL) {
            scope_id = state->root_lexical_scope_id;
        } else if (directive == PDXE_PYTHON_DIRECTIVE_NONLOCAL) {
            scope_id = python_enclosing_function_namespace(state, function_id);
        } else {
            scope_id = namespace_id;
        }
        const PDXELexicalScope *owner = usage_lexical_scope(state, scope_id);
        if (!owner) {
            return;
        }
        whole_scope = directive == 0 && (owner->kind == PDXE_LEXICAL_SCOPE_FUNCTION ||
                                         owner->kind == PDXE_LEXICAL_SCOPE_COMPREHENSION);
    } else if (ctx->language == PDXE_LANG_POWERSHELL) {
        scope_id = lexical_ancestor_of_kind(state, current_id, true, false);
        whole_scope = true;
    } else if (ctx->language == PDXE_LANG_JAVASCRIPT || ctx->language == PDXE_LANG_TYPESCRIPT ||
               ctx->language == PDXE_LANG_TSX || ctx->language == PDXE_LANG_ARKTS) {
        bool is_var = js_var_binding(node, state);
        scope_id = lexical_ancestor_of_kind(state, current_id, is_var, !is_var);
        if (scope_id == 0) {
            scope_id = lexical_ancestor_of_kind(state, current_id, true, false);
        }
        if (scope_id == 0) {
            scope_id = state->root_lexical_scope_id;
        }
        whole_scope = true; /* var hoisting and let/const TDZ */
    } else if (ctx->language == PDXE_LANG_RUST && import_binding) {
        scope_id = lexical_import_scope(state, current_id);
        if (scope_id == 0) {
            scope_id = state->root_lexical_scope_id;
        }
        /* A Rust use/extern-crate declaration is an item whose name is in
         * scope for the complete containing block or module, independent of
         * textual declaration order. */
        whole_scope = true;
    } else {
        scope_id = lexical_ancestor_of_kind(state, current_id, true, true);
        if (scope_id == 0) {
            scope_id = state->root_lexical_scope_id;
        }
    }
    const PDXELexicalScope *scope = usage_lexical_scope(state, scope_id);
    if (!scope || !ensure_lexical_binding_capacity(state)) {
        return;
    }
    PDXELexicalBinding *binding = &state->lexical_bindings[state->lexical_binding_count++];
    binding->scope_id = scope_id;
    binding->active_start = whole_scope ? scope->start_byte : ts_node_end_byte(node);
    /* Zero means "the finalized end of this concrete scope". Split
     * signature/body languages extend the scope after parameters are seen. */
    binding->active_end = 0;
    binding->name = name;
}

static int compare_lexical_bindings(const void *left_ptr, const void *right_ptr) {
    const PDXELexicalBinding *left = (const PDXELexicalBinding *)left_ptr;
    const PDXELexicalBinding *right = (const PDXELexicalBinding *)right_ptr;
    if (left->scope_id != right->scope_id) {
        return left->scope_id < right->scope_id ? -1 : 1;
    }
    int name_order = strcmp(left->name, right->name);
    if (name_order != 0) {
        return name_order;
    }
    if (left->active_start != right->active_start) {
        return left->active_start < right->active_start ? -1 : 1;
    }
    if (left->active_end != right->active_end) {
        return left->active_end < right->active_end ? -1 : 1;
    }
    return 0;
}

static int compare_binding_key(const PDXELexicalBinding *binding, uint32_t scope_id,
                               const char *name) {
    if (binding->scope_id != scope_id) {
        return binding->scope_id < scope_id ? -1 : 1;
    }
    return strcmp(binding->name, name);
}

static int lexical_binding_lower_bound(const WalkState *state, uint32_t scope_id,
                                       const char *name) {
    int low = 0;
    int high = state->lexical_binding_count;
    while (low < high) {
        int middle = low + (high - low) / PAIR_LEN;
        if (compare_binding_key(&state->lexical_bindings[middle], scope_id, name) < 0) {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    return low;
}

static bool lexical_binding_active_at(const WalkState *state, uint32_t scope_id, const char *name,
                                      uint32_t position) {
    const PDXELexicalScope *scope = usage_lexical_scope(state, scope_id);
    if (!scope) {
        return false;
    }
    int index = lexical_binding_lower_bound(state, scope_id, name);
    while (index < state->lexical_binding_count) {
        const PDXELexicalBinding *binding = &state->lexical_bindings[index++];
        if (binding->scope_id != scope_id || strcmp(binding->name, name) != 0) {
            break;
        }
        uint32_t active_end = binding->active_end ? binding->active_end : scope->end_byte;
        if (binding->active_start <= position && position < active_end) {
            return true;
        }
    }
    return false;
}

static const PDXELexicalScope *lexical_binding_scope_for_usage(const WalkState *state,
                                                              const PDXEUsage *usage,
                                                              const char *name) {
    uint32_t scope_id = usage->lexical_scope_id;
    int remaining = state ? state->lexical_scope_count : 0;
    while (scope_id != 0 && remaining-- > 0) {
        const PDXELexicalScope *scope = usage_lexical_scope(state, scope_id);
        if (!scope) {
            return NULL;
        }
        if (lexical_binding_active_at(state, scope_id, name, usage->site_start_byte)) {
            return scope;
        }
        scope_id = scope->lookup_parent_id;
    }
    return NULL;
}

void pdxe_finalize_lexical_usages(PDXEExtractCtx *ctx, WalkState *state) {
    if (!ctx || !state) {
        return;
    }
    if (state->lexical_binding_count > 1) {
        qsort(state->lexical_bindings, (size_t)state->lexical_binding_count,
              sizeof(*state->lexical_bindings), compare_lexical_bindings);
    }
    for (int i = state->usage_start_index; i < ctx->result->usages.count; i++) {
        PDXEUsage *usage = &ctx->result->usages.items[i];
        if (!usage->ref_name || !usage->ref_name[0]) {
            continue;
        }
        const char *name = lexical_binding_key(ctx, state, usage->ref_name);
        if (!name) {
            continue;
        }
        const PDXELexicalScope *binding_scope = lexical_binding_scope_for_usage(state, usage, name);
        if (!binding_scope) {
            continue;
        }
        usage->semantic_reference_blocked = true;
        usage->semantic_reference_local_shadow = binding_scope->kind != PDXE_LEXICAL_SCOPE_MODULE;
    }

    /* Allocation failure must never widen textual fallback. Exact semantic
     * occurrence proof still runs first, but every unproven value fails closed
     * until the next clean extraction. */
    if (state->lexical_binding_tracking_failed) {
        for (int i = state->usage_start_index; i < ctx->result->usages.count; i++) {
            PDXEUsage *usage = &ctx->result->usages.items[i];
            usage->semantic_reference_blocked = true;
            const PDXELexicalScope *scope = usage_lexical_scope(state, usage->lexical_scope_id);
            usage->semantic_reference_local_shadow =
                scope && scope->kind != PDXE_LEXICAL_SCOPE_MODULE;
        }
    }
}

static char *perl_direct_coderef_name(PDXEExtractCtx *ctx, TSNode node, WalkState *state) {
    if (!ctx || ctx->language != PDXE_LANG_PERL ||
        strcmp(ts_node_type(node), "refgen_expression") != 0) {
        return NULL;
    }
    uint32_t depth;
    if (!(walk_depth_of(state, node, &depth) ? walk_is_direct_argument_value(state, depth)
                                             : is_direct_argument_value(node))) {
        return NULL;
    }
    char *text = pdxe_node_text(ctx->arena, node, ctx->source);
    const char *amp = text ? strchr(text, '&') : NULL;
    if (!amp) {
        return NULL;
    }
    const char *start = amp + 1;
    while (*start && isspace((unsigned char)*start)) {
        start++;
    }
    const char *end = start;
    while (*end && (isalnum((unsigned char)*end) || *end == '_' || *end == ':')) {
        end++;
    }
    return end > start ? pdxe_arena_strndup(ctx->arena, start, (size_t)(end - start)) : NULL;
}

static bool inside_direct_perl_coderef(PDXEExtractCtx *ctx, TSNode node, WalkState *state) {
    if (!ctx || ctx->language != PDXE_LANG_PERL) {
        return false;
    }
    uint32_t depth;
    if (walk_depth_of(state, node, &depth)) {
        /* The nearest refgen or argument list decides. */
        uint32_t nearest = walk_nearest_above(state, depth, PDXE_NEAREST_PERL_CODEREF);
        return nearest != 0 &&
               strcmp(ts_node_type(state->frames[nearest - 1].node), "refgen_expression") == 0 &&
               walk_is_direct_argument_value(state, nearest - 1);
    }
    usage_slow_parent_fallback_test_note();
    for (TSNode parent = ts_node_parent(node); !ts_node_is_null(parent);
         parent = ts_node_parent(parent)) {
        if (strcmp(ts_node_type(parent), "refgen_expression") == 0) {
            return is_direct_argument_value(parent);
        }
        if (is_argument_container_kind(ts_node_type(parent))) {
            return false;
        }
    }
    return false;
}

static bool emit_direct_perl_coderef_usage(PDXEExtractCtx *ctx, TSNode node,
                                           const char *enclosing_func_qn,
                                           uint32_t lexical_scope_id, WalkState *state) {
    char *name = perl_direct_coderef_name(ctx, node, state);
    if (!name || !name[0]) {
        return false;
    }
    PDXEUsage usage = {0};
    usage.ref_name = name;
    usage.enclosing_func_qn = enclosing_func_qn;
    usage.kind = PDXE_USAGE_CALL_REFERENCE;
    usage.may_be_call_reference = true;
    usage.lexical_scope_id = lexical_scope_id;
    usage.site_start_byte = ts_node_start_byte(node);
    usage.site_end_byte = ts_node_end_byte(node);
    pdxe_usages_push(&ctx->result->usages, ctx->arena, usage);
    return true;
}

// Try to emit a usage for a reference node. Returns early if the node should be skipped.
static void try_emit_usage(PDXEExtractCtx *ctx, TSNode node, const PDXELangSpec *spec,
                           bool inside_call, bool inside_import) {
    if (emit_direct_perl_coderef_usage(ctx, node, pdxe_enclosing_func_qn_cached(ctx, node), 0,
                                       NULL)) {
        return;
    }
    if (inside_direct_perl_coderef(ctx, node, NULL)) {
        return;
    }
    if (!is_reference_node(node, ctx->language, NULL)) {
        return;
    }
    if (is_call_argument_label(node)) {
        return;
    }
    if (inside_call || inside_import) {
        return;
    }
    if (is_binding_occurrence(ctx, node, spec, NULL) ||
        is_write_occurrence(ctx, node, spec, NULL)) {
        return;
    }
    char *name = reference_name(ctx, node);
    if (name && name[0] && !pdxe_is_keyword(name, ctx->language)) {
        PDXEUsage usage = {0};
        usage.ref_name = name;
        usage.enclosing_func_qn = pdxe_enclosing_func_qn_cached(ctx, node);
        stamp_usage_site(ctx, &usage, node, name, NULL);
        pdxe_usages_push(&ctx->result->usages, ctx->arena, usage);
    }
}

// Iterative usage walker — explicit stack.
//
// The call/import ancestry that gates usage emission is maintained as ENTER/
// EXIT counters on the walk instead of per-node ancestor re-walks: the old
// is_inside_call/is_inside_import helpers climbed every ancestor via
// ts_node_parent, and tree-sitter's ts_node_parent RE-DESCENDS from the root
// scanning siblings — O(depth x sibling-position) per node, which went
// quadratic on wide nodes (a 1,536-argument call in dotnet/runtime's JIT
// torture tests put 92% of extract time into these walks; 490 s for one
// 147 KB file). Counter semantics match the helpers exactly: strict ancestors
// only — a node is emitted BEFORE its own kind increments the counters, so a
// call node itself does not count as "inside a call".
static void walk_usages(PDXEExtractCtx *ctx, TSNode root, const PDXELangSpec *spec) {
    typedef struct {
        TSNode node;
        uint32_t next_child;
        bool counts_call;
        bool counts_import;
    } UsageFrame;
    int cap = 256;
    UsageFrame *frames = (UsageFrame *)pdxe_arena_alloc(ctx->arena, (size_t)cap * sizeof(*frames));
    if (!frames) {
        return;
    }
    bool has_imports = spec->import_node_types && spec->import_node_types[0];
    bool has_from_imports = spec->import_from_types && spec->import_from_types[0];
    int call_depth = 0;
    int import_depth = 0;
    int top = 0;
    frames[top++] = (UsageFrame){root, 0, false, false};
    bool entering = true;

    while (top > 0) {
        UsageFrame *f = &frames[top - 1];
        if (entering) {
            try_emit_usage(ctx, f->node, spec, call_depth > 0, import_depth > 0);
            f->counts_call = pdxe_kind_in_set(f->node, spec->call_node_types);
            f->counts_import =
                (has_imports && pdxe_kind_in_set(f->node, spec->import_node_types)) ||
                (has_from_imports && pdxe_kind_in_set(f->node, spec->import_from_types));
            if (f->counts_call) {
                call_depth++;
            }
            if (f->counts_import) {
                import_depth++;
            }
        }
        uint32_t count = ts_node_child_count(f->node);
        if (f->next_child < count) {
            TSNode child = ts_node_child(f->node, f->next_child);
            f->next_child++;
            if (top == cap) {
                int new_cap = cap * 2;
                UsageFrame *grown =
                    (UsageFrame *)pdxe_arena_alloc(ctx->arena, (size_t)new_cap * sizeof(*grown));
                if (!grown) {
                    return;
                }
                memcpy(grown, frames, (size_t)cap * sizeof(*frames));
                frames = grown;
                cap = new_cap;
            }
            frames[top++] = (UsageFrame){child, 0, false, false};
            entering = true;
        } else {
            if (f->counts_call) {
                call_depth--;
            }
            if (f->counts_import) {
                import_depth--;
            }
            top--;
            entering = false;
        }
    }
}

void pdxe_extract_usages(PDXEExtractCtx *ctx) {
    const PDXELangSpec *spec = pdxe_lang_spec(ctx->language);
    if (!spec) {
        return;
    }

    walk_usages(ctx, ctx->root, spec);
}

// --- Unified handler: called once per node by the cursor walk ---
// Uses WalkState flags instead of parent-chain walks for O(1) context checks.

static void handle_usages_at_node(PDXEExtractCtx *ctx, TSNode node, const PDXELangSpec *spec,
                                  WalkState *state);

/* Every possible callable reference recorded at this node gets the node's path. */
void handle_usages(PDXEExtractCtx *ctx, TSNode node, const PDXELangSpec *spec, WalkState *state) {
    int usages_before = ctx->result->usages.count;
    handle_usages_at_node(ctx, node, spec, state);
    pdxe_walk_stamp_site_paths(ctx, state, ctx->result->calls.count, usages_before);
}

static void handle_usages_at_node(PDXEExtractCtx *ctx, TSNode node, const PDXELangSpec *spec,
                                  WalkState *state) {
    if (emit_direct_perl_coderef_usage(ctx, node, state->enclosing_func_qn,
                                       active_lexical_scope_id(state), state)) {
        return;
    }
    if (inside_direct_perl_coderef(ctx, node, state)) {
        return;
    }
    bool reference_node = is_reference_node(node, ctx->language, state);
    const PDXEOccurrenceSpec *occurrence = &occurrence_specs[ctx->language];
    bool possible_binding_leaf =
        ts_node_is_named(node) && ts_node_named_child_count(node) == 0 &&
        (state->inside_import || is_exact_language_binding(ctx, node, state) ||
         is_policy_binding(ctx, node, occurrence, state));
    if (!reference_node && !possible_binding_leaf) {
        return;
    }

    // An invocation relationship replaces only the exact AST occurrence it
    // consumes. Receivers, qualifiers, computed keys, arguments, and nested
    // bodies remain ordinary value references. Explicit callable syntax keeps
    // that exact occurrence as a typed reference instead of erasing it.
    bool exact_callee =
        (!ts_node_is_null(state->callee_expr) && ts_node_eq(node, state->callee_expr)) ||
        (!ts_node_is_null(state->callee_leaf) && ts_node_eq(node, state->callee_leaf));
    if (exact_callee) {
        if (reference_node && state->invocation_kind == PDXE_INVOCATION_CALLABLE_REFERENCE) {
            char *name = reference_name(ctx, node);
            if (name && name[0] && !pdxe_is_keyword(name, ctx->language)) {
                PDXEUsage usage = {0};
                usage.ref_name = name;
                usage.enclosing_func_qn = state->enclosing_func_qn;
                usage.lexical_scope_id = usage_lexical_scope_id_for_node(ctx, state, node);
                /* Kotlin callable-reference syntax is only a precision
                 * candidate. The semantic pass decides whether `::name`
                 * denotes one exact callable; unresolved properties and
                 * ambiguous values must remain ordinary USAGE. */
                if (ctx->language == PDXE_LANG_KOTLIN) {
                    usage.kind = PDXE_USAGE_VALUE;
                    usage.may_be_call_reference = true;
                } else {
                    usage.kind = PDXE_USAGE_CALL_REFERENCE;
                }
                usage.site_start_byte = ts_node_start_byte(node);
                usage.site_end_byte = ts_node_end_byte(node);
                pdxe_usages_push(&ctx->result->usages, ctx->arena, usage);
            }
        }
        return;
    }
    if (is_forward_sibling_callee(ctx->language, node)) {
        return;
    }
    if (is_call_argument_label_walk(node, state)) {
        return;
    }
    // Imports do not emit ordinary usages, but their binding occurrence must
    // be recorded before the subtree is skipped.
    if (state->inside_import) {
        if (is_import_binding_occurrence(ctx, node, spec, state)) {
            char *binding_name = reference_name(ctx, node);
            if (binding_name && binding_name[0] && !pdxe_is_keyword(binding_name, ctx->language)) {
                record_lexical_binding(ctx, state, node, spec, binding_name, true);
            }
        }
        return;
    }

    bool python_scope_directive =
        ctx->language == PDXE_LANG_PYTHON && (python_directive_ancestor(state, node, false) ||
                                             python_directive_ancestor(state, node, true));
    if (python_scope_directive || is_binding_occurrence(ctx, node, spec, state)) {
        char *binding_name = reference_name(ctx, node);
        if (binding_name && binding_name[0] && !pdxe_is_keyword(binding_name, ctx->language)) {
            record_lexical_binding(ctx, state, node, spec, binding_name, false);
        }
        return;
    }
    if (!reference_node) {
        return;
    }
    if (is_write_occurrence(ctx, node, spec, state)) {
        if (ctx->language == PDXE_LANG_OBJECTSCRIPT_UDL ||
            ctx->language == PDXE_LANG_OBJECTSCRIPT_ROUTINE) {
            char *binding_name = reference_name(ctx, node);
            if (binding_name && binding_name[0] && !pdxe_is_keyword(binding_name, ctx->language)) {
                record_lexical_binding(ctx, state, node, spec, binding_name, false);
            }
        }
        return;
    }

    char *name = reference_name(ctx, node);
    if (name && name[0] && !pdxe_is_keyword(name, ctx->language)) {
        PDXEUsage usage = {0};
        usage.ref_name = name;
        usage.enclosing_func_qn = state->enclosing_func_qn;
        usage.lexical_scope_id = usage_lexical_scope_id_for_node(ctx, state, node);
        /* The member half of a selector is its own reference node
         * (field_identifier); record that shape — the name alone cannot carry
         * it, and the Go Field guard keys on it (#1962). */
        usage.is_member_access = strcmp(ts_node_type(node), "field_identifier") == 0;
        stamp_usage_site(ctx, &usage, node, name, state);
        pdxe_usages_push(&ctx->result->usages, ctx->arena, usage);
    }
}
