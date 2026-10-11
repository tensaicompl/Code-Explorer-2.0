/*
 * The C and C++ expression evaluator's per-file step budget, and what running it out
 * does (docs/plan/ISSUES.md, issue 74; engine/patches/0012-c-lsp-eval-budget.patch).
 *
 *     c_eval_budget
 *
 * The budget of a source of length L is 10000 steps per started 32 KiB: at least
 * 10000, at most 320000, and nothing but L decides it. The helper is checked at the
 * lengths that matter, the boundaries of a tranche and of the cap, the largest int and
 * every int at or below zero that matters, and then against an independent 64-bit
 * reference at every length up to past the cap; under the sanitizers (make check-asan)
 * the largest lengths also show that nothing overflows.
 *
 * Then two generated C++ sources, written here and nowhere else, are extracted:
 *
 *   - one larger than a tranche, whose evaluation takes about twice the old fixed
 *     10000 steps and about half its own budget, which must extract losing nothing;
 *   - one smaller than a tranche, whose evaluation takes about twice 10000 steps,
 *     which must still report lost work: a small file keeps the old protection, and a
 *     budget running out is never silent.
 */

#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "pdxe.h"
#include "lsp/c_lsp.h"

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

/* The budget computed another way: 64-bit, rounding up by addition. */
static int64_t reference_budget(int64_t len) {
    int64_t tranches = len <= 0 ? 1 : (len + 32767) / 32768;
    int64_t budget = tranches * 10000;
    return budget > 320000 ? 320000 : budget;
}

static void check_budget(int len, int expected) {
    int got = c_eval_step_budget_for_source_len(len);
    CHECK(got == expected, "a source of %d bytes has a budget of %d, not %d", len, got, expected);
}

static void check_helper(void) {
    check_budget(INT_MIN, 10000);
    check_budget(-1, 10000);
    check_budget(0, 10000);
    check_budget(1, 10000);
    check_budget(32 * 1024, 10000);
    check_budget(32 * 1024 + 1, 20000);
    check_budget(64 * 1024, 20000);
    check_budget(64 * 1024 + 1, 30000);
    check_budget(533 * 1024, 170000);
    check_budget(31 * 32 * 1024, 310000);
    check_budget(31 * 32 * 1024 + 1, 320000);
    check_budget(1024 * 1024, 320000);
    check_budget(1024 * 1024 + 1, 320000);
    check_budget(2 * 1024 * 1024, 320000);
    check_budget(INT_MAX - 1, 320000);
    check_budget(INT_MAX, 320000);

    int mismatches = 0;
    int previous = 0;
    for (int len = -1; len <= 1024 * 1024 + 3 * 32768; len++) {
        int got = c_eval_step_budget_for_source_len(len);
        if (got != reference_budget(len) || got < previous) {
            if (mismatches++ < 5) {
                printf("FAIL a source of %d bytes has a budget of %d, the reference %lld\n", len,
                       got, (long long)reference_budget(len));
            }
        }
        previous = got;
    }
    CHECK(mismatches == 0, "%d lengths disagree with the reference", mismatches);
}

/*
 * Project-authored C++: a cursor type and, for each unit, a record type and a function
 * that reads a value through `chain` member calls on the cursor. The evaluator's work
 * is a fixed number of steps per unit (about 37 at a chain of 3, about 175 at 5), and
 * a unit is about 200 bytes.
 */
static char *generate(int units, int chain, size_t *len) {
    size_t cap = 256 + (size_t)units * (256 + 8 * (size_t)chain);
    char *out = malloc(cap);
    if (!out) {
        return NULL;
    }
    size_t n = (size_t)snprintf(out, cap,
                                "namespace sample {\n\nstruct Cursor {\n    Cursor &next();\n"
                                "    int value() const;\n};\n\n");
    for (int u = 0; u < units; u++) {
        n += (size_t)snprintf(out + n, cap - n,
                              "// Record %d holds one row of the sample table.\n"
                              "struct Record%d {\n    int id;\n    int weight;\n"
                              "    const char *label;\n};\n\n"
                              "int score_%d(Cursor &cursor) {\n    return cursor",
                              u, u, u);
        for (int c = 0; c < chain; c++) {
            n += (size_t)snprintf(out + n, cap - n, ".next()");
        }
        n += (size_t)snprintf(out + n, cap - n, ".value();\n}\n\n");
    }
    n += (size_t)snprintf(out + n, cap - n, "}  // namespace sample\n");
    *len = n;
    return out;
}

static uint32_t lost_extracting(pdxe_ctx *ctx, int units, int chain, size_t *len) {
    char *source = generate(units, chain, len);
    if (!source) {
        printf("FAIL cannot allocate the source\n");
        failures++;
        return 0;
    }
    pdxe_file_result *r = NULL;
    uint32_t lost = 0;
    if (pdxe_extract_file(ctx, pdxe_language_id("cpp"), "src/sample.cpp",
                          (const uint8_t *)source, *len, &r) != PDXE_OK ||
        !r) {
        printf("FAIL cannot extract the generated source\n");
        failures++;
    } else {
        CHECK(r->truncated == 0, "the generated source was truncated");
        lost = r->extraction_lost;
        pdxe_result_free(ctx, r);
    }
    free(source);
    return lost;
}

static void check_extraction(void) {
    pdxe_ctx *ctx = NULL;
    if (pdxe_init(&ctx) != PDXE_OK) {
        printf("FAIL cannot initialise the engine\n");
        failures++;
        return;
    }
    size_t len = 0;
    uint32_t lost = lost_extracting(ctx, 540, 3, &len);
    CHECK(len > 3 * 32768 && len <= 4 * 32768,
          "the large source is %zu bytes, not four tranches", len);
    CHECK(lost == 0, "the large source lost work %u times within its budget", lost);

    lost = lost_extracting(ctx, 120, 5, &len);
    CHECK(len <= 32768, "the small source is %zu bytes, more than one tranche", len);
    CHECK(lost > 0, "the small source ran out of its budget without losing work");
    pdxe_shutdown(ctx);
}

int main(void) {
    check_helper();
    check_extraction();
    if (failures) {
        printf("%d failure(s)\n", failures);
        return 1;
    }
    printf("ok\n");
    return 0;
}
