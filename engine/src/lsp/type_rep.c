#include "type_rep.h"
#include <stdint.h>
#include <string.h>

/* No real allocation lives in the first page; values below this are garbage
 * (e.g. small integers or truncated string bytes misread as pointers). */
enum { TR_MIN_PLAUSIBLE_PTR = 4096 };

// Singleton UNKNOWN type (no allocation needed).
static const PDXEType unknown_singleton = {.kind = PDXE_TYPE_UNKNOWN};

const PDXEType *pdxe_type_unknown(void) {
    return &unknown_singleton;
}

const PDXEType *pdxe_type_named(PDXEArena *a, const char *qualified_name) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_NAMED;
    t->data.named.qualified_name = pdxe_arena_strdup(a, qualified_name);
    return t;
}

const PDXEType *pdxe_type_pointer(PDXEArena *a, const PDXEType *elem) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_POINTER;
    t->data.pointer.elem = elem;
    return t;
}

const PDXEType *pdxe_type_slice(PDXEArena *a, const PDXEType *elem) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_SLICE;
    t->data.slice.elem = elem;
    return t;
}

const PDXEType *pdxe_type_map(PDXEArena *a, const PDXEType *key, const PDXEType *value) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_MAP;
    t->data.map.key = key;
    t->data.map.value = value;
    return t;
}

const PDXEType *pdxe_type_channel(PDXEArena *a, const PDXEType *elem, int direction) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_CHANNEL;
    t->data.channel.elem = elem;
    t->data.channel.direction = direction;
    return t;
}

const PDXEType *pdxe_type_func(PDXEArena *a, const char **param_names, const PDXEType **param_types,
                             const PDXEType **return_types) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_FUNC;

    // Copy all arrays into arena memory to avoid dangling stack pointers.
    if (return_types) {
        int count = 0;
        while (return_types[count])
            count++;
        const PDXEType **arr =
            (const PDXEType **)pdxe_arena_alloc(a, (count + 1) * sizeof(const PDXEType *));
        if (arr) {
            for (int i = 0; i < count; i++)
                arr[i] = return_types[i];
            arr[count] = NULL;
            t->data.func.return_types = arr;
        }
    }
    if (param_types) {
        int count = 0;
        while (param_types[count])
            count++;
        const PDXEType **arr =
            (const PDXEType **)pdxe_arena_alloc(a, (count + 1) * sizeof(const PDXEType *));
        if (arr) {
            for (int i = 0; i < count; i++)
                arr[i] = param_types[i];
            arr[count] = NULL;
            t->data.func.param_types = arr;
        }
    }
    if (param_names) {
        int count = 0;
        while (param_names[count])
            count++;
        const char **arr = (const char **)pdxe_arena_alloc(a, (count + 1) * sizeof(const char *));
        if (arr) {
            for (int i = 0; i < count; i++)
                arr[i] = param_names[i];
            arr[count] = NULL;
            t->data.func.param_names = arr;
        }
    }
    return t;
}

const PDXEType **pdxe_type_materialize_signature_params(PDXEArena *a, const char *const *type_texts,
                                                      int count, PDXETypeTextParser parser,
                                                      void *parser_ctx) {
    if (count <= 0)
        return NULL;

    const PDXEType **types =
        (const PDXEType **)pdxe_arena_alloc(a, ((size_t)count + 1) * sizeof(const PDXEType *));
    if (!types)
        return NULL;

    for (int i = 0; i < count; i++) {
        const char *text = type_texts ? type_texts[i] : NULL;
        if (!text || text[0] == '\0' || strcmp(text, "?") == 0) {
            types[i] = pdxe_type_unknown();
            continue;
        }

        const PDXEType *parsed = parser ? parser(a, text, parser_ctx) : NULL;
        types[i] = parsed ? parsed : pdxe_type_unknown();
    }
    types[count] = NULL;
    return types;
}

const PDXEType *pdxe_type_func_replace_returns(PDXEArena *a, const PDXEType *old_signature,
                                             const PDXEType *const *new_return_types) {
    if (!old_signature || old_signature->kind != PDXE_TYPE_FUNC)
        return pdxe_type_unknown();

    /* pdxe_type_func only reads and then copies this vector into arena memory. */
    return pdxe_type_func(a, old_signature->data.func.param_names,
                         old_signature->data.func.param_types, (const PDXEType **)new_return_types);
}

