/*
 * What a cache keeps comes back exactly.
 *
 *     abi_result_build_roundtrip <fixture dir>...
 *
 * For every source file under the directories:
 *
 *   - the interface's arrays rebuilt through pdxe_result_build equal the extracted
 *     ones, field by field, strings by content;
 *   - a rebuilt result has no surface of its own to give;
 *   - the file's surface, decoded and encoded again, is the same bytes.
 *
 * Every definition's base classes come back too, in order, and are the rebuilt
 * result's own: rebuilt from a copy that is then overwritten and freed, they still
 * compare equal. Definitions with no base, one base and several must all occur
 * (fixtures/bases), and a definition with none has no array (issue 40).
 *
 * And surfaces that are not what the encoder writes are refused: truncated, of
 * another version, with a field missing, a field extra, a field of the wrong type,
 * or a counted array whose count disagrees with it.
 *
 * And for every resolution fixture (a directory holding expected.tsv), resolving the
 * files as one project leaves each file's surface byte for byte as it was: a surface
 * holds only what extraction found in the file itself, never anything a resolution
 * over other files computed.
 */

#include <dirent.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

#include "pdxe.h"
#include "pdxe_core.h"
#include "shim_internal.h"

static int failures = 0;

#define CHECK(cond, ...)                                                                           \
    do {                                                                                           \
        if (!(cond)) {                                                                             \
            printf("FAIL ");                                                                       \
            printf(__VA_ARGS__);                                                                   \
            printf("\n");                                                                          \
            failures++;                                                                            \
        }                                                                                          \
    } while (0)

static int same_str(const char *a, const char *b) {
    return (!a && !b) || (a && b && strcmp(a, b) == 0);
}

static int same_span(pdxe_span a, pdxe_span b) {
    return memcmp(&a, &b, sizeof(a)) == 0;
}

