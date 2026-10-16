/*
 * The engine's public interface, implemented over the vendored engine.
 *
 * This file translates. The engine has its own model: arena-owned results, a
 * language enumeration starting at zero, kind labels of its own choosing, qualified
 * names that begin with a project name, and positions that are lines for some facts
 * and byte ranges for others. The interface in include/pdxe.h is the model the rest
 * of this project uses. Everything here is the difference between the two, and each
 * difference is stated where it is bridged.
 */

#include <limits.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "pdxe.h"
#include "pdxe_core.h"
#include "foundation/compat.h"
#include "foundation/hash_table.h"
#include "foundation/log.h"
#include "foundation/str_util.h"
#include "lsp/rust_cargo.h"
#include "pipeline/lsp_resolve.h"
#include "pipeline/lsp_surface.h"
#include "pipeline/pass_lsp_cross.h"
#include "pipeline/pipeline_internal.h"
#include "lookup_counts.h"
#include "lost_work.h"
#include "shim_internal.h"

/* --- versions and the project sentinel -------------------------------------- */

/*
 * Bumped whenever the engine's output can change for the same input: a vendoring
 * refresh, a patch, a change to this translation. It is part of every cache key, so
 * a cached extraction from an older engine is never mistaken for a current one.
 */
/* 2: sites carry their node-type path (issue 46). 3: definitions carry every route
 * binding with its declaring node (issue 54). 4: the C and C++ expression evaluator's
 * per-file step budget grows with the source, 10000 steps per started 32 KiB up to
 * 320000 (issue 74, patch 0012). 5: sites inside a nested callable the extractor does
 * not represent belong to the nearest represented callable, whose node their path
 * starts at, while keeping their nested lexical scope (issue 75, patch 0013). 6: a
 * site's owner is joined to its definition by the declaration occurrence of the
 * callable around it, never by comparing qualified names (issue 76, patch 0014). 7: C
 * and C++ preprocessing reads nothing but the source given, never a file an #include
 * or __has_include names (issue 83, patch 0015), nor a clock: __DATE__ and __TIME__ are
 * fixed (issue 84, patch 0016), nor the platform: sizeof in #if has one fixed data model
 * (issue 85, patch 0017), and character constants and #line numbers are read by fixed
 * rules (issue 86, patch 0018), nor the machine's memory or speed: its work is bounded
 * by the source, and its #if arithmetic is defined (issue 87, patch 0019), and a file's
 * site node-type paths are bounded, complete or absent (issue 88, patch 0020), and a
 * tree nested deeper than 512 is not extracted (issue 89, patch 0021). 8: a bare Rust
 * call of a function its block declares is locally bound, and a Rust file's per-file
 * typed answers, which its surface holds, need path evidence and leave the names a
 * `match` arm or a block binds alone (issue 104, patch 0028); engine 7 cached the
 * extractions before it under the same key (issue 112). 9: a Rust file's `use`
 * declarations are read from its syntax tree, so what nested and multi-line lists,
 * aliases, `self` and `pub use` bind reaches its per-file typed answers and the calls
 * they add (issue 111, patch 0029). 10: a Rust file's extraction records the members
 * its `macro_rules!` invocations may write, which no definition represents (issue 110,
 * patch 0030). 11: a Rust `use` binds only in its lexical scope (an inline module's, a
 * block's), so what a file's per-file typed answers and the calls they add see changes
 * (issue 114, patch 0031). 12: a Rust `Type::method` answers a trait impl's method only
 * when the impl is proven to apply, and a Rust file's surface (version 6) carries its
 * trait impls' headers (issue 109, patch 0032). 13: a Rust call whose function is a
 * turbofish path or method (`f::<T>()`, `x.m::<T>()`) is extracted as a call of that path
 * or method, its type arguments no part of the callee (issue 107A, patch 0033). */
static const char ENGINE_VERSION_STRING[] = "13";

const char *pdxe_version(void) {
    return ENGINE_VERSION_STRING;
}

/*
 * The engine makes a project name the first segment of every qualified name. The
 * interface has no project, and the qualified names it promises carry none: they
 * are the language's own. So names are computed under this sentinel and returned
 * without it. The sentinel is only ever added once, as the first segment, so
 * removing exactly one leading copy is unambiguous even for a repository that
 * happens to contain a directory of the same name.
 */
static const char PROJECT_SENTINEL[] = "__pdx__";

/*
 * The language's own qualified name, from one the engine computed. Returns a pointer
 * into the same string: removing a prefix needs no copy, so ownership is unchanged.
 */
static const char *strip_project(const char *qn) {
    if (!qn) {
        return NULL;
    }
    size_t n = sizeof(PROJECT_SENTINEL) - 1;
    if (strncmp(qn, PROJECT_SENTINEL, n) == 0) {
        if (qn[n] == '.') {
            return qn + n + 1;
        }
        if (qn[n] == '\0') {
            return qn + n;
        }
    }
    return qn;
}

/* --- lifecycle -------------------------------------------------------------- */

struct pdxe_ctx {
    int unused; /* parser state is the engine's, thread-local; the context marks a thread */
};

/*
 * The engine's own initialisation is process-wide, while contexts are per thread.
 * Counting contexts keeps the two consistent: the first context starts the engine,
 * the last one stops it, and a thread finishing early never stops it under another.
 */
static atomic_int live_contexts = 0;

/*
 * Where the engine's log lines go: nowhere. The engine is a library inside a program
 * that speaks a protocol on its standard streams, and a stray line there corrupts
 * that protocol. A replacing sink is used rather than only lowering the level,
 * because the engine reads a log level from the environment as it initialises, and a
 * level can be raised again that way; a sink replaces the destination whatever the
 * level. Diagnostics the caller needs come back through return values instead.
 */
static void discard_log_line(const char *line) {
    (void)line;
}

int pdxe_init(pdxe_ctx **out) {
    if (!out) {
        return PDXE_E_INVALID;
    }
    *out = NULL;
    pdxe_ctx *ctx = (pdxe_ctx *)calloc(1, sizeof(*ctx));
    if (!ctx) {
        return PDXE_E_NOMEM;
    }
    if (atomic_fetch_add(&live_contexts, 1) == 0) {
        if (pdxe_engine_init() != 0) {
            atomic_fetch_sub(&live_contexts, 1);
            free(ctx);
            return PDXE_E_INTERNAL;
        }
        /* After the engine's own initialisation, which reads the environment, so
         * that this is the last word on where its output goes. */
        pdxe_log_set_sink_ex(discard_log_line, PDXE_LOG_SINK_REPLACE);
        pdxe_log_set_level(PDXE_LOG_NONE);
    }
    *out = ctx;
    return PDXE_OK;
}

void pdxe_shutdown(pdxe_ctx *ctx) {
    if (!ctx) {
        return;
    }
    free(ctx);
    if (atomic_fetch_sub(&live_contexts, 1) == 1) {
        pdxe_engine_shutdown();
    }
}

/* --- languages -------------------------------------------------------------- */

/*
 * The matrix's identifiers, and the typescript dialect that needs its own grammar.
 *
 * The engine numbers languages from zero, and the interface promises that zero
 * means unknown. So the number handed out is the engine's plus one, and extraction
 * subtracts it again. Without the offset the first language in the engine's list
 * would be indistinguishable from an unknown one.
 */
typedef struct {
    const char *id;
    PDXELanguage lang;
} language_entry;

static const language_entry LANGUAGES[] = {
    {"java", PDXE_LANG_JAVA},         {"kotlin", PDXE_LANG_KOTLIN},
    {"scala", PDXE_LANG_SCALA},       {"typescript", PDXE_LANG_TYPESCRIPT},
    {"tsx", PDXE_LANG_TSX},           {"javascript", PDXE_LANG_JAVASCRIPT},
    {"python", PDXE_LANG_PYTHON},     {"go", PDXE_LANG_GO},
    {"c", PDXE_LANG_C},               {"cpp", PDXE_LANG_CPP},
    {"csharp", PDXE_LANG_CSHARP},     {"rust", PDXE_LANG_RUST},
    {"php", PDXE_LANG_PHP},           {"perl", PDXE_LANG_PERL},
    {"ada", PDXE_LANG_ADA},           {"bash", PDXE_LANG_BASH},
    {"ruby", PDXE_LANG_RUBY},         {"swift", PDXE_LANG_SWIFT},
    {"objc", PDXE_LANG_OBJC},         {"groovy", PDXE_LANG_GROOVY},
    {"lua", PDXE_LANG_LUA},           {"sql", PDXE_LANG_SQL},
    {"protobuf", PDXE_LANG_PROTOBUF}, {"graphql", PDXE_LANG_GRAPHQL},
    {"yaml", PDXE_LANG_YAML},         {"json", PDXE_LANG_JSON},
    {"toml", PDXE_LANG_TOML},         {"hcl", PDXE_LANG_HCL},
    {"dockerfile", PDXE_LANG_DOCKERFILE},
    {"markdown", PDXE_LANG_MARKDOWN}, {"xml", PDXE_LANG_XML},
    {"properties", PDXE_LANG_PROPERTIES},
};

int pdxe_language_id(const char *lang_id) {
    if (!lang_id) {
        return 0;
    }
    for (size_t i = 0; i < sizeof(LANGUAGES) / sizeof(LANGUAGES[0]); i++) {
        if (strcmp(lang_id, LANGUAGES[i].id) == 0) {
            return (int)LANGUAGES[i].lang + 1;
        }
    }
    return 0;
}

/* The engine's language for an interface number, or -1 when it is not one. */
static int engine_language(int lang) {
    if (lang <= 0 || lang > (int)PDXE_LANG_COUNT) {
        return -1;
    }
    return lang - 1;
}

/* --- kinds ------------------------------------------------------------------ */

/*
 * The engine's kind labels, normalised to the interface's. A label with no entry
 * becomes "variable", and the caller still has the engine's own label, so nothing
 * is lost by the normalisation, only made uniform.
 *
 * "Object" is the singleton declaration some languages have; it is a class in every
 * sense the graph cares about. "Constant" is a module-level binding.
 */
static const char *normalise_kind(const char *label) {
    static const struct {
        const char *engine;
        const char *ours;
    } KINDS[] = {
        {"Class", "class"},           {"Interface", "interface"},
        {"Enum", "enum"},             {"Struct", "struct"},
        {"Trait", "trait"},           {"Type", "type_alias"},
        {"TypeAlias", "type_alias"},  {"Function", "function"},
        {"Method", "method"},         {"Constructor", "constructor"},
        {"Field", "field"},           {"Property", "field"},
        {"Variable", "variable"},     {"Constant", "variable"},
        {"Macro", "macro"},           {"Module", "module"},
        {"Object", "class"},
    };
    if (!label) {
        return "variable";
    }
    for (size_t i = 0; i < sizeof(KINDS) / sizeof(KINDS[0]); i++) {
        if (strcmp(label, KINDS[i].engine) == 0) {
            return KINDS[i].ours;
        }
    }
    return "variable";
}

/* --- positions -------------------------------------------------------------- */

/*
 * Line starts of a source, so a byte offset can be turned into a line and column.
 * Lines and columns are 1-based. Columns count bytes, not characters: the interface
 * promises byte offsets throughout, and a column that counted characters would
 * disagree with them on any line containing a multi-byte character.
 */
typedef struct {
    uint32_t *starts;
    uint32_t count;
    uint32_t len;
} line_index;

static int line_index_build(line_index *li, const uint8_t *bytes, size_t len) {
    li->len = (uint32_t)len;
    li->count = 1;
    for (size_t i = 0; i < len; i++) {
        if (bytes[i] == '\n') {
            li->count++;
        }
    }
    li->starts = (uint32_t *)malloc((size_t)li->count * sizeof(uint32_t));
    if (!li->starts) {
        return -1;
    }
    uint32_t line = 0;
    li->starts[line++] = 0;
    for (size_t i = 0; i < len; i++) {
        if (bytes[i] == '\n' && line < li->count) {
            li->starts[line++] = (uint32_t)(i + 1);
        }
    }
    return 0;
}

static void line_index_free(line_index *li) {
    free(li->starts);
    li->starts = NULL;
}

