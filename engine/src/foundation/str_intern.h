/*
 * str_intern.h — String interning pool.
 *
 * Deduplicates strings: identical strings share a single allocation.
 * Returns stable pointers — safe to compare by pointer equality after interning.
 *
 * Uses an arena for string storage (bulk free) + hash table for dedup lookup.
 */
#ifndef PDXE_STR_INTERN_H
#define PDXE_STR_INTERN_H

#include <stddef.h>
#include <stdint.h>

typedef struct PDXEInternPool PDXEInternPool;

/* Create a new intern pool. */
PDXEInternPool *pdxe_intern_create(void);

/* Free the pool and all interned strings. */
void pdxe_intern_free(PDXEInternPool *pool);

/* Intern a NUL-terminated string. Returns a stable pointer.
 * The same input always returns the same pointer. */
const char *pdxe_intern(PDXEInternPool *pool, const char *s);

/* Intern a string of known length. */
const char *pdxe_intern_n(PDXEInternPool *pool, const char *s, size_t len);

/* Number of unique strings in the pool. */
uint32_t pdxe_intern_count(const PDXEInternPool *pool);

/* Total bytes stored (unique strings only). */
size_t pdxe_intern_bytes(const PDXEInternPool *pool);

#endif /* PDXE_STR_INTERN_H */