static void compare(const char *path, const pdxe_file_result *a, const pdxe_file_result *b) {
    CHECK(a->n_defs == b->n_defs && a->n_calls == b->n_calls && a->n_imports == b->n_imports &&
              a->n_usages == b->n_usages && a->n_types == b->n_types && a->n_rws == b->n_rws,
          "%s: array lengths differ", path);
    for (uint32_t i = 0; i < a->n_defs && i < b->n_defs; i++) {
        const pdxe_definition *x = &a->defs[i], *y = &b->defs[i];
        CHECK(same_str(x->name, y->name) && same_str(x->qualified_name, y->qualified_name) &&
                  same_str(x->kind, y->kind) && same_str(x->engine_kind, y->engine_kind) &&
                  same_str(x->signature, y->signature) && same_str(x->doc, y->doc) &&
                  same_span(x->span, y->span) && same_span(x->body_span, y->body_span) &&
                  x->parent_index == y->parent_index && x->visibility == y->visibility &&
                  x->is_test == y->is_test && x->is_entry_point == y->is_entry_point &&
                  x->cyclomatic == y->cyclomatic && x->cognitive == y->cognitive &&
                  x->loop_depth == y->loop_depth && x->n_base_classes == y->n_base_classes,
              "%s: definition %u differs", path, i);
        for (uint32_t b = 0; b < x->n_base_classes && b < y->n_base_classes; b++) {
            CHECK(same_str(x->base_classes[b], y->base_classes[b]),
                  "%s: definition %u's base %u differs", path, i, b);
        }
    }
    for (uint32_t i = 0; i < a->n_calls && i < b->n_calls; i++) {
        const pdxe_call *x = &a->calls[i], *y = &b->calls[i];
        CHECK(same_str(x->callee_text, y->callee_text) &&
                  same_str(x->receiver_text, y->receiver_text) &&
                  x->caller_index == y->caller_index && same_span(x->span, y->span) &&
                  x->is_reference == y->is_reference && x->typed_only == y->typed_only &&
                  x->lexical == y->lexical,
              "%s: call %u differs", path, i);
    }
    for (uint32_t i = 0; i < a->n_imports && i < b->n_imports; i++) {
        const pdxe_import *x = &a->imports[i], *y = &b->imports[i];
        CHECK(same_str(x->module_text, y->module_text) &&
                  same_str(x->imported_name, y->imported_name) && same_str(x->alias, y->alias) &&
                  same_span(x->span, y->span),
              "%s: import %u differs", path, i);
    }
    for (uint32_t i = 0; i < a->n_usages && i < b->n_usages; i++) {
        const pdxe_usage *x = &a->usages[i], *y = &b->usages[i];
        CHECK(same_str(x->name, y->name) && x->scope_index == y->scope_index &&
                  same_span(x->span, y->span) && x->lexical == y->lexical,
              "%s: usage %u differs", path, i);
    }
    for (uint32_t i = 0; i < a->n_types && i < b->n_types; i++) {
        const pdxe_type_ref *x = &a->types[i], *y = &b->types[i];
        CHECK(same_str(x->type_text, y->type_text) && x->scope_index == y->scope_index &&
                  same_span(x->span, y->span),
              "%s: type reference %u differs", path, i);
    }
    for (uint32_t i = 0; i < a->n_rws && i < b->n_rws; i++) {
        const pdxe_rw *x = &a->rws[i], *y = &b->rws[i];
        CHECK(same_str(x->field_text, y->field_text) && x->scope_index == y->scope_index &&
                  x->is_write == y->is_write && same_span(x->span, y->span),
              "%s: field access %u differs", path, i);
    }
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

static const struct {
    const char *ext;
    const char *lang;
} EXTENSIONS[] = {
    {".py", "python"},   {".ts", "typescript"}, {".tsx", "tsx"},       {".js", "javascript"},
    {".go", "go"},       {".java", "java"},     {".cs", "csharp"},     {".c", "c"},
    {".h", "c"},         {".cpp", "cpp"},       {".hpp", "cpp"},       {".rs", "rust"},
    {".kt", "kotlin"},   {".scala", "scala"},   {".php", "php"},       {".pm", "perl"},
    {".adb", "ada"},     {".sh", "bash"},       {".rb", "ruby"},       {".swift", "swift"},
    {".m", "objc"},      {".groovy", "groovy"}, {".lua", "lua"},       {".sql", "sql"},
    {".proto", "protobuf"}, {".graphql", "graphql"}, {".yaml", "yaml"}, {".json", "json"},
    {".toml", "toml"},   {".tf", "hcl"},        {".dockerfile", "dockerfile"},
    {".md", "markdown"}, {".xml", "xml"},       {".properties", "properties"},
};

static int language_of(const char *name) {
    const char *dot = strrchr(name, '.');
    for (size_t i = 0; dot && i < sizeof(EXTENSIONS) / sizeof(EXTENSIONS[0]); i++) {
        if (strcmp(dot, EXTENSIONS[i].ext) == 0) {
            return pdxe_language_id(EXTENSIONS[i].lang);
        }
    }
    return 0;
}

static int files_checked = 0;
static int defs_without_bases = 0;
static int defs_with_one_base = 0;
static int defs_with_several_bases = 0;

/* A copy of the definitions the caller owns: the array, and each base array and base
 * string. Its other strings stay the result's, which outlives the copy. */
static pdxe_definition *copy_defs(const pdxe_file_result *r) {
    pdxe_definition *copy = calloc(r->n_defs ? r->n_defs : 1, sizeof(*copy));
    for (uint32_t i = 0; copy && i < r->n_defs; i++) {
        copy[i] = r->defs[i];
        if (r->defs[i].n_base_classes) {
            const char **bases = calloc(r->defs[i].n_base_classes, sizeof(*bases));
            for (uint32_t b = 0; bases && b < r->defs[i].n_base_classes; b++) {
                bases[b] = strdup(r->defs[i].base_classes[b]);
            }
            copy[i].base_classes = bases;
        }
    }
    return copy;
}

/* Overwrites and frees what copy_defs made, so anything still pointing into it reads
 * something else. */
static void destroy_defs(pdxe_definition *copy, uint32_t n) {
    for (uint32_t i = 0; i < n; i++) {
        for (uint32_t b = 0; b < copy[i].n_base_classes; b++) {
            char *s = (char *)copy[i].base_classes[b];
            memset(s, 'Z', strlen(s));
            free(s);
            copy[i].base_classes[b] = NULL;
        }
        free((void *)copy[i].base_classes);
    }
    free(copy);
}

static void check_file(pdxe_ctx *ctx, const char *path) {
    int lang = language_of(path);
    if (!lang) {
        return;
    }
    size_t len = 0;
    unsigned char *bytes = read_all(path, &len);
    pdxe_file_result *r = NULL;
    if (!bytes || pdxe_extract_file(ctx, lang, path, bytes, len, &r) != PDXE_OK) {
        CHECK(0, "%s: extraction failed", path);
        free(bytes);
        return;
    }
    files_checked++;
    for (uint32_t i = 0; i < r->n_defs; i++) {
        const pdxe_definition *d = &r->defs[i];
        CHECK((d->n_base_classes == 0) == (d->base_classes == NULL),
              "%s: definition %u has %u bases and %s array", path, i, d->n_base_classes,
              d->base_classes ? "an" : "no");
        for (uint32_t b = 0; b < d->n_base_classes; b++) {
            CHECK(d->base_classes[b] && d->base_classes[b][0], "%s: definition %u's base %u is empty",
                  path, i, b);
        }
        defs_without_bases += d->n_base_classes == 0;
        defs_with_one_base += d->n_base_classes == 1;
        defs_with_several_bases += d->n_base_classes > 1;
    }

    /* Rebuilt from a copy that is destroyed before the comparison: the bases must be
     * the rebuilt result's own. */
    pdxe_definition *copy = copy_defs(r);
    pdxe_file_result *from_copy = NULL;
    CHECK(copy && pdxe_result_build(ctx, copy, r->n_defs, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                                    NULL, 0, &from_copy) == PDXE_OK,
          "%s: rebuild from a copy failed", path);
    if (copy) {
        destroy_defs(copy, r->n_defs);
    }
    if (from_copy) {
        CHECK(from_copy->n_defs == r->n_defs, "%s: a rebuild lost definitions", path);
        for (uint32_t i = 0; i < from_copy->n_defs && i < r->n_defs; i++) {
            const pdxe_definition *x = &r->defs[i], *y = &from_copy->defs[i];
            CHECK(x->n_base_classes == y->n_base_classes &&
                      (y->n_base_classes == 0) == (y->base_classes == NULL),
                  "%s: definition %u's bases were not rebuilt", path, i);
            for (uint32_t b = 0; b < x->n_base_classes && b < y->n_base_classes; b++) {
                CHECK(same_str(x->base_classes[b], y->base_classes[b]) &&
                          y->base_classes[b] != x->base_classes[b],
                      "%s: definition %u's base %u is not a copy of its own", path, i, b);
            }
        }
        pdxe_result_free(ctx, from_copy);
    }

    pdxe_file_result *rebuilt = NULL;
    CHECK(pdxe_result_build(ctx, r->defs, r->n_defs, r->calls, r->n_calls, r->imports,
                            r->n_imports, r->usages, r->n_usages, r->types, r->n_types, r->rws,
                            r->n_rws, &rebuilt) == PDXE_OK,
          "%s: rebuild failed", path);
    if (rebuilt) {
        compare(path, r, rebuilt);
        uint8_t *none = NULL;
        size_t none_len = 0;
        CHECK(pdxe_surface_export(rebuilt, path, &none, &none_len) == PDXE_E_INVALID && !none,
              "%s: a rebuilt result gave a surface", path);
    }

    uint8_t *surface = NULL;
    size_t surface_len = 0;
    CHECK(pdxe_surface_export(r, path, &surface, &surface_len) == PDXE_OK, "%s: export failed",
          path);
    PDXEFileResult *decoded = NULL;
    char *decoded_path = NULL;
    int decoded_lang = 0;
    uint32_t decoded_lost = UINT32_MAX;
    if (surface && pdxe_surface_decode(surface, surface_len, &decoded, &decoded_path,
                                       &decoded_lang, &decoded_lost) == PDXE_OK) {
        CHECK(strcmp(decoded_path, path) == 0 && decoded_lang == lang,
              "%s: surface names another file", path);
        CHECK(decoded_lost == 0, "%s: extraction lost work on a fixture", path);
        uint8_t *again = NULL;
        size_t again_len = 0;
        CHECK(pdxe_surface_encode(decoded, decoded_path, decoded_lang, decoded_lost, &again,
                                  &again_len) == PDXE_OK &&
                  again_len == surface_len && memcmp(again, surface, surface_len) == 0,
              "%s: the surface does not survive decoding", path);
        free(again);
        pdxe_free_result(decoded);
        free(decoded_path);
    } else if (surface) {
        CHECK(0, "%s: the surface does not decode", path);
    }
    pdxe_surface_free(surface);
    pdxe_result_free(ctx, rebuilt);
    pdxe_result_free(ctx, r);
    free(bytes);
}

/* Every source file under `dir`, relative to it. */
static void collect_sources(const char *root, const char *rel, char ***paths, size_t *n) {
    char dir[4096];
    snprintf(dir, sizeof(dir), "%s%s%s", root, rel[0] ? "/" : "", rel);
    DIR *d = opendir(dir);
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
            collect_sources(root, child, paths, n);
        } else if (language_of(child)) {
            *paths = realloc(*paths, (*n + 1) * sizeof(char *));
            (*paths)[(*n)++] = strdup(child);
        }
    }
    closedir(d);
}

