#ifndef PDXE_LSP_TYPE_REP_H
#define PDXE_LSP_TYPE_REP_H

#include "../arena.h"
#include <stdbool.h>
#include <stdint.h>

// PDXETypeKind enumerates all type representations.
typedef enum {
    PDXE_TYPE_UNKNOWN = 0,
    PDXE_TYPE_NAMED,       // named type: "Database", "http.Request"
    PDXE_TYPE_POINTER,     // *T
    PDXE_TYPE_SLICE,       // []T
    PDXE_TYPE_MAP,         // map[K]V
    PDXE_TYPE_CHANNEL,     // chan T
    PDXE_TYPE_FUNC,        // func(params) returns
    PDXE_TYPE_INTERFACE,   // interface{...}
    PDXE_TYPE_STRUCT,      // struct{...}
    PDXE_TYPE_BUILTIN,     // int, string, bool, error, etc.
    PDXE_TYPE_TUPLE,       // multi-return (T1, T2) / TS tuple [T,U]
    PDXE_TYPE_TYPE_PARAM,  // generic type parameter: T, K, V
    PDXE_TYPE_REFERENCE,   // T& (C++ lvalue reference)
    PDXE_TYPE_RVALUE_REF,  // T&& (C++ rvalue reference)
    PDXE_TYPE_TEMPLATE,    // Parameterized type: vector<T> — stores template name + args
    PDXE_TYPE_ALIAS,       // Type alias: using/typedef — stores alias name + underlying type
    PDXE_TYPE_UNION,       // Python: A | B; TS: A | B | C — sorted-canonical list (shared)
    PDXE_TYPE_LITERAL,     // Python: Literal["foo", 3] — wraps a base type + literal value text
    PDXE_TYPE_PROTOCOL,    // Python: typing.Protocol — like INTERFACE but matched structurally
    PDXE_TYPE_MODULE,      // Python: import os; os is a module-typed binding
    PDXE_TYPE_CALLABLE,    // Python: Callable[[A, B], R] — untyped-named callable variant of FUNC

    // --- TS-specific kinds (added in TS LSP integration) ---
    PDXE_TYPE_INTERSECTION,  // TS: A & B — intersection type
    PDXE_TYPE_TS_LITERAL,    // TS: "foo" / 42 / true literal types (tag+value layout, distinct
                            // from Python's PDXE_TYPE_LITERAL which uses base+literal_text)
    PDXE_TYPE_INDEXED,       // TS: T[K] — indexed access type
    PDXE_TYPE_KEYOF,         // TS: keyof T
    PDXE_TYPE_TYPEOF_QUERY,  // TS: typeof x in type position
    PDXE_TYPE_CONDITIONAL,   // TS: T extends U ? X : Y
    PDXE_TYPE_OBJECT_LIT,    // TS: { a: T1; b: T2 } anonymous object type
    PDXE_TYPE_INFER,         // TS: `infer X` placeholder inside conditional
    PDXE_TYPE_MAPPED,        // TS: {[K in keyof T]: ...} — v1 stub, members may be NULL
} PDXETypeKind;

// Forward declaration
typedef struct PDXEType PDXEType;

// Language-specific adapter used to parse one ordered signature type spelling.
typedef const PDXEType *(*PDXETypeTextParser)(PDXEArena *arena, const char *text, void *parser_ctx);

// PDXETypeParam represents a generic type parameter with optional constraint.
typedef struct {
    const char* name;        // "T", "K", "V"
    const PDXEType* constraint; // interface constraint, or NULL for "any"
} PDXETypeParam;

