/*
 * A site's node-type path is complete or absent, and one file's paths are bounded
 * (docs/plan/ISSUES.md, issue 88; decision 57;
 * engine/patches/0020-bounded-site-node-type-paths.patch).
 *
 *     site_path_budget
 *
 * Every call, possible callable reference and route binding stored the node types from
 * its owner down to itself, so n calls nested inside one another stored about n²
 * entries: 10,000 of them, 30 KB of C, stored 100,030,000. Now one budget per file,
 * shared by every producer, holds a path to PDXE_AST_PATH_MAX_DEPTH entries and all of
 * them together to PDXE_AST_PATH_MAX_TOTAL_ENTRIES, checked before anything is
 * allocated. The first request past either is lost work, once, and no path is
 * computed or allocated for the file after it.
 *
 *   - The path builder itself (pdxe_site_path_alloc): 1024 entries are given and 1025
 *     refused; requests totalling 499,999 and 500,000 are given and the one that would
 *     make 500,001 refused; after a refusal nothing is asked of the arena, whatever the
 *     size, and lost work is counted once.
 *   - Whole extractions, the counts written here and found again in the counters:
 *     - C calls nested 254 deep, 512 deep as a tree (issue 89's ceiling), keep every
 *       path, the innermost 510 entries; one call more is 514 deep and is not extracted
 *       at all, no path asked for. A path of 1024 entries cannot be reached by an
 *       extracted file: the per-path ceiling is the builder's, defence in depth;
 *     - statements of 100 nested calls cross the total at a call this test computes
 *       from the grammar's path shape, and every later site, a shallow one included,
 *       has no path;
 *     - every path kept is complete: from the owner's node type down to the call's,
 *       of exactly its nesting's length, never cut short;
 *     - Python route bindings, calls and callable references draw on one counter: the
 *       entries counted are exactly those of the distinct paths kept, and routes
 *       spent first move the call that crosses the total earlier.
 */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "lost_work.h"
#include "pdxe.h"
#include "pdxe_core.h"
#include "shim_internal.h"
#include "foundation/arena.h"

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

/* A growing string. */
typedef struct {
    char *text;
    size_t len;
    size_t cap;
} buf_t;

static void put(buf_t *b, const char *s) {
    size_t n = strlen(s);
    if (b->len + n + 1 > b->cap) {
        size_t cap = b->cap ? b->cap : 256;
        while (b->len + n + 1 > cap) {
            cap *= 2;
        }
        char *grown = realloc(b->text, cap);
        if (!grown) {
            printf("FAIL out of memory\n");
            exit(1);
        }
        b->text = grown;
        b->cap = cap;
    }
    memcpy(b->text + b->len, s, n + 1);
    b->len += n;
}

static void repeat(buf_t *b, const char *s, long times) {
    for (long i = 0; i < times; i++) {
        put(b, s);
    }
}

static uint64_t lost_now(void) {
    return pdxe_losses().work;
}

static uint32_t length_of(const char **path) {
    uint32_t n = 0;
    while (path && path[n]) {
        n++;
    }
    return n;
}

/* ------------------------------------------------------------------------------------ */
/* The path builder. */

