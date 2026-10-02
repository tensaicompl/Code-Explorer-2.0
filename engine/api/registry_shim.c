/*
 * The registry the resolution sources expect, served from our own storage.
 *
 * The vendored cross-file resolution reads a graph store the reference builds as it
 * indexes. This project builds no such store inside the engine, so the lookups it
 * calls are answered here, from the definitions and imports the interface layer adds
 * for each file before resolution runs.
 *
 * Every function below reproduces the reference's contract, not merely its
 * signature. A shim that compiled against the right declarations but answered with
 * subtly different semantics would link cleanly and degrade resolution quietly, and
 * nothing would notice until accuracy was measured. So each one says which contract
 * it keeps, and where the reference's behaviour depends on a subsystem that does not
 * exist here, which answer that subsystem's absence implies.
 */

#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "pdxe_core.h"
#include "foundation/hash_table.h"
#include "pipeline/pipeline_internal.h"
#include "pipeline/worker_pool.h"
#include "shim_internal.h"
#include "discover/userconfig.h"
#include "lost_work.h" /* a failed allocation here loses work: counted */

/* --- storage ---------------------------------------------------------------- */

/* A growable array of pointers. The pointed-to records never move. */
typedef struct {
    const void **items;
    int count;
    int cap;
} ptr_list;

static int ptr_list_push(ptr_list *l, const void *item) {
    if (l->count == l->cap) {
        int cap = l->cap ? l->cap * 2 : 4;
        const void **grown = (const void **)pdxe_counted_realloc((void *)l->items, (size_t)cap * sizeof(*grown));
        if (!grown) {
            return -1;
        }
        l->items = grown;
        l->cap = cap;
    }
    l->items[l->count++] = item;
    return 0;
}

struct pdxe_gbuf_t {
    /*
     * Each node is allocated on its own, so a pointer handed to a caller stays valid
     * however many nodes are added afterwards. An array of structures would move on
     * growth and invalidate every pointer the resolution sources were holding.
     */
    pdxe_gbuf_node_t **nodes;
    uint32_t n_nodes;
    uint32_t cap_nodes;

    pdxe_gbuf_edge_t **edges;
    uint32_t n_edges;
    uint32_t cap_edges;

    PDXEHashTable *by_qn;          /* qualified name -> its one node */
    PDXEHashTable *by_name;        /* short name -> ptr_list of nodes, see find_by_name */
    PDXEHashTable *by_source_type; /* "<id>\x1f<type>" -> ptr_list of edges */
    PDXEHashTable *by_edge_key;    /* an edge's identity -> the edge, see edge_key */

    /* Keys the tables above borrow, owned here for the registry's lifetime. */
    ptr_list owned_keys;
};

static char *dup_or_null(const char *s) {
    return s ? pdxe_counted_strdup(s) : NULL;
}

static bool dup_failed(const char *in, const char *out) {
    return in != NULL && out == NULL;
}

pdxe_gbuf_t *pdxe_gbuf_new(void) {
    pdxe_gbuf_t *gb = (pdxe_gbuf_t *)pdxe_counted_calloc(1, sizeof(*gb));
    if (!gb) {
        return NULL;
    }
    gb->by_qn = pdxe_ht_create(256);
    gb->by_name = pdxe_ht_create(256);
    gb->by_source_type = pdxe_ht_create(64);
    gb->by_edge_key = pdxe_ht_create(64);
    if (!gb->by_qn || !gb->by_name || !gb->by_source_type || !gb->by_edge_key) {
        pdxe_gbuf_free(gb);
        return NULL;
    }
    return gb;
}

static void free_list_value(const char *key, void *value, void *userdata) {
    (void)key;
    (void)userdata;
    ptr_list *l = (ptr_list *)value;
    if (l) {
        free((void *)l->items);
        free(l);
    }
}

