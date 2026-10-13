/*
 * C and C++ preprocessing is bounded by its source and nothing else (docs/plan/ISSUES.md,
 * issue 87; decision 56; engine/patches/0019-bounded-deterministic-preprocessing.patch).
 *
 *     c_family_preprocess_budget
 *
 * Preprocessing wrote one newline for every line a forward #line skipped, so #line
 * 400000000 made 400 MB of expanded text, and macros that double expanded
 * exponentially: whether a file completed depended on the memory of the machine. Now
 * every ceiling is a count from the source's length and fixed constants, never a time,
 * the environment or the machine:
 *
 *   - the byte budget is 24 bytes a source byte, at least 256 KiB and at most 8 MiB,
 *     computed without overflow: checked at its boundaries and against an independent
 *     64-bit reference at every length up to past the cap;
 *   - a forward jump of up to PDXE_PREPROCESS_MAX_LINE_PADDING lines is padded, one of
 *     more is one #line marker: #line 400000000, 2147483647 and 4294967295 make a few
 *     bytes, map their lines exactly, and extract row for row as a near #line does (byte
 *     offsets aside, which count the directive's and the marker's bytes), so a marker is
 *     never a fact; jumps of 4095, 4096 and 4097 lines;
 *   - the expanded text written, the text and tokens expansion generates, how many
 *     macros it expands (an empty one included) and how deeply it nests are each
 *     checked at their ceiling minus one, at it and plus one, the counts written here
 *     and found again in the counters;
 *   - a doubling macro below its ceilings expands; above them it stops inside the
 *     expansion, at the counters' first step past a ceiling, the expansion is discarded,
 *     the file is extracted from its source alone, and the extraction loses work exactly
 *     once, the same on every run;
 *   - a normal source of nearly 2 MiB still preprocesses within its budget, and one of a
 *     quarter of a megabyte extracts losing nothing.
 *
 * No environment variable or test seam is involved: the ceilings are the engine's own.
 */

#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "pdxe.h"
#include "pdxe_core.h"
#include "preprocessor.h"
#include "shim_internal.h"

#include "differential_rows.h"

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

/* Preprocesses `source` as C, and returns the expanded text ("" when there is none),
 * leaving its counters in *stats. */
static PDXEPreprocessedSource *preprocess(const char *source, const char *rel,
                                          PDXEPreprocessStats *stats) {
    PDXEPreprocessedSource *pp =
        pdxe_preprocess_with_map(source, (int)strlen(source), rel, NULL, NULL, 0);
    *stats = *pdxe_preprocess_last_stats();
    CHECK(!!pp == !stats->exhausted, "%s: the result and the counters disagree", rel);
    return pp;
}

/* ------------------------------------------------------------------------------------ */
/* The budget formula. */

static int64_t reference_budget(int64_t len) {
    int64_t budget = len * 24;
    if (budget < 256 * 1024) {
        budget = 256 * 1024;
    }
    return budget > 8 * 1024 * 1024 ? 8 * 1024 * 1024 : budget;
}

