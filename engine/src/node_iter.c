/*
 * node_iter.c — stepping through a node's children by cursor (node_iter.h; issue 93).
 */
#define PDXE_NODE_ITER_UNCOUNTED 1 /* this file calls tree-sitter itself */
#include "node_iter.h"

#include <stdlib.h>
#include <string.h>

#include "foundation/compat.h"   // PDXE_TLS
#include "foundation/mem_core.h" // pdxe_calloc, pdxe_free
#include "lost_work.h"           // pdxe_lost_work: a cursor that disagreed with the count

/* --- the thread's cursors ------------------------------------------------------ */

/* Cursors are reset, never made, once the pool has them: making one allocates its
 * stack, and child walks run once per visited node (49.8 M cursor stacks on the Go
 * corpus before pooling, waste sanitizer 2026-09-17). Heap-backed, since 128 cursors
 * in static thread storage would be charged to every thread in the image. */
enum { CURSOR_POOL_SIZE = 128, CURSOR_POOL_WORDS = CURSOR_POOL_SIZE / 64 };
typedef struct {
    TSTreeCursor cursors[CURSOR_POOL_SIZE];
    uint64_t live[CURSOR_POOL_WORDS]; /* the cursor has been made */
    uint64_t busy[CURSOR_POOL_WORDS]; /* a lease holds it */
} cursor_pool_t;
static PDXE_TLS cursor_pool_t *tl_cursor_pool;

static bool pool_bit(const uint64_t *words, int slot) {
    return (words[slot / 64] >> (slot % 64)) & 1U;
}

static void pool_set(uint64_t *words, int slot, bool on) {
    uint64_t bit = (uint64_t)1 << (slot % 64);
    words[slot / 64] = on ? (words[slot / 64] | bit) : (words[slot / 64] & ~bit);
}

static int pool_free_slot(const cursor_pool_t *pool, int depth) {
    if (depth >= 0 && depth < CURSOR_POOL_SIZE && !pool_bit(pool->busy, depth)) {
        return depth;
    }
    /* Any other: the highest free, which recursive walks preferring their depth reach
     * last. */
    for (int w = CURSOR_POOL_WORDS - 1; w >= 0; w--) {
        uint64_t free_bits = ~pool->busy[w];
        if (free_bits) {
            return w * 64 + (63 - __builtin_clzll(free_bits));
        }
    }
    return -1;
}

TSTreeCursor *pdxe_cursor_acquire(pdxe_cursor_lease_t *lease, int depth, TSNode node) {
    if (!tl_cursor_pool) {
        tl_cursor_pool = pdxe_calloc(PDXE_MEM_CLASS_OTHER, sizeof(cursor_pool_t));
    }
    cursor_pool_t *pool = tl_cursor_pool;
    int slot = pool ? pool_free_slot(pool, depth) : -1;
    if (slot >= 0) {
        if (pool_bit(pool->live, slot)) {
            ts_tree_cursor_reset(&pool->cursors[slot], node);
        } else {
            pool->cursors[slot] = ts_tree_cursor_new(node);
            pool_set(pool->live, slot, true);
        }
        pool_set(pool->busy, slot, true);
        lease->slot = slot;
        lease->cursor = &pool->cursors[slot];
        return lease->cursor;
    }
    /* No pool, or every slot leased: a private cursor is always correct. */
    lease->private_cursor = ts_tree_cursor_new(node);
    lease->slot = -1;
    lease->cursor = &lease->private_cursor;
    return lease->cursor;
}

void pdxe_cursor_release(pdxe_cursor_lease_t *lease) {
    if (!lease->cursor) {
        return;
    }
    if (lease->slot < 0) {
        ts_tree_cursor_delete(&lease->private_cursor);
    } else if (tl_cursor_pool) {
        pool_set(tl_cursor_pool->busy, lease->slot, false);
    }
    lease->cursor = NULL;
}

#if defined(PDXE_ENABLE_TEST_SEAMS)
static void nav_release_thread(void);
#endif

void pdxe_node_iter_release_thread(void) {
    if (tl_cursor_pool) {
        bool busy = false;
        for (int slot = 0; slot < CURSOR_POOL_SIZE; slot++) {
            /* a leased cursor belongs to a walk still on this stack: leave it */
            if (pool_bit(tl_cursor_pool->busy, slot)) {
                busy = true;
            } else if (pool_bit(tl_cursor_pool->live, slot)) {
                ts_tree_cursor_delete(&tl_cursor_pool->cursors[slot]);
                pool_set(tl_cursor_pool->live, slot, false);
            }
        }
        /* Outstanding leases point INTO this block, so it is only released once no walk
         * still holds a slot. */
        if (!busy) {
            pdxe_free(PDXE_MEM_CLASS_OTHER, tl_cursor_pool);
            tl_cursor_pool = NULL;
        }
    }
#if defined(PDXE_ENABLE_TEST_SEAMS)
    nav_release_thread();
#endif
}

/* --- collections refused, in test builds ----------------------------------------- */

/* PDX_ENGINE_TEST_REFUSE_LOOKUP_INDEXES=children: every collection of children is refused
 * as a failed allocation refuses it (lost work, counted), so tests see what a pass does
 * without one. Only that exact value: `all` is the lookup indexes' (lookup_counts.h). */
#if defined(PDXE_ENABLE_TEST_SEAMS)
static bool children_refused(void) {
    static int refused = -1;
    if (refused < 0) {
        const char *value = getenv("PDX_ENGINE_TEST_REFUSE_LOOKUP_INDEXES");
        refused = value && strcmp(value, "children") == 0;
    }
    return refused != 0;
}
#else
#define children_refused() false
#endif

