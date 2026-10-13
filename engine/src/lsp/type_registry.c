#include "type_registry.h"
#include "../lookup_counts.h"
#include <string.h>
#include <stdlib.h>

// FNV-1a 64-bit hash. Public domain. Inline so no extra dep.
static uint64_t fnv1a(const char *s) {
    uint64_t h = 0xcbf29ce484222325ULL;
    if (!s)
        return h;
    while (*s) {
        h ^= (unsigned char)*s++;
        h *= 0x100000001b3ULL;
    }
    return h;
}

static uint64_t fnv1a_pair(const char *a, const char *b) {
    uint64_t h = 0xcbf29ce484222325ULL;
    if (a) {
        while (*a) {
            h ^= (unsigned char)*a++;
            h *= 0x100000001b3ULL;
        }
    }
    h ^= 0xff; // separator
    h *= 0x100000001b3ULL;
    if (b) {
        while (*b) {
            h ^= (unsigned char)*b++;
            h *= 0x100000001b3ULL;
        }
    }
    return h;
}

static int next_pow2(int n) {
    int p = 1;
    while (p < n)
        p <<= 1;
    return p;
}

/* --- Issue 92: live exact-key indexes ------------------------------------------
 *
 * Kept as entries are added, in every registry, so that no lookup scans the registry.
 * A registry that is never finalized (every per-file resolver's) answered each lookup
 * by scanning every entry, which made a file's typed resolution quadratic in its
 * definitions. Each key's chain is in ascending registration order: its head is the
 * entry the scan found first, and walking it visits exactly the entries the scan
 * matched, in the scan's order. A finalized registry keeps its hash indexes for the
 * entries it was finalized with, and reads the entries added later from these chains
 * instead of scanning them.
 *
 * An index that cannot grow (its allocation failed, which the memory core counts as
 * lost work) leaves those lookups answering nothing from then on: never a fallback
 * to scanning, which would make the work depend on the machine's memory.
 *
 * Test builds keep the scans for reference: with PDX_ENGINE_TEST_LINEAR_LOOKUP_REFERENCE
 * set when a registry is initialised, its lookups scan exactly as before. */
enum {
    LIVE_FUNC_QN,
    LIVE_TYPE_QN,
    LIVE_METHOD,
    LIVE_FREE_FUNC,
    LIVE_RECEIVER,
    LIVE_TYPE_SHORT,
    LIVE_TYPE_LAST
};

static bool live_kind_is_type(int kind) {
    return kind == LIVE_TYPE_QN || kind == LIVE_TYPE_SHORT || kind == LIVE_TYPE_LAST;
}

static PDXERegistryLiveTable *live_table(PDXETypeRegistry *reg, int kind) {
    switch (kind) {
    case LIVE_FUNC_QN:
        return &reg->live_func_qn;
    case LIVE_TYPE_QN:
        return &reg->live_type_qn;
    case LIVE_METHOD:
        return &reg->live_method;
    case LIVE_RECEIVER:
        return &reg->live_receiver;
    case LIVE_TYPE_SHORT:
        return &reg->live_type_short;
    case LIVE_TYPE_LAST:
        return &reg->live_type_last;
    default:
        return &reg->live_free_func;
    }
}

static const PDXERegistryLiveTable *live_table_const(const PDXETypeRegistry *reg, int kind) {
    return live_table((PDXETypeRegistry *)reg, kind);
}

static int *live_next(const PDXETypeRegistry *reg, int kind) {
    switch (kind) {
    case LIVE_FUNC_QN:
        return reg->live_func_next_qn;
    case LIVE_TYPE_QN:
        return reg->live_type_next_qn;
    case LIVE_METHOD:
        return reg->live_func_next_method;
    case LIVE_RECEIVER:
        return reg->live_func_next_receiver;
    case LIVE_TYPE_SHORT:
        return reg->live_type_next_short;
    case LIVE_TYPE_LAST:
        return reg->live_type_next_last;
    default:
        return reg->live_func_next_free;
    }
}

/* The key's hash, finished with a 64-bit mixer: FNV-1a's low bits, which pick the slot,
 * cluster on keys that differ only in a counter (C1, C2, ...), and linear probing
 * makes a cluster's every lookup longer. */
static uint64_t live_hash(int kind, const char *a, const char *b) {
    uint64_t h = kind == LIVE_METHOD ? fnv1a_pair(a, b) : fnv1a(a);
    h ^= h >> 33;
    h *= 0xff51afd7ed558ccdULL;
    h ^= h >> 33;
    h *= 0xc4ceb9fe1a85ec53ULL;
    h ^= h >> 33;
    return h;
}

/* Entry `e`'s key in index `kind`; false when the entry has none there. */
static bool live_key_of(const PDXETypeRegistry *reg, int kind, int e, const char **a,
                        const char **b) {
    *b = NULL;
    switch (kind) {
    case LIVE_FUNC_QN:
        *a = reg->funcs[e].qualified_name;
        return *a != NULL;
    case LIVE_TYPE_QN:
        *a = reg->types[e].qualified_name;
        return *a != NULL;
    case LIVE_METHOD:
        *a = reg->funcs[e].receiver_type;
        *b = reg->funcs[e].short_name;
        return *a != NULL && *b != NULL;
    case LIVE_RECEIVER:
        *a = reg->funcs[e].receiver_type;
        return *a != NULL;
    case LIVE_TYPE_SHORT:
        *a = reg->types[e].short_name;
        return *a != NULL;
    case LIVE_TYPE_LAST: {
        const char *qn = reg->types[e].qualified_name;
        if (!qn) {
            return false;
        }
        const char *dot = strrchr(qn, '.');
        *a = dot ? dot + 1 : qn;
        return true;
    }
    default:
        *a = reg->funcs[e].short_name;
        return reg->funcs[e].receiver_type == NULL && *a != NULL;
    }
}

/* Whether entry `e` has the key (a, b) in index `kind`: the predicate the scans used. */
static bool entry_has_key(const PDXETypeRegistry *reg, int kind, int e, const char *a,
                          const char *b) {
    const char *ka, *kb;
    if (!live_key_of(reg, kind, e, &ka, &kb)) {
        return false;
    }
    if (kind != LIVE_FUNC_QN && kind != LIVE_TYPE_QN) {
        PDXE_LOOKUP_COUNT(PDXE_LC_NAME_COMPARES);
    } else {
        PDXE_LOOKUP_COUNT(PDXE_LC_QN_COMPARES);
    }
    return strcmp(ka, a) == 0 && (kind != LIVE_METHOD || strcmp(kb, b) == 0);
}

/* The slot of key (a, b), hashed h: where it is, or the empty slot it would take. */
static int live_slot(const PDXETypeRegistry *reg, int kind, uint64_t h, const char *a,
                     const char *b) {
    const PDXERegistryLiveTable *t = live_table_const(reg, kind);
    int mask = t->cap - 1;
    for (int at = (int)(h & (uint64_t)mask);; at = (at + 1) & mask) {
        const PDXERegistryLiveSlot *slot = &t->slots[at];
        PDXE_LOOKUP_COUNT(PDXE_LC_REGISTRY_PROBES);
        if (slot->head < 0 || (slot->hash == h && entry_has_key(reg, kind, slot->head, a, b))) {
            return at;
        }
    }
}

/* The first entry with key (a, b) in index `kind`, or -1. */
static int live_head(const PDXETypeRegistry *reg, int kind, const char *a, const char *b) {
    const PDXERegistryLiveTable *t = live_table_const(reg, kind);
    if (reg->live_failed || t->cap == 0 || !a || (kind == LIVE_METHOD && !b)) {
        return -1;
    }
    return t->slots[live_slot(reg, kind, live_hash(kind, a, b), a, b)].head;
}

/* Doubles a table, placing every key again; false when it cannot be allocated. */
static bool live_table_grow(PDXETypeRegistry *reg, PDXERegistryLiveTable *t) {
    if (t->cap > (1 << 29) || PDXE_LOOKUP_INDEX_REFUSED("registry")) {
        return false;
    }
    int cap = t->cap ? t->cap * 2 : 64;
    PDXERegistryLiveSlot *slots =
        (PDXERegistryLiveSlot *)pdxe_arena_alloc(reg->arena, (size_t)cap * sizeof(*slots));
    if (!slots) {
        return false;
    }
    for (int i = 0; i < cap; i++) {
        slots[i].head = -1;
    }
    for (int i = 0; i < t->cap; i++) {
        if (t->slots[i].head < 0) {
            continue;
        }
        int at = (int)(t->slots[i].hash & (uint64_t)(cap - 1));
        while (slots[at].head >= 0) {
            at = (at + 1) & (cap - 1);
        }
        slots[at] = t->slots[i];
    }
    t->slots = slots;
    t->cap = cap;
    return true;
}

/* Grows a per-entry chain array to `cap` entries, keeping what it holds. */
static bool live_grow_next(PDXETypeRegistry *reg, int **next, int old_cap, int cap) {
    if (PDXE_LOOKUP_INDEX_REFUSED("registry")) {
        return false;
    }
    int *grown = (int *)pdxe_arena_alloc(reg->arena, (size_t)cap * sizeof(int));
    if (!grown) {
        return false;
    }
    if (*next && old_cap > 0) {
        memcpy(grown, *next, (size_t)old_cap * sizeof(int));
    }
    *next = grown;
    return true;
}

/* Chains entry `e` at the end of its key's chain in index `kind`. */
static bool live_insert(PDXETypeRegistry *reg, int kind, int e) {
    int *next = live_next(reg, kind);
    next[e] = -1;
    const char *a, *b;
    if (!live_key_of(reg, kind, e, &a, &b)) {
        return true;
    }
    PDXERegistryLiveTable *t = live_table(reg, kind);
    if (2 * (t->used + 1) > t->cap && !live_table_grow(reg, t)) {
        return false;
    }
    uint64_t h = live_hash(kind, a, b);
    PDXERegistryLiveSlot *slot = &t->slots[live_slot(reg, kind, h, a, b)];
    if (slot->head < 0) {
        slot->hash = h;
        slot->head = e;
        slot->tail = e;
        t->used++;
    } else {
        next[slot->tail] = e;
        slot->tail = e;
    }
    return true;
}

