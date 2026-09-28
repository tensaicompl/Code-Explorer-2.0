#ifndef PDXE_LSP_TYPE_REGISTRY_H
#define PDXE_LSP_TYPE_REGISTRY_H

#include "type_rep.h"
#include "../arena.h"
#include <stdbool.h>

// Language-specific function metadata. Added at struct tail so existing
// callers that memset to zero before populating other fields keep working.
typedef enum {
    PDXE_FUNC_FLAG_NONE = 0,
    PDXE_FUNC_FLAG_PROPERTY = 1 << 0,        // @property -> obj.attr returns getter return
    PDXE_FUNC_FLAG_CLASSMETHOD = 1 << 1,     // @classmethod -> first arg is cls (the class)
    PDXE_FUNC_FLAG_STATICMETHOD = 1 << 2,    // @staticmethod -> no implicit self/cls
    PDXE_FUNC_FLAG_ABSTRACTMETHOD = 1 << 3,  // @abstractmethod -> still callable for resolution
    PDXE_FUNC_FLAG_OVERLOAD = 1 << 4,        // @overload entry — non-implementation stub
    PDXE_FUNC_FLAG_ASYNC = 1 << 5,           // async def — return is Coroutine[..., T]
    PDXE_FUNC_FLAG_GENERATOR = 1 << 6,       // contains yield — return is Generator[T, ...]
    PDXE_FUNC_FLAG_FINAL = 1 << 7,           // @final — overrides not allowed
    PDXE_FUNC_FLAG_RUST_TRAIT_IMPL = 1 << 8, // exact method from impl Trait for Type
    PDXE_FUNC_FLAG_RUST_ABSTRACT = 1 << 9,   // required trait method without a default body
    /* Python only: more than one registered definition has this exact QN.
     * Ordinary call resolution keeps its historical language-specific choice,
     * but a function value cannot name one materialized definition exactly. */
    PDXE_FUNC_FLAG_AMBIGUOUS_BINDING = 1 << 10,
} PDXEFuncFlags;

// Registered function/method with full type signature.
typedef struct {
    const char *qualified_name;    // e.g., "proj.pkg.TypeName.MethodName"
    const char *receiver_type;     // e.g., "proj.pkg.TypeName" (NULL for functions)
    const char *short_name;        // e.g., "MethodName"
    const PDXEType *signature;      // FUNC type with param/return types
    const char **type_param_names; // NULL-terminated, e.g., ["T", "R", NULL] for generics
    int min_params;                // Minimum required params (excluding defaulted). -1 = unknown.
    int flags;                     // PDXE_FUNC_FLAG_* bitfield
    const char **decorator_qns;    // NULL-terminated decorator QNs (Python only); used for
                                   // user-decorator return-type substitution.
    /* Rust only: canonical trait QN for a concrete trait-impl method. It may
     * remain NULL when raw cross-file provenance is ambiguous; the Rust trait
     * flag still prevents that method from being mistaken for inherent. */
    const char *impl_trait_qn;
} PDXERegisteredFunc;

// Registered type with fields and method names.
typedef struct {
    const char *qualified_name;    // e.g., "proj.pkg.TypeName"
    const char *short_name;        // e.g., "TypeName"
    const char **field_names;      // NULL-terminated
    const PDXEType **field_types;   // NULL-terminated (parallel to field_names)
    const char **method_names;     // NULL-terminated (short names)
    const char **method_qns;       // NULL-terminated (qualified names, parallel)
    const char **embedded_types;   // NULL-terminated (embedded/anonymous field type QNs)
    const char *alias_of;          // QN of aliased type (type Foo = Bar), NULL if not alias
    const char **type_param_names; // NULL-terminated, e.g., ["T", "K", NULL] for template classes
    bool is_interface;
    bool is_object; // Kotlin `object`/`companion object` singleton (member calls are static)

    // --- TS-specific fields (NULL/empty for non-TS types — backward compatible) ---
    // TS interfaces / object types may be callable: `interface F { (x:number): string }`.
    const PDXEType *call_signature; // FUNC type or NULL
    // TS objects can have an index signature: `{ [key:string]: V }` or `{ [i:number]: V }`.
    const PDXEType *index_key_type;   // BUILTIN("string"|"number") or NULL
    const PDXEType *index_value_type; // V or NULL
    // Generic constraints, parallel to type_param_names. NULL or shorter array means "any".
    const PDXEType **type_param_constraints; // NULL-terminated, parallel to type_param_names
} PDXERegisteredType;

