/*
 * Nothing the interface does writes to standard output or standard error.
 *
 *     abi_no_output <smoke fixture dir> <resolution fixture dir>...
 *
 * The process's own output streams are pointed at files, then every file in the
 * smoke directory is extracted, every resolution directory is resolved as one
 * project (through the cache path as well as directly), and the streams are
 * restored. Both files must be empty.
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

static int resolve_all(pdxe_ctx *ctx, const char *dir, int through_cache) {
    char **paths = NULL;
    size_t n = 0;
    collect(dir, "", &paths, &n);
    pdxe_project *p = NULL;
    int failures = pdxe_resolve_project_begin(ctx, &p) != PDXE_OK;
    pdxe_file_result **results = calloc(n ? n : 1, sizeof(*results));
    unsigned char **sources = calloc(n ? n : 1, sizeof(*sources));
    for (size_t i = 0; !failures && i < n; i++) {
        char full[8200];
        snprintf(full, sizeof(full), "%s/%s", dir, paths[i]);
        size_t len = 0;
        sources[i] = read_all(full, &len);
        int lang = resolution_language(paths[i]);
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
                              pdxe_resolve_project_results(p, &res, &count) != PDXE_OK);
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

static long size_of(const char *path) {
    struct stat st;
    return stat(path, &st) == 0 ? (long)st.st_size : -1;
}

int main(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "usage: abi_no_output <smoke dir> <resolution dir>...\n");
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
    if (!failures) {
        failures += extract_all(ctx, argv[1]);
        for (int a = 2; a < argc; a++) {
            failures += resolve_all(ctx, argv[a], 0);
            failures += resolve_all(ctx, argv[a], 1);
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
    int ok = failures == 0 && out_size == 0 && err_size == 0;
    printf("%s: %d failed operations, %ld bytes on stdout, %ld bytes on stderr\n",
           ok ? "ok" : "FAIL", failures, out_size, err_size);
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