void pdxe_gbuf_free(pdxe_gbuf_t *gb) {
    if (!gb) {
        return;
    }
    for (uint32_t i = 0; i < gb->n_nodes; i++) {
        pdxe_gbuf_node_t *n = gb->nodes[i];
        free((void *)n->label);
        free((void *)n->name);
        free((void *)n->qualified_name);
        free((void *)n->file_path);
        free((void *)n->properties_json);
        free(n);
    }
    free(gb->nodes);
    for (uint32_t i = 0; i < gb->n_edges; i++) {
        pdxe_gbuf_edge_t *e = gb->edges[i];
        free((void *)e->type);
        free((void *)e->target_qualified_name);
        free((void *)e->properties_json);
        free(e);
    }
    free(gb->edges);
    if (gb->by_name) {
        pdxe_ht_foreach(gb->by_name, free_list_value, NULL);
        pdxe_ht_free(gb->by_name);
    }
    if (gb->by_source_type) {
        pdxe_ht_foreach(gb->by_source_type, free_list_value, NULL);
        pdxe_ht_free(gb->by_source_type);
    }
    if (gb->by_qn) {
        pdxe_ht_free(gb->by_qn);
    }
    if (gb->by_edge_key) {
        pdxe_ht_free(gb->by_edge_key);
    }
    for (int i = 0; i < gb->owned_keys.count; i++) {
        free((void *)gb->owned_keys.items[i]);
    }
    free((void *)gb->owned_keys.items);
    free(gb);
}

/* A key the registry keeps for as long as it lives, so a table may borrow it. */
static const char *own_key(pdxe_gbuf_t *gb, const char *key) {
    char *copy = pdxe_counted_strdup(key);
    if (!copy || ptr_list_push(&gb->owned_keys, copy) != 0) {
        free(copy);
        return NULL;
    }
    return copy;
}

/* The list stored under a key, created on first use. */
static ptr_list *list_for(pdxe_gbuf_t *gb, PDXEHashTable *table, const char *key) {
    ptr_list *l = (ptr_list *)pdxe_ht_get(table, key);
    if (l) {
        return l;
    }
    const char *owned = own_key(gb, key);
    if (!owned) {
        return NULL;
    }
    l = (ptr_list *)pdxe_counted_calloc(1, sizeof(*l));
    if (!l) {
        return NULL;
    }
    pdxe_ht_set(table, owned, l);
    return l;
}

/* The short-name list a node belongs on. A node without a name is listed under "". */
static ptr_list *name_list(pdxe_gbuf_t *gb, const char *name) {
    return list_for(gb, gb->by_name, name ? name : "");
}

/*
 * Takes a node off a list the way the reference does: the last entry moves into its
 * place. The order this leaves is the order the reference's lookups return, so it
 * is kept rather than tidied.
 */
static void swap_remove(ptr_list *l, const pdxe_gbuf_node_t *n) {
    if (!l) {
        return;
    }
    for (int i = 0; i < l->count; i++) {
        if (l->items[i] == n) {
            l->items[i] = l->items[--l->count];
            return;
        }
    }
}

static int str_or_empty_cmp(const char *a, const char *b) {
    return strcmp(a ? a : "", b ? b : "");
}

/* Replaces a string field with a copy of `value`. Returns false if the copy failed. */
static bool replace_str(const char **field, const char *value) {
    char *copy = dup_or_null(value);
    if (dup_failed(value, copy)) {
        return false;
    }
    free((void *)*field);
    *field = copy;
    return true;
}

static int64_t update_node(pdxe_gbuf_t *gb, pdxe_gbuf_node_t *existing, const char *label,
                           const char *name, const char *file_path, int start_line, int end_line,
                           const char *properties_json) {
    /* A package's module never takes over the directory that shares its name. */
    if (existing->label && label && strcmp(label, "Module") == 0 &&
        (strcmp(existing->label, "Project") == 0 || strcmp(existing->label, "Folder") == 0)) {
        return existing->id;
    }
    int c = str_or_empty_cmp(file_path, existing->file_path);
    if (c == 0) {
        c = existing->start_line - start_line;
    }
    if (c == 0) {
        c = str_or_empty_cmp(existing->name, name);
    }
    if (c == 0) {
        c = str_or_empty_cmp(existing->label, label);
    }
    if (c > 0) {
        return existing->id; /* the existing node is the one that survives */
    }

    bool name_changed = !existing->name || !name || strcmp(existing->name, name) != 0;
    if (name_changed) {
        swap_remove((ptr_list *)pdxe_ht_get(gb->by_name, existing->name ? existing->name : ""),
                    existing);
    }
    if (!replace_str(&existing->label, label) || !replace_str(&existing->name, name) ||
        !replace_str(&existing->file_path, file_path)) {
        return 0;
    }
    existing->start_line = start_line;
    existing->end_line = end_line;
    if (properties_json && !replace_str(&existing->properties_json, properties_json)) {
        return 0;
    }
    if (name_changed) {
        ptr_list *l = name_list(gb, existing->name);
        if (!l || ptr_list_push(l, existing) != 0) {
            return 0;
        }
    }
    return existing->id;
}

