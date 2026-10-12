/*
 * Grammar scanners keep their state within the parser's buffer, and never end the
 * process (docs/plan/ISSUES.md, issue 91; decision 60;
 * engine/patches/0023-bounded-external-scanner-state.patch).
 *
 *     scanner_state
 *
 * The parser keeps an external scanner's state between tokens in a buffer of
 * TREE_SITTER_SERIALIZATION_BUFFER_SIZE (1,024) bytes, and checks, in debug builds
 * only, that the state fits. Four scanners did not keep it within: YAML's wrote two
 * bytes past it from 254 nested mappings (a 41 KB file crashed the process), Markdown's
 * copied its open blocks past it without a bound, Python's could write one byte past
 * it, and Kotlin's called abort() at 1,025 open strings. All of it happens during the
 * parse, before the depth ceiling (issue 89) is measured.
 *
 * Each now bounds its state to what the buffer holds (the bound derived from the
 * buffer, the header and the bytes an entry takes): a push past it is refused and the
 * token that would make it is not claimed, so the parse recovers; the serializer
 * checks each complete entry before writing it; the deserializer reads only a
 * complete header and complete entries within the length it is given.
 *
 *   - Directly, through the scanners' own code compiled into this test
 *     (tests/scanner_state/): the ceiling, exactly; serialized lengths, with guard
 *     bytes either side of the buffer, for every state size from 0 to past the
 *     ceiling (the state filled past it, too, the way only the old code could); round
 *     trips; every length a deserializer can be handed, from 0 to past the buffer, in
 *     a block of exactly that size.
 *   - End to end, each in a child process, so an abort or a crash is reported rather
 *     than taken: YAML mappings, Markdown block quotes and lists, Kotlin string
 *     templates and Python indentation nested to each side of their ceilings complete,
 *     the same on every run.
 */

#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

#include "pdxe.h"
#include "scanner_state/hooks.h"

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

/* The parser's buffer, between guard bytes the serializer must never touch. */
enum { GUARD = 64, GUARD_BYTE = 0xA5 };

typedef struct {
    unsigned char *block;
    char *buffer;
    size_t size;
} guarded_t;

static guarded_t guarded_new(void) {
    guarded_t g;
    g.size = unit_serialization_buffer_size();
    g.block = (unsigned char *)malloc(g.size + 2 * GUARD);
    memset(g.block, GUARD_BYTE, g.size + 2 * GUARD);
    g.buffer = (char *)g.block + GUARD;
    return g;
}

static bool guards_intact(const guarded_t *g) {
    for (size_t i = 0; i < GUARD; i++) {
        if (g->block[i] != GUARD_BYTE || g->block[GUARD + g->size + i] != GUARD_BYTE) {
            return false;
        }
    }
    return true;
}

/* A copy in a block of exactly `length` bytes, so reading past it is caught. */
static char *exact_copy(const char *bytes, size_t length) {
    char *copy = (char *)malloc(length > 0 ? length : 1);
    if (length > 0) {
        memcpy(copy, bytes, length);
    }
    return copy;
}

/* A scanner whose state is one stack of fixed-size entries after a fixed header. */
typedef struct {
    const char *name;
    size_t header;
    size_t entry;
    size_t max;
    void *(*make)(void);
    void (*drop)(void *);
    unsigned (*serialize)(void *, char *);
    void (*deserialize)(void *, const char *, unsigned);
    size_t (*count)(void *);
    bool (*push)(void *, bool);
} stack_scanner_t;

/* The ceiling is exactly the entries the buffer holds after the header, and a full
 * stack serializes to exactly that. */
static void ceiling(const stack_scanner_t *sc) {
    size_t limit = unit_serialization_buffer_size();
    CHECK(sc->max == (limit - sc->header) / sc->entry, "%s: ceiling %zu is not (%zu - %zu) / %zu",
          sc->name, sc->max, limit, sc->header, sc->entry);
    void *s = sc->make();
    size_t pushed = 0;
    while (pushed <= sc->max + 1 && sc->push(s, false)) {
        pushed++;
    }
    CHECK(pushed == sc->max, "%s: %zu pushes accepted, the ceiling is %zu", sc->name, pushed,
          sc->max);
    CHECK(sc->count(s) == sc->max, "%s: %zu entries after the refusal", sc->name, sc->count(s));
    CHECK(!sc->push(s, false) && sc->count(s) == sc->max, "%s: a push past the ceiling was taken",
          sc->name);
    guarded_t g = guarded_new();
    unsigned length = sc->serialize(s, g.buffer);
    CHECK(length == sc->header + sc->entry * sc->max && length <= limit && guards_intact(&g),
          "%s: a full stack serialized to %u bytes", sc->name, length);
    free(g.block);
    sc->drop(s);
}

