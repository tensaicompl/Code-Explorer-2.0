/*
 * Nothing the interface does writes to standard output or standard error.
 *
 *     abi_no_output [--corpus <dir>] [--starved-typescript]
 *                   <smoke fixture dir> <resolution fixture dir>...
 *
 * The process's own output streams are pointed at files, then every file in the
 * smoke directory is extracted, every resolution directory is resolved as one
 * project (through the cache path as well as directly), and the streams are
 * restored. Both files must be empty.
 *
 * With --corpus, the sanitizer corpus (bench/corpus) is covered too: every file in it
 * is extracted, and each language's directory is resolved as one project, directly
 * and through the cache. The TypeScript and TSX projects must then be clean.
 *
 * With --starved-typescript, the environment has given the TypeScript resolver a
 * budget too small for any file (PDX_ENGINE_TS_TYPE_BUDGET), and the TypeScript and
 * TSX corpus projects must report the work it cost them: run health degraded, with
 * lost work counted. That proves the budget really ran out, along the path that once
 * printed to standard error, and the streams must still be empty (issue 28).
 *
 * The engine's log level is raised to its most verbose first, so the test shows the
 * interface silences logging whatever the environment asks for. The engine's own
 * debugging switches, which print unconditionally, are cleared: setting one is an
 * explicit request for output, and the header says so.
 */

#include <dirent.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#include "pdxe.h"

static const char *const DEBUG_SWITCHES[] = {
    "PDX_ENGINE_LSP_DEBUG", "PDX_ENGINE_LSP_KOTLIN_AST", "PDX_ENGINE_MEM_STATS_OUT",
    "PDX_ENGINE_MEM_CENSUS", "PDX_ENGINE_MEM_PHASES", "PDX_ENGINE_MEM_ALLOCATOR_STATS",
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
    if (buf && n > 0 && fread(buf, 1, (size_t)n, f) != (size_t)n) {
        free(buf);
        buf = NULL;
    }
    fclose(f);
    *len = n > 0 ? (size_t)n : 0;
    return buf;
}

/* The language a smoke fixture is written in: its name up to the first dot. */
static int smoke_language(const char *name) {
    char lang[64];
    size_t n = strcspn(name, ".");
    if (n == 0 || n >= sizeof(lang)) {
        return 0;
    }
    memcpy(lang, name, n);
    lang[n] = '\0';
    return pdxe_language_id(lang);
}

static const struct {
    const char *ext;
    const char *lang;
} EXTENSIONS[] = {
    {".py", "python"}, {".ts", "typescript"}, {".js", "javascript"}, {".go", "go"},
    {".java", "java"}, {".cs", "csharp"},     {".c", "c"},           {".h", "c"},
    {".cpp", "cpp"},   {".hpp", "cpp"},       {".rs", "rust"},
};

static int resolution_language(const char *name) {
    const char *dot = strrchr(name, '.');
    for (size_t i = 0; dot && i < sizeof(EXTENSIONS) / sizeof(EXTENSIONS[0]); i++) {
        if (strcmp(dot, EXTENSIONS[i].ext) == 0) {
            return pdxe_language_id(EXTENSIONS[i].lang);
        }
    }
    return 0;
}

static int extract_all(pdxe_ctx *ctx, const char *dir) {
    DIR *d = opendir(dir);
    if (!d) {
        return 1;
    }
    int failures = 0;
    struct dirent *e;
    while ((e = readdir(d)) != NULL) {
        int lang = e->d_name[0] == '.' ? 0 : smoke_language(e->d_name);
        if (!lang) {
            continue;
        }
        char path[4096];
        snprintf(path, sizeof(path), "%s/%s", dir, e->d_name);
        size_t len = 0;
        unsigned char *bytes = read_all(path, &len);
        pdxe_file_result *r = NULL;
        failures += !bytes || pdxe_extract_file(ctx, lang, e->d_name, bytes, len, &r) != PDXE_OK;
        pdxe_result_free(ctx, r);
        free(bytes);
    }
    closedir(d);
    return failures;
}

/* Every source file under `dir`, relative to it, found recursively. */
static void collect(const char *root, const char *rel, char ***paths, size_t *n) {
    char dir[4096];
    snprintf(dir, sizeof(dir), "%s%s%s", root, rel[0] ? "/" : "", rel);
    DIR *d = opendir(dir);
    if (!d) {
        return;
    }
    struct dirent *e;
    while ((e = readdir(d)) != NULL) {
        if (e->d_name[0] == '.') {
            continue;
        }
        char child[4096];
        snprintf(child, sizeof(child), "%s%s%s", rel, rel[0] ? "/" : "", e->d_name);
        char full[8200];
        snprintf(full, sizeof(full), "%s/%s", root, child);
        struct stat st;
        if (stat(full, &st) != 0) {
            continue;
        }
        if (S_ISDIR(st.st_mode)) {
            collect(root, child, paths, n);
        } else if (resolution_language(child)) {
            *paths = realloc(*paths, (*n + 1) * sizeof(char *));
            (*paths)[(*n)++] = strdup(child);
        }
    }
    closedir(d);
}