static void live_add_func(PDXETypeRegistry *reg, int e) {
    if (reg->live_failed) {
        return;
    }
    if (e >= reg->live_func_cap) {
        int cap = reg->func_cap;
        if (!live_grow_next(reg, &reg->live_func_next_qn, reg->live_func_cap, cap) ||
            !live_grow_next(reg, &reg->live_func_next_method, reg->live_func_cap, cap) ||
            !live_grow_next(reg, &reg->live_func_next_free, reg->live_func_cap, cap) ||
            !live_grow_next(reg, &reg->live_func_next_receiver, reg->live_func_cap, cap)) {
            reg->live_failed = true;
            return;
        }
        reg->live_func_cap = cap;
    }
    if (!live_insert(reg, LIVE_FUNC_QN, e) || !live_insert(reg, LIVE_METHOD, e) ||
        !live_insert(reg, LIVE_FREE_FUNC, e) || !live_insert(reg, LIVE_RECEIVER, e)) {
        reg->live_failed = true;
    }
}

static void live_add_type(PDXETypeRegistry *reg, int e) {
    if (reg->live_failed) {
        return;
    }
    if (e >= reg->live_type_cap) {
        int cap = reg->type_cap;
        if (!live_grow_next(reg, &reg->live_type_next_qn, reg->live_type_cap, cap) ||
            !live_grow_next(reg, &reg->live_type_next_short, reg->live_type_cap, cap) ||
            !live_grow_next(reg, &reg->live_type_next_last, reg->live_type_cap, cap)) {
            reg->live_failed = true;
            return;
        }
        reg->live_type_cap = cap;
    }
    if (!live_insert(reg, LIVE_TYPE_QN, e) || !live_insert(reg, LIVE_TYPE_SHORT, e) ||
        !live_insert(reg, LIVE_TYPE_LAST, e)) {
        reg->live_failed = true;
    }
}

/* The entries with key (a, b) in index `kind`, from entry `from` on, in ascending
 * registration order: read from the key's live chain, or, for the reference, by
 * scanning every entry from `from` as before issue 92. */
typedef struct {
    const PDXETypeRegistry *reg;
    int kind;
    const char *a;
    const char *b;
    int from;
    int next;
} match_iter;

static void match_open(match_iter *it, const PDXETypeRegistry *reg, int kind, const char *a,
                       const char *b, int from) {
    it->reg = reg;
    it->kind = kind;
    it->a = a;
    it->b = b;
    it->from = from;
#if defined(PDXE_ENABLE_TEST_SEAMS)
    if (reg->reference_scan) {
        it->next = from;
        return;
    }
#endif
    it->next = live_head(reg, kind, a, b);
}

static int match_next(match_iter *it) {
    const PDXETypeRegistry *reg = it->reg;
#if defined(PDXE_ENABLE_TEST_SEAMS)
    if (reg->reference_scan) {
        int count = live_kind_is_type(it->kind) ? reg->type_count : reg->func_count;
        while (it->next < count) {
            int i = it->next++;
            PDXE_LOOKUP_COUNT(PDXE_LC_REGISTRY_ENTRIES);
            if (it->a && (it->kind != LIVE_METHOD || it->b) &&
                entry_has_key(reg, it->kind, i, it->a, it->b)) {
                return i;
            }
        }
        return -1;
    }
#endif
    const int *next = live_next(reg, it->kind);
    while (it->next >= 0) {
        int i = it->next;
        it->next = next[i];
        PDXE_LOOKUP_COUNT(PDXE_LC_REGISTRY_PROBES);
        if (i >= it->from) {
            return i;
        }
    }
    return -1;
}

/* The first entry with key (a, b) from entry `from` on, or -1. */
static int match_first(const PDXETypeRegistry *reg, int kind, const char *a, const char *b,
                       int from) {
    match_iter it;
    match_open(&it, reg, kind, a, b, from);
    return match_next(&it);
}

/* The entry after `i` with the same key as entry `i` in index `kind`, or -1. */
static int match_after(const PDXETypeRegistry *reg, int kind, int i) {
    if (!reg || i < 0) {
        return -1;
    }
#if defined(PDXE_ENABLE_TEST_SEAMS)
    if (reg->reference_scan) {
        const char *a, *b;
        if (!live_key_of(reg, kind, i, &a, &b)) {
            return -1;
        }
        return match_first(reg, kind, a, b, i + 1);
    }
#endif
    if (reg->live_failed) {
        return -1;
    }
    PDXE_LOOKUP_COUNT(PDXE_LC_REGISTRY_PROBES);
    return live_next(reg, kind)[i];
}

int pdxe_registry_func_first_by_qn(const PDXETypeRegistry *reg, const char *qualified_name) {
    return reg && qualified_name ? match_first(reg, LIVE_FUNC_QN, qualified_name, NULL, 0) : -1;
}

int pdxe_registry_func_next_by_qn(const PDXETypeRegistry *reg, int i) {
    return match_after(reg, LIVE_FUNC_QN, i);
}

int pdxe_registry_type_first_by_qn(const PDXETypeRegistry *reg, const char *qualified_name) {
    return reg && qualified_name ? match_first(reg, LIVE_TYPE_QN, qualified_name, NULL, 0) : -1;
}

int pdxe_registry_type_next_by_qn(const PDXETypeRegistry *reg, int i) {
    return match_after(reg, LIVE_TYPE_QN, i);
}

int pdxe_registry_func_first_by_receiver(const PDXETypeRegistry *reg, const char *receiver_qn) {
    return reg && receiver_qn ? match_first(reg, LIVE_RECEIVER, receiver_qn, NULL, 0) : -1;
}

int pdxe_registry_func_next_by_receiver(const PDXETypeRegistry *reg, int i) {
    return match_after(reg, LIVE_RECEIVER, i);
}

int pdxe_registry_func_first_by_method(const PDXETypeRegistry *reg, const char *receiver_qn,
                                       const char *method_name) {
    return reg && receiver_qn && method_name
               ? match_first(reg, LIVE_METHOD, receiver_qn, method_name, 0)
               : -1;
}

int pdxe_registry_func_next_by_method(const PDXETypeRegistry *reg, int i) {
    return match_after(reg, LIVE_METHOD, i);
}

int pdxe_registry_free_func_first(const PDXETypeRegistry *reg, const char *short_name) {
    return reg && short_name ? match_first(reg, LIVE_FREE_FUNC, short_name, NULL, 0) : -1;
}

int pdxe_registry_free_func_next(const PDXETypeRegistry *reg, int i) {
    return match_after(reg, LIVE_FREE_FUNC, i);
}

static void build_qn_index(PDXETypeRegistry *reg, PDXEArena *idx_arena, bool for_funcs) {
    int count = for_funcs ? reg->func_count : reg->type_count;
    if (count == 0)
        return;
    int bucket_count = next_pow2(count * 2);
    if (bucket_count < 16)
        bucket_count = 16;

    int *buckets = (int *)pdxe_arena_alloc(idx_arena, (size_t)bucket_count * sizeof(int));
    PDXERegistryHashEntry *entries = (PDXERegistryHashEntry *)pdxe_arena_alloc(
        idx_arena, (size_t)count * sizeof(PDXERegistryHashEntry));
    if (!buckets || !entries)
        return;
    for (int i = 0; i < bucket_count; i++)
        buckets[i] = -1;

    for (int i = 0; i < count; i++) {
        const char *qn = for_funcs ? reg->funcs[i].qualified_name : reg->types[i].qualified_name;
        if (!qn)
            continue;
        uint64_t h = fnv1a(qn);
        int slot = (int)(h & (uint64_t)(bucket_count - 1));
        entries[i].hash = h;
        entries[i].payload_index = i;
        entries[i].next_index = buckets[slot];
        entries[i].slot = slot;
        buckets[slot] = i;
    }
    if (for_funcs) {
        reg->func_qn_buckets = buckets;
        reg->func_qn_entries = entries;
        reg->func_qn_bucket_count = bucket_count;
        reg->func_qn_entry_count = count;
    } else {
        reg->type_qn_buckets = buckets;
        reg->type_qn_entries = entries;
        reg->type_qn_bucket_count = bucket_count;
        reg->type_qn_entry_count = count;
    }
}

static void build_method_index(PDXETypeRegistry *reg, PDXEArena *idx_arena) {
    if (reg->func_count == 0)
        return;
    int bucket_count = next_pow2(reg->func_count * 2);
    if (bucket_count < 16)
        bucket_count = 16;
    int *buckets = (int *)pdxe_arena_alloc(idx_arena, (size_t)bucket_count * sizeof(int));
    PDXERegistryHashEntry *entries = (PDXERegistryHashEntry *)pdxe_arena_alloc(
        reg->arena, (size_t)reg->func_count * sizeof(PDXERegistryHashEntry));
    if (!buckets || !entries)
        return;
    for (int i = 0; i < bucket_count; i++)
        buckets[i] = -1;

    int idx = 0;
    for (int i = 0; i < reg->func_count; i++) {
        const PDXERegisteredFunc *f = &reg->funcs[i];
        if (!f->receiver_type || !f->short_name)
            continue;
        uint64_t h = fnv1a_pair(f->receiver_type, f->short_name);
        int slot = (int)(h & (uint64_t)(bucket_count - 1));
        entries[idx].hash = h;
        entries[idx].payload_index = i;
        entries[idx].next_index = buckets[slot];
        entries[idx].slot = slot;
        buckets[slot] = idx;
        idx++;
    }
    reg->method_buckets = buckets;
    reg->method_entries = entries;
    reg->method_bucket_count = bucket_count;
    reg->method_entry_count = idx;
}