static void check_formula(void) {
    CHECK(PDXE_PREPROCESS_EXPANSION_FACTOR == 24 && PDXE_PREPROCESS_MIN_EXPANDED_BYTES == 262144u &&
              PDXE_PREPROCESS_MAX_EXPANDED_BYTES == 8388608u &&
              PDXE_PREPROCESS_MAX_GENERATED_TOKENS == 500000u &&
              PDXE_PREPROCESS_MAX_EXPANSIONS == 500000u &&
              PDXE_PREPROCESS_MAX_EXPANSION_DEPTH == 256u && PDXE_PREPROCESS_MAX_LINE_PADDING == 4096u,
          "a ceiling is not the value decision 56 records");
    static const struct {
        int len;
        size_t budget;
    } EDGES[] = {
        /* The floor: 10,922 × 24 = 262,128 is under it, 10,923 × 24 = 262,152 over. */
        {INT_MIN, 262144},  {-1, 262144},       {0, 262144},        {1, 262144},
        {10922, 262144},    {10923, 262152},    {10924, 262176},    {18591, 446184},
        {100000, 2400000},
        /* The cap: 349,525 × 24 = 8,388,600 is under it; 349,526 × 24 would pass it. */
        {349524, 8388576},  {349525, 8388600},  {349526, 8388608},  {2097152, 8388608},
        {INT_MAX, 8388608},
    };
    for (size_t i = 0; i < sizeof(EDGES) / sizeof(EDGES[0]); i++) {
        size_t got = pdxe_preprocess_budget_for_source_len(EDGES[i].len);
        CHECK(got == EDGES[i].budget, "a source of %d bytes has a budget of %zu, not %zu",
              EDGES[i].len, got, EDGES[i].budget);
    }
    for (int len = -2; len <= 600000; len++) {
        size_t got = pdxe_preprocess_budget_for_source_len(len);
        if ((int64_t)got != reference_budget(len)) {
            CHECK(0, "a source of %d bytes has a budget of %zu, not %lld", len, got,
                  (long long)reference_budget(len));
            break;
        }
    }
}

/* ------------------------------------------------------------------------------------ */
/* #line: a far jump is one marker, mapped exactly, and never a fact. */

static pdxe_ctx *ctx = NULL;

/* Every row and the surface of one extraction, as one string; *lost is its lost work. */
static char *extract(int lang, const char *rel, const char *source, uint32_t *lost) {
    pdxe_file_result *r = NULL;
    if (pdxe_extract_file(ctx, lang, rel, (const uint8_t *)source, strlen(source), &r) != PDXE_OK ||
        !r) {
        printf("FAIL %s: the extraction failed\n", rel);
        exit(1);
    }
    const PDXEFileResult *engine = pdxe_result_engine(r);
    uint8_t *surface = NULL;
    size_t surface_len = 0;
    if (!engine || pdxe_surface_export(r, rel, &surface, &surface_len) != PDXE_OK) {
        printf("FAIL %s: no result or no surface\n", rel);
        exit(1);
    }
    if (lost) {
        *lost = r->extraction_lost;
    }
    FILE *o = tmpfile();
    if (!o) {
        printf("FAIL no temporary file\n");
        exit(1);
    }
    diff_rows_file(o, rel, engine, source);
    fputs("{\"c\":\"surface\",\"json\":", o);
    fwrite(surface, 1, surface_len, o);
    fputs("}\n", o);
    long n = ftell(o);
    char *text = malloc((size_t)n + 1);
    if (!text || fseek(o, 0, SEEK_SET) != 0 || fread(text, 1, (size_t)n, o) != (size_t)n) {
        printf("FAIL cannot read the rows back\n");
        exit(1);
    }
    text[n] = '\0';
    fclose(o);
    pdxe_surface_free(surface);
    pdxe_result_free(ctx, r);
    return text;
}

/* `rows` with every byte offset blanked: a directive's length moves the raw offsets, and
 * offsets in the expanded text count the marker's bytes. Lines and every other field
 * remain. The surface's text ranges and the source's length are offsets too (issue 90). */
static void blank_offsets(char *rows) {
    static const char *const KEYS[] = {"\"start\":", "\"end\":", "\"site_start_byte\":",
                                       "\"site_end_byte\":", "\"source_len\":",
                                       "\"expr_start\":", "\"expr_end\":",
                                       "\"signature_start\":", "\"signature_end\":"};
    for (size_t k = 0; k < sizeof(KEYS) / sizeof(KEYS[0]); k++) {
        for (char *at = strstr(rows, KEYS[k]); at; at = strstr(at, KEYS[k])) {
            at += strlen(KEYS[k]);
            while (*at >= '0' && *at <= '9') {
                *at++ = '#';
            }
        }
    }
    static const char *const RANGES[] = {"\"callee_name\":[", "\"ref_name\":[",
                                         "\"callee_qn\":[", "\"source_text\":["};
    for (size_t k = 0; k < sizeof(RANGES) / sizeof(RANGES[0]); k++) {
        for (char *at = strstr(rows, RANGES[k]); at; at = strstr(at, RANGES[k])) {
            at += strlen(RANGES[k]);
            while ((*at >= '0' && *at <= '9') || *at == ',') {
                if (*at != ',') {
                    *at = '#';
                }
                at++;
            }
        }
    }
}

