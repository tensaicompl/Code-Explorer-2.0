/*
 * Vendored subset: resolving an import through a build configuration's path
 * aliases, extracted verbatim from the reference's path-alias source by
 * scripts/vendor/extract-functions.py. The loader that walks the repository and
 * reads build configuration is deliberately not here: aliases reach the engine
 * already resolved, through its interface.
 */

#include "pipeline/path_alias.h"
#include "pipeline/pipeline_internal.h"
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>


/* Strip .ts/.tsx/.js/.jsx in place. Returns its argument. */
static char *strip_resolved_ext(char *path) {
    if (!path) {
        return path;
    }
    size_t len = strlen(path);
    if (len > 3 && path[len - 3] == '.' && (path[len - 2] == 't' || path[len - 2] == 'j') &&
        path[len - 1] == 's') {
        path[len - 3] = '\0';
        return path;
    }
    if (len > 4 && path[len - 4] == '.' && (path[len - 3] == 't' || path[len - 3] == 'j') &&
        path[len - 2] == 's' && path[len - 1] == 'x') {
        path[len - 4] = '\0';
    }
    return path;
}

char *pdxe_path_alias_resolve(const pdxe_path_alias_map_t *map, const char *module_path) {
    if (!map || !module_path) {
        return NULL;
    }
    size_t mod_len = strlen(module_path);

    for (int i = 0; i < map->count; i++) {
        const pdxe_path_alias_t *e = &map->entries[i];

        if (e->has_wildcard) {
            size_t prefix_len = strlen(e->alias_prefix);
            size_t suffix_len = strlen(e->alias_suffix);
            if (mod_len < prefix_len + suffix_len) {
                continue;
            }
            if (strncmp(module_path, e->alias_prefix, prefix_len) != 0) {
                continue;
            }
            if (suffix_len > 0 &&
                strcmp(module_path + mod_len - suffix_len, e->alias_suffix) != 0) {
                continue;
            }
            size_t wild_len = mod_len - prefix_len - suffix_len;
            const char *wild_start = module_path + prefix_len;
            size_t tp_len = strlen(e->target_prefix);
            size_t ts_len = strlen(e->target_suffix);
            char *result = malloc(tp_len + wild_len + ts_len + 1);
            if (!result) {
                return NULL;
            }
            memcpy(result, e->target_prefix, tp_len);
            memcpy(result + tp_len, wild_start, wild_len);
            memcpy(result + tp_len + wild_len, e->target_suffix, ts_len);
            result[tp_len + wild_len + ts_len] = '\0';
            return strip_resolved_ext(result);
        }

        if (strcmp(module_path, e->alias_prefix) == 0) {
            return strip_resolved_ext(strdup(e->target_prefix));
        }
    }

    /* baseUrl fallback. Apply only to non-relative imports that look
     * sub-path-ish (contain '/' but don't start with '.' or '@'); skips
     * obvious package names like "react" or "lodash". */
    if (map->base_url && module_path[0] != '.' && module_path[0] != '@' &&
        strchr(module_path, '/') != NULL) {
        size_t bu_len = strlen(map->base_url);
        size_t need = bu_len + 1 + mod_len + 1;
        char *result = malloc(need);
        if (!result) {
            return NULL;
        }
        snprintf(result, need, "%s/%s", map->base_url, module_path);
        return strip_resolved_ext(result);
    }
    return NULL;
}

const pdxe_path_alias_map_t *pdxe_path_alias_find_for_file(const pdxe_path_alias_collection_t *coll,
                                                         const char *rel_path) {
    if (!coll || !rel_path) {
        return NULL;
    }
    for (int i = 0; i < coll->count; i++) {
        const char *prefix = coll->scopes[i].dir_prefix;
        size_t plen = strlen(prefix);
        if (plen == 0) {
            return coll->scopes[i].map;
        }
        if (strncmp(rel_path, prefix, plen) == 0 &&
            (rel_path[plen] == '/' || rel_path[plen] == '\0')) {
            return coll->scopes[i].map;
        }
    }
    return NULL;
}