/* Index: bare(embedded_type) -> chain of TYPE indices declaring it. One entry per
 * (type, embedded_type). Types are iterated DESCENDING and prepended so each bucket
 * chain ends up in ASCENDING type-index order (preserves the Rust loops' first-match
 * tie-break); a type's multiple entries land adjacent so the iterator can dedup them. */
static void build_type_embed_index(PDXETypeRegistry *reg, PDXEArena *idx_arena) {
    int ecount = 0;
    for (int i = 0; i < reg->type_count; i++) {
        const char **e = reg->types[i].embedded_types;
        if (!e)
            continue;
        for (int j = 0; e[j]; j++)
            ecount++;
    }
    if (ecount == 0)
        return;
    int bucket_count = next_pow2(ecount * 2);
    if (bucket_count < 16)
        bucket_count = 16;
    int *buckets = (int *)pdxe_arena_alloc(idx_arena, (size_t)bucket_count * sizeof(int));
    PDXERegistryHashEntry *entries = (PDXERegistryHashEntry *)pdxe_arena_alloc(
        idx_arena, (size_t)ecount * sizeof(PDXERegistryHashEntry));
    if (!buckets || !entries)
        return;
    for (int i = 0; i < bucket_count; i++)
        buckets[i] = -1;
    int idx = 0;
    for (int i = reg->type_count - 1; i >= 0; i--) {
        const char **e = reg->types[i].embedded_types;
        if (!e)
            continue;
        for (int j = 0; e[j]; j++) {
            const char *s = e[j];
            if (!s)
                continue;
            const char *dot = strrchr(s, '.');
            const char *bare = dot ? dot + 1 : s;
            uint64_t h = fnv1a(bare);
            int slot = (int)(h & (uint64_t)(bucket_count - 1));
            entries[idx].hash = h;
            entries[idx].payload_index = i;
            entries[idx].next_index = buckets[slot];
            entries[idx].slot = slot;
            buckets[slot] = idx;
            idx++;
        }
    }
    reg->type_embed_buckets = buckets;
    reg->type_embed_entries = entries;
    reg->type_embed_bucket_count = bucket_count;
    reg->type_embed_entry_count = idx;
}

/* Optional index: final qualified-name segment -> chain of TYPE indices.
 * Descending iteration plus prepend preserves ascending registry scan order.
 * Consumers opt in after the QN index captures the finalized tail boundary. */
void pdxe_registry_build_type_short_index(PDXETypeRegistry *reg) {
    if (!reg)
        return;
    reg->type_short_buckets = NULL;
    reg->type_short_entries = NULL;
    reg->type_short_bucket_count = 0;
    if (!reg->arena || !reg->type_qn_buckets || reg->type_qn_bucket_count <= 0)
        return;
    PDXEArena *idx_arena = reg->arena;
    int count = 0;
    for (int i = 0; i < reg->type_count; i++) {
        if (reg->types[i].qualified_name)
            count++;
    }
    if (count == 0)
        return;
    int bucket_count = next_pow2(count * 2);
    if (bucket_count < 16)
        bucket_count = 16;
    int *buckets = (int *)pdxe_arena_alloc(idx_arena, (size_t)bucket_count * sizeof(int));
    PDXERegistryHashEntry *entries = (PDXERegistryHashEntry *)pdxe_arena_alloc(
        idx_arena, (size_t)count * sizeof(PDXERegistryHashEntry));
    if (!buckets || !entries)
        return;
    for (int i = 0; i < bucket_count; i++)
        buckets[i] = -1;
    int idx = 0;
    for (int i = reg->type_count - 1; i >= 0; i--) {
        const char *qn = reg->types[i].qualified_name;
        if (!qn)
            continue;
        const char *dot = strrchr(qn, '.');
        const char *short_name = dot ? dot + 1 : qn;
        uint64_t h = fnv1a(short_name);
        int slot = (int)(h & (uint64_t)(bucket_count - 1));
        entries[idx].hash = h;
        entries[idx].payload_index = i;
        entries[idx].next_index = buckets[slot];
        entries[idx].slot = slot;
        buckets[slot] = idx;
        idx++;
    }
    reg->type_short_buckets = buckets;
    reg->type_short_entries = entries;
    reg->type_short_bucket_count = bucket_count;
}

/* Index: short_name -> chain of FREE-function (receiver_type==NULL) indices.
 * Descending-iterate + prepend for ascending chain order (as above). */
static void build_type_short_index(PDXETypeRegistry *reg, PDXEArena *idx_arena) {
    int tcount = 0;
    for (int i = 0; i < reg->type_count; i++) {
        if (reg->types[i].short_name)
            tcount++;
    }
    if (tcount == 0)
        return;
    int bucket_count = next_pow2(tcount * 2);
    if (bucket_count < 16)
        bucket_count = 16;
    int *buckets = (int *)pdxe_arena_alloc(idx_arena, (size_t)bucket_count * sizeof(int));
    PDXERegistryHashEntry *entries = (PDXERegistryHashEntry *)pdxe_arena_alloc(
        idx_arena, (size_t)tcount * sizeof(PDXERegistryHashEntry));
    if (!buckets || !entries)
        return;
    for (int i = 0; i < bucket_count; i++)
        buckets[i] = -1;
    int idx = 0;
    /* Reverse insertion so each chain yields ASCENDING types[] order — callers
     * that used first-match-in-registration-order keep their tie-breaks. */
    for (int i = reg->type_count - 1; i >= 0; i--) {
        const PDXERegisteredType *t = &reg->types[i];
        if (!t->short_name)
            continue;
        uint64_t h = fnv1a(t->short_name);
        int slot = (int)(h & (uint64_t)(bucket_count - 1));
        entries[idx].hash = h;
        entries[idx].payload_index = i;
        entries[idx].next_index = buckets[slot];
        entries[idx].slot = slot;
        buckets[slot] = idx;
        idx++;
    }
    reg->type_short_buckets = buckets;
    reg->type_short_entries = entries;
    reg->type_short_bucket_count = bucket_count;
    reg->type_short_entry_count = idx;
}

static void build_ffunc_short_index(PDXETypeRegistry *reg, PDXEArena *idx_arena) {
    int fcount = 0;
    for (int i = 0; i < reg->func_count; i++) {
        const PDXERegisteredFunc *f = &reg->funcs[i];
        if (!f->receiver_type && f->short_name)
            fcount++;
    }
    if (fcount == 0)
        return;
    int bucket_count = next_pow2(fcount * 2);
    if (bucket_count < 16)
        bucket_count = 16;
    int *buckets = (int *)pdxe_arena_alloc(idx_arena, (size_t)bucket_count * sizeof(int));
    PDXERegistryHashEntry *entries = (PDXERegistryHashEntry *)pdxe_arena_alloc(
        idx_arena, (size_t)fcount * sizeof(PDXERegistryHashEntry));
    if (!buckets || !entries)
        return;
    for (int i = 0; i < bucket_count; i++)
        buckets[i] = -1;
    int idx = 0;
    for (int i = reg->func_count - 1; i >= 0; i--) {
        const PDXERegisteredFunc *f = &reg->funcs[i];
        if (f->receiver_type || !f->short_name)
            continue;
        uint64_t h = fnv1a(f->short_name);
        int slot = (int)(h & (uint64_t)(bucket_count - 1));
        entries[idx].hash = h;
        entries[idx].payload_index = i;
        entries[idx].next_index = buckets[slot];
        entries[idx].slot = slot;
        buckets[slot] = idx;
        idx++;
    }
    reg->ffunc_short_buckets = buckets;
    reg->ffunc_short_entries = entries;
    reg->ffunc_short_bucket_count = bucket_count;
    reg->ffunc_short_entry_count = idx;
}

void pdxe_registry_types_by_short_name(const PDXETypeRegistry *reg, const char *short_name,
                                      PDXETypeShortIter *out) {
    out->reg = reg;
    out->chain = false;
    out->key = short_name;
    out->shadow = NULL;
    out->hash = fnv1a(short_name);
    out->live = false;
    if (reg->type_qn_buckets && reg->type_qn_bucket_count > 0) {
        if (reg->type_short_buckets && reg->type_short_bucket_count > 0) {
            int slot = (int)(out->hash & (uint64_t)(reg->type_short_bucket_count - 1));
            out->chain_idx = reg->type_short_buckets[slot];
            out->tail_i = reg->type_qn_entry_count;
        } else {
            out->chain_idx = -1;
            out->tail_i = 0; /* auxiliary index not built: every entry is the tail */
        }
        out->tail_end = reg->type_count;
    } else {
        out->chain_idx = -1;
        out->tail_i = 0;
        out->tail_end = reg->type_count;
    }
#if defined(PDXE_ENABLE_TEST_SEAMS)
    if (reg->reference_scan) {
        return; /* the tail scanned entry by entry, as before issue 92 */
    }
#endif
    /* Issue 92: the tail's entries named short_name, by short name or by last QN
     * segment, from the live indexes instead of every tail entry. */
    out->live = true;
    out->live_from = out->tail_i;
    out->live_short = match_first(reg, LIVE_TYPE_SHORT, short_name, NULL, out->tail_i);
    out->live_last = match_first(reg, LIVE_TYPE_LAST, short_name, NULL, out->tail_i);
    out->tail_i = out->tail_end;
}

static int type_short_next_local(PDXETypeShortIter *it) {
    const PDXETypeRegistry *reg = it->reg;
    while (it->chain_idx >= 0) {
        const PDXERegistryHashEntry *e = &reg->type_short_entries[it->chain_idx];
        int p = e->payload_index;
        uint64_t h = e->hash;
        it->chain_idx = e->next_index;
        if (h != it->hash)
            continue;
        return p;
    }
    if (it->live) {
        /* The two ascending chains merged: each entry once, in registration order. */
        int a = it->live_short;
        int b = it->live_last;
        if (a < 0 && b < 0) {
            return -1;
        }
        int p = b < 0 || (a >= 0 && a <= b) ? a : b;
        if (a == p) {
            it->live_short = match_after(reg, LIVE_TYPE_SHORT, a);
        }
        if (b == p) {
            it->live_last = match_after(reg, LIVE_TYPE_LAST, b);
        }
        return p;
    }
    if (it->tail_i < it->tail_end)
        return it->tail_i++;
    return -1;
}