int64_t pdxe_gbuf_upsert_node(pdxe_gbuf_t *gb, const char *label, const char *name,
                              const char *qualified_name, const char *file_path, int start_line,
                              int end_line, const char *properties_json) {
    if (!gb || !qualified_name) {
        return 0;
    }
    pdxe_gbuf_node_t *existing = (pdxe_gbuf_node_t *)pdxe_ht_get(gb->by_qn, qualified_name);
    if (existing) {
        return update_node(gb, existing, label, name, file_path, start_line, end_line,
                           properties_json);
    }

    if (gb->n_nodes == gb->cap_nodes) {
        uint32_t cap = gb->cap_nodes ? gb->cap_nodes * 2 : 64;
        pdxe_gbuf_node_t **grown =
            (pdxe_gbuf_node_t **)pdxe_counted_realloc(gb->nodes, (size_t)cap * sizeof(*grown));
        if (!grown) {
            return 0;
        }
        gb->nodes = grown;
        gb->cap_nodes = cap;
    }
    pdxe_gbuf_node_t *n = (pdxe_gbuf_node_t *)pdxe_counted_calloc(1, sizeof(*n));
    if (!n) {
        return 0;
    }
    n->label = dup_or_null(label);
    n->name = dup_or_null(name);
    n->qualified_name = pdxe_counted_strdup(qualified_name);
    n->file_path = dup_or_null(file_path);
    n->properties_json = dup_or_null(properties_json);
    if (dup_failed(label, n->label) || dup_failed(name, n->name) || !n->qualified_name ||
        dup_failed(file_path, n->file_path) || dup_failed(properties_json, n->properties_json)) {
        free((void *)n->label);
        free((void *)n->name);
        free((void *)n->qualified_name);
        free((void *)n->file_path);
        free((void *)n->properties_json);
        free(n);
        return 0;
    }
    n->start_line = start_line;
    n->end_line = end_line;

    /* Identifiers start at 1 and follow the order nodes first arrive, so 0 means none. */
    n->id = (int64_t)gb->n_nodes + 1;
    gb->nodes[gb->n_nodes++] = n;
    pdxe_ht_set(gb->by_qn, n->qualified_name, n);
    ptr_list *l = name_list(gb, n->name);
    if (!l || ptr_list_push(l, n) != 0) {
        return 0;
    }
    return n->id;
}

/*
 * The identity under which the reference's store collapses edges: source, target and
 * type, and for an import also the local name it binds, read from its properties up
 * to the closing quote. Allocated, so no identity is ever cut short.
 */
static char *edge_key(int64_t source_id, int64_t target_id, const char *type,
                      const char *properties_json) {
    static const char local_name_key[] = "\"local_name\":\"";
    const char *ln = NULL;
    size_t ln_len = 0;
    if (properties_json && strcmp(type, "IMPORTS") == 0) {
        ln = strstr(properties_json, local_name_key);
        if (ln) {
            ln += sizeof(local_name_key) - 1;
            const char *end = strchr(ln, '"');
            ln_len = end ? (size_t)(end - ln) : strlen(ln);
        }
    }
    size_t cap = strlen(type) + ln_len + 64;
    char *key = (char *)pdxe_counted_malloc(cap);
    if (!key) {
        return NULL;
    }
    if (ln) {
        snprintf(key, cap, "%lld:%lld:%s:%.*s", (long long)source_id, (long long)target_id, type,
                 (int)ln_len, ln);
    } else {
        snprintf(key, cap, "%lld:%lld:%s", (long long)source_id, (long long)target_id, type);
    }
    return key;
}

/*
 * Whether a repeated edge's properties replace the ones it arrived with first. The
 * reference prefers properties over none, then the higher confidence, then the
 * larger text. The only edges added here are imports, which carry no confidence, so
 * the comparison reduces to the rest.
 */
static bool edge_props_replace(const char *existing, const char *incoming) {
    if (!incoming || strcmp(incoming, "{}") == 0) {
        return false;
    }
    if (!existing || strcmp(existing, "{}") == 0) {
        return true;
    }
    return strcmp(incoming, existing) > 0;
}

