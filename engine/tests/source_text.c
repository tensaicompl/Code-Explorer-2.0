/*
 * Text a fact takes from the file is a range of its source, never a copy, and a nested
 * definition's qualified name is given by its relation to an earlier definition's
 * (docs/plan/ISSUES.md, issue 90; decision 61).
 *
 *   - pdxe_text's layout;
 *   - ranges: a callee spelled out in the source, an argument, a Python attribute
 *     chain, a route's declaring node are ranges whose bytes are the text exactly;
 *   - owned: text read after C-family macro expansion, and a callee the engine names
 *     otherwise than the source spells it at the call;
 *   - qualified names: nested classes are members of their enclosing class, and C++
 *     functions in nested namespaces, which are no definitions, the prefix of an earlier
 *     function's name they share, each spelled out exactly; pdxe_qualified_name's fixed
 *     vectors, truncation and refusals;
 *   - parameter lists: a range of the source, nested functions as defaults included;
 *   - pdxe_result_build: ranges kept as ranges, owned text copied, and every malformed
 *     text and qualified name refused;
 *   - the surface: version 5, its ranges checked against the source's length it records
 *     when decoded, and bound only to a source of that length holding no NUL in them;
 *     definition references and structural names checked;
 *   - retained text: in the four shapes issue 90 measured (C arguments holding calls,
 *     JavaScript call chains, Python attribute chains, Java classes nested in classes),
 *     and the two its audit found (C++ namespaces with a function at every level,
 *     JavaScript functions as parameters' defaults), the text an extraction owns grows
 *     with the source, never with the square of the nesting.
 */

#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

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

/* One pointer and four 32-bit fields: no padding, and the same on every 64-bit target
 * the project builds for. */
_Static_assert(offsetof(pdxe_text, owned) == 0, "owned first");
_Static_assert(offsetof(pdxe_text, start) == sizeof(void *), "start after the pointer");
_Static_assert(offsetof(pdxe_text, end) == sizeof(void *) + 4, "end after start");
_Static_assert(offsetof(pdxe_text, kind) == sizeof(void *) + 8, "kind after end");
_Static_assert(offsetof(pdxe_text, reserved) == sizeof(void *) + 12, "reserved last");
_Static_assert(sizeof(pdxe_text) == sizeof(void *) + 16, "no padding");

static pdxe_ctx *ctx;

static pdxe_file_result *extract(const char *lang, const char *path, const char *source) {
    pdxe_file_result *r = NULL;
    int rc = pdxe_extract_file(ctx, pdxe_language_id(lang), path, (const uint8_t *)source,
                               strlen(source), &r);
    if (rc != PDXE_OK || !r) {
        printf("FAIL %s: extraction failed (%d)\n", path, rc);
        exit(1);
    }
    return r;
}

#define SRC(s) ((const uint8_t *)(s)), strlen(s)

/* The call whose text is `callee`, or NULL. */
static const pdxe_call *call_named(const pdxe_file_result *r, const char *source,
                                   const char *callee, int raw) {
    for (uint32_t i = 0; i < r->n_calls; i++) {
        const pdxe_call *c = &r->calls[i];
        int positioned = c->span.end_byte > c->span.start_byte;
        if (positioned == raw && tv_text_is(&c->callee_text, SRC(source), callee)) {
            return c;
        }
    }
    return NULL;
}

/* A range's bytes are the text, and lie where the fact is. */
static void check_range(const pdxe_text *t, const char *source, const char *text,
                        const char *what) {
    CHECK(t->kind == PDXE_TEXT_SOURCE && !t->owned && t->reserved == 0, "%s: not a range", what);
    CHECK(tv_text_is(t, SRC(source), text), "%s: the range reads %s, not %s", what,
          tv_text(t, SRC(source)), text);
}

static void check_owned(const pdxe_text *t, const char *text, const char *what) {
    CHECK(t->kind == PDXE_TEXT_OWNED && t->owned && t->start == 0 && t->end == 0 &&
              t->reserved == 0,
          "%s: not owned text", what);
    CHECK(t->owned && strcmp(t->owned, text) == 0, "%s: owns %s, not %s", what,
          t->owned ? t->owned : "(null)", text);
}