static int projects_checked = 0;

static void check_resolution_leaves_surfaces_alone(pdxe_ctx *ctx, const char *dir) {
    char **paths = NULL;
    size_t n = 0;
    collect_sources(dir, "", &paths, &n);
    pdxe_file_result **results = calloc(n ? n : 1, sizeof(*results));
    unsigned char **sources = calloc(n ? n : 1, sizeof(*sources));
    size_t *lens = calloc(n ? n : 1, sizeof(*lens));
    uint8_t **before = calloc(n ? n : 1, sizeof(*before));
    size_t *before_len = calloc(n ? n : 1, sizeof(*before_len));
    pdxe_project *p = NULL;
    CHECK(pdxe_resolve_project_begin(ctx, &p) == PDXE_OK, "%s: cannot begin a project", dir);
    for (size_t i = 0; p && i < n; i++) {
        char full[8200];
        snprintf(full, sizeof(full), "%s/%s", dir, paths[i]);
        sources[i] = read_all(full, &lens[i]);
        int lang = language_of(paths[i]);
        CHECK(sources[i] && pdxe_extract_file(ctx, lang, paths[i], sources[i], lens[i],
                                              &results[i]) == PDXE_OK &&
                  pdxe_surface_export(results[i], paths[i], &before[i], &before_len[i]) ==
                      PDXE_OK &&
                  pdxe_resolve_project_add_file(p, lang, paths[i], sources[i], lens[i],
                                                results[i]) == PDXE_OK,
              "%s/%s: cannot prepare", dir, paths[i]);
        uint8_t *during = NULL;
        size_t during_len = 0;
        CHECK(pdxe_surface_export(results[i], paths[i], &during, &during_len) == PDXE_E_INVALID,
              "%s/%s: a surface was given while a project held the result", dir, paths[i]);
        pdxe_surface_free(during);
    }
    CHECK(p && pdxe_resolve_project_run(p) == PDXE_OK, "%s: resolution failed", dir);
    pdxe_resolve_project_end(p);
    for (size_t i = 0; i < n; i++) {
        uint8_t *after = NULL;
        size_t after_len = 0;
        CHECK(results[i] &&
                  pdxe_surface_export(results[i], paths[i], &after, &after_len) == PDXE_OK &&
                  after_len == before_len[i] && memcmp(after, before[i], after_len) == 0,
              "%s/%s: resolving the project changed the file's surface", dir, paths[i]);
        pdxe_surface_free(after);
        pdxe_surface_free(before[i]);
        pdxe_result_free(ctx, results[i]);
        free(sources[i]);
        free(paths[i]);
    }
    free(results);
    free(sources);
    free(lens);
    free(before);
    free(before_len);
    free(paths);
    projects_checked++;
}