static const char FAR_SOURCE[] = "#define KEEP 1\n"
                                 "int before(void);\n"
                                 "#line %s\n"
                                 "void after(void) { callee(); }\n";

static void check_far_lines(void) {
    static const char *const FAR[] = {"400000000", "2147483647", "4294967295"};
    const int languages[] = {pdxe_language_id("c"), pdxe_language_id("cpp")};
    const char *const rels[] = {"src/far.c", "src/far.cpp"};
    for (size_t i = 0; i < sizeof(FAR) / sizeof(FAR[0]); i++) {
        char source[256];
        snprintf(source, sizeof(source), FAR_SOURCE, FAR[i]);
        const uint32_t line = (uint32_t)strtoul(FAR[i], NULL, 10);

        PDXEPreprocessStats stats;
        PDXEPreprocessedSource *pp = preprocess(source, "src/far.c", &stats);
        char expected[256];
        snprintf(expected, sizeof(expected),
                 "\nint before ( void ) ;\n#line %s \"src/far.c\"\nvoid after ( void ) { callee ( ) ; }",
                 FAR[i]);
        CHECK(pp && strcmp(pp->source, expected) == 0, "#line %s: the expanded text is %s",
              FAR[i], pp ? pp->source : "(none)");
        CHECK(stats.serialized_bytes == strlen(expected) && stats.line_markers == 1 &&
                  stats.longest_line_jump == line - 2,
              "#line %s: %zu bytes, %zu markers, a jump of %u", FAR[i], stats.serialized_bytes,
              stats.line_markers, stats.longest_line_jump);
        if (pp) {
            CHECK(pp->expanded_line_count == 4 && pp->original_line_by_expanded_line[2] == 2 &&
                      pp->original_line_by_expanded_line[3] == 0 &&
                      pp->original_line_by_expanded_line[4] == line &&
                      pp->belongs_to_main_file[4] == 1,
                  "#line %s: the lines are not mapped back exactly", FAR[i]);
            pdxe_preprocessed_source_free(pp);
        }

        /* The same declarations on the same physical lines, with a #line that skips
         * nothing: every row and the surface must be the same. */
        for (size_t l = 0; l < 2; l++) {
            uint32_t lost = 1;
            char *far = extract(languages[l], rels[l], source, &lost);
            CHECK(lost == 0, "#line %s, %s: lost work", FAR[i], rels[l]);
            char near_source[256];
            snprintf(near_source, sizeof(near_source), FAR_SOURCE, "4");
            char *near = extract(languages[l], rels[l], near_source, NULL);
            blank_offsets(far);
            blank_offsets(near);
            CHECK(strcmp(far, near) == 0, "#line %s, %s: a marker changed the rows:\n%s\n%s",
                  FAR[i], rels[l], far, near);
            CHECK(strstr(far, "\"callee\":\"callee\",") != NULL, "#line %s, %s: callee is lost",
                  FAR[i], rels[l]);
            free(far);
            free(near);
        }
    }
}