static void ranges_and_owned_text(void) {
    /* A JavaScript member chain: each link's callee is the chain before it. */
    static const char js[] = "function m() { return x.f().g().h(); }\n";
    pdxe_file_result *r = extract("javascript", "web/m.js", js);
    for (const char *const *callee = (const char *const[]){"x.f", "x.f().g", "x.f().g().h", NULL};
         *callee; callee++) {
        const pdxe_call *c = call_named(r, js, *callee, 1);
        CHECK(c != NULL, "js: no call of %s", *callee);
        if (c) {
            check_range(&c->callee_text, js, *callee, *callee);
            CHECK(c->callee_text.start == c->span.start_byte, "js: %s is not where its call is",
                  *callee);
        }
    }
    pdxe_result_free(ctx, r);

    /* C arguments holding calls, each argument its own range; macro-expanded text owned. */
    static const char c_src[] = "#define TWICE(v) twice((v) + 1)\n"
                                "int g(int a) { return f(a, f(a, h(a))) + TWICE(a); }\n";
    r = extract("c", "src/g.c", c_src);
    const pdxe_call *outer = NULL;
    for (uint32_t i = 0; i < r->n_calls; i++) {
        const pdxe_call *c = &r->calls[i];
        if (tv_text_is(&c->callee_text, SRC(c_src), "f") && c->n_args == 2 &&
            tv_text_is(&c->args[1].expr, SRC(c_src), "f(a, h(a))")) {
            outer = c;
        }
    }
    CHECK(outer != NULL, "c: no outer call of f");
    if (outer) {
        check_range(&outer->callee_text, c_src, "f", "c callee");
        check_range(&outer->args[0].expr, c_src, "a", "c first argument");
        check_range(&outer->args[1].expr, c_src, "f(a, h(a))", "c nested argument");
    }
    const pdxe_call *expanded = call_named(r, c_src, "twice", 0);
    CHECK(expanded != NULL, "c: no call the expanded text holds");
    if (expanded) {
        check_owned(&expanded->callee_text, "twice", "expanded callee");
        CHECK(expanded->n_args == 1, "expanded: %u arguments", expanded->n_args);
        if (expanded->n_args == 1) {
            /* The expanded text is the preprocessor's, its tokens spaced apart. */
            check_owned(&expanded->args[0].expr, "( a ) + 1", "expanded argument");
        }
    }
    pdxe_result_free(ctx, r);

    /* A Python attribute chain: each usage's name is the chain up to it. */
    static const char py[] = "def h():\n    v = a.b.c\n    return v\n";
    r = extract("python", "app/h.py", py);
    int chains = 0;
    for (uint32_t i = 0; i < r->n_usages; i++) {
        const pdxe_text *name = &r->usages[i].name;
        const char *text = tv_text(name, SRC(py));
        if (text && (strcmp(text, "a.b") == 0 || strcmp(text, "a.b.c") == 0)) {
            check_range(name, py, text, "python attribute");
            chains++;
        }
    }
    CHECK(chains == 2, "python: %d attribute chains, expected a.b and a.b.c", chains);
    pdxe_result_free(ctx, r);

    /* Captured arguments followed by a splat: the array holds the captured ones only,
     * and a splat after the last takes no slot. Writing one for it overran the array
     * into the next allocation, the keywords' text, which no sanitizer sees inside an
     * arena block; 15 files of the census showed it. */
    static const char splat[] = "def f():\n    return g(e, a1=a1, a2=a2, a3=a3, a4=a4, **rest)\n";
    r = extract("python", "app/f.py", splat);
    const pdxe_call *g = call_named(r, splat, "g", 1);
    CHECK(g && g->n_args == 5, "splat: %u arguments", g ? g->n_args : 0);
    for (uint32_t k = 1; g && k < g->n_args; k++) {
        char kw[16];
        snprintf(kw, sizeof(kw), "a%u", k);
        CHECK(g->args[k].keyword && strcmp(g->args[k].keyword, kw) == 0,
              "splat: argument %u's keyword is %s", k,
              g->args[k].keyword ? g->args[k].keyword : "(null)");
        check_range(&g->args[k].expr, splat, kw, "splat argument");
    }
    pdxe_result_free(ctx, r);

    /* A route's declaring node; a constructor's callee, named at the `new`, owned. */
    static const char java[] =
        "package p;\nimport org.springframework.web.bind.annotation.*;\n@RestController\n"
        "class C {\n  @GetMapping(\"/x\")\n  String x() { throw new E(\"m\"); }\n}\n";
    r = extract("java", "src/main/java/p/C.java", java);
    int routes = 0;
    for (uint32_t i = 0; i < r->n_defs; i++) {
        for (uint32_t k = 0; k < r->defs[i].n_routes; k++) {
            check_range(&r->defs[i].routes[k].source_text, java, "@GetMapping(\"/x\")",
                        "route source");
            routes++;
        }
    }
    CHECK(routes == 1, "java: %d routes", routes);
    const pdxe_call *made = call_named(r, java, "E", 1);
    CHECK(made != NULL, "java: no constructor call");
    if (made) {
        check_owned(&made->callee_text, "E", "constructor callee");
        check_range(&made->args[0].expr, java, "\"m\"", "constructor argument");
    }
    pdxe_result_free(ctx, r);
}

static void nested_qualified_names(void) {
    static const char java[] = "package p;\nclass A { class B { class C { void m() { f(); } } } }\n";
    pdxe_file_result *r = extract("java", "src/main/java/p/A.java", java);
    /* The file's module is `src.main.java.p.A`, the name its class takes too. */
    static const char *const expected[][2] = {
        {"A", "src.main.java.p.A"},
        {"B", "src.main.java.p.A.B"},
        {"C", "src.main.java.p.A.B.C"},
        {"m", "src.main.java.p.A.B.C.m"},
    };
    for (size_t e = 0; e < sizeof(expected) / sizeof(expected[0]); e++) {
        int found = 0;
        for (uint32_t i = 0; i < r->n_defs; i++) {
            if (strcmp(r->defs[i].name, expected[e][0]) != 0) {
                continue;
            }
            found = 1;
            const char *qn = tv_qn(r, i);
            CHECK(qn && strcmp(qn, expected[e][1]) == 0, "java %s: %s, not %s", expected[e][0],
                  qn ? qn : "(none)", expected[e][1]);
            if (strcmp(expected[e][0], "A") != 0) {
                CHECK(r->defs[i].qn_form == PDXE_QN_MEMBER && !r->defs[i].qn_text &&
                          r->defs[i].qn_base < i,
                      "java %s: not a member of an earlier definition", expected[e][0]);
            }
        }
        CHECK(found, "java: no definition %s", expected[e][0]);
    }
    pdxe_result_free(ctx, r);

    /* A C++ function in nested namespaces, which are no definitions of their own: its
     * name is the module's extended by them, never a member of anything. */
    static const char cpp[] = "namespace geo { namespace detail { int f() { return 1; } } }\n";
    r = extract("cpp", "src/n.cpp", cpp);
    int functions = 0;
    for (uint32_t i = 0; i < r->n_defs; i++) {
        if (strcmp(r->defs[i].name, "f") != 0) {
            continue;
        }
        functions++;
        const char *qn = tv_qn(r, i);
        CHECK(qn && strcmp(qn, "src.n.geo.detail.f") == 0, "cpp f: %s", qn ? qn : "(none)");
        CHECK(r->defs[i].qn_form == PDXE_QN_EXTENDED && r->defs[i].qn_text &&
                  strcmp(r->defs[i].qn_text, ".geo.detail.f") == 0,
              "cpp f: not the module's name extended");
    }
    CHECK(functions == 1, "cpp: %d functions f", functions);
    pdxe_result_free(ctx, r);

    /* Functions at every level of nested namespaces: each name is the prefix of an
     * earlier function's it shares, the namespaces' names, then its own. Never the
     * module's name extended by the whole path, which repeats it at every level. */
    static const char each[] = "namespace a { int f() { return 0; }\n"
                               "namespace b { int g() { return 0; } int g2() { return 0; }\n"
                               "namespace c { int h() { return 0; } } } }\n"
                               "namespace a { int f2() { return 0; } }\n";
    r = extract("cpp", "src/n.cpp", each);
    static const struct {
        const char *name;
        const char *qn;
        uint32_t form;
        const char *base;
        uint32_t keep;
        const char *text;
    } shared[] = {
        {"f", "src.n.a.f", PDXE_QN_EXTENDED, "src/n.cpp", 0, ".a.f"},
        {"g", "src.n.a.b.g", PDXE_QN_PREFIX, "f", 7, ".b.g"},
        {"g2", "src.n.a.b.g2", PDXE_QN_PREFIX, "g", 9, ".g2"},
        {"h", "src.n.a.b.c.h", PDXE_QN_PREFIX, "g", 9, ".c.h"},
        {"f2", "src.n.a.f2", PDXE_QN_PREFIX, "f", 7, ".f2"},
    };
    for (size_t e = 0; e < sizeof(shared) / sizeof(shared[0]); e++) {
        int found = 0;
        for (uint32_t i = 0; i < r->n_defs; i++) {
            const pdxe_definition *d = &r->defs[i];
            if (strcmp(d->name, shared[e].name) != 0) {
                continue;
            }
            found++;
            const char *qn = tv_qn(r, i);
            CHECK(qn && strcmp(qn, shared[e].qn) == 0, "cpp %s: %s, not %s", shared[e].name,
                  qn ? qn : "(none)", shared[e].qn);
            CHECK(d->qn_form == shared[e].form && d->qn_base < i &&
                      strcmp(r->defs[d->qn_base].name, shared[e].base) == 0 &&
                      d->qn_keep == shared[e].keep && d->qn_text &&
                      strcmp(d->qn_text, shared[e].text) == 0,
                  "cpp %s: form %u base %s keep %u text %s", shared[e].name, d->qn_form,
                  d->qn_base < r->n_defs ? r->defs[d->qn_base].name : "(none)", d->qn_keep,
                  d->qn_text ? d->qn_text : "(none)");
        }
        CHECK(found == 1, "cpp: %d definitions %s", found, shared[e].name);
    }
    pdxe_result_free(ctx, r);
}