static void check_builder(void) {
    CHECK(PDXE_AST_PATH_MAX_DEPTH == 1024u && PDXE_AST_PATH_MAX_TOTAL_ENTRIES == 500000u,
          "a ceiling is not the value decision 57 records");

    PDXEArena arena;
    pdxe_arena_init(&arena);

    /* One path: 1024 entries are given, 1025 refused. */
    PDXEFileResult *result = calloc(1, sizeof(*result));
    uint64_t before = lost_now();
    const char **path = pdxe_site_path_alloc(&arena, result, PDXE_AST_PATH_MAX_DEPTH);
    CHECK(path && path[PDXE_AST_PATH_MAX_DEPTH] == NULL && result->site_paths.entries == 1024 &&
              !result->site_paths.exhausted && lost_now() == before,
          "a path of 1024 entries was refused");
    path = pdxe_site_path_alloc(&arena, result, PDXE_AST_PATH_MAX_DEPTH + 1);
    CHECK(!path && result->site_paths.exhausted && lost_now() == before + 1 &&
              result->site_paths.entries == 1024 && result->site_paths.requests == 1,
          "a path of 1025 entries was given, or lost no work");
    free(result);

    /* The total: 499,999 and then 500,000 are given; 500,001 is refused, and after it
     * nothing, even one entry, is asked of the arena, and no more work is lost. */
    result = calloc(1, sizeof(*result));
    before = lost_now();
    uint32_t requests = 0;
    for (uint32_t given = 0; given + 1000 <= 499000; given += 1000) {
        path = pdxe_site_path_alloc(&arena, result, 1000);
        requests++;
        if (!path) {
            CHECK(0, "a path within the total was refused at %u", given);
            break;
        }
    }
    path = pdxe_site_path_alloc(&arena, result, 999);
    requests++;
    CHECK(path && result->site_paths.entries == 499999, "499,999 entries were not given (%u)",
          result->site_paths.entries);
    path = pdxe_site_path_alloc(&arena, result, 1);
    requests++;
    CHECK(path && result->site_paths.entries == 500000 && !result->site_paths.exhausted &&
              lost_now() == before,
          "the 500,000th entry was refused");
    path = pdxe_site_path_alloc(&arena, result, 1);
    CHECK(!path && result->site_paths.exhausted && result->site_paths.entries == 500000 &&
              lost_now() == before + 1,
          "the 500,001st entry was given, or lost no work");
    for (int i = 0; i < 3; i++) {
        path = pdxe_site_path_alloc(&arena, result, 1);
        CHECK(!path, "a path was given after the budget was spent");
    }
    CHECK(result->site_paths.requests == requests && result->site_paths.paths == requests &&
              lost_now() == before + 1,
          "the arena was asked %u times, not %u, or work was lost again",
          result->site_paths.requests, requests);
    free(result);

    /* A total reached by one request too many, without wrapping: the largest request
     * is refused as soon as it is larger than what remains. */
    result = calloc(1, sizeof(*result));
    result->site_paths.entries = PDXE_AST_PATH_MAX_TOTAL_ENTRIES - 10;
    path = pdxe_site_path_alloc(&arena, result, 11);
    CHECK(!path && result->site_paths.exhausted, "a request past what remains was given");
    free(result);
    pdxe_arena_destroy(&arena);
}

/* ------------------------------------------------------------------------------------ */
/* Whole extractions. */

static pdxe_ctx *ctx = NULL;

typedef struct {
    pdxe_file_result *r;
    const PDXEFileResult *e;
    uint32_t lost;
} extraction_t;

static extraction_t extract(const char *language, const char *rel, const char *source) {
    extraction_t x = {0};
    if (pdxe_extract_file(ctx, pdxe_language_id(language), rel, (const uint8_t *)source,
                          strlen(source), &x.r) != PDXE_OK ||
        !x.r) {
        printf("FAIL %s: the extraction failed\n", rel);
        exit(1);
    }
    x.e = pdxe_result_engine(x.r);
    x.lost = x.r->extraction_lost;
    return x;
}

/* Whether `path` is exactly the path of the k-th of a chain of nested C calls (k from
 * 1), under `prefix`: the prefix, the outer call, then argument_list and call_expression
 * once for each level in. */
static int is_nested_call_path(const char **path, const char *const *prefix, int k) {
    int i = 0;
    for (; prefix[i]; i++) {
        if (!path[i] || strcmp(path[i], prefix[i]) != 0) {
            return 0;
        }
    }
    if (!path[i] || strcmp(path[i++], "call_expression") != 0) {
        return 0;
    }
    for (int level = 1; level < k; level++) {
        if (!path[i] || strcmp(path[i++], "argument_list") != 0 || !path[i] ||
            strcmp(path[i++], "call_expression") != 0) {
            return 0;
        }
    }
    return path[i] == NULL;
}

static const char *const FILE_SCOPE[] = {"translation_unit", "declaration", "init_declarator",
                                         NULL};

/* `depth` C calls nested in one initialiser; with `paren`, the innermost wrapped in
 * parentheses, one node type deeper. The innermost path is 2 × depth + 2 entries, and
 * one more with `paren`. */
static char *nested(int depth, int paren) {
    buf_t b = {0};
    put(&b, "int v = ");
    repeat(&b, "f(", depth - 1);
    put(&b, paren ? "(f(0))" : "f(0)");
    repeat(&b, ")", depth - 1);
    put(&b, ";\nint after = g(0);\n");
    return b.text;
}

