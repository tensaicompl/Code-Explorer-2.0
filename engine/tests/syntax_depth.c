/*
 * Semantic extraction is admitted by syntax depth (docs/plan/ISSUES.md, issue 89;
 * decision 58; engine/patches/0021-bounded-syntax-depth.patch).
 *
 *     syntax_depth
 *
 * Extraction's work and memory grew faster than linearly with how deeply a file's
 * syntax nests, in many places: a few KB of Java classes or JavaScript functions nested
 * 1,000 deep took 45 to 56 seconds. Now, after the parse and before any semantic pass,
 * one cursor walk measures the tree's depth (the root at depth 1), stopping at the first
 * node deeper than PDXE_MAX_SYNTAX_DEPTH, 512. A deeper tree is not extracted: the result
 * is valid, holds no fact, counts lost work once, runs no semantic pass and skips
 * cross-file resolution. The macro-expanded tree of C and C++ is held to the same depth,
 * its pass skipped alone.
 *
 *   - The scan: the root alone is depth 1; it visits every node once; it stops at the
 *     first node past a depth or node ceiling; it says whether an ERROR or MISSING node
 *     holds the deepest node.
 *   - Depths 511 and 512 are extracted, 513 is not: the scan stops at depth 513, no
 *     semantic pass runs, no fact or path is made, work is lost once.
 *   - Java classes, JavaScript functions, a Python member chain and C calls, nested past
 *     the ceiling, are refused alike; nested half as deep they are extracted.
 *   - The expanded tree past the ceiling skips the expanded pass alone.
 *   - With issue 88: a tree under the depth whose paths pass their budget loses work to
 *     the budget; a tree over it loses work to the depth before any path is asked for.
 */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "lang_specs.h"
#include "pdxe.h"
#include "pdxe_core.h"
#include "shim_internal.h"
#include "tree_sitter/api.h"

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

/* ------------------------------------------------------------------------------------ */
/* The scan. */

static TSTree *parse_c(const char *source) {
    TSParser *parser = ts_parser_new();
    ts_parser_set_language(parser, pdxe_ts_language(PDXE_LANG_C));
    TSTree *tree = ts_parser_parse_string(parser, NULL, source, (uint32_t)strlen(source));
    ts_parser_delete(parser);
    return tree;
}

/* `k` parentheses around 0 in an initialiser: translation_unit, declaration,
 * init_declarator, then k parenthesized_expression, and their children one deeper. */
static char *parens(int k) {
    buf_t b = {0};
    put(&b, "int v = ");
    repeat(&b, "(", k);
    put(&b, "0");
    repeat(&b, ")", k);
    put(&b, ";\n");
    return b.text;
}

static void check_scan(void) {
    CHECK(PDXE_MAX_SYNTAX_DEPTH == 512u, "the ceiling is not the value decision 58 records");

    TSTree *tree = parse_c("");
    PDXESyntaxDepthScan scan = pdxe_syntax_depth_scan(ts_tree_root_node(tree), 0, 0);
    CHECK(scan.depth == 1 && scan.nodes == 1 && !scan.rejected,
          "an empty file is not the root alone, at depth 1 (%u, %u nodes)", scan.depth,
          scan.nodes);
    ts_tree_delete(tree);

    /* int v; is translation_unit > declaration > (primitive_type, identifier, ;). */
    tree = parse_c("int v;");
    scan = pdxe_syntax_depth_scan(ts_tree_root_node(tree), 0, 0);
    CHECK(scan.depth == 3 && scan.nodes == ts_node_descendant_count(ts_tree_root_node(tree)),
          "int v; is not 3 deep with every node visited once (%u, %u)", scan.depth, scan.nodes);
    ts_tree_delete(tree);

    /* k parentheses are k + 4 deep; a full scan visits every node exactly once. */
    for (int k = 1; k <= 300; k += 37) {
        char *source = parens(k);
        tree = parse_c(source);
        TSNode root = ts_tree_root_node(tree);
        scan = pdxe_syntax_depth_scan(root, 0, 0);
        CHECK(scan.depth == (uint32_t)k + 4 && scan.nodes == ts_node_descendant_count(root) &&
                  !scan.recovery_on_path,
              "%d parentheses: depth %u, %u of %u nodes", k, scan.depth, scan.nodes,
              ts_node_descendant_count(root));
        /* Stopped at the first node past a depth: never deeper than one past it. */
        scan = pdxe_syntax_depth_scan(root, 10, 0);
        if (k + 4 > 10) {
            CHECK(scan.rejected && !scan.out_of_nodes && scan.depth == 11 &&
                      scan.nodes < ts_node_descendant_count(root),
                  "%d parentheses: the scan did not stop at depth 11 (%u)", k, scan.depth);
        } else {
            CHECK(!scan.rejected && scan.depth == (uint32_t)k + 4,
                  "%d parentheses: refused under the ceiling", k);
        }
        /* And at the first node past a count. */
        scan = pdxe_syntax_depth_scan(root, 0, 5);
        CHECK(scan.rejected && scan.out_of_nodes && scan.nodes == 6,
              "%d parentheses: the scan did not stop at node 6 (%u)", k, scan.nodes);
        ts_tree_delete(tree);
        free(source);
    }

    /* The deepest node under parser recovery says so. */
    tree = parse_c("@@ ((((((((((0))))))))))\n");
    scan = pdxe_syntax_depth_scan(ts_tree_root_node(tree), 0, 0);
    CHECK(scan.recovery_on_path, "an ERROR or MISSING node on the deepest path was not seen");
    ts_tree_delete(tree);
}

