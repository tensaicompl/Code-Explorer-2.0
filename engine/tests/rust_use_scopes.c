/*
 * A Rust `use` binds only in its lexical scope (docs/plan/ISSUES.md, issue 114;
 * engine/patches/0031-rust-use-lexical-scope.patch).
 *
 *     rust_use_scopes
 *
 * The typed pass read the `use` declarations at the top of a file and in its inline
 * modules into one map, and answered every site of the file from it: a test module's
 * import of another `fetch` made the module's own call of its own `fetch` a typed
 * answer to the import. Now each `use` binds in the scope it is declared in (the file's
 * module, an inline module's body, a block), a lookup goes from the site's scope out to
 * the first module, and the extraction records the scopes, the imports declared inside
 * them and each call's scope, for Stage 3.
 *
 *   - The issue's reproduction: a call outside the test module reaches the file's own
 *     `fetch`, never the test module's import.
 *   - A macro is read in its invocation's scope: a built-in's arguments, and a
 *     `macro_rules!` expansion, which the typed pass walks from a buffer of its own.
 *   - The scopes, the imports inside them and each call's scope are recorded as the
 *     source gives them, and cross the interface.
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

typedef struct {
    pdxe_file_result *r;
    const PDXEFileResult *e;
} extraction_t;

static extraction_t extract(const char *rel, const char *source) {
    extraction_t x = {0};
    if (pdxe_extract_file(ctx, pdxe_language_id("rust"), rel, (const uint8_t *)source,
                          strlen(source), &x.r) != PDXE_OK ||
        !x.r) {
        printf("FAIL %s: the extraction failed\n", rel);
        exit(1);
    }
    x.e = pdxe_result_engine(x.r);
    return x;
}

static bool ends_with(const char *s, const char *suffix) {
    size_t n = s ? strlen(s) : 0, m = strlen(suffix);
    return n >= m && strcmp(s + n - m, suffix) == 0;
}

/* The per-file typed answers from the function named `caller` whose target ends with
 * `target`. */
static int answers(const PDXEFileResult *e, const char *caller, const char *target) {
    char suffix[64];
    snprintf(suffix, sizeof suffix, ".%s", caller);
    int n = 0;
    for (int i = 0; i < e->resolved_calls.count; i++) {
        const PDXEResolvedCall *c = &e->resolved_calls.items[i];
        n += ends_with(c->caller_qn, suffix) && ends_with(c->callee_qn, target);
    }
    return n;
}

/* The issue's reproduction, one file: the test module imports a `fetch` that is not
 * the file's own. */
static void check_reproduction(void) {
    extraction_t x = extract(
        "src/repro.rs", "pub fn fetch() -> u32 {\n    0\n}\n\npub fn call() -> u32 {\n    fetch()\n}\n\n"
                        "#[cfg(test)]\nmod tests {\n    use crate::one::fetch;\n\n    fn probe() -> u32 {\n"
                        "        fetch()\n    }\n}\n");
    CHECK(answers(x.e, "call", ".repro.fetch") == 1,
          "reproduction: `call` is not answered with the file's own `fetch`");
    CHECK(answers(x.e, "call", "one.fetch") == 0,
          "reproduction: the test module's import answered `call`");
    CHECK(answers(x.e, "probe", "one.fetch") == 1,
          "reproduction: the test module's own call is not read through its import");
    pdxe_result_free(ctx, x.r);
}

/* A macro is read where it is invoked: `f` and `h` import `AtomicU32` in their bodies,
 * `g` and `k` do not. */
static void check_macros(void) {
    extraction_t x = extract(
        "src/mac.rs",
        "macro_rules! fresh {\n    () => {\n        AtomicU32::new(0).load(std::sync::atomic::Ordering::"
        "SeqCst)\n    };\n}\n\npub fn f() -> u32 {\n    use std::sync::atomic::AtomicU32;\n    let c = "
        "AtomicU32::new(0);\n    assert_eq!(c.load(std::sync::atomic::Ordering::SeqCst), 0);\n    1\n}\n\n"
        "pub fn g() -> u32 {\n    let c = AtomicU32::new(0);\n    assert_eq!(c.load(std::sync::atomic::"
        "Ordering::SeqCst), 0);\n    1\n}\n\npub fn h() -> u32 {\n    use std::sync::atomic::AtomicU32;\n"
        "    fresh!()\n}\n\npub fn k() -> u32 {\n    fresh!()\n}\n");
    const char *load = "std.sync.atomic.AtomicU32.load";
    CHECK(answers(x.e, "f", load) == 1, "macros: `assert_eq!`'s argument in `f` is not answered");
    CHECK(answers(x.e, "g", load) == 0, "macros: `assert_eq!`'s argument in `g` is answered");
    CHECK(answers(x.e, "h", load) == 1, "macros: the expansion in `h` is not answered");
    CHECK(answers(x.e, "k", load) == 0, "macros: the expansion in `k` is answered");
    pdxe_result_free(ctx, x.r);
}