/* --- the iterator -------------------------------------------------------------- */

/* This thread's loops in progress, the innermost first (anchors for pdxe_node_parent). */
static PDXE_TLS PDXEChildIter *tl_iters;

static void iter_register(PDXEChildIter *it) {
    it->outer = tl_iters;
    tl_iters = it;
}

static void iter_unregister(PDXEChildIter *it) {
    /* Loops end innermost first, except a loop stepped by hand that runs out while a later
     * one is still going: unlinked wherever it is. */
    for (PDXEChildIter **at = &tl_iters; *at; at = &(*at)->outer) {
        if (*at == it) {
            *at = it->outer;
            break;
        }
    }
    it->outer = NULL;
}

/* The iterator's own cursor steps, counted in test builds as any other. */
static bool step_first(TSTreeCursor *cursor) {
    PDXE_LOOKUP_COUNT(PDXE_LC_NAV_CURSOR);
    return ts_tree_cursor_goto_first_child(cursor);
}

static bool step_next(TSTreeCursor *cursor) {
    PDXE_LOOKUP_COUNT(PDXE_LC_NAV_CURSOR);
    return ts_tree_cursor_goto_next_sibling(cursor);
}

static void step_up(TSTreeCursor *cursor) {
    PDXE_LOOKUP_COUNT(PDXE_LC_NAV_CURSOR);
    ts_tree_cursor_goto_parent(cursor);
}

static void iter_inconsistent(PDXEChildIter *it) {
    /* The cursor ran out before the count: the rest is never looked up by position. */
    pdxe_lost_work();
    PDXE_LOOKUP_COUNT(PDXE_LC_NAV_LOST);
    pdxe_child_iter_finish(it);
}

/* Named iteration from the cursor's current child: the first named child at or after
 * it, entering an anonymous wrapper that holds named children and leaving one that is
 * done, each as ts_node_named_child does (a wrapper is entered when its named children
 * are counted, and gives at most that many). False when the parent has no more. */
static bool iter_settle_named(PDXEChildIter *it) {
    TSTreeCursor *cursor = it->lease.cursor;
    for (;;) {
        TSNode current = ts_tree_cursor_current_node(cursor);
        if (ts_node_is_named(current)) {
            if (it->wrappers > 0) {
                it->wrapper_left[it->wrappers - 1]--;
            }
            it->node = current;
            return true;
        }
        uint32_t inner = ts_node_named_child_count(current);
        if (inner > 0 && it->wrappers < PDXE_CHILD_ITER_WRAPPERS && step_first(cursor)) {
            it->wrapper_left[it->wrappers++] = inner;
            continue;
        }
        if (inner > 0) {
            /* Deeper than any grammar nests them: never looked up by position. */
            return false;
        }
        /* Past this child: to its next sibling, out of each wrapper that is done. */
        for (;;) {
            bool owed = it->wrappers == 0 || it->wrapper_left[it->wrappers - 1] > 0;
            if (owed && step_next(cursor)) {
                break;
            }
            if (it->wrappers == 0) {
                return false;
            }
            step_up(cursor);
            it->wrappers--;
        }
    }
}

bool pdxe_child_iter_next(PDXEChildIter *it) {
    if (it->finished) {
        return false;
    }
    if (!it->started) {
        it->started = true;
        if (it->limit == 0) {
            pdxe_child_iter_finish(it);
            return false;
        }
        TSTreeCursor *cursor = pdxe_cursor_acquire(&it->lease, -1, it->parent);
        iter_register(it);
        if (!step_first(cursor)) {
            iter_inconsistent(it);
            return false;
        }
        it->index = 0;
        if (it->named) {
            if (!iter_settle_named(it)) {
                iter_inconsistent(it);
                return false;
            }
        } else {
            it->node = ts_tree_cursor_current_node(cursor);
        }
        return true;
    }
    /* Never past the count, whatever the cursor would still give. */
    if (it->index + 1 >= it->limit) {
        pdxe_child_iter_finish(it);
        return false;
    }
    TSTreeCursor *cursor = it->lease.cursor;
    it->prev = it->node;
    if (it->named) {
        /* Past the current named child, out of each wrapper that is done. */
        for (;;) {
            bool owed = it->wrappers == 0 || it->wrapper_left[it->wrappers - 1] > 0;
            if (owed && step_next(cursor)) {
                break;
            }
            if (it->wrappers == 0) {
                iter_inconsistent(it);
                return false;
            }
            step_up(cursor);
            it->wrappers--;
        }
        if (!iter_settle_named(it)) {
            iter_inconsistent(it);
            return false;
        }
    } else {
        if (!step_next(cursor)) {
            iter_inconsistent(it);
            return false;
        }
        it->node = ts_tree_cursor_current_node(cursor);
    }
    it->index++;
    return true;
}

const char *pdxe_child_iter_field_name(const PDXEChildIter *it) {
    if (!it->started || it->finished || !it->lease.cursor || it->wrappers > 0) {
        return NULL;
    }
    return ts_tree_cursor_current_field_name(it->lease.cursor);
}

static void siblings_free(PDXEPathSiblings *sib);

void pdxe_child_iter_finish(PDXEChildIter *it) {
    if (it->lease.cursor) {
        iter_unregister(it);
    }
    it->finished = true;
    pdxe_cursor_release(&it->lease);
    if (it->collected) {
        siblings_free(it->collected);
        free(it->collected);
        it->collected = NULL;
    }
}

