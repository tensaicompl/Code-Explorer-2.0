/*
 * Every language of the language matrix, through the interface: one small file each,
 * extracted, must parse and must yield definitions.
 *
 *     abi_smoke <fixture dir>
 *
 * Each fixture is named after the language it is written in, `<language>.<ext>`, and
 * the language is taken from the name, so a fixture cannot silently be read as some
 * other language than the one it tests. Every language the matrix lists must have a
 * fixture: a missing one fails the run rather than shrinking it.
 */

#include <dirent.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "pdxe.h"

/* The language matrix, and the one variant of it the interface names separately. */
static const char *const LANGUAGES[] = {
    "java",   "kotlin",   "scala",  "typescript", "tsx",        "javascript", "python",
    "go",     "c",        "cpp",    "csharp",     "rust",       "php",        "perl",
    "ada",    "bash",     "ruby",   "swift",      "objc",       "groovy",     "lua",
    "sql",    "protobuf", "graphql", "yaml",      "json",       "toml",       "hcl",
    "dockerfile", "markdown", "xml", "properties",
};
enum { N_LANGUAGES = sizeof(LANGUAGES) / sizeof(LANGUAGES[0]) };

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

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: abi_smoke <fixture dir>\n");
        return 2;
    }
    pdxe_ctx *ctx = NULL;
    if (pdxe_init(&ctx) != PDXE_OK) {
        fprintf(stderr, "abi_smoke: init failed\n");
        return 1;
    }
    int failures = 0;
    for (int i = 0; i < N_LANGUAGES; i++) {
        const char *lang = LANGUAGES[i];
        char path[4096] = "";
        DIR *d = opendir(argv[1]);
        struct dirent *e;
        size_t lang_len = strlen(lang);
        while (d && (e = readdir(d)) != NULL) {
            if (strncmp(e->d_name, lang, lang_len) == 0 && e->d_name[lang_len] == '.') {
                snprintf(path, sizeof(path), "%s/%s", argv[1], e->d_name);
                break;
            }
        }
        if (d) {
            closedir(d);
        }
        if (!path[0]) {
            printf("FAIL %-11s no fixture\n", lang);
            failures++;
            continue;
        }
        int id = pdxe_language_id(lang);
        size_t len = 0;
        unsigned char *bytes = read_all(path, &len);
        pdxe_file_result *r = NULL;
        int rc = (id > 0 && bytes) ? pdxe_extract_file(ctx, id, path, bytes, len, &r) : -99;
        if (rc != PDXE_OK || !r) {
            printf("FAIL %-11s extraction returned %d\n", lang, rc);
            failures++;
        } else {
            /* Every file has a module definition; the file's own content must add more. */
            uint32_t own = 0;
            for (uint32_t k = 0; k < r->n_defs; k++) {
                own += strcmp(r->defs[k].kind, "module") != 0;
            }
            if (r->status != PDXE_FILE_PARSED || own == 0) {
                printf("FAIL %-11s status %d, %u definitions besides the module\n", lang,
                       r->status, own);
                failures++;
            } else {
                printf("ok   %-11s %u definitions besides the module, %u calls\n", lang, own,
                       r->n_calls);
            }
        }
        pdxe_result_free(ctx, r);
        free(bytes);
    }
    pdxe_shutdown(ctx);
    return failures ? 1 : 0;
}
