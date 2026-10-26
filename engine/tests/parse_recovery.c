/*
 * The parser's error recovery is bounded (docs/plan/ISSUES.md, issue 93; decision 74;
 * engine/patches/0038-bounded-parse-recovery.patch).
 *
 *     parse_recovery
 *
 * Recovering into an earlier parse state wraps every subtree popped since in a new error
 * node, so when one error node holds the rest of a file (JavaScript after an unclosed
 * parenthesis) it is rebuilt for each token taken and the parse is quadratic: 89 s at
 * 2 MiB. The runtime counts that work (the subtrees retained and summarised again) and
 * stops the parse once it passes a bound linear in the bytes read; the stopped parse is
 * not left to be resumed. The engine gives such a file no fact, counts lost work once and
 * skips its cross-file resolution, as for a tree past the depth ceiling.
 *
 *   - parse_recovery_is_linear: the wide error nodes of 1 and 2 MiB stop with their work
 *     at most 2.5 times apart; the file's length does not grow it quadratically.
 *   - parse_recovery_bound_is_deterministic: the same bytes stop at the same work, on a
 *     fresh parser and a reused one, and extract the same refused result twice.
 *   - Below the bound: a file the census holds with the most recovery work for its size
 *     (a bare list of numbers) and a linear error shape parse as before.
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

/* `let x = (` before functions, to `bytes`: one error node holds the rest of the file. */
static char *open_paren(size_t bytes) {
    char *text = malloc(bytes + 64);
    if (!text) {
        printf("FAIL out of memory\n");
        exit(1);
    }
    size_t len = (size_t)sprintf(text, "let x = (\n");
    for (long n = 0; len < bytes; n++) {
        len += (size_t)sprintf(text + len, "function f%ld(a) { return a + %ld; }\n", n, n);
    }
    return text;
}

typedef struct {
    TSTree *tree;
    bool exhausted;
    uint64_t work;
} parsed_t;

static parsed_t parse_with(TSParser *parser, PDXELanguage language, const char *source) {
    ts_parser_set_language(parser, pdxe_ts_language(language));
    parsed_t p = {0};
    p.tree = ts_parser_parse_string(parser, NULL, source, (uint32_t)strlen(source));
    p.exhausted = ts_parser_recovery_exhausted(parser);
    p.work = ts_parser_recovery_work(parser);
    return p;
}

static parsed_t parse(PDXELanguage language, const char *source) {
    TSParser *parser = ts_parser_new();
    parsed_t p = parse_with(parser, language, source);
    ts_parser_delete(parser);
    return p;
}

