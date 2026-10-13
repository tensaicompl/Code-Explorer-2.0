/*
 * node_iter.h — the engine's one way to step through a node's children (issue 93).
 *
 * tree-sitter's positional lookups cost the position. ts_node_child(node, i) and
 * ts_node_named_child(node, i) walk the node's children from the first every call;
 * ts_node_parent descends from the tree's root, passing every earlier child of every
 * ancestor; the sibling lookups ask for the parent and then walk its children. A list
 * a grammar's repetition builds is a balanced tree of hidden nodes, so there each costs
 * the logarithm of the position. A node the parser's recovery builds (an error node
 * holding the rest of a file) is one flat run of children, so there each costs the
 * position itself, and a pass that steps through such a node by index, or asks for a
 * parent or a sibling within it, is quadratic in the file.
 *
 * Children are therefore visited by cursor, in source order, each step a constant:
 *
 *     PDXE_FOR_EACH_CHILD(it, node) {          // every child, as ts_node_child
 *         TSNode child = it.node;              // it.index is its position
 *         ...
 *     }
 *     PDXE_FOR_EACH_NAMED_CHILD(it, node) { ... }   // as ts_node_named_child
 *
 * A loop in progress is also an anchor for the parent and sibling lookups below
 * (pdxe_node_parent and the rest): its current child's parent is its node and its
 * previous child the one before, its node's other children are found in a collection
 * made once when a lookup needs it, and a node below them is found from them, so a walk
 * whose recursion steps through children this way has its whole chain of ancestors at
 * hand without registering a path.
 *
 * The children visited, and their order, are exactly those of ts_node_child(node, i)
 * for i below ts_node_child_count(node) (ts_node_named_child and its count for the named
 * form), anonymous wrappers holding named children included. The loop may be left by
 * break, return or goto: the cursor is given back when the loop's variable goes out of
 * scope. A cursor that runs out before the count, which a consistent tree never does,
 * ends the loop there and is counted as lost work in test builds; nothing is ever
 * looked up by position in its place.
 *
 * Where a pass needs its children in reverse, or by position, it collects them once
 * (pdxe_collect_children) and indexes the collection. A collection that cannot get
 * its memory returns nothing: what the pass would have done with those children is
 * lost work, and the pass never falls back to positional lookups.
 *
 * Test builds (PDXE_ENABLE_TEST_SEAMS) count navigation work by kind
 * (lookup_counts.h): a child reached by index, weighted by its index + 1; a cursor
 * step; a parent or sibling lookup, weighted by the children passed on the way from the
 * root (or the walk's own frame) down to the node; and a field lookup. Release builds
 * count nothing.
 */
#ifndef PDXE_NODE_ITER_H
#define PDXE_NODE_ITER_H

#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#include "arena.h"
#include "lookup_counts.h"
#include "tree_sitter/api.h"

/* A cursor leased from this thread's pool (pdxe_cursor_acquire). */
typedef struct {
    TSTreeCursor *cursor;
    TSTreeCursor private_cursor;
    int slot; /* -1: private */
} pdxe_cursor_lease_t;

/* Reusable cursors for walks: a lease is exclusive until released, so a walk that
 * recurses, or a loop nested in another, never shares a cursor with its callers.
 * `depth` is the slot a recursive walk prefers (its recursion depth), or -1 for any:
 * a slot already in use hands out another free one, and only when every slot is in
 * use is a private cursor made (release deletes it). Released with the thread
 * parser. */
TSTreeCursor *pdxe_cursor_acquire(pdxe_cursor_lease_t *lease, int depth, TSNode node);
void pdxe_cursor_release(pdxe_cursor_lease_t *lease);
/* Deletes this thread's cursors (pdxe_destroy_thread_parser). */
void pdxe_node_iter_release_thread(void);

/* A node's children, collected once by a lookup that needs them by position. */
typedef struct PDXEPathSiblings PDXEPathSiblings;