static const PDXERegisteredType *lookup_type_self(const PDXETypeRegistry *reg,
                                                 const char *qualified_name);
static const PDXERegisteredFunc *lookup_func_self(const PDXETypeRegistry *reg,
                                                 const char *qualified_name);

/* A base entry is shadowed when the head holds an entry with the same QN. */
static bool type_shadowed(const PDXETypeRegistry *shadow, const PDXETypeRegistry *reg, int i) {
    if (!shadow || shadow == reg || !reg->types[i].qualified_name) {
        return false;
    }
    return lookup_type_self(shadow, reg->types[i].qualified_name) != NULL;
}
static bool func_shadowed(const PDXETypeRegistry *shadow, const PDXETypeRegistry *reg, int i) {
    if (!shadow || shadow == reg || !reg->funcs[i].qualified_name) {
        return false;
    }
    return lookup_func_self(shadow, reg->funcs[i].qualified_name) != NULL;
}

static void type_short_open_linear(const PDXETypeRegistry *reg, PDXETypeShortIter *out) {
    out->reg = reg;
    out->hash = 0;
    out->chain_idx = -1;
    out->tail_i = 0;
    out->tail_end = reg->type_count;
    out->live = false;
}

int pdxe_type_short_iter_next(PDXETypeShortIter *it) {
    for (;;) {
        int p = type_short_next_local(it);
        if (p >= 0) {
            if (it->chain && type_shadowed(it->shadow, it->reg, p)) {
                continue;
            }
            return p;
        }
        if (!it->chain || !it->reg->fallback) {
            return -1;
        }
        const PDXETypeRegistry *next = it->reg->fallback;
        const char *key = it->key;
        const PDXETypeRegistry *shadow = it->shadow;
        if (key) {
            pdxe_registry_types_by_short_name(next, key, it);
        } else {
            type_short_open_linear(next, it);
        }
        it->chain = true;
        it->key = key;
        it->shadow = shadow;
    }
}

void pdxe_registry_types_by_short_name_chain(const PDXETypeRegistry *head, const char *short_name,
                                            PDXETypeShortIter *out) {
    pdxe_registry_types_by_short_name(head, short_name, out);
    out->chain = true;
    out->shadow = head;
}

void pdxe_registry_all_types_chain(const PDXETypeRegistry *head, PDXETypeShortIter *out) {
    type_short_open_linear(head, out);
    out->chain = true;
    out->key = NULL;
    out->shadow = head;
}

void pdxe_registry_types_by_embedded_bare(const PDXETypeRegistry *reg, const char *bare,
                                         PDXETypeEmbedIter *out) {
    out->reg = reg;
    out->chain = false;
    out->key = bare;
    out->shadow = NULL;
    out->hash = fnv1a(bare);
    out->prev_type = -1;
    if (reg->type_qn_buckets && reg->type_qn_bucket_count > 0) {
        /* finalized: walk the embed chain (if built) then the post-finalize tail */
        if (reg->type_embed_buckets && reg->type_embed_bucket_count > 0) {
            int slot = (int)(out->hash & (uint64_t)(reg->type_embed_bucket_count - 1));
            out->chain_idx = reg->type_embed_buckets[slot];
        } else {
            out->chain_idx = -1;
        }
        out->tail_i = reg->type_qn_entry_count;
        out->tail_end = reg->type_count;
    } else {
        /* unfinalized: full linear scan over all types (identical candidate set) */
        out->chain_idx = -1;
        out->tail_i = 0;
        out->tail_end = reg->type_count;
    }
}

static int type_embed_next_local(PDXETypeEmbedIter *it) {
    const PDXETypeRegistry *reg = it->reg;
    while (it->chain_idx >= 0) {
        const PDXERegistryHashEntry *e = &reg->type_embed_entries[it->chain_idx];
        int p = e->payload_index;
        uint64_t h = e->hash;
        it->chain_idx = e->next_index;
        if (h != it->hash)
            continue; /* hash collision */
        if (p == it->prev_type)
            continue; /* same type, multiple matching embeds — yield once */
        it->prev_type = p;
        return p;
    }
    if (it->tail_i < it->tail_end)
        return it->tail_i++;
    return -1;
}

int pdxe_type_embed_iter_next(PDXETypeEmbedIter *it) {
    for (;;) {
        int p = type_embed_next_local(it);
        if (p >= 0) {
            if (it->chain && type_shadowed(it->shadow, it->reg, p)) {
                continue;
            }
            return p;
        }
        if (!it->chain || !it->reg->fallback) {
            return -1;
        }
        const PDXETypeRegistry *next = it->reg->fallback;
        const char *key = it->key;
        const PDXETypeRegistry *shadow = it->shadow;
        pdxe_registry_types_by_embedded_bare(next, key, it);
        it->chain = true;
        it->shadow = shadow;
    }
}

void pdxe_registry_types_by_embedded_bare_chain(const PDXETypeRegistry *head, const char *bare,
                                               PDXETypeEmbedIter *out) {
    pdxe_registry_types_by_embedded_bare(head, bare, out);
    out->chain = true;
    out->shadow = head;
}

void pdxe_registry_free_funcs_by_short_name(const PDXETypeRegistry *reg, const char *short_name,
                                           PDXEFreeFuncIter *out) {
    out->reg = reg;
    out->chain = false;
    out->key = short_name;
    out->shadow = NULL;
    out->hash = fnv1a(short_name);
    out->live_idx = -1;
    if (reg->func_qn_buckets && reg->func_qn_bucket_count > 0) {
        if (reg->ffunc_short_buckets && reg->ffunc_short_bucket_count > 0) {
            int slot = (int)(out->hash & (uint64_t)(reg->ffunc_short_bucket_count - 1));
            out->chain_idx = reg->ffunc_short_buckets[slot];
        } else {
            out->chain_idx = -1;
        }
        out->tail_i = reg->func_qn_entry_count;
        out->tail_end = reg->func_count;
    } else {
        out->chain_idx = -1;
        out->tail_i = 0;
        out->tail_end = reg->func_count;
    }
#if defined(PDXE_ENABLE_TEST_SEAMS)
    if (reg->reference_scan) {
        return; /* the tail scanned entry by entry, as before issue 92 */
    }
#endif
    /* Issue 92: the tail's receiver-less funcs named short_name, from the live index
     * instead of every tail entry. */
    out->live_idx = match_first(reg, LIVE_FREE_FUNC, short_name, NULL, out->tail_i);
    out->tail_i = out->tail_end;
}

static int free_func_next_local(PDXEFreeFuncIter *it) {
    const PDXETypeRegistry *reg = it->reg;
    while (it->chain_idx >= 0) {
        const PDXERegistryHashEntry *e = &reg->ffunc_short_entries[it->chain_idx];
        int p = e->payload_index;
        uint64_t h = e->hash;
        it->chain_idx = e->next_index;
        if (h != it->hash)
            continue;
        return p; /* each free func has one short_name → no dedup needed */
    }
    if (it->live_idx >= 0) {
        int p = it->live_idx;
        it->live_idx = match_after(reg, LIVE_FREE_FUNC, p);
        return p;
    }
    if (it->tail_i < it->tail_end)
        return it->tail_i++;
    return -1;
}

static void free_func_open_linear(const PDXETypeRegistry *reg, PDXEFreeFuncIter *out) {
    out->reg = reg;
    out->hash = 0;
    out->chain_idx = -1;
    out->tail_i = 0;
    out->tail_end = reg->func_count;
    out->live_idx = -1;
}

int pdxe_free_func_iter_next(PDXEFreeFuncIter *it) {
    for (;;) {
        int p = free_func_next_local(it);
        if (p >= 0) {
            if (it->chain && func_shadowed(it->shadow, it->reg, p)) {
                continue;
            }
            return p;
        }
        if (!it->chain || !it->reg->fallback) {
            return -1;
        }
        const PDXETypeRegistry *next = it->reg->fallback;
        const char *key = it->key;
        const PDXETypeRegistry *shadow = it->shadow;
        if (key) {
            pdxe_registry_free_funcs_by_short_name(next, key, it);
        } else {
            free_func_open_linear(next, it);
        }
        it->chain = true;
        it->key = key;
        it->shadow = shadow;
    }
}

void pdxe_registry_free_funcs_by_short_name_chain(const PDXETypeRegistry *head,
                                                 const char *short_name, PDXEFreeFuncIter *out) {
    pdxe_registry_free_funcs_by_short_name(head, short_name, out);
    out->chain = true;
    out->shadow = head;
}

void pdxe_registry_all_funcs_chain(const PDXETypeRegistry *head, PDXEFreeFuncIter *out) {
    free_func_open_linear(head, out);
    out->chain = true;
    out->key = NULL;
    out->shadow = head;
}

void pdxe_registry_methods(const PDXETypeRegistry *reg, const char *receiver_qn,
                          const char *method_name, PDXEMethodIter *out) {
    memset(out, 0, sizeof(*out)); /* chain=false, shadow=NULL */
    out->reg = reg;
    out->receiver_qn = receiver_qn;
    out->method_name = method_name;
    out->chain_idx = -1;
    if (!reg || !receiver_qn || !method_name) {
        return;
    }
    out->hash = fnv1a_pair(receiver_qn, method_name);
    if (reg->method_buckets && reg->method_bucket_count > 0) {
        int slot = (int)(out->hash & (uint64_t)(reg->method_bucket_count - 1));
        out->chain_idx = reg->method_buckets[slot];
        out->tail_i = reg->func_qn_entry_count;
        out->tail_end = reg->func_count;
    } else {
        out->tail_i = 0;
        out->tail_end = reg->func_count;
    }
    /* The entries from tail_i on come from the key's live chain (issue 92). */
    out->live_idx = live_head(reg, LIVE_METHOD, receiver_qn, method_name);
}