/* 1-based line and column of a byte offset. */
static void position_of(const line_index *li, uint32_t byte, uint32_t *line, uint32_t *col) {
    uint32_t lo = 0, hi = li->count;
    while (hi - lo > 1) {
        uint32_t mid = lo + (hi - lo) / 2;
        if (li->starts[mid] <= byte) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    *line = lo + 1;
    *col = byte - li->starts[lo] + 1;
}

/* A span from a byte range. */
static pdxe_span span_of_bytes(const line_index *li, uint32_t start, uint32_t end) {
    pdxe_span s;
    memset(&s, 0, sizeof(s));
    if (!li || end < start || start > li->len) {
        return s;
    }
    if (end > li->len) {
        end = li->len;
    }
    s.start_byte = start;
    s.end_byte = end;
    position_of(li, start, &s.start_line, &s.start_col);
    position_of(li, end, &s.end_line, &s.end_col);
    return s;
}

/*
 * A span from a range of lines. The engine records definitions by line only, so
 * their spans cover whole lines: from the first byte of the first to the end of the
 * last. That is what the engine knows, stated as exactly as it knows it.
 */
static pdxe_span span_of_lines(const line_index *li, uint32_t first, uint32_t last) {
    pdxe_span s;
    memset(&s, 0, sizeof(s));
    if (!li || first == 0 || first > li->count) {
        return s;
    }
    if (last < first) {
        last = first;
    }
    if (last > li->count) {
        last = li->count;
    }
    s.start_byte = li->starts[first - 1];
    s.end_byte = last < li->count ? li->starts[last] : li->len;
    /* Exclude the line's own terminator, which belongs to no definition. */
    if (s.end_byte > s.start_byte && last < li->count) {
        s.end_byte--;
    }
    s.start_line = first;
    s.start_col = 1;
    s.end_line = last;
    s.end_col = s.end_byte - li->starts[last - 1] + 1;
    return s;
}

/* --- text ------------------------------------------------------------------- */

/* The source a result's ranges are ranges of (issue 90). */
typedef struct {
    const char *bytes;
    size_t len;
} source_view;

/*
 * A fact's text as the interface gives it (issue 90, decision 61). When the fact was
 * found in the file's own source and `s` is exactly the bytes that begin at `start`
 * inside the fact's site [start, limit), it is the range of those bytes: the text is
 * then the source's, and costs a range whatever its length. Otherwise it is `s`, held
 * by the result as before: text the engine assembled or rewrote, or read in C-family
 * source after macro expansion. NULL is no text.
 */
static pdxe_text text_of(const source_view *src, const char *s, bool raw, uint32_t start,
                         uint32_t limit) {
    pdxe_text t = {0};
    if (!s) {
        return t;
    }
    uint32_t end = 0;
    if (raw && src && pdxe_text_in_site(src->bytes, src->len, s, start, limit, &end)) {
        t.kind = PDXE_TEXT_SOURCE;
        t.start = start;
        t.end = end;
        return t;
    }
    t.kind = PDXE_TEXT_OWNED;
    t.owned = s;
    return t;
}

/* The owned text `s`; NONE for NULL. */
static pdxe_text owned_text(const char *s) {
    pdxe_text t = {0};
    if (s) {
        t.kind = PDXE_TEXT_OWNED;
        t.owned = s;
    }
    return t;
}

/* --- results ---------------------------------------------------------------- */

/*
 * What the interface hands out, and what it must free. The public structure is the
 * first member, so the pointer the caller holds is also a pointer to this.
 *
 * A result from extraction keeps the engine's result alive and borrows its strings:
 * the engine's arena already owns them, and copying every one would double the
 * memory for nothing. A result rebuilt from a cache has no engine result, so it owns
 * copies instead, in `owned`.
 */
/* Issue 92: the positions of a result's definitions by qualified name (see
 * qn_positions_open below). */
typedef struct qn_positions qn_positions;
static void qn_positions_free(qn_positions *x);

typedef struct {
    pdxe_file_result pub;
    PDXEFileResult *engine;
    char **owned;
    size_t n_owned;
    size_t cap_owned;
    /* Set while a project is resolving through this result; see add_file. */
    bool in_project;
    /* The interface language it was extracted as; 0 for a result rebuilt from a cache. */
    int lang;
    /* What extraction lost on this file: failed allocations and work budgets run out
     * (lost_work.h). Typed resolution happens partly during extraction, and its
     * answers are reported by a run, so a run counts this as its own loss. */
    uint32_t lost;
    /* The calls' arguments, one array the calls point into (issue 47). */
    pdxe_call_arg *call_args;
    /* The definitions' routes, one array the definitions point into (issue 54). */
    pdxe_route *routes;
    /* For a result from extraction, its copy of the source, which its ranges are ranges
     * of: the surface is exported from it (issue 90). NULL for a rebuilt result. */
    char *source;
    size_t source_len;
    /* While the result is translated, its definitions' positions by qualified name;
     * NULL once translation ends. */
    qn_positions *names;
} result_holder;

static void holder_free(result_holder *h) {
    if (!h) {
        return;
    }
    free(h->pub.defs);
    free(h->pub.calls);
    free(h->pub.imports);
    free(h->pub.usages);
    free(h->pub.types);
    free(h->pub.rws);
    free(h->pub.channels);
    free(h->pub.envs);
    free(h->pub.diags);
    free(h->pub.throws);
    free(h->pub.impl_traits);
    free(h->pub.macro_members);
    free(h->pub.use_scopes);
    free(h->pub.scoped_imports);
    free(h->call_args);
    free(h->routes);
    free(h->source);
    qn_positions_free(h->names);
    if (h->engine) {
        pdxe_free_result(h->engine);
    }
    for (size_t i = 0; i < h->n_owned; i++) {
        free(h->owned[i]);
    }
    free(h->owned);
    free(h);
}

/* Hands `s`, already a heap copy, to the holder to free. False when it cannot. */
static bool adopt(result_holder *h, char *s) {
    if (h->n_owned == h->cap_owned) {
        size_t cap = h->cap_owned ? h->cap_owned * 2 : 64;
        char **grown = (char **)realloc(h->owned, cap * sizeof(*grown));
        if (!grown) {
            return false;
        }
        h->owned = grown;
        h->cap_owned = cap;
    }
    h->owned[h->n_owned++] = s;
    return true;
}

const PDXEFileResult *pdxe_result_engine(const pdxe_file_result *r) {
    return r ? ((const result_holder *)r)->engine : NULL;
}

void pdxe_result_free(pdxe_ctx *ctx, pdxe_file_result *r) {
    (void)ctx;
    holder_free((result_holder *)r);
}

/* One step of a qualified name towards its whole one: the bytes this definition adds
 * (`dot` before `text` for a member), the definition whose name they follow, or
 * PDXE_NO_PARENT once the name is whole, and for a prefix how much of that name they
 * follow. False for a malformed step. */
typedef struct {
    const char *text;
    bool dot;
    uint32_t next;
    bool prefix; /* the bytes follow only the first `keep` of the next name */
    uint32_t keep;
} qualified_name_step_t;

static bool qualified_name_step(const pdxe_definition *defs, uint32_t i,
                                qualified_name_step_t *step) {
    const pdxe_definition *d = &defs[i];
    memset(step, 0, sizeof(*step));
    step->next = PDXE_NO_PARENT;
    switch (d->qn_form) {
    case PDXE_QN_WHOLE:
        step->text = d->qn_text;
        return d->qn_text != NULL;
    case PDXE_QN_MEMBER:
        step->text = d->name;
        step->dot = true;
        step->next = d->qn_base;
        return d->name != NULL && d->qn_base < i;
    case PDXE_QN_EXTENDED:
        step->text = d->qn_text;
        step->next = d->qn_base;
        return d->qn_text != NULL && d->qn_base < i;
    case PDXE_QN_PREFIX:
        step->text = d->qn_text;
        step->next = d->qn_base;
        step->prefix = true;
        step->keep = d->qn_keep;
        return d->qn_text != NULL && d->qn_base < i &&
               (d->qn_text[0] == '\0' || d->qn_text[0] == '.');
    default:
        return false;
    }
}

/* The length of definition `i`'s qualified name, walking only until a whole name or a
 * prefix, whose length its own bytes give. False for a malformed step on the way. */
static bool qualified_name_len(const pdxe_definition *defs, uint32_t i, size_t *len) {
    size_t total = 0;
    for (;;) {
        qualified_name_step_t step;
        if (!qualified_name_step(defs, i, &step)) {
            return false;
        }
        total += strlen(step.text) + (step.dot ? 1 : 0);
        if (step.next == PDXE_NO_PARENT) {
            break;
        }
        if (step.prefix) {
            total += step.keep;
            break;
        }
        i = step.next;
    }
    *len = total;
    return true;
}

/* Whether the first `keep` bytes of definition `b`'s name, `len` bytes long, end before
 * a `.` that `b` adds itself, after its own base's name: read in `b`'s own bytes, so the
 * check costs the same however long the chain behind `b` is (issue 90). */
static bool qualified_name_prefix_valid(const pdxe_definition *defs, uint32_t b, uint32_t keep,
                                        size_t len) {
    qualified_name_step_t step;
    if (!qualified_name_step(defs, b, &step) || keep >= len) {
        return false;
    }
    size_t added = strlen(step.text) + (step.dot ? 1 : 0);
    size_t own = step.prefix ? step.keep : len - added; /* where `b`'s own bytes begin */
    if (keep <= own) {
        return false;
    }
    size_t at = keep - own;
    char byte = step.dot ? (at == 0 ? '.' : step.text[at - 1]) : step.text[at];
    return byte == '.';
}

int pdxe_qualified_name(const pdxe_definition *defs, uint32_t n_defs, uint32_t index, char *out,
                        size_t cap, size_t *len) {
    if (!defs || !len || index >= n_defs || (cap > 0 && !out)) {
        return PDXE_E_INVALID;
    }
    size_t total = 0;
    if (!qualified_name_len(defs, index, &total)) {
        return PDXE_E_INVALID;
    }
    /* Then the bytes, each step's written where it ends up, as many as fit: `whole` is
     * the length of the name at hand, `seen` how much of it the name asked for uses. A
     * prefix step sees only the first `keep` bytes of the next name, whose length is
     * walked for once more. Every step is checked, whether or not anything is written. */
    size_t limit = cap == 0 ? 0 : total < cap - 1 ? total : cap - 1;
    size_t whole = total;
    size_t seen = total;
    for (uint32_t i = index;;) {
        qualified_name_step_t step;
        (void)qualified_name_step(defs, i, &step);
        size_t n = strlen(step.text);
        size_t start = step.prefix ? step.keep : whole - n - (step.dot ? 1 : 0);
        for (size_t at = start; at < seen; at++) {
            size_t k = at - start;
            char byte = step.dot ? (k == 0 ? '.' : step.text[k - 1]) : step.text[k];
            if (at < limit) {
                out[at] = byte;
            }
        }
        if (step.next == PDXE_NO_PARENT) {
            break;
        }
        if (step.prefix) {
            if (!qualified_name_len(defs, step.next, &whole) ||
                !qualified_name_prefix_valid(defs, step.next, step.keep, whole)) {
                return PDXE_E_INVALID;
            }
        } else {
            whole = start;
        }
        seen = seen < start ? seen : start;
        i = step.next;
    }
    *len = total;
    if (cap > 0) {
        out[limit] = '\0';
    }
    return PDXE_OK;
}

/* Allocates an array of `n`, or returns a non-NULL empty sentinel when n is zero. */
#define ALLOC_ARRAY(type, n) ((type *)calloc((n) ? (size_t)(n) : 1, sizeof(type)))

/*
 * Whether the engine made a definition up rather than found it in the file.
 *
 * To resolve calls on built-in types, the engine adds the standard library's
 * definitions to a file's result, recorded against a path in angle brackets rather
 * than the file's own. They are scaffolding for its resolver, which registers them
 * again itself when resolution runs. Reported as the file's definitions, every file
 * in the language would claim to define the same dozen built-ins.
 */
static bool is_synthetic(const PDXEDefinition *d) {
    return d->file_path != NULL && d->file_path[0] == '<';
}

/*
 * The position, in the interface's definition array, of the definition whose
 * qualified name is `qn`, or PDXE_NO_PARENT.
 *
 * Facts name their enclosing function by the engine's qualified name; the interface
 * refers to it by position in its own array, which omits synthetic definitions and so
 * does not line up with the engine's. Counted over the engine's definitions, skipping
 * the synthetic ones exactly as the translation does, so the project can answer the
 * same question for a result it never translated. Names are compared without the
 * project sentinel, and the first definition with a name is the one found.
 *
 * Issue 92: built once over a result's definitions, from the engine's own strings, and
 * looked up by hash: scanning the definitions for every fact that names one made a
 * file's translation quadratic in its definitions. Test builds keep the scan for
 * reference (PDX_ENGINE_TEST_LINEAR_LOOKUP_REFERENCE).
 */
struct qn_positions {
    const PDXEDefArray *defs;
    uint32_t *position; /* engine definition -> interface position, PDXE_NO_PARENT if omitted */
    int *slots;         /* open addressing: the first engine definition with a name; -1 empty */
    uint64_t *hashes;   /* the name's hash, per slot */
    size_t cap;         /* a power of two, at least twice the names */
    bool reference;     /* test builds: answer by scanning, as before issue 92 */
};

static uint64_t qn_hash(const char *s) {
    uint64_t h = 1469598103934665603ULL;
    for (; *s; s++) {
        h = (h ^ (unsigned char)*s) * 1099511628211ULL;
    }
    return h;
}

/* The slot of `qn` (stripped, hashed h): where it is, or the empty slot it would take. */
static size_t qn_slot(const qn_positions *x, const char *qn, uint64_t h) {
    size_t mask = x->cap - 1;
    for (size_t at = (size_t)h & mask;; at = (at + 1) & mask) {
        int d = x->slots[at];
        PDXE_LOOKUP_COUNT(PDXE_LC_DEF_ENTRIES);
        if (d < 0) {
            return at;
        }
        if (x->hashes[at] == h) {
            PDXE_LOOKUP_COUNT(PDXE_LC_IFACE_COMPARES);
            if (strcmp(strip_project(x->defs->items[d].qualified_name), qn) == 0) {
                return at;
            }
        }
    }
}

static void qn_positions_free(qn_positions *x) {
    if (!x) {
        return;
    }
    free(x->position);
    free(x->slots);
    free(x->hashes);
    free(x);
}

/* The positions of `defs` by qualified name; NULL when they cannot be allocated, which
 * the caller reports as such. */
static qn_positions *qn_positions_open(const PDXEDefArray *defs) {
    if (PDXE_LOOKUP_INDEX_REFUSED("interface")) {
        return NULL;
    }
    qn_positions *x = (qn_positions *)calloc(1, sizeof(*x));
    if (!x) {
        return NULL;
    }
    x->defs = defs;
    size_t n = defs->count > 0 ? (size_t)defs->count : 1;
    x->position = (uint32_t *)calloc(n, sizeof(uint32_t));
    x->cap = 16;
    while (x->cap < 2 * n) {
        x->cap *= 2;
    }
    x->slots = (int *)malloc(x->cap * sizeof(int));
    x->hashes = (uint64_t *)calloc(x->cap, sizeof(uint64_t));
    if (!x->position || !x->slots || !x->hashes) {
        qn_positions_free(x);
        return NULL;
    }
    for (size_t i = 0; i < x->cap; i++) {
        x->slots[i] = -1;
    }
#if defined(PDXE_ENABLE_TEST_SEAMS)
    const char *reference = getenv("PDX_ENGINE_TEST_LINEAR_LOOKUP_REFERENCE");
    x->reference = reference && reference[0];
#endif
    uint32_t kept = 0;
    for (int i = 0; i < defs->count; i++) {
        const PDXEDefinition *d = &defs->items[i];
        if (is_synthetic(d)) {
            x->position[i] = PDXE_NO_PARENT;
            continue;
        }
        x->position[i] = kept++;
        if (!d->qualified_name) {
            continue;
        }
        const char *qn = strip_project(d->qualified_name);
        uint64_t h = qn_hash(qn);
        size_t at = qn_slot(x, qn, h);
        if (x->slots[at] < 0) { /* the first definition with the name keeps it */
            x->slots[at] = i;
            x->hashes[at] = h;
        }
    }
    return x;
}

/* The interface position of the first non-synthetic definition named `engine_qn`. */
static uint32_t qn_position(const qn_positions *x, const char *engine_qn) {
    if (!engine_qn) {
        return PDXE_NO_PARENT;
    }
    const char *qn = strip_project(engine_qn);
#if defined(PDXE_ENABLE_TEST_SEAMS)
    if (x->reference) {
        uint32_t kept = 0;
        for (int i = 0; i < x->defs->count; i++) {
            const PDXEDefinition *d = &x->defs->items[i];
            PDXE_LOOKUP_COUNT(PDXE_LC_DEF_ENTRIES);
            if (is_synthetic(d)) {
                continue;
            }
            PDXE_LOOKUP_COUNT(PDXE_LC_IFACE_COMPARES);
            if (d->qualified_name && strcmp(strip_project(d->qualified_name), qn) == 0) {
                return kept;
            }
            kept++;
        }
        return PDXE_NO_PARENT;
    }
#endif
    int d = x->slots[qn_slot(x, qn, qn_hash(qn))];
    return d < 0 ? PDXE_NO_PARENT : x->position[d];
}

/*
 * Where a call or usage is. The engine gives most sites a byte range and some only a
 * line. In C-family source it also reads the text after macro expansion, and what it
 * finds there is positioned in that text, which the caller never sees: such a site
 * gets the empty span, not a position in the file that would be wrong.
 */
static pdxe_span site_span(const line_index *li, PDXESourceOrigin origin, uint32_t start,
                           uint32_t end, int line) {
    if (origin != PDXE_SOURCE_ORIGIN_RAW) {
        return (pdxe_span){0};
    }
    if (end > start) {
        return span_of_bytes(li, start, end);
    }
    if (line > 0) {
        return span_of_lines(li, (uint32_t)line, (uint32_t)line);
    }
    return (pdxe_span){0};
}

/*
 * Whether a usage is a site where a callable may be passed as a value. The interface
 * reports these as calls with `is_reference` set, not as usages. The test is the
 * reference's own: the sites it offers to typed resolution as call references.
 */
static bool is_reference_site(const PDXEUsage *u) {
    return pdxe_pipeline_usage_semantic_reference_candidate(u);
}

static uint32_t count_reference_sites(const PDXEUsageArray *usages) {
    uint32_t n = 0;
    for (int i = 0; i < usages->count; i++) {
        if (is_reference_site(&usages->items[i])) {
            n++;
        }
    }
    return n;
}

/* What the extractor saw around a call, as the interface's lexical bits. */
static uint16_t call_lexical_of(const PDXECall *c) {
    return (uint16_t)((c->is_method ? PDXE_LEX_UNRESOLVED_MEMBER : 0) |
                      (c->callee_is_locally_bound ? PDXE_LEX_LOCALLY_BOUND : 0) |
                      (c->receiver_is_self_attribute ? PDXE_LEX_SELF_ROOTED : 0));
}

/* What the extractor saw around a usage, as the interface's lexical bits. */
static uint16_t lexical_of(const PDXEUsage *u) {
    return (uint16_t)((u->kind == PDXE_USAGE_CALL_REFERENCE ? PDXE_LEX_EXPLICIT_REFERENCE : 0) |
                     (u->is_member_access ? PDXE_LEX_MEMBER_ACCESS : 0) |
                     (u->semantic_reference_blocked ? PDXE_LEX_BLOCKED : 0) |
                     (u->semantic_reference_blocked && u->semantic_reference_local_shadow
                          ? PDXE_LEX_BLOCKED_LOCALLY
                          : 0));
}

/* The interface's description of an engine call. */
/* The length of a NULL-terminated array of strings; 0 for none. */
static uint32_t count_strings(const char *const *v) {
    uint32_t n = 0;
    while (v && v[n]) {
        n++;
    }
    return n;
}

/*
 * Who owns a call or callable reference in the graph (issues 75 and 76; decision 48).
 *
 * The engine decides it where it finds the site: the declaration occurrence of the
 * represented callable around it, or the file (PDXESiteOwner). The caller is the
 * definition with that occurrence, found in an index built once per file. Qualified
 * names are not compared: the callable a call was found in can be named otherwise than
 * its definition, and they still join. A raw site's owner is decided by occurrence
 * alone; where none is a definition's, it is the file's, through the definition a
 * superseded C-family callable was replaced by when there is one (PDXEOwnerAlias).
 *
 * A site with no position in the file is never a site: what C, C++ and CUDA find in
 * the macro-expanded text, and what a resolver adds where nothing can be located.
 * Its caller is context for resolution only, never ownership in the graph: the
 * expanded definition with its owner's occurrence when the preprocessed pass adopted
 * one, otherwise the callable its own name names, as the engine has always reported.
 */
typedef struct {
    const PDXEFileResult *engine;
    PDXEOccurrenceIndex occurrences; /* the engine's definitions */
    const qn_positions *names;       /* their interface positions, and by name */
    uint32_t file;                   /* the file's own module definition */
} owner_join;

static void owner_join_open(owner_join *join, const PDXEFileResult *engine,
                            const qn_positions *names) {
    join->engine = engine;
    join->names = names;
    pdxe_occurrence_index_build(&join->occurrences, engine->defs.items, engine->defs.count);
    join->file = qn_position(names, engine->module_qn);
}

static void owner_join_close(owner_join *join) {
    pdxe_occurrence_index_free(&join->occurrences);
}

/* The interface position of the definition with `declaration`, through a supersession
 * alias for a raw one; PDXE_NO_PARENT when there is none, or more than one. */
static uint32_t owner_position(const owner_join *join, PDXECallableOccurrence declaration) {
    int found = pdxe_occurrence_index_find(&join->occurrences, declaration);
    if (found == PDXE_OCCURRENCE_NONE) {
        for (int i = 0; i < join->engine->owner_alias_count; i++) {
            const PDXEOwnerAlias *alias = &join->engine->owner_aliases[i];
            if (pdxe_occurrence_equal(alias->superseded, declaration)) {
                found = alias->replacement;
                break;
            }
        }
    }
    return found >= 0 && found < join->engine->defs.count ? join->names->position[found]
                                                          : PDXE_NO_PARENT;
}

/* The caller a call or callable reference reports: see owner_join. */
static uint32_t caller_position(const owner_join *join, PDXESiteOwner owner,
                                PDXESourceOrigin origin, const char *enclosing_func_qn) {
    if (owner.decided && origin == PDXE_SOURCE_ORIGIN_RAW) {
        if (!pdxe_occurrence_valid(owner.declaration)) {
            return join->file;
        }
        uint32_t position = owner_position(join, owner.declaration);
        return position != PDXE_NO_PARENT ? position : join->file;
    }
    if (owner.decided) {
        uint32_t position = owner_position(join, owner.declaration);
        if (position != PDXE_NO_PARENT) {
            return position;
        }
    }
    return qn_position(join->names, enclosing_func_qn);
}

/* `join`: report the represented callable that owns the call, as a file's own calls
 * do (owner_join); NULL for a resolution's site, which names the callable the call was
 * found in, all a surface records, so cached and fresh answers agree. */
static pdxe_call call_of(const qn_positions *names, const line_index *li,
                         const source_view *src, const PDXECall *c, const owner_join *join) {
    pdxe_call o = {0};
    /* The engine's own static node types, borrowed; arguments are attached by the
     * caller, which owns the array they go in. */
    o.n_ast_path = count_strings(c->ast_path);
    o.ast_path = o.n_ast_path ? c->ast_path : NULL;
    /* The callee as the source spells it, from the start of the call, is a range
     * (issue 90): a member chain's links each name the whole chain before them. */
    o.callee_text = text_of(src, c->callee_name, c->source_origin == PDXE_SOURCE_ORIGIN_RAW,
                            c->site_start_byte, c->site_end_byte);
    /*
     * The engine keeps a call's receiver inside its callee text rather than on its own;
     * a receiver is only ever split off by the resolver that needs it.
     */
    o.receiver_text = NULL;
    o.caller_index = join ? caller_position(join, c->owner, c->source_origin, c->enclosing_func_qn)
                          : qn_position(names, c->enclosing_func_qn);
    o.span = site_span(li, c->source_origin, c->site_start_byte, c->site_end_byte, c->start_line);
    o.is_reference = 0;
    o.typed_only = c->requires_lsp_resolution ? 1 : 0;
    o.lexical = call_lexical_of(c);
    /* Issue 114: the `use` scope the engine placed it in, if any. */
    o.use_scope = c->use_scope > 0 ? (uint32_t)(c->use_scope - 1) : PDXE_NO_PARENT;
    return o;
}

/* The interface's description of a usage that may be a call reference. */
static pdxe_call reference_of(const qn_positions *names, const line_index *li,
                              const source_view *src, const PDXEUsage *u, const owner_join *join) {
    pdxe_call o = {0};
    o.callee_text = text_of(src, u->ref_name, u->source_origin == PDXE_SOURCE_ORIGIN_RAW,
                            u->site_start_byte, u->site_end_byte);
    o.receiver_text = NULL;
    o.n_ast_path = count_strings(u->ast_path);
    o.ast_path = o.n_ast_path ? u->ast_path : NULL;
    o.caller_index = join ? caller_position(join, u->owner, u->source_origin, u->enclosing_func_qn)
                          : qn_position(names, u->enclosing_func_qn);
    o.span = site_span(li, u->source_origin, u->site_start_byte, u->site_end_byte, 0);
    o.is_reference = 1;
    o.typed_only = 0;
    o.lexical = lexical_of(u);
    o.use_scope = PDXE_NO_PARENT;
    return o;
}

/* Zeros past the end of a source buffer: the parser reads ahead of the last byte. */
enum { SOURCE_PAD = 16 };

/* Translates an engine result into the interface's. Takes ownership of `engine`. */
static int translate(PDXEFileResult *engine, const line_index *li, const source_view *src,
                     pdxe_file_result **out) {
    result_holder *h = (result_holder *)calloc(1, sizeof(*h));
    if (!h) {
        pdxe_free_result(engine);
        return PDXE_E_NOMEM;
    }
    h->engine = engine;
    pdxe_file_result *r = &h->pub;

    r->status = engine->has_error      ? PDXE_FILE_FAILED
                : engine->parse_incomplete ? PDXE_FILE_PARTIAL
                                           : PDXE_FILE_PARSED;

    /*
     * Definitions: first the ones the file actually has, then their parents. Parents
     * are named by qualified name and can only be resolved to positions once every
     * definition has its position, so that is a second pass.
     */
    const PDXEDefArray *defs = &engine->defs;
    r->defs = ALLOC_ARRAY(pdxe_definition, defs->count);
    if (!r->defs) {
        holder_free(h);
        return PDXE_E_NOMEM;
    }
    uint32_t kept = 0;
    for (int i = 0; i < defs->count; i++) {
        const PDXEDefinition *d = &defs->items[i];
        if (is_synthetic(d)) {
            continue;
        }
        pdxe_definition *o = &r->defs[kept++];
        o->name = d->name;
        /* Held whole until every parent is known; related in a third pass below. */
        o->qn_form = PDXE_QN_WHOLE;
        o->qn_base = PDXE_NO_PARENT;
        o->qn_text = strip_project(d->qualified_name);
        o->kind = normalise_kind(d->label);
        o->engine_kind = d->label;
        /* The parameter list: the engine holds one read from the file's own source as
         * its range, and copies only one read from another buffer (issue 90). */
        if (d->signature) {
            o->signature = owned_text(d->signature);
        } else if (d->signature_end > d->signature_start) {
            if (d->signature_end > src->len) {
                holder_free(h);
                return PDXE_E_INTERNAL;
            }
            o->signature.kind = PDXE_TEXT_SOURCE;
            o->signature.start = d->signature_start;
            o->signature.end = d->signature_end;
        }
        o->doc = d->docstring;
        o->span = span_of_lines(li, d->start_line, d->end_line);
        /* The engine records no separate body range; the whole definition stands in. */
        o->body_span = o->span;
        o->parent_index = PDXE_NO_PARENT; /* resolved in the second pass below */
        o->visibility = d->is_exported ? PDXE_VIS_PUBLIC : PDXE_VIS_NON_PUBLIC;
        o->is_test = d->is_test ? 1 : 0;
        o->is_entry_point = d->is_entry_point ? 1 : 0;
        /* All three metrics are the engine's own: no fallback walk is needed. */
        o->cyclomatic = d->complexity > 0 ? (uint32_t)d->complexity : 0;
        o->cognitive = d->cognitive > 0 ? (uint32_t)d->cognitive : 0;
        o->loop_depth = d->loop_depth > 0 ? (uint32_t)d->loop_depth : 0;
        /* The engine's own NULL-terminated array, borrowed like its strings: order,
         * spelling and case as recorded, nothing resolved. None is NULL and 0. */
        uint32_t n_bases = 0;
        while (d->base_classes && d->base_classes[n_bases]) {
            n_bases++;
        }
        o->base_classes = n_bases ? d->base_classes : NULL;
        o->n_base_classes = n_bases;
        /* Decorators, parameter types and route, borrowed as recorded (issue 47).
         * The parameter types are counted, not terminated; an array with a missing
         * entry is not passed on. */
        o->n_decorators = count_strings(d->decorators);
        o->decorators = o->n_decorators ? d->decorators : NULL;
        uint32_t n_types =
            d->signature_param_types && d->signature_param_count > 0
                ? (uint32_t)d->signature_param_count
                : 0;
        for (uint32_t t = 0; t < n_types; t++) {
            if (!d->signature_param_types[t]) {
                n_types = 0;
            }
        }
        o->signature_param_types = n_types ? d->signature_param_types : NULL;
        o->n_signature_param_types = n_types;
        o->route_path = d->route_path;
        o->route_method = d->route_method;
        /* Issue 117: a Rust trait impl's method, its trait and the impl's header
         * (issue 109), borrowed as recorded; an inherent method, or any other
         * definition, has neither. */
        o->impl_trait = d->impl_trait;
        o->n_impl_bounds = count_strings(d->impl_bounds);
        o->impl_bounds = o->n_impl_bounds ? d->impl_bounds : NULL;
    }
    r->n_defs = kept;

    /* Routes, translated into one array the definitions point into (issue 54). Their
     * strings and node-type paths are the engine's, borrowed. */
    size_t n_routes = 0;
    for (int i = 0; i < defs->count; i++) {
        if (!is_synthetic(&defs->items[i]) && defs->items[i].routes &&
            defs->items[i].route_count > 0) {
            n_routes += (size_t)defs->items[i].route_count;
        }
    }
    if (n_routes > 0) {
        h->routes = (pdxe_route *)calloc(n_routes, sizeof(pdxe_route));
        if (!h->routes) {
            holder_free(h);
            return PDXE_E_NOMEM;
        }
        size_t next = 0;
        kept = 0;
        for (int i = 0; i < defs->count; i++) {
            const PDXEDefinition *d = &defs->items[i];
            if (is_synthetic(d)) {
                continue;
            }
            pdxe_definition *o = &r->defs[kept++];
            if (!d->routes || d->route_count <= 0) {
                continue;
            }
            o->routes = &h->routes[next];
            for (int k = 0; k < d->route_count; k++) {
                const PDXERouteFact *f = &d->routes[k];
                if (!f->method || !f->path || !f->callee_text) {
                    continue;
                }
                pdxe_route *route = &h->routes[next + o->n_routes];
                route->method = f->method;
                route->path = f->path;
                route->callee_text = f->callee_text;
                /* The declaring node's own text is a range of the source (issue 90). */
                route->source_text = text_of(src, f->source_text, true, f->start_byte, f->end_byte);
                route->span = f->end_byte > f->start_byte
                                  ? span_of_bytes(li, f->start_byte, f->end_byte)
                                  : (pdxe_span){0};
                route->n_ast_path = count_strings(f->ast_path);
                route->ast_path = route->n_ast_path ? f->ast_path : NULL;
                o->n_routes++;
            }
            if (o->n_routes == 0) {
                o->routes = NULL;
            }
            next += (size_t)d->route_count;
        }
    }

    /* Second pass: parents, now that every kept definition has its position. Every
     * fact naming a definition is placed through one index of their names (issue 92). */
    h->names = qn_positions_open(defs);
    if (!h->names) {
        holder_free(h);
        return PDXE_E_NOMEM;
    }
    const qn_positions *names = h->names;
    kept = 0;
    for (int i = 0; i < defs->count; i++) {
        const PDXEDefinition *d = &defs->items[i];
        if (is_synthetic(d)) {
            continue;
        }
        r->defs[kept++].parent_index = qn_position(names, d->parent_class);
    }

    /* Third pass: each qualified name by its relation to an earlier one (issue 90). */
    if (r->n_defs > 0) {
        const char **qn = (const char **)calloc(r->n_defs, sizeof(*qn));
        if (!qn) {
            holder_free(h);
            return PDXE_E_NOMEM;
        }
        for (uint32_t i = 0; i < r->n_defs; i++) {
            qn[i] = r->defs[i].qn_text;
        }
        pdxe_qn_index spelled;
        if (!pdxe_qn_index_init(&spelled, qn, r->n_defs)) {
            free(qn);
            holder_free(h);
            return PDXE_E_NOMEM;
        }
        uint32_t file = qn_position(names, engine->module_qn);
        for (uint32_t i = 0; i < r->n_defs; i++) {
            pdxe_definition *o = &r->defs[i];
            uint32_t base = PDXE_NO_PARENT;
            uint32_t keep = 0;
            const char *rest = NULL;
            int form = pdxe_qn_choose(&spelled, qn, o->name, i, o->parent_index, file, &base, &keep,
                                      &rest);
            o->qn_form = (uint32_t)form;
            o->qn_base = form == PDXE_QN_WHOLE ? PDXE_NO_PARENT : base;
            o->qn_keep = keep;
            o->qn_text = form == PDXE_QN_WHOLE ? qn[i] : form == PDXE_QN_MEMBER ? NULL : rest;
        }
        pdxe_qn_index_free(&spelled);
        free(qn);
    }

    /* Calls: the engine's, then the usages that may be call references. */
    const PDXECallArray *calls = &engine->calls;
    const PDXEUsageArray *usages = &engine->usages;
    uint32_t n_refs = count_reference_sites(usages);
    r->calls = ALLOC_ARRAY(pdxe_call, (uint32_t)calls->count + n_refs);
    if (!r->calls) {
        holder_free(h);
        return PDXE_E_NOMEM;
    }
    owner_join join;
    owner_join_open(&join, engine, names);
    for (int i = 0; i < calls->count; i++) {
        r->calls[r->n_calls++] = call_of(names, li, src, &calls->items[i], &join);
    }
    for (int i = 0; i < usages->count; i++) {
        if (is_reference_site(&usages->items[i])) {
            r->calls[r->n_calls++] = reference_of(names, li, src, &usages->items[i], &join);
        }
    }
    owner_join_close(&join);
    /* The calls' captured arguments, translated into one array (issue 47). */
    size_t n_args = 0;
    for (int i = 0; i < calls->count; i++) {
        if (calls->items[i].args && calls->items[i].arg_count > 0) {
            n_args += (size_t)calls->items[i].arg_count;
        }
    }
    if (n_args > 0) {
        h->call_args = (pdxe_call_arg *)calloc(n_args, sizeof(pdxe_call_arg));
        if (!h->call_args) {
            holder_free(h);
            return PDXE_E_NOMEM;
        }
        size_t next = 0;
        for (int i = 0; i < calls->count; i++) {
            const PDXECall *c = &calls->items[i];
            if (!c->args || c->arg_count <= 0) {
                continue;
            }
            r->calls[i].args = &h->call_args[next];
            for (int a = 0; a < c->arg_count; a++) {
                pdxe_call_arg *o = &h->call_args[next + (size_t)r->calls[i].n_args];
                /* The engine holds an argument read from the file's own source as its
                 * range, and copies only one read from another buffer (issue 90). */
                const PDXECallArg *arg = &c->args[a];
                if (arg->expr) {
                    o->expr = owned_text(arg->expr);
                } else if (arg->expr_start <= arg->expr_end && arg->expr_end <= src->len) {
                    o->expr.kind = PDXE_TEXT_SOURCE;
                    o->expr.start = arg->expr_start;
                    o->expr.end = arg->expr_end;
                } else {
                    holder_free(h);
                    return PDXE_E_INTERNAL;
                }
                o->value = c->args[a].value;
                o->keyword = c->args[a].keyword;
                o->index = c->args[a].index > 0 ? (uint32_t)c->args[a].index : 0;
                r->calls[i].n_args++;
            }
            next += (size_t)c->arg_count;
        }
    }

    /* Imports. The engine records neither position nor a separate alias for them. */
    const PDXEImportArray *imports = &engine->imports;
    r->imports = ALLOC_ARRAY(pdxe_import, imports->count);
    if (!r->imports) {
        holder_free(h);
        return PDXE_E_NOMEM;
    }
    for (int i = 0; i < imports->count; i++) {
        const PDXEImport *im = &imports->items[i];
        r->imports[i].module_text = im->module_path;
        r->imports[i].imported_name = im->local_name;
        r->imports[i].alias = NULL;
    }
    r->n_imports = (uint32_t)imports->count;

    /* Issue 114: Rust's `use` scopes and the imports declared inside them, as the
     * engine recorded them (in the file's arena, which the holder keeps). */
    const PDXEUseScopeArray *use_scopes = &engine->use_scopes;
    if (use_scopes->count > 0) {
        r->use_scopes = ALLOC_ARRAY(pdxe_use_scope, use_scopes->count);
        if (!r->use_scopes) {
            holder_free(h);
            return PDXE_E_NOMEM;
        }
        for (int i = 0; i < use_scopes->count; i++) {
            r->use_scopes[i].parent = use_scopes->items[i].parent < 0
                                          ? PDXE_NO_PARENT
                                          : (uint32_t)use_scopes->items[i].parent;
            r->use_scopes[i].module = use_scopes->items[i].module ? 1 : 0;
        }
        r->n_use_scopes = (uint32_t)use_scopes->count;
    }
    const PDXEScopedImportArray *scoped = &engine->scoped_imports;
    if (scoped->count > 0) {
        r->scoped_imports = ALLOC_ARRAY(pdxe_scoped_import, scoped->count);
        if (!r->scoped_imports) {
            holder_free(h);
            return PDXE_E_NOMEM;
        }
        for (int i = 0; i < scoped->count; i++) {
            r->scoped_imports[i].import.module_text = scoped->items[i].import.module_path;
            r->scoped_imports[i].import.imported_name = scoped->items[i].import.local_name;
            r->scoped_imports[i].import.alias = NULL;
            r->scoped_imports[i].scope = (uint32_t)scoped->items[i].scope;
        }
        r->n_scoped_imports = (uint32_t)scoped->count;
    }

    /* Usages: every use of a name, except those already reported as references. */
    r->usages = ALLOC_ARRAY(pdxe_usage, (uint32_t)usages->count - n_refs);
    if (!r->usages) {
        holder_free(h);
        return PDXE_E_NOMEM;
    }
    for (int i = 0; i < usages->count; i++) {
        const PDXEUsage *u = &usages->items[i];
        if (is_reference_site(u)) {
            continue;
        }
        pdxe_usage *o = &r->usages[r->n_usages++];
        o->name = text_of(src, u->ref_name, u->source_origin == PDXE_SOURCE_ORIGIN_RAW,
                          u->site_start_byte, u->site_end_byte);
        o->scope_index = qn_position(names, u->enclosing_func_qn);
        o->span = site_span(li, u->source_origin, u->site_start_byte, u->site_end_byte, 0);
        o->lexical = lexical_of(u);
    }

    /* Type references, field accesses, channels, configuration reads, throws.
     * Positionless: the engine records only the scope each was found in. */
    const PDXETypeRefArray *types = &engine->type_refs;
    r->types = ALLOC_ARRAY(pdxe_type_ref, types->count);
    const PDXERWArray *rws = &engine->rw;
    r->rws = ALLOC_ARRAY(pdxe_rw, rws->count);
    const PDXEChannelArray *channels = &engine->channels;
    r->channels = ALLOC_ARRAY(pdxe_channel, channels->count);
    const PDXEEnvAccessArray *envs = &engine->env_accesses;
    r->envs = ALLOC_ARRAY(pdxe_env_access, envs->count);
    r->diags = ALLOC_ARRAY(pdxe_diag, engine->error_msg ? 1 : 0);
    const PDXEThrowArray *throws = &engine->throws;
    r->throws = ALLOC_ARRAY(pdxe_throw, throws->count);
    if (!r->types || !r->rws || !r->channels || !r->envs || !r->diags || !r->throws) {
        holder_free(h);
        return PDXE_E_NOMEM;
    }
    for (int i = 0; i < types->count; i++) {
        r->types[i].type_text = types->items[i].type_name;
        r->types[i].scope_index = qn_position(names, types->items[i].enclosing_func_qn);
    }
    r->n_types = (uint32_t)types->count;
    for (int i = 0; i < rws->count; i++) {
        r->rws[i].field_text = rws->items[i].var_name;
        r->rws[i].scope_index = qn_position(names, rws->items[i].enclosing_func_qn);
        r->rws[i].is_write = rws->items[i].is_write ? 1 : 0;
    }
    r->n_rws = (uint32_t)rws->count;
    for (int i = 0; i < channels->count; i++) {
        r->channels[i].channel_text = channels->items[i].channel_name;
        r->channels[i].is_listen = channels->items[i].direction == PDXE_CHANNEL_LISTEN ? 1 : 0;
    }
    r->n_channels = (uint32_t)channels->count;
    for (int i = 0; i < envs->count; i++) {
        r->envs[i].key = envs->items[i].env_key;
    }
    r->n_envs = (uint32_t)envs->count;
    if (engine->error_msg) {
        r->diags[0].message = engine->error_msg;
        r->n_diags = 1;
    }
    for (int i = 0; i < throws->count; i++) {
        r->throws[i].exception_text = throws->items[i].exception_name;
        r->throws[i].scope_index = qn_position(names, throws->items[i].enclosing_func_qn);
    }
    r->n_throws = (uint32_t)throws->count;
    qn_positions_free(h->names);
    h->names = NULL;
    r->truncated = engine->walk_truncated ? 1 : 0;

    /* `impl Trait for Type` relations, borrowed like the other strings, in the
     * engine's order. The type's qualified name loses the project prefix every
     * qualified name the interface gives loses; nothing else is changed. A relation
     * with a missing string, which only a failed allocation leaves, is not reported. */
    const PDXEImplTraitArray *impls = &engine->impl_traits;
    if (impls->count > 0) {
        r->impl_traits = (pdxe_impl_trait *)calloc((size_t)impls->count, sizeof(pdxe_impl_trait));
        if (!r->impl_traits) {
            holder_free(h);
            return PDXE_E_NOMEM;
        }
        for (int i = 0; i < impls->count; i++) {
            const PDXEImplTrait *it = &impls->items[i];
            if (!it->trait_name || !it->struct_name || !it->struct_qn) {
                continue;
            }
            pdxe_impl_trait *o = &r->impl_traits[r->n_impl_traits++];
            o->trait_name = it->trait_name;
            o->struct_name = it->struct_name;
            o->struct_qn = strip_project(it->struct_qn);
            /* Issue 117: the impl's header (issue 109), borrowed as recorded. */
            o->n_impl_bounds = count_strings(it->impl_bounds);
            o->impl_bounds = o->n_impl_bounds ? it->impl_bounds : NULL;
        }
        if (r->n_impl_traits == 0) {
            free(r->impl_traits);
            r->impl_traits = NULL;
        }
    }

    /* Issue 110: the members the file's macro invocations may write, borrowed like the
     * other strings, in the engine's (byte) order. */
    const PDXEMacroMemberArray *members = &engine->macro_members;
    if (members->count > 0) {
        r->macro_members =
            (pdxe_macro_member *)calloc((size_t)members->count, sizeof(pdxe_macro_member));
        if (!r->macro_members) {
            holder_free(h);
            return PDXE_E_NOMEM;
        }
        for (int i = 0; i < members->count; i++) {
            if (!members->items[i].type_name) {
                continue;
            }
            pdxe_macro_member *o = &r->macro_members[r->n_macro_members++];
            o->type_name = members->items[i].type_name;
            o->member = members->items[i].member;
        }
        if (r->n_macro_members == 0) {
            free(r->macro_members);
            r->macro_members = NULL;
        }
    }

    *out = r;
    return PDXE_OK;
}

int pdxe_extract_file(pdxe_ctx *ctx, int lang, const char *rel_path, const uint8_t *bytes,
                      size_t len, pdxe_file_result **out) {
    if (out) {
        *out = NULL;
    }
    if (!ctx || !out || (!bytes && len > 0)) {
        return PDXE_E_INVALID;
    }
    int engine_lang = engine_language(lang);
    if (engine_lang < 0) {
        return PDXE_E_LANG;
    }
    /* The engine measures source length in an int. */
    if (len > (size_t)INT_MAX) {
        return PDXE_E_TOOLARGE;
    }
    /*
     * The engine reads its source as a C string and its parser reads ahead of the
     * last byte, so it is handed a copy ending in zeros, never the caller's buffer,
     * which ends where the file does. Nothing in the result points into the copy; the
     * result keeps it, since its ranges are ranges of these bytes and its surface is
     * exported from them (issue 90).
     */
    char *source = (char *)malloc(len + SOURCE_PAD);
    if (!source) {
        return PDXE_E_NOMEM;
    }
    if (len > 0) {
        memcpy(source, bytes, len);
    }
    memset(source + len, 0, SOURCE_PAD);

    /*
     * The engine keeps the path it is given: the file's module definition is named by
     * it. So it is given a copy the result owns, never the caller's string, which the
     * caller may free as soon as this returns.
     */
    char *path = strdup(rel_path ? rel_path : "");
    line_index li;
    if (!path || line_index_build(&li, (const uint8_t *)source, len) != 0) {
        free(path);
        free(source);
        return PDXE_E_NOMEM;
    }
    pdxe_losses_t before = pdxe_losses();
    PDXEFileResult *engine = pdxe_engine_extract_file(source, (int)len, (PDXELanguage)engine_lang,
                                                      PROJECT_SENTINEL, path, 0, NULL, NULL);
    pdxe_losses_t after = pdxe_losses();
    if (!engine) {
        line_index_free(&li);
        free(path);
        free(source);
        return PDXE_E_NOMEM;
    }
    source_view src = {source, len};
    int rc = translate(engine, &li, &src, out);
    line_index_free(&li);
    if (rc != PDXE_OK) {
        free(path);
        free(source);
        return rc;
    }
    result_holder *h = (result_holder *)*out;
    h->source = source;
    h->source_len = len;
    if (!adopt(h, path)) {
        holder_free(h);
        free(path);
        *out = NULL;
        return PDXE_E_NOMEM;
    }
    h->lang = lang;
    uint64_t lost = (after.allocations - before.allocations) + (after.work - before.work);
    h->lost = lost > UINT32_MAX ? UINT32_MAX : (uint32_t)lost;
    /* The caller sees the count the surface carries, from the same field. */
    h->pub.extraction_lost = h->lost;
    /* The file's declared package or namespace, borrowed like the other strings. */
    h->pub.declared_namespace = h->engine ? h->engine->namespace_name : NULL;
    return PDXE_OK;
}


/* --- results rebuilt from a cache -------------------------------------------- */

/* Whether a counted array of strings is there and every string in it is. */
static bool strings_present(const char *const *v, uint32_t n) {
    if (n == 0) {
        return true;
    }
    if (!v) {
        return false;
    }
    for (uint32_t i = 0; i < n; i++) {
        if (!v[i]) {
            return false;
        }
    }
    return true;
}

static const char *own(result_holder *h, const char *s, bool *failed);

/* Whether a text handed to pdxe_result_build is well formed for a source of `len` bytes
 * (issue 90): a known kind, `reserved` 0, a range inside the source, owned text that is
 * there; no text only where `optional`. */
static bool text_valid(const pdxe_text *t, size_t len, bool optional) {
    if (t->reserved != 0) {
        return false;
    }
    switch (t->kind) {
    case PDXE_TEXT_NONE:
        return optional && !t->owned && t->start == 0 && t->end == 0;
    case PDXE_TEXT_SOURCE:
        return !t->owned && t->start <= t->end && (size_t)t->end <= len;
    case PDXE_TEXT_OWNED:
        return t->owned && t->start == 0 && t->end == 0;
    default:
        return false;
    }
}

/* Whether definition `i`'s qualified name is well formed on its own (issue 90); a
 * prefix's `qn_keep` is checked against its base by qualified_names_valid. */
static bool qualified_name_valid(const pdxe_definition *d, uint32_t i) {
    switch (d->qn_form) {
    case PDXE_QN_WHOLE:
        return d->qn_text && d->qn_base == PDXE_NO_PARENT && d->qn_keep == 0;
    case PDXE_QN_MEMBER:
        return !d->qn_text && d->name && d->qn_base < i && d->qn_keep == 0;
    case PDXE_QN_EXTENDED:
        return d->qn_text && d->qn_base < i && d->qn_keep == 0;
    case PDXE_QN_PREFIX:
        return d->qn_text && d->qn_base < i && (d->qn_text[0] == '\0' || d->qn_text[0] == '.');
    default:
        return false;
    }
}

/* Whether every prefix's `qn_keep` is a `.` its base adds (issue 90), from every
 * name's length, computed once in order: PDXE_OK, PDXE_E_INVALID or PDXE_E_NOMEM. Each
 * definition's own form is already valid. */
static int qualified_names_check(const pdxe_definition *defs, uint32_t n) {
    if (n == 0) {
        return PDXE_OK;
    }
    size_t *lens = (size_t *)calloc(n, sizeof(*lens));
    if (!lens) {
        return PDXE_E_NOMEM;
    }
    bool ok = true;
    for (uint32_t i = 0; ok && i < n; i++) {
        const pdxe_definition *d = &defs[i];
        switch (d->qn_form) {
        case PDXE_QN_WHOLE:
            lens[i] = strlen(d->qn_text);
            break;
        case PDXE_QN_MEMBER:
            lens[i] = lens[d->qn_base] + 1 + strlen(d->name);
            break;
        case PDXE_QN_EXTENDED:
            lens[i] = lens[d->qn_base] + strlen(d->qn_text);
            break;
        default:
            ok = qualified_name_prefix_valid(defs, d->qn_base, d->qn_keep, lens[d->qn_base]);
            lens[i] = (size_t)d->qn_keep + strlen(d->qn_text);
            break;
        }
    }
    free(lens);
    return ok ? PDXE_OK : PDXE_E_INVALID;
}

/* A copy the holder owns of a counted array of strings, the array and each string;
 * NULL for none. Sets *failed when a copy could not be made. */
static const char **own_strings(result_holder *h, const char *const *v, uint32_t n,
                                bool *failed) {
    if (n == 0 || *failed) {
        return NULL;
    }
    const char **copy = (const char **)calloc(n, sizeof(*copy));
    if (!copy || !adopt(h, (char *)copy)) {
        free(copy);
        *failed = true;
        return NULL;
    }
    for (uint32_t i = 0; i < n; i++) {
        copy[i] = own(h, v[i], failed);
    }
    return copy;
}

/* A copy the holder owns, or NULL for NULL. Sets *failed when a copy could not be made. */
static const char *own(result_holder *h, const char *s, bool *failed) {
    if (!s) {
        return NULL;
    }
    char *copy = strdup(s);
    if (!copy || !adopt(h, copy)) {
        free(copy);
        *failed = true;
        return NULL;
    }
    return copy;
}

/* A text the holder owns: a range as it is, owned text copied. */
static pdxe_text own_text(result_holder *h, pdxe_text t, bool *failed) {
    if (t.kind == PDXE_TEXT_OWNED) {
        t.owned = own(h, t.owned, failed);
    }
    return t;
}

/*
 * A result rebuilt from the parts a cache keeps. Everything is copied, strings
 * included, so the caller's arrays may be freed on return. The rebuilt result has no
 * engine result behind it: it describes the file, and resolving the file takes its
 * surface (pdxe_surface_import). Channel, configuration, diagnostic and throw arrays
 * are not among the parts and come back empty; the status is `parsed`, and the result
 * is not truncated. A definition's base classes are copied, the array and each string,
 * and so are its routes and the `impl Trait for Type` relations. A source range is kept
 * as a range of the file's source, never resolved: the rebuilt result holds no source.
 */
int pdxe_result_build(pdxe_ctx *ctx, size_t source_len, const pdxe_definition *defs,
                      uint32_t n_defs, const pdxe_call *calls, uint32_t n_calls,
                      const pdxe_import *imports, uint32_t n_imports, const pdxe_usage *usages,
                      uint32_t n_usages, const pdxe_type_ref *types, uint32_t n_types,
                      const pdxe_rw *rws, uint32_t n_rws, const pdxe_impl_trait *impl_traits,
                      uint32_t n_impl_traits, pdxe_file_result **out) {
    if (out) {
        *out = NULL;
    }
    if (!ctx || !out || (n_defs && !defs) || (n_calls && !calls) || (n_imports && !imports) ||
        (n_usages && !usages) || (n_types && !types) || (n_rws && !rws) ||
        (n_impl_traits && !impl_traits)) {
        return PDXE_E_INVALID;
    }
    /* Every counted array of a definition or call is there, and holds what it must;
     * every text and qualified name is well formed (issue 90). */
    for (uint32_t i = 0; i < n_defs; i++) {
        if (!qualified_name_valid(&defs[i], i) ||
            !text_valid(&defs[i].signature, source_len, true) ||
            !strings_present(defs[i].decorators, defs[i].n_decorators) ||
            !strings_present(defs[i].signature_param_types, defs[i].n_signature_param_types) ||
            !strings_present(defs[i].impl_bounds, defs[i].n_impl_bounds) ||
            (defs[i].n_routes && !defs[i].routes)) {
            return PDXE_E_INVALID;
        }
        for (uint32_t k = 0; k < defs[i].n_routes; k++) {
            const pdxe_route *route = &defs[i].routes[k];
            if (!route->method || !route->path || !route->callee_text ||
                !text_valid(&route->source_text, source_len, true) ||
                !strings_present(route->ast_path, route->n_ast_path)) {
                return PDXE_E_INVALID;
            }
        }
    }
    int names = qualified_names_check(defs, n_defs);
    if (names != PDXE_OK) {
        return names;
    }
    for (uint32_t i = 0; i < n_calls; i++) {
        if (!text_valid(&calls[i].callee_text, source_len, false) ||
            !strings_present(calls[i].ast_path, calls[i].n_ast_path) ||
            (calls[i].n_args && !calls[i].args)) {
            return PDXE_E_INVALID;
        }
        for (uint32_t a = 0; a < calls[i].n_args; a++) {
            if (!text_valid(&calls[i].args[a].expr, source_len, false)) {
                return PDXE_E_INVALID;
            }
        }
    }
    for (uint32_t i = 0; i < n_usages; i++) {
        if (!text_valid(&usages[i].name, source_len, false)) {
            return PDXE_E_INVALID;
        }
    }
    /* Every relation has its three strings, and the header it counts. */
    for (uint32_t i = 0; i < n_impl_traits; i++) {
        if (!impl_traits[i].trait_name || !impl_traits[i].struct_name ||
            !impl_traits[i].struct_qn ||
            !strings_present(impl_traits[i].impl_bounds, impl_traits[i].n_impl_bounds)) {
            return PDXE_E_INVALID;
        }
    }
    /* Every base a definition counts is a string, in an array that holds it. */
    for (uint32_t i = 0; i < n_defs; i++) {
        if (defs[i].n_base_classes && !defs[i].base_classes) {
            return PDXE_E_INVALID;
        }
        for (uint32_t b = 0; b < defs[i].n_base_classes; b++) {
            if (!defs[i].base_classes[b]) {
                return PDXE_E_INVALID;
            }
        }
    }
    result_holder *h = (result_holder *)calloc(1, sizeof(*h));
    if (!h) {
        return PDXE_E_NOMEM;
    }
    pdxe_file_result *r = &h->pub;
    r->status = PDXE_FILE_PARSED;
    r->defs = ALLOC_ARRAY(pdxe_definition, n_defs);
    r->calls = ALLOC_ARRAY(pdxe_call, n_calls);
    r->imports = ALLOC_ARRAY(pdxe_import, n_imports);
    r->usages = ALLOC_ARRAY(pdxe_usage, n_usages);
    r->types = ALLOC_ARRAY(pdxe_type_ref, n_types);
    r->rws = ALLOC_ARRAY(pdxe_rw, n_rws);
    r->channels = ALLOC_ARRAY(pdxe_channel, 0);
    r->envs = ALLOC_ARRAY(pdxe_env_access, 0);
    r->diags = ALLOC_ARRAY(pdxe_diag, 0);
    r->throws = ALLOC_ARRAY(pdxe_throw, 0);
    r->impl_traits =
        n_impl_traits ? (pdxe_impl_trait *)calloc(n_impl_traits, sizeof(pdxe_impl_trait)) : NULL;
    if (!r->defs || !r->calls || !r->imports || !r->usages || !r->types || !r->rws ||
        !r->channels || !r->envs || !r->diags || !r->throws || (n_impl_traits && !r->impl_traits)) {
        holder_free(h);
        return PDXE_E_NOMEM;
    }
    bool failed = false;
    for (uint32_t i = 0; i < n_defs; i++) {
        pdxe_definition *d = &r->defs[i];
        *d = defs[i];
        d->name = own(h, defs[i].name, &failed);
        d->qn_text = own(h, defs[i].qn_text, &failed);
        d->kind = own(h, defs[i].kind, &failed);
        d->engine_kind = own(h, defs[i].engine_kind, &failed);
        d->signature = own_text(h, defs[i].signature, &failed);
        d->doc = own(h, defs[i].doc, &failed);
        d->base_classes = NULL;
        d->n_base_classes = 0;
        if (defs[i].n_base_classes && !failed) {
            /* The array is the holder's too: it frees it with the strings. */
            const char **bases =
                (const char **)calloc(defs[i].n_base_classes, sizeof(*bases));
            if (!bases || !adopt(h, (char *)bases)) {
                free(bases);
                failed = true;
                continue;
            }
            for (uint32_t b = 0; b < defs[i].n_base_classes; b++) {
                bases[b] = own(h, defs[i].base_classes[b], &failed);
            }
            d->base_classes = bases;
            d->n_base_classes = defs[i].n_base_classes;
        }
        d->decorators = own_strings(h, defs[i].decorators, defs[i].n_decorators, &failed);
        d->n_decorators = d->decorators ? defs[i].n_decorators : 0;
        d->signature_param_types = own_strings(h, defs[i].signature_param_types,
                                               defs[i].n_signature_param_types, &failed);
        d->n_signature_param_types =
            d->signature_param_types ? defs[i].n_signature_param_types : 0;
        d->route_path = own(h, defs[i].route_path, &failed);
        d->route_method = own(h, defs[i].route_method, &failed);
        d->impl_trait = own(h, defs[i].impl_trait, &failed);
        d->impl_bounds = own_strings(h, (const char *const *)defs[i].impl_bounds,
                                     defs[i].n_impl_bounds, &failed);
        d->n_impl_bounds = d->impl_bounds ? defs[i].n_impl_bounds : 0;
        d->routes = NULL;
        d->n_routes = 0;
        if (defs[i].n_routes && !failed) {
            /* The array is the holder's too: it frees it with the strings. */
            pdxe_route *routes = (pdxe_route *)calloc(defs[i].n_routes, sizeof(*routes));
            if (!routes || !adopt(h, (char *)routes)) {
                free(routes);
                failed = true;
                continue;
            }
            for (uint32_t k = 0; k < defs[i].n_routes; k++) {
                const pdxe_route *from = &defs[i].routes[k];
                routes[k].method = own(h, from->method, &failed);
                routes[k].path = own(h, from->path, &failed);
                routes[k].callee_text = own(h, from->callee_text, &failed);
                routes[k].source_text = own_text(h, from->source_text, &failed);
                routes[k].span = from->span;
                routes[k].ast_path = own_strings(h, from->ast_path, from->n_ast_path, &failed);
                routes[k].n_ast_path = routes[k].ast_path ? from->n_ast_path : 0;
            }
            d->routes = routes;
            d->n_routes = defs[i].n_routes;
        }
    }
    for (uint32_t i = 0; i < n_calls; i++) {
        r->calls[i] = calls[i];
        r->calls[i].callee_text = own_text(h, calls[i].callee_text, &failed);
        r->calls[i].receiver_text = own(h, calls[i].receiver_text, &failed);
        r->calls[i].ast_path = own_strings(h, calls[i].ast_path, calls[i].n_ast_path, &failed);
        r->calls[i].n_ast_path = r->calls[i].ast_path ? calls[i].n_ast_path : 0;
        r->calls[i].args = NULL;
        r->calls[i].n_args = 0;
        /* Issue 114: a result built from parts has no `use` scopes to place it in. */
        r->calls[i].use_scope = PDXE_NO_PARENT;
        if (calls[i].n_args && !failed) {
            pdxe_call_arg *args = (pdxe_call_arg *)calloc(calls[i].n_args, sizeof(*args));
            if (!args || !adopt(h, (char *)args)) {
                free(args);
                failed = true;
                continue;
            }
            for (uint32_t a = 0; a < calls[i].n_args; a++) {
                args[a].expr = own_text(h, calls[i].args[a].expr, &failed);
                args[a].value = own(h, calls[i].args[a].value, &failed);
                args[a].keyword = own(h, calls[i].args[a].keyword, &failed);
                args[a].index = calls[i].args[a].index;
            }
            r->calls[i].args = args;
            r->calls[i].n_args = calls[i].n_args;
        }
    }
    for (uint32_t i = 0; i < n_imports; i++) {
        r->imports[i] = imports[i];
        r->imports[i].module_text = own(h, imports[i].module_text, &failed);
        r->imports[i].imported_name = own(h, imports[i].imported_name, &failed);
        r->imports[i].alias = own(h, imports[i].alias, &failed);
    }
    for (uint32_t i = 0; i < n_usages; i++) {
        r->usages[i] = usages[i];
        r->usages[i].name = own_text(h, usages[i].name, &failed);
    }
    for (uint32_t i = 0; i < n_types; i++) {
        r->types[i] = types[i];
        r->types[i].type_text = own(h, types[i].type_text, &failed);
    }
    for (uint32_t i = 0; i < n_rws; i++) {
        r->rws[i] = rws[i];
        r->rws[i].field_text = own(h, rws[i].field_text, &failed);
    }
    for (uint32_t i = 0; i < n_impl_traits; i++) {
        r->impl_traits[i].trait_name = own(h, impl_traits[i].trait_name, &failed);
        r->impl_traits[i].struct_name = own(h, impl_traits[i].struct_name, &failed);
        r->impl_traits[i].struct_qn = own(h, impl_traits[i].struct_qn, &failed);
        r->impl_traits[i].impl_bounds =
            own_strings(h, (const char *const *)impl_traits[i].impl_bounds,
                        impl_traits[i].n_impl_bounds, &failed);
        r->impl_traits[i].n_impl_bounds =
            r->impl_traits[i].impl_bounds ? impl_traits[i].n_impl_bounds : 0;
    }
    if (failed) {
        holder_free(h);
        return PDXE_E_NOMEM;
    }
    r->n_defs = n_defs;
    r->n_calls = n_calls;
    r->n_imports = n_imports;
    r->n_usages = n_usages;
    r->n_types = n_types;
    r->n_rws = n_rws;
    r->n_impl_traits = n_impl_traits;
    *out = r;
    return PDXE_OK;
}

/* =============================================================================
 * Cross-file typed resolution
 * =============================================================================
 *
 * The vendored resolver expects to run inside the reference's indexing pipeline,
 * after that pipeline has built a graph of the project. This project has no such
 * pipeline inside the engine, so the steps that build what the resolver reads are
 * performed here, in the order the reference performs them, before the resolver
 * runs: the directory structure, then every definition and the registry, then the
 * import edges, then resolution itself.
 *
 * Each step names the reference pass it stands in for and inserts in its order. The
 * order matters: identifiers follow it, lookups by short name return nodes in it,
 * and when two definitions share a qualified name the store keeps one of them by a
 * rule of its own (see pdxe_gbuf_upsert_node).
 */


typedef struct {
    char **items;
    size_t count;
    size_t cap;
} string_list;

static char *keep(string_list *l, char *s) {
    if (!s) {
        return NULL;
    }
    if (l->count == l->cap) {
        size_t cap = l->cap ? l->cap * 2 : 16;
        char **grown = (char **)realloc(l->items, cap * sizeof(char *));
        if (!grown) {
            free(s);
            return NULL;
        }
        l->items = grown;
        l->cap = cap;
    }
    l->items[l->count++] = s;
    return s;
}

static void string_list_free(string_list *l) {
    for (size_t i = 0; i < l->count; i++) {
        free(l->items[i]);
    }
    free(l->items);
    l->items = NULL;
    l->count = l->cap = 0;
}

/*
 * What the project keeps about a file besides its result: where the result came from,
 * what it looked like before the resolver wrote into it, and how to position the
 * sites the resolver adds.
 */
typedef enum {
    FROM_CALLER,  /* the caller's own extraction result, resolved through and restored */
    FROM_SURFACE, /* rebuilt from the file's surface; the caller's result came from a cache */
    EXTRACT_HERE, /* the caller supplied no result: the file is extracted when the run starts */
} result_source;

typedef struct {
    result_source source;
    int abi_lang;
    /* The caller's result the project is resolving through, or NULL if it owns one. */
    result_holder *borrowed;
    /* The resolver appends calls and merges into resolutions: their state before it did,
     * recorded when the run starts. */
    bool snapshot;
    int n_calls_before;
    PDXEResolvedCall *resolved_before;
    int n_resolved_before;
    line_index li;
    /* Whether the resolver asked for the file's source, and whether it was given it. */
    bool source_asked;
    bool source_served;
    /* What extraction lost on the file before it reached the project. */
    uint32_t extraction_lost;
} file_state;

struct pdxe_project {
    pdxe_ctx *ctx;

    pdxe_file_info_t *files;
    PDXEFileResult **cache;
    char **sources;
    int *source_lens;
    file_state *state;
    int n_files;
    int cap_files;

    pdxe_gbuf_t *gbuf;
    pdxe_registry_t *registry;
    PDXEHashTable *pkgmap;
    pdxe_path_alias_collection_t *aliases;
    /* The root crate manifest, in the engine's own shape, when one was supplied. */
    PDXECargoManifest crate;
    bool crate_set;
    pdxe_pipeline_ctx_t pctx;

    bool metadata_set;
    bool ran;

    pdxe_resolution *results;
    uint32_t n_results;
    /* The run's health, valid once a run has returned 0. */
    pdxe_run_health health;
    bool health_known;

    /* Surfaces imported and not yet matched to a file, by path. */
    PDXEFileResult **surfaces;
    char **surface_paths;
    int *surface_langs;
    uint32_t *surface_lost;
    /* Each surface's text ranges, bound to the file's source when the run starts. */
    pdxe_surface_texts **surface_texts;
    int n_surfaces;
    int cap_surfaces;

    string_list owned;
};

/*
 * The resolver asks for each file's source by the path it was added under. This
 * answers from memory, with the padding the parser needs, in a buffer the resolver
 * frees as it frees what it reads from disk.
 */
static char *project_source(const char *path, int *out_len, void *userdata) {
    struct pdxe_project *p = (struct pdxe_project *)userdata;
    if (!p || !path || !out_len) {
        return NULL;
    }
    for (int i = 0; i < p->n_files; i++) {
        if (strcmp(p->files[i].path, path) != 0) {
            continue;
        }
        p->state[i].source_asked = true;
        int len = p->source_lens[i];
        char *buf = (char *)malloc((size_t)len + SOURCE_PAD);
        if (!buf) {
            return NULL;
        }
        memcpy(buf, p->sources[i], (size_t)len);
        memset(buf + len, 0, SOURCE_PAD);
        *out_len = len;
        p->state[i].source_served = true;
        return buf;
    }
    return NULL;
}

int pdxe_resolve_project_begin(pdxe_ctx *ctx, pdxe_project **out) {
    if (!out) {
        return PDXE_E_INVALID;
    }
    *out = NULL;
    if (!ctx) {
        return PDXE_E_INVALID;
    }
    pdxe_project *p = (pdxe_project *)calloc(1, sizeof(*p));
    if (!p) {
        return PDXE_E_NOMEM;
    }
    p->ctx = ctx;
    p->gbuf = pdxe_gbuf_new();
    p->registry = pdxe_registry_new();
    if (!p->gbuf || !p->registry) {
        pdxe_resolve_project_end(p);
        return PDXE_E_NOMEM;
    }
    *out = p;
    return PDXE_OK;
}

/*
 * Adds one file.
 *
 * When `r` is a result extraction returned, the project resolves through it rather
 * than extracting the file again: the extraction the caller already paid for is the
 * one the resolver reads. The resolver writes into what it reads (it merges its
 * answers into the result's resolutions and appends the call sites it finds), so the
 * project takes a copy of both before it starts and puts them back when it ends. The
 * caller's result is therefore exactly as it was once the project has ended; until
 * then it must stay alive and may belong to no other project.
 *
 * When `r` is NULL the caller has no result for the file, and extracting it here is
 * the only way to have one. That is the caller's choice, made in the open; the
 * project never extracts a file it was given a result for.
 */
static int grow_files(pdxe_project *p) {
    if (p->n_files < p->cap_files) {
        return PDXE_OK;
    }
    int cap = p->cap_files ? p->cap_files * 2 : 16;
    pdxe_file_info_t *files = (pdxe_file_info_t *)realloc(p->files, (size_t)cap * sizeof(*files));
    if (files) {
        p->files = files;
    }
    PDXEFileResult **cache = (PDXEFileResult **)realloc(p->cache, (size_t)cap * sizeof(*cache));
    if (cache) {
        p->cache = cache;
    }
    char **sources = (char **)realloc(p->sources, (size_t)cap * sizeof(*sources));
    if (sources) {
        p->sources = sources;
    }
    int *lens = (int *)realloc(p->source_lens, (size_t)cap * sizeof(*lens));
    if (lens) {
        p->source_lens = lens;
    }
    file_state *state = (file_state *)realloc(p->state, (size_t)cap * sizeof(*state));
    if (state) {
        p->state = state;
    }
    if (!files || !cache || !sources || !lens || !state) {
        return PDXE_E_NOMEM;
    }
    p->cap_files = cap;
    return PDXE_OK;
}

int pdxe_resolve_project_add_file(pdxe_project *p, int lang, const char *rel_path,
                                  const uint8_t *bytes, size_t len, const pdxe_file_result *r) {
    if (!p || !rel_path || (!bytes && len > 0)) {
        return PDXE_E_INVALID;
    }
    if (p->ran) {
        return PDXE_E_INVALID;
    }
    int engine_lang = engine_language(lang);
    if (engine_lang < 0) {
        return PDXE_E_LANG;
    }
    if (len > (size_t)INT_MAX) {
        return PDXE_E_TOOLARGE;
    }
    for (int i = 0; i < p->n_files; i++) {
        if (strcmp(p->files[i].rel_path, rel_path) == 0) {
            return PDXE_E_INVALID; /* a path names one file */
        }
    }
    result_holder *holder = (result_holder *)r;
    if (holder && holder->in_project) {
        return PDXE_E_INVALID; /* already being resolved by another project */
    }
    if (holder && holder->engine && holder->lang != lang) {
        return PDXE_E_INVALID; /* extracted as a different language */
    }
    if (grow_files(p) != PDXE_OK) {
        return PDXE_E_NOMEM;
    }

    file_state st;
    memset(&st, 0, sizeof(st));
    st.abi_lang = lang;
    st.source = !holder ? EXTRACT_HERE : holder->engine ? FROM_CALLER : FROM_SURFACE;
    char *source = (char *)malloc(len + SOURCE_PAD);
    char *path_copy = strdup(rel_path);
    char *rel_copy = strdup(rel_path);
    if (!source || !path_copy || !rel_copy ||
        line_index_build(&st.li, (const uint8_t *)(len > 0 ? (const char *)bytes : ""), len) != 0) {
        free(source);
        free(path_copy);
        free(rel_copy);
        return PDXE_E_NOMEM;
    }
    if (len > 0) {
        memcpy(source, bytes, len);
    }
    memset(source + len, 0, SOURCE_PAD);
    if (st.source == FROM_CALLER) {
        st.borrowed = holder;
        holder->in_project = true;
        st.extraction_lost = holder->lost;
    }

    int i = p->n_files++;
    memset(&p->files[i], 0, sizeof(p->files[i]));
    /* The path is the key the source provider answers by, not a location on disk. */
    p->files[i].path = path_copy;
    p->files[i].rel_path = rel_copy;
    p->files[i].language = (PDXELanguage)engine_lang;
    p->files[i].size = (int64_t)len;
    p->cache[i] = st.source == FROM_CALLER ? holder->engine : NULL;
    p->sources[i] = source;
    p->source_lens[i] = (int)len;
    p->state[i] = st;
    return PDXE_OK;
}

/* --- surfaces -------------------------------------------------------------- */

int pdxe_surface_export(const pdxe_file_result *r, const char *rel_path, uint8_t **out,
                        size_t *out_len) {
    const result_holder *h = (const result_holder *)r;
    if (out) {
        *out = NULL;
    }
    if (out_len) {
        *out_len = 0;
    }
    /* Only a result extraction produced has an engine result to take a surface from,
     * and only while no project is writing into it. */
    if (!h || !rel_path || !out || !out_len || !h->engine || h->in_project) {
        return PDXE_E_INVALID;
    }
    return pdxe_surface_encode(h->engine, rel_path, h->lang, h->lost, h->source, h->source_len,
                               out, out_len);
}

void pdxe_surface_free(uint8_t *bytes) {
    free(bytes);
}

int pdxe_surface_import(pdxe_project *p, const uint8_t *bytes, size_t len) {
    if (!p || !bytes || p->ran) {
        return PDXE_E_INVALID;
    }
    PDXEFileResult *r = NULL;
    char *path = NULL;
    int lang = 0;
    uint32_t lost = 0;
    pdxe_surface_texts *texts = NULL;
    int rc = pdxe_surface_decode(bytes, len, &r, &path, &lang, &lost, &texts);
    if (rc != PDXE_OK) {
        return rc;
    }
    for (int i = 0; i < p->n_surfaces; i++) {
        if (strcmp(p->surface_paths[i], path) == 0) {
            pdxe_free_result(r);
            free(path);
            pdxe_surface_texts_free(texts);
            return PDXE_E_INVALID; /* one surface per path */
        }
    }
    if (p->n_surfaces == p->cap_surfaces) {
        int cap = p->cap_surfaces ? p->cap_surfaces * 2 : 16;
        PDXEFileResult **rs = (PDXEFileResult **)realloc(p->surfaces, (size_t)cap * sizeof(*rs));
        if (rs) {
            p->surfaces = rs;
        }
        char **ps = (char **)realloc(p->surface_paths, (size_t)cap * sizeof(*ps));
        if (ps) {
            p->surface_paths = ps;
        }
        int *ls = (int *)realloc(p->surface_langs, (size_t)cap * sizeof(*ls));
        if (ls) {
            p->surface_langs = ls;
        }
        uint32_t *lo = (uint32_t *)realloc(p->surface_lost, (size_t)cap * sizeof(*lo));
        if (lo) {
            p->surface_lost = lo;
        }
        pdxe_surface_texts **ts =
            (pdxe_surface_texts **)realloc(p->surface_texts, (size_t)cap * sizeof(*ts));
        if (ts) {
            p->surface_texts = ts;
        }
        if (!rs || !ps || !ls || !lo || !ts) {
            pdxe_free_result(r);
            free(path);
            pdxe_surface_texts_free(texts);
            return PDXE_E_NOMEM;
        }
        p->cap_surfaces = cap;
    }
    p->surfaces[p->n_surfaces] = r;
    p->surface_paths[p->n_surfaces] = path;
    p->surface_langs[p->n_surfaces] = lang;
    p->surface_lost[p->n_surfaces] = lost;
    p->surface_texts[p->n_surfaces] = texts;
    p->n_surfaces++;
    return PDXE_OK;
}

/*
 * Gives every file its engine result, now that everything the caller will supply has
 * arrived: its own extraction, the surface imported for it, or an extraction made
 * here for a file added without one. A result rebuilt from a cache with no surface
 * cannot be resolved, and a surface for no file cannot be either; both are errors,
 * never a silent extraction. Records each result's state before the resolver writes
 * into it.
 */
static int materialise_results(pdxe_project *p) {
    for (int i = 0; i < p->n_files; i++) {
        file_state *st = &p->state[i];
        if (st->source != FROM_CALLER) {
            int found = -1;
            for (int k = 0; k < p->n_surfaces; k++) {
                if (p->surfaces[k] && strcmp(p->surface_paths[k], p->files[i].rel_path) == 0) {
                    found = k;
                    break;
                }
            }
            if (found >= 0) {
                if (p->surface_langs[found] != st->abi_lang) {
                    return PDXE_E_INVALID;
                }
                /* Its text ranges become the text of the source the file was added
                 * with, which must be the length the surface records (issue 90). */
                pdxe_surface_texts *texts = p->surface_texts[found];
                p->surface_texts[found] = NULL;
                int bound = pdxe_surface_bind(p->surfaces[found], texts, p->sources[i],
                                              (size_t)p->source_lens[i]);
                if (bound != PDXE_OK) {
                    return bound;
                }
                p->cache[i] = p->surfaces[found];
                p->surfaces[found] = NULL;
                st->source = FROM_SURFACE;
                st->extraction_lost = p->surface_lost[found];
            } else if (st->source == EXTRACT_HERE) {
                p->cache[i] = pdxe_engine_extract_file(p->sources[i], p->source_lens[i],
                                                       p->files[i].language, PROJECT_SENTINEL,
                                                       p->files[i].rel_path, 0, NULL, NULL);
                if (!p->cache[i]) {
                    return PDXE_E_NOMEM;
                }
            } else {
                return PDXE_E_INVALID;
            }
        }
        PDXEFileResult *engine = p->cache[i];
        st->snapshot = true;
        st->n_calls_before = engine->calls.count;
        st->n_resolved_before = engine->resolved_calls.count;
        if (st->source == FROM_CALLER && engine->resolved_calls.count > 0) {
            size_t size = (size_t)engine->resolved_calls.count * sizeof(PDXEResolvedCall);
            st->resolved_before = (PDXEResolvedCall *)malloc(size);
            if (!st->resolved_before) {
                st->snapshot = false; /* nothing written yet, so nothing to restore */
                return PDXE_E_NOMEM;
            }
            memcpy(st->resolved_before, engine->resolved_calls.items, size);
        }
    }
    for (int k = 0; k < p->n_surfaces; k++) {
        if (p->surfaces[k]) {
            return PDXE_E_INVALID;
        }
    }
    return PDXE_OK;
}

/* --- metadata -------------------------------------------------------------- */

static int cmp_package(const void *a, const void *b) {
    const pdxe_package_entry *x = (const pdxe_package_entry *)a;
    const pdxe_package_entry *y = (const pdxe_package_entry *)b;
    int c = strcmp(x->import_prefix ? x->import_prefix : "", y->import_prefix ? y->import_prefix : "");
    if (c != 0) {
        return c;
    }
    return strcmp(x->entry_path ? x->entry_path : "", y->entry_path ? y->entry_path : "");
}

/*
 * Aliases most specific first: the longest prefix wins, which is the resolution rule
 * of every build tool that has aliases. The reference sorts on length alone with a
 * sort that is not stable, so aliases whose prefixes are equally long come out in an
 * order that depends on the platform's sort. Ties are broken here by content, so the
 * same aliases resolve the same way everywhere.
 */
static int cmp_alias(const void *a, const void *b) {
    const pdxe_path_alias_t *x = (const pdxe_path_alias_t *)a;
    const pdxe_path_alias_t *y = (const pdxe_path_alias_t *)b;
    size_t lx = strlen(x->alias_prefix), ly = strlen(y->alias_prefix);
    if (lx != ly) {
        return lx > ly ? -1 : 1;
    }
    int c;
    if ((c = strcmp(x->alias_prefix, y->alias_prefix)) != 0) return c;
    if ((c = strcmp(x->alias_suffix, y->alias_suffix)) != 0) return c;
    if ((c = strcmp(x->target_prefix, y->target_prefix)) != 0) return c;
    return strcmp(x->target_suffix, y->target_suffix);
}

/* Scopes nearest first: the deepest directory is the nearest ancestor of a file. */
static int cmp_scope(const void *a, const void *b) {
    const pdxe_path_alias_scope_t *x = (const pdxe_path_alias_scope_t *)a;
    const pdxe_path_alias_scope_t *y = (const pdxe_path_alias_scope_t *)b;
    size_t lx = strlen(x->dir_prefix), ly = strlen(y->dir_prefix);
    if (lx != ly) {
        return lx > ly ? -1 : 1;
    }
    return strcmp(x->dir_prefix, y->dir_prefix);
}

static char *dup_str(const char *s) {
    return strdup(s ? s : "");
}

static void free_aliases(pdxe_path_alias_collection_t *c) {
    if (!c) {
        return;
    }
    for (int i = 0; i < c->count; i++) {
        pdxe_path_alias_scope_t *sc = &c->scopes[i];
        if (sc->map) {
            for (int k = 0; k < sc->map->count; k++) {
                free(sc->map->entries[k].alias_prefix);
                free(sc->map->entries[k].alias_suffix);
                free(sc->map->entries[k].target_prefix);
                free(sc->map->entries[k].target_suffix);
            }
            free(sc->map->entries);
            free(sc->map->base_url);
            free(sc->map);
        }
        free(sc->dir_prefix);
        free(sc->source_rel_path);
    }
    free(c->scopes);
    free(c);
}

int pdxe_resolve_project_set_metadata(pdxe_project *p, const pdxe_resolution_metadata *m) {
    if (!p || !m || p->metadata_set || p->ran) {
        return PDXE_E_INVALID;
    }
    p->metadata_set = true;

    /* Packages: an import prefix to the module its entry path names, exactly as the
     * reference computes a package map's values. Sorted first, so that when one
     * prefix appears twice the same entry wins whatever order they arrived in. */
    if (m->n_packages > 0 && m->packages) {
        pdxe_package_entry *sorted =
            (pdxe_package_entry *)malloc((size_t)m->n_packages * sizeof(*sorted));
        if (!sorted) {
            return PDXE_E_NOMEM;
        }
        memcpy(sorted, m->packages, (size_t)m->n_packages * sizeof(*sorted));
        qsort(sorted, m->n_packages, sizeof(*sorted), cmp_package);
        p->pkgmap = pdxe_ht_create(m->n_packages * 2 + 8);
        if (!p->pkgmap) {
            free(sorted);
            return PDXE_E_NOMEM;
        }
        for (uint32_t i = 0; i < m->n_packages; i++) {
            if (!sorted[i].import_prefix || !sorted[i].import_prefix[0]) {
                continue;
            }
            char *key = keep(&p->owned, dup_str(sorted[i].import_prefix));
            char *qn = keep(&p->owned,
                            pdxe_pipeline_fqn_module(PROJECT_SENTINEL, sorted[i].entry_path
                                                                           ? sorted[i].entry_path
                                                                           : ""));
            if (!key || !qn) {
                free(sorted);
                return PDXE_E_NOMEM;
            }
            pdxe_ht_set(p->pkgmap, key, qn);
        }
        free(sorted);
    }

    /* Aliases: scopes and their entries in resolution order. */
    if (m->n_alias_scopes > 0 && m->alias_scopes) {
        pdxe_path_alias_collection_t *c =
            (pdxe_path_alias_collection_t *)calloc(1, sizeof(*c));
        if (!c) {
            return PDXE_E_NOMEM;
        }
        c->scopes = (pdxe_path_alias_scope_t *)calloc(m->n_alias_scopes, sizeof(*c->scopes));
        if (!c->scopes) {
            free(c);
            return PDXE_E_NOMEM;
        }
        c->count = (int)m->n_alias_scopes;
        for (uint32_t i = 0; i < m->n_alias_scopes; i++) {
            const pdxe_alias_scope *in = &m->alias_scopes[i];
            pdxe_path_alias_scope_t *sc = &c->scopes[i];
            sc->dir_prefix = dup_str(in->dir_prefix);
            sc->source_rel_path = dup_str(in->dir_prefix);
            sc->map = (pdxe_path_alias_map_t *)calloc(1, sizeof(*sc->map));
            if (!sc->dir_prefix || !sc->source_rel_path || !sc->map) {
                free_aliases(c);
                return PDXE_E_NOMEM;
            }
            sc->map->base_url = in->base_url ? strdup(in->base_url) : NULL;
            if (in->n_aliases > 0 && in->aliases) {
                sc->map->entries =
                    (pdxe_path_alias_t *)calloc(in->n_aliases, sizeof(*sc->map->entries));
                if (!sc->map->entries) {
                    free_aliases(c);
                    return PDXE_E_NOMEM;
                }
                sc->map->count = (int)in->n_aliases;
                for (uint32_t k = 0; k < in->n_aliases; k++) {
                    const pdxe_path_alias *a = &in->aliases[k];
                    pdxe_path_alias_t *e = &sc->map->entries[k];
                    e->alias_prefix = dup_str(a->alias_prefix);
                    e->alias_suffix = dup_str(a->alias_suffix);
                    e->target_prefix = dup_str(a->target_prefix);
                    e->target_suffix = dup_str(a->target_suffix);
                    e->has_wildcard = a->has_wildcard != 0;
                    if (!e->alias_prefix || !e->alias_suffix || !e->target_prefix ||
                        !e->target_suffix) {
                        free_aliases(c);
                        return PDXE_E_NOMEM;
                    }
                }
                qsort(sc->map->entries, (size_t)sc->map->count, sizeof(*sc->map->entries), cmp_alias);
            }
        }
        qsort(c->scopes, (size_t)c->count, sizeof(*c->scopes), cmp_scope);
        p->aliases = c;
    }

    /* The crate manifest, turned into what the engine's manifest reader produces:
     * the same caps, and each member named by the last segment of its path. */
    if (m->crate_manifest) {
        const pdxe_crate_manifest *in = m->crate_manifest;
        PDXECargoManifest *out = &p->crate;
        memset(out, 0, sizeof(*out));
        out->package_name = in->package_name ? keep(&p->owned, dup_str(in->package_name)) : NULL;
        if (in->package_name && !out->package_name) {
            return PDXE_E_NOMEM;
        }
        out->is_workspace_root = in->is_workspace_root != 0;
        for (uint32_t i = 0; in->dependencies && i < in->n_dependencies &&
                             out->dep_count < PDXE_CARGO_MAX_DEPS;
             i++) {
            const pdxe_crate_dependency *d = &in->dependencies[i];
            if (!d->name) {
                continue;
            }
            PDXECargoDep *dep = &out->deps[out->dep_count];
            dep->name = keep(&p->owned, dup_str(d->name));
            dep->path = d->path ? keep(&p->owned, dup_str(d->path)) : NULL;
            if (!dep->name || (d->path && !dep->path)) {
                return PDXE_E_NOMEM;
            }
            out->dep_count++;
        }
        for (uint32_t i = 0; in->member_paths && i < in->n_members &&
                             out->member_count < PDXE_CARGO_MAX_MEMBERS;
             i++) {
            if (!in->member_paths[i]) {
                continue;
            }
            char *path = keep(&p->owned, dup_str(in->member_paths[i]));
            if (!path) {
                return PDXE_E_NOMEM;
            }
            const char *last = strrchr(path, '/');
            out->members[out->member_count].member_name = last ? last + 1 : path;
            out->members[out->member_count].member_path = path;
            out->member_count++;
        }
        p->crate_set = true;
    }
    return PDXE_OK;
}

/* --- the reference's setup passes, in its order ---------------------------- */

static const char *base_name(const char *path) {
    const char *slash = strrchr(path, '/');
    return slash ? slash + 1 : path;
}

/*
 * Structure: the project's own node, then for each file in the order it was added,
 * its node followed by the directories above it not yet seen, deepest first. Stands
 * in for the reference's structure pass and inserts in exactly its order, which is
 * what fixes identifiers and the order of short-name lookups. The properties are the
 * reference's too, though nothing the resolver reads looks at them.
 */
static int build_structure(pdxe_project *p) {
    if (!pdxe_gbuf_upsert_node(p->gbuf, "Project", PROJECT_SENTINEL, PROJECT_SENTINEL, NULL, 0, 0,
                               "{}")) {
        return PDXE_E_NOMEM;
    }
    PDXEHashTable *seen = pdxe_ht_create(64);
    string_list seen_keys = {0};
    int rc = seen ? PDXE_OK : PDXE_E_NOMEM;
    for (int i = 0; rc == PDXE_OK && i < p->n_files; i++) {
        const char *rel = p->files[i].rel_path;
        const char *base = base_name(rel);
        const char *ext = strrchr(base, '.');
        size_t props_cap = strlen(ext ? ext : "") + 32;
        char *props = (char *)malloc(props_cap);
        char *file_qn = pdxe_pipeline_fqn_compute(PROJECT_SENTINEL, rel, "__file__");
        if (!props || !file_qn) {
            free(props);
            free(file_qn);
            rc = PDXE_E_NOMEM;
            break;
        }
        snprintf(props, props_cap, "{\"extension\":\"%s\"}", ext ? ext : "");
        int64_t id = pdxe_gbuf_upsert_node(p->gbuf, "File", base, file_qn, rel, 0, 0, props);
        free(props);
        free(file_qn);
        if (!id) {
            rc = PDXE_E_NOMEM;
            break;
        }

        /* The directories above the file, from the nearest up to the first seen. */
        const char *slash = strrchr(rel, '/');
        char *walk = slash ? pdxe_strndup(rel, (size_t)(slash - rel)) : strdup("");
        if (!walk) {
            rc = PDXE_E_NOMEM;
            break;
        }
        while (walk[0] != '\0' && !pdxe_ht_has(seen, walk)) {
            char *key = strdup(walk);
            char *folder_qn = pdxe_pipeline_fqn_folder(PROJECT_SENTINEL, walk);
            if (!key || !keep(&seen_keys, key) || !folder_qn ||
                !pdxe_gbuf_upsert_node(p->gbuf, "Folder", base_name(walk), folder_qn, walk, 0, 0,
                                       "{}")) {
                free(folder_qn);
                rc = PDXE_E_NOMEM;
                break;
            }
            pdxe_ht_set(seen, key, key);
            free(folder_qn);
            char *up = strrchr(walk, '/');
            if (up) {
                *up = '\0';
            } else {
                walk[0] = '\0';
            }
        }
        free(walk);
    }
    if (seen) {
        pdxe_ht_free(seen);
    }
    string_list_free(&seen_keys);
    return rc;
}

/*
 * Definitions: for each file in order, each definition becomes a node (or updates
 * the node already under its qualified name, by the reference's rule), and those the
 * reference treats as resolvable symbols also enter the registry. Stands in for the
 * reference's definitions pass: a definition without a name or qualified name is
 * skipped, a missing kind is a function, a missing path is the file's own, and the
 * registry takes every qualifying definition, whichever node survived.
 */
static int build_definitions(pdxe_project *p) {
    for (int i = 0; i < p->n_files; i++) {
        const PDXEDefArray *defs = &p->cache[i]->defs;
        for (int k = 0; k < defs->count; k++) {
            const PDXEDefinition *d = &defs->items[k];
            if (!d->qualified_name || !d->name) {
                continue;
            }
            int64_t id = pdxe_gbuf_upsert_node(
                p->gbuf, d->label ? d->label : "Function", d->name, d->qualified_name,
                d->file_path ? d->file_path : p->files[i].rel_path, (int)d->start_line,
                (int)d->end_line, NULL);
            if (!id) {
                return PDXE_E_NOMEM;
            }
            if (pdxe_label_is_registry_symbol(d->label)) {
                pdxe_registry_add(p->registry, d->name, d->qualified_name, d->label);
            }
        }
    }
    return PDXE_OK;
}

/*
 * Import edges: one per import the resolver can place, none for one it cannot, and
 * never an edge from a file to itself. Stands in for the reference's
 * create_import_edges_for_file, including the escaping of the local name.
 */
static int build_imports(pdxe_project *p) {
    const char **rels = (const char **)calloc((size_t)p->n_files, sizeof(char *));
    if (!rels) {
        return PDXE_E_NOMEM;
    }
    for (int i = 0; i < p->n_files; i++) {
        rels[i] = p->files[i].rel_path;
    }
    /* Built from every file, because an incomplete map changes answers. */
    PDXEHashTable *nsmap = pdxe_pipeline_namespace_map_build(
        PROJECT_SENTINEL, (PDXEFileResult *const *)p->cache, rels, p->n_files);
    free(rels);

    for (int i = 0; i < p->n_files; i++) {
        const char *rel = p->files[i].rel_path;
        char *file_qn = pdxe_pipeline_fqn_compute(PROJECT_SENTINEL, rel, "__file__");
        const pdxe_gbuf_node_t *src = file_qn ? pdxe_gbuf_find_by_qn(p->gbuf, file_qn) : NULL;
        if (!src) {
            free(file_qn);
            continue;
        }
        const PDXEImportArray *imports = &p->cache[i]->imports;
        for (int k = 0; k < imports->count; k++) {
            const PDXEImport *imp = &imports->items[k];
            if (!imp->module_path) {
                continue;
            }
            const pdxe_gbuf_node_t *target =
                pdxe_pipeline_resolve_import_node(&p->pctx, rel, file_qn, imp, nsmap);
            if (!target || target->id == src->id) {
                continue;
            }
            char escaped[128];
            pdxe_json_escape(escaped, (int)sizeof(escaped), imp->local_name ? imp->local_name : "");
            char props[256];
            snprintf(props, sizeof(props), "{\"local_name\":\"%s\"}", escaped);
            pdxe_gbuf_add_edge(p->gbuf, src->id, target->id, "IMPORTS", target->qualified_name, props);
        }
        free(file_qn);
    }
    if (nsmap) {
        pdxe_pipeline_namespace_map_free(nsmap);
    }
    return PDXE_OK;
}

/* Every answer the typed pass gives is a type-directed one. */
static const char TYPED_STRATEGY[] = "lsp_typed";

/*
 * The reference labels an answer whose resolver named no strategy as an override of
 * the textual match; so does this, so the label is never missing.
 */
static const char *engine_strategy_of(const PDXEResolvedCall *rc) {
    return rc->strategy ? rc->strategy : "lsp_override";
}

typedef struct {
    pdxe_resolution *items;
    uint32_t count;
    uint32_t cap;
} resolution_list;

static pdxe_resolution *next_resolution(resolution_list *l) {
    if (l->count == l->cap) {
        uint32_t cap = l->cap ? l->cap * 2 : 64;
        pdxe_resolution *grown =
            (pdxe_resolution *)realloc(l->items, (size_t)cap * sizeof(*grown));
        if (!grown) {
            return NULL;
        }
        l->items = grown;
        l->cap = cap;
    }
    pdxe_resolution *out = &l->items[l->count++];
    memset(out, 0, sizeof(*out));
    return out;
}

/* A target in no file of the project, such as a built-in, has no path here. */
static const char *target_path(const pdxe_gbuf_node_t *target) {
    return (target && target->file_path && target->file_path[0] != '<') ? target->file_path
                                                                          : NULL;
}

/*
 * The node a call or reference is attributed to: its enclosing definition, or the
 * file when there is none or when the enclosing name is a directory's (a class-level
 * site in a language whose modules are directories). The reference's rule, in both
 * its calls and its usages passes.
 */
static const pdxe_gbuf_node_t *source_node(const pdxe_project *p, int i, const char *enclosing_qn) {
    const pdxe_gbuf_node_t *src = NULL;
    if (enclosing_qn && enclosing_qn[0]) {
        src = pdxe_gbuf_find_by_qn(p->gbuf, enclosing_qn);
        if (src && src->label &&
            (strcmp(src->label, "Folder") == 0 || strcmp(src->label, "Project") == 0)) {
            src = NULL;
        }
    }
    if (!src) {
        char *file_qn = pdxe_pipeline_fqn_compute(PROJECT_SENTINEL, p->files[i].rel_path, "__file__");
        src = file_qn ? pdxe_gbuf_find_by_qn(p->gbuf, file_qn) : NULL;
        free(file_qn);
    }
    return src;
}

/*
 * Issue 92: a file's invocation rows by the keys the resolver's matcher joins a call on,
 * built once per file. Matching each call against every row of the file made collecting
 * a file's answers quadratic in its calls. The matcher accepts a row only at the call's
 * own occurrence (a row with a site) or, for a row without one (the legacy 0:0 site), by
 * the call's callee leaf, which the row names by its callee or, for some strategies, by
 * its reason; and only for the call's caller, or its caller's Class.method tail under
 * the tail rule. So the rows are chained by site, and the rows without a site by
 * (caller, leaf) and (tail, leaf): every row the matcher can accept for a call is on the
 * call's chains, each chain is in ascending row order, and the matcher gives the full
 * scan's answer (pdxe_pipeline_lsp_rows_t).
 */
enum {
    ROWS_BY_SITE,          /* rows with a site, by site */
    ROWS_BY_CALLER_LEAF,   /* rows without a site, by caller and callee leaf */
    ROWS_BY_CALLER_REASON, /* ... by caller and reason leaf, for the strategies joined so */
    ROWS_BY_TAIL_LEAF,     /* ... by the caller's Class.method tail and callee leaf */
    ROWS_BY_TAIL_REASON,   /* ... by the caller's tail and reason leaf */
    ROWS_KINDS
};

typedef struct {
    const char *a;
    const char *b;
    uint32_t start;
    uint32_t end;
} row_key;

typedef struct {
    const PDXEResolvedCallArray *rows;
    int *next[ROWS_KINDS];  /* per row: the next row with its key, -1 */
    int *first[ROWS_KINDS]; /* open addressing: a key's first row, -1 empty */
    size_t cap;
    bool reference; /* test builds: match against every row, as before issue 92 */
} invocation_index;

/* Row r's key of `kind`; false when the row has none there. */
static bool row_key_of(const PDXEResolvedCall *r, int kind, row_key *k) {
    memset(k, 0, sizeof(*k));
    if (r->kind != PDXE_RESOLVED_INVOCATION || !r->caller_qn || !r->callee_qn) {
        return false; /* a row the matcher never accepts */
    }
    if (kind == ROWS_BY_SITE) {
        k->start = r->site_start_byte;
        k->end = r->site_end_byte;
        return pdxe_pipeline_source_site_present(r->site_start_byte, r->site_end_byte);
    }
    if (!pdxe_pipeline_source_site_legacy(r->site_start_byte, r->site_end_byte)) {
        return false;
    }
    bool by_reason = kind == ROWS_BY_CALLER_REASON || kind == ROWS_BY_TAIL_REASON;
    if (by_reason && (!pdxe_pipeline_invocation_reason_join_strategy(r->strategy) || !r->reason)) {
        return false;
    }
    k->a = kind == ROWS_BY_CALLER_LEAF || kind == ROWS_BY_CALLER_REASON
               ? r->caller_qn
               : pdxe_pipeline_qn_class_method_tail(r->caller_qn);
    k->b = pdxe_lsp_bare_segment(by_reason ? r->reason : r->callee_qn);
    return k->a && k->b;
}

static uint64_t row_key_hash(int kind, const row_key *k) {
    uint64_t h;
    if (kind == ROWS_BY_SITE) {
        h = ((uint64_t)k->start << 32) | k->end;
    } else {
        h = qn_hash(k->a) * 1099511628211ULL ^ qn_hash(k->b);
    }
    h ^= h >> 33;
    h *= 0xff51afd7ed558ccdULL;
    h ^= h >> 33;
    return h;
}

static bool row_key_eq(int kind, const row_key *x, const row_key *y) {
    if (kind == ROWS_BY_SITE) {
        return x->start == y->start && x->end == y->end;
    }
    return strcmp(x->a, y->a) == 0 && strcmp(x->b, y->b) == 0;
}

/* The slot of key k of `kind`: where its chain is, or the empty slot it would take. */
static size_t row_slot(const invocation_index *x, int kind, const row_key *k) {
    size_t mask = x->cap - 1;
    for (size_t at = (size_t)row_key_hash(kind, k) & mask;; at = (at + 1) & mask) {
        int r = x->first[kind][at];
        row_key held;
        if (r < 0 || (row_key_of(&x->rows->items[r], kind, &held) && row_key_eq(kind, &held, k))) {
            return at;
        }
    }
}

static void invocation_index_free(invocation_index *x) {
    for (int kind = 0; kind < ROWS_KINDS; kind++) {
        free(x->next[kind]);
        free(x->first[kind]);
    }
}

/* Builds the index of `rows`; false when it cannot be allocated (the memory core counts
 * that as lost work), which the caller reports as such. */
static bool invocation_index_open(invocation_index *x, const PDXEResolvedCallArray *rows) {
    memset(x, 0, sizeof(*x));
    x->rows = rows;
#if defined(PDXE_ENABLE_TEST_SEAMS)
    const char *reference = getenv("PDX_ENGINE_TEST_LINEAR_LOOKUP_REFERENCE");
    x->reference = reference && reference[0];
    if (x->reference) {
        return true;
    }
#endif
    if (PDXE_LOOKUP_INDEX_REFUSED("interface")) {
        return false;
    }
    size_t n = rows->count > 0 ? (size_t)rows->count : 1;
    x->cap = 16;
    while (x->cap < 2 * n) {
        x->cap *= 2;
    }
    for (int kind = 0; kind < ROWS_KINDS; kind++) {
        x->next[kind] = (int *)pdxe_counted_malloc(n * sizeof(int));
        x->first[kind] = (int *)pdxe_counted_malloc(x->cap * sizeof(int));
        if (!x->next[kind] || !x->first[kind]) {
            invocation_index_free(x);
            return false;
        }
        for (size_t at = 0; at < x->cap; at++) {
            x->first[kind][at] = -1;
        }
    }
    /* Prepending in descending order leaves every chain ascending. */
    for (int r = rows->count - 1; r >= 0; r--) {
        for (int kind = 0; kind < ROWS_KINDS; kind++) {
            row_key k;
            x->next[kind][r] = -1;
            if (!row_key_of(&rows->items[r], kind, &k)) {
                continue;
            }
            size_t at = row_slot(x, kind, &k);
            x->next[kind][r] = x->first[kind][at];
            x->first[kind][at] = r;
        }
    }
    return true;
}

/* The first row with key k of `kind`, -1 when none. */
static int rows_first(const invocation_index *x, int kind, const row_key *k) {
    return x->first[kind][row_slot(x, kind, k)];
}

/* The resolver's typed answer for `call`: pdxe_pipeline_find_lsp_resolution_in_graph,
 * inspecting only the rows on the call's chains. */
static const PDXEResolvedCall *invocation_of(const invocation_index *x, const PDXECall *call,
                                             bool allow_tail, const pdxe_gbuf_t *gbuf) {
    if (x->reference || !call->enclosing_func_qn || !call->callee_name || x->rows->count == 0) {
        return pdxe_pipeline_find_lsp_resolution_in_graph(x->rows, call, allow_tail, gbuf,
                                                          PROJECT_SENTINEL);
    }
    const char *leaf = pdxe_lsp_bare_segment(call->callee_name);
    const char *tail = pdxe_pipeline_qn_class_method_tail(call->enclosing_func_qn);
    pdxe_pipeline_lsp_rows_t exact = pdxe_pipeline_lsp_rows_none();
    pdxe_pipeline_lsp_rows_t by_tail = pdxe_pipeline_lsp_rows_none();
    if (pdxe_pipeline_source_site_present(call->site_start_byte, call->site_end_byte)) {
        row_key site = {NULL, NULL, call->site_start_byte, call->site_end_byte};
        int first = rows_first(x, ROWS_BY_SITE, &site);
        pdxe_pipeline_lsp_rows_add(&exact, 0, first, x->next[ROWS_BY_SITE]);
        pdxe_pipeline_lsp_rows_add(&by_tail, 0, first, x->next[ROWS_BY_SITE]);
    }
    row_key caller = {call->enclosing_func_qn, leaf, 0, 0};
    pdxe_pipeline_lsp_rows_add(&exact, 1, rows_first(x, ROWS_BY_CALLER_LEAF, &caller),
                               x->next[ROWS_BY_CALLER_LEAF]);
    pdxe_pipeline_lsp_rows_add(&exact, 2, rows_first(x, ROWS_BY_CALLER_REASON, &caller),
                               x->next[ROWS_BY_CALLER_REASON]);
    if (tail) {
        row_key by = {tail, leaf, 0, 0};
        pdxe_pipeline_lsp_rows_add(&by_tail, 1, rows_first(x, ROWS_BY_TAIL_LEAF, &by),
                                   x->next[ROWS_BY_TAIL_LEAF]);
        pdxe_pipeline_lsp_rows_add(&by_tail, 2, rows_first(x, ROWS_BY_TAIL_REASON, &by),
                                   x->next[ROWS_BY_TAIL_REASON]);
    }
    return pdxe_pipeline_find_lsp_resolution_in_rows(x->rows, call, allow_tail, gbuf,
                                                     PROJECT_SENTINEL, exact, by_tail);
}

/*
 * The typed answer for each call: the reference's calls pass, up to the point where
 * it has a target. The answer is joined to the call by the reference's own matcher
 * (caller, callee leaf, exact site, confidence floor, ambiguity), and counts only when
 * its target is a node of the project, found the way the reference finds it, and is
 * not the calling definition itself: the reference's calls pass sets such an answer
 * aside and resolves the call by name instead, so it is not reported here either.
 */
static int collect_calls(pdxe_project *p, int i, uint32_t n_refs, resolution_list *out) {
    const PDXEFileResult *res = p->cache[i];
    const file_state *st = &p->state[i];
    PDXELanguage lang = p->files[i].language;
    bool allow_tail = pdxe_pipeline_lsp_allow_tail_match(lang);
    /* Each answer's site names its caller by position (issue 92: by index). */
    qn_positions *names = qn_positions_open(&res->defs);
    if (!names) {
        return PDXE_E_NOMEM;
    }
    invocation_index rows;
    if (!invocation_index_open(&rows, &res->resolved_calls)) {
        qn_positions_free(names);
        return PDXE_E_NOMEM;
    }
    for (int j = 0; j < res->calls.count; j++) {
        const PDXECall *call = &res->calls.items[j];
        if (!call->callee_name) {
            continue;
        }
        const PDXEResolvedCall *lsp = invocation_of(&rows, call, allow_tail, p->gbuf);
        if (!lsp) {
            continue;
        }
        bool exact_external = call->requires_lsp_resolution &&
                              pdxe_pipeline_kotlin_external_target(lang, lsp->callee_qn);
        const pdxe_gbuf_node_t *target =
            exact_external ? pdxe_pipeline_lsp_target_node_strict(p->gbuf, PROJECT_SENTINEL,
                                                                  lsp->callee_qn, allow_tail)
                           : pdxe_pipeline_lsp_target_node(p->gbuf, PROJECT_SENTINEL,
                                                           lsp->callee_qn, allow_tail);
        if (!target) {
            continue; /* no typed answer: the caller resolves the call by other means */
        }
        const pdxe_gbuf_node_t *src = source_node(p, i, call->enclosing_func_qn);
        if (!src || src->id == target->id) {
            continue;
        }
        pdxe_resolution *o = next_resolution(out);
        if (!o) {
            invocation_index_free(&rows);
            qn_positions_free(names);
            return PDXE_E_NOMEM;
        }
        /* The resolver's own calls follow extraction's calls and references. */
        o->call_index = j < st->n_calls_before ? (uint32_t)j : (uint32_t)j + n_refs;
        source_view text = {p->sources[i], (size_t)p->source_lens[i]};
        o->site = call_of(names, &st->li, &text, call, NULL);
        o->site.use_scope = PDXE_NO_PARENT; /* a resolution's site, issue 114 */
        o->target_qualified_name = strip_project(target->qualified_name);
        o->target_rel_path = target_path(target);
        o->score = (double)lsp->confidence;
        o->engine_strategy = engine_strategy_of(lsp);
    }
    invocation_index_free(&rows);
    qn_positions_free(names);
    return PDXE_OK;
}

/*
 * The typed answer for each reference: the reference's usages pass, up to the point
 * where it has a target. A typed answer settles a reference even when its target is
 * no node of the project, in which case the reference emits nothing for it; the
 * resolution is reported regardless, so the caller knows not to guess.
 */
static int collect_references(pdxe_project *p, int i, resolution_list *out) {
    const PDXEFileResult *res = p->cache[i];
    const file_state *st = &p->state[i];
    PDXELanguage lang = p->files[i].language;
    bool allow_tail = pdxe_pipeline_lsp_allow_tail_match(lang);
    uint32_t index = (uint32_t)st->n_calls_before;
    /* Each answer's site names its caller by position (issue 92: by index). */
    qn_positions *names = qn_positions_open(&res->defs);
    if (!names) {
        return PDXE_E_NOMEM;
    }
    /* Issue 92: the file's reference rows by occurrence, so each reference is matched
     * against the rows at its own site -- every row either rule can accept, in their
     * order -- not against every row of the file. Test builds keep the full match for
     * reference (PDX_ENGINE_TEST_LINEAR_LOOKUP_REFERENCE). */
    pdxe_pipeline_lsp_reference_index_t by_site;
    pdxe_pipeline_lsp_reference_index_t *sites = &by_site;
#if defined(PDXE_ENABLE_TEST_SEAMS)
    const char *reference = getenv("PDX_ENGINE_TEST_LINEAR_LOOKUP_REFERENCE");
    if (reference && reference[0]) {
        sites = NULL;
    }
#endif
    if (sites && (PDXE_LOOKUP_INDEX_REFUSED("interface") ||
                  !pdxe_pipeline_lsp_reference_index_build(&res->resolved_calls, sites))) {
        pdxe_lost_allocation(); /* the index could not be allocated */
        qn_positions_free(names);
        return PDXE_E_NOMEM;
    }
    for (int u = 0; u < res->usages.count; u++) {
        const PDXEUsage *usage = &res->usages.items[u];
        if (!is_reference_site(usage)) {
            continue;
        }
        uint32_t call_index = index++;
        if (!usage->ref_name) {
            continue;
        }
        const PDXEResolvedCall *ref = pdxe_pipeline_find_lsp_reference_indexed_in_graph(
            &res->resolved_calls, sites, usage, allow_tail, p->gbuf, PROJECT_SENTINEL);
        if (ref && !pdxe_pipeline_usage_allows_semantic_reference(usage, ref)) {
            ref = NULL;
        }
        if (!ref) {
            continue;
        }
        const pdxe_gbuf_node_t *target =
            pdxe_pipeline_lsp_target_node(p->gbuf, PROJECT_SENTINEL, ref->callee_qn, allow_tail);
        pdxe_resolution *o = next_resolution(out);
        if (!o) {
            pdxe_pipeline_lsp_reference_index_free(sites);
            qn_positions_free(names);
            return PDXE_E_NOMEM;
        }
        o->call_index = call_index;
        source_view text = {p->sources[i], (size_t)p->source_lens[i]};
        o->site = reference_of(names, &st->li, &text, usage, NULL);
        o->target_qualified_name =
            strip_project(target ? target->qualified_name : ref->callee_qn);
        o->target_rel_path = target_path(target);
        o->score = (double)ref->confidence;
        o->engine_strategy = engine_strategy_of(ref);
    }
    pdxe_pipeline_lsp_reference_index_free(sites);
    qn_positions_free(names);
    return PDXE_OK;
}

static int cmp_resolution_site(const void *a, const void *b) {
    const pdxe_resolution *x = (const pdxe_resolution *)a;
    const pdxe_resolution *y = (const pdxe_resolution *)b;
    return x->call_index < y->call_index ? -1 : x->call_index > y->call_index ? 1 : 0;
}

/*
 * The results: for each file in the order added, its resolved sites in call order.
 * Each site appears at most once, so the order is total.
 */
static int collect_results(pdxe_project *p) {
    resolution_list out = {0};
    for (int i = 0; i < p->n_files; i++) {
        uint32_t first = out.count;
        uint32_t n_refs = count_reference_sites(&p->cache[i]->usages);
        int rc = collect_calls(p, i, n_refs, &out);
        if (rc == PDXE_OK) {
            rc = collect_references(p, i, &out);
        }
        if (rc != PDXE_OK) {
            free(out.items);
            return rc;
        }
        for (uint32_t k = first; k < out.count; k++) {
            out.items[k].rel_path = p->files[i].rel_path;
            out.items[k].strategy = TYPED_STRATEGY;
            out.items[k].candidates = 1;
        }
        if (out.count - first > 1) {
            qsort(out.items + first, out.count - first, sizeof(*out.items), cmp_resolution_site);
        }
    }
    p->results = out.items;
    p->n_results = out.count;
    return PDXE_OK;
}

/*
 * Classifies every file by what typed resolution did with it, and decides whether the
 * run was clean (pdxe_run_health in the header). Two accounts are kept and compared:
 * the pass's own record of what it did, and the interface's, from the sources the pass
 * asked for and the results it resolved through. Where they disagree, what the run did
 * is not known, and that is counted as a failure: a run is never called clean on an
 * account that does not add up.
 */
static void compute_health(pdxe_project *p) {
    pdxe_run_health *h = &p->health;
    memset(h, 0, sizeof(*h));
    h->files = (uint32_t)p->n_files;
    if (p->n_files == 0) {
        h->status = PDXE_RUN_CLEAN;
        return;
    }
    const pdxe_lsp_cross_record_t *rec = &p->pctx.lsp_cross;
    uint32_t asked_and_served = 0;
    bool has_definitions = false;
    for (int i = 0; i < p->n_files; i++) {
        const PDXEFileResult *r = p->cache[i];
        const file_state *st = &p->state[i];
        if (r && (r->defs.count > 0 || r->impl_traits.count > 0)) {
            has_definitions = true;
        }
        if (!pdxe_pxc_has_cross_lsp(p->files[i].language)) {
            h->files_untyped++;
        } else if (p->source_lens[i] == 0) {
            h->files_empty++;
        } else if (!rec->completed) {
            h->files_not_reached++;
        } else if (!st->source_served) {
            h->files_source_unavailable++;
        } else if (r && r->lsp_skipped) {
            h->files_over_budget++;
            asked_and_served++;
        } else {
            h->files_resolved++;
            asked_and_served++;
        }
    }
    /* Work lost to failed allocations and exhausted budgets: during the run, and on
     * each file's extraction, where part of its typed resolution happened. */
    uint64_t lost = rec->allocations_failed + rec->work_lost;
    for (int i = 0; i < p->n_files; i++) {
        lost += p->state[i].extraction_lost;
    }
    h->pass_failures = lost > UINT32_MAX ? UINT32_MAX : (uint32_t)lost;
    if (rec->completed) {
        if (has_definitions && !rec->definitions_collected) {
            h->pass_failures++;
        }
        /* The pass counts empty sources among those it could not obtain. */
        bool agrees =
            (uint32_t)rec->files_skipped_no_lsp == h->files_untyped &&
            (uint32_t)rec->files_skipped_no_source == h->files_empty + h->files_source_unavailable &&
            (uint32_t)rec->files_dispatched == asked_and_served;
        if (!agrees) {
            h->pass_failures++;
        }
    }
    bool degraded = h->files_not_reached || h->files_source_unavailable ||
                    h->files_over_budget || h->pass_failures;
    h->status = degraded ? PDXE_RUN_DEGRADED : PDXE_RUN_CLEAN;
}

int pdxe_resolve_project_run(pdxe_project *p) {
    if (!p || p->ran) {
        return PDXE_E_INVALID;
    }
    p->ran = true;
    /* Everything the run loses is counted on this thread while it runs (lost_work.h);
     * the run is this thread's only work until it returns. */
    pdxe_losses_t before = pdxe_losses();

    memset(&p->pctx, 0, sizeof(p->pctx));
    p->pctx.project_name = PROJECT_SENTINEL;
    p->pctx.repo_path = NULL; /* no repository on disk; nothing is read from one */
    p->pctx.gbuf = p->gbuf;
    p->pctx.registry = p->registry;
    /* No pipeline: the pass gives one only the per-file surface rows the reference
     * persists for its incremental builds, which this project never reads, so they are
     * not built at all. */
    p->pctx.pipeline = NULL;
    p->pctx.result_cache = p->cache;
    p->pctx.path_aliases = p->aliases;

    pdxe_pipeline_set_pkgmap(p->pkgmap);
    pdxe_pxc_set_source_provider(project_source, p);
    pdxe_pxc_set_supplied_rust_manifest(p->crate_set ? &p->crate : NULL);

    int rc = materialise_results(p);
    if (rc == PDXE_OK) {
        rc = build_structure(p);
    }
    if (rc == PDXE_OK) {
        rc = build_definitions(p);
    }
    if (rc == PDXE_OK) {
        rc = build_imports(p);
    }
    if (rc == PDXE_OK && p->n_files > 0) {
        pdxe_pipeline_pass_lsp_cross(&p->pctx, p->files, p->n_files, p->cache);
        rc = collect_results(p);
    }
    if (rc == PDXE_OK) {
        pdxe_losses_t after = pdxe_losses();
        p->pctx.lsp_cross.allocations_failed = after.allocations - before.allocations;
        p->pctx.lsp_cross.work_lost = after.work - before.work;
        compute_health(p);
        p->health_known = true;
    }

    pdxe_pxc_set_source_provider(NULL, NULL);
    pdxe_pxc_set_supplied_rust_manifest(NULL);
    pdxe_pipeline_set_pkgmap(NULL);
    return rc;
}

int pdxe_resolve_project_health(const pdxe_project *p, pdxe_run_health *out) {
    if (!p || !out || !p->health_known) {
        return PDXE_E_INVALID;
    }
    *out = p->health;
    return PDXE_OK;
}

int pdxe_resolve_project_results(pdxe_project *p, const pdxe_resolution **out, uint32_t *n) {
    if (!p || !out || !n) {
        return PDXE_E_INVALID;
    }
    *out = p->results;
    *n = p->n_results;
    return p->ran ? PDXE_OK : PDXE_E_INVALID;
}

void pdxe_resolve_project_end(pdxe_project *p) {
    if (!p) {
        return;
    }
    /* The resolver's shared registries and the module names they borrow, which the
     * pass parks on the context to outlive it; released together, as the reference
     * releases them at the end of its run. */
    if (p->pctx.seq_cross_arena_live) {
        pdxe_arena_destroy(&p->pctx.seq_cross_arena);
        p->pctx.seq_cross_arena_live = false;
    }
    if (p->pctx.seq_cross_def_modules) {
        for (int i = 0; i < p->pctx.seq_cross_def_module_count; i++) {
            free(p->pctx.seq_cross_def_modules[i]);
        }
        free(p->pctx.seq_cross_def_modules);
        p->pctx.seq_cross_def_modules = NULL;
    }
    for (int i = 0; i < p->n_files; i++) {
        file_state *st = &p->state[i];
        PDXEFileResult *engine = p->cache[i];
        if (st->borrowed && engine && st->snapshot) {
            /* Hand the caller's result back as it was: the resolver's additions go. */
            engine->calls.count = st->n_calls_before;
            if (st->n_resolved_before > 0) {
                memcpy(engine->resolved_calls.items, st->resolved_before,
                       (size_t)st->n_resolved_before * sizeof(PDXEResolvedCall));
            }
            engine->resolved_calls.count = st->n_resolved_before;
        }
        if (st->borrowed) {
            st->borrowed->in_project = false;
        } else if (engine) {
            pdxe_free_result(engine);
        }
        free(st->resolved_before);
        line_index_free(&st->li);
        free(p->files[i].path);
        free(p->files[i].rel_path);
        free(p->sources[i]);
    }
    free(p->files);
    free(p->cache);
    free(p->sources);
    free(p->source_lens);
    free(p->state);
    for (int k = 0; k < p->n_surfaces; k++) {
        if (p->surfaces[k]) {
            pdxe_free_result(p->surfaces[k]);
        }
        pdxe_surface_texts_free(p->surface_texts[k]);
        free(p->surface_paths[k]);
    }
    free(p->surfaces);
    free(p->surface_paths);
    free(p->surface_langs);
    free(p->surface_lost);
    free(p->surface_texts);
    free(p->results);
    if (p->pkgmap) {
        pdxe_ht_free(p->pkgmap);
    }
    free_aliases(p->aliases);
    pdxe_gbuf_free(p->gbuf);
    if (p->registry) {
        pdxe_registry_free(p->registry);
    }
    string_list_free(&p->owned);
    free(p);
}

void pdxe_project_debug_raw_resolutions(struct pdxe_project *p, pdxe_raw_resolution_fn fn,
                                        void *userdata) {
    if (!p || !fn) {
        return;
    }
    for (int i = 0; i < p->n_files; i++) {
        const PDXEResolvedCallArray *rc = &p->cache[i]->resolved_calls;
        for (int k = 0; k < rc->count; k++) {
            const PDXEResolvedCall *c = &rc->items[k];
            fn(p->files[i].rel_path, c->callee_qn, c->strategy, c->reason, (int)c->kind,
               c->site_start_byte, c->site_end_byte, userdata);
        }
    }
}

void pdxe_project_debug_imports(struct pdxe_project *p, pdxe_import_edge_fn fn, void *userdata) {
    if (!p || !fn || !p->ran) {
        return;
    }
    static const char key[] = "\"local_name\":\"";
    for (int i = 0; i < p->n_files; i++) {
        char *file_qn = pdxe_pipeline_fqn_compute(PROJECT_SENTINEL, p->files[i].rel_path, "__file__");
        const pdxe_gbuf_node_t *src = file_qn ? pdxe_gbuf_find_by_qn(p->gbuf, file_qn) : NULL;
        free(file_qn);
        const pdxe_gbuf_edge_t **edges = NULL;
        int n = 0;
        if (!src || pdxe_gbuf_find_edges_by_source_type(p->gbuf, src->id, "IMPORTS", &edges, &n) != 0) {
            continue;
        }
        for (int k = 0; k < n; k++) {
            const pdxe_gbuf_node_t *target = pdxe_gbuf_find_by_id(p->gbuf, edges[k]->target_id);
            const char *props = edges[k]->properties_json;
            const char *ln = props ? strstr(props, key) : NULL;
            char local[256] = "";
            if (ln) {
                ln += sizeof(key) - 1;
                const char *end = strchr(ln, '"');
                size_t len = end ? (size_t)(end - ln) : strlen(ln);
                if (len >= sizeof(local)) {
                    len = sizeof(local) - 1;
                }
                memcpy(local, ln, len);
                local[len] = '\0';
            }
            fn(p->files[i].rel_path, local, target ? strip_project(target->qualified_name) : NULL,
               target ? target->label : NULL, userdata);
        }
    }
}

void pdxe_project_debug_lsp_surfaces(struct pdxe_project *p, pdxe_lsp_surface_fn fn,
                                     void *userdata) {
    if (!p || !fn || !p->ran || p->n_files <= 0) {
        return;
    }
    char **modules = (char **)calloc((size_t)p->n_files, sizeof(char *));
    int *starts = (int *)calloc((size_t)p->n_files + 1, sizeof(int));
    PDXEArena arena;
    pdxe_arena_init(&arena);
    if (modules && starts) {
        int count = 0;
        PDXELSPDef *defs = pdxe_pxc_collect_all_defs(&p->pctx, &arena, p->cache, p->files,
                                                    p->n_files, PROJECT_SENTINEL, modules,
                                                    &count, starts);
        pdxe_lsp_surface_row_t *rows = NULL;
        int n = 0;
        /* As the pass does: no definitions anywhere, no rows at all. */
        if (defs && pdxe_lsp_surface_build_rows(&p->pctx, PROJECT_SENTINEL, p->cache, p->files,
                                                p->n_files, defs, starts, &rows, &n) == 0) {
            for (int i = 0; i < n; i++) {
                fn(rows[i].rel_path, rows[i].defs_json, userdata);
            }
            pdxe_store_free_lsp_surfaces(rows, n);
        }
        free(defs);
    }
    for (int i = 0; modules && i < p->n_files; i++) {
        free(modules[i]);
    }
    free(modules);
    free(starts);
    pdxe_arena_destroy(&arena);
}
