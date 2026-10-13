/*
 * A file's surface: everything typed resolution reads from the file's extraction
 * result, as bytes an extraction cache can keep.
 *
 * Typed resolution runs over the whole repository on every build, so that its answer
 * never depends on a previous one. A file whose content has not changed is not
 * extracted again for it: the cache hands back this surface instead, and resolution
 * rebuilds the engine's result from it and parses the file's source once more for
 * the tree, which is all it lacks. For that to give the same answer as a fresh
 * extraction, the surface must hold every value the resolver could read. So it holds
 * every field of every fact the engine records for a file, not a selection: a
 * selection would be a guess about the resolver that an engine refresh could make
 * wrong without anything noticing. A test checks the tables below against the
 * engine's own structure definitions, field by field.
 *
 * Only what the rebuilt result cannot carry is left out: the parse tree (resolution
 * parses again), the retained source (resolution is handed it), the result's memory
 * pool, and the parts results of other files that an aggregate one would own.
 *
 * The encoding is JSON with every field written, in a fixed order, with null for an
 * absent string, so equal results always encode to equal bytes.
 *
 * Strings are written byte for byte as the engine holds them, and read back the same.
 * They are often the file's own text, which need not be valid UTF-8, and resolving
 * from a surface must see exactly what resolving fresh would: a string made valid on
 * the way through would be a different name. So a surface of such a file is JSON in
 * shape but not strict JSON, and the same holds for a floating-point value the engine
 * computed as infinite or not a number. Nothing but this codec reads a surface.
 *
 * Nothing is held twice (issue 90, decision 61). Text a fact found in the file's own
 * source, spelled exactly as the source spells it within the fact's site, is written
 * as its range, [start, end], and an argument the engine holds as a range stays one:
 * a member chain's links, or an argument holding a call holding an argument, would
 * otherwise repeat every byte they overlap. A qualified name equal to a definition's is
 * written as that definition's index, and a definition's own qualified name by its
 * relation to an earlier definition's, [base] for the base's name, `.` and its own
 * name, [base, rest] for the base's name followed by `rest`: nested definitions would
 * otherwise each repeat the names around them. The surface records the source's
 * length; decoding checks every range and index, and the text of a range is filled in
 * from the file's source when the file is resolved (pdxe_surface_bind). What decoding
 * gives is byte for byte what the extraction held.
 */

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "pdxe.h"
#include "pdxe_core.h"
#include "foundation/hash_table.h"
#include "shim_internal.h"
#include "yyjson/yyjson.h"

/* 2: the surface carries what the extraction lost (the "lost" count). 3: calls and
 * usages carry their node-type path (issue 46). 4: definitions carry their route
 * facts (issue 54). 5: text as source ranges, qualified names by definition, and the
 * source's length (issue 90). */
enum { SURFACE_VERSION = 5 };

/* The file's bytes, and the engine's numbers, as they are; see the comment above. */
#define SURFACE_WRITE_FLAGS (YYJSON_WRITE_ALLOW_INVALID_UNICODE | YYJSON_WRITE_ALLOW_INF_AND_NAN)
#define SURFACE_READ_FLAGS (YYJSON_READ_ALLOW_INVALID_UNICODE | YYJSON_READ_ALLOW_INF_AND_NAN)

/* --- field tables ----------------------------------------------------------- */

typedef enum {
    F_STR,     /* const char * */
    F_STRV,    /* const char **, NULL-terminated */
    F_STRN,    /* const char **, counted by the int at count_offset */
    F_U32N,    /* uint32_t *, counted by the int at count_offset */
    F_ARGS,    /* PDXECallArg *, counted by the int at count_offset */
    F_ROUTES,  /* PDXERouteFact *, counted by the int at count_offset */
    F_INT,     /* int */
    F_U32,     /* uint32_t */
    F_BOOL,    /* bool */
    F_FLOAT,   /* float */
    F_ENUM,    /* an enumeration, stored as int */
    F_TEXT,    /* const char *, text a site may hold as a range of the file's source */
    F_QNREF,   /* const char *, a name that may be a definition's qualified name */
    F_TEXTQN,  /* const char *, either of the two */
    F_QNAME,   /* const char *, a definition's own qualified name */
} field_kind;

/* The site of an F_TEXT or F_TEXTQN field has no origin: it is always the file's own. */
#define RAW_ORIGIN ((size_t)-1)

typedef struct {
    const char *key;
    field_kind kind;
    size_t offset;
    size_t count_offset;
    /* F_TEXT, F_TEXTQN: the uint32_t start and end of the site the text is in, and its
     * PDXESourceOrigin, or RAW_ORIGIN. */
    size_t start_offset;
    size_t end_offset;
    size_t origin_offset;
} field;

/* The engine's enumerations are stored as int; the codec relies on that. */
_Static_assert(sizeof(PDXESourceOrigin) == sizeof(int), "enumerations must be int-sized");
_Static_assert(sizeof(PDXEUsageKind) == sizeof(int), "enumerations must be int-sized");
_Static_assert(sizeof(PDXEResolvedKind) == sizeof(int), "enumerations must be int-sized");
_Static_assert(sizeof(PDXEStringRefKind) == sizeof(int), "enumerations must be int-sized");
_Static_assert(sizeof(PDXEChannelDirection) == sizeof(int), "enumerations must be int-sized");
_Static_assert(sizeof(PDXELanguage) == sizeof(int), "enumerations must be int-sized");

