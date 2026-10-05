/*
 * What a cache keeps comes back exactly.
 *
 *     abi_result_build_roundtrip <fixture dir>...
 *
 * For every source file under the directories:
 *
 *   - the interface's arrays rebuilt through pdxe_result_build equal the extracted
 *     ones, field by field, strings by content;
 *   - a rebuilt result has no surface of its own to give;
 *   - the file's surface, decoded and encoded again, is the same bytes.
 *
 * Every definition's base classes come back too, in order, and are the rebuilt
 * result's own: rebuilt from a copy that is then overwritten and freed, they still
 * compare equal. Definitions with no base, one base and several must all occur
 * (fixtures/bases), and a definition with none has no array (issue 40).
 *
 * So do a file's `impl Trait for Type` relations, in order, the rebuilt result's own
 * in the same way; files with none, one and several must all occur (fixtures/bases),
 * and a file with none has no array (issue 42).
 *
 * And a definition's decorators, parameter types and route, and a call's node-type
 * path and captured arguments (issues 46 and 47): compared field by field, rebuilt
 * from copies that are then destroyed, and refused when counted without an array or
 * with a NULL where a string is required. Every call extraction positions in the raw
 * source has a path, and decorators, routes and arguments all occur (fixtures/facts).
 *
 * And a definition's route facts (issue 54): every one positioned in the source with
 * its node-type path, compared and rebuilt from copies the same way, refused when
 * counted without an array, missing a required string, or with a path counted without
 * an array or holding a NULL; definitions with one route and with several occur.
 *
 * And surfaces that are not what the encoder writes are refused: truncated, of
 * another version, with a field missing, a field extra, a field of the wrong type,
 * or a counted array whose count disagrees with it.
 *
 * And for every resolution fixture (a directory holding expected.tsv), resolving the
 * files as one project leaves each file's surface byte for byte as it was: a surface
 * holds only what extraction found in the file itself, never anything a resolution
 * over other files computed.
 */

#include <dirent.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

#include "pdxe.h"
#include "pdxe_core.h"
#include "shim_internal.h"

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

static int same_str(const char *a, const char *b) {
    return (!a && !b) || (a && b && strcmp(a, b) == 0);
}

static int same_span(pdxe_span a, pdxe_span b) {
    return memcmp(&a, &b, sizeof(a)) == 0;
}

static int same_strings(const char *const *a, uint32_t na, const char *const *b, uint32_t nb) {
    if (na != nb || (na == 0) != (a == NULL) || (nb == 0) != (b == NULL)) {
        return 0;
    }
    for (uint32_t i = 0; i < na; i++) {
        if (!same_str(a[i], b[i])) {
            return 0;
        }
    }
    return 1;
}

static int same_args(const pdxe_call *x, const pdxe_call *y) {
    if (x->n_args != y->n_args || (y->n_args == 0) != (y->args == NULL)) {
        return 0;
    }
    for (uint32_t a = 0; a < x->n_args; a++) {
        if (!same_str(x->args[a].expr, y->args[a].expr) ||
            !same_str(x->args[a].value, y->args[a].value) ||
            !same_str(x->args[a].keyword, y->args[a].keyword) ||
            x->args[a].index != y->args[a].index) {
            return 0;
        }
    }
    return 1;
}

static int same_routes(const pdxe_definition *x, const pdxe_definition *y) {
    if (x->n_routes != y->n_routes || (y->n_routes == 0) != (y->routes == NULL)) {
        return 0;
    }
    for (uint32_t k = 0; k < x->n_routes; k++) {
        const pdxe_route *a = &x->routes[k], *b = &y->routes[k];
        if (!same_str(a->method, b->method) || !same_str(a->path, b->path) ||
            !same_str(a->callee_text, b->callee_text) ||
            !same_str(a->source_text, b->source_text) || !same_span(a->span, b->span) ||
            !same_strings(a->ast_path, a->n_ast_path, b->ast_path, b->n_ast_path)) {
            return 0;
        }
    }
    return 1;
}

/* Whether every string and path of y's routes is y's own, not x's. */
static int routes_are_copies(const pdxe_definition *x, const pdxe_definition *y) {
    if (x->n_routes && x->routes == y->routes) {
        return 0;
    }
    for (uint32_t k = 0; k < x->n_routes && k < y->n_routes; k++) {
        const pdxe_route *a = &x->routes[k], *b = &y->routes[k];
        if (a->method == b->method || a->path == b->path || a->callee_text == b->callee_text ||
            (a->n_ast_path && a->ast_path == b->ast_path)) {
            return 0;
        }
    }
    return 1;
}

