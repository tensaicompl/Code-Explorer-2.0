/*
 * Counts who owns the call and callable-reference sites of a list of source files
 * (docs/plan/ISSUES.md, issues 75 and 76; decision 48):
 *
 *     owner_census [--list | --occurrences] <root>   < "<language>\t<path>" lines
 *
 * Each file is extracted through the interface, as a build extracts it. Its sites are
 * the calls and callable references a segment stores: those with a position in the
 * file and a node-type path. A site is inside a represented callable when its bytes
 * lie within the declaration occurrence of one of the file's callable definitions read
 * from the raw source (the engine's own, PDXECallableOccurrence). Such a site must be
 * owned by a definition, never by the file. Bytes, not lines: two definitions on one
 * line are told apart, and a decorator above a function is not in it. A class is no
 * callable, so its span decides nothing.
 *
 * One row per file extension, tab-separated, then a row for all of them:
 *
 *     <ext> sites=N inside=N orphan=N owned_outside=N file=N adopted_lines=N
 *
 *   orphan          inside a represented callable and owned by the file: what issues
 *                   75 and 76 found, which must be 0
 *   owned_outside   owned by a definition without being inside a callable read from the
 *                   raw source: one the preprocessed pass of C, C++ or CUDA adopted in
 *                   place of a raw definition it superseded (PDXEOwnerAlias)
 *   file            outside every represented callable and owned by the file
 *   adopted_lines   of `file`, those on the lines of a callable that pass adopted. Its
 *                   occurrence is in the macro-expanded text, to which no byte of the raw
 *                   source maps, so only lines can place a raw site in it
 *
 * --list also prints each orphan, owned_outside and adopted_lines site, as
 * "<class>\t<path>:<line>\t<callee>[\t<definition>]". --occurrences prints instead the
 * raw declaration occurrence of every callable, "occurrence\t<path>\t<start>\t<end>", so
 * another engine's sites in the same bytes can be counted by them. A file that cannot be
 * read or extracted stops the census.
 */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "pdxe.h"
#include "pdxe_core.h"
#include "shim_internal.h"

enum { MAX_EXTENSIONS = 64, LINE_MAX_BYTES = 8192 };

typedef struct {
    char ext[16];
    unsigned long sites, inside, orphan, owned_outside, file, adopted_lines;
} row;

static row ROWS[MAX_EXTENSIONS + 1];
static int n_rows;