int pdxe_gbuf_add_edge(pdxe_gbuf_t *gb, int64_t source_id, int64_t target_id, const char *type,
                       const char *target_qualified_name, const char *properties_json) {
    if (!gb || !type || source_id <= 0 || source_id > (int64_t)gb->n_nodes) {
        return -1;
    }
    char *key = edge_key(source_id, target_id, type, properties_json);
    if (!key) {
        return -1;
    }
    pdxe_gbuf_edge_t *existing = (pdxe_gbuf_edge_t *)pdxe_ht_get(gb->by_edge_key, key);
    if (existing) {
        free(key);
        if (edge_props_replace(existing->properties_json, properties_json) &&
            !replace_str(&existing->properties_json, properties_json)) {
            return -1;
        }
        return 0;
    }
    if (gb->n_edges == gb->cap_edges) {
        uint32_t cap = gb->cap_edges ? gb->cap_edges * 2 : 64;
        pdxe_gbuf_edge_t **grown =
            (pdxe_gbuf_edge_t **)pdxe_counted_realloc(gb->edges, (size_t)cap * sizeof(*grown));
        if (!grown) {
            free(key);
            return -1;
        }
        gb->edges = grown;
        gb->cap_edges = cap;
    }
    pdxe_gbuf_edge_t *e = (pdxe_gbuf_edge_t *)pdxe_counted_calloc(1, sizeof(*e));
    if (!e) {
        free(key);
        return -1;
    }
    e->source_id = source_id;
    e->target_id = target_id;
    e->type = pdxe_counted_strdup(type);
    e->target_qualified_name = dup_or_null(target_qualified_name);
    e->properties_json = dup_or_null(properties_json);
    if (!e->type || dup_failed(target_qualified_name, e->target_qualified_name) ||
        dup_failed(properties_json, e->properties_json) ||
        ptr_list_push(&gb->owned_keys, key) != 0) {
        free((void *)e->type);
        free((void *)e->target_qualified_name);
        free((void *)e->properties_json);
        free(e);
        free(key);
        return -1;
    }
    gb->edges[gb->n_edges++] = e;
    pdxe_ht_set(gb->by_edge_key, key, e);

    char st_key[64];
    snprintf(st_key, sizeof(st_key), "%lld\x1f%s", (long long)source_id, type);
    ptr_list *l = list_for(gb, gb->by_source_type, st_key);
    return (l && ptr_list_push(l, e) == 0) ? 0 : -1;
}

uint32_t pdxe_gbuf_node_count(const pdxe_gbuf_t *gb) {
    return gb ? gb->n_nodes : 0;
}

/* --- the lookups the resolution sources call -------------------------------- */

const pdxe_gbuf_node_t *pdxe_gbuf_find_by_id(const pdxe_gbuf_t *gb, int64_t id) {
    if (!gb || id <= 0 || id > (int64_t)gb->n_nodes) {
        return NULL;
    }
    return gb->nodes[id - 1];
}

/*
 * Every node with that short name, in the order described in graph_buffer.h. The
 * returned array is the registry's own and stays valid until the next node is added,
 * which does not happen once resolution has started.
 */
int pdxe_gbuf_find_by_name(const pdxe_gbuf_t *gb, const char *short_name,
                           const pdxe_gbuf_node_t ***hits, int *hit_count) {
    if (!gb || !hits || !hit_count) {
        return -1;
    }
    const ptr_list *l = (const ptr_list *)pdxe_ht_get(gb->by_name, short_name ? short_name : "");
    if (l && l->count > 0) {
        *hits = (const pdxe_gbuf_node_t **)l->items;
        *hit_count = l->count;
    } else {
        *hits = NULL;
        *hit_count = 0;
    }
    return 0;
}

const pdxe_gbuf_node_t *pdxe_gbuf_find_by_qn(const pdxe_gbuf_t *gb, const char *qn) {
    if (!gb || !qn) {
        return NULL;
    }
    return (const pdxe_gbuf_node_t *)pdxe_ht_get(gb->by_qn, qn);
}

/*
 * Relationships of one type leaving one node. Returns 0 whether or not any exist,
 * and a non-zero value only for a malformed request, which is how the caller reads
 * it: a non-zero return discards the answer.
 */
int pdxe_gbuf_find_edges_by_source_type(const pdxe_gbuf_t *gb, int64_t source_id,
                                        const char *type, const pdxe_gbuf_edge_t ***edges,
                                        int *edge_count) {
    if (edges) {
        *edges = NULL;
    }
    if (edge_count) {
        *edge_count = 0;
    }
    if (!gb || !type || !edges || !edge_count) {
        return -1;
    }
    char key[64];
    snprintf(key, sizeof(key), "%lld\x1f%s", (long long)source_id, type);
    const ptr_list *l = (const ptr_list *)pdxe_ht_get(gb->by_source_type, key);
    if (l) {
        *edges = (const pdxe_gbuf_edge_t **)l->items;
        *edge_count = l->count;
    }
    return 0;
}