const PDXEType *pdxe_type_builtin(PDXEArena *a, const char *name) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_BUILTIN;
    t->data.builtin.name = pdxe_arena_strdup(a, name);
    return t;
}

const PDXEType *pdxe_type_tuple(PDXEArena *a, const PDXEType **elems, int count) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_TUPLE;
    // Copy elems array
    const PDXEType **arr =
        (const PDXEType **)pdxe_arena_alloc(a, (count + 1) * sizeof(const PDXEType *));
    if (!arr)
        return &unknown_singleton;
    for (int i = 0; i < count; i++)
        arr[i] = elems[i];
    arr[count] = NULL;
    t->data.tuple.elems = arr;
    t->data.tuple.count = count;
    return t;
}

const PDXEType *pdxe_type_type_param(PDXEArena *a, const char *name) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_TYPE_PARAM;
    t->data.type_param.name = pdxe_arena_strdup(a, name);
    return t;
}

const PDXEType *pdxe_type_reference(PDXEArena *a, const PDXEType *elem) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_REFERENCE;
    t->data.reference.elem = elem;
    return t;
}

const PDXEType *pdxe_type_rvalue_ref(PDXEArena *a, const PDXEType *elem) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_RVALUE_REF;
    t->data.reference.elem = elem;
    return t;
}

const PDXEType *pdxe_type_template(PDXEArena *a, const char *name, const PDXEType **args,
                                 int arg_count) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_TEMPLATE;
    t->data.template_type.template_name = pdxe_arena_strdup(a, name);
    if (args && arg_count > 0) {
        const PDXEType **arr =
            (const PDXEType **)pdxe_arena_alloc(a, (arg_count + 1) * sizeof(const PDXEType *));
        if (arr) {
            for (int i = 0; i < arg_count; i++)
                arr[i] = args[i];
            arr[arg_count] = NULL;
            t->data.template_type.template_args = arr;
        }
    }
    t->data.template_type.arg_count = arg_count;
    return t;
}

const PDXEType *pdxe_type_alias(PDXEArena *a, const char *alias_qn, const PDXEType *underlying) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_ALIAS;
    t->data.alias.alias_qn = pdxe_arena_strdup(a, alias_qn);
    t->data.alias.underlying = underlying;
    return t;
}

// --- Python-flavored constructors -------------------------------------------

// Dedupe members by structural equality, in place. Returns new length.
// Preserves first-seen order so output is deterministic.
static int union_member_dedupe(const PDXEType **scratch, int count) {
    int out = 0;
    for (int i = 0; i < count; i++) {
        bool seen = false;
        for (int j = 0; j < out; j++) {
            if (pdxe_type_equal(scratch[i], scratch[j])) {
                seen = true;
                break;
            }
        }
        if (!seen) {
            scratch[out++] = scratch[i];
        }
    }
    return out;
}

// Shared by Python (pdxe_type_union) and TS (`A | B`). Flattens nested UNIONs and
// dedupes members.
const PDXEType *pdxe_type_union(PDXEArena *a, const PDXEType **members, int count) {
    if (!members || count <= 0)
        return &unknown_singleton;

    // Flatten: nested UNIONs unfold their members into the parent.
    int flat_cap = count * 2 + 4;
    const PDXEType **flat = (const PDXEType **)pdxe_arena_alloc(a, flat_cap * sizeof(const PDXEType *));
    if (!flat)
        return &unknown_singleton;
    int flat_count = 0;
    for (int i = 0; i < count; i++) {
        const PDXEType *m = members[i];
        if (!m || pdxe_type_is_unknown(m))
            continue;
        if (m->kind == PDXE_TYPE_UNION) {
            for (int j = 0; j < m->data.union_type.count; j++) {
                if (flat_count < flat_cap)
                    flat[flat_count++] = m->data.union_type.members[j];
            }
        } else {
            if (flat_count < flat_cap)
                flat[flat_count++] = m;
        }
    }
    if (flat_count == 0)
        return &unknown_singleton;

    // Dedupe by structural equality.
    int unique_count = union_member_dedupe(flat, flat_count);
    if (unique_count == 1)
        return flat[0];

    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_UNION;
    const PDXEType **arr =
        (const PDXEType **)pdxe_arena_alloc(a, (unique_count + 1) * sizeof(const PDXEType *));
    if (!arr)
        return &unknown_singleton;
    for (int i = 0; i < unique_count; i++)
        arr[i] = flat[i];
    arr[unique_count] = NULL;
    t->data.union_type.members = arr;
    t->data.union_type.count = unique_count;
    return t;
}

