/*
 * A usage's ancestors are answered from the walk's own path (docs/plan/ISSUES.md,
 * issue 90; decision 59; engine/patches/0022-linear-admitted-depth-extraction.patch).
 *
 *     usage_ancestry
 *
 * Classifying a usage asks about its ancestors: the nearest function or import above
 * it, whether a global statement or a parameter list holds it, the first ancestor whose
 * role decides a binding. Each was asked by climbing with ts_node_parent, which
 * descends from the root, so a usage cost its depth squared: a 400-link Python member
 * chain took 3 s, eight of them 25 s. The unified walk now keeps a frame for every
 * node on its path, and each question is answered from the frames, each frame's answer
 * computed once.
 *
 *   - Frames answer exactly what the climbs answered: every fixture, in every language
 *     with a usage rule the frames serve, extracts to the same result with the frames
 *     and with the climbs (the test switch PDX_ENGINE_TEST_CLIMB_ANCESTRY).
 *   - The work is linear: classifying a member chain of 100, 200 and 400 links, 1 to 16
 *     such chains, nested classes, functions, definitions and calls examines a number
 *     of ancestors linear in the nesting, and climbs with ts_node_parent never. The
 *     climbs, for comparison, examine a number quadratic in it.
 *   - The issue's fixtures extract whole: no work lost.
 *
 * Counts, never times: the counters are the engine's own (test builds only).
 */

#define PDXE_USAGE_ANCESTRY_TEST_API 1

#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "pdxe.h"
#include "pdxe_core.h"
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

/* A growing string. */
typedef struct {
    char *text;
    size_t len;
    size_t cap;
} buf_t;

static void put(buf_t *b, const char *s) {
    size_t n = strlen(s);
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
    memcpy(b->text + b->len, s, n + 1);
    b->len += n;
}

