/*
 * site_index.h -- issue 92: the entries of an array that only grows, by their source
 * site.
 *
 * A resolver that asks whether an entry already stands at a site (a call it may carry,
 * a reference it would duplicate) scanned the whole array per question, which made a
 * file of many sites quadratic. This index places entries as the array grows, in
 * order: each site's chain holds its entries in ascending order, so walking it visits
 * exactly the entries such a scan matched by site, in the scan's order, and the
 * caller applies the scan's other conditions to them. Entries without a site
 * (end <= start) are placed on no chain.
 *
 * Owned by the resolver's context and allocated in its arena. An array that cannot
 * grow (the memory core counts the failed allocation as lost work) leaves the index
 * failed: its lookups answer that no entry stands at the site, never a scan.
 */
#ifndef PDXE_LSP_SITE_INDEX_H
#define PDXE_LSP_SITE_INDEX_H

#include "../arena.h"
#include "../lookup_counts.h"
#include <stdbool.h>
#include <stdint.h>
#include <string.h>

typedef struct {
    uint32_t start;
    uint32_t end;
    int first; /* the site's first entry, -1 for an empty slot */
    int last;  /* its last, where the next is chained */
} PDXESiteSlot;

typedef struct {
    PDXESiteSlot *slots; /* open addressing, a power of two, at most half used */
    int cap;
    int used;
    int *next; /* per entry: the next entry at its site, -1 */
    int next_cap;
    int covered; /* entries [0, covered) are placed */
    bool failed;
} PDXESiteIndex;

static inline uint64_t pdxe_site_hash(uint32_t start, uint32_t end) {
    uint64_t h = ((uint64_t)start << 32) | end;
    h ^= h >> 33;
    h *= 0xff51afd7ed558ccdULL;
    h ^= h >> 33;
    return h;
}

static inline int pdxe_site_slot(const PDXESiteSlot *slots, int cap, uint32_t start,
                                 uint32_t end) {
    int mask = cap - 1;
    for (int at = (int)(pdxe_site_hash(start, end) & (uint64_t)mask);; at = (at + 1) & mask) {
        if (slots[at].first < 0 || (slots[at].start == start && slots[at].end == end)) {
            return at;
        }
    }
}

/* Places the next entry (index x->covered), whose site is [start, end). */
static inline void pdxe_site_index_place(PDXESiteIndex *x, PDXEArena *arena, uint32_t start,
                                         uint32_t end) {
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
    if (end <= start) {
        return;
    }
    if (2 * (x->used + 1) > x->cap) {
        int cap = x->cap ? x->cap * 2 : 64;
        PDXESiteSlot *slots = (PDXESiteSlot *)pdxe_arena_alloc(arena, (size_t)cap * sizeof(*slots));
        if (!slots) {
            x->failed = true;
            return;
        }
        for (int at = 0; at < cap; at++) {
            slots[at].first = -1;
        }
        for (int at = 0; at < x->cap; at++) {
            if (x->slots[at].first >= 0) {
                slots[pdxe_site_slot(slots, cap, x->slots[at].start, x->slots[at].end)] =
                    x->slots[at];
            }
        }
        x->slots = slots;
        x->cap = cap;
    }
    PDXESiteSlot *slot = &x->slots[pdxe_site_slot(x->slots, x->cap, start, end)];
    if (slot->first < 0) {
        slot->start = start;
        slot->end = end;
        slot->first = i;
        x->used++;
    } else {
        x->next[slot->last] = i;
    }
    slot->last = i;
}

/* The first entry at site [start, end), -1 when none. */
static inline int pdxe_site_index_first(const PDXESiteIndex *x, uint32_t start, uint32_t end) {
    if (x->failed || x->cap == 0 || end <= start) {
        return -1;
    }
    return x->slots[pdxe_site_slot(x->slots, x->cap, start, end)].first;
}

/* The entry after entry i at its site, -1 when none. */
static inline int pdxe_site_index_next(const PDXESiteIndex *x, int i) {
    return x->failed ? -1 : x->next[i];
}

#endif /* PDXE_LSP_SITE_INDEX_H */
