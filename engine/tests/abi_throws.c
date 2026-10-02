/*
 * Throws cross the interface, and so does a walk cut short.
 *
 *     abi_throws <throws fixture dir>
 *     abi_throws --truncated <throws fixture dir>
 *
 * Each fixture raises an exception in a definition of its own: every one is reported,
 * with the exception as the source spells it and the definition it is in, and without
 * a position, which the engine does not record for throws. Nothing is truncated. A
 * result rebuilt from a cache carries no throws: they are the cache's to keep.
 *
 * The Java fixture also declares an exception its method does not throw. The engine
 * reads declared exceptions only where the grammar names them as a field, and the
 * Java grammar does not, so only the throw statement is reported (issue 25). The
 * count is exact on purpose: if the engine starts reading declarations, this test
 * says so.
 *
 * With --truncated the environment sets a node budget too small for any fixture, and
 * every result must say it was truncated.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "pdxe.h"

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

/* One fixture and what it must report: exception and enclosing definition, by name. */
typedef struct {
    const char *file;
    const char *lang;
    const char *expected[2][2];
    int n_expected;
} fixture;

static const fixture FIXTURES[] = {
    {"Thrower.java", "java", {{"IllegalArgumentException", "read"}}, 1},
    {"raiser.py", "python", {{"ValueError", "parse"}}, 1},
    {"thrower.ts", "typescript", {{"RangeError", "check"}}, 1},
};

static unsigned char *read_all(const char *path, size_t *len) {
    FILE *f = fopen(path, "rb");
    if (!f) {
        return NULL;
    }
    fseek(f, 0, SEEK_END);
    long n = ftell(f);
    fseek(f, 0, SEEK_SET);
    unsigned char *buf = malloc(n > 0 ? (size_t)n : 1);
    *len = buf ? fread(buf, 1, (size_t)n, f) : 0;
    fclose(f);
    return buf;
}

static int zero_span(pdxe_span s) {
    return s.start_byte == 0 && s.end_byte == 0 && s.start_line == 0 && s.start_col == 0 &&
           s.end_line == 0 && s.end_col == 0;
}

static int reported(const pdxe_file_result *r, const char *exception, const char *scope) {
    for (uint32_t i = 0; i < r->n_throws; i++) {
        const pdxe_throw *t = &r->throws[i];
        if (t->scope_index == PDXE_NO_PARENT || t->scope_index >= r->n_defs) {
            continue;
        }
        if (strcmp(t->exception_text, exception) == 0 &&
            strcmp(r->defs[t->scope_index].name, scope) == 0) {
            return 1;
        }
    }
    return 0;
}

int main(int argc, char **argv) {
    int truncated = argc == 3 && strcmp(argv[1], "--truncated") == 0;
    if (argc != 2 && !truncated) {
        fprintf(stderr, "usage: abi_throws [--truncated] <throws fixture dir>\n");
        return 2;
    }
    const char *dir = argv[argc - 1];
    pdxe_ctx *ctx = NULL;
    if (pdxe_init(&ctx) != PDXE_OK) {
        return 1;
    }
    for (size_t f = 0; f < sizeof(FIXTURES) / sizeof(FIXTURES[0]); f++) {
        const fixture *fx = &FIXTURES[f];
        char path[4096];
        snprintf(path, sizeof(path), "%s/%s", dir, fx->file);
        size_t len = 0;
        unsigned char *src = read_all(path, &len);
        pdxe_file_result *r = NULL;
        if (!src || pdxe_extract_file(ctx, pdxe_language_id(fx->lang), fx->file, src, len, &r) !=
                        PDXE_OK) {
            printf("FAIL %s: cannot extract\n", fx->file);
            return 1;
        }
        if (truncated) {
            CHECK(r->truncated == 1, "%s: not reported as truncated under a tiny budget",
                  fx->file);
        } else {
            CHECK(r->truncated == 0, "%s: reported as truncated with no budget", fx->file);
            for (int e = 0; e < fx->n_expected; e++) {
                CHECK(reported(r, fx->expected[e][0], fx->expected[e][1]),
                      "%s: %s thrown in %s is not reported", fx->file, fx->expected[e][0],
                      fx->expected[e][1]);
            }
            CHECK(r->n_throws == (uint32_t)fx->n_expected, "%s: %u throws, expected %d",
                  fx->file, r->n_throws, fx->n_expected);
            for (uint32_t i = 0; i < r->n_throws; i++) {
                CHECK(zero_span(r->throws[i].span), "%s: throw %u has a position", fx->file, i);
            }

            pdxe_file_result *rebuilt = NULL;
            CHECK(pdxe_result_build(ctx, r->defs, r->n_defs, r->calls, r->n_calls, r->imports,
                                    r->n_imports, r->usages, r->n_usages, r->types, r->n_types,
                                    r->rws, r->n_rws, &rebuilt) == PDXE_OK,
                  "%s: cannot rebuild", fx->file);
            if (rebuilt) {
                CHECK(rebuilt->n_throws == 0 && rebuilt->throws != NULL &&
                          rebuilt->truncated == 0 && rebuilt->extraction_lost == 0,
                      "%s: a rebuilt result carries throws, a truncation or lost work",
                      fx->file);
                pdxe_result_free(ctx, rebuilt);
            }
        }
        pdxe_result_free(ctx, r);
        free(src);
    }
    pdxe_shutdown(ctx);
    if (failures) {
        printf("%d failure(s)\n", failures);
        return 1;
    }
    printf("ok\n");
    return 0;
}