static void putf(buf_t *b, const char *fmt, ...) {
    char line[1024];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(line, sizeof line, fmt, ap);
    va_end(ap);
    put(b, line);
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

/* Every fact of an extraction, in order: two extractions are the same exactly when
 * these are. Text is spelled out from `source`, so a range and its text compare equal. */
static void describe(buf_t *b, const pdxe_file_result *r, const char *source) {
    const uint8_t *src = (const uint8_t *)source;
    size_t len = strlen(source);
    putf(b, "status %d truncated %u lost %u ns ", r->status, r->truncated, r->extraction_lost);
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
        put_text(b, d->route_path);
        put_text(b, d->route_method);
        for (uint32_t k = 0; k < d->n_routes; k++) {
            const pdxe_route *route = &d->routes[k];
            put(b, " route ");
            put_text(b, route->method);
            put_text(b, route->path);
            put_text(b, route->callee_text);
            put_text(b, tv_text(&route->source_text, src, len));
            put_span(b, route->span);
            put_list(b, route->ast_path, route->n_ast_path);
        }
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
    for (uint32_t i = 0; i < r->n_channels; i++) {
        put(b, "channel ");
        put_text(b, r->channels[i].channel_text);
        putf(b, " %u ", r->channels[i].is_listen);
        put_span(b, r->channels[i].span);
        put(b, "\n");
    }
    for (uint32_t i = 0; i < r->n_envs; i++) {
        put(b, "env ");
        put_text(b, r->envs[i].key);
        put_span(b, r->envs[i].span);
        put(b, "\n");
    }
    for (uint32_t i = 0; i < r->n_diags; i++) {
        put(b, "diag ");
        put_text(b, r->diags[i].message);
        put_span(b, r->diags[i].span);
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
}

/* One extraction's facts and its work: ancestors examined and ts_node_parent climbs. */
typedef struct {
    buf_t facts;
    uint64_t work;
    uint64_t fallbacks;
    uint32_t lost;
    uint32_t usages;
    uint32_t calls;
} run_t;

static pdxe_ctx *ctx = NULL;

static run_t run(const char *language, const char *path, const char *source, int climb) {
    run_t out;
    memset(&out, 0, sizeof out);
    if (climb) {
        setenv("PDX_ENGINE_TEST_CLIMB_ANCESTRY", "1", 1);
    }
    pdxe_usage_field_lookup_test_reset();
    pdxe_file_result *r = NULL;
    int rc = pdxe_extract_file(ctx, pdxe_language_id(language), path, (const uint8_t *)source,
                               strlen(source), &r);
    out.work = pdxe_usage_field_lookup_test_work();
    out.fallbacks = pdxe_usage_slow_parent_fallback_test_count();
    unsetenv("PDX_ENGINE_TEST_CLIMB_ANCESTRY");
    if (rc != PDXE_OK || !r) {
        printf("FAIL %s %s: extraction failed (%d)\n", language, path, rc);
        failures++;
        put(&out.facts, "<failed>");
        return out;
    }
    describe(&out.facts, r, source);
    out.lost = r->extraction_lost;
    out.usages = r->n_usages;
    out.calls = r->n_calls;
    pdxe_result_free(ctx, r);
    return out;
}

static void release(run_t *r) {
    free(r->facts.text);
    memset(r, 0, sizeof *r);
}

/* The first line where two descriptions differ, for the failure message. */
static void first_difference(const char *a, const char *b, char *out, size_t cap) {
    const char *la = a, *lb = b;
    while (*la && *lb) {
        const char *ea = strchr(la, '\n'), *eb = strchr(lb, '\n');
        size_t na = ea ? (size_t)(ea - la) : strlen(la), nb = eb ? (size_t)(eb - lb) : strlen(lb);
        if (na != nb || memcmp(la, lb, na) != 0) {
            snprintf(out, cap, "frames: %.*s | climbs: %.*s", (int)(na > 300 ? 300 : na), la,
                     (int)(nb > 300 ? 300 : nb), lb);
            return;
        }
        la += na + (ea ? 1 : 0);
        lb += nb + (eb ? 1 : 0);
    }
    snprintf(out, cap, "one description is a prefix of the other");
}

/* Frames and climbs give the same facts, and the frames climb with ts_node_parent
 * never. */
static void same_facts(const char *language, const char *path, const char *source) {
    run_t frames = run(language, path, source, 0);
    run_t climbs = run(language, path, source, 1);
    if (strcmp(frames.facts.text ? frames.facts.text : "", climbs.facts.text ? climbs.facts.text : "") !=
        0) {
        char diff[800];
        first_difference(frames.facts.text, climbs.facts.text, diff, sizeof diff);
        CHECK(0, "%s %s: the frames and the climbs disagree: %s", language, path, diff);
    }
    CHECK(frames.fallbacks == 0, "%s %s: %llu ts_node_parent climbs with the frames", language,
          path, (unsigned long long)frames.fallbacks);
    release(&frames);
    release(&climbs);
}

typedef struct {
    const char *language;
    const char *path;
    const char *source;
} fixture_t;

/* Every rule the frames serve, in the languages that have it. */
static const fixture_t FIXTURES[] = {
    /* Python: imports and aliases, global and nonlocal, parameters and their defaults,
     * lambdas and comprehensions, class names, callable references, keyword labels, a
     * member chain, writes. */
    {"python", "app/rules.py",
     "import os.path\n"
     "import json as j\n"
     "from collections import OrderedDict as OD, deque\n"
     "from . import sibling\n"
     "from __future__ import annotations\n"
     "counter = 0\n"
     "\n"
     "def outer(run, size=len, *args, key=sorted, **kw):\n"
     "    global counter\n"
     "    counter += 1\n"
     "    total = 0\n"
     "    def inner(value=total, fn=lambda x=size: x):\n"
     "        nonlocal total\n"
     "        total = value + fn()\n"
     "        return [v for v in range(total) if v]\n"
     "    class Local:\n"
     "        attr = run\n"
     "    run(inner)\n"
     "    run(obj.method)\n"
     "    run((handler))\n"
     "    run(key=j.dumps, value=OD())\n"
     "    callback = handler\n"
     "    a.b.c.d.e = deque()\n"
     "    return os.path.join(sibling.x, Local.attr, counter)\n"
     "\n"
     "class Shape:\n"
     "    def area(self, scale=1.0):\n"
     "        return self.w * self.h * scale\n"},
    /* JavaScript and TypeScript: var against let and const in blocks and functions,
     * imports of every form, member values passed as arguments, arrows. */
    {"javascript", "web/rules.js",
     "import def, { a as b, c } from './m';\n"
     "import * as ns from './n';\n"
     "export function f(x, { y, z }, ...rest) {\n"
     "  var hoisted = x;\n"
     "  if (x) { let scoped = y; const fixed = z; var late = scoped + fixed; }\n"
     "  for (let i = 0; i < 3; i++) { var j = i; }\n"
     "  register(handlers.onClick, ns.go, b);\n"
     "  const g = (p, q = hoisted) => p + q + late + j;\n"
     "  return c(def, g, rest);\n"
     "}\n"},
    {"typescript", "web/rules.ts",
     "import legacy = require('legacy');\n"
     "import { Thing as Renamed } from './things';\n"
     "export class Box<T> {\n"
     "  constructor(private item: T, cfg: Config) {}\n"
     "  open(f: (t: T) => void): void { let local = this.item; f(local); use(this.open); }\n"
     "}\n"
     "function run(cb = legacy.make): Renamed { var v = cb(); return v as Renamed; }\n"},
    /* Rust: scoped identifiers, every use form, extern crate aliases, closures,
     * modules. */
    {"rust", "src/rules.rs",
     "extern crate core as kernel;\n"
     "use std::collections::{HashMap, HashSet as Set, self};\n"
     "use crate::a::b::{self, c::{d, e as f}};\n"
     "mod inner { use super::kernel; pub fn g(x: i32) -> i32 { kernel::id(x) } }\n"
     "fn run(map: HashMap<i32, i32>, s: Set<i32>) -> i32 {\n"
     "    let total = map.len() + s.len();\n"
     "    let add = |y: i32| y + total as i32;\n"
     "    apply(inner::g, add, collections::BTreeMap::new);\n"
     "    std::process::exit(add(inner::g(1)))\n"
     "}\n"},
    /* Java: nested classes, lambdas, method references, parameters and locals. */
    {"java", "src/main/java/p/Rules.java",
     "package p;\n"
     "import java.util.function.Function;\n"
     "class Rules {\n"
     "  int count;\n"
     "  class Inner { int value(int x) { int y = x + count; return y; } }\n"
     "  void run(Function<Integer, Integer> f) {\n"
     "    int local = f.apply(count);\n"
     "    apply(this::run, Rules::helper, x -> x + local);\n"
     "  }\n"
     "  static int helper(int z) { return z; }\n"
     "}\n"},
    /* Kotlin: callable references, navigation, lambdas, parameters. */
    {"kotlin", "src/main/kotlin/p/Rules.kt",
     "package p\n"
     "class Rules(val seed: Int) {\n"
     "    fun run(f: (Int) -> Int): Int {\n"
     "        val local = f(seed)\n"
     "        apply(::helper, this::run, other.member)\n"
     "        return listOf(1, 2).map { x -> x + local }.sum()\n"
     "    }\n"
     "    fun helper(z: Int) = z\n"
     "}\n"},
    /* C#: method groups through generic names, member access and parentheses. */
    {"csharp", "src/Rules.cs",
     "using System;\n"
     "namespace P {\n"
     "  class Rules {\n"
     "    int count;\n"
     "    void Run(Func<int, int> f, int seed) {\n"
     "      var local = f(seed) + count;\n"
     "      Apply(Helper, this.Helper, (Helper), Make<int>, x => x + local);\n"
     "    }\n"
     "    static int Helper(int z) { return z; }\n"
     "  }\n"
     "}\n"},
    /* Go: selectors passed as values, short variable declarations, closures. */
    {"go", "pkg/rules.go",
     "package pkg\n"
     "import (\n"
     "  \"fmt\"\n"
     "  str \"strings\"\n"
     ")\n"
     "func Run(f func(int) int, seed int) int {\n"
     "  local := f(seed)\n"
     "  apply(obj.Method, str.ToUpper, fmt.Sprint)\n"
     "  g := func(x int) int { return x + local }\n"
     "  return g(local)\n"
     "}\n"},
    /* Perl: code references passed as arguments, lexical declarations. */
    {"perl", "lib/Rules.pm",
     "package Rules;\n"
     "sub run {\n"
     "  my ($self, $cb) = @_;\n"
     "  my $local = $cb->(1);\n"
     "  register(\\&handler, \\&Other::thing, $local);\n"
     "  my $inner = sub { my $x = shift; return $x + $local; };\n"
     "  return $inner->($local);\n"
     "}\n"
     "sub handler { return 1; }\n"
     "1;\n"},
    /* HCL: attribute keys bind, expressions read. */
    {"hcl", "infra/rules.tf",
     "variable \"region\" { default = \"eu-west-1\" }\n"
     "resource \"aws_instance\" \"web\" {\n"
     "  ami = var.ami\n"
     "  tags = { Name = local.name, Region = var.region }\n"
     "}\n"},
    /* C and C++: parameters, locals, function pointers passed as values, fields. */
    {"c", "src/rules.c",
     "struct point { int x, y; };\n"
     "static int helper(int v) { return v; }\n"
     "int run(int (*f)(int), struct point p) {\n"
     "  int local = f(p.x) + p.y;\n"
     "  apply(helper, f, (helper));\n"
     "  return local;\n"
     "}\n"},
    {"cpp", "src/rules.cpp",
     "namespace n { int helper(int v) { return v; } }\n"
     "class Rules { int count; public: int run(int seed) {\n"
     "  auto add = [&](int x) { return x + count + seed; };\n"
     "  apply(n::helper, add, &Rules::run);\n"
     "  return add(seed); } };\n"},
    /* The rest of the reachable languages: their usages take the same paths. */
    {"php", "src/Rules.php",
     "<?php\n"
     "namespace P;\n"
     "use Other\\Thing as T;\n"
     "function run($cb, $seed = DEFAULT) { $local = $cb($seed); apply('helper', [$this, 'm'], T::make(...)); return $local; }\n"},
    {"ruby", "lib/rules.rb",
     "class Rules\n"
     "  def run(cb, seed = 1)\n"
     "    local = cb.call(seed)\n"
     "    apply(method(:helper), &blk)\n"
     "    [1, 2].map { |x| x + local }\n"
     "  end\n"
     "end\n"},
    {"lua", "src/rules.lua",
     "local M = {}\n"
     "function M.run(cb, seed)\n"
     "  local value = cb(seed)\n"
     "  apply(M.helper, function(x) return x + value end)\n"
     "  return value\n"
     "end\n"
     "return M\n"},
    {"scala", "src/main/scala/p/Rules.scala",
     "package p\n"
     "object Rules {\n"
     "  def run(f: Int => Int, seed: Int): Int = { val local = f(seed); apply(helper _, x => x + local); local }\n"
     "  def helper(z: Int): Int = z\n"
     "}\n"},
    {"swift", "Sources/Rules.swift",
     "func run(f: (Int) -> Int, seed: Int) -> Int {\n"
     "  let local = f(seed)\n"
     "  apply(helper, { x in x + local })\n"
     "  return local\n"
     "}\n"},
    {"bash", "scripts/rules.sh",
     "run() { local value=\"$1\"; helper \"$value\" \"${HOME}\"; echo \"$value\"; }\n"},
    {"sql", "db/rules.sql",
     "CREATE FUNCTION f(a integer) RETURNS integer AS $$ SELECT a + b FROM t WHERE c = a $$ LANGUAGE sql;\n"},
    {"groovy", "src/Rules.groovy",
     "def run(cb, seed) { def local = cb(seed); apply(this.&helper, { x -> x + local }); local }\n"},
    {"objc", "src/Rules.m",
     "int run(int (^cb)(int), int seed) { int local = cb(seed); apply(helper, ^(int x) { return x + local; }); return local; }\n"},
    {"ada", "src/rules.adb",
     "package body Rules is\n"
     "   function Run (Seed : Integer) return Integer is\n"
     "      Local : Integer := Helper (Seed);\n"
     "   begin\n"
     "      return Local + Seed;\n"
     "   end Run;\n"
     "end Rules;\n"},
    /* Parser recovery: empty (missing) nodes, which the frames do not answer for. */
    {"python", "app/broken.py", "def f(a, b=):\n    return (a.b.c(, x) + \n"},
    {"javascript", "web/broken.js", "function f(a, { b, }) { return a.b.( + ; var = ; }\n"},
    {"java", "src/main/java/p/Broken.java", "class Broken { void m(int a) { int = a + ; f(a, ); } }\n"},
};

/* A Python function with `count` member chains of `links` links each. */
static char *python_chains(int links, int count) {
    buf_t b = {0};
    put(&b, "def h():\n");
    for (int c = 0; c < count; c++) {
        put(&b, "    v = a");
        for (int i = 0; i < links; i++) {
            put(&b, ".b");
        }
        put(&b, "\n");
    }
    put(&b, "    return f(v)\n");
    return b.text;
}

static char *java_classes(int n) {
    buf_t b = {0};
    put(&b, "package p;\n");
    for (int i = 0; i < n; i++) {
        putf(&b, "class C%d { int f%d; ", i, i);
    }
    put(&b, "void m(int a) { f(a); }");
    for (int i = 0; i < n; i++) {
        put(&b, "}");
    }
    put(&b, "\n");
    return b.text;
}

static char *js_functions(int n) {
    buf_t b = {0};
    for (int i = 0; i < n; i++) {
        putf(&b, "function f%d(a%d) { var v%d = a%d; ", i, i, i, i);
    }
    put(&b, "g(a0);");
    for (int i = 0; i < n; i++) {
        put(&b, "}");
    }
    put(&b, "\n");
    return b.text;
}

static char *python_defs(int n) {
    buf_t b = {0};
    for (int i = 0; i < n; i++) {
        for (int k = 0; k < i; k++) {
            put(&b, "    ");
        }
        putf(&b, "def f%d(a%d, b%d=a):\n", i, i, i);
    }
    for (int k = 0; k < n; k++) {
        put(&b, "    ");
    }
    put(&b, "return a0 + b0\n");
    return b.text;
}

static char *c_calls(int n) {
    buf_t b = {0};
    put(&b, "int g(int a) { return ");
    for (int i = 0; i < n; i++) {
        put(&b, "f(a, ");
    }
    put(&b, "a");
    for (int i = 0; i < n; i++) {
        put(&b, ")");
    }
    put(&b, "; }\n");
    return b.text;
}

typedef char *(*shape_fn)(int);

static char *python_chain(int links) {
    return python_chains(links, 1);
}

/* Work for nesting n, 2n and 4n: no climb, and each doubling at most doubles the work
 * (give or take what the file holds besides the nesting). At n and 2n the climbs give
 * the same facts and, where they were quadratic, examine at 2n at least three times
 * what they did at n: the counter would see them come back. (Climbing 4n is the
 * slowness the issue is about.) */
static void linear(const char *what, const char *language, const char *path, shape_fn shape,
                   int n, int climbs_quadratic) {
    uint64_t work[3], climbed[2] = {0, 0};
    for (int k = 0; k < 3; k++) {
        char *source = shape(n << k);
        run_t frames = run(language, path, source, 0);
        CHECK(frames.lost == 0, "%s %d: work lost (%u)", what, n << k, frames.lost);
        CHECK(frames.fallbacks == 0, "%s %d: %llu ts_node_parent climbs", what, n << k,
              (unsigned long long)frames.fallbacks);
        work[k] = frames.work;
        if (k < 2) {
            run_t climbs = run(language, path, source, 1);
            CHECK(strcmp(frames.facts.text, climbs.facts.text) == 0,
                  "%s %d: the frames and the climbs disagree", what, n << k);
            climbed[k] = climbs.work + climbs.fallbacks;
            release(&climbs);
        }
        release(&frames);
        free(source);
    }
    uint64_t slack = work[0] / 4 + 64;
    CHECK(work[1] <= 2 * work[0] + slack && work[2] <= 2 * work[1] + slack,
          "%s: work %llu, %llu, %llu for nesting %d, %d, %d is not linear", what,
          (unsigned long long)work[0], (unsigned long long)work[1], (unsigned long long)work[2],
          n, 2 * n, 4 * n);
    CHECK(!climbs_quadratic || climbed[1] >= 3 * climbed[0],
          "%s: the climbs examined %llu, %llu: not quadratic, so this proves nothing", what,
          (unsigned long long)climbed[0], (unsigned long long)climbed[1]);
    printf("  %s: work %llu, %llu, %llu (climbs %llu, %llu)\n", what, (unsigned long long)work[0],
           (unsigned long long)work[1], (unsigned long long)work[2],
           (unsigned long long)climbed[0], (unsigned long long)climbed[1]);
}

int main(void) {
    if (pdxe_init(&ctx) != PDXE_OK) {
        printf("FAIL pdxe_init\n");
        return 1;
    }

    /* Frames answer exactly what the climbs answered. */
    for (size_t i = 0; i < sizeof FIXTURES / sizeof FIXTURES[0]; i++) {
        same_facts(FIXTURES[i].language, FIXTURES[i].path, FIXTURES[i].source);
    }

    /* The issue's member chain at 100, 200 and 400 links. */
    linear("python member chain", "python", "app/h.py", python_chain, 100, 1);
    {
        char *source = python_chain(400);
        run_t chain = run("python", "app/h.py", source, 0);
        CHECK(chain.fallbacks == 0, "400-link chain: %llu ts_node_parent climbs",
              (unsigned long long)chain.fallbacks);
        CHECK(chain.lost == 0, "400-link chain: work lost");
        CHECK(chain.usages > 0 && chain.calls > 0, "400-link chain: no usage or call");
        release(&chain);
        free(source);
    }

    /* 1, 2, 4, 8 and 16 such chains: the work is the chains', each the same. */
    {
        uint64_t one = 0, previous = 0;
        for (int count = 1; count <= 16; count *= 2) {
            char *source = python_chains(400, count);
            run_t chains = run("python", "app/h.py", source, 0);
            CHECK(chains.fallbacks == 0, "%d chains: %llu ts_node_parent climbs", count,
                  (unsigned long long)chains.fallbacks);
            CHECK(chains.lost == 0, "%d chains: work lost", count);
            if (count == 1) {
                one = chains.work;
            } else {
                CHECK(chains.work <= 2 * previous + 64, "%d chains: work %llu after %llu",
                      count, (unsigned long long)chains.work, (unsigned long long)previous);
            }
            CHECK(chains.work <= (uint64_t)count * one + 64,
                  "%d chains: work %llu, one chain %llu", count,
                  (unsigned long long)chains.work, (unsigned long long)one);
            printf("  %d chains of 400 links: work %llu\n", count,
                   (unsigned long long)chains.work);
            previous = chains.work;
            release(&chains);
            free(source);
        }
    }

    /* The other shapes the issue measured, and definitions and calls nested alike. */
    linear("java nested classes", "java", "src/main/java/p/C.java", java_classes, 60, 1);
    linear("javascript nested functions", "javascript", "web/f.js", js_functions, 60, 1);
    linear("python nested definitions", "python", "app/defs.py", python_defs, 30, 1);
    /* An argument's climb stops at its argument list: nested calls were never
     * quadratic here, and stay linear. */
    linear("c nested calls", "c", "src/calls.c", c_calls, 60, 0);

    pdxe_shutdown(ctx);
    if (failures) {
        printf("%d failure(s)\n", failures);
        return 1;
    }
    printf("usage_ancestry: ok\n");
    return 0;
}