const PDXEType *pdxe_type_optional(PDXEArena *a, const PDXEType *inner) {
    if (!inner)
        return &unknown_singleton;
    const PDXEType *none_t = pdxe_type_builtin(a, "None");
    const PDXEType *members[2] = {inner, none_t};
    return pdxe_type_union(a, members, 2);
}

const PDXEType *pdxe_type_literal(PDXEArena *a, const PDXEType *base, const char *literal_text) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_LITERAL;
    t->data.literal.base = base ? base : &unknown_singleton;
    t->data.literal.literal_text = literal_text ? pdxe_arena_strdup(a, literal_text) : NULL;
    return t;
}

const PDXEType *pdxe_type_protocol(PDXEArena *a, const char *qualified_name, const char **method_names,
                                 const PDXEType **method_sigs) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_PROTOCOL;
    t->data.protocol.qualified_name = qualified_name ? pdxe_arena_strdup(a, qualified_name) : NULL;

    int n = 0;
    if (method_names) {
        while (method_names[n])
            n++;
    }
    if (n > 0) {
        const char **names = (const char **)pdxe_arena_alloc(a, (n + 1) * sizeof(const char *));
        const PDXEType **sigs =
            (const PDXEType **)pdxe_arena_alloc(a, (n + 1) * sizeof(const PDXEType *));
        if (names && sigs) {
            for (int i = 0; i < n; i++) {
                names[i] = pdxe_arena_strdup(a, method_names[i]);
                sigs[i] = method_sigs ? method_sigs[i] : NULL;
            }
            names[n] = NULL;
            sigs[n] = NULL;
            t->data.protocol.method_names = names;
            t->data.protocol.method_sigs = sigs;
        }
    }
    return t;
}

const PDXEType *pdxe_type_module(PDXEArena *a, const char *module_qn) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_MODULE;
    t->data.module.module_qn = module_qn ? pdxe_arena_strdup(a, module_qn) : NULL;
    return t;
}

const PDXEType *pdxe_type_callable(PDXEArena *a, const PDXEType **param_types, int param_count,
                                 const PDXEType *return_type) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_CALLABLE;
    t->data.callable.param_count = param_count;
    t->data.callable.return_type = return_type ? return_type : &unknown_singleton;
    if (param_count > 0 && param_types) {
        const PDXEType **arr =
            (const PDXEType **)pdxe_arena_alloc(a, (param_count + 1) * sizeof(const PDXEType *));
        if (arr) {
            for (int i = 0; i < param_count; i++)
                arr[i] = param_types[i];
            arr[param_count] = NULL;
            t->data.callable.param_types = arr;
        }
    }
    return t;
}

// --- TS-specific constructors -----------------------------------------------

const PDXEType *pdxe_type_intersection(PDXEArena *a, const PDXEType **members, int count) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_INTERSECTION;
    if (members && count > 0) {
        const PDXEType **arr =
            (const PDXEType **)pdxe_arena_alloc(a, (count + 1) * sizeof(const PDXEType *));
        if (arr) {
            for (int i = 0; i < count; i++)
                arr[i] = members[i];
            arr[count] = NULL;
            t->data.union_type.members = arr;
        }
    }
    t->data.union_type.count = count;
    return t;
}

const PDXEType *pdxe_type_ts_literal(PDXEArena *a, const char *tag, const char *value) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_TS_LITERAL;
    t->data.literal_ts.tag = tag ? pdxe_arena_strdup(a, tag) : NULL;
    t->data.literal_ts.value = value ? pdxe_arena_strdup(a, value) : NULL;
    return t;
}

const PDXEType *pdxe_type_indexed(PDXEArena *a, const PDXEType *object, const PDXEType *index) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_INDEXED;
    t->data.indexed.object = object;
    t->data.indexed.index = index;
    return t;
}

const PDXEType *pdxe_type_keyof(PDXEArena *a, const PDXEType *operand) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_KEYOF;
    t->data.keyof.operand = operand;
    return t;
}