/* Jumps of PDXE_PREPROCESS_MAX_LINE_PADDING lines, one fewer and one more. */
static void check_padding_threshold(void) {
    for (unsigned jump = PDXE_PREPROCESS_MAX_LINE_PADDING - 1;
         jump <= PDXE_PREPROCESS_MAX_LINE_PADDING + 1; jump++) {
        const unsigned line = 2 + jump;
        char source[128];
        snprintf(source, sizeof(source), "#define KEEP 1\nint a;\n#line %u\nint b;\n", line);
        PDXEPreprocessStats stats;
        PDXEPreprocessedSource *pp = preprocess(source, "src/pad.c", &stats);
        const int marked = jump > PDXE_PREPROCESS_MAX_LINE_PADDING;
        CHECK(stats.longest_line_jump == jump && stats.line_markers == (size_t)marked,
              "a jump of %u: %zu markers, a jump of %u", jump, stats.line_markers,
              stats.longest_line_jump);
        if (!pp) {
            continue;
        }
        CHECK((strstr(pp->source, "#line") != NULL) == marked, "a jump of %u is %s", jump,
              marked ? "padded" : "marked");
        CHECK(stats.serialized_bytes == strlen(pp->source),
              "a jump of %u: %zu bytes counted, %zu written", jump, stats.serialized_bytes,
              strlen(pp->source));
        const int last = pp->expanded_line_count;
        CHECK(last == (marked ? 4 : (int)line) && strstr(pp->source, "int b") &&
                  pp->original_line_by_expanded_line[last] == line &&
                  pp->belongs_to_main_file[last] == 1,
              "a jump of %u: int b is not on line %u", jump, line);
        pdxe_preprocessed_source_free(pp);
    }
}

/* ------------------------------------------------------------------------------------ */
/* Every ceiling at its edge, with the engine's own constants. */

/* "#define KEEP 1\nint a;\n", then `full` jumps of the padding, then one of `last` lines:
 * the written text grows by one byte a line of the last jump. */
static char *padded_source(long full, unsigned last) {
    buf_t b = {0};
    put(&b, "#define KEEP 1\nint a;\n");
    unsigned long line = 2;
    char directive[64];
    for (long i = 0; i <= full; i++) {
        line += i < full ? PDXE_PREPROCESS_MAX_LINE_PADDING : last;
        snprintf(directive, sizeof(directive), "#line %lu\nint a;\n", line);
        put(&b, directive);
    }
    return b.text;
}

static void check_serialized_edge(void) {
    /* Find the jumps that put the written text just under the budget. */
    PDXEPreprocessStats stats;
    long full = 0;
    size_t base = 0;
    for (;; full++) {
        char *source = padded_source(full, 1);
        PDXEPreprocessedSource *pp = preprocess(source, "src/serial.c", &stats);
        free(source);
        if (!pp) {
            CHECK(0, "the written text passed the budget before its edge was found");
            return;
        }
        pdxe_preprocessed_source_free(pp);
        base = stats.serialized_bytes - 1;
        if (base + PDXE_PREPROCESS_MAX_LINE_PADDING - 1 >= stats.budget) {
            break;
        }
    }
    CHECK(stats.budget == PDXE_PREPROCESS_MIN_EXPANDED_BYTES, "the budget is %zu", stats.budget);
    const size_t edge = stats.budget - base;
    CHECK(edge >= 2 && edge + 1 <= PDXE_PREPROCESS_MAX_LINE_PADDING,
          "no last jump reaches the budget exactly (%zu)", edge);
    for (size_t last = edge - 1; last <= edge + 1; last++) {
        char *source = padded_source(full, (unsigned)last);
        PDXEPreprocessedSource *pp = preprocess(source, "src/serial.c", &stats);
        free(source);
        const size_t written = base + last;
        if (written <= stats.budget) {
            CHECK(pp && stats.serialized_bytes == written && strlen(pp->source) == written,
                  "%zu bytes written within a budget of %zu did not complete", written,
                  stats.budget);
        } else {
            CHECK(!pp && stats.exhausted && strcmp(stats.ceiling, "serialized bytes") == 0 &&
                      stats.serialized_bytes > stats.budget,
                  "%zu bytes written past a budget of %zu completed", written, stats.budget);
        }
        if (pp) {
            pdxe_preprocessed_source_free(pp);
        }
    }
}

/* A condition's expansion is generated and never written: 64 bytes for each Y, and
 * `tail` for a Z of that length. */