static int method_next_local(PDXEMethodIter *it) {
    const PDXETypeRegistry *reg = it->reg;
    while (it->chain_idx >= 0) {
        const PDXERegistryHashEntry *e = &reg->method_entries[it->chain_idx];
        int p = e->payload_index;
        uint64_t h = e->hash;
        it->chain_idx = e->next_index;
        if (h != it->hash) {
            continue;
        }
        const PDXERegisteredFunc *f = &reg->funcs[p];
        if (f->receiver_type && f->short_name && strcmp(f->receiver_type, it->receiver_qn) == 0 &&
            strcmp(f->short_name, it->method_name) == 0) {
            return p;
        }
    }
#if defined(PDXE_ENABLE_TEST_SEAMS)
    if (reg->reference_scan) {
        while (it->tail_i < it->tail_end) {
            int p = it->tail_i++;
            const PDXERegisteredFunc *f = &reg->funcs[p];
            PDXE_LOOKUP_COUNT(PDXE_LC_REGISTRY_ENTRIES);
            if (f->receiver_type && f->short_name &&
                strcmp(f->receiver_type, it->receiver_qn) == 0 &&
                strcmp(f->short_name, it->method_name) == 0) {
                return p;
            }
        }
        return -1;
    }
#endif
    /* The key's entries from tail_i up to the snapshot, in registration order. */
    while (it->live_idx >= 0) {
        int p = it->live_idx;
        it->live_idx = reg->live_func_next_method[p];
        PDXE_LOOKUP_COUNT(PDXE_LC_REGISTRY_PROBES);
        if (p >= it->tail_i && p < it->tail_end) {
            return p;
        }
    }
    return -1;
}

int pdxe_method_iter_next(PDXEMethodIter *it) {
    if (!it || !it->reg || !it->receiver_qn || !it->method_name) {
        return -1;
    }
    for (;;) {
        int p = method_next_local(it);
        if (p >= 0) {
            if (it->chain && func_shadowed(it->shadow, it->reg, p)) {
                continue;
            }
            return p;
        }
        if (!it->chain || !it->reg->fallback) {
            return -1;
        }
        const PDXETypeRegistry *next = it->reg->fallback;
        const PDXETypeRegistry *shadow = it->shadow;
        pdxe_registry_methods(next, it->receiver_qn, it->method_name, it);
        it->chain = true;
        it->shadow = shadow;
    }
}

void pdxe_registry_methods_chain(const PDXETypeRegistry *head, const char *receiver_qn,
                                const char *method_name, PDXEMethodIter *out) {
    pdxe_registry_methods(head, receiver_qn, method_name, out);
    out->chain = true;
    out->shadow = head;
}

PDXERegisteredFunc *pdxe_registry_func_for_update(PDXETypeRegistry *head, const char *qualified_name) {
    if (!head || !qualified_name || head->read_only) {
        return NULL;
    }
    const PDXERegisteredFunc *own = lookup_func_self(head, qualified_name);
    if (own) {
        return (PDXERegisteredFunc *)own;
    }
    for (const PDXETypeRegistry *r = head->fallback; r; r = r->fallback) {
        const PDXERegisteredFunc *base = lookup_func_self(r, qualified_name);
        if (base) {
            int before = head->func_count;
            pdxe_registry_add_func(head, *base);
            if (head->func_count == before + 1) {
                return &head->funcs[before];
            }
            return NULL;
        }
    }
    return NULL;
}

PDXERegisteredType *pdxe_registry_type_for_update(PDXETypeRegistry *head, const char *qualified_name) {
    if (!head || !qualified_name || head->read_only) {
        return NULL;
    }
    const PDXERegisteredType *own = lookup_type_self(head, qualified_name);
    if (own) {
        return (PDXERegisteredType *)own;
    }
    for (const PDXETypeRegistry *r = head->fallback; r; r = r->fallback) {
        const PDXERegisteredType *base = lookup_type_self(r, qualified_name);
        if (base) {
            int before = head->type_count;
            pdxe_registry_add_type(head, *base);
            if (head->type_count == before + 1) {
                return &head->types[before];
            }
            return NULL;
        }
    }
    return NULL;
}

void pdxe_registry_finalize_into(PDXETypeRegistry *reg, PDXEArena *idx_arena) {
    if (!reg || !idx_arena)
        return;
    build_qn_index(reg, idx_arena, /*for_funcs=*/true);
    build_qn_index(reg, idx_arena, /*for_funcs=*/false);
    build_method_index(reg, idx_arena);
    build_type_embed_index(reg, idx_arena);
    build_ffunc_short_index(reg, idx_arena);
    build_type_short_index(reg, idx_arena);
}

void pdxe_registry_finalize(PDXETypeRegistry *reg) {
    if (!reg)
        return;
    pdxe_registry_finalize_into(reg, reg->arena);
}

void pdxe_registry_init(PDXETypeRegistry *reg, PDXEArena *arena) {
    memset(reg, 0, sizeof(PDXETypeRegistry));
    reg->arena = arena;
#if defined(PDXE_ENABLE_TEST_SEAMS)
    /* PDX_ENGINE_TEST_LINEAR_LOOKUP_REFERENCE=<non-empty>: this registry's lookups scan
     * every entry, as before issue 92, so tests can compare the answers. */
    const char *reference = getenv("PDX_ENGINE_TEST_LINEAR_LOOKUP_REFERENCE");
    reg->reference_scan = reference && reference[0];
#endif
}

void pdxe_registry_add_func(PDXETypeRegistry *reg, PDXERegisteredFunc func) {
    if (reg->read_only) {
        return; /* sealed Tier-2 shared registry: refuse post-finalize mutation (O(n^2)+race guard)
                 */
    }
    if (reg->func_count >= reg->func_cap) {
        int new_cap = reg->func_cap == 0 ? 64 : reg->func_cap * 2;
        PDXERegisteredFunc *new_items = (PDXERegisteredFunc *)pdxe_arena_alloc(
            reg->arena, (size_t)new_cap * sizeof(PDXERegisteredFunc));
        if (!new_items)
            return;
        if (reg->funcs && reg->func_count > 0) {
            memcpy(new_items, reg->funcs, (size_t)reg->func_count * sizeof(PDXERegisteredFunc));
        }
        reg->funcs = new_items;
        reg->func_cap = new_cap;
    }
    reg->funcs[reg->func_count++] = func;
    live_add_func(reg, reg->func_count - 1);
}

void pdxe_registry_add_type(PDXETypeRegistry *reg, PDXERegisteredType type) {
    if (reg->read_only) {
        return; /* sealed Tier-2 shared registry: refuse post-finalize mutation (O(n^2)+race guard)
                 */
    }
    if (reg->type_count >= reg->type_cap) {
        int new_cap = reg->type_cap == 0 ? 64 : reg->type_cap * 2;
        PDXERegisteredType *new_items = (PDXERegisteredType *)pdxe_arena_alloc(
            reg->arena, (size_t)new_cap * sizeof(PDXERegisteredType));
        if (!new_items)
            return;
        if (reg->types && reg->type_count > 0) {
            memcpy(new_items, reg->types, (size_t)reg->type_count * sizeof(PDXERegisteredType));
        }
        reg->types = new_items;
        reg->type_cap = new_cap;
    }
    reg->types[reg->type_count++] = type;
    live_add_type(reg, reg->type_count - 1);
}

static const PDXERegisteredFunc *lookup_method_self(const PDXETypeRegistry *reg,
                                                   const char *receiver_qn,
                                                   const char *method_name) {
    int from = 0;
    // Hashed path when registry is finalized.
    if (reg->method_buckets && reg->method_bucket_count > 0) {
        uint64_t h = fnv1a_pair(receiver_qn, method_name);
        int slot = (int)(h & (uint64_t)(reg->method_bucket_count - 1));
        for (int idx = reg->method_buckets[slot]; idx >= 0;
             idx = reg->method_entries[idx].next_index) {
            PDXE_LOOKUP_COUNT(PDXE_LC_REGISTRY_PROBES);
            if (reg->method_entries[idx].hash != h)
                continue;
            const PDXERegisteredFunc *f = &reg->funcs[reg->method_entries[idx].payload_index];
            PDXE_LOOKUP_COUNT(PDXE_LC_NAME_COMPARES);
            if (f->receiver_type && f->short_name && strcmp(f->receiver_type, receiver_qn) == 0 &&
                strcmp(f->short_name, method_name) == 0) {
                return f;
            }
        }
        /* Funcs added AFTER finalize are not in the buckets: they stay visible
         * through the live index (issue 92), first by registration order, as the
         * scan of that tail found them. */
        from = reg->func_qn_entry_count;
    }
    int i = match_first(reg, LIVE_METHOD, receiver_qn, method_name, from);
    return i >= 0 ? &reg->funcs[i] : NULL;
}

const PDXERegisteredFunc *pdxe_registry_lookup_method(const PDXETypeRegistry *reg,
                                                    const char *receiver_qn,
                                                    const char *method_name) {
    if (!reg || !receiver_qn || !method_name)
        return NULL;
    const PDXERegisteredFunc *r = lookup_method_self(reg, receiver_qn, method_name);
    if (!r && reg->fallback) {
        return pdxe_registry_lookup_method(reg->fallback, receiver_qn, method_name);
    }
    return r;
}

