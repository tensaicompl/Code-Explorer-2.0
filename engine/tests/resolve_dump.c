/*
 * Prints what typed resolution decided for a directory of source files, one row per
 * decision, in a form that can be compared with the reference engine's graph.
 *
 *     resolve_dump [--reverse] [--cached] <dir>
 *
 * --reverse adds the files in reverse path order, to show which answers depend on
 * the order files are added.
 *
 * --cached resolves every file the way a build resolves a file its cache already
 * holds: the extraction result is reduced to its surface and to the parts a cache
 * keeps, freed, and the file is added with a result rebuilt from those parts and its
 * surface imported, so nothing is extracted twice. The answers must not change.
 * Every file under <dir> in a language this harness knows is extracted, added to one
 * project in path order, and resolved. Each row is tab-separated:
 *
 *     CALLS           <source> <target> <engine strategy> <score>
 *     CALL_REFERENCE  <source> <target> <engine strategy> <score>
 *     IMPORTS         <file>   <target> <local name>
 *
 * A source is the qualified name of the calling definition, or FILE:<path> for a call
 * at file or module scope, which is how the reference attributes those. Targets are
 * qualified names without the project name. Rows are one per site, not collapsed:
 * the comparison collapses both sides the same way.
 *
 * A file named pdx-metadata.txt in <dir> supplies repository metadata, one entry per
 * line, fields separated by tabs:
 *
 *     package  <import prefix> <entry path>
 *     scope    <directory prefix> <base url>
 *     alias    <alias prefix> <alias suffix> <target prefix> <target suffix> <0|1>
 *     crate-package    <name>
 *     crate-workspace
 *     crate-dependency <name> <repository-relative path, or empty>
 *     crate-member     <path as the manifest lists it>
 *
 * An alias belongs to the scope above it. Any crate line means the repository has a
 * root crate manifest.
 */

#include <dirent.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

#include "pdxe.h"
#include "shim_internal.h"

static const struct {
    const char *ext;
    const char *lang;
} EXTENSIONS[] = {
    {".py", "python"}, {".ts", "typescript"}, {".tsx", "tsx"},   {".js", "javascript"},
    {".jsx", "javascript"}, {".go", "go"},    {".java", "java"}, {".cs", "csharp"},
    {".c", "c"},        {".h", "c"},          {".cpp", "cpp"},   {".hpp", "cpp"},
    {".cc", "cpp"},     {".rs", "rust"},      {".kt", "kotlin"}, {".php", "php"},
};

static const char *language_of(const char *path) {
    const char *dot = strrchr(path, '.');
    if (!dot) {
        return NULL;
    }
    for (size_t i = 0; i < sizeof(EXTENSIONS) / sizeof(EXTENSIONS[0]); i++) {
        if (strcmp(dot, EXTENSIONS[i].ext) == 0) {
            return EXTENSIONS[i].lang;
        }
    }
    return NULL;
}

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

static int cmp_str(const void *a, const void *b) {
    return strcmp(*(char *const *)a, *(char *const *)b);
}

static void walk(const char *root, const char *rel, strings *out) {
    char path[4096];
    snprintf(path, sizeof(path), "%s%s%s", root, rel[0] ? "/" : "", rel);
    DIR *d = opendir(path);
    if (!d) {
        return;
    }
    struct dirent *e;
    while ((e = readdir(d)) != NULL) {
        if (e->d_name[0] == '.') {
            continue;
        }
        char child[4096];
        snprintf(child, sizeof(child), "%s%s%s", rel, rel[0] ? "/" : "", e->d_name);
        char full[8200];
        snprintf(full, sizeof(full), "%s/%s", root, child);
        struct stat st;
        if (stat(full, &st) != 0) {
            continue;
        }
        if (S_ISDIR(st.st_mode)) {
            walk(root, child, out);
        } else if (language_of(child)) {
            push(out, strdup(child));
        }
    }
    closedir(d);
}

