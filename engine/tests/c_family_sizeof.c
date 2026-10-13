/*
 * C and C++ preprocessing's sizeof uses one fixed data model, whatever the platform
 * (docs/plan/ISSUES.md, issue 85; decision 54;
 * engine/patches/0017-fixed-preprocessor-sizeof-model.patch).
 *
 *     c_family_sizeof
 *
 * The preprocessing library evaluates sizeof(type) inside #if and #elif, an extension no
 * compiler has, from a table of type sizes. It filled the table from the platform the
 * engine was built for: `long` is 8 bytes on Linux and macOS and 4 on Windows, `long
 * double` 16 on x86-64 and on Linux for AArch64 and 8 on Apple silicon and Windows. The
 * same source, path and engine version could extract differently on each, and no cache
 * key names a platform. The table is now written out once, LP64 as 64-bit Linux has it,
 * and is the same on every platform.
 *
 * A build of this engine reads its own platform's sizes, so the other platforms are
 * simulated: the test seam PDX_ENGINE_TEST_HOST_SIZES gives the library the sizes a
 * build elsewhere would have read, through the library's own table of a caller's sizes,
 * which it prefers to its own (a Windows build, an Apple silicon one, a 32-bit one, and
 * one in which every size is wrong). None of them may change anything:
 *
 *   - every spelling the library recognises has its fixed size, with signed and unsigned
 *     forms and repeated pointers folded as the library folds them, in C and C++;
 *   - a spelling it does not recognise (void *, wchar_t, size_t, ptrdiff_t, bool, a
 *     qualified or array type, a struct) is still not evaluated as a size;
 *   - a sizeof without parentheses, which the library evaluates by its own rules, gives
 *     the same expansion on every platform;
 *   - a whole extraction (every row of tests/differential_rows.h and the surface), whose
 *     calls depend on which branch an #if and an #elif on sizeof take, is byte-identical
 *     and takes the branch of the fixed model.
 *
 * The expected sizes are written here, never computed with this platform's sizeof, so a
 * native size in the table fails on every platform where it differs.
 */

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

#define HOST_SIZES "PDX_ENGINE_TEST_HOST_SIZES"

/* The fixed model: every spelling the library recognises, and the forms it folds into
 * them (signed and unsigned before a name are dropped, a second * is dropped). */
static const struct {
    const char *spelling;
    int size;
} SIZES[] = {
    {"char", 1},
    {"signed char", 1},
    {"unsigned char", 1},
    {"short", 2},
    {"short int", 2},
    {"unsigned short", 2},
    {"signed short int", 2},
    {"short unsigned int", 2},
    {"int", 4},
    {"signed int", 4},
    {"unsigned int", 4},
    {"long", 8},
    {"long int", 8},
    {"signed long", 8},
    {"unsigned long", 8},
    {"unsigned long int", 8},
    {"long unsigned int", 8},
    {"long long", 8},
    {"signed long long", 8},
    {"unsigned long long", 8},
    {"float", 4},
    {"double", 8},
    {"long double", 16},
    {"char *", 8},
    {"char **", 8},
    {"unsigned char *", 8},
    {"short *", 8},
    {"short int *", 8},
    {"unsigned short *", 8},
    {"int *", 8},
    {"int **", 8},
    {"unsigned int *", 8},
    {"long *", 8},
    {"long int *", 8},
    {"unsigned long *", 8},
    {"long long *", 8},
    {"unsigned long long *", 8},
    {"float *", 8},
    {"double *", 8},
    {"long double *", 8},
    {"long double **", 8},
};

/* Spellings the library does not recognise: never evaluated as a size. */
static const char *const UNKNOWN[] = {"void *",        "wchar_t",   "size_t",
                                      "ptrdiff_t",     "bool",      "_Bool",
                                      "unsigned",      "signed",    "long long int",
                                      "const char *",  "int [ 4 ]", "struct s",
                                      "unsigned long long int"};