static void compare(const char *path, const pdxe_file_result *a, const pdxe_file_result *b) {
    CHECK(a->n_defs == b->n_defs && a->n_calls == b->n_calls && a->n_imports == b->n_imports &&
              a->n_usages == b->n_usages && a->n_types == b->n_types && a->n_rws == b->n_rws,
          "%s: array lengths differ", path);
    for (uint32_t i = 0; i < a->n_defs && i < b->n_defs; i++) {
        const pdxe_definition *x = &a->defs[i], *y = &b->defs[i];
        CHECK(same_str(x->name, y->name) && same_str(x->qualified_name, y->qualified_name) &&
                  same_str(x->kind, y->kind) && same_str(x->engine_kind, y->engine_kind) &&
                  same_str(x->signature, y->signature) && same_str(x->doc, y->doc) &&
                  same_span(x->span, y->span) && same_span(x->body_span, y->body_span) &&
                  x->parent_index == y->parent_index && x->visibility == y->visibility &&
                  x->is_test == y->is_test && x->is_entry_point == y->is_entry_point &&
                  x->cyclomatic == y->cyclomatic && x->cognitive == y->cognitive &&
                  x->loop_depth == y->loop_depth && x->n_base_classes == y->n_base_classes,
              "%s: definition %u differs", path, i);
        for (uint32_t b = 0; b < x->n_base_classes && b < y->n_base_classes; b++) {
            CHECK(same_str(x->base_classes[b], y->base_classes[b]),
                  "%s: definition %u's base %u differs", path, i, b);
        }
        CHECK(same_strings(x->decorators, x->n_decorators, y->decorators, y->n_decorators) &&
                  same_strings(x->signature_param_types, x->n_signature_param_types,
                               y->signature_param_types, y->n_signature_param_types) &&
                  same_str(x->route_path, y->route_path) &&
                  same_str(x->route_method, y->route_method) && same_routes(x, y),
              "%s: definition %u's decorators, parameter types or route differ", path, i);
    }
    for (uint32_t i = 0; i < a->n_calls && i < b->n_calls; i++) {
        const pdxe_call *x = &a->calls[i], *y = &b->calls[i];
        CHECK(same_str(x->callee_text, y->callee_text) &&
                  same_str(x->receiver_text, y->receiver_text) &&
                  x->caller_index == y->caller_index && same_span(x->span, y->span) &&
                  x->is_reference == y->is_reference && x->typed_only == y->typed_only &&
                  x->lexical == y->lexical &&
                  same_strings(x->ast_path, x->n_ast_path, y->ast_path, y->n_ast_path) &&
                  same_args(x, y),
              "%s: call %u differs", path, i);
    }
    for (uint32_t i = 0; i < a->n_imports && i < b->n_imports; i++) {
        const pdxe_import *x = &a->imports[i], *y = &b->imports[i];
        CHECK(same_str(x->module_text, y->module_text) &&
                  same_str(x->imported_name, y->imported_name) && same_str(x->alias, y->alias) &&
                  same_span(x->span, y->span),
              "%s: import %u differs", path, i);
    }
    for (uint32_t i = 0; i < a->n_usages && i < b->n_usages; i++) {
        const pdxe_usage *x = &a->usages[i], *y = &b->usages[i];
        CHECK(same_str(x->name, y->name) && x->scope_index == y->scope_index &&
                  same_span(x->span, y->span) && x->lexical == y->lexical,
              "%s: usage %u differs", path, i);
    }
    for (uint32_t i = 0; i < a->n_types && i < b->n_types; i++) {
        const pdxe_type_ref *x = &a->types[i], *y = &b->types[i];
        CHECK(same_str(x->type_text, y->type_text) && x->scope_index == y->scope_index &&
                  same_span(x->span, y->span),
              "%s: type reference %u differs", path, i);
    }
    for (uint32_t i = 0; i < a->n_rws && i < b->n_rws; i++) {
        const pdxe_rw *x = &a->rws[i], *y = &b->rws[i];
        CHECK(same_str(x->field_text, y->field_text) && x->scope_index == y->scope_index &&
                  x->is_write == y->is_write && same_span(x->span, y->span),
              "%s: field access %u differs", path, i);
    }
    CHECK(a->n_impl_traits == b->n_impl_traits &&
              (b->n_impl_traits == 0) == (b->impl_traits == NULL),
          "%s: impl relations differ in number", path);
    for (uint32_t i = 0; i < a->n_impl_traits && i < b->n_impl_traits; i++) {
        const pdxe_impl_trait *x = &a->impl_traits[i], *y = &b->impl_traits[i];
        CHECK(same_str(x->trait_name, y->trait_name) && same_str(x->struct_name, y->struct_name) &&
                  same_str(x->struct_qn, y->struct_qn),
              "%s: impl relation %u differs", path, i);
    }
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

static const struct {
    const char *ext;
    const char *lang;
} EXTENSIONS[] = {
    {".py", "python"},   {".ts", "typescript"}, {".tsx", "tsx"},       {".js", "javascript"},
    {".go", "go"},       {".java", "java"},     {".cs", "csharp"},     {".c", "c"},
    {".h", "c"},         {".cpp", "cpp"},       {".hpp", "cpp"},       {".rs", "rust"},
    {".kt", "kotlin"},   {".scala", "scala"},   {".php", "php"},       {".pm", "perl"},
    {".adb", "ada"},     {".sh", "bash"},       {".rb", "ruby"},       {".swift", "swift"},
    {".m", "objc"},      {".groovy", "groovy"}, {".lua", "lua"},       {".sql", "sql"},
    {".proto", "protobuf"}, {".graphql", "graphql"}, {".yaml", "yaml"}, {".json", "json"},
    {".toml", "toml"},   {".tf", "hcl"},        {".dockerfile", "dockerfile"},
    {".md", "markdown"}, {".xml", "xml"},       {".properties", "properties"},
};

static int language_of(const char *name) {
    const char *dot = strrchr(name, '.');
    for (size_t i = 0; dot && i < sizeof(EXTENSIONS) / sizeof(EXTENSIONS[0]); i++) {
        if (strcmp(dot, EXTENSIONS[i].ext) == 0) {
            return pdxe_language_id(EXTENSIONS[i].lang);
        }
    }
    return 0;
}

static int files_checked = 0;
static int defs_without_bases = 0;
static int defs_with_one_base = 0;
static int defs_with_several_bases = 0;
static int calls_with_path = 0;
static int calls_with_args = 0;
static int defs_with_decorators = 0;
static int defs_with_route = 0;
static int defs_with_param_types = 0;
static int defs_with_one_route = 0;
static int defs_with_several_routes = 0;
static int files_without_impls = 0;
static int files_with_one_impl = 0;
static int files_with_several_impls = 0;

/* A copy of a result's calls the caller owns: the array, each path and argument array,
 * and their strings. Other strings stay the result's. */
static pdxe_call *copy_calls(const pdxe_file_result *r) {
    pdxe_call *copy = calloc(r->n_calls ? r->n_calls : 1, sizeof(*copy));
    for (uint32_t i = 0; copy && i < r->n_calls; i++) {
        copy[i] = r->calls[i];
        if (r->calls[i].n_ast_path) {
            const char **p = calloc(r->calls[i].n_ast_path, sizeof(*p));
            for (uint32_t t = 0; p && t < r->calls[i].n_ast_path; t++) {
                p[t] = strdup(r->calls[i].ast_path[t]);
            }
            copy[i].ast_path = p;
        }
        if (r->calls[i].n_args) {
            pdxe_call_arg *a = calloc(r->calls[i].n_args, sizeof(*a));
            for (uint32_t k = 0; a && k < r->calls[i].n_args; k++) {
                a[k] = r->calls[i].args[k];
                a[k].expr = strdup(r->calls[i].args[k].expr);
                a[k].value = r->calls[i].args[k].value ? strdup(r->calls[i].args[k].value) : NULL;
                a[k].keyword =
                    r->calls[i].args[k].keyword ? strdup(r->calls[i].args[k].keyword) : NULL;
            }
            copy[i].args = a;
        }
    }
    return copy;
}

static void scrub(const char *s) {
    if (s) {
        memset((char *)s, 'Z', strlen(s));
        free((char *)s);
    }
}

/* Overwrites and frees what copy_calls made. */
static void destroy_calls(pdxe_call *copy, uint32_t n) {
    for (uint32_t i = 0; i < n; i++) {
        for (uint32_t t = 0; t < copy[i].n_ast_path; t++) {
            scrub(copy[i].ast_path[t]);
        }
        free((void *)copy[i].ast_path);
        for (uint32_t k = 0; k < copy[i].n_args; k++) {
            scrub(copy[i].args[k].expr);
            scrub(copy[i].args[k].value);
            scrub(copy[i].args[k].keyword);
        }
        free((void *)copy[i].args);
    }
    free(copy);
}

/* A copy of a result's impl relations the caller owns, array and strings. */
static pdxe_impl_trait *copy_impls(const pdxe_file_result *r) {
    pdxe_impl_trait *copy = calloc(r->n_impl_traits ? r->n_impl_traits : 1, sizeof(*copy));
    for (uint32_t i = 0; copy && i < r->n_impl_traits; i++) {
        copy[i].trait_name = strdup(r->impl_traits[i].trait_name);
        copy[i].struct_name = strdup(r->impl_traits[i].struct_name);
        copy[i].struct_qn = strdup(r->impl_traits[i].struct_qn);
    }
    return copy;
}

/* Overwrites and frees what copy_impls made. */
static void destroy_impls(pdxe_impl_trait *copy, uint32_t n) {
    for (uint32_t i = 0; i < n; i++) {
        const char *strings[] = {copy[i].trait_name, copy[i].struct_name, copy[i].struct_qn};
        for (size_t s = 0; s < 3; s++) {
            char *p = (char *)strings[s];
            if (p) {
                memset(p, 'Z', strlen(p));
                free(p);
            }
        }
    }
    free(copy);
}

/* A copy of the definitions the caller owns: the array, each base array and base
 * string, and each route array with its strings and paths. Its other strings stay the
 * result's, which outlives the copy. */
static pdxe_definition *copy_defs(const pdxe_file_result *r) {
    pdxe_definition *copy = calloc(r->n_defs ? r->n_defs : 1, sizeof(*copy));
    for (uint32_t i = 0; copy && i < r->n_defs; i++) {
        copy[i] = r->defs[i];
        if (r->defs[i].n_base_classes) {
            const char **bases = calloc(r->defs[i].n_base_classes, sizeof(*bases));
            for (uint32_t b = 0; bases && b < r->defs[i].n_base_classes; b++) {
                bases[b] = strdup(r->defs[i].base_classes[b]);
            }
            copy[i].base_classes = bases;
        }
        if (r->defs[i].n_routes) {
            pdxe_route *routes = calloc(r->defs[i].n_routes, sizeof(*routes));
            for (uint32_t k = 0; routes && k < r->defs[i].n_routes; k++) {
                const pdxe_route *from = &r->defs[i].routes[k];
                routes[k] = *from;
                routes[k].method = strdup(from->method);
                routes[k].path = strdup(from->path);
                routes[k].callee_text = strdup(from->callee_text);
                routes[k].source_text = from->source_text ? strdup(from->source_text) : NULL;
                if (from->n_ast_path) {
                    const char **types = calloc(from->n_ast_path, sizeof(*types));
                    for (uint32_t t = 0; types && t < from->n_ast_path; t++) {
                        types[t] = strdup(from->ast_path[t]);
                    }
                    routes[k].ast_path = types;
                }
            }
            copy[i].routes = routes;
        }
    }
    return copy;
}

/* Overwrites and frees what copy_defs made, so anything still pointing into it reads
 * something else. */
static void destroy_defs(pdxe_definition *copy, uint32_t n) {
    for (uint32_t i = 0; i < n; i++) {
        for (uint32_t b = 0; b < copy[i].n_base_classes; b++) {
            char *s = (char *)copy[i].base_classes[b];
            memset(s, 'Z', strlen(s));
            free(s);
            copy[i].base_classes[b] = NULL;
        }
        free((void *)copy[i].base_classes);
        for (uint32_t k = 0; copy[i].routes && k < copy[i].n_routes; k++) {
            pdxe_route *route = (pdxe_route *)&copy[i].routes[k];
            scrub(route->method);
            scrub(route->path);
            scrub(route->callee_text);
            scrub(route->source_text);
            for (uint32_t t = 0; t < route->n_ast_path; t++) {
                scrub(route->ast_path[t]);
            }
            free((void *)route->ast_path);
        }
        free((void *)copy[i].routes);
    }
    free(copy);
}

static void check_file(pdxe_ctx *ctx, const char *path) {
    int lang = language_of(path);
    if (!lang) {
        return;
    }
    size_t len = 0;
    unsigned char *bytes = read_all(path, &len);
    pdxe_file_result *r = NULL;
    if (!bytes || pdxe_extract_file(ctx, lang, path, bytes, len, &r) != PDXE_OK) {
        CHECK(0, "%s: extraction failed", path);
        free(bytes);
        return;
    }
    files_checked++;
    for (uint32_t i = 0; i < r->n_defs; i++) {
        const pdxe_definition *d = &r->defs[i];
        CHECK((d->n_base_classes == 0) == (d->base_classes == NULL),
              "%s: definition %u has %u bases and %s array", path, i, d->n_base_classes,
              d->base_classes ? "an" : "no");
        for (uint32_t b = 0; b < d->n_base_classes; b++) {
            CHECK(d->base_classes[b] && d->base_classes[b][0], "%s: definition %u's base %u is empty",
                  path, i, b);
        }
        defs_without_bases += d->n_base_classes == 0;
        defs_with_one_base += d->n_base_classes == 1;
        defs_with_several_bases += d->n_base_classes > 1;
    }
    CHECK((r->n_impl_traits == 0) == (r->impl_traits == NULL),
          "%s: %u impl relations and %s array", path, r->n_impl_traits,
          r->impl_traits ? "an" : "no");
    for (uint32_t i = 0; i < r->n_impl_traits; i++) {
        const pdxe_impl_trait *it = &r->impl_traits[i];
        CHECK(it->trait_name && it->trait_name[0] && it->struct_name && it->struct_name[0] &&
                  it->struct_qn && it->struct_qn[0],
              "%s: impl relation %u has an empty string", path, i);
    }
    for (uint32_t i = 0; i < r->n_calls; i++) {
        const pdxe_call *c = &r->calls[i];
        int raw = c->span.end_byte > c->span.start_byte;
        CHECK(!raw || c->n_ast_path > 0, "%s: call %u (%s) is in the source but has no path", path,
              i, c->callee_text);
        CHECK((c->n_ast_path == 0) == (c->ast_path == NULL) && (c->n_args == 0) == (c->args == NULL),
              "%s: call %u counts an array it does not have", path, i);
        for (uint32_t t = 0; t < c->n_ast_path; t++) {
            CHECK(c->ast_path[t] && c->ast_path[t][0], "%s: call %u's path has an empty type",
                  path, i);
        }
        calls_with_path += c->n_ast_path > 0;
        calls_with_args += c->n_args > 0;
    }
    for (uint32_t i = 0; i < r->n_defs; i++) {
        defs_with_decorators += r->defs[i].n_decorators > 0;
        defs_with_route += r->defs[i].route_path != NULL;
        defs_with_param_types += r->defs[i].n_signature_param_types > 0;
        const pdxe_definition *d = &r->defs[i];
        CHECK((d->n_routes == 0) == (d->routes == NULL), "%s: definition %u counts %u routes "
              "and has %s array", path, i, d->n_routes, d->routes ? "an" : "no");
        for (uint32_t k = 0; k < d->n_routes; k++) {
            const pdxe_route *route = &d->routes[k];
            CHECK(route->method && route->method[0] && route->path && route->path[0] == '/' &&
                      route->callee_text && route->callee_text[0] && route->source_text &&
                      route->span.end_byte > route->span.start_byte && route->span.start_line > 0 &&
                      route->n_ast_path > 0,
                  "%s: definition %u's route %u is not a positioned route", path, i, k);
            for (uint32_t t = 0; t < route->n_ast_path; t++) {
                CHECK(route->ast_path[t] && route->ast_path[t][0],
                      "%s: definition %u's route %u's path has an empty type", path, i, k);
            }
        }
        defs_with_one_route += d->n_routes == 1;
        defs_with_several_routes += d->n_routes > 1;
    }
    files_without_impls += r->n_impl_traits == 0;
    files_with_one_impl += r->n_impl_traits == 1;
    files_with_several_impls += r->n_impl_traits > 1;

    /* Rebuilt from a copy that is destroyed before the comparison: the bases must be
     * the rebuilt result's own. */
    pdxe_definition *copy = copy_defs(r);
    pdxe_impl_trait *impls = copy_impls(r);
    pdxe_call *calls = copy_calls(r);
    pdxe_file_result *from_copy = NULL;
    CHECK(copy && impls && calls &&
              pdxe_result_build(ctx, copy, r->n_defs, r->n_calls ? calls : NULL, r->n_calls, NULL,
                                0, NULL, 0, NULL, 0, NULL, 0, r->n_impl_traits ? impls : NULL,
                                r->n_impl_traits, &from_copy) == PDXE_OK,
          "%s: rebuild from a copy failed", path);
    if (copy) {
        destroy_defs(copy, r->n_defs);
    }
    if (calls) {
        destroy_calls(calls, r->n_calls);
    }
    if (impls) {
        destroy_impls(impls, r->n_impl_traits);
    }
    if (from_copy) {
        CHECK(from_copy->n_defs == r->n_defs, "%s: a rebuild lost definitions", path);
        for (uint32_t i = 0; i < from_copy->n_defs && i < r->n_defs; i++) {
            const pdxe_definition *x = &r->defs[i], *y = &from_copy->defs[i];
            CHECK(x->n_base_classes == y->n_base_classes &&
                      (y->n_base_classes == 0) == (y->base_classes == NULL),
                  "%s: definition %u's bases were not rebuilt", path, i);
            for (uint32_t b = 0; b < x->n_base_classes && b < y->n_base_classes; b++) {
                CHECK(same_str(x->base_classes[b], y->base_classes[b]) &&
                          y->base_classes[b] != x->base_classes[b],
                      "%s: definition %u's base %u is not a copy of its own", path, i, b);
            }
        }
        for (uint32_t i = 0; i < from_copy->n_calls && i < r->n_calls; i++) {
            const pdxe_call *x = &r->calls[i], *y = &from_copy->calls[i];
            CHECK(same_strings(x->ast_path, x->n_ast_path, y->ast_path, y->n_ast_path) &&
                      same_args(x, y) && (x->n_ast_path == 0 || x->ast_path != y->ast_path) &&
                      (x->n_args == 0 || x->args != y->args),
                  "%s: call %u's path or arguments are not a copy of its own", path, i);
        }
        for (uint32_t i = 0; i < from_copy->n_defs && i < r->n_defs; i++) {
            const pdxe_definition *x = &r->defs[i], *y = &from_copy->defs[i];
            CHECK(same_strings(x->decorators, x->n_decorators, y->decorators, y->n_decorators) &&
                      same_strings(x->signature_param_types, x->n_signature_param_types,
                                   y->signature_param_types, y->n_signature_param_types) &&
                      same_str(x->route_path, y->route_path) &&
                      same_str(x->route_method, y->route_method) &&
                      (x->route_path == NULL || x->route_path != y->route_path) &&
                      same_routes(x, y) && routes_are_copies(x, y),
                  "%s: definition %u's facts are not a copy of its own", path, i);
        }
        CHECK(from_copy->n_impl_traits == r->n_impl_traits &&
                  (from_copy->n_impl_traits == 0) == (from_copy->impl_traits == NULL),
              "%s: the impl relations were not rebuilt", path);
        for (uint32_t i = 0; i < from_copy->n_impl_traits && i < r->n_impl_traits; i++) {
            const pdxe_impl_trait *x = &r->impl_traits[i], *y = &from_copy->impl_traits[i];
            CHECK(same_str(x->trait_name, y->trait_name) &&
                      same_str(x->struct_name, y->struct_name) &&
                      same_str(x->struct_qn, y->struct_qn) && y->trait_name != x->trait_name &&
                      y->struct_name != x->struct_name && y->struct_qn != x->struct_qn,
                  "%s: impl relation %u is not a copy of its own", path, i);
        }
        pdxe_result_free(ctx, from_copy);
    }

    pdxe_file_result *rebuilt = NULL;
    CHECK(pdxe_result_build(ctx, r->defs, r->n_defs, r->calls, r->n_calls, r->imports,
                            r->n_imports, r->usages, r->n_usages, r->types, r->n_types, r->rws,
                            r->n_rws, r->impl_traits, r->n_impl_traits, &rebuilt) == PDXE_OK,
          "%s: rebuild failed", path);
    if (rebuilt) {
        compare(path, r, rebuilt);
        uint8_t *none = NULL;
        size_t none_len = 0;
        CHECK(pdxe_surface_export(rebuilt, path, &none, &none_len) == PDXE_E_INVALID && !none,
              "%s: a rebuilt result gave a surface", path);
    }

    uint8_t *surface = NULL;
    size_t surface_len = 0;
    CHECK(pdxe_surface_export(r, path, &surface, &surface_len) == PDXE_OK, "%s: export failed",
          path);
    PDXEFileResult *decoded = NULL;
    char *decoded_path = NULL;
    int decoded_lang = 0;
    uint32_t decoded_lost = UINT32_MAX;
    if (surface && pdxe_surface_decode(surface, surface_len, &decoded, &decoded_path,
                                       &decoded_lang, &decoded_lost) == PDXE_OK) {
        CHECK(strcmp(decoded_path, path) == 0 && decoded_lang == lang,
              "%s: surface names another file", path);
        CHECK(decoded_lost == 0, "%s: extraction lost work on a fixture", path);
        uint8_t *again = NULL;
        size_t again_len = 0;
        CHECK(pdxe_surface_encode(decoded, decoded_path, decoded_lang, decoded_lost, &again,
                                  &again_len) == PDXE_OK &&
                  again_len == surface_len && memcmp(again, surface, surface_len) == 0,
              "%s: the surface does not survive decoding", path);
        free(again);
        pdxe_free_result(decoded);
        free(decoded_path);
    } else if (surface) {
        CHECK(0, "%s: the surface does not decode", path);
    }
    pdxe_surface_free(surface);
    pdxe_result_free(ctx, rebuilt);
    pdxe_result_free(ctx, r);
    free(bytes);
}

/* Every source file under `dir`, relative to it. */
static void collect_sources(const char *root, const char *rel, char ***paths, size_t *n) {
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
            collect_sources(root, child, paths, n);
        } else if (language_of(child)) {
            *paths = realloc(*paths, (*n + 1) * sizeof(char *));
            (*paths)[(*n)++] = strdup(child);
        }
    }
    closedir(d);
}