/* Every file under `dir`, relative to it, whatever its name: a corpus directory's
 * files are all in the directory's language. */
static void collect_all(const char *root, const char *rel, char ***paths, size_t *n) {
    char dir[4096];
    snprintf(dir, sizeof(dir), "%s%s%s", root, rel[0] ? "/" : "", rel);
    DIR *d = opendir(dir);
    if (!d) {
        return;
    }
    struct dirent *e;
    while ((e = readdir(d)) != NULL) {
        if (e->d_name[0] == '.') {
            continue;
        }
        char child[4096];
        snprintf(child, sizeof(child), "%s%s%s", rel, rel[0] ? "/" : "", e->d_name);
        char full[8200];
        snprintf(full, sizeof(full), "%s/%s", root, child);
        struct stat st;
        if (stat(full, &st) != 0) {
            continue;
        }
        if (S_ISDIR(st.st_mode)) {
            collect_all(root, child, paths, n);
        } else {
            *paths = realloc(*paths, (*n + 1) * sizeof(char *));
            (*paths)[(*n)++] = strdup(child);
        }
    }
    closedir(d);
}

/* Resolves the files under `dir` as one project. `corpus_lang` is the language of
 * every file for a corpus directory, 0 for a resolution fixture, whose files are
 * named by extension. The run's health is written to *health when it completes. */
static int resolve_all(pdxe_ctx *ctx, const char *dir, int corpus_lang, int through_cache,
                       pdxe_run_health *health) {
    char **paths = NULL;
    size_t n = 0;
    if (corpus_lang) {
        collect_all(dir, "", &paths, &n);
    } else {
        collect(dir, "", &paths, &n);
    }
    pdxe_project *p = NULL;
    int failures = pdxe_resolve_project_begin(ctx, &p) != PDXE_OK;
    pdxe_file_result **results = calloc(n ? n : 1, sizeof(*results));
    unsigned char **sources = calloc(n ? n : 1, sizeof(*sources));
    for (size_t i = 0; !failures && i < n; i++) {
        char full[8200];
        snprintf(full, sizeof(full), "%s/%s", dir, paths[i]);
        size_t len = 0;
        sources[i] = read_all(full, &len);
        int lang = corpus_lang ? corpus_lang : resolution_language(paths[i]);
        failures += !sources[i] ||
                    pdxe_extract_file(ctx, lang, paths[i], sources[i], len, &results[i]) != PDXE_OK;
        if (!failures && through_cache) {
            uint8_t *surface = NULL;
            size_t surface_len = 0;
            pdxe_file_result *rebuilt = NULL;
            const pdxe_file_result *r = results[i];
            failures += pdxe_surface_export(r, paths[i], &surface, &surface_len) != PDXE_OK ||
                        pdxe_result_build(ctx, r->defs, r->n_defs, r->calls, r->n_calls,
                                          r->imports, r->n_imports, r->usages, r->n_usages,
                                          r->types, r->n_types, r->rws, r->n_rws,
                                          &rebuilt) != PDXE_OK ||
                        pdxe_surface_import(p, surface, surface_len) != PDXE_OK;
            pdxe_surface_free(surface);
            pdxe_result_free(ctx, results[i]);
            results[i] = rebuilt;
        }
        failures += !failures && pdxe_resolve_project_add_file(p, lang, paths[i], sources[i], len,
                                                               results[i]) != PDXE_OK;
    }
    const pdxe_resolution *res = NULL;
    uint32_t count = 0;
    failures += !failures && (pdxe_resolve_project_run(p) != PDXE_OK ||
                              pdxe_resolve_project_results(p, &res, &count) != PDXE_OK ||
                              pdxe_resolve_project_health(p, health) != PDXE_OK);
    pdxe_resolve_project_end(p);
    for (size_t i = 0; i < n; i++) {
        pdxe_result_free(ctx, results[i]);
        free(sources[i]);
        free(paths[i]);
    }
    free(results);
    free(sources);
    free(paths);
    return failures;
}

/* The sanitizer corpus: every file extracted, and every language directory resolved,
 * directly and through the cache. With `starved`, the TypeScript and TSX projects
 * must have lost work to the budget. */