static void check_depth(void) {
    for (int extra = 0; extra <= 1; extra++) {
        char *source = nested(254 + extra, 0);
        extraction_t x = extract("c", "src/deep.c", source);
        if (!extra) {
            int complete = 0;
            uint32_t longest = 0;
            for (int i = 0; i < x.e->calls.count; i++) {
                const PDXECall *c = &x.e->calls.items[i];
                if (strcmp(c->callee_name, "g") == 0) {
                    continue;
                }
                CHECK(c->ast_path && is_nested_call_path(c->ast_path, FILE_SCOPE, i + 1),
                      "call %d's path is not its complete path", i);
                complete += c->ast_path != NULL;
                if (length_of(c->ast_path) > longest) {
                    longest = length_of(c->ast_path);
                }
            }
            CHECK(complete == 254 && longest == 510 && x.lost == 0 &&
                      !x.e->site_paths.exhausted && !x.e->syntax_rejected,
                  "254 nested calls: %d complete paths, the longest %u, lost %u", complete,
                  longest, x.lost);
        } else {
            CHECK(x.e->syntax_rejected && x.lost == 1 && x.e->calls.count == 0 &&
                      x.e->site_paths.requests == 0 && !x.e->site_paths.exhausted,
                  "255 nested calls were not refused before any path (lost %u, %u requests)",
                  x.lost, x.e->site_paths.requests);
        }
        pdxe_result_free(ctx, x.r);
        free(source);
    }
}

/* `statements` statements of `depth` nested calls in one function, then a shallow call.
 * In a function, the k-th call of a chain has a path of 2k + 2 entries, so a statement
 * spends depth² + 3 × depth. */
static char *statements(int count, int depth) {
    buf_t b = {0};
    put(&b, "void g(void) {\n");
    for (int s = 0; s < count; s++) {
        repeat(&b, "f(", depth - 1);
        put(&b, "f(0)");
        repeat(&b, ")", depth - 1);
        put(&b, ";\n");
    }
    put(&b, "    shallow();\n}\n");
    return b.text;
}

static const char *const IN_FUNCTION[] = {"function_definition", "compound_statement",
                                          "expression_statement", NULL};

static void check_total(void) {
    /* 48 statements spend 48 × 10,300 = 494,400 entries; in the 49th, calls 1 to 73
     * spend 73² + 3 × 73 = 5,548 more, 499,948 in all, and the 74th, of 150 entries,
     * would make 500,098. */
    char *source = statements(50, 100);
    extraction_t x = extract("c", "src/wide.c", source);
    CHECK(x.lost == 1 && x.e->site_paths.exhausted && x.e->site_paths.entries == 499948 &&
              x.e->site_paths.paths == 48 * 100 + 73 &&
              x.e->site_paths.requests == x.e->site_paths.paths,
          "the total was not crossed where the grammar says: lost %u, %u entries, %u paths, "
          "%u requests",
          x.lost, x.e->site_paths.entries, x.e->site_paths.paths, x.e->site_paths.requests);
    uint64_t kept = 0;
    int first_absent = -1;
    for (int i = 0; i < x.e->calls.count; i++) {
        const PDXECall *c = &x.e->calls.items[i];
        if (!c->ast_path) {
            if (first_absent < 0) {
                first_absent = i;
            }
            continue;
        }
        CHECK(first_absent < 0, "call %d has a path after call %d had none", i, first_absent);
        kept += length_of(c->ast_path);
        CHECK(is_nested_call_path(c->ast_path, IN_FUNCTION, i % 100 + 1),
              "call %d's path is not its complete path", i);
    }
    CHECK(first_absent == 48 * 100 + 73 && kept == x.e->site_paths.entries &&
              kept <= PDXE_AST_PATH_MAX_TOTAL_ENTRIES,
          "the first call without a path is %d, and %llu entries are kept", first_absent,
          (unsigned long long)kept);
    CHECK(x.e->calls.count == 50 * 100 + 1, "calls were dropped with their paths (%d)",
          x.e->calls.count);
    pdxe_result_free(ctx, x.r);
    free(source);
}

