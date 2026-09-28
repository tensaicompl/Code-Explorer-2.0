/*
 * hash_table.h — String → void* hash table.
 *
 * Public API unchanged across implementation rewrites. As of v2 the
 * internals are Verstable (https://github.com/JacksonAllan/Verstable),
 * a 2024 state-of-the-art open-addressing hash table with quadratic
 * probing + per-bucket 4-bit hash fragments. The struct is opaque —
 * callers MUST go through pdxe_ht_create() and the API functions.
 *
 * Keys are borrowed pointers — the table does not copy or free them.
 * Callers own the key strings for the lifetime of the entry.
 */
#ifndef PDXE_HASH_TABLE_H
#define PDXE_HASH_TABLE_H

#include <stddef.h>
#include <stdint.h>
#include "mem_core.h" /* pdxe_mem_class_t; same directory: the lsp_all unit has no -Isrc */
#include <stdbool.h>

/* Opaque — full definition lives in hash_table.c. */
typedef struct PDXEHashTable PDXEHashTable;

/* Create a hash table with initial capacity hint (used to pre-reserve
 * buckets and avoid early growth; 0 = library default). */
PDXEHashTable *pdxe_ht_create(uint32_t initial_capacity);

/* Same, with the memory class the table's buckets and entries are charged
 * to. pdxe_ht_create charges PDXE_MEM_CLASS_HASH_TABLE; an owner that wants
 * its indexes attributed (the graph buffer: gbuf_index) names its class. */
PDXEHashTable *pdxe_ht_create_in(pdxe_mem_class_t cls, uint32_t initial_capacity);

/* Free the hash table (does NOT free keys or values). */
void pdxe_ht_free(PDXEHashTable *ht);

/* Insert or update. Returns previous value (NULL if new key). */
void *pdxe_ht_set(PDXEHashTable *ht, const char *key, void *value);

/* Lookup. Returns NULL if not found. */
void *pdxe_ht_get(const PDXEHashTable *ht, const char *key);

/* Check if key exists. */
bool pdxe_ht_has(const PDXEHashTable *ht, const char *key);

/* Return the stored key pointer for a given lookup key, or NULL.
 * Useful when you need the canonical (heap-owned) key string
 * rather than your own local copy. */
const char *pdxe_ht_get_key(const PDXEHashTable *ht, const char *key);

/* Delete. Returns removed value (NULL if not found). */
void *pdxe_ht_delete(PDXEHashTable *ht, const char *key);

/* Number of entries. */
uint32_t pdxe_ht_count(const PDXEHashTable *ht);

/* Iteration: call fn(key, value, userdata) for each entry. */
typedef void (*pdxe_ht_iter_fn)(const char *key, void *value, void *userdata);
void pdxe_ht_foreach(const PDXEHashTable *ht, pdxe_ht_iter_fn fn, void *userdata);

/* Clear all entries (keeps allocated memory). */
void pdxe_ht_clear(PDXEHashTable *ht);

#endif /* PDXE_HASH_TABLE_H */
