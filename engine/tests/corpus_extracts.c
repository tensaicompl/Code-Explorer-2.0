/*
 * The engine processes every file of the sanitizer corpus safely.
 *
 *     corpus_extracts <corpus dir>
 *
 * The corpus (bench/corpus) is test data with an explicit layout: each top-level
 * directory is named after the language its files are written in, one of the
 * languages the interface's tests cover (matrix_languages.h), and everything beneath
 * it is a source in that language. Anything else is a failure, not something to skip:
 * a file at the top level, a directory named after no language, a hidden entry.
 *
 * Every file is read and extracted through the interface. Extraction must succeed, and
 * the file's status must be the one its name declares:
 *
 *   recovery_*  source a parser has to recover from: parsed, or partial
 *   failed_*    source the engine refuses by design: failed, with a diagnostic
 *   any other   parsed
 *
 * The corpus must hold at least one file and at most CORPUS_MAX_FILES, and at least
 * one file in every language. The counts are printed, so a run shows what it did.
 *
 * Built with the rest of the tests, and run by make check-asan under the address,
 * undefined-behaviour and leak sanitizers, where any report fails the run.
 */

#include <dirent.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

#include "matrix_languages.h"
#include "pdxe.h"

/* The plan's bound on the corpus: small enough to run on every nightly build. */
enum { CORPUS_MAX_FILES = 200 };

static int failures = 0;
static int files_by_language[N_MATRIX_LANGUAGES];

#define FAIL(...)                                                                                  \
    do {                                                                                           \
        printf("FAIL ");                                                                           \
        printf(__VA_ARGS__);                                                                       \
        printf("\n");                                                                              \
        failures++;                                                                                \
    } while (0)

static int language_index(const char *name) {
    for (int i = 0; i < N_MATRIX_LANGUAGES; i++) {
        if (strcmp(MATRIX_LANGUAGES[i], name) == 0) {
            return i;
        }
    }
    return -1;
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

static int is_dir(const char *path) {
    struct stat st;
    return stat(path, &st) == 0 && S_ISDIR(st.st_mode);
}

static int cmp_names(const void *a, const void *b) {
    return strcmp(*(char *const *)a, *(char *const *)b);
}

/* The visible entries of a directory, sorted, or NULL with *n = 0 if it cannot be read. */
static char **entries(const char *dir, size_t *n) {
    *n = 0;
    DIR *d = opendir(dir);
    if (!d) {
        return NULL;
    }
    char **names = NULL;
    struct dirent *e;
    while ((e = readdir(d)) != NULL) {
        if (strcmp(e->d_name, ".") == 0 || strcmp(e->d_name, "..") == 0) {
            continue;
        }
        names = realloc(names, (*n + 1) * sizeof(char *));
        names[(*n)++] = strdup(e->d_name);
    }
    closedir(d);
    qsort(names, *n, sizeof(char *), cmp_names);
    return names;
}

/* The status a file must come back with, from its name. */
static int expected(const char *name, int status) {
    if (strncmp(name, "recovery_", 9) == 0) {
        return status == PDXE_FILE_PARSED || status == PDXE_FILE_PARTIAL;
    }
    if (strncmp(name, "failed_", 7) == 0) {
        return status == PDXE_FILE_FAILED;
    }
    return status == PDXE_FILE_PARSED;
}

static void extract_one(pdxe_ctx *ctx, int lang_index, const char *full, const char *rel,
                        const char *name) {
    const char *language = MATRIX_LANGUAGES[lang_index];
    int lang = pdxe_language_id(language);
    if (lang <= 0) {
        FAIL("%s: the engine does not know %s", rel, language);
        return;
    }
    size_t len = 0;
    unsigned char *bytes = read_all(full, &len);
    if (!bytes) {
        FAIL("%s: unreadable", rel);
        return;
    }
    pdxe_file_result *r = NULL;
    int rc = pdxe_extract_file(ctx, lang, rel, bytes, len, &r);
    files_by_language[lang_index]++;
    if (rc != PDXE_OK || !r) {
        FAIL("%s: extraction returned %d", rel, rc);
    } else {
        if (!expected(name, r->status)) {
            FAIL("%s: status %d is not the one its name declares", rel, r->status);
        }
        if (r->status == PDXE_FILE_FAILED && r->n_diags == 0) {
            FAIL("%s: failed without a diagnostic", rel);
        }
    }
    pdxe_result_free(ctx, r);
    free(bytes);
}

/* Every file beneath `dir`, a directory of `lang_index`'s sources, recursively. */
static void walk(pdxe_ctx *ctx, int lang_index, const char *dir, const char *rel) {
    size_t n = 0;
    char **names = entries(dir, &n);
    if (!names) {
        FAIL("%s: unreadable directory", rel);
        return;
    }
    for (size_t i = 0; i < n; i++) {
        char full[4096];
        char child[4096];
        snprintf(full, sizeof(full), "%s/%s", dir, names[i]);
        snprintf(child, sizeof(child), "%s/%s", rel, names[i]);
        if (names[i][0] == '.') {
            FAIL("%s: a hidden entry in the corpus", child);
        } else if (is_dir(full)) {
            walk(ctx, lang_index, full, child);
        } else {
            extract_one(ctx, lang_index, full, child, names[i]);
        }
        free(names[i]);
    }
    free(names);
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: corpus_extracts <corpus dir>\n");
        return 2;
    }
    const char *root = argv[1];
    size_t n = 0;
    char **names = entries(root, &n);
    if (!names) {
        printf("FAIL %s: no corpus\n", root);
        return 1;
    }
    pdxe_ctx *ctx = NULL;
    if (pdxe_init(&ctx) != PDXE_OK) {
        return 1;
    }
    for (size_t i = 0; i < n; i++) {
        char full[4096];
        snprintf(full, sizeof(full), "%s/%s", root, names[i]);
        int index = language_index(names[i]);
        if (!is_dir(full)) {
            FAIL("%s: a file at the top of the corpus, where only language directories go",
                 names[i]);
        } else if (index < 0) {
            FAIL("%s: a directory named after no language the tests cover", names[i]);
        } else {
            walk(ctx, index, full, names[i]);
        }
        free(names[i]);
    }
    free(names);
    pdxe_shutdown(ctx);

    int total = 0;
    int covered = 0;
    for (int i = 0; i < N_MATRIX_LANGUAGES; i++) {
        total += files_by_language[i];
        if (files_by_language[i] > 0) {
            covered++;
        } else {
            FAIL("%s: no file in the corpus", MATRIX_LANGUAGES[i]);
        }
        printf("%-12s %d\n", MATRIX_LANGUAGES[i], files_by_language[i]);
    }
    if (total == 0 || total > CORPUS_MAX_FILES) {
        FAIL("the corpus holds %d files; it must hold between 1 and %d", total,
             CORPUS_MAX_FILES);
    }
    printf("%s: %d files extracted, %d of %d languages covered, %d failure(s)\n",
           failures ? "FAIL" : "ok", total, covered, N_MATRIX_LANGUAGES, failures);
    return failures ? 1 : 0;
}