/* ------------------------------------------------------------------------------------ */
/* Whole extractions. */

static pdxe_ctx *ctx = NULL;

typedef struct {
    pdxe_file_result *r;
    const PDXEFileResult *e;
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
    return x;
}

static int calls(const PDXEFileResult *e, const char *callee) {
    int n = 0;
    for (int i = 0; i < e->calls.count; i++) {
        n += strcmp(e->calls.items[i].callee_name, callee) == 0;
    }
    return n;
}

/* A refused file: no semantic pass, no fact, no path, no cross-file resolution, work lost
 * once, and no parse failure. */
static void check_refused(const char *what, extraction_t x) {
    const PDXEFileResult *e = x.e;
    CHECK(e->syntax_rejected && e->syntax.rejected && e->syntax.depth == PDXE_MAX_SYNTAX_DEPTH + 1,
          "%s: not refused at the first node past the ceiling (%u)", what, e->syntax.depth);
    CHECK(e->semantic_passes == 0 && e->defs.count == 0 && e->calls.count == 0 &&
              e->usages.count == 0 && e->imports.count == 0 && e->resolved_calls.count == 0 &&
              e->site_paths.requests == 0 && e->walk_nodes_visited == 0,
          "%s: semantic work ran (%u passes, %d defs, %d calls)", what, e->semantic_passes,
          e->defs.count, e->calls.count);
    CHECK(x.r->extraction_lost == 1 && e->lsp_skipped && !e->has_error && !x.r->truncated,
          "%s: lost %u, lsp_skipped %d, has_error %d", what, x.r->extraction_lost,
          (int)e->lsp_skipped, (int)e->has_error);
    CHECK(e->syntax.nodes < e->tree_nodes, "%s: the scan did not stop early (%u of %u)", what,
          e->syntax.nodes, e->tree_nodes);
}

/* A function returning f(0) inside `k` parentheses: function_definition,
 * compound_statement, return_statement, k parenthesized_expression, then call_expression,
 * argument_list and its children: k + 7 deep. */
static char *returning(int k) {
    buf_t b = {0};
    put(&b, "int g(void) { return ");
    repeat(&b, "(", k);
    put(&b, "f(0)");
    repeat(&b, ")", k);
    put(&b, "; }\n");
    return b.text;
}

static void check_boundary(void) {
    for (uint32_t depth = 511; depth <= 513; depth++) {
        char *source = returning((int)depth - 7);
        extraction_t x = extract("c", "src/deep.c", source);
        if (depth <= PDXE_MAX_SYNTAX_DEPTH) {
            CHECK(!x.e->syntax_rejected && x.e->syntax.depth == depth && x.e->semantic_passes > 0 &&
                      calls(x.e, "f") == 1 && x.r->extraction_lost == 0 && !x.e->lsp_skipped,
                  "depth %u was not extracted normally (scan %u, %u passes, lost %u)", depth,
                  x.e->syntax.depth, x.e->semantic_passes, x.r->extraction_lost);
        } else {
            check_refused("depth 513", x);
        }
        pdxe_result_free(ctx, x.r);
        free(source);
    }
}

