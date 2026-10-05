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
 */

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "pdxe.h"
#include "pdxe_core.h"
#include "shim_internal.h"
#include "yyjson/yyjson.h"

/* 2: the surface carries what the extraction lost (the "lost" count). 3: calls and
 * usages carry their node-type path (issue 46). 4: definitions carry their route
 * facts (issue 54). */
enum { SURFACE_VERSION = 4 };

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
} field_kind;

typedef struct {
    const char *key;
    field_kind kind;
    size_t offset;
    size_t count_offset;
} field;

/* The engine's enumerations are stored as int; the codec relies on that. */
_Static_assert(sizeof(PDXESourceOrigin) == sizeof(int), "enumerations must be int-sized");
_Static_assert(sizeof(PDXEUsageKind) == sizeof(int), "enumerations must be int-sized");
_Static_assert(sizeof(PDXEResolvedKind) == sizeof(int), "enumerations must be int-sized");
_Static_assert(sizeof(PDXEStringRefKind) == sizeof(int), "enumerations must be int-sized");
_Static_assert(sizeof(PDXEChannelDirection) == sizeof(int), "enumerations must be int-sized");
_Static_assert(sizeof(PDXELanguage) == sizeof(int), "enumerations must be int-sized");

#define FIELD(type, kind, name) {#name, kind, offsetof(type, name), 0}
#define COUNTED(type, kind, name, count) {#name, kind, offsetof(type, name), offsetof(type, count)}

static const field DEFINITION_FIELDS[] = {
    FIELD(PDXEDefinition, F_STR, name),
    FIELD(PDXEDefinition, F_STR, qualified_name),
    FIELD(PDXEDefinition, F_STR, label),
    FIELD(PDXEDefinition, F_STR, file_path),
    FIELD(PDXEDefinition, F_U32, start_line),
    FIELD(PDXEDefinition, F_U32, end_line),
    FIELD(PDXEDefinition, F_STR, signature),
    FIELD(PDXEDefinition, F_STR, return_type),
    FIELD(PDXEDefinition, F_STR, receiver),
    FIELD(PDXEDefinition, F_STR, docstring),
    FIELD(PDXEDefinition, F_STR, parent_class),
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
    FIELD(PDXERouteFact, F_STR, source_text),
    FIELD(PDXERouteFact, F_STRV, ast_path),
    FIELD(PDXERouteFact, F_U32, start_byte),
    FIELD(PDXERouteFact, F_U32, end_byte),
};

static const field CALL_FIELDS[] = {
    FIELD(PDXECall, F_STR, callee_name),
    FIELD(PDXECall, F_STR, enclosing_func_qn),
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
};

static const field IMPORT_FIELDS[] = {
    FIELD(PDXEImport, F_STR, local_name),
    FIELD(PDXEImport, F_STR, module_path),
};

static const field USAGE_FIELDS[] = {
    FIELD(PDXEUsage, F_STR, ref_name),
    FIELD(PDXEUsage, F_STR, enclosing_func_qn),
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
    FIELD(PDXEThrow, F_STR, enclosing_func_qn),
};

static const field READ_WRITE_FIELDS[] = {
    FIELD(PDXEReadWrite, F_STR, var_name),
    FIELD(PDXEReadWrite, F_STR, enclosing_func_qn),
    FIELD(PDXEReadWrite, F_BOOL, is_write),
    FIELD(PDXEReadWrite, F_BOOL, is_member_access),
};

static const field TYPE_REF_FIELDS[] = {
    FIELD(PDXETypeRef, F_STR, type_name),
    FIELD(PDXETypeRef, F_STR, enclosing_func_qn),
};

static const field ENV_ACCESS_FIELDS[] = {
    FIELD(PDXEEnvAccess, F_STR, env_key),
    FIELD(PDXEEnvAccess, F_STR, enclosing_func_qn),
};