/* Named iteration enters anonymous wrappers that hold named children, as
 * ts_node_named_child does; this deep at most. */
enum { PDXE_CHILD_ITER_WRAPPERS = 8 };

typedef struct PDXEChildIter {
    TSNode parent;  /* the node whose children are visited */
    TSNode node;    /* the current child, once pdxe_child_iter_next has returned true */
    uint32_t index; /* the current child's position among those visited */
    uint32_t limit; /* the children to visit: the node's child count, or named child count */
    bool named;
    bool started;
    bool finished;
    uint8_t wrappers;                                /* anonymous wrappers entered */
    uint32_t wrapper_left[PDXE_CHILD_ITER_WRAPPERS]; /* named children each still owes */
    pdxe_cursor_lease_t lease;
    struct PDXEChildIter *outer; /* the loop this one runs inside, while it runs */
    TSNode prev;                 /* the child before the current one (every-child loops) */
    struct PDXEPathSiblings *collected; /* the node's children, once a lookup needs them */
} PDXEChildIter;

static inline PDXEChildIter pdxe_child_iter_begin(TSNode parent, bool named) {
    PDXEChildIter it;
    it.parent = parent;
    it.node = (TSNode){0};
    it.index = 0;
    it.limit = ts_node_is_null(parent)
                   ? 0
                   : (named ? ts_node_named_child_count(parent) : ts_node_child_count(parent));
    it.named = named;
    it.started = false;
    it.finished = false;
    it.wrappers = 0;
    it.lease.cursor = NULL;
    it.lease.slot = -1;
    it.outer = NULL;
    it.prev = (TSNode){0};
    it.collected = NULL;
    return it;
}

/* Steps to the next child: true with it->node and it->index set, false once every
 * child has been visited (the cursor is then given back). */
bool pdxe_child_iter_next(PDXEChildIter *it);
/* Gives the cursor back; idempotent. The loops below call it as their variable goes
 * out of scope. */
void pdxe_child_iter_finish(PDXEChildIter *it);
/* The current child's field in the parent, as ts_node_field_name_for_child gives it for
 * that child (the cursor's own field name: a constant, where the lookup by position costs
 * the position); NULL for none. */
const char *pdxe_child_iter_field_name(const PDXEChildIter *it);

#define PDXE_CHILD_ITER_SCOPED __attribute__((cleanup(pdxe_child_iter_finish)))

#define PDXE_FOR_EACH_CHILD(it, parent_node)                                                       \
    for (PDXEChildIter it PDXE_CHILD_ITER_SCOPED = pdxe_child_iter_begin((parent_node), false);     \
         pdxe_child_iter_next(&it);)

#define PDXE_FOR_EACH_NAMED_CHILD(it, parent_node)                                                 \
    for (PDXEChildIter it PDXE_CHILD_ITER_SCOPED = pdxe_child_iter_begin((parent_node), true);      \
         pdxe_child_iter_next(&it);)

/* Collects `node`'s children (its named children when `named`) into an array cut from
 * `arena`, in source order, by one cursor pass, for a pass that needs them in reverse
 * or by position. *out_n is set to how many were collected. NULL and *out_n 0 for a
 * node without children, and when the array cannot be had: what the caller would have
 * done with them is then lost work (counted in test builds), never done by position. */
TSNode *pdxe_collect_children(PDXEArena *arena, TSNode node, bool named, uint32_t *out_n);

/* --- a walk's path: parents and siblings without descending from the root ------ */