static const PDXERegisteredType *lookup_type_self(const PDXETypeRegistry *reg,
                                                 const char *qualified_name) {
    PDXE_LOOKUP_COUNT(PDXE_LC_TYPE_SELF);
    int from = 0;
    if (reg->type_qn_buckets && reg->type_qn_bucket_count > 0) {
        uint64_t h = fnv1a(qualified_name);
        int slot = (int)(h & (uint64_t)(reg->type_qn_bucket_count - 1));
        for (int idx = reg->type_qn_buckets[slot]; idx >= 0;
             idx = reg->type_qn_entries[idx].next_index) {
            PDXE_LOOKUP_COUNT(PDXE_LC_REGISTRY_PROBES);
            if (reg->type_qn_entries[idx].hash != h)
                continue;
            int p = reg->type_qn_entries[idx].payload_index;
            PDXE_LOOKUP_COUNT(PDXE_LC_QN_COMPARES);
            if (reg->types[p].qualified_name &&
                strcmp(reg->types[p].qualified_name, qualified_name) == 0) {
                return &reg->types[p];
            }
        }
        /* Types added after finalize: the live index (see lookup_method_self). */
        from = reg->type_qn_entry_count;
    }
    int i = match_first(reg, LIVE_TYPE_QN, qualified_name, NULL, from);
    return i >= 0 ? &reg->types[i] : NULL;
}

const PDXERegisteredType *pdxe_registry_lookup_type(const PDXETypeRegistry *reg,
                                                  const char *qualified_name) {
    if (!reg || !qualified_name)
        return NULL;
    const PDXERegisteredType *r = lookup_type_self(reg, qualified_name);
    if (!r && reg->fallback) {
        return pdxe_registry_lookup_type(reg->fallback, qualified_name);
    }
    return r;
}

static const PDXERegisteredFunc *lookup_func_self(const PDXETypeRegistry *reg,
                                                 const char *qualified_name) {
    PDXE_LOOKUP_COUNT(PDXE_LC_FUNC_SELF);
    int from = 0;
    if (reg->func_qn_buckets && reg->func_qn_bucket_count > 0) {
        uint64_t h = fnv1a(qualified_name);
        int slot = (int)(h & (uint64_t)(reg->func_qn_bucket_count - 1));
        for (int idx = reg->func_qn_buckets[slot]; idx >= 0;
             idx = reg->func_qn_entries[idx].next_index) {
            PDXE_LOOKUP_COUNT(PDXE_LC_REGISTRY_PROBES);
            if (reg->func_qn_entries[idx].hash != h)
                continue;
            int p = reg->func_qn_entries[idx].payload_index;
            PDXE_LOOKUP_COUNT(PDXE_LC_QN_COMPARES);
            if (reg->funcs[p].qualified_name &&
                strcmp(reg->funcs[p].qualified_name, qualified_name) == 0) {
                return &reg->funcs[p];
            }
        }
        /* Funcs added after finalize: the live index (see lookup_method_self). */
        from = reg->func_qn_entry_count;
    }
    int i = match_first(reg, LIVE_FUNC_QN, qualified_name, NULL, from);
    return i >= 0 ? &reg->funcs[i] : NULL;
}

const PDXERegisteredFunc *pdxe_registry_lookup_func(const PDXETypeRegistry *reg,
                                                  const char *qualified_name) {
    if (!reg || !qualified_name)
        return NULL;
    const PDXERegisteredFunc *r = lookup_func_self(reg, qualified_name);
    if (!r && reg->fallback) {
        return pdxe_registry_lookup_func(reg->fallback, qualified_name);
    }
    return r;
}

const PDXERegisteredType *pdxe_registry_resolve_alias(const PDXETypeRegistry *reg,
                                                    const char *type_qn) {
    if (!reg || !type_qn)
        return NULL;
    const PDXERegisteredType *rt = pdxe_registry_lookup_type(reg, type_qn);
    for (int i = 0; i < 16 && rt && rt->alias_of; i++) {
        const PDXERegisteredType *next = pdxe_registry_lookup_type(reg, rt->alias_of);
        if (!next)
            return rt;
        rt = next;
    }
    return rt;
}

const PDXERegisteredFunc *pdxe_registry_lookup_method_aliased(const PDXETypeRegistry *reg,
                                                            const char *receiver_qn,
                                                            const char *method_name) {
    if (!reg || !receiver_qn || !method_name)
        return NULL;

    // Direct lookup first
    const PDXERegisteredFunc *f = pdxe_registry_lookup_method(reg, receiver_qn, method_name);
    if (f)
        return f;

    // Follow alias chain
    const PDXERegisteredType *rt = pdxe_registry_lookup_type(reg, receiver_qn);
    for (int i = 0; i < 16 && rt && rt->alias_of; i++) {
        f = pdxe_registry_lookup_method(reg, rt->alias_of, method_name);
        if (f)
            return f;
        rt = pdxe_registry_lookup_type(reg, rt->alias_of);
    }
    return NULL;
}

const PDXERegisteredFunc *pdxe_registry_lookup_symbol(const PDXETypeRegistry *reg,
                                                    const char *package_qn, const char *name) {
    if (!reg || !package_qn || !name)
        return NULL;

    // Build expected QN: package_qn.name
    size_t pkg_len = strlen(package_qn);
    size_t name_len = strlen(name);
    size_t total_len = pkg_len + 1 + name_len;

    char buf[512];
    if (total_len >= sizeof(buf))
        return NULL;

    memcpy(buf, package_qn, pkg_len);
    buf[pkg_len] = '.';
    memcpy(buf + pkg_len + 1, name, name_len);
    buf[total_len] = '\0';

    return pdxe_registry_lookup_func(reg, buf);
}

// Count parameters in a FUNC signature.
static int count_func_params(const PDXERegisteredFunc *f) {
    if (!f || !f->signature || f->signature->kind != PDXE_TYPE_FUNC)
        return -1;
    if (!f->signature->data.func.param_types)
        return 0;
    int count = 0;
    while (f->signature->data.func.param_types[count])
        count++;
    return count;
}

const PDXERegisteredFunc *pdxe_registry_lookup_method_by_args(const PDXETypeRegistry *reg,
                                                            const char *receiver_qn,
                                                            const char *method_name,
                                                            int arg_count) {
    if (!reg || !receiver_qn || !method_name)
        return NULL;

    const PDXERegisteredFunc *first_match = NULL;
    const PDXERegisteredFunc *range_match = NULL;

    // Hashed path when finalized: walk only the (receiver,name) overload chain
    // instead of every registered func. The chain holds exactly the funcs whose
    // (receiver_type, short_name) hash to this key, so results are identical to
    // the linear scan below — just O(overloads) instead of O(func_count). This
    // matters on project-wide registries: a header with thousands of method
    // calls × tens of thousands of registered funcs was O(calls×funcs) (#410).
    if (reg->method_buckets && reg->method_bucket_count > 0) {
        // The chain is in descending registration order, but the linear scan
        // resolves ties by LOWEST registration index (first exact match, else
        // first range match, else first match). Track the lowest index in each
        // category so the hashed path returns the identical func.
        int exact_pi = -1, range_pi = -1, first_pi = -1;
        uint64_t h = fnv1a_pair(receiver_qn, method_name);
        int slot = (int)(h & (uint64_t)(reg->method_bucket_count - 1));
        for (int idx = reg->method_buckets[slot]; idx >= 0;
             idx = reg->method_entries[idx].next_index) {
            if (reg->method_entries[idx].hash != h)
                continue;
            int pi = reg->method_entries[idx].payload_index;
            const PDXERegisteredFunc *f = &reg->funcs[pi];
            PDXE_LOOKUP_COUNT(PDXE_LC_REGISTRY_PROBES);
            if (!f->receiver_type || !f->short_name || strcmp(f->receiver_type, receiver_qn) != 0 ||
                strcmp(f->short_name, method_name) != 0)
                continue;
            PDXE_LOOKUP_COUNT(PDXE_LC_ARG_CANDIDATES);
            if (first_pi < 0 || pi < first_pi)
                first_pi = pi;
            int pc = count_func_params(f);
            if (pc == arg_count) {
                if (exact_pi < 0 || pi < exact_pi)
                    exact_pi = pi;
                continue;
            }
            int min_pc = (f->min_params >= 0) ? f->min_params : pc;
            if (arg_count >= min_pc && arg_count <= pc) {
                if (range_pi < 0 || pi < range_pi)
                    range_pi = pi;
            }
        }
        /* Mirror lookup_method_self's post-finalize visibility contract.  The
         * hash chain covers the finalize-time prefix only; overloads added
         * later must participate in the same exact/range/first ordering. They
         * come from the live index, in registration order (issue 92). */
        match_iter tail;
        match_open(&tail, reg, LIVE_METHOD, receiver_qn, method_name, reg->func_qn_entry_count);
        for (int pi; (pi = match_next(&tail)) >= 0;) {
            const PDXERegisteredFunc *f = &reg->funcs[pi];
            PDXE_LOOKUP_COUNT(PDXE_LC_ARG_CANDIDATES);
            if (first_pi < 0 || pi < first_pi)
                first_pi = pi;
            int pc = count_func_params(f);
            if (pc == arg_count) {
                if (exact_pi < 0 || pi < exact_pi)
                    exact_pi = pi;
                continue;
            }
            int min_pc = (f->min_params >= 0) ? f->min_params : pc;
            if (arg_count >= min_pc && arg_count <= pc && (range_pi < 0 || pi < range_pi)) {
                range_pi = pi;
            }
        }
        int sel = exact_pi >= 0 ? exact_pi : (range_pi >= 0 ? range_pi : first_pi);
        if (sel >= 0)
            return &reg->funcs[sel];
        if (reg->fallback) {
            return pdxe_registry_lookup_method_by_args(reg->fallback, receiver_qn, method_name,
                                                      arg_count);
        }
        return NULL;
    }

    /* Unfinalized: the key's overloads in registration order, from the live index
     * (issue 92), exactly those the scan of every func matched, in its order. */
    match_iter it;
    match_open(&it, reg, LIVE_METHOD, receiver_qn, method_name, 0);
    for (int i; (i = match_next(&it)) >= 0;) {
        const PDXERegisteredFunc *f = &reg->funcs[i];
        PDXE_LOOKUP_COUNT(PDXE_LC_ARG_CANDIDATES);
        {
            if (!first_match)
                first_match = f;
            int pc = count_func_params(f);
            if (pc == arg_count)
                return f; // exact match
            // Accept if arg_count is in [min_params, pc] (default args)
            int min_pc = (f->min_params >= 0) ? f->min_params : pc;
            if (!range_match && arg_count >= min_pc && arg_count <= pc) {
                range_match = f;
            }
        }
    }
    const PDXERegisteredFunc *res = range_match ? range_match : first_match;
    if (!res && reg->fallback) {
        return pdxe_registry_lookup_method_by_args(reg->fallback, receiver_qn, method_name,
                                                  arg_count);
    }
    return res;
}