static row *row_for(const char *path) {
    const char *slash = strrchr(path, '/');
    const char *dot = strrchr(slash ? slash : path, '.');
    const char *ext = dot ? dot : "(none)";
    for (int i = 0; i < n_rows; i++) {
        if (strcmp(ROWS[i].ext, ext) == 0) {
            return &ROWS[i];
        }
    }
    if (n_rows == MAX_EXTENSIONS) {
        snprintf(ROWS[MAX_EXTENSIONS].ext, sizeof(ROWS[MAX_EXTENSIONS].ext), "(other)");
        return &ROWS[MAX_EXTENSIONS];
    }
    row *r = &ROWS[n_rows++];
    snprintf(r->ext, sizeof(r->ext), "%s", ext);
    return r;
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

/* Whether a site at bytes [start, end) lies within a callable read from the raw source. */
static int inside_raw_callable(const PDXEFileResult *engine, uint32_t start, uint32_t end) {
    for (int i = 0; i < engine->defs.count; i++) {
        PDXECallableOccurrence o = engine->defs.items[i].occurrence;
        if (pdxe_occurrence_valid(o) && o.origin == PDXE_SOURCE_ORIGIN_RAW && o.start_byte <= start &&
            end <= o.end_byte) {
            return 1;
        }
    }
    return 0;
}

/* The callable adopted from the macro-expanded text whose lines hold `line`, or NULL. */
static const PDXEDefinition *adopted_callable_on(const PDXEFileResult *engine, uint32_t line) {
    for (int i = 0; i < engine->defs.count; i++) {
        const PDXEDefinition *d = &engine->defs.items[i];
        if (pdxe_occurrence_valid(d->occurrence) &&
            d->occurrence.origin == PDXE_SOURCE_ORIGIN_PREPROCESSED && d->start_line <= line &&
            line <= d->end_line) {
            return d;
        }
    }
    return NULL;
}

static void print_occurrences(const char *path, const pdxe_file_result *r) {
    const PDXEFileResult *engine = pdxe_result_engine(r);
    for (int i = 0; i < engine->defs.count; i++) {
        PDXECallableOccurrence o = engine->defs.items[i].occurrence;
        if (pdxe_occurrence_valid(o) && o.origin == PDXE_SOURCE_ORIGIN_RAW) {
            printf("occurrence\t%s\t%u\t%u\n", path, o.start_byte, o.end_byte);
        }
    }
}

static void census(const char *path, const pdxe_file_result *r, int list) {
    const PDXEFileResult *engine = pdxe_result_engine(r);
    row *out = row_for(path);
    for (uint32_t i = 0; i < r->n_calls; i++) {
        const pdxe_call *c = &r->calls[i];
        if (c->span.end_byte <= c->span.start_byte || c->n_ast_path == 0) {
            continue; /* no position, or no path: never a site */
        }
        out->sites++;
        int owned = c->caller_index != PDXE_NO_PARENT && c->caller_index < r->n_defs &&
                    strcmp(r->defs[c->caller_index].kind, "module") != 0;
        int inside = inside_raw_callable(engine, c->span.start_byte, c->span.end_byte);
        if (inside) {
            out->inside++;
            if (!owned) {
                out->orphan++;
                if (list) {
                    printf("orphan\t%s:%u\t%s\n", path, c->span.start_line, c->callee_text);
                }
            }
        } else if (owned) {
            out->owned_outside++;
            if (list) {
                printf("owned_outside\t%s:%u\t%s\t%s\n", path, c->span.start_line, c->callee_text,
                       r->defs[c->caller_index].qualified_name);
            }
        } else {
            out->file++;
            const PDXEDefinition *d = adopted_callable_on(engine, c->span.start_line);
            if (d) {
                out->adopted_lines++;
                if (list) {
                    printf("adopted_lines\t%s:%u\t%s\t%s\n", path, c->span.start_line,
                           c->callee_text, d->qualified_name ? d->qualified_name : "");
                }
            }
        }
    }
}

static void print_row(const row *r, const char *name) {
    printf("%s\tsites=%lu\tinside=%lu\torphan=%lu\towned_outside=%lu\tfile=%lu\tadopted_lines=%lu\n",
           name, r->sites, r->inside, r->orphan, r->owned_outside, r->file, r->adopted_lines);
}

int main(int argc, char **argv) {
    int list = argc == 3 && strcmp(argv[1], "--list") == 0;
    int occurrences = argc == 3 && strcmp(argv[1], "--occurrences") == 0;
    if (argc != 2 + list + occurrences) {
        fprintf(stderr, "usage: owner_census [--list | --occurrences] <root> < "
                        "\"<language>\\t<path>\" lines\n");
        return 2;
    }
    const char *root = argv[argc - 1];
    pdxe_ctx *ctx = NULL;
    if (pdxe_init(&ctx) != PDXE_OK) {
        return 1;
    }
    char line[LINE_MAX_BYTES];
    while (fgets(line, sizeof(line), stdin)) {
        line[strcspn(line, "\r\n")] = '\0';
        char *tab = strchr(line, '\t');
        if (!tab) {
            continue;
        }
        *tab = '\0';
        const char *lang = line, *rel = tab + 1;
        char path[LINE_MAX_BYTES * 2];
        snprintf(path, sizeof(path), "%s/%s", root, rel);
        size_t len = 0;
        unsigned char *bytes = read_all(path, &len);
        pdxe_file_result *r = NULL;
        int id = pdxe_language_id(lang);
        if (!bytes || id < 0 || pdxe_extract_file(ctx, id, rel, bytes, len, &r) != PDXE_OK) {
            fprintf(stderr, "cannot extract %s as %s\n", rel, lang);
            return 1;
        }
        if (occurrences) {
            print_occurrences(rel, r);
        } else {
            census(rel, r, list);
        }
        pdxe_result_free(ctx, r);
        free(bytes);
    }
    if (occurrences) {
        pdxe_shutdown(ctx);
        return 0;
    }
    row all = {.ext = "all"};
    for (int i = 0; i <= MAX_EXTENSIONS; i++) {
        const row *r = &ROWS[i];
        if (i >= n_rows && (i < MAX_EXTENSIONS || r->sites == 0)) {
            continue;
        }
        print_row(r, r->ext);
        all.sites += r->sites;
        all.inside += r->inside;
        all.orphan += r->orphan;
        all.owned_outside += r->owned_outside;
        all.file += r->file;
        all.adopted_lines += r->adopted_lines;
    }
    print_row(&all, "all");
    pdxe_shutdown(ctx);
    return 0;
}