const PDXEType *pdxe_type_typeof_query(PDXEArena *a, const char *expr) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_TYPEOF_QUERY;
    t->data.typeof_query.expr = expr ? pdxe_arena_strdup(a, expr) : NULL;
    return t;
}

const PDXEType *pdxe_type_conditional(PDXEArena *a, const PDXEType *check, const PDXEType *extends,
                                    const PDXEType *true_branch, const PDXEType *false_branch) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_CONDITIONAL;
    t->data.conditional.check = check;
    t->data.conditional.extends = extends;
    t->data.conditional.true_branch = true_branch;
    t->data.conditional.false_branch = false_branch;
    return t;
}

const PDXEType *pdxe_type_object_lit(PDXEArena *a, const char **prop_names, const PDXEType **prop_types,
                                   const PDXEType *call_signature, const PDXEType *index_value) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_OBJECT_LIT;
    if (prop_names && prop_types) {
        int count = 0;
        while (prop_names[count] && prop_types[count])
            count++;
        if (count > 0) {
            const char **name_arr =
                (const char **)pdxe_arena_alloc(a, (count + 1) * sizeof(const char *));
            const PDXEType **type_arr =
                (const PDXEType **)pdxe_arena_alloc(a, (count + 1) * sizeof(const PDXEType *));
            if (name_arr && type_arr) {
                for (int i = 0; i < count; i++) {
                    name_arr[i] = prop_names[i];
                    type_arr[i] = prop_types[i];
                }
                name_arr[count] = NULL;
                type_arr[count] = NULL;
                t->data.object_lit.prop_names = name_arr;
                t->data.object_lit.prop_types = type_arr;
            }
        }
    }
    t->data.object_lit.call_signature = call_signature;
    t->data.object_lit.index_value = index_value;
    return t;
}

const PDXEType *pdxe_type_infer(PDXEArena *a, const char *name) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_INFER;
    t->data.infer.name = name ? pdxe_arena_strdup(a, name) : NULL;
    return t;
}

const PDXEType *pdxe_type_mapped(PDXEArena *a, const char *key_name, const PDXEType *key_constraint,
                               const PDXEType *value) {
    PDXEType *t = (PDXEType *)pdxe_arena_alloc(a, sizeof(PDXEType));
    if (!t)
        return &unknown_singleton;
    memset(t, 0, sizeof(PDXEType));
    t->kind = PDXE_TYPE_MAPPED;
    t->data.mapped.key_name = key_name ? pdxe_arena_strdup(a, key_name) : NULL;
    t->data.mapped.key_constraint = key_constraint;
    t->data.mapped.value = value;
    return t;
}

// Operations

const PDXEType *pdxe_type_deref(const PDXEType *t) {
    if (!t)
        return t;
    // Unwrap references transparently (C++ member access through refs)
    if (t->kind == PDXE_TYPE_REFERENCE || t->kind == PDXE_TYPE_RVALUE_REF)
        return t->data.reference.elem;
    if (t->kind != PDXE_TYPE_POINTER)
        return t;
    return t->data.pointer.elem;
}

const PDXEType *pdxe_type_elem(const PDXEType *t) {
    if (!t)
        return pdxe_type_unknown();
    switch (t->kind) {
    case PDXE_TYPE_POINTER:
        return t->data.pointer.elem;
    case PDXE_TYPE_SLICE:
        return t->data.slice.elem;
    case PDXE_TYPE_CHANNEL:
        return t->data.channel.elem;
    case PDXE_TYPE_REFERENCE:
        return t->data.reference.elem;
    case PDXE_TYPE_RVALUE_REF:
        return t->data.reference.elem;
    default:
        return pdxe_type_unknown();
    }
}

bool pdxe_type_is_unknown(const PDXEType *t) {
    if (!t)
        return true;
    /* Guard against dangling pointers from stale field_types entries.
     * Check alignment before dereferencing — misaligned pointer means garbage. */
    if (((uintptr_t)t & (_Alignof(PDXEType) - 1)) != 0)
        return true;
    return t->kind == PDXE_TYPE_UNKNOWN;
}

bool pdxe_type_is_interface(const PDXEType *t) {
    return t && t->kind == PDXE_TYPE_INTERFACE;
}

