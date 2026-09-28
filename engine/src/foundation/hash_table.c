/*
 * hash_table.c — PDXEHashTable backed by Verstable.
 *
 * Public API in hash_table.h is unchanged. Internals are a Verstable
 * template instantiation (const char* → void*). Verstable is a 2024
 * open-addressing hash table using quadratic probing with metadata
 * stored separately from buckets (4-bit hash fragment + 11-bit
 * displacement + 1-bit in-home-bucket flag per uint16_t). Documented
 * in vendored/verstable/verstable.h.
 *
 * Why swap the prior Robin Hood implementation: cumulative profiling
 * showed pdxe_ht_get is a hot path in resolve_file_calls's per-call
 * registry resolution. Verstable's 4-bit hash-fragment metadata
 * sidesteps most key comparisons during chain walks, which the prior
 * implementation could not.
 *
 * Lifetime: keys are BORROWED pointers (caller owns the strings).
 * Verstable's KEY_TY is const char*; the templated comparison +
 * hash use the standard vt_cmpr_string / vt_hash_string helpers.
 */
#include "foundation/constants.h"
#include "hash_table.h"
#include "mem_events.h"
#include <stdlib.h>
#include <string.h>

/* Instantiate a Verstable map of (const char* → void*). The single
 * include below generates static inline functions named pdxe_vt_init,
 * pdxe_vt_cleanup, pdxe_vt_get, pdxe_vt_insert, etc., plus the pdxe_vt
 * struct itself. */
/* Verstable allocates its bucket/entry blocks through the memory core; the
 * table's ctx is the class those blocks are charged to (Verstable hands the
 * ctx and the block size to both hooks). Without this the graph buffer's
 * 8.5M node keys and 15.8M edge keys on the kernel were memory no class
 * could see. */
static void *ht_alloc_in(size_t size, pdxe_mem_class_t *cls) {
    return pdxe_alloc(*cls, size);
}
static void ht_free_in(void *ptr, size_t size, pdxe_mem_class_t *cls) {
    (void)size;
    pdxe_free(*cls, ptr);
}
#define NAME pdxe_vt
#define KEY_TY const char *
#define VAL_TY void *
#define HASH_FN vt_hash_string
#define CMPR_FN vt_cmpr_string
#define CTX_TY pdxe_mem_class_t
#define MALLOC_FN ht_alloc_in
#define FREE_FN ht_free_in
#include "verstable/verstable.h"

/* The opaque PDXEHashTable struct holds the Verstable instance + a
 * count cache (Verstable's _size traversal is O(buckets) so we keep
 * our own atomic-free counter). */
struct PDXEHashTable {
    pdxe_vt vt;
    /* Waste sanitizer: where the table was created, how often it grew, and a
     * write counter (a lookup only repeats when nothing was written between). */
    void *waste_site;
    uint32_t waste_growths;
    uint64_t waste_generation;
};

#if defined(PDXE_MEMWASTE) && PDXE_MEMWASTE
#define HT_SITE() __builtin_return_address(0)
enum { HT_SLOT_BYTES = sizeof(pdxe_vt_bucket) + sizeof(uint16_t) };
#define HT_WROTE(ht) ((ht)->waste_generation++)
#define HT_LOOKED_UP(ht, key, found)                                                 \
    do {                                                                             \
        if (pdxe_memev_enabled()) {                                                   \
            /* site = the table's creation site: lookups are charged to the table */ \
            pdxe_work_note_ht(PDXE_WORK_HT_GET, (ht)->waste_site, (ht), (key),         \
                             (ht)->waste_generation, (found));                       \
        }                                                                            \
    } while (0)
#else
#define HT_SITE() NULL
#define HT_WROTE(ht) ((void)0)
#define HT_LOOKED_UP(ht, key, found) ((void)0)
#endif

static PDXEHashTable *ht_create_at(pdxe_mem_class_t cls, uint32_t initial_capacity, void *site);

PDXEHashTable *pdxe_ht_create(uint32_t initial_capacity) {
    return ht_create_at(PDXE_MEM_CLASS_HASH_TABLE, initial_capacity, HT_SITE());
}

PDXEHashTable *pdxe_ht_create_in(pdxe_mem_class_t cls, uint32_t initial_capacity) {
    return ht_create_at(cls, initial_capacity, HT_SITE());
}

static PDXEHashTable *ht_create_at(pdxe_mem_class_t cls, uint32_t initial_capacity, void *site) {
    PDXEHashTable *ht = (PDXEHashTable *)pdxe_calloc(cls, sizeof(*ht));
    if (!ht)
        return NULL;
    ht->waste_site = site;
    pdxe_vt_init(&ht->vt, cls);
    if (initial_capacity > 0) {
        /* Reserve enough buckets for the requested entries. Verstable
         * computes the minimum bucket count internally. */
        if (!pdxe_vt_reserve(&ht->vt, (size_t)initial_capacity)) {
            pdxe_vt_cleanup(&ht->vt);
            pdxe_free(cls, ht);
            return NULL;
        }
    }
    return ht;
}