// Hash-table bucket entry. Chains collisions via next-index list for overload sets.
typedef struct {
    uint64_t hash;     // FNV-1a of key
    int payload_index; // index into reg->funcs[] or reg->types[]
    int next_index;    // -1 = end of chain; else index of next bucket entry in same chain
    int slot;          // bucket slot this entry sits in (for resize)
} PDXERegistryHashEntry;

// Cross-file type/function registry.
typedef struct PDXETypeRegistry {
    PDXERegisteredFunc *funcs;
    int func_count;
    int func_cap;

    PDXERegisteredType *types;
    int type_count;
    int type_cap;

    PDXEArena *arena; // owns all string data

    /* Optional fallback registry (Tier 2 two-level lookup). When a
     * lookup misses in this registry, it chains to `fallback`. Used by
     * TS/PHP cross-LSP: a small per-file registry (the file's own
     * AST-refined types) chains to a shared, immutable base registry
     * (stdlib + all project defs) built once. NULL = no chaining. */
    const struct PDXETypeRegistry *fallback;

    // Hash indexes (built lazily by pdxe_registry_finalize, NULL until then).
    // Lookups fall back to linear scan when these are NULL.
    int *func_qn_buckets; // bucket → first entry index in func_qn_entries; -1 = empty
    PDXERegistryHashEntry *func_qn_entries; // entries indexed by linear order
    int func_qn_bucket_count;
    int func_qn_entry_count;

    int *type_qn_buckets;
    PDXERegistryHashEntry *type_qn_entries;
    int type_qn_bucket_count;
    int type_qn_entry_count;

    // Methods indexed by (receiver_type, short_name) — chain holds overloads.
    int *method_buckets;
    PDXERegistryHashEntry *method_entries;
    int method_bucket_count;
    int method_entry_count;

    // Auxiliary short-name / embedded-type indexes. The type short-name index is
    // opt-in after finalize; the others are built by finalize. Current C/C++ cross
    // registries opt in, while other languages incur no construction cost.
    // Type short-name index: fnv1a(last-'.'-segment of qualified_name) -> chain
    // of TYPE indices. payload_index = type index.
    int *type_short_buckets;
    PDXERegistryHashEntry *type_short_entries;
    int type_short_bucket_count;
    int type_short_entry_count;
    // Embedded-type index: fnv1a(bare last-'.'-segment of each embedded_type) -> chain
    // of TYPE indices declaring it. payload_index = type index (a type may appear once
    // per embedded entry; consumers dedup adjacent same-type via the iterator).
    int *type_embed_buckets;
    PDXERegistryHashEntry *type_embed_entries;
    int type_embed_bucket_count;
    int type_embed_entry_count;
    // Free-function short-name index: fnv1a(short_name) -> chain of FREE-function
    // (receiver_type==NULL) indices. payload_index = func index.
    int *ffunc_short_buckets;
    PDXERegistryHashEntry *ffunc_short_entries;
    int ffunc_short_bucket_count;
    int ffunc_short_entry_count;

    /* Sealed / read-only. Set true by the pdxe_X_build_cross_registry builders
     * (c/cpp, python, c#, ts, go) right after finalize: a Tier-2 cross-registry
     * is built ONCE and shared READ-ONLY across the parallel resolve workers.
     * pdxe_registry_add_func/_type no-op on a sealed registry, so a per-file
     * resolver can never mutate the shared, finalized registry. Without this,
     * post-finalize adds accumulate in a tail the hash index does not cover ->
     * every lookup linear-scans it -> O(files*defs) (the Linux-kernel full-index
     * hang) plus a heap data race across workers. */
    bool read_only;
} PDXETypeRegistry;