TSNode *pdxe_collect_children(PDXEArena *arena, TSNode node, bool named, uint32_t *out_n) {
    *out_n = 0;
    uint32_t count =
        ts_node_is_null(node) ? 0 : (named ? ts_node_named_child_count(node) : ts_node_child_count(node));
    if (count == 0) {
        return NULL;
    }
    TSNode *kids = children_refused() ? NULL
                                      : (TSNode *)pdxe_arena_alloc(arena, (size_t)count * sizeof(TSNode));
    if (!kids) {
        /* The arena counted the allocation it could not make (a refusal is counted as
         * one). */
        if (children_refused()) {
            pdxe_lost_allocation();
        }
        PDXE_LOOKUP_COUNT(PDXE_LC_NAV_LOST);
        return NULL;
    }
    uint32_t n = 0;
    for (PDXEChildIter it PDXE_CHILD_ITER_SCOPED = pdxe_child_iter_begin(node, named);
         pdxe_child_iter_next(&it);) {
        kids[n++] = it.node;
    }
    *out_n = n;
    return n ? kids : NULL;
}

/* --- a walk's path ------------------------------------------------------------- */

/* tree-sitter's own lookups, counted in test builds as any other call is. */
#if defined(PDXE_ENABLE_TEST_SEAMS)
#define ROOT_PARENT(n) pdxe_counted_parent(n)
#define ROOT_SIBLING(n, which) pdxe_counted_sibling((n), (which))
#define CHILD_WITH_DESCENDANT(n, d) pdxe_counted_child_with_descendant((n), (d))
#else
#define ROOT_PARENT(n) ts_node_parent(n)
#define ROOT_SIBLING(n, which) root_sibling((n), (which))
#define CHILD_WITH_DESCENDANT(n, d) ts_node_child_with_descendant((n), (d))
#endif

enum { SIB_PREV, SIB_NEXT, SIB_PREV_NAMED, SIB_NEXT_NAMED };

#if !defined(PDXE_ENABLE_TEST_SEAMS)
static TSNode root_sibling(TSNode node, int which) {
    switch (which) {
    case SIB_PREV:
        return ts_node_prev_sibling(node);
    case SIB_NEXT:
        return ts_node_next_sibling(node);
    case SIB_PREV_NAMED:
        return ts_node_prev_named_sibling(node);
    default:
        return ts_node_next_named_sibling(node);
    }
}
#else
/* pdxe_counted_sibling's order */
static const int SIB_TO_NAV[4] = {PDXE_NAV_PREV_SIBLING, PDXE_NAV_NEXT_SIBLING,
                                  PDXE_NAV_PREV_NAMED_SIBLING, PDXE_NAV_NEXT_NAMED_SIBLING};
#undef ROOT_SIBLING
#define ROOT_SIBLING(n, which) pdxe_counted_sibling((n), SIB_TO_NAV[(which)])
#endif

/* A node's children, collected once by a lookup that needs them by position: a path
 * node's while it is on the path, a loop's node's while the loop runs. */
struct PDXEPathSiblings {
    const void *parent_id; /* whose they are; NULL: none collected */
    const void *tree;
    TSNode *kids;
    int32_t *prev_named; /* by position: the previous named child's position, -1 for none */
    int32_t *next_named; /* the next named child's, -1 for none */
    uint32_t count;
    uint32_t capacity;
};

/* Above this many children, which child holds a node is found in the collection. */
enum { PATH_WIDE = 16 };
/* Wide nodes off the path whose collections a walk keeps: a lookup below the walk's node
 * passes through at most one before reaching it. */
enum { PATH_RECENT = 4 };

static PDXE_TLS PDXEWalkPath *tl_walk_path;

static void siblings_free(PDXEPathSiblings *sib) {
    free(sib->kids);
    free(sib->prev_named);
    free(sib->next_named);
    memset(sib, 0, sizeof *sib);
}

/* Collects `parent`'s children into `sib`, unless it holds them already. False when they
 * cannot be had: lost work, counted. */
static bool siblings_collect(PDXEPathSiblings *sib, TSNode parent) {
    if (sib->parent_id == parent.id && sib->tree == parent.tree) {
        return true;
    }
    sib->parent_id = NULL;
    uint32_t count = ts_node_child_count(parent);
    if (children_refused()) {
        pdxe_lost_allocation();
        PDXE_LOOKUP_COUNT(PDXE_LC_NAV_LOST);
        return false;
    }
    if (count > sib->capacity) {
        TSNode *kids = (TSNode *)pdxe_counted_realloc(sib->kids, (size_t)count * sizeof *kids);
        if (kids) {
            sib->kids = kids;
        }
        int32_t *prev_named =
            (int32_t *)pdxe_counted_realloc(sib->prev_named, (size_t)count * sizeof *prev_named);
        if (prev_named) {
            sib->prev_named = prev_named;
        }
        int32_t *next_named =
            (int32_t *)pdxe_counted_realloc(sib->next_named, (size_t)count * sizeof *next_named);
        if (next_named) {
            sib->next_named = next_named;
        }
        if (!kids || !prev_named || !next_named) {
            PDXE_LOOKUP_COUNT(PDXE_LC_NAV_LOST);
            return false;
        }
        sib->capacity = count;
    }
    uint32_t n = 0;
    int32_t last_named = -1;
    PDXE_FOR_EACH_CHILD(it, parent) {
        sib->kids[n] = it.node;
        sib->prev_named[n] = last_named;
        if (ts_node_is_named(it.node)) {
            last_named = (int32_t)n;
        }
        n++;
    }
    int32_t following = -1;
    for (uint32_t i = n; i-- > 0;) {
        sib->next_named[i] = following;
        if (ts_node_is_named(sib->kids[i])) {
            following = (int32_t)i;
        }
    }
    sib->count = n;
    sib->parent_id = parent.id;
    sib->tree = parent.tree;
    return true;
}