void pdxe_ht_free(PDXEHashTable *ht) {
    if (!ht)
        return;
    pdxe_mem_class_t cls = ht->vt.ctx;
#if defined(PDXE_MEMWASTE) && PDXE_MEMWASTE
    if (pdxe_memev_enabled()) {
        pdxe_memev_container(PDXE_WORK_CT_HASH_TABLE, ht->waste_site,
                            (uint64_t)pdxe_vt_bucket_count(&ht->vt) * HT_SLOT_BYTES,
                            (uint64_t)pdxe_vt_size(&ht->vt) * HT_SLOT_BYTES, ht->waste_growths);
    }
#endif
    pdxe_vt_cleanup(&ht->vt);
    pdxe_free(cls, ht);
}

void *pdxe_ht_set(PDXEHashTable *ht, const char *key, void *value) {
    if (!ht || !key)
        return NULL;
    /* ONE probe: get_or_insert either inserts (key, value) or hands back the
     * existing entry, whose previous value is returned and whose key AND value
     * are replaced -- exactly what _insert did. The old get-then-insert hashed
     * every key twice: 83 M repeated strlen calls inside vt_hash_string on the
     * Go corpus (waste sanitizer, 2026-09-17). Verstable reports an insertion
     * as a size change. */
    void *prev = NULL;
    HT_WROTE(ht);
#if defined(PDXE_MEMWASTE) && PDXE_MEMWASTE
    size_t buckets_before = pdxe_vt_bucket_count(&ht->vt);
#endif
    size_t size_before = pdxe_vt_size(&ht->vt);
    pdxe_vt_itr itr = pdxe_vt_get_or_insert(&ht->vt, key, value);
    if (!pdxe_vt_is_end(itr) && pdxe_vt_size(&ht->vt) == size_before) {
        prev = itr.data->val;
        itr.data->key = key;
        itr.data->val = value;
    }
#if defined(PDXE_MEMWASTE) && PDXE_MEMWASTE
    bool grew = pdxe_vt_bucket_count(&ht->vt) != buckets_before;
    if (grew) {
        ht->waste_growths++;
    }
    if (pdxe_memev_enabled()) {
        pdxe_work_note(PDXE_WORK_HT_SET, ht->waste_site, 0, grew ? 1 : 0, 0);
    }
#endif
    return prev;
}

void *pdxe_ht_get(const PDXEHashTable *ht, const char *key) {
    if (!ht || !key)
        return NULL;
    pdxe_vt_itr itr = pdxe_vt_get(&ht->vt, key);
    HT_LOOKED_UP(ht, key, !pdxe_vt_is_end(itr));
    if (pdxe_vt_is_end(itr))
        return NULL;
    return itr.data->val;
}

bool pdxe_ht_has(const PDXEHashTable *ht, const char *key) {
    if (!ht || !key)
        return false;
    pdxe_vt_itr itr = pdxe_vt_get(&ht->vt, key);
    HT_LOOKED_UP(ht, key, !pdxe_vt_is_end(itr));
    return !pdxe_vt_is_end(itr);
}

const char *pdxe_ht_get_key(const PDXEHashTable *ht, const char *key) {
    if (!ht || !key)
        return NULL;
    pdxe_vt_itr itr = pdxe_vt_get(&ht->vt, key);
    HT_LOOKED_UP(ht, key, !pdxe_vt_is_end(itr));
    if (pdxe_vt_is_end(itr))
        return NULL;
    return itr.data->key;
}

void *pdxe_ht_delete(PDXEHashTable *ht, const char *key) {
    if (!ht || !key)
        return NULL;
    pdxe_vt_itr itr = pdxe_vt_get(&ht->vt, key);
    if (pdxe_vt_is_end(itr))
        return NULL;
    void *prev = itr.data->val;
    HT_WROTE(ht);
    (void)pdxe_vt_erase(&ht->vt, key);
    return prev;
}

uint32_t pdxe_ht_count(const PDXEHashTable *ht) {
    if (!ht)
        return 0;
    return (uint32_t)pdxe_vt_size(&ht->vt);
}

void pdxe_ht_foreach(const PDXEHashTable *ht, pdxe_ht_iter_fn fn, void *userdata) {
    if (!ht || !fn)
        return;
    for (pdxe_vt_itr itr = pdxe_vt_first(&ht->vt); !pdxe_vt_is_end(itr); itr = pdxe_vt_next(itr)) {
        fn(itr.data->key, itr.data->val, userdata);
    }
}

void pdxe_ht_clear(PDXEHashTable *ht) {
    if (!ht)
        return;
    HT_WROTE(ht);
    pdxe_vt_clear(&ht->vt);
}