/* sizeof without parentheses, which the library evaluates by its own rules. */
static const char *const BARE[] = {"sizeof long == 8", "sizeof int == 4",
                                   "sizeof long double == 16", "sizeof char"};

/* Other platforms' sizes, as a build there would have read them; "" is this build's own. */
static const struct {
    const char *name;
    const char *sizes;
} HOSTS[] = {
    {"this build", ""},
    {"64-bit Windows", "long=4,long int=4,long double=8"},
    {"Apple silicon", "long double=8"},
    {"a 32-bit build", "long=4,long int=4,long double=12,char *=4,short *=4,short int *=4,"
                       "int *=4,long *=4,long int *=4,long long *=4,float *=4,double *=4,"
                       "long double *=4"},
    {"every size wrong",
     "char=99,short=99,short int=99,int=99,long=99,long int=99,long long=99,float=99,"
     "double=99,long double=99,char *=99,short *=99,short int *=99,int *=99,long *=99,"
     "long int *=99,long long *=99,float *=99,double *=99,long double *=99"},
};

#define N_HOSTS (sizeof(HOSTS) / sizeof(HOSTS[0]))

static void be_host(size_t h) {
    if (HOSTS[h].sizes[0]) {
        setenv(HOST_SIZES, HOSTS[h].sizes, 1);
    } else {
        unsetenv(HOST_SIZES);
    }
}

/* The expanded text of `source`, or "" when the library made none. */
static char *expand(const char *source, int cpp_mode) {
    PDXEPreprocessedSource *pp =
        pdxe_preprocess_with_map(source, (int)strlen(source), "src/model.c", NULL, NULL, cpp_mode);
    char *text = strdup(pp ? pp->source : "");
    if (pp) {
        pdxe_preprocessed_source_free(pp);
    }
    return text;
}

static char *probe(const char *condition) {
    static char source[1024];
    snprintf(source, sizeof(source),
             "#if %s\n#define PICK() picked_true()\n#else\n#define PICK() picked_false()\n#endif\n"
             "int f(void) { return PICK(); }\n",
             condition);
    return source;
}

/* Every recognised spelling has its fixed size, on every platform. */
static void check_sizes(void) {
    for (size_t h = 0; h < N_HOSTS; h++) {
        be_host(h);
        for (int cpp = 0; cpp <= 1; cpp++) {
            for (size_t i = 0; i < sizeof(SIZES) / sizeof(SIZES[0]); i++) {
                char condition[128];
                snprintf(condition, sizeof(condition), "sizeof(%s) == %d", SIZES[i].spelling,
                         SIZES[i].size);
                char *text = expand(probe(condition), cpp);
                CHECK(strstr(text, "picked_true") != NULL,
                      "%s, %s: sizeof(%s) is not %d", HOSTS[h].name, cpp ? "C++" : "C",
                      SIZES[i].spelling, SIZES[i].size);
                free(text);
            }
        }
    }
    unsetenv(HOST_SIZES);
}

/* What the library does not recognise, or evaluates by its own rules, is the same on every
 * platform; an unrecognised spelling is never a size. */
