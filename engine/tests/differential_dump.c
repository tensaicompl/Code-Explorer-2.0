/*
 * The vendored engine's side of the engine differential (P2-16, decision 50): every
 * row tests/differential_rows.h prints, for one corpus, after the cross-file typed
 * pass, as JSON lines on standard output. scripts/engine/differential.py compares
 * them with the reference engine's rows for the same files.
 *
 *     differential_dump <list> <dir>
 *
 * <list> names the corpus's files, one per line: the engine language id, a tab, the
 * path below <dir>. Nothing else under <dir> is read but pdx-metadata.txt, the
 * repository metadata in the format resolve_dump.c describes.
 *
 * Two runs, each over every file in path order:
 *
 *   - Fresh: every file extracted and added with its result; the project resolves
 *     through the results, so after the run each result holds what the typed pass
 *     wrote into it, as the reference's do when its calls pass starts. Every row of
 *     every file is printed from there.
 *   - Cached: every file added as a build adds a file its cache holds, its result
 *     rebuilt and its surface exported and imported (pdxe_surface_export, _import),
 *     so typed resolution reads what the surface carried. Each file's cross-file
 *     surface (pdxe_project_debug_lsp_surfaces) is printed from this run, as
 *     {"c":"surface","file":...,"json":...}, and every answer the run wrote as
 *     {"c":"cached_typed",...}, which the harness requires to equal the fresh run's.
 *
 * A run that is not clean is refused: its missing answers would read as the engine's.
 * Exit 0, or 1 with one line on standard error.
 */

#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "fixture_metadata.h"
#include "pdxe.h"
#include "pdxe_core.h"
#include "shim_internal.h"

#include "differential_rows.h"

static int fail(const char *what, const char *detail) {
    fprintf(stderr, "differential_dump: %s%s%s\n", what, detail ? ": " : "", detail ? detail : "");
    return 1;
}