/*
 * Counters the matcher in lsp_resolve.h bumps each time it falls back to matching a
 * target by its class-and-method tail. The reference's parallel pass owns them and
 * reports them in its timing log; nothing here reads them, but the matcher's code
 * refers to them, so they must exist.
 */
_Atomic uint64_t g_lsp_tail_lookups = 0;
_Atomic uint64_t g_lsp_tail_candidates = 0;

/*
 * The reference's contract with no pipeline: the rows are freed at once. The
 * interface never gives the resolution sources a pipeline, because the rows are all a
 * pipeline would receive and nothing here reads them, so that is the only case.
 */
void pdxe_pipeline_set_lsp_surfaces(pdxe_pipeline_t *p, pdxe_lsp_surface_row_t *rows, int count) {
    (void)p;
    pdxe_store_free_lsp_surfaces(rows, count);
}

/* Frees what the surface builder allocated for each row, then the rows. */
void pdxe_store_free_lsp_surfaces(pdxe_lsp_surface_row_t *rows, int count) {
    if (!rows) {
        return;
    }
    for (int i = 0; i < count; i++) {
        free((void *)rows[i].project);
        free((void *)rows[i].rel_path);
        free((void *)rows[i].surface_sha);
        free((void *)rows[i].defs_json);
        free((void *)rows[i].ref_bloom);
        free((void *)rows[i].config_ctx);
    }
    free(rows);
}

/* --- results ---------------------------------------------------------------- */

/*
 * The reference's contract, first branch unchanged: a result held in memory is
 * returned, subject to the caller's filter. That filter is honoured here too; a shim
 * that ignored it would hand passes results they had asked to skip.
 *
 * The reference's second branch reads a result back from disk. Nothing is ever
 * written there, so that branch has nothing to find and the answer is none.
 */
PDXEFileResult *pdxe_pipeline_result_acquire(const pdxe_pipeline_ctx_t *ctx,
                                             PDXEFileResult **cache, int i,
                                             pdxe_result_want_fn want, bool *loaded) {
    (void)ctx;
    if (loaded) {
        *loaded = false;
    }
    if (cache && i >= 0 && cache[i]) {
        return (!want || want(cache[i])) ? cache[i] : NULL;
    }
    return NULL;
}

/*
 * Frees a result only if it was loaded from disk for this one use. None ever is,
 * so this frees nothing; the condition is kept so the contract reads the same.
 */
void pdxe_pipeline_result_release(PDXEFileResult *r, bool loaded) {
    if (r && loaded) {
        pdxe_free_result(r);
    }
}

/* --- work distribution ------------------------------------------------------ */

/*
 * The reference's contract: every index from 0 to count - 1 is visited exactly once,
 * the call returns when all are done, and a count of zero or less does nothing. A
 * loop in order keeps every part of it. Parallelism is the caller's, one engine
 * context per thread.
 */
void pdxe_parallel_for(int count, pdxe_parallel_fn fn, void *ctx, pdxe_parallel_for_opts_t opts) {
    (void)opts;
    if (!fn || count <= 0) {
        return;
    }
    for (int i = 0; i < count; i++) {
        fn(i, ctx);
    }
}

/* --- language by file name -------------------------------------------------- */

/*
 * The lookup itself is the reference's own, vendored (src/resolve/language_lookup.c).
 * It first consults a per-user language configuration, which the reference loads
 * from files when its indexer starts. This project loads none, because its language
 * matrix is fixed, so there is never a configuration and the lookup answers from its
 * tables, as the reference's does when no configuration file exists.
 */
const pdxe_userconfig_t *pdxe_get_user_lang_config(void) {
    return NULL;
}

PDXELanguage pdxe_userconfig_lookup(const pdxe_userconfig_t *cfg, const char *ext) {
    (void)cfg;
    (void)ext;
    return PDXE_LANG_COUNT;
}

/* --- the package map ------------------------------------------------------ */

/*
 * The reference keeps one package map per process and reads it with no argument.
 * Here it is per thread, set by the interface for the length of one project's
 * resolution and cleared after, because each thread resolves its own project.
 */
static _Thread_local PDXEHashTable *thread_pkgmap = NULL;

void pdxe_pipeline_set_pkgmap(struct PDXEHashTable *map) {
    thread_pkgmap = map;
}

PDXEHashTable *pdxe_pipeline_get_pkgmap(void) {
    return thread_pkgmap;
}