static unsigned char *read_all(const char *path, size_t *len) {
    FILE *f = fopen(path, "rb");
    if (!f) {
        return NULL;
    }
    fseek(f, 0, SEEK_END);
    long n = ftell(f);
    fseek(f, 0, SEEK_SET);
    unsigned char *buf = malloc(n > 0 ? (size_t)n : 1);
    if (buf && n > 0 && fread(buf, 1, (size_t)n, f) != (size_t)n) {
        free(buf);
        buf = NULL;
    }
    fclose(f);
    *len = n > 0 ? (size_t)n : 0;
    return buf;
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

/* --- output ----------------------------------------------------------------- */

static const char *CALLABLE_KINDS[] = {"Function", "Method", "Constructor", "Class"};

typedef struct {
    pdxe_file_result **results;
    strings *files;
} corpus;

/* The engine kind of the definition with this qualified name, if any file has one. */
static const char *engine_kind_of(const corpus *c, const char *qn) {
    for (size_t i = 0; i < c->files->count; i++) {
        const pdxe_file_result *r = c->results[i];
        for (uint32_t k = 0; r && k < r->n_defs; k++) {
            if (r->defs[k].qualified_name && strcmp(r->defs[k].qualified_name, qn) == 0) {
                return r->defs[k].engine_kind;
            }
        }
    }
    return NULL;
}

static int is_callable(const corpus *c, const pdxe_resolution *res) {
    if (!res->target_rel_path) {
        return 1; /* a built-in the engine supplies: always a function or a type */
    }
    const char *kind = engine_kind_of(c, res->target_qualified_name);
    for (size_t i = 0; kind && i < sizeof(CALLABLE_KINDS) / sizeof(CALLABLE_KINDS[0]); i++) {
        if (strcmp(kind, CALLABLE_KINDS[i]) == 0) {
            return 1;
        }
    }
    return 0;
}

static int file_index(const corpus *c, const char *rel) {
    for (size_t i = 0; i < c->files->count; i++) {
        if (strcmp(c->files->items[i], rel) == 0) {
            return (int)i;
        }
    }
    return -1;
}

static void print_source(const corpus *c, const pdxe_resolution *res) {
    int fi = file_index(c, res->rel_path);
    const pdxe_file_result *r = fi >= 0 ? c->results[fi] : NULL;
    uint32_t ci = res->site.caller_index;
    if (r && ci != PDXE_NO_PARENT && ci < r->n_defs && strcmp(r->defs[ci].kind, "module") != 0) {
        printf("%s", r->defs[ci].qualified_name);
    } else {
        printf("FILE:%s", res->rel_path);
    }
}

static void print_import(const char *rel, const char *local, const char *target, const char *label,
                         void *ud) {
    (void)ud;
    (void)label;
    printf("IMPORTS\tFILE:%s\t%s\t%s\n", rel, target ? target : "-", local);
}

int main(int argc, char **argv) {
    int reverse = 0, cached = 0;
    for (int a = 1; a < argc - 1; a++) {
        if (strcmp(argv[a], "--reverse") == 0) {
            reverse = 1;
        } else if (strcmp(argv[a], "--cached") == 0) {
            cached = 1;
        }
    }
    if (argc != 2 + reverse + cached) {
        fprintf(stderr, "usage: resolve_dump [--reverse] [--cached] <dir>\n");
        return 2;
    }
    const char *dir = argv[argc - 1];
    strings files = {0};
    walk(dir, "", &files);
    if (files.count > 0) {
        qsort(files.items, files.count, sizeof(char *), cmp_str);
    }
    for (size_t i = 0; reverse && i < files.count / 2; i++) {
        char *swap = files.items[i];
        files.items[i] = files.items[files.count - 1 - i];
        files.items[files.count - 1 - i] = swap;
    }

    pdxe_ctx *ctx = NULL;
    if (pdxe_init(&ctx) != PDXE_OK) {
        return 1;
    }
    pdxe_project *p = NULL;
    if (pdxe_resolve_project_begin(ctx, &p) != PDXE_OK) {
        return 1;
    }
    pdxe_file_result **results = calloc(files.count ? files.count : 1, sizeof(*results));
    unsigned char **sources = calloc(files.count ? files.count : 1, sizeof(*sources));
    for (size_t i = 0; i < files.count; i++) {
        char path[4096];
        snprintf(path, sizeof(path), "%s/%s", dir, files.items[i]);
        size_t len = 0;
        sources[i] = read_all(path, &len);
        int lang = pdxe_language_id(language_of(files.items[i]));
        if (!sources[i] || pdxe_extract_file(ctx, lang, files.items[i], sources[i], len,
                                             &results[i]) != PDXE_OK) {
            fprintf(stderr, "cannot extract %s\n", files.items[i]);
            return 1;
        }
        if (cached) {
            /* What a cache keeps: the surface and the interface's arrays. */
            uint8_t *surface = NULL;
            size_t surface_len = 0;
            pdxe_file_result *rebuilt = NULL;
            const pdxe_file_result *r = results[i];
            if (pdxe_surface_export(r, files.items[i], &surface, &surface_len) != PDXE_OK ||
                pdxe_result_build(ctx, r->defs, r->n_defs, r->calls, r->n_calls, r->imports,
                                  r->n_imports, r->usages, r->n_usages, r->types, r->n_types,
                                  r->rws, r->n_rws, &rebuilt) != PDXE_OK ||
                pdxe_surface_import(p, surface, surface_len) != PDXE_OK) {
                fprintf(stderr, "cannot cache %s\n", files.items[i]);
                return 1;
            }
            pdxe_surface_free(surface);
            pdxe_result_free(ctx, results[i]);
            results[i] = rebuilt;
        }
        int rc = pdxe_resolve_project_add_file(p, lang, files.items[i], sources[i], len, results[i]);
        if (rc != PDXE_OK) {
            fprintf(stderr, "cannot add %s: %d\n", files.items[i], rc);
            return 1;
        }
    }

    metadata m;
    memset(&m, 0, sizeof(m));
    load_metadata(dir, &m);
    if (m.present) {
        m.crate.dependencies = m.dependencies;
        m.crate.member_paths = m.members;
        pdxe_resolution_metadata md = {m.packages, m.n_packages, m.scopes, m.n_scopes,
                                       m.has_crate ? &m.crate : NULL};
        if (pdxe_resolve_project_set_metadata(p, &md) != PDXE_OK) {
            fprintf(stderr, "cannot set metadata\n");
            return 1;
        }
    }

    if (pdxe_resolve_project_run(p) != PDXE_OK) {
        fprintf(stderr, "resolution failed\n");
        return 1;
    }
    corpus c = {results, &files};
    const pdxe_resolution *res = NULL;
    uint32_t n = 0;
    pdxe_resolve_project_results(p, &res, &n);
    for (uint32_t i = 0; i < n; i++) {
        const char *kind = res[i].site.is_reference
                               ? (is_callable(&c, &res[i]) ? "CALL_REFERENCE" : NULL)
                               : "CALLS";
        if (!kind) {
            continue; /* a reference to a value that is not callable is a plain use */
        }
        printf("%s\t", kind);
        print_source(&c, &res[i]);
        printf("\t%s\t%s\t%.2f\n", res[i].target_qualified_name,
               res[i].engine_strategy ? res[i].engine_strategy : "-", res[i].score);
    }
    pdxe_project_debug_imports(p, print_import, NULL);

    pdxe_resolve_project_end(p);
    for (size_t i = 0; i < files.count; i++) {
        pdxe_result_free(ctx, results[i]);
        free(sources[i]);
        free(files.items[i]);
    }
    free(results);
    free(sources);
    free(files.items);
    for (size_t i = 0; i < metadata_strings.count; i++) {
        free(metadata_strings.items[i]);
    }
    free(metadata_strings.items);
    pdxe_shutdown(ctx);
    return 0;
}