/* The issue's shapes, nested past the ceiling and half as deep. */
static void check_reproducers(void) {
    for (int deep = 0; deep <= 1; deep++) {
        const int n = deep ? 300 : 150;
        buf_t java = {0}, js = {0}, py = {0}, c = {0};
        char unit[64];
        put(&java, "package p;\n");
        for (int i = 0; i < n; i++) {
            snprintf(unit, sizeof(unit), "class C%d { ", i);
            put(&java, unit);
        }
        put(&java, "void m() { f(); }");
        repeat(&java, "}", n);
        put(&java, "\n");
        for (int i = 0; i < n; i++) {
            snprintf(unit, sizeof(unit), "function f%d() { ", i);
            put(&js, unit);
        }
        put(&js, "g();");
        repeat(&js, "}", n);
        put(&js, "\n");
        put(&py, "def h():\n    v = a");
        repeat(&py, ".b", 2 * n);
        put(&py, "\n    return f(v)\n");
        put(&c, "int v = ");
        repeat(&c, "f(", n);
        put(&c, "0");
        repeat(&c, ")", n);
        put(&c, ";\n");
        const struct {
            const char *language;
            const char *rel;
            const char *source;
        } cases[] = {
            {"java", "src/main/java/p/C.java", java.text},
            {"javascript", "web/f.js", js.text},
            {"python", "app/h.py", py.text},
            {"c", "src/calls.c", c.text},
        };
        for (size_t i = 0; i < sizeof(cases) / sizeof(cases[0]); i++) {
            extraction_t x = extract(cases[i].language, cases[i].rel, cases[i].source);
            char what[96];
            snprintf(what, sizeof(what), "%s nested %d", cases[i].language, n);
            if (deep) {
                check_refused(what, x);
            } else {
                CHECK(!x.e->syntax_rejected && x.e->semantic_passes > 0 && x.e->calls.count > 0,
                      "%s was not extracted (depth %u)", what, x.e->syntax.depth);
            }
            pdxe_result_free(ctx, x.r);
        }
        free(java.text);
        free(js.text);
        free(py.text);
        free(c.text);
    }
}

/* The expanded tree past the ceiling: 33 uses of a macro of 16 open parentheses, and as
 * many of 16 closing ones, are 528 parentheses deep once expanded; the source itself is
 * shallow, and the expansion small enough for every preprocessing ceiling. */
static void check_expanded(void) {
    buf_t b = {0};
    put(&b, "#define O ((((((((((((((((\n#define C ))))))))))))))))\n"
            "int before(void);\nint g(void) { before(); return ");
    repeat(&b, "O ", 33);
    put(&b, "0 ");
    repeat(&b, "C ", 33);
    put(&b, "; }\n");
    extraction_t x = extract("c", "src/expanded.c", b.text);
    int expanded = 0;
    for (int i = 0; i < x.e->calls.count; i++) {
        expanded += x.e->calls.items[i].source_origin != PDXE_SOURCE_ORIGIN_RAW;
    }
    CHECK(!x.e->syntax_rejected && x.e->pp_syntax_rejected && x.e->pp_syntax.rejected &&
              x.r->extraction_lost == 1 && calls(x.e, "before") == 1 && expanded == 0,
          "the expanded pass past the ceiling was not skipped alone (raw refused %d, expanded "
          "refused %d, lost %u, expanded calls %d)",
          (int)x.e->syntax_rejected, (int)x.e->pp_syntax_rejected, x.r->extraction_lost, expanded);
    pdxe_result_free(ctx, x.r);
    free(b.text);
}

/* Issue 88 and issue 89 apart: paths past their budget under the depth, and a tree over
 * the depth before any path. */
static void check_with_path_budget(void) {
    buf_t b = {0};
    put(&b, "void work(void) {\n");
    for (int s = 0; s < 60; s++) {
        repeat(&b, "f(", 99);
        put(&b, "f(0)");
        repeat(&b, ")", 99);
        put(&b, ";\n");
    }
    put(&b, "}\n");
    extraction_t x = extract("c", "src/paths.c", b.text);
    CHECK(!x.e->syntax_rejected && x.e->syntax.depth < PDXE_MAX_SYNTAX_DEPTH &&
              x.e->site_paths.exhausted && x.r->extraction_lost == 1,
          "paths past their budget under the depth: refused %d, depth %u, exhausted %d, lost %u",
          (int)x.e->syntax_rejected, x.e->syntax.depth, (int)x.e->site_paths.exhausted,
          x.r->extraction_lost);
    pdxe_result_free(ctx, x.r);
    free(b.text);

    /* 255 nested calls are 514 deep: refused before any path is asked for. */
    b = (buf_t){0};
    put(&b, "int v = ");
    repeat(&b, "f(", 255);
    put(&b, "0");
    repeat(&b, ")", 255);
    put(&b, ";\n");
    x = extract("c", "src/deep_calls.c", b.text);
    check_refused("255 nested calls", x);
    CHECK(!x.e->site_paths.exhausted, "the path budget was spent on a refused file");
    pdxe_result_free(ctx, x.r);
    free(b.text);
}

int main(void) {
    check_scan();
    if (pdxe_init(&ctx) != PDXE_OK) {
        printf("FAIL the engine did not start\n");
        return 1;
    }
    check_boundary();
    check_reproducers();
    check_expanded();
    check_with_path_budget();
    pdxe_shutdown(ctx);
    if (failures) {
        printf("%d failure(s)\n", failures);
        return 1;
    }
    printf("ok: semantic extraction is admitted by syntax depth\n");
    return 0;
}
