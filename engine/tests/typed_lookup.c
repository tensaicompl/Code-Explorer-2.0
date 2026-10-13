/*
 * Typed resolution and the interface's translation look up by index (docs/plan/ISSUES.md,
 * issue 92; decision 62; engine/patches/0025-indexed-typed-resolution.patch).
 *
 *     typed_lookup <resolve fixture root>
 *
 * The per-file resolvers' registries, their language helpers and the interface's
 * translation found a definition, a type, an import or a site by scanning every one,
 * per question, which made a file's typed resolution quadratic in what it holds: 16,000
 * Python classes took 62 s, a 2 MiB nest of JavaScript calls 441 s. Every such lookup is
 * now read from an index keyed exactly as the scan matched, whose chains keep the scan's
 * order, so the answers are the scan's.
 *
 *   - The answers are the scan's: every typed-resolution fixture, the duplicate, overload,
 *     shadowing, alias and ambiguity fixtures below, generated code and the issue's own
 *     shapes extract to the same facts and resolve to the same answers, run health
 *     included, with the indexes and with the scans (the test switch
 *     PDX_ENGINE_TEST_LINEAR_LOOKUP_REFERENCE).
 *   - The work is linear: flat files of 1,000 to 16,000 classes in nine languages, 64 to
 *     512 groups of nested JavaScript calls, the interface's translation of 16,000
 *     definitions and generated code examine a number of entries, compare a number of
 *     names and probe a number of slots that doubles when the file doubles, and the
 *     indexes never fall back to examining a registry entry by entry. The scans, for
 *     comparison, examine a number quadratic in the file.
 *   - An index that cannot get its memory degrades, never scans: with each family of
 *     indexes refused (PDX_ENGINE_TEST_REFUSE_LOOKUP_INDEXES), extraction and resolution
 *     complete or fail as the interface says, the refusal is lost work, and the work
 *     stays linear.
 *   - Duplicate qualified names keep the first definition, and names are compared
 *     without the project's prefix, in the interface's translation as before.
 *
 * Counts, never times: the counters are the engine's own (test builds only).
 */

#define PDXE_LOOKUP_COUNTS_TEST_API 1

#include <dirent.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

#include "fixture_metadata.h"
#include "lookup_counts.h"
#include "pdxe.h"
#include "pdxe_core.h"
#include "shim_internal.h"
#include "text_view.h"

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

static const char *COUNTER_NAMES[PDXE_LC_COUNT] = {
    "registry entries", "registry probes", "qualified-name comparisons",
    "name comparisons", "type self-lookups", "function self-lookups",
    "Python module-class scans", "TypeScript symbol candidates", "overload candidates",
    "interface definition entries", "interface comparisons", "TypeScript usages",
    "answer rows", "import items"};

/* --- text ------------------------------------------------------------------- */

typedef struct {
    char *text;
    size_t len;
    size_t cap;
} buf_t;

static void put_n(buf_t *b, const char *s, size_t n) {
    if (b->len + n + 1 > b->cap) {
        size_t cap = b->cap ? b->cap * 2 : 4096;
        while (cap < b->len + n + 1) {
            cap *= 2;
        }
        b->text = (char *)realloc(b->text, cap);
        if (!b->text) {
            abort();
        }
        b->cap = cap;
    }
    memcpy(b->text + b->len, s, n);
    b->len += n;
    b->text[b->len] = '\0';
}

static void put(buf_t *b, const char *s) {
    put_n(b, s, strlen(s));
}

static void putf(buf_t *b, const char *fmt, ...) {
    char line[4096];
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(line, sizeof line, fmt, ap);
    va_end(ap);
    if (n < 0 || (size_t)n >= sizeof line) {
        abort(); /* a generator line is short */
    }
    put_n(b, line, (size_t)n);
}

static void put_text(buf_t *b, const char *s) {
    put(b, s ? s : "<null>");
    put(b, "\x1f");
}

static void put_span(buf_t *b, pdxe_span s) {
    putf(b, "[%u,%u %u:%u-%u:%u]", s.start_byte, s.end_byte, s.start_line, s.start_col,
         s.end_line, s.end_col);
}

static void put_list(buf_t *b, const char **items, uint32_t n) {
    putf(b, "(%u:", n);
    for (uint32_t i = 0; i < n; i++) {
        put_text(b, items[i]);
    }
    put(b, ")");
}

/* --- projects --------------------------------------------------------------- */

typedef struct {
    char *path;
    const char *language;
    char *source;
    size_t len;
} source_file;

typedef struct {
    const char *name;
    source_file *files;
    size_t count;
    size_t cap;
    char *metadata_dir; /* a fixture directory whose pdx-metadata.txt applies, or NULL */
} project_t;

static void add_file(project_t *p, const char *path, const char *language, buf_t *text) {
    if (p->count == p->cap) {
        p->cap = p->cap ? p->cap * 2 : 8;
        p->files = (source_file *)realloc(p->files, p->cap * sizeof(*p->files));
        if (!p->files) {
            abort();
        }
    }
    source_file *f = &p->files[p->count++];
    f->path = strdup(path);
    f->language = language;
    f->source = text->text ? text->text : strdup("");
    f->len = text->len;
    memset(text, 0, sizeof *text);
}

static void add_text(project_t *p, const char *path, const char *language, const char *text) {
    buf_t b = {0};
    put(&b, text);
    add_file(p, path, language, &b);
}

static void project_free(project_t *p) {
    for (size_t i = 0; i < p->count; i++) {
        free(p->files[i].path);
        free(p->files[i].source);
    }
    free(p->files);
    free(p->metadata_dir);
    memset(p, 0, sizeof *p);
}

/* Every fact of an extraction, in order: two extractions are the same exactly when
 * these are. Text is spelled out from the source, so a range and its text compare
 * equal. */