static char *condition_source(long ys, int tail) {
    buf_t b = {0};
    char z[128];
    put(&b, "#define Y yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy\n");
    snprintf(z, sizeof(z), "#define Z %.*s\n", tail ? tail : 1,
             "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz");
    put(&b, z);
    put(&b, "#define CHECK");
    repeat(&b, " Y", ys);
    if (tail) {
        put(&b, " Z");
    }
    put(&b, "\n#if CHECK\n#endif\nint a;\n");
    return b.text;
}

static void check_generated_bytes_edge(void) {
    const long ys = PDXE_PREPROCESS_MIN_EXPANDED_BYTES / 64;
    const struct {
        long ys;
        int tail;
        size_t generated;
    } CASES[] = {
        {ys - 1, 63, PDXE_PREPROCESS_MIN_EXPANDED_BYTES - 1},
        {ys, 0, PDXE_PREPROCESS_MIN_EXPANDED_BYTES},
        {ys, 1, PDXE_PREPROCESS_MIN_EXPANDED_BYTES + 1},
    };
    for (size_t i = 0; i < sizeof(CASES) / sizeof(CASES[0]); i++) {
        char *source = condition_source(CASES[i].ys, CASES[i].tail);
        PDXEPreprocessStats stats;
        PDXEPreprocessedSource *pp = preprocess(source, "src/bytes.c", &stats);
        free(source);
        CHECK(stats.budget == PDXE_PREPROCESS_MIN_EXPANDED_BYTES &&
                  stats.generated_bytes == CASES[i].generated,
              "%zu bytes were counted, not %zu (budget %zu)", stats.generated_bytes,
              CASES[i].generated, stats.budget);
        if (CASES[i].generated <= stats.budget) {
            CHECK(pp != NULL, "%zu generated bytes within the budget did not complete",
                  CASES[i].generated);
        } else {
            CHECK(!pp && stats.exhausted && strcmp(stats.ceiling, "generated bytes") == 0 &&
                      stats.serialized_bytes == 0,
                  "%zu generated bytes past the budget did not stop the expansion",
                  CASES[i].generated);
        }
        if (pp) {
            pdxe_preprocessed_source_free(pp);
        }
    }
}

/* Each T generates ten tokens in one expansion, each X one. */
static char *token_source(long ts, long xs) {
    buf_t b = {0};
    put(&b, "#define X x\n#define T x x x x x x x x x x\nint v[] = {\n");
    repeat(&b, "T ", ts);
    repeat(&b, "X ", xs);
    put(&b, "\n};\n");
    return b.text;
}

static void check_generated_tokens_edge(void) {
    const long ts = PDXE_PREPROCESS_MAX_GENERATED_TOKENS / 10;
    const struct {
        long ts;
        long xs;
        size_t generated;
    } CASES[] = {
        {ts - 1, 9, PDXE_PREPROCESS_MAX_GENERATED_TOKENS - 1},
        {ts, 0, PDXE_PREPROCESS_MAX_GENERATED_TOKENS},
        {ts, 1, PDXE_PREPROCESS_MAX_GENERATED_TOKENS + 1},
    };
    for (size_t i = 0; i < sizeof(CASES) / sizeof(CASES[0]); i++) {
        char *source = token_source(CASES[i].ts, CASES[i].xs);
        PDXEPreprocessStats stats;
        PDXEPreprocessedSource *pp = preprocess(source, "src/tokens.c", &stats);
        free(source);
        CHECK(stats.generated_tokens == CASES[i].generated,
              "%zu tokens were counted, not %zu", stats.generated_tokens, CASES[i].generated);
        if (CASES[i].generated <= PDXE_PREPROCESS_MAX_GENERATED_TOKENS) {
            /* Only the token ceiling is near: the bytes and expansions are well within. */
            CHECK(pp && stats.generated_bytes * 2 < stats.budget &&
                      stats.serialized_bytes < stats.budget &&
                      stats.expansions * 5 < PDXE_PREPROCESS_MAX_EXPANSIONS,
                  "%zu generated tokens at most the ceiling did not complete",
                  CASES[i].generated);
        } else {
            CHECK(!pp && stats.exhausted && strcmp(stats.ceiling, "generated tokens") == 0 &&
                      stats.serialized_bytes == 0,
                  "%zu generated tokens did not stop the expansion", CASES[i].generated);
        }
        if (pp) {
            pdxe_preprocessed_source_free(pp);
        }
    }
}