/* A parameter list is a range of the source: nested functions given as parameters'
 * defaults are no copies of one another (issue 90). After C macro expansion it is owned. */
static void parameter_lists(void) {
    static const char js[] = "function f0(a = function f1(b = function f2() {}) {}) {}\n";
    pdxe_file_result *r = extract("javascript", "web/d.js", js);
    static const char *const want[][2] = {
        {"f0", "(a = function f1(b = function f2() {}) {})"},
        {"f1", "(b = function f2() {})"},
        {"f2", "()"},
    };
    for (size_t e = 0; e < sizeof(want) / sizeof(want[0]); e++) {
        int found = 0;
        for (uint32_t i = 0; i < r->n_defs; i++) {
            if (strcmp(r->defs[i].name, want[e][0]) == 0) {
                found++;
                check_range(&r->defs[i].signature, js, want[e][1], want[e][0]);
            }
        }
        CHECK(found == 1, "js: %d definitions %s", found, want[e][0]);
    }
    pdxe_result_free(ctx, r);

    /* A method only the macro-expanded text gives (the raw parse misreads its class)
     * holds the expanded text's parameter list; what the raw parse reads holds ranges. */
    static const char cpp[] = "#define PANEL_API\n#define TWICE(v) ((v) + (v))\n"
                              "int measure_panel(long y) { return 2; }\n"
                              "class PANEL_API Panel : public Base {\n public:\n"
                              "  int size() const { return measure_panel(1); }\n};\n";
    r = extract("cpp", "src/panel.cpp", cpp);
    int seen = 0;
    for (uint32_t i = 0; i < r->n_defs; i++) {
        const pdxe_definition *d = &r->defs[i];
        if (strcmp(d->name, "size") == 0) {
            check_owned(&d->signature, "( )", "the adopted size's parameters");
            seen++;
        } else if (strcmp(d->name, "measure_panel") == 0) {
            check_range(&d->signature, cpp, "(long y)", "measure_panel's parameters");
            seen++;
        } else if (strcmp(d->name, "TWICE") == 0) {
            check_range(&d->signature, cpp, "(v)", "the macro's parameters");
            seen++;
        }
    }
    CHECK(seen == 3, "cpp: %d of the three definitions", seen);
    pdxe_result_free(ctx, r);
}