static void describe_file(buf_t *b, const pdxe_file_result *r, const source_file *f) {
    const uint8_t *src = (const uint8_t *)f->source;
    size_t len = f->len;
    putf(b, "file %s status %d truncated %u lost %u ns ", f->path, r->status, r->truncated,
         r->extraction_lost);
    put_text(b, r->declared_namespace);
    put(b, "\n");
    for (uint32_t i = 0; i < r->n_defs; i++) {
        const pdxe_definition *d = &r->defs[i];
        put(b, "def ");
        put_text(b, d->name);
        put_text(b, tv_qn(r, i));
        put_text(b, d->kind);
        put_text(b, d->engine_kind);
        put_text(b, tv_text(&d->signature, src, len));
        put_text(b, d->doc);
        put_span(b, d->span);
        put_span(b, d->body_span);
        putf(b, " %u %u %u %u %u %u %u ", d->parent_index, d->visibility, d->is_test,
             d->is_entry_point, d->cyclomatic, d->cognitive, d->loop_depth);
        put_list(b, d->base_classes, d->n_base_classes);
        put_list(b, d->decorators, d->n_decorators);
        put_list(b, d->signature_param_types, d->n_signature_param_types);
        put(b, "\n");
    }
    for (uint32_t i = 0; i < r->n_calls; i++) {
        const pdxe_call *c = &r->calls[i];
        put(b, "call ");
        put_text(b, tv_text(&c->callee_text, src, len));
        put_text(b, c->receiver_text);
        putf(b, " %u %u %u %u ", c->caller_index, c->is_reference, c->typed_only, c->lexical);
        put_span(b, c->span);
        put_list(b, c->ast_path, c->n_ast_path);
        for (uint32_t k = 0; k < c->n_args; k++) {
            put(b, " arg ");
            put_text(b, tv_text(&c->args[k].expr, src, len));
            put_text(b, c->args[k].value);
            put_text(b, c->args[k].keyword);
            putf(b, "%u", c->args[k].index);
        }
        put(b, "\n");
    }
    for (uint32_t i = 0; i < r->n_imports; i++) {
        put(b, "import ");
        put_text(b, r->imports[i].module_text);
        put_text(b, r->imports[i].imported_name);
        put_text(b, r->imports[i].alias);
        put_span(b, r->imports[i].span);
        put(b, "\n");
    }
    for (uint32_t i = 0; i < r->n_usages; i++) {
        put(b, "usage ");
        put_text(b, tv_text(&r->usages[i].name, src, len));
        putf(b, " %u %u ", r->usages[i].scope_index, r->usages[i].lexical);
        put_span(b, r->usages[i].span);
        put(b, "\n");
    }
    for (uint32_t i = 0; i < r->n_types; i++) {
        put(b, "type ");
        put_text(b, r->types[i].type_text);
        putf(b, " %u ", r->types[i].scope_index);
        put_span(b, r->types[i].span);
        put(b, "\n");
    }
    for (uint32_t i = 0; i < r->n_rws; i++) {
        put(b, "rw ");
        put_text(b, r->rws[i].field_text);
        putf(b, " %u %u ", r->rws[i].scope_index, r->rws[i].is_write);
        put_span(b, r->rws[i].span);
        put(b, "\n");
    }
    for (uint32_t i = 0; i < r->n_throws; i++) {
        put(b, "throw ");
        put_text(b, r->throws[i].exception_text);
        putf(b, " %u ", r->throws[i].scope_index);
        put_span(b, r->throws[i].span);
        put(b, "\n");
    }
    for (uint32_t i = 0; i < r->n_impl_traits; i++) {
        put(b, "impl ");
        put_text(b, r->impl_traits[i].trait_name);
        put_text(b, r->impl_traits[i].struct_name);
        put_text(b, r->impl_traits[i].struct_qn);
        put(b, "\n");
    }
    /* What the file's own resolver decided while it was extracted, where its registry
     * is never finalized: every row, in order. */
    const PDXEFileResult *engine = pdxe_result_engine(r);
    for (int i = 0; engine && i < engine->resolved_calls.count; i++) {
        const PDXEResolvedCall *rc = &engine->resolved_calls.items[i];
        put(b, "resolved ");
        put_text(b, rc->caller_qn);
        put_text(b, rc->callee_qn);
        put_text(b, rc->strategy);
        put_text(b, rc->reason);
        putf(b, " %.6f %d %u %u %d\n", (double)rc->confidence, (int)rc->kind, rc->site_start_byte,
             rc->site_end_byte, (int)rc->source_origin);
    }
}

/* One answer, every field: what it targets, how, how sure, its candidates and its site. */
static void describe_answer(buf_t *b, const pdxe_resolution *x, const project_t *p) {
    const source_file *f = NULL;
    for (size_t i = 0; i < p->count && x->rel_path; i++) {
        if (strcmp(p->files[i].path, x->rel_path) == 0) {
            f = &p->files[i];
        }
    }
    put(b, "answer ");
    put_text(b, x->rel_path);
    putf(b, "%u ", x->call_index);
    put_text(b, x->target_qualified_name);
    put_text(b, x->target_rel_path);
    putf(b, "%.17g ", x->score);
    put_text(b, x->strategy);
    putf(b, "%u ", x->candidates);
    put_text(b, x->engine_strategy);
    put_text(b, f ? tv_text(&x->site.callee_text, (const uint8_t *)f->source, f->len) : "?");
    put_text(b, x->site.receiver_text);
    putf(b, " %u %u %u %u ", x->site.caller_index, x->site.is_reference, x->site.typed_only,
         x->site.lexical);
    put_span(b, x->site.span);
    put(b, "\n");
}

/* --- runs ------------------------------------------------------------------- */

typedef enum { INDEXED, REFERENCE, REFUSED } mode_t_;

typedef struct {
    buf_t facts;                 /* every file's facts, then every answer and the health */
    uint64_t work[PDXE_LC_COUNT]; /* the counts, extraction and resolution together */
    int extract_rc;              /* the first extraction that failed, else PDXE_OK */
    int run_rc;
    pdxe_run_health health;
    uint32_t lost;    /* work every extraction lost, summed */
    uint32_t answers;
    uint32_t defs;
} outcome_t;

static pdxe_ctx *ctx = NULL;

static void set_mode(mode_t_ mode, const char *family) {
    unsetenv("PDX_ENGINE_TEST_LINEAR_LOOKUP_REFERENCE");
    unsetenv("PDX_ENGINE_TEST_REFUSE_LOOKUP_INDEXES");
    if (mode == REFERENCE) {
        setenv("PDX_ENGINE_TEST_LINEAR_LOOKUP_REFERENCE", "1", 1);
    } else if (mode == REFUSED) {
        setenv("PDX_ENGINE_TEST_REFUSE_LOOKUP_INDEXES", family, 1);
    }
}

/* Extracts every file of `p` and, when `resolve`, resolves them as one project: the
 * facts, the answers, the health and the work, in one mode. */