static int projects_checked = 0;

static void check_resolution_leaves_surfaces_alone(pdxe_ctx *ctx, const char *dir) {
    char **paths = NULL;
    size_t n = 0;
    collect_sources(dir, "", &paths, &n);
    pdxe_file_result **results = calloc(n ? n : 1, sizeof(*results));
    unsigned char **sources = calloc(n ? n : 1, sizeof(*sources));
    size_t *lens = calloc(n ? n : 1, sizeof(*lens));
    uint8_t **before = calloc(n ? n : 1, sizeof(*before));
    size_t *before_len = calloc(n ? n : 1, sizeof(*before_len));
    pdxe_project *p = NULL;
    CHECK(pdxe_resolve_project_begin(ctx, &p) == PDXE_OK, "%s: cannot begin a project", dir);
    for (size_t i = 0; p && i < n; i++) {
        char full[8200];
        snprintf(full, sizeof(full), "%s/%s", dir, paths[i]);
        sources[i] = read_all(full, &lens[i]);
        int lang = language_of(paths[i]);
        CHECK(sources[i] && pdxe_extract_file(ctx, lang, paths[i], sources[i], lens[i],
                                              &results[i]) == PDXE_OK &&
                  pdxe_surface_export(results[i], paths[i], &before[i], &before_len[i]) ==
                      PDXE_OK &&
                  pdxe_resolve_project_add_file(p, lang, paths[i], sources[i], lens[i],
                                                results[i]) == PDXE_OK,
              "%s/%s: cannot prepare", dir, paths[i]);
        uint8_t *during = NULL;
        size_t during_len = 0;
        CHECK(pdxe_surface_export(results[i], paths[i], &during, &during_len) == PDXE_E_INVALID,
              "%s/%s: a surface was given while a project held the result", dir, paths[i]);
        pdxe_surface_free(during);
    }
    CHECK(p && pdxe_resolve_project_run(p) == PDXE_OK, "%s: resolution failed", dir);
    pdxe_resolve_project_end(p);
    for (size_t i = 0; i < n; i++) {
        uint8_t *after = NULL;
        size_t after_len = 0;
        CHECK(results[i] &&
                  pdxe_surface_export(results[i], paths[i], &after, &after_len) == PDXE_OK &&
                  after_len == before_len[i] && memcmp(after, before[i], after_len) == 0,
              "%s/%s: resolving the project changed the file's surface", dir, paths[i]);
        pdxe_surface_free(after);
        pdxe_surface_free(before[i]);
        pdxe_result_free(ctx, results[i]);
        free(sources[i]);
        free(paths[i]);
    }
    free(results);
    free(sources);
    free(lens);
    free(before);
    free(before_len);
    free(paths);
    projects_checked++;
}

