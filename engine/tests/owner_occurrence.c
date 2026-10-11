/*
 * A site is joined to the definition that owns it by declaration occurrence (issue 76;
 * decision 48), never by comparing qualified names.
 *
 *     owner_occurrence <corpus dir>
 *
 * The index first. A definition is found by its exact occurrence, the buffer included:
 * a raw and a macro-expanded occurrence at the same numeric byte span are two, and
 * neither finds the other. Two definitions with one occurrence are reported
 * ambiguous, never the first of them. An occurrence with no bytes is never found. A
 * scan of the same definitions, which lookups fall back to when the table cannot be
 * allocated, answers exactly as the table does.
 *
 * Then the corpus (bench/corpus, laid out as corpus_extracts reads it), every file
 * extracted through the interface. No two callable definitions of a file share an
 * occurrence. A call or callable-reference site whose bytes lie within a callable's
 * raw occurrence is owned by a definition, never by the file. A site owned by a
 * callable read from the raw source lies within that callable's occurrence.
 */

#include <dirent.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

#include "matrix_languages.h"
#include "pdxe.h"
#include "pdxe_core.h"
#include "shim_internal.h"

static int failures = 0;
static int checked_sites = 0;

#define FAIL(...)                                                                                  \
    do {                                                                                           \
        printf("FAIL ");                                                                           \
        printf(__VA_ARGS__);                                                                       \
        printf("\n");                                                                              \
        failures++;                                                                                \
    } while (0)

static PDXECallableOccurrence occ(uint32_t start, uint32_t end, PDXESourceOrigin origin) {
    return (PDXECallableOccurrence){start, end, (uint8_t)origin};
}

static void expect_found(const PDXEOccurrenceIndex *index, PDXECallableOccurrence o, int want,
                         const char *what) {
    int got = pdxe_occurrence_index_find(index, o);
    if (got != want) {
        FAIL("%s: %u-%u in buffer %u found %d, want %d", what, o.start_byte, o.end_byte,
             (unsigned)o.origin, got, want);
    }
}

static void check_index(const PDXEDefinition *defs, int count, int with_table) {
    PDXEOccurrenceIndex index;
    if (with_table) {
        pdxe_occurrence_index_build(&index, defs, count);
        if (!index.slots) {
            FAIL("the table was not built");
        }
    } else {
        index = (PDXEOccurrenceIndex){defs, count, NULL, 0};
    }
    const char *how = with_table ? "table" : "scan";
    expect_found(&index, occ(10, 20, PDXE_SOURCE_ORIGIN_RAW), 0, how);
    expect_found(&index, occ(10, 20, PDXE_SOURCE_ORIGIN_PREPROCESSED), 1, how);
    expect_found(&index, occ(30, 40, PDXE_SOURCE_ORIGIN_RAW), 2, how);
    expect_found(&index, occ(30, 40, PDXE_SOURCE_ORIGIN_PREPROCESSED), PDXE_OCCURRENCE_NONE, how);
    expect_found(&index, occ(10, 21, PDXE_SOURCE_ORIGIN_RAW), PDXE_OCCURRENCE_NONE, how);
    expect_found(&index, occ(11, 20, PDXE_SOURCE_ORIGIN_RAW), PDXE_OCCURRENCE_NONE, how);
    expect_found(&index, occ(50, 60, PDXE_SOURCE_ORIGIN_RAW), PDXE_OCCURRENCE_AMBIGUOUS, how);
    expect_found(&index, occ(0, 0, PDXE_SOURCE_ORIGIN_RAW), PDXE_OCCURRENCE_NONE, how);
    if (with_table) {
        pdxe_occurrence_index_free(&index);
    }
}