static outcome_t run(const project_t *p, mode_t_ mode, const char *family, int resolve) {
    outcome_t out;
    memset(&out, 0, sizeof out);
    set_mode(mode, family);
    pdxe_lookup_counts_reset();
    pdxe_file_result **results =
        (pdxe_file_result **)calloc(p->count ? p->count : 1, sizeof(*results));
    if (!results) {
        abort();
    }
    for (size_t i = 0; i < p->count; i++) {
        const source_file *f = &p->files[i];
        int rc = pdxe_extract_file(ctx, pdxe_language_id(f->language), f->path,
                                   (const uint8_t *)f->source, f->len, &results[i]);
        if (rc != PDXE_OK || !results[i]) {
            if (out.extract_rc == PDXE_OK) {
                out.extract_rc = rc != PDXE_OK ? rc : -1;
            }
            putf(&out.facts, "file %s failed %d\n", f->path, rc);
            results[i] = NULL;
            continue;
        }
        describe_file(&out.facts, results[i], f);
        out.lost += results[i]->extraction_lost;
        out.defs += results[i]->n_defs;
    }
    if (resolve && out.extract_rc == PDXE_OK) {
        pdxe_project *proj = NULL;
        out.run_rc = pdxe_resolve_project_begin(ctx, &proj);
        for (size_t i = 0; out.run_rc == PDXE_OK && i < p->count; i++) {
            const source_file *f = &p->files[i];
            out.run_rc = pdxe_resolve_project_add_file(proj, pdxe_language_id(f->language),
                                                       f->path, (const uint8_t *)f->source,
                                                       f->len, results[i]);
        }
        metadata m;
        memset(&m, 0, sizeof(m));
        if (out.run_rc == PDXE_OK && p->metadata_dir) {
            load_metadata(p->metadata_dir, &m);
            if (m.present) {
                m.crate.dependencies = m.dependencies;
                m.crate.member_paths = m.members;
                pdxe_resolution_metadata md = {m.packages, m.n_packages, m.scopes, m.n_scopes,
                                               m.has_crate ? &m.crate : NULL};
                out.run_rc = pdxe_resolve_project_set_metadata(proj, &md);
            }
        }
        if (out.run_rc == PDXE_OK) {
            out.run_rc = pdxe_resolve_project_run(proj);
        }
        if (out.run_rc == PDXE_OK) {
            pdxe_resolve_project_health(proj, &out.health);
            const pdxe_resolution *res = NULL;
            uint32_t n = 0;
            pdxe_resolve_project_results(proj, &res, &n);
            out.answers = n;
            for (uint32_t i = 0; i < n; i++) {
                describe_answer(&out.facts, &res[i], p);
            }
            putf(&out.facts, "health %d %u %u %u %u %u %u %u %u\n", out.health.status,
                 out.health.files, out.health.files_resolved, out.health.files_untyped,
                 out.health.files_empty, out.health.files_over_budget,
                 out.health.files_source_unavailable, out.health.files_not_reached,
                 out.health.pass_failures);
        } else {
            putf(&out.facts, "run failed %d\n", out.run_rc);
        }
        if (proj) {
            pdxe_resolve_project_end(proj);
        }
    }
    pdxe_lookup_counts_read(out.work);
    set_mode(INDEXED, NULL);
    for (size_t i = 0; i < p->count; i++) {
        if (results[i]) {
            pdxe_result_free(ctx, results[i]);
        }
    }
    free(results);
    return out;
}

static void release(outcome_t *o) {
    free(o->facts.text);
    memset(o, 0, sizeof *o);
}

/* The first line where two descriptions differ, for the failure message. */
static void first_difference(const char *a, const char *b, char *out, size_t cap) {
    const char *la = a ? a : "", *lb = b ? b : "";
    while (*la && *lb) {
        const char *ea = strchr(la, '\n'), *eb = strchr(lb, '\n');
        size_t na = ea ? (size_t)(ea - la) : strlen(la), nb = eb ? (size_t)(eb - lb) : strlen(lb);
        if (na != nb || memcmp(la, lb, na) != 0) {
            snprintf(out, cap, "indexed: %.*s | scans: %.*s", (int)(na > 300 ? 300 : na), la,
                     (int)(nb > 300 ? 300 : nb), lb);
            return;
        }
        la += na + (ea ? 1 : 0);
        lb += nb + (eb ? 1 : 0);
    }
    snprintf(out, cap, "one description is a prefix of the other");
}

/* The indexes and the scans give the same facts and the same answers, and the indexes
 * never examine a registry entry by entry. */
static void same_answers(const project_t *p, int resolve) {
    outcome_t indexed = run(p, INDEXED, NULL, resolve);
    outcome_t scans = run(p, REFERENCE, NULL, resolve);
    const char *a = indexed.facts.text ? indexed.facts.text : "";
    const char *b = scans.facts.text ? scans.facts.text : "";
    if (strcmp(a, b) != 0) {
        char diff[800];
        first_difference(a, b, diff, sizeof diff);
        CHECK(0, "%s: the indexes and the scans disagree: %s", p->name, diff);
    }
    CHECK(indexed.work[PDXE_LC_REGISTRY_ENTRIES] == 0,
          "%s: the indexes examined %llu registry entries one by one", p->name,
          (unsigned long long)indexed.work[PDXE_LC_REGISTRY_ENTRIES]);
    CHECK(indexed.extract_rc == PDXE_OK && indexed.run_rc == PDXE_OK,
          "%s: extraction %d, resolution %d", p->name, indexed.extract_rc, indexed.run_rc);
    release(&indexed);
    release(&scans);
}

/* --- the resolve fixtures ---------------------------------------------------- */

static const struct {
    const char *ext;
    const char *lang;
} EXTENSIONS[] = {
    {".py", "python"},      {".ts", "typescript"}, {".tsx", "tsx"},   {".js", "javascript"},
    {".jsx", "javascript"}, {".go", "go"},         {".java", "java"}, {".cs", "csharp"},
    {".c", "c"},            {".h", "c"},           {".cpp", "cpp"},   {".hpp", "cpp"},
    {".cc", "cpp"},         {".rs", "rust"},       {".kt", "kotlin"}, {".php", "php"},
    {".pl", "perl"},        {".pm", "perl"},
};

static const char *language_of(const char *path) {
    const char *dot = strrchr(path, '.');
    if (!dot) {
        return NULL;
    }
    for (size_t i = 0; i < sizeof(EXTENSIONS) / sizeof(EXTENSIONS[0]); i++) {
        if (strcmp(dot, EXTENSIONS[i].ext) == 0) {
            return EXTENSIONS[i].lang;
        }
    }
    return NULL;
}

static int cmp_str(const void *a, const void *b) {
    return strcmp(*(char *const *)a, *(char *const *)b);
}

