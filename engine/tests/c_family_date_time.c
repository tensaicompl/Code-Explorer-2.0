/*
 * C and C++ preprocessing reads no clock (docs/plan/ISSUES.md, issue 84; decision 53;
 * engine/patches/0016-fixed-date-time-macros.patch).
 *
 *     c_family_date_time
 *
 * The preprocessing library predefined __DATE__ and __TIME__ from the clock, in the
 * process's local time zone and with the locale's month name, so a source that expanded
 * either extracted differently with the time, the zone and the locale, none of which is
 * an input any cache key names. They are now fixed, in the library's own formats, at the
 * start of the Unix epoch: "Jan 01 1970" and "00:00:00". Nothing is read to make them.
 *
 *   - The expansion itself (pdxe_preprocess_with_map), C and C++: exactly those values.
 *     No clock running now gives them, so they were not read from one.
 *   - What extraction reports: every row of tests/differential_rows.h and the surface,
 *     byte-identical under time zones east and west of UTC and a fractional one, set as
 *     POSIX TZ rules so that no zone database is needed, and under every month-name
 *     locale this machine has; the call's string arguments are the fixed values.
 *   - A source's own definitions keep their precedence: one that defines __DATE__ gets
 *     its own value, one that undefines __TIME__ keeps the name, and #ifdef sees both
 *     defined before that.
 *   - __TIMESTAMP__ is no predefined macro of the library and stays a name.
 */

#include <locale.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

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

static const char STAMP_SOURCE[] = "void record_stamp(const char *, const char *);\n"
                                   "\n"
                                   "#define STAMP() record_stamp(__DATE__, __TIME__)\n"
                                   "#define TS() record_timestamp(__TIMESTAMP__)\n"
                                   "\n"
                                   "void f(void) {\n"
                                   "    STAMP();\n"
                                   "    TS();\n"
                                   "}\n";

static const char OVERRIDE_SOURCE[] = "#ifdef __TIME__\n"
                                      "#define TIME_SEEN() saw_time_defined()\n"
                                      "#endif\n"
                                      "#ifdef __DATE__\n"
                                      "#define DATE_SEEN() saw_date_defined()\n"
                                      "#endif\n"
                                      "#define __DATE__ \"CUSTOM_DATE\"\n"
                                      "#undef __TIME__\n"
                                      "#define STAMP() record_stamp(__DATE__, __TIME__)\n"
                                      "void g(void) { TIME_SEEN(); DATE_SEEN(); STAMP(); }\n";

/* Time zones as POSIX rules, which the C library reads without a zone database: UTC, New
 * Zealand (east, with daylight saving), the US Pacific coast (west, with daylight saving)
 * and Nepal's fractional offset. */
static const char *const ZONES[] = {"UTC0", "NZST-12NZDT,M9.5.0,M4.1.0/3",
                                    "PST8PDT,M3.2.0,M11.1.0", "<+0545>-5:45"};

/* Locales whose month names differ; those the machine lacks are skipped. */
static const char *const LOCALES[] = {"C",           "C.UTF-8",     "en_US.UTF-8", "de_DE.UTF-8",
                                      "fr_FR.UTF-8", "pl_PL.UTF-8", "ja_JP.UTF-8"};

static pdxe_ctx *ctx = NULL;

/* The expanded text of `source`, or NULL with none. */
static char *expand(const char *source, int cpp_mode) {
    PDXEPreprocessedSource *pp =
        pdxe_preprocess_with_map(source, (int)strlen(source), "src/stamp.c", NULL, NULL, cpp_mode);
    if (!pp) {
        return NULL;
    }
    char *text = strdup(pp->source);
    pdxe_preprocessed_source_free(pp);
    return text;
}

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
    diff_rows_file(o, rel, engine);
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

static void set_zone(const char *zone) {
    if (setenv("TZ", zone, 1) != 0) {
        printf("FAIL cannot set TZ\n");
        exit(1);
    }
    tzset();
}