static void qualified_name_vectors(void) {
    pdxe_definition defs[5];
    memset(defs, 0, sizeof(defs));
    defs[0] = (pdxe_definition){.name = "b", .qn_text = "a.b", .qn_base = PDXE_NO_PARENT,
                                .qn_form = PDXE_QN_WHOLE};
    defs[1] = (pdxe_definition){.name = "C", .qn_base = 0, .qn_form = PDXE_QN_MEMBER};
    defs[2] = (pdxe_definition){.name = "D", .qn_text = "$D", .qn_base = 1,
                                .qn_form = PDXE_QN_EXTENDED};
    defs[3] = (pdxe_definition){.name = "e", .qn_base = 2, .qn_form = PDXE_QN_MEMBER};
    defs[4] = (pdxe_definition){.name = "f", .qn_text = "::f", .qn_base = 0,
                                .qn_form = PDXE_QN_EXTENDED};
    static const char *const want[] = {"a.b", "a.b.C", "a.b.C$D", "a.b.C$D.e", "a.b::f"};
    for (uint32_t i = 0; i < 5; i++) {
        char buf[64];
        size_t len = 0;
        CHECK(pdxe_qualified_name(defs, 5, i, buf, sizeof(buf), &len) == PDXE_OK &&
                  len == strlen(want[i]) && strcmp(buf, want[i]) == 0,
              "vector %u: %s, not %s", i, buf, want[i]);
    }
    char small[5];
    size_t len = 0;
    CHECK(pdxe_qualified_name(defs, 5, 3, small, sizeof(small), &len) == PDXE_OK && len == 9 &&
              strcmp(small, "a.b.") == 0,
          "a short buffer holds %s and the length %zu", small, len);
    CHECK(pdxe_qualified_name(defs, 5, 3, NULL, 0, &len) == PDXE_OK && len == 9,
          "the length alone is %zu", len);
    CHECK(pdxe_qualified_name(defs, 5, 5, small, sizeof(small), &len) == PDXE_E_INVALID,
          "an index past the definitions");
    pdxe_definition bad[2];
    memcpy(bad, defs, sizeof(bad));
    bad[1].qn_base = 1; /* not earlier than itself */
    CHECK(pdxe_qualified_name(bad, 2, 1, small, sizeof(small), &len) == PDXE_E_INVALID,
          "a base that is not earlier");
    memcpy(bad, defs, sizeof(bad));
    bad[1].qn_form = 7;
    CHECK(pdxe_qualified_name(bad, 2, 1, small, sizeof(small), &len) == PDXE_E_INVALID,
          "an unknown form");
    memcpy(bad, defs, sizeof(bad));
    bad[1].name = NULL;
    CHECK(pdxe_qualified_name(bad, 2, 1, small, sizeof(small), &len) == PDXE_E_INVALID,
          "a member with no name");

    /* Prefixes: the first `keep` bytes of an earlier name, ending before a `.` the
     * earlier definition adds itself, then the rest. */
    pdxe_definition pre[5];
    memset(pre, 0, sizeof(pre));
    pre[0] = (pdxe_definition){.name = "m", .qn_text = "m", .qn_base = PDXE_NO_PARENT,
                               .qn_form = PDXE_QN_WHOLE};
    pre[1] = (pdxe_definition){.name = "f", .qn_text = ".a.f", .qn_base = 0,
                               .qn_form = PDXE_QN_EXTENDED};
    pre[2] = (pdxe_definition){.name = "g", .qn_text = ".b.g", .qn_base = 1, .qn_keep = 3,
                               .qn_form = PDXE_QN_PREFIX};
    pre[3] = (pdxe_definition){.name = "x", .qn_base = 2, .qn_form = PDXE_QN_MEMBER};
    pre[4] = (pdxe_definition){.name = "c", .qn_text = ".c", .qn_base = 2, .qn_keep = 5,
                               .qn_form = PDXE_QN_PREFIX};
    static const char *const pre_want[] = {"m", "m.a.f", "m.a.b.g", "m.a.b.g.x", "m.a.b.c"};
    for (uint32_t i = 0; i < 5; i++) {
        char buf[64];
        size_t n = 0;
        CHECK(pdxe_qualified_name(pre, 5, i, buf, sizeof(buf), &n) == PDXE_OK &&
                  n == strlen(pre_want[i]) && strcmp(buf, pre_want[i]) == 0,
              "prefix vector %u: %s, not %s", i, buf, pre_want[i]);
    }
    char six[6];
    CHECK(pdxe_qualified_name(pre, 5, 3, six, sizeof(six), &len) == PDXE_OK && len == 9 &&
              strcmp(six, "m.a.b") == 0,
          "a short buffer holds %s of a name through a prefix, length %zu", six, len);
    CHECK(pdxe_qualified_name(pre, 5, 4, NULL, 0, &len) == PDXE_OK && len == 7,
          "a prefix's length alone is %zu", len);
    const struct {
        const char *what;
        uint32_t keep;
        const char *text;
        uint32_t base;
    } refused[] = {
        {"a keep that is no `.`", 4, ".c", 2},
        {"a keep at the base's length", 7, ".c", 2},
        {"a keep past the base", 9, ".c", 2},
        {"a keep in the bytes the base takes from its own base", 3, ".c", 2},
        {"a keep of nothing", 0, ".c", 2},
        {"a rest not after a `.`", 5, "c", 2},
        {"a base that is not earlier", 5, ".c", 4},
        {"a keep in a member's name", 7, ".c", 3},
    };
    for (size_t i = 0; i < sizeof(refused) / sizeof(refused[0]); i++) {
        pdxe_definition bent[5];
        memcpy(bent, pre, sizeof(bent));
        bent[4].qn_keep = refused[i].keep;
        bent[4].qn_text = refused[i].text;
        bent[4].qn_base = refused[i].base;
        CHECK(pdxe_qualified_name(bent, 5, 4, small, sizeof(small), &len) == PDXE_E_INVALID &&
                  pdxe_qualified_name(bent, 5, 4, NULL, 0, &len) == PDXE_E_INVALID,
              "%s was spelled", refused[i].what);
    }
}

/* pdxe_result_build with one call whose callee is `callee`. */
static int build_with_callee(pdxe_text callee, size_t source_len, pdxe_file_result **out) {
    pdxe_call call;
    memset(&call, 0, sizeof(call));
    call.callee_text = callee;
    call.caller_index = PDXE_NO_PARENT;
    return pdxe_result_build(ctx, source_len, NULL, 0, &call, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                             NULL, 0, out);
}

static pdxe_text range(uint32_t start, uint32_t end) {
    return (pdxe_text){NULL, start, end, PDXE_TEXT_SOURCE, 0};
}

static pdxe_text owned(const char *s) {
    return (pdxe_text){s, 0, 0, PDXE_TEXT_OWNED, 0};
}