static void walk(const char *root, const char *rel, strings *out) {
    char path[4096];
    snprintf(path, sizeof(path), "%s%s%s", root, rel[0] ? "/" : "", rel);
    DIR *d = opendir(path);
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
            walk(root, child, out);
        } else if (language_of(child)) {
            push(out, strdup(child));
        }
    }
    closedir(d);
}

static int read_all(const char *path, buf_t *out) {
    FILE *f = fopen(path, "rb");
    if (!f) {
        return 0;
    }
    char chunk[65536];
    size_t n;
    while ((n = fread(chunk, 1, sizeof chunk, f)) > 0) {
        put_n(out, chunk, n);
    }
    fclose(f);
    return 1;
}

/* Every typed-resolution fixture: its files in path order, with its metadata. */
static void resolve_fixtures(const char *root) {
    strings dirs = {0};
    DIR *d = opendir(root);
    CHECK(d != NULL, "cannot open the resolve fixtures at %s", root);
    if (!d) {
        return;
    }
    struct dirent *e;
    while ((e = readdir(d)) != NULL) {
        if (e->d_name[0] != '.') {
            push(&dirs, strdup(e->d_name));
        }
    }
    closedir(d);
    if (dirs.count > 0) {
        qsort(dirs.items, dirs.count, sizeof(char *), cmp_str);
    }
    int seen = 0;
    for (size_t i = 0; i < dirs.count; i++) {
        char dir[4096];
        snprintf(dir, sizeof dir, "%s/%s", root, dirs.items[i]);
        strings files = {0};
        walk(dir, "", &files);
        if (files.count > 0) {
            qsort(files.items, files.count, sizeof(char *), cmp_str);
        }
        project_t p = {0};
        p.name = dirs.items[i];
        p.metadata_dir = strdup(dir);
        for (size_t k = 0; k < files.count; k++) {
            char path[8200];
            snprintf(path, sizeof path, "%s/%s", dir, files.items[k]);
            buf_t text = {0};
            CHECK(read_all(path, &text), "cannot read %s", path);
            add_file(&p, files.items[k], language_of(files.items[k]), &text);
            free(files.items[k]);
        }
        free(files.items);
        if (p.count > 0) {
            same_answers(&p, 1);
            seen++;
        }
        project_free(&p);
        free(dirs.items[i]);
    }
    free(dirs.items);
    CHECK(seen >= 20, "only %d resolve fixtures found under %s", seen, root);
}

/* --- the issue's cases ------------------------------------------------------- */

/*
 * Duplicate short names, duplicate qualified names, overloads, one class name in two
 * modules, a nested class beside a root class of the same name, imported aliases,
 * local against external types, one function in several files, member lookups on a
 * receiver and ambiguous TypeScript symbols: in every language whose registry, helpers
 * or translation look them up.
 */