/* Entries of the distinct path arrays a result holds, routes, calls and references. */
static uint64_t distinct_entries(const PDXEFileResult *e, int *routes, int *references) {
    const char ***seen = calloc((size_t)(e->calls.count + e->usages.count + 1) +
                                    (size_t)e->defs.count * 16,
                                sizeof(*seen));
    size_t n_seen = 0;
    uint64_t total = 0;
    *routes = 0;
    *references = 0;
#define SEE(p)                                                                                     \
    do {                                                                                           \
        int dup = 0;                                                                               \
        for (size_t s = n_seen; s > 0 && !dup; s--) {                                              \
            dup = seen[s - 1] == (p);                                                              \
        }                                                                                          \
        if (!dup) {                                                                                \
            seen[n_seen++] = (p);                                                                  \
            total += length_of(p);                                                                 \
        }                                                                                          \
    } while (0)
    for (int i = 0; i < e->defs.count; i++) {
        for (int k = 0; k < e->defs.items[i].route_count && k < 16; k++) {
            if (e->defs.items[i].routes[k].ast_path) {
                SEE(e->defs.items[i].routes[k].ast_path);
                (*routes)++;
            }
        }
    }
    for (int i = 0; i < e->calls.count; i++) {
        if (e->calls.items[i].ast_path) {
            SEE(e->calls.items[i].ast_path);
        }
    }
    for (int i = 0; i < e->usages.count; i++) {
        if (e->usages.items[i].ast_path) {
            SEE(e->usages.items[i].ast_path);
            (*references)++;
        }
    }
#undef SEE
    free(seen);
    return total;
}

/* Python: `routes` Flask route bindings, a function passing callables as values, then
 * statements of nested calls enough to cross the total. */
static char *python_source(int routes) {
    buf_t b = {0};
    char line[160];
    put(&b, "from flask import Flask\napp = Flask(__name__)\n\n");
    for (int i = 0; i < routes; i++) {
        snprintf(line, sizeof(line), "@app.route(\"/r%d\")\ndef r%d():\n    return %d\n\n", i,
                 i, i);
        put(&b, line);
    }
    put(&b, "def handler():\n    pass\n\ndef wire():\n    register(handler)\n    register(r0)\n\n"
            "def work():\n");
    for (int s = 0; s < 60; s++) {
        put(&b, "    ");
        repeat(&b, "f(", 99);
        put(&b, "f(0)");
        repeat(&b, ")", 99);
        put(&b, "\n");
    }
    return b.text;
}

/* The index, among calls named f, of the first without a path. */
static int first_absent_f(const PDXEFileResult *e) {
    int index = 0;
    for (int i = 0; i < e->calls.count; i++) {
        if (strcmp(e->calls.items[i].callee_name, "f") != 0) {
            continue;
        }
        if (!e->calls.items[i].ast_path) {
            return index;
        }
        index++;
    }
    return -1;
}

static void check_shared(void) {
    char *without = python_source(1);
    char *with = python_source(400);
    extraction_t a = extract("python", "app/routes_a.py", without);
    extraction_t b = extract("python", "app/routes_b.py", with);
    int routes_a, refs_a, routes_b, refs_b;
    uint64_t entries_a = distinct_entries(a.e, &routes_a, &refs_a);
    uint64_t entries_b = distinct_entries(b.e, &routes_b, &refs_b);
    CHECK(routes_b == 400 && refs_b >= 2, "the fixture has %d routes and %d references with paths",
          routes_b, refs_b);
    CHECK(entries_a == a.e->site_paths.entries && entries_b == b.e->site_paths.entries,
          "the counter is not the entries kept by routes, calls and references together "
          "(%llu against %u, %llu against %u)",
          (unsigned long long)entries_a, a.e->site_paths.entries, (unsigned long long)entries_b,
          b.e->site_paths.entries);
    CHECK(a.e->site_paths.exhausted && b.e->site_paths.exhausted && a.lost >= 1 && b.lost >= 1,
          "the Python fixtures did not cross the total");
    int fa = first_absent_f(a.e);
    int fb = first_absent_f(b.e);
    CHECK(fa > 0 && fb > 0 && fb < fa,
          "routes spent first did not move the crossing call earlier (%d, then %d)", fa, fb);
    pdxe_result_free(ctx, a.r);
    pdxe_result_free(ctx, b.r);
    free(without);
    free(with);
}

int main(void) {
    check_builder();
    if (pdxe_init(&ctx) != PDXE_OK) {
        printf("FAIL the engine did not start\n");
        return 1;
    }
    check_depth();
    check_total();
    check_shared();
    pdxe_shutdown(ctx);
    if (failures) {
        printf("%d failure(s)\n", failures);
        return 1;
    }
    printf("ok: site paths are complete or absent, and bounded per file\n");
    return 0;
}