#define FIELD(type, kind, name) {#name, kind, offsetof(type, name), 0, 0, 0, 0}
#define COUNTED(type, kind, name, count)                                                         \
    {#name, kind, offsetof(type, name), offsetof(type, count), 0, 0, 0}
#define TEXT_FIELD(type, kind, name, start, end, origin)                                         \
    {#name, kind, offsetof(type, name), 0, offsetof(type, start), offsetof(type, end),            \
     offsetof(type, origin)}
#define RAW_TEXT_FIELD(type, kind, name, start, end)                                             \
    {#name, kind, offsetof(type, name), 0, offsetof(type, start), offsetof(type, end), RAW_ORIGIN}

static const field DEFINITION_FIELDS[] = {
    FIELD(PDXEDefinition, F_STR, name),
    FIELD(PDXEDefinition, F_QNAME, qualified_name),
    FIELD(PDXEDefinition, F_STR, label),
    FIELD(PDXEDefinition, F_STR, file_path),
    FIELD(PDXEDefinition, F_U32, start_line),
    FIELD(PDXEDefinition, F_U32, end_line),
    FIELD(PDXEDefinition, F_STR, signature),
    FIELD(PDXEDefinition, F_U32, signature_start),
    FIELD(PDXEDefinition, F_U32, signature_end),
    FIELD(PDXEDefinition, F_STR, return_type),
    FIELD(PDXEDefinition, F_STR, receiver),
    FIELD(PDXEDefinition, F_STR, docstring),
    FIELD(PDXEDefinition, F_QNREF, parent_class),
    FIELD(PDXEDefinition, F_STRV, decorators),
    FIELD(PDXEDefinition, F_STRV, base_classes),
    FIELD(PDXEDefinition, F_STRV, param_names),
    FIELD(PDXEDefinition, F_STRV, param_types),
    COUNTED(PDXEDefinition, F_STRN, signature_param_types, signature_param_count),
    FIELD(PDXEDefinition, F_INT, signature_param_count),
    FIELD(PDXEDefinition, F_STRV, return_types),
    FIELD(PDXEDefinition, F_STR, route_path),
    FIELD(PDXEDefinition, F_STR, route_method),
    FIELD(PDXEDefinition, F_INT, complexity),
    FIELD(PDXEDefinition, F_INT, cognitive),
    FIELD(PDXEDefinition, F_INT, loop_count),
    FIELD(PDXEDefinition, F_INT, loop_depth),
    FIELD(PDXEDefinition, F_BOOL, is_recursive),
    FIELD(PDXEDefinition, F_INT, param_count),
    FIELD(PDXEDefinition, F_INT, max_access_depth),
    FIELD(PDXEDefinition, F_INT, linear_scan_in_loop),
    FIELD(PDXEDefinition, F_INT, alloc_in_loop),
    FIELD(PDXEDefinition, F_BOOL, recursion_in_loop),
    FIELD(PDXEDefinition, F_BOOL, unguarded_recursion),
    FIELD(PDXEDefinition, F_INT, lines),
    COUNTED(PDXEDefinition, F_U32N, fingerprint, fingerprint_k),
    FIELD(PDXEDefinition, F_INT, fingerprint_k),
    FIELD(PDXEDefinition, F_BOOL, is_exported),
    FIELD(PDXEDefinition, F_BOOL, is_abstract),
    FIELD(PDXEDefinition, F_BOOL, is_test),
    FIELD(PDXEDefinition, F_BOOL, is_entry_point),
    FIELD(PDXEDefinition, F_STR, structural_profile),
    FIELD(PDXEDefinition, F_STR, body_tokens),
    FIELD(PDXEDefinition, F_STR, impl_trait),
    COUNTED(PDXEDefinition, F_ROUTES, routes, route_count),
    FIELD(PDXEDefinition, F_INT, route_count),
};

static const field ROUTE_FIELDS[] = {
    FIELD(PDXERouteFact, F_STR, method),
    FIELD(PDXERouteFact, F_STR, path),
    FIELD(PDXERouteFact, F_STR, callee_text),
    RAW_TEXT_FIELD(PDXERouteFact, F_TEXT, source_text, start_byte, end_byte),
    FIELD(PDXERouteFact, F_STRV, ast_path),
    FIELD(PDXERouteFact, F_U32, start_byte),
    FIELD(PDXERouteFact, F_U32, end_byte),
};

static const field CALL_FIELDS[] = {
    TEXT_FIELD(PDXECall, F_TEXT, callee_name, site_start_byte, site_end_byte, source_origin),
    FIELD(PDXECall, F_QNREF, enclosing_func_qn),
    FIELD(PDXECall, F_STR, first_string_arg),
    FIELD(PDXECall, F_STR, second_arg_name),
    COUNTED(PDXECall, F_ARGS, args, arg_count),
    FIELD(PDXECall, F_INT, arg_count),
    FIELD(PDXECall, F_INT, loop_depth),
    FIELD(PDXECall, F_INT, branch_depth),
    FIELD(PDXECall, F_INT, start_line),
    FIELD(PDXECall, F_U32, site_start_byte),
    FIELD(PDXECall, F_U32, site_end_byte),
    FIELD(PDXECall, F_ENUM, source_origin),
    FIELD(PDXECall, F_BOOL, is_method),
    FIELD(PDXECall, F_BOOL, requires_lsp_resolution),
    FIELD(PDXECall, F_BOOL, callee_is_locally_bound),
    FIELD(PDXECall, F_BOOL, receiver_is_self_attribute),
    FIELD(PDXECall, F_STRV, ast_path),
};

static const field CALL_ARG_FIELDS[] = {
    FIELD(PDXECallArg, F_STR, expr),
    FIELD(PDXECallArg, F_STR, value),
    FIELD(PDXECallArg, F_STR, keyword),
    FIELD(PDXECallArg, F_INT, index),
    FIELD(PDXECallArg, F_U32, expr_start),
    FIELD(PDXECallArg, F_U32, expr_end),
};

static const field IMPORT_FIELDS[] = {
    FIELD(PDXEImport, F_STR, local_name),
    FIELD(PDXEImport, F_STR, module_path),
};

static const field USAGE_FIELDS[] = {
    TEXT_FIELD(PDXEUsage, F_TEXT, ref_name, site_start_byte, site_end_byte, source_origin),
    FIELD(PDXEUsage, F_QNREF, enclosing_func_qn),
    FIELD(PDXEUsage, F_U32, lexical_scope_id),
    FIELD(PDXEUsage, F_U32, site_start_byte),
    FIELD(PDXEUsage, F_U32, site_end_byte),
    FIELD(PDXEUsage, F_ENUM, kind),
    FIELD(PDXEUsage, F_ENUM, source_origin),
    FIELD(PDXEUsage, F_BOOL, may_be_call_reference),
    FIELD(PDXEUsage, F_BOOL, semantic_reference_blocked),
    FIELD(PDXEUsage, F_BOOL, semantic_reference_local_shadow),
    FIELD(PDXEUsage, F_BOOL, is_member_access),
    FIELD(PDXEUsage, F_STRV, ast_path),
};

static const field THROW_FIELDS[] = {
    FIELD(PDXEThrow, F_STR, exception_name),
    FIELD(PDXEThrow, F_QNREF, enclosing_func_qn),
};

static const field READ_WRITE_FIELDS[] = {
    FIELD(PDXEReadWrite, F_STR, var_name),
    FIELD(PDXEReadWrite, F_QNREF, enclosing_func_qn),
    FIELD(PDXEReadWrite, F_BOOL, is_write),
    FIELD(PDXEReadWrite, F_BOOL, is_member_access),
};

static const field TYPE_REF_FIELDS[] = {
    FIELD(PDXETypeRef, F_STR, type_name),
    FIELD(PDXETypeRef, F_QNREF, enclosing_func_qn),
};

static const field ENV_ACCESS_FIELDS[] = {
    FIELD(PDXEEnvAccess, F_STR, env_key),
    FIELD(PDXEEnvAccess, F_QNREF, enclosing_func_qn),
};

static const field TYPE_ASSIGN_FIELDS[] = {
    FIELD(PDXETypeAssign, F_STR, var_name),
    FIELD(PDXETypeAssign, F_STR, type_name),
    FIELD(PDXETypeAssign, F_QNREF, enclosing_func_qn),
};

static const field IMPL_TRAIT_FIELDS[] = {
    FIELD(PDXEImplTrait, F_STR, trait_name),
    FIELD(PDXEImplTrait, F_STR, struct_name),
    FIELD(PDXEImplTrait, F_QNREF, struct_qn),
};

static const field RESOLVED_CALL_FIELDS[] = {
    FIELD(PDXEResolvedCall, F_QNREF, caller_qn),
    TEXT_FIELD(PDXEResolvedCall, F_TEXTQN, callee_qn, site_start_byte, site_end_byte,
               source_origin),
    FIELD(PDXEResolvedCall, F_STR, strategy),
    FIELD(PDXEResolvedCall, F_FLOAT, confidence),
    FIELD(PDXEResolvedCall, F_STR, reason),
    FIELD(PDXEResolvedCall, F_ENUM, kind),
    FIELD(PDXEResolvedCall, F_U32, site_start_byte),
    FIELD(PDXEResolvedCall, F_U32, site_end_byte),
    FIELD(PDXEResolvedCall, F_ENUM, source_origin),
};

static const field STRING_REF_FIELDS[] = {
    FIELD(PDXEStringRef, F_STR, value),
    FIELD(PDXEStringRef, F_QNREF, enclosing_func_qn),
    FIELD(PDXEStringRef, F_STR, key_path),
    FIELD(PDXEStringRef, F_ENUM, kind),
};

static const field INFRA_BINDING_FIELDS[] = {
    FIELD(PDXEInfraBinding, F_STR, source_name),
    FIELD(PDXEInfraBinding, F_STR, target_url),
    FIELD(PDXEInfraBinding, F_STR, broker),
};

static const field CHANNEL_FIELDS[] = {
    FIELD(PDXEChannel, F_STR, channel_name),
    FIELD(PDXEChannel, F_STR, transport),
    FIELD(PDXEChannel, F_QNREF, enclosing_func_qn),
    FIELD(PDXEChannel, F_ENUM, direction),
};

/* The file-level values, written as one object. */
static const field FILE_FIELDS[] = {
    FIELD(PDXEFileResult, F_STR, module_qn),
    FIELD(PDXEFileResult, F_STR, namespace_name),
    FIELD(PDXEFileResult, F_STRV, exports),
    FIELD(PDXEFileResult, F_STRV, constants),
    FIELD(PDXEFileResult, F_STRV, global_vars),
    FIELD(PDXEFileResult, F_STRV, macros),
    FIELD(PDXEFileResult, F_BOOL, has_error),
    FIELD(PDXEFileResult, F_STR, error_msg),
    FIELD(PDXEFileResult, F_BOOL, parse_incomplete),
    FIELD(PDXEFileResult, F_BOOL, parse_unusable),
    FIELD(PDXEFileResult, F_STR, error_ranges),
    FIELD(PDXEFileResult, F_INT, error_region_count),
    FIELD(PDXEFileResult, F_BOOL, is_test_file),
    FIELD(PDXEFileResult, F_INT, imports_count),
    FIELD(PDXEFileResult, F_BOOL, lsp_skipped),
    FIELD(PDXEFileResult, F_BOOL, walk_truncated),
    FIELD(PDXEFileResult, F_U32, tree_nodes),
    FIELD(PDXEFileResult, F_U32, walk_nodes_visited),
    FIELD(PDXEFileResult, F_ENUM, cached_lang),
};

#define N(table) (sizeof(table) / sizeof((table)[0]))

/* One array of the result: where its items, count and capacity live, and its fields. */
typedef struct {
    const char *key;
    size_t items_offset; /* the array struct inside PDXEFileResult */
    size_t item_size;
    const field *fields;
    size_t n_fields;
} array_spec;

/* Every array type in the result has the same shape: items, count, cap. */
typedef struct {
    void *items;
    int count;
    int cap;
} any_array;

#define ARRAY(key, member, type, table) \
    {key, offsetof(PDXEFileResult, member), sizeof(type), table, N(table)}

static const array_spec ARRAYS[] = {
    ARRAY("defs", defs, PDXEDefinition, DEFINITION_FIELDS),
    ARRAY("calls", calls, PDXECall, CALL_FIELDS),
    ARRAY("imports", imports, PDXEImport, IMPORT_FIELDS),
    ARRAY("usages", usages, PDXEUsage, USAGE_FIELDS),
    ARRAY("throws", throws, PDXEThrow, THROW_FIELDS),
    ARRAY("rw", rw, PDXEReadWrite, READ_WRITE_FIELDS),
    ARRAY("type_refs", type_refs, PDXETypeRef, TYPE_REF_FIELDS),
    ARRAY("env_accesses", env_accesses, PDXEEnvAccess, ENV_ACCESS_FIELDS),
    ARRAY("type_assigns", type_assigns, PDXETypeAssign, TYPE_ASSIGN_FIELDS),
    ARRAY("impl_traits", impl_traits, PDXEImplTrait, IMPL_TRAIT_FIELDS),
    ARRAY("resolved_calls", resolved_calls, PDXEResolvedCall, RESOLVED_CALL_FIELDS),
    ARRAY("string_refs", string_refs, PDXEStringRef, STRING_REF_FIELDS),
    ARRAY("infra_bindings", infra_bindings, PDXEInfraBinding, INFRA_BINDING_FIELDS),
    ARRAY("channels", channels, PDXEChannel, CHANNEL_FIELDS),
};

_Static_assert(offsetof(PDXEDefArray, items) == offsetof(any_array, items) &&
                   offsetof(PDXEDefArray, count) == offsetof(any_array, count) &&
                   offsetof(PDXEDefArray, cap) == offsetof(any_array, cap),
               "result arrays must share one layout");

/* --- encoding ---------------------------------------------------------------- */

#define AT(base, offset, type) ((type *)((char *)(base) + (offset)))
#define AT_CONST(base, offset, type) ((type const *)((const char *)(base) + (offset)))

static yyjson_mut_val *string_value(yyjson_mut_doc *doc, const char *s) {
    return s ? yyjson_mut_strcpy(doc, s) : yyjson_mut_null(doc);
}

bool pdxe_text_in_site(const char *source, size_t source_len, const char *s, uint32_t start,
                       uint32_t limit, uint32_t *end) {
    if (!source || !s || start >= limit || (size_t)limit > source_len) {
        return false;
    }
    size_t n = strlen(s);
    if (n > (size_t)(limit - start) || memcmp(source + start, s, n) != 0) {
        return false;
    }
    *end = start + (uint32_t)n;
    return true;
}

static int qn_relation(const char *qn, const char *name, const char *base, const char **rest) {
    if (!qn || !base) {
        return -1;
    }
    size_t n = strlen(base);
    if (strncmp(qn, base, n) != 0) {
        return -1;
    }
    const char *after = qn + n;
    if (after[0] == '.' && name && strcmp(after + 1, name) == 0) {
        return PDXE_QN_MEMBER;
    }
    if ((unsigned char)after[0] < 0x80) {
        *rest = after;
        return PDXE_QN_EXTENDED;
    }
    return -1;
}

bool pdxe_qn_index_init(pdxe_qn_index *x, const char *const *qns, uint32_t n) {
    memset(x, 0, sizeof(*x));
    x->names = pdxe_ht_create(n > 0 ? n : 1);
    if (!x->names) {
        return false;
    }
    for (uint32_t i = 0; i < n; i++) {
        if (qns[i] && !pdxe_ht_get(x->names, qns[i])) {
            pdxe_ht_set(x->names, qns[i], (void *)(uintptr_t)((uintptr_t)i + 1));
        }
    }
    return true;
}

void pdxe_qn_index_free(pdxe_qn_index *x) {
    if (x->names) {
        pdxe_ht_free(x->names);
    }
    free(x->key);
    free(x->prefixes);
    free(x->bounds);
    free(x->hashes);
    memset(x, 0, sizeof(*x));
}

uint32_t pdxe_qn_index_find(const pdxe_qn_index *x, const char *qn) {
    uintptr_t found = qn && x->names ? (uintptr_t)pdxe_ht_get(x->names, qn) : 0;
    return found ? (uint32_t)(found - 1) : UINT32_MAX;
}

/* The first definition whose qualified name is the first `len` bytes of `qn`. */
static uint32_t qn_index_find_prefix(pdxe_qn_index *x, const char *qn, size_t len) {
    if (len + 1 > x->key_cap) {
        size_t cap = len + 1 > 2 * x->key_cap ? len + 1 : 2 * x->key_cap;
        char *key = (char *)realloc(x->key, cap);
        if (!key) {
            return UINT32_MAX; /* the name is then held whole, which is never wrong */
        }
        x->key = key;
        x->key_cap = cap;
    }
    memcpy(x->key, qn, len);
    x->key[len] = '\0';
    return pdxe_qn_index_find(x, x->key);
}

/* The `.`-bounded prefixes of `qn`, `len` bytes, shortest first, and the whole name
 * last, each with its hash: one pass over the name. False when they cannot be held, or
 * the name is too long for a prefix length. */
static bool qn_bounds(pdxe_qn_index *x, const char *qn, size_t len, size_t *n) {
    *n = 0;
    if (len == 0 || len > UINT32_MAX) {
        return false;
    }
    uint64_t h = 1469598103934665603ULL;
    for (size_t k = 0; k <= len; k++) {
        if (k > 0 && (k == len || qn[k] == '.')) {
            if (*n == x->bounds_cap) {
                size_t cap = x->bounds_cap ? 2 * x->bounds_cap : 16;
                uint32_t *bounds = (uint32_t *)realloc(x->bounds, cap * sizeof(*bounds));
                if (bounds) {
                    x->bounds = bounds;
                }
                uint64_t *hashes = bounds ? (uint64_t *)realloc(x->hashes, cap * sizeof(*hashes))
                                          : NULL;
                if (hashes) {
                    x->hashes = hashes;
                }
                if (!bounds || !hashes) {
                    return false;
                }
                x->bounds_cap = cap;
            }
            x->bounds[*n] = (uint32_t)k;
            x->hashes[*n] = h;
            (*n)++;
        }
        if (k < len) {
            h = (h ^ (unsigned char)qn[k]) * 1099511628211ULL;
        }
    }
    return true;
}

/* The recorded prefix that is the first `len` bytes of `qn`, hashed `hash`, or NULL. */
static const pdxe_qn_prefix *qn_prefix_find(const pdxe_qn_index *x, const char *qn,
                                            uint32_t len, uint64_t hash) {
    if (!x->prefixes) {
        return NULL;
    }
    size_t mask = x->prefix_cap - 1;
    for (size_t at = (size_t)hash & mask;; at = (at + 1) & mask) {
        const pdxe_qn_prefix *p = &x->prefixes[at];
        if (!p->text) {
            return NULL;
        }
        if (p->hash == hash && p->len == len && memcmp(p->text, qn, len) == 0) {
            return p;
        }
    }
}

/* Records a prefix not yet recorded. False when it cannot: a prefix missing from the
 * index only leaves a later name held with more bytes, never wrong. */
static bool qn_prefix_add(pdxe_qn_index *x, const pdxe_qn_prefix *p) {
    if (2 * (x->n_prefixes + 1) > x->prefix_cap) {
        size_t cap = x->prefix_cap ? 2 * x->prefix_cap : 64;
        pdxe_qn_prefix *grown = (pdxe_qn_prefix *)calloc(cap, sizeof(*grown));
        if (!grown) {
            return false;
        }
        for (size_t i = 0; i < x->prefix_cap; i++) {
            if (x->prefixes[i].text) {
                size_t at = (size_t)x->prefixes[i].hash & (cap - 1);
                while (grown[at].text) {
                    at = (at + 1) & (cap - 1);
                }
                grown[at] = x->prefixes[i];
            }
        }
        free(x->prefixes);
        x->prefixes = grown;
        x->prefix_cap = cap;
    }
    size_t at = (size_t)p->hash & (x->prefix_cap - 1);
    while (x->prefixes[at].text) {
        at = (at + 1) & (x->prefix_cap - 1);
    }
    x->prefixes[at] = *p;
    x->n_prefixes++;
    return true;
}

int pdxe_qn_choose(pdxe_qn_index *x, const char *const *qns, const char *name, uint32_t self,
                   uint32_t parent, uint32_t module, uint32_t *base, uint32_t *keep,
                   const char **rest) {
    const char *qn = qns[self];
    *base = UINT32_MAX;
    *keep = 0;
    *rest = NULL;
    if (!qn) {
        return PDXE_QN_WHOLE;
    }
    size_t len = strlen(qn);
    size_t name_len = name ? strlen(name) : 0;
    int form = PDXE_QN_WHOLE;
    size_t held = len; /* the bytes the form leaves to hold */
    size_t own = 0;    /* where the bytes this definition adds begin */
    if (name && len > name_len && qn[len - name_len - 1] == '.' &&
        memcmp(qn + len - name_len, name, name_len) == 0) {
        uint32_t enclosing = qn_index_find_prefix(x, qn, len - name_len - 1);
        if (enclosing < self) {
            *base = enclosing;
            form = PDXE_QN_MEMBER;
            held = 0;
            own = len - name_len - 1;
        }
    }
    const uint32_t candidates[2] = {parent, module};
    for (int k = 0; k < 2 && form == PDXE_QN_WHOLE; k++) {
        uint32_t b = candidates[k];
        if (b >= self || !qns[b]) {
            continue;
        }
        const char *after = NULL;
        int related = qn_relation(qn, name, qns[b], &after);
        if (related == PDXE_QN_MEMBER || related == PDXE_QN_EXTENDED) {
            *base = b;
            form = related;
            held = related == PDXE_QN_EXTENDED ? strlen(after) : 0;
            own = related == PDXE_QN_EXTENDED ? (size_t)(after - qn) : len - name_len - 1;
            *rest = related == PDXE_QN_EXTENDED ? after : NULL;
        }
    }
    size_t n_bounds = 0;
    bool bounded = qn_bounds(x, qn, len, &n_bounds);
    /* The longest prefix an earlier definition added, when it leaves fewer bytes. */
    for (size_t i = n_bounds; bounded && form != PDXE_QN_MEMBER && i-- > 0;) {
        const pdxe_qn_prefix *p = qn_prefix_find(x, qn, x->bounds[i], x->hashes[i]);
        if (!p) {
            continue;
        }
        if (p->def < self && len - p->len < held) {
            const char *after = qn + p->len;
            *base = p->def;
            *rest = after;
            held = len - p->len;
            own = p->len;
            if (!p->whole) {
                form = PDXE_QN_PREFIX;
                *keep = p->len;
            } else if (after[0] == '.' && name && strcmp(after + 1, name) == 0) {
                form = PDXE_QN_MEMBER;
                *rest = NULL;
                held = 0;
            } else {
                form = PDXE_QN_EXTENDED;
            }
        }
        break;
    }
    /* This name's prefixes beyond its base, longest first, until one is recorded. */
    for (size_t i = n_bounds; bounded && i-- > 0 && x->bounds[i] > own;) {
        if (qn_prefix_find(x, qn, x->bounds[i], x->hashes[i])) {
            break;
        }
        pdxe_qn_prefix p = {x->hashes[i], qn, x->bounds[i], self, x->bounds[i] == len};
        if (!qn_prefix_add(x, &p)) {
            break;
        }
    }
    if (form == PDXE_QN_WHOLE) {
        *base = UINT32_MAX;
        *rest = NULL;
    }
    return form;
}

/* What encoding needs beyond the item at hand (issue 90). */
typedef struct {
    yyjson_mut_doc *doc;
    const PDXEFileResult *r;
    const char *source;
    size_t source_len;
    const char **qns;     /* every definition's qualified name, by index */
    pdxe_qn_index names;  /* qualified name -> the first definition with it */
    size_t index;         /* the item's index in its array */
} encoder;

/* The first definition whose qualified name is `qn`, or UINT32_MAX. */
static uint32_t definition_named(const encoder *e, const char *qn) {
    return pdxe_qn_index_find(&e->names, qn);
}

/* An F_TEXT value: the text's range when it is the source's own within its site. */
static yyjson_mut_val *text_value(encoder *e, const void *item, const field *f,
                                  const char *s) {
    bool raw = f->origin_offset == RAW_ORIGIN ||
               *AT_CONST(item, f->origin_offset, int) == PDXE_SOURCE_ORIGIN_RAW;
    uint32_t start = *AT_CONST(item, f->start_offset, uint32_t);
    uint32_t limit = *AT_CONST(item, f->end_offset, uint32_t);
    uint32_t end = 0;
    if (s && raw && pdxe_text_in_site(e->source, e->source_len, s, start, limit, &end)) {
        yyjson_mut_val *range = yyjson_mut_arr(e->doc);
        yyjson_mut_arr_append(range, yyjson_mut_uint(e->doc, start));
        yyjson_mut_arr_append(range, yyjson_mut_uint(e->doc, end));
        return range;
    }
    return string_value(e->doc, s);
}

/* A definition's own qualified name, by its relation to an earlier definition's
 * (pdxe_qn_choose), else whole: `[base]`, `[base, "rest"]`, `[base, "rest", keep]`. */
static yyjson_mut_val *qualified_name_value(encoder *e, const PDXEDefinition *d) {
    if (!d->qualified_name) {
        return yyjson_mut_null(e->doc);
    }
    uint32_t base = UINT32_MAX;
    uint32_t keep = 0;
    const char *rest = NULL;
    int form = pdxe_qn_choose(&e->names, e->qns, d->name, (uint32_t)e->index,
                              definition_named(e, d->parent_class),
                              definition_named(e, e->r->module_qn), &base, &keep, &rest);
    if (form == PDXE_QN_WHOLE) {
        return yyjson_mut_strcpy(e->doc, d->qualified_name);
    }
    yyjson_mut_val *v = yyjson_mut_arr(e->doc);
    yyjson_mut_arr_append(v, yyjson_mut_uint(e->doc, base));
    if (form == PDXE_QN_EXTENDED || form == PDXE_QN_PREFIX) {
        yyjson_mut_arr_append(v, yyjson_mut_strcpy(e->doc, rest));
    }
    if (form == PDXE_QN_PREFIX) {
        yyjson_mut_arr_append(v, yyjson_mut_uint(e->doc, keep));
    }
    return v;
}

static yyjson_mut_val *encode_fields(encoder *e, const void *item, const field *fields,
                                     size_t n);

static yyjson_mut_val *encode_field(encoder *e, const void *item, const field *f) {
    yyjson_mut_doc *doc = e->doc;
    switch (f->kind) {
    case F_STR:
        return string_value(doc, *AT_CONST(item, f->offset, const char *));
    case F_TEXT:
        return text_value(e, item, f, *AT_CONST(item, f->offset, const char *));
    case F_QNREF:
    case F_TEXTQN: {
        const char *s = *AT_CONST(item, f->offset, const char *);
        uint32_t def = definition_named(e, s);
        if (def != UINT32_MAX) {
            return yyjson_mut_uint(doc, def);
        }
        return f->kind == F_TEXTQN ? text_value(e, item, f, s) : string_value(doc, s);
    }
    case F_QNAME:
        return qualified_name_value(e, (const PDXEDefinition *)item);
    case F_STRV: {
        const char *const *v = *AT_CONST(item, f->offset, const char *const *);
        if (!v) {
            return yyjson_mut_null(doc);
        }
        yyjson_mut_val *arr = yyjson_mut_arr(doc);
        for (; *v; v++) {
            yyjson_mut_arr_append(arr, yyjson_mut_strcpy(doc, *v));
        }
        return arr;
    }
    case F_STRN: {
        const char *const *v = *AT_CONST(item, f->offset, const char *const *);
        int n = *AT_CONST(item, f->count_offset, int);
        if (!v) {
            return yyjson_mut_null(doc);
        }
        yyjson_mut_val *arr = yyjson_mut_arr(doc);
        for (int i = 0; i < n; i++) {
            yyjson_mut_arr_append(arr, string_value(doc, v[i]));
        }
        return arr;
    }
    case F_U32N: {
        const uint32_t *v = *AT_CONST(item, f->offset, const uint32_t *);
        int n = *AT_CONST(item, f->count_offset, int);
        if (!v) {
            return yyjson_mut_null(doc);
        }
        yyjson_mut_val *arr = yyjson_mut_arr(doc);
        for (int i = 0; i < n; i++) {
            yyjson_mut_arr_append(arr, yyjson_mut_uint(doc, v[i]));
        }
        return arr;
    }
    case F_ARGS: {
        const PDXECallArg *v = *AT_CONST(item, f->offset, const PDXECallArg *);
        int n = *AT_CONST(item, f->count_offset, int);
        if (!v) {
            return yyjson_mut_null(doc);
        }
        yyjson_mut_val *arr = yyjson_mut_arr(doc);
        for (int i = 0; i < n; i++) {
            yyjson_mut_arr_append(arr, encode_fields(e, &v[i], CALL_ARG_FIELDS,
                                                     N(CALL_ARG_FIELDS)));
        }
        return arr;
    }
    case F_ROUTES: {
        const PDXERouteFact *v = *AT_CONST(item, f->offset, const PDXERouteFact *);
        int n = *AT_CONST(item, f->count_offset, int);
        if (!v) {
            return yyjson_mut_null(doc);
        }
        yyjson_mut_val *arr = yyjson_mut_arr(doc);
        for (int i = 0; i < n; i++) {
            yyjson_mut_arr_append(arr, encode_fields(e, &v[i], ROUTE_FIELDS, N(ROUTE_FIELDS)));
        }
        return arr;
    }
    case F_INT:
    case F_ENUM:
        return yyjson_mut_sint(doc, *AT_CONST(item, f->offset, int));
    case F_U32:
        return yyjson_mut_uint(doc, *AT_CONST(item, f->offset, uint32_t));
    case F_BOOL:
        return yyjson_mut_bool(doc, *AT_CONST(item, f->offset, bool));
    case F_FLOAT:
        /* Stored as the float's exact bits: a decimal round trip could change it. */
        {
            float value = *AT_CONST(item, f->offset, float);
            uint32_t bits;
            memcpy(&bits, &value, sizeof(bits));
            return yyjson_mut_uint(doc, bits);
        }
    }
    return yyjson_mut_null(doc);
}

static yyjson_mut_val *encode_fields(encoder *e, const void *item, const field *fields,
                                     size_t n) {
    yyjson_mut_val *obj = yyjson_mut_obj(e->doc);
    for (size_t i = 0; i < n; i++) {
        yyjson_mut_obj_add(obj, yyjson_mut_str(e->doc, fields[i].key),
                           encode_field(e, item, &fields[i]));
    }
    return obj;
}

int pdxe_surface_encode(const PDXEFileResult *r, const char *rel_path, int abi_lang,
                        uint32_t lost, const char *source, size_t source_len, uint8_t **out,
                        size_t *out_len) {
    if (!r || !rel_path || !out || !out_len || (!source && source_len > 0)) {
        return PDXE_E_INVALID;
    }
    *out = NULL;
    *out_len = 0;
    encoder e = {0};
    e.r = r;
    e.source = source;
    e.source_len = source_len;
    /* Every definition's qualified name, to the first definition that has it. */
    uint32_t n_defs = r->defs.count > 0 ? (uint32_t)r->defs.count : 0;
    e.qns = (const char **)calloc(n_defs ? n_defs : 1, sizeof(*e.qns));
    if (!e.qns) {
        return PDXE_E_NOMEM;
    }
    for (uint32_t i = 0; i < n_defs; i++) {
        e.qns[i] = r->defs.items[i].qualified_name;
    }
    if (!pdxe_qn_index_init(&e.names, e.qns, n_defs)) {
        free(e.qns);
        return PDXE_E_NOMEM;
    }
    e.doc = yyjson_mut_doc_new(NULL);
    if (!e.doc) {
        pdxe_qn_index_free(&e.names);
        free(e.qns);
        return PDXE_E_NOMEM;
    }
    yyjson_mut_doc *doc = e.doc;
    yyjson_mut_val *root = yyjson_mut_obj(doc);
    yyjson_mut_doc_set_root(doc, root);
    yyjson_mut_obj_add_int(doc, root, "v", SURFACE_VERSION);
    yyjson_mut_obj_add_strcpy(doc, root, "path", rel_path);
    yyjson_mut_obj_add_int(doc, root, "lang", abi_lang);
    yyjson_mut_obj_add_uint(doc, root, "lost", lost);
    yyjson_mut_obj_add_uint(doc, root, "source_len", source_len);
    yyjson_mut_obj_add_val(doc, root, "file", encode_fields(&e, r, FILE_FIELDS, N(FILE_FIELDS)));
    for (size_t a = 0; a < N(ARRAYS); a++) {
        const any_array *arr = AT_CONST(r, ARRAYS[a].items_offset, any_array);
        yyjson_mut_val *list = yyjson_mut_arr(doc);
        for (int i = 0; i < arr->count; i++) {
            const void *item = (const char *)arr->items + (size_t)i * ARRAYS[a].item_size;
            e.index = (size_t)i;
            yyjson_mut_arr_append(list,
                                  encode_fields(&e, item, ARRAYS[a].fields, ARRAYS[a].n_fields));
        }
        yyjson_mut_obj_add_val(doc, root, ARRAYS[a].key, list);
    }
    size_t len = 0;
    char *json = yyjson_mut_write(doc, SURFACE_WRITE_FLAGS, &len);
    yyjson_mut_doc_free(doc);
    pdxe_qn_index_free(&e.names);
    free(e.qns);
    if (!json) {
        return PDXE_E_NOMEM;
    }
    *out = (uint8_t *)json;
    *out_len = len;
    return PDXE_OK;
}

/* --- decoding ---------------------------------------------------------------- */

/* A string decoding left unset: its source range, filled in by pdxe_surface_bind. */
typedef struct {
    const char **slot;
    uint32_t start;
    uint32_t end;
} pending_text;

struct pdxe_surface_texts {
    pending_text *items;
    size_t n;
    size_t cap;
    size_t source_len;
};

/* A string that is a definition's qualified name, set once every definition is. */
typedef struct {
    const char **slot;
    uint64_t def;
} pending_name;

/* What decoding needs beyond the item at hand (issue 90). */
typedef struct {
    PDXEArena *arena;
    PDXEFileResult *r;
    size_t index; /* the item's index in its array */
    pdxe_surface_texts *texts;
    pending_name *names;
    size_t n_names;
    size_t cap_names;
} decoder;

/* Appends one item to a growable array of `size`-byte items. False when it cannot. */
static bool grow_append(void **items, size_t *n, size_t *cap, size_t size, const void *item) {
    if (*n == *cap) {
        size_t next = *cap ? *cap * 2 : 64;
        void *grown = realloc(*items, next * size);
        if (!grown) {
            return false;
        }
        *items = grown;
        *cap = next;
    }
    memcpy((char *)*items + *n * size, item, size);
    (*n)++;
    return true;
}

/* A copy in the result's memory pool, or NULL for a JSON null. False on bad input. */
static bool decode_string(PDXEArena *arena, yyjson_val *v, const char **out) {
    if (yyjson_is_null(v)) {
        *out = NULL;
        return true;
    }
    if (!yyjson_is_str(v)) {
        return false;
    }
    *out = pdxe_arena_strndup(arena, yyjson_get_str(v), yyjson_get_len(v));
    return *out != NULL;
}

/* A source range [start, end] of a source of the surface's length, left for the bind. */
static bool decode_range(decoder *d, yyjson_val *v, const char **slot) {
    yyjson_val *a = yyjson_arr_get(v, 0);
    yyjson_val *b = yyjson_arr_get(v, 1);
    if (yyjson_arr_size(v) != 2 || !yyjson_is_uint(a) || !yyjson_is_uint(b)) {
        return false;
    }
    uint64_t start = yyjson_get_uint(a);
    uint64_t end = yyjson_get_uint(b);
    if (start > end || end > d->texts->source_len) {
        return false;
    }
    *slot = NULL;
    pending_text t = {slot, (uint32_t)start, (uint32_t)end};
    return grow_append((void **)&d->texts->items, &d->texts->n, &d->texts->cap, sizeof(t), &t);
}

/* A definition's index, standing for its qualified name, set once all are decoded. */
static bool decode_name_ref(decoder *d, yyjson_val *v, const char **slot) {
    *slot = NULL;
    pending_name n = {slot, yyjson_get_uint(v)};
    return grow_append((void **)&d->names, &d->n_names, &d->cap_names, sizeof(n), &n);
}

/* A definition's own qualified name, whole or by its relation to an earlier one's: of a
 * prefix of it, only one ending before a `.` (pdxe_qn_choose). */
static bool decode_qualified_name(decoder *d, yyjson_val *v, PDXEDefinition *def) {
    if (!yyjson_is_arr(v)) {
        return decode_string(d->arena, v, &def->qualified_name);
    }
    size_t n = yyjson_arr_size(v);
    yyjson_val *base_v = yyjson_arr_get(v, 0);
    if (n < 1 || n > 3 || !yyjson_is_uint(base_v) || yyjson_get_uint(base_v) >= d->index) {
        return false;
    }
    const char *base = d->r->defs.items[yyjson_get_uint(base_v)].qualified_name;
    const char *rest = n >= 2 ? yyjson_get_str(yyjson_arr_get(v, 1)) : def->name;
    size_t rest_len = n >= 2 ? yyjson_get_len(yyjson_arr_get(v, 1)) : rest ? strlen(rest) : 0;
    if (!base || !rest || (n >= 2 && !yyjson_is_str(yyjson_arr_get(v, 1)))) {
        return false;
    }
    size_t base_len = strlen(base);
    if (n == 3) {
        yyjson_val *keep_v = yyjson_arr_get(v, 2);
        if (!yyjson_is_uint(keep_v) || yyjson_get_uint(keep_v) >= base_len ||
            base[yyjson_get_uint(keep_v)] != '.' || (rest_len > 0 && rest[0] != '.')) {
            return false;
        }
        base_len = (size_t)yyjson_get_uint(keep_v);
    }
    size_t dot = n == 1 ? 1 : 0;
    char *qn = (char *)pdxe_arena_alloc(d->arena, base_len + dot + rest_len + 1);
    if (!qn) {
        return false;
    }
    memcpy(qn, base, base_len);
    if (dot) {
        qn[base_len] = '.';
    }
    memcpy(qn + base_len + dot, rest, rest_len);
    qn[base_len + dot + rest_len] = '\0';
    def->qualified_name = qn;
    return true;
}

static bool decode_fields(decoder *d, yyjson_val *obj, void *item, const field *fields, size_t n);

static bool decode_field(decoder *d, yyjson_val *v, void *item, const field *f) {
    PDXEArena *arena = d->arena;
    if (!v) {
        return false; /* every field is always written */
    }
    switch (f->kind) {
    case F_STR:
        return decode_string(arena, v, AT(item, f->offset, const char *));
    case F_TEXT:
        return yyjson_is_arr(v) ? decode_range(d, v, AT(item, f->offset, const char *))
                                : decode_string(arena, v, AT(item, f->offset, const char *));
    case F_QNREF:
        return yyjson_is_uint(v) ? decode_name_ref(d, v, AT(item, f->offset, const char *))
                                 : decode_string(arena, v, AT(item, f->offset, const char *));
    case F_TEXTQN:
        return yyjson_is_uint(v)  ? decode_name_ref(d, v, AT(item, f->offset, const char *))
               : yyjson_is_arr(v) ? decode_range(d, v, AT(item, f->offset, const char *))
                                  : decode_string(arena, v, AT(item, f->offset, const char *));
    case F_QNAME:
        return decode_qualified_name(d, v, (PDXEDefinition *)item);
    case F_STRV:
    case F_STRN: {
        const char ***slot = AT(item, f->offset, const char **);
        if (yyjson_is_null(v)) {
            *slot = NULL;
            return true;
        }
        if (!yyjson_is_arr(v)) {
            return false;
        }
        size_t n = yyjson_arr_size(v);
        const char **list = (const char **)pdxe_arena_calloc(arena, (n + 1) * sizeof(char *));
        if (!list) {
            return false;
        }
        size_t i, max;
        yyjson_val *e;
        yyjson_arr_foreach(v, i, max, e) {
            if (!decode_string(arena, e, &list[i]) || (f->kind == F_STRV && !list[i])) {
                return false;
            }
        }
        *slot = list;
        return true;
    }
    case F_U32N: {
        uint32_t **slot = AT(item, f->offset, uint32_t *);
        if (yyjson_is_null(v)) {
            *slot = NULL;
            return true;
        }
        if (!yyjson_is_arr(v)) {
            return false;
        }
        size_t n = yyjson_arr_size(v);
        uint32_t *list = (uint32_t *)pdxe_arena_calloc(arena, (n ? n : 1) * sizeof(uint32_t));
        if (!list) {
            return false;
        }
        size_t i, max;
        yyjson_val *e;
        yyjson_arr_foreach(v, i, max, e) {
            if (!yyjson_is_uint(e) || yyjson_get_uint(e) > UINT32_MAX) {
                return false;
            }
            list[i] = (uint32_t)yyjson_get_uint(e);
        }
        *slot = list;
        return true;
    }
    case F_ARGS: {
        PDXECallArg **slot = AT(item, f->offset, PDXECallArg *);
        if (yyjson_is_null(v)) {
            *slot = NULL;
            return true;
        }
        if (!yyjson_is_arr(v)) {
            return false;
        }
        size_t n = yyjson_arr_size(v);
        PDXECallArg *list = (PDXECallArg *)pdxe_arena_calloc(arena, (n ? n : 1) * sizeof(*list));
        if (!list) {
            return false;
        }
        size_t i, max;
        yyjson_val *e;
        yyjson_arr_foreach(v, i, max, e) {
            if (!decode_fields(d, e, &list[i], CALL_ARG_FIELDS, N(CALL_ARG_FIELDS))) {
                return false;
            }
            /* An expression the engine holds as a range must be one of this source;
             * one it copied has no range (issue 90). */
            if (list[i].expr ? list[i].expr_start != 0 || list[i].expr_end != 0
                             : list[i].expr_start > list[i].expr_end ||
                                   list[i].expr_end > d->texts->source_len) {
                return false;
            }
        }
        *slot = list;
        return true;
    }
    case F_ROUTES: {
        PDXERouteFact **slot = AT(item, f->offset, PDXERouteFact *);
        if (yyjson_is_null(v)) {
            *slot = NULL;
            return true;
        }
        if (!yyjson_is_arr(v)) {
            return false;
        }
        size_t n = yyjson_arr_size(v);
        PDXERouteFact *list =
            (PDXERouteFact *)pdxe_arena_calloc(arena, (n ? n : 1) * sizeof(*list));
        if (!list) {
            return false;
        }
        size_t i, max;
        yyjson_val *e;
        yyjson_arr_foreach(v, i, max, e) {
            if (!decode_fields(d, e, &list[i], ROUTE_FIELDS, N(ROUTE_FIELDS))) {
                return false;
            }
        }
        *slot = list;
        return true;
    }
    case F_INT:
    case F_ENUM:
        if (!yyjson_is_int(v)) {
            return false;
        }
        *AT(item, f->offset, int) = (int)yyjson_get_int(v);
        return true;
    case F_U32:
        if (!yyjson_is_uint(v) || yyjson_get_uint(v) > UINT32_MAX) {
            return false;
        }
        *AT(item, f->offset, uint32_t) = (uint32_t)yyjson_get_uint(v);
        return true;
    case F_BOOL:
        if (!yyjson_is_bool(v)) {
            return false;
        }
        *AT(item, f->offset, bool) = yyjson_get_bool(v);
        return true;
    case F_FLOAT: {
        if (!yyjson_is_uint(v) || yyjson_get_uint(v) > UINT32_MAX) {
            return false;
        }
        uint32_t bits = (uint32_t)yyjson_get_uint(v);
        float value;
        memcpy(&value, &bits, sizeof(value));
        *AT(item, f->offset, float) = value;
        return true;
    }
    }
    return false;
}

static bool decode_fields(decoder *d, yyjson_val *obj, void *item, const field *fields, size_t n) {
    if (!yyjson_is_obj(obj) || yyjson_obj_size(obj) != n) {
        return false;
    }
    for (size_t i = 0; i < n; i++) {
        if (!decode_field(d, yyjson_obj_get(obj, fields[i].key), item, &fields[i])) {
            return false;
        }
    }
    /* A counted array and its count must agree, or a later reader overruns it. */
    for (size_t i = 0; i < n; i++) {
        if (fields[i].kind == F_STRN || fields[i].kind == F_U32N || fields[i].kind == F_ARGS ||
            fields[i].kind == F_ROUTES) {
            yyjson_val *v = yyjson_obj_get(obj, fields[i].key);
            int count = *AT(item, fields[i].count_offset, int);
            if (yyjson_is_arr(v) ? (size_t)count != yyjson_arr_size(v) : count != 0) {
                return false;
            }
        }
    }
    return true;
}

void pdxe_surface_texts_free(pdxe_surface_texts *texts) {
    if (texts) {
        free(texts->items);
        free(texts);
    }
}

int pdxe_surface_decode(const uint8_t *bytes, size_t len, PDXEFileResult **out, char **rel_path,
                        int *abi_lang, uint32_t *lost, pdxe_surface_texts **texts) {
    if (!bytes || !out || !rel_path || !abi_lang || !lost || !texts) {
        return PDXE_E_INVALID;
    }
    *out = NULL;
    *rel_path = NULL;
    *texts = NULL;
    yyjson_doc *doc = yyjson_read((const char *)bytes, len, SURFACE_READ_FLAGS);
    if (!doc) {
        return PDXE_E_INVALID;
    }
    yyjson_val *root = yyjson_doc_get_root(doc);
    yyjson_val *version = yyjson_obj_get(root, "v");
    yyjson_val *path = yyjson_obj_get(root, "path");
    yyjson_val *lang = yyjson_obj_get(root, "lang");
    yyjson_val *lost_count = yyjson_obj_get(root, "lost");
    yyjson_val *source_len = yyjson_obj_get(root, "source_len");
    if (!yyjson_is_obj(root) || !yyjson_is_int(version) ||
        yyjson_get_int(version) != SURFACE_VERSION || !yyjson_is_str(path) ||
        !yyjson_is_int(lang) || !yyjson_is_uint(lost_count) ||
        yyjson_get_uint(lost_count) > UINT32_MAX || !yyjson_is_uint(source_len) ||
        yyjson_get_uint(source_len) > UINT32_MAX) {
        yyjson_doc_free(doc);
        return PDXE_E_INVALID;
    }
    int lang_id = (int)yyjson_get_int(lang);
    uint32_t lost_value = (uint32_t)yyjson_get_uint(lost_count); /* checked to fit above */
    PDXEFileResult *r = pdxe_result_alloc();
    char *path_copy = strdup(yyjson_get_str(path));
    decoder d = {0};
    d.texts = (pdxe_surface_texts *)calloc(1, sizeof(*d.texts));
    if (!r || !path_copy || !d.texts) {
        free(r);
        free(path_copy);
        free(d.texts);
        yyjson_doc_free(doc);
        return PDXE_E_NOMEM;
    }
    d.texts->source_len = (size_t)yyjson_get_uint(source_len);
    pdxe_arena_init(&r->arena);
    d.arena = &r->arena;
    d.r = r;
    bool ok = decode_fields(&d, yyjson_obj_get(root, "file"), r, FILE_FIELDS, N(FILE_FIELDS));
    for (size_t a = 0; ok && a < N(ARRAYS); a++) {
        yyjson_val *list = yyjson_obj_get(root, ARRAYS[a].key);
        if (!yyjson_is_arr(list)) {
            ok = false;
            break;
        }
        size_t n = yyjson_arr_size(list);
        any_array *arr = AT(r, ARRAYS[a].items_offset, any_array);
        arr->items = n ? pdxe_arena_calloc(&r->arena, n * ARRAYS[a].item_size) : NULL;
        arr->count = 0;
        arr->cap = (int)n;
        if (n && !arr->items) {
            ok = false;
            break;
        }
        size_t i, max;
        yyjson_val *e;
        yyjson_arr_foreach(list, i, max, e) {
            void *item = (char *)arr->items + i * ARRAYS[a].item_size;
            d.index = i;
            if (!decode_fields(&d, e, item, ARRAYS[a].fields, ARRAYS[a].n_fields)) {
                ok = false;
                break;
            }
            /* A parameter list the engine holds as a range must be one of this source;
             * one it copied has no range (issue 90). */
            const PDXEDefinition *def = (const PDXEDefinition *)item;
            if (ARRAYS[a].fields == DEFINITION_FIELDS &&
                (def->signature ? def->signature_start != 0 || def->signature_end != 0
                                : def->signature_start > def->signature_end ||
                                      def->signature_end > d.texts->source_len)) {
                ok = false;
                break;
            }
            arr->count++;
        }
    }
    /* Every name a definition stands for is that definition's, now that all are in. */
    for (size_t k = 0; ok && k < d.n_names; k++) {
        if (d.names[k].def >= (uint64_t)r->defs.count ||
            !r->defs.items[d.names[k].def].qualified_name) {
            ok = false;
            break;
        }
        *d.names[k].slot = r->defs.items[d.names[k].def].qualified_name;
    }
    free(d.names);
    /* The top level holds exactly the version, the path, the language, what the
     * extraction lost, the source's length, the file values and the arrays: anything
     * else is a surface this version did not write. */
    if (ok && yyjson_obj_size(root) != 6 + N(ARRAYS)) {
        ok = false;
    }
    yyjson_doc_free(doc);
    if (!ok) {
        pdxe_free_result(r);
        free(path_copy);
        pdxe_surface_texts_free(d.texts);
        return PDXE_E_INVALID;
    }
    r->cached_tree = NULL;
    *out = r;
    *rel_path = path_copy;
    *abi_lang = lang_id;
    *lost = lost_value;
    *texts = d.texts;
    return PDXE_OK;
}

int pdxe_surface_bind(PDXEFileResult *r, pdxe_surface_texts *texts, const char *source,
                      size_t source_len) {
    if (!r || !texts || (!source && source_len > 0) || source_len != texts->source_len) {
        pdxe_surface_texts_free(texts);
        return PDXE_E_INVALID;
    }
    /* A range holds no NUL: text the engine held never did, so one is not this source's. */
    for (size_t k = 0; k < texts->n; k++) {
        const pending_text *t = &texts->items[k];
        if (memchr(source + t->start, 0, t->end - t->start)) {
            pdxe_surface_texts_free(texts);
            return PDXE_E_INVALID;
        }
    }
    for (size_t k = 0; k < texts->n; k++) {
        const pending_text *t = &texts->items[k];
        *t->slot = pdxe_arena_strndup(&r->arena, source + t->start, t->end - t->start);
        if (!*t->slot) {
            pdxe_surface_texts_free(texts);
            return PDXE_E_NOMEM;
        }
    }
    pdxe_surface_texts_free(texts);
    return PDXE_OK;
}