static void issue_fixtures(void) {
    project_t py = {.name = "python duplicates, aliases, nesting, wildcards"};
    add_text(&py, "pkg/a.py", "python",
             "class Shape:\n    def area(self):\n        return 1\n"
             "class Shape:\n    def area(self):\n        return 2\n"
             "    def perimeter(self):\n        return 3\n"
             "def helper():\n    a = 1\n    return a\n"
             "def helper():\n    b = 2\n    return b\n"
             "class Outer:\n    class Inner:\n        def go(self):\n            return helper()\n"
             "class Inner:\n    def go(self):\n        return 0\n"
             "def use():\n    s = Shape()\n    s.area()\n    s.perimeter()\n"
             "    Outer.Inner().go()\n    Inner().go()\n    helper()\n");
    add_text(&py, "pkg/b.py", "python",
             "from pkg.a import Shape as S, helper as h\n"
             "from pkg.a import *\n"
             "import pkg.a as A\n"
             "class Shape:\n    def area(self):\n        return 9\n"
             "def helper():\n    return 3\n"
             "def run():\n    S().area()\n    h()\n    Shape().area()\n    helper()\n"
             "    x = S()\n    x.perimeter()\n    A.helper()\n    A.Shape().area()\n");
    add_text(&py, "pkg/c.py", "python",
             "from pkg.a import *\nfrom pkg.b import *\n"
             "def helper():\n    return 4\n"
             "def go():\n    helper()\n    Shape().area()\n    run()\n");
    same_answers(&py, 1);
    project_free(&py);

    project_t java = {.name = "java overloads, same class in two packages, nesting"};
    add_text(&java, "src/p/Shapes.java", "java",
             "package p;\n"
             "public class Shapes {\n"
             "  public int area(int x) { return x; }\n"
             "  public int area(String s) { return 0; }\n"
             "  public int area() { return 1; }\n"
             "  int width; int height; String label;\n"
             "  static class Inner { void go() { } }\n"
             "  void use() { area(1); area(\"a\"); area(); new Inner().go(); Util.twice(2); }\n"
             "}\n"
             "class Shapes2 extends Shapes { int area(int x) { return 2; } void run() { area(3); } }\n");
    add_text(&java, "src/q/Shapes.java", "java",
             "package q;\n"
             "public class Shapes { public int area(int x) { return x; } }\n");
    add_text(&java, "src/p/Util.java", "java",
             "package p;\n"
             "import static p.Util.twice;\n"
             "import q.Shapes;\n"
             "public class Util {\n"
             "  static int twice(int x) { return x; }\n"
             "  static int twice(long x) { return 0; }\n"
             "  void run() { twice(1); new Shapes().area(3); new p.Shapes().area(\"s\"); }\n"
             "}\n");
    same_answers(&java, 1);
    project_free(&java);

    project_t kt = {.name = "kotlin overloads, sole definers, local functions"};
    add_text(&kt, "src/a/Main.kt", "kotlin",
             "package a\n"
             "fun util(x: Int) = x\n"
             "fun util(x: String) = 0\n"
             "class Box {\n  fun open() = 1\n  fun open(n: Int) = n\n}\n"
             "fun main() {\n  util(1)\n  util(\"s\")\n  Box().open()\n  Box().open(2)\n"
             "  fun local() = 3\n  local()\n  only()\n}\n");
    add_text(&kt, "src/b/Other.kt", "kotlin",
             "package b\nfun util(x: Int) = 2\nfun only() = 5\nfun run() {\n  util(1)\n}\n");
    same_answers(&kt, 1);
    project_free(&kt);

    project_t ts = {.name = "typescript ambiguous symbols, aliases, same names in two modules"};
    add_text(&ts, "src/a.ts", "typescript",
             "export function f(x: number): number { return x; }\n"
             "export class K { m(x: number) { return x; } m2() { return this.m(1); } }\n");
    add_text(&ts, "src/b.ts", "typescript",
             "export function f(x: string): string { return x; }\n"
             "export class K { m(x: string) { return x; } }\n");
    add_text(&ts, "src/c.ts", "typescript",
             "import { f } from './a';\n"
             "import { f as g, K } from './b';\n"
             "import * as A from './a';\n"
             "function f2() { return f(1) + g('x'); }\n"
             "function use(k: K) { k.m('s'); new A.K().m2(); A.f(2); }\n"
             "function dup() { return 1; }\n"
             "function dup() { return 2; }\n"
             "function caller() { dup(); f2(); const cb = () => 1; use(cb as any); }\n");
    same_answers(&ts, 1);
    project_free(&ts);

    project_t cpp = {.name = "c++ overloads, one class in two namespaces, operators, pointers"};
    add_text(&cpp, "src/a.cpp", "cpp",
             "namespace n1 { struct Shape { int area(int x); int area(double d); }; }\n"
             "namespace n2 { struct Shape { int area(int x); }; }\n"
             "namespace n3 { struct Logger { void log(); }; }\n"
             "struct V { int x; };\n"
             "V operator+(V a, V b) { return a; }\n"
             "int f(int x) { return x; }\n"
             "int f(double x) { return 0; }\n"
             "void g(int) {}\n"
             "void (*fp)(int) = g;\n"
             "void use() {\n"
             "  n1::Shape s; s.area(1); s.area(2.0);\n"
             "  n2::Shape t; t.area(3);\n"
             "  V a, b; V c = a + b;\n"
             "  f(1); f(2.0);\n"
             "  fp(1);\n"
             "  void (*local)(int) = g; local(2);\n"
             "  n3::Logger lg; lg.log();\n"
             "}\n");
    same_answers(&cpp, 1);
    project_free(&cpp);

    project_t cs = {.name = "c# overloads, extensions, one class in two namespaces"};
    add_text(&cs, "src/A.cs", "csharp",
             "namespace N1 { public class Shape { public int Area(int x) { return x; }"
             " public int Area(string s) { return 0; } } }\n"
             "namespace N2 { public class Shape { public int Area(int x) { return x; } } }\n"
             "namespace N1 { public static class Ext { public static int Twice(this Shape s)"
             " { return 2; } } }\n"
             "namespace N1 { public class User { void Use() { var s = new Shape(); s.Area(1);"
             " s.Area(\"a\"); s.Twice(); Helper(); } static void Helper() {} } }\n");
    same_answers(&cs, 1);
    project_free(&cs);

    project_t go = {.name = "go embedding, interfaces, aliases, one type in two packages"};
    add_text(&go, "src/a/a.go", "go",
             "package a\n"
             "type Shape struct{ inner *Shape }\n"
             "func (s *Shape) Area() int { return 1 }\n"
             "type Alias = Shape\n"
             "type I interface { Area() int }\n"
             "type Box struct { Shape }\n"
             "func Generic[T any](x T) T { return x }\n"
             "func Use(i I, b Box) { i.Area(); b.Area(); var s Shape; s.inner.Area(); Helper();"
             " Generic(1) }\n"
             "func Helper() {}\n");
    add_text(&go, "src/b/b.go", "go",
             "package b\n"
             "import \"a\"\n"
             "type Shape struct{}\n"
             "func (s Shape) Area() int { return 2 }\n"
             "func Run() { a.Helper(); var s Shape; s.Area() }\n");
    same_answers(&go, 1);
    project_free(&go);

    project_t rs = {.name = "rust impls, macros, one name in two modules"};
    add_text(&rs, "src/lib.rs", "rust",
             "mod a { pub struct S; impl S { pub fn new() -> S { S } pub fn go(&self) -> i32 { 1 } }"
             " pub fn helper() -> i32 { 1 } }\n"
             "mod b { pub struct S; impl S { pub fn go(&self) -> i32 { 2 } }"
             " pub fn helper() -> i32 { 2 } }\n"
             "macro_rules! m { () => { a::helper() }; ($x:expr) => { b::helper() } }\n"
             "use a::S;\n"
             "#[derive(Clone, Debug)]\n"
             "struct P { x: i32 }\n"
             "fn run() { let s = S::new(); s.go(); a::helper(); b::helper(); m!(); m!(1);"
             " println!(\"x\"); let p = P { x: 1 }; let q = p.clone(); }\n");
    same_answers(&rs, 1);
    project_free(&rs);

    project_t php = {.name = "php docblocks, one function in two namespaces"};
    add_text(&php, "src/a.php", "php",
             "<?php\nnamespace A;\n"
             "/** @return Shape */\nfunction make() { return new Shape(); }\n"
             "function helper() { return 1; }\n"
             "/** @property int $w */\nclass Shape {\n  /** @return int */\n"
             "  public function area() { return helper(); }\n}\n"
             "function use_it() { $s = make(); $s->area(); helper(); \\B\\helper(); }\n");
    add_text(&php, "src/b.php", "php",
             "<?php\nnamespace B;\nfunction helper() { return 2; }\n"
             "class Shape { public function area() { return 3; } }\n");
    same_answers(&php, 1);
    project_free(&php);

    project_t perl = {.name = "perl packages and @ISA"};
    add_text(&perl, "lib/A.pm", "perl",
             "package A;\nsub new { bless {}, shift }\nsub go { my $s = shift; $s->help(); }\n"
             "sub help { 1 }\npackage B;\nour @ISA = ('A');\n"
             "sub go { my $s = shift; $s->SUPER::go(); $s->help(); }\n1;\n");
    same_answers(&perl, 1);
    project_free(&perl);

    /* Two definitions of one name that lead to different types: which one a lookup
     * finds decides what the call on its result resolves to, so the order a duplicate
     * is found in shows. */
    project_t order = {.name = "duplicates found in order"};
    add_text(&order, "order/dup.py", "python",
             "class A:\n    def m(self):\n        return 1\n"
             "class B:\n    def m(self):\n        return 2\n"
             "def make() -> A:\n    return A()\n"
             "def make() -> B:\n    return B()\n"
             "class Twice:\n    def first(self):\n        return 1\n"
             "class Twice:\n    def second(self):\n        return 2\n"
             "def use():\n    make().m()\n    t = Twice()\n    t.first()\n    t.second()\n"
             "    b = B()\n    b.m()\n    A().m()\n");
    add_text(&order, "order/Dup.java", "java",
             "package order;\n"
             "class A { void m() {} }\nclass B { void m() {} }\n"
             "class P {\n  A f(Object o) { return null; }\n  B f(Object p) { return null; }\n"
             "  void use() { f(null).m(); new B().m(); new A().m(); }\n}\n");
    add_text(&order, "order/dup.ts", "typescript",
             "class A { m() { return 1; } }\nclass B { m() { return 2; } }\n"
             "function make(): A { return new A(); }\nfunction make(): B { return new B(); }\n"
             "function use() { make().m(); new B().m(); new A().m(); }\n");
    add_text(&order, "order/dup.kt", "kotlin",
             "package order\nclass A { fun m() = 1 }\nclass B { fun m() = 2 }\n"
             "fun make(): A = A()\nfun make(): B = B()\nfun use() { make().m(); B().m(); A().m() }\n");
    same_answers(&order, 1);
    project_free(&order);

    project_t c = {.name = "c function pointers and shadowing"};
    add_text(&c, "src/fp.c", "c",
             "void a(void) {}\nvoid b(void) {}\n"
             "void (*fp)(void) = a;\n"
             "void use(void) { fp(); { void (*fp)(void) = b; fp(); } fp(); int x; (void)x; }\n");
    same_answers(&c, 1);
    project_free(&c);
}