/* Each T expands eleven macros, each E0 one, and none creates a token. */
static char *expansion_source(long ts, long es) {
    buf_t b = {0};
    put(&b, "#define E0\n#define T E0 E0 E0 E0 E0 E0 E0 E0 E0 E0\nint v = 1\n");
    repeat(&b, "T ", ts);
    repeat(&b, "E0 ", es);
    put(&b, ";\n");
    return b.text;
}

static void check_expansions_edge(void) {
    const long ts = PDXE_PREPROCESS_MAX_EXPANSIONS / 11;
    const long rest = (long)(PDXE_PREPROCESS_MAX_EXPANSIONS - (size_t)ts * 11);
    const struct {
        long es;
        size_t expansions;
    } CASES[] = {
        {rest - 1, PDXE_PREPROCESS_MAX_EXPANSIONS - 1},
        {rest, PDXE_PREPROCESS_MAX_EXPANSIONS},
        {rest + 1, PDXE_PREPROCESS_MAX_EXPANSIONS + 1},
    };
    for (size_t i = 0; i < sizeof(CASES) / sizeof(CASES[0]); i++) {
        char *source = expansion_source(ts, CASES[i].es);
        PDXEPreprocessStats stats;
        PDXEPreprocessedSource *pp = preprocess(source, "src/empty.c", &stats);
        free(source);
        CHECK(stats.expansions == CASES[i].expansions && stats.generated_tokens == 0,
              "%zu expansions were counted, not %zu (%zu tokens)", stats.expansions,
              CASES[i].expansions, stats.generated_tokens);
        if (CASES[i].expansions <= PDXE_PREPROCESS_MAX_EXPANSIONS) {
            CHECK(pp && strstr(pp->source, "int v = 1"),
                  "%zu expansions at most the ceiling did not complete", CASES[i].expansions);
        } else {
            CHECK(!pp && stats.exhausted && strcmp(stats.ceiling, "expansions") == 0,
                  "%zu expansions did not stop the expansion", CASES[i].expansions);
        }
        if (pp) {
            pdxe_preprocessed_source_free(pp);
        }
    }
}

/* M<n> expands through n + 1 macros, nesting n + 1 deep. */
static char *chain_source(unsigned depth) {
    buf_t b = {0};
    char line[64];
    put(&b, "#define M0 end\n");
    for (unsigned i = 1; i < depth; i++) {
        snprintf(line, sizeof(line), "#define M%u M%u\n", i, i - 1);
        put(&b, line);
    }
    snprintf(line, sizeof(line), "int v = M%u;\n", depth - 1);
    put(&b, line);
    return b.text;
}

static void check_depth_edge(void) {
    for (unsigned depth = PDXE_PREPROCESS_MAX_EXPANSION_DEPTH - 1;
         depth <= PDXE_PREPROCESS_MAX_EXPANSION_DEPTH + 1; depth++) {
        char *source = chain_source(depth);
        PDXEPreprocessStats stats;
        PDXEPreprocessedSource *pp = preprocess(source, "src/chain.c", &stats);
        free(source);
        if (depth <= PDXE_PREPROCESS_MAX_EXPANSION_DEPTH) {
            CHECK(pp && stats.deepest_expansion == depth && strstr(pp->source, "int v = end ;"),
                  "a chain %u deep did not expand (%u)", depth, stats.deepest_expansion);
        } else {
            CHECK(!pp && stats.exhausted && strcmp(stats.ceiling, "expansion depth") == 0 &&
                      stats.deepest_expansion == depth,
                  "a chain %u deep did not stop at the ceiling (%u)", depth,
                  stats.deepest_expansion);
        }
        if (pp) {
            pdxe_preprocessed_source_free(pp);
        }
    }
}