static unsigned char *read_file(const char *path, size_t *len) {
    FILE *f = fopen(path, "rb");
    if (!f) {
        return NULL;
    }
    if (fseek(f, 0, SEEK_END) != 0) {
        fclose(f);
        return NULL;
    }
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

typedef struct {
    char *lang;
    char *rel;
    int lang_id;
    unsigned char *source;
    size_t len;
} corpus_file;

static int cmp_file(const void *a, const void *b) {
    return strcmp(((const corpus_file *)a)->rel, ((const corpus_file *)b)->rel);
}

static int set_metadata(pdxe_project *p, metadata *m) {
    if (!m->present) {
        return PDXE_OK;
    }
    m->crate.dependencies = m->dependencies;
    m->crate.member_paths = m->members;
    pdxe_resolution_metadata md = {m->packages, m->n_packages, m->scopes, m->n_scopes,
                                   m->has_crate ? &m->crate : NULL};
    return pdxe_resolve_project_set_metadata(p, &md);
}

static int clean(pdxe_project *p) {
    pdxe_run_health h;
    if (pdxe_resolve_project_health(p, &h) != PDXE_OK || h.status != PDXE_RUN_CLEAN) {
        fprintf(stderr,
                "differential_dump: typed resolution was degraded: %u files not reached, %u "
                "without source, %u over budget, %u pass failures\n",
                h.files_not_reached, h.files_source_unavailable, h.files_over_budget,
                h.pass_failures);
        return 0;
    }
    return 1;
}

static void print_surface(const char *rel, const char *json, void *ud) {
    (void)ud;
    dr_begin(stdout, "surface", rel);
    dr_kstr(stdout, "json", json);
    fputs("}\n", stdout);
}

static void print_cached_typed(const char *rel, const char *callee, const char *strategy,
                               const char *reason, int kind, uint32_t start, uint32_t end,
                               void *ud) {
    (void)ud;
    dr_begin(stdout, "cached_typed", rel);
    dr_kstr(stdout, "callee", callee);
    dr_kstr(stdout, "strategy", strategy);
    dr_kstr(stdout, "reason", reason);
    dr_kint(stdout, "kind", kind);
    dr_kint(stdout, "start", start);
    dr_kint(stdout, "end", end);
    fputs("}\n", stdout);
}

int main(int argc, char **argv) {
    if (argc != 3) {
        fprintf(stderr, "usage: differential_dump <list> <dir>\n");
        return 2;
    }
    const char *dir = argv[2];
    FILE *list = fopen(argv[1], "r");
    if (!list) {
        return fail("cannot read the list", argv[1]);
    }
    corpus_file *files = NULL;
    size_t n = 0, cap = 0;
    char line[4096];
    while (fgets(line, sizeof(line), list)) {
        line[strcspn(line, "\r\n")] = '\0';
        if (!line[0]) {
            continue;
        }
        char *tab = strchr(line, '\t');
        if (!tab || !tab[1]) {
            fclose(list);
            return fail("a list line is not <language>\\t<path>", line);
        }
        *tab = '\0';
        if (n == cap) {
            cap = cap ? cap * 2 : 16;
            files = realloc(files, cap * sizeof(*files));
            if (!files) {
                fclose(list);
                return fail("out of memory", NULL);
            }
        }
        corpus_file *f = &files[n++];
        memset(f, 0, sizeof(*f));
        f->lang = strdup(line);
        f->rel = strdup(tab + 1);
        f->lang_id = pdxe_language_id(f->lang);
        if (!f->lang || !f->rel || f->lang_id == 0) {
            fclose(list);
            return fail("not an engine language", line);
        }
    }
    fclose(list);
    if (n == 0) {
        return fail("the list names no file", argv[1]);
    }
    qsort(files, n, sizeof(*files), cmp_file);
    for (size_t i = 0; i < n; i++) {
        char path[8192];
        snprintf(path, sizeof(path), "%s/%s", dir, files[i].rel);
        files[i].source = read_file(path, &files[i].len);
        if (!files[i].source) {
            return fail("cannot read", files[i].rel);
        }
    }
    metadata m;
    memset(&m, 0, sizeof(m));
    load_metadata(dir, &m);

    pdxe_ctx *ctx = NULL;
    if (pdxe_init(&ctx) != PDXE_OK) {
        return fail("the engine did not start", NULL);
    }
    pdxe_file_result **results = calloc(n, sizeof(*results));
    if (!results) {
        return fail("out of memory", NULL);
    }

    /* The fresh run. */
    pdxe_project *p = NULL;
    if (pdxe_resolve_project_begin(ctx, &p) != PDXE_OK) {
        return fail("cannot begin a project", NULL);
    }
    for (size_t i = 0; i < n; i++) {
        corpus_file *f = &files[i];
        if (pdxe_extract_file(ctx, f->lang_id, f->rel, f->source, f->len, &results[i]) !=
            PDXE_OK) {
            return fail("cannot extract", f->rel);
        }
        if (pdxe_resolve_project_add_file(p, f->lang_id, f->rel, f->source, f->len,
                                          results[i]) != PDXE_OK) {
            return fail("cannot add", f->rel);
        }
    }
    if (set_metadata(p, &m) != PDXE_OK) {
        return fail("cannot set the metadata", NULL);
    }
    if (pdxe_resolve_project_run(p) != PDXE_OK) {
        return fail("resolution failed", NULL);
    }
    if (!clean(p)) {
        return 1;
    }
    for (size_t i = 0; i < n; i++) {
        const PDXEFileResult *engine = pdxe_result_engine(results[i]);
        if (!engine) {
            return fail("no engine result behind", files[i].rel);
        }
        diff_rows_file(stdout, files[i].rel, engine);
    }
    pdxe_resolve_project_end(p);

    /* The cached run: every file as a build adds one its cache holds. */
    pdxe_project *q = NULL;
    if (pdxe_resolve_project_begin(ctx, &q) != PDXE_OK) {
        return fail("cannot begin a project", NULL);
    }
    pdxe_file_result **rebuilt = calloc(n, sizeof(*rebuilt));
    if (!rebuilt) {
        return fail("out of memory", NULL);
    }
    for (size_t i = 0; i < n; i++) {
        const pdxe_file_result *r = results[i];
        uint8_t *surface = NULL;
        size_t surface_len = 0;
        if (pdxe_surface_export(r, files[i].rel, &surface, &surface_len) != PDXE_OK ||
            pdxe_result_build(ctx, r->defs, r->n_defs, r->calls, r->n_calls, r->imports,
                              r->n_imports, r->usages, r->n_usages, r->types, r->n_types, r->rws,
                              r->n_rws, r->impl_traits, r->n_impl_traits,
                              &rebuilt[i]) != PDXE_OK ||
            pdxe_surface_import(q, surface, surface_len) != PDXE_OK) {
            return fail("cannot cache", files[i].rel);
        }
        pdxe_surface_free(surface);
        if (pdxe_resolve_project_add_file(q, files[i].lang_id, files[i].rel, files[i].source,
                                          files[i].len, rebuilt[i]) != PDXE_OK) {
            return fail("cannot add", files[i].rel);
        }
    }
    if (set_metadata(q, &m) != PDXE_OK) {
        return fail("cannot set the metadata", NULL);
    }
    if (pdxe_resolve_project_run(q) != PDXE_OK) {
        return fail("resolution from surfaces failed", NULL);
    }
    if (!clean(q)) {
        return 1;
    }
    pdxe_project_debug_lsp_surfaces(q, print_surface, NULL);
    pdxe_project_debug_raw_resolutions(q, print_cached_typed, NULL);
    pdxe_resolve_project_end(q);

    if (fflush(stdout) != 0 || ferror(stdout)) {
        return fail("cannot write the rows", NULL);
    }
    for (size_t i = 0; i < n; i++) {
        pdxe_result_free(ctx, results[i]);
        pdxe_result_free(ctx, rebuilt[i]);
        free(files[i].source);
        free(files[i].lang);
        free(files[i].rel);
    }
    free(results);
    free(rebuilt);
    free(files);
    for (size_t i = 0; i < metadata_strings.count; i++) {
        free(metadata_strings.items[i]);
    }
    free(metadata_strings.items);
    pdxe_shutdown(ctx);
    return 0;
}