/* --- shapes ----------------------------------------------------------------- */

/* A file of n one-method classes, each method calling a function the file does not
 * define: the issue's flat shape, in each language the issue measured. */
static void flat(buf_t *b, const char *language, int n) {
    if (strcmp(language, "java") == 0 || strcmp(language, "kotlin") == 0) {
        put(b, strcmp(language, "java") == 0 ? "package p;\n" : "package p\n");
    } else if (strcmp(language, "go") == 0) {
        put(b, "package p\n");
    } else if (strcmp(language, "php") == 0) {
        put(b, "<?php\n");
    } else if (strcmp(language, "csharp") == 0) {
        put(b, "namespace P {\n");
    }
    for (int i = 0; i < n; i++) {
        if (strcmp(language, "python") == 0) {
            putf(b, "class C%d:\n    def m(self):\n        f()\n", i);
        } else if (strcmp(language, "java") == 0) {
            putf(b, "class C%d { void m() { f(); } }\n", i);
        } else if (strcmp(language, "typescript") == 0) {
            putf(b, "class C%d { m(): void { f(); } }\n", i);
        } else if (strcmp(language, "cpp") == 0) {
            putf(b, "class C%d { void m() { f(); } };\n", i);
        } else if (strcmp(language, "csharp") == 0) {
            putf(b, "class C%d { void M() { F(); } }\n", i);
        } else if (strcmp(language, "go") == 0) {
            putf(b, "type T%d struct{}\nfunc (t T%d) M() { f() }\n", i, i);
        } else if (strcmp(language, "rust") == 0) {
            putf(b, "struct T%d;\nimpl T%d { fn m(&self) { f(); } }\n", i, i);
        } else if (strcmp(language, "php") == 0) {
            putf(b, "class C%d { function m() { f(); } }\n", i);
        } else if (strcmp(language, "kotlin") == 0) {
            putf(b, "class C%d { fun m() { f() } }\n", i);
        }
    }
    if (strcmp(language, "csharp") == 0) {
        put(b, "}\n");
    }
}

/* n groups of JavaScript calls nested `depth` deep: free functions and methods. */
static void nested_groups(buf_t *b, int n, int depth) {
    put(b, "class O { m(a, b) { return a; } }\nconst o = new O();\n");
    for (int i = 0; i < n; i++) {
        putf(b, "function g%d(a) { return ", i);
        for (int d = 0; d < depth; d++) {
            put(b, i % 2 ? "o.m(a, " : "f(a, ");
        }
        put(b, "a");
        for (int d = 0; d < depth; d++) {
            put(b, ")");
        }
        put(b, "; }\n");
    }
}

/* About n definitions for the interface's translation: classes holding a nested class
 * and methods with a local function, every module function defined twice (one
 * qualified name, two definitions), and calls between them. */
static void translation(buf_t *b, int n) {
    for (int i = 0; i < n / 8; i++) {
        putf(b,
             "class C%d:\n"
             "    class N%d:\n"
             "        def inner(self):\n"
             "            return helper%d()\n"
             "    def m(self):\n"
             "        def local():\n"
             "            return self.inner2()\n"
             "        return local()\n"
             "    def inner2(self):\n"
             "        return C%d.N%d().inner()\n"
             "def helper%d():\n"
             "    return 1\n"
             "def helper%d():\n"
             "    return C%d().m()\n",
             i, i, i, i, i, i, i, i);
    }
}

/* Generated code, as protocol-buffer modules, models and API clients are: thousands of
 * small definitions with the same few member names. */
static void generated(project_t *p, int n) {
    buf_t py = {0}, java = {0}, ts = {0};
    put(&py, "from proto import message as _message\n_REGISTRY = {}\n");
    put(&java, "package gen;\npublic final class Protos {\n");
    put(&ts, "import { client } from './client';\n");
    for (int i = 0; i < n; i++) {
        putf(&py,
             "class Msg%d(_message.Message):\n"
             "    __slots__ = ()\n"
             "    def ByteSize(self):\n        return _size(self)\n"
             "    def field_%d(self):\n        return self._fields.get(%d)\n"
             "    def SerializeToString(self):\n        return self.ByteSize()\n"
             "def register_%d():\n    _REGISTRY[%d] = Msg%d().SerializeToString()\n",
             i, i, i, i, i, i);
        putf(&java,
             "  public static final class M%d {\n"
             "    private int f;\n"
             "    public int getF() { return f; }\n"
             "    public static Builder newBuilder() { return new Builder(); }\n"
             "    public static final class Builder {\n"
             "      private int f;\n"
             "      public Builder setF(int v) { f = v; return this; }\n"
             "      public M%d build() { M%d m = new M%d(); m.f = f; return m; }\n"
             "    }\n"
             "    static M%d make() { return newBuilder().setF(%d).build(); }\n"
             "  }\n",
             i, i, i, i, i, i);
        putf(&ts,
             "export interface Model%d { id: number; name: string; }\n"
             "export class Api%d {\n"
             "  get(id: number): Model%d { return client.get<Model%d>('/%d/' + id); }\n"
             "  list(): Model%d[] { return [this.get(1), this.get(2)]; }\n"
             "}\n",
             i, i, i, i, i, i);
    }
    put(&java, "}\n");
    add_file(p, "gen/messages_pb2.py", "python", &py);
    add_file(p, "gen/Protos.java", "java", &java);
    add_file(p, "gen/api.ts", "typescript", &ts);
}

/* --- linear work ------------------------------------------------------------- */