// Initialize a registry.
void pdxe_registry_init(PDXETypeRegistry *reg, PDXEArena *arena);

// Build the hash indexes after all funcs/types have been added. Subsequent lookups
// use O(1) hashed dispatch instead of linear scans. Calling this is OPTIONAL — the
// linear-scan path remains correct. Single-file resolvers (small registries) skip
// finalize and stay linear; project-wide registries (many thousands of entries) call
// it once after pass-1.5 def-collection.
void pdxe_registry_finalize(PDXETypeRegistry *reg);

// Like pdxe_registry_finalize, but the hash-index allocations (buckets/entries)
// come from idx_arena instead of reg->arena. Per-file cross resolvers MUST use
// this with a scratch arena destroyed after the walk: their reg->arena is the
// pipeline-lifetime result arena, and per-file index allocations accumulated
// there add GBs across a large repo (FastAPI incremental test: +1.1 GB RSS).
void pdxe_registry_finalize_into(PDXETypeRegistry *reg, PDXEArena *idx_arena);

// Build the optional final-QN-segment type index. Call immediately after
// finalization. Consumers that do not opt in avoid its type_count-sized allocation
// and scan. Allocation failure preserves the iterator's linear fallback.
void pdxe_registry_build_type_short_index(PDXETypeRegistry *reg);

// Register a function/method.
void pdxe_registry_add_func(PDXETypeRegistry *reg, PDXERegisteredFunc func);

// Register a type.
void pdxe_registry_add_type(PDXETypeRegistry *reg, PDXERegisteredType type);

// Look up a method by receiver type QN + method name.
const PDXERegisteredFunc *pdxe_registry_lookup_method(const PDXETypeRegistry *reg,
                                                    const char *receiver_qn,
                                                    const char *method_name);

// Look up a type by qualified name.
const PDXERegisteredType *pdxe_registry_lookup_type(const PDXETypeRegistry *reg,
                                                  const char *qualified_name);

// Look up a function by qualified name.
const PDXERegisteredFunc *pdxe_registry_lookup_func(const PDXETypeRegistry *reg,
                                                  const char *qualified_name);

// Look up a symbol (type or function) in a package by short name.
// package_qn is the package prefix (e.g., "proj.pkg").
const PDXERegisteredFunc *pdxe_registry_lookup_symbol(const PDXETypeRegistry *reg,
                                                    const char *package_qn, const char *name);

// Resolve type alias chain: follow alias_of until concrete type found (max 16 levels).
const PDXERegisteredType *pdxe_registry_resolve_alias(const PDXETypeRegistry *reg,
                                                    const char *type_qn);

// Look up a method by receiver type QN + method name, following alias chains.
const PDXERegisteredFunc *pdxe_registry_lookup_method_aliased(const PDXETypeRegistry *reg,
                                                            const char *receiver_qn,
                                                            const char *method_name);

// Look up a method by receiver type + name, preferring the overload with matching arg count.
// Falls back to any match if no exact arg count match found.
const PDXERegisteredFunc *pdxe_registry_lookup_method_by_args(const PDXETypeRegistry *reg,
                                                            const char *receiver_qn,
                                                            const char *method_name, int arg_count);

// Look up a free function by package + name, preferring matching arg count.
const PDXERegisteredFunc *pdxe_registry_lookup_symbol_by_args(const PDXETypeRegistry *reg,
                                                            const char *package_qn,
                                                            const char *name, int arg_count);

// Look up a method by receiver type + name, scoring overloads by parameter type match.
// arg_types may contain NULL entries for unknown types. Falls back to arg-count matching.
const PDXERegisteredFunc *pdxe_registry_lookup_method_by_types(const PDXETypeRegistry *reg,
                                                             const char *receiver_qn,
                                                             const char *method_name,
                                                             const PDXEType **arg_types,
                                                             int arg_count);

