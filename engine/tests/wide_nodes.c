/*
 * Stepping through a wide node costs its width (docs/plan/ISSUES.md, issue 93;
 * engine/patches/0026-linear-wide-node-traversal.patch).
 *
 *     wide_nodes <corpus dir> <smoke fixtures dir>
 *     wide_nodes --refused <corpus dir> <smoke fixtures dir>
 *
 * tree-sitter's positional lookups cost the position: a child by index walks the
 * children from the first, a parent is found by descending from the root, a sibling by
 * finding the parent and walking its children. In a list a grammar's repetition builds
 * that is the logarithm of the position; in a node the parser's recovery builds, one
 * flat run holding the rest of a file, it is the position itself, and every pass that
 * stepped through such a node by index, or asked for a parent or a sibling inside it,
 * was quadratic in the file: a 2 MiB Kotlin file of one-line nested classes took 764 s.
 * Children are now visited by cursor (node_iter.h), and parents and siblings are found
 * from the walk's own path or the loop in progress.
 *
 *   - The answers are tree-sitter's: on every node of every corpus and smoke file, the
 *     iterator gives exactly ts_node_child's and ts_node_named_child's children and
 *     ts_node_field_name_for_child's fields, the parent and sibling lookups exactly
 *     ts_node_parent's and the four sibling lookups', from a walk's path and from a loop
 *     in progress, and the tree walk visits the nodes a stack walk visits, in its order.
 *   - The work is linear: in every language, wide modules, class bodies, function bodies,
 *     argument and parameter lists, literals, switches, decorators, imports and files
 *     the parser's recovery folds into one error node, at 1,000 to 8,000 items, reach a
 *     number of children by position, step a cursor, look up parents, siblings and
 *     fields a number of times that doubles when the file doubles (positional lookups
 *     weighted by the position, parent and sibling lookups by the children passed).
 *   - The issue's own files are what they are said to be: one error node holding the
 *     rest of the file, as wide as the file is long, and their extraction's work linear.
 *   - A collection that cannot get its memory is lost work, never a positional walk:
 *     with every collection refused (PDX_ENGINE_TEST_REFUSE_LOOKUP_INDEXES=children,
 *     --refused), extraction completes, counts the loss and stays linear.
 *
 * Counts, never times: the counters are the engine's own (test builds only).
 */

#define PDXE_LOOKUP_COUNTS_TEST_API 1

#include <dirent.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