static void build_keeps_and_refuses(void) {
    pdxe_file_result *r = NULL;
    /* Accepted: a range stays the same range; owned text is copied. */
    CHECK(build_with_callee(range(2, 5), 10, &r) == PDXE_OK && r &&
              r->calls[0].callee_text.kind == PDXE_TEXT_SOURCE &&
              r->calls[0].callee_text.start == 2 && r->calls[0].callee_text.end == 5,
          "a range was not kept as the same range");
    pdxe_result_free(ctx, r);
    r = NULL;
    CHECK(build_with_callee(range(10, 10), 10, &r) == PDXE_OK && r,
          "an empty range at the source's end was refused");
    pdxe_result_free(ctx, r);
    r = NULL;
    static const char name[] = "named";
    CHECK(build_with_callee(owned(name), 10, &r) == PDXE_OK && r &&
              r->calls[0].callee_text.kind == PDXE_TEXT_OWNED &&
              r->calls[0].callee_text.owned != name &&
              strcmp(r->calls[0].callee_text.owned, name) == 0,
          "owned text was not copied");
    pdxe_result_free(ctx, r);

    /* Refused: each malformed text, and no text where text is required. */
    const struct {
        const char *what;
        pdxe_text t;
    } refused[] = {
        {"an unknown kind", {NULL, 0, 0, 3, 0}},
        {"another unknown kind", {NULL, 0, 0, 255, 0}},
        {"a reserved field set", {NULL, 1, 2, PDXE_TEXT_SOURCE, 1}},
        {"a range starting past its end", range(5, 4)},
        {"a range past the source", range(4, 11)},
        {"a range with a string", {"x", 1, 2, PDXE_TEXT_SOURCE, 0}},
        {"owned text with no string", {NULL, 0, 0, PDXE_TEXT_OWNED, 0}},
        {"owned text with a range", {"x", 1, 2, PDXE_TEXT_OWNED, 0}},
        {"no text for a callee", {NULL, 0, 0, PDXE_TEXT_NONE, 0}},
        {"no text with a range", {NULL, 1, 2, PDXE_TEXT_NONE, 0}},
    };
    for (size_t i = 0; i < sizeof(refused) / sizeof(refused[0]); i++) {
        r = NULL;
        CHECK(build_with_callee(refused[i].t, 10, &r) == PDXE_E_INVALID && !r,
              "%s was accepted", refused[i].what);
    }

    /* An argument and a usage need their text too; a route's declaring node may have
     * none. */
    pdxe_call call;
    memset(&call, 0, sizeof(call));
    call.callee_text = owned("f");
    call.caller_index = PDXE_NO_PARENT;
    pdxe_call_arg arg = {{NULL, 0, 0, PDXE_TEXT_NONE, 0}, NULL, NULL, 0};
    call.args = &arg;
    call.n_args = 1;
    r = NULL;
    CHECK(pdxe_result_build(ctx, 10, NULL, 0, &call, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                            &r) == PDXE_E_INVALID && !r,
          "an argument with no expression was accepted");
    arg.expr = range(0, 3);
    CHECK(pdxe_result_build(ctx, 10, NULL, 0, &call, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                            &r) == PDXE_OK && r && r->calls[0].args[0].expr.start == 0 &&
              r->calls[0].args[0].expr.end == 3,
          "an argument's range was not kept");
    pdxe_result_free(ctx, r);
    pdxe_usage usage;
    memset(&usage, 0, sizeof(usage));
    usage.scope_index = PDXE_NO_PARENT;
    r = NULL;
    CHECK(pdxe_result_build(ctx, 10, NULL, 0, NULL, 0, NULL, 0, &usage, 1, NULL, 0, NULL, 0, NULL, 0,
                            &r) == PDXE_E_INVALID && !r,
          "a usage with no name was accepted");
    usage.name = range(9, 12);
    CHECK(pdxe_result_build(ctx, 10, NULL, 0, NULL, 0, NULL, 0, &usage, 1, NULL, 0, NULL, 0, NULL, 0,
                            &r) == PDXE_E_INVALID && !r,
          "a usage's range past the source was accepted");

    pdxe_definition def;
    memset(&def, 0, sizeof(def));
    def.name = "m";
    def.qn_text = "p.m";
    def.qn_base = PDXE_NO_PARENT;
    def.qn_form = PDXE_QN_WHOLE;
    def.kind = "function";
    def.engine_kind = "Function";
    def.parent_index = PDXE_NO_PARENT;
    pdxe_route route = {"GET", "/x", "get", {NULL, 0, 0, PDXE_TEXT_NONE, 0}, {0}, NULL, 0};
    def.routes = &route;
    def.n_routes = 1;
    r = NULL;
    CHECK(pdxe_result_build(ctx, 10, &def, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                            &r) == PDXE_OK && r &&
              r->defs[0].routes[0].source_text.kind == PDXE_TEXT_NONE,
          "a route with no declaring text was refused");
    pdxe_result_free(ctx, r);
    route.source_text = range(3, 2);
    r = NULL;
    CHECK(pdxe_result_build(ctx, 10, &def, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                            &r) == PDXE_E_INVALID && !r,
          "a route's malformed range was accepted");
    def.n_routes = 0;
    def.routes = NULL;

    /* Qualified names: each form well formed, or refused. */
    pdxe_definition two[2];
    two[0] = def;
    two[1] = def;
    two[1].name = "n";
    const struct {
        const char *what;
        const char *text;
        uint32_t base;
        uint32_t form;
        int ok;
    } names[] = {
        {"a member", NULL, 0, PDXE_QN_MEMBER, 1},
        {"an extension", "$n", 0, PDXE_QN_EXTENDED, 1},
        {"a member of itself", NULL, 1, PDXE_QN_MEMBER, 0},
        {"a member of a later definition", NULL, 2, PDXE_QN_MEMBER, 0},
        {"a member with text", "x", 0, PDXE_QN_MEMBER, 0},
        {"an extension with no text", NULL, 0, PDXE_QN_EXTENDED, 0},
        {"a whole name with a base", "p.n", 0, PDXE_QN_WHOLE, 0},
        {"a whole name with no text", NULL, PDXE_NO_PARENT, PDXE_QN_WHOLE, 0},
        {"an unknown form", "p.n", PDXE_NO_PARENT, 4, 0},
    };
    for (size_t i = 0; i < sizeof(names) / sizeof(names[0]); i++) {
        two[1].qn_text = names[i].text;
        two[1].qn_base = names[i].base;
        two[1].qn_form = names[i].form;
        r = NULL;
        int rc = pdxe_result_build(ctx, 10, two, 2, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                                   NULL, 0, &r);
        CHECK(names[i].ok ? rc == PDXE_OK && r : rc == PDXE_E_INVALID && !r, "%s: %s",
              names[i].what, names[i].ok ? "refused" : "accepted");
        pdxe_result_free(ctx, r);
    }

    /* A prefix: `p.m`, `p.m.a.f` (extended), then a third name from `p.m.a.f`'s first
     * `keep` bytes. Only a keep before a `.` its base adds itself is accepted, and only
     * a prefix has one. */
    pdxe_definition three[3];
    three[0] = def;
    three[1] = def;
    three[1].name = "f";
    three[1].qn_text = ".a.f";
    three[1].qn_base = 0;
    three[1].qn_form = PDXE_QN_EXTENDED;
    three[2] = def;
    three[2].name = "g";
    three[2].qn_base = 1;
    three[2].qn_form = PDXE_QN_PREFIX;
    const struct {
        const char *what;
        const char *text;
        uint32_t keep;
        uint32_t form;
        int ok;
        const char *qn;
    } prefixes[] = {
        {"a prefix", ".g", 5, PDXE_QN_PREFIX, 1, "p.m.a.g"},
        {"a prefix with no rest", "", 5, PDXE_QN_PREFIX, 1, "p.m.a"},
        {"a keep that is no `.`", ".g", 4, PDXE_QN_PREFIX, 0, NULL},
        {"a keep in the base's own base", ".g", 3, PDXE_QN_PREFIX, 0, NULL},
        {"a keep at the base's length", ".g", 7, PDXE_QN_PREFIX, 0, NULL},
        {"a keep of nothing", ".g", 0, PDXE_QN_PREFIX, 0, NULL},
        {"a rest not after a `.`", "g", 5, PDXE_QN_PREFIX, 0, NULL},
        {"a prefix with no text", NULL, 5, PDXE_QN_PREFIX, 0, NULL},
        {"an extension with a keep", ".g", 5, PDXE_QN_EXTENDED, 0, NULL},
    };
    for (size_t i = 0; i < sizeof(prefixes) / sizeof(prefixes[0]); i++) {
        three[2].qn_text = prefixes[i].text;
        three[2].qn_keep = prefixes[i].keep;
        three[2].qn_form = prefixes[i].form;
        r = NULL;
        int rc = pdxe_result_build(ctx, 10, three, 3, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                                   NULL, 0, &r);
        if (prefixes[i].ok) {
            const char *qn = rc == PDXE_OK && r ? tv_qn(r, 2) : NULL;
            CHECK(qn && strcmp(qn, prefixes[i].qn) == 0 && r->defs[2].qn_keep == prefixes[i].keep,
                  "%s: %s", prefixes[i].what, qn ? qn : "refused");
        } else {
            CHECK(rc == PDXE_E_INVALID && !r, "%s: accepted", prefixes[i].what);
        }
        pdxe_result_free(ctx, r);
    }

    /* A parameter list: a range kept, owned text copied, a malformed one refused. */
    def.signature = range(1, 4);
    r = NULL;
    CHECK(pdxe_result_build(ctx, 10, &def, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                            &r) == PDXE_OK && r && r->defs[0].signature.kind == PDXE_TEXT_SOURCE &&
              r->defs[0].signature.start == 1 && r->defs[0].signature.end == 4,
          "a parameter list's range was not kept");
    pdxe_result_free(ctx, r);
    static const char params[] = "(int x)";
    def.signature = owned(params);
    r = NULL;
    CHECK(pdxe_result_build(ctx, 10, &def, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0,
                            &r) == PDXE_OK && r && r->defs[0].signature.owned != params &&
              strcmp(r->defs[0].signature.owned, params) == 0,
          "an owned parameter list was not copied");
    pdxe_result_free(ctx, r);
    const pdxe_text bad_params[] = {range(4, 11), range(5, 4), {NULL, 0, 0, PDXE_TEXT_OWNED, 0},
                                    {NULL, 0, 0, 9, 0}};
    for (size_t i = 0; i < sizeof(bad_params) / sizeof(bad_params[0]); i++) {
        def.signature = bad_params[i];
        r = NULL;
        CHECK(pdxe_result_build(ctx, 10, &def, 1, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL,
                                0, &r) == PDXE_E_INVALID && !r,
              "malformed parameter list %zu was accepted", i);
    }
    def.signature = (pdxe_text){NULL, 0, 0, PDXE_TEXT_NONE, 0};
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

static int decode(const char *bytes, PDXEFileResult **r, pdxe_surface_texts **texts) {
    char *path = NULL;
    int lang = 0;
    uint32_t lost = 0;
    *r = NULL;
    *texts = NULL;
    int rc = bytes ? pdxe_surface_decode((const uint8_t *)bytes, strlen(bytes), r, &path, &lang,
                                         &lost, texts)
                   : PDXE_E_INVALID;
    free(path);
    return rc;
}

static void expect_refused(const char *what, char *bytes) {
    PDXEFileResult *r = NULL;
    pdxe_surface_texts *texts = NULL;
    CHECK(bytes && decode(bytes, &r, &texts) == PDXE_E_INVALID && !r && !texts,
          "a surface %s was accepted", what);
    if (r) {
        pdxe_free_result(r);
    }
    pdxe_surface_texts_free(texts);
    free(bytes);
}

static void surface_ranges(void) {
    /* A nested class (a structural name), its method calling a member chain (a range),
     * an argument (a range) and a name its definition's (a reference). */
    static const char py[] = "class A:\n    class B:\n        def m(self):\n"
                             "            return self.x.y(z)\n";
    pdxe_file_result *r = extract("python", "app/a.py", py);
    uint8_t *surface = NULL;
    size_t surface_len = 0;
    CHECK(pdxe_surface_export(r, "app/a.py", &surface, &surface_len) == PDXE_OK,
          "no surface");
    char *good = malloc(surface_len + 1);
    memcpy(good, surface, surface_len);
    good[surface_len] = '\0';
    pdxe_surface_free(surface);
    pdxe_result_free(ctx, r);
    CHECK(strncmp(good, "{\"v\":5,", 7) == 0, "the surface is not version 5: %.12s", good);
    char len_key[64];
    snprintf(len_key, sizeof(len_key), "\"source_len\":%zu,", strlen(py));
    CHECK(strstr(good, len_key) != NULL, "the surface does not record the source's length");
    CHECK(strstr(good, "\"callee_name\":[") != NULL, "no callee held as a range");
    CHECK(strstr(good, "\"qualified_name\":[") != NULL, "no qualified name held structurally");
    CHECK(strstr(good, "\"enclosing_func_qn\":2") != NULL ||
              strstr(good, "\"enclosing_func_qn\":3") != NULL,
          "no name held as its definition");
    CHECK(strstr(good, "self.x.y") == NULL, "a range's text was copied into the surface");

    /* Decoded, its texts are bound to a source of the length it records, only. */
    PDXEFileResult *decoded = NULL;
    pdxe_surface_texts *texts = NULL;
    CHECK(decode(good, &decoded, &texts) == PDXE_OK && decoded && texts, "the surface does not decode");
    if (decoded) {
        CHECK(pdxe_surface_bind(decoded, texts, py, strlen(py) - 1) == PDXE_E_INVALID,
              "a surface bound to a source of another length");
        pdxe_free_result(decoded);
    }
    CHECK(decode(good, &decoded, &texts) == PDXE_OK, "the surface does not decode again");
    if (decoded) {
        char *holed = strdup(py);
        char *at = strstr(holed, "self.x.y");
        at[5] = '\0'; /* a NUL inside a range: not this source */
        CHECK(pdxe_surface_bind(decoded, texts, holed, strlen(py)) == PDXE_E_INVALID,
              "a surface bound to a source with a NUL in a range");
        free(holed);
        pdxe_free_result(decoded);
    }
    CHECK(decode(good, &decoded, &texts) == PDXE_OK, "the surface does not decode a third time");
    if (decoded) {
        CHECK(pdxe_surface_bind(decoded, texts, py, strlen(py)) == PDXE_OK,
              "the surface does not bind to its own source");
        int read = 0;
        for (int i = 0; i < decoded->calls.count; i++) {
            const char *callee = decoded->calls.items[i].callee_name;
            read += callee && strcmp(callee, "self.x.y") == 0;
        }
        CHECK(read == 1, "the bound callee was not read from the source");
        pdxe_free_result(decoded);
    }

    /* Refused: another version; a length that a range passes; a range past its end or
     * starting past it; a definition reference or a base past the definitions. */
    expect_refused("of version 4", replace(good, "\"v\":5", "\"v\":4"));
    expect_refused("without the source's length", replace(good, len_key, ""));
    expect_refused("whose length is shorter than a range",
                   replace(good, len_key, "\"source_len\":3,"));
    const char *range_at = strstr(good, "\"callee_name\":[");
    if (range_at) {
        unsigned a = 0, b = 0;
        sscanf(range_at, "\"callee_name\":[%u,%u]", &a, &b);
        char from[64], to[64];
        snprintf(from, sizeof(from), "\"callee_name\":[%u,%u]", a, b);
        snprintf(to, sizeof(to), "\"callee_name\":[%u,%u]", b + 1, b);
        expect_refused("with a range starting past its end", replace(good, from, to));
        snprintf(to, sizeof(to), "\"callee_name\":[%u,%zu]", a, strlen(py) + 1);
        expect_refused("with a range past the source", replace(good, from, to));
        snprintf(to, sizeof(to), "\"callee_name\":[%u]", a);
        expect_refused("with a range of one number", replace(good, from, to));
        snprintf(to, sizeof(to), "\"callee_name\":[-1,%u]", b);
        expect_refused("with a negative range", replace(good, from, to));
    }
    expect_refused("naming a definition it does not have",
                   replace(good, "\"enclosing_func_qn\":", "\"enclosing_func_qn\":99"));
    expect_refused("whose name is given by a later definition",
                   replace(good, "\"qualified_name\":[", "\"qualified_name\":[9"));
    free(good);
}

/* The surface holds a prefix as `[base, "rest", keep]` and a parameter list as its range;
 * decoding spells each name out and refuses a keep, a rest or a range that does not fit. */
static void surface_prefixes(void) {
    static const char cpp[] = "namespace a { int f() { return 0; }\n"
                              "namespace b { int g(int x) { return x; } } }\n";
    pdxe_file_result *r = extract("cpp", "src/n.cpp", cpp);
    uint8_t *surface = NULL;
    size_t surface_len = 0;
    CHECK(pdxe_surface_export(r, "src/n.cpp", &surface, &surface_len) == PDXE_OK, "no surface");
    pdxe_result_free(ctx, r);
    char *good = malloc(surface_len + 1);
    memcpy(good, surface, surface_len);
    good[surface_len] = '\0';
    pdxe_surface_free(surface);
    const char *at = strstr(good, "\".b.g\",");
    unsigned keep = 0, base = 0;
    CHECK(at && sscanf(at, "\".b.g\",%u]", &keep) == 1 && keep > 0,
          "g is not held as a prefix: %s", good);
    if (!at || keep == 0) {
        free(good);
        return;
    }
    const char *open = at;
    while (open > good && *open != '[') {
        open--;
    }
    CHECK(sscanf(open, "[%u,", &base) == 1, "g's prefix has no base");
    CHECK(strstr(good, "\"signature\":null,\"signature_start\":") != NULL &&
              strstr(good, "(int x)") == NULL,
          "a parameter list was copied into the surface");

    PDXEFileResult *decoded = NULL;
    pdxe_surface_texts *texts = NULL;
    CHECK(decode(good, &decoded, &texts) == PDXE_OK && decoded, "the surface does not decode");
    if (decoded) {
        int spelled = 0;
        for (int i = 0; i < decoded->defs.count; i++) {
            const PDXEDefinition *d = &decoded->defs.items[i];
            const char *want = strcmp(d->name, "g") == 0   ? "src.n.a.b.g"
                               : strcmp(d->name, "f") == 0 ? "src.n.a.f"
                                                           : NULL;
            if (want && d->qualified_name) {
                size_t n = strlen(d->qualified_name), w = strlen(want);
                spelled += n >= w && strcmp(d->qualified_name + n - w, want) == 0;
            }
            if (strcmp(d->name, "g") == 0) {
                CHECK(!d->signature && d->signature_end > d->signature_start &&
                          d->signature_end - d->signature_start == 7 &&
                          memcmp(cpp + d->signature_start, "(int x)", 7) == 0,
                      "g's parameter list is not its range");
            }
        }
        CHECK(spelled == 2, "the decoded names are not spelled out: %d", spelled);
        pdxe_free_result(decoded);
    }
    pdxe_surface_texts_free(texts);

    char from[64], to[64];
    snprintf(from, sizeof(from), "\".b.g\",%u]", keep);
    snprintf(to, sizeof(to), "\".b.g\",%u]", keep - 1);
    expect_refused("whose prefix ends at no `.`", replace(good, from, to));
    snprintf(to, sizeof(to), "\".b.g\",%u]", keep + 1000);
    expect_refused("whose prefix is longer than its base", replace(good, from, to));
    snprintf(to, sizeof(to), "\"b.g\",%u]", keep);
    expect_refused("whose prefix's rest follows no `.`", replace(good, from, to));
    snprintf(to, sizeof(to), "\".b.g\",%u,1]", keep);
    expect_refused("whose name has four parts", replace(good, from, to));
    snprintf(to, sizeof(to), "\".b.g\",\"%u\"]", keep);
    expect_refused("whose keep is no number", replace(good, from, to));
    /* The first parameter list held as a range. */
    unsigned sig_start = 0, sig_end = 0;
    for (const char *sig = strstr(good, "\"signature\":null,\"signature_start\":"); sig;
         sig = strstr(sig + 1, "\"signature\":null,\"signature_start\":")) {
        if (sscanf(sig, "\"signature\":null,\"signature_start\":%u,\"signature_end\":%u",
                   &sig_start, &sig_end) == 2 &&
            sig_end > sig_start) {
            break;
        }
        sig_end = 0;
    }
    CHECK(sig_end > 0, "no parameter list range in the surface");
    if (sig_end > 0) {
        char range_from[128], range_to[128];
        snprintf(range_from, sizeof(range_from),
                 "\"signature\":null,\"signature_start\":%u,\"signature_end\":%u", sig_start,
                 sig_end);
        snprintf(range_to, sizeof(range_to),
                 "\"signature\":\"()\",\"signature_start\":%u,\"signature_end\":%u", sig_start,
                 sig_end);
        expect_refused("with a parameter list copied and ranged",
                       replace(good, range_from, range_to));
        snprintf(range_to, sizeof(range_to),
                 "\"signature\":null,\"signature_start\":%u,\"signature_end\":%zu", sig_start,
                 strlen(cpp) + 1);
        expect_refused("with a parameter list past the source", replace(good, range_from, range_to));
        snprintf(range_to, sizeof(range_to),
                 "\"signature\":null,\"signature_start\":%u,\"signature_end\":%u", sig_end + 1,
                 sig_end);
        expect_refused("with a parameter list starting past its end",
                       replace(good, range_from, range_to));
    }
    free(good);
}

/* --- retained text ---------------------------------------------------------- */

typedef struct {
    unsigned long long ranges, owned_bytes, name_bytes;
} retained;

static void count_text(retained *t, const pdxe_text *x) {
    if (x->kind == PDXE_TEXT_SOURCE) {
        t->ranges++;
    } else if (x->kind == PDXE_TEXT_OWNED) {
        t->owned_bytes += strlen(x->owned) + 1;
    }
}

/* The text an extraction holds of its own: owned text, and the qualified names'. */
static retained retained_of(const char *lang, const char *path, const char *source) {
    pdxe_file_result *r = extract(lang, path, source);
    retained t = {0, 0, 0};
    CHECK(r->extraction_lost == 0, "%s: lost work", path);
    for (uint32_t i = 0; i < r->n_calls; i++) {
        count_text(&t, &r->calls[i].callee_text);
        for (uint32_t k = 0; k < r->calls[i].n_args; k++) {
            count_text(&t, &r->calls[i].args[k].expr);
        }
    }
    for (uint32_t i = 0; i < r->n_usages; i++) {
        count_text(&t, &r->usages[i].name);
    }
    for (uint32_t i = 0; i < r->n_defs; i++) {
        t.name_bytes += r->defs[i].qn_text ? strlen(r->defs[i].qn_text) + 1 : 0;
        count_text(&t, &r->defs[i].signature);
        for (uint32_t k = 0; k < r->defs[i].n_routes; k++) {
            count_text(&t, &r->defs[i].routes[k].source_text);
        }
    }
    pdxe_result_free(ctx, r);
    return t;
}

static char *repeat(const char *s, int n) {
    size_t len = strlen(s);
    char *out = malloc(len * (size_t)n + 1);
    for (int i = 0; i < n; i++) {
        memcpy(out + len * (size_t)i, s, len);
    }
    out[len * (size_t)n] = '\0';
    return out;
}

/* One construct of a shape, nested `n` deep. */
static char *shape(int which, int n) {
    char *out = NULL;
    char *a = NULL, *b = NULL;
    size_t cap;
    switch (which) {
    case 0: /* C: arguments holding calls */
        a = repeat("f(a, ", n);
        b = repeat(")", n);
        cap = strlen(a) + strlen(b) + 64;
        out = malloc(cap);
        snprintf(out, cap, "int g(int a) { return %sa%s; }\n", a, b);
        break;
    case 1: /* JavaScript: a call chain */
        a = repeat(".f()", n);
        cap = strlen(a) + 64;
        out = malloc(cap);
        snprintf(out, cap, "function m() { return x%s; }\n", a);
        break;
    case 2: /* Python: an attribute chain */
        a = repeat(".b", n);
        cap = strlen(a) + 64;
        out = malloc(cap);
        snprintf(out, cap, "def h():\n    v = a%s\n    return f(v)\n", a);
        break;
    case 4: { /* C++: a function at every level of nested namespaces */
        out = malloc((size_t)n * 48 + 64);
        char *at = out;
        for (int i = 0; i < n; i++) {
            at += sprintf(at, "namespace n%d { int f%d() { return 0; } ", i, i);
        }
        for (int i = 0; i < n; i++) {
            *at++ = '}';
        }
        sprintf(at, "\n");
        break;
    }
    case 5: { /* JavaScript: functions given as parameters' defaults */
        out = malloc((size_t)n * 32 + 64);
        char *at = out;
        for (int i = 0; i < n; i++) {
            at += sprintf(at, "function f%d(a = ", i);
        }
        at += sprintf(at, "1");
        for (int i = 0; i < n; i++) {
            at += sprintf(at, ") {}");
        }
        sprintf(at, "\n");
        break;
    }
    default: { /* Java: classes nested in classes */
        size_t len = 64;
        for (int i = 0; i < n; i++) {
            len += 16;
        }
        out = malloc(len);
        char *at = out;
        at += sprintf(at, "package p;\n");
        for (int i = 0; i < n; i++) {
            at += sprintf(at, "class C%d { ", i);
        }
        at += sprintf(at, "void m() { f(); }");
        for (int i = 0; i < n; i++) {
            *at++ = '}';
        }
        sprintf(at, "\n");
    }
    }
    free(a);
    free(b);
    return out;
}

static void retained_text_is_linear(void) {
    static const char *const langs[] = {"c", "javascript", "python", "java", "cpp", "javascript"};
    static const char *const paths[] = {"src/g.c", "web/m.js", "app/h.py",
                                        "src/main/java/p/C.java", "src/n.cpp", "web/d.js"};
    static const char *const names[] = {"C arguments", "JavaScript call chains",
                                        "Python attribute chains", "Java nested classes",
                                        "C++ namespaces", "JavaScript parameter defaults"};
    for (int which = 0; which < 6; which++) {
        unsigned long long held[3], ranges[3];
        size_t len[3];
        for (int step = 0; step < 3; step++) {
            int n = 40 << step; /* 40, 80, 160 deep */
            char *one = shape(which, n);
            char *source = repeat(one, 4);
            retained t = retained_of(langs[which], paths[which], source);
            held[step] = t.owned_bytes + t.name_bytes;
            ranges[step] = t.ranges;
            len[step] = strlen(source);
            free(one);
            free(source);
        }
        printf("%s: owned and name bytes %llu, %llu, %llu; ranges %llu, %llu, %llu; source %zu, "
               "%zu, %zu\n",
               names[which], held[0], held[1], held[2], ranges[0], ranges[1], ranges[2], len[0],
               len[1], len[2]);
        /* Twice as deep is twice the source: linear text at most doubles, nested copies
         * would quadruple. Every step is checked, and the deepest is bounded by its
         * source. */
        for (int step = 1; step < 3; step++) {
            CHECK(held[step] <= 3 * held[step - 1] + 64,
                  "%s: %llu bytes at depth %d, %llu at half the depth: not linear", names[which],
                  held[step], 40 << step, held[step - 1]);
        }
        CHECK(held[2] <= len[2], "%s: %llu bytes held for %zu of source", names[which], held[2],
              len[2]);
        CHECK(which == 3 || ranges[2] >= 4 * 160, "%s: only %llu ranges", names[which], ranges[2]);
    }
}

int main(void) {
    if (pdxe_init(&ctx) != PDXE_OK) {
        return 1;
    }
    ranges_and_owned_text();
    nested_qualified_names();
    qualified_name_vectors();
    build_keeps_and_refuses();
    surface_ranges();
    surface_prefixes();
    parameter_lists();
    retained_text_is_linear();
    pdxe_shutdown(ctx);
    printf("%s: %d failures\n", failures ? "FAIL" : "ok", failures);
    return failures ? 1 : 0;
}