// Look up a free function by package + name, scoring overloads by parameter type match.
const PDXERegisteredFunc *pdxe_registry_lookup_symbol_by_types(const PDXETypeRegistry *reg,
                                                             const char *package_qn,
                                                             const char *name,
                                                             const PDXEType **arg_types,
                                                             int arg_count);

// --- Auxiliary index iterators (language fallback fast paths) ---
//
// Iterate registry TYPE indices whose qualified-name final segment may equal
// `short_name`. When the optional index is present this walks its hash chain plus
// any post-finalize tail; otherwise it degrades to the original full scan. Results
// preserve ascending registry order. The index is a hash prefilter; callers must
// re-check their exact predicate, including tail entries.
// Built by finalize into type_short_buckets. Shared by the C++ short-name lookup
// and by cs_resolve_type_name's step-9 fallback, which scanned type_count per
// unresolved name — quadratic against the shared Tier-2 registry.
typedef struct {
    const PDXETypeRegistry *reg;
    uint64_t hash;
    int chain_idx;
    int tail_i;
    int tail_end;
    /* Chain mode (see pdxe_registry_*_chain): when this registry is exhausted
     * the iterator re-opens on reg->fallback; `reg` then names the registry
     * the yielded index belongs to, so callers deref it->reg->types[i], never
     * the registry they started from. `key` re-opens the same query on the
     * next link; NULL = linear scan. `shadow` is the head: fallback entries
     * whose qualified_name the head also holds are skipped (an overlay copy
     * hides its base original). */
    bool chain;
    const char *key;
    const PDXETypeRegistry *shadow;
} PDXETypeShortIter;
void pdxe_registry_types_by_short_name(const PDXETypeRegistry *reg, const char *short_name,
                                      PDXETypeShortIter *out);
int pdxe_type_short_iter_next(PDXETypeShortIter *it);

// Iterate registry TYPE indices whose embedded_types contain an entry whose BARE
// name (last '.'-segment) equals `bare`. On a finalized registry this walks the
// embedded-type index plus any post-finalize tail; on an unfinalized registry it
// degrades to a full linear scan over all types (identical candidate set). Each
// matching type index is yielded at most once, in ascending registry order. The
// index is a bare-name PREFILTER — the caller MUST still apply its own exact
// predicate on each yielded type. Read-only, allocation-free. Usage:
//   PDXETypeEmbedIter it; pdxe_registry_types_by_embedded_bare(reg, bare, &it);
//   int ti; while ((ti = pdxe_type_embed_iter_next(&it)) >= 0) { ... reg->types[ti] ... }
typedef struct {
    const PDXETypeRegistry *reg;
    uint64_t hash;
    int chain_idx; // next entry in the embed chain, or -1
    int tail_i;    // next tail/linear type index
    int tail_end;  // reg->type_count snapshot
    int prev_type; // last yielded type index (adjacent-dedup); -1 = none
    bool chain;
    const char *key;
    const PDXETypeRegistry *shadow;
} PDXETypeEmbedIter;
void pdxe_registry_types_by_embedded_bare(const PDXETypeRegistry *reg, const char *bare,
                                         PDXETypeEmbedIter *out);
int pdxe_type_embed_iter_next(PDXETypeEmbedIter *it);

// Iterate FREE-function (receiver_type==NULL) indices whose short_name equals
// `short_name`. Same finalized/unfinalized behavior as above; caller re-checks its
// own predicate. Read-only, allocation-free.
typedef struct {
    const PDXETypeRegistry *reg;
    uint64_t hash;
    int chain_idx;
    int tail_i;
    int tail_end;
    bool chain;
    const char *key;
    const PDXETypeRegistry *shadow;
} PDXEFreeFuncIter;