static void walk(pdxe_ctx *ctx, const char *dir) {
    DIR *d = opendir(dir);
    if (!d) {
        return;
    }
    struct dirent *e;
    while ((e = readdir(d)) != NULL) {
        if (e->d_name[0] == '.') {
            continue;
        }
        char path[4096];
        snprintf(path, sizeof(path), "%s/%s", dir, e->d_name);
        struct stat st;
        if (stat(path, &st) != 0) {
            continue;
        }
        if (S_ISDIR(st.st_mode)) {
            char expected[8200];
            snprintf(expected, sizeof(expected), "%s/expected.tsv", path);
            if (stat(expected, &st) == 0) {
                check_resolution_leaves_surfaces_alone(ctx, path);
            }
            walk(ctx, path);
        } else {
            check_file(ctx, path);
        }
    }
    closedir(d);
}

/* Replaces the first occurrence of `from` in `s` with `to`, into a new string. */
static char *replace(const char *s, const char *from, const char *to) {
    const char *at = strstr(s, from);
    if (!at) {
        return NULL;
    }
    size_t n = strlen(s) - strlen(from) + strlen(to);
    char *out = malloc(n + 1);
    size_t head = (size_t)(at - s);
    memcpy(out, s, head);
    strcpy(out + head, to);
    strcat(out, at + strlen(from));
    return out;
}