/* The scopes, the imports inside them and each call's scope: the file, `inner`,
 * `helper`'s body and the block in it. */
static void check_recorded(void) {
    const char *source =
        "use crate::a::x;\n\nmod inner {\n    use super::helper;\n\n    pub fn f() -> u32 {\n"
        "        helper()\n    }\n}\n\npub fn helper() -> u32 {\n    use crate::a::y;\n    let a = "
        "{\n        use crate::b::z;\n        z()\n    };\n    a + y() + x()\n}\n";
    extraction_t x = extract("src/lib.rs", source);
    const PDXEUseScopeArray *scopes = &x.e->use_scopes;
    static const struct {
        int parent;
        bool module;
    } SCOPES[] = {{-1, true}, {0, true}, {0, false}, {2, false}};
    CHECK(scopes->count == 4, "recorded: %d scopes, not 4", scopes->count);
    for (int i = 0; i < 4 && i < scopes->count; i++) {
        CHECK(scopes->items[i].parent == SCOPES[i].parent &&
                  scopes->items[i].module == SCOPES[i].module,
              "recorded: scope %d is (%d, %d)", i, scopes->items[i].parent,
              scopes->items[i].module);
    }
    static const struct {
        const char *text;
        int scope;
    } IMPORTS[] = {{"super::helper", 1}, {"crate::a::y", 2}, {"crate::b::z", 3}};
    const PDXEScopedImportArray *scoped = &x.e->scoped_imports;
    CHECK(scoped->count == 3, "recorded: %d scoped imports, not 3", scoped->count);
    for (int i = 0; i < 3 && i < scoped->count; i++) {
        CHECK(strcmp(scoped->items[i].import.module_path, IMPORTS[i].text) == 0 &&
                  scoped->items[i].scope == IMPORTS[i].scope,
              "recorded: scoped import %d is %s in %d", i, scoped->items[i].import.module_path,
              scoped->items[i].scope);
    }
    CHECK(x.e->imports.count == 1, "recorded: the file's module has %d imports",
          x.e->imports.count);
    static const struct {
        const char *callee;
        uint32_t scope;
    } CALLS[] = {{"helper", 1}, {"z", 3}, {"y", 2}, {"x", 2}};
    for (size_t k = 0; k < sizeof CALLS / sizeof CALLS[0]; k++) {
        int seen = 0;
        for (uint32_t i = 0; i < x.r->n_calls; i++) {
            const pdxe_call *c = &x.r->calls[i];
            if (c->is_reference || c->callee_text.kind != PDXE_TEXT_SOURCE ||
                c->callee_text.end - c->callee_text.start != strlen(CALLS[k].callee)) {
                continue;
            }
            if (strncmp(source + c->callee_text.start, CALLS[k].callee,
                        strlen(CALLS[k].callee)) != 0) {
                continue;
            }
            seen++;
            CHECK(c->use_scope == CALLS[k].scope, "recorded: `%s` is in scope %u, not %u",
                  CALLS[k].callee, c->use_scope, CALLS[k].scope);
        }
        CHECK(seen == 1, "recorded: %d calls of `%s`", seen, CALLS[k].callee);
    }
    CHECK(x.r->n_use_scopes == 4 && x.r->n_scoped_imports == 3 &&
              x.r->use_scopes[1].parent == 0 && x.r->use_scopes[1].module == 1 &&
              x.r->use_scopes[0].parent == PDXE_NO_PARENT &&
              x.r->scoped_imports[2].scope == 3 &&
              strcmp(x.r->scoped_imports[2].import.module_text, "crate::b::z") == 0,
          "recorded: the interface does not carry the scopes as the engine records them");
    pdxe_result_free(ctx, x.r);
}

int main(void) {
    if (pdxe_init(&ctx) != PDXE_OK) {
        printf("FAIL the engine did not start\n");
        return 1;
    }
    check_reproduction();
    check_macros();
    check_recorded();
    pdxe_shutdown(ctx);
    if (failures) {
        printf("%d failure(s)\n", failures);
        return 1;
    }
    printf("ok: a Rust `use` binds only in its lexical scope\n");
    return 0;
}