/* Every state size from empty to well past the ceiling, filled past it the way only
 * the old code could: never more than the buffer, nothing outside it, complete entries
 * only, and a round trip gives the same state and the same bytes. */
static void sweep(const stack_scanner_t *sc) {
    size_t limit = unit_serialization_buffer_size();
    for (size_t n = 0; n <= sc->max + 16; n++) {
        void *s = sc->make();
        for (size_t i = 0; i < n; i++) {
            sc->push(s, true);
        }
        guarded_t g = guarded_new();
        unsigned length = sc->serialize(s, g.buffer);
        size_t kept = n < sc->max ? n : sc->max;
        CHECK(length <= limit, "%s %zu: serialized %u bytes", sc->name, n, length);
        CHECK(guards_intact(&g), "%s %zu: a write outside the buffer", sc->name, n);
        CHECK(length == sc->header + sc->entry * kept,
              "%s %zu: serialized %u bytes, not a header and %zu complete entries", sc->name, n,
              length, kept);
        char *copy = exact_copy(g.buffer, length);
        void *t = sc->make();
        sc->deserialize(t, copy, length);
        CHECK(sc->count(t) == kept, "%s %zu: %zu entries after a round trip", sc->name, n,
              sc->count(t));
        guarded_t again = guarded_new();
        unsigned length_again = sc->serialize(t, again.buffer);
        CHECK(length_again == length && memcmp(again.buffer, g.buffer, length) == 0,
              "%s %zu: a round trip changed the state", sc->name, n);
        free(again.block);
        free(copy);
        free(g.block);
        sc->drop(t);
        sc->drop(s);
    }
}

/* Every length a deserializer could be handed, from none to past the buffer, its bytes
 * a fixed pattern, in a block of exactly that size: it reads within it, keeps at most
 * the ceiling, and what it keeps serializes within the buffer. */
static void malformed(const stack_scanner_t *sc) {
    size_t limit = unit_serialization_buffer_size();
    unsigned char pattern[2048];
    for (size_t i = 0; i < sizeof pattern; i++) {
        pattern[i] = (unsigned char)(i * 37 + 11);
    }
    for (size_t length = 0; length <= limit + 8; length++) {
        char *copy = exact_copy((const char *)pattern, length);
        void *s = sc->make();
        sc->deserialize(s, copy, (unsigned)length);
        size_t expect = length < sc->header ? 0 : (length - sc->header) / sc->entry;
        if (expect > sc->max) {
            expect = sc->max;
        }
        if (length > limit && sc->header > 0) {
            expect = 0; /* past the buffer, not a state the parser keeps: none of it */
        }
        CHECK(sc->count(s) == expect, "%s: %zu entries from %zu bytes, expected %zu", sc->name,
              sc->count(s), length, expect);
        guarded_t g = guarded_new();
        unsigned out = sc->serialize(s, g.buffer);
        CHECK(out <= limit && guards_intact(&g) && (out - sc->header) % sc->entry == 0,
              "%s: from %zu bytes, serialized %u", sc->name, length, out);
        free(g.block);
        free(copy);
        sc->drop(s);
    }
}

static void stack_scanner(const stack_scanner_t *sc) {
    ceiling(sc);
    sweep(sc);
    malformed(sc);
}

/* Kotlin's string start, at the ceiling: not claimed, the stack untouched. Popping an
 * empty stack is harmless. */
static void kotlin_string_start(void) {
    size_t max = unit_kt_max_delimiters();
    void *s = unit_kt_new();
    for (size_t i = 0; i + 1 < max; i++) {
        unit_kt_push(s, false);
    }
    CHECK(unit_kt_scan_string_start(s, "\"x\"") && unit_kt_delimiters(s) == max,
          "kotlin: the last string start the state holds was refused");
    CHECK(!unit_kt_scan_string_start(s, "\"x\"") && unit_kt_delimiters(s) == max,
          "kotlin: a string start past the ceiling was claimed, or pushed");
    CHECK(!unit_kt_scan_string_start(s, "\"\"\"x\"\"\"") && unit_kt_delimiters(s) == max,
          "kotlin: a triple-quoted string start past the ceiling was claimed, or pushed");
    unit_kt_free(s);
    s = unit_kt_new();
    unit_kt_pop(s);
    CHECK(unit_kt_delimiters(s) == 0, "kotlin: popping an empty stack changed it");
    unit_kt_free(s);
}

