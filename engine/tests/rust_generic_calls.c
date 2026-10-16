/*
 * A Rust call whose function carries a turbofish is a call of that function
 * (docs/plan/ISSUES.md, issue 107A; engine/patches/0033-rust-turbofish-call-extraction.patch).
 *
 *     rust_generic_calls
 *
 * In the pinned grammar `f::<T>()`'s function is a generic_function, whose own
 * `function` field is the path or the method (`x.m::<T>()`). The callee lookup knew no
 * generic_function, so such a call had no callee and was dropped. Now it is its
 * function's call, recorded exactly as the same call without the type arguments.
 *
 *   - Every call of a source against its twin, the same source with each call's own
 *     turbofish replaced by spaces of its length: the same callee (its text the twin's
 *     once blanked), owner, arguments, loop and branch depth, line, site, buffer,
 *     method and local-binding flags, `use` scope and syntax path (less the
 *     generic_function a chain's link sits below); and every usage against its twin's,
 *     so the occurrence a call consumes is the same and no path is left over as a use.
 *   - A call in a macro's tokens is none: the grammar gives a token tree no call
 *     (issue 107B's, untouched).
 */

#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

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

static pdxe_ctx *ctx = NULL;

static const char SOURCE[] = "use crate::codec;\n"
                             "\n"
                             "macro_rules! wrap {\n"
                             "    ($e:expr) => {\n"
                             "        $e\n"
                             "    };\n"
                             "}\n"
                             "\n"
                             "pub fn pick<T>(_path: &str) -> u8 {\n"
                             "    0\n"
                             "}\n"
                             "\n"
                             "pub fn calls(store: &Store, err: Failure, x: u8) -> u8 {\n"
                             "    let a = pick::<u8>(\"/orders\");\n"
                             "    let b = codec::decode::<u8>(x, handler);\n"
                             "    let c = Grid::<u8>::sized::<u16>();\n"
                             "    let d = store.widen::<u8, Vec<u16>>(x);\n"
                             "    let e = start::<u8>().widen_by::<u16>().settle_on::<u32>();\n"
                             "    if let Ok(found) = err.downcast::<u8>() {\n"
                             "        return found;\n"
                             "    }\n"
                             "    for _ in 0..2 {\n"
                             "        while x > 0 {\n"
                             "            store.fetch::<u8>();\n"
                             "        }\n"
                             "    }\n"
                             "    fn inner<T>() -> u8 {\n"
                             "        0\n"
                             "    }\n"
                             "    let f = wrap!(phantom::<u8>());\n"
                             "    a + b + c + d + e + f + inner::<u8>() + <S as P>::probe::<u8>(&x)\n"
                             "}\n";

/* `source` with each call's own turbofish (`::<...>` right before the call's `(`)
 * replaced by spaces of its length. */
static char *without_turbofish(const char *source) {
    size_t n = strlen(source);
    char *out = malloc(n + 1);
    if (!out) {
        printf("FAIL out of memory\n");
        exit(1);
    }
    memcpy(out, source, n + 1);
    for (size_t i = 0; i + 2 < n; i++) {
        if (strncmp(source + i, "::<", 3) != 0) {
            continue;
        }
        size_t end = i + 3;
        int depth = 1;
        while (depth > 0 && end < n) {
            depth += source[end] == '<' ? 1 : source[end] == '>' ? -1 : 0;
            end++;
        }
        if (end < n && source[end] == '(') {
            memset(out + i, ' ', end - i);
        }
        i = end - 1;
    }
    return out;
}

static const char *or_empty(const char *s) {
    return s ? s : "";
}

/* The node types of `path` but generic_function, joined. */
static void path_of(const char **path, char *out, size_t cap) {
    out[0] = '\0';
    for (int i = 0; path && path[i]; i++) {
        if (strcmp(path[i], "generic_function") != 0) {
            strncat(out, path[i], cap - strlen(out) - 2);
            strncat(out, "/", cap - strlen(out) - 1);
        }
    }
}