static void walk(pdxe_ctx *ctx, const char *dir) {
    DIR *d = opendir(dir);
    if (!d) {
        return;
    }
    struct dirent *e;
    while ((e = readdir(d)) != NULL) {
        if (e->d_name[0] == '.') {
            continue;
        }
        char path[4096];
        snprintf(path, sizeof(path), "%s/%s", dir, e->d_name);
        struct stat st;
        if (stat(path, &st) != 0) {
            continue;
        }
        if (S_ISDIR(st.st_mode)) {
            char expected[8200];
            snprintf(expected, sizeof(expected), "%s/expected.tsv", path);
            if (stat(expected, &st) == 0) {
                check_resolution_leaves_surfaces_alone(ctx, path);
            }
            walk(ctx, path);
        } else {
            check_file(ctx, path);
        }
    }
    closedir(d);
}

/* Replaces the first occurrence of `from` in `s` with `to`, into a new string. */
static char *replace(const char *s, const char *from, const char *to) {
    const char *at = strstr(s, from);
    if (!at) {
        return NULL;
    }
    size_t n = strlen(s) - strlen(from) + strlen(to);
    char *out = malloc(n + 1);
    size_t head = (size_t)(at - s);
    memcpy(out, s, head);
    strcpy(out + head, to);
    strcat(out, at + strlen(from));
    return out;
}