/* Python's state is two stacks: one byte for each open string delimiter (at most 255)
 * and two for each indentation level above the first, after two fixed bytes. An odd
 * number of delimiters puts every indentation entry at an odd offset: the old check,
 * `size < 1024` before a two-byte write, wrote one byte past the buffer there. */
static void python_state(void) {
    size_t limit = unit_serialization_buffer_size();
    size_t fixed = unit_py_fixed_bytes();
    size_t indent = unit_py_indent_bytes();
    const size_t delimiter_counts[] = {0, 1, 2, 3, 254, 255};
    for (size_t k = 0; k < sizeof delimiter_counts / sizeof delimiter_counts[0]; k++) {
        size_t d = delimiter_counts[k];
        void *s = unit_py_new();
        for (size_t i = 0; i < d; i++) {
            CHECK(unit_py_push_delimiter(s, false), "python: delimiter %zu of %zu refused", i, d);
        }
        size_t levels = 0;
        while (levels < limit && unit_py_push_indent(s, false)) {
            levels++;
        }
        CHECK(levels == (limit - fixed - d) / indent,
              "python %zu delimiters: %zu indentation levels, the ceiling is %zu", d, levels,
              (limit - fixed - d) / indent);
        guarded_t g = guarded_new();
        unsigned length = unit_py_serialize(s, g.buffer);
        CHECK(length == fixed + d + indent * levels && guards_intact(&g),
              "python %zu delimiters, %zu levels: serialized %u bytes", d, levels, length);
        char *copy = exact_copy(g.buffer, length);
        void *t = unit_py_new();
        unit_py_deserialize(t, copy, length);
        CHECK(unit_py_delimiters(t) == d && unit_py_indents(t) == levels,
              "python %zu delimiters, %zu levels: %zu and %zu after a round trip", d, levels,
              unit_py_delimiters(t), unit_py_indents(t));
        free(copy);
        unit_py_free(t);
        /* Filled past the ceiling as only the old code could: still within the buffer,
         * complete entries only. */
        for (size_t i = 0; i < 64; i++) {
            unit_py_push_indent(s, true);
        }
        memset(g.block, GUARD_BYTE, g.size + 2 * GUARD);
        length = unit_py_serialize(s, g.buffer);
        CHECK(length <= limit && guards_intact(&g) && (length - fixed - d) % indent == 0,
              "python %zu delimiters past the ceiling: serialized %u bytes", d, length);
        free(g.block);
        unit_py_free(s);
    }
    /* At most 255 delimiters, the count's width; and none once the buffer is full. */
    void *s = unit_py_new();
    size_t delimiters = 0;
    while (delimiters < 300 && unit_py_push_delimiter(s, false)) {
        delimiters++;
    }
    CHECK(delimiters == 255, "python: %zu delimiters accepted", delimiters);
    unit_py_free(s);
    s = unit_py_new();
    while (unit_py_push_indent(s, false)) {
    }
    size_t free_bytes = limit - fixed - indent * unit_py_indents(s);
    size_t taken = 0;
    while (taken < 4 && unit_py_push_delimiter(s, false)) {
        taken++;
    }
    CHECK(taken == free_bytes, "python: %zu delimiters taken into %zu free bytes", taken,
          free_bytes);
    unit_py_free(s);
    /* Every length a deserializer could be handed. */
    unsigned char pattern[2048];
    for (size_t i = 0; i < sizeof pattern; i++) {
        pattern[i] = (unsigned char)(i * 37 + 11);
    }
    for (size_t length = 0; length <= limit + 8; length++) {
        char *copy = exact_copy((const char *)pattern, length);
        void *t = unit_py_new();
        unit_py_deserialize(t, copy, (unsigned)length);
        guarded_t g = guarded_new();
        unsigned out = unit_py_serialize(t, g.buffer);
        CHECK(out <= limit && guards_intact(&g), "python: from %zu bytes, serialized %u", length,
              out);
        CHECK(length <= limit || (unit_py_delimiters(t) == 0 && unit_py_indents(t) == 0),
              "python: kept state from %zu bytes, past the buffer", length);
        free(g.block);
        free(copy);
        unit_py_free(t);
    }
}

