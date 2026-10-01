/*
 * A result keeps nothing of the caller's: not the path, not the source.
 *
 *     abi_inputs_are_copied <smoke fixture dir>
 *
 * Every fixture is extracted twice. Once from a path and source the caller keeps
 * alive; once from a path and source in buffers the caller overwrites and frees as
 * soon as extraction returns. The second result must describe the file exactly as the
 * first does, surface included, and under the address sanitizer any read of the freed
 * buffers fails the run outright.
 *
 * The interface's header promises that every string in a result belongs to the result.
 * The engine names a file's module definition by the path it is given, and it once
 * kept the caller's pointer for that, which only a caller freeing its path early would
 * notice.
 */

#include <dirent.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "pdxe.h"

static const char *LANGUAGES[] = {
    "java", "kotlin", "scala",  "typescript", "tsx",      "javascript", "python",   "go",
    "c",    "cpp",    "csharp", "rust",       "php",      "perl",       "ada",      "bash",
    "ruby", "swift",  "objc",   "groovy",     "lua",      "sql",        "protobuf", "graphql",
    "yaml", "json",   "toml",   "hcl",        "dockerfile", "markdown", "xml",      "properties",
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
    *len = buf && n > 0 ? fread(buf, 1, (size_t)n, f) : 0;
    fclose(f);
    return buf;
}

/* The fixture named <lang>.<ext>, or NULL. */
static char *fixture_for(const char *dir, const char *lang) {
    DIR *d = opendir(dir);
    if (!d) {
        return NULL;
    }
    struct dirent *e;
    char *found = NULL;
    size_t n = strlen(lang);
    while ((e = readdir(d)) != NULL) {
        if (strncmp(e->d_name, lang, n) == 0 && e->d_name[n] == '.') {
            found = strdup(e->d_name);
            break;
        }
    }
    closedir(d);
    return found;
}

static int export_surface(const pdxe_file_result *r, const char *path, uint8_t **out,
                          size_t *len) {
    return pdxe_surface_export(r, path, out, len);
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: abi_inputs_are_copied <smoke fixture dir>\n");
        return 2;
    }
    pdxe_ctx *ctx = NULL;
    if (pdxe_init(&ctx) != PDXE_OK) {
        return 1;
    }
    int failures = 0;
    for (size_t l = 0; l < sizeof(LANGUAGES) / sizeof(LANGUAGES[0]); l++) {
        const char *lang = LANGUAGES[l];
        char *name = fixture_for(argv[1], lang);
        if (!name) {
            printf("FAIL %s: no fixture\n", lang);
            failures++;
            continue;
        }
        char full[4096];
        snprintf(full, sizeof(full), "%s/%s", argv[1], name);
        size_t len = 0;
        unsigned char *kept = read_all(full, &len);
        int id = pdxe_language_id(lang);

        pdxe_file_result *a = NULL;
        uint8_t *sa = NULL;
        size_t la = 0;
        if (!kept || pdxe_extract_file(ctx, id, name, kept, len, &a) != PDXE_OK ||
            export_surface(a, name, &sa, &la) != PDXE_OK) {
            printf("FAIL %s: cannot extract with inputs kept\n", lang);
            failures++;
            free(name);
            free(kept);
            continue;
        }

        /* The same path and source, in buffers that are gone once extraction returns. */
        char *path = strdup(name);
        unsigned char *source = malloc(len ? len : 1);
        memcpy(source, kept, len);
        pdxe_file_result *b = NULL;
        int rc = pdxe_extract_file(ctx, id, path, source, len, &b);
        memset(path, 0xA5, strlen(path));
        memset(source, 0xA5, len);
        free(path);
        free(source);
        /* Reuse what was freed, as any caller's next allocation would. */
        for (int i = 0; i < 64; i++) {
            char *junk = malloc((size_t)(16 << (i % 8)));
            memset(junk, 0x5A, (size_t)(16 << (i % 8)));
            free(junk);
        }
        uint8_t *sb = NULL;
        size_t lb = 0;
        if (rc != PDXE_OK || export_surface(b, name, &sb, &lb) != PDXE_OK) {
            printf("FAIL %s: the result depends on the caller's freed buffers\n", lang);
            failures++;
        } else if (la != lb || memcmp(sa, sb, la) != 0) {
            printf("FAIL %s: the result changed when the caller's buffers went away\n", lang);
            failures++;
        }
        pdxe_surface_free(sa);
        pdxe_surface_free(sb);
        pdxe_result_free(ctx, a);
        pdxe_result_free(ctx, b);
        free(kept);
        free(name);
    }
    pdxe_shutdown(ctx);
    if (failures) {
        printf("%d failure(s)\n", failures);
        return 1;
    }
    printf("ok\n");
    return 0;
}