bool pdxe_type_is_pointer(const PDXEType *t) {
    return t && t->kind == PDXE_TYPE_POINTER;
}

bool pdxe_type_is_reference(const PDXEType *t) {
    return t && (t->kind == PDXE_TYPE_REFERENCE || t->kind == PDXE_TYPE_RVALUE_REF);
}

bool pdxe_type_is_union(const PDXEType *t) {
    return t && t->kind == PDXE_TYPE_UNION;
}

bool pdxe_type_is_protocol(const PDXEType *t) {
    return t && t->kind == PDXE_TYPE_PROTOCOL;
}

bool pdxe_type_is_module(const PDXEType *t) {
    return t && t->kind == PDXE_TYPE_MODULE;
}

static bool str_eq_or_both_null(const char *a, const char *b) {
    if (a == b)
        return true;
    if (!a || !b)
        return false;
    return strcmp(a, b) == 0;
}

bool pdxe_type_equal(const PDXEType *a, const PDXEType *b) {
    if (a == b)
        return true;
    if (!a || !b)
        return false;
    if (a->kind != b->kind)
        return false;

    switch (a->kind) {
    case PDXE_TYPE_UNKNOWN:
        return true;
    case PDXE_TYPE_NAMED:
        return str_eq_or_both_null(a->data.named.qualified_name, b->data.named.qualified_name);
    case PDXE_TYPE_BUILTIN:
        return str_eq_or_both_null(a->data.builtin.name, b->data.builtin.name);
    case PDXE_TYPE_TYPE_PARAM:
        return str_eq_or_both_null(a->data.type_param.name, b->data.type_param.name);
    case PDXE_TYPE_POINTER:
        return pdxe_type_equal(a->data.pointer.elem, b->data.pointer.elem);
    case PDXE_TYPE_SLICE:
        return pdxe_type_equal(a->data.slice.elem, b->data.slice.elem);
    case PDXE_TYPE_REFERENCE:
    case PDXE_TYPE_RVALUE_REF:
        return pdxe_type_equal(a->data.reference.elem, b->data.reference.elem);
    case PDXE_TYPE_MAP:
        return pdxe_type_equal(a->data.map.key, b->data.map.key) &&
               pdxe_type_equal(a->data.map.value, b->data.map.value);
    case PDXE_TYPE_CHANNEL:
        return a->data.channel.direction == b->data.channel.direction &&
               pdxe_type_equal(a->data.channel.elem, b->data.channel.elem);
    case PDXE_TYPE_TUPLE: {
        if (a->data.tuple.count != b->data.tuple.count)
            return false;
        for (int i = 0; i < a->data.tuple.count; i++) {
            if (!pdxe_type_equal(a->data.tuple.elems[i], b->data.tuple.elems[i]))
                return false;
        }
        return true;
    }
    case PDXE_TYPE_TEMPLATE: {
        if (!str_eq_or_both_null(a->data.template_type.template_name,
                                 b->data.template_type.template_name))
            return false;
        if (a->data.template_type.arg_count != b->data.template_type.arg_count)
            return false;
        for (int i = 0; i < a->data.template_type.arg_count; i++) {
            if (!pdxe_type_equal(a->data.template_type.template_args[i],
                                b->data.template_type.template_args[i]))
                return false;
        }
        return true;
    }
    case PDXE_TYPE_ALIAS:
        return str_eq_or_both_null(a->data.alias.alias_qn, b->data.alias.alias_qn);
    case PDXE_TYPE_UNION: {
        if (a->data.union_type.count != b->data.union_type.count)
            return false;
        // Order-independent: every a-member must appear in b's set.
        for (int i = 0; i < a->data.union_type.count; i++) {
            bool found = false;
            for (int j = 0; j < b->data.union_type.count; j++) {
                if (pdxe_type_equal(a->data.union_type.members[i], b->data.union_type.members[j])) {
                    found = true;
                    break;
                }
            }
            if (!found)
                return false;
        }
        return true;
    }
    case PDXE_TYPE_LITERAL:
        return pdxe_type_equal(a->data.literal.base, b->data.literal.base) &&
               str_eq_or_both_null(a->data.literal.literal_text, b->data.literal.literal_text);
    case PDXE_TYPE_PROTOCOL:
        return str_eq_or_both_null(a->data.protocol.qualified_name,
                                   b->data.protocol.qualified_name);
    case PDXE_TYPE_MODULE:
        return str_eq_or_both_null(a->data.module.module_qn, b->data.module.module_qn);
    case PDXE_TYPE_CALLABLE: {
        if (a->data.callable.param_count != b->data.callable.param_count)
            return false;
        if (!pdxe_type_equal(a->data.callable.return_type, b->data.callable.return_type))
            return false;
        if (a->data.callable.param_count > 0) {
            for (int i = 0; i < a->data.callable.param_count; i++) {
                if (!pdxe_type_equal(a->data.callable.param_types[i],
                                    b->data.callable.param_types[i]))
                    return false;
            }
        }
        return true;
    }
    case PDXE_TYPE_INTERSECTION: {
        // Same shape as UNION; compare order-independently.
        if (a->data.union_type.count != b->data.union_type.count)
            return false;
        for (int i = 0; i < a->data.union_type.count; i++) {
            bool found = false;
            for (int j = 0; j < b->data.union_type.count; j++) {
                if (pdxe_type_equal(a->data.union_type.members[i], b->data.union_type.members[j])) {
                    found = true;
                    break;
                }
            }
            if (!found)
                return false;
        }
        return true;
    }
    case PDXE_TYPE_TS_LITERAL:
        return str_eq_or_both_null(a->data.literal_ts.tag, b->data.literal_ts.tag) &&
               str_eq_or_both_null(a->data.literal_ts.value, b->data.literal_ts.value);
    case PDXE_TYPE_INDEXED:
        return pdxe_type_equal(a->data.indexed.object, b->data.indexed.object) &&
               pdxe_type_equal(a->data.indexed.index, b->data.indexed.index);
    case PDXE_TYPE_KEYOF:
        return pdxe_type_equal(a->data.keyof.operand, b->data.keyof.operand);
    case PDXE_TYPE_TYPEOF_QUERY:
        return str_eq_or_both_null(a->data.typeof_query.expr, b->data.typeof_query.expr);
    case PDXE_TYPE_CONDITIONAL:
        return pdxe_type_equal(a->data.conditional.check, b->data.conditional.check) &&
               pdxe_type_equal(a->data.conditional.extends, b->data.conditional.extends) &&
               pdxe_type_equal(a->data.conditional.true_branch, b->data.conditional.true_branch) &&
               pdxe_type_equal(a->data.conditional.false_branch, b->data.conditional.false_branch);
    case PDXE_TYPE_INFER:
        return str_eq_or_both_null(a->data.infer.name, b->data.infer.name);
    case PDXE_TYPE_OBJECT_LIT:
    case PDXE_TYPE_MAPPED:
    case PDXE_TYPE_FUNC:
    case PDXE_TYPE_INTERFACE:
    case PDXE_TYPE_STRUCT:
        // Structural equality on these is expensive and rarely needed by callers
        // beyond pointer identity (already checked above). Treat as not-equal.
        return false;
    }
    return false;
}