/* ---- End to end, in a child process ------------------------------------------- */

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
        b->cap = cap;
    }
    memcpy(b->text + b->len, s, n + 1);
    b->len += n;
}

static void put_repeat(buf_t *b, const char *s, size_t times) {
    for (size_t i = 0; i < times; i++) {
        put(b, s);
    }
}

/* `levels` block mappings nested one in another, two spaces deeper each. */
static char *yaml_mappings(size_t levels) {
    buf_t b = {0};
    char line[64];
    for (size_t i = 0; i + 1 < levels; i++) {
        put_repeat(&b, "  ", i);
        snprintf(line, sizeof line, "k%zu:\n", i);
        put(&b, line);
    }
    put_repeat(&b, "  ", levels - 1);
    put(&b, "v: 1\n");
    return b.text;
}

/* `levels` compact sequences on one line: the shortest deep state, `- - - x`. */
static char *yaml_sequences(size_t levels) {
    buf_t b = {0};
    put_repeat(&b, "- ", levels);
    put(&b, "x\n");
    return b.text;
}

static char *markdown_quotes(size_t levels) {
    buf_t b = {0};
    put_repeat(&b, ">", levels);
    put(&b, " text\n");
    return b.text;
}

static char *markdown_lists(size_t levels) {
    buf_t b = {0};
    for (size_t i = 0; i < levels; i++) {
        put_repeat(&b, "  ", i);
        put(&b, "- item\n");
    }
    return b.text;
}

static char *kotlin_templates(size_t levels) {
    buf_t b = {0};
    put(&b, "fun m() { val s = ");
    put_repeat(&b, "\"${", levels);
    put(&b, "1");
    put_repeat(&b, "}\"", levels);
    put(&b, " }\n");
    return b.text;
}

/* `levels` nested blocks, one space deeper each. */
static char *python_blocks(size_t levels) {
    buf_t b = {0};
    put(&b, "def f(x):\n");
    for (size_t i = 1; i <= levels; i++) {
        put_repeat(&b, " ", i);
        put(&b, i == levels ? "return x\n" : "if x:\n");
    }
    return b.text;
}

/* Extracts in a child: whether it finished normally, and what it found. */
static bool child_extract(const char *language, const char *path, const char *source,
                          char *summary, size_t cap) {
    int fds[2];
    if (pipe(fds) != 0) {
        return false;
    }
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0) {
        close(fds[0]);
        pdxe_ctx *ctx = NULL;
        pdxe_file_result *r = NULL;
        char line[256] = "no result";
        if (pdxe_init(&ctx) == PDXE_OK &&
            pdxe_extract_file(ctx, pdxe_language_id(language), path, (const uint8_t *)source,
                              strlen(source), &r) == PDXE_OK &&
            r) {
            snprintf(line, sizeof line, "status %d lost %u defs %u calls %u usages %u",
                     r->status, r->extraction_lost, r->n_defs, r->n_calls, r->n_usages);
        }
        ssize_t written = write(fds[1], line, strlen(line));
        _exit(written > 0 ? 0 : 3);
    }
    close(fds[1]);
    ssize_t n = read(fds[0], summary, cap - 1);
    summary[n > 0 ? n : 0] = '\0';
    close(fds[0]);
    int status = 0;
    if (waitpid(pid, &status, 0) != pid) {
        return false;
    }
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        snprintf(summary, cap, "%s %d", WIFSIGNALED(status) ? "signal" : "exit",
                 WIFSIGNALED(status) ? WTERMSIG(status) : WEXITSTATUS(status));
        return false;
    }
    return true;
}

static void end_to_end(const char *what, const char *language, const char *path,
                       char *(*shape)(size_t), size_t levels) {
    char *source = shape(levels);
    char first[256], second[256];
    bool ok = child_extract(language, path, source, first, sizeof first);
    CHECK(ok, "%s %zu: the extraction did not finish (%s)", what, levels, first);
    if (ok) {
        CHECK(child_extract(language, path, source, second, sizeof second) &&
                  strcmp(first, second) == 0,
              "%s %zu: not the same on every run (%s, then %s)", what, levels, first, second);
        printf("  %s %zu: %s\n", what, levels, first);
    }
    free(source);
}