static void expect_refused(const char *what, const char *bytes) {
    PDXEFileResult *r = NULL;
    char *path = NULL;
    int lang = 0;
    uint32_t lost = 0;
    int rc = bytes ? pdxe_surface_decode((const uint8_t *)bytes, strlen(bytes), &r, &path, &lang,
                                         &lost)
                   : PDXE_E_INVALID;
    CHECK(bytes && rc == PDXE_E_INVALID && !r && !path, "a surface %s was accepted", what);
    if (r) {
        pdxe_free_result(r);
    }
    free(path);
}

static void check_refusals(pdxe_ctx *ctx) {
    static const char source[] = "class A:\n    def m(self, x: int) -> int:\n        return x\n";
    pdxe_file_result *r = NULL;
    uint8_t *surface = NULL;
    size_t len = 0;
    if (pdxe_extract_file(ctx, pdxe_language_id("python"), "a.py", (const uint8_t *)source,
                          sizeof(source) - 1, &r) != PDXE_OK ||
        pdxe_surface_export(r, "a.py", &surface, &len) != PDXE_OK) {
        CHECK(0, "cannot make a surface to corrupt");
        pdxe_result_free(ctx, r);
        return;
    }
    char *good = malloc(len + 1);
    memcpy(good, surface, len);
    good[len] = '\0';

    char *truncated = strdup(good);
    truncated[len / 2] = '\0';
    expect_refused("cut short", truncated);
    free(truncated);

    char *m;
    expect_refused("of another version", m = replace(good, "\"v\":4", "\"v\":3"));
    free(m);
    expect_refused("without what extraction lost", m = replace(good, "\"lost\":0,", ""));
    free(m);
    expect_refused("with a negative loss", m = replace(good, "\"lost\":0", "\"lost\":-1"));
    free(m);
    expect_refused("without a field", m = replace(good, "\"lsp_skipped\":false,", ""));
    free(m);
    expect_refused("with an extra field", m = replace(good, "\"lsp_skipped\":false",
                                                      "\"lsp_skipped\":false,\"extra\":1"));
    free(m);
    expect_refused("with a field of the wrong type",
                   m = replace(good, "\"lsp_skipped\":false", "\"lsp_skipped\":0"));
    free(m);
    expect_refused("with an extra top-level key", m = replace(good, "\"v\":4", "\"v\":4,\"x\":0"));
    free(m);
    /* A definition's bases: counted bases with no array, and a NULL base. */
    pdxe_definition with_bases = r->defs[0];
    pdxe_file_result *refused = NULL;
    with_bases.n_base_classes = 1;
    with_bases.base_classes = NULL;
    CHECK(pdxe_result_build(ctx, &with_bases, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                            NULL, 0, &refused) == PDXE_E_INVALID && !refused,
          "a definition counting bases it has no array for was rebuilt");
    const char *null_base[] = {"Base", NULL};
    with_bases.n_base_classes = 2;
    with_bases.base_classes = null_base;
    CHECK(pdxe_result_build(ctx, &with_bases, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                            NULL, 0, &refused) == PDXE_E_INVALID && !refused,
          "a definition with a NULL base was rebuilt");
    /* A definition's decorators counted with no array, and parameter types holding a
     * NULL; a call's path counted with no array or holding a NULL; arguments counted
     * with no array; an argument with no expression. */
    pdxe_definition with_decorators = r->defs[0];
    with_decorators.n_base_classes = 0;
    with_decorators.base_classes = NULL;
    with_decorators.n_decorators = 1;
    with_decorators.decorators = NULL;
    CHECK(pdxe_result_build(ctx, &with_decorators, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                            NULL, 0, &refused) == PDXE_E_INVALID && !refused,
          "decorators counted with no array were rebuilt");
    const char *null_param[] = {"int", NULL};
    with_decorators.n_decorators = 0;
    with_decorators.decorators = NULL;
    with_decorators.n_signature_param_types = 2;
    with_decorators.signature_param_types = null_param;
    CHECK(pdxe_result_build(ctx, &with_decorators, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                            NULL, 0, &refused) == PDXE_E_INVALID && !refused,
          "a parameter type that is NULL was rebuilt");
    pdxe_call bad_call = {0};
    bad_call.callee_text = "f";
    bad_call.caller_index = PDXE_NO_PARENT;
    bad_call.n_ast_path = 1;
    CHECK(pdxe_result_build(ctx, NULL, 0, &bad_call, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                            &refused) == PDXE_E_INVALID && !refused,
          "a path counted with no array was rebuilt");
    const char *null_type[] = {"block", NULL};
    bad_call.ast_path = null_type;
    bad_call.n_ast_path = 2;
    CHECK(pdxe_result_build(ctx, NULL, 0, &bad_call, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                            &refused) == PDXE_E_INVALID && !refused,
          "a path with a NULL type was rebuilt");
    bad_call.ast_path = NULL;
    bad_call.n_ast_path = 0;
    bad_call.n_args = 1;
    CHECK(pdxe_result_build(ctx, NULL, 0, &bad_call, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                            &refused) == PDXE_E_INVALID && !refused,
          "arguments counted with no array were rebuilt");
    const pdxe_call_arg no_expr = {NULL, "v", NULL, 0};
    bad_call.args = &no_expr;
    CHECK(pdxe_result_build(ctx, NULL, 0, &bad_call, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                            &refused) == PDXE_E_INVALID && !refused,
          "an argument with no expression was rebuilt");
    /* Impl relations: counted with no array, and one with a NULL string. */
    CHECK(pdxe_result_build(ctx, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 1,
                            &refused) == PDXE_E_INVALID && !refused,
          "impl relations counted with no array were rebuilt");
    const pdxe_impl_trait null_qn = {"Shape", "Square", NULL};
    CHECK(pdxe_result_build(ctx, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0, &null_qn, 1,
                            &refused) == PDXE_E_INVALID && !refused,
          "an impl relation with a NULL qualified name was rebuilt");
    /* Routes (issue 54): counted with no array; one without its method; one whose path is
     * counted with no array; one whose path holds a NULL. */
    pdxe_definition with_routes = r->defs[0];
    with_routes.n_base_classes = 0;
    with_routes.base_classes = NULL;
    with_routes.n_decorators = 0;
    with_routes.decorators = NULL;
    with_routes.n_signature_param_types = 0;
    with_routes.signature_param_types = NULL;
    with_routes.n_routes = 1;
    with_routes.routes = NULL;
    CHECK(pdxe_result_build(ctx, &with_routes, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                            NULL, 0, &refused) == PDXE_E_INVALID && !refused,
          "routes counted with no array were rebuilt");
    pdxe_route route = {"GET", "/x", "GetMapping", "@GetMapping(\"/x\")", {0}, NULL, 0};
    route.method = NULL;
    with_routes.routes = &route;
    CHECK(pdxe_result_build(ctx, &with_routes, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                            NULL, 0, &refused) == PDXE_E_INVALID && !refused,
          "a route with no method was rebuilt");
    route.method = "GET";
    route.n_ast_path = 1;
    CHECK(pdxe_result_build(ctx, &with_routes, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                            NULL, 0, &refused) == PDXE_E_INVALID && !refused,
          "a route path counted with no array was rebuilt");
    const char *null_step[] = {"method_declaration", NULL};
    route.ast_path = null_step;
    route.n_ast_path = 2;
    CHECK(pdxe_result_build(ctx, &with_routes, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                            NULL, 0, &refused) == PDXE_E_INVALID && !refused,
          "a route path with a NULL type was rebuilt");
    route.n_ast_path = 1;
    pdxe_file_result *accepted = NULL;
    CHECK(pdxe_result_build(ctx, &with_routes, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                            NULL, 0, &accepted) == PDXE_OK && accepted &&
              accepted->defs[0].n_routes == 1 &&
              same_str(accepted->defs[0].routes[0].path, "/x") &&
              accepted->defs[0].routes[0].path != route.path,
          "a well-formed route was not rebuilt as a copy");
    if (accepted) {
        pdxe_result_free(ctx, accepted);
    }

    expect_refused("whose count disagrees with its array",
                   m = replace(good, "\"signature_param_count\":1", "\"signature_param_count\":2"));
    free(m);

    free(good);
    pdxe_surface_free(surface);
    pdxe_result_free(ctx, r);
}