/* ------------------------------------------------------------------------------------ */
/* A doubling macro, extracted whole. */

/* D<k> is leaf() called 2^k times. */
static char *bomb_source(int k) {
    buf_t b = {0};
    char line[64];
    put(&b, "void before(void);\n#define D0 leaf();\n");
    for (int i = 1; i <= k; i++) {
        snprintf(line, sizeof(line), "#define D%d D%d D%d\n", i, i - 1, i - 1);
        put(&b, line);
    }
    snprintf(line, sizeof(line), "void run(void) { before(); D%d }\n", k);
    put(&b, line);
    return b.text;
}

/* Whether any row came from an expanded text. */
static int has_preprocessed_rows(const char *source, uint32_t *lost) {
    pdxe_file_result *r = NULL;
    if (pdxe_extract_file(ctx, pdxe_language_id("c"), "src/bomb.c", (const uint8_t *)source,
                          strlen(source), &r) != PDXE_OK ||
        !r) {
        printf("FAIL the bomb's extraction failed\n");
        exit(1);
    }
    const PDXEFileResult *e = pdxe_result_engine(r);
    int found = 0;
    for (int i = 0; i < e->calls.count; i++) {
        found |= e->calls.items[i].source_origin == PDXE_SOURCE_ORIGIN_PREPROCESSED;
    }
    for (int i = 0; i < e->usages.count; i++) {
        found |= e->usages.items[i].source_origin == PDXE_SOURCE_ORIGIN_PREPROCESSED;
    }
    for (int i = 0; i < e->resolved_calls.count; i++) {
        found |= e->resolved_calls.items[i].source_origin == PDXE_SOURCE_ORIGIN_PREPROCESSED;
    }
    *lost = r->extraction_lost;
    pdxe_result_free(ctx, r);
    return found;
}

static void check_bomb(void) {
    PDXEPreprocessStats stats;
    PDXEPreprocessedSource *pp = NULL;
    /* Below the ceilings: 4096 calls of leaf, from the expansion. */
    char *source = bomb_source(12);
    uint32_t lost = 1;
    char *rows = extract(pdxe_language_id("c"), "src/bomb.c", source, &lost);
    CHECK(lost == 0, "a bomb below its ceilings lost work");
    CHECK(strstr(rows, "\"callee\":\"leaf\",") && strstr(rows, "\"callee\":\"before\","),
          "a bomb below its ceilings did not expand");
    CHECK(has_preprocessed_rows(source, &lost), "a bomb below its ceilings has no expansion");
    free(rows);
    free(source);

    /* Doubling macros that create nothing: 2^40 expansions would take days. They stop at
     * the expansion past the ceiling, and lose work once. */
    buf_t empty = {0};
    put(&empty, "void before(void);\n#define D0\n");
    for (int i = 1; i <= 40; i++) {
        char line[64];
        snprintf(line, sizeof(line), "#define D%d D%d D%d\n", i, i - 1, i - 1);
        put(&empty, line);
    }
    put(&empty, "void run(void) { before(); D40 }\n");
    pp = preprocess(empty.text, "src/bomb.c", &stats);
    CHECK(!pp && stats.exhausted && strcmp(stats.ceiling, "expansions") == 0 &&
              stats.expansions == PDXE_PREPROCESS_MAX_EXPANSIONS + 1 && stats.generated_tokens == 0,
          "doubling empty macros did not stop at the expansion ceiling: %s, %zu",
          stats.ceiling ? stats.ceiling : "-", stats.expansions);
    lost = 0;
    rows = extract(pdxe_language_id("c"), "src/bomb.c", empty.text, &lost);
    CHECK(lost == 1 && strstr(rows, "\"callee\":\"before\","),
          "doubling empty macros lost work %u times", lost);
    free(rows);
    free(empty.text);

    /* Above them: 2^24 calls would be 67 million tokens. It stops when the generated
     * text first passes 256 KiB, and the file is extracted from its source alone, losing
     * work once, the same on every run. */
    source = bomb_source(24);
    pp = preprocess(source, "src/bomb.c", &stats);
    CHECK(!pp && stats.exhausted && strcmp(stats.ceiling, "generated bytes") == 0 &&
              stats.generated_bytes > stats.budget && stats.generated_bytes <= stats.budget + 8 &&
              stats.generated_tokens < PDXE_PREPROCESS_MAX_GENERATED_TOKENS &&
              stats.serialized_bytes == 0,
          "the bomb did not stop at its first step past the budget: %s, %zu bytes, %zu tokens",
          stats.ceiling ? stats.ceiling : "-", stats.generated_bytes, stats.generated_tokens);
    char *first = NULL;
    for (int run = 0; run < 2; run++) {
        lost = 0;
        rows = extract(pdxe_language_id("c"), "src/bomb.c", source, &lost);
        CHECK(lost == 1, "a bomb above its ceilings lost work %u times, not once", lost);
        CHECK(!strstr(rows, "\"callee\":\"leaf\",") && strstr(rows, "\"callee\":\"before\","),
              "a bomb above its ceilings was not extracted from its source alone");
        CHECK(!has_preprocessed_rows(source, &lost) && lost == 1,
              "a bomb above its ceilings kept rows of its expansion");
        if (first) {
            CHECK(strcmp(first, rows) == 0, "the bomb's extraction changed between runs");
            free(rows);
        } else {
            first = rows;
        }
    }
    free(first);
    free(source);
}

