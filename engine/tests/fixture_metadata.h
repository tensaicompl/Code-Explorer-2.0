/*
 * Shared by the engine's test programs that resolve a fixture directory
 * (resolve_dump, differential_dump): a growable list of strings, and the fixture's
 * repository metadata, read from the file pdx-metadata.txt in the directory (the
 * format resolve_dump.c describes).
 */

#ifndef PDX_FIXTURE_METADATA_H
#define PDX_FIXTURE_METADATA_H

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "pdxe.h"

typedef struct {
    char **items;
    size_t count, cap;
} strings;

static void push(strings *s, char *item) {
    if (s->count == s->cap) {
        s->cap = s->cap ? s->cap * 2 : 32;
        s->items = realloc(s->items, s->cap * sizeof(char *));
        if (!s->items) {
            exit(2);
        }
    }
    s->items[s->count++] = item;
}

/* --- metadata --------------------------------------------------------------- */

typedef struct {
    pdxe_package_entry packages[64];
    uint32_t n_packages;
    pdxe_alias_scope scopes[16];
    uint32_t n_scopes;
    pdxe_path_alias aliases[16][32];
    pdxe_crate_manifest crate;
    pdxe_crate_dependency dependencies[64];
    const char *members[64];
    int has_crate;
    int present;
} metadata;

/* Every string the metadata parser made, freed together at the end. */
static strings metadata_strings = {0};

static char *field(char **cursor) {
    char *start = *cursor;
    if (!start) {
        return NULL;
    }
    char *tab = strchr(start, '\t');
    if (tab) {
        *tab = '\0';
        *cursor = tab + 1;
    } else {
        *cursor = NULL;
    }
    char *copy = strdup(start);
    push(&metadata_strings, copy);
    return copy;
}

static void load_metadata(const char *dir, metadata *m) {
    char path[4096];
    snprintf(path, sizeof(path), "%s/pdx-metadata.txt", dir);
    FILE *f = fopen(path, "r");
    if (!f) {
        return;
    }
    m->present = 1;
    char line[2048];
    while (fgets(line, sizeof(line), f)) {
        line[strcspn(line, "\r\n")] = '\0';
        char *c = line;
        char *kind = field(&c);
        if (!kind || kind[0] == '#' || kind[0] == '\0') {
            continue;
        }
        if (strcmp(kind, "package") == 0 && m->n_packages < 64) {
            pdxe_package_entry *e = &m->packages[m->n_packages++];
            e->import_prefix = field(&c);
            e->entry_path = field(&c);
        } else if (strcmp(kind, "scope") == 0 && m->n_scopes < 16) {
            pdxe_alias_scope *s = &m->scopes[m->n_scopes];
            s->dir_prefix = field(&c);
            s->base_url = field(&c);
            s->aliases = m->aliases[m->n_scopes];
            s->n_aliases = 0;
            m->n_scopes++;
        } else if (strncmp(kind, "crate-", 6) == 0) {
            m->has_crate = 1;
            if (strcmp(kind, "crate-package") == 0) {
                m->crate.package_name = field(&c);
            } else if (strcmp(kind, "crate-workspace") == 0) {
                m->crate.is_workspace_root = 1;
            } else if (strcmp(kind, "crate-dependency") == 0 && m->crate.n_dependencies < 64) {
                pdxe_crate_dependency *d = &m->dependencies[m->crate.n_dependencies++];
                d->name = field(&c);
                char *path = field(&c);
                d->path = (path && path[0]) ? path : NULL;
            } else if (strcmp(kind, "crate-member") == 0 && m->crate.n_members < 64) {
                m->members[m->crate.n_members++] = field(&c);
            }
        } else if (strcmp(kind, "alias") == 0 && m->n_scopes > 0) {
            pdxe_alias_scope *s = &m->scopes[m->n_scopes - 1];
            pdxe_path_alias *a = &m->aliases[m->n_scopes - 1][s->n_aliases++];
            a->alias_prefix = field(&c);
            a->alias_suffix = field(&c);
            a->target_prefix = field(&c);
            a->target_suffix = field(&c);
            char *w = field(&c);
            a->has_wildcard = (uint8_t)(w && w[0] == '1');
        }
    }
    fclose(f);
}

#endif /* PDX_FIXTURE_METADATA_H */