static const field TYPE_ASSIGN_FIELDS[] = {
    FIELD(PDXETypeAssign, F_STR, var_name),
    FIELD(PDXETypeAssign, F_STR, type_name),
    FIELD(PDXETypeAssign, F_STR, enclosing_func_qn),
};

static const field IMPL_TRAIT_FIELDS[] = {
    FIELD(PDXEImplTrait, F_STR, trait_name),
    FIELD(PDXEImplTrait, F_STR, struct_name),
    FIELD(PDXEImplTrait, F_STR, struct_qn),
};

static const field RESOLVED_CALL_FIELDS[] = {
    FIELD(PDXEResolvedCall, F_STR, caller_qn),
    FIELD(PDXEResolvedCall, F_STR, callee_qn),
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
    FIELD(PDXEStringRef, F_STR, enclosing_func_qn),
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
    FIELD(PDXEChannel, F_STR, enclosing_func_qn),
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

static yyjson_mut_val *encode_fields(yyjson_mut_doc *doc, const void *item, const field *fields,
                                     size_t n);

static yyjson_mut_val *encode_field(yyjson_mut_doc *doc, const void *item, const field *f) {
    switch (f->kind) {
    case F_STR:
        return string_value(doc, *AT_CONST(item, f->offset, const char *));
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
            yyjson_mut_arr_append(arr, encode_fields(doc, &v[i], CALL_ARG_FIELDS,
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
            yyjson_mut_arr_append(arr, encode_fields(doc, &v[i], ROUTE_FIELDS, N(ROUTE_FIELDS)));
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

static yyjson_mut_val *encode_fields(yyjson_mut_doc *doc, const void *item, const field *fields,
                                     size_t n) {
    yyjson_mut_val *obj = yyjson_mut_obj(doc);
    for (size_t i = 0; i < n; i++) {
        yyjson_mut_obj_add(obj, yyjson_mut_str(doc, fields[i].key),
                           encode_field(doc, item, &fields[i]));
    }
    return obj;
}

int pdxe_surface_encode(const PDXEFileResult *r, const char *rel_path, int abi_lang,
                        uint32_t lost, uint8_t **out, size_t *out_len) {
    if (!r || !rel_path || !out || !out_len) {
        return PDXE_E_INVALID;
    }
    *out = NULL;
    *out_len = 0;
    yyjson_mut_doc *doc = yyjson_mut_doc_new(NULL);
    if (!doc) {
        return PDXE_E_NOMEM;
    }
    yyjson_mut_val *root = yyjson_mut_obj(doc);
    yyjson_mut_doc_set_root(doc, root);
    yyjson_mut_obj_add_int(doc, root, "v", SURFACE_VERSION);
    yyjson_mut_obj_add_strcpy(doc, root, "path", rel_path);
    yyjson_mut_obj_add_int(doc, root, "lang", abi_lang);
    yyjson_mut_obj_add_uint(doc, root, "lost", lost);
    yyjson_mut_obj_add_val(doc, root, "file", encode_fields(doc, r, FILE_FIELDS, N(FILE_FIELDS)));
    for (size_t a = 0; a < N(ARRAYS); a++) {
        const any_array *arr = AT_CONST(r, ARRAYS[a].items_offset, any_array);
        yyjson_mut_val *list = yyjson_mut_arr(doc);
        for (int i = 0; i < arr->count; i++) {
            const void *item = (const char *)arr->items + (size_t)i * ARRAYS[a].item_size;
            yyjson_mut_arr_append(list,
                                  encode_fields(doc, item, ARRAYS[a].fields, ARRAYS[a].n_fields));
        }
        yyjson_mut_obj_add_val(doc, root, ARRAYS[a].key, list);
    }
    size_t len = 0;
    char *json = yyjson_mut_write(doc, SURFACE_WRITE_FLAGS, &len);
    yyjson_mut_doc_free(doc);
    if (!json) {
        return PDXE_E_NOMEM;
    }
    *out = (uint8_t *)json;
    *out_len = len;
    return PDXE_OK;
}

/* --- decoding ---------------------------------------------------------------- */

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

static bool decode_fields(PDXEArena *arena, yyjson_val *obj, void *item, const field *fields,
                          size_t n);

static bool decode_field(PDXEArena *arena, yyjson_val *v, void *item, const field *f) {
    if (!v) {
        return false; /* every field is always written */
    }
    switch (f->kind) {
    case F_STR:
        return decode_string(arena, v, AT(item, f->offset, const char *));
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
            if (!decode_fields(arena, e, &list[i], CALL_ARG_FIELDS, N(CALL_ARG_FIELDS))) {
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
            if (!decode_fields(arena, e, &list[i], ROUTE_FIELDS, N(ROUTE_FIELDS))) {
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

static bool decode_fields(PDXEArena *arena, yyjson_val *obj, void *item, const field *fields,
                          size_t n) {
    if (!yyjson_is_obj(obj) || yyjson_obj_size(obj) != n) {
        return false;
    }
    for (size_t i = 0; i < n; i++) {
        if (!decode_field(arena, yyjson_obj_get(obj, fields[i].key), item, &fields[i])) {
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

int pdxe_surface_decode(const uint8_t *bytes, size_t len, PDXEFileResult **out, char **rel_path,
                        int *abi_lang, uint32_t *lost) {
    if (!bytes || !out || !rel_path || !abi_lang || !lost) {
        return PDXE_E_INVALID;
    }
    *out = NULL;
    *rel_path = NULL;
    yyjson_doc *doc = yyjson_read((const char *)bytes, len, SURFACE_READ_FLAGS);
    if (!doc) {
        return PDXE_E_INVALID;
    }
    yyjson_val *root = yyjson_doc_get_root(doc);
    yyjson_val *version = yyjson_obj_get(root, "v");
    yyjson_val *path = yyjson_obj_get(root, "path");
    yyjson_val *lang = yyjson_obj_get(root, "lang");
    yyjson_val *lost_count = yyjson_obj_get(root, "lost");
    if (!yyjson_is_obj(root) || !yyjson_is_int(version) ||
        yyjson_get_int(version) != SURFACE_VERSION || !yyjson_is_str(path) ||
        !yyjson_is_int(lang) || !yyjson_is_uint(lost_count) ||
        yyjson_get_uint(lost_count) > UINT32_MAX) {
        yyjson_doc_free(doc);
        return PDXE_E_INVALID;
    }
    int lang_id = (int)yyjson_get_int(lang);
    uint32_t lost_value = (uint32_t)yyjson_get_uint(lost_count); /* checked to fit above */
    PDXEFileResult *r = pdxe_result_alloc();
    char *path_copy = strdup(yyjson_get_str(path));
    if (!r || !path_copy) {
        free(r);
        free(path_copy);
        yyjson_doc_free(doc);
        return PDXE_E_NOMEM;
    }
    pdxe_arena_init(&r->arena);
    bool ok = decode_fields(&r->arena, yyjson_obj_get(root, "file"), r, FILE_FIELDS,
                            N(FILE_FIELDS));
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
            if (!decode_fields(&r->arena, e, item, ARRAYS[a].fields, ARRAYS[a].n_fields)) {
                ok = false;
                break;
            }
            arr->count++;
        }
    }
    /* The top level holds exactly the version, the path, the language, what the
     * extraction lost, the file values and the arrays: anything else is a surface this
     * version did not write. */
    if (ok && yyjson_obj_size(root) != 5 + N(ARRAYS)) {
        ok = false;
    }
    yyjson_doc_free(doc);
    if (!ok) {
        pdxe_free_result(r);
        free(path_copy);
        return PDXE_E_INVALID;
    }
    r->cached_tree = NULL;
    *out = r;
    *rel_path = path_copy;
    *abi_lang = lang_id;
    *lost = lost_value;
    return PDXE_OK;
}