typedef void (*shape_fn)(project_t *p, int n, const void *arg);

static void flat_shape(project_t *p, int n, const void *arg) {
    const char *language = (const char *)arg;
    static const struct {
        const char *language;
        const char *path;
    } PATHS[] = {{"python", "flat.py"}, {"java", "p/Flat.java"}, {"typescript", "flat.ts"},
                 {"cpp", "flat.cpp"},   {"csharp", "Flat.cs"},  {"go", "p/flat.go"},
                 {"rust", "flat.rs"},   {"php", "flat.php"},    {"kotlin", "p/Flat.kt"}};
    buf_t b = {0};
    flat(&b, language, n);
    for (size_t i = 0; i < sizeof PATHS / sizeof PATHS[0]; i++) {
        if (strcmp(PATHS[i].language, language) == 0) {
            add_file(p, PATHS[i].path, language, &b);
            return;
        }
    }
    abort();
}

static void groups_shape(project_t *p, int n, const void *arg) {
    buf_t b = {0};
    nested_groups(&b, n, *(const int *)arg);
    add_file(p, "nest.js", "javascript", &b);
}

static void translation_shape(project_t *p, int n, const void *arg) {
    (void)arg;
    buf_t b = {0};
    translation(&b, n);
    add_file(p, "pkg/defs.py", "python", &b);
}

static void generated_shape(project_t *p, int n, const void *arg) {
    (void)arg;
    generated(p, n);
}

static void print_work(const char *what, int n, const outcome_t *o) {
    printf("%s %d:", what, n);
    for (int k = 0; k < PDXE_LC_COUNT; k++) {
        if (o->work[k]) {
            printf(" %s %llu,", COUNTER_NAMES[k], (unsigned long long)o->work[k]);
        }
    }
    printf(" definitions %u, answers %u\n", o->defs, o->answers);
}

/*
 * The work at each size, the sizes doubling: every count at most 2.5 times the last (a
 * quadratic count quadruples), the slots probed at most twice the growth of the file
 * over the whole range (open addressing's clusters vary a little with the keys), and no
 * registry examined entry by entry. `clean`: every extraction and run is whole.
 */
static void linear_work(const char *what, shape_fn shape, const void *arg, const int *sizes,
                        int count, int resolve, mode_t_ mode, const char *family, int clean) {
    uint64_t work[8][PDXE_LC_COUNT];
    int done = 0;
    for (int s = 0; s < count && s < 8; s++) {
        project_t p = {.name = what};
        shape(&p, sizes[s], arg);
        outcome_t o = run(&p, mode, family, resolve);
        memcpy(work[s], o.work, sizeof o.work);
        done = s + 1;
        print_work(what, sizes[s], &o);
        int before = failures;
        CHECK(o.work[PDXE_LC_REGISTRY_ENTRIES] == 0, "%s %d: %llu registry entries examined one "
              "by one", what, sizes[s], (unsigned long long)o.work[PDXE_LC_REGISTRY_ENTRIES]);
        if (clean) {
            CHECK(o.extract_rc == PDXE_OK && o.lost == 0, "%s %d: extraction %d lost %u", what,
                  sizes[s], o.extract_rc, o.lost);
            CHECK(!resolve || (o.run_rc == PDXE_OK && o.health.status == PDXE_RUN_CLEAN),
                  "%s %d: resolution %d, health %d (%u pass failures)", what, sizes[s], o.run_rc,
                  o.health.status, o.health.pass_failures);
        }
        release(&o);
        project_free(&p);
        for (int k = 0; s > 0 && k < PDXE_LC_COUNT; k++) {
            if (k == PDXE_LC_REGISTRY_PROBES) {
                continue;
            }
            CHECK(work[s][k] <= work[s - 1][k] * 5 / 2 + 256,
                  "%s: %s grew from %llu at %d to %llu at %d", what, COUNTER_NAMES[k],
                  (unsigned long long)work[s - 1][k], sizes[s - 1],
                  (unsigned long long)work[s][k], sizes[s]);
        }
        if (failures > before) {
            return; /* the larger sizes would only take longer to say the same */
        }
    }
    int last = done - 1;
    uint64_t growth = (uint64_t)sizes[last] / (uint64_t)sizes[0];
    CHECK(work[last][PDXE_LC_REGISTRY_PROBES] <=
              2 * growth * work[0][PDXE_LC_REGISTRY_PROBES] + 4096,
          "%s: registry probes grew from %llu at %d to %llu at %d", what,
          (unsigned long long)work[0][PDXE_LC_REGISTRY_PROBES], sizes[0],
          (unsigned long long)work[last][PDXE_LC_REGISTRY_PROBES], sizes[last]);
}

/* The scans, for comparison: their work grows with the square of the file (more than
 * sixfold when it quadruples, where linear work would grow fourfold), which is what the
 * counts above would show of any scan an index replaced. */
static void scans_are_quadratic(const char *language) {
    uint64_t total[2] = {0, 0};
    const int sizes[2] = {500, 2000};
    for (int s = 0; s < 2; s++) {
        project_t p = {.name = "scans"};
        flat_shape(&p, sizes[s], language);
        outcome_t o = run(&p, REFERENCE, NULL, 0);
        /* What only a scan counts: registry entries, module statements and the
         * translation's definitions examined one by one. */
        total[s] = o.work[PDXE_LC_REGISTRY_ENTRIES] + o.work[PDXE_LC_PY_CLASS_SCAN] +
                   o.work[PDXE_LC_DEF_ENTRIES];
        release(&o);
        project_free(&p);
    }
    printf("scans %s: %llu at 500, %llu at 2000\n", language, (unsigned long long)total[0],
           (unsigned long long)total[1]);
    CHECK(total[1] >= 6 * total[0], "%s: the scans' work grew from %llu to only %llu", language,
          (unsigned long long)total[0], (unsigned long long)total[1]);
}

/* --- duplicates and the project's prefix --------------------------------------- */

/* A qualified name two definitions share names the first, in the interface's
 * translation as before: what each function holds is scoped to the first `helper`. And
 * the engine's names, which carry the project's prefix, are found without it. */