void pdxe_walk_path_open(PDXEWalkPath *path) {
    path->outer = tl_walk_path;
    tl_walk_path = path;
}

void pdxe_walk_path_close(PDXEWalkPath *path) {
    if (tl_walk_path == path) {
        tl_walk_path = path->outer;
    }
    for (uint32_t d = 0; d < path->siblings_capacity; d++) {
        siblings_free(&path->siblings[d]);
    }
    free(path->siblings);
    free(path->entries);
    if (path->recent) {
        for (int r = 0; r < PATH_RECENT; r++) {
            siblings_free(&path->recent[r]);
        }
        free(path->recent);
    }
    path->recent = NULL;
    path->siblings = NULL;
    path->entries = NULL;
    path->siblings_capacity = 0;
    path->capacity = 0;
    path->depth = 0;
}

void pdxe_walk_path_at(PDXEWalkPath *path, uint32_t depth, TSNode node, uint8_t known,
                       TSNode prev, uint32_t index) {
    if (path->failed) {
        return;
    }
    /* A walk enters one level at a time: a deeper jump would leave stale entries. */
    if (path->capacity > 0 && depth > path->depth + 1) {
        path->failed = true;
        return;
    }
    if (depth >= path->capacity) {
        uint32_t capacity = path->capacity ? path->capacity * 2 : 64;
        while (capacity <= depth) {
            capacity *= 2;
        }
        PDXEPathEntry *grown =
            (PDXEPathEntry *)pdxe_counted_realloc(path->entries, (size_t)capacity * sizeof *grown);
        if (!grown) {
            path->failed = true;
            return;
        }
        path->entries = grown;
        path->capacity = capacity;
    }
    PDXEPathEntry *entry = &path->entries[depth];
    entry->node = node;
    entry->prev = prev;
    entry->index = index;
    entry->known = depth > 0 ? known : 0;
    path->depth = depth;
}

static bool same_node(TSNode a, TSNode b) {
    return a.id == b.id && a.tree == b.tree;
}

/* `node`'s depth on the path, -1 when it is not on it. */
static int path_find(PDXEWalkPath *path, TSNode node) {
    if (path->hint <= path->depth && same_node(path->entries[path->hint].node, node)) {
        return (int)path->hint;
    }
    for (int k = (int)path->depth; k >= 0; k--) {
        if (same_node(path->entries[k].node, node)) {
            path->hint = (uint32_t)k;
            return k;
        }
    }
    return -1;
}

/* Whether `outer` holds `node` by its bytes (a node of positive width it holds is its
 * descendant; an empty one at an edge may be a neighbour's, which the descent finds). */
static bool holds(TSNode outer, TSNode node) {
    return outer.tree == node.tree && !same_node(outer, node) &&
           ts_node_start_byte(outer) <= ts_node_start_byte(node) &&
           ts_node_end_byte(node) <= ts_node_end_byte(outer);
}

/* The children of the path node at `depth`, collected once; NULL when they cannot be
 * (lost work, counted). */
static PDXEPathSiblings *path_siblings(PDXEWalkPath *path, uint32_t depth) {
    if (depth >= path->siblings_capacity) {
        uint32_t capacity = path->siblings_capacity ? path->siblings_capacity * 2 : 16;
        while (capacity <= depth) {
            capacity *= 2;
        }
        PDXEPathSiblings *grown = (PDXEPathSiblings *)pdxe_counted_realloc(
            path->siblings, (size_t)capacity * sizeof *grown);
        if (!grown) {
            return NULL;
        }
        memset(grown + path->siblings_capacity, 0,
               (size_t)(capacity - path->siblings_capacity) * sizeof *grown);
        path->siblings = grown;
        path->siblings_capacity = capacity;
    }
    PDXEPathSiblings *sib = &path->siblings[depth];
    return siblings_collect(sib, path->entries[depth].node) ? sib : NULL;
}

/* The children of a loop's node, collected once while the loop runs; NULL when they
 * cannot be (lost work, counted). */
static PDXEPathSiblings *iter_siblings(PDXEChildIter *it) {
    if (!it->collected) {
        it->collected = (PDXEPathSiblings *)pdxe_counted_calloc(1, sizeof *it->collected);
        if (!it->collected) {
            return NULL;
        }
    }
    return siblings_collect(it->collected, it->parent) ? it->collected : NULL;
}

static bool iter_running(const PDXEChildIter *it) {
    return it->started && !it->finished && it->wrappers == 0;
}

/* The position of the first child that may hold or be a node starting at `start`: the
 * last child starting before it, then every child starting at it (an empty child shares
 * its start with the next). Children are in source order. */