static void parse_recovery_is_linear(void) {
    char *one = open_paren((size_t)1 << 20);
    char *two = open_paren(((size_t)2 << 20) - 64);
    parsed_t a = parse(PDXE_LANG_JAVASCRIPT, one);
    parsed_t b = parse(PDXE_LANG_JAVASCRIPT, two);
    CHECK(!a.tree && a.exhausted && !b.tree && b.exhausted,
          "a wide error node was not stopped by the recovery bound (%d, %d)", (int)a.exhausted,
          (int)b.exhausted);
    CHECK(a.work > 0 && b.work <= a.work * 5 / 2,
          "parse_recovery_is_linear: 1 MiB did %llu, 2 MiB %llu", (unsigned long long)a.work,
          (unsigned long long)b.work);
    free(one);
    free(two);
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

/* A refused file: no fact, work lost once, no cross-file resolution, no parse failure. */
static void check_refused(const char *what, extraction_t x) {
    const PDXEFileResult *e = x.e;
    CHECK(e->defs.count == 0 && e->calls.count == 0 && e->usages.count == 0 &&
              e->imports.count == 0 && e->resolved_calls.count == 0 && e->semantic_passes == 0,
          "%s: facts were made (%d defs, %d calls, %u passes)", what, e->defs.count,
          e->calls.count, e->semantic_passes);
    CHECK(x.r->extraction_lost == 1 && e->lsp_skipped && !e->has_error && !x.r->truncated,
          "%s: lost %u, lsp_skipped %d, has_error %d", what, x.r->extraction_lost,
          (int)e->lsp_skipped, (int)e->has_error);
}

static void parse_recovery_bound_is_deterministic(void) {
    char *source = open_paren((size_t)1 << 20);
    parsed_t fresh = parse(PDXE_LANG_JAVASCRIPT, source);

    /* A reused parser: after a valid file, the wide one; then a valid file again, which is
     * parsed whole, never taken for the stopped parse's continuation. */
    TSParser *parser = ts_parser_new();
    parsed_t before = parse_with(parser, PDXE_LANG_JAVASCRIPT, "let y = 1;\n");
    parsed_t again = parse_with(parser, PDXE_LANG_JAVASCRIPT, source);
    parsed_t after = parse_with(parser, PDXE_LANG_JAVASCRIPT, "let y = 1;\n");
    CHECK(before.tree && !before.exhausted && before.work == 0, "a valid file did recovery work");
    CHECK(!fresh.tree && !again.tree && fresh.exhausted && again.exhausted &&
              fresh.work == again.work,
          "parse_recovery_bound_is_deterministic: %llu and %llu",
          (unsigned long long)fresh.work, (unsigned long long)again.work);
    CHECK(after.tree && !after.exhausted &&
              ts_node_descendant_count(ts_tree_root_node(after.tree)) ==
                  ts_node_descendant_count(ts_tree_root_node(before.tree)),
          "the parse after a stopped one is not the valid file's own");
    ts_tree_delete(before.tree);
    ts_tree_delete(after.tree);
    ts_parser_delete(parser);

    /* Extracted twice, the same refusal. */
    extraction_t first = extract("javascript", "src/open.js", source);
    extraction_t second = extract("javascript", "src/open.js", source);
    check_refused("the wide error node", first);
    check_refused("the wide error node, again", second);
    pdxe_result_free(ctx, first.r);
    pdxe_result_free(ctx, second.r);
    free(source);
}

/* What the census holds with the most recovery work, and a linear shape, parse as before. */
static void below_the_bound(void) {
    char numbers[8192];
    size_t len = 0;
    for (int i = 0; i < 1100 && len + 16 < sizeof numbers; i++) {
        len += (size_t)snprintf(numbers + len, sizeof numbers - len, "%d, ", i * 7);
    }
    parsed_t table = parse(PDXE_LANG_C, numbers);
    CHECK(table.tree && !table.exhausted && table.work > 0,
          "a bare table of numbers was refused (work %llu)", (unsigned long long)table.work);
    ts_tree_delete(table.tree);

    extraction_t c = extract("c", "src/table.h", numbers);
    CHECK(c.r->extraction_lost == 0 && !c.e->lsp_skipped, "the table lost work");
    pdxe_result_free(ctx, c.r);

    char *classes = malloc((size_t)1 << 20);
    size_t n = 0;
    for (int i = 0; n < ((size_t)1 << 20) - 64; i++) {
        n += (size_t)sprintf(classes + n, "class C%d { class I { fun m() { I() } } }\n", i);
    }
    parsed_t kotlin = parse(PDXE_LANG_KOTLIN, classes);
    CHECK(kotlin.tree && !kotlin.exhausted, "a linear error shape was refused");
    ts_tree_delete(kotlin.tree);
    free(classes);
}

int main(void) {
    if (pdxe_init(&ctx) != PDXE_OK) {
        printf("FAIL the engine did not start\n");
        return 1;
    }
    parse_recovery_is_linear();
    parse_recovery_bound_is_deterministic();
    below_the_bound();
    pdxe_shutdown(ctx);
    if (failures) {
        printf("%d failure(s)\n", failures);
        return 1;
    }
    printf("parse_recovery: ok\n");
    return 0;
}