static int cover_corpus(pdxe_ctx *ctx, const char *root, int starved, int *starved_proven) {
    DIR *d = opendir(root);
    if (!d) {
        return 1;
    }
    int failures = 0;
    struct dirent *e;
    while ((e = readdir(d)) != NULL) {
        if (e->d_name[0] == '.') {
            continue;
        }
        int lang = pdxe_language_id(e->d_name);
        if (!lang) {
            failures++;
            continue;
        }
        char dir[4096];
        snprintf(dir, sizeof(dir), "%s/%s", root, e->d_name);
        int typescript = strcmp(e->d_name, "typescript") == 0 || strcmp(e->d_name, "tsx") == 0;
        for (int through_cache = 0; through_cache <= 1; through_cache++) {
            pdxe_run_health health;
            memset(&health, 0, sizeof(health));
            failures += resolve_all(ctx, dir, lang, through_cache, &health);
            if (starved && typescript) {
                if (health.status == PDXE_RUN_DEGRADED && health.pass_failures > 0) {
                    (*starved_proven)++;
                } else {
                    failures++;
                }
            } else if (typescript && health.status != PDXE_RUN_CLEAN) {
                /* Unstarved, the same projects are clean, so the starved run's loss
                 * is the budget's and nothing else's. */
                failures++;
            }
        }
    }
    closedir(d);
    return failures;
}

static long size_of(const char *path) {
    struct stat st;
    return stat(path, &st) == 0 ? (long)st.st_size : -1;
}

int main(int argc, char **argv) {
    const char *corpus = NULL;
    int starved = 0;
    int first = 1;
    while (first < argc && strncmp(argv[first], "--", 2) == 0) {
        if (strcmp(argv[first], "--corpus") == 0 && first + 1 < argc) {
            corpus = argv[first + 1];
            first += 2;
        } else if (strcmp(argv[first], "--starved-typescript") == 0) {
            starved = 1;
            first++;
        } else {
            fprintf(stderr, "abi_no_output: unknown option %s\n", argv[first]);
            return 2;
        }
    }
    if (argc - first < 2 || (starved && !corpus)) {
        fprintf(stderr, "usage: abi_no_output [--corpus <dir>] [--starved-typescript] "
                        "<smoke dir> <resolution dir>...\n");
        return 2;
    }
    setenv("PDX_ENGINE_LOG_LEVEL", "debug", 1);
    for (size_t i = 0; i < sizeof(DEBUG_SWITCHES) / sizeof(DEBUG_SWITCHES[0]); i++) {
        unsetenv(DEBUG_SWITCHES[i]);
    }

    const char *tmp = getenv("TMPDIR");
    tmp = (tmp && tmp[0]) ? tmp : "/tmp";
    char out_path[4096];
    char err_path[4096];
    snprintf(out_path, sizeof(out_path), "%s/abi_no_output_stdout_XXXXXX", tmp);
    snprintf(err_path, sizeof(err_path), "%s/abi_no_output_stderr_XXXXXX", tmp);
    int out_fd = mkstemp(out_path);
    int err_fd = mkstemp(err_path);
    int saved_out = dup(STDOUT_FILENO);
    int saved_err = dup(STDERR_FILENO);
    if (out_fd < 0 || err_fd < 0 || saved_out < 0 || saved_err < 0) {
        fprintf(stderr, "abi_no_output: cannot capture the output streams\n");
        return 2;
    }
    fflush(stdout);
    fflush(stderr);
    dup2(out_fd, STDOUT_FILENO);
    dup2(err_fd, STDERR_FILENO);

    pdxe_ctx *ctx = NULL;
    int failures = pdxe_init(&ctx) != PDXE_OK;
    int starved_proven = 0;
    if (!failures) {
        failures += extract_all(ctx, argv[first]);
        for (int a = first + 1; a < argc; a++) {
            pdxe_run_health health;
            failures += resolve_all(ctx, argv[a], 0, 0, &health);
            failures += resolve_all(ctx, argv[a], 0, 1, &health);
        }
        if (corpus) {
            failures += cover_corpus(ctx, corpus, starved, &starved_proven);
        }
        pdxe_shutdown(ctx);
    }

    fflush(stdout);
    fflush(stderr);
    dup2(saved_out, STDOUT_FILENO);
    dup2(saved_err, STDERR_FILENO);
    close(out_fd);
    close(err_fd);

    long out_size = size_of(out_path);
    long err_size = size_of(err_path);
    /* Two TypeScript-family directories, each resolved directly and through the cache. */
    int starved_ok = !starved || starved_proven == 4;
    int ok = failures == 0 && out_size == 0 && err_size == 0 && starved_ok;
    printf("%s: %d failed operations, %ld bytes on stdout, %ld bytes on stderr\n",
           ok ? "ok" : "FAIL", failures, out_size, err_size);
    if (starved) {
        printf("typescript budget exhausted, with the loss counted, in %d of 4 runs\n",
               starved_proven);
    }
    if (!ok && err_size > 0) {
        printf("captured stderr is in %s\n", err_path);
    } else {
        unlink(err_path);
    }
    if (!ok && out_size > 0) {
        printf("captured stdout is in %s\n", out_path);
    } else {
        unlink(out_path);
    }
    return ok ? 0 : 1;
}