/* A normal source of `size` bytes, every function of which uses a macro: about 0.18
 * generated tokens a source byte, temporaries included. */
static char *normal_source(size_t size, size_t *len) {
    buf_t b = {0};
    put(&b, "#define MAX(a, b) ((a) > (b) ? (a) : (b))\n");
    char line[192];
    for (int i = 0; b.len < size; i++) {
        snprintf(line, sizeof(line),
                 "int step_%d(int value, int limit) {\n    int bounded = MAX(value, limit);\n"
                 "    record(bounded + %d);\n    return bounded;\n}\n\n",
                 i, i);
        put(&b, line);
    }
    *len = b.len;
    return b.text;
}

/* Nearly 2 MiB preprocesses within its budget, about 70% of the token ceiling; a quarter
 * of a megabyte is extracted whole, losing nothing (the whole extraction of 2 MiB takes
 * seconds more and proves nothing more about preprocessing). */
static void check_large_normal_source(void) {
    size_t len = 0;
    char *source = normal_source(1900 * 1024, &len);
    PDXEPreprocessStats stats;
    PDXEPreprocessedSource *pp = preprocess(source, "src/large.c", &stats);
    CHECK(pp && !stats.exhausted && stats.budget == PDXE_PREPROCESS_MAX_EXPANDED_BYTES &&
              stats.generated_tokens > 300000,
          "a normal source of %zu bytes did not preprocess (%s, %zu tokens)", len,
          stats.exhausted ? stats.ceiling : "no expansion", stats.generated_tokens);
    if (pp) {
        pdxe_preprocessed_source_free(pp);
    }
    free(source);

    source = normal_source(256 * 1024, &len);
    uint32_t lost = 1;
    CHECK(has_preprocessed_rows(source, &lost) && lost == 0,
          "a normal source of %zu bytes lost its expansion", len);
    free(source);
}

int main(void) {
    if (pdxe_init(&ctx) != PDXE_OK) {
        printf("FAIL the engine did not start\n");
        return 1;
    }
    check_formula();
    check_far_lines();
    check_padding_threshold();
    check_serialized_edge();
    check_generated_bytes_edge();
    check_generated_tokens_edge();
    check_expansions_edge();
    check_depth_edge();
    check_bomb();
    check_large_normal_source();
    pdxe_shutdown(ctx);
    if (failures) {
        printf("%d failure(s)\n", failures);
        return 1;
    }
    printf("ok: preprocessing is bounded by its source, at every ceiling's edge\n");
    return 0;
}
