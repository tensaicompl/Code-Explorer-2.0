/*
 * name_index.h -- issue 92: the entries of an array that only grows, by a name.
 *
 * The counterpart of site_index.h for a string key: a resolver that looks entries up
 * by name scanned the whole array per lookup. This index places entries as the array
 * grows, in order, so each name's chain holds its entries in ascending order: walking
 * it visits exactly the entries such a scan matched by name, in the scan's order, and
 * the caller applies the scan's other conditions to them. An entry without a name is
 * placed on no chain. The names must stay valid and unchanged while the index lives.
 *
 * Owned by the resolver's context and allocated in its arena. An array that cannot
 * grow (the memory core counts the failed allocation as lost work) leaves the index
 * failed: its lookups find nothing, never a scan.
 */
#ifndef PDXE_LSP_NAME_INDEX_H
#define PDXE_LSP_NAME_INDEX_H

#include "../arena.h"
#include "../lookup_counts.h"
#include <stdbool.h>
#include <stdint.h>
#include <string.h>

typedef struct {
    const char *name; /* NULL for an empty slot */
    int first;
    int last;
} PDXENameSlot;

typedef struct {
    PDXENameSlot *slots; /* open addressing, a power of two, at most half used */
    int cap;
    int used;
    int *next; /* per entry: the next entry with its name, -1 */
    int next_cap;
    int covered; /* entries [0, covered) are placed */
    bool failed;
} PDXENameIndex;

static inline uint64_t pdxe_name_hash(const char *s) {
    uint64_t h = 1469598103934665603ULL;
    for (; *s; s++) {
        h = (h ^ (unsigned char)*s) * 1099511628211ULL;
    }
    return h;
}

static inline int pdxe_name_slot(const PDXENameSlot *slots, int cap, const char *name) {
    int mask = cap - 1;
    for (int at = (int)(pdxe_name_hash(name) & (uint64_t)mask);; at = (at + 1) & mask) {
        PDXE_LOOKUP_COUNT(PDXE_LC_NAME_PROBES);
        if (!slots[at].name || strcmp(slots[at].name, name) == 0) {
            return at;
        }
    }
}

/* Places the next entry (index x->covered), named `name` (NULL: on no chain). */
static inline void pdxe_name_index_place(PDXENameIndex *x, PDXEArena *arena, const char *name) {
    if (x->failed) {
        return;
    }
    int i = x->covered;
    if ((i >= x->next_cap || 2 * (x->used + 1) > x->cap) && PDXE_LOOKUP_INDEX_REFUSED("resolver")) {
        x->failed = true;
        return;
    }
    if (i >= x->next_cap) {
        int cap = x->next_cap ? x->next_cap * 2 : 64;
        int *next = (int *)pdxe_arena_alloc(arena, (size_t)cap * sizeof(int));
        if (!next) {
            x->failed = true;
            return;
        }
        if (x->next_cap > 0) {
            memcpy(next, x->next, (size_t)x->next_cap * sizeof(int));
        }
        x->next = next;
        x->next_cap = cap;
    }
    x->next[i] = -1;
    x->covered++;
    if (!name) {
        return;
    }
    if (2 * (x->used + 1) > x->cap) {
        int cap = x->cap ? x->cap * 2 : 64;
        PDXENameSlot *slots = (PDXENameSlot *)pdxe_arena_alloc(arena, (size_t)cap * sizeof(*slots));
        if (!slots) {
            x->failed = true;
            return;
        }
        memset(slots, 0, (size_t)cap * sizeof(*slots));
        for (int at = 0; at < x->cap; at++) {
            if (x->slots[at].name) {
                slots[pdxe_name_slot(slots, cap, x->slots[at].name)] = x->slots[at];
            }
        }
        x->slots = slots;
        x->cap = cap;
    }
    PDXENameSlot *slot = &x->slots[pdxe_name_slot(x->slots, x->cap, name)];
    if (!slot->name) {
        slot->name = name;
        slot->first = i;
        x->used++;
    } else {
        x->next[slot->last] = i;
    }
    slot->last = i;
}

/* The first entry named `name`, -1 when none. */
static inline int pdxe_name_index_first(const PDXENameIndex *x, const char *name) {
    if (x->failed || x->cap == 0 || !name) {
        return -1;
    }
    const PDXENameSlot *slot = &x->slots[pdxe_name_slot(x->slots, x->cap, name)];
    return slot->name ? slot->first : -1;
}

/* The entry after entry i with its name, -1 when none. */
static inline int pdxe_name_index_next(const PDXENameIndex *x, int i) {
    return x->failed ? -1 : x->next[i];
}

#endif /* PDXE_LSP_NAME_INDEX_H */