static void check_others(void) {
    for (int cpp = 0; cpp <= 1; cpp++) {
        for (size_t i = 0; i < sizeof(UNKNOWN) / sizeof(UNKNOWN[0]); i++) {
            char condition[128];
            snprintf(condition, sizeof(condition), "sizeof(%s) == 8 || sizeof(%s) != 8",
                     UNKNOWN[i], UNKNOWN[i]);
            unsetenv(HOST_SIZES);
            char *own = expand(probe(condition), cpp);
            CHECK(strstr(own, "picked_true") == NULL,
                  "%s: sizeof(%s) was evaluated as a size", cpp ? "C++" : "C", UNKNOWN[i]);
            for (size_t h = 1; h < N_HOSTS; h++) {
                be_host(h);
                char *text = expand(probe(condition), cpp);
                CHECK(strcmp(own, text) == 0, "%s, %s: sizeof(%s) changed", HOSTS[h].name,
                      cpp ? "C++" : "C", UNKNOWN[i]);
                free(text);
            }
            free(own);
        }
        for (size_t i = 0; i < sizeof(BARE) / sizeof(BARE[0]); i++) {
            unsetenv(HOST_SIZES);
            char *own = expand(probe(BARE[i]), cpp);
            for (size_t h = 1; h < N_HOSTS; h++) {
                be_host(h);
                char *text = expand(probe(BARE[i]), cpp);
                CHECK(strcmp(own, text) == 0, "%s, %s: #if %s changed", HOSTS[h].name,
                      cpp ? "C++" : "C", BARE[i]);
                free(text);
            }
            free(own);
        }
    }
    unsetenv(HOST_SIZES);
}

static const char MODEL_SOURCE[] =
    "int model_lp64(void);\n"
    "#if sizeof(long) == 8 && sizeof(long double) == 16 && sizeof(char *) == 8\n"
    "#define MODEL() model_lp64()\n"
    "#elif sizeof(long) == 4\n"
    "#define MODEL() model_llp64()\n"
    "#else\n"
    "#define MODEL() model_other()\n"
    "#endif\n"
    "#if sizeof(long double) == 16\n"
    "#define WIDE() wide_sixteen()\n"
    "#elif sizeof(long double) == 8\n"
    "#define WIDE() wide_eight()\n"
    "#endif\n"
    "int f(void) { return MODEL() + WIDE(); }\n";

static pdxe_ctx *ctx = NULL;

/* Every row and the surface of one extraction, as one string. */
static char *extract(int lang, const char *rel) {
    pdxe_file_result *r = NULL;
    if (pdxe_extract_file(ctx, lang, rel, (const uint8_t *)MODEL_SOURCE, strlen(MODEL_SOURCE),
                          &r) != PDXE_OK ||
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
    diff_rows_file(o, rel, engine, MODEL_SOURCE);
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

/* The whole extraction takes the fixed model's branch, on every platform. */
static void check_extraction(void) {
    struct {
        const char *name;
        int lang;
        const char *rel;
    } cases[] = {
        {"C", pdxe_language_id("c"), "src/model.c"},
        {"C++", pdxe_language_id("cpp"), "src/model.cpp"},
    };
    for (size_t i = 0; i < sizeof(cases) / sizeof(cases[0]); i++) {
        unsetenv(HOST_SIZES);
        char *own = extract(cases[i].lang, cases[i].rel);
        CHECK(strstr(own, "\"callee\":\"model_lp64\"") != NULL &&
                  strstr(own, "\"callee\":\"wide_sixteen\"") != NULL,
              "%s: the fixed model's branches were not taken", cases[i].name);
        CHECK(strstr(own, "model_llp64") == NULL && strstr(own, "model_other") == NULL &&
                  strstr(own, "wide_eight") == NULL,
              "%s: another model's branch was taken", cases[i].name);
        for (size_t h = 1; h < N_HOSTS; h++) {
            be_host(h);
            char *text = extract(cases[i].lang, cases[i].rel);
            CHECK(strcmp(own, text) == 0, "%s: %s changed the result", cases[i].name,
                  HOSTS[h].name);
            free(text);
        }
        free(own);
    }
    unsetenv(HOST_SIZES);
}

int main(void) {
    if (pdxe_init(&ctx) != PDXE_OK) {
        printf("FAIL the engine did not start\n");
        return 1;
    }
    check_sizes();
    check_others();
    check_extraction();
    pdxe_shutdown(ctx);
    if (failures) {
        printf("%d failure(s)\n", failures);
        return 1;
    }
    printf("ok: sizeof in #if has one fixed model, %zu spellings, %zu platforms\n",
           sizeof(SIZES) / sizeof(SIZES[0]), N_HOSTS);
    return 0;
}
