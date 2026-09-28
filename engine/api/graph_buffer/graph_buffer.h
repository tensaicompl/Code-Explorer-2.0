/*
 * The graph store the resolution sources expect, served from our own registry.
 *
 * The reference builds a graph in memory as it indexes, and its cross-file
 * resolution reads that graph to find definitions and to walk a file's imports.
 * This project builds no such graph inside the engine: definitions arrive through
 * the project interface and the graph is assembled afterwards, in Rust.
 *
 * So the store is not vendored. This header declares the same types and the same
 * lookups, and the shim answers them from the definitions the caller added, which
 * lets the resolution sources compile and run unchanged.
 *
 * Identifiers are assigned in the order nodes are first added, so they are stable
 * within one project and meaningless outside it. The resolution sources only ever
 * compare them for equality.
 */

#ifndef PDXE_GRAPH_BUFFER_H
#define PDXE_GRAPH_BUFFER_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque handle to the registry the shim keeps. */
typedef struct pdxe_gbuf_t pdxe_gbuf_t;

/*
 * One definition, in the shape the resolution sources read. Only the fields they
 * use are carried; a field they never read would be a field the shim has to keep
 * correct for no reason.
 */
typedef struct {
    int64_t id;
    const char *label;           /* the engine's kind string */
    const char *name;            /* short name */
    const char *qualified_name;  /* fully qualified name */
    const char *file_path;       /* repository-relative path of the defining file */
    int start_line;              /* first line, 1-based; 0 for directories and files */
    int end_line;                /* last line */
    const char *properties_json; /* extras, or NULL */
} pdxe_gbuf_node_t;

/* One relationship. Only imports are ever asked for. */
typedef struct {
    int64_t source_id;
    int64_t target_id;
    const char *type;
    const char *target_qualified_name;
    const char *properties_json; /* extras, or NULL: import edges carry the local name */
} pdxe_gbuf_edge_t;

/* The node with that identifier, or NULL. */
const pdxe_gbuf_node_t *pdxe_gbuf_find_by_id(const pdxe_gbuf_t *gb, int64_t id);

/*
 * Every node with that short name; a NULL name is looked up as the empty one. The
 * order is the reference's: the order nodes were added, except that a node an
 * update renamed leaves its old list by swapping the last entry into its place and
 * joins the end of its new one. Returns 0; `hits` points at storage the registry
 * owns.
 */
int pdxe_gbuf_find_by_name(const pdxe_gbuf_t *gb, const char *short_name,
                           const pdxe_gbuf_node_t ***hits, int *hit_count);

/*
 * The node with that qualified name, or NULL when there is none. There is never more
 * than one: adding a second node under a name updates the first, as the reference's
 * store does (see pdxe_gbuf_upsert_node).
 */
const pdxe_gbuf_node_t *pdxe_gbuf_find_by_qn(const pdxe_gbuf_t *gb, const char *qn);

/*
 * Relationships of one type leaving one node. Asked for only with "IMPORTS", and
 * served from the imports of that file's extraction result.
 */
int pdxe_gbuf_find_edges_by_source_type(const pdxe_gbuf_t *gb, int64_t source_id,
                                        const char *type, const pdxe_gbuf_edge_t ***edges,
                                        int *edge_count);

#ifdef __cplusplus
}
#endif

#endif /* PDXE_GRAPH_BUFFER_H */