static void expect_refused(const char *what, const char *bytes) {
    PDXEFileResult *r = NULL;
    char *path = NULL;
    int lang = 0;
    uint32_t lost = 0;
    int rc = bytes ? pdxe_surface_decode((const uint8_t *)bytes, strlen(bytes), &r, &path, &lang,
                                         &lost)
                   : PDXE_E_INVALID;
    CHECK(bytes && rc == PDXE_E_INVALID && !r && !path, "a surface %s was accepted", what);
    if (r) {
        pdxe_free_result(r);
    }
    free(path);
}

static void check_refusals(pdxe_ctx *ctx) {
    static const char source[] = "class A:\n    def m(self, x: int) -> int:\n        return x\n";
    pdxe_file_result *r = NULL;
    uint8_t *surface = NULL;
    size_t len = 0;
    if (pdxe_extract_file(ctx, pdxe_language_id("python"), "a.py", (const uint8_t *)source,
                          sizeof(source) - 1, &r) != PDXE_OK ||
        pdxe_surface_export(r, "a.py", &surface, &len) != PDXE_OK) {
        CHECK(0, "cannot make a surface to corrupt");
        pdxe_result_free(ctx, r);
        return;
    }
    char *good = malloc(len + 1);
    memcpy(good, surface, len);
    good[len] = '\0';

    char *truncated = strdup(good);
    truncated[len / 2] = '\0';
    expect_refused("cut short", truncated);
    free(truncated);

    char *m;
    expect_refused("of another version", m = replace(good, "\"v\":2", "\"v\":1"));
    free(m);
    expect_refused("without what extraction lost", m = replace(good, "\"lost\":0,", ""));
    free(m);
    expect_refused("with a negative loss", m = replace(good, "\"lost\":0", "\"lost\":-1"));
    free(m);
    expect_refused("without a field", m = replace(good, "\"lsp_skipped\":false,", ""));
    free(m);
    expect_refused("with an extra field", m = replace(good, "\"lsp_skipped\":false",
                                                      "\"lsp_skipped\":false,\"extra\":1"));
    free(m);
    expect_refused("with a field of the wrong type",
                   m = replace(good, "\"lsp_skipped\":false", "\"lsp_skipped\":0"));
    free(m);
    expect_refused("with an extra top-level key", m = replace(good, "\"v\":2", "\"v\":2,\"x\":0"));
    free(m);
    /* A definition's bases: counted bases with no array, and a NULL base. */
    pdxe_definition with_bases = r->defs[0];
    pdxe_file_result *refused = NULL;
    with_bases.n_base_classes = 1;
    with_bases.base_classes = NULL;
    CHECK(pdxe_result_build(ctx, &with_bases, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                            &refused) == PDXE_E_INVALID && !refused,
          "a definition counting bases it has no array for was rebuilt");
    const char *null_base[] = {"Base", NULL};
    with_bases.n_base_classes = 2;
    with_bases.base_classes = null_base;
    CHECK(pdxe_result_build(ctx, &with_bases, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                            &refused) == PDXE_E_INVALID && !refused,
          "a definition with a NULL base was rebuilt");

    expect_refused("whose count disagrees with its array",
                   m = replace(good, "\"signature_param_count\":1", "\"signature_param_count\":2"));
    free(m);

    free(good);
    pdxe_surface_free(surface);
    pdxe_result_free(ctx, r);
}

int main(int argc, char **argv) {
    if (argc < 2) {
        fprintf(stderr, "usage: abi_result_build_roundtrip <fixture dir>...\n");
        return 2;
    }
    pdxe_ctx *ctx = NULL;
    if (pdxe_init(&ctx) != PDXE_OK) {
        return 1;
    }
    for (int a = 1; a < argc; a++) {
        walk(ctx, argv[a]);
    }
    check_refusals(ctx);
    pdxe_shutdown(ctx);
    CHECK(defs_without_bases > 0 && defs_with_one_base > 0 && defs_with_several_bases > 0,
          "bases seen: %d definitions with none, %d with one, %d with several",
          defs_without_bases, defs_with_one_base, defs_with_several_bases);
    printf("%s: %d files, %d projects, %d failures\n", failures ? "FAIL" : "ok", files_checked,
           projects_checked, failures);
    return failures || files_checked == 0 ? 1 : 0;
}
