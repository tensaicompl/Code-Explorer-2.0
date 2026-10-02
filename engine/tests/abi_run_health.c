/*
 * A run says whether typed resolution did all its work, apart from what it found.
 *
 *     abi_run_health <clean|degraded> <none|some> <dir> [<counter>=<n>...] -- <file>...
 *
 * The files are extracted, added to one project in the order given and resolved. The
 * run must complete, its health must have the status given, it must report no answer
 * or at least one as given, and the named counters must hold the values given. Every
 * file is always counted exactly once.
 *
 * The cases this is registered with (engine/CMakeLists.txt) are the point:
 *
 *   - a project with no typed answers to find is clean, not degraded;
 *   - so is one with definitions none of which typed resolution keeps, whose shared
 *     registries are then built from nothing (engine/patches/0008);
 *   - a project whose typed resolution was made to skip a file is degraded, even when
 *     the skip leaves no answer at all, so that no answer is never read as clean;
 *   - a degraded project still reports the answers it did find.
 *
 *   - a run that lost an answer it found, to an allocation failing while the answer is
 *     copied into the result, is degraded, and keeps the answers it did not lose.
 *
 * A file is made to be skipped through the engine's own test switch, which marks the
 * file named in PDX_ENGINE_TEST_LSP_SKIP_ON exactly as its node budget would; an
 * answer is lost through PDX_ENGINE_TEST_FAIL_ANSWER_ON, which fails the copy of every
 * answer whose target contains its text as a failed allocation would. The switches
 * exist only in a test build.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "pdxe.h"

static const char *language_of(const char *path) {
    const char *dot = strrchr(path, '.');
    if (!dot) {
        return "";
    }
    if (strcmp(dot, ".py") == 0) {
        return "python";
    }
    if (strcmp(dot, ".md") == 0) {
        return "markdown";
    }
    if (strcmp(dot, ".ts") == 0) {
        return "typescript";
    }
    return "";
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
    *len = buf && n > 0 ? fread(buf, 1, (size_t)n, f) : 0;
    fclose(f);
    return buf;
}

static int counter(const pdxe_run_health *h, const char *name, uint32_t *out) {
    static const struct {
        const char *name;
        size_t offset;
    } NAMES[] = {
        {"files", offsetof(pdxe_run_health, files)},
        {"files_resolved", offsetof(pdxe_run_health, files_resolved)},
        {"files_untyped", offsetof(pdxe_run_health, files_untyped)},
        {"files_empty", offsetof(pdxe_run_health, files_empty)},
        {"files_over_budget", offsetof(pdxe_run_health, files_over_budget)},
        {"files_source_unavailable", offsetof(pdxe_run_health, files_source_unavailable)},
        {"files_not_reached", offsetof(pdxe_run_health, files_not_reached)},
        {"pass_failures", offsetof(pdxe_run_health, pass_failures)},
    };
    for (size_t i = 0; i < sizeof(NAMES) / sizeof(NAMES[0]); i++) {
        if (strcmp(NAMES[i].name, name) == 0) {
            *out = *(const uint32_t *)((const char *)h + NAMES[i].offset);
            return 1;
        }
    }
    return 0;
}

int main(int argc, char **argv) {
    if (argc < 6) {
        fprintf(stderr, "usage: abi_run_health <clean|degraded> <none|some> <dir> "
                        "[<counter>=<n>...] -- <file>...\n");
        return 2;
    }
    int want_degraded = strcmp(argv[1], "degraded") == 0;
    int want_answers = strcmp(argv[2], "some") == 0;
    const char *dir = argv[3];
    int sep = 4;
    while (sep < argc && strcmp(argv[sep], "--") != 0) {
        sep++;
    }
    if (sep >= argc - 1) {
        fprintf(stderr, "no files after --\n");
        return 2;
    }
    int n_files = argc - sep - 1;

    pdxe_ctx *ctx = NULL;
    pdxe_project *p = NULL;
    if (pdxe_init(&ctx) != PDXE_OK || pdxe_resolve_project_begin(ctx, &p) != PDXE_OK) {
        printf("FAIL cannot start\n");
        return 1;
    }
    pdxe_file_result **results = calloc((size_t)n_files, sizeof(*results));
    unsigned char **sources = calloc((size_t)n_files, sizeof(*sources));
    for (int i = 0; i < n_files; i++) {
        const char *rel = argv[sep + 1 + i];
        char path[4096];
        snprintf(path, sizeof(path), "%s/%s", dir, rel);
        size_t len = 0;
        sources[i] = read_all(path, &len);
        int lang = pdxe_language_id(language_of(rel));
        if (!sources[i] || pdxe_extract_file(ctx, lang, rel, sources[i], len, &results[i]) !=
                               PDXE_OK ||
            pdxe_resolve_project_add_file(p, lang, rel, sources[i], len, results[i]) != PDXE_OK) {
            printf("FAIL %s: cannot extract or add\n", rel);
            return 1;
        }
    }

    pdxe_run_health before;
    int failures = 0;
    if (pdxe_resolve_project_health(p, &before) != PDXE_E_INVALID) {
        printf("FAIL health is reported before the run\n");
        failures++;
    }
    if (pdxe_resolve_project_run(p) != PDXE_OK) {
        printf("FAIL the run did not complete\n");
        return 1;
    }
    pdxe_run_health h;
    const pdxe_resolution *res = NULL;
    uint32_t n_res = 0;
    if (pdxe_resolve_project_health(p, &h) != PDXE_OK ||
        pdxe_resolve_project_results(p, &res, &n_res) != PDXE_OK) {
        printf("FAIL no health or results after the run\n");
        return 1;
    }
    printf("status=%d files=%u resolved=%u untyped=%u empty=%u over_budget=%u "
           "source_unavailable=%u not_reached=%u pass_failures=%u answers=%u\n",
           h.status, h.files, h.files_resolved, h.files_untyped, h.files_empty,
           h.files_over_budget, h.files_source_unavailable, h.files_not_reached,
           h.pass_failures, n_res);

    if ((h.status == PDXE_RUN_DEGRADED) != want_degraded) {
        printf("FAIL the run is %s\n", h.status == PDXE_RUN_DEGRADED ? "degraded" : "clean");
        failures++;
    }
    if ((n_res > 0) != want_answers) {
        printf("FAIL %u answers\n", n_res);
        failures++;
    }
    uint32_t counted = h.files_resolved + h.files_untyped + h.files_empty + h.files_over_budget +
                       h.files_source_unavailable + h.files_not_reached;
    if (h.files != (uint32_t)n_files || counted != h.files) {
        printf("FAIL %u files counted %u times\n", h.files, counted);
        failures++;
    }
    for (int a = 4; a < sep; a++) {
        char name[64];
        unsigned value = 0;
        uint32_t actual = 0;
        if (sscanf(argv[a], "%63[a-z_]=%u", name, &value) != 2 || !counter(&h, name, &actual)) {
            fprintf(stderr, "bad expectation %s\n", argv[a]);
            return 2;
        }
        if (actual != value) {
            printf("FAIL %s is %u, expected %u\n", name, actual, value);
            failures++;
        }
    }

    pdxe_resolve_project_end(p);
    for (int i = 0; i < n_files; i++) {
        pdxe_result_free(ctx, results[i]);
        free(sources[i]);
    }
    free(results);
    free(sources);
    pdxe_shutdown(ctx);
    if (failures) {
        printf("%d failure(s)\n", failures);
        return 1;
    }
    printf("ok\n");
    return 0;
}