// --- Overload scoring by parameter type ---

// Unwrap pointer/reference to get the core QN for comparison.
static const char *type_to_qn_simple(const PDXEType *t) {
    if (!t)
        return NULL;
    // Unwrap references and pointers
    while (t) {
        switch (t->kind) {
        case PDXE_TYPE_POINTER:
            t = t->data.pointer.elem;
            continue;
        case PDXE_TYPE_REFERENCE:
            t = t->data.reference.elem;
            continue;
        case PDXE_TYPE_RVALUE_REF:
            t = t->data.reference.elem;
            continue;
        case PDXE_TYPE_NAMED:
            return t->data.named.qualified_name;
        case PDXE_TYPE_TEMPLATE:
            return t->data.template_type.template_name;
        case PDXE_TYPE_BUILTIN:
            return t->data.builtin.name;
        default:
            return NULL;
        }
    }
    return NULL;
}

// Check if two types are compatible via implicit conversion.
static bool c_types_compatible(const char *expected_qn, const char *actual_qn) {
    if (!expected_qn || !actual_qn)
        return false;
    // const char* / char* -> std::string, std::string_view
    if (strcmp(actual_qn, "char") == 0) {
        if (strcmp(expected_qn, "std.string") == 0 ||
            strcmp(expected_qn, "std.basic_string") == 0 ||
            strcmp(expected_qn, "std.string_view") == 0 ||
            strcmp(expected_qn, "std.basic_string_view") == 0 ||
            strcmp(expected_qn, "absl.string_view") == 0)
            return true;
    }
    // Numeric promotions: all numeric builtins are interconvertible
    static const char *numerics[] = {"int",       "long",    "short",    "float",    "double",
                                     "unsigned",  "size_t",  "int8_t",   "int16_t",  "int32_t",
                                     "int64_t",   "uint8_t", "uint16_t", "uint32_t", "uint64_t",
                                     "ptrdiff_t", NULL};
    bool exp_numeric = false, act_numeric = false;
    for (int i = 0; numerics[i]; i++) {
        if (strcmp(expected_qn, numerics[i]) == 0)
            exp_numeric = true;
        if (strcmp(actual_qn, numerics[i]) == 0)
            act_numeric = true;
    }
    if (exp_numeric && act_numeric)
        return true;
    // bool <-> int
    if ((strcmp(expected_qn, "bool") == 0 && act_numeric) ||
        (exp_numeric && strcmp(actual_qn, "bool") == 0))
        return true;
    return false;
}

// Score an overload match: higher = better. 0 = wrong arg count (no match).
static int score_overload_match(const PDXERegisteredFunc *f, const PDXEType **arg_types,
                                int arg_count) {
    int pc = count_func_params(f);
    int min_pc = (f->min_params >= 0) ? f->min_params : pc;
    if (arg_count < min_pc || arg_count > pc)
        return 0; // out of range
    if (!arg_types || !f->signature || !f->signature->data.func.param_types)
        return 50;
    int score = 50;
    for (int i = 0; i < arg_count; i++) {
        const PDXEType *expected = f->signature->data.func.param_types[i];
        const PDXEType *actual = arg_types[i];
        if (!expected || !actual || pdxe_type_is_unknown(actual))
            continue; // neutral
        const char *exp_qn = type_to_qn_simple(expected);
        const char *act_qn = type_to_qn_simple(actual);
        if (!exp_qn || !act_qn)
            continue;
        if (strcmp(exp_qn, act_qn) == 0) {
            score += 10; // exact type match
        } else if (c_types_compatible(exp_qn, act_qn)) {
            score += 5; // implicit conversion
        }
    }
    return score;
}

const PDXERegisteredFunc *pdxe_registry_lookup_method_by_types(const PDXETypeRegistry *reg,
                                                             const char *receiver_qn,
                                                             const char *method_name,
                                                             const PDXEType **arg_types,
                                                             int arg_count) {
    if (!reg || !receiver_qn || !method_name)
        return NULL;
    // If no type info, fall back to arg-count matching
    if (!arg_types)
        return pdxe_registry_lookup_method_by_args(reg, receiver_qn, method_name, arg_count);

    const PDXERegisteredFunc *best = NULL;
    int best_score = 0;
    const PDXERegisteredFunc *first_match = NULL;

    // Hashed path when finalized: score only the (receiver,name) overload chain
    // instead of every registered func. Identical result set to the linear scan
    // below (same membership test), but O(overloads) instead of O(func_count) —
    // the hot per-call lookup that made large-header resolve O(calls×funcs)
    // (#410: a 10989-def header took ~34s/file in the resolve phase).
    if (reg->method_buckets && reg->method_bucket_count > 0) {
        // The chain is in descending registration order. The linear scan keeps
        // the first (lowest-index) func at the best score (strict '>'), and
        // falls back to the lowest-index match. Track lowest index per category
        // so the hashed path returns the identical func.
        int best_pi = -1, first_pi = -1;
        uint64_t h = fnv1a_pair(receiver_qn, method_name);
        int slot = (int)(h & (uint64_t)(reg->method_bucket_count - 1));
        for (int idx = reg->method_buckets[slot]; idx >= 0;
             idx = reg->method_entries[idx].next_index) {
            if (reg->method_entries[idx].hash != h)
                continue;
            int pi = reg->method_entries[idx].payload_index;
            const PDXERegisteredFunc *f = &reg->funcs[pi];
            PDXE_LOOKUP_COUNT(PDXE_LC_REGISTRY_PROBES);
            if (!f->receiver_type || !f->short_name || strcmp(f->receiver_type, receiver_qn) != 0 ||
                strcmp(f->short_name, method_name) != 0)
                continue;
            PDXE_LOOKUP_COUNT(PDXE_LC_ARG_CANDIDATES);
            if (first_pi < 0 || pi < first_pi)
                first_pi = pi;
            int s = score_overload_match(f, arg_types, arg_count);
            if (s > best_score || (s == best_score && best_pi >= 0 && pi < best_pi)) {
                best_score = s;
                best_pi = pi;
            }
        }
        match_iter tail;
        match_open(&tail, reg, LIVE_METHOD, receiver_qn, method_name, reg->func_qn_entry_count);
        for (int pi; (pi = match_next(&tail)) >= 0;) {
            const PDXERegisteredFunc *f = &reg->funcs[pi];
            PDXE_LOOKUP_COUNT(PDXE_LC_ARG_CANDIDATES);
            if (first_pi < 0 || pi < first_pi)
                first_pi = pi;
            int s = score_overload_match(f, arg_types, arg_count);
            if (s > best_score || (s == best_score && best_pi >= 0 && pi < best_pi)) {
                best_score = s;
                best_pi = pi;
            }
        }
        // best_score starts at 0; a score of 0 means "wrong arg count" → no
        // match, exactly as the linear scan (which never sets best on s==0).
        int sel = (best_pi >= 0 && best_score > 0) ? best_pi : first_pi;
        if (sel >= 0)
            return &reg->funcs[sel];
        if (reg->fallback) {
            return pdxe_registry_lookup_method_by_types(reg->fallback, receiver_qn, method_name,
                                                       arg_types, arg_count);
        }
        return NULL;
    }

    match_iter it;
    match_open(&it, reg, LIVE_METHOD, receiver_qn, method_name, 0);
    for (int i; (i = match_next(&it)) >= 0;) {
        const PDXERegisteredFunc *f = &reg->funcs[i];
        PDXE_LOOKUP_COUNT(PDXE_LC_ARG_CANDIDATES);
        {
            if (!first_match)
                first_match = f;
            int s = score_overload_match(f, arg_types, arg_count);
            if (s > best_score) {
                best_score = s;
                best = f;
            }
        }
    }
    const PDXERegisteredFunc *res = best ? best : first_match;
    if (!res && reg->fallback) {
        return pdxe_registry_lookup_method_by_types(reg->fallback, receiver_qn, method_name,
                                                   arg_types, arg_count);
    }
    return res;
}