static void index_tests(void) {
    PDXEDefinition defs[6];
    memset(defs, 0, sizeof(defs));
    defs[0].occurrence = occ(10, 20, PDXE_SOURCE_ORIGIN_RAW);
    defs[1].occurrence = occ(10, 20, PDXE_SOURCE_ORIGIN_PREPROCESSED); /* the same numbers */
    defs[2].occurrence = occ(30, 40, PDXE_SOURCE_ORIGIN_RAW);
    defs[3].occurrence = occ(50, 60, PDXE_SOURCE_ORIGIN_RAW); /* two with one occurrence */
    defs[4].occurrence = occ(50, 60, PDXE_SOURCE_ORIGIN_RAW);
    /* defs[5] is no callable: no occurrence */
    check_index(defs, 6, 1);
    check_index(defs, 6, 0);
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

/* The engine definition behind the interface's definition `position`: the interface
 * leaves out the engine's synthetic ones, named by a path in angle brackets. */
static const PDXEDefinition *engine_definition(const PDXEFileResult *engine, uint32_t position) {
    uint32_t kept = 0;
    for (int i = 0; i < engine->defs.count; i++) {
        const PDXEDefinition *d = &engine->defs.items[i];
        if (d->file_path && d->file_path[0] == '<') {
            continue;
        }
        if (kept++ == position) {
            return d;
        }
    }
    return NULL;
}

static int raw_occurrence_holds(PDXECallableOccurrence o, uint32_t start, uint32_t end) {
    return pdxe_occurrence_valid(o) && o.origin == PDXE_SOURCE_ORIGIN_RAW && o.start_byte <= start &&
           end <= o.end_byte;
}

static void check_file(pdxe_ctx *ctx, const char *lang, const char *path, const char *rel) {
    size_t len = 0;
    unsigned char *bytes = read_all(path, &len);
    pdxe_file_result *r = NULL;
    if (!bytes || pdxe_extract_file(ctx, pdxe_language_id(lang), rel, bytes, len, &r) != PDXE_OK) {
        FAIL("%s: cannot extract", rel);
        free(bytes);
        return;
    }
    const PDXEFileResult *engine = pdxe_result_engine(r);
    PDXEOccurrenceIndex index;
    pdxe_occurrence_index_build(&index, engine->defs.items, engine->defs.count);
    for (int i = 0; i < engine->defs.count; i++) {
        if (pdxe_occurrence_index_find(&index, engine->defs.items[i].occurrence) ==
            PDXE_OCCURRENCE_AMBIGUOUS) {
            FAIL("%s: two definitions share the occurrence of %s", rel,
                 engine->defs.items[i].qualified_name);
        }
    }
    pdxe_occurrence_index_free(&index);
    for (uint32_t i = 0; i < r->n_calls; i++) {
        const pdxe_call *c = &r->calls[i];
        if (c->span.end_byte <= c->span.start_byte || c->n_ast_path == 0) {
            continue; /* never a site */
        }
        checked_sites++;
        const PDXEDefinition *owner =
            c->caller_index != PDXE_NO_PARENT && c->caller_index < r->n_defs &&
                    strcmp(r->defs[c->caller_index].kind, "module") != 0
                ? engine_definition(engine, c->caller_index)
                : NULL;
        int inside = 0;
        for (int k = 0; k < engine->defs.count && !inside; k++) {
            inside = raw_occurrence_holds(engine->defs.items[k].occurrence, c->span.start_byte,
                                          c->span.end_byte);
        }
        if (inside && !owner) {
            FAIL("%s:%u: %s lies in a callable and is owned by the file", rel, c->span.start_line,
                 c->callee_text);
        }
        if (owner && pdxe_occurrence_valid(owner->occurrence) &&
            owner->occurrence.origin == PDXE_SOURCE_ORIGIN_RAW &&
            !raw_occurrence_holds(owner->occurrence, c->span.start_byte, c->span.end_byte)) {
            FAIL("%s:%u: %s is owned by %s, which does not hold it", rel, c->span.start_line,
                 c->callee_text, owner->qualified_name);
        }
    }
    pdxe_result_free(ctx, r);
    free(bytes);
}

static void walk(pdxe_ctx *ctx, const char *lang, const char *dir, const char *rel) {
    DIR *d = opendir(dir);
    if (!d) {
        FAIL("%s: cannot open", dir);
        return;
    }
    struct dirent *e;
    while ((e = readdir(d)) != NULL) {
        if (e->d_name[0] == '.') {
            continue;
        }
        char path[4096];
        char child[4096];
        snprintf(path, sizeof(path), "%s/%s", dir, e->d_name);
        snprintf(child, sizeof(child), "%s/%s", rel, e->d_name);
        struct stat st;
        if (stat(path, &st) != 0) {
            continue;
        }
        if (S_ISDIR(st.st_mode)) {
            walk(ctx, lang, path, child);
        } else {
            check_file(ctx, lang, path, child);
        }
    }
    closedir(d);
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: owner_occurrence <corpus dir>\n");
        return 2;
    }
    index_tests();
    pdxe_ctx *ctx = NULL;
    if (pdxe_init(&ctx) != PDXE_OK) {
        return 1;
    }
    for (int i = 0; i < N_MATRIX_LANGUAGES; i++) {
        char dir[4096];
        snprintf(dir, sizeof(dir), "%s/%s", argv[1], MATRIX_LANGUAGES[i]);
        struct stat st;
        if (stat(dir, &st) == 0 && S_ISDIR(st.st_mode)) {
            walk(ctx, MATRIX_LANGUAGES[i], dir, MATRIX_LANGUAGES[i]);
        }
    }
    pdxe_shutdown(ctx);
    printf("owner_occurrence: %d sites checked, %d failures\n", checked_sites, failures);
    return failures == 0 && checked_sites > 0 ? 0 : 1;
}