/*
 * ts_node_parent descends from the root, and the sibling lookups ask for the parent, so
 * inside a wide node each costs the position. A walk registers the path it is on (the
 * node it is at and its ancestors, by depth) and the lookups below start from the
 * nearest node on that path that holds theirs: a node on the path has its parent and
 * its previous sibling an index away, and any other node is found below the deepest
 * path node holding it. Their answers are tree-sitter's (ts_node_parent,
 * ts_node_prev_sibling and the rest); with no path registered, or for a node no path
 * node holds, they are tree-sitter's own lookups.
 *
 * Where a lookup needs the children of a path node by position (a sibling other than
 * the previous one, or which child holds a node), they are collected once while the node
 * is on the path. A collection that cannot get its memory is lost work: the lookup
 * answers that there is no such node, and never steps through the children by position.
 */
enum {
    PDXE_PATH_PARENT = 1, /* the entry below is the node's parent */
    PDXE_PATH_PLACE = 2,  /* `prev` and `index` are the node's among its parent's children */
};

typedef struct {
    TSNode node;
    TSNode prev;    /* the previous sibling (null for the first), with PDXE_PATH_PLACE */
    uint32_t index; /* the position among the parent's children, with PDXE_PATH_PLACE */
    uint8_t known;
} PDXEPathEntry;

typedef struct PDXEWalkPath {
    PDXEPathEntry *entries;     /* by depth, 0 the walk's root; set up to `depth` */
    uint32_t depth;
    uint32_t capacity;
    bool failed;                /* could not grow: lookups descend from the root */
    PDXEPathSiblings *siblings; /* by depth: entries[d].node's children, once asked */
    uint32_t siblings_capacity;
    PDXEPathSiblings *recent; /* wide nodes off the path a lookup passed through, lately */
    uint64_t recent_used[4];
    uint64_t recent_tick;
    uint32_t hint; /* where the last lookup found its node */
    struct PDXEWalkPath *outer;
} PDXEWalkPath;

/* Registers `path` (zeroed by the caller) as this thread's until it is closed; a walk
 * run inside another's registers over it and gives it back. */
void pdxe_walk_path_open(PDXEWalkPath *path);
void pdxe_walk_path_close(PDXEWalkPath *path);
/* The walk is at `node`, at `depth`: entries above it are the nodes it has left. */
void pdxe_walk_path_at(PDXEWalkPath *path, uint32_t depth, TSNode node, uint8_t known,
                       TSNode prev, uint32_t index);

TSNode pdxe_node_parent(TSNode node);
TSNode pdxe_node_prev_sibling(TSNode node);
TSNode pdxe_node_next_sibling(TSNode node);
TSNode pdxe_node_prev_named_sibling(TSNode node);
TSNode pdxe_node_next_named_sibling(TSNode node);

/* --- a whole subtree in source order ---------------------------------------- */

/*
 * Every node of `root`'s subtree, `root` first, each before its children, by one cursor,
 * the walk registered as this thread's path while it runs:
 *
 *     PDXE_TREE_WALK(w, root) {
 *         TSNode node = w.node;                  // w.depth is its depth below root
 *         if (...) pdxe_tree_walk_skip_children(&w);
 *     }
 *
 * The order is that of a stack walk pushing each node's children in reverse. Leaving the
 * loop early gives the cursor and the path back, as PDXE_FOR_EACH_CHILD does.
 */
typedef struct {
    TSNode root;
    TSNode node;    /* the current node, once pdxe_tree_walk_next has returned true */
    uint32_t depth; /* its depth below root */
    bool started;
    bool finished;
    bool skip;    /* the current node's children are not to be visited */
    bool entered; /* the walk came to the current node from its parent */
    pdxe_cursor_lease_t lease;
    PDXEWalkPath path;
} PDXETreeWalk;

static inline PDXETreeWalk pdxe_tree_walk_begin(TSNode root) {
    PDXETreeWalk w;
    memset(&w, 0, sizeof w);
    w.root = root;
    w.lease.slot = -1;
    return w;
}

bool pdxe_tree_walk_next(PDXETreeWalk *w);
void pdxe_tree_walk_skip_children(PDXETreeWalk *w);
void pdxe_tree_walk_finish(PDXETreeWalk *w);