static const stack_scanner_t *unit_scanner;

static void unit_stack_scanner(void) { stack_scanner(unit_scanner); }

/* Runs one scanner's checks in a child, so that an abort or a crash in the scanner's
 * own code is reported as such rather than ending the test. */
static void in_child(const char *what, void (*checks)(void)) {
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0) {
        failures = 0; /* the child's own checks only */
        checks();
        fflush(stdout);
        _exit(failures ? 1 : 0);
    }
    int status = 0;
    if (pid < 0 || waitpid(pid, &status, 0) != pid) {
        CHECK(0, "%s: could not run", what);
        return;
    }
    CHECK(WIFEXITED(status) && WEXITSTATUS(status) == 0, "%s: %s %d", what,
          WIFSIGNALED(status) ? "ended by signal" : "failed, exit",
          WIFSIGNALED(status) ? WTERMSIG(status) : WEXITSTATUS(status));
}

int main(void) {
    const stack_scanner_t yaml = {"yaml", unit_yaml_header_bytes(), unit_yaml_entry_bytes(),
                                  unit_yaml_max_entries(), unit_yaml_new, unit_yaml_free,
                                  unit_yaml_serialize, unit_yaml_deserialize, unit_yaml_entries,
                                  unit_yaml_push};
    const stack_scanner_t markdown = {"markdown", unit_md_header_bytes(), unit_md_block_bytes(),
                                      unit_md_max_blocks(), unit_md_new, unit_md_free,
                                      unit_md_serialize, unit_md_deserialize, unit_md_blocks,
                                      unit_md_push};
    const stack_scanner_t kotlin = {"kotlin", 0, 1, unit_kt_max_delimiters(), unit_kt_new,
                                    unit_kt_free, unit_kt_serialize, unit_kt_deserialize,
                                    unit_kt_delimiters, unit_kt_push};
    printf("  ceilings: yaml %zu entries, markdown %zu blocks, kotlin %zu delimiters\n",
           yaml.max, markdown.max, kotlin.max);

    unit_scanner = &yaml;
    in_child("yaml state", unit_stack_scanner);
    unit_scanner = &markdown;
    in_child("markdown state", unit_stack_scanner);
    unit_scanner = &kotlin;
    in_child("kotlin state", unit_stack_scanner);
    in_child("kotlin string start", kotlin_string_start);
    in_child("python state", python_state);

    /* YAML: 253 entries fill the state; 254 was the old crash, from 510 bytes of compact
     * sequences. */
    const size_t yaml_levels[] = {252, 253, 254, 400};
    for (size_t i = 0; i < sizeof yaml_levels / sizeof yaml_levels[0]; i++) {
        end_to_end("yaml mappings", "yaml", "config/deep.yaml", yaml_mappings, yaml_levels[i]);
        end_to_end("yaml sequences", "yaml", "config/seq.yaml", yaml_sequences, yaml_levels[i]);
    }
    const size_t markdown_levels[] = {253, 254, 255, 300};
    for (size_t i = 0; i < sizeof markdown_levels / sizeof markdown_levels[0]; i++) {
        end_to_end("markdown quotes", "markdown", "docs/deep.md", markdown_quotes,
                   markdown_levels[i]);
        end_to_end("markdown lists", "markdown", "docs/lists.md", markdown_lists,
                   markdown_levels[i]);
    }
    /* Kotlin: 1,025 open templates is the old abort. */
    const size_t kotlin_levels[] = {1023, 1024, 1025, 1100};
    for (size_t i = 0; i < sizeof kotlin_levels / sizeof kotlin_levels[0]; i++) {
        end_to_end("kotlin templates", "kotlin", "src/main/kotlin/Deep.kt", kotlin_templates,
                   kotlin_levels[i]);
    }
    /* Python: 511 levels above the first fill the state. */
    const size_t python_levels[] = {510, 511, 512, 520};
    for (size_t i = 0; i < sizeof python_levels / sizeof python_levels[0]; i++) {
        end_to_end("python blocks", "python", "app/deep.py", python_blocks, python_levels[i]);
    }

    if (failures) {
        printf("%d failure(s)\n", failures);
        return 1;
    }
    printf("scanner_state: ok\n");
    return 0;
}