bool pdxe_type_protocol_satisfied_by(const PDXEType *proto, const PDXEType *candidate) {
    if (!proto || proto->kind != PDXE_TYPE_PROTOCOL)
        return false;
    if (!candidate)
        return false;
    // candidate must be a NAMED or PROTOCOL type with a method-name set we
    // can inspect. For PROTOCOL candidates, trivially satisfied if every
    // proto method appears in candidate's method list.
    if (candidate->kind == PDXE_TYPE_PROTOCOL) {
        if (!proto->data.protocol.method_names)
            return true;
        for (int i = 0; proto->data.protocol.method_names[i]; i++) {
            const char *needed = proto->data.protocol.method_names[i];
            bool found = false;
            if (candidate->data.protocol.method_names) {
                for (int j = 0; candidate->data.protocol.method_names[j]; j++) {
                    if (str_eq_or_both_null(needed, candidate->data.protocol.method_names[j])) {
                        found = true;
                        break;
                    }
                }
            }
            if (!found)
                return false;
        }
        return true;
    }
    // Nominal candidates require the registry — caller's responsibility.
    return false;
}

const PDXEType *pdxe_type_resolve_alias(const PDXEType *t) {
    for (int i = 0; i < 16 && t; i++) {
        if (t->kind != PDXE_TYPE_ALIAS)
            return t;
        if (!t->data.alias.underlying)
            return t;
        t = t->data.alias.underlying;
    }
    return t;
}