int main(int argc, char **argv) {
    if (argc < 2) {
        fprintf(stderr, "usage: abi_result_build_roundtrip <fixture dir>...\n");
        return 2;
    }
    pdxe_ctx *ctx = NULL;
    if (pdxe_init(&ctx) != PDXE_OK) {
        return 1;
    }
    for (int a = 1; a < argc; a++) {
        walk(ctx, argv[a]);
    }
    check_refusals(ctx);
    pdxe_shutdown(ctx);
    CHECK(defs_without_bases > 0 && defs_with_one_base > 0 && defs_with_several_bases > 0,
          "bases seen: %d definitions with none, %d with one, %d with several",
          defs_without_bases, defs_with_one_base, defs_with_several_bases);
    CHECK(calls_with_path > 0 && calls_with_args > 0 && defs_with_decorators > 0 &&
              defs_with_route > 0 && defs_with_param_types > 0,
          "facts seen: %d calls with a path, %d with arguments; %d definitions with "
          "decorators, %d with a route, %d with parameter types",
          calls_with_path, calls_with_args, defs_with_decorators, defs_with_route,
          defs_with_param_types);
    CHECK(defs_with_one_route > 0 && defs_with_several_routes > 0,
          "routes seen: %d definitions with one, %d with several", defs_with_one_route,
          defs_with_several_routes);
    CHECK(files_without_impls > 0 && files_with_one_impl > 0 && files_with_several_impls > 0,
          "impl relations seen: %d files with none, %d with one, %d with several",
          files_without_impls, files_with_one_impl, files_with_several_impls);
    printf("%s: %d files, %d projects, %d failures\n", failures ? "FAIL" : "ok", files_checked,
           projects_checked, failures);
    return failures || files_checked == 0 ? 1 : 0;
}