// PDXEType is a tagged union representing Go types.
struct PDXEType {
    PDXETypeKind kind;
    union {
        struct { const char* qualified_name; } named;      // NAMED
        struct { const PDXEType* elem; } pointer;            // POINTER
        struct { const PDXEType* elem; } slice;              // SLICE
        struct { const PDXEType* key; const PDXEType* value; } map;  // MAP
        struct { const PDXEType* elem; int direction; } channel;    // CHANNEL (0=bidi, 1=send, 2=recv)
        struct {
            const char** param_names;  // NULL-terminated
            const PDXEType** param_types; // NULL-terminated
            const PDXEType** return_types; // NULL-terminated
        } func;                                             // FUNC
        struct {
            const char** method_names;  // NULL-terminated
            const PDXEType** method_sigs; // NULL-terminated (each is FUNC)
        } interface_type;                                   // INTERFACE
        struct {
            const char** field_names;   // NULL-terminated
            const PDXEType** field_types; // NULL-terminated
        } struct_type;                                      // STRUCT
        struct { const char* name; } builtin;               // BUILTIN
        struct {
            const PDXEType** elems;      // NULL-terminated
            int count;
        } tuple;                                            // TUPLE
        struct { const char* name; } type_param;            // TYPE_PARAM
        struct { const PDXEType* elem; } reference;            // REFERENCE / RVALUE_REF
        struct {
            const char* template_name;      // "std::vector", "std::map"
            const PDXEType** template_args;  // NULL-terminated
            int arg_count;
        } template_type;                                      // TEMPLATE
        struct {
            const char* alias_qn;          // "proj.ns.MyAlias"
            const PDXEType* underlying;     // the actual type it aliases
        } alias;                                              // ALIAS
        struct {
            const PDXEType** members;       // NULL-terminated, deduplicated, sorted by kind/qn
            int count;
        } union_type;                                         // UNION / INTERSECTION (shared)
        struct {
            const PDXEType* base;           // base type (e.g. BUILTIN("int"), BUILTIN("str"))
            const char* literal_text;      // canonical text: "3", "\"foo\"", "True"
        } literal;                                            // LITERAL (Python)
        struct {
            const char* qualified_name;    // e.g. "typing.Iterable"
            const char** method_names;     // NULL-terminated method names — structural matching
            const PDXEType** method_sigs;   // NULL-terminated signatures (each is FUNC/CALLABLE)
        } protocol;                                           // PROTOCOL
        struct {
            const char* module_qn;         // module qualified name (matches PDXEImport.module_path)
        } module;                                             // MODULE
        struct {
            const PDXEType** param_types;   // NULL-terminated; NULL element means "Any" / unknown
            const PDXEType* return_type;    // single return; for tuples wrap in PDXE_TYPE_TUPLE
            int param_count;               // -1 = elliptic / Callable[..., R]
        } callable;                                           // CALLABLE

        // --- TS-specific data ---
        struct {
            // Tag distinguishes string / number / boolean / bigint / null / undefined literals.
            // For boolean literals, value points to "true" or "false".
            const char* tag;               // "string" | "number" | "boolean" | "bigint" | "null" | "undefined"
            const char* value;             // textual representation; arena-owned
        } literal_ts;                                         // TS_LITERAL
        struct {
            const PDXEType* object;         // T in T[K]
            const PDXEType* index;          // K in T[K]
        } indexed;                                            // INDEXED
        struct { const PDXEType* operand; } keyof;             // KEYOF
        struct { const char* expr; } typeof_query;            // TYPEOF_QUERY (referenced expression text)
        struct {
            const PDXEType* check;          // T
            const PDXEType* extends;        // U
            const PDXEType* true_branch;    // X
            const PDXEType* false_branch;   // Y
        } conditional;                                        // CONDITIONAL
        struct {
            const char** prop_names;       // NULL-terminated
            const PDXEType** prop_types;    // NULL-terminated, parallel to prop_names
            const PDXEType* call_signature; // FUNC type or NULL
            const PDXEType* index_value;    // type produced by string/number index, or NULL
        } object_lit;                                         // OBJECT_LIT
        struct { const char* name; } infer;                   // INFER (e.g., `infer R`)
        struct {
            const char* key_name;          // "K" in {[K in keyof T]: V}
            const PDXEType* key_constraint; // `keyof T`
            const PDXEType* value;          // V (may reference key_name as TYPE_PARAM)
        } mapped;                                             // MAPPED (v1 stub-friendly)
    } data;
};

// Constructors (arena-allocated)
const PDXEType* pdxe_type_unknown(void);
const PDXEType* pdxe_type_named(PDXEArena* a, const char* qualified_name);
const PDXEType* pdxe_type_pointer(PDXEArena* a, const PDXEType* elem);
const PDXEType* pdxe_type_slice(PDXEArena* a, const PDXEType* elem);
const PDXEType* pdxe_type_map(PDXEArena* a, const PDXEType* key, const PDXEType* value);
const PDXEType* pdxe_type_channel(PDXEArena* a, const PDXEType* elem, int direction);
const PDXEType* pdxe_type_func(PDXEArena* a, const char** param_names, const PDXEType** param_types, const PDXEType** return_types);
// Materialize exactly count positional parameter slots. NULL, empty, exact "?",
// and parser failures become UNKNOWN; the returned vector is NULL-terminated.
const PDXEType **pdxe_type_materialize_signature_params(PDXEArena *a, const char *const *type_texts,
                                                      int count, PDXETypeTextParser parser,
                                                      void *parser_ctx);
// Rebuild a FUNC with new returns while preserving its parameter names/types.
const PDXEType *pdxe_type_func_replace_returns(PDXEArena *a, const PDXEType *old_signature,
                                             const PDXEType *const *new_return_types);