void pdxe_registry_free_funcs_by_short_name(const PDXETypeRegistry *reg, const char *short_name,
                                           PDXEFreeFuncIter *out);
int pdxe_free_func_iter_next(PDXEFreeFuncIter *it);

// Iterate function indices for one exact (receiver QN, method name) key.  This
// exposes the existing finalized method bucket without making Rust scan the
// project-wide func array merely to distinguish inherent and trait-impl
// entries that intentionally share the same source-level QN.  The caller may
// filter on language-specific flags. Read-only and allocation-free.
typedef struct {
    const PDXETypeRegistry *reg;
    const char *receiver_qn;
    const char *method_name;
    uint64_t hash;
    int chain_idx;
    int tail_i;
    int tail_end;
    bool chain;
    const PDXETypeRegistry *shadow;
} PDXEMethodIter;
void pdxe_registry_methods(const PDXETypeRegistry *reg, const char *receiver_qn,
                          const char *method_name, PDXEMethodIter *out);
int pdxe_method_iter_next(PDXEMethodIter *it);

/* ── Chain-aware iteration and copy-on-write (per-file overlay contract) ──
 *
 * A resolve walk is handed a per-file OVERLAY registry chained (`fallback`) to
 * the immutable shared base. Lookups already chain; these make the index
 * iterators and the whole-registry scans chain too, and give a walk one way
 * to refine an entry: copy it into the head first. Nothing behind `fallback`
 * is ever written. Every yielded index belongs to it->reg at that moment. */
void pdxe_registry_types_by_short_name_chain(const PDXETypeRegistry *head, const char *short_name,
                                            PDXETypeShortIter *out);
void pdxe_registry_types_by_embedded_bare_chain(const PDXETypeRegistry *head, const char *bare,
                                               PDXETypeEmbedIter *out);
void pdxe_registry_free_funcs_by_short_name_chain(const PDXETypeRegistry *head,
                                                 const char *short_name, PDXEFreeFuncIter *out);
void pdxe_registry_methods_chain(const PDXETypeRegistry *head, const char *receiver_qn,
                                const char *method_name, PDXEMethodIter *out);
/* Linear scans over every func / type in the chain (head first, shadowed
 * base entries skipped). Same iterator types, same it->reg contract. */
void pdxe_registry_all_funcs_chain(const PDXETypeRegistry *head, PDXEFreeFuncIter *out);
void pdxe_registry_all_types_chain(const PDXETypeRegistry *head, PDXETypeShortIter *out);

/* The writable entry for qualified_name: the head's own entry if it has one,
 * else a copy of the first fallback entry added to the head (copy-on-write),
 * else NULL. NULL also when the head is sealed (read_only) -- a walk must never
 * mutate a shared registry, so the caller skips the refinement, exactly as the
 * sealed-registry no-op in pdxe_registry_add_func does today. */
PDXERegisteredFunc *pdxe_registry_func_for_update(PDXETypeRegistry *head, const char *qualified_name);
PDXERegisteredType *pdxe_registry_type_for_update(PDXETypeRegistry *head, const char *qualified_name);

// --- TS-specific helpers (return NULL for types without these signatures) ---

// If the type has a call signature (e.g., `interface F { (x:number): string }`), return
// a synthesised PDXERegisteredFunc whose qualified_name is "<type_qn>.__call" and
// short_name is "__call". Returns NULL if no call signature is present, the type is
// missing, or the receiver type was not registered. Caller must NOT free.
const PDXERegisteredFunc *pdxe_registry_lookup_callable(const PDXETypeRegistry *reg, PDXEArena *arena,
                                                      const char *type_qn);

// If the type has an index signature, return the value type produced by indexing with
// the given key type (string vs number). Returns NULL if no matching index signature.
const PDXEType *pdxe_registry_lookup_index_signature(const PDXETypeRegistry *reg, const char *type_qn,
                                                   const PDXEType *key_type);

#endif // PDXE_LSP_TYPE_REGISTRY_H
