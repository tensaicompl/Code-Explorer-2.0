/*
 * What C and C++ extraction makes of character constants and #line numbers is the same on
 * every platform (docs/plan/ISSUES.md, issue 86; decision 55;
 * engine/patches/0018-host-independent-literals-and-line.patch).
 *
 *     c_family_char_literals
 *
 * The preprocessing library gave an ordinary one-byte character constant the value of
 * the plain char of the compiler that built it: '\xff' was -1 where char is signed
 * (x86-64, Apple silicon, Windows) and 255 where it is unsigned (Linux on AArch64). It
 * read #line numbers with atol, whose C long is 32 bits on Windows and 64 elsewhere, so
 * #line 2147483648 gave line 2147483647 on Windows, and #line 4294967296 gave line 0 on
 * Linux and macOS. Now a one-byte constant is a signed 8-bit char everywhere, and a #line
 * number is its leading decimal digits read at a fixed width, at most 4294967295.
 *
 * Whole extractions, C and C++, whose calls are chosen by #if, #elif and macro expansion
 * on those values, must call what the fixed rules choose: every row of
 * tests/differential_rows.h and the surface. Each large #line names a file of its own,
 * and its __LINE__ is carried back to the source's own lines by a macro (a far jump is
 * written as one #line marker either way, issue 87). preprocessor_host_independence.cpp
 * builds the library with char unsigned too.
 */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "pdxe.h"
#include "pdxe_core.h"
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

static pdxe_ctx *ctx = NULL;

/* Every row and the surface of one extraction, as one string. */
static char *extract(int lang, const char *rel, const char *source) {
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

static int calls(const char *rows, const char *callee) {
    char needle[256];
    snprintf(needle, sizeof(needle), "\"callee\":\"%s\",", callee);
    return strstr(rows, needle) != NULL;
}

static const char LITERALS[] = "#if '\\xff' < 0\n"
                               "#define BRANCH() signed_branch()\n"
                               "#else\n"
                               "#define BRANCH() unsigned_branch()\n"
                               "#endif\n"
                               "#if '\\x80' > 0\n"
                               "#define LOW() low_positive()\n"
                               "#elif '\\x80' == -128 && '\\377' == -1 && '\\x7f' == 127\n"
                               "#define LOW() low_is_minus_128()\n"
                               "#endif\n"
                               "#if 'ab' == 0x6162 && '\\xff\\xff\\xff\\xff' == -1\n"
                               "#define MULTI() multi_is_int32()\n"
                               "#endif\n"
                               "#if u'\\xff' == 255 && L'\\xff' == 255 && U'\\xff' == 255\n"
                               "#define WIDE() wide_unsigned()\n"
                               "#endif\n"
                               "void selected(void) {\n"
                               "    BRANCH();\n"
                               "    LOW();\n"
                               "    MULTI();\n"
                               "    WIDE();\n"
                               "}\n";

/* #line 2147483648 and 4294967296, in a file of another name, then back to the source's
 * own lines: what __LINE__ was there decides the calls below. */
static const char LINES[] = "#define KEEP 1\n"
                            "#line 2147483648 \"wide.c\"\n"
                            "#if __LINE__ == 2147483648\n"
                            "#define WIDE_LINE() line_exact()\n"
                            "#elif __LINE__ == 2147483647\n"
                            "#define WIDE_LINE() line_long_saturated()\n"
                            "#endif\n"
                            "#line 4294967296 \"huge.c\"\n"
                            "#if __LINE__ == 4294967295\n"
                            "#define HUGE_LINE() line_clamped()\n"
                            "#elif __LINE__ == 0\n"
                            "#define HUGE_LINE() line_wrapped()\n"
                            "#elif __LINE__ == 2147483647\n"
                            "#define HUGE_LINE() line_long_saturated()\n"
                            "#endif\n"
                            "#line 20 \"%s\"\n"
                            "void lines(void) {\n"
                            "    WIDE_LINE();\n"
                            "    HUGE_LINE();\n"
                            "}\n";

int main(void) {
    if (pdxe_init(&ctx) != PDXE_OK) {
        printf("FAIL the engine did not start\n");
        return 1;
    }
    struct {
        const char *name;
        int lang;
        const char *rel;
    } cases[] = {
        {"C", pdxe_language_id("c"), "src/chars.c"},
        {"C++", pdxe_language_id("cpp"), "src/chars.cpp"},
    };
    for (size_t i = 0; i < sizeof(cases) / sizeof(cases[0]); i++) {
        char *rows = extract(cases[i].lang, cases[i].rel, LITERALS);
        for (const char *const *callee = (const char *const[]){"signed_branch", "low_is_minus_128",
                                                               "multi_is_int32", "wide_unsigned",
                                                               NULL};
             *callee; callee++) {
            CHECK(calls(rows, *callee), "%s: %s is not called", cases[i].name, *callee);
        }
        CHECK(!calls(rows, "unsigned_branch") && !calls(rows, "low_positive"),
              "%s: a branch of unsigned char was taken", cases[i].name);
        free(rows);

        char source[2048];
        snprintf(source, sizeof(source), LINES, cases[i].rel);
        rows = extract(cases[i].lang, cases[i].rel, source);
        CHECK(calls(rows, "line_exact"), "%s: #line 2147483648 did not give that line",
              cases[i].name);
        CHECK(calls(rows, "line_clamped"), "%s: #line 4294967296 did not give 4294967295",
              cases[i].name);
        CHECK(!calls(rows, "line_wrapped") && !calls(rows, "line_long_saturated"),
              "%s: a platform's long decided a line", cases[i].name);
        free(rows);
    }
    pdxe_shutdown(ctx);
    if (failures) {
        printf("%d failure(s)\n", failures);
        return 1;
    }
    printf("ok: character constants and #line numbers take the fixed rules\n");
    return 0;
}