/* The fixed values, in the expansion, for C and C++. */
static void check_expansion(void) {
    for (int cpp = 0; cpp <= 1; cpp++) {
        char *text = expand(STAMP_SOURCE, cpp);
        const char *mode = cpp ? "C++" : "C";
        CHECK(text != NULL, "%s: the source was not preprocessed", mode);
        if (!text) {
            continue;
        }
        CHECK(strstr(text, "\"Jan 01 1970\"") != NULL, "%s: __DATE__ is not \"Jan 01 1970\":\n%s",
              mode, text);
        CHECK(strstr(text, "\"00:00:00\"") != NULL, "%s: __TIME__ is not \"00:00:00\":\n%s", mode,
              text);
        CHECK(strstr(text, "__TIMESTAMP__") != NULL, "%s: __TIMESTAMP__ was expanded:\n%s", mode,
              text);
        free(text);
    }
}

/* One result under every zone and locale, carrying the fixed values. */
static void check_extraction(void) {
    struct {
        const char *name;
        int lang;
        const char *rel;
    } cases[] = {
        {"C", pdxe_language_id("c"), "src/stamp.c"},
        {"C++", pdxe_language_id("cpp"), "src/stamp.cpp"},
    };
    for (size_t i = 0; i < sizeof(cases) / sizeof(cases[0]); i++) {
        set_zone(ZONES[0]);
        setlocale(LC_ALL, "C");
        char *first = extract(cases[i].lang, cases[i].rel, STAMP_SOURCE);
        CHECK(strstr(first, "\"callee\":\"record_stamp\"") != NULL,
              "%s: no call of record_stamp was found", cases[i].name);
        CHECK(strstr(first, "\"first_string_arg\":\"Jan 01 1970\"") != NULL,
              "%s: the call's first string argument is not the fixed date", cases[i].name);
        CHECK(strstr(first, "\"value\":\"00:00:00\"") != NULL,
              "%s: the call's second argument is not the fixed time", cases[i].name);
        for (size_t z = 0; z < sizeof(ZONES) / sizeof(ZONES[0]); z++) {
            for (size_t l = 0; l < sizeof(LOCALES) / sizeof(LOCALES[0]); l++) {
                if (!setlocale(LC_ALL, LOCALES[l])) {
                    continue;
                }
                set_zone(ZONES[z]);
                char *again = extract(cases[i].lang, cases[i].rel, STAMP_SOURCE);
                CHECK(strcmp(first, again) == 0, "%s: TZ=%s, locale %s changed the result",
                      cases[i].name, ZONES[z], LOCALES[l]);
                free(again);
            }
        }
        setlocale(LC_ALL, "C");
        free(first);
    }
    set_zone(ZONES[0]);
}

/* A source's own definitions come first, as they always did. */
static void check_overrides(void) {
    for (int cpp = 0; cpp <= 1; cpp++) {
        char *text = expand(OVERRIDE_SOURCE, cpp);
        const char *mode = cpp ? "C++" : "C";
        CHECK(text != NULL, "%s: the source was not preprocessed", mode);
        if (!text) {
            continue;
        }
        CHECK(strstr(text, "saw_time_defined") != NULL, "%s: #ifdef __TIME__ was false", mode);
        CHECK(strstr(text, "saw_date_defined") != NULL, "%s: #ifdef __DATE__ was false", mode);
        CHECK(strstr(text, "\"CUSTOM_DATE\"") != NULL,
              "%s: the source's own __DATE__ was not used:\n%s", mode, text);
        CHECK(strstr(text, "__TIME__") != NULL && strstr(text, "00:00:00") == NULL,
              "%s: an undefined __TIME__ was expanded:\n%s", mode, text);
        free(text);
    }
}

int main(void) {
    if (pdxe_init(&ctx) != PDXE_OK) {
        printf("FAIL the engine did not start\n");
        return 1;
    }
    int locales = 0;
    for (size_t l = 0; l < sizeof(LOCALES) / sizeof(LOCALES[0]); l++) {
        locales += setlocale(LC_ALL, LOCALES[l]) != NULL;
    }
    setlocale(LC_ALL, "C");
    check_expansion();
    check_extraction();
    check_overrides();
    pdxe_shutdown(ctx);
    if (failures) {
        printf("%d failure(s)\n", failures);
        return 1;
    }
    printf("ok: __DATE__ and __TIME__ are fixed, under %zu time zones and %d locales\n",
           sizeof(ZONES) / sizeof(ZONES[0]), locales);
    return 0;
}