static uint32_t siblings_seek(const PDXEPathSiblings *sib, uint32_t start) {
    uint32_t lo = 0, hi = sib->count;
    while (lo < hi) { /* the first child starting after `start` */
        uint32_t mid = lo + (hi - lo) / 2;
        if (ts_node_start_byte(sib->kids[mid]) <= start) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    while (lo > 0 && ts_node_start_byte(sib->kids[lo - 1]) == start) {
        lo--;
    }
    return lo > 0 ? lo - 1 : 0;
}

/* `node`'s position among the collected children, -1 when it is not one of them. */
static int32_t siblings_position(const PDXEPathSiblings *sib, TSNode node) {
    uint32_t start = ts_node_start_byte(node);
    for (uint32_t i = siblings_seek(sib, start);
         i < sib->count && ts_node_start_byte(sib->kids[i]) <= start; i++) {
        if (same_node(sib->kids[i], node)) {
            return (int32_t)i;
        }
    }
    return -1;
}

/* The collection of a wide node off the path, kept among the walk's few recent ones
 * (the least recently used given up); NULL when it cannot be had (lost work, counted) or
 * there is no walk. */
static PDXEPathSiblings *path_recent(PDXEWalkPath *path, TSNode node, bool *lost) {
    if (!path || path->failed) {
        return NULL;
    }
    if (!path->recent) {
        path->recent = (PDXEPathSiblings *)pdxe_counted_calloc(PATH_RECENT, sizeof *path->recent);
        if (!path->recent) {
            *lost = true;
            return NULL;
        }
    }
    int slot = 0;
    for (int r = 0; r < PATH_RECENT; r++) {
        if (path->recent[r].parent_id == node.id && path->recent[r].tree == node.tree) {
            slot = r;
            break;
        }
        if (path->recent_used[r] < path->recent_used[slot]) {
            slot = r;
        }
    }
    path->recent_used[slot] = ++path->recent_tick;
    if (!siblings_collect(&path->recent[slot], node)) {
        *lost = true;
        return NULL;
    }
    return &path->recent[slot];
}

/* The child of `at` holding `node` (or `node` itself), as ts_node_child_with_descendant
 * finds it; a wide node's is found in a collection. Null when `at` holds no such child. */
static TSNode child_holding(TSNode at, TSNode node, bool *lost) {
    if (ts_node_child_count(at) > PATH_WIDE) {
        PDXEPathSiblings *sib = path_recent(tl_walk_path, at, lost);
        if (*lost) {
            return (TSNode){0};
        }
        if (sib) {
            uint32_t start = ts_node_start_byte(node);
            uint32_t end = ts_node_end_byte(node);
            for (uint32_t i = siblings_seek(sib, start);
                 i < sib->count && ts_node_start_byte(sib->kids[i]) <= start; i++) {
                TSNode kid = sib->kids[i];
                if (same_node(kid, node)) {
                    return kid;
                }
                if (ts_node_end_byte(kid) < end) {
                    continue;
                }
                /* A node of positive width a child's bytes hold is its descendant; an
                 * empty one at a child's edge is asked of the child. */
                if (end > start ? ts_node_end_byte(kid) > start
                                : !ts_node_is_null(CHILD_WITH_DESCENDANT(kid, node))) {
                    return kid;
                }
            }
            return (TSNode){0};
        }
    }
    return CHILD_WITH_DESCENDANT(at, node);
}

/* The parent of `node`, a descendant of `from`, by descending from `from` as
 * ts_node_parent descends from the root. Null when `from` does not hold it, or when a
 * collection it needed could not be had (*lost). */
static TSNode descend_to_parent_lost(TSNode from, TSNode node, bool *lost) {
    TSNode at = from;
    for (;;) {
        TSNode next = child_holding(at, node, lost);
        if (ts_node_is_null(next)) {
            return next;
        }
        if (same_node(next, node)) {
            return at;
        }
        at = next;
    }
}

static TSNode descend_to_parent(TSNode from, TSNode node) {
    bool lost = false;
    return descend_to_parent_lost(from, node, &lost);
}

/* The parent of `node`, which `from` holds, `sib` being `from`'s collected children:
 * the child holding `node` is found among them, and the parent below it. */
static TSNode parent_in(TSNode from, const PDXEPathSiblings *sib, TSNode node) {
    uint32_t start = ts_node_start_byte(node);
    uint32_t end = ts_node_end_byte(node);
    for (uint32_t i = siblings_seek(sib, start);
         i < sib->count && ts_node_start_byte(sib->kids[i]) <= start; i++) {
        TSNode kid = sib->kids[i];
        if (same_node(kid, node)) {
            return from;
        }
        if (ts_node_end_byte(kid) >= end && ts_node_end_byte(kid) >= start) {
            TSNode found = descend_to_parent(kid, node);
            if (!ts_node_is_null(found)) {
                return found;
            }
        }
    }
    return (TSNode){0};
}

/* The parent of `node` below the path node at `depth`, which holds it: where that node
 * is wide, its child holding `node` is found in its collected children. */
static TSNode parent_below(PDXEWalkPath *path, uint32_t depth, TSNode node, bool *lost) {
    TSNode from = path->entries[depth].node;
    if (ts_node_child_count(from) <= PATH_WIDE) {
        return descend_to_parent(from, node);
    }
    PDXEPathSiblings *sib = path_siblings(path, depth);
    if (!sib) {
        *lost = true;
        return (TSNode){0};
    }
    return parent_in(from, sib, node);
}

/* The parent of `node` from the loops in progress: the current child's is the loop's
 * node; a node below the current child is found from it; any other child of the loop's
 * node, or a node below it, from the loop's collection. Null when no loop holds it. */
static TSNode iter_parent(TSNode node, bool *lost) {
    for (PDXEChildIter *it = tl_iters; it; it = it->outer) {
        if (iter_running(it) && same_node(it->node, node)) {
            PDXE_LOOKUP_COUNT(PDXE_LC_NAV_PARENT);
            return it->parent;
        }
    }
    for (PDXEChildIter *it = tl_iters; it; it = it->outer) {
        if (iter_running(it) && holds(it->node, node)) {
            PDXE_LOOKUP_COUNT(PDXE_LC_NAV_PARENT);
            TSNode found = descend_to_parent(it->node, node);
            if (!ts_node_is_null(found)) {
                return found;
            }
        }
    }
    for (PDXEChildIter *it = tl_iters; it; it = it->outer) {
        if (!iter_running(it) || !holds(it->parent, node)) {
            continue;
        }
        PDXE_LOOKUP_COUNT(PDXE_LC_NAV_PARENT);
        TSNode found;
        if (ts_node_child_count(it->parent) <= PATH_WIDE) {
            found = descend_to_parent(it->parent, node);
        } else {
            PDXEPathSiblings *sib = iter_siblings(it);
            if (!sib) {
                *lost = true;
                return (TSNode){0};
            }
            found = parent_in(it->parent, sib, node);
        }
        if (!ts_node_is_null(found)) {
            return found;
        }
    }
    return (TSNode){0};
}

TSNode pdxe_node_parent(TSNode node) {
    if (ts_node_is_null(node)) {
        return node;
    }
    PDXEWalkPath *path = tl_walk_path;
    int k = path && !path->failed && path->entries ? path_find(path, node) : -1;
    if (k > 0 && (path->entries[k].known & PDXE_PATH_PARENT)) {
        PDXE_LOOKUP_COUNT(PDXE_LC_NAV_PARENT);
        path->hint = (uint32_t)k - 1;
        return path->entries[k - 1].node;
    }
    bool lost = false;
    TSNode from_loop = iter_parent(node, &lost);
    if (!ts_node_is_null(from_loop) || lost) {
        return from_loop; /* a lost collection is lost work: never a positional fallback */
    }
    if (!path || path->failed || !path->entries) {
        return ROOT_PARENT(node);
    }
    for (int d = k > 0 ? k - 1 : (int)path->depth; k != 0 && d >= 0; d--) {
        if (!holds(path->entries[d].node, node)) {
            continue;
        }
        PDXE_LOOKUP_COUNT(PDXE_LC_NAV_PARENT);
        TSNode found = parent_below(path, (uint32_t)d, node, &lost);
        if (!ts_node_is_null(found) || lost) {
            return found;
        }
    }
    return ROOT_PARENT(node);
}

/*
 * tree-sitter's sibling lookups are positional. Among the parent's children (hidden
 * nodes stepped through, as the cursor does), the next sibling is the first after the
 * node whose end is past the node's end: an empty child at the node's end is passed
 * over. The previous sibling of a node of positive size is the child just before it.
 * The previous sibling of an empty node (a parser's missing one) depends on the hidden
 * nodes around it (ts_node__prev_sibling's trailing-empty-descendant rule), so it is
 * always tree-sitter's own lookup.
 */
static bool positive_size(TSNode node) {
    return ts_node_end_byte(node) > ts_node_start_byte(node);
}

/* A sibling of `node` among `parent`'s children, by stepping through them: for a parent
 * that is no anchor, whose children are local to the node asked about. */
static TSNode local_sibling(TSNode parent, TSNode node, int which) {
    TSNode before = {0}, named_before = {0};
    uint32_t end = ts_node_end_byte(node);
    bool seen = false;
    PDXE_FOR_EACH_CHILD(it, parent) {
        TSNode kid = it.node;
        if (seen) {
            if (ts_node_end_byte(kid) > end && (which == SIB_NEXT || ts_node_is_named(kid))) {
                return kid;
            }
            continue;
        }
        if (same_node(kid, node)) {
            if (which == SIB_PREV) {
                return before;
            }
            if (which == SIB_PREV_NAMED) {
                return named_before;
            }
            seen = true;
            continue;
        }
        before = kid;
        if (ts_node_is_named(kid)) {
            named_before = kid;
        }
    }
    return (TSNode){0};
}

/* The sibling of the child at `position` among the collected children. */
static TSNode siblings_answer(const PDXEPathSiblings *sib, int32_t position, TSNode node,
                              int which) {
    uint32_t end = ts_node_end_byte(node);
    int32_t at = -1;
    switch (which) {
    case SIB_PREV:
        at = position - 1;
        break;
    case SIB_PREV_NAMED:
        at = sib->prev_named[position];
        break;
    case SIB_NEXT:
        at = position + 1;
        while ((uint32_t)at < sib->count && ts_node_end_byte(sib->kids[at]) <= end) {
            at++;
        }
        if ((uint32_t)at >= sib->count) {
            at = -1;
        }
        break;
    default:
        at = sib->next_named[position];
        while (at >= 0 && ts_node_end_byte(sib->kids[at]) <= end) {
            at = sib->next_named[at];
        }
        break;
    }
    return at >= 0 ? sib->kids[at] : (TSNode){0};
}

/* `parent`'s collected children when it is an anchor (a loop's node, or a path node),
 * NULL when it is not; *lost when they cannot be had. */
static PDXEPathSiblings *anchor_siblings(TSNode parent, bool *lost) {
    for (PDXEChildIter *it = tl_iters; it; it = it->outer) {
        if (iter_running(it) && same_node(it->parent, parent)) {
            PDXEPathSiblings *sib = iter_siblings(it);
            *lost = !sib;
            return sib;
        }
    }
    PDXEWalkPath *path = tl_walk_path;
    int d = path && !path->failed && path->entries ? path_find(path, parent) : -1;
    if (d >= 0) {
        PDXEPathSiblings *sib = path_siblings(path, (uint32_t)d);
        *lost = !sib;
        return sib;
    }
    return NULL;
}

/* A sibling of `node` among its parent's children: from an anchor's place or collection,
 * else by stepping through the parent's children. */
static TSNode path_sibling(TSNode node, int which) {
    bool backwards = which == SIB_PREV || which == SIB_PREV_NAMED;
    if (ts_node_is_null(node) || (backwards && !positive_size(node))) {
        return ROOT_SIBLING(node, which);
    }
    PDXEWalkPath *path = tl_walk_path;
    int k = path && !path->failed && path->entries ? path_find(path, node) : -1;
    PDXEPathSiblings *sib = NULL;
    int32_t position = -1;
    TSNode parent = {0};
    bool lost = false;
    if (k > 0 && (path->entries[k].known & PDXE_PATH_PARENT)) {
        if ((path->entries[k].known & PDXE_PATH_PLACE) && which == SIB_PREV) {
            PDXE_LOOKUP_COUNT(PDXE_LC_NAV_SIBLING);
            return path->entries[k].prev;
        }
        parent = path->entries[k - 1].node;
        sib = path_siblings(path, (uint32_t)k - 1);
        lost = !sib;
        if (path->entries[k].known & PDXE_PATH_PLACE) {
            position = (int32_t)path->entries[k].index;
        }
    } else {
        PDXEChildIter *at = NULL;
        for (PDXEChildIter *it = tl_iters; it && !at; it = it->outer) {
            if (iter_running(it) && same_node(it->node, node)) {
                at = it;
            }
        }
        if (at) {
            if (which == SIB_PREV && !at->named) {
                PDXE_LOOKUP_COUNT(PDXE_LC_NAV_SIBLING);
                return at->index > 0 ? at->prev : (TSNode){0};
            }
            parent = at->parent;
            sib = iter_siblings(at);
            lost = !sib;
            if (!at->named) {
                position = (int32_t)at->index;
            }
        } else {
            parent = pdxe_node_parent(node);
            if (ts_node_is_null(parent)) {
                return parent;
            }
            sib = anchor_siblings(parent, &lost);
        }
    }
    PDXE_LOOKUP_COUNT(PDXE_LC_NAV_SIBLING);
    if (!sib && !lost && ts_node_child_count(parent) > PATH_WIDE) {
        sib = path_recent(tl_walk_path, parent, &lost);
    }
    if (lost) {
        return (TSNode){0}; /* lost work, counted: never a positional fallback */
    }
    if (!sib) {
        return local_sibling(parent, node, which);
    }
    if (position < 0 || (uint32_t)position >= sib->count || !same_node(sib->kids[position], node)) {
        position = siblings_position(sib, node);
    }
    if (position < 0) {
        return local_sibling(parent, node, which);
    }
    return siblings_answer(sib, position, node, which);
}

TSNode pdxe_node_prev_sibling(TSNode node) {
    return path_sibling(node, SIB_PREV);
}

TSNode pdxe_node_next_sibling(TSNode node) {
    return path_sibling(node, SIB_NEXT);
}

TSNode pdxe_node_prev_named_sibling(TSNode node) {
    return path_sibling(node, SIB_PREV_NAMED);
}

TSNode pdxe_node_next_named_sibling(TSNode node) {
    return path_sibling(node, SIB_NEXT_NAMED);
}

/* --- a whole subtree in source order ---------------------------------------- */

static void tree_walk_record(PDXETreeWalk *w) {
    PDXEWalkPath *path = &w->path;
    if (w->entered || w->depth > path->depth || path->failed) {
        pdxe_walk_path_at(path, w->depth, w->node, PDXE_PATH_PARENT | PDXE_PATH_PLACE, (TSNode){0}, 0);
    } else {
        PDXEPathEntry before = path->entries[w->depth];
        pdxe_walk_path_at(path, w->depth, w->node, PDXE_PATH_PARENT | PDXE_PATH_PLACE, before.node,
                          before.index + 1);
    }
}

bool pdxe_tree_walk_next(PDXETreeWalk *w) {
    if (w->finished) {
        return false;
    }
    if (!w->started) {
        w->started = true;
        if (ts_node_is_null(w->root)) {
            pdxe_tree_walk_finish(w);
            return false;
        }
        pdxe_cursor_acquire(&w->lease, -1, w->root);
        pdxe_walk_path_open(&w->path);
        w->node = w->root;
        w->depth = 0;
        w->entered = true;
        tree_walk_record(w);
        return true;
    }
    TSTreeCursor *cursor = w->lease.cursor;
    if (!w->skip && step_first(cursor)) {
        w->depth++;
        w->entered = true;
    } else {
        w->entered = false;
        bool moved = false;
        while (w->depth > 0) {
            if (step_next(cursor)) {
                moved = true;
                break;
            }
            step_up(cursor);
            w->depth--;
        }
        if (!moved) {
            pdxe_tree_walk_finish(w);
            return false;
        }
    }
    w->skip = false;
    w->node = ts_tree_cursor_current_node(cursor);
    tree_walk_record(w);
    return true;
}

void pdxe_tree_walk_skip_children(PDXETreeWalk *w) {
    w->skip = true;
}

void pdxe_tree_walk_finish(PDXETreeWalk *w) {
    if (w->started && !w->finished) {
        pdxe_walk_path_close(&w->path);
        pdxe_cursor_release(&w->lease);
    }
    w->finished = true;
}

/* --- test builds: navigation work, counted ------------------------------------ */

#if defined(PDXE_ENABLE_TEST_SEAMS)

/* The weighing cursor: its own, so weighing never disturbs a walk's. */
static PDXE_TLS TSTreeCursor nav_cursor;
static PDXE_TLS bool nav_cursor_live;

static void nav_release_thread(void) {
    if (nav_cursor_live) {
        ts_tree_cursor_delete(&nav_cursor);
        nav_cursor_live = false;
    }
}

/* The children passed on the way from `from` down to `target`: at each level, those
 * before the one holding it, and that one. What a positional descent costs where a
 * node's children are one flat run; an upper bound where they are a balanced tree. */
static uint64_t nav_descent_weight(TSNode from, TSNode target) {
    if (ts_node_is_null(from) || ts_node_is_null(target)) {
        return 1;
    }
    if (nav_cursor_live) {
        ts_tree_cursor_reset(&nav_cursor, from);
    } else {
        nav_cursor = ts_tree_cursor_new(from);
        nav_cursor_live = true;
    }
    uint32_t start = ts_node_start_byte(target);
    uint32_t end = ts_node_end_byte(target);
    uint64_t weight = 1;
    for (;;) {
        if (ts_tree_cursor_current_node(&nav_cursor).id == target.id ||
            !ts_tree_cursor_goto_first_child(&nav_cursor)) {
            return weight;
        }
        bool holds = false;
        do {
            weight++;
            TSNode child = ts_tree_cursor_current_node(&nav_cursor);
            uint32_t child_start = ts_node_start_byte(child);
            if (child_start > start) {
                break;
            }
            if (child.id == target.id || (start >= child_start && end <= ts_node_end_byte(child) &&
                                          (end > start || ts_node_end_byte(child) > start))) {
                holds = true;
                break;
            }
        } while (ts_tree_cursor_goto_next_sibling(&nav_cursor));
        if (!holds) {
            return weight;
        }
    }
}

TSNode pdxe_counted_child(TSNode node, uint32_t index) {
    PDXE_LOOKUP_COUNT_N(PDXE_LC_NAV_INDEXED, (uint64_t)index + 1);
    return ts_node_child(node, index);
}

TSNode pdxe_counted_named_child(TSNode node, uint32_t index) {
    PDXE_LOOKUP_COUNT_N(PDXE_LC_NAV_INDEXED, (uint64_t)index + 1);
    return ts_node_named_child(node, index);
}

const char *pdxe_counted_field_name_for_child(TSNode node, uint32_t index) {
    PDXE_LOOKUP_COUNT_N(PDXE_LC_NAV_INDEXED, (uint64_t)index + 1);
    return ts_node_field_name_for_child(node, index);
}

TSNode pdxe_counted_parent(TSNode node) {
    if (!ts_node_is_null(node)) {
        PDXE_LOOKUP_COUNT_N(PDXE_LC_NAV_PARENT,
                            nav_descent_weight(ts_tree_root_node(node.tree), node));
    }
    return ts_node_parent(node);
}

TSNode pdxe_counted_sibling(TSNode node, int which) {
    if (!ts_node_is_null(node)) {
        PDXE_LOOKUP_COUNT_N(PDXE_LC_NAV_SIBLING,
                            nav_descent_weight(ts_tree_root_node(node.tree), node));
    }
    switch (which) {
    case PDXE_NAV_NEXT_SIBLING:
        return ts_node_next_sibling(node);
    case PDXE_NAV_PREV_SIBLING:
        return ts_node_prev_sibling(node);
    case PDXE_NAV_NEXT_NAMED_SIBLING:
        return ts_node_next_named_sibling(node);
    default:
        return ts_node_prev_named_sibling(node);
    }
}

TSNode pdxe_counted_child_by_field_id(TSNode node, TSFieldId field) {
    PDXE_LOOKUP_COUNT(PDXE_LC_NAV_FIELD);
    return ts_node_child_by_field_id(node, field);
}

TSNode pdxe_counted_child_with_descendant(TSNode node, TSNode descendant) {
    TSNode child = ts_node_child_with_descendant(node, descendant);
    /* The children passed at this one level. */
    PDXE_LOOKUP_COUNT_N(PDXE_LC_NAV_PARENT, ts_node_is_null(child) ? ts_node_child_count(node) + 1
                                                                    : nav_descent_weight(node, child));
    return child;
}

TSNode pdxe_counted_descendant_for_byte_range(TSNode node, uint32_t start, uint32_t end) {
    TSNode found = ts_node_descendant_for_byte_range(node, start, end);
    PDXE_LOOKUP_COUNT_N(PDXE_LC_NAV_PARENT, nav_descent_weight(node, found));
    return found;
}

bool pdxe_counted_cursor_step(TSTreeCursor *cursor, int which) {
    switch (which) {
    case PDXE_NAV_FIRST_CHILD:
        PDXE_LOOKUP_COUNT(PDXE_LC_NAV_CURSOR);
        return ts_tree_cursor_goto_first_child(cursor);
    case PDXE_NAV_NEXT:
        PDXE_LOOKUP_COUNT(PDXE_LC_NAV_CURSOR);
        return ts_tree_cursor_goto_next_sibling(cursor);
    case PDXE_NAV_UP:
        PDXE_LOOKUP_COUNT(PDXE_LC_NAV_CURSOR);
        return ts_tree_cursor_goto_parent(cursor);
    case PDXE_NAV_LAST_CHILD: {
        /* To the last child, a cursor passes every child. */
        TSNode at = ts_tree_cursor_current_node(cursor);
        PDXE_LOOKUP_COUNT_N(PDXE_LC_NAV_SIBLING, 1 + (uint64_t)ts_node_child_count(at));
        return ts_tree_cursor_goto_last_child(cursor);
    }
    default: {
        /* Backwards, a cursor may count its position again from the parent's first
         * child: weighed as the parent's children. */
        TSNode parent = ts_node_parent(ts_tree_cursor_current_node(cursor));
        PDXE_LOOKUP_COUNT_N(PDXE_LC_NAV_SIBLING,
                            1 + (ts_node_is_null(parent) ? 0 : (uint64_t)ts_node_child_count(parent)));
        return ts_tree_cursor_goto_previous_sibling(cursor);
    }
    }
}

#endif /* PDXE_ENABLE_TEST_SEAMS */