static void check_twins(void) {
    char *plain = without_turbofish(SOURCE);
    pdxe_file_result *rt = NULL, *rp = NULL;
    if (pdxe_extract_file(ctx, pdxe_language_id("rust"), "src/calls.rs", (const uint8_t *)SOURCE,
                          strlen(SOURCE), &rt) != PDXE_OK ||
        pdxe_extract_file(ctx, pdxe_language_id("rust"), "src/calls.rs", (const uint8_t *)plain,
                          strlen(plain), &rp) != PDXE_OK ||
        !rt || !rp) {
        printf("FAIL the extraction failed\n");
        exit(1);
    }
    const PDXECallArray *t = &pdxe_result_engine(rt)->calls;
    const PDXECallArray *p = &pdxe_result_engine(rp)->calls;
    CHECK(t->count == p->count, "%d calls, the twin %d", t->count, p->count);
    int sites = 0, in_loops = 0, in_branch = 0, bound = 0;
    for (int i = 0; i < t->count && i < p->count; i++) {
        const PDXECall *a = &t->items[i], *b = &p->items[i];
        char *callee = without_turbofish(or_empty(a->callee_name));
        CHECK(strcmp(callee, or_empty(b->callee_name)) == 0, "%s: the twin's is %s", callee,
              or_empty(b->callee_name));
        free(callee);
        const char *name = or_empty(b->callee_name);
        CHECK(strcmp(or_empty(a->enclosing_func_qn), or_empty(b->enclosing_func_qn)) == 0,
              "%s: caller %s, the twin's %s", name, or_empty(a->enclosing_func_qn),
              or_empty(b->enclosing_func_qn));
        CHECK(a->owner.decided == b->owner.decided &&
                  a->owner.declaration.start_byte == b->owner.declaration.start_byte &&
                  a->owner.declaration.end_byte == b->owner.declaration.end_byte &&
                  a->owner.declaration.origin == b->owner.declaration.origin,
              "%s: another owner", name);
        CHECK(strcmp(or_empty(a->first_string_arg), or_empty(b->first_string_arg)) == 0 &&
                  strcmp(or_empty(a->second_arg_name), or_empty(b->second_arg_name)) == 0,
              "%s: other string or handler arguments", name);
        CHECK(a->arg_count == b->arg_count, "%s: %d arguments, the twin %d", name, a->arg_count,
              b->arg_count);
        for (int k = 0; k < a->arg_count && k < b->arg_count; k++) {
            const PDXECallArg *x = &a->args[k], *y = &b->args[k];
            CHECK(x->index == y->index && x->expr_start == y->expr_start &&
                      x->expr_end == y->expr_end &&
                      strcmp(or_empty(x->expr), or_empty(y->expr)) == 0 &&
                      strcmp(or_empty(x->value), or_empty(y->value)) == 0 &&
                      strcmp(or_empty(x->keyword), or_empty(y->keyword)) == 0,
                  "%s: argument %d differs", name, k);
        }
        CHECK(a->loop_depth == b->loop_depth && a->branch_depth == b->branch_depth,
              "%s: depths %d/%d, the twin's %d/%d", name, a->loop_depth, a->branch_depth,
              b->loop_depth, b->branch_depth);
        CHECK(a->start_line == b->start_line && a->site_start_byte == b->site_start_byte &&
                  a->site_end_byte == b->site_end_byte && a->source_origin == b->source_origin,
              "%s: another site", name);
        CHECK(a->is_method == b->is_method &&
                  a->requires_lsp_resolution == b->requires_lsp_resolution &&
                  a->callee_is_locally_bound == b->callee_is_locally_bound &&
                  a->receiver_is_self_attribute == b->receiver_is_self_attribute,
              "%s: other flags", name);
        CHECK(a->use_scope == b->use_scope, "%s: scope %d, the twin's %d", name, a->use_scope,
              b->use_scope);
        char pa[1024], pb[1024];
        path_of(a->ast_path, pa, sizeof pa);
        path_of(b->ast_path, pb, sizeof pb);
        CHECK(strcmp(pa, pb) == 0, "%s: path %s, the twin's %s", name, pa, pb);
        CHECK(strstr(name, "phantom") == NULL, "a call in a macro's tokens: %s", name);
        sites += a->site_end_byte > a->site_start_byte;
        in_loops += a->loop_depth == 2;
        in_branch += a->branch_depth == 1 && strcmp(name, "err.downcast") == 0;
        bound += a->callee_is_locally_bound && strcmp(name, "inner") == 0;
    }
    /* Every usage against its twin's: what a call consumes is the same occurrence, so
     * no name the turbofish call reads is left over as a value use, and none added. */
    const PDXEUsageArray *ut = &pdxe_result_engine(rt)->usages;
    const PDXEUsageArray *up = &pdxe_result_engine(rp)->usages;
    CHECK(ut->count == up->count, "%d usages, the twin %d", ut->count, up->count);
    for (int i = 0; i < ut->count && i < up->count; i++) {
        const PDXEUsage *a = &ut->items[i], *b = &up->items[i];
        CHECK(strcmp(or_empty(a->ref_name), or_empty(b->ref_name)) == 0 &&
                  strcmp(or_empty(a->enclosing_func_qn), or_empty(b->enclosing_func_qn)) == 0 &&
                  a->site_start_byte == b->site_start_byte &&
                  a->site_end_byte == b->site_end_byte && a->kind == b->kind &&
                  a->may_be_call_reference == b->may_be_call_reference &&
                  a->is_member_access == b->is_member_access,
              "usage %d: %s at %u, the twin's %s at %u", i, or_empty(a->ref_name),
              a->site_start_byte, or_empty(b->ref_name), b->site_start_byte);
    }
    /* pick, codec::decode, Grid::<u8>::sized, store.widen, the chain's three,
     * err.downcast, store.fetch, wrap, inner, <S as P>::probe, and nothing else. */
    CHECK(sites == 12, "%d sites, not 12", sites);
    CHECK(in_loops == 1, "store.fetch is not two loops deep");
    CHECK(in_branch == 1, "err.downcast is not in the branch");
    CHECK(bound == 1, "inner::<u8>() is not locally bound");
    pdxe_result_free(ctx, rt);
    pdxe_result_free(ctx, rp);
    free(plain);
}

int main(void) {
    if (pdxe_init(&ctx) != PDXE_OK) {
        printf("FAIL the engine did not start\n");
        return 1;
    }
    check_twins();
    pdxe_shutdown(ctx);
    if (failures) {
        printf("%d failure(s)\n", failures);
        return 1;
    }
    printf("ok: a Rust turbofish call is its function's call\n");
    return 0;
}