#define PDXE_TREE_WALK(w, root_node)                                                               \
    for (PDXETreeWalk w __attribute__((cleanup(pdxe_tree_walk_finish))) =                         \
             pdxe_tree_walk_begin(root_node);                                                      \
         pdxe_tree_walk_next(&w);)

/* --- test builds: navigation work, counted ------------------------------------ */

#if defined(PDXE_ENABLE_TEST_SEAMS)
enum { /* pdxe_counted_sibling */
    PDXE_NAV_NEXT_SIBLING,
    PDXE_NAV_PREV_SIBLING,
    PDXE_NAV_NEXT_NAMED_SIBLING,
    PDXE_NAV_PREV_NAMED_SIBLING,
};
enum { /* pdxe_counted_cursor_step */
    PDXE_NAV_FIRST_CHILD,
    PDXE_NAV_NEXT,
    PDXE_NAV_PREVIOUS,
    PDXE_NAV_UP,
    PDXE_NAV_LAST_CHILD,
};
TSNode pdxe_counted_child(TSNode node, uint32_t index);
TSNode pdxe_counted_named_child(TSNode node, uint32_t index);
const char *pdxe_counted_field_name_for_child(TSNode node, uint32_t index);
TSNode pdxe_counted_parent(TSNode node);
TSNode pdxe_counted_sibling(TSNode node, int which);
TSNode pdxe_counted_child_by_field_id(TSNode node, TSFieldId field);
TSNode pdxe_counted_child_with_descendant(TSNode node, TSNode descendant);
TSNode pdxe_counted_descendant_for_byte_range(TSNode node, uint32_t start, uint32_t end);
bool pdxe_counted_cursor_step(TSTreeCursor *cursor, int which);
#endif

#if defined(PDXE_ENABLE_TEST_SEAMS) && !defined(PDXE_NODE_ITER_UNCOUNTED)

#define ts_node_child(node, index) pdxe_counted_child((node), (index))
#define ts_node_named_child(node, index) pdxe_counted_named_child((node), (index))
#define ts_node_parent(node) pdxe_counted_parent(node)
#define ts_node_next_sibling(node) pdxe_counted_sibling((node), PDXE_NAV_NEXT_SIBLING)
#define ts_node_prev_sibling(node) pdxe_counted_sibling((node), PDXE_NAV_PREV_SIBLING)
#define ts_node_next_named_sibling(node) pdxe_counted_sibling((node), PDXE_NAV_NEXT_NAMED_SIBLING)
#define ts_node_prev_named_sibling(node) pdxe_counted_sibling((node), PDXE_NAV_PREV_NAMED_SIBLING)
#define ts_node_field_name_for_child(node, index) pdxe_counted_field_name_for_child((node), (index))
#define ts_node_child_by_field_id(node, field) pdxe_counted_child_by_field_id((node), (field))
#define ts_node_descendant_for_byte_range(node, start, end)                                        \
    pdxe_counted_descendant_for_byte_range((node), (start), (end))
#define ts_node_child_with_descendant(node, descendant)                                            \
    pdxe_counted_child_with_descendant((node), (descendant))
#define ts_tree_cursor_goto_first_child(cursor) pdxe_counted_cursor_step((cursor), PDXE_NAV_FIRST_CHILD)
#define ts_tree_cursor_goto_next_sibling(cursor) pdxe_counted_cursor_step((cursor), PDXE_NAV_NEXT)
#define ts_tree_cursor_goto_previous_sibling(cursor)                                               \
    pdxe_counted_cursor_step((cursor), PDXE_NAV_PREVIOUS)
#define ts_tree_cursor_goto_parent(cursor) pdxe_counted_cursor_step((cursor), PDXE_NAV_UP)
#define ts_tree_cursor_goto_last_child(cursor) pdxe_counted_cursor_step((cursor), PDXE_NAV_LAST_CHILD)
#endif

#endif /* PDXE_NODE_ITER_H */