// Generic substitution: recursively replace TYPE_PARAM with concrete types.
const PDXEType *pdxe_type_substitute(PDXEArena *a, const PDXEType *t, const char **type_params,
                                   const PDXEType **type_args) {
    if (!t)
        return pdxe_type_unknown();
    if (!type_params || !type_args)
        return t;

    /* type_args may be SHORTER than type_params — a class template instantiated
     * with fewer args than declared params, or trailing default template args
     * (e.g. `Box<Widget>` for `template<class T, class U, class V>`). Indexing
     * type_args[i] by the type_params loop index would then read past the args
     * array, yielding a bogus PDXEType* that is later dereferenced -> SEGV (#427).
     * type_params is always NULL-terminated; type_args is either parallel-length
     * (some callers pass a fixed positional array that is NOT NULL-terminated) or
     * shorter-and-NULL-terminated. Bound the length walk by the param count so it
     * can never run off a non-terminated args array, then bound every type_args[i]
     * access by the result. */
    int nparams = 0;
    while (type_params[nparams]) {
        nparams++;
    }
    /* Contract: type_args must be NULL-terminated (it may be shorter than
     * type_params). A misaligned or null-page value can never be a real
     * PDXEType* — it means a caller passed an unterminated array and the walk
     * is reading uninitialized memory (seen on bitcoin's serialize.h: an
     * explicit-template-arg call bound T to stack garbage that was woven into
     * the registered type graph and dereferenced later -> SIGSEGV). Treat such
     * values as the terminator so garbage can never enter a type graph. */
    int args_len = 0;
    while (args_len < nparams && type_args[args_len] &&
           ((uintptr_t)type_args[args_len] & (sizeof(void *) - 1)) == 0 &&
           (uintptr_t)type_args[args_len] >= TR_MIN_PLAUSIBLE_PTR) {
        args_len++;
    }

    switch (t->kind) {
    case PDXE_TYPE_TYPE_PARAM: {
        for (int i = 0; type_params[i]; i++) {
            if (strcmp(t->data.type_param.name, type_params[i]) == 0) {
                return (i < args_len && type_args[i]) ? type_args[i] : t;
            }
        }
        return t; // unmatched param stays as-is
    }
    case PDXE_TYPE_NAMED: {
        // Also substitute NAMED types matching template param names.
        // c_parse_return_type_text may parse "A" as NAMED("test.main.A")
        // instead of TYPE_PARAM("A") — check both full QN and short name.
        const char *qn = t->data.named.qualified_name;
        if (qn) {
            const char *short_name = strrchr(qn, '.');
            short_name = short_name ? short_name + 1 : qn;
            for (int i = 0; type_params[i]; i++) {
                if (strcmp(qn, type_params[i]) == 0 || strcmp(short_name, type_params[i]) == 0) {
                    return (i < args_len && type_args[i]) ? type_args[i] : t;
                }
            }
        }
        return t;
    }
    case PDXE_TYPE_POINTER:
        return pdxe_type_pointer(
            a, pdxe_type_substitute(a, t->data.pointer.elem, type_params, type_args));
    case PDXE_TYPE_REFERENCE:
        return pdxe_type_reference(
            a, pdxe_type_substitute(a, t->data.reference.elem, type_params, type_args));
    case PDXE_TYPE_RVALUE_REF:
        return pdxe_type_rvalue_ref(
            a, pdxe_type_substitute(a, t->data.reference.elem, type_params, type_args));
    case PDXE_TYPE_SLICE:
        return pdxe_type_slice(a,
                              pdxe_type_substitute(a, t->data.slice.elem, type_params, type_args));
    case PDXE_TYPE_MAP:
        return pdxe_type_map(a, pdxe_type_substitute(a, t->data.map.key, type_params, type_args),
                            pdxe_type_substitute(a, t->data.map.value, type_params, type_args));
    case PDXE_TYPE_CHANNEL:
        return pdxe_type_channel(
            a, pdxe_type_substitute(a, t->data.channel.elem, type_params, type_args),
            t->data.channel.direction);
    case PDXE_TYPE_TUPLE: {
        int count = t->data.tuple.count;
        const PDXEType **elems =
            (const PDXEType **)pdxe_arena_alloc(a, (count + 1) * sizeof(const PDXEType *));
        if (!elems)
            return t;
        for (int i = 0; i < count; i++) {
            elems[i] = pdxe_type_substitute(a, t->data.tuple.elems[i], type_params, type_args);
        }
        elems[count] = NULL;
        return pdxe_type_tuple(a, elems, count);
    }
    case PDXE_TYPE_UNION:
    case PDXE_TYPE_INTERSECTION: {
        int count = t->data.union_type.count;
        if (count <= 0 || !t->data.union_type.members)
            return t;
        const PDXEType **elems =
            (const PDXEType **)pdxe_arena_alloc(a, (count + 1) * sizeof(const PDXEType *));
        if (!elems)
            return t;
        for (int i = 0; i < count; i++) {
            elems[i] =
                pdxe_type_substitute(a, t->data.union_type.members[i], type_params, type_args);
        }
        elems[count] = NULL;
        return t->kind == PDXE_TYPE_UNION ? pdxe_type_union(a, elems, count)
                                         : pdxe_type_intersection(a, elems, count);
    }
    case PDXE_TYPE_INDEXED:
        return pdxe_type_indexed(
            a, pdxe_type_substitute(a, t->data.indexed.object, type_params, type_args),
            pdxe_type_substitute(a, t->data.indexed.index, type_params, type_args));
    case PDXE_TYPE_KEYOF:
        return pdxe_type_keyof(
            a, pdxe_type_substitute(a, t->data.keyof.operand, type_params, type_args));
    case PDXE_TYPE_CONDITIONAL:
        return pdxe_type_conditional(
            a, pdxe_type_substitute(a, t->data.conditional.check, type_params, type_args),
            pdxe_type_substitute(a, t->data.conditional.extends, type_params, type_args),
            pdxe_type_substitute(a, t->data.conditional.true_branch, type_params, type_args),
            pdxe_type_substitute(a, t->data.conditional.false_branch, type_params, type_args));
    case PDXE_TYPE_FUNC: {
        // Recurse into param_types and return_types. Param/return arrays may be NULL.
        const PDXEType **new_params = NULL;
        const PDXEType **new_returns = NULL;
        if (t->data.func.param_types) {
            int pc = 0;
            while (t->data.func.param_types[pc])
                pc++;
            new_params =
                (const PDXEType **)pdxe_arena_alloc(a, (size_t)(pc + 1) * sizeof(const PDXEType *));
            if (!new_params)
                return t;
            for (int i = 0; i < pc; i++) {
                new_params[i] =
                    pdxe_type_substitute(a, t->data.func.param_types[i], type_params, type_args);
            }
            new_params[pc] = NULL;
        }
        if (t->data.func.return_types) {
            int rc = 0;
            while (t->data.func.return_types[rc])
                rc++;
            new_returns =
                (const PDXEType **)pdxe_arena_alloc(a, (size_t)(rc + 1) * sizeof(const PDXEType *));
            if (!new_returns)
                return t;
            for (int i = 0; i < rc; i++) {
                new_returns[i] =
                    pdxe_type_substitute(a, t->data.func.return_types[i], type_params, type_args);
            }
            new_returns[rc] = NULL;
        }
        return pdxe_type_func(a, t->data.func.param_names, new_params, new_returns);
    }
    case PDXE_TYPE_TEMPLATE: {
        if (!t->data.template_type.template_args || t->data.template_type.arg_count == 0)
            return t;
        int ac = t->data.template_type.arg_count;
        const PDXEType **new_args =
            (const PDXEType **)pdxe_arena_alloc(a, (size_t)(ac + 1) * sizeof(const PDXEType *));
        if (!new_args)
            return t;
        for (int i = 0; i < ac; i++) {
            new_args[i] = pdxe_type_substitute(a, t->data.template_type.template_args[i],
                                              type_params, type_args);
        }
        new_args[ac] = NULL;
        return pdxe_type_template(a, t->data.template_type.template_name, new_args, ac);
    }
    default:
        // BUILTIN, INTERFACE, STRUCT, LITERAL, TYPEOF_QUERY, OBJECT_LIT, INFER, MAPPED,
        // ALIAS — no in-place substitution needed at v1 (or stub-only).
        return t;
    }
}