#include "lang_specs.h"
#include "lookup_counts.h"
#include "pdxe.h"
#include "pdxe_core.h"
#include "tree_sitter/api.h"

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
        b->text = realloc(b->text, cap);
        if (!b->text) {
            printf("FAIL out of memory\n");
            exit(1);
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

/* `unit` with every ~I~ replaced by `i`. */
static void put_unit(buf_t *b, const char *unit, int i) {
    char number[16];
    snprintf(number, sizeof number, "%d", i);
    for (const char *at = unit; *at;) {
        const char *mark = strstr(at, "~I~");
        if (!mark) {
            put(b, at);
            return;
        }
        put_n(b, at, (size_t)(mark - at));
        put(b, number);
        at = mark + 3;
    }
}

static char *read_file(const char *path, size_t *len) {
    FILE *f = fopen(path, "rb");
    if (!f) {
        return NULL;
    }
    fseek(f, 0, SEEK_END);
    long n = ftell(f);
    fseek(f, 0, SEEK_SET);
    char *text = malloc((size_t)n + 1);
    if (!text || fread(text, 1, (size_t)n, f) != (size_t)n) {
        fclose(f);
        free(text);
        return NULL;
    }
    fclose(f);
    text[n] = '\0';
    *len = (size_t)n;
    return text;
}

/* --- the answers are tree-sitter's -------------------------------------------- */

static uint64_t checks = 0;

static bool same(TSNode a, TSNode b) {
    if (ts_node_is_null(a) || ts_node_is_null(b)) {
        return ts_node_is_null(a) == ts_node_is_null(b);
    }
    return a.id == b.id && a.tree == b.tree;
}

static void same_lookups(const char *where, TSNode node) {
    TSNode got[5] = {pdxe_node_parent(node), pdxe_node_prev_sibling(node),
                     pdxe_node_next_sibling(node), pdxe_node_prev_named_sibling(node),
                     pdxe_node_next_named_sibling(node)};
    TSNode want[5] = {(ts_node_parent)(node), (ts_node_prev_sibling)(node),
                      (ts_node_next_sibling)(node), (ts_node_prev_named_sibling)(node),
                      (ts_node_next_named_sibling)(node)};
    static const char *const names[5] = {"parent", "previous sibling", "next sibling",
                                         "previous named sibling", "next named sibling"};
    for (int k = 0; k < 5; k++) {
        checks++;
        CHECK(same(got[k], want[k]), "%s: the %s of a %s at byte %u is not tree-sitter's", where,
              names[k], ts_node_type(node), ts_node_start_byte(node));
    }
}

static void same_children(const char *where, TSNode node) {
    uint32_t count = ts_node_child_count(node), i = 0;
    PDXE_FOR_EACH_CHILD(it, node) {
        checks++;
        const char *field = pdxe_child_iter_field_name(&it);
        const char *want_field = (ts_node_field_name_for_child)(node, i);
        CHECK(it.index == i && same(it.node, (ts_node_child)(node, i)) &&
                  ((!field && !want_field) || (field && want_field && !strcmp(field, want_field))),
              "%s: child %u of a %s is not tree-sitter's", where, i, ts_node_type(node));
        /* From the loop in progress: the child's own lookups. */
        same_lookups(where, it.node);
        i++;
    }
    CHECK(i == count, "%s: %u of the %u children of a %s", where, i, count, ts_node_type(node));
    count = ts_node_named_child_count(node);
    i = 0;
    PDXE_FOR_EACH_NAMED_CHILD(it, node) {
        checks++;
        CHECK(same(it.node, (ts_node_named_child)(node, i)),
              "%s: named child %u of a %s is not tree-sitter's", where, i, ts_node_type(node));
        i++;
    }
    CHECK(i == count, "%s: %u of the %u named children of a %s", where, i, count,
          ts_node_type(node));
}

/* A stack walk pushing each node's children in reverse, by index: the order the tree
 * walk must keep. */
static void reference_order(TSNode root, TSNode **out, uint32_t *n, uint32_t **depths) {
    uint32_t cap = 1024, top = 0, count = 0;
    TSNode *stack = malloc(cap * sizeof *stack);
    uint32_t *stack_depth = malloc(cap * sizeof *stack_depth);
    TSNode *order = malloc(cap * sizeof *order);
    uint32_t *order_depth = malloc(cap * sizeof *order_depth);
    uint32_t order_cap = cap;
    stack[top] = root;
    stack_depth[top++] = 0;
    while (top > 0) {
        TSNode node = stack[--top];
        uint32_t depth = stack_depth[top];
        if (count == order_cap) {
            order_cap *= 2;
            order = realloc(order, order_cap * sizeof *order);
            order_depth = realloc(order_depth, order_cap * sizeof *order_depth);
        }
        order[count] = node;
        order_depth[count++] = depth;
        uint32_t children = (ts_node_child_count)(node);
        if (top + children > cap) {
            while (top + children > cap) {
                cap *= 2;
            }
            stack = realloc(stack, cap * sizeof *stack);
            stack_depth = realloc(stack_depth, cap * sizeof *stack_depth);
        }
        for (uint32_t i = children; i-- > 0;) {
            stack[top] = (ts_node_child)(node, i);
            stack_depth[top++] = depth + 1;
        }
    }
    free(stack);
    free(stack_depth);
    *out = order;
    *depths = order_depth;
    *n = count;
}

static void same_answers(const char *language, const char *path) {
    size_t len = 0;
    char *source = read_file(path, &len);
    if (!source) {
        CHECK(0, "%s: unreadable", path);
        return;
    }
    TSParser *parser = ts_parser_new();
    ts_parser_set_language(parser, pdxe_ts_language((PDXELanguage)(pdxe_language_id(language) - 1)));
    TSTree *tree = ts_parser_parse_string(parser, NULL, source, (uint32_t)len);
    TSNode root = ts_tree_root_node(tree);

    /* A walk with its path registered, as the unified walk keeps it. */
    PDXEWalkPath path_reg = {0};
    pdxe_walk_path_open(&path_reg);
    TSTreeCursor cursor = ts_tree_cursor_new(root);
    uint32_t depth = 0;
    bool entered = true;
    for (;;) {
        TSNode node = ts_tree_cursor_current_node(&cursor);
        if (entered || depth > path_reg.depth) {
            pdxe_walk_path_at(&path_reg, depth, node, PDXE_PATH_PARENT | PDXE_PATH_PLACE,
                              (TSNode){0}, 0);
        } else {
            PDXEPathEntry before = path_reg.entries[depth];
            pdxe_walk_path_at(&path_reg, depth, node, PDXE_PATH_PARENT | PDXE_PATH_PLACE,
                              before.node, before.index + 1);
        }
        same_lookups(path, node);
        same_children(path, node);
        if ((ts_tree_cursor_goto_first_child)(&cursor)) {
            depth++;
            entered = true;
            continue;
        }
        entered = false;
        if ((ts_tree_cursor_goto_next_sibling)(&cursor)) {
            continue;
        }
        bool found = false;
        while ((ts_tree_cursor_goto_parent)(&cursor)) {
            depth--;
            if ((ts_tree_cursor_goto_next_sibling)(&cursor)) {
                found = true;
                break;
            }
        }
        if (!found) {
            break;
        }
    }
    ts_tree_cursor_delete(&cursor);
    pdxe_walk_path_close(&path_reg);

    /* The tree walk's order and depths are the stack walk's. */
    TSNode *order = NULL;
    uint32_t *depths = NULL, n = 0, at = 0;
    reference_order(root, &order, &n, &depths);
    PDXE_TREE_WALK(w, root) {
        checks++;
        if (at >= n || !same(w.node, order[at]) || w.depth != depths[at]) {
            CHECK(0, "%s: the tree walk's node %u is not the stack walk's", path, at);
            break;
        }
        /* From the tree walk's path. */
        same_lookups(path, w.node);
        at++;
    }
    CHECK(at == n, "%s: the tree walk visited %u of %u nodes", path, at, n);
    free(order);
    free(depths);
    ts_tree_delete(tree);
    ts_parser_delete(parser);
    free(source);
}

/* Every smoke fixture (<language>.<ext>) and every corpus file of its language. */
static void answers_are_tree_sitters(const char *corpus, const char *smoke) {
    DIR *dir = opendir(smoke);
    if (!dir) {
        CHECK(0, "%s: no smoke fixtures", smoke);
        return;
    }
    struct dirent *e;
    int files = 0;
    while ((e = readdir(dir)) != NULL) {
        if (e->d_name[0] == '.') {
            continue;
        }
        char language[sizeof e->d_name];
        snprintf(language, sizeof language, "%s", e->d_name);
        char *dot = strchr(language, '.');
        if (dot) {
            *dot = '\0';
        }
        if (pdxe_language_id(language) == 0) {
            continue;
        }
        char path[4096];
        snprintf(path, sizeof path, "%s/%s", smoke, e->d_name);
        same_answers(language, path);
        files++;
        char corpus_dir[2048];
        snprintf(corpus_dir, sizeof corpus_dir, "%.1500s/%s", corpus, language);
        DIR *cd = opendir(corpus_dir);
        if (!cd) {
            continue;
        }
        struct dirent *ce;
        while ((ce = readdir(cd)) != NULL) {
            if (ce->d_name[0] == '.') {
                continue;
            }
            snprintf(path, sizeof path, "%.2047s/%s", corpus_dir, ce->d_name);
            struct stat st;
            if (stat(path, &st) == 0 && S_ISREG(st.st_mode)) {
                same_answers(language, path);
                files++;
            }
        }
        closedir(cd);
    }
    closedir(dir);
    printf("answers: %d files, %llu lookups and children compared with tree-sitter's\n", files,
           (unsigned long long)checks);
    CHECK(files >= 32, "only %d files", files);
}

/* --- the work is linear ---------------------------------------------------------- */

typedef struct {
    const char *language;
    const char *ext;
    const char *name;
    const char *prefix;
    const char *unit; /* repeated, ~I~ the item's number */
    const char *suffix;
} shape_t;

static const shape_t SHAPES[] = {
    {"javascript", "js", "module", "", "/** d */\nexport function f~I~(a) { return g~I~(a); }\n", ""},
    {"javascript", "js", "class", "class C {\n", "  m~I~(a) { return this.g~I~(a); }\n", "}\n"},
    {"javascript", "js", "body", "function f() {\n", "  const x~I~ = g~I~(x~I~);\n", "}\n"},
    {"javascript", "js", "arguments", "f(\n", "  a~I~,\n", ");\n"},
    {"javascript", "js", "parameters", "function f(\n", "  p~I~,\n", ") { return p0; }\n"},
    {"javascript", "js", "array", "const x = [\n", "  g~I~(),\n", "];\n"},
    {"javascript", "js", "object", "const x = {\n", "  k~I~: g~I~(),\n", "};\n"},
    {"javascript", "js", "switch", "switch (x) {\n", "  case ~I~: f~I~(); break;\n", "}\n"},
    {"javascript", "js", "error", "let x = (\n", "function f~I~() { g~I~(); }\n", ""},
    {"javascript", "js", "require", "", "const m~I~ = require('./m~I~');\n", ""},
    {"javascript", "js", "jsx", "const x = <div>\n", "  <Item~I~ onClick={() => go~I~()} />\n", "</div>;\n"},
    {"typescript", "ts", "class", "class C {\n",
     "  /** doc */\n  @Get('/p~I~')\n  m~I~(a: number) { return this.g~I~(a); }\n", "}\n"},
    {"typescript", "ts", "decorators", "", "@d~I~()\n", "class C {}\n"},
    {"typescript", "ts", "interface", "interface I {\n", "  m~I~(a: string): number;\n", "}\n"},
    {"typescript", "ts", "enum", "enum E {\n", "  A~I~ = ~I~,\n", "}\n"},
    {"typescript", "ts", "union", "type T =\n", "  | 'v~I~'\n", ";\n"},
    {"typescript", "ts", "imports", "", "import { a~I~ } from './m~I~';\n", ""},
    {"typescript", "ts", "error", "class C {\n  m( {\n", "  m~I~() { g~I~(); }\n", ""},
    {"tsx", "tsx", "jsx", "const x = <div>\n", "  <Item~I~ onClick={() => go~I~()} />\n", "</div>;\n"},
    {"python", "py", "module", "",
     "# c\n@route('/p~I~')\ndef f~I~(a):\n    \"\"\"D.\"\"\"\n    return g~I~(a)\n", ""},
    {"python", "py", "class", "class C:\n", "    def m~I~(self, a):\n        return self.g~I~(a)\n", ""},
    {"python", "py", "body", "def f():\n", "    x~I~ = g~I~(x~I~)\n", ""},
    {"python", "py", "arguments", "f(\n", "    a~I~,\n", ")\n"},
    {"python", "py", "parameters", "def f(\n", "    p~I~,\n", "):\n    return p0\n"},
    {"python", "py", "dict", "x = {\n", "    'k~I~': g~I~(),\n", "}\n"},
    {"python", "py", "decorators", "", "@d~I~\n", "def f():\n    pass\n"},
    {"python", "py", "error", "x = (\n", "def f~I~(): g~I~()\n", ""},
    {"python", "py", "match", "match x:\n", "    case ~I~:\n        f~I~()\n", ""},
    {"java", "java", "class", "class C {\n",
     "  /** d */ @GetMapping(\"/p~I~\") public int m~I~(int a) { return g~I~(a); }\n", "}\n"},
    {"java", "java", "body", "class C { void f() {\n", "  int x~I~ = g~I~(1);\n", "} }\n"},
    {"java", "java", "switch", "class C { void f(int x) { switch (x) {\n",
     "  case ~I~: f~I~(); break;\n", "} } }\n"},
    {"java", "java", "enum", "enum E {\n", "  A~I~,\n", "}\n"},
    {"java", "java", "error", "class C { void f( {\n", "  void m~I~() { g~I~(); }\n", ""},
    {"kotlin", "kt", "module", "", "/** d */\n@Ann fun f~I~(a: Int) = g~I~(a)\n", ""},
    {"kotlin", "kt", "class", "class C {\n", "  fun m~I~(a: Int): Int { return g~I~(a) }\n", "}\n"},
    {"kotlin", "kt", "when", "fun f(x: Int) = when (x) {\n", "  ~I~ -> f~I~()\n", "  else -> 0\n}\n"},
    {"kotlin", "kt", "error", "", "class O~I~ { class I~I~ { fun m() { I~I~() } } }\n", ""},
    {"csharp", "cs", "class", "class C {\n",
     "  /// d\n  public int F~I~;\n  public int M~I~(int a) { return G~I~(a); }\n", "}\n"},
    {"csharp", "cs", "properties", "class C {\n", "  public int P~I~ { get; set; }\n", "}\n"},
    {"csharp", "cs", "error", "class C { void F( {\n", "  void M~I~() { G~I~(); }\n", ""},
    {"go", "go", "module", "package p\n", "// F doc\nfunc F~I~(a int) int { return g~I~(a) }\n", ""},
    {"go", "go", "body", "package p\nfunc f() {\n", "  x~I~ := g~I~(1)\n  _ = x~I~\n", "}\n"},
    {"go", "go", "struct", "package p\ntype S struct {\n", "  F~I~ int\n", "}\n"},
    {"go", "go", "error", "package p\nfunc f( {\n", "func F~I~() { g~I~() }\n", ""},
    {"rust", "rs", "module", "",
     "/// d\n#[inline]\nfn f~I~(a: i32) -> i32 { println!(\"{}\", a); g~I~(a) }\n", ""},
    {"rust", "rs", "impl", "struct S;\nimpl S {\n", "  fn m~I~(&self) -> i32 { self.g~I~() }\n", "}\n"},
    {"rust", "rs", "match", "fn f(x: i32) {\n  match x {\n", "    ~I~ => f~I~(),\n", "    _ => (),\n  }\n}\n"},
    {"rust", "rs", "error", "fn f( {\n", "fn g~I~() { h~I~(); }\n", ""},
    {"php", "php", "module", "<?php\n", "/** @return int */\nfunction f~I~($a) { return g~I~($a); }\n", ""},
    {"php", "php", "class", "<?php\nclass C {\n",
     "  /** @return int */\n  public function m~I~($a) { return $this->g~I~($a); }\n", "}\n"},
    {"php", "php", "error", "<?php\nfunction f( {\n", "function g~I~() { h~I~(); }\n", ""},
    {"cpp", "cpp", "class", "class C {\npublic:\n", "  int m~I~(int a) { return g~I~(a); }\n", "};\n"},
    {"cpp", "cpp", "body", "int f() {\n", "  x~I~ = g~I~(x~I~);\n", "}\n"},
    {"c", "c", "module", "", "// d\nint f~I~(int a) { return g~I~(a); }\n", ""},
    {"c", "c", "initializer", "int x[] = {\n", "  ~I~,\n", "};\n"},
    {"ruby", "rb", "class", "class C\n", "  def m~I~(a)\n    g~I~(a)\n  end\n", "end\n"},
    {"swift", "swift", "class", "class C {\n", "  func m~I~() { g~I~() }\n", "}\n"},
    {"scala", "scala", "module", "object O {\n", "  def f~I~(a: Int): Int = g~I~(a)\n", "}\n"},
    {"lua", "lua", "table", "t = {\n", "  k~I~ = g~I~(),\n", "}\n"},
    {"perl", "pm", "module", "package P;\n", "sub f~I~ { return g~I~(@_); }\n", "1;\n"},
    {"groovy", "groovy", "module", "", "def f~I~(a) { g~I~(a) }\n", ""},
    {"objc", "m", "module", "", "int f~I~(int a) { return g~I~(a); }\n", ""},
    {"bash", "sh", "module", "", "f~I~() { g~I~ \"$1\"; }\n", ""},
    {"sql", "sql", "module", "", "CREATE TABLE t~I~ (id INT);\n", ""},
    {"hcl", "tf", "module", "", "resource \"aws_s3_bucket\" \"b~I~\" {\n  bucket = \"b~I~\"\n}\n", ""},
    {"yaml", "yaml", "module", "", "key~I~: value~I~\n", ""},
    {"json", "json", "module", "{\n", "\"k~I~\": ~I~,\n", "\"end\": 0}\n"},
    {"toml", "toml", "tables", "", "[t~I~]\nk = ~I~\n", ""},
    {"markdown", "md", "module", "", "# H~I~\n\ntext ~I~\n\n", ""},
    {"xml", "xml", "module", "<root>\n", "  <e~I~ a=\"~I~\"/>\n", "</root>\n"},
    {"protobuf", "proto", "module", "syntax = \"proto3\";\n", "message M~I~ { int32 f = 1; }\n", ""},
    {"graphql", "graphql", "module", "", "type T~I~ { f: Int }\n", ""},
    {"properties", "properties", "module", "", "k~I~=v~I~\n", ""},
    {"dockerfile", "dockerfile", "module", "FROM x\n", "RUN echo ~I~\n", ""},
    {"ada", "adb", "module", "package body P is\n",
     "   procedure P~I~ is begin null; end P~I~;\n", "end P;\n"},
};

static const pdxe_lookup_counter NAVIGATION[] = {PDXE_LC_NAV_INDEXED, PDXE_LC_NAV_CURSOR,
                                                 PDXE_LC_NAV_PARENT, PDXE_LC_NAV_SIBLING,
                                                 PDXE_LC_NAV_FIELD};
static const char *const NAVIGATION_NAMES[] = {"children reached by position", "cursor steps",
                                               "parent lookups", "sibling lookups",
                                               "field lookups"};
enum { NAVIGATION_KINDS = sizeof NAVIGATION / sizeof NAVIGATION[0] };

typedef struct {
    uint64_t work[PDXE_LC_COUNT];
    int rc;
    uint32_t lost;
    uint32_t defs;
} outcome_t;

static outcome_t extract(const char *language, const char *path, const buf_t *b) {
    outcome_t o;
    memset(&o, 0, sizeof o);
    pdxe_lookup_counts_reset();
    pdxe_file_result *r = NULL;
    o.rc = pdxe_extract_file(ctx, pdxe_language_id(language), path, (const uint8_t *)b->text,
                             b->len, &r);
    pdxe_lookup_counts_read(o.work);
    if (r) {
        o.lost = r->extraction_lost;
        o.defs = r->n_defs;
        pdxe_result_free(ctx, r);
    }
    return o;
}

static void build(buf_t *b, const shape_t *s, int n) {
    b->len = 0;
    put(b, s->prefix);
    for (int i = 0; i < n; i++) {
        put_unit(b, s->unit, i);
    }
    put(b, s->suffix);
}

/* The work at 1,000, 2,000, 4,000 and 8,000 items: each count at most 2.5 times the
 * last (a quadratic count quadruples), plus a little for what the file holds besides
 * the items. `refused`: every collection refused, whose loss must be counted. */
static void linear_shape(const shape_t *s, bool refused) {
    static const int SIZES[] = {1000, 2000, 4000, 8000};
    uint64_t previous[NAVIGATION_KINDS] = {0};
    buf_t b = {0};
    char path[128];
    snprintf(path, sizeof path, "wide/%s.%s", s->name, s->ext);
    for (int k = 0; k < 4; k++) {
        build(&b, s, SIZES[k]);
        outcome_t o = extract(s->language, path, &b);
        CHECK(o.rc == PDXE_OK, "%s %s %d: extraction %d", s->language, s->name, SIZES[k], o.rc);
        /* No child walk cut short (a file's own budgets may still lose work). */
        CHECK(refused || o.work[PDXE_LC_NAV_LOST] == 0, "%s %s %d: %llu child walks cut short",
              s->language, s->name, SIZES[k], (unsigned long long)o.work[PDXE_LC_NAV_LOST]);
        for (int c = 0; c < NAVIGATION_KINDS; c++) {
            uint64_t now = o.work[NAVIGATION[c]];
            CHECK(k == 0 || now <= previous[c] * 5 / 2 + 4096,
                  "%s %s: %s grew from %llu at %d to %llu at %d", s->language, s->name,
                  NAVIGATION_NAMES[c], (unsigned long long)previous[c], SIZES[k - (k > 0)],
                  (unsigned long long)now, SIZES[k]);
            previous[c] = now;
        }
    }
    free(b.text);
}

/* --- the issue's own files ----------------------------------------------------------- */

typedef struct {
    uint32_t errors, widest, span_start, span_end;
    uint64_t nodes;
    /* The parse stopped at its error recovery's bound (issue 93, patches/0038): no tree. */
    bool refused;
    uint64_t recovery_work;
} error_shape_t;

static error_shape_t error_shape(const char *language, const buf_t *b) {
    error_shape_t e = {0};
    TSParser *parser = ts_parser_new();
    ts_parser_set_language(parser, pdxe_ts_language((PDXELanguage)(pdxe_language_id(language) - 1)));
    TSTree *tree = ts_parser_parse_string(parser, NULL, b->text, (uint32_t)b->len);
    e.recovery_work = ts_parser_recovery_work(parser);
    if (!tree) {
        e.refused = ts_parser_recovery_exhausted(parser);
        ts_parser_delete(parser);
        return e;
    }
    TSTreeCursor cursor = ts_tree_cursor_new(ts_tree_root_node(tree));
    for (;;) {
        TSNode node = ts_tree_cursor_current_node(&cursor);
        e.nodes++;
        if (ts_node_is_error(node)) {
            e.errors++;
            uint32_t width = ts_node_child_count(node);
            if (width > e.widest) {
                e.widest = width;
                e.span_start = ts_node_start_byte(node);
                e.span_end = ts_node_end_byte(node);
            }
        }
        if ((ts_tree_cursor_goto_first_child)(&cursor)) {
            continue;
        }
        bool found = false;
        while (!(found = (ts_tree_cursor_goto_next_sibling)(&cursor))) {
            if (!(ts_tree_cursor_goto_parent)(&cursor)) {
                break;
            }
        }
        if (!found) {
            break;
        }
    }
    ts_tree_cursor_delete(&cursor);
    ts_tree_delete(tree);
    ts_parser_delete(parser);
    return e;
}

/* The issue's two files at 1,000 to 8,000 lines: one error node as wide as the file is
 * long, and the extraction's navigation work proportional to the nodes, at every size.
 * The JavaScript file's parse is quadratic in its error node (issue 93): past the
 * recovery bound (patches/0038, 4,000 lines and up here) the parse stops and the file is
 * refused, losing work once, with no definition. */
static void issue_files(void) {
    static const shape_t ISSUE[] = {
        {"kotlin", "kt", "nested classes", "", "class O~I~ { class I~I~ { fun m() { I~I~() } } }\n", ""},
        {"javascript", "js", "unclosed parenthesis", "let x = (\n", "function f~I~() { g~I~(); }\n", ""},
    };
    static const int SIZES[] = {1000, 2000, 4000, 8000};
    buf_t b = {0};
    for (size_t s = 0; s < sizeof ISSUE / sizeof ISSUE[0]; s++) {
        for (int k = 0; k < 4; k++) {
            build(&b, &ISSUE[s], SIZES[k]);
            error_shape_t e = error_shape(ISSUE[s].language, &b);
            outcome_t o = extract(ISSUE[s].language, "issue93", &b);
            if (e.refused) {
                printf("%s %s %d lines: %zu bytes, refused by the recovery bound after %llu "
                       "units of recovery\n",
                       ISSUE[s].language, ISSUE[s].name, SIZES[k], b.len,
                       (unsigned long long)e.recovery_work);
                CHECK(strcmp(ISSUE[s].language, "javascript") == 0,
                      "%s %d: a linear shape was refused", ISSUE[s].language, SIZES[k]);
                CHECK(o.rc == PDXE_OK && o.lost == 1 && o.defs == 0,
                      "%s %d: refused, yet extraction %d, lost %u, %u definitions",
                      ISSUE[s].language, SIZES[k], o.rc, o.lost, o.defs);
                continue;
            }
            uint64_t work = 0;
            for (int c = 0; c < NAVIGATION_KINDS; c++) {
                work += o.work[NAVIGATION[c]];
            }
            printf("%s %s %d lines: %zu bytes, %llu nodes, %u error nodes, the widest %u "
                   "children over bytes %u-%u; navigation %llu (%.1f a node)\n",
                   ISSUE[s].language, ISSUE[s].name, SIZES[k], b.len, (unsigned long long)e.nodes,
                   e.errors, e.widest, e.span_start, e.span_end, (unsigned long long)work,
                   (double)work / (double)e.nodes);
            CHECK(e.widest + 1 >= (uint32_t)SIZES[k] && e.span_end - e.span_start >= b.len / 2,
                  "%s %d: no error node holds the file (widest %u)", ISSUE[s].language, SIZES[k],
                  e.widest);
            CHECK(o.rc == PDXE_OK && o.lost == 0 && o.defs >= (uint32_t)SIZES[k],
                  "%s %d: extraction %d, lost %u, %u definitions", ISSUE[s].language, SIZES[k],
                  o.rc, o.lost, o.defs);
            CHECK(work <= 64 * e.nodes, "%s %d: navigation %llu for %llu nodes",
                  ISSUE[s].language, SIZES[k], (unsigned long long)work,
                  (unsigned long long)e.nodes);
        }
    }
    free(b.text);
}

/* --- a refused collection is lost work -------------------------------------------- */

/* The shapes whose extraction collects children: an error node's (Kotlin's recovery),
 * config files' (the variable walk in reverse), a class body's (decorators before a
 * member, nested classes), a PHP file's (its doc comments). Each completes, counts the
 * refusal, and stays linear: nothing walks the children by position instead. */
static void refused_collections(void) {
    static const char *const REFUSED[] = {"kotlin/error", "yaml/module", "json/module",
                                         "toml/tables",  "typescript/class", "php/module",
                                         "php/class",    "csharp/class",     "javascript/error"};
    for (size_t r = 0; r < sizeof REFUSED / sizeof REFUSED[0]; r++) {
        const shape_t *found = NULL;
        for (size_t s = 0; s < sizeof SHAPES / sizeof SHAPES[0]; s++) {
            char key[64];
            snprintf(key, sizeof key, "%s/%s", SHAPES[s].language, SHAPES[s].name);
            if (strcmp(key, REFUSED[r]) == 0) {
                found = &SHAPES[s];
            }
        }
        if (!found) {
            CHECK(0, "no shape %s", REFUSED[r]);
            continue;
        }
        linear_shape(found, true);
        buf_t b = {0};
        build(&b, found, 1000);
        outcome_t o = extract(found->language, "refused", &b);
        printf("refused %s: %u lost, %llu child walks cut short\n", REFUSED[r], o.lost,
               (unsigned long long)o.work[PDXE_LC_NAV_LOST]);
        free(b.text);
    }
    /* At least the error node's and the config files' collections were refused. */
    buf_t b = {0};
    build(&b, &SHAPES[0], 1);
    for (size_t s = 0; s < sizeof SHAPES / sizeof SHAPES[0]; s++) {
        if (strcmp(SHAPES[s].language, "kotlin") == 0 && strcmp(SHAPES[s].name, "error") == 0) {
            build(&b, &SHAPES[s], 1000);
            outcome_t o = extract("kotlin", "refused", &b);
            CHECK(o.rc == PDXE_OK && o.lost > 0 && o.work[PDXE_LC_NAV_LOST] > 0,
                  "kotlin error refused: extraction %d, %u lost, %llu cut short", o.rc, o.lost,
                  (unsigned long long)o.work[PDXE_LC_NAV_LOST]);
        }
    }
    free(b.text);
}

int main(int argc, char **argv) {
    bool refused = argc > 1 && strcmp(argv[1], "--refused") == 0;
    int first = refused ? 2 : 1;
    if (argc < first + 2) {
        printf("usage: wide_nodes [--refused] <corpus dir> <smoke fixtures dir>\n");
        return 2;
    }
    if (pdxe_init(&ctx) != PDXE_OK) {
        printf("FAIL pdxe_init\n");
        return 1;
    }
    if (refused) {
        refused_collections();
    } else {
        answers_are_tree_sitters(argv[first], argv[first + 1]);
        for (size_t s = 0; s < sizeof SHAPES / sizeof SHAPES[0]; s++) {
            linear_shape(&SHAPES[s], false);
        }
        printf("linear: %zu shapes at 1,000 to 8,000 items\n", sizeof SHAPES / sizeof SHAPES[0]);
        issue_files();
    }
    pdxe_shutdown(ctx);
    if (failures) {
        printf("%d failures\n", failures);
        return 1;
    }
    printf("OK\n");
    return 0;
}