const PDXEType* pdxe_type_builtin(PDXEArena* a, const char* name);
const PDXEType* pdxe_type_tuple(PDXEArena* a, const PDXEType** elems, int count);
const PDXEType* pdxe_type_type_param(PDXEArena* a, const char* name);
const PDXEType* pdxe_type_reference(PDXEArena* a, const PDXEType* elem);
const PDXEType* pdxe_type_rvalue_ref(PDXEArena* a, const PDXEType* elem);
const PDXEType* pdxe_type_template(PDXEArena* a, const char* name, const PDXEType** args, int arg_count);
const PDXEType* pdxe_type_alias(PDXEArena* a, const char* alias_qn, const PDXEType* underlying);

// Python-flavored constructors. UNION normalizes input: nested unions are
// flattened, duplicates removed, single-member unions collapse to that
// member, and the empty union is UNKNOWN. Members must be arena-allocated.
// Shared with TS LSP — both call this same constructor for `A | B`.
const PDXEType* pdxe_type_union(PDXEArena* a, const PDXEType** members, int count);
const PDXEType* pdxe_type_optional(PDXEArena* a, const PDXEType* t);  // Optional[T] == Union[T, None]
const PDXEType* pdxe_type_literal(PDXEArena* a, const PDXEType* base, const char* literal_text);
const PDXEType* pdxe_type_protocol(PDXEArena* a, const char* qualified_name,
    const char** method_names, const PDXEType** method_sigs);
const PDXEType* pdxe_type_module(PDXEArena* a, const char* module_qn);
const PDXEType* pdxe_type_callable(PDXEArena* a, const PDXEType** param_types, int param_count,
    const PDXEType* return_type);

// --- TS-specific constructors ---
const PDXEType* pdxe_type_intersection(PDXEArena* a, const PDXEType** members, int count);
// tag is one of "string"|"number"|"boolean"|"bigint"|"null"|"undefined".
// Distinct from pdxe_type_literal (Python) which uses base+literal_text.
const PDXEType* pdxe_type_ts_literal(PDXEArena* a, const char* tag, const char* value);
const PDXEType* pdxe_type_indexed(PDXEArena* a, const PDXEType* object, const PDXEType* index);
const PDXEType* pdxe_type_keyof(PDXEArena* a, const PDXEType* operand);
const PDXEType* pdxe_type_typeof_query(PDXEArena* a, const char* expr);
const PDXEType* pdxe_type_conditional(PDXEArena* a,
    const PDXEType* check, const PDXEType* extends,
    const PDXEType* true_branch, const PDXEType* false_branch);
// prop_names and prop_types are NULL-terminated parallel arrays; either may be NULL for empty.
const PDXEType* pdxe_type_object_lit(PDXEArena* a,
    const char** prop_names, const PDXEType** prop_types,
    const PDXEType* call_signature, const PDXEType* index_value);
const PDXEType* pdxe_type_infer(PDXEArena* a, const char* name);
const PDXEType* pdxe_type_mapped(PDXEArena* a,
    const char* key_name, const PDXEType* key_constraint, const PDXEType* value);

// Operations
const PDXEType* pdxe_type_deref(const PDXEType* t);         // remove one pointer level
const PDXEType* pdxe_type_elem(const PDXEType* t);           // get element type (slice/chan/pointer)
bool pdxe_type_is_unknown(const PDXEType* t);
bool pdxe_type_is_interface(const PDXEType* t);
bool pdxe_type_is_pointer(const PDXEType* t);
bool pdxe_type_is_reference(const PDXEType* t);
bool pdxe_type_is_union(const PDXEType* t);
bool pdxe_type_is_protocol(const PDXEType* t);
bool pdxe_type_is_module(const PDXEType* t);

// Structural equality on type representation (used by union dedup and
// protocol-method-set matching). Two types are equal if their kinds match
// and their structural members match recursively.
bool pdxe_type_equal(const PDXEType* a, const PDXEType* b);

// Test whether `candidate` satisfies the structural protocol `proto`.
// Walks proto.method_names against candidate's method set (NAMED → registry
// lookup is the caller's job; this helper only matches existing method
// signatures stored on a PROTOCOL).
bool pdxe_type_protocol_satisfied_by(const PDXEType* proto, const PDXEType* candidate);

// Follow alias chain with cycle detection (max 16 levels).
const PDXEType* pdxe_type_resolve_alias(const PDXEType* t);

// Generic type substitution: replace type params in t with concrete types.
// type_params: NULL-terminated array of param names
// type_args: corresponding concrete types
const PDXEType* pdxe_type_substitute(PDXEArena* a, const PDXEType* t,
    const char** type_params, const PDXEType** type_args);

#endif // PDXE_LSP_TYPE_REP_H