static void duplicates_and_prefix(void) {
    project_t p = {.name = "duplicate qualified names"};
    add_text(&p, "pkg/dup.py", "python",
             "def other():\n    z = 0\n    return z\n"
             "def helper():\n    a = 1\n    return a\n"
             "def helper():\n    b = 2\n    return b\n");
    const source_file *f = &p.files[0];
    pdxe_file_result *r = NULL;
    int rc = pdxe_extract_file(ctx, pdxe_language_id("python"), f->path,
                               (const uint8_t *)f->source, f->len, &r);
    CHECK(rc == PDXE_OK && r, "duplicate qualified names: extraction %d", rc);
    if (rc == PDXE_OK && r) {
        uint32_t first = PDXE_NO_PARENT, second = PDXE_NO_PARENT, other = PDXE_NO_PARENT;
        for (uint32_t i = 0; i < r->n_defs; i++) {
            if (r->defs[i].name && strcmp(r->defs[i].name, "helper") == 0) {
                if (first == PDXE_NO_PARENT) {
                    first = i;
                } else {
                    second = i;
                }
            } else if (r->defs[i].name && strcmp(r->defs[i].name, "other") == 0) {
                other = i;
            }
        }
        CHECK(first != PDXE_NO_PARENT && second != PDXE_NO_PARENT && other != PDXE_NO_PARENT,
              "duplicate qualified names: definitions other %u, helper %u and %u", other, first,
              second);
        int seen = 0;
        for (uint32_t i = 0; i < r->n_usages; i++) {
            const char *name = tv_text(&r->usages[i].name, (const uint8_t *)f->source, f->len);
            if (!name) {
                continue;
            }
            uint32_t want = strcmp(name, "z") == 0 ? other
                            : strcmp(name, "a") == 0 || strcmp(name, "b") == 0 ? first
                                                                               : UINT32_MAX - 1;
            if (want == UINT32_MAX - 1) {
                continue;
            }
            seen++;
            CHECK(r->usages[i].scope_index == want,
                  "duplicate qualified names: usage %s is scoped to %u, not %u", name,
                  r->usages[i].scope_index, want);
        }
        CHECK(seen >= 3, "duplicate qualified names: %d scoped usages", seen);
        pdxe_result_free(ctx, r);
    }
    same_answers(&p, 1);
    project_free(&p);
}

/* --- refusals ------------------------------------------------------------------ */

/* Each family of indexes refused its memory: extraction and resolution complete or fail
 * as the interface says, the refusal is lost work, no registry is scanned, and the work
 * stays linear. */
static void refusals(void) {
    static const char *FAMILIES[] = {"registry", "scope", "resolver", "interface", "all"};
    for (size_t i = 0; i < sizeof FAMILIES / sizeof FAMILIES[0]; i++) {
        const char *family = FAMILIES[i];
        project_t p = {.name = family};
        translation_shape(&p, 2000, NULL);
        flat_shape(&p, 1000, "typescript");
        flat_shape(&p, 1000, "java");
        buf_t js = {0};
        nested_groups(&js, 8, 20);
        put(&js, "function lexical() {\n  const cb = () => 1;\n");
        for (int k = 0; k < 64; k++) {
            put(&js, "  h(cb);\n");
        }
        put(&js, "}\n");
        add_file(&p, "nest.js", "javascript", &js);
        outcome_t o = run(&p, REFUSED, family, 1);
        printf("refused %s: extraction %d, resolution %d, lost %u, pass failures %u\n", family,
               o.extract_rc, o.run_rc, o.lost, o.health.pass_failures);
        CHECK(o.extract_rc == PDXE_OK || o.extract_rc == PDXE_E_NOMEM,
              "refused %s: extraction %d", family, o.extract_rc);
        CHECK(o.run_rc == PDXE_OK || o.run_rc == PDXE_E_NOMEM, "refused %s: resolution %d",
              family, o.run_rc);
        CHECK(o.lost > 0 || o.health.pass_failures > 0 || o.extract_rc == PDXE_E_NOMEM ||
                  o.run_rc == PDXE_E_NOMEM,
              "refused %s: the refusal was not lost work", family);
        CHECK(o.work[PDXE_LC_REGISTRY_ENTRIES] == 0,
              "refused %s: %llu registry entries examined one by one", family,
              (unsigned long long)o.work[PDXE_LC_REGISTRY_ENTRIES]);
        release(&o);
        project_free(&p);
        char what[64];
        snprintf(what, sizeof what, "refused %s, flat python", family);
        static const int SIZES[] = {1000, 2000, 4000};
        linear_work(what, flat_shape, "python", SIZES, 3, 0, REFUSED, family, 0);
    }
}

/* --- main ---------------------------------------------------------------------- */

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: typed_lookup <resolve fixture root>\n");
        return 2;
    }
    if (pdxe_init(&ctx) != PDXE_OK) {
        return 1;
    }

    resolve_fixtures(argv[1]);
    issue_fixtures();
    duplicates_and_prefix();

    project_t gen = {.name = "generated code"};
    generated(&gen, 200);
    same_answers(&gen, 1);
    project_free(&gen);
    static const int GENERATED[] = {250, 500, 1000};
    linear_work("generated code", generated_shape, NULL, GENERATED, 3, 1, INDEXED, NULL, 1);

    static const char *LANGUAGES[] = {"python", "java", "typescript", "cpp", "csharp",
                                      "go",     "rust", "php",        "kotlin"};
    static const int FLAT[] = {1000, 2000, 4000, 8000, 16000};
    for (size_t i = 0; i < sizeof LANGUAGES / sizeof LANGUAGES[0]; i++) {
        char what[64];
        snprintf(what, sizeof what, "flat %s", LANGUAGES[i]);
        linear_work(what, flat_shape, LANGUAGES[i], FLAT, 5, 0, INDEXED, NULL, 1);
    }
    scans_are_quadratic("python");
    scans_are_quadratic("java");
    scans_are_quadratic("typescript");

    static const int GROUPS[] = {64, 128, 256, 512};
    static const int DEPTH = 100;
    linear_work("nested javascript groups", groups_shape, &DEPTH, GROUPS, 4, 0, INDEXED, NULL, 0);
    project_t groups = {.name = "nested javascript groups"};
    groups_shape(&groups, 32, &DEPTH);
    same_answers(&groups, 1);
    project_free(&groups);

    static const int DEFINITIONS[] = {4000, 8000, 16000};
    linear_work("translation", translation_shape, NULL, DEFINITIONS, 3, 1, INDEXED, NULL, 1);
    project_t defs = {.name = "translation"};
    translation_shape(&defs, 2000, NULL);
    same_answers(&defs, 1);
    project_free(&defs);

    refusals();

    pdxe_shutdown(ctx);
    for (size_t i = 0; i < metadata_strings.count; i++) {
        free(metadata_strings.items[i]);
    }
    free(metadata_strings.items);
    if (failures) {
        printf("%d failure(s)\n", failures);
        return 1;
    }
    printf("ok\n");
    return 0;
}