const PDXERegisteredFunc *pdxe_registry_lookup_symbol_by_types(const PDXETypeRegistry *reg,
                                                             const char *package_qn,
                                                             const char *name,
                                                             const PDXEType **arg_types,
                                                             int arg_count) {
    if (!reg || !package_qn || !name)
        return NULL;
    if (!arg_types)
        return pdxe_registry_lookup_symbol_by_args(reg, package_qn, name, arg_count);

    size_t pkg_len = strlen(package_qn);
    size_t name_len = strlen(name);
    size_t total_len = pkg_len + 1 + name_len;
    char buf[512];
    if (total_len >= sizeof(buf))
        return NULL;
    memcpy(buf, package_qn, pkg_len);
    buf[pkg_len] = '.';
    memcpy(buf + pkg_len + 1, name, name_len);
    buf[total_len] = '\0';

    const PDXERegisteredFunc *best = NULL;
    int best_score = 0;
    const PDXERegisteredFunc *first_match = NULL;

    // Hashed path when finalized: walk only the QN overload chain. The func QN
    // index chains every func sharing a qualified_name, so this scores the exact
    // same overload set as the linear scan, in O(overloads) not O(func_count).
    if (reg->func_qn_buckets && reg->func_qn_bucket_count > 0) {
        int best_pi = -1, first_pi = -1;
        uint64_t h = fnv1a(buf);
        int slot = (int)(h & (uint64_t)(reg->func_qn_bucket_count - 1));
        for (int idx = reg->func_qn_buckets[slot]; idx >= 0;
             idx = reg->func_qn_entries[idx].next_index) {
            if (reg->func_qn_entries[idx].hash != h)
                continue;
            int pi = reg->func_qn_entries[idx].payload_index;
            const PDXERegisteredFunc *f = &reg->funcs[pi];
            PDXE_LOOKUP_COUNT(PDXE_LC_REGISTRY_PROBES);
            if (!f->qualified_name || strcmp(f->qualified_name, buf) != 0)
                continue;
            PDXE_LOOKUP_COUNT(PDXE_LC_TS_CANDIDATES);
            if (first_pi < 0 || pi < first_pi)
                first_pi = pi;
            int s = score_overload_match(f, arg_types, arg_count);
            if (s > best_score || (s == best_score && best_pi >= 0 && pi < best_pi)) {
                best_score = s;
                best_pi = pi;
            }
        }
        match_iter tail;
        match_open(&tail, reg, LIVE_FUNC_QN, buf, NULL, reg->func_qn_entry_count);
        for (int pi; (pi = match_next(&tail)) >= 0;) {
            const PDXERegisteredFunc *f = &reg->funcs[pi];
            PDXE_LOOKUP_COUNT(PDXE_LC_TS_CANDIDATES);
            if (first_pi < 0 || pi < first_pi)
                first_pi = pi;
            int s = score_overload_match(f, arg_types, arg_count);
            if (s > best_score || (s == best_score && best_pi >= 0 && pi < best_pi)) {
                best_score = s;
                best_pi = pi;
            }
        }
        int sel = (best_pi >= 0 && best_score > 0) ? best_pi : first_pi;
        if (sel >= 0)
            return &reg->funcs[sel];
        if (reg->fallback) {
            return pdxe_registry_lookup_symbol_by_types(reg->fallback, package_qn, name, arg_types,
                                                       arg_count);
        }
        return NULL;
    }

    match_iter it;
    match_open(&it, reg, LIVE_FUNC_QN, buf, NULL, 0);
    for (int i; (i = match_next(&it)) >= 0;) {
        const PDXERegisteredFunc *f = &reg->funcs[i];
        PDXE_LOOKUP_COUNT(PDXE_LC_TS_CANDIDATES);
        {
            if (!first_match)
                first_match = f;
            int s = score_overload_match(f, arg_types, arg_count);
            if (s > best_score) {
                best_score = s;
                best = f;
            }
        }
    }
    const PDXERegisteredFunc *res = best ? best : first_match;
    if (!res && reg->fallback) {
        return pdxe_registry_lookup_symbol_by_types(reg->fallback, package_qn, name, arg_types,
                                                   arg_count);
    }
    return res;
}

const PDXERegisteredFunc *pdxe_registry_lookup_symbol_by_args(const PDXETypeRegistry *reg,
                                                            const char *package_qn,
                                                            const char *name, int arg_count) {
    if (!reg || !package_qn || !name)
        return NULL;

    size_t pkg_len = strlen(package_qn);
    size_t name_len = strlen(name);
    size_t total_len = pkg_len + 1 + name_len;
    char buf[512];
    if (total_len >= sizeof(buf))
        return NULL;
    memcpy(buf, package_qn, pkg_len);
    buf[pkg_len] = '.';
    memcpy(buf + pkg_len + 1, name, name_len);
    buf[total_len] = '\0';

    const PDXERegisteredFunc *first_match = NULL;
    const PDXERegisteredFunc *range_match = NULL;

    // Hashed path when finalized: walk only the QN overload chain (see
    // pdxe_registry_lookup_symbol_by_types). O(overloads) not O(func_count).
    if (reg->func_qn_buckets && reg->func_qn_bucket_count > 0) {
        int exact_pi = -1, range_pi = -1, first_pi = -1;
        uint64_t h = fnv1a(buf);
        int slot = (int)(h & (uint64_t)(reg->func_qn_bucket_count - 1));
        for (int idx = reg->func_qn_buckets[slot]; idx >= 0;
             idx = reg->func_qn_entries[idx].next_index) {
            if (reg->func_qn_entries[idx].hash != h)
                continue;
            int pi = reg->func_qn_entries[idx].payload_index;
            const PDXERegisteredFunc *f = &reg->funcs[pi];
            PDXE_LOOKUP_COUNT(PDXE_LC_REGISTRY_PROBES);
            if (!f->qualified_name || strcmp(f->qualified_name, buf) != 0)
                continue;
            PDXE_LOOKUP_COUNT(PDXE_LC_TS_CANDIDATES);
            if (first_pi < 0 || pi < first_pi)
                first_pi = pi;
            int pc = count_func_params(f);
            if (pc == arg_count) {
                if (exact_pi < 0 || pi < exact_pi)
                    exact_pi = pi;
                continue;
            }
            int min_pc = (f->min_params >= 0) ? f->min_params : pc;
            if (arg_count >= min_pc && arg_count <= pc) {
                if (range_pi < 0 || pi < range_pi)
                    range_pi = pi;
            }
        }
        match_iter tail;
        match_open(&tail, reg, LIVE_FUNC_QN, buf, NULL, reg->func_qn_entry_count);
        for (int pi; (pi = match_next(&tail)) >= 0;) {
            const PDXERegisteredFunc *f = &reg->funcs[pi];
            PDXE_LOOKUP_COUNT(PDXE_LC_TS_CANDIDATES);
            if (first_pi < 0 || pi < first_pi)
                first_pi = pi;
            int pc = count_func_params(f);
            if (pc == arg_count) {
                if (exact_pi < 0 || pi < exact_pi)
                    exact_pi = pi;
                continue;
            }
            int min_pc = (f->min_params >= 0) ? f->min_params : pc;
            if (arg_count >= min_pc && arg_count <= pc && (range_pi < 0 || pi < range_pi)) {
                range_pi = pi;
            }
        }
        int sel = exact_pi >= 0 ? exact_pi : (range_pi >= 0 ? range_pi : first_pi);
        if (sel >= 0)
            return &reg->funcs[sel];
        if (reg->fallback) {
            return pdxe_registry_lookup_symbol_by_args(reg->fallback, package_qn, name, arg_count);
        }
        return NULL;
    }

    match_iter it;
    match_open(&it, reg, LIVE_FUNC_QN, buf, NULL, 0);
    for (int i; (i = match_next(&it)) >= 0;) {
        const PDXERegisteredFunc *f = &reg->funcs[i];
        PDXE_LOOKUP_COUNT(PDXE_LC_TS_CANDIDATES);
        {
            if (!first_match)
                first_match = f;
            int pc = count_func_params(f);
            if (pc == arg_count)
                return f;
            int min_pc = (f->min_params >= 0) ? f->min_params : pc;
            if (!range_match && arg_count >= min_pc && arg_count <= pc) {
                range_match = f;
            }
        }
    }
    const PDXERegisteredFunc *res = range_match ? range_match : first_match;
    if (!res && reg->fallback) {
        return pdxe_registry_lookup_symbol_by_args(reg->fallback, package_qn, name, arg_count);
    }
    return res;
}

// --- TS-specific helpers ---

const PDXERegisteredFunc *pdxe_registry_lookup_callable(const PDXETypeRegistry *reg, PDXEArena *arena,
                                                      const char *type_qn) {
    if (!reg || !type_qn || !arena)
        return NULL;

    const PDXERegisteredType *rt = pdxe_registry_lookup_type(reg, type_qn);
    if (!rt || !rt->call_signature)
        return NULL;

    // Synthesise a PDXERegisteredFunc whose QN is "<type_qn>.__call".
    // Allocated in the caller's arena so it lives at least as long as the registry context.
    size_t qn_len = strlen(type_qn);
    const char suffix[] = ".__call";
    size_t total = qn_len + sizeof(suffix); // sizeof includes the NUL
    char *qn = (char *)pdxe_arena_alloc(arena, total);
    if (!qn)
        return NULL;
    memcpy(qn, type_qn, qn_len);
    memcpy(qn + qn_len, suffix, sizeof(suffix));

    PDXERegisteredFunc *f = (PDXERegisteredFunc *)pdxe_arena_alloc(arena, sizeof(PDXERegisteredFunc));
    if (!f)
        return NULL;
    memset(f, 0, sizeof(*f));
    f->qualified_name = qn;
    f->short_name = "__call";
    f->receiver_type = rt->qualified_name;
    f->signature = rt->call_signature;
    f->min_params = -1;
    return f;
}

const PDXEType *pdxe_registry_lookup_index_signature(const PDXETypeRegistry *reg, const char *type_qn,
                                                   const PDXEType *key_type) {
    if (!reg || !type_qn)
        return NULL;

    const PDXERegisteredType *rt = pdxe_registry_lookup_type(reg, type_qn);
    if (!rt || !rt->index_value_type)
        return NULL;

    // If no expected key type is recorded, accept any index. Otherwise require a match
    // on BUILTIN tag — TS allows string and number index signatures and one of each.
    if (!rt->index_key_type || !key_type)
        return rt->index_value_type;

    if (rt->index_key_type->kind == PDXE_TYPE_BUILTIN && key_type->kind == PDXE_TYPE_BUILTIN) {
        const char *a = rt->index_key_type->data.builtin.name;
        const char *b = key_type->data.builtin.name;
        if (a && b && strcmp(a, b) == 0)
            return rt->index_value_type;
        // String index also accepts numeric keys (number → toString).
        if (a && strcmp(a, "string") == 0)
            return rt->index_value_type;
    }
    return NULL;
}
