/* rust_lsp.c — Type-aware call resolution for Rust source files.
 *
 * Mirrors the structure of `go_lsp.c` and reverse-engineers the relevant
 * pieces of `rust-analyzer` (`hir-def/resolver.rs`,
 * `hir-ty/method_resolution.rs`, `hir-ty/infer.rs`) into a per-file walk
 * driven by tree-sitter-rust.
 *
 * The compilation unit is split into clearly-labelled sections:
 *
 *   1.  Init + helpers            (~150 lines)
 *   2.  Builtin / prelude tables  (~100 lines)
 *   3.  Path & use resolution     (~250 lines)
 *   4.  Type-AST → PDXEType        (~250 lines)
 *   5.  Generic substitution      (~150 lines)
 *   6.  Expression evaluator      (~700 lines)
 *   7.  Method dispatch           (~400 lines)
 *   8.  Macro handling            (~200 lines)
 *   9.  Statement / pattern bind  (~400 lines)
 *   10. Function & file walk      (~250 lines)
 *   11. Per-file entry            (~250 lines)
 *   12. Cross-file + batch        (~250 lines)
 *
 * Total ~3300 lines, matching the depth of go_lsp.c (2750) and py_lsp.c
 * (3188). The code is structured so each section has a single coherent
 * responsibility — there are no surprise back-edges between sections.
 */

#include "rust_lsp.h"
#include "../lookup_counts.h"
#include "rust_cargo.h"
#include "../helpers.h"
#include <ctype.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* ════════════════════════════════════════════════════════════════════
 * 1. Initialisation + arena helpers
 * ════════════════════════════════════════════════════════════════════ */

/* Forward declarations for early callers in the file. */
static void rust_resolve_calls_in_node(RustLSPContext *ctx, TSNode node);
static int rust_use_scope_of(RustLSPContext *ctx, TSNode node);
static void rust_emit_resolved_call(RustLSPContext *ctx, const char *callee_qn,
                                    const char *strategy, float confidence);
static void rust_inject_syn_call(RustLSPContext *ctx, const char *callee_qn);
static bool rust_map_source_range(const RustLSPContext *ctx, uint32_t start, uint32_t end,
                                  uint32_t *mapped_start, uint32_t *mapped_end);
static void rust_emit_unresolved_call(RustLSPContext *ctx, const char *expr_text,
                                      const char *reason);
static const PDXEType *rust_lookup_field(RustLSPContext *ctx, const char *type_qn,
                                        const char *field_name, int depth);
static const PDXERegisteredFunc *rust_lookup_method_in_trait(RustLSPContext *ctx,
                                                            const char *trait_qn,
                                                            const char *method_name,
                                                            bool allow_abstract);
static char *rust_node_text(RustLSPContext *ctx, TSNode node);
static const char *convert_path_to_qn(PDXEArena *arena, const char *path);
static bool rust_type_derefs_to_first_arg(const char *type_qn);
static const char *rust_lookup_type_param_bound(RustLSPContext *ctx, const char *name);
static void rust_collect_bounds_from_text(RustLSPContext *ctx, const char *text);
static void rust_record_type_param_bound(RustLSPContext *ctx, const char *param_name,
                                         const char *trait_qn);
static const char *rust_path_identity(RustLSPContext *ctx, const char *path);
static const char *rmi_type_in_module(PDXEArena *arena, const char *module_qn, const char *text);
static const char *rust_scope_binding(RustLSPContext *ctx, struct RustUseScope *sc,
                                      const char *local_name, bool *withheld);
static RustUseScope *rust_file_scope(RustLSPContext *ctx);
static void rust_declare_uses(RustLSPContext *ctx, TSNode items, int s);
static int rust_visible_globs(RustLSPContext *ctx, int upto, const char ***out);
static int rust_split_path(PDXEArena *arena, const char *path, char ***out);
static const char *rust_resolve_use_ranked(RustLSPContext *ctx, const char *local_name,
                                           int *rank);

void rust_lsp_init(RustLSPContext *ctx, PDXEArena *arena, const char *source, int source_len,
                   const PDXETypeRegistry *registry, const char *module_qn,
                   PDXEResolvedCallArray *out) {
    memset(ctx, 0, sizeof(RustLSPContext));
    ctx->arena = arena;
    ctx->source = source;
    ctx->source_len = source_len;
    ctx->root_source = source;
    ctx->registry = registry;
    ctx->module_qn = module_qn;
    ctx->resolved_calls = out;
    ctx->current_scope = pdxe_scope_push(arena, NULL);

    const char *dbg = getenv("PDX_ENGINE_LSP_DEBUG");
    ctx->debug = (dbg && dbg[0]);
}

/* Issue 114: a new scope, numbered after every scope before it in source order.
 * -1 when the table cannot grow (lost work, counted where the allocation failed). */
static int rust_use_scope_new(RustLSPContext *ctx, uint32_t start_byte, uint32_t end_byte,
                              int parent, bool module, int module_depth) {
    if (ctx->use_scope_count >= ctx->use_scope_cap) {
        int cap = ctx->use_scope_cap ? ctx->use_scope_cap * 2 : 8;
        RustUseScope *grown =
            (RustUseScope *)pdxe_arena_alloc(ctx->arena, (size_t)cap * sizeof(RustUseScope));
        if (!grown) {
            return -1;
        }
        if (ctx->use_scope_count > 0) {
            memcpy(grown, ctx->use_scopes, (size_t)ctx->use_scope_count * sizeof(RustUseScope));
        }
        ctx->use_scopes = grown;
        ctx->use_scope_cap = cap;
    }
    RustUseScope *sc = &ctx->use_scopes[ctx->use_scope_count];
    memset(sc, 0, sizeof(*sc));
    sc->start_byte = start_byte;
    sc->end_byte = end_byte;
    sc->parent = parent;
    sc->module = module;
    sc->module_depth = module_depth;
    return ctx->use_scope_count++;
}

/* The file's scope, made when a caller adds to it before the use trees are read. */
static RustUseScope *rust_file_scope(RustLSPContext *ctx) {
    if (ctx->use_scope_count == 0 &&
        rust_use_scope_new(ctx, 0, ctx->source_len > 0 ? (uint32_t)ctx->source_len : 0, -1, true,
                           0) < 0) {
        return NULL;
    }
    return &ctx->use_scopes[0];
}

/* Doubling-array push of a `(local, full-path)` entry into one scope (issue 92:
 * growing by a fixed step copied the entries O(uses^2) times over). */
static void rust_scope_add_use(RustLSPContext *ctx, RustUseScope *sc, const char *local_name,
                               const char *module_path) {
    if (!sc || !local_name || !module_path) {
        return;
    }
    if (sc->count >= sc->cap) {
        int new_cap = sc->cap ? sc->cap * 2 : 16;
        const char **nl =
            (const char **)pdxe_arena_alloc(ctx->arena, (new_cap + 1) * sizeof(char *));
        const char **np =
            (const char **)pdxe_arena_alloc(ctx->arena, (new_cap + 1) * sizeof(char *));
        if (!nl || !np) {
            return;
        }
        if (sc->count > 0) {
            memcpy(nl, sc->names, sc->count * sizeof(char *));
            memcpy(np, sc->paths, sc->count * sizeof(char *));
        }
        sc->names = nl;
        sc->paths = np;
        sc->cap = new_cap;
    }
    sc->names[sc->count] = pdxe_arena_strdup(ctx->arena, local_name);
    sc->paths[sc->count] = pdxe_arena_strdup(ctx->arena, module_path);
    sc->count++;
}

static void rust_scope_add_glob(RustLSPContext *ctx, RustUseScope *sc, const char *module_qn) {
    if (!sc || !module_qn) {
        return;
    }
    if (sc->glob_count >= sc->glob_cap) {
        int new_cap = sc->glob_cap ? sc->glob_cap * 2 : 4;
        const char **ng =
            (const char **)pdxe_arena_alloc(ctx->arena, (new_cap + 1) * sizeof(char *));
        if (!ng) {
            return;
        }
        if (sc->glob_count > 0) {
            memcpy(ng, sc->globs, sc->glob_count * sizeof(char *));
        }
        sc->globs = ng;
        sc->glob_cap = new_cap;
    }
    sc->globs[sc->glob_count++] = pdxe_arena_strdup(ctx->arena, module_qn);
}

static void rust_scope_add_link(RustLSPContext *ctx, RustUseScope *sc, int target) {
    if (!sc || target < 0) {
        return;
    }
    if (sc->link_count >= sc->link_cap) {
        int new_cap = sc->link_cap ? sc->link_cap * 2 : 2;
        int *nl = (int *)pdxe_arena_alloc(ctx->arena, (size_t)new_cap * sizeof(int));
        if (!nl) {
            return;
        }
        if (sc->link_count > 0) {
            memcpy(nl, sc->links, (size_t)sc->link_count * sizeof(int));
        }
        sc->links = nl;
        sc->link_cap = new_cap;
    }
    sc->links[sc->link_count++] = target;
}

/* A caller's import of the file (issue 111's bridged entries): in the file's scope,
 * after its own use trees, seen where the file's module's names are. */
void rust_lsp_add_use(RustLSPContext *ctx, const char *local_name, const char *module_path) {
    if (!ctx || !local_name || !module_path) {
        return;
    }
    rust_scope_add_use(ctx, rust_file_scope(ctx), local_name, module_path);
}

void rust_lsp_add_glob(RustLSPContext *ctx, const char *module_qn) {
    if (!ctx || !module_qn) {
        return;
    }
    rust_scope_add_glob(ctx, rust_file_scope(ctx), module_qn);
}

static char *rust_node_text(RustLSPContext *ctx, TSNode node) {
    return pdxe_node_text(ctx->arena, node, ctx->source);
}

/* ════════════════════════════════════════════════════════════════════
 * 2. Builtin / prelude tables
 * ════════════════════════════════════════════════════════════════════ */

/* Rust primitive types that the grammar reports as `primitive_type`. */
static const char *RUST_PRIMITIVES[] = {"i8",   "i16",  "i32", "i64",  "i128",  "isize", "u8",
                                        "u16",  "u32",  "u64", "u128", "usize", "f32",   "f64",
                                        "bool", "char", "str", "()",   "!",     NULL};

static bool is_rust_primitive(const char *name) {
    if (!name) {
        return false;
    }
    for (const char **p = RUST_PRIMITIVES; *p; p++) {
        if (strcmp(*p, name) == 0) {
            return true;
        }
    }
    return false;
}

/* Names of macros that behave like println-family: side effects only,
 * return type `()`. */
static bool is_void_macro(const char *name) {
    if (!name) {
        return false;
    }
    static const char *m[] = {"println",
                              "print",
                              "eprintln",
                              "eprint",
                              "panic",
                              "unimplemented",
                              "todo",
                              "unreachable",
                              "assert",
                              "assert_eq",
                              "assert_ne",
                              "debug_assert",
                              "debug_assert_eq",
                              "debug_assert_ne",
                              "writeln",
                              "write",
                              NULL};
    for (const char **p = m; *p; p++) {
        if (strcmp(*p, name) == 0) {
            return true;
        }
    }
    return false;
}

/* Names of macros that produce a `String` value. */
static bool is_string_macro(const char *name) {
    if (!name) {
        return false;
    }
    return strcmp(name, "format") == 0 || strcmp(name, "concat") == 0 ||
           strcmp(name, "stringify") == 0 || strcmp(name, "env") == 0 ||
           strcmp(name, "include_str") == 0;
}

/* Prelude trait names whose method short-names we treat as universally
 * available (for emit-on-best-effort when we cannot pin down the trait
 * impl). Borrowed from `core::prelude::v1`. */
static bool is_prelude_trait_method(const char *method_name) {
    if (!method_name) {
        return false;
    }
    static const char *m[] = {/* Clone / Copy / Default */
                              "clone", "default",
                              /* PartialEq / Eq / PartialOrd / Ord */
                              "eq", "ne", "cmp", "partial_cmp", "lt", "le", "gt", "ge",
                              /* Hash */
                              "hash",
                              /* Display / Debug */
                              "fmt", "to_string",
                              /* From / Into / TryFrom / TryInto */
                              "from", "into", "try_from", "try_into",
                              /* AsRef / AsMut / Borrow / BorrowMut */
                              "as_ref", "as_mut", "borrow", "borrow_mut",
                              /* Deref */
                              "deref", "deref_mut",
                              /* Drop */
                              "drop",
                              /* Iterator (most-used subset) */
                              "next", "iter", "iter_mut", "into_iter", "map", "filter", "fold",
                              "for_each", "collect", "count", "sum", "max", "min", "any", "all",
                              "find", "position", "enumerate", "zip", "chain", "take", "skip",
                              "rev", "cloned", "copied", "by_ref", "step_by", "flat_map", "flatten",
                              "filter_map", "peekable",
                              /* Future */
                              "poll", NULL};
    for (const char **p = m; *p; p++) {
        if (strcmp(*p, method_name) == 0) {
            return true;
        }
    }
    return false;
}

/* ════════════════════════════════════════════════════════════════════
 * 3. Path & use resolution
 * ════════════════════════════════════════════════════════════════════ */

/* Return the last `::`-separated segment of a Rust path (`std::io::Read` →
 * `Read`). Pointer aliases into `path` — caller does not own. */
static const char *path_last_segment(const char *path) {
    if (!path || !path[0]) {
        return path;
    }
    const char *p = path;
    const char *last = path;
    while (*p) {
        if (p[0] == ':' && p[1] == ':') {
            last = p + 2;
            p += 2;
            continue;
        }
        p++;
    }
    return last;
}

/* Convert a Rust path with `::` separators into our internal QN form using
 * `.` separators. Always allocates a fresh string. */
static const char *convert_path_to_qn(PDXEArena *arena, const char *path) {
    if (!path || !path[0]) {
        return path;
    }
    size_t len = strlen(path);
    char *out = (char *)pdxe_arena_alloc(arena, len + 1);
    if (!out) {
        return path;
    }
    size_t j = 0;
    for (size_t i = 0; i < len; i++) {
        if (path[i] == ':' && i + 1 < len && path[i + 1] == ':') {
            out[j++] = '.';
            i++;
        } else {
            out[j++] = path[i];
        }
    }
    out[j] = '\0';
    return out;
}

/* In-place strip turbofish segments (`::<...>`) from a Rust path. The
 * grammar exposes paths like `Vec::<i32>::new` or `parse::<u32>(s)` —
 * the LSP cares about the underlying name, not the explicit type
 * arguments, so we collapse `head::<args>::tail` to `head::tail`.
 *
 * Modifies `path` in place. Safe on NULL. */
static void rust_strip_turbofish(char *path) {
    if (!path)
        return;
    char *read = path;
    char *write = path;
    while (*read) {
        if (read[0] == ':' && read[1] == ':' && read[2] == '<') {
            /* Skip ::< … > balanced. */
            int depth = 1;
            const char *p = read + 3;
            while (*p && depth > 0) {
                if (*p == '<')
                    depth++;
                else if (*p == '>')
                    depth--;
                p++;
            }
            read = (char *)p;
            continue;
        }
        *write++ = *read++;
    }
    *write = '\0';
}

/* What one scope's `use` declarations bind `local_name` to: the path, or NULL when
 * they bind nothing of that name. `*withheld` when its own use trees bind the name to
 * two different paths (issue 111): that is no import at all, and no other scope's
 * binding is taken instead.
 *
 * Found through the scope's index by local name (issue 92), in the order the
 * entries were added: its own use trees, in source order, then (in the file's scope)
 * the imports a caller bridged in, which come after them. When the index cannot grow
 * nothing is found (lost work, counted where the allocation failed), never a scan. */
static const char *rust_scope_binding(RustLSPContext *ctx, RustUseScope *sc,
                                      const char *local_name, bool *withheld) {
    bool scan = false;
#if defined(PDXE_ENABLE_TEST_SEAMS)
    const char *reference = getenv("PDX_ENGINE_TEST_LINEAR_LOOKUP_REFERENCE");
    scan = reference && reference[0];
#endif
    PDXENameIndex *by_name = &sc->by_name;
    while (!scan && !by_name->failed && by_name->covered < sc->count) {
        pdxe_name_index_place(by_name, ctx->arena, sc->names[by_name->covered]);
    }
    const char *found = NULL;
    for (int i = scan ? 0 : pdxe_name_index_first(by_name, local_name);
         scan ? i < sc->count : i >= 0; i = scan ? i + 1 : pdxe_name_index_next(by_name, i)) {
        PDXE_LOOKUP_COUNT(PDXE_LC_IMPORT_ENTRIES);
        if (strcmp(sc->names[i], local_name) != 0) {
            continue;
        }
        if (i >= sc->tree_count) {
            /* A bridged import: what the use trees bind comes first. */
            return found ? found : sc->paths[i];
        }
        if (!found) {
            found = sc->paths[i];
        } else if (strcmp(found, sc->paths[i]) != 0) {
            *withheld = true; /* bound to two paths */
            return NULL;
        }
    }
    return found;
}

/* What `local_name` is bound to in scope `s` and the enclosing modules its globs
 * name (`use super::*`), in that order. */
static const char *rust_scope_lookup(RustLSPContext *ctx, int s, const char *local_name,
                                     bool *withheld) {
    RustUseScope *sc = &ctx->use_scopes[s];
    const char *found = rust_scope_binding(ctx, sc, local_name, withheld);
    for (int l = 0; !found && !*withheld && l < sc->link_count; l++) {
        /* A link names an enclosing module, so following them ends. */
        if (sc->links[l] < s) {
            found = rust_scope_lookup(ctx, sc->links[l], local_name, withheld);
        }
    }
    return found;
}

/* Issue 114: look up a `use` alias where the walk is and return its whole path, or
 * NULL if no `use` in force there binds it. The returned pointer aliases into the use
 * map. The scopes are read from the innermost out: the first that binds the name
 * decides, and the first module ends the search (Rust's modules do not see their
 * parent's names). A name a scope binds to two paths is withheld. `*rank` is the
 * deciding scope's distance from the innermost (0), -1 when none decides. */
static const char *rust_resolve_use_ranked(RustLSPContext *ctx, const char *local_name,
                                           int *rank) {
    *rank = -1;
    if (!ctx || !local_name || ctx->use_scope_count == 0) {
        return NULL;
    }
    int s = ctx->use_scope >= 0 && ctx->use_scope < ctx->use_scope_count ? ctx->use_scope : 0;
    for (int r = 0; s >= 0; r++) {
        bool withheld = false;
        const char *found = rust_scope_lookup(ctx, s, local_name, &withheld);
        if (found || withheld) {
            *rank = r;
            return found;
        }
        if (ctx->use_scopes[s].module) {
            break;
        }
        s = ctx->use_scopes[s].parent;
    }
    return NULL;
}

static const char *rust_resolve_use(RustLSPContext *ctx, const char *local_name) {
    int rank;
    return rust_resolve_use_ranked(ctx, local_name, &rank);
}

/* The Rust prelude is auto-imported into every module. We map each name
 * to its canonical QN so bare references (`String`, `Vec::new`, …)
 * resolve without an explicit `use`. The list mirrors `core::prelude::v1`
 * + `alloc::prelude` + `std::prelude::v1`. The mapping is consulted before
 * the project-local fallback so prelude names always win. */
typedef struct {
    const char *name;
    const char *qn;
} RustPreludeEntry;

static const RustPreludeEntry RUST_PRELUDE[] = {{"String", "alloc.string.String"},
                                                {"ToString", "alloc.string.ToString"},
                                                {"Vec", "alloc.vec.Vec"},
                                                {"VecDeque", "alloc.collections.VecDeque"},
                                                {"HashMap", "alloc.collections.HashMap"},
                                                {"BTreeMap", "alloc.collections.BTreeMap"},
                                                {"HashSet", "alloc.collections.HashSet"},
                                                {"BTreeSet", "alloc.collections.BTreeSet"},
                                                {"Box", "alloc.boxed.Box"},
                                                {"Rc", "alloc.rc.Rc"},
                                                {"Arc", "alloc.sync.Arc"},
                                                {"Option", "core.option.Option"},
                                                {"Some", "core.option.Option.Some"},
                                                {"None", "core.option.Option.None"},
                                                {"Result", "core.result.Result"},
                                                {"Ok", "core.result.Result.Ok"},
                                                {"Err", "core.result.Result.Err"},
                                                {"Iterator", "core.iter.Iterator"},
                                                {"IntoIterator", "core.iter.IntoIterator"},
                                                {"Future", "core.future.Future"},
                                                {"Clone", "core.clone.Clone"},
                                                {"Copy", "core.marker.Copy"},
                                                {"Send", "core.marker.Send"},
                                                {"Sync", "core.marker.Sync"},
                                                {"Default", "core.default.Default"},
                                                {"PartialEq", "core.cmp.PartialEq"},
                                                {"Eq", "core.cmp.Eq"},
                                                {"PartialOrd", "core.cmp.PartialOrd"},
                                                {"Ord", "core.cmp.Ord"},
                                                {"Hash", "core.hash.Hash"},
                                                {"Display", "core.fmt.Display"},
                                                {"Debug", "core.fmt.Debug"},
                                                {"From", "core.convert.From"},
                                                {"Into", "core.convert.Into"},
                                                {"TryFrom", "core.convert.TryFrom"},
                                                {"TryInto", "core.convert.TryInto"},
                                                {"AsRef", "core.convert.AsRef"},
                                                {"AsMut", "core.convert.AsMut"},
                                                {"Borrow", "core.borrow.Borrow"},
                                                {"BorrowMut", "core.borrow.BorrowMut"},
                                                {"Deref", "core.ops.Deref"},
                                                {"DerefMut", "core.ops.DerefMut"},
                                                {"Drop", "core.ops.Drop"},
                                                {"RefCell", "core.cell.RefCell"},
                                                {"Cell", "core.cell.Cell"},
                                                {"Mutex", "std.sync.Mutex"},
                                                {"RwLock", "std.sync.RwLock"},
                                                {NULL, NULL}};

static const char *rust_lookup_prelude(const char *name) {
    if (!name)
        return NULL;
    for (const RustPreludeEntry *e = RUST_PRELUDE; e->name; e++) {
        if (strcmp(e->name, name) == 0)
            return e->qn;
    }
    return NULL;
}

/* Canonical trait-name index used while registries are still under
 * construction.  Every suffix of an interface QN is indexed, but a suffix is
 * usable only if all registrations with that spelling name the same QN.  This
 * makes raw cross-file paths (`Pinger`, `contract::Pinger`) O(1) to resolve
 * without the old leaf-first guess or an impls x types scan. */
typedef struct {
    PDXEIdxMemo unique;
    PDXEIdxMemo ambiguous;
} RustTraitNameIndex;

static void rust_trait_name_index_add(RustTraitNameIndex *index, PDXEArena *arena,
                                      const PDXETypeRegistry *reg, const char *key, int type_index) {
    if (!index || !arena || !reg || !key || !key[0] || type_index < 0 ||
        type_index >= reg->type_count) {
        return;
    }
    int32_t previous = pdxe_idxmemo_get(&index->unique, key);
    if (previous < 0) {
        pdxe_idxmemo_put_if_absent(&index->unique, arena, key, type_index);
        return;
    }
    const char *previous_qn = reg->types[previous].qualified_name;
    const char *current_qn = reg->types[type_index].qualified_name;
    if (previous_qn && current_qn && strcmp(previous_qn, current_qn) != 0) {
        pdxe_idxmemo_put_if_absent(&index->ambiguous, arena, key, 1);
    }
}

static void rust_trait_name_index_build(RustTraitNameIndex *index, PDXEArena *arena,
                                        const PDXETypeRegistry *reg) {
    memset(index, 0, sizeof(*index));
    if (!arena || !reg) {
        return;
    }
    for (int i = 0; i < reg->type_count; i++) {
        const PDXERegisteredType *type = &reg->types[i];
        if (!type->is_interface || !type->qualified_name) {
            continue;
        }
        const char *suffix = type->qualified_name;
        for (;;) {
            rust_trait_name_index_add(index, arena, reg, suffix, i);
            const char *dot = strchr(suffix, '.');
            if (!dot || !dot[1]) {
                break;
            }
            suffix = dot + 1;
        }
    }
}

static const char *rust_trait_name_index_lookup(const RustTraitNameIndex *index,
                                                const PDXETypeRegistry *reg, const char *key) {
    if (!index || !reg || !key || pdxe_idxmemo_get(&index->ambiguous, key) >= 0) {
        return NULL;
    }
    int32_t type_index = pdxe_idxmemo_get(&index->unique, key);
    if (type_index < 0 || type_index >= reg->type_count) {
        return NULL;
    }
    return reg->types[type_index].qualified_name;
}

static const char *rust_canonical_impl_trait(PDXEArena *arena, const PDXETypeRegistry *reg,
                                             const RustTraitNameIndex *index, const char *raw_trait,
                                             const char *def_module_qn) {
    if (!arena || !reg || !index || !raw_trait || !raw_trait[0]) {
        return NULL;
    }
    while (isspace((unsigned char)*raw_trait)) {
        raw_trait++;
    }
    if (strncmp(raw_trait, "dyn ", 4) == 0) {
        raw_trait += 4;
    } else if (strncmp(raw_trait, "impl ", 5) == 0) {
        raw_trait += 5;
    }
    const char *normalized = convert_path_to_qn(arena, raw_trait);
    if (!normalized || !normalized[0]) {
        return NULL;
    }

    /* A bare/relative trait declared in the impl's own module is exact and
     * therefore wins even if another module declares the same leaf. */
    const char *relative = normalized;
    if (strncmp(relative, "self.", 5) == 0) {
        relative += 5;
    }
    if (def_module_qn && relative[0] && strncmp(relative, "crate.", 6) != 0 &&
        strncmp(relative, "super.", 6) != 0) {
        const char *local = pdxe_arena_sprintf(arena, "%s.%s", def_module_qn, relative);
        const char *local_qn = rust_trait_name_index_lookup(index, reg, local);
        if (local_qn) {
            return local_qn;
        }
    }

    const char *exact = rust_trait_name_index_lookup(index, reg, normalized);
    if (exact) {
        return exact;
    }

    /* Prelude traits have canonical external QNs.  This remains fail-closed:
     * the canonical QN must actually be registered in this registry. */
    if (!strchr(normalized, '.')) {
        const char *prelude = rust_lookup_prelude(normalized);
        const char *prelude_qn = rust_trait_name_index_lookup(index, reg, prelude);
        if (prelude_qn) {
            return prelude_qn;
        }
    }

    /* `crate::x::Trait` has no literal `crate` segment in extracted QNs.
     * Resolve its semantic tail only when globally unique. */
    const char *suffix = normalized;
    if (strncmp(suffix, "crate.", 6) == 0) {
        suffix += 6;
    } else {
        while (strncmp(suffix, "super.", 6) == 0) {
            suffix += 6;
        }
    }
    return rust_trait_name_index_lookup(index, reg, suffix);
}

static void rust_registered_type_add_embedded(PDXEArena *arena, PDXERegisteredType *type,
                                              const char *embedded_qn) {
    if (!arena || !type || !embedded_qn || !embedded_qn[0]) {
        return;
    }
    int count = 0;
    if (type->embedded_types) {
        while (type->embedded_types[count]) {
            if (strcmp(type->embedded_types[count], embedded_qn) == 0) {
                return;
            }
            count++;
        }
    }
    const char **items =
        (const char **)pdxe_arena_alloc(arena, (size_t)(count + 2) * sizeof(*items));
    if (!items) {
        return;
    }
    for (int i = 0; i < count; i++) {
        items[i] = type->embedded_types[i];
    }
    items[count] = embedded_qn;
    items[count + 1] = NULL;
    type->embedded_types = items;
}

/* Issue 109: the index of `name` among the first `count` parameters, or -1. */
static int rust_impl_param_index(const char *const *params, int count, const char *name) {
    for (int i = 0; i < count; i++) {
        if (strcmp(params[i], name) == 0) {
            return i;
        }
    }
    return -1;
}

/* Issue 109: a trait impl's header (PDXEImplTrait's impl_bounds), read once at
 * registration. `trait_qn` is the impl's own trait, already canonical; each bound's
 * trait is made canonical as an impl's trait is, from the module declaring the impl,
 * so a bound and an impl that name one trait name the same QN. NULL with no header,
 * or when the reading cannot be made whole (counted where the allocation failed): an
 * impl without facts is never proven to apply. */
static const PDXERustImplFacts *rust_impl_facts_read(PDXEArena *arena, const PDXETypeRegistry *reg,
                                                     const RustTraitNameIndex *traits,
                                                     const char *const *entries,
                                                     const char *module_qn, const char *trait_qn) {
    if (!arena || !entries || !entries[0]) {
        return NULL;
    }
    int n_params = 0, n_self = 0, n_trait = 0, n_bounds = 0, n_args = 0;
    for (int i = 0; entries[i]; i++) {
        switch (entries[i][0]) {
        case 'p':
            n_params++;
            break;
        case 'a':
            n_self++;
            break;
        case 't':
            n_trait++;
            break;
        case 'b':
        case 'r':
        case 'l':
        case 'o':
            n_bounds++;
            break;
        case 'g':
            n_args++;
            break;
        default:
            break;
        }
    }
    PDXERustImplFacts *f = (PDXERustImplFacts *)pdxe_arena_alloc(arena, sizeof(*f));
    if (!f) {
        return NULL;
    }
    memset(f, 0, sizeof(*f));
    f->params = (const char **)pdxe_arena_alloc(arena, (size_t)(n_params + 1) * sizeof(char *));
    f->self_args = (int *)pdxe_arena_alloc(arena, (size_t)(n_self + 1) * sizeof(int));
    f->trait_args = (const char **)pdxe_arena_alloc(arena, (size_t)(n_trait + 1) * sizeof(char *));
    f->bounds = (PDXERustBound *)pdxe_arena_alloc(arena, (size_t)(n_bounds + 1) * sizeof(PDXERustBound));
    const char **args = (const char **)pdxe_arena_alloc(arena, (size_t)(n_args + 1) * sizeof(char *));
    if (!f->params || !f->self_args || !f->trait_args || !f->bounds || !args) {
        return NULL;
    }
    f->trait_qn = trait_qn;
    f->self_form = 'o';
    int n_used_args = 0;
    int subject = -1;
    bool in_predicate = false;
    PDXERustBound *last_trait = NULL;
    for (int i = 0; entries[i]; i++) {
        const char *e = entries[i];
        if (!e[0] || e[1] != ':') {
            f->opaque = true;
            continue;
        }
        const char *text = e + 2;
        PDXERustBound *bound = NULL;
        switch (e[0]) {
        case 's':
            if (i != 0) {
                f->opaque = true;
            } else if (strcmp(text, "plain") == 0) {
                f->self_form = 'p';
            } else if (strcmp(text, "blanket") == 0) {
                f->self_form = 'b';
            } else if (strcmp(text, "other") != 0) {
                f->opaque = true;
            }
            break;
        case 'p':
            f->params[f->param_count++] = text;
            break;
        case 'a': {
            int index = rust_impl_param_index(f->params, f->param_count, text);
            if (index < 0) {
                f->opaque = true;
            } else {
                f->self_args[f->self_arg_count++] = index;
            }
            break;
        }
        case 't':
            f->trait_args[f->trait_arg_count++] = text;
            break;
        case 'w':
            subject = rust_impl_param_index(f->params, f->param_count, text);
            in_predicate = true;
            last_trait = NULL;
            break;
        case 'b':
        case 'r':
        case 'l':
        case 'o':
            if (!in_predicate) {
                f->opaque = true;
                break;
            }
            bound = &f->bounds[f->bound_count++];
            memset(bound, 0, sizeof(*bound));
            bound->subject = subject;
            bound->kind = e[0] == 'b'   ? PDXE_RUST_BOUND_TRAIT
                          : e[0] == 'r' ? PDXE_RUST_BOUND_RELAXED
                          : e[0] == 'l' ? PDXE_RUST_BOUND_LIFETIME
                                        : PDXE_RUST_BOUND_OTHER;
            last_trait = NULL;
            if (bound->kind == PDXE_RUST_BOUND_TRAIT) {
                bound->trait_qn = rust_canonical_impl_trait(arena, reg, traits, text, module_qn);
                bound->args = args + n_used_args;
                last_trait = bound;
            }
            break;
        case 'g':
            if (!last_trait) {
                f->opaque = true;
                break;
            }
            args[n_used_args++] = text;
            last_trait->arg_count++;
            break;
        default: /* x:, or a kind no grammar has */
            f->opaque = true;
            break;
        }
    }
    return f;
}

/* Issue 109: one more `impl Trait for Type` relation of `type`, with its header. */
static void rust_registered_type_add_impl(PDXEArena *arena, PDXERegisteredType *type,
                                          const PDXERustImplFacts *facts) {
    if (!arena || !type || !facts) {
        return;
    }
    if (type->rust_impl_count == type->rust_impl_cap) {
        int cap = type->rust_impl_cap ? type->rust_impl_cap * 2 : 4;
        const PDXERustImplFacts **grown = (const PDXERustImplFacts **)pdxe_arena_alloc(
            arena, (size_t)cap * sizeof(*grown));
        if (!grown) {
            return;
        }
        for (int i = 0; i < type->rust_impl_count; i++) {
            grown[i] = type->rust_impls[i];
        }
        type->rust_impls = grown;
        type->rust_impl_cap = cap;
    }
    type->rust_impls[type->rust_impl_count++] = facts;
}

/* Strip a leading `&` / `&mut` reference prefix from a textual type so we
 * can compare the inner head segment against builtins. */
static const char *skip_ref_prefix(const char *text) {
    if (!text) {
        return text;
    }
    while (*text == '&' || isspace((unsigned char)*text)) {
        text++;
    }
    if (strncmp(text, "mut ", 4) == 0) {
        text += 4;
        while (isspace((unsigned char)*text)) {
            text++;
        }
    }
    /* Also strip a single explicit lifetime: `'a ` */
    if (*text == '\'') {
        text++;
        while (*text && (isalnum((unsigned char)*text) || *text == '_')) {
            text++;
        }
        while (isspace((unsigned char)*text)) {
            text++;
        }
    }
    return text;
}

/* Resolve an otherwise-relative multi-segment type only when its head is an
 * explicit top-level `mod head;` declaration and exactly one graph-qualified
 * candidate exists. Rust filename-stem QNs do not encode whether a source is
 * a crate root, so both the child and sibling layouts are tested and any
 * ambiguity stays unresolved. No short-name/suffix guess is admitted. */
static const char *rust_registered_relative_path(RustLSPContext *ctx, const char *normalized) {
    if (!ctx || !ctx->arena || !ctx->registry || !ctx->module_qn || !normalized || !normalized[0]) {
        return NULL;
    }
    const char *head_end = strchr(normalized, '.');
    if (!head_end || head_end == normalized) {
        return NULL;
    }
    char *head = pdxe_arena_strndup(ctx->arena, normalized, (size_t)(head_end - normalized));
    if (!head || pdxe_idxmemo_get(&ctx->declared_modules, head) < 0) {
        return NULL;
    }

    const char *current = pdxe_arena_sprintf(ctx->arena, "%s.%s", ctx->module_qn, normalized);
    const char *parent_end = strrchr(ctx->module_qn, '.');
    const char *parent =
        parent_end ? pdxe_arena_sprintf(ctx->arena, "%.*s.%s", (int)(parent_end - ctx->module_qn),
                                       ctx->module_qn, normalized)
                   : NULL;
    bool current_exists = current && pdxe_registry_lookup_type(ctx->registry, current);
    bool parent_exists = parent && pdxe_registry_lookup_type(ctx->registry, parent);
    if (current_exists == parent_exists) {
        return NULL;
    }
    return current_exists ? current : parent;
}

/* Resolve a Rust *path expression* (e.g. `Foo::bar` or `crate::x::y`)
 * into a canonical QN. The resolver cascades through these rules,
 * matching what `rust-analyzer`'s name resolver does at the path level:
 *
 *   1.  `Self::X` → `<self_type_qn>.X`
 *   2.  `crate::a::b` → `<root_module_qn>.a.b`
 *   3.  `super::a` → strip last segment of `module_qn` and prepend
 *   4.  Single-segment + matches a `use` local-name → `<full path>.X`
 *   5.  Multi-segment whose first segment is a `use` local → splice
 *   6.  Falls through unchanged (caller decides what to do).
 *
 * The returned string is arena-owned; in case (6) we return the input
 * with `::` already converted to `.`. */
static const char *rust_resolve_path_expr_text(RustLSPContext *ctx, const char *path) {
    if (!ctx || !path || !path[0]) {
        return path;
    }

    /* Self:: handling — we treat the receiver type's QN as the head. */
    if (strncmp(path, "Self::", 6) == 0 && ctx->self_type_qn) {
        return pdxe_arena_sprintf(ctx->arena, "%s.%s",
                                 ctx->self_path_qn ? ctx->self_path_qn : ctx->self_type_qn,
                                 convert_path_to_qn(ctx->arena, path + 6));
    }
    if (strcmp(path, "Self") == 0 && ctx->self_type_qn) {
        return ctx->self_type_qn;
    }

    /* crate:: → <root>. We approximate the crate root as the first dotted
     * segment of `module_qn` after the project prefix. The pipeline
     * forms `module_qn` as `<project>.<crate>.<rel-path-segments>`, so
     * the first two segments are project + crate root. */
    if (strncmp(path, "crate::", 7) == 0 && ctx->module_qn) {
        const char *p = ctx->module_qn;
        int dots = 0;
        const char *second_dot = NULL;
        for (; *p; p++) {
            if (*p == '.') {
                if (++dots == 2) {
                    second_dot = p;
                    break;
                }
            }
        }
        size_t crate_len =
            second_dot ? (size_t)(second_dot - ctx->module_qn) : strlen(ctx->module_qn);
        char *crate_buf = pdxe_arena_strndup(ctx->arena, ctx->module_qn, crate_len);
        return pdxe_arena_sprintf(ctx->arena, "%s.%s", crate_buf,
                                 convert_path_to_qn(ctx->arena, path + 7));
    }

    /* super:: → drop last segment of module_qn. */
    if (strncmp(path, "super::", 7) == 0 && ctx->module_qn) {
        const char *dot = strrchr(ctx->module_qn, '.');
        if (dot) {
            char *parent =
                pdxe_arena_strndup(ctx->arena, ctx->module_qn, (size_t)(dot - ctx->module_qn));
            return pdxe_arena_sprintf(ctx->arena, "%s.%s", parent,
                                     convert_path_to_qn(ctx->arena, path + 7));
        }
    }

    /* Find first "::" — split into head + tail. */
    const char *sep = strstr(path, "::");
    if (!sep) {
        const char *full = rust_resolve_use(ctx, path);
        if (full) {
            return convert_path_to_qn(ctx->arena, full);
        }
        /* Prelude name (e.g. `String`, `Vec`)? */
        const char *prelude = rust_lookup_prelude(path);
        if (prelude) {
            return prelude;
        }
        /* Bare identifier — assume same module. */
        if (ctx->module_qn) {
            return pdxe_arena_sprintf(ctx->arena, "%s.%s", ctx->module_qn, path);
        }
        return path;
    }

    char *head = pdxe_arena_strndup(ctx->arena, path, (size_t)(sep - path));
    const char *tail = sep + 2;

    const char *full = rust_resolve_use(ctx, head);
    if (full) {
        /* The use-map's full path already includes `head` as the last
         * segment; concat its parent with the rest. */
        const char *full_dotted = convert_path_to_qn(ctx->arena, full);
        const char *tail_dotted = convert_path_to_qn(ctx->arena, tail);
        return pdxe_arena_sprintf(ctx->arena, "%s.%s", full_dotted, tail_dotted);
    }

    /* Prelude head: `String::from` → `alloc.string.String.from`. */
    const char *prelude = rust_lookup_prelude(head);
    if (prelude) {
        return pdxe_arena_sprintf(ctx->arena, "%s.%s", prelude,
                                 convert_path_to_qn(ctx->arena, tail));
    }

    /* Cargo-manifest aware routing — when a Cargo.toml has been parsed
     * and the path head matches either a declared dependency or a
     * workspace member, return the canonical form `<head>.<tail>` so
     * the resolver doesn't pollute the module-prefix space. */
    if (ctx->cargo_manifest) {
        const PDXECargoManifest *m = (const PDXECargoManifest *)ctx->cargo_manifest;
        const PDXECargoMember *mem = pdxe_cargo_find_member(m, head);
        if (mem) {
            /* Workspace member: route to `<member_name>.<tail>` so the
             * pipeline's cross-crate resolution can match it. */
            return pdxe_arena_sprintf(ctx->arena, "%s.%s", head,
                                     convert_path_to_qn(ctx->arena, tail));
        }
        if (pdxe_cargo_is_known_dep(m, head)) {
            /* Declared dependency: same canonical form. The actual
             * methods may have been pre-seeded by rust_crates_seed.c;
             * otherwise the call is correctly attributed to an
             * external crate rather than fabricated locally. */
            return pdxe_arena_sprintf(ctx->arena, "%s.%s", head,
                                     convert_path_to_qn(ctx->arena, tail));
        }
    }

    /* Treat unknown-head paths as absolute: `std::io::Read` → `std.io.Read`.
     * Type parsing performs the narrower exact project-relative probe; doing
     * it here would reclassify `Type::method` as a direct free-function call
     * before the UFCS/constructor dispatch logic sees it. */
    return convert_path_to_qn(ctx->arena, path);
}

/* Whether `qn` names something registered: a type, a function, or a name of a
 * registered type (`Type.method`), which the caller looks up. */
static bool rust_qn_registered(RustLSPContext *ctx, const char *qn) {
    if (!qn || !ctx->registry) {
        return false;
    }
    if (pdxe_registry_lookup_type(ctx->registry, qn) || pdxe_registry_lookup_func(ctx->registry, qn)) {
        return true;
    }
    const char *dot = strrchr(qn, '.');
    if (!dot) {
        return false;
    }
    char *head = pdxe_arena_strndup(ctx->arena, qn, (size_t)(dot - qn));
    return head && pdxe_registry_lookup_type(ctx->registry, head) != NULL;
}

/* A path expression's qualified name: as its text reads (above), or, where that names
 * nothing registered, as Rust's name resolution reads it (issues 105 and 113), and then
 * `*via_identity` is set. */
static const char *rust_resolve_path_expr_ex(RustLSPContext *ctx, const char *path,
                                             bool *via_identity) {
    const char *qn = rust_resolve_path_expr_text(ctx, path);
    if (via_identity) {
        *via_identity = false;
    }
    if (!ctx || !path || !path[0] || rust_qn_registered(ctx, qn)) {
        return qn;
    }
    if (strcmp(path, "Self") == 0 || strncmp(path, "Self::", 6) == 0) {
        return qn;
    }
    const char *identity = rust_path_identity(ctx, path);
    if (identity && via_identity) {
        *via_identity = true;
    }
    return identity ? identity : qn;
}

static const char *rust_resolve_path_expr(RustLSPContext *ctx, const char *path) {
    return rust_resolve_path_expr_ex(ctx, path, NULL);
}

/* ════════════════════════════════════════════════════════════════════
 * 4. Type-AST → PDXEType
 * ════════════════════════════════════════════════════════════════════ */

/* Reconstruct the textual Rust path under a `scoped_type_identifier` /
 * `scoped_identifier` node. We deliberately walk the named children
 * rather than using the literal source text so we do not preserve
 * whitespace or trailing turbofish noise. */
static char *gather_scoped_path(RustLSPContext *ctx, TSNode node) {
    /* Fall back to the raw source text — the grammar already produces a
     * tight `path::to::name` literal under the node. */
    return rust_node_text(ctx, node);
}

/* Resolve a textual Rust path (with `::`) into a registered type's QN, or
 * NULL if no match. */
static const char *resolve_path_to_type_qn(RustLSPContext *ctx, const char *path) {
    if (!ctx || !path || !path[0]) {
        return NULL;
    }
    if (is_rust_primitive(path)) {
        return NULL;
    }
    const char *qn = rust_resolve_path_expr(ctx, path);
    if (!qn) {
        return NULL;
    }
    if (pdxe_registry_lookup_type(ctx->registry, qn)) {
        return qn;
    }
    const char *normalized = convert_path_to_qn(ctx->arena, path);
    if (!normalized || strcmp(qn, normalized) != 0) {
        return qn;
    }
    const char *relative = rust_registered_relative_path(ctx, normalized);
    if (relative) {
        return relative;
    }
    return qn; /* may not be registered yet but caller can still wrap as NAMED */
}

const PDXEType *rust_parse_type_node(RustLSPContext *ctx, TSNode node) {
    if (ts_node_is_null(node)) {
        return pdxe_type_unknown();
    }
    const char *kind = ts_node_type(node);

    /* primitive_type: i32, bool, char, str, … */
    if (strcmp(kind, "primitive_type") == 0) {
        char *name = rust_node_text(ctx, node);
        if (!name) {
            return pdxe_type_unknown();
        }
        return pdxe_type_builtin(ctx->arena, name);
    }

    /* type_identifier: simple named type */
    if (strcmp(kind, "type_identifier") == 0) {
        char *name = rust_node_text(ctx, node);
        if (!name) {
            return pdxe_type_unknown();
        }
        if (is_rust_primitive(name)) {
            return pdxe_type_builtin(ctx->arena, name);
        }
        if (strcmp(name, "Self") == 0 && ctx->self_type_qn) {
            return pdxe_type_named(ctx->arena, ctx->self_type_qn);
        }
        const char *qn = rust_resolve_path_expr(ctx, name);
        return pdxe_type_named(ctx->arena, qn);
    }

    /* scoped_type_identifier: A::B::C */
    if (strcmp(kind, "scoped_type_identifier") == 0) {
        char *path = gather_scoped_path(ctx, node);
        if (!path) {
            return pdxe_type_unknown();
        }
        const char *qn = resolve_path_to_type_qn(ctx, path);
        return pdxe_type_named(ctx->arena, qn);
    }

    /* reference_type: &T or &mut T */
    if (strcmp(kind, "reference_type") == 0) {
        TSNode inner = ts_node_child_by_field_name(node, "type", 4);
        if (ts_node_is_null(inner)) {
            uint32_t nc = ts_node_named_child_count(node);
            if (nc > 0) {
                inner = ts_node_named_child(node, nc - 1);
            }
        }
        const PDXEType *elem = rust_parse_type_node(ctx, inner);
        return pdxe_type_reference(ctx->arena, elem);
    }

    /* pointer_type: *const T / *mut T */
    if (strcmp(kind, "pointer_type") == 0) {
        TSNode inner = ts_node_child_by_field_name(node, "type", 4);
        if (ts_node_is_null(inner)) {
            uint32_t nc = ts_node_named_child_count(node);
            if (nc > 0) {
                inner = ts_node_named_child(node, nc - 1);
            }
        }
        return pdxe_type_pointer(ctx->arena, rust_parse_type_node(ctx, inner));
    }

    /* array_type: [T; N] — treated as slice T */
    if (strcmp(kind, "array_type") == 0) {
        TSNode elem = ts_node_child_by_field_name(node, "element", 7);
        if (ts_node_is_null(elem)) {
            elem = ts_node_child_by_field_name(node, "type", 4);
        }
        if (ts_node_is_null(elem) && ts_node_named_child_count(node) > 0) {
            elem = ts_node_named_child(node, 0);
        }
        return pdxe_type_slice(ctx->arena, rust_parse_type_node(ctx, elem));
    }

    /* slice_type: [T] */
    if (strcmp(kind, "slice_type") == 0) {
        TSNode elem = ts_node_child_by_field_name(node, "element", 7);
        if (ts_node_is_null(elem) && ts_node_named_child_count(node) > 0) {
            elem = ts_node_named_child(node, 0);
        }
        return pdxe_type_slice(ctx->arena, rust_parse_type_node(ctx, elem));
    }

    /* tuple_type: (T1, T2, …) */
    if (strcmp(kind, "tuple_type") == 0) {
        const PDXEType *elems[16];
        int count = 0;
        PDXE_FOR_EACH_NAMED_CHILD(it, node) {
            if (!(count < 16)) {
                break;
            }
            elems[count++] = rust_parse_type_node(ctx, it.node);
        }
        if (count == 0) {
            return pdxe_type_builtin(ctx->arena, "()");
        }
        if (count == 1) {
            return elems[0];
        }
        return pdxe_type_tuple(ctx->arena, elems, count);
    }

    /* unit_type: () */
    if (strcmp(kind, "unit_type") == 0) {
        return pdxe_type_builtin(ctx->arena, "()");
    }

    /* never_type: ! */
    if (strcmp(kind, "never_type") == 0) {
        return pdxe_type_builtin(ctx->arena, "!");
    }

    /* generic_type: Foo<T1, T2, …> */
    if (strcmp(kind, "generic_type") == 0) {
        TSNode tn = ts_node_child_by_field_name(node, "type", 4);
        if (ts_node_is_null(tn) && ts_node_named_child_count(node) > 0) {
            tn = ts_node_named_child(node, 0);
        }
        char *head = rust_node_text(ctx, tn);
        if (!head) {
            return pdxe_type_unknown();
        }
        const char *head_qn = resolve_path_to_type_qn(ctx, head);

        /* Gather type_arguments. */
        TSNode args = ts_node_child_by_field_name(node, "type_arguments", 14);
        const PDXEType *targs[16];
        int targ_count = 0;
        if (!ts_node_is_null(args)) {
            PDXE_FOR_EACH_NAMED_CHILD(it, args) {
                if (!(targ_count < 15)) {
                    break;
                }
                TSNode tc = it.node;
                const char *tk = ts_node_type(tc);
                /* Skip lifetime arguments — we ignore lifetimes entirely. */
                if (strcmp(tk, "lifetime") == 0) {
                    continue;
                }
                targs[targ_count++] = rust_parse_type_node(ctx, tc);
            }
        }
        if (targ_count > 0) {
            return pdxe_type_template(ctx->arena, head_qn, targs, targ_count);
        }
        return pdxe_type_named(ctx->arena, head_qn);
    }

    /* function_type: fn(T1, T2) -> R */
    if (strcmp(kind, "function_type") == 0) {
        return pdxe_type_func(ctx->arena, NULL, NULL, NULL);
    }

    /* dynamic_type: dyn Trait — record as named on the trait QN */
    if (strcmp(kind, "dynamic_type") == 0) {
        TSNode inner = ts_node_child_by_field_name(node, "trait", 5);
        if (ts_node_is_null(inner) && ts_node_named_child_count(node) > 0) {
            inner = ts_node_named_child(node, 0);
        }
        return rust_parse_type_node(ctx, inner);
    }

    /* abstract_type: impl Trait — best-effort same as dyn Trait */
    if (strcmp(kind, "abstract_type") == 0) {
        TSNode inner = ts_node_child_by_field_name(node, "trait", 5);
        if (ts_node_is_null(inner) && ts_node_named_child_count(node) > 0) {
            inner = ts_node_named_child(node, 0);
        }
        return rust_parse_type_node(ctx, inner);
    }

    /* bounded_type: T + Trait + 'a — take the first child */
    if (strcmp(kind, "bounded_type") == 0 && ts_node_named_child_count(node) > 0) {
        return rust_parse_type_node(ctx, ts_node_named_child(node, 0));
    }

    /* parenthesized_type or wrapped types */
    if (strcmp(kind, "parenthesized_type") == 0 && ts_node_named_child_count(node) > 0) {
        return rust_parse_type_node(ctx, ts_node_named_child(node, 0));
    }

    /* qualified_type: <T as Trait>::Item */
    if (strcmp(kind, "qualified_type") == 0) {
        return pdxe_type_unknown();
    }

    return pdxe_type_unknown();
}

/* Parse a textual Rust type (`Vec<String>`, `&mut Foo`, `Result<T, E>`)
 * into a PDXEType. Used when we receive types as strings (return types of
 * extracted `PDXEDefinition`s, cross-file `PDXERustLSPDef::return_types`,
 * stdlib seed entries).
 *
 * The parser is intentionally simple: it recognises the small surface
 * area that tree-sitter would produce in `rust_parse_type_node` but
 * without a parser. This is the same trade-off `pdxe_rust_parse_return_type_text`
 * makes for Go. */
static const PDXEType *parse_type_text_with_params(PDXEArena *arena, const char *text,
                                                  const char *module_qn, const char **type_params) {
    if (!text || !text[0]) {
        return pdxe_type_unknown();
    }
    /* Skip leading whitespace + lifetime + mut markers. */
    while (*text == ' ' || *text == '\t') {
        text++;
    }

    /* HRTB: `for<'a, 'b> Fn(&'a T) -> R` — strip the higher-rank
     * binder. We don't reason about explicit lifetimes anywhere, so
     * dropping it leaves the rest of the type untouched. */
    if (strncmp(text, "for<", 4) == 0) {
        const char *p = text + 4;
        int depth = 1;
        while (*p && depth > 0) {
            if (*p == '<')
                depth++;
            else if (*p == '>')
                depth--;
            p++;
        }
        while (*p == ' ')
            p++;
        text = p;
        if (!*text)
            return pdxe_type_unknown();
    }

    /* Bare leading lifetime (e.g. `'a`) — accept and skip, treating
     * the rest of the text as the actual type. Rare outside of HRTBs
     * but cheap to handle. */
    if (text[0] == '\'' && (isalpha((unsigned char)text[1]) || text[1] == '_')) {
        const char *p = text + 1;
        while (*p && (isalnum((unsigned char)*p) || *p == '_'))
            p++;
        while (*p == ' ')
            p++;
        text = p;
        if (!*text)
            return pdxe_type_unknown();
    }

    /* Reference: &T or &'a T or &mut T or &'a mut T */
    if (text[0] == '&') {
        const char *p = text + 1;
        if (*p == '\'') {
            p++;
            while (*p && (isalnum((unsigned char)*p) || *p == '_')) {
                p++;
            }
        }
        while (*p == ' ') {
            p++;
        }
        if (strncmp(p, "mut ", 4) == 0) {
            p += 4;
        }
        const PDXEType *elem = parse_type_text_with_params(arena, p, module_qn, type_params);
        return pdxe_type_reference(arena, elem);
    }

    /* Pointer: *const T / *mut T */
    if (text[0] == '*') {
        const char *p = text + 1;
        if (strncmp(p, "const ", 6) == 0) {
            p += 6;
        } else if (strncmp(p, "mut ", 4) == 0) {
            p += 4;
        }
        return pdxe_type_pointer(arena,
                                parse_type_text_with_params(arena, p, module_qn, type_params));
    }

    /* Slice: [T] */
    if (text[0] == '[' && text[strlen(text) - 1] == ']') {
        const char *p = text + 1;
        const char *end = text + strlen(text) - 1;
        size_t inner_len = (size_t)(end - p);
        char *inner = pdxe_arena_strndup(arena, p, inner_len);
        /* Array form `[T; N]` — strip the count. */
        char *semi = strchr(inner, ';');
        if (semi) {
            *semi = '\0';
            /* Trim trailing whitespace. */
            char *q = semi - 1;
            while (q > inner && isspace((unsigned char)*q)) {
                *q-- = '\0';
            }
        }
        return pdxe_type_slice(arena,
                              parse_type_text_with_params(arena, inner, module_qn, type_params));
    }

    /* Unit / never */
    if (strcmp(text, "()") == 0) {
        return pdxe_type_builtin(arena, "()");
    }
    if (strcmp(text, "!") == 0) {
        return pdxe_type_builtin(arena, "!");
    }

    /* Tuple: (T1, T2, …) — only when not a single parenthesised type. */
    if (text[0] == '(' && text[strlen(text) - 1] == ')') {
        const char *p = text + 1;
        size_t inner_len = strlen(text) - 2;
        char *inner = pdxe_arena_strndup(arena, p, inner_len);
        /* Detect comma at top level. */
        int depth = 0;
        bool has_comma = false;
        for (char *q = inner; *q; q++) {
            if (*q == '<' || *q == '(' || *q == '[')
                depth++;
            else if (*q == '>' || *q == ')' || *q == ']')
                depth--;
            else if (*q == ',' && depth == 0) {
                has_comma = true;
                break;
            }
        }
        if (!has_comma) {
            return parse_type_text_with_params(arena, inner, module_qn, type_params);
        }
        /* Split by top-level commas. */
        const PDXEType *elems[16];
        int count = 0;
        char *start = inner;
        depth = 0;
        for (char *q = inner;; q++) {
            if (*q == '<' || *q == '(' || *q == '[')
                depth++;
            else if (*q == '>' || *q == ')' || *q == ']')
                depth--;
            if ((*q == ',' && depth == 0) || *q == '\0') {
                char save = *q;
                *q = '\0';
                /* Trim. */
                while (*start == ' ')
                    start++;
                if (count < 15 && *start) {
                    elems[count++] =
                        parse_type_text_with_params(arena, start, module_qn, type_params);
                }
                if (save == '\0')
                    break;
                start = q + 1;
            }
        }
        if (count == 0)
            return pdxe_type_builtin(arena, "()");
        if (count == 1)
            return elems[0];
        return pdxe_type_tuple(arena, elems, count);
    }

    /* Generic head: head<args> */
    const char *lt = strchr(text, '<');
    if (lt) {
        const char *gt = text + strlen(text) - 1;
        if (*gt == '>') {
            size_t head_len = (size_t)(lt - text);
            char *head = pdxe_arena_strndup(arena, text, head_len);
            /* Recursive split of args by top-level commas. */
            const char *args = lt + 1;
            size_t args_len = (size_t)(gt - args);
            char *abuf = pdxe_arena_strndup(arena, args, args_len);
            const PDXEType *targs[16];
            int targ_count = 0;
            int depth = 0;
            char *start = abuf;
            for (char *q = abuf;; q++) {
                if (*q == '<' || *q == '(' || *q == '[')
                    depth++;
                else if (*q == '>' || *q == ')' || *q == ']')
                    depth--;
                if ((*q == ',' && depth == 0) || *q == '\0') {
                    char save = *q;
                    *q = '\0';
                    while (*start == ' ')
                        start++;
                    /* Skip lifetime args. */
                    if (*start != '\'' && *start && targ_count < 15) {
                        targs[targ_count++] =
                            parse_type_text_with_params(arena, start, module_qn, type_params);
                    }
                    if (save == '\0')
                        break;
                    start = q + 1;
                }
            }
            const char *head_qn = head;
            /* Issues 105 and 113: the type the head names where it is written. */
            const char *canonical_head = rmi_type_in_module(arena, module_qn, head);
            if (canonical_head) {
                head_qn = canonical_head;
            }
            if (is_rust_primitive(head)) {
                /* Primitives don't take generics in practice except for str ref — pass through. */
                return pdxe_type_builtin(arena, head);
            }
            /* Map a few well-known std type sugars. */
            return pdxe_type_template(arena, head_qn, targs, targ_count);
        }
    }

    /* Bare identifier or path. */
    if (is_rust_primitive(text)) {
        return pdxe_type_builtin(arena, text);
    }
    if (type_params) {
        for (int i = 0; type_params[i]; i++) {
            if (strcmp(text, type_params[i]) == 0) {
                return pdxe_type_type_param(arena, text);
            }
        }
    }
    /* Self -> module-qualified placeholder caller will substitute. */
    if (strcmp(text, "Self") == 0) {
        return pdxe_type_named(arena, "Self");
    }
    /* Issues 105 and 113: the type the text names in the module it is written in,
     * through that module's `use` declarations and globs, `crate`, `self`, `super`
     * and re-exports, when the repository's modules are indexed. */
    if (!strchr(text, '.')) {
        const char *canonical = rmi_type_in_module(arena, module_qn, text);
        if (canonical) {
            return pdxe_type_named(arena, canonical);
        }
    }
    /* Has `::` → absolute path; treat dotted paths as already-qualified
     * QNs (cross-file callers pass module-qualified text directly). */
    if (strstr(text, "::")) {
        return pdxe_type_named(arena, convert_path_to_qn(arena, text));
    }
    if (strchr(text, '.')) {
        return pdxe_type_named(arena, text);
    }
    return pdxe_type_named(arena, pdxe_arena_sprintf(arena, "%s.%s", module_qn, text));
}

/* Public-ish helper used by the cross-file path. */
static const PDXEType *rust_parse_return_type_text(PDXEArena *arena, const char *text,
                                                  const char *module_qn) {
    return parse_type_text_with_params(arena, text, module_qn, NULL);
}

typedef struct {
    const char *module_qn;
} RustSignatureParamParserContext;

static const PDXEType *rust_signature_param_type_adapter(PDXEArena *arena, const char *text,
                                                        void *parser_ctx) {
    const RustSignatureParamParserContext *ctx =
        (const RustSignatureParamParserContext *)parser_ctx;
    return rust_parse_return_type_text(arena, text, ctx ? ctx->module_qn : NULL);
}

/* ════════════════════════════════════════════════════════════════════
 * 5. Generic substitution
 * ════════════════════════════════════════════════════════════════════ */

/* Recursively substitute every `TYPE_PARAM` reference in `t` whose name
 * matches `params[i]` with `args[i]`. Preserves structure for composite
 * types. */
static const PDXEType *rust_substitute_type(PDXEArena *arena, const PDXEType *t, const char **params,
                                           const PDXEType **args) {
    if (!t || !params || !args) {
        return t;
    }
    switch (t->kind) {
    case PDXE_TYPE_TYPE_PARAM:
        for (int i = 0; params[i]; i++) {
            if (strcmp(t->data.type_param.name, params[i]) == 0) {
                return args[i];
            }
        }
        return t;
    case PDXE_TYPE_REFERENCE:
        return pdxe_type_reference(
            arena, rust_substitute_type(arena, t->data.reference.elem, params, args));
    case PDXE_TYPE_POINTER:
        return pdxe_type_pointer(arena,
                                rust_substitute_type(arena, t->data.pointer.elem, params, args));
    case PDXE_TYPE_SLICE:
        return pdxe_type_slice(arena, rust_substitute_type(arena, t->data.slice.elem, params, args));
    case PDXE_TYPE_TEMPLATE: {
        const PDXEType *new_args[16];
        int n = 0;
        for (; n < t->data.template_type.arg_count && n < 16; n++) {
            new_args[n] =
                rust_substitute_type(arena, t->data.template_type.template_args[n], params, args);
        }
        return pdxe_type_template(arena, t->data.template_type.template_name, new_args, n);
    }
    case PDXE_TYPE_TUPLE: {
        const PDXEType *new_elems[16];
        int n = 0;
        for (; n < t->data.tuple.count && n < 16; n++) {
            new_elems[n] = rust_substitute_type(arena, t->data.tuple.elems[n], params, args);
        }
        return pdxe_type_tuple(arena, new_elems, n);
    }
    default:
        return t;
    }
}

/* Naive Hindley-Milner-style type unification. Walks `param_type`
 * structurally against `arg_type`; whenever a `TYPE_PARAM` is bound for
 * the first time, store the corresponding `arg_type`. Subsequent
 * conflicting bindings are ignored (best-effort). */
static void rust_unify_type(const PDXEType *param_type, const PDXEType *arg_type,
                            const char **type_param_names, const PDXEType **inferred,
                            int param_count) {
    if (!param_type || !arg_type || pdxe_type_is_unknown(arg_type)) {
        return;
    }
    if (param_type->kind == PDXE_TYPE_TYPE_PARAM) {
        for (int i = 0; i < param_count; i++) {
            if (strcmp(param_type->data.type_param.name, type_param_names[i]) == 0) {
                if (!inferred[i]) {
                    inferred[i] = arg_type;
                }
                return;
            }
        }
        return;
    }
    if (param_type->kind == PDXE_TYPE_REFERENCE && arg_type->kind == PDXE_TYPE_REFERENCE) {
        rust_unify_type(param_type->data.reference.elem, arg_type->data.reference.elem,
                        type_param_names, inferred, param_count);
        return;
    }
    if (param_type->kind == PDXE_TYPE_REFERENCE) {
        rust_unify_type(param_type->data.reference.elem, arg_type, type_param_names, inferred,
                        param_count);
        return;
    }
    if (param_type->kind == PDXE_TYPE_SLICE && arg_type->kind == PDXE_TYPE_SLICE) {
        rust_unify_type(param_type->data.slice.elem, arg_type->data.slice.elem, type_param_names,
                        inferred, param_count);
        return;
    }
    if (param_type->kind == PDXE_TYPE_TEMPLATE && arg_type->kind == PDXE_TYPE_TEMPLATE) {
        if (param_type->data.template_type.arg_count == arg_type->data.template_type.arg_count) {
            int ac = param_type->data.template_type.arg_count;
            for (int i = 0; i < ac; i++) {
                rust_unify_type(param_type->data.template_type.template_args[i],
                                arg_type->data.template_type.template_args[i], type_param_names,
                                inferred, param_count);
            }
        }
        return;
    }
    /* TUPLE unification — needed for tuple-return generics like
     * `fn pair<A, B>(a: A, b: B) -> (A, B)`. */
    if (param_type->kind == PDXE_TYPE_TUPLE && arg_type->kind == PDXE_TYPE_TUPLE) {
        int pc = param_type->data.tuple.count;
        int ac = arg_type->data.tuple.count;
        int min_ = pc < ac ? pc : ac;
        for (int i = 0; i < min_; i++) {
            rust_unify_type(param_type->data.tuple.elems[i], arg_type->data.tuple.elems[i],
                            type_param_names, inferred, param_count);
        }
        return;
    }
    /* POINTER unification. */
    if (param_type->kind == PDXE_TYPE_POINTER && arg_type->kind == PDXE_TYPE_POINTER) {
        rust_unify_type(param_type->data.pointer.elem, arg_type->data.pointer.elem,
                        type_param_names, inferred, param_count);
        return;
    }
    /* Bidirectional fallback: if `arg_type` (rather than `param_type`)
     * carries the type-param marker, swap and retry. This lets
     * `unify(known_concrete, fresh_var)` solve the var. */
    if (arg_type->kind == PDXE_TYPE_TYPE_PARAM) {
        rust_unify_type(arg_type, param_type, type_param_names, inferred, param_count);
        return;
    }
}

/* Apply a solved type-param environment to a type, recursively
 * substituting bound param names with their concrete types. Returns
 * the substituted type (arena-allocated when new structure is built).
 *
 * This is the post-solve step of HM-lite: after `rust_unify_type` has
 * filled the `inferred` array, this helper walks a target type and
 * rewrites every `TYPE_PARAM` reference. */
static const PDXEType *rust_apply_subst(PDXEArena *arena, const PDXEType *t, const char **names,
                                       const PDXEType **inferred, int count) {
    if (!t)
        return t;
    switch (t->kind) {
    case PDXE_TYPE_TYPE_PARAM:
        for (int i = 0; i < count; i++) {
            if (inferred[i] && names[i] && strcmp(t->data.type_param.name, names[i]) == 0) {
                return inferred[i];
            }
        }
        return t;
    case PDXE_TYPE_REFERENCE:
        return pdxe_type_reference(
            arena, rust_apply_subst(arena, t->data.reference.elem, names, inferred, count));
    case PDXE_TYPE_POINTER:
        return pdxe_type_pointer(
            arena, rust_apply_subst(arena, t->data.pointer.elem, names, inferred, count));
    case PDXE_TYPE_SLICE:
        return pdxe_type_slice(arena,
                              rust_apply_subst(arena, t->data.slice.elem, names, inferred, count));
    case PDXE_TYPE_TEMPLATE: {
        int n = t->data.template_type.arg_count;
        if (n <= 0)
            return t;
        const PDXEType *new_args[16];
        if (n > 16)
            n = 16;
        for (int i = 0; i < n; i++) {
            new_args[i] = rust_apply_subst(arena, t->data.template_type.template_args[i], names,
                                           inferred, count);
        }
        return pdxe_type_template(arena, t->data.template_type.template_name, new_args, n);
    }
    case PDXE_TYPE_TUPLE: {
        int n = t->data.tuple.count;
        if (n <= 0)
            return t;
        const PDXEType *new_elems[16];
        if (n > 16)
            n = 16;
        for (int i = 0; i < n; i++) {
            new_elems[i] = rust_apply_subst(arena, t->data.tuple.elems[i], names, inferred, count);
        }
        return pdxe_type_tuple(arena, new_elems, n);
    }
    default:
        return t;
    }
}

/* ════════════════════════════════════════════════════════════════════
 * 6. Expression evaluator
 * ════════════════════════════════════════════════════════════════════ */

/* Evaluate the type of a literal child like `integer_literal`, `float_literal`,
 * `string_literal`, `char_literal`, `boolean_literal`. */
static const PDXEType *rust_eval_literal_type(RustLSPContext *ctx, const char *kind, TSNode node) {
    if (strcmp(kind, "integer_literal") == 0) {
        char *text = rust_node_text(ctx, node);
        /* Look at suffix. */
        if (text) {
            /* Strip leading `-` if any. */
            const char *p = text;
            if (*p == '-')
                p++;
            /* Find suffix start (first non-digit/non-_/non-x/non-X/non-hex). */
            while (*p && (isdigit((unsigned char)*p) || *p == '_' || *p == '.' || *p == 'x' ||
                          *p == 'X' || *p == 'b' || *p == 'B' || *p == 'o' || *p == 'O' ||
                          (*p >= 'a' && *p <= 'f') || (*p >= 'A' && *p <= 'F'))) {
                p++;
            }
            if (*p) {
                return pdxe_type_builtin(ctx->arena, p);
            }
        }
        return pdxe_type_builtin(ctx->arena, "i32");
    }
    if (strcmp(kind, "float_literal") == 0) {
        char *text = rust_node_text(ctx, node);
        if (text) {
            const char *p = text;
            while (*p && (isdigit((unsigned char)*p) || *p == '.' || *p == 'e' || *p == 'E' ||
                          *p == '+' || *p == '-' || *p == '_')) {
                p++;
            }
            if (*p) {
                return pdxe_type_builtin(ctx->arena, p);
            }
        }
        return pdxe_type_builtin(ctx->arena, "f64");
    }
    if (strcmp(kind, "string_literal") == 0 || strcmp(kind, "raw_string_literal") == 0) {
        /* &'static str — represented as &str */
        return pdxe_type_reference(ctx->arena, pdxe_type_builtin(ctx->arena, "str"));
    }
    if (strcmp(kind, "char_literal") == 0) {
        return pdxe_type_builtin(ctx->arena, "char");
    }
    if (strcmp(kind, "boolean_literal") == 0 || strcmp(kind, "true") == 0 ||
        strcmp(kind, "false") == 0) {
        return pdxe_type_builtin(ctx->arena, "bool");
    }
    return pdxe_type_unknown();
}

/* Look up the registered method or field type for a field/method-style
 * access. Order: inherent method → field → trait method (with single-impl
 * preference). */
static const PDXEType *rust_eval_member_access(RustLSPContext *ctx, const PDXEType *recv,
                                              const char *member);

/* Issue 105: the type of field `name` of the receiver's type (through references), or
 * unknown; a method of the name is no field. */
static const PDXEType *rust_eval_field_access(RustLSPContext *ctx, const PDXEType *recv,
                                              const char *name) {
    const PDXEType *base = recv;
    while (base && (base->kind == PDXE_TYPE_REFERENCE || base->kind == PDXE_TYPE_POINTER)) {
        base = base->kind == PDXE_TYPE_REFERENCE ? base->data.reference.elem
                                                 : base->data.pointer.elem;
    }
    const char *type_qn = NULL;
    if (base && base->kind == PDXE_TYPE_NAMED) {
        type_qn = base->data.named.qualified_name;
    } else if (base && base->kind == PDXE_TYPE_TEMPLATE) {
        type_qn = base->data.template_type.template_name;
    }
    const PDXEType *field = type_qn ? rust_lookup_field(ctx, type_qn, name, 0) : NULL;
    return field ? field : pdxe_type_unknown();
}

/* Issue 105: whether the receiver's type (through references) has a method `name` the
 * registry holds, as rust_eval_member_access looks one up. */
static bool rust_receiver_has_method(RustLSPContext *ctx, const PDXEType *recv, const char *name) {
    const PDXEType *base = recv;
    while (base && (base->kind == PDXE_TYPE_REFERENCE || base->kind == PDXE_TYPE_POINTER)) {
        base = base->kind == PDXE_TYPE_REFERENCE ? base->data.reference.elem
                                                 : base->data.pointer.elem;
    }
    const char *type_qn = NULL;
    if (base && base->kind == PDXE_TYPE_NAMED) {
        type_qn = base->data.named.qualified_name;
    } else if (base && base->kind == PDXE_TYPE_TEMPLATE) {
        type_qn = base->data.template_type.template_name;
    } else if (base && base->kind == PDXE_TYPE_BUILTIN) {
        const char *nm = base->data.builtin.name;
        type_qn = strcmp(nm, "str") == 0      ? "core.str"
                  : strcmp(nm, "String") == 0 ? "alloc.string.String"
                                              : nm;
    } else if (base && base->kind == PDXE_TYPE_SLICE) {
        type_qn = "core.slice";
    }
    return type_qn && rust_lookup_method(ctx, type_qn, name) != NULL;
}

const PDXEType *rust_eval_expr_type(RustLSPContext *ctx, TSNode node) {
    if (ts_node_is_null(node)) {
        return pdxe_type_unknown();
    }
    const char *kind = ts_node_type(node);

    /* Identifier: scope or registered symbol. */
    if (strcmp(kind, "identifier") == 0) {
        char *name = rust_node_text(ctx, node);
        if (!name) {
            return pdxe_type_unknown();
        }
        const PDXEType *t = pdxe_scope_lookup(ctx->current_scope, name);
        if (!pdxe_type_is_unknown(t)) {
            return t;
        }
        /* Module-level function. */
        const PDXERegisteredFunc *f =
            pdxe_registry_lookup_symbol(ctx->registry, ctx->module_qn, name);
        if (f && f->signature) {
            return f->signature;
        }
        /* Use-aliased symbol: resolve path then look up. */
        const char *full = rust_resolve_use(ctx, name);
        if (full) {
            const char *qn = convert_path_to_qn(ctx->arena, full);
            const PDXERegisteredFunc *uf = pdxe_registry_lookup_func(ctx->registry, qn);
            if (uf && uf->signature) {
                return uf->signature;
            }
            const PDXERegisteredType *ut = pdxe_registry_lookup_type(ctx->registry, qn);
            if (ut) {
                return pdxe_type_named(ctx->arena, qn);
            }
        }
        /* Same-module type? */
        const char *type_qn = pdxe_arena_sprintf(ctx->arena, "%s.%s", ctx->module_qn, name);
        if (pdxe_registry_lookup_type(ctx->registry, type_qn)) {
            return pdxe_type_named(ctx->arena, type_qn);
        }
        return pdxe_type_unknown();
    }

    /* self_parameter token: bound in scope as `self`. */
    if (strcmp(kind, "self") == 0 || strcmp(kind, "self_parameter") == 0) {
        return pdxe_scope_lookup(ctx->current_scope, "self");
    }

    /* scoped_identifier: A::B::C — could be a function or a type. */
    if (strcmp(kind, "scoped_identifier") == 0) {
        char *path = rust_node_text(ctx, node);
        if (!path) {
            return pdxe_type_unknown();
        }
        const char *qn = rust_resolve_path_expr(ctx, path);
        if (!qn) {
            return pdxe_type_unknown();
        }
        const PDXERegisteredFunc *f = pdxe_registry_lookup_func(ctx->registry, qn);
        if (f && f->signature) {
            return f->signature;
        }
        if (pdxe_registry_lookup_type(ctx->registry, qn)) {
            return pdxe_type_named(ctx->arena, qn);
        }
        return pdxe_type_unknown();
    }

    /* generic_function: foo::<T> */
    if (strcmp(kind, "generic_function") == 0) {
        TSNode fn = ts_node_child_by_field_name(node, "function", 8);
        if (!ts_node_is_null(fn)) {
            return rust_eval_expr_type(ctx, fn);
        }
    }

    /* field_expression: obj.field or obj.0 (tuple) */
    if (strcmp(kind, "field_expression") == 0) {
        TSNode value = ts_node_child_by_field_name(node, "value", 5);
        TSNode field = ts_node_child_by_field_name(node, "field", 5);
        if (ts_node_is_null(value) || ts_node_is_null(field)) {
            return pdxe_type_unknown();
        }
        const PDXEType *recv = rust_eval_expr_type(ctx, value);
        const char *fk = ts_node_type(field);
        if (strcmp(fk, "integer_literal") == 0) {
            /* Tuple field access. */
            if (recv) {
                const PDXEType *base = recv;
                while (base && base->kind == PDXE_TYPE_REFERENCE) {
                    base = base->data.reference.elem;
                }
                if (base && base->kind == PDXE_TYPE_TUPLE) {
                    char *idx_text = rust_node_text(ctx, field);
                    int idx = atoi(idx_text);
                    if (idx >= 0 && idx < base->data.tuple.count) {
                        return base->data.tuple.elems[idx];
                    }
                }
            }
            return pdxe_type_unknown();
        }
        char *fname = rust_node_text(ctx, field);
        if (!fname) {
            return pdxe_type_unknown();
        }
        /* Issue 105: `x.f` read as a value is a field, never a method of the name (a
         * method is only called, `x.f()`, typed above). */
        return rust_eval_field_access(ctx, recv, fname);
    }

    /* call_expression: any callable invocation. */
    if (strcmp(kind, "call_expression") == 0) {
        TSNode func_node = ts_node_child_by_field_name(node, "function", 8);
        TSNode args_node = ts_node_child_by_field_name(node, "arguments", 9);
        if (ts_node_is_null(func_node)) {
            return pdxe_type_unknown();
        }
        /* A call through a turbofish (`start::<u8>()`, `x.m::<T>()`) is its function's
         * call (issue 107A), and returns what the function does (issue 105). */
        if (strcmp(ts_node_type(func_node), "generic_function") == 0) {
            TSNode inner = ts_node_child_by_field_name(func_node, "function", 8);
            if (!ts_node_is_null(inner)) {
                func_node = inner;
            }
        }
        const char *fk = ts_node_type(func_node);

        /* Constructor of a tuple struct, unit-like struct, or stdlib
         * factory invoked via path. Try the registry; on miss, also try
         * UFCS-style method lookup to catch `String::new()`,
         * `Vec::new()`, etc. */
        if (strcmp(fk, "identifier") == 0 || strcmp(fk, "scoped_identifier") == 0) {
            char *path = rust_node_text(ctx, func_node);
            if (path) {
                /* Strip ALL turbofish (`::<...>`) so the lookup below
                 * ignores explicit type arguments — handles forms like
                 * `Vec::<i32>::new` and `parse::<u32>`. */
                rust_strip_turbofish(path);
                const char *qn = rust_resolve_path_expr(ctx, path);
                if (qn) {
                    const PDXERegisteredType *rt = pdxe_registry_lookup_type(ctx->registry, qn);
                    if (rt) {
                        return pdxe_type_named(ctx->arena, qn);
                    }
                    const PDXERegisteredFunc *f = pdxe_registry_lookup_func(ctx->registry, qn);
                    if (f && f->signature && f->signature->kind == PDXE_TYPE_FUNC) {
                        const PDXEType *const *rt_arr = f->signature->data.func.return_types;
                        if (rt_arr && rt_arr[0]) {
                            const PDXEType *ret = rt_arr[0];
                            /* Constructor-style on a stdlib receiver with
                             * `unknown` return — substitute the receiver
                             * type so chains keep typing. The heuristic
                             * matches `new`, `default`, anything starting
                             * with `from_` / `with_`, plus a small list of
                             * common factory verbs.
                             *
                             * For smart-pointer factories (`Box::new`,
                             * `Rc::new`, `Arc::new`, `RefCell::new`,
                             * `Pin::new`, `Mutex::new`, `RwLock::new`)
                             * we also try to capture the first call
                             * argument's type as a TEMPLATE arg so the
                             * Deref chain has something to peel. */
                            if (f->receiver_type && pdxe_type_is_unknown(ret) && f->short_name &&
                                (strcmp(f->short_name, "new") == 0 ||
                                 strcmp(f->short_name, "default") == 0 ||
                                 strcmp(f->short_name, "open") == 0 ||
                                 strcmp(f->short_name, "create") == 0 ||
                                 strcmp(f->short_name, "create_new") == 0 ||
                                 strcmp(f->short_name, "bind") == 0 ||
                                 strcmp(f->short_name, "connect") == 0 ||
                                 strcmp(f->short_name, "spawn") == 0 ||
                                 strcmp(f->short_name, "now") == 0 ||
                                 strncmp(f->short_name, "from_", 5) == 0 ||
                                 strncmp(f->short_name, "with_", 5) == 0 ||
                                 strcmp(f->short_name, "from") == 0)) {
                                /* Detect smart-pointer wrappers and
                                 * capture the first arg's type so
                                 * `let b = Box::new(x); b.method()`
                                 * dispatches via Deref. */
                                bool is_wrapper = strcmp(f->short_name, "new") == 0 &&
                                                  rust_type_derefs_to_first_arg(f->receiver_type);
                                if (is_wrapper && !ts_node_is_null(args_node)) {
                                    uint32_t anc = ts_node_named_child_count(args_node);
                                    if (anc > 0) {
                                        const PDXEType *arg_t = rust_eval_expr_type(
                                            ctx, ts_node_named_child(args_node, 0));
                                        if (arg_t && !pdxe_type_is_unknown(arg_t)) {
                                            return pdxe_type_template(ctx->arena, f->receiver_type,
                                                                     &arg_t, 1);
                                        }
                                    }
                                }
                                return pdxe_type_named(ctx->arena, f->receiver_type);
                            }
                            /* Self -> receiver_type substitution. */
                            if (f->receiver_type && ret->kind == PDXE_TYPE_NAMED &&
                                strcmp(ret->data.named.qualified_name, "Self") == 0) {
                                return pdxe_type_named(ctx->arena, f->receiver_type);
                            }
                            return ret;
                        }
                    }
                    /* UFCS path lookup: split off short name and try
                     * `pdxe_registry_lookup_method`. */
                    const char *dot = strrchr(qn, '.');
                    if (dot) {
                        char *head = pdxe_arena_strndup(ctx->arena, qn, (size_t)(dot - qn));
                        const char *short_name = dot + 1;
                        const PDXERegisteredFunc *m =
                            pdxe_registry_lookup_method_aliased(ctx->registry, head, short_name);
                        if (!m && ctx->module_qn) {
                            const char *full_head =
                                pdxe_arena_sprintf(ctx->arena, "%s.%s", ctx->module_qn, head);
                            m = pdxe_registry_lookup_method_aliased(ctx->registry, full_head,
                                                                   short_name);
                            if (m)
                                head = (char *)full_head;
                        }
                        if (m && m->signature && m->signature->kind == PDXE_TYPE_FUNC &&
                            m->signature->data.func.return_types &&
                            m->signature->data.func.return_types[0]) {
                            const PDXEType *ret = m->signature->data.func.return_types[0];
                            /* Substitute Self / unknown returns with the
                             * receiver type so chained calls keep typing. */
                            if (ret->kind == PDXE_TYPE_NAMED &&
                                strcmp(ret->data.named.qualified_name, "Self") == 0) {
                                return pdxe_type_named(ctx->arena, head);
                            }
                            if (pdxe_type_is_unknown(ret) &&
                                (strcmp(short_name, "new") == 0 ||
                                 strcmp(short_name, "default") == 0 ||
                                 strcmp(short_name, "with_capacity") == 0 ||
                                 strcmp(short_name, "from") == 0)) {
                                /* Smart-pointer wrap: capture first arg
                                 * type into a TEMPLATE so Deref can peel. */
                                if (strcmp(short_name, "new") == 0 &&
                                    rust_type_derefs_to_first_arg(head) &&
                                    !ts_node_is_null(args_node)) {
                                    uint32_t anc = ts_node_named_child_count(args_node);
                                    if (anc > 0) {
                                        const PDXEType *arg_t = rust_eval_expr_type(
                                            ctx, ts_node_named_child(args_node, 0));
                                        if (arg_t && !pdxe_type_is_unknown(arg_t)) {
                                            return pdxe_type_template(ctx->arena, head, &arg_t, 1);
                                        }
                                    }
                                }
                                return pdxe_type_named(ctx->arena, head);
                            }
                            return ret;
                        }
                    }
                }
            }
        }

        /* Method call expressed as field_expression callee. */
        if (strcmp(fk, "field_expression") == 0) {
            TSNode value = ts_node_child_by_field_name(func_node, "value", 5);
            TSNode field = ts_node_child_by_field_name(func_node, "field", 5);
            if (!ts_node_is_null(value) && !ts_node_is_null(field)) {
                const PDXEType *recv = rust_eval_expr_type(ctx, value);
                char *mname = rust_node_text(ctx, field);
                if (mname && recv) {
                    const PDXEType *base = recv;
                    while (base && base->kind == PDXE_TYPE_REFERENCE) {
                        base = base->data.reference.elem;
                    }
                    /* Template Vec<T> method handling */
                    if (base && base->kind == PDXE_TYPE_TEMPLATE) {
                        const char *tname = base->data.template_type.template_name;
                        /* Iterator-producing methods on Vec/&[T]/Option/Result/HashMap. */
                        if (strstr(tname, "Vec") || strstr(tname, "VecDeque") ||
                            strstr(tname, "HashSet") || strstr(tname, "BTreeSet")) {
                            if (strcmp(mname, "iter") == 0 || strcmp(mname, "iter_mut") == 0 ||
                                strcmp(mname, "into_iter") == 0 || strcmp(mname, "drain") == 0) {
                                /* Iterator<Item=T> — represent loosely as the elem type for our
                                 * downstream chain calls. Even when the elem type is not
                                 * known, return Iterator (with no args) so further chain calls
                                 * can still attribute via Iterator's registered methods. */
                                if (base->data.template_type.arg_count > 0) {
                                    return pdxe_type_template(
                                        ctx->arena, "core.iter.Iterator",
                                        &base->data.template_type.template_args[0], 1);
                                }
                                return pdxe_type_template(ctx->arena, "core.iter.Iterator", NULL, 0);
                            }
                            /* Methods returning the element type directly. */
                            if (strcmp(mname, "remove") == 0 || strcmp(mname, "swap_remove") == 0) {
                                if (base->data.template_type.arg_count > 0) {
                                    return base->data.template_type.template_args[0];
                                }
                            }
                            if (strcmp(mname, "len") == 0 || strcmp(mname, "capacity") == 0) {
                                return pdxe_type_builtin(ctx->arena, "usize");
                            }
                            if (strcmp(mname, "is_empty") == 0 || strcmp(mname, "contains") == 0) {
                                return pdxe_type_builtin(ctx->arena, "bool");
                            }
                            if (strcmp(mname, "first") == 0 || strcmp(mname, "last") == 0 ||
                                strcmp(mname, "get") == 0 || strcmp(mname, "pop") == 0) {
                                if (base->data.template_type.arg_count > 0) {
                                    const PDXEType *opt_args[1] = {
                                        base->data.template_type.template_args[0]};
                                    return pdxe_type_template(ctx->arena, "core.option.Option",
                                                             opt_args, 1);
                                }
                            }
                            if (strcmp(mname, "as_slice") == 0) {
                                if (base->data.template_type.arg_count > 0) {
                                    return pdxe_type_reference(
                                        ctx->arena,
                                        pdxe_type_slice(ctx->arena,
                                                       base->data.template_type.template_args[0]));
                                }
                            }
                        }
                        if (strstr(tname, "Option")) {
                            if (strcmp(mname, "unwrap") == 0 || strcmp(mname, "expect") == 0 ||
                                strcmp(mname, "unwrap_or") == 0 ||
                                strcmp(mname, "unwrap_or_default") == 0 ||
                                strcmp(mname, "unwrap_or_else") == 0) {
                                if (base->data.template_type.arg_count > 0) {
                                    return base->data.template_type.template_args[0];
                                }
                            }
                            if (strcmp(mname, "is_some") == 0 || strcmp(mname, "is_none") == 0) {
                                return pdxe_type_builtin(ctx->arena, "bool");
                            }
                            if (strcmp(mname, "as_ref") == 0) {
                                if (base->data.template_type.arg_count > 0) {
                                    const PDXEType *arg0 = pdxe_type_reference(
                                        ctx->arena, base->data.template_type.template_args[0]);
                                    return pdxe_type_template(ctx->arena, "core.option.Option",
                                                             &arg0, 1);
                                }
                            }
                        }
                        if (strstr(tname, "Result")) {
                            if (strcmp(mname, "unwrap") == 0 || strcmp(mname, "expect") == 0 ||
                                strcmp(mname, "unwrap_or") == 0) {
                                if (base->data.template_type.arg_count > 0) {
                                    return base->data.template_type.template_args[0];
                                }
                            }
                            if (strcmp(mname, "ok") == 0 &&
                                base->data.template_type.arg_count > 0) {
                                const PDXEType *a0 = base->data.template_type.template_args[0];
                                return pdxe_type_template(ctx->arena, "core.option.Option", &a0, 1);
                            }
                            if (strcmp(mname, "err") == 0 &&
                                base->data.template_type.arg_count > 1) {
                                const PDXEType *a1 = base->data.template_type.template_args[1];
                                return pdxe_type_template(ctx->arena, "core.option.Option", &a1, 1);
                            }
                            if (strcmp(mname, "is_ok") == 0 || strcmp(mname, "is_err") == 0) {
                                return pdxe_type_builtin(ctx->arena, "bool");
                            }
                        }
                        if (strstr(tname, "Iterator")) {
                            /* map/filter/take/skip/rev/chain → Iterator (with the relevant elem) */
                            if (strcmp(mname, "collect") == 0) {
                                /* Often Vec<Item>; without turbofish info we model as Vec<elem>. */
                                if (base->data.template_type.arg_count > 0) {
                                    return pdxe_type_template(ctx->arena, "alloc.vec.Vec",
                                                             base->data.template_type.template_args,
                                                             base->data.template_type.arg_count);
                                }
                            }
                            if (strcmp(mname, "count") == 0 || strcmp(mname, "len") == 0) {
                                return pdxe_type_builtin(ctx->arena, "usize");
                            }
                            if (strcmp(mname, "next") == 0 || strcmp(mname, "last") == 0 ||
                                strcmp(mname, "nth") == 0 || strcmp(mname, "find") == 0 ||
                                strcmp(mname, "max") == 0 || strcmp(mname, "min") == 0 ||
                                strcmp(mname, "max_by") == 0 || strcmp(mname, "min_by") == 0) {
                                if (base->data.template_type.arg_count > 0) {
                                    const PDXEType *a0 = base->data.template_type.template_args[0];
                                    return pdxe_type_template(ctx->arena, "core.option.Option", &a0,
                                                             1);
                                }
                            }
                            if (strcmp(mname, "filter") == 0 || strcmp(mname, "take") == 0 ||
                                strcmp(mname, "skip") == 0 || strcmp(mname, "rev") == 0 ||
                                strcmp(mname, "cloned") == 0 || strcmp(mname, "copied") == 0 ||
                                strcmp(mname, "step_by") == 0 || strcmp(mname, "fuse") == 0 ||
                                strcmp(mname, "peekable") == 0 || strcmp(mname, "by_ref") == 0 ||
                                strcmp(mname, "take_while") == 0 ||
                                strcmp(mname, "skip_while") == 0 || strcmp(mname, "inspect") == 0) {
                                return base; /* preserves Iterator<Item> */
                            }
                            if ((strcmp(mname, "sum") == 0 || strcmp(mname, "product") == 0 ||
                                 strcmp(mname, "fold") == 0 || strcmp(mname, "reduce") == 0) &&
                                base->data.template_type.arg_count > 0) {
                                return base->data.template_type.template_args[0];
                            }
                            if (strcmp(mname, "any") == 0 || strcmp(mname, "all") == 0) {
                                return pdxe_type_builtin(ctx->arena, "bool");
                            }
                            if (strcmp(mname, "position") == 0) {
                                const PDXEType *usize = pdxe_type_builtin(ctx->arena, "usize");
                                return pdxe_type_template(ctx->arena, "core.option.Option", &usize,
                                                         1);
                            }
                            if (strcmp(mname, "enumerate") == 0 &&
                                base->data.template_type.arg_count > 0) {
                                /* Iterator<(usize, T)>. */
                                const PDXEType *pair[2] = {
                                    pdxe_type_builtin(ctx->arena, "usize"),
                                    base->data.template_type.template_args[0]};
                                const PDXEType *tup = pdxe_type_tuple(ctx->arena, pair, 2);
                                return pdxe_type_template(ctx->arena, "core.iter.Iterator", &tup, 1);
                            }
                        }
                        /* HashMap<K, V> / BTreeMap<K, V> generics. */
                        if (strstr(tname, "HashMap") || strstr(tname, "BTreeMap")) {
                            if (strcmp(mname, "len") == 0) {
                                return pdxe_type_builtin(ctx->arena, "usize");
                            }
                            if (strcmp(mname, "is_empty") == 0 ||
                                strcmp(mname, "contains_key") == 0) {
                                return pdxe_type_builtin(ctx->arena, "bool");
                            }
                            if (strcmp(mname, "get") == 0 || strcmp(mname, "get_mut") == 0 ||
                                strcmp(mname, "remove") == 0) {
                                if (base->data.template_type.arg_count > 1) {
                                    const PDXEType *v = base->data.template_type.template_args[1];
                                    return pdxe_type_template(ctx->arena, "core.option.Option", &v,
                                                             1);
                                }
                            }
                            if (strcmp(mname, "iter") == 0 || strcmp(mname, "iter_mut") == 0) {
                                /* Iterator<(K, V)>. */
                                if (base->data.template_type.arg_count > 1) {
                                    const PDXEType *pair[2] = {
                                        base->data.template_type.template_args[0],
                                        base->data.template_type.template_args[1]};
                                    const PDXEType *tup = pdxe_type_tuple(ctx->arena, pair, 2);
                                    return pdxe_type_template(ctx->arena, "core.iter.Iterator", &tup,
                                                             1);
                                }
                            }
                            if (strcmp(mname, "keys") == 0 &&
                                base->data.template_type.arg_count > 0) {
                                return pdxe_type_template(ctx->arena, "core.iter.Iterator",
                                                         &base->data.template_type.template_args[0],
                                                         1);
                            }
                            if (strcmp(mname, "values") == 0 &&
                                base->data.template_type.arg_count > 1) {
                                return pdxe_type_template(ctx->arena, "core.iter.Iterator",
                                                         &base->data.template_type.template_args[1],
                                                         1);
                            }
                        }
                    }
                    /* Fall through: registered method on the named type.
                     * Unwrap the resulting FUNC signature to its first
                     * return type so chains like
                     * `String::new().to_uppercase().len()` keep typing
                     * across each link. */
                    const PDXEType *t = rust_eval_member_access(ctx, recv, mname);
                    if (t && t->kind == PDXE_TYPE_FUNC && t->data.func.return_types &&
                        t->data.func.return_types[0]) {
                        const PDXEType *ret = t->data.func.return_types[0];
                        /* Self -> receiver. */
                        if (ret->kind == PDXE_TYPE_NAMED &&
                            strcmp(ret->data.named.qualified_name, "Self") == 0) {
                            const PDXEType *rb = recv;
                            while (rb && rb->kind == PDXE_TYPE_REFERENCE)
                                rb = rb->data.reference.elem;
                            if (rb && rb->kind == PDXE_TYPE_NAMED) {
                                return pdxe_type_named(ctx->arena, rb->data.named.qualified_name);
                            }
                        }
                        return ret;
                    }
                    /* Issue 105: `x.f()` calls a method; a field of the name is no
                     * answer (a field is called as `(x.f)()`). */
                    return rust_receiver_has_method(ctx, recv, mname) ? t : pdxe_type_unknown();
                }
            }
            return pdxe_type_unknown(); /* issue 105: a method call is never a field's */
        }

        /* Fallback: function expression's return type. */
        const PDXEType *func_type = rust_eval_expr_type(ctx, func_node);
        if (func_type && func_type->kind == PDXE_TYPE_FUNC && func_type->data.func.return_types &&
            func_type->data.func.return_types[0]) {
            return func_type->data.func.return_types[0];
        }
        if (func_type && func_type->kind == PDXE_TYPE_NAMED) {
            return func_type;
        }
        return pdxe_type_unknown();
    }

    /* macro_invocation: vec!, format!, … */
    if (strcmp(kind, "macro_invocation") == 0) {
        TSNode mname = ts_node_child_by_field_name(node, "macro", 5);
        if (ts_node_is_null(mname) && ts_node_named_child_count(node) > 0) {
            mname = ts_node_named_child(node, 0);
        }
        char *name = rust_node_text(ctx, mname);
        if (!name) {
            return pdxe_type_unknown();
        }
        if (strcmp(name, "vec") == 0) {
            /* vec![T] — peek first argument's type. */
            TSNode args = ts_node_child_by_field_name(node, "arguments", 9);
            if (!ts_node_is_null(args)) {
                /* args is a token_tree; skim its named children for the first
                 * expression token. */
                PDXE_FOR_EACH_NAMED_CHILD(it, args) {
                    TSNode c = it.node;
                    const PDXEType *elem = rust_eval_expr_type(ctx, c);
                    if (elem && !pdxe_type_is_unknown(elem)) {
                        return pdxe_type_template(ctx->arena, "alloc.vec.Vec", &elem, 1);
                    }
                }
            }
            return pdxe_type_template(ctx->arena, "alloc.vec.Vec", NULL, 0);
        }
        if (is_string_macro(name)) {
            return pdxe_type_named(ctx->arena, "alloc.string.String");
        }
        if (is_void_macro(name)) {
            return pdxe_type_builtin(ctx->arena, "()");
        }
        return pdxe_type_unknown();
    }

    /* reference_expression: &x or &mut x */
    if (strcmp(kind, "reference_expression") == 0) {
        TSNode value = ts_node_child_by_field_name(node, "value", 5);
        if (ts_node_is_null(value) && ts_node_named_child_count(node) > 0) {
            value = ts_node_named_child(node, 0);
        }
        return pdxe_type_reference(ctx->arena, rust_eval_expr_type(ctx, value));
    }

    /* unary_expression — *x dereferences, !x is bool, -x same as operand */
    if (strcmp(kind, "unary_expression") == 0) {
        TSNode operand = ts_node_named_child_count(node) > 0
                             ? ts_node_named_child(node, ts_node_named_child_count(node) - 1)
                             : (TSNode){0};
        char *op = NULL;
        PDXE_FOR_EACH_CHILD(it, node) {
            TSNode c = it.node;
            if (!ts_node_is_named(c)) {
                op = rust_node_text(ctx, c);
                if (op)
                    break;
            }
        }
        const PDXEType *inner =
            ts_node_is_null(operand) ? pdxe_type_unknown() : rust_eval_expr_type(ctx, operand);
        if (op && strcmp(op, "*") == 0) {
            if (inner && inner->kind == PDXE_TYPE_REFERENCE) {
                return inner->data.reference.elem;
            }
            if (inner && inner->kind == PDXE_TYPE_POINTER) {
                return inner->data.pointer.elem;
            }
            return inner;
        }
        if (op && strcmp(op, "!") == 0) {
            return pdxe_type_builtin(ctx->arena, "bool");
        }
        return inner;
    }

    /* binary_expression — comparisons → bool, logical → bool, arith → left */
    if (strcmp(kind, "binary_expression") == 0) {
        TSNode left = ts_node_child_by_field_name(node, "left", 4);
        PDXE_FOR_EACH_CHILD(it, node) {
            TSNode c = it.node;
            if (ts_node_is_named(c))
                continue;
            char *op = rust_node_text(ctx, c);
            if (!op)
                continue;
            if (strcmp(op, "==") == 0 || strcmp(op, "!=") == 0 || strcmp(op, "<") == 0 ||
                strcmp(op, ">") == 0 || strcmp(op, "<=") == 0 || strcmp(op, ">=") == 0 ||
                strcmp(op, "&&") == 0 || strcmp(op, "||") == 0) {
                return pdxe_type_builtin(ctx->arena, "bool");
            }
            break;
        }
        if (!ts_node_is_null(left)) {
            return rust_eval_expr_type(ctx, left);
        }
        return pdxe_type_unknown();
    }

    /* index_expression */
    if (strcmp(kind, "index_expression") == 0) {
        TSNode value = ts_node_child_by_field_name(node, "value", 5);
        if (ts_node_is_null(value) && ts_node_named_child_count(node) > 0) {
            value = ts_node_named_child(node, 0);
        }
        const PDXEType *op_type = rust_eval_expr_type(ctx, value);
        const PDXEType *base = op_type;
        while (base && base->kind == PDXE_TYPE_REFERENCE)
            base = base->data.reference.elem;
        if (base && base->kind == PDXE_TYPE_SLICE) {
            return base->data.slice.elem;
        }
        if (base && base->kind == PDXE_TYPE_TEMPLATE) {
            const char *nm = base->data.template_type.template_name;
            if ((strstr(nm, "Vec") || strstr(nm, "VecDeque")) &&
                base->data.template_type.arg_count > 0) {
                return base->data.template_type.template_args[0];
            }
            if ((strstr(nm, "HashMap") || strstr(nm, "BTreeMap")) &&
                base->data.template_type.arg_count > 1) {
                return base->data.template_type.template_args[1];
            }
        }
        return pdxe_type_unknown();
    }

    /* parenthesized_expression */
    if (strcmp(kind, "parenthesized_expression") == 0 && ts_node_named_child_count(node) > 0) {
        return rust_eval_expr_type(ctx, ts_node_named_child(node, 0));
    }

    /* try_expression: e? — peel one Result<T, _> / Option<T> layer. */
    if (strcmp(kind, "try_expression") == 0 && ts_node_named_child_count(node) > 0) {
        /* Issue 105: `?` gives a Result's or an Option's value; of anything else, what
         * its Try impl says, which is not read: unknown, never the value's own type. */
        const PDXEType *inner = rust_eval_expr_type(ctx, ts_node_named_child(node, 0));
        if (inner && inner->kind == PDXE_TYPE_TEMPLATE) {
            const char *nm = inner->data.template_type.template_name;
            const char *last = strrchr(nm, '.');
            last = last ? last + 1 : nm;
            if ((strcmp(last, "Result") == 0 || strcmp(last, "Option") == 0) &&
                inner->data.template_type.arg_count > 0) {
                return inner->data.template_type.template_args[0];
            }
        }
        return pdxe_type_unknown();
    }

    /* await_expression: future.await — peel one Future / Poll layer naively. */
    if (strcmp(kind, "await_expression") == 0 && ts_node_named_child_count(node) > 0) {
        /* Issue 105: only a future's output is known: an `async` function's call
         * (core.future.Future<T>, rust_populate_cross_registry). Anything else awaited
         * (a type implementing IntoFuture) gives what its impl says, which is not read:
         * unknown, never the awaited value's own type. */
        const PDXEType *inner = rust_eval_expr_type(ctx, ts_node_named_child(node, 0));
        if (inner && inner->kind == PDXE_TYPE_TEMPLATE && inner->data.template_type.arg_count > 0 &&
            strcmp(inner->data.template_type.template_name, "core.future.Future") == 0) {
            return inner->data.template_type.template_args[0];
        }
        return pdxe_type_unknown();
    }

    /* type_cast_expression: x as T */
    if (strcmp(kind, "type_cast_expression") == 0) {
        TSNode tn = ts_node_child_by_field_name(node, "type", 4);
        if (!ts_node_is_null(tn)) {
            return rust_parse_type_node(ctx, tn);
        }
    }

    /* tuple_expression */
    if (strcmp(kind, "tuple_expression") == 0) {
        const PDXEType *elems[16];
        int count = 0;
        PDXE_FOR_EACH_NAMED_CHILD(it, node) {
            if (!(count < 16)) {
                break;
            }
            elems[count++] = rust_eval_expr_type(ctx, it.node);
        }
        if (count == 0) {
            return pdxe_type_builtin(ctx->arena, "()");
        }
        /* Issue 105: `(x,)` is a tuple of one, never `x` (a parenthesized expression
         * is a node of its own). */
        return pdxe_type_tuple(ctx->arena, elems, count);
    }

    /* array_expression: [a, b, c] */
    if (strcmp(kind, "array_expression") == 0) {
        if (ts_node_named_child_count(node) > 0) {
            const PDXEType *elem = rust_eval_expr_type(ctx, ts_node_named_child(node, 0));
            return pdxe_type_slice(ctx->arena, elem);
        }
        return pdxe_type_unknown();
    }

    /* range_expression: a..b, a..=b, a.., ..b, .. — model as
     * Iterator<elem> so chains like `(0..n).map(...).count()` keep
     * typing. The element type is the type of the start/end expr. */
    if (strcmp(kind, "range_expression") == 0) {
        const PDXEType *elem_type = NULL;
        PDXE_FOR_EACH_NAMED_CHILD(it, node) {
            TSNode c = it.node;
            const PDXEType *t = rust_eval_expr_type(ctx, c);
            if (t && !pdxe_type_is_unknown(t)) {
                elem_type = t;
                break;
            }
        }
        if (elem_type) {
            return pdxe_type_template(ctx->arena, "core.iter.Iterator", &elem_type, 1);
        }
        return pdxe_type_template(ctx->arena, "core.iter.Iterator", NULL, 0);
    }

    /* unit_expression */
    if (strcmp(kind, "unit_expression") == 0) {
        return pdxe_type_builtin(ctx->arena, "()");
    }

    /* struct_expression: Foo { … } */
    if (strcmp(kind, "struct_expression") == 0) {
        TSNode name_node = ts_node_child_by_field_name(node, "name", 4);
        if (ts_node_is_null(name_node) && ts_node_named_child_count(node) > 0) {
            name_node = ts_node_named_child(node, 0);
        }
        if (!ts_node_is_null(name_node)) {
            char *path = rust_node_text(ctx, name_node);
            if (path) {
                return pdxe_type_named(ctx->arena, resolve_path_to_type_qn(ctx, path));
            }
        }
        return pdxe_type_unknown();
    }

    /* if_expression / match_expression / block / loop_expression — value of
     * the trailing expression in the consequence. */
    if (strcmp(kind, "if_expression") == 0) {
        TSNode cons = ts_node_child_by_field_name(node, "consequence", 11);
        if (!ts_node_is_null(cons)) {
            return rust_eval_expr_type(ctx, cons);
        }
    }
    if (strcmp(kind, "match_expression") == 0) {
        /* Take the type of the first arm's value if available. */
        TSNode body = ts_node_child_by_field_name(node, "body", 4);
        if (!ts_node_is_null(body)) {
            PDXE_FOR_EACH_NAMED_CHILD(it, body) {
                TSNode arm = it.node;
                if (strcmp(ts_node_type(arm), "match_arm") == 0) {
                    TSNode v = ts_node_child_by_field_name(arm, "value", 5);
                    if (!ts_node_is_null(v)) {
                        return rust_eval_expr_type(ctx, v);
                    }
                }
            }
        }
    }
    if (strcmp(kind, "block") == 0) {
        /* Find the last expression child (the trailing expression of the block). */
        TSNode last = {0};
        bool found = false;
        /* The last such: every named child in source order, the last kept. */
        PDXE_FOR_EACH_NAMED_CHILD(it, node) {
            TSNode c = it.node;
            const char *ck = ts_node_type(c);
            if (strcmp(ck, "expression_statement") != 0 && strcmp(ck, "let_declaration") != 0 &&
                strcmp(ck, "empty_statement") != 0 && strcmp(ck, "line_comment") != 0 &&
                strcmp(ck, "block_comment") != 0) {
                last = c;
                found = true;
            }
        }
        if (found) {
            return rust_eval_expr_type(ctx, last);
        }
        return pdxe_type_builtin(ctx->arena, "()");
    }
    if (strcmp(kind, "loop_expression") == 0) {
        return pdxe_type_builtin(ctx->arena, "()");
    }

    /* closure_expression: |a, b| body — best effort. Return type comes from
     * the body's last expression if present. */
    if (strcmp(kind, "closure_expression") == 0) {
        TSNode body = ts_node_child_by_field_name(node, "body", 4);
        if (ts_node_is_null(body)) {
            return pdxe_type_func(ctx->arena, NULL, NULL, NULL);
        }
        return pdxe_type_func(ctx->arena, NULL, NULL, NULL);
    }

    /* Literals fall through. */
    if (strcmp(kind, "integer_literal") == 0 || strcmp(kind, "float_literal") == 0 ||
        strcmp(kind, "string_literal") == 0 || strcmp(kind, "raw_string_literal") == 0 ||
        strcmp(kind, "char_literal") == 0 || strcmp(kind, "boolean_literal") == 0 ||
        strcmp(kind, "true") == 0 || strcmp(kind, "false") == 0) {
        return rust_eval_literal_type(ctx, kind, node);
    }

    /* break / continue / return — expressions with no useful type for callers. */
    if (strcmp(kind, "return_expression") == 0 || strcmp(kind, "break_expression") == 0 ||
        strcmp(kind, "continue_expression") == 0 || strcmp(kind, "yield_expression") == 0) {
        return pdxe_type_builtin(ctx->arena, "!");
    }

    return pdxe_type_unknown();
}

/* Bidirectional wrapper. Evaluate `node` with an `expected` hint. The
 * hint is used post-synthesis: if synthesis returned an under-specified
 * type (unknown, or a TEMPLATE without args, or a NAMED that the hint
 * refines), we substitute the hint when it matches structurally. */
const PDXEType *rust_eval_expr_typed(RustLSPContext *ctx, TSNode node, const PDXEType *expected) {
    const PDXEType *synth = rust_eval_expr_type(ctx, node);
    if (!expected)
        return synth;
    if (!synth || pdxe_type_is_unknown(synth)) {
        return expected;
    }
    /* Template with same head and missing args -> use expected. */
    if (synth->kind == PDXE_TYPE_TEMPLATE && expected->kind == PDXE_TYPE_TEMPLATE &&
        synth->data.template_type.template_name && expected->data.template_type.template_name &&
        strcmp(synth->data.template_type.template_name,
               expected->data.template_type.template_name) == 0 &&
        synth->data.template_type.arg_count == 0 && expected->data.template_type.arg_count > 0) {
        return expected;
    }
    /* NAMED matches expected NAMED — no refinement needed. */
    if (synth->kind == PDXE_TYPE_NAMED && expected->kind == PDXE_TYPE_TEMPLATE &&
        synth->data.named.qualified_name && expected->data.template_type.template_name &&
        strcmp(synth->data.named.qualified_name, expected->data.template_type.template_name) == 0) {
        return expected; /* refine NAMED → TEMPLATE with args */
    }
    return synth;
}

/* Member access type: returns the PDXEType for `recv.field_or_method`.
 * Methods return their FUNC signature; fields return the field type. */
static const PDXEType *rust_eval_member_access(RustLSPContext *ctx, const PDXEType *recv,
                                              const char *member) {
    if (!recv || !member)
        return pdxe_type_unknown();
    /* Auto-deref through references. */
    const PDXEType *base = recv;
    while (base && (base->kind == PDXE_TYPE_REFERENCE || base->kind == PDXE_TYPE_POINTER)) {
        base = (base->kind == PDXE_TYPE_REFERENCE) ? base->data.reference.elem
                                                  : base->data.pointer.elem;
    }
    if (!base)
        return pdxe_type_unknown();

    const char *type_qn = NULL;
    const PDXEType **template_args = NULL;
    int template_count = 0;
    const char **template_params = NULL;

    if (base->kind == PDXE_TYPE_NAMED) {
        type_qn = base->data.named.qualified_name;
    } else if (base->kind == PDXE_TYPE_TEMPLATE) {
        type_qn = base->data.template_type.template_name;
        template_args = (const PDXEType **)base->data.template_type.template_args;
        template_count = base->data.template_type.arg_count;
    } else if (base->kind == PDXE_TYPE_BUILTIN) {
        /* Map a few primitives into stdlib QNs so registered methods on
         * `str`/`String`/integers are findable. */
        const char *nm = base->data.builtin.name;
        if (strcmp(nm, "str") == 0)
            type_qn = "core.str";
        else if (strcmp(nm, "String") == 0)
            type_qn = "alloc.string.String";
        else
            type_qn = nm;
    } else if (base->kind == PDXE_TYPE_SLICE) {
        type_qn = "core.slice";
    } else {
        return pdxe_type_unknown();
    }
    if (!type_qn)
        return pdxe_type_unknown();

    /* Check inherent method first. */
    const PDXERegisteredFunc *method = rust_lookup_method(ctx, type_qn, member);
    if (method && method->signature) {
        if (template_args && method->type_param_names) {
            template_params = method->type_param_names;
            const PDXEType *sub =
                rust_substitute_type(ctx->arena, method->signature, template_params, template_args);
            return sub;
        }
        /* Substitute any Self placeholder on the return type. */
        const PDXEType *sig = method->signature;
        if (sig && sig->kind == PDXE_TYPE_FUNC && sig->data.func.return_types &&
            sig->data.func.return_types[0]) {
            const PDXEType *ret = sig->data.func.return_types[0];
            if (ret && ret->kind == PDXE_TYPE_NAMED &&
                strcmp(ret->data.named.qualified_name, "Self") == 0) {
                return pdxe_type_named(ctx->arena, type_qn);
            }
        }
        return method->signature;
    }

    /* Field on the type's RegisteredType. */
    const PDXEType *ft = rust_lookup_field(ctx, type_qn, member, 0);
    if (ft) {
        if (template_args && template_params) {
            return rust_substitute_type(ctx->arena, ft, template_params, template_args);
        }
        return ft;
    }
    return pdxe_type_unknown();
}

/* ════════════════════════════════════════════════════════════════════
 * 7. Method dispatch + field lookup
 * ════════════════════════════════════════════════════════════════════ */

static const PDXEType *rust_lookup_field(RustLSPContext *ctx, const char *type_qn,
                                        const char *field_name, int depth) {
    if (!type_qn || !field_name || depth > PDXE_LSP_MAX_LOOKUP_DEPTH)
        return NULL;
    const PDXERegisteredType *rt = pdxe_registry_lookup_type(ctx->registry, type_qn);
    if (!rt)
        return NULL;
    if (rt->alias_of)
        return rust_lookup_field(ctx, rt->alias_of, field_name, depth + 1);
    if (rt->field_names) {
        for (int i = 0; rt->field_names[i]; i++) {
            if (strcmp(rt->field_names[i], field_name) == 0 && rt->field_types &&
                rt->field_types[i]) {
                return rt->field_types[i];
            }
        }
    }
    if (rt->embedded_types) {
        for (int i = 0; rt->embedded_types[i]; i++) {
            const PDXEType *f = rust_lookup_field(ctx, rt->embedded_types[i], field_name, depth + 1);
            if (f)
                return f;
        }
    }
    return NULL;
}

/* Hardcoded Deref<Target=U> map for stdlib smart pointers + guards. The
 * format mirrors the de-facto rule: `<smart-pointer-QN-prefix>` →
 * `<inner-type position>` where inner is taken from the receiver's
 * template args. We don't store U literally; instead we let the call
 * site pass `template_args[idx]` as the new receiver type for retry. */
static bool rust_type_derefs_to_first_arg(const char *type_qn) {
    if (!type_qn)
        return false;
    static const char *derefable[] = {"alloc.boxed.Box",
                                      "std.boxed.Box",
                                      "alloc.rc.Rc",
                                      "std.rc.Rc",
                                      "alloc.sync.Arc",
                                      "std.sync.Arc",
                                      "core.cell.RefCell",
                                      "std.cell.RefCell",
                                      "core.cell.Cell",
                                      "std.cell.Cell",
                                      "std.sync.MutexGuard",
                                      "std.sync.RwLockReadGuard",
                                      "std.sync.RwLockWriteGuard",
                                      "core.pin.Pin",
                                      NULL};
    for (const char **p = derefable; *p; p++) {
        if (strcmp(*p, type_qn) == 0)
            return true;
    }
    return false;
}

/* Returns the inner type after one Deref step. For NAMED types we check
 * for a registered `embedded_types` entry whose tail is "Target=<U>" or,
 * for stdlib smart pointers, peel the first template arg. Returns NULL
 * if no Deref relationship is known. */
static const PDXEType *rust_deref_step(RustLSPContext *ctx, const PDXEType *t) {
    if (!t)
        return NULL;
    /* Smart pointers: Box<T>, Rc<T>, Arc<T>, etc. The grammar gives us a
     * TEMPLATE; the inner type is the first template arg. */
    if (t->kind == PDXE_TYPE_TEMPLATE && t->data.template_type.template_name) {
        if (rust_type_derefs_to_first_arg(t->data.template_type.template_name)) {
            if (t->data.template_type.arg_count > 0) {
                return t->data.template_type.template_args[0];
            }
            return NULL;
        }
    }
    /* NAMED smart-pointer (rare — only when the user wrote `Rc` without
     * type arg) — no inner to peel. */
    if (t->kind == PDXE_TYPE_NAMED && t->data.named.qualified_name) {
        if (rust_type_derefs_to_first_arg(t->data.named.qualified_name)) {
            return NULL;
        }
        /* Project type with a registered Deref impl. We approximate this
         * by checking `embedded_types` for an entry of the form
         * `<TraitQN>:DerefTarget:<U>` — set up by `impl Deref for X`
         * post-processing. (Currently not produced by extract_defs; the
         * call still works for stdlib types.) */
        const PDXERegisteredType *rt =
            pdxe_registry_lookup_type(ctx->registry, t->data.named.qualified_name);
        if (rt && rt->embedded_types) {
            for (int i = 0; rt->embedded_types[i]; i++) {
                const char *e = rt->embedded_types[i];
                static const char prefix[] = "DerefTarget:";
                if (strncmp(e, prefix, sizeof(prefix) - 1) == 0) {
                    return pdxe_type_named(ctx->arena, e + sizeof(prefix) - 1);
                }
            }
        }
    }
    return NULL;
}

static const PDXERegisteredFunc *rust_registry_lookup_inherent_method(const PDXETypeRegistry *reg,
                                                                     const char *receiver_qn,
                                                                     const char *method_name) {
    if (!reg || !receiver_qn || !method_name) {
        return NULL;
    }
    int best_index = -1;
    PDXEMethodIter it;
    pdxe_registry_methods(reg, receiver_qn, method_name, &it);
    for (int index; (index = pdxe_method_iter_next(&it)) >= 0;) {
        const PDXERegisteredFunc *candidate = &reg->funcs[index];
        if ((candidate->flags & PDXE_FUNC_FLAG_RUST_TRAIT_IMPL) == 0 &&
            (best_index < 0 || index < best_index)) {
            best_index = index;
        }
    }
    if (best_index >= 0) {
        return &reg->funcs[best_index];
    }
    return reg->fallback
               ? rust_registry_lookup_inherent_method(reg->fallback, receiver_qn, method_name)
               : NULL;
}

static const PDXERegisteredFunc *rust_registry_lookup_inherent_method_aliased(
    const PDXETypeRegistry *reg, const char *receiver_qn, const char *method_name) {
    const PDXERegisteredFunc *method =
        rust_registry_lookup_inherent_method(reg, receiver_qn, method_name);
    if (method) {
        return method;
    }
    const PDXERegisteredType *type = pdxe_registry_lookup_type(reg, receiver_qn);
    for (int depth = 0; depth < 16 && type && type->alias_of; depth++) {
        method = rust_registry_lookup_inherent_method(reg, type->alias_of, method_name);
        if (method) {
            return method;
        }
        type = pdxe_registry_lookup_type(reg, type->alias_of);
    }
    return NULL;
}

/* Find the trait-impl method for one concrete receiver. With trait_qn==NULL,
 * exactly one implemented trait may contribute this method; two distinct
 * traits are ambiguous and deliberately resolve to NULL. */
static const PDXERegisteredFunc *rust_registry_lookup_trait_impl_method(const PDXETypeRegistry *reg,
                                                                       const char *receiver_qn,
                                                                       const char *trait_qn,
                                                                       const char *method_name,
                                                                       bool *out_ambiguous) {
    if (out_ambiguous) {
        *out_ambiguous = false;
    }
    if (!reg || !receiver_qn || !method_name) {
        return NULL;
    }
    int best_index = -1;
    const char *matched_trait = NULL;
    PDXEMethodIter it;
    pdxe_registry_methods(reg, receiver_qn, method_name, &it);
    for (int index; (index = pdxe_method_iter_next(&it)) >= 0;) {
        const PDXERegisteredFunc *candidate = &reg->funcs[index];
        if ((candidate->flags & PDXE_FUNC_FLAG_RUST_TRAIT_IMPL) == 0 || !candidate->impl_trait_qn) {
            continue;
        }
        if (trait_qn && strcmp(candidate->impl_trait_qn, trait_qn) != 0) {
            continue;
        }
        if (!trait_qn && matched_trait && strcmp(matched_trait, candidate->impl_trait_qn) != 0) {
            if (out_ambiguous) {
                *out_ambiguous = true;
            }
            return NULL;
        }
        matched_trait = candidate->impl_trait_qn;
        if (best_index < 0 || index < best_index) {
            best_index = index;
        }
    }
    if (best_index >= 0) {
        return &reg->funcs[best_index];
    }
    return reg->fallback ? rust_registry_lookup_trait_impl_method(
                               reg->fallback, receiver_qn, trait_qn, method_name, out_ambiguous)
                         : NULL;
}

static const PDXERegisteredFunc *rust_lookup_method_depth(RustLSPContext *ctx, const char *type_qn,
                                                         const char *member_name, int depth) {
    if (!type_qn || !member_name)
        return NULL;
    if (depth > PDXE_LSP_MAX_LOOKUP_DEPTH)
        return NULL;

    /* Direct inherent method lookup. */
    const PDXERegisteredFunc *f =
        rust_registry_lookup_inherent_method_aliased(ctx->registry, type_qn, member_name);
    if (f)
        return f;

    bool trait_ambiguous = false;
    f = rust_registry_lookup_trait_impl_method(ctx->registry, type_qn, NULL, member_name,
                                               &trait_ambiguous);
    if (f) {
        return f;
    }
    if (trait_ambiguous) {
        return NULL;
    }

    /* Follow alias / embedded chain. */
    const PDXERegisteredType *rt = pdxe_registry_lookup_type(ctx->registry, type_qn);
    if (rt) {
        if (rt->alias_of) {
            f = rust_lookup_method_depth(ctx, rt->alias_of, member_name, depth + 1);
            if (f)
                return f;
        }
        if (rt->embedded_types) {
            for (int i = 0; rt->embedded_types[i]; i++) {
                /* Skip the synthetic DerefTarget marker — handled by the
                 * caller in the receiver-walk loop. */
                if (strncmp(rt->embedded_types[i], "DerefTarget:", 12) == 0)
                    continue;
                /* Skip bound markers used for type-param trait bound
                 * recording — those live under "Bound:<TraitQN>". */
                if (strncmp(rt->embedded_types[i], "Bound:", 6) == 0) {
                    /* Dispatch through the bound trait. */
                    f = rust_lookup_method_depth(ctx, rt->embedded_types[i] + 6, member_name,
                                                 depth + 1);
                    if (f)
                        return f;
                    continue;
                }
                f = rust_lookup_method_depth(ctx, rt->embedded_types[i], member_name, depth + 1);
                if (f)
                    return f;
            }
        }
    }
    return NULL;
}

const PDXERegisteredFunc *rust_lookup_method(RustLSPContext *ctx, const char *type_qn,
                                            const char *member_name) {
    return rust_lookup_method_depth(ctx, type_qn, member_name, 0);
}

static const PDXERegisteredFunc *rust_lookup_method_in_trait(RustLSPContext *ctx,
                                                            const char *trait_qn,
                                                            const char *method_name,
                                                            bool allow_abstract) {
    if (!ctx || !trait_qn || !method_name)
        return NULL;
    const PDXETypeRegistry *reg = ctx->registry;
    int best_default = -1;
    int best_any = -1;
    PDXEMethodIter it;
    pdxe_registry_methods(reg, trait_qn, method_name, &it);
    for (int index; (index = pdxe_method_iter_next(&it)) >= 0;) {
        const PDXERegisteredFunc *candidate = &reg->funcs[index];
        if (candidate->flags & PDXE_FUNC_FLAG_RUST_TRAIT_IMPL) {
            continue;
        }
        if (best_any < 0 || index < best_any) {
            best_any = index;
        }
        if ((candidate->flags & PDXE_FUNC_FLAG_RUST_ABSTRACT) == 0 &&
            (best_default < 0 || index < best_default)) {
            best_default = index;
        }
    }
    int selected = best_default >= 0 ? best_default : (allow_abstract ? best_any : -1);
    if (selected >= 0) {
        return &reg->funcs[selected];
    }
    if (reg->fallback) {
        RustLSPContext fallback_ctx = *ctx;
        fallback_ctx.registry = reg->fallback;
        return rust_lookup_method_in_trait(&fallback_ctx, trait_qn, method_name, allow_abstract);
    }
    return NULL;
}

/* Resolve an ordinary method call with Rust precedence: inherent first,
 * concrete trait override second, then an unambiguous trait default. */
static const PDXERegisteredFunc *rust_resolve_trait_method(RustLSPContext *ctx,
                                                          const char *receiver_type_qn,
                                                          const char *method_name,
                                                          int *out_impl_count,
                                                          bool *out_trait_dispatch) {
    if (out_impl_count)
        *out_impl_count = 0;
    if (out_trait_dispatch)
        *out_trait_dispatch = false;
    if (!ctx || !receiver_type_qn || !method_name)
        return NULL;
    const PDXETypeRegistry *reg = ctx->registry;

    const PDXERegisteredType *receiver_type = pdxe_registry_lookup_type(reg, receiver_type_qn);
    if (receiver_type && receiver_type->is_interface) {
        const PDXERegisteredFunc *trait_method =
            rust_lookup_method_in_trait(ctx, receiver_type_qn, method_name, true);
        if (trait_method && out_impl_count) {
            *out_impl_count = 2; /* dynamic/bound trait dispatch */
        }
        if (trait_method && out_trait_dispatch) {
            *out_trait_dispatch = true;
        }
        return trait_method;
    }

    /* Ordinary concrete dispatch always prefers an inherent method.  Trait
     * implementations may share the exact same receiver/name/QN, so the
     * generic registry first-match lookup is not precise enough here. */
    const PDXERegisteredFunc *inh =
        rust_registry_lookup_inherent_method_aliased(reg, receiver_type_qn, method_name);
    if (inh) {
        if (out_impl_count)
            *out_impl_count = 1;
        return inh;
    }

    /* Negative memo (sealed registry only): this whole cascade — embedded-
     * impl walk + trait-default tail — reads nothing but the registry and the
     * two query strings, so under read_only a miss is a pure fact. Macro-
     * expanded kernel rust asks the same failing (receiver, method) question
     * thousands of times per file; without the memo each repeat re-paid the
     * full walk (~63 s per trait-heavy file). Only the full miss (0 impls,
     * no trait default) is memoized, preserving out_impl_count fidelity for
     * the ambiguous (>=2 impls) case. */
    bool nm_active = reg && reg->read_only;
    uint64_t nm_key = 0;
    if (nm_active) {
        nm_key = pdxe_negmemo_key(1, receiver_type_qn, method_name);
        if (pdxe_negmemo_contains(&ctx->neg_memo, nm_key)) {
            return NULL;
        }
    }

    bool trait_ambiguous = false;
    const PDXERegisteredFunc *impl_method = rust_registry_lookup_trait_impl_method(
        reg, receiver_type_qn, NULL, method_name, &trait_ambiguous);
    if (trait_ambiguous) {
        if (out_impl_count) {
            *out_impl_count = 2;
        }
        return NULL;
    }
    if (impl_method) {
        if (out_impl_count) {
            *out_impl_count = 1;
        }
        if (out_trait_dispatch) {
            *out_trait_dispatch = true;
        }
        return impl_method;
    }

    /* No concrete override. Resolve a default through the receiver's exact
     * type-level impl relations. Two traits contributing the same default
     * method are ambiguous without a source-level qualification/import proof. */
    const PDXERegisteredFunc *default_method = NULL;
    const char *default_trait = NULL;
    if (receiver_type && receiver_type->embedded_types) {
        for (int i = 0; receiver_type->embedded_types[i]; i++) {
            const char *trait_qn = receiver_type->embedded_types[i];
            if (strncmp(trait_qn, "DerefTarget:", 12) == 0 || strncmp(trait_qn, "Bound:", 6) == 0) {
                continue;
            }
            const PDXERegisteredType *trait_type = pdxe_registry_lookup_type(reg, trait_qn);
            if (!trait_type || !trait_type->is_interface) {
                continue;
            }
            const PDXERegisteredFunc *candidate =
                rust_lookup_method_in_trait(ctx, trait_qn, method_name, false);
            if (!candidate) {
                continue;
            }
            if (default_trait && strcmp(default_trait, trait_qn) != 0) {
                if (out_impl_count) {
                    *out_impl_count = 2;
                }
                return NULL;
            }
            default_trait = trait_qn;
            default_method = candidate;
        }
    }
    if (default_method) {
        if (out_impl_count) {
            *out_impl_count = 1;
        }
        if (out_trait_dispatch) {
            *out_trait_dispatch = true;
        }
        return default_method;
    }
    if (nm_active) {
        pdxe_negmemo_insert(&ctx->neg_memo, ctx->arena, nm_key);
    }
    return NULL;
}

// Find the sole concrete implementer of trait `trait_qn` that declares
// `method_name`, returning that impl's method (NULL if none or 2+), setting
// *out_n to the count (capped at 2). Used for `Trait::method` UFCS so it
// resolves to the concrete impl rather than the trait's own abstract method.
// Type-level impl links are canonical full QNs. Concrete lookup additionally
// requires the function's exact impl-trait provenance, so an inherent method
// with the same receiver/name can never satisfy this trait-specific path.
static const PDXERegisteredFunc *rust_find_sole_trait_impl(RustLSPContext *ctx, const char *trait_qn,
                                                          const char *method_name, int *out_n) {
    if (out_n)
        *out_n = 0;
    if (!ctx || !trait_qn || !method_name)
        return NULL;
    const PDXETypeRegistry *reg = ctx->registry;
    /* Negative memo (sealed registry only) — registry-pure cascade; only the
     * zero-implementer miss is memoized (out_n fidelity for the 2+ case). */
    bool nm_active = reg && reg->read_only;
    uint64_t nm_key = 0;
    if (nm_active) {
        nm_key = pdxe_negmemo_key(2, trait_qn, method_name);
        if (pdxe_negmemo_contains(&ctx->neg_memo, nm_key)) {
            return NULL;
        }
    }
    const PDXERegisteredFunc *first = NULL;
    const char *first_qn = NULL;
    int n = 0;
    const char *tdot = strrchr(trait_qn, '.');
    const char *tbare = tdot ? tdot + 1 : trait_qn;
    PDXETypeEmbedIter eit;
    pdxe_registry_types_by_embedded_bare(reg, tbare, &eit);
    for (int ti; n < 2 && (ti = pdxe_type_embed_iter_next(&eit)) >= 0;) {
        const PDXERegisteredType *t = &reg->types[ti];
        if (!t->embedded_types || !t->qualified_name)
            continue;
        bool impls = false;
        for (int j = 0; t->embedded_types[j]; j++) {
            const char *e = t->embedded_types[j];
            if (strcmp(e, trait_qn) == 0) {
                impls = true;
                break;
            }
        }
        if (!impls)
            continue;
        const PDXERegisteredFunc *mf = rust_registry_lookup_trait_impl_method(
            reg, t->qualified_name, trait_qn, method_name, NULL);
        if (!first_qn) {
            first = mf;
            first_qn = t->qualified_name;
            n = 1;
        } else if (strcmp(first_qn, t->qualified_name) != 0) {
            n = 2;
        }
    }
    if (out_n)
        *out_n = n;
    if (nm_active && n == 0) {
        pdxe_negmemo_insert(&ctx->neg_memo, ctx->arena, nm_key);
    }
    return n == 1 ? first : NULL;
}

/* ════════════════════════════════════════════════════════════════════
 * 8. Macro handling
 * ════════════════════════════════════════════════════════════════════ */

/* Walk a `macro_invocation`'s `token_tree` looking for nested call/method
 * call expressions. We do this so calls inside `vec![foo()]`,
 * `assert_eq!(a.bar(), 0)`, and `dbg!(get_value())` still get attributed
 * to the enclosing function. */
static void rust_walk_macro_tokens(RustLSPContext *ctx, TSNode node) {
    if (ts_node_is_null(node))
        return;
    const char *kind = ts_node_type(node);
    if (strcmp(kind, "call_expression") == 0 || strcmp(kind, "macro_invocation") == 0 ||
        strcmp(kind, "field_expression") == 0) {
        rust_resolve_calls_in_node(ctx, node);
        return;
    }
    PDXE_FOR_EACH_CHILD(it, node) {
        TSNode c = it.node;
        if (!ts_node_is_null(c))
            rust_walk_macro_tokens(ctx, c);
    }
}

/* Map a Rust infix/index operator token to the std::ops trait method that
 * the compiler desugars it to. `a + b` calls `Add::add`, `a[i]` calls
 * `Index::index`, etc. Returns NULL for operators with no overloadable
 * trait method (comparison/logical operators route through PartialEq /
 * PartialOrd whose methods we don't model here — sound to skip). */
static const char *rust_binop_trait_method(const char *op_text) {
    if (!op_text)
        return NULL;
    if (strcmp(op_text, "+") == 0)
        return "add";
    if (strcmp(op_text, "-") == 0)
        return "sub";
    if (strcmp(op_text, "*") == 0)
        return "mul";
    if (strcmp(op_text, "/") == 0)
        return "div";
    if (strcmp(op_text, "%") == 0)
        return "rem";
    if (strcmp(op_text, "&") == 0)
        return "bitand";
    if (strcmp(op_text, "|") == 0)
        return "bitor";
    if (strcmp(op_text, "^") == 0)
        return "bitxor";
    if (strcmp(op_text, "<<") == 0)
        return "shl";
    if (strcmp(op_text, ">>") == 0)
        return "shr";
    return NULL;
}

/* If `recv` is a user-defined NAMED type that defines operator method
 * `method` (via inherent impl or `impl <trait> for T`), emit a CALLS edge to
 * it. Models Rust operator-overload desugaring (`a + b` → T::add, `a[i]` →
 * T::index). Sound-only: we emit nothing when the operand type is unknown,
 * primitive, or the type has no such method registered — so we never guess on
 * built-in arithmetic. */
static void rust_emit_operator_call(RustLSPContext *ctx, const PDXEType *recv, const char *method,
                                    TSNode site) {
    if (!recv || !method)
        return;
    const PDXEType *base = recv;
    while (base && (base->kind == PDXE_TYPE_REFERENCE || base->kind == PDXE_TYPE_POINTER)) {
        base = (base->kind == PDXE_TYPE_REFERENCE) ? base->data.reference.elem
                                                  : base->data.pointer.elem;
    }
    /* Only user-defined named types — built-in arithmetic must not emit. */
    if (!base || base->kind != PDXE_TYPE_NAMED)
        return;
    const char *type_qn = base->data.named.qualified_name;
    if (!type_qn || is_rust_primitive(type_qn))
        return;
    int impl_count = 0;
    const PDXERegisteredFunc *m = rust_resolve_trait_method(ctx, type_qn, method, &impl_count, NULL);
    if (m && m->qualified_name) {
        uint32_t saved_start = ctx->emit_site_start_byte;
        uint32_t saved_end = ctx->emit_site_end_byte;
        uint32_t mapped_start = 0;
        uint32_t mapped_end = 0;
        bool mapped = !ts_node_is_null(site) &&
                      rust_map_source_range(ctx, ts_node_start_byte(site), ts_node_end_byte(site),
                                            &mapped_start, &mapped_end);
        ctx->emit_site_start_byte = mapped ? mapped_start : 0;
        ctx->emit_site_end_byte = mapped ? mapped_end : 0;
        rust_emit_resolved_call(ctx, m->qualified_name, "lsp_operator_trait",
                                PDXE_RUST_CONF_OPERATOR);
        /* `a + b` is a binary_expression, never a syntactic call node, so the
         * extractor produced no PDXECall to pair with the resolved_call above.
         * Inject one so the pipeline emits the CALLS edge. Inside a built-in
         * expression macro rust_emit_resolved_call already injects through
         * inject_syn_calls; outside it, inject explicitly. Generated
         * macro_rules! source has no faithful mapped site and stays semantic
         * only. */
        if (mapped && ctx->inject_syn_calls == 0)
            rust_inject_syn_call(ctx, m->qualified_name);
        ctx->emit_site_start_byte = saved_start;
        ctx->emit_site_end_byte = saved_end;
    }
}

/* ── User-defined macro_rules! support ────────────────────────────
 *
 * Strategy: collect every `macro_rules!` definition in the file
 * during the pre-walk, store each rule's transcriber text, and on
 * `macro_invocation` re-parse the transcriber as a synthetic Rust
 * function body so any calls inside the body are attributed to the
 * enclosing function of the invocation site.
 *
 * Recursion guard caps expansion depth at 8 (matches the rustc
 * default for macro recursion safety). */

typedef struct RustMacroRule {
    const char *macro_name;
    const char *pattern_text; /* left-hand side (without outer brackets) */
    int pattern_len;
    const char *transcriber_text;
    int transcriber_len;
    uint32_t definition_start_byte;
    uint32_t definition_end_byte;
    uint32_t scope_start_byte;
    uint32_t scope_end_byte;
    /* Issue 110: the transcriber's own token tree, read for the members an invocation
     * may write (pdxe_rust_export_macro_members), while the file's tree lives. */
    TSNode transcriber_node;
} RustMacroRule;

/* Strip a single outer pair of matching brackets from a token-tree
 * text representation. Returns the inner span via out_text/out_len.
 * If no brackets, returns the original. */
static void rust_macro_strip_outer(const char *tt, int len, const char **out_text, int *out_len) {
    if (len >= 2 && ((tt[0] == '{' && tt[len - 1] == '}') || (tt[0] == '(' && tt[len - 1] == ')') ||
                     (tt[0] == '[' && tt[len - 1] == ']'))) {
        *out_text = tt + 1;
        *out_len = len - 2;
    } else {
        *out_text = tt;
        *out_len = len;
    }
}

static void rust_record_macro_rule(RustLSPContext *ctx, const char *macro_name, TSNode pattern,
                                   TSNode transcriber, TSNode definition, TSNode scope) {
    if (!ctx || !macro_name || ts_node_is_null(transcriber))
        return;
    if (ctx->macro_rules_count >= ctx->macro_rules_cap) {
        /* Doubling (issue 92: growing by 16 copied the rules O(rules^2) times over). */
        int new_cap = ctx->macro_rules_cap ? ctx->macro_rules_cap * 2 : 16;
        struct RustMacroRule **narr = (struct RustMacroRule **)pdxe_arena_alloc(
            ctx->arena, new_cap * sizeof(struct RustMacroRule *));
        if (!narr)
            return;
        if (ctx->macro_rules_arr && ctx->macro_rules_count > 0) {
            memcpy(narr, ctx->macro_rules_arr,
                   ctx->macro_rules_count * sizeof(struct RustMacroRule *));
        }
        ctx->macro_rules_arr = narr;
        ctx->macro_rules_cap = new_cap;
    }
    RustMacroRule *r = (RustMacroRule *)pdxe_arena_alloc(ctx->arena, sizeof(*r));
    if (!r)
        return;
    memset(r, 0, sizeof(*r));
    r->macro_name = pdxe_arena_strdup(ctx->arena, macro_name);
    r->definition_start_byte = ts_node_start_byte(definition);
    r->definition_end_byte = ts_node_end_byte(definition);
    r->scope_start_byte = ts_node_start_byte(scope);
    r->scope_end_byte = ts_node_end_byte(scope);
    r->transcriber_node = transcriber;

    /* Cache pattern text for matching at invocation time. */
    if (!ts_node_is_null(pattern)) {
        char *pt = pdxe_node_text(ctx->arena, pattern, ctx->source);
        if (pt) {
            const char *inner;
            int inner_len;
            rust_macro_strip_outer(pt, (int)strlen(pt), &inner, &inner_len);
            r->pattern_text = pdxe_arena_strndup(ctx->arena, inner, (size_t)inner_len);
            r->pattern_len = inner_len;
        }
    }

    char *tt = pdxe_node_text(ctx->arena, transcriber, ctx->source);
    if (tt) {
        int len = (int)strlen(tt);
        const char *inner;
        int inner_len;
        rust_macro_strip_outer(tt, len, &inner, &inner_len);
        r->transcriber_text = pdxe_arena_strndup(ctx->arena, inner, (size_t)inner_len);
        r->transcriber_len = inner_len;
    }
    ctx->macro_rules_arr[ctx->macro_rules_count++] = r;
}

static void rust_collect_macro_rules(RustLSPContext *ctx, TSNode root) {
    if (ts_node_is_null(root))
        return;
    PDXE_FOR_EACH_CHILD(it, root) {
        TSNode c = it.node;
        if (ts_node_is_null(c))
            continue;
        const char *k = ts_node_type(c);
        if (strcmp(k, "macro_definition") == 0) {
            TSNode name_node = ts_node_child_by_field_name(c, "name", 4);
            if (ts_node_is_null(name_node))
                continue;
            char *macro_name = rust_node_text(ctx, name_node);
            if (!macro_name)
                continue;
            PDXE_FOR_EACH_CHILD(it2, c) {
                TSNode rule = it2.node;
                if (ts_node_is_null(rule) || !ts_node_is_named(rule))
                    continue;
                if (strcmp(ts_node_type(rule), "macro_rule") != 0)
                    continue;
                TSNode left = ts_node_child_by_field_name(rule, "left", 4);
                TSNode right = ts_node_child_by_field_name(rule, "right", 5);
                if (!ts_node_is_null(right)) {
                    rust_record_macro_rule(ctx, macro_name, left, right, c, root);
                }
            }
        } else if (strcmp(k, "mod_item") == 0) {
            TSNode body = ts_node_child_by_field_name(c, "body", 4);
            if (!ts_node_is_null(body))
                rust_collect_macro_rules(ctx, body);
        }
    }
}

/* macro_rules! uses textual scope: a definition is visible only after its
 * declaration, inside the containing module. A nested module may shadow an
 * outer definition; within one scope the latest preceding definition wins.
 * Every rule belonging to the selected definition shares the same source
 * coordinates, so expansion can still try its patterns in declaration order. */
static RustMacroRule *rust_visible_macro_definition(RustLSPContext *ctx, const char *macro_name,
                                                    TSNode invocation) {
    if (!ctx || !macro_name || !ctx->macro_rules_arr)
        return NULL;

    uint32_t position = 0;
    bool have_position = false;
    if (ctx->macro_origin_valid) {
        position = ctx->macro_origin_byte;
        have_position = true;
    } else if (ctx->source == ctx->root_source && !ts_node_is_null(invocation)) {
        position = ts_node_start_byte(invocation);
        have_position = true;
    }
    if (!have_position)
        return NULL;

    RustMacroRule *best = NULL;
    uint32_t best_scope_width = UINT32_MAX;
    /* Issue 92: the rules of this name, in recorded order, from the name index, where
     * every rule was scanned per invocation. When the index cannot grow no definition
     * is visible: lost work (the failed allocation is counted), never a scan. */
    bool scan = false;
#if defined(PDXE_ENABLE_TEST_SEAMS)
    const char *reference = getenv("PDX_ENGINE_TEST_LINEAR_LOOKUP_REFERENCE");
    scan = reference && reference[0];
#endif
    PDXENameIndex *by_name = &ctx->macro_rules_by_name;
    while (!scan && !by_name->failed && by_name->covered < ctx->macro_rules_count) {
        const RustMacroRule *placed = ctx->macro_rules_arr[by_name->covered];
        pdxe_name_index_place(by_name, ctx->arena, placed ? placed->macro_name : NULL);
    }
    for (int i = scan ? 0 : pdxe_name_index_first(by_name, macro_name);
         scan ? i < ctx->macro_rules_count : i >= 0;
         i = scan ? i + 1 : pdxe_name_index_next(by_name, i)) {
        PDXE_LOOKUP_COUNT(PDXE_LC_NAME_COMPARES);
        RustMacroRule *rule = ctx->macro_rules_arr[i];
        if (!rule || !rule->macro_name || strcmp(rule->macro_name, macro_name) != 0 ||
            position < rule->definition_end_byte || position < rule->scope_start_byte ||
            position > rule->scope_end_byte) {
            continue;
        }
        uint32_t scope_width = rule->scope_end_byte - rule->scope_start_byte;
        if (!best || scope_width < best_scope_width ||
            (scope_width == best_scope_width &&
             rule->definition_start_byte > best->definition_start_byte)) {
            best = rule;
            best_scope_width = scope_width;
        }
    }
    return best;
}

static bool rust_has_visible_macro_definition(RustLSPContext *ctx, const char *macro_name,
                                              TSNode invocation) {
    return rust_visible_macro_definition(ctx, macro_name, invocation) != NULL;
}

/* ── Metavar matching + substitution ─────────────────────────────
 *
 * Real `macro_rules!` semantics in a clean-room subset. We implement
 * the common shapes:
 *   $name:expr / $name:ty / $name:ident / $name:tt / $name:path /
 *   $name:pat / $name:literal / $name:block / $name:stmt
 *
 * And one form of repetition: `$(...)<sep><kind>` where kind is `*`,
 * `+`, or `?`. Repetitions inside repetitions are NOT supported.
 *
 * Substitution writes the bound values back into the transcriber and
 * re-parses the result so any calls inside are visible to the
 * resolver. */

#define RUST_MACRO_MAX_BINDINGS 32
#define RUST_MACRO_MAX_REPS 32

typedef struct {
    char name[32];     /* metavar name (without `$`) */
    const char *value; /* substring of the invocation args */
    int value_len;
} MacroBinding;

typedef struct {
    MacroBinding kv[RUST_MACRO_MAX_BINDINGS];
    int count;
    /* Repetition bindings: each metavar that appears inside `$(...)*`
     * collects an array of values (one per iteration). */
    struct {
        char name[32];
        const char **values; /* arena-allocated array of strings */
        int *lengths;
        int count;
    } reps[RUST_MACRO_MAX_REPS];
    int rep_count;
} MacroEnv;

/* Skip leading ASCII whitespace and Rust line comments. */
static int macro_skip_ws(const char *s, int len, int from) {
    while (from < len) {
        char c = s[from];
        if (c == ' ' || c == '\t' || c == '\n' || c == '\r') {
            from++;
            continue;
        }
        if (c == '/' && from + 1 < len && s[from + 1] == '/') {
            from += 2;
            while (from < len && s[from] != '\n')
                from++;
            continue;
        }
        break;
    }
    return from;
}

/* Consume an identifier (ASCII rust ident). Returns end position or
 * `from` if no identifier. */
static int macro_consume_ident(const char *s, int len, int from) {
    int p = from;
    if (p >= len)
        return from;
    char c = s[p];
    if (!(c == '_' || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')))
        return from;
    p++;
    while (p < len) {
        c = s[p];
        if (c == '_' || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
            (c >= '0' && c <= '9')) {
            p++;
        } else
            break;
    }
    return p;
}

/* Consume a balanced bracket group starting at `from` (which must
 * point at `(`, `[`, or `{`). Returns end position (just after the
 * close bracket) or `from` if not a bracket. */
static int macro_consume_balanced(const char *s, int len, int from) {
    if (from >= len)
        return from;
    char open = s[from];
    char close = 0;
    if (open == '(')
        close = ')';
    else if (open == '[')
        close = ']';
    else if (open == '{')
        close = '}';
    else
        return from;
    int depth = 1;
    int p = from + 1;
    while (p < len && depth > 0) {
        char c = s[p];
        if (c == '"') {
            /* Skip string literal. */
            p++;
            while (p < len && s[p] != '"') {
                if (s[p] == '\\' && p + 1 < len)
                    p += 2;
                else
                    p++;
            }
            if (p < len)
                p++;
            continue;
        }
        if (c == '\'') {
            /* Could be lifetime or char literal — accept char. */
            int q = p + 1;
            if (q < len && s[q] == '\\') {
                q++;
                if (q < len)
                    q++;
            } else if (q < len)
                q++;
            if (q < len && s[q] == '\'') {
                p = q + 1;
                continue;
            }
            /* otherwise treat as lifetime — skip until non-ident. */
            p = macro_consume_ident(s, len, p + 1);
            continue;
        }
        if (c == '(' || c == '[' || c == '{')
            depth++;
        else if (c == ')' || c == ']' || c == '}')
            depth--;
        p++;
    }
    return p;
}

/* Consume a fragment from the input matching the given fragment kind.
 * Returns end position or `from` if the consumption failed. */
static int macro_consume_fragment(const char *s, int len, int from, const char *frag) {
    from = macro_skip_ws(s, len, from);
    if (from >= len)
        return from;
    if (!frag)
        frag = "tt";

    if (strcmp(frag, "ident") == 0) {
        return macro_consume_ident(s, len, from);
    }
    if (strcmp(frag, "literal") == 0) {
        /* Numeric, string, char, bool literal. */
        char c = s[from];
        if (c == '"')
            return macro_consume_balanced(s, len, from);
        if (c == '\'') {
            int q = from + 1;
            if (q < len && s[q] == '\\') {
                q++;
                if (q < len)
                    q++;
            } else if (q < len)
                q++;
            if (q < len && s[q] == '\'')
                return q + 1;
            return from;
        }
        if (c == '-' || (c >= '0' && c <= '9')) {
            int p = from;
            if (c == '-')
                p++;
            while (p < len && ((s[p] >= '0' && s[p] <= '9') || s[p] == '_' || s[p] == '.' ||
                               s[p] == 'x' || s[p] == 'X' || s[p] == 'b' || s[p] == 'o' ||
                               (s[p] >= 'a' && s[p] <= 'f') || (s[p] >= 'A' && s[p] <= 'F')))
                p++;
            /* Allow type suffix. */
            int after = macro_consume_ident(s, len, p);
            return after;
        }
        return macro_consume_ident(s, len, from);
    }
    if (strcmp(frag, "tt") == 0) {
        char c = s[from];
        if (c == '(' || c == '[' || c == '{') {
            return macro_consume_balanced(s, len, from);
        }
        /* Single token: identifier, literal, or single-char punct. */
        if (c == '"' || c == '\'') {
            return macro_consume_fragment(s, len, from, "literal");
        }
        if (c == '_' || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
            (c >= '0' && c <= '9') || c == '-') {
            int p = macro_consume_ident(s, len, from);
            if (p > from)
                return p;
            return macro_consume_fragment(s, len, from, "literal");
        }
        return from + 1;
    }
    /* expr / ty / path / pat / stmt / block — balance brackets, stop
     * at top-level `,` or end of input. */
    if (strcmp(frag, "block") == 0) {
        if (s[from] == '{')
            return macro_consume_balanced(s, len, from);
        return from;
    }
    /* For expr/ty/path/pat/stmt: greedy balanced span. */
    int depth = 0;
    int p = from;
    while (p < len) {
        char c = s[p];
        if (c == '"' || c == '\'') {
            int q = macro_consume_fragment(s, len, p, "literal");
            if (q == p) {
                p++;
                continue;
            }
            p = q;
            continue;
        }
        if (c == '(' || c == '[' || c == '{') {
            int q = macro_consume_balanced(s, len, p);
            if (q == p)
                break;
            p = q;
            continue;
        }
        if (c == ')' || c == ']' || c == '}')
            break;
        if (depth == 0 && c == ',')
            break;
        if (strcmp(frag, "stmt") == 0 && c == ';')
            break;
        p++;
    }
    return p;
}

/* Bind a metavar in the env. Returns false if the table is full or the
 * name doesn't fit. */
static bool macro_env_bind(MacroEnv *env, const char *name, int name_len, const char *val,
                           int val_len) {
    if (env->count >= RUST_MACRO_MAX_BINDINGS)
        return false;
    if (name_len <= 0 || name_len >= 32)
        return false;
    MacroBinding *b = &env->kv[env->count++];
    memcpy(b->name, name, name_len);
    b->name[name_len] = '\0';
    b->value = val;
    b->value_len = val_len;
    return true;
}

static const MacroBinding *macro_env_lookup(const MacroEnv *env, const char *name) {
    for (int i = 0; i < env->count; i++) {
        if (strcmp(env->kv[i].name, name) == 0)
            return &env->kv[i];
    }
    return NULL;
}

/* Match a pattern against an input. Pattern fragments like `$x:expr`
 * bind into `env`. Returns true if the pattern matches the whole input
 * (or up to the end of pattern, with trailing whitespace in input). */
static bool macro_pattern_match(const char *pat, int pat_len, const char *in, int in_len,
                                MacroEnv *env) {
    int pp = 0; /* pattern pos */
    int ip = 0; /* input pos */

    while (pp < pat_len) {
        pp = macro_skip_ws(pat, pat_len, pp);
        ip = macro_skip_ws(in, in_len, ip);
        if (pp >= pat_len)
            break;

        char pc = pat[pp];

        /* Metavar: $name:frag or $name */
        if (pc == '$') {
            pp++;
            /* Repetition group `$( ... )<sep><kind>` — for now we
             * treat any rep group as a wildcard accepting the rest of
             * the input (best-effort). We bind no names inside reps. */
            pp = macro_skip_ws(pat, pat_len, pp);
            if (pp < pat_len && pat[pp] == '(') {
                int after = macro_consume_balanced(pat, pat_len, pp);
                pp = after;
                /* Skip optional separator + kind (one char each). */
                if (pp < pat_len && pat[pp] != ' ' && pat[pp] != '$' && pat[pp] != '\n') {
                    pp++;
                }
                if (pp < pat_len && (pat[pp] == '*' || pat[pp] == '+' || pat[pp] == '?')) {
                    pp++;
                }
                /* Consume the rest of the input greedy until we hit
                 * the next literal pattern char. */
                int next_lit = pp;
                while (next_lit < pat_len && (pat[next_lit] == ' ' || pat[next_lit] == '\t' ||
                                              pat[next_lit] == '\n' || pat[next_lit] == '\r')) {
                    next_lit++;
                }
                if (next_lit >= pat_len) {
                    ip = in_len;
                } else {
                    char target = pat[next_lit];
                    while (ip < in_len && in[ip] != target)
                        ip++;
                }
                continue;
            }
            /* Metavar name. */
            int name_start = pp;
            pp = macro_consume_ident(pat, pat_len, pp);
            int name_end = pp;
            if (name_end == name_start)
                return false;
            const char *frag = "tt";
            if (pp < pat_len && pat[pp] == ':') {
                pp++;
                int frag_start = pp;
                pp = macro_consume_ident(pat, pat_len, pp);
                /* Copy frag into a small buffer. */
                static char frag_buf[16];
                int fl = pp - frag_start;
                if (fl > 0 && fl < (int)sizeof(frag_buf)) {
                    memcpy(frag_buf, pat + frag_start, fl);
                    frag_buf[fl] = '\0';
                    frag = frag_buf;
                }
            }
            /* Consume the fragment from input. */
            int val_start = ip;
            int val_end = macro_consume_fragment(in, in_len, ip, frag);
            if (val_end <= val_start) {
                /* For "expr"/"ty" the matcher may need to accept empty
                 * input (e.g. trailing `$($x:expr),*` form). */
                return false;
            }
            ip = val_end;
            if (!macro_env_bind(env, pat + name_start, name_end - name_start, in + val_start,
                                val_end - val_start)) {
                return false;
            }
            continue;
        }

        /* Literal: must match input verbatim. */
        if (ip >= in_len)
            return false;
        char ic = in[ip];
        if (pc != ic)
            return false;
        pp++;
        ip++;
    }
    /* Allow trailing whitespace in input. */
    ip = macro_skip_ws(in, in_len, ip);
    return ip == in_len;
}

/* Substitute env bindings into the transcriber text. Allocates a
 * fresh string in the arena. */
static char *macro_substitute(PDXEArena *arena, const char *xs, int xs_len, const MacroEnv *env) {
    /* Estimate output size: each metavar expansion could be up to
     * ~256 bytes; bound the total at xs_len * 4 + 4KB. */
    int cap = xs_len * 4 + 4096;
    char *out = (char *)pdxe_arena_alloc(arena, cap + 1);
    if (!out)
        return NULL;
    int op = 0;
    int xp = 0;
    while (xp < xs_len) {
        char c = xs[xp];
        if (c == '$' && xp + 1 < xs_len) {
            /* Skip rep groups: `$(...)<sep><kind>` — naive expansion:
             * emit the body once, no separator handling. */
            if (xs[xp + 1] == '(') {
                int body_start = xp + 2;
                int after = macro_consume_balanced(xs, xs_len, xp + 1);
                int body_end = after - 1;
                /* Emit the body recursively substituted. */
                char *inner = macro_substitute(arena, xs + body_start, body_end - body_start, env);
                if (inner) {
                    int il = (int)strlen(inner);
                    if (op + il < cap) {
                        memcpy(out + op, inner, il);
                        op += il;
                    }
                }
                xp = after;
                /* Skip optional separator + kind. */
                if (xp < xs_len && xs[xp] != ' ' && xs[xp] != '\n')
                    xp++;
                if (xp < xs_len && (xs[xp] == '*' || xs[xp] == '+' || xs[xp] == '?')) {
                    xp++;
                }
                continue;
            }
            /* $name reference. */
            int name_start = xp + 1;
            int name_end = macro_consume_ident(xs, xs_len, name_start);
            if (name_end > name_start) {
                char name_buf[32];
                int nl = name_end - name_start;
                if (nl < 32) {
                    memcpy(name_buf, xs + name_start, nl);
                    name_buf[nl] = '\0';
                    const MacroBinding *b = macro_env_lookup(env, name_buf);
                    if (b && b->value && op + b->value_len < cap) {
                        memcpy(out + op, b->value, b->value_len);
                        op += b->value_len;
                        xp = name_end;
                        continue;
                    }
                }
            }
        }
        if (op < cap)
            out[op++] = c;
        xp++;
    }
    out[op] = '\0';
    return out;
}

/* Re-parse a user macro's transcriber body as a synthetic Rust
 * function and walk it for calls. */
extern const TSLanguage *tree_sitter_rust(void);
static void rust_expand_user_macro(RustLSPContext *ctx, const char *mname, TSNode invocation) {
    if (!ctx || !mname || !ctx->macro_rules_arr)
        return;
    if (ctx->macro_expand_depth >= 8)
        return;

    RustMacroRule *visible = rust_visible_macro_definition(ctx, mname, invocation);
    if (!visible)
        return;

    /* Try each rule from the lexically selected definition until one matches;
     * first matching rule wins. */
    RustMacroRule *hit = NULL;
    MacroEnv env;
    memset(&env, 0, sizeof(env));

    /* Extract the invocation argument text. */
    const char *inv_args = NULL;
    int inv_args_len = 0;
    if (!ts_node_is_null(invocation)) {
        TSNode args_tt = ts_node_child_by_field_name(invocation, "arguments", 9);
        if (!ts_node_is_null(args_tt)) {
            char *at = pdxe_node_text(ctx->arena, args_tt, ctx->source);
            if (at) {
                rust_macro_strip_outer(at, (int)strlen(at), &inv_args, &inv_args_len);
            }
        }
    }

    /* Issue 92: the definition's rules are among those of its name, in recorded order:
     * the name index rust_visible_macro_definition has just brought up to date. */
    bool scan = false;
#if defined(PDXE_ENABLE_TEST_SEAMS)
    const char *reference = getenv("PDX_ENGINE_TEST_LINEAR_LOOKUP_REFERENCE");
    scan = reference && reference[0];
#endif
    const PDXENameIndex *by_name = &ctx->macro_rules_by_name;
    for (int i = scan ? 0 : pdxe_name_index_first(by_name, mname);
         scan ? i < ctx->macro_rules_count : i >= 0;
         i = scan ? i + 1 : pdxe_name_index_next(by_name, i)) {
        PDXE_LOOKUP_COUNT(PDXE_LC_NAME_COMPARES);
        RustMacroRule *r = ctx->macro_rules_arr[i];
        if (strcmp(r->macro_name, mname) != 0 ||
            r->definition_start_byte != visible->definition_start_byte ||
            r->scope_start_byte != visible->scope_start_byte ||
            r->scope_end_byte != visible->scope_end_byte) {
            continue;
        }
        memset(&env, 0, sizeof(env));
        if (r->pattern_text && inv_args &&
            macro_pattern_match(r->pattern_text, r->pattern_len, inv_args, inv_args_len, &env)) {
            hit = r;
            break;
        }
        /* Fall back to first-rule match without arg substitution if
         * the pattern doesn't match — still expand to walk the body. */
        if (!hit)
            hit = r;
    }
    if (!hit || !hit->transcriber_text || hit->transcriber_len <= 0)
        return;

    /* Substitute the bound metavars into the transcriber body. */
    char *substituted =
        macro_substitute(ctx->arena, hit->transcriber_text, hit->transcriber_len, &env);
    if (!substituted)
        return;

    /* Expansion memo (see RustLSPContext.macro_memo): reset per top-level
     * invocation, dedup identical (macro, substituted body) within the
     * recursion chain — kills the exponential breadth of self-recursive
     * macro_rules without losing any distinctly-attributed source site. */
    if (ctx->macro_expand_depth == 0 && ctx->macro_memo.slots) {
        memset(ctx->macro_memo.slots, 0, sizeof(uint64_t) * (size_t)ctx->macro_memo.cap);
        ctx->macro_memo.count = 0;
    }
    uint64_t mm_key = pdxe_negmemo_key(3, mname, substituted);
    if (pdxe_negmemo_contains(&ctx->macro_memo, mm_key)) {
        return;
    }
    pdxe_negmemo_insert(&ctx->macro_memo, ctx->arena, mm_key);

    /* Wrap and parse. */
    char *wrapped = pdxe_arena_sprintf(ctx->arena, "fn __pdxe_macro_expand() { %s; }\n", substituted);
    if (!wrapped)
        return;

    TSParser *parser = ts_parser_new();
    if (!parser)
        return;
    ts_parser_set_language(parser, tree_sitter_rust());
    TSTree *tree = ts_parser_parse_string(parser, NULL, wrapped, (uint32_t)strlen(wrapped));
    if (tree) {
        ctx->macro_expand_depth++;
        TSNode root = ts_tree_root_node(tree);
        PDXE_FOR_EACH_CHILD(it, root) {
            TSNode top = it.node;
            if (ts_node_is_null(top))
                continue;
            if (strcmp(ts_node_type(top), "function_item") != 0)
                continue;
            TSNode body = ts_node_child_by_field_name(top, "body", 4);
            if (ts_node_is_null(body))
                continue;
            /* Swap source so node_text inside reads from `wrapped`. A
             * transcriber is generated source: even when this expansion was
             * reached through a mapped built-in-macro argument, its offsets
             * do not have an affine relationship to the invocation tokens. */
            const char *saved_source = ctx->source;
            int saved_len = ctx->source_len;
            bool saved_map_active = ctx->site_map_active;
            uint32_t saved_map_synthetic_start = ctx->site_map_synthetic_start_byte;
            uint32_t saved_map_synthetic_end = ctx->site_map_synthetic_end_byte;
            uint32_t saved_map_original_start = ctx->site_map_original_start_byte;
            uint32_t saved_emit_start = ctx->emit_site_start_byte;
            uint32_t saved_emit_end = ctx->emit_site_end_byte;
            ctx->source = wrapped;
            ctx->source_len = (int)strlen(wrapped);
            ctx->site_map_active = false;
            ctx->emit_site_start_byte = 0;
            ctx->emit_site_end_byte = 0;
            rust_resolve_calls_in_node(ctx, body);
            ctx->source = saved_source;
            ctx->source_len = saved_len;
            ctx->site_map_active = saved_map_active;
            ctx->site_map_synthetic_start_byte = saved_map_synthetic_start;
            ctx->site_map_synthetic_end_byte = saved_map_synthetic_end;
            ctx->site_map_original_start_byte = saved_map_original_start;
            ctx->emit_site_start_byte = saved_emit_start;
            ctx->emit_site_end_byte = saved_emit_end;
        }
        ctx->macro_expand_depth--;
        ts_tree_delete(tree);
    }
    ts_parser_delete(parser);
}

/* Re-parse the argument list of a built-in expression-macro (format!,
 * println!, assert!, …) as ordinary Rust expressions and walk them for
 * calls. The tree-sitter-rust grammar tokenises macro arguments rather than
 * parsing them as expression AST, so a method call like `format!("{}",
 * d.label())` never appears as a call_expression/field_expression node and
 * rust_walk_macro_tokens cannot recover it. Because these macros DO evaluate
 * their arguments as normal expressions, re-parsing the argument text and
 * resolving calls in it is sound — it recovers exactly the calls the program
 * makes. We wrap the args in a block so each comma-separated argument parses
 * as its own statement; the current scope (params/locals) is preserved so
 * typed receivers still resolve. */
static void rust_resolve_macro_arg_exprs(RustLSPContext *ctx, TSNode invocation) {
    if (!ctx || ts_node_is_null(invocation))
        return;
    if (ctx->macro_expand_depth >= 8)
        return;

    /* The grammar exposes the argument list as a `token_tree` child rather
     * than via an `arguments` field, so locate it by node type. */
    TSNode args_tt = {0};
    PDXE_FOR_EACH_CHILD(it, invocation) {
        TSNode c = it.node;
        if (!ts_node_is_null(c) && strcmp(ts_node_type(c), "token_tree") == 0) {
            args_tt = c;
            break;
        }
    }
    if (ts_node_is_null(args_tt))
        return;
    char *at = pdxe_node_text(ctx->arena, args_tt, ctx->source);
    if (!at)
        return;
    const char *inner;
    int inner_len;
    rust_macro_strip_outer(at, (int)strlen(at), &inner, &inner_len);
    if (inner_len <= 0)
        return;
    char *arg_text = pdxe_arena_strndup(ctx->arena, inner, (size_t)inner_len);
    if (!arg_text)
        return;

    /* Same expansion memo as rust_expand_user_macro (site tag 4), but ONLY
     * inside a recursion chain (depth > 0): there the identical argument text
     * is re-parsed on every re-expansion of a self-recursive macro and has no
     * distinct source site. Top-level invocations (depth 0) always parse —
     * identical args in different enclosing functions attribute differently
     * and must all be walked. */
    if (ctx->macro_expand_depth > 0) {
        uint64_t am_key = pdxe_negmemo_key(4, arg_text, NULL);
        if (pdxe_negmemo_contains(&ctx->macro_memo, am_key)) {
            return;
        }
        pdxe_negmemo_insert(&ctx->macro_memo, ctx->arena, am_key);
    }

    /* Wrap the comma-separated arguments in a tuple expression so the whole
     * thing parses as one valid expression (a trailing format-spec arg like
     * `width = w` would otherwise break statement parsing). Calls inside any
     * element are still walked. */
    static const char macro_arg_prefix[] = "fn __pdxe_macro_args() { let _ = (";
    char *wrapped = pdxe_arena_sprintf(ctx->arena, "%s%s); }\n", macro_arg_prefix, arg_text);
    if (!wrapped)
        return;

    TSParser *parser = ts_parser_new();
    if (!parser)
        return;
    ts_parser_set_language(parser, tree_sitter_rust());
    TSTree *tree = ts_parser_parse_string(parser, NULL, wrapped, (uint32_t)strlen(wrapped));
    if (tree) {
        TSNode root = ts_tree_root_node(tree);
        /* Bail out if the synthetic source failed to parse cleanly — a
         * format-string-only arg, named args, or other non-expression token
         * soup must not produce bogus edges. */
        if (!ts_node_has_error(root)) {
            ctx->macro_expand_depth++;
            PDXE_FOR_EACH_CHILD(it, root) {
                TSNode top = it.node;
                if (ts_node_is_null(top))
                    continue;
                if (strcmp(ts_node_type(top), "function_item") != 0)
                    continue;
                TSNode body = ts_node_child_by_field_name(top, "body", 4);
                if (ts_node_is_null(body))
                    continue;
                const char *saved_source = ctx->source;
                int saved_len = ctx->source_len;
                bool saved_map_active = ctx->site_map_active;
                uint32_t saved_map_synthetic_start = ctx->site_map_synthetic_start_byte;
                uint32_t saved_map_synthetic_end = ctx->site_map_synthetic_end_byte;
                uint32_t saved_map_original_start = ctx->site_map_original_start_byte;

                /* `arg_text` is a byte-for-byte copy of the token tree's inner
                 * text. Translate that inner range through any enclosing
                 * built-in-macro map before switching to the synthetic wrapper.
                 * If the current source is macro_rules!-generated, no faithful
                 * original occurrence exists and the new map stays disabled. */
                uint32_t inner_start = ts_node_start_byte(args_tt) + (uint32_t)(inner - at);
                uint32_t inner_end = inner_start + (uint32_t)inner_len;
                uint32_t original_start = 0;
                uint32_t original_end = 0;
                bool mapped = rust_map_source_range(ctx, inner_start, inner_end, &original_start,
                                                    &original_end);
                ctx->source = wrapped;
                ctx->source_len = (int)strlen(wrapped);
                ctx->site_map_active = mapped;
                if (mapped) {
                    ctx->site_map_synthetic_start_byte = (uint32_t)strlen(macro_arg_prefix);
                    ctx->site_map_synthetic_end_byte =
                        ctx->site_map_synthetic_start_byte + (uint32_t)inner_len;
                    ctx->site_map_original_start_byte = original_start;
                }
                /* The syntactic extractor never produced call nodes for these
                 * macro-hidden expressions, so resolved_calls emitted here have
                 * no PDXECall to pair with. Inject matching synthetic calls. */
                ctx->inject_syn_calls++;
                rust_resolve_calls_in_node(ctx, body);
                ctx->inject_syn_calls--;
                ctx->source = saved_source;
                ctx->source_len = saved_len;
                ctx->site_map_active = saved_map_active;
                ctx->site_map_synthetic_start_byte = saved_map_synthetic_start;
                ctx->site_map_synthetic_end_byte = saved_map_synthetic_end;
                ctx->site_map_original_start_byte = saved_map_original_start;
            }
            ctx->macro_expand_depth--;
        }
        ts_tree_delete(tree);
    }
    ts_parser_delete(parser);
}

/* ════════════════════════════════════════════════════════════════════
 * 9. Statement / pattern binding
 * ════════════════════════════════════════════════════════════════════ */

/* Recursively bind every identifier inside a pattern node to the given
 * fallback type. Handles tuple_pattern, struct_pattern, tuple_struct_pattern,
 * ref_pattern, mut_pattern, captured_pattern, identifier. */
static void rust_bind_pattern(RustLSPContext *ctx, TSNode pattern, const PDXEType *type) {
    if (ts_node_is_null(pattern))
        return;
    const char *kind = ts_node_type(pattern);

    if (strcmp(kind, "identifier") == 0) {
        char *name = rust_node_text(ctx, pattern);
        if (name && strcmp(name, "_") != 0) {
            pdxe_scope_bind(ctx->current_scope, name, type);
        }
        return;
    }
    if (strcmp(kind, "captured_pattern") == 0) {
        /* name @ subpattern */
        TSNode name_node = ts_node_child_by_field_name(pattern, "name", 4);
        if (!ts_node_is_null(name_node)) {
            char *name = rust_node_text(ctx, name_node);
            if (name && strcmp(name, "_") != 0) {
                pdxe_scope_bind(ctx->current_scope, name, type);
            }
        }
        TSNode sub = ts_node_child_by_field_name(pattern, "pattern", 7);
        if (!ts_node_is_null(sub))
            rust_bind_pattern(ctx, sub, type);
        return;
    }
    if (strcmp(kind, "ref_pattern") == 0 || strcmp(kind, "mut_pattern") == 0 ||
        strcmp(kind, "reference_pattern") == 0) {
        if (ts_node_named_child_count(pattern) > 0) {
            rust_bind_pattern(ctx, ts_node_named_child(pattern, 0), type);
        }
        return;
    }
    if (strcmp(kind, "tuple_pattern") == 0) {
        const PDXEType *base = type;
        while (base && base->kind == PDXE_TYPE_REFERENCE)
            base = base->data.reference.elem;
        PDXE_FOR_EACH_NAMED_CHILD(it, pattern) {
            const PDXEType *elem_t = pdxe_type_unknown();
            if (base && base->kind == PDXE_TYPE_TUPLE && (int)it.index < base->data.tuple.count) {
                elem_t = base->data.tuple.elems[it.index];
            }
            rust_bind_pattern(ctx, it.node, elem_t);
        }
        return;
    }
    if (strcmp(kind, "tuple_struct_pattern") == 0) {
        /* Some(x), Ok(x), Err(e) — peel one Option/Result/template. */
        const PDXEType *base = type;
        while (base && base->kind == PDXE_TYPE_REFERENCE)
            base = base->data.reference.elem;
        const PDXEType *inner = pdxe_type_unknown();
        if (base && base->kind == PDXE_TYPE_TEMPLATE && base->data.template_type.arg_count > 0) {
            inner = base->data.template_type.template_args[0];
        }
        /* First named child is the path; subsequent are sub-patterns. */
        PDXE_FOR_EACH_NAMED_CHILD(it, pattern) {
            if (it.index >= 1) {
                rust_bind_pattern(ctx, it.node, inner);
            }
        }
        return;
    }
    if (strcmp(kind, "struct_pattern") == 0) {
        /* For each field_pattern, bind the local name to the field's type. */
        TSNode body = ts_node_child_by_field_name(pattern, "body", 4);
        TSNode iter = ts_node_is_null(body) ? pattern : body;
        const PDXEType *base = type;
        while (base && base->kind == PDXE_TYPE_REFERENCE)
            base = base->data.reference.elem;
        const char *type_qn = NULL;
        if (base && base->kind == PDXE_TYPE_NAMED)
            type_qn = base->data.named.qualified_name;
        else if (base && base->kind == PDXE_TYPE_TEMPLATE)
            type_qn = base->data.template_type.template_name;

        PDXE_FOR_EACH_NAMED_CHILD(it, iter) {
            TSNode fp = it.node;
            const char *fk = ts_node_type(fp);
            if (strcmp(fk, "field_pattern") == 0) {
                TSNode name_node = ts_node_child_by_field_name(fp, "name", 4);
                TSNode pat_node = ts_node_child_by_field_name(fp, "pattern", 7);
                char *fname = rust_node_text(ctx, name_node);
                if (!fname)
                    continue;
                const PDXEType *ft =
                    type_qn ? rust_lookup_field(ctx, type_qn, fname, 0) : pdxe_type_unknown();
                if (!ft)
                    ft = pdxe_type_unknown();
                if (!ts_node_is_null(pat_node)) {
                    rust_bind_pattern(ctx, pat_node, ft);
                } else {
                    pdxe_scope_bind(ctx->current_scope, fname, ft);
                }
            } else if (strcmp(fk, "shorthand_field_identifier") == 0 ||
                       strcmp(fk, "identifier") == 0) {
                char *fname = rust_node_text(ctx, fp);
                if (fname && strcmp(fname, "_") != 0) {
                    const PDXEType *ft =
                        type_qn ? rust_lookup_field(ctx, type_qn, fname, 0) : pdxe_type_unknown();
                    pdxe_scope_bind(ctx->current_scope, fname, ft ? ft : pdxe_type_unknown());
                }
            }
        }
        return;
    }
    if (strcmp(kind, "or_pattern") == 0) {
        /* For an OR pattern we attempt to bind names from the first branch. */
        if (ts_node_named_child_count(pattern) > 0) {
            rust_bind_pattern(ctx, ts_node_named_child(pattern, 0), type);
        }
        return;
    }
    /* Other patterns we ignore for binding purposes. */
}

static const char *rust_resolve_callable_value(RustLSPContext *ctx, TSNode value,
                                               const char **source_name);

void rust_process_statement(RustLSPContext *ctx, TSNode node) {
    if (ts_node_is_null(node))
        return;
    const char *kind = ts_node_type(node);

    /* let_declaration: let pat: T = expr;
     *
     * Bidirectional inference: if the user wrote `let v: Vec<String> = …;`
     * we pass `Vec<String>` as the expected hint when synthesising the
     * RHS. That lets ambiguous calls like `Vec::new()` keep their full
     * template arguments through the chain. */
    if (strcmp(kind, "let_declaration") == 0) {
        TSNode pat = ts_node_child_by_field_name(node, "pattern", 7);
        TSNode tn = ts_node_child_by_field_name(node, "type", 4);
        TSNode val = ts_node_child_by_field_name(node, "value", 5);

        const PDXEType *annotated = NULL;
        if (!ts_node_is_null(tn)) {
            annotated = rust_parse_type_node(ctx, tn);
        }
        const PDXEType *let_type = annotated;
        if ((!let_type || pdxe_type_is_unknown(let_type)) && !ts_node_is_null(val)) {
            /* Synthesis: evaluate the RHS with the (possibly NULL)
             * annotated type as a hint. */
            let_type = rust_eval_expr_typed(ctx, val, annotated);
        }
        if (!let_type)
            let_type = pdxe_type_unknown();
        if (!ts_node_is_null(pat))
            rust_bind_pattern(ctx, pat, let_type);
        if (!ts_node_is_null(pat) && strcmp(ts_node_type(pat), "identifier") == 0 &&
            !ts_node_is_null(val)) {
            char *binding_name = rust_node_text(ctx, pat);
            const char *value_kind = ts_node_type(val);
            const char *callable_qn = NULL;
            if (strcmp(value_kind, "identifier") == 0) {
                char *value_name = rust_node_text(ctx, val);
                if (value_name && pdxe_scope_contains(ctx->current_scope, value_name)) {
                    callable_qn = pdxe_scope_lookup_callable(ctx->current_scope, value_name);
                } else if (value_name) {
                    const char *resolved = rust_resolve_path_expr(ctx, value_name);
                    const PDXERegisteredFunc *function =
                        resolved ? pdxe_registry_lookup_func(ctx->registry, resolved) : NULL;
                    if (!function && resolved && ctx->module_qn) {
                        function = pdxe_registry_lookup_func(
                            ctx->registry,
                            pdxe_arena_sprintf(ctx->arena, "%s.%s", ctx->module_qn, resolved));
                    }
                    callable_qn = function ? function->qualified_name : NULL;
                }
            } else if (strcmp(value_kind, "scoped_identifier") == 0) {
                char *value_path = rust_node_text(ctx, val);
                const char *resolved = value_path ? rust_resolve_path_expr(ctx, value_path) : NULL;
                const PDXERegisteredFunc *function =
                    resolved ? pdxe_registry_lookup_func(ctx->registry, resolved) : NULL;
                callable_qn = function ? function->qualified_name : NULL;
            }
            if (binding_name && callable_qn) {
                pdxe_scope_bind_callable(ctx->current_scope, binding_name, let_type, callable_qn);
            }
        }
        return;
    }

    if (strcmp(kind, "assignment_expression") == 0 ||
        strcmp(kind, "compound_assignment_expr") == 0) {
        TSNode left = ts_node_child_by_field_name(node, "left", 4);
        TSNode right = ts_node_child_by_field_name(node, "right", 5);
        if (!ts_node_is_null(left) && strcmp(ts_node_type(left), "identifier") == 0) {
            char *name = rust_node_text(ctx, left);
            if (name && pdxe_scope_contains(ctx->current_scope, name)) {
                const char *target = NULL;
                if (strcmp(kind, "assignment_expression") == 0 &&
                    ctx->callable_control_flow_depth == 0 && !ts_node_is_null(right)) {
                    target = rust_resolve_callable_value(ctx, right, NULL);
                }
                pdxe_scope_update_callable(ctx->current_scope, name, target);
            }
        }
        return;
    }

    /* const_item / static_item: const NAME: T = …; */
    if (strcmp(kind, "const_item") == 0 || strcmp(kind, "static_item") == 0) {
        TSNode name_node = ts_node_child_by_field_name(node, "name", 4);
        TSNode tn = ts_node_child_by_field_name(node, "type", 4);
        const PDXEType *type =
            ts_node_is_null(tn) ? pdxe_type_unknown() : rust_parse_type_node(ctx, tn);
        if (!ts_node_is_null(name_node)) {
            char *name = rust_node_text(ctx, name_node);
            if (name)
                pdxe_scope_bind(ctx->current_scope, name, type);
        }
        return;
    }
}

/* ════════════════════════════════════════════════════════════════════
 * 10. Function & file walk
 * ════════════════════════════════════════════════════════════════════ */

/* Inject a synthetic PDXECall into result->calls so the downstream pipeline
 * (pdxe_pipeline_find_lsp_resolution) can pair it with the resolved_call and
 * emit a CALLS edge. `callee_qn`'s last dot segment is used as the textual
 * callee_name, matching how the resolver's short-name comparison works. Only
 * used for calls the syntactic extractor cannot see (operator desugaring,
 * macro-hidden method calls). */
static bool rust_map_source_range(const RustLSPContext *ctx, uint32_t start, uint32_t end,
                                  uint32_t *mapped_start, uint32_t *mapped_end) {
    if (!ctx || !mapped_start || !mapped_end || end <= start)
        return false;
    if (ctx->site_map_active) {
        if (start < ctx->site_map_synthetic_start_byte || end > ctx->site_map_synthetic_end_byte) {
            return false;
        }
        *mapped_start =
            ctx->site_map_original_start_byte + (start - ctx->site_map_synthetic_start_byte);
        *mapped_end =
            ctx->site_map_original_start_byte + (end - ctx->site_map_synthetic_start_byte);
        return *mapped_end > *mapped_start;
    }
    /* Only the immutable root buffer has identity coordinates. A
     * macro_rules! transcriber is reparsed from substituted generated text and
     * cannot be mapped by a single offset. */
    if (ctx->source != ctx->root_source)
        return false;
    *mapped_start = start;
    *mapped_end = end;
    return true;
}

static void rust_inject_syn_call(RustLSPContext *ctx, const char *callee_qn) {
    if (!ctx || !ctx->syn_calls || !callee_qn || !ctx->enclosing_func_qn)
        return;
    const char *dot = strrchr(callee_qn, '.');
    const char *short_name = dot ? dot + 1 : callee_qn;
    if (!short_name || !short_name[0])
        return;
    PDXECall call = {0};
    call.callee_name = pdxe_arena_strdup(ctx->arena, short_name);
    call.enclosing_func_qn = ctx->enclosing_func_qn;
    if (ctx->emit_site_end_byte > ctx->emit_site_start_byte) {
        call.site_start_byte = ctx->emit_site_start_byte;
        call.site_end_byte = ctx->emit_site_end_byte;
        call.requires_lsp_resolution = true;
    }
    /* Issue 114: in the `use` scope the typed pass answered it in. */
    call.use_scope = ctx->use_scope_count > 0 ? ctx->use_scope + 1 : 0;
    pdxe_calls_push(ctx->syn_calls, ctx->arena, call);
}

/* The generic call extractor intentionally filters Rust built-in macro names
 * that live in its keyword table (`println`, `panic`, ...). Once the LSP has
 * proven a canonical macro target, restore exactly one source-occurrence
 * carrier and require that semantic result: a same-named local function must
 * never win through the legacy fallback. Non-keyword macros already have a
 * parser carrier, so leave them alone to avoid duplicate cross-pass carriers. */
static void rust_ensure_known_macro_carrier(RustLSPContext *ctx, const char *macro_name,
                                            const char *callee_qn) {
    if (!ctx || !ctx->syn_calls || !macro_name || !callee_qn ||
        !pdxe_is_keyword(macro_name, PDXE_LANG_RUST) ||
        ctx->emit_site_end_byte <= ctx->emit_site_start_byte) {
        return;
    }

    const char *dot = strrchr(callee_qn, '.');
    const char *short_name = dot ? dot + 1 : callee_qn;
    /* Issue 92: the calls at the emission's site, from the site index, where every
     * call was scanned per emission. When the index cannot grow, the carrier is
     * neither found nor restored: lost work (the failed allocation is counted). */
    bool scan = false;
#if defined(PDXE_ENABLE_TEST_SEAMS)
    const char *reference = getenv("PDX_ENGINE_TEST_LINEAR_LOOKUP_REFERENCE");
    scan = reference && reference[0];
#endif
    PDXESiteIndex *sites = &ctx->syn_sites;
    while (!scan && !sites->failed && sites->covered < ctx->syn_calls->count) {
        const PDXECall *call = &ctx->syn_calls->items[sites->covered];
        pdxe_site_index_place(sites, ctx->arena, call->site_start_byte, call->site_end_byte);
    }
    if (!scan && sites->failed) {
        return;
    }
    for (int i = scan ? 0 : pdxe_site_index_first(sites, ctx->emit_site_start_byte,
                                                  ctx->emit_site_end_byte);
         scan ? i < ctx->syn_calls->count : i >= 0;
         i = scan ? i + 1 : pdxe_site_index_next(sites, i)) {
        PDXECall *call = &ctx->syn_calls->items[i];
        PDXE_LOOKUP_COUNT(PDXE_LC_ANSWER_ROWS);
        if (!call->callee_name || !call->enclosing_func_qn ||
            strcmp(call->enclosing_func_qn, ctx->enclosing_func_qn) != 0 ||
            (strcmp(call->callee_name, macro_name) != 0 &&
             strcmp(call->callee_name, short_name) != 0) ||
            call->site_start_byte != ctx->emit_site_start_byte ||
            call->site_end_byte != ctx->emit_site_end_byte) {
            continue;
        }
        call->requires_lsp_resolution = true;
        return;
    }
    rust_inject_syn_call(ctx, callee_qn);
}

static void rust_emit_resolved_call_reason(RustLSPContext *ctx, const char *callee_qn,
                                           const char *strategy, float confidence,
                                           const char *reason) {
    if (!ctx || !ctx->resolved_calls || !callee_qn || !ctx->enclosing_func_qn)
        return;
    PDXEResolvedCall rc = {
        .caller_qn = ctx->enclosing_func_qn,
        .callee_qn = callee_qn,
        .strategy = strategy,
        .confidence = confidence,
        .reason = reason,
        .kind = PDXE_RESOLVED_INVOCATION,
        .site_start_byte = ctx->emit_site_start_byte,
        .site_end_byte = ctx->emit_site_end_byte,
    };
    pdxe_resolvedcall_push(ctx->resolved_calls, ctx->arena, rc);
    if (ctx->inject_syn_calls > 0 && ctx->emit_site_end_byte > ctx->emit_site_start_byte) {
        rust_inject_syn_call(ctx, callee_qn);
    }
}

static void rust_emit_resolved_call(RustLSPContext *ctx, const char *callee_qn,
                                    const char *strategy, float confidence) {
    rust_emit_resolved_call_reason(ctx, callee_qn, strategy, confidence, NULL);
}

static bool rust_qn_has_suffix(const char *qualified_name, const char *suffix) {
    if (!qualified_name || !suffix) {
        return false;
    }
    size_t qn_len = strlen(qualified_name);
    size_t suffix_len = strlen(suffix);
    return qn_len >= suffix_len && strcmp(qualified_name + qn_len - suffix_len, suffix) == 0 &&
           (qn_len == suffix_len || qualified_name[qn_len - suffix_len - 1U] == '.');
}

/* Resolve only an exact identifier/scoped-path value. No branch, closure, or
 * other composite expression enters this helper. A suffix fallback is admitted
 * only for a qualified import path and only when it identifies one free func. */
static const char *rust_resolve_callable_value(RustLSPContext *ctx, TSNode value,
                                               const char **source_name) {
    const char *kind = ts_node_type(value);
    if (strcmp(kind, "identifier") != 0 && strcmp(kind, "scoped_identifier") != 0) {
        return NULL;
    }
    char *path = rust_node_text(ctx, value);
    if (!path || !path[0]) {
        return NULL;
    }
    if (source_name) {
        *source_name = path;
    }
    if (strcmp(kind, "identifier") == 0 && pdxe_scope_contains(ctx->current_scope, path)) {
        return pdxe_scope_lookup_callable(ctx->current_scope, path);
    }
    const char *resolved = rust_resolve_path_expr(ctx, path);
    if (!resolved) {
        return NULL;
    }
    const PDXERegisteredFunc *function = pdxe_registry_lookup_func(ctx->registry, resolved);
    if (!function && ctx->module_qn) {
        function = pdxe_registry_lookup_func(
            ctx->registry, pdxe_arena_sprintf(ctx->arena, "%s.%s", ctx->module_qn, resolved));
    }
    if (function && !function->receiver_type) {
        return function->qualified_name;
    }
    if (!strchr(resolved, '.')) {
        return NULL;
    }
    const char *tail = strrchr(resolved, '.');
    tail = tail ? tail + 1 : resolved;
    const PDXERegisteredFunc *unique = NULL;
    int matches = 0;
    PDXEFreeFuncIter iter;
    pdxe_registry_free_funcs_by_short_name(ctx->registry, tail, &iter);
    for (int index; matches < 2 && (index = pdxe_free_func_iter_next(&iter)) >= 0;) {
        const PDXERegisteredFunc *candidate = &ctx->registry->funcs[index];
        if (!candidate->receiver_type && candidate->qualified_name &&
            rust_qn_has_suffix(candidate->qualified_name, resolved)) {
            unique = candidate;
            matches++;
        }
    }
    return matches == 1 && unique ? unique->qualified_name : NULL;
}

static void rust_resolve_callable_argument_references(RustLSPContext *ctx, TSNode arguments) {
    if (ts_node_is_null(arguments)) {
        return;
    }
    PDXE_FOR_EACH_NAMED_CHILD(it, arguments) {
        TSNode value = it.node;
        const char *source_name = NULL;
        const char *target = rust_resolve_callable_value(ctx, value, &source_name);
        if (!target) {
            continue;
        }
        uint32_t start = 0;
        uint32_t end = 0;
        if (!rust_map_source_range(ctx, ts_node_start_byte(value), ts_node_end_byte(value), &start,
                                   &end)) {
            continue;
        }
        PDXEResolvedCall reference = {
            .caller_qn = ctx->enclosing_func_qn,
            .callee_qn = target,
            .strategy = "lsp_callable_value_reference",
            .confidence = PDXE_RUST_CONF_DIRECT,
            .reason = source_name,
            .kind = PDXE_RESOLVED_CALL_REFERENCE,
            .site_start_byte = start,
            .site_end_byte = end,
        };
        pdxe_resolvedcall_push(ctx->resolved_calls, ctx->arena, reference);
    }
}

static void rust_emit_unresolved_call(RustLSPContext *ctx, const char *expr_text,
                                      const char *reason) {
    if (!ctx || !ctx->resolved_calls || !ctx->enclosing_func_qn)
        return;
    PDXEResolvedCall rc = {
        .caller_qn = ctx->enclosing_func_qn,
        .callee_qn = expr_text ? expr_text : "?",
        .strategy = "lsp_unresolved",
        .confidence = 0.0f,
        .reason = reason,
        .kind = PDXE_RESOLVED_INVOCATION,
        .site_start_byte = ctx->emit_site_start_byte,
        .site_end_byte = ctx->emit_site_end_byte,
    };
    pdxe_resolvedcall_push(ctx->resolved_calls, ctx->arena, rc);
}

/* Issue 104: a qualified path's qualifier names where its function is.
 *
 * The segments of a dotted module path as a Rust path spells them: a `src`
 * directory, a `mod.rs` file's `mod` and a crate root's `lib` or `main` are left
 * out, since no path names them. Returns the count; `out` holds the segments. */
static int rust_path_segments(PDXEArena *arena, const char *dotted, size_t len, char ***out) {
    int cap = 8, n = 0;
    char **segs = (char **)pdxe_arena_alloc(arena, (size_t)cap * sizeof(char *));
    size_t start = 0;
    for (size_t i = 0; segs && i <= len; i++) {
        if (i < len && dotted[i] != '.')
            continue;
        if (i > start) {
            if (n == cap) {
                char **grown = (char **)pdxe_arena_alloc(arena, (size_t)cap * 2 * sizeof(char *));
                if (!grown)
                    return -1;
                memcpy(grown, segs, (size_t)n * sizeof(char *));
                segs = grown;
                cap *= 2;
            }
            segs[n++] = pdxe_arena_strndup(arena, dotted + start, i - start);
        }
        start = i + 1;
    }
    if (!segs)
        return -1;
    /* A file module: its file segment is the module itself. */
    if (n > 0 && (strcmp(segs[n - 1], "mod") == 0 || strcmp(segs[n - 1], "lib") == 0 ||
                  strcmp(segs[n - 1], "main") == 0)) {
        n--;
    }
    int kept = 0;
    for (int i = 0; i < n; i++) {
        if (strcmp(segs[i], "src") != 0) {
            segs[kept++] = segs[i];
        }
    }
    *out = segs;
    return kept;
}

/* Whether `module` (path segments, `src` left out) ends with `want[0..wn)`, or its
 * parent does: a module re-exports what its child modules define
 * (`middleware::from_fn` for `middleware/from_fn.rs`). */
static bool rust_module_ends_with(char **module, int mn, char **want, int wn) {
    for (int drop = 0; drop <= 1; drop++) {
        int m = mn - drop;
        if (m < wn || wn <= 0)
            continue;
        bool same = true;
        for (int k = 0; k < wn && same; k++) {
            same = strcmp(module[m - wn + k], want[k]) == 0;
        }
        if (same)
            return true;
    }
    return false;
}

/* The segments of a Rust path (`a::b::c`), each without the white space a
 * `use` list spread over lines leaves around it. Returns the count, or -1. */
static int rust_split_path(PDXEArena *arena, const char *path, char ***out) {
    int cap = 8, n = 0;
    char **segs = (char **)pdxe_arena_alloc(arena, (size_t)cap * sizeof(char *));
    const char *p = path;
    while (segs && p) {
        const char *sep = strstr(p, "::");
        const char *s = p;
        const char *e = sep ? sep : p + strlen(p);
        while (s < e && isspace((unsigned char)*s))
            s++;
        while (e > s && isspace((unsigned char)e[-1]))
            e--;
        if (n == cap) {
            char **grown = (char **)pdxe_arena_alloc(arena, (size_t)cap * 2 * sizeof(char *));
            if (!grown)
                return -1;
            memcpy(grown, segs, (size_t)n * sizeof(char *));
            segs = grown;
            cap *= 2;
        }
        segs[n++] = pdxe_arena_strndup(arena, s, (size_t)(e - s));
        p = sep ? sep + 2 : NULL;
    }
    if (!segs)
        return -1;
    *out = segs;
    return n;
}

static bool rust_path_anchor(const char *segment) {
    return strcmp(segment, "crate") == 0 || strcmp(segment, "self") == 0 ||
           strcmp(segment, "super") == 0;
}

/* Two crate names are the same crate's when they differ only in `-` and `_`:
 * a package `pdx-core` is the crate `pdx_core` in a path. */
static bool rust_same_crate_name(const char *a, size_t alen, const char *b) {
    size_t blen = strlen(b);
    if (alen != blen)
        return false;
    for (size_t i = 0; i < alen; i++) {
        char x = a[i] == '-' ? '_' : a[i];
        char y = b[i] == '-' ? '_' : b[i];
        if (x != y)
            return false;
    }
    return true;
}

/* The length, in rust_path_segments' segments, of the root of the crate a path
 * names by `name`, as the function's module path `dotted[0..len)` places it: the
 * directory of that name with the crate's `src` directory in it, or the
 * repository's own package with `src` at the top. -1 when the function is not
 * in a crate of that name. */
static int rust_crate_root(RustLSPContext *ctx, const char *dotted, size_t len, const char *name) {
    int kept = 0;
    size_t start = 0;
    const char *prev = NULL;
    size_t prev_len = 0;
    for (size_t i = 0; i <= len; i++) {
        if (i < len && dotted[i] != '.')
            continue;
        const char *seg = dotted + start;
        size_t seg_len = i - start;
        bool is_src = seg_len == 3 && strncmp(seg, "src", 3) == 0;
        if (is_src && prev) {
            if (rust_same_crate_name(prev, prev_len, name))
                return kept;
            const PDXECargoManifest *m = (const PDXECargoManifest *)ctx->cargo_manifest;
            if (kept == 1 && m && m->package_name &&
                rust_same_crate_name(name, strlen(name), m->package_name))
                return kept; /* the project's own package, `src` at the top */
        }
        if (!is_src)
            kept++;
        prev = seg;
        prev_len = seg_len;
        start = i + 1;
    }
    return -1;
}

/* Whether the module `fmod[0..fn)` is `fmod[0..base)` followed by `rest[0..rn)`,
 * or a child of that (a module re-exports what its child modules define). */
static bool rust_module_under(char **fmod, int fn, int base, char **rest, int rn) {
    if (base < 0 || (fn != base + rn && fn != base + rn + 1))
        return false;
    for (int k = 0; k < rn; k++) {
        if (strcmp(fmod[base + k], rest[k]) != 0)
            return false;
    }
    return true;
}

/* Whether the qualifier `q[0..qn)` names the module `fmod[0..fn)` of a function
 * whose module path is `fqn[0..flen)`. `cur[0..cn)` is the caller's module (cn
 * -1 when unknown). An empty qualifier agrees with every module. */
static bool rust_module_agrees(RustLSPContext *ctx, const char *fqn, size_t flen, char **fmod,
                               int fn, char **cur, int cn, char **q, int qn) {
    if (qn == 0)
        return true;
    if (!rust_path_anchor(q[0])) {
        if (rust_module_ends_with(fmod, fn, q, qn))
            return true;
        /* A crate's name: the rest of the path is from that crate's root. */
        return rust_module_under(fmod, fn, rust_crate_root(ctx, fqn, flen, q[0]), q + 1, qn - 1);
    }
    if (cn < 0)
        return false;
    int lead = 0;
    int base = cn; /* the caller's module, as a prefix of `cur` */
    if (strcmp(q[0], "crate") == 0) {
        /* The crate root: the caller's module path up to the `src` directory. */
        const char *src = strstr(ctx->module_qn, ".src");
        if (!src || (src[4] != '.' && src[4] != '\0'))
            return false;
        char **root = NULL; /* a prefix of `cur`: only its length is needed */
        base = rust_path_segments(ctx->arena, ctx->module_qn, (size_t)(src - ctx->module_qn),
                                  &root);
        (void)root;
        if (base < 0)
            return false;
        lead = 1;
    } else if (strcmp(q[0], "self") == 0) {
        lead = 1;
    } else {
        while (lead < qn && strcmp(q[lead], "super") == 0) {
            lead++;
        }
        base = cn - lead;
    }
    /* The function's module is `cur[0..base)` followed by the rest of the qualifier
     * (or a child of it); for `super`, the inline module case admits the module one
     * level down as well. */
    int slack = strcmp(q[0], "super") == 0 ? 1 : 0;
    for (int b = base; b <= base + slack && b <= cn; b++) {
        if (b < 0)
            continue;
        bool same = true;
        for (int k = 0; k < b && same && k < fn; k++) {
            same = strcmp(fmod[k], cur[k]) == 0;
        }
        if (same && b <= fn && rust_module_under(fmod, fn, b, q + lead, qn - lead))
            return true;
    }
    return false;
}

/* A call's qualifier (`a::b` in `a::b::f`), read once for every function it is
 * weighed against. A head the file imports under a name stands for its import's
 * path. `cur` is the caller's module (`cn` -1 when unknown). */
typedef struct {
    char **q;
    int qn;
    char **cur;
    int cn;
} RustQualifier;

/* Reads the qualifier of `path`. False when it names no free function's module:
 * `Self` names a type, whose associated functions are never free functions. */
static bool rust_qualifier_read(RustLSPContext *ctx, const char *path, RustQualifier *out) {
    char **q = NULL;
    int qn = rust_split_path(ctx->arena, path, &q) - 1; /* the last is the function */
    if (qn < 0)
        return false;
    if (qn > 0 && !rust_path_anchor(q[0]) && strcmp(q[0], "Self") != 0) {
        const char *imported = rust_resolve_use(ctx, q[0]);
        char **useg = NULL;
        int un = imported ? rust_split_path(ctx->arena, imported, &useg) : -1;
        if (un > 0) {
            char **joined =
                (char **)pdxe_arena_alloc(ctx->arena, (size_t)(un + qn - 1) * sizeof(char *));
            if (!joined)
                return false;
            memcpy(joined, useg, (size_t)un * sizeof(char *));
            memcpy(joined + un, q + 1, (size_t)(qn - 1) * sizeof(char *));
            q = joined;
            qn = un + qn - 1;
        }
    }
    if (qn > 0 && strcmp(q[0], "Self") == 0)
        return false;
    out->q = q;
    out->qn = qn;
    out->cur = NULL;
    out->cn = ctx->module_qn ? rust_path_segments(ctx->arena, ctx->module_qn,
                                                  strlen(ctx->module_qn), &out->cur)
                             : -1;
    return true;
}

/* Whether the free function `fqn` is in the module the qualifier names. A function
 * elsewhere that shares the call's last name is no evidence for the call:
 * `from_request::expand` is not the crate root's `expand` and
 * `multer::parse_boundary` no function of this project. The qualifier is read as
 * Rust reads it: `crate`, `self` and `super` anchor it at the caller's crate root,
 * module and parent module (`super` inside an inline module names the file's own
 * module, so that is admitted too); a crate's name stands for that crate's root; and
 * an inline module of this file, which qualified names leave out, for a function of
 * this file. An empty qualifier agrees with every function. */
static bool rust_qualifier_agrees(RustLSPContext *ctx, const RustQualifier *ql, const char *fqn) {
    if (ql->qn == 0)
        return true;
    const char *fdot = strrchr(fqn, '.');
    if (!fdot)
        return false;
    size_t flen = (size_t)(fdot - fqn);
    char **fmod = NULL;
    int fn = rust_path_segments(ctx->arena, fqn, flen, &fmod);
    if (fn < 0)
        return false;
    if (rust_module_agrees(ctx, fqn, flen, fmod, fn, ql->cur, ql->cn, ql->q, ql->qn))
        return true;
    /* A function of this file through its inline modules, which its qualified name
     * leaves out: the qualifier without them. */
    if (ctx->module_qn && strlen(ctx->module_qn) == flen &&
        strncmp(fqn, ctx->module_qn, flen) == 0) {
        int keep = ql->qn;
        while (keep > 0 && pdxe_idxmemo_get(&ctx->inline_modules, ql->q[keep - 1]) >= 0) {
            keep--;
        }
        if (keep < ql->qn &&
            rust_module_agrees(ctx, fqn, flen, fmod, fn, ql->cur, ql->cn, ql->q, keep))
            return true;
    }
    return false;
}

/* Issue 104: what a bare name's import says about where its function is. The
 * path the `use` in force imports it by, when that path has a qualifier; else NULL.
 * `*rank` as rust_resolve_use_ranked's. */
static const char *rust_import_evidence(RustLSPContext *ctx, const char *name, int *rank) {
    const char *imported = rust_resolve_use_ranked(ctx, name, rank);
    return imported && strstr(imported, "::") ? imported : NULL;
}

/* Appends scope `s`'s glob imports, and those of the enclosing modules it links to. */
static void rust_scope_globs(RustLSPContext *ctx, int s, const char ***list, int *n, int *cap) {
    RustUseScope *sc = &ctx->use_scopes[s];
    for (int g = 0; g < sc->glob_count; g++) {
        if (*n >= *cap) {
            int new_cap = *cap ? *cap * 2 : 8;
            const char **grown =
                (const char **)pdxe_arena_alloc(ctx->arena, (size_t)new_cap * sizeof(char *));
            if (!grown) {
                return;
            }
            if (*n > 0) {
                memcpy(grown, *list, (size_t)*n * sizeof(char *));
            }
            *list = grown;
            *cap = new_cap;
        }
        (*list)[(*n)++] = sc->globs[g];
    }
    for (int l = 0; l < sc->link_count; l++) {
        if (sc->links[l] < s) {
            rust_scope_globs(ctx, sc->links[l], list, n, cap);
        }
    }
}

/* Issue 114: the glob imports in force where the walk is: those of the scopes from
 * the innermost out to the first module, or of the `upto` innermost when `upto` is
 * not negative. */
static int rust_visible_globs(RustLSPContext *ctx, int upto, const char ***out) {
    *out = NULL;
    int n = 0, cap = 0;
    if (ctx->use_scope_count == 0) {
        return 0;
    }
    int s = ctx->use_scope >= 0 && ctx->use_scope < ctx->use_scope_count ? ctx->use_scope : 0;
    for (int r = 0; s >= 0 && (upto < 0 || r < upto); r++) {
        rust_scope_globs(ctx, s, out, &n, &cap);
        if (ctx->use_scopes[s].module) {
            break;
        }
        s = ctx->use_scopes[s].parent;
    }
    return n;
}

/* The qualifiers of the glob imports `globs` (`use a::b::*`), each as the path that
 * brings `name` in. Returns how many were read into `out`, which holds `glob_n`. */
static int rust_glob_qualifiers(RustLSPContext *ctx, const char *name, const char **globs,
                                int glob_n, RustQualifier *out) {
    int n = 0;
    for (int i = 0; i < glob_n; i++) {
        const char *g = globs[i];
        if (!g || !g[0])
            continue;
        /* Stored dotted; a Rust path again, with the name the glob brings in. */
        size_t gl = strlen(g);
        char *p = (char *)pdxe_arena_alloc(ctx->arena, gl * 2 + strlen(name) + 3);
        if (!p)
            return n;
        size_t w = 0;
        for (size_t k = 0; k < gl; k++) {
            if (g[k] == '.') {
                p[w++] = ':';
                p[w++] = ':';
            } else {
                p[w++] = g[k];
            }
        }
        p[w] = '\0';
        strcat(p, "::");
        strcat(p, name);
        if (rust_qualifier_read(ctx, p, &out[n]))
            n++;
    }
    return n;
}

/* The free functions named `wanted` in the caller's crate that the evidence admits:
 * the qualifier `ql` when `read`, or one of the glob qualifiers. Counts up to two
 * (`*agreeing` the first). */
static int rust_agreeing_free_funcs(RustLSPContext *ctx, const char *wanted, size_t crate_len,
                                    bool read, const RustQualifier *ql,
                                    const RustQualifier *globs, int glob_n,
                                    const PDXERegisteredFunc **agreeing) {
    int agree = 0;
    *agreeing = NULL;
    /* Iterate only free funcs whose short_name == wanted via the index; the
     * receiver/short_name/crate-prefix re-checks below are unchanged. */
    PDXEFreeFuncIter ffit;
    pdxe_registry_free_funcs_by_short_name(ctx->registry, wanted, &ffit);
    for (int i; (read || glob_n > 0) && agree < 2 && (i = pdxe_free_func_iter_next(&ffit)) >= 0;) {
        const PDXERegisteredFunc *f = &ctx->registry->funcs[i];
        if (!f->short_name || !f->qualified_name)
            continue;
        if (f->receiver_type)
            continue; /* free functions only */
        if (strcmp(f->short_name, wanted) != 0)
            continue;
        /* Crate-scoped: QN must start with the same prefix. */
        if (strncmp(f->qualified_name, ctx->module_qn, crate_len) != 0)
            continue;
        bool admitted = read && rust_qualifier_agrees(ctx, ql, f->qualified_name);
        for (int g = 0; !admitted && g < glob_n; g++) {
            admitted = rust_qualifier_agrees(ctx, &globs[g], f->qualified_name);
        }
        if (admitted) {
            agree++;
            if (agree == 1)
                *agreeing = f;
        }
    }
    return agree;
}

/* The glob qualifiers for `name` of the globs in force (the `upto` innermost scopes'
 * when `upto` is not negative), read into an arena array. */
static int rust_glob_evidence(RustLSPContext *ctx, const char *name, int upto,
                              RustQualifier **out) {
    const char **globs = NULL;
    int glob_n = rust_visible_globs(ctx, upto, &globs);
    *out = NULL;
    if (glob_n == 0) {
        return 0;
    }
    *out = (RustQualifier *)pdxe_arena_alloc(ctx->arena, (size_t)glob_n * sizeof(RustQualifier));
    return *out ? rust_glob_qualifiers(ctx, name, globs, glob_n, *out) : 0;
}

/* ── Issue 109: a trait impl applies only when its header holds ───────────────
 *
 * `Type::method` reaches a trait's method through an impl of the trait for the type,
 * and an impl applies to a type only when what its header requires holds for it. A
 * typed answer names an impl's method only when the impl is proven to apply: every
 * predicate's bounds hold for the site's types, from the registry's impls. Absence of
 * another impl is no proof, since a blanket impl (registered under its parameter's
 * name), a macro's impl (issue 110) or another crate's may be the one that applies.
 *
 * A bound is proven, or unknown. No bound is ever shown false from these facts, for
 * the same reasons, so an unsatisfied bound is unknown too, and unknown never counts
 * as proven. */

static bool rust_word_char(char c) {
    return isalnum((unsigned char)c) || c == '_';
}

/* A node's tokens as extraction writes a header's texts (extract_defs.c,
 * rust_ib_text): comments left out, a space only between two word characters. */
static char *rust_tokens_text(RustLSPContext *ctx, TSNode node) {
    uint32_t start = ts_node_start_byte(node);
    uint32_t end = ts_node_end_byte(node);
    char *out = (char *)pdxe_arena_alloc(ctx->arena, (size_t)(end > start ? end - start : 0) + 1);
    if (!out) {
        return NULL;
    }
    size_t len = 0;
    PDXE_TREE_WALK(w, node) {
        const char *kind = ts_node_type(w.node);
        if (strcmp(kind, "line_comment") == 0 || strcmp(kind, "block_comment") == 0) {
            pdxe_tree_walk_skip_children(&w);
            continue;
        }
        if (ts_node_child_count(w.node) != 0) {
            continue;
        }
        uint32_t s = ts_node_start_byte(w.node);
        uint32_t e = ts_node_end_byte(w.node);
        if (e <= s || e > (uint32_t)ctx->source_len) {
            continue;
        }
        if (len > 0 && rust_word_char(out[len - 1]) && rust_word_char(ctx->source[s])) {
            out[len++] = ' ';
        }
        memcpy(out + len, ctx->source + s, e - s);
        len += e - s;
    }
    out[len] = '\0';
    return out;
}

/* The generic parameters of `item` (a function or an impl) come into scope. */
static void rust_push_generic_names(RustLSPContext *ctx, TSNode item) {
    TSNode params = ts_node_child_by_field_name(item, "type_parameters", 15);
    if (ts_node_is_null(params)) {
        return;
    }
    PDXE_FOR_EACH_NAMED_CHILD(it, params) {
        const char *kind = ts_node_type(it.node);
        if (strcmp(kind, "type_parameter") != 0 && strcmp(kind, "const_parameter") != 0) {
            continue;
        }
        TSNode name_node = ts_node_child_by_field_name(it.node, "name", 4);
        char *name = ts_node_is_null(name_node) ? NULL : rust_node_text(ctx, name_node);
        if (!name) {
            continue;
        }
        if (ctx->generic_name_count == ctx->generic_name_cap) {
            int cap = ctx->generic_name_cap ? ctx->generic_name_cap * 2 : 8;
            const char **grown =
                (const char **)pdxe_arena_alloc(ctx->arena, (size_t)cap * sizeof(*grown));
            if (!grown) {
                return;
            }
            for (int i = 0; i < ctx->generic_name_count; i++) {
                grown[i] = ctx->generic_names[i];
            }
            ctx->generic_names = grown;
            ctx->generic_name_cap = cap;
        }
        ctx->generic_names[ctx->generic_name_count++] = name;
    }
}

static bool rust_is_generic_name(const RustLSPContext *ctx, const char *name) {
    for (int i = 0; i < ctx->generic_name_count; i++) {
        if (strcmp(ctx->generic_names[i], name) == 0) {
            return true;
        }
    }
    return false;
}

/* A site's type argument: the node, and its text as a header's texts are written. */
typedef struct {
    TSNode node;
    const char *text;
} RustSiteTypeArg;

/* The type arguments a call path gives its type (`Type::<A, B>::method`), lifetimes
 * left out; 0 when it gives none. At most `cap`; more is none (never a prefix). */
static int rust_site_type_args(RustLSPContext *ctx, TSNode func_path, RustSiteTypeArg *out,
                               int cap) {
    if (ts_node_is_null(func_path)) {
        return 0;
    }
    const char *kind = ts_node_type(func_path);
    if (strcmp(kind, "generic_type") != 0 && strcmp(kind, "generic_type_with_turbofish") != 0) {
        return 0;
    }
    TSNode type_args = ts_node_child_by_field_name(func_path, "type_arguments", 14);
    if (ts_node_is_null(type_args)) {
        return 0;
    }
    int n = 0;
    PDXE_FOR_EACH_NAMED_CHILD(it, type_args) {
        const char *k = ts_node_type(it.node);
        if (strcmp(k, "lifetime") == 0 || strcmp(k, "line_comment") == 0 ||
            strcmp(k, "block_comment") == 0) {
            continue;
        }
        if (n == cap) {
            return 0;
        }
        out[n].node = it.node;
        out[n].text = rust_tokens_text(ctx, it.node);
        if (!out[n].text) {
            return 0;
        }
        n++;
    }
    return n;
}

/* Whether `text` holds one of the impl's parameter names as a word. */
static bool rust_text_names_param(const char *text, const PDXERustImplFacts *f) {
    for (const char *p = text; *p;) {
        if (!rust_word_char(*p) || (p > text && rust_word_char(p[-1]))) {
            p++;
            continue;
        }
        const char *q = p;
        while (*q && rust_word_char(*q)) {
            q++;
        }
        for (int i = 0; i < f->param_count; i++) {
            size_t len = strlen(f->params[i]);
            if ((size_t)(q - p) == len && strncmp(p, f->params[i], len) == 0) {
                return true;
            }
        }
        p = q;
    }
    return false;
}

/* A bound's generic argument with the site's types put in for the impl's parameters:
 * the argument itself when it names none, the site's type when it is exactly one the
 * site gives, NULL otherwise (unknown). */
static const char *rust_bound_arg_at_site(const char *arg, const PDXERustImplFacts *f,
                                          const RustSiteTypeArg *const *sigma) {
    int index = rust_impl_param_index(f->params, f->param_count, arg);
    if (index >= 0) {
        return sigma[index] ? sigma[index]->text : NULL;
    }
    return rust_text_names_param(arg, f) ? NULL : arg;
}

/* Whether relation `r`, an impl of the bound's trait for the site's type, holds for it
 * whatever the type's arguments: a plain self type, no requirement, and trait
 * arguments that are its own free parameters or the bound's arguments exactly. */
static bool rust_relation_proves(const PDXERustImplFacts *r, const PDXERustBound *bound,
                                 const PDXERustImplFacts *f, const RustSiteTypeArg *const *sigma) {
    if (r->opaque || r->derived || r->self_form != 'p' || r->trait_arg_count != bound->arg_count) {
        return false;
    }
    for (int i = 0; i < r->bound_count; i++) {
        if (r->bounds[i].kind != PDXE_RUST_BOUND_RELAXED &&
            r->bounds[i].kind != PDXE_RUST_BOUND_LIFETIME) {
            return false;
        }
    }
    for (int i = 0; i < r->trait_arg_count; i++) {
        const char *own = r->trait_args[i];
        int param = rust_impl_param_index(r->params, r->param_count, own);
        if (param >= 0) {
            /* Free only when no other position of the header holds it. */
            bool free_param = true;
            for (int k = 0; k < r->self_arg_count && free_param; k++) {
                free_param = r->self_args[k] != param;
            }
            for (int k = 0; k < r->trait_arg_count && free_param; k++) {
                free_param = k == i || strcmp(r->trait_args[k], own) != 0;
            }
            if (!free_param) {
                return false;
            }
            continue;
        }
        if (rust_text_names_param(own, r)) {
            return false;
        }
        const char *wanted = rust_bound_arg_at_site(bound->args[i], f, sigma);
        if (!wanted || strcmp(wanted, own) != 0) {
            return false;
        }
    }
    return true;
}

/* Whether the site's type `arg` is proven to implement `bound`'s trait: an impl the
 * registry holds for exactly that type, which holds for it unconditionally. */
static bool rust_type_proven_to_implement(RustLSPContext *ctx, const RustSiteTypeArg *arg,
                                          const PDXERustBound *bound, const PDXERustImplFacts *f,
                                          const RustSiteTypeArg *const *sigma) {
    if (!bound->trait_qn || ts_node_is_null(arg->node)) {
        return false;
    }
    const char *kind = ts_node_type(arg->node);
    if (strcmp(kind, "type_identifier") != 0 && strcmp(kind, "scoped_type_identifier") != 0 &&
        strcmp(kind, "generic_type") != 0) {
        return false; /* `_`, a reference, a tuple, a primitive, ...: not a registered type */
    }
    TSNode base = arg->node;
    if (strcmp(kind, "generic_type") == 0) {
        base = ts_node_child_by_field_name(arg->node, "type", 4);
        if (ts_node_is_null(base)) {
            return false;
        }
    }
    if (strcmp(ts_node_type(base), "type_identifier") == 0) {
        char *name = rust_node_text(ctx, base);
        if (!name || strcmp(name, "_") == 0 || rust_is_generic_name(ctx, name) ||
            is_rust_primitive(name)) {
            return false;
        }
    }
    const PDXEType *type = rust_parse_type_node(ctx, arg->node);
    const char *qn = NULL;
    if (type && type->kind == PDXE_TYPE_NAMED) {
        qn = type->data.named.qualified_name;
    } else if (type && type->kind == PDXE_TYPE_TEMPLATE) {
        qn = type->data.template_type.template_name;
    }
    if (!qn) {
        return false;
    }
    for (const PDXETypeRegistry *reg = ctx->registry; reg; reg = reg->fallback) {
        for (int ti = pdxe_registry_type_first_by_qn(reg, qn); ti >= 0;
             ti = pdxe_registry_type_next_by_qn(reg, ti)) {
            const PDXERegisteredType *t = &reg->types[ti];
            for (int k = 0; k < t->rust_impl_count; k++) {
                const PDXERustImplFacts *r = t->rust_impls[k];
                if (r->trait_qn && strcmp(r->trait_qn, bound->trait_qn) == 0 &&
                    rust_relation_proves(r, bound, f, sigma)) {
                    return true;
                }
            }
        }
    }
    return false;
}

/* Whether the impl `f` is proven to apply at a call whose path gives `site` (n type
 * arguments) to the impl's self type: every predicate's bounds hold. */
static bool rust_impl_proven_to_apply(RustLSPContext *ctx, const PDXERustImplFacts *f,
                                      const RustSiteTypeArg *site, int n) {
    if (!f || f->opaque || f->self_form == 'o') {
        return false;
    }
    const RustSiteTypeArg **sigma = NULL;
    if (f->param_count > 0) {
        sigma = (const RustSiteTypeArg **)pdxe_arena_alloc(
            ctx->arena, (size_t)f->param_count * sizeof(*sigma));
        if (!sigma) {
            return false;
        }
        memset(sigma, 0, (size_t)f->param_count * sizeof(*sigma));
    }
    /* A plain self type's arguments are the site's, position for position, when it
     * gives exactly as many. A blanket impl's parameter is the call's type, which the
     * path does not name as a registered type: unknown. */
    if (f->self_form == 'p' && n > 0 && n == f->self_arg_count) {
        for (int i = 0; i < n; i++) {
            sigma[f->self_args[i]] = &site[i];
        }
    }
    for (int i = 0; i < f->bound_count; i++) {
        const PDXERustBound *b = &f->bounds[i];
        if (b->kind == PDXE_RUST_BOUND_RELAXED || b->kind == PDXE_RUST_BOUND_LIFETIME) {
            continue;
        }
        if (b->kind != PDXE_RUST_BOUND_TRAIT || b->subject < 0 || !sigma[b->subject] ||
            !rust_type_proven_to_implement(ctx, sigma[b->subject], b, f, sigma)) {
            return false;
        }
    }
    return true;
}

/* Issue 105: records the predicates of `item`'s where clause (an impl's, a function's)
 * whose subject is a type: `Router<S>: Clone` as Router's QN and Clone's. */
static void rust_push_where_predicates(RustLSPContext *ctx, TSNode item) {
    PDXE_FOR_EACH_CHILD(it, item) {
        if (strcmp(ts_node_type(it.node), "where_clause") != 0) {
            continue;
        }
        PDXE_FOR_EACH_NAMED_CHILD(p, it.node) {
            if (strcmp(ts_node_type(p.node), "where_predicate") != 0) {
                continue;
            }
            TSNode left = ts_node_child_by_field_name(p.node, "left", 4);
            TSNode bounds = ts_node_child_by_field_name(p.node, "bounds", 6);
            if (ts_node_is_null(left) || ts_node_is_null(bounds)) {
                continue;
            }
            if (strcmp(ts_node_type(left), "generic_type") == 0) {
                left = ts_node_child_by_field_name(left, "type", 4);
            }
            const char *lk = ts_node_is_null(left) ? "" : ts_node_type(left);
            if (strcmp(lk, "type_identifier") != 0 && strcmp(lk, "scoped_type_identifier") != 0) {
                continue;
            }
            char *subject_text = rust_node_text(ctx, left);
            const char *subject = subject_text ? rust_resolve_path_expr(ctx, subject_text) : NULL;
            if (!subject) {
                continue;
            }
            PDXE_FOR_EACH_NAMED_CHILD(b, bounds) {
                TSNode trait = b.node;
                if (strcmp(ts_node_type(trait), "generic_type") == 0) {
                    trait = ts_node_child_by_field_name(trait, "type", 4);
                }
                const char *tk = ts_node_is_null(trait) ? "" : ts_node_type(trait);
                if (strcmp(tk, "type_identifier") != 0 &&
                    strcmp(tk, "scoped_type_identifier") != 0) {
                    continue;
                }
                char *trait_text = rust_node_text(ctx, trait);
                const char *trait_qn = trait_text ? rust_resolve_path_expr(ctx, trait_text) : NULL;
                if (!trait_qn) {
                    continue;
                }
                if (ctx->where_count >= ctx->where_cap) {
                    int cap = ctx->where_cap ? ctx->where_cap * 2 : 8;
                    const char **s = (const char **)pdxe_arena_alloc(ctx->arena,
                                                                     (size_t)cap * sizeof(char *));
                    const char **t = (const char **)pdxe_arena_alloc(ctx->arena,
                                                                     (size_t)cap * sizeof(char *));
                    if (!s || !t) {
                        return;
                    }
                    if (ctx->where_count > 0) {
                        memcpy(s, ctx->where_subjects, (size_t)ctx->where_count * sizeof(char *));
                        memcpy(t, ctx->where_traits, (size_t)ctx->where_count * sizeof(char *));
                    }
                    ctx->where_subjects = s;
                    ctx->where_traits = t;
                    ctx->where_cap = cap;
                }
                ctx->where_subjects[ctx->where_count] = subject;
                ctx->where_traits[ctx->where_count] = trait_qn;
                ctx->where_count++;
            }
        }
    }
}

/* Issue 105: whether a predicate in force bounds the type `type_qn` by the trait
 * `trait_qn`: a call of the trait's method on that type goes through the bound, as
 * method resolution takes it before any impl. */
static bool rust_where_bounds(const RustLSPContext *ctx, const char *type_qn,
                              const char *trait_qn) {
    for (int i = 0; type_qn && trait_qn && i < ctx->where_count; i++) {
        if (strcmp(ctx->where_subjects[i], type_qn) == 0 &&
            strcmp(ctx->where_traits[i], trait_qn) == 0) {
            return true;
        }
    }
    return false;
}

static const PDXERustImplFacts *rust_sole_impl_of(const RustLSPContext *ctx, const char *type_qn,
                                                  const char *trait_qn);

/* Issue 105: a trait impl's method answers a method call (`x.m()`) on a value of the
 * type `type_qn` only when its impl is proven to apply, as issue 109 requires of
 * `Type::method`; a trait's own method (a default) only when the type's one impl of the
 * trait is. The receiver gives the self type no type arguments a header could be
 * checked against, so only an impl whose header requires nothing is: a bound left
 * unproven, an opaque or missing header, no impl or several, is no answer. A trait's
 * method reached through a bound or a trait object is the trait's, and a derive's method
 * and an inherent one are answered as before. */
static bool rust_method_dispatch_proven(RustLSPContext *ctx, const char *type_qn,
                                        const PDXERegisteredFunc *m) {
    if (!m || (m->flags & PDXE_FUNC_FLAG_RUST_DERIVED) != 0) {
        return true;
    }
    if ((m->flags & PDXE_FUNC_FLAG_RUST_TRAIT_IMPL) != 0) {
        /* A predicate in force that bounds the receiver's type by the method's trait:
         * the call is through that bound, the trait's method, not this impl's. */
        if (rust_where_bounds(ctx, m->receiver_type, m->impl_trait_qn)) {
            return false;
        }
        return rust_impl_proven_to_apply(ctx, m->rust_impl, NULL, 0);
    }
    const PDXERegisteredType *owner =
        m->receiver_type ? pdxe_registry_lookup_type(ctx->registry, m->receiver_type) : NULL;
    if (!owner || !owner->is_interface || !type_qn) {
        return true;
    }
    const PDXERegisteredType *receiver = pdxe_registry_lookup_type(ctx->registry, type_qn);
    if (receiver && receiver->is_interface) {
        return true;
    }
    const PDXERustImplFacts *impl = rust_sole_impl_of(ctx, type_qn, m->receiver_type);
    if (impl && impl->derived) {
        return true;
    }
    return rust_impl_proven_to_apply(ctx, impl, NULL, 0);
}

/* The one impl of `trait_qn` the registry holds for the type `type_qn`, or NULL when
 * it holds none or several. */
static const PDXERustImplFacts *rust_sole_impl_of(const RustLSPContext *ctx, const char *type_qn,
                                                  const char *trait_qn) {
    const PDXERustImplFacts *found = NULL;
    for (const PDXETypeRegistry *reg = ctx->registry; reg; reg = reg->fallback) {
        for (int ti = pdxe_registry_type_first_by_qn(reg, type_qn); ti >= 0;
             ti = pdxe_registry_type_next_by_qn(reg, ti)) {
            const PDXERegisteredType *t = &reg->types[ti];
            for (int k = 0; k < t->rust_impl_count; k++) {
                const PDXERustImplFacts *r = t->rust_impls[k];
                if (!r->trait_qn || strcmp(r->trait_qn, trait_qn) != 0 || r == found) {
                    continue;
                }
                if (found) {
                    return NULL;
                }
                found = r;
            }
        }
    }
    return found;
}

/* The method a call `head_qn::name` reaches, given `m`, the one the registry found:
 * NULL when no method is proven to be it.
 *
 * - An inherent method is the type's own associated item, the one the path names: `m`,
 *   unless a trait impl's method of the name is beside it, since which of them applies
 *   then depends on the inherent impl's header, which is not read.
 *   A method the registry synthesises for a `#[derive]` names no definition of the
 *   repository, so no typed edge is drawn to it either way: `m`, as before.
 * - A trait impl's method, when the impl is proven to apply. The type may have several
 *   impls with a method of the name; each is checked, and the one proven is the target
 *   only when it is the only one and no other is an impl of its trait: an impl of
 *   another trait that applied too would make the call ambiguous, so one of them not
 *   proven does not apply; two impls of one trait may both apply, the call's arguments
 *   choosing between them (issue 108). An inherent method of the name beside them is
 *   no answer either, since it takes precedence when it applies.
 * - A trait's own method (a default), when the type's one impl of the trait is proven
 *   to apply.
 *
 * The path's type arguments are the self type's only when the method was found under
 * the type the path names, never through an alias of it. */
static const PDXERegisteredFunc *rust_ufcs_target(RustLSPContext *ctx, TSNode func_path,
                                                  const char *head_qn,
                                                  const PDXERegisteredFunc *m) {
    if (m->flags & PDXE_FUNC_FLAG_RUST_DERIVED) {
        return m;
    }
    RustSiteTypeArg site[16];
    int n = 0;
    if (!m->receiver_type || !head_qn || strcmp(m->receiver_type, head_qn) == 0) {
        n = rust_site_type_args(ctx, func_path, site, 16);
    }
    if (m->flags & PDXE_FUNC_FLAG_RUST_TRAIT_IMPL) {
        const PDXERegisteredFunc *proven = NULL;
        int proven_count = 0;
        PDXEMethodIter it;
        pdxe_registry_methods_chain(ctx->registry, m->receiver_type, m->short_name, &it);
        for (int index; (index = pdxe_method_iter_next(&it)) >= 0;) {
            const PDXERegisteredFunc *candidate = &it.reg->funcs[index];
            if ((candidate->flags & PDXE_FUNC_FLAG_RUST_TRAIT_IMPL) == 0) {
                return NULL;
            }
            if ((candidate->flags & PDXE_FUNC_FLAG_RUST_DERIVED) == 0 &&
                rust_impl_proven_to_apply(ctx, candidate->rust_impl, site, n)) {
                proven = candidate;
                proven_count++;
            }
        }
        if (proven_count != 1) {
            return NULL;
        }
        /* Another impl of the same trait (or of a trait not known) may apply as well:
         * which of them a call reaches is then the trait's arguments' question, inferred
         * from the call's own arguments, which is not asked here (issue 108). */
        pdxe_registry_methods_chain(ctx->registry, m->receiver_type, m->short_name, &it);
        for (int index; (index = pdxe_method_iter_next(&it)) >= 0;) {
            const PDXERegisteredFunc *candidate = &it.reg->funcs[index];
            if (candidate != proven &&
                (!candidate->impl_trait_qn || !proven->impl_trait_qn ||
                 strcmp(candidate->impl_trait_qn, proven->impl_trait_qn) == 0)) {
                return NULL;
            }
        }
        return proven;
    }
    const PDXERegisteredType *owner =
        m->receiver_type ? pdxe_registry_lookup_type(ctx->registry, m->receiver_type) : NULL;
    if (!owner || !owner->is_interface) {
        /* The type's own item, unless a trait impl's method of the name is beside it:
         * which applies is then the inherent impl's header's question, unasked. */
        PDXEMethodIter it;
        pdxe_registry_methods_chain(ctx->registry, m->receiver_type, m->short_name, &it);
        for (int index; (index = pdxe_method_iter_next(&it)) >= 0;) {
            if (it.reg->funcs[index].flags & PDXE_FUNC_FLAG_RUST_TRAIT_IMPL) {
                return NULL;
            }
        }
        return m;
    }
    const PDXERustImplFacts *impl = head_qn ? rust_sole_impl_of(ctx, head_qn, m->receiver_type)
                                            : NULL;
    if (impl && impl->derived) {
        return m;
    }
    n = rust_site_type_args(ctx, func_path, site, 16);
    return rust_impl_proven_to_apply(ctx, impl, site, n) ? m : NULL;
}

/* Entry hook implementation: classify a call_expression and emit the best
 * edge we can. The wrapper below establishes the exact original occurrence
 * for every return path through this function. */
static void rust_resolve_call_expression_inner(RustLSPContext *ctx, TSNode node) {
    TSNode func_node = ts_node_child_by_field_name(node, "function", 8);
    TSNode args_node = ts_node_child_by_field_name(node, "arguments", 9);
    if (ts_node_is_null(func_node))
        return;
    rust_resolve_callable_argument_references(ctx, args_node);
    const char *fk = ts_node_type(func_node);

    /* Method call via field_expression. The grammar can also expose
     * `s.cast::<T>(x)` as a `generic_function` callee whose inner
     * `function` is a field_expression — peel that wrapper here. */
    if (strcmp(fk, "generic_function") == 0) {
        TSNode inner = ts_node_child_by_field_name(func_node, "function", 8);
        if (!ts_node_is_null(inner)) {
            func_node = inner;
            fk = ts_node_type(func_node);
        }
    }
    if (strcmp(fk, "field_expression") == 0) {
        TSNode value = ts_node_child_by_field_name(func_node, "value", 5);
        TSNode field = ts_node_child_by_field_name(func_node, "field", 5);
        if (ts_node_is_null(value) || ts_node_is_null(field))
            return;
        char *mname = rust_node_text(ctx, field);
        if (!mname)
            return;
        /* Strip turbofish from the method name in case the grammar
         * embedded it as part of the field token. */
        rust_strip_turbofish(mname);

        const PDXEType *recv = rust_eval_expr_type(ctx, value);
        const PDXEType *base = recv;
        while (base && (base->kind == PDXE_TYPE_REFERENCE || base->kind == PDXE_TYPE_POINTER)) {
            base = (base->kind == PDXE_TYPE_REFERENCE) ? base->data.reference.elem
                                                      : base->data.pointer.elem;
        }

        const char *type_qn = NULL;
        if (base && base->kind == PDXE_TYPE_NAMED)
            type_qn = base->data.named.qualified_name;
        else if (base && base->kind == PDXE_TYPE_TEMPLATE)
            type_qn = base->data.template_type.template_name;
        else if (base && base->kind == PDXE_TYPE_BUILTIN) {
            const char *nm = base->data.builtin.name;
            if (strcmp(nm, "str") == 0)
                type_qn = "core.str";
            else if (strcmp(nm, "String") == 0)
                type_qn = "alloc.string.String";
            else if (is_rust_primitive(nm)) {
                /* Primitive integer / float / char / bool / unit / never
                 * methods are registered under their primitive name. */
                type_qn = nm;
            }
        } else if (base && base->kind == PDXE_TYPE_SLICE) {
            /* `&[T]` / `[T]` method dispatch. */
            type_qn = "core.slice";
        }

        if (type_qn) {
            int impl_count = 0;
            bool trait_dispatch = false;
            const PDXERegisteredFunc *m =
                rust_resolve_trait_method(ctx, type_qn, mname, &impl_count, &trait_dispatch);
            if (m && !rust_method_dispatch_proven(ctx, type_qn, m)) {
                rust_emit_unresolved_call(ctx, m->qualified_name, "impl_unproven");
                return;
            }
            if (m) {
                const char *strategy = "lsp_method_dispatch";
                float conf = PDXE_RUST_CONF_METHOD;
                const PDXERegisteredType *method_owner =
                    m->receiver_type ? pdxe_registry_lookup_type(ctx->registry, m->receiver_type)
                                     : NULL;
                if ((m->flags & PDXE_FUNC_FLAG_RUST_TRAIT_IMPL) != 0 ||
                    (method_owner && method_owner->is_interface) || trait_dispatch) {
                    strategy = "lsp_trait_dispatch";
                    conf = (impl_count == 1) ? PDXE_RUST_CONF_TRAIT_SOLE : PDXE_RUST_CONF_TRAIT_AMB;
                }
                rust_emit_resolved_call(ctx, m->qualified_name, strategy, conf);
                (void)args_node;
                return;
            }

            /* Walk the Deref chain — `Box<T>::method` may live on `T`,
             * `Rc<RefCell<T>>::method` peels two levels, etc. We bound
             * the chain at 8 hops to mirror the rust-analyzer cap. */
            const PDXEType *cur = base;
            for (int hop = 0; hop < 8; hop++) {
                const PDXEType *next = rust_deref_step(ctx, cur);
                if (!next)
                    break;
                /* Unwrap reference layers introduced by deref. */
                while (next &&
                       (next->kind == PDXE_TYPE_REFERENCE || next->kind == PDXE_TYPE_POINTER)) {
                    next = (next->kind == PDXE_TYPE_REFERENCE) ? next->data.reference.elem
                                                              : next->data.pointer.elem;
                }
                if (!next)
                    break;
                const char *next_qn = NULL;
                if (next->kind == PDXE_TYPE_NAMED)
                    next_qn = next->data.named.qualified_name;
                else if (next->kind == PDXE_TYPE_TEMPLATE)
                    next_qn = next->data.template_type.template_name;
                else if (next->kind == PDXE_TYPE_BUILTIN) {
                    const char *nm = next->data.builtin.name;
                    if (strcmp(nm, "str") == 0)
                        next_qn = "core.str";
                    else if (strcmp(nm, "String") == 0)
                        next_qn = "alloc.string.String";
                }
                if (!next_qn)
                    break;
                int hop_impls = 0;
                const PDXERegisteredFunc *hm =
                    rust_resolve_trait_method(ctx, next_qn, mname, &hop_impls, NULL);
                if (hm && !rust_method_dispatch_proven(ctx, next_qn, hm)) {
                    rust_emit_unresolved_call(ctx, hm->qualified_name, "impl_unproven");
                    return;
                }
                if (hm) {
                    rust_emit_resolved_call(ctx, hm->qualified_name, "lsp_deref_dispatch",
                                            PDXE_RUST_CONF_PROMOTED);
                    return;
                }
                cur = next;
            }

            /* Chalk-lite: receiver is typed as a NAMED with a name
             * that matches a current type-param bound. Resolve through
             * the bound trait. */
            {
                /* Just the local tail name. */
                const char *short_qn = type_qn;
                const char *dot = strrchr(type_qn, '.');
                if (dot)
                    short_qn = dot + 1;
                const char *bound = rust_lookup_type_param_bound(ctx, short_qn);
                if (bound) {
                    int bimpls = 0;
                    const PDXERegisteredFunc *bm =
                        rust_resolve_trait_method(ctx, bound, mname, &bimpls, NULL);
                    if (bm && !rust_method_dispatch_proven(ctx, bound, bm)) {
                        rust_emit_unresolved_call(ctx, bm->qualified_name, "impl_unproven");
                        return;
                    }
                    if (bm) {
                        rust_emit_resolved_call(ctx, bm->qualified_name, "lsp_bound_dispatch",
                                                PDXE_RUST_CONF_TRAIT_AMB);
                        return;
                    }
                }
            }

            /* Prelude trait method best-effort. */
            if (is_prelude_trait_method(mname)) {
                rust_emit_resolved_call(ctx, pdxe_arena_sprintf(ctx->arena, "%s.%s", type_qn, mname),
                                        "lsp_prelude_trait", PDXE_RUST_CONF_TRAIT_AMB);
                return;
            }
            rust_emit_unresolved_call(ctx, pdxe_arena_sprintf(ctx->arena, "%s.%s", type_qn, mname),
                                      "method_not_found");
            return;
        }

        /* Receiver type is unknown — record best-effort with the textual
         * receiver path so downstream can still see what we tried. */
        char *recv_text = rust_node_text(ctx, value);
        rust_emit_unresolved_call(
            ctx, pdxe_arena_sprintf(ctx->arena, "%s.%s", recv_text ? recv_text : "?", mname),
            "unknown_receiver_type");
        return;
    }

    /* Direct identifier or scoped path call. */
    if (strcmp(fk, "identifier") == 0 || strcmp(fk, "scoped_identifier") == 0 ||
        strcmp(fk, "generic_function") == 0) {
        TSNode actual_func = func_node;
        if (strcmp(fk, "generic_function") == 0) {
            TSNode inner = ts_node_child_by_field_name(actual_func, "function", 8);
            if (!ts_node_is_null(inner))
                actual_func = inner;
        }
        char *path = rust_node_text(ctx, actual_func);
        if (!path)
            return;
        /* Strip ALL turbofish (`Vec::<i32>::new` → `Vec::new`). */
        rust_strip_turbofish(path);

        if (strcmp(ts_node_type(actual_func), "identifier") == 0) {
            const char *alias_target = pdxe_scope_lookup_callable(ctx->current_scope, path);
            if (alias_target) {
                rust_emit_resolved_call_reason(ctx, alias_target, "lsp_callable_alias",
                                               PDXE_RUST_CONF_DIRECT, path);
                return;
            }
            if (pdxe_scope_contains(ctx->current_scope, path)) {
                return;
            }
        }

        bool via_identity = false;
        const char *qn = rust_resolve_path_expr_ex(ctx, path, &via_identity);
        if (!qn)
            return;

        /* Try registered free function first. Also try module-prefixed
         * fallback so `Logger::new` (which resolves to "Logger.new")
         * still finds the project's `<module>.Logger.new`. A path that names a
         * method only as Rust's name resolution reads it (`Type::method`, issue 105)
         * goes to the UFCS dispatch below, which proves its impl (issue 109). */
        const PDXERegisteredFunc *direct = pdxe_registry_lookup_func(ctx->registry, qn);
        if (direct && !via_identity) {
            /* A type's method its whole path names (`Self::m` in an impl of a type with
             * no arguments) is a trait impl's only when the impl is proven to apply, as
             * through the dispatch below (issue 109): never by the name alone. */
            const PDXERegisteredType *owner =
                direct->receiver_type
                    ? pdxe_registry_lookup_type(ctx->registry, direct->receiver_type)
                    : NULL;
            if (direct->receiver_type && (!owner || !owner->is_interface)) {
                const char *last = strrchr(qn, '.');
                const char *owner_qn =
                    last ? pdxe_arena_strndup(ctx->arena, qn, (size_t)(last - qn)) : NULL;
                TSNode owner_path = strcmp(ts_node_type(actual_func), "scoped_identifier") == 0
                                        ? ts_node_child_by_field_name(actual_func, "path", 4)
                                        : (TSNode){0};
                const PDXERegisteredFunc *proven =
                    rust_ufcs_target(ctx, owner_path, owner_qn, direct);
                if (!proven) {
                    rust_emit_unresolved_call(ctx, qn, "impl_unproven");
                    return;
                }
                if (proven != direct) {
                    rust_emit_resolved_call(ctx, proven->qualified_name, "lsp_ufcs",
                                            PDXE_RUST_CONF_UFCS);
                    return;
                }
            }
            rust_emit_resolved_call(ctx, qn, "lsp_direct", PDXE_RUST_CONF_DIRECT);
            return;
        }
        /* A free function the path names only as Rust reads it is answered below,
         * under the label of the reading that also finds it (the workspace member's,
         * the imported path's), and as itself when none does or another names a
         * different function. */
        const PDXERegisteredFunc *identity_free =
            direct && !direct->receiver_type ? direct : NULL;
        if (ctx->module_qn && strstr(qn, ".") == NULL) {
            const char *full = pdxe_arena_sprintf(ctx->arena, "%s.%s", ctx->module_qn, qn);
            if (pdxe_registry_lookup_func(ctx->registry, full)) {
                rust_emit_resolved_call(ctx, full, "lsp_direct", PDXE_RUST_CONF_DIRECT);
                return;
            }
        }

        /* UFCS form: T::method or trait_qn::method. */
        const char *dot = strrchr(qn, '.');
        if (dot && !identity_free) {
            char *head = pdxe_arena_strndup(ctx->arena, qn, (size_t)(dot - qn));
            const char *short_name = dot + 1;
            /* If `head` is a trait, `Trait::method` UFCS resolves to the sole
             * concrete impl (lsp_trait_ufcs), NEVER the trait's own abstract
             * method that the inherent lookup below would find. Resolve the trait
             * QN (head or module-qualified) via its is_interface flag — set at
             * type-registration time, so it is reliable even on an early pass
             * before impl links are wired. When the impl isn't known yet, emit
             * nothing: a partial-pass lsp_ufcs to the abstract method would
             * otherwise outrank (higher conf) the real trait_ufcs from the
             * complete pass and win the join. */
            const char *trait_qn = NULL;
            const PDXERegisteredType *head_t = pdxe_registry_lookup_type(ctx->registry, head);
            if (head_t && head_t->is_interface) {
                trait_qn = head;
            } else if (ctx->module_qn) {
                const char *fh = pdxe_arena_sprintf(ctx->arena, "%s.%s", ctx->module_qn, head);
                const PDXERegisteredType *ft = pdxe_registry_lookup_type(ctx->registry, fh);
                if (ft && ft->is_interface)
                    trait_qn = fh;
            }
            if (trait_qn) {
                int tn = 0;
                const PDXERegisteredFunc *ti_m =
                    rust_find_sole_trait_impl(ctx, trait_qn, short_name, &tn);
                if (tn == 1) {
                    rust_emit_resolved_call(
                        ctx,
                        ti_m ? ti_m->qualified_name
                             : pdxe_arena_sprintf(ctx->arena, "%s.%s", trait_qn, short_name),
                        "lsp_trait_ufcs", PDXE_RUST_CONF_TRAIT_SOLE);
                }
                return;
            }
            /* Issue 109: the type's arguments, if the path gives them, are what an
             * impl's header is checked against: `Type::<A, B>::method`. */
            TSNode func_path = strcmp(ts_node_type(actual_func), "scoped_identifier") == 0
                                   ? ts_node_child_by_field_name(actual_func, "path", 4)
                                   : (TSNode){0};
            const char *m_head = head;
            const PDXERegisteredFunc *m =
                pdxe_registry_lookup_method_aliased(ctx->registry, head, short_name);
            if (!m && ctx->module_qn) {
                /* Fall back to module-qualified head: `Logger.new` →
                 * `<module>.Logger.new`. */
                const char *full_head =
                    pdxe_arena_sprintf(ctx->arena, "%s.%s", ctx->module_qn, head);
                m = pdxe_registry_lookup_method_aliased(ctx->registry, full_head, short_name);
                m_head = full_head;
            }
            if (m) {
                /* A trait impl's method only when the impl is proven to apply; else
                 * no answer, never another of the name's. */
                m = rust_ufcs_target(ctx, func_path, m_head, m);
                if (!m) {
                    return;
                }
                rust_emit_resolved_call(ctx, m->qualified_name,
                                        strcmp(short_name, "new") == 0 ? "lsp_constructor"
                                                                       : "lsp_ufcs",
                                        PDXE_RUST_CONF_UFCS);
                return;
            }
            /* Trait method through single-impl dispatch. */
            int impls = 0;
            const char *tm_head = head;
            const PDXERegisteredFunc *tm =
                rust_resolve_trait_method(ctx, head, short_name, &impls, NULL);
            if (!tm && ctx->module_qn) {
                const char *full_head =
                    pdxe_arena_sprintf(ctx->arena, "%s.%s", ctx->module_qn, head);
                tm = rust_resolve_trait_method(ctx, full_head, short_name, &impls, NULL);
                tm_head = full_head;
            }
            if (tm && impls == 1) {
                tm = rust_ufcs_target(ctx, func_path, tm_head, tm);
                if (!tm) {
                    return;
                }
                rust_emit_resolved_call(ctx, tm->qualified_name, "lsp_trait_ufcs",
                                        PDXE_RUST_CONF_TRAIT_SOLE);
                return;
            }
        }

        const char *tail = strrchr(path, ':');
        if (tail && tail > path && tail[-1] == ':') {
            tail += 1;
        } else {
            tail = path;
        }

        /* Cross-crate workspace-member resolution (#56): when the call
         * path's head is a declared Cargo workspace member (e.g.
         * `crate_a::helper` from inside crate_b) we cannot rely on the
         * caller-crate-scoped fallback below — that filters by the
         * CALLER's module prefix and would resolve to a same-named local
         * function instead. Route to the function defined inside the
         * MEMBER crate by matching the registered QN's `.<member>.`
         * path segment plus the call tail. Requires a parsed manifest
         * (threaded through pass_lsp_cross.c); NULL manifest skips this. */
        if (ctx->cargo_manifest && tail && *tail) {
            const char *head_sep = strstr(path, "::");
            if (head_sep && head_sep > path) {
                char *head = pdxe_arena_strndup(ctx->arena, path, (size_t)(head_sep - path));
                const PDXECargoManifest *m = (const PDXECargoManifest *)ctx->cargo_manifest;
                if (head && pdxe_cargo_find_member(m, head)) {
                    /* `.crate_a.` — the member directory appears as a dotted
                     * QN segment for every def inside that crate. */
                    char *needle = pdxe_arena_sprintf(ctx->arena, ".%s.", head);
                    RustQualifier mem_ql;
                    bool mem_read = rust_qualifier_read(ctx, path, &mem_ql);
                    const PDXERegisteredFunc *mem_unique = NULL;
                    int mem_matches = 0;
                    /* Iterate only free funcs whose short_name == tail via the index;
                     * the receiver/short_name/needle re-checks below are unchanged. */
                    PDXEFreeFuncIter ffit;
                    pdxe_registry_free_funcs_by_short_name(ctx->registry, tail, &ffit);
                    for (int i; mem_matches < 2 && (i = pdxe_free_func_iter_next(&ffit)) >= 0;) {
                        const PDXERegisteredFunc *f = &ctx->registry->funcs[i];
                        if (!f->short_name || !f->qualified_name)
                            continue;
                        if (f->receiver_type)
                            continue; /* free functions only */
                        if (strcmp(f->short_name, tail) != 0)
                            continue;
                        if (!strstr(f->qualified_name, needle))
                            continue; /* not defined in the member crate */
                        if (!mem_read || !rust_qualifier_agrees(ctx, &mem_ql, f->qualified_name))
                            continue; /* issue 104: not in the module the path names */
                        mem_matches++;
                        if (mem_matches == 1)
                            mem_unique = f;
                    }
                    if (mem_matches == 1 && mem_unique &&
                        (!identity_free || mem_unique == identity_free)) {
                        rust_emit_resolved_call(ctx, mem_unique->qualified_name, "lsp_cross_crate",
                                                PDXE_RUST_CONF_DIRECT);
                        return;
                    }
                }
            }
        }

        /* Global short-name fallback: scan the registry for a unique
         * function whose short_name matches the path's tail and whose
         * QN starts with the current crate prefix. This gives `mod
         * foo; use foo::bar; bar()` a chance to resolve when the
         * intermediate module wasn't tracked through an explicit
         * use-map entry.
         *
         * Issue 104: a function unique by its last name alone is no evidence
         * that it is the one called, and was answered as typed. What is: the
         * path the call spells, or for a bare name the path the file imports
         * it by (a `use` or a glob); the one function in the module that path
         * names is the answer. A bare name nothing imports, and a path that
         * names a module no such function is in, have no answer. The label is
         * unchanged: it names the one function of its short name that the
         * path admits. */
        if (tail && *tail && ctx->module_qn) {
            /* Crate prefix is the first dotted segment of module_qn after
             * the project name, but for simplicity we just match on
             * "starts with first dot-segment". */
            const char *first_dot = strchr(ctx->module_qn, '.');
            size_t crate_len =
                first_dot ? (size_t)(first_dot - ctx->module_qn) : strlen(ctx->module_qn);
            int rank = -1;
            const char *evidence = tail != path ? path : rust_import_evidence(ctx, path, &rank);
            const PDXERegisteredFunc *agreeing = NULL;
            int agree = 0;
            if (evidence && tail == path && rank > 0) {
                /* Issue 114: a glob of a nearer scope that brings the name in
                 * shadows the outer scope's import, as Rust's nearer names do. */
                RustQualifier *inner = NULL;
                int inner_n = rust_glob_evidence(ctx, tail, rank, &inner);
                if (inner_n > 0) {
                    agree = rust_agreeing_free_funcs(ctx, tail, crate_len, false, NULL, inner,
                                                     inner_n, &agreeing);
                    if (agree > 0) {
                        evidence = NULL;
                    }
                }
            }
            /* Issue 111: an import binds the name its path ends in, under an alias
             * when it has one (`use a::on_service as service;`): that name is the
             * function's. */
            const char *wanted = evidence && tail == path ? path_last_segment(evidence) : tail;
            if (agree == 0) {
                RustQualifier ql;
                bool read = evidence && rust_qualifier_read(ctx, evidence, &ql);
                RustQualifier *globs = NULL;
                int glob_n = evidence ? 0 : rust_glob_evidence(ctx, tail, -1, &globs);
                agree = rust_agreeing_free_funcs(ctx, wanted, crate_len, read, &ql, globs, glob_n,
                                                 &agreeing);
            }
            if (agree == 1 && agreeing && (!identity_free || agreeing == identity_free)) {
                if (strcmp(wanted, tail) != 0) {
                    /* Called by its alias: the site joins by the name it spells, as
                     * Python's imported aliases do. */
                    rust_emit_resolved_call_reason(ctx, agreeing->qualified_name,
                                                   "lsp_import_alias", PDXE_RUST_CONF_PROMOTED,
                                                   path);
                } else {
                    rust_emit_resolved_call(ctx, agreeing->qualified_name, "lsp_short_name_unique",
                                            PDXE_RUST_CONF_PROMOTED);
                }
                return;
            }
        }

        if (identity_free) {
            rust_emit_resolved_call(ctx, qn, "lsp_direct", PDXE_RUST_CONF_DIRECT);
            return;
        }
        /* Last-ditch: emit with the resolved path. */
        rust_emit_unresolved_call(ctx, qn, "function_not_in_registry");
        return;
    }
}

static void rust_resolve_call_expression(RustLSPContext *ctx, TSNode node) {
    uint32_t saved_start = ctx->emit_site_start_byte;
    uint32_t saved_end = ctx->emit_site_end_byte;
    uint32_t mapped_start = 0;
    uint32_t mapped_end = 0;
    if (rust_map_source_range(ctx, ts_node_start_byte(node), ts_node_end_byte(node), &mapped_start,
                              &mapped_end)) {
        ctx->emit_site_start_byte = mapped_start;
        ctx->emit_site_end_byte = mapped_end;
    } else {
        ctx->emit_site_start_byte = 0;
        ctx->emit_site_end_byte = 0;
    }
    rust_resolve_call_expression_inner(ctx, node);
    ctx->emit_site_start_byte = saved_start;
    ctx->emit_site_end_byte = saved_end;
}

/* Walk every node in a function body, recording calls and refining scope
 * for control-flow constructs that bind variables. */
#define PDXE_RUST_EVAL_STEP_CAP 200000 /* per-file budget */
static void rust_resolve_calls_in_node(RustLSPContext *ctx, TSNode node) {
    if (ts_node_is_null(node))
        return;
    /* Pathological-input guard: bail out once we've spent too many
     * eval steps on this file. Prevents hangs on adversarial input. */
    if (ctx->eval_step_count > PDXE_RUST_EVAL_STEP_CAP) {
        if (ctx->eval_step_count == PDXE_RUST_EVAL_STEP_CAP + 1) {
            ctx->eval_step_count++; /* counted once per file */
            pdxe_lost_work();
        }
        return;
    }
    ctx->eval_step_count++;
    const char *kind = ts_node_type(node);

    /* Bind variables introduced by this statement. */
    rust_process_statement(ctx, node);

    /* Resolve a call expression. */
    if (strcmp(kind, "call_expression") == 0) {
        rust_resolve_call_expression(ctx, node);
        /* Closure-parameter inference: when the call is a known
         * iterator-style method that takes a closure of `Item`, stash
         * the receiver's element type so the closure_expression child
         * binds its first param to it. We compute this here (after
         * the call edge is emitted) so the recursion picks it up. */
        TSNode fn = ts_node_child_by_field_name(node, "function", 8);
        if (!ts_node_is_null(fn) && strcmp(ts_node_type(fn), "field_expression") == 0) {
            TSNode val = ts_node_child_by_field_name(fn, "value", 5);
            TSNode fld = ts_node_child_by_field_name(fn, "field", 5);
            if (!ts_node_is_null(val) && !ts_node_is_null(fld)) {
                char *mname = rust_node_text(ctx, fld);
                static const char *item_methods[] = {
                    "map",    "filter",     "for_each",   "find",        "position", "any",
                    "all",    "take_while", "skip_while", "filter_map",  "inspect",  "max_by",
                    "min_by", "max_by_key", "min_by_key", "sort_by_key", NULL};
                bool is_item_method = false;
                if (mname) {
                    for (const char **mm = item_methods; *mm; mm++) {
                        if (strcmp(mname, *mm) == 0) {
                            is_item_method = true;
                            break;
                        }
                    }
                }
                if (is_item_method) {
                    const PDXEType *recv = rust_eval_expr_type(ctx, val);
                    const PDXEType *base = recv;
                    while (base &&
                           (base->kind == PDXE_TYPE_REFERENCE || base->kind == PDXE_TYPE_POINTER)) {
                        base = (base->kind == PDXE_TYPE_REFERENCE) ? base->data.reference.elem
                                                                  : base->data.pointer.elem;
                    }
                    if (base && base->kind == PDXE_TYPE_TEMPLATE &&
                        base->data.template_type.arg_count > 0) {
                        const char *tn = base->data.template_type.template_name;
                        if (tn && (strstr(tn, "Iterator") || strstr(tn, "Vec") ||
                                   strstr(tn, "VecDeque") || strstr(tn, "Option") ||
                                   strstr(tn, "Result") || strstr(tn, "HashSet") ||
                                   strstr(tn, "BTreeSet") || strstr(tn, "Slice"))) {
                            ctx->pending_closure_param_type =
                                base->data.template_type.template_args[0];
                        }
                    } else if (base && base->kind == PDXE_TYPE_SLICE) {
                        ctx->pending_closure_param_type = base->data.slice.elem;
                    }
                }
            }
        }
        /* Continue recursion so calls inside arguments are also seen. */
    }

    /* Operator-overload desugaring: `a + b` calls <T as Add>::add when the
     * left operand is a user-defined type T with that operator method;
     * `a[i]` calls T::index. The tree-sitter-rust grammar models these as
     * binary_expression / index_expression rather than call_expression, so
     * lang_specs.c's call-type whitelist never sees them — we recover the
     * call edge here. Sound-only via rust_emit_operator_call (no edge unless
     * the operand type actually defines the method). */
    if (strcmp(kind, "binary_expression") == 0) {
        TSNode left = ts_node_child_by_field_name(node, "left", 4);
        if (!ts_node_is_null(left)) {
            PDXE_FOR_EACH_CHILD(it, node) {
                TSNode c = it.node;
                if (ts_node_is_named(c))
                    continue;
                char *op = rust_node_text(ctx, c);
                const char *method = rust_binop_trait_method(op);
                if (method) {
                    rust_emit_operator_call(ctx, rust_eval_expr_type(ctx, left), method, node);
                }
                break; /* operator is the sole anonymous child */
            }
        }
    } else if (strcmp(kind, "index_expression") == 0) {
        TSNode value = ts_node_child_by_field_name(node, "value", 5);
        if (ts_node_is_null(value) && ts_node_named_child_count(node) > 0) {
            value = ts_node_named_child(node, 0);
        }
        if (!ts_node_is_null(value)) {
            rust_emit_operator_call(ctx, rust_eval_expr_type(ctx, value), "index", node);
        }
    }

    /* Macro invocation: walk inner tokens for nested calls and try the
     * macro-as-function mapping. */
    if (strcmp(kind, "macro_invocation") == 0) {
        uint32_t saved_start = ctx->emit_site_start_byte;
        uint32_t saved_end = ctx->emit_site_end_byte;
        uint32_t mapped_start = 0;
        uint32_t mapped_end = 0;
        bool mapped = rust_map_source_range(ctx, ts_node_start_byte(node), ts_node_end_byte(node),
                                            &mapped_start, &mapped_end);
        ctx->emit_site_start_byte = mapped ? mapped_start : 0;
        ctx->emit_site_end_byte = mapped ? mapped_end : 0;

        uint32_t saved_macro_origin = ctx->macro_origin_byte;
        bool saved_macro_origin_valid = ctx->macro_origin_valid;
        /* A mapped macro-argument wrapper still identifies an exact original
         * occurrence; generated macro_rules! transcribers deliberately do not
         * map and therefore inherit the invocation that established them. */
        if (mapped) {
            ctx->macro_origin_byte = mapped_start;
            ctx->macro_origin_valid = true;
        }

        TSNode mname_node = ts_node_child_by_field_name(node, "macro", 5);
        if (!ts_node_is_null(mname_node)) {
            char *mname = rust_node_text(ctx, mname_node);
            if (mname) {
                /* A source-visible macro_rules! definition wins over prelude
                 * macros with the same name. Do this before applying any
                 * built-in argument grammar or canonical std path. */
                if (rust_has_visible_macro_definition(ctx, mname, node)) {
                    rust_expand_user_macro(ctx, mname, node);
                } else {
                    /* For known std macros emit a synthetic call under their
                     * canonical paths so trace tools can see the dependency. */
                    const char *path = NULL;
                    /* Macros whose arguments are ordinary Rust expressions — their
                     * call sites are lost to tree-sitter's macro tokenisation, so
                     * re-parse the args to recover calls like `format!("{}",
                     * d.label())`. */
                    bool expr_arg_macro =
                        strcmp(mname, "println") == 0 || strcmp(mname, "eprintln") == 0 ||
                        strcmp(mname, "print") == 0 || strcmp(mname, "eprint") == 0 ||
                        strcmp(mname, "format") == 0 || strcmp(mname, "write") == 0 ||
                        strcmp(mname, "writeln") == 0 || strcmp(mname, "panic") == 0 ||
                        strcmp(mname, "assert") == 0 || strcmp(mname, "assert_eq") == 0 ||
                        strcmp(mname, "assert_ne") == 0 || strcmp(mname, "debug_assert") == 0 ||
                        strcmp(mname, "debug_assert_eq") == 0 ||
                        strcmp(mname, "debug_assert_ne") == 0 || strcmp(mname, "dbg") == 0;
                    if (expr_arg_macro) {
                        rust_resolve_macro_arg_exprs(ctx, node);
                    }
                    if (strcmp(mname, "println") == 0 || strcmp(mname, "eprintln") == 0 ||
                        strcmp(mname, "print") == 0 || strcmp(mname, "eprint") == 0 ||
                        strcmp(mname, "format") == 0 || strcmp(mname, "write") == 0 ||
                        strcmp(mname, "writeln") == 0) {
                        path = pdxe_arena_sprintf(ctx->arena, "std.macros.%s", mname);
                    } else if (strcmp(mname, "vec") == 0) {
                        path = "alloc.vec.vec";
                    } else if (strcmp(mname, "panic") == 0) {
                        path = "core.panicking.panic";
                    } else if (strcmp(mname, "include") == 0) {
                        /* `include!` pulls another file in at compile time —
                         * we never see the included source. Emit a
                         * documentation edge so trace tools can flag it. */
                        path = "core.macros.include";
                    } else if (strcmp(mname, "include_str") == 0 ||
                               strcmp(mname, "include_bytes") == 0) {
                        /* Equivalent for data inclusion. */
                        path = pdxe_arena_sprintf(ctx->arena, "core.macros.%s", mname);
                    } else if (strcmp(mname, "env") == 0 || strcmp(mname, "option_env") == 0) {
                        /* Compile-time env var read. Specifically: an
                         * `include!(concat!(env!("OUT_DIR"), …))` pattern
                         * indicates code generated by a build.rs that we
                         * cannot see. We surface the env! call so trace
                         * tools know to look for OUT_DIR. */
                        path = pdxe_arena_sprintf(ctx->arena, "core.macros.%s", mname);
                    }
                    if (path) {
                        rust_emit_resolved_call(ctx, path, "lsp_macro", PDXE_RUST_CONF_MACRO_KNOWN);
                        rust_ensure_known_macro_carrier(ctx, mname, path);
                    }
                }
            }
        }
        TSNode args = ts_node_child_by_field_name(node, "arguments", 9);
        if (!ts_node_is_null(args))
            rust_walk_macro_tokens(ctx, args);
        ctx->macro_origin_byte = saved_macro_origin;
        ctx->macro_origin_valid = saved_macro_origin_valid;
        ctx->emit_site_start_byte = saved_start;
        ctx->emit_site_end_byte = saved_end;
        /* Don't recurse normally below — we already drilled into args. */
        return;
    }

    /* Push a fresh scope for blocks and constructs introducing new bindings. */
    bool push_scope =
        (strcmp(kind, "block") == 0 || strcmp(kind, "if_expression") == 0 ||
         strcmp(kind, "if_let_expression") == 0 || strcmp(kind, "while_expression") == 0 ||
         strcmp(kind, "while_let_expression") == 0 || strcmp(kind, "for_expression") == 0 ||
         strcmp(kind, "match_arm") == 0 || strcmp(kind, "closure_expression") == 0);
    bool uncertain_callable_flow =
        strcmp(kind, "if_expression") == 0 || strcmp(kind, "if_let_expression") == 0 ||
        strcmp(kind, "while_expression") == 0 || strcmp(kind, "while_let_expression") == 0 ||
        strcmp(kind, "for_expression") == 0 || strcmp(kind, "loop_expression") == 0 ||
        strcmp(kind, "match_arm") == 0 || strcmp(kind, "closure_expression") == 0;

    PDXEScope *saved = ctx->current_scope;
    if (push_scope) {
        ctx->current_scope = pdxe_scope_push(ctx->arena, ctx->current_scope);
    }
    /* Issue 114: a block that declares a `use` is the scope its bindings are seen
     * in, inside it only. */
    int saved_use_scope = ctx->use_scope;
    if (strcmp(kind, "block") == 0) {
        int declared = rust_use_scope_of(ctx, node);
        if (declared >= 0) {
            ctx->use_scope = declared;
        }
    }
    /* Issue 104: a function declared in a block is a local item of the block,
     * named everywhere in it, before its declaration too: a call of that name
     * is the local function's, never a namesake's elsewhere in the project. */
    if (strcmp(kind, "block") == 0) {
        PDXE_FOR_EACH_NAMED_CHILD(fit, node) {
            if (strcmp(ts_node_type(fit.node), "function_item") != 0)
                continue;
            TSNode fname = ts_node_child_by_field_name(fit.node, "name", 4);
            char *local = ts_node_is_null(fname) ? NULL : rust_node_text(ctx, fname);
            if (local)
                pdxe_scope_bind(ctx->current_scope, local, pdxe_type_unknown());
        }
    }
    if (uncertain_callable_flow) {
        ctx->callable_control_flow_depth++;
    }

    /* if_let / while_let bind a pattern from the value's matched form.
     * Modern tree-sitter-rust parses `if let X = y { ... }` as
     * `if_expression` containing a `let_condition` child rather than as
     * the legacy `if_let_expression`. Same for `while let`. We handle
     * both shapes here. */
    if (strcmp(kind, "if_let_expression") == 0 || strcmp(kind, "while_let_expression") == 0) {
        TSNode pat = ts_node_child_by_field_name(node, "pattern", 7);
        TSNode val = ts_node_child_by_field_name(node, "value", 5);
        if (!ts_node_is_null(pat) && !ts_node_is_null(val)) {
            const PDXEType *vt = rust_eval_expr_type(ctx, val);
            rust_bind_pattern(ctx, pat, vt);
        }
    }
    if (strcmp(kind, "if_expression") == 0 || strcmp(kind, "while_expression") == 0) {
        /* Look for a let_condition (or let_chain) anywhere in the
         * condition slot. */
        TSNode cond = ts_node_child_by_field_name(node, "condition", 9);
        if (!ts_node_is_null(cond)) {
            /* Walk one level down for let_condition / let_chain. */
            const char *ck = ts_node_type(cond);
            TSNode targets[8];
            int tcount = 0;
            if (strcmp(ck, "let_condition") == 0) {
                targets[tcount++] = cond;
            } else if (strcmp(ck, "let_chain") == 0) {
                PDXE_FOR_EACH_NAMED_CHILD(it, cond) {
                    if (!(tcount < 8)) {
                        break;
                    }
                    TSNode c2 = it.node;
                    if (strcmp(ts_node_type(c2), "let_condition") == 0) {
                        targets[tcount++] = c2;
                    }
                }
            }
            for (int t = 0; t < tcount; t++) {
                TSNode lc = targets[t];
                TSNode pat = ts_node_child_by_field_name(lc, "pattern", 7);
                TSNode val = ts_node_child_by_field_name(lc, "value", 5);
                if (!ts_node_is_null(pat) && !ts_node_is_null(val)) {
                    const PDXEType *vt = rust_eval_expr_type(ctx, val);
                    rust_bind_pattern(ctx, pat, vt);
                }
            }
        }
    }

    /* for_expression: bind the loop variable from the iter's element type. */
    if (strcmp(kind, "for_expression") == 0) {
        TSNode pat = ts_node_child_by_field_name(node, "pattern", 7);
        TSNode val = ts_node_child_by_field_name(node, "value", 5);
        if (!ts_node_is_null(pat) && !ts_node_is_null(val)) {
            const PDXEType *vt = rust_eval_expr_type(ctx, val);
            const PDXEType *base = vt;
            while (base && base->kind == PDXE_TYPE_REFERENCE)
                base = base->data.reference.elem;
            const PDXEType *elem = pdxe_type_unknown();
            if (base && base->kind == PDXE_TYPE_SLICE) {
                elem = base->data.slice.elem;
            } else if (base && base->kind == PDXE_TYPE_TEMPLATE) {
                const char *nm = base->data.template_type.template_name;
                if ((strstr(nm, "Vec") || strstr(nm, "VecDeque") || strstr(nm, "Iterator") ||
                     strstr(nm, "Range")) &&
                    base->data.template_type.arg_count > 0) {
                    elem = base->data.template_type.template_args[0];
                }
                if ((strstr(nm, "HashMap") || strstr(nm, "BTreeMap")) &&
                    base->data.template_type.arg_count > 1) {
                    /* Iter over (K, V) tuples. */
                    const PDXEType *pair[2] = {base->data.template_type.template_args[0],
                                              base->data.template_type.template_args[1]};
                    elem = pdxe_type_tuple(ctx->arena, pair, 2);
                }
            }
            rust_bind_pattern(ctx, pat, elem);
        }
    }

    /* match_expression: per-arm scope handled when we descend. */
    if (strcmp(kind, "match_arm") == 0) {
        /* The match value type is captured by the parent walker — best
         * effort: peek at the arm's pattern and let rust_bind_pattern do the
         * work using pdxe_type_unknown() if we cannot derive it. */
        /* Issue 104: the names the arm's pattern binds are locals of the arm,
         * so `Ok(Self(params)) => Ok(Some(Self(params)))` passes the local
         * `params`, never the module's function of that name. The pattern is
         * the match pattern's first named child; a guard follows it. */
        TSNode mp = ts_node_child_by_field_name(node, "pattern", 7);
        if (!ts_node_is_null(mp)) {
            TSNode pat = mp;
            if (strcmp(ts_node_type(mp), "match_pattern") == 0) {
                pat = (TSNode){0};
                PDXE_FOR_EACH_NAMED_CHILD(pit, mp) {
                    pat = pit.node;
                    break;
                }
            }
            rust_bind_pattern(ctx, pat, pdxe_type_unknown());
        }
    }

    /* closure_expression: bind closure parameters.
     *
     * Priority order for each param:
     *   1. Explicit type annotation (`|n: &i32|`) — use it directly.
     *   2. `ctx->pending_closure_param_type` for the FIRST param when the
     *      surrounding call resolver inferred one (`.map(|x| ...)` on
     *      Iterator<T>).
     *   3. Otherwise unknown.
     *
     * After binding, clear the pending type so it doesn't leak into a
     * sibling closure. */
    if (strcmp(kind, "closure_expression") == 0) {
        const PDXEType *hint = ctx->pending_closure_param_type;
        ctx->pending_closure_param_type = NULL;
        TSNode params = ts_node_child_by_field_name(node, "parameters", 10);
        if (!ts_node_is_null(params)) {
            PDXE_FOR_EACH_NAMED_CHILD(it, params) {
                TSNode p = it.node;
                /* For `parameter`-shaped nodes, peel off the type
                 * annotation if present; otherwise treat the whole node
                 * as the pattern. */
                TSNode pat = p;
                const PDXEType *bound = (it.index == 0 && hint) ? hint : pdxe_type_unknown();
                if (strcmp(ts_node_type(p), "parameter") == 0) {
                    TSNode tn = ts_node_child_by_field_name(p, "type", 4);
                    TSNode pn = ts_node_child_by_field_name(p, "pattern", 7);
                    if (!ts_node_is_null(pn))
                        pat = pn;
                    if (!ts_node_is_null(tn)) {
                        bound = rust_parse_type_node(ctx, tn);
                    }
                }
                rust_bind_pattern(ctx, pat, bound);
            }
        }
    }

    /* Recurse. An item a block declares (a function, an impl) brings its own generic
     * parameters into scope in its body (issue 109). */
    int saved_generic_names = ctx->generic_name_count;
    if (strcmp(kind, "function_item") == 0 || strcmp(kind, "impl_item") == 0) {
        rust_push_generic_names(ctx, node);
    }
    PDXE_FOR_EACH_CHILD(it, node) {
        TSNode c = it.node;
        if (!ts_node_is_null(c)) {
            rust_resolve_calls_in_node(ctx, c);
        }
    }
    ctx->generic_name_count = saved_generic_names;

    if (push_scope) {
        ctx->current_scope = saved;
    }
    ctx->use_scope = saved_use_scope;
    if (uncertain_callable_flow) {
        ctx->callable_control_flow_depth--;
    }
}

/* Process a single function: bind parameters / `self`, then walk body. */
static void rust_process_function(RustLSPContext *ctx, TSNode func_node, const char *parent_qn) {
    TSNode name_node = ts_node_child_by_field_name(func_node, "name", 4);
    if (ts_node_is_null(name_node))
        return;
    char *name = rust_node_text(ctx, name_node);
    if (!name || !name[0])
        return;

    const char *prefix = parent_qn ? parent_qn : ctx->module_qn;
    ctx->enclosing_func_qn = pdxe_arena_sprintf(ctx->arena, "%s.%s", prefix, name);

    PDXEScope *saved = ctx->current_scope;
    ctx->current_scope = pdxe_scope_push(ctx->arena, ctx->current_scope);

    /* Issue 109: the function's generic parameters are in scope in its body. */
    int saved_generic_names = ctx->generic_name_count;
    rust_push_generic_names(ctx, func_node);
    int saved_where = ctx->where_count; /* issue 105 */
    rust_push_where_predicates(ctx, func_node);

    /* Chalk-lite: capture the active function's type-parameter bounds
     * and where-clause bounds into ctx so dispatch through generic
     * receivers can route via the bound trait. */
    int saved_bound_count = ctx->type_param_bound_count;
    TSNode tp_list = ts_node_child_by_field_name(func_node, "type_parameters", 15);
    if (!ts_node_is_null(tp_list)) {
        char *tp_text = rust_node_text(ctx, tp_list);
        if (tp_text)
            rust_collect_bounds_from_text(ctx, tp_text);
    }
    TSNode where_clause = ts_node_child_by_field_name(func_node, "where_clause", 12);
    if (!ts_node_is_null(where_clause)) {
        char *wt = rust_node_text(ctx, where_clause);
        if (wt)
            rust_collect_bounds_from_text(ctx, wt);
    }

    /* Bind self for impl/trait methods. */
    TSNode params = ts_node_child_by_field_name(func_node, "parameters", 10);
    if (!ts_node_is_null(params)) {
        PDXE_FOR_EACH_NAMED_CHILD(it, params) {
            TSNode p = it.node;
            const char *pk = ts_node_type(p);
            if (strcmp(pk, "self_parameter") == 0) {
                if (ctx->self_type_qn) {
                    /* Determine if &self / &mut self / self by reference. */
                    char *text = rust_node_text(ctx, p);
                    const PDXEType *self_t = pdxe_type_named(ctx->arena, ctx->self_type_qn);
                    if (text && strchr(text, '&')) {
                        self_t = pdxe_type_reference(ctx->arena, self_t);
                    }
                    pdxe_scope_bind(ctx->current_scope, "self", self_t);
                }
                continue;
            }
            if (strcmp(pk, "parameter") == 0) {
                TSNode pat = ts_node_child_by_field_name(p, "pattern", 7);
                TSNode tn = ts_node_child_by_field_name(p, "type", 4);
                const PDXEType *pt =
                    ts_node_is_null(tn) ? pdxe_type_unknown() : rust_parse_type_node(ctx, tn);
                if (!ts_node_is_null(pat))
                    rust_bind_pattern(ctx, pat, pt);
            }
        }
    }

    /* Walk function body. */
    TSNode body = ts_node_child_by_field_name(func_node, "body", 4);
    if (!ts_node_is_null(body)) {
        rust_resolve_calls_in_node(ctx, body);
    }

    ctx->current_scope = saved;
    ctx->enclosing_func_qn = NULL;
    /* Restore bound-env count so the caller's bounds are unaffected. */
    ctx->type_param_bound_count = saved_bound_count;
    ctx->generic_name_count = saved_generic_names;
    ctx->where_count = saved_where;
}

/* Walk an `impl_item`, processing each `function_item` inside its body
 * with the appropriate `self_type_qn` (and `self_trait_qn` for trait impls). */
/* Chalk-lite: record a `T: Trait` bound in the per-function bound
 * environment. The arrays grow by 8 to keep allocs cheap. */
static void rust_record_type_param_bound(RustLSPContext *ctx, const char *param_name,
                                         const char *trait_qn) {
    if (!ctx || !param_name || !trait_qn)
        return;
    /* Grow by 8s. */
    if (ctx->type_param_bound_count % 8 == 0) {
        int new_cap = ctx->type_param_bound_count + 8;
        void *narr = pdxe_arena_alloc(ctx->arena, new_cap * sizeof(*ctx->type_param_bounds));
        if (!narr)
            return;
        if (ctx->type_param_bounds && ctx->type_param_bound_count > 0) {
            memcpy(narr, ctx->type_param_bounds,
                   ctx->type_param_bound_count * sizeof(*ctx->type_param_bounds));
        }
        ctx->type_param_bounds = narr;
    }
    ctx->type_param_bounds[ctx->type_param_bound_count].param_name =
        pdxe_arena_strdup(ctx->arena, param_name);
    ctx->type_param_bounds[ctx->type_param_bound_count].trait_qn =
        pdxe_arena_strdup(ctx->arena, trait_qn);
    ctx->type_param_bound_count++;
}

/* Look up the first trait bound for a given type-param name. Returns
 * NULL if `name` has no bound recorded. */
static const char *rust_lookup_type_param_bound(RustLSPContext *ctx, const char *name) {
    if (!ctx || !name)
        return NULL;
    for (int i = 0; i < ctx->type_param_bound_count; i++) {
        if (strcmp(ctx->type_param_bounds[i].param_name, name) == 0) {
            return ctx->type_param_bounds[i].trait_qn;
        }
    }
    return NULL;
}

/* Parse the impl/function's <T: Bound + Bound, U: Bound> + where clause
 * text into the per-context bound environment. We don't reason about
 * lifetimes; we record only trait bounds and associated-type bindings.
 *
 * Format we accept (simplified TOML-like grammar):
 *   `<T: Clone + Debug, U: Iterator<Item = V>>`
 *   `where T: Clone, U: Iterator<Item = V>`
 *
 * Multiple bounds are split on `+` (top-level), entries on `,`. */
static void rust_collect_bounds_from_text(RustLSPContext *ctx, const char *text) {
    if (!ctx || !text)
        return;
    /* Walk text, find segments separated by `,` at top depth. For each
     * segment, split on `:` to get (param, bound-list); split bounds on `+`
     * at top depth. Resolve each bound through the path resolver to its QN. */
    int len = (int)strlen(text);
    int from = 0;
    while (from < len) {
        /* Skip whitespace + leading 'where'/punct. */
        while (from < len && (text[from] == ' ' || text[from] == '\n' || text[from] == '<' ||
                              text[from] == ',' || text[from] == '>' || text[from] == 'w')) {
            if (text[from] == 'w' && from + 5 < len && strncmp(text + from, "where", 5) == 0) {
                from += 5;
            } else {
                from++;
            }
        }
        if (from >= len)
            break;

        /* Param name. */
        int name_start = from;
        if (text[from] == '\'') {
            /* Lifetime — skip. */
            from++;
            while (from < len && (isalnum((unsigned char)text[from]) || text[from] == '_'))
                from++;
            continue;
        }
        while (from < len && (isalnum((unsigned char)text[from]) || text[from] == '_'))
            from++;
        int name_end = from;
        if (name_end == name_start) {
            from++;
            continue;
        }
        char *pname =
            pdxe_arena_strndup(ctx->arena, text + name_start, (size_t)(name_end - name_start));
        /* Look for `:`. */
        while (from < len && (text[from] == ' ' || text[from] == '\t'))
            from++;
        if (from >= len || text[from] != ':') {
            /* No bound; skip to next entry. */
            while (from < len && text[from] != ',' && text[from] != '>' && text[from] != '\n')
                from++;
            continue;
        }
        from++; /* consume `:` */

        /* Bound list: split on `+` at depth 0, terminated by `,` / `>` /
         * end of where clause. */
        int depth = 0;
        int bound_start = from;
        while (from < len) {
            char c = text[from];
            if (c == '<' || c == '(' || c == '[')
                depth++;
            else if (c == '>' || c == ')' || c == ']') {
                if (depth == 0)
                    break;
                depth--;
            } else if (depth == 0 && (c == '+' || c == ',' || c == '\n')) {
                /* End of one bound. */
                int b_end = from;
                /* Trim trailing whitespace. */
                while (b_end > bound_start && (text[b_end - 1] == ' ' || text[b_end - 1] == '\t')) {
                    b_end--;
                }
                /* Trim leading whitespace. */
                int b_start = bound_start;
                while (b_start < b_end && (text[b_start] == ' ' || text[b_start] == '\t')) {
                    b_start++;
                }
                if (b_end > b_start) {
                    char *btext =
                        pdxe_arena_strndup(ctx->arena, text + b_start, (size_t)(b_end - b_start));
                    /* Strip any `<…>` associated-type suffix for the
                     * trait QN lookup — we keep the trait name only. */
                    char *langle = strchr(btext, '<');
                    if (langle)
                        *langle = '\0';
                    const char *qn = rust_resolve_path_expr(ctx, btext);
                    if (qn) {
                        rust_record_type_param_bound(ctx, pname, qn);
                    }
                }
                if (c == '+') {
                    from++;
                    bound_start = from;
                    continue;
                }
                /* End of entry. */
                break;
            }
            from++;
        }
        /* Advance past entry terminator. */
        if (from < len && (text[from] == ',' || text[from] == '\n'))
            from++;
    }
}

/* Helper: does `name` appear in the impl's `<T, U, ...>` type
 * parameter list? Used to detect blanket impls (impl<T: Trait> X for T). */
static bool rust_impl_has_type_param(RustLSPContext *ctx, TSNode impl_node, const char *name) {
    if (!name)
        return false;
    TSNode tp = ts_node_child_by_field_name(impl_node, "type_parameters", 15);
    if (ts_node_is_null(tp))
        return false;
    PDXE_FOR_EACH_NAMED_CHILD(it, tp) {
        TSNode c = it.node;
        const char *ck = ts_node_type(c);
        if (strcmp(ck, "type_identifier") == 0 || strcmp(ck, "constrained_type_parameter") == 0) {
            char *nm = rust_node_text(ctx, c);
            if (!nm)
                continue;
            /* For constrained_type_parameter, the name is the
             * first identifier-y child. */
            if (strcmp(ck, "constrained_type_parameter") == 0) {
                TSNode lhs = ts_node_child_by_field_name(c, "left", 4);
                if (!ts_node_is_null(lhs))
                    nm = rust_node_text(ctx, lhs);
            }
            if (nm && strcmp(nm, name) == 0)
                return true;
        }
    }
    return false;
}

static void rust_process_impl(RustLSPContext *ctx, TSNode impl_node) {
    TSNode type_node = ts_node_child_by_field_name(impl_node, "type", 4);
    if (ts_node_is_null(type_node))
        return;
    /* Issue 105: `impl<S, E> MethodRouter<S, E>` is MethodRouter's: `self` and `Self` are
     * that type, its path without the arguments. Its methods' callers keep the text as
     * written (below), which is how extraction names the owner of their calls. */
    TSNode self_node = type_node;
    if (strcmp(ts_node_type(type_node), "generic_type") == 0) {
        TSNode base = ts_node_child_by_field_name(type_node, "type", 4);
        if (!ts_node_is_null(base)) {
            self_node = base;
        }
    }
    char *type_text = rust_node_text(ctx, type_node);
    if (!type_text)
        return;

    /* Detect blanket impl: `impl<T: Trait> ForeignTrait for T { ... }`
     * where type_text is a name that appears in the impl's type
     * parameters. In that case the receiver isn't a concrete type — it's
     * any T satisfying the bound. We register the methods on the trait
     * QN itself so dispatch through T: Trait finds them. */
    TSNode trait_node = ts_node_child_by_field_name(impl_node, "trait", 5);
    bool is_blanket =
        !ts_node_is_null(trait_node) && rust_impl_has_type_param(ctx, impl_node, type_text);
    const char *effective_recv = NULL;

    if (is_blanket) {
        char *tt = rust_node_text(ctx, trait_node);
        if (tt)
            effective_recv = rust_resolve_path_expr(ctx, tt);
    } else {
        effective_recv = rust_resolve_path_expr(ctx, type_text);
    }
    if (!effective_recv)
        return;

    const char *saved_self = ctx->self_type_qn;
    const char *saved_self_path = ctx->self_path_qn;
    const char *saved_trait = ctx->self_trait_qn;
    ctx->self_type_qn = effective_recv;
    ctx->self_path_qn = effective_recv;
    if (!is_blanket && !ts_node_eq(self_node, type_node)) {
        char *self_text = rust_node_text(ctx, self_node);
        const char *self_qn = self_text ? rust_resolve_path_expr(ctx, self_text) : NULL;
        if (self_qn) {
            ctx->self_type_qn = self_qn;
        }
    }
    ctx->self_trait_qn = NULL;
    /* Issue 113B: an impl's methods are the callers of their calls as extraction names
     * their owner, the file's module and the impl's type as written, whatever the type
     * resolves to (an imported `HeaderMap` named through its import joined no site). A
     * blanket impl's keep the trait's. */
    const char *caller_parent =
        is_blanket || !ctx->module_qn
            ? effective_recv
            : pdxe_arena_sprintf(ctx->arena, "%s.%s", ctx->module_qn, type_text);
    if (!caller_parent) {
        caller_parent = effective_recv;
    }
    /* Issue 109: the impl's generic parameters are in scope in its methods. */
    int saved_generic_names = ctx->generic_name_count;
    rust_push_generic_names(ctx, impl_node);
    int saved_where = ctx->where_count; /* issue 105 */
    rust_push_where_predicates(ctx, impl_node);

    if (!ts_node_is_null(trait_node) && !is_blanket) {
        char *tt = rust_node_text(ctx, trait_node);
        if (tt)
            ctx->self_trait_qn = rust_resolve_path_expr(ctx, tt);
    }

    TSNode body = ts_node_child_by_field_name(impl_node, "body", 4);
    if (!ts_node_is_null(body)) {
        PDXE_FOR_EACH_CHILD(it, body) {
            TSNode c = it.node;
            if (ts_node_is_null(c) || !ts_node_is_named(c))
                continue;
            const char *ck = ts_node_type(c);
            if (strcmp(ck, "function_item") == 0) {
                rust_process_function(ctx, c, caller_parent);
            }
        }
    }

    ctx->self_type_qn = saved_self;
    ctx->self_path_qn = saved_self_path;
    ctx->self_trait_qn = saved_trait;
    ctx->generic_name_count = saved_generic_names;
    ctx->where_count = saved_where;
}

void rust_lsp_process_file(RustLSPContext *ctx, TSNode root) {
    if (ts_node_is_null(root))
        return;

    /* Pass 0: collect macro_rules! definitions before walking bodies so
     * macro_invocation handlers can expand user-defined macros. */
    rust_collect_macro_rules(ctx, root);

    /* Record bare `mod foo;` declarations (file links). The pipeline
     * uses these to know which sibling files to include in cross-file
     * resolution. We just store them as Imports with a `mod:` prefix
     * so the pipeline can distinguish them from `use` imports.
     *
     * `mod foo { ... }` (inline module) is NOT recorded — the body is
     * already in this file. Only bare `mod foo;` declarations are. */
    {
        PDXE_FOR_EACH_CHILD(it, root) {
            TSNode c = it.node;
            if (ts_node_is_null(c))
                continue;
            if (strcmp(ts_node_type(c), "mod_item") != 0)
                continue;
            /* Inline mod has a `body` field; bare decl does not. */
            TSNode body = ts_node_child_by_field_name(c, "body", 4);
            if (!ts_node_is_null(body))
                continue; /* inline */
            TSNode mname = ts_node_child_by_field_name(c, "name", 4);
            if (ts_node_is_null(mname))
                continue;
            char *name = rust_node_text(ctx, mname);
            if (!name)
                continue;
            pdxe_idxmemo_put_if_absent(&ctx->declared_modules, ctx->arena, name, 1);
            /* Surface as a synthetic CALLS edge from "<module>" to
             * the sibling module so the cross-file pass picks it up
             * via short-name fallback. We attribute it to the file's
             * synthetic module-scope caller. */
            const char *save_caller = ctx->enclosing_func_qn;
            ctx->enclosing_func_qn = ctx->module_qn;
            rust_emit_resolved_call(ctx,
                                    pdxe_arena_sprintf(ctx->arena, "%s.%s", ctx->module_qn, name),
                                    "lsp_mod_decl", 0.70f);
            ctx->enclosing_func_qn = save_caller;
        }
    }

    /* Pass 1: bind module-level const/static so functions can see them. */
    PDXE_FOR_EACH_CHILD(it, root) {
        TSNode c = it.node;
        if (ts_node_is_null(c))
            continue;
        const char *ck = ts_node_type(c);
        if (strcmp(ck, "const_item") == 0 || strcmp(ck, "static_item") == 0) {
            rust_process_statement(ctx, c);
        }
    }

    /* Pass 2: walk every top-level item. */
    PDXE_FOR_EACH_CHILD(kid_it, root) {
        TSNode c = kid_it.node;
        if (ts_node_is_null(c))
            continue;
        const char *ck = ts_node_type(c);
        if (strcmp(ck, "function_item") == 0) {
            rust_process_function(ctx, c, NULL);
        } else if (strcmp(ck, "impl_item") == 0) {
            rust_process_impl(ctx, c);
        } else if (strcmp(ck, "mod_item") == 0) {
            /* Inline module — recurse into its declaration_list, where its own
             * `use` declarations are in force and the file's are not (issue 114). */
            TSNode body = ts_node_child_by_field_name(c, "body", 4);
            if (!ts_node_is_null(body)) {
                int saved_use_scope = ctx->use_scope;
                int declared = rust_use_scope_of(ctx, body);
                if (declared >= 0) {
                    ctx->use_scope = declared;
                }
                PDXE_FOR_EACH_CHILD(it2, body) {
                    TSNode mc = it2.node;
                    if (ts_node_is_null(mc))
                        continue;
                    const char *mck = ts_node_type(mc);
                    if (strcmp(mck, "function_item") == 0) {
                        rust_process_function(ctx, mc, NULL);
                    } else if (strcmp(mck, "impl_item") == 0) {
                        rust_process_impl(ctx, mc);
                    }
                }
                ctx->use_scope = saved_use_scope;
            }
        }
    }
}

/* ════════════════════════════════════════════════════════════════════
 * 11. Per-file entry: build registry + run
 * ════════════════════════════════════════════════════════════════════ */

/* Issue 111: a `use` tree read from the syntax tree, never from its text, so that
 * nested and multi-line lists, comments, aliases, `self`, globs and a visibility
 * before `use` all mean what Rust says. The pinned grammar's shapes:
 *
 *   use_declaration: visibility_modifier? `use` argument: <tree> `;`
 *   <tree>: identifier | crate | self | super | scoped_identifier (path?, name)
 *         | scoped_use_list (path?, list: use_list)
 *         | use_list ({ <tree> or a comment, ... })
 *         | use_as_clause (path, alias)
 *         | use_wildcard (<path>?)
 *
 * The path a path node spells, its segments joined by `::`, a leading `::` left
 * out: in Rust 2018 it names an extern crate, as the bare name does. NULL for a
 * node that is no path (a macro's metavariable). */
static char *rust_use_path(RustLSPContext *ctx, TSNode node) {
    if (ts_node_is_null(node)) {
        return NULL;
    }
    const char *kind = ts_node_type(node);
    if (strcmp(kind, "identifier") == 0 || strcmp(kind, "crate") == 0 ||
        strcmp(kind, "self") == 0 || strcmp(kind, "super") == 0) {
        return rust_node_text(ctx, node);
    }
    if (strcmp(kind, "scoped_identifier") != 0) {
        return NULL;
    }
    char *name = rust_use_path(ctx, ts_node_child_by_field_name(node, "name", 4));
    TSNode head = ts_node_child_by_field_name(node, "path", 4);
    if (!name || ts_node_is_null(head)) {
        return name;
    }
    char *path = rust_use_path(ctx, head);
    return path ? pdxe_arena_sprintf(ctx->arena, "%s::%s", path, name) : NULL;
}

/* `prefix::path`, or `path` alone at the top of a tree. */
static char *rust_use_join(RustLSPContext *ctx, const char *prefix, const char *path) {
    if (!path) {
        return NULL;
    }
    return prefix ? pdxe_arena_sprintf(ctx->arena, "%s::%s", prefix, path)
                  : pdxe_arena_strdup(ctx->arena, path);
}

static int rust_enclosing_module(RustLSPContext *ctx, int s, int k);
static int rust_super_count(const char *path);

/* Binds `name` to `path` in scope `s`, unless the name is no binding: `_` (`use Trait
 * as _;`), or a keyword a path ends in (`use super::{self};`, which Rust rejects).
 *
 * Issue 114: `use super::x` in an inline module names what its enclosing module binds
 * `x` to when that module imports it: the enclosing module's own `use` bindings, read
 * before this one (scopes are read outside in). A name the enclosing module defines
 * itself keeps the path, which names that module. */
static void rust_use_bind(RustLSPContext *ctx, int s, const char *name, const char *path) {
    if (!name || !path || !name[0] || strcmp(name, "_") == 0 || strcmp(name, "self") == 0 ||
        strcmp(name, "super") == 0 || strcmp(name, "crate") == 0) {
        return;
    }
    const char *last = strrchr(path, ':');
    if (last && last > path && last[-1] == ':') {
        size_t head_len = (size_t)(last - 1 - path);
        char *head = pdxe_arena_strndup(ctx->arena, path, head_len);
        int k = head ? rust_super_count(head) : 0;
        if (k > 0 && k <= ctx->use_scopes[s].module_depth) {
            int module = rust_enclosing_module(ctx, s, k);
            bool withheld = false;
            const char *imported =
                module >= 0 ? rust_scope_lookup(ctx, module, last + 1, &withheld) : NULL;
            if (imported) {
                path = imported;
            }
        }
    }
    rust_scope_add_use(ctx, &ctx->use_scopes[s], name, path);
}

/* Issue 114: the scope of the module `k` modules out from scope `s`'s own module
 * (`super` k times), or -1 past the file. */
static int rust_enclosing_module(RustLSPContext *ctx, int s, int k) {
    while (s >= 0 && !ctx->use_scopes[s].module) {
        s = ctx->use_scopes[s].parent;
    }
    for (; s >= 0 && k > 0; k--) {
        s = ctx->use_scopes[s].parent;
        while (s >= 0 && !ctx->use_scopes[s].module) {
            s = ctx->use_scopes[s].parent;
        }
    }
    return s;
}

/* How many `super` segments `path` is made of, 0 when it is anything else. */
static int rust_super_count(const char *path) {
    int k = 0;
    for (const char *p = path; *p;) {
        if (strncmp(p, "super", 5) != 0 || (p[5] != '\0' && strncmp(p + 5, "::", 2) != 0)) {
            return 0;
        }
        k++;
        p += 5;
        if (*p) {
            p += 2;
        }
    }
    return k;
}

/* Adds what one use tree binds in scope `s`, below `prefix` (NULL at the top): each
 * name to the path it is imported by, in source order, and each glob's module. */
static void rust_collect_use_tree(RustLSPContext *ctx, TSNode tree, const char *prefix, int s) {
    const char *kind = ts_node_type(tree);
    if (strcmp(kind, "use_list") == 0) {
        PDXE_FOR_EACH_NAMED_CHILD(it, tree) {
            rust_collect_use_tree(ctx, it.node, prefix, s);
        }
        return;
    }
    if (strcmp(kind, "scoped_use_list") == 0) {
        TSNode path = ts_node_child_by_field_name(tree, "path", 4);
        TSNode list = ts_node_child_by_field_name(tree, "list", 4);
        const char *base = prefix;
        if (!ts_node_is_null(path)) {
            base = rust_use_join(ctx, prefix, rust_use_path(ctx, path));
            if (!base) {
                return;
            }
        }
        if (!ts_node_is_null(list)) {
            rust_collect_use_tree(ctx, list, base, s);
        }
        return;
    }
    if (strcmp(kind, "use_as_clause") == 0) {
        TSNode path = ts_node_child_by_field_name(tree, "path", 4);
        TSNode alias = ts_node_child_by_field_name(tree, "alias", 5);
        if (ts_node_is_null(path) || ts_node_is_null(alias)) {
            return;
        }
        /* `{self as name}` names the prefix itself. */
        const char *full = strcmp(ts_node_type(path), "self") == 0 && prefix
                               ? prefix
                               : rust_use_join(ctx, prefix, rust_use_path(ctx, path));
        rust_use_bind(ctx, s, rust_node_text(ctx, alias), full);
        return;
    }
    if (strcmp(kind, "use_wildcard") == 0) {
        TSNode path = (TSNode){0};
        PDXE_FOR_EACH_NAMED_CHILD(it, tree) {
            path = it.node;
            break;
        }
        const char *module =
            ts_node_is_null(path) ? prefix : rust_use_join(ctx, prefix, rust_use_path(ctx, path));
        if (module) {
            rust_scope_add_glob(ctx, &ctx->use_scopes[s], convert_path_to_qn(ctx->arena, module));
            /* Issue 114: `use super::*` in an inline module names an enclosing
             * module of this file, whose own `use` bindings it brings in too. */
            int k = rust_super_count(module);
            if (k > 0 && k <= ctx->use_scopes[s].module_depth) {
                rust_scope_add_link(ctx, &ctx->use_scopes[s], rust_enclosing_module(ctx, s, k));
            }
        }
        return;
    }
    if (strcmp(kind, "self") == 0 && prefix) {
        /* `{self}` binds the prefix under its last name. */
        rust_use_bind(ctx, s, path_last_segment(prefix), prefix);
        return;
    }
    /* A path; anything else (a comment, a metavariable) binds nothing. */
    char *full = rust_use_join(ctx, prefix, rust_use_path(ctx, tree));
    if (full) {
        rust_use_bind(ctx, s, path_last_segment(full), full);
    }
}

/* Issue 114: what pdxe_rust_export_use_scopes records into, as the collector goes. */
typedef struct {
    uint32_t start, end;
    int call;
} RustCallOccurrence;

typedef struct RustScopeExport {
    PDXEFileResult *result;
    RustCallOccurrence *calls; /* the calls to place, by occurrence */
    int call_count;
    bool failed;
} RustScopeExport;

/* The last name of an import's text, as the extractor reads one (extract_imports.c's
 * path_last), so a scoped import is recorded exactly as the file's own are. */
static const char *rust_import_last(PDXEArena *a, const char *path) {
    const char *last_sep = NULL;
    for (const char *p = path; *p; p++) {
        if (*p == '/' || *p == '.' || *p == ':' || *p == '\\') {
            last_sep = p;
        }
    }
    return last_sep ? pdxe_arena_strdup(a, last_sep + 1) : path;
}

/* Records a `use` declared in scope `s` (not the file's), as the file's own imports are
 * recorded: its text without `use ` and `;`, and that text's last name. */
static void rust_export_scoped_import(RustLSPContext *ctx, TSNode decl, int s) {
    RustScopeExport *ex = ctx->scope_export;
    char *full = pdxe_node_text(ctx->arena, decl, ctx->source);
    if (!full) {
        ex->failed = true;
        return;
    }
    if (strncmp(full, "use ", 4) == 0) {
        full += 4;
    }
    size_t len = strlen(full);
    if (len > 0 && full[len - 1] == ';') {
        full[len - 1] = '\0';
    }
    PDXEScopedImportArray *arr = &ex->result->scoped_imports;
    if (arr->count >= arr->cap) {
        int cap = arr->cap ? arr->cap * 2 : 8;
        PDXEScopedImport *grown =
            (PDXEScopedImport *)pdxe_arena_alloc(ctx->arena, (size_t)cap * sizeof(*grown));
        if (!grown) {
            ex->failed = true;
            return;
        }
        if (arr->count > 0) {
            memcpy(grown, arr->items, (size_t)arr->count * sizeof(*grown));
        }
        arr->items = grown;
        arr->cap = cap;
    }
    PDXEScopedImport *imp = &arr->items[arr->count++];
    imp->import.local_name = rust_import_last(ctx->arena, full);
    imp->import.module_path = full;
    imp->scope = s;
}

/* Places the calls whose occurrence is exactly `node` in scope `s`. */
static void rust_export_place_calls(RustLSPContext *ctx, TSNode node, int s) {
    RustScopeExport *ex = ctx->scope_export;
    uint32_t start = ts_node_start_byte(node), end = ts_node_end_byte(node);
    int lo = 0, hi = ex->call_count;
    while (lo < hi) {
        int mid = lo + (hi - lo) / 2;
        const RustCallOccurrence *o = &ex->calls[mid];
        if (o->start < start || (o->start == start && o->end < end)) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    for (int i = lo; i < ex->call_count && ex->calls[i].start == start && ex->calls[i].end == end;
         i++) {
        ex->result->calls.items[ex->calls[i].call].use_scope = s + 1;
    }
}

/* Binds what the `use` declarations among `items`' children declare in scope `s`. A
 * visibility before `use` (`pub`, `pub(crate)`, `pub(in path)`) binds the names in the
 * scope all the same: it says who else may name them, not where they are visible. */
static void rust_declare_uses(RustLSPContext *ctx, TSNode items, int s) {
    PDXE_FOR_EACH_NAMED_CHILD(it, items) {
        if (strcmp(ts_node_type(it.node), "use_declaration") == 0) {
            TSNode tree = ts_node_child_by_field_name(it.node, "argument", 8);
            if (!ts_node_is_null(tree)) {
                rust_collect_use_tree(ctx, tree, NULL, s);
            }
            if (ctx->scope_export && s != 0) {
                rust_export_scoped_import(ctx, it.node, s);
            }
        }
    }
    ctx->use_scopes[s].tree_count = ctx->use_scopes[s].count;
}

/* Whether a block declares a `use` of its own. */
static bool rust_declares_use(TSNode block) {
    PDXE_FOR_EACH_NAMED_CHILD(it, block) {
        if (strcmp(ts_node_type(it.node), "use_declaration") == 0) {
            return true;
        }
    }
    return false;
}

/* Issue 114: records the scopes below `node`, which is in scope `s`: each inline
 * module's body (a module, one deeper) and each block that declares a `use`, with
 * what its `use` declarations bind; and the names of the inline modules. */
static void rust_collect_scopes(RustLSPContext *ctx, TSNode node, int s) {
    PDXE_FOR_EACH_NAMED_CHILD(it, node) {
        TSNode c = it.node;
        const char *kind = ts_node_type(c);
        if (strcmp(kind, "use_declaration") == 0) {
            continue;
        }
        if (ctx->scope_export && ctx->scope_export->call_count > 0) {
            rust_export_place_calls(ctx, c, s);
        }
        int inner = s;
        if (strcmp(kind, "mod_item") == 0) {
            TSNode body = ts_node_child_by_field_name(c, "body", 4);
            if (!ts_node_is_null(body)) {
                /* Issue 104: an inline module's name, for the paths through it. */
                TSNode mname = ts_node_child_by_field_name(c, "name", 4);
                char *name = ts_node_is_null(mname) ? NULL : rust_node_text(ctx, mname);
                if (name)
                    pdxe_idxmemo_put_if_absent(&ctx->inline_modules, ctx->arena, name, 1);
                int depth = ctx->use_scopes[s].module_depth + 1;
                int m = rust_use_scope_new(ctx, ts_node_start_byte(body), ts_node_end_byte(body),
                                           s, true, depth);
                if (m >= 0) {
                    rust_declare_uses(ctx, body, m);
                    rust_collect_scopes(ctx, body, m);
                }
                continue;
            }
        } else if (strcmp(kind, "block") == 0 && rust_declares_use(c)) {
            int b = rust_use_scope_new(ctx, ts_node_start_byte(c), ts_node_end_byte(c), s, false,
                                       ctx->use_scopes[s].module_depth);
            if (b >= 0) {
                rust_declare_uses(ctx, c, b);
                inner = b;
            }
        }
        rust_collect_scopes(ctx, c, inner);
    }
}

/* Issue 111 and 114: reads the file's `use` declarations from its syntax tree, each
 * into the scope it is declared in: the file, an inline module, a block. */
static void rust_collect_uses(RustLSPContext *ctx, TSNode root) {
    if (!rust_file_scope(ctx)) {
        return;
    }
    ctx->use_scopes[0].start_byte = ts_node_start_byte(root);
    ctx->use_scopes[0].end_byte = ts_node_end_byte(root);
    rust_declare_uses(ctx, root, 0);
    rust_collect_scopes(ctx, root, 0);
    ctx->use_scope = 0;
}

/* Issue 114: the recorded scope whose node is `node`, or -1 (a block that declares
 * no `use` is none). Only the file's own tree has scopes: a macro's expansion, parsed
 * from a buffer of its own, keeps the scope of its invocation. Scopes are numbered in
 * source order, so their starts never decrease. */
static int rust_use_scope_of(RustLSPContext *ctx, TSNode node) {
    if (ctx->source != ctx->root_source || ctx->use_scope_count == 0) {
        return -1;
    }
    uint32_t start = ts_node_start_byte(node), end = ts_node_end_byte(node);
    int lo = 0, hi = ctx->use_scope_count;
    while (lo < hi) {
        int mid = lo + (hi - lo) / 2;
        if (ctx->use_scopes[mid].start_byte < start) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    for (int i = lo; i < ctx->use_scope_count && ctx->use_scopes[i].start_byte == start; i++) {
        if (ctx->use_scopes[i].end_byte == end) {
            return i;
        }
    }
    return -1;
}

/* ════════════════════════════════════════════════════════════════════
 * Issues 105 and 113: a Rust path read as Rust resolves it
 * ════════════════════════════════════════════════════════════════════
 *
 * The cross-file pass read a path's text: `crate::` became the first two segments of
 * the file's module name, a re-export was never followed, and a name a glob brings in
 * or another crate of the workspace names was found nowhere. A type in another file's
 * signature was qualified by that file's module whatever imported it. Each of those
 * paths named no registered definition, and its call no answer.
 *
 * The repository's Rust modules are indexed once per cross-file pass, from the files:
 * each file's module (its qualified-name prefix, as the definitions are named), the
 * directory its child modules' files are in, its parent and its crate root, and what
 * its own `use` declarations bind, read by the collector the file's typed pass uses. A
 * path is walked segment by segment, as Rust's name resolution does: `crate`, `self`
 * and `super`; a name the site's scope binds by `use` or glob; a child module's file
 * (`a.rs`, `a/mod.rs`); an item the module defines; a name the module's own `use`
 * binds, followed in the module that declares it (a re-export, `pub use`); the names a
 * glob of the module brings in; a library crate of the workspace by the name of its
 * directory, when exactly one crate has it.
 *
 * Nothing is guessed. A name no rule finds, two definitions of one name (cfg variants
 * or several of one qualified name, issue 108's), two globs that bring a name in, a
 * walk past its depth bound, are no answer. The index is read only where a path's text
 * names nothing registered, so every answer the pass gave before stands. */

#define RMI_MAX_DEPTH 24

typedef struct {
    const char *rel_path;
    const char *module_qn;
    /* The qualified name its items are named under: the module's, but a module file named
     * `index.rs` names its items as its parent's (issue 113C, not renamed here), so its
     * items are found where extraction put them. */
    const char *item_qn;
    const char *child_dir; /* the directory of its child modules' files ("" at the top) */
    int parent;            /* -1 for a crate root, or when no parent file is found */
    int crate_root;        /* -1 when the module is under no crate root */
    bool is_crate_root;
    RustUseScope *uses; /* the file's own scope: its `use` bindings and globs */
} RustModuleEntry;

struct PDXERustModuleIndex {
    PDXEArena *arena;
    RustModuleEntry *modules;
    int count;
    int cap;
    PDXEIdxMemo by_path; /* rel_path -> module */
    PDXEIdxMemo by_qn;   /* module qualified name -> module */
    /* Items (types, traits, free functions) by qualified name, a `#cfg(...)`
     * disambiguator left out; `item_dups` holds the names more than one has. */
    PDXEIdxMemo items;
    PDXEIdxMemo item_dups;
    const char **item_qns;
    bool *item_is_type;
    int item_count;
    int item_cap;
    /* Library crates by name, `-` read as `_`; `crate_dups` as `item_dups`. */
    PDXEIdxMemo crates;
    PDXEIdxMemo crate_dups;
    /* What a name means in a module, once worked out (rmi_encode). */
    PDXEIdxMemo memo;
    bool finished;
};

/* What a path names: a module, an item, or nothing. `rest` is what follows an item in
 * the path (`Type::method`: the method's name), NULL when nothing does. */
typedef struct {
    int module;
    int item;
    const char *rest;
} RustRid;

static const RustRid RMI_NONE = {-1, -1, NULL};

static _Thread_local const PDXERustModuleIndex *g_rust_modules = NULL;

void pdxe_rust_modules_set(const PDXERustModuleIndex *index) {
    g_rust_modules = index;
}

const PDXERustModuleIndex *pdxe_rust_modules_get(void) {
    return g_rust_modules;
}

/* A glob's module as the collector records it (dotted) read back as the path it is. */
static const char *rust_qn_to_path(PDXEArena *a, const char *dotted) {
    size_t n = strlen(dotted), dots = 0;
    for (size_t i = 0; i < n; i++) {
        dots += dotted[i] == '.';
    }
    char *out = (char *)pdxe_arena_alloc(a, n + dots + 1);
    if (!out) {
        return NULL;
    }
    char *o = out;
    for (size_t i = 0; i < n; i++) {
        if (dotted[i] == '.') {
            *o++ = ':';
            *o++ = ':';
        } else {
            *o++ = dotted[i];
        }
    }
    *o = '\0';
    return out;
}

static bool rmi_found(RustRid r) {
    return r.module >= 0 || r.item >= 0;
}

static const char *rmi_dirname(PDXEArena *a, const char *path) {
    const char *slash = strrchr(path, '/');
    return slash ? pdxe_arena_strndup(a, path, (size_t)(slash - path)) : "";
}

static const char *rmi_basename(const char *path) {
    const char *slash = strrchr(path, '/');
    return slash ? slash + 1 : path;
}

static const char *rmi_join(PDXEArena *a, const char *dir, const char *name) {
    return dir[0] ? pdxe_arena_sprintf(a, "%s/%s", dir, name) : pdxe_arena_strdup(a, name);
}

/* Whether `dir`'s last component is `name`. */
static bool rmi_dir_is(const char *dir, const char *name) {
    return strcmp(rmi_basename(dir), name) == 0;
}

/* A name with `-` read as `_`, as a package's name is a crate's in a path. */
static char *rmi_crate_name(PDXEArena *a, const char *name) {
    char *out = pdxe_arena_strdup(a, name);
    for (char *p = out; p && *p; p++) {
        if (*p == '-') {
            *p = '_';
        }
    }
    return out;
}

PDXERustModuleIndex *pdxe_rust_modules_new(PDXEArena *arena) {
    PDXERustModuleIndex *x =
        (PDXERustModuleIndex *)pdxe_arena_alloc(arena, sizeof(PDXERustModuleIndex));
    if (!x) {
        return NULL;
    }
    memset(x, 0, sizeof(*x));
    x->arena = arena;
    return x;
}

/* An item: a type (or trait) or a free function, by its qualified name. */
void pdxe_rust_modules_add_item(PDXERustModuleIndex *x, const char *qualified_name, bool is_type) {
    if (!x || x->finished || !qualified_name) {
        return;
    }
    const char *last_dot = strrchr(qualified_name, '.');
    const char *hash = strchr(last_dot ? last_dot : qualified_name, '#');
    const char *key = hash ? pdxe_arena_strndup(x->arena, qualified_name,
                                               (size_t)(hash - qualified_name))
                           : pdxe_arena_strdup(x->arena, qualified_name);
    if (!key) {
        return;
    }
    int32_t prior = pdxe_idxmemo_get(&x->items, key);
    if (prior >= 0) {
        if (strcmp(x->item_qns[prior], qualified_name) != 0 ||
            x->item_is_type[prior] != is_type) {
            pdxe_idxmemo_put_if_absent(&x->item_dups, x->arena, key, 1);
        }
        return;
    }
    if (x->item_count >= x->item_cap) {
        int cap = x->item_cap ? x->item_cap * 2 : 256;
        const char **q = (const char **)pdxe_arena_alloc(x->arena, (size_t)cap * sizeof(char *));
        bool *t = (bool *)pdxe_arena_alloc(x->arena, (size_t)cap * sizeof(bool));
        if (!q || !t) {
            return;
        }
        if (x->item_count > 0) {
            memcpy(q, x->item_qns, (size_t)x->item_count * sizeof(char *));
            memcpy(t, x->item_is_type, (size_t)x->item_count * sizeof(bool));
        }
        x->item_qns = q;
        x->item_is_type = t;
        x->item_cap = cap;
    }
    x->item_qns[x->item_count] = pdxe_arena_strdup(x->arena, qualified_name);
    x->item_is_type[x->item_count] = is_type;
    pdxe_idxmemo_put_if_absent(&x->items, x->arena, key, x->item_count);
    x->item_count++;
}

/* A Rust file: its path, its module's qualified name, and its source, whose file-level
 * `use` declarations are read here, by the collector the file's own typed pass uses. */
void pdxe_rust_modules_add_file(PDXERustModuleIndex *x, const char *rel_path,
                                const char *module_qn, const char *source, int source_len) {
    if (!x || x->finished || !rel_path || !module_qn) {
        return;
    }
    if (pdxe_idxmemo_get(&x->by_path, rel_path) >= 0) {
        return;
    }
    if (x->count >= x->cap) {
        int cap = x->cap ? x->cap * 2 : 64;
        RustModuleEntry *grown =
            (RustModuleEntry *)pdxe_arena_alloc(x->arena, (size_t)cap * sizeof(RustModuleEntry));
        if (!grown) {
            return;
        }
        if (x->count > 0) {
            memcpy(grown, x->modules, (size_t)x->count * sizeof(RustModuleEntry));
        }
        x->modules = grown;
        x->cap = cap;
    }
    RustModuleEntry *m = &x->modules[x->count];
    memset(m, 0, sizeof(*m));
    m->rel_path = pdxe_arena_strdup(x->arena, rel_path);
    m->module_qn = pdxe_arena_strdup(x->arena, module_qn);
    m->item_qn = m->module_qn;
    size_t qn_len = m->module_qn ? strlen(m->module_qn) : 0;
    const char *slash = strrchr(rel_path, '/');
    if (strcmp(slash ? slash + 1 : rel_path, "index.rs") == 0 && qn_len > 6 &&
        strcmp(m->module_qn + qn_len - 6, ".index") == 0) {
        m->item_qn = pdxe_arena_strndup(x->arena, m->module_qn, qn_len - 6);
    }
    m->parent = -1;
    m->crate_root = -1;
    if (source && source_len > 0) {
        TSParser *parser = ts_parser_new();
        if (parser) {
            ts_parser_set_language(parser, tree_sitter_rust());
            TSTree *tree = ts_parser_parse_string(parser, NULL, source, (uint32_t)source_len);
            if (tree) {
                RustLSPContext ctx;
                rust_lsp_init(&ctx, x->arena, source, source_len, NULL, m->module_qn, NULL);
                RustUseScope *file = rust_file_scope(&ctx);
                if (file) {
                    rust_declare_uses(&ctx, ts_tree_root_node(tree), 0);
                    m->uses = &ctx.use_scopes[0];
                    /* Its names indexed now, in the index's arena: a lookup during the
                     * pass never places one in a file's arena. */
                    bool withheld = false;
                    (void)rust_scope_binding(&ctx, m->uses, "", &withheld);
                }
                ts_tree_delete(tree);
            }
            ts_parser_delete(parser);
        }
    }
    pdxe_idxmemo_put_if_absent(&x->by_path, x->arena, m->rel_path, x->count);
    pdxe_idxmemo_put_if_absent(&x->by_qn, x->arena, m->module_qn, x->count);
    x->count++;
}

static int rmi_file(const PDXERustModuleIndex *x, const char *rel_path) {
    return rel_path ? pdxe_idxmemo_get(&x->by_path, rel_path) : -1;
}

/* Whether a `src` directory is among the components of `dir`: a file below one is in
 * a crate's module tree, never a target of its own. */
static bool rmi_under_src(const char *dir) {
    for (const char *p = dir; p && *p;) {
        const char *slash = strchr(p, '/');
        size_t len = slash ? (size_t)(slash - p) : strlen(p);
        if (len == 3 && strncmp(p, "src", 3) == 0) {
            return true;
        }
        p = slash ? slash + 1 : NULL;
    }
    return false;
}

/* Whether `rel_path` is a crate root: a package's `src/lib.rs` or `src/main.rs`, a
 * binary in `src/bin`, a test, bench or example target (a file directly in a package's
 * `tests`, `benches` or `examples`, outside its `src`), a build script. */
static bool rmi_is_crate_root(PDXEArena *a, const char *rel_path) {
    const char *base = rmi_basename(rel_path);
    const char *dir = rmi_dirname(a, rel_path);
    if (strcmp(base, "lib.rs") == 0 || strcmp(base, "main.rs") == 0) {
        return rmi_dir_is(dir, "src");
    }
    if (strcmp(base, "build.rs") == 0) {
        return !rmi_under_src(dir);
    }
    if ((rmi_dir_is(dir, "tests") || rmi_dir_is(dir, "benches") || rmi_dir_is(dir, "examples")) &&
        !rmi_under_src(dir)) {
        return true;
    }
    if (rmi_dir_is(dir, "bin")) {
        return rmi_dir_is(rmi_dirname(a, dir), "src");
    }
    return false;
}

/* The module file of the module whose files are in directory `p`: `p/mod.rs`; a crate
 * root when `p` is a package's `src` (its library's, else its binary's); else `p.rs`. */
static int rmi_module_of_dir(const PDXERustModuleIndex *x, const char *p) {
    int m = rmi_file(x, rmi_join(x->arena, p, "mod.rs"));
    if (m >= 0) {
        return m;
    }
    if (rmi_dir_is(p, "src")) {
        m = rmi_file(x, rmi_join(x->arena, p, "lib.rs"));
        return m >= 0 ? m : rmi_file(x, rmi_join(x->arena, p, "main.rs"));
    }
    return p[0] ? rmi_file(x, pdxe_arena_sprintf(x->arena, "%s.rs", p)) : -1;
}

/* Works out every module's child directory, parent and crate root, and indexes the
 * library crates by name. `root_package` is the repository's root package's name,
 * the crate a top-level `src/lib.rs` is. */
void pdxe_rust_modules_finish(PDXERustModuleIndex *x, const char *root_package) {
    if (!x || x->finished) {
        return;
    }
    for (int i = 0; i < x->count; i++) {
        RustModuleEntry *m = &x->modules[i];
        const char *dir = rmi_dirname(x->arena, m->rel_path);
        m->is_crate_root = rmi_is_crate_root(x->arena, m->rel_path);
        if (m->is_crate_root || strcmp(rmi_basename(m->rel_path), "mod.rs") == 0) {
            m->child_dir = dir;
        } else {
            size_t len = strlen(m->rel_path);
            m->child_dir = pdxe_arena_strndup(x->arena, m->rel_path, len > 3 ? len - 3 : len);
        }
        if (!m->is_crate_root) {
            const char *home =
                strcmp(rmi_basename(m->rel_path), "mod.rs") == 0 ? rmi_dirname(x->arena, dir) : dir;
            int parent = rmi_module_of_dir(x, home);
            m->parent = parent != i ? parent : -1;
        }
    }
    for (int i = 0; i < x->count; i++) {
        int at = i;
        for (int hops = 0; at >= 0 && !x->modules[at].is_crate_root && hops < x->count; hops++) {
            at = x->modules[at].parent;
        }
        x->modules[i].crate_root = at >= 0 && x->modules[at].is_crate_root ? at : -1;
        RustModuleEntry *m = &x->modules[i];
        if (!m->is_crate_root || strcmp(rmi_basename(m->rel_path), "lib.rs") != 0) {
            continue;
        }
        const char *src = rmi_dirname(x->arena, m->rel_path);
        const char *package = rmi_dirname(x->arena, src);
        const char *name = package[0] ? rmi_basename(package) : root_package;
        if (!name || !name[0]) {
            continue;
        }
        char *crate = rmi_crate_name(x->arena, name);
        if (!crate) {
            continue;
        }
        if (pdxe_idxmemo_get(&x->crates, crate) >= 0) {
            pdxe_idxmemo_put_if_absent(&x->crate_dups, x->arena, crate, 1);
        } else {
            pdxe_idxmemo_put_if_absent(&x->crates, x->arena, crate, i);
        }
    }
    x->finished = true;
}

/* The child module `name` of module `m`: `<child dir>/name.rs` or `.../name/mod.rs`.
 * Keys are made in `a`, the caller's arena. */
static int rmi_child(PDXEArena *a, const PDXERustModuleIndex *x, int m, const char *name) {
    const char *dir = x->modules[m].child_dir;
    int c = rmi_file(x, rmi_join(a, dir, pdxe_arena_sprintf(a, "%s.rs", name)));
    if (c < 0) {
        c = rmi_file(x, rmi_join(a, dir, pdxe_arena_sprintf(a, "%s/mod.rs", name)));
    }
    return c != m ? c : -1;
}

/* The item `name` module `m` defines: its index, -1 for none, -2 when several. */
static int rmi_item(PDXEArena *a, const PDXERustModuleIndex *x, int m, const char *name) {
    const char *key = pdxe_arena_sprintf(a, "%s.%s", x->modules[m].item_qn, name);
    if (!key) {
        return -1;
    }
    if (pdxe_idxmemo_get(&x->item_dups, key) >= 0) {
        return -2;
    }
    return pdxe_idxmemo_get(&x->items, key);
}

static int rmi_crate(const PDXERustModuleIndex *x, const char *name) {
    if (pdxe_idxmemo_get(&x->crate_dups, name) >= 0) {
        return -1;
    }
    return pdxe_idxmemo_get(&x->crates, name);
}

/* A result in the memo, never negative: 0 for nothing, 1 + module, or
 * RMI_ITEM_BASE + item. */
#define RMI_ITEM_BASE (1 << 29)

static int32_t rmi_encode(RustRid r) {
    if (r.module >= 0) {
        return 1 + r.module;
    }
    if (r.item >= 0) {
        return RMI_ITEM_BASE + r.item;
    }
    return 0;
}

static RustRid rmi_decode(int32_t v) {
    RustRid r = RMI_NONE;
    if (v >= RMI_ITEM_BASE) {
        r.item = v - RMI_ITEM_BASE;
    } else if (v > 0) {
        r.module = v - 1;
    }
    return r;
}

static RustRid rmi_path_in_module(RustLSPContext *ctx, const PDXERustModuleIndex *x, int m,
                                  const char *path, int inline_depth, int depth, bool *cut);

/* Two results that name one thing. */
static bool rmi_same(RustRid a, RustRid b) {
    return a.module == b.module && a.item == b.item;
}

/* What `name` is in module `m`, as another module sees it through a path: a child
 * module's file, an item the module defines, a name its own `use` binds (followed
 * where it is declared), or one exactly one of its globs brings in. */
static RustRid rmi_in_module(RustLSPContext *ctx, const PDXERustModuleIndex *x, int m,
                             const char *name, int depth, bool *cut) {
    if (m < 0 || !name || !name[0]) {
        return RMI_NONE;
    }
    if (depth > RMI_MAX_DEPTH) {
        *cut = true;
        return RMI_NONE;
    }
    /* The memo is the index's, filled as names are worked out: one pass, one thread. */
    PDXEIdxMemo *memo = (PDXEIdxMemo *)&x->memo;
    const char *probe = pdxe_arena_sprintf(ctx->arena, "%d\x1f%s", m, name);
    int32_t cached = probe ? pdxe_idxmemo_get(memo, probe) : -1;
    if (cached >= 0) {
        return rmi_decode(cached);
    }
    RustRid found = RMI_NONE;
    bool here_cut = false;
    int child = rmi_child(ctx->arena, x, m, name);
    if (child >= 0) {
        found.module = child;
    } else {
        int item = rmi_item(ctx->arena, x, m, name);
        if (item >= 0) {
            found.item = item;
        } else if (item == -1) {
            RustUseScope *uses = x->modules[m].uses;
            bool withheld = false;
            const char *path = uses ? rust_scope_binding(ctx, uses, name, &withheld) : NULL;
            if (path && !withheld) {
                found = rmi_path_in_module(ctx, x, m, path, 0, depth + 1, &here_cut);
            } else if (!withheld && uses) {
                RustRid only = RMI_NONE;
                bool several = false;
                for (int g = 0; g < uses->glob_count && !several; g++) {
                    const char *glob = rust_qn_to_path(ctx->arena, uses->globs[g]);
                    RustRid module = rmi_path_in_module(ctx, x, m, glob, 0, depth + 1, &here_cut);
                    if (module.module < 0) {
                        continue;
                    }
                    RustRid r = rmi_in_module(ctx, x, module.module, name, depth + 1, &here_cut);
                    if (!rmi_found(r)) {
                        continue;
                    }
                    if (rmi_found(only) && !rmi_same(only, r)) {
                        several = true;
                    } else {
                        only = r;
                    }
                }
                found = several ? RMI_NONE : only;
            }
        }
    }
    if (here_cut) {
        *cut = true; /* a bound was met below: what was found is not the whole answer */
    } else if (probe) {
        pdxe_idxmemo_put_if_absent(memo, x->arena, pdxe_arena_strdup(x->arena, probe),
                                   rmi_encode(found));
    }
    return found;
}

/* Walks `segs[0..n)` on from `from`: through modules by rmi_in_module, and past an
 * item, its associated name (`Type::method`), which the caller looks up. */
static RustRid rmi_walk(RustLSPContext *ctx, const PDXERustModuleIndex *x, RustRid from,
                        char **segs, int n, int depth, bool *cut) {
    RustRid at = from;
    for (int i = 0; i < n && rmi_found(at); i++) {
        if (at.module >= 0) {
            at = rmi_in_module(ctx, x, at.module, segs[i], depth, cut);
            continue;
        }
        /* An item: one associated name may follow a type, nothing a function. */
        if (i == n - 1 && x->item_is_type[at.item]) {
            at.rest = segs[i];
            return at;
        }
        return RMI_NONE;
    }
    return at;
}

/* `super` `k` times from module `m`. */
static int rmi_super(const PDXERustModuleIndex *x, int m, int k) {
    for (; m >= 0 && k > 0; k--) {
        m = x->modules[m].parent;
    }
    return m;
}

/* The segments of `path`, each without a turbofish (`Type::<A>::f` reads `Type`, `f`). */
static int rmi_segments(RustLSPContext *ctx, const char *path, char ***out) {
    char *copy = pdxe_arena_strdup(ctx->arena, path);
    if (!copy) {
        return -1;
    }
    rust_strip_turbofish(copy);
    const char *p = copy;
    while (*p == ':') {
        p++; /* a leading `::` names the crate's extern prelude: read the rest */
    }
    return rust_split_path(ctx->arena, p, out);
}

/* What `path`, written in module `m` (a `use` path, a glob's module, a signature's
 * type), names. `inline_depth` is how many inline modules the writing scope is in:
 * its first `super`s name the file's own module. */
static RustRid rmi_path_in_module(RustLSPContext *ctx, const PDXERustModuleIndex *x, int m,
                                  const char *path, int inline_depth, int depth, bool *cut) {
    char **segs = NULL;
    int n = path ? rmi_segments(ctx, path, &segs) : -1;
    if (n <= 0 || m < 0) {
        return RMI_NONE;
    }
    if (depth > RMI_MAX_DEPTH) {
        *cut = true;
        return RMI_NONE;
    }
    RustRid start = RMI_NONE;
    int used = 1;
    if (strcmp(segs[0], "crate") == 0) {
        start.module = x->modules[m].crate_root;
    } else if (strcmp(segs[0], "self") == 0) {
        start.module = m;
    } else if (strcmp(segs[0], "super") == 0) {
        int k = 0;
        while (k < n && strcmp(segs[k], "super") == 0) {
            k++;
        }
        used = k;
        int inline_hops = k < inline_depth ? k : inline_depth;
        start.module = rmi_super(x, m, k - inline_hops);
    } else if (strcmp(segs[0], "Self") == 0) {
        return RMI_NONE;
    } else {
        start = rmi_in_module(ctx, x, m, segs[0], depth + 1, cut);
        if (!rmi_found(start)) {
            int crate = rmi_crate(x, segs[0]);
            if (crate >= 0) {
                start.module = crate;
            }
        }
    }
    if (!rmi_found(start)) {
        return RMI_NONE;
    }
    return rmi_walk(ctx, x, start, segs + used, n - used, depth + 1, cut);
}

/* What `path`, written where the walk is, names: its first name as the site's scope
 * binds it (a `use`, a glob in force), else as the file's module does. */
/* What the glob imports of the site's scopes nearer than `upto` (all, when negative)
 * bring in as `name`: the one item or module they agree on; none when none does, or
 * when they bring in different ones (`*several` set). */
static RustRid rmi_through_globs(RustLSPContext *ctx, const PDXERustModuleIndex *x, int m,
                                 const char *name, int upto, int inline_depth, bool *cut,
                                 bool *several) {
    const char **globs = NULL;
    int glob_n = rust_visible_globs(ctx, upto, &globs);
    RustRid only = RMI_NONE;
    *several = false;
    for (int g = 0; g < glob_n && !*several; g++) {
        const char *glob = rust_qn_to_path(ctx->arena, globs[g]);
        RustRid module = rmi_path_in_module(ctx, x, m, glob, inline_depth, 1, cut);
        if (module.module < 0) {
            continue;
        }
        RustRid r = rmi_in_module(ctx, x, module.module, name, 1, cut);
        if (!rmi_found(r)) {
            continue;
        }
        if (rmi_found(only) && !rmi_same(only, r)) {
            *several = true;
        } else {
            only = r;
        }
    }
    return *several ? RMI_NONE : only;
}

static RustRid rmi_path_at_site(RustLSPContext *ctx, const char *path) {
    const PDXERustModuleIndex *x = g_rust_modules;
    if (!x || !x->finished || !ctx->module_qn || !path) {
        return RMI_NONE;
    }
    int m = pdxe_idxmemo_get(&x->by_qn, ctx->module_qn);
    if (m < 0) {
        return RMI_NONE;
    }
    char **segs = NULL;
    int n = rmi_segments(ctx, path, &segs);
    if (n <= 0) {
        return RMI_NONE;
    }
    int inline_depth = 0;
    if (ctx->use_scope >= 0 && ctx->use_scope < ctx->use_scope_count) {
        inline_depth = ctx->use_scopes[ctx->use_scope].module_depth;
    }
    bool cut = false;
    const char *head = segs[0];
    if (strcmp(head, "crate") == 0 || strcmp(head, "self") == 0 || strcmp(head, "super") == 0) {
        return rmi_path_in_module(ctx, x, m, path, inline_depth, 0, &cut);
    }
    if (strcmp(head, "Self") == 0) {
        return RMI_NONE;
    }
    RustRid start = RMI_NONE;
    int rank = -1;
    const char *imported = rust_resolve_use_ranked(ctx, head, &rank);
    bool several = false;
    if (imported && rank > 0) {
        /* A glob of a nearer scope that brings the name in shadows the `use` of an outer
         * one (issue 114). */
        start = rmi_through_globs(ctx, x, m, head, rank, inline_depth, &cut, &several);
        if (several) {
            return RMI_NONE;
        }
    }
    if (imported && !rmi_found(start)) {
        start = rmi_path_in_module(ctx, x, m, imported, inline_depth, 1, &cut);
    } else if (!imported && rank < 0) {
        /* Not bound by a `use` in force (a name bound to two paths is withheld). */
        if (inline_depth == 0) {
            int child = rmi_child(ctx->arena, x, m, head);
            if (child >= 0) {
                start.module = child;
            }
        }
        if (!rmi_found(start)) {
            int item = rmi_item(ctx->arena, x, m, head);
            if (item >= 0) {
                start.item = item;
            } else if (item == -1) {
                start = rmi_through_globs(ctx, x, m, head, -1, inline_depth, &cut, &several);
                if (!rmi_found(start) && !several) {
                    int crate = rmi_crate(x, head);
                    if (crate >= 0) {
                        start.module = crate;
                    }
                }
            }
        }
    }
    if (!rmi_found(start)) {
        return RMI_NONE;
    }
    return rmi_walk(ctx, x, start, segs + 1, n - 1, 1, &cut);
}

/* The qualified name a result names: an item's, with what follows it in the path. */
static const char *rmi_qn(RustLSPContext *ctx, RustRid r) {
    const PDXERustModuleIndex *x = g_rust_modules;
    if (!x || r.item < 0) {
        return NULL;
    }
    const char *qn = x->item_qns[r.item];
    return r.rest ? pdxe_arena_sprintf(ctx->arena, "%s.%s", qn, r.rest) : qn;
}

/* A type's text in a signature written in module `module_qn` (another file's return,
 * parameter or field type): the type it names, or NULL. */
static const char *rmi_type_in_module(PDXEArena *arena, const char *module_qn, const char *text) {
    const PDXERustModuleIndex *x = g_rust_modules;
    if (!x || !x->finished || !module_qn || !text || !text[0]) {
        return NULL;
    }
    int m = pdxe_idxmemo_get(&x->by_qn, module_qn);
    if (m < 0) {
        return NULL;
    }
    RustLSPContext scratch;
    rust_lsp_init(&scratch, arena, "", 0, NULL, module_qn, NULL);
    bool cut = false;
    RustRid r = rmi_path_in_module(&scratch, x, m, text, 0, 0, &cut);
    if (r.item < 0 || r.rest || !x->item_is_type[r.item]) {
        return NULL;
    }
    return x->item_qns[r.item];
}

/* Issues 105 and 113: what a path at the site names, as rmi_path_at_site reads it, as
 * a qualified name; NULL when it names nothing the index holds. */
static const char *rust_path_identity(RustLSPContext *ctx, const char *path) {
    return rmi_qn(ctx, rmi_path_at_site(ctx, path));
}

static int rust_call_occurrence_cmp(const void *a, const void *b) {
    const RustCallOccurrence *x = (const RustCallOccurrence *)a;
    const RustCallOccurrence *y = (const RustCallOccurrence *)b;
    if (x->start != y->start) {
        return x->start < y->start ? -1 : 1;
    }
    if (x->end != y->end) {
        return x->end < y->end ? -1 : 1;
    }
    return x->call < y->call ? -1 : (x->call > y->call);
}

void pdxe_rust_export_use_scopes(PDXEArena *arena, PDXEFileResult *result, const char *source,
                                int source_len, TSNode root) {
    if (!arena || !result || !source || ts_node_is_null(root)) {
        return;
    }
    RustScopeExport ex = {.result = result};
    /* The calls of the file's own text not placed yet (the typed pass places the calls
     * it adds itself), by occurrence. */
    int n = 0;
    for (int i = 0; i < result->calls.count; i++) {
        const PDXECall *c = &result->calls.items[i];
        n += c->use_scope == 0 && c->source_origin == PDXE_SOURCE_ORIGIN_RAW &&
             c->site_end_byte > c->site_start_byte;
    }
    if (n > 0) {
        ex.calls = (RustCallOccurrence *)pdxe_arena_alloc(arena, (size_t)n * sizeof(*ex.calls));
        if (!ex.calls) {
            ex.failed = true;
        } else {
            for (int i = 0; i < result->calls.count; i++) {
                const PDXECall *c = &result->calls.items[i];
                if (c->use_scope == 0 && c->source_origin == PDXE_SOURCE_ORIGIN_RAW &&
                    c->site_end_byte > c->site_start_byte) {
                    ex.calls[ex.call_count++] =
                        (RustCallOccurrence){c->site_start_byte, c->site_end_byte, i};
                }
            }
            qsort(ex.calls, (size_t)ex.call_count, sizeof(*ex.calls), rust_call_occurrence_cmp);
        }
    }
    RustLSPContext ctx;
    rust_lsp_init(&ctx, arena, source, source_len, NULL, result->module_qn, NULL);
    ctx.scope_export = &ex;
    if (!ex.failed) {
        rust_collect_uses(&ctx, root);
    }
    if (!ex.failed && ctx.use_scope_count > 0) {
        PDXEUseScope *scopes =
            (PDXEUseScope *)pdxe_arena_alloc(arena, (size_t)ctx.use_scope_count * sizeof(*scopes));
        if (!scopes) {
            ex.failed = true;
        } else {
            for (int i = 0; i < ctx.use_scope_count; i++) {
                scopes[i].parent = ctx.use_scopes[i].parent;
                scopes[i].module = ctx.use_scopes[i].module;
            }
            result->use_scopes.items = scopes;
            result->use_scopes.count = ctx.use_scope_count;
            result->use_scopes.cap = ctx.use_scope_count;
        }
    } else {
        ex.failed = true;
    }
    if (ex.failed) {
        /* A table not made whole is no table: no scope, no scoped import, and every
         * call unplaced, the typed pass's own included. */
        memset(&result->use_scopes, 0, sizeof(result->use_scopes));
        memset(&result->scoped_imports, 0, sizeof(result->scoped_imports));
        for (int i = 0; i < result->calls.count; i++) {
            result->calls.items[i].use_scope = 0;
        }
    }
}

/* ── Issue 110: the members a macro invocation may write ───────────
 *
 * An item a `macro_rules!` invocation writes is no definition: extraction reads the
 * file's syntax, and an expansion is in no file. So when a macro writes an `impl` for a
 * type, the type's members the registry holds are not all of them, and its one member of
 * a name is no evidence that a call of the name means it (decision 65). What is recorded
 * is what an invocation may write, read from the definition's own tokens; nothing is
 * expanded:
 *
 *   - for every `impl` a transcriber holds, at any depth, the type it is for, by the last
 *     name of its path, with the name of every function directly in its body;
 *   - a type or function name the transcriber gives by a metavariable is what the rule's
 *     pattern binds it to, as expansion's own matcher (`macro_pattern_match`) binds it,
 *     for the rules that matcher matches the invocation against; when it matches none,
 *     every rule is read and each identifier of the invocation's arguments stands for
 *     it, as it does for a metavariable no matched rule binds (one in a repetition);
 *   - a function name the transcriber generates (`fn [<…>]`) is any member (NULL);
 *   - an invocation directly in an `impl` body writes its transcriber's own functions as
 *     members of that impl's type.
 *
 * It over-approximates where it cannot bind: these facts only withhold a draw. Only a definition
 * the invocation sees in its own file is read (textual scope, as expansion finds one);
 * a definition in another file, a macro another macro's transcriber invokes, and a macro
 * outside the repository are not followed. */

typedef struct {
    TSNode *items;
    int count;
    int cap;
} RustMMTokens;

typedef struct {
    RustLSPContext *ctx;
    PDXEMacroMemberArray *out;
    bool failed;
} RustMMExport;

/* The tokens of a token tree in order, a repetition's (`$( … )*`) spliced in, a nested
 * token tree kept as one token. */
static void rust_mm_tokens(RustMMExport *ex, TSNode tree, RustMMTokens *out) {
    PDXE_FOR_EACH_CHILD(it, tree) {
        TSNode c = it.node;
        if (strcmp(ts_node_type(c), "token_repetition") == 0) {
            rust_mm_tokens(ex, c, out);
            continue;
        }
        if (out->count >= out->cap) {
            int cap = out->cap ? out->cap * 2 : 32;
            TSNode *grown = (TSNode *)pdxe_arena_alloc(ex->ctx->arena, (size_t)cap * sizeof(*grown));
            if (!grown) {
                ex->failed = true;
                return;
            }
            if (out->count > 0) {
                memcpy(grown, out->items, (size_t)out->count * sizeof(*grown));
            }
            out->items = grown;
            out->cap = cap;
        }
        out->items[out->count++] = c;
    }
}

static bool rust_mm_is(TSNode n, const char *type) {
    return strcmp(ts_node_type(n), type) == 0;
}

/* Whether token `n` is spelled exactly `text`. */
static bool rust_mm_spelled(const RustLSPContext *ctx, TSNode n, const char *text) {
    uint32_t start = ts_node_start_byte(n);
    uint32_t end = ts_node_end_byte(n);
    size_t len = strlen(text);
    return end >= start && end <= (uint32_t)ctx->source_len && (size_t)(end - start) == len &&
           memcmp(ctx->source + start, text, len) == 0;
}

/* The first byte of token `n`, or 0. */
static char rust_mm_first(const RustLSPContext *ctx, TSNode n) {
    uint32_t start = ts_node_start_byte(n);
    return start < (uint32_t)ctx->source_len && ts_node_end_byte(n) > start ? ctx->source[start]
                                                                              : '\0';
}

/* Whether token `n` is a word: an identifier, or a keyword the grammar reads as its own
 * token (`gen`, `default`, …), which a path or a function may still be named by. */
static bool rust_mm_word(const RustLSPContext *ctx, TSNode n) {
    if (rust_mm_is(n, "identifier")) {
        return true;
    }
    uint32_t start = ts_node_start_byte(n);
    uint32_t end = ts_node_end_byte(n);
    if (ts_node_child_count(n) > 0 || end <= start || end > (uint32_t)ctx->source_len) {
        return false;
    }
    for (uint32_t i = start; i < end; i++) {
        char c = ctx->source[i];
        bool letter = (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_';
        if (!letter && !(i > start && c >= '0' && c <= '9')) {
            return false;
        }
    }
    return true;
}

/* The change in angle-bracket depth a punctuation token makes; `->` and `=>` none. */
static int rust_mm_angles(const RustLSPContext *ctx, TSNode n) {
    if (ts_node_named_child_count(n) > 0 || ts_node_is_named(n)) {
        return 0;
    }
    uint32_t start = ts_node_start_byte(n);
    uint32_t end = ts_node_end_byte(n);
    if (end > (uint32_t)ctx->source_len) {
        return 0;
    }
    int delta = 0;
    for (uint32_t i = start; i < end; i++) {
        char c = ctx->source[i];
        if (c == '<') {
            delta++;
        } else if (c == '>' && !(i > start && (ctx->source[i - 1] == '-' || ctx->source[i - 1] == '='))) {
            delta--;
        }
    }
    return delta;
}

static void rust_mm_add(RustMMExport *ex, const char *type_name, const char *member) {
    if (ex->failed || !type_name || !type_name[0]) {
        return;
    }
    PDXEMacroMemberArray *arr = ex->out;
    if (arr->count >= arr->cap) {
        int cap = arr->cap ? arr->cap * 2 : 8;
        PDXEMacroMember *grown =
            (PDXEMacroMember *)pdxe_arena_alloc(ex->ctx->arena, (size_t)cap * sizeof(*grown));
        if (!grown) {
            ex->failed = true;
            return;
        }
        if (arr->count > 0) {
            memcpy(grown, arr->items, (size_t)arr->count * sizeof(*grown));
        }
        arr->items = grown;
        arr->cap = cap;
    }
    arr->items[arr->count++] = (PDXEMacroMember){type_name, member};
}

/* What one invocation's metavariables may stand for: the bindings of a rule the
 * engine's matcher matched it against (expansion's own matcher), or, with none, every
 * identifier of its arguments. */
typedef struct {
    const MacroEnv *env;  /* the matched rule's bindings, or NULL */
    const char **idents;  /* every identifier of the arguments, at any depth */
    int ident_count;
    int ident_cap;
} RustMMBinding;

static void rust_mm_collect_identifiers(RustMMExport *ex, TSNode tree, RustMMBinding *b) {
    PDXE_FOR_EACH_CHILD(it, tree) {
        if (ex->failed) {
            break;
        }
        TSNode c = it.node;
        if (rust_mm_is(c, "identifier")) {
            const char *name = rust_node_text(ex->ctx, c);
            if (!name) {
                ex->failed = true;
                return;
            }
            if (b->ident_count >= b->ident_cap) {
                int cap = b->ident_cap ? b->ident_cap * 2 : 16;
                const char **grown = (const char **)pdxe_arena_alloc(
                    ex->ctx->arena, (size_t)cap * sizeof(*grown));
                if (!grown) {
                    ex->failed = true;
                    return;
                }
                if (b->ident_count > 0) {
                    memcpy(grown, b->idents, (size_t)b->ident_count * sizeof(*grown));
                }
                b->idents = grown;
                b->ident_cap = cap;
            }
            b->idents[b->ident_count++] = name;
        } else if (ts_node_child_count(c) > 0) {
            rust_mm_collect_identifiers(ex, c, b);
        }
    }
}

/* The last name of the path a bound value spells as a type (`&'static [u8]` none,
 * `Cow<'static, str>` `Cow`, `crate::x::Y<T>` `Y`), or NULL. */
static const char *rust_mm_value_type_name(RustMMExport *ex, const char *v, int len) {
    int k = 0;
    for (;;) {
        while (k < len && (v[k] == ' ' || v[k] == '\t' || v[k] == '\n' || v[k] == '\r' ||
                           v[k] == '&')) {
            k++;
        }
        if (k < len && v[k] == '\'') {
            k = macro_consume_ident(v, len, k + 1);
            continue;
        }
        int word = macro_consume_ident(v, len, k);
        if ((word - k == 3 && strncmp(v + k, "mut", 3) == 0) ||
            (word - k == 3 && strncmp(v + k, "dyn", 3) == 0)) {
            k = word;
            continue;
        }
        break;
    }
    int last_start = -1, last_end = -1;
    while (k < len) {
        if (v[k] == '$') {
            k++;
        }
        int word = macro_consume_ident(v, len, k);
        if (word == k) {
            break;
        }
        bool prefix = (word - k == 4 && strncmp(v + k, "self", 4) == 0) ||
                      (word - k == 5 && strncmp(v + k, "super", 5) == 0) ||
                      (word - k == 5 && strncmp(v + k, "crate", 5) == 0);
        if (prefix) {
            last_start = -1;
        } else {
            last_start = k;
            last_end = word;
        }
        k = word;
        if (k + 1 < len && v[k] == ':' && v[k + 1] == ':') {
            k += 2;
        } else {
            break;
        }
    }
    if (last_start < 0) {
        return NULL;
    }
    char *name = pdxe_arena_strndup(ex->ctx->arena, v + last_start, (size_t)(last_end - last_start));
    if (!name) {
        ex->failed = true;
    }
    return name;
}

/* The names metavariable token `meta` may stand for: its binding, read as a type or as
 * a name, when the matched rule bound it; every identifier of the arguments otherwise. */
static int rust_mm_meta_names(RustMMExport *ex, TSNode meta, const RustMMBinding *b,
                              bool as_type, const char **one, const char ***many) {
    *many = NULL;
    if (b->env) {
        const char *text = rust_node_text(ex->ctx, meta);
        if (!text) {
            ex->failed = true;
            return 0;
        }
        const MacroBinding *bound = macro_env_lookup(b->env, text[0] == '$' ? text + 1 : text);
        if (bound && bound->value_len > 0) {
            if (as_type) {
                /* A type that is no named path (a tuple, a slice) has no members here. */
                *one = rust_mm_value_type_name(ex, bound->value, bound->value_len);
                return *one ? 1 : 0;
            }
            int word = macro_consume_ident(bound->value, bound->value_len, 0);
            if (word > 0 && word == bound->value_len) {
                *one = pdxe_arena_strndup(ex->ctx->arena, bound->value, (size_t)word);
                if (!*one) {
                    ex->failed = true;
                    return 0;
                }
                return 1;
            }
        }
    }
    *many = b->idents;
    return b->ident_count;
}

/* The type `tokens[from, to)` names, as the last name of its path: the identifier, the
 * metavariable (`*meta`), or neither (a tuple, an array, a slice, a pointer's target
 * left as written). A leading `&`, lifetime, `mut` or `dyn` is passed over. */
static TSNode rust_mm_type_name(RustMMExport *ex, const RustMMTokens *t, int from, int to,
                                bool *meta) {
    const RustLSPContext *ctx = ex->ctx;
    TSNode none = {0};
    *meta = false;
    int k = from;
    while (k < to) {
        TSNode n = t->items[k];
        if (rust_mm_spelled(ctx, n, "&") || rust_mm_spelled(ctx, n, "&&") ||
            rust_mm_is(n, "mutable_specifier") || rust_mm_is(n, "dyn")) {
            k++;
        } else if (rust_mm_spelled(ctx, n, "'") && k + 1 < to) {
            k += 2;
        } else if (rust_mm_is(n, "lifetime")) {
            k++;
        } else {
            break;
        }
    }
    TSNode last = none;
    bool have = false;
    while (k < to) {
        TSNode n = t->items[k];
        if (rust_mm_is(n, "self") || rust_mm_is(n, "super") || rust_mm_is(n, "crate") ||
            rust_mm_spelled(ctx, n, "self") || rust_mm_spelled(ctx, n, "super") ||
            rust_mm_spelled(ctx, n, "crate") || rust_mm_spelled(ctx, n, "$crate")) {
            have = false;
            k++;
        } else if (rust_mm_is(n, "metavariable") || rust_mm_word(ctx, n)) {
            last = n;
            have = true;
            k++;
        } else if (rust_mm_spelled(ctx, n, "::")) {
            k++;
        } else {
            break;
        }
    }
    if (!have) {
        return none;
    }
    *meta = rust_mm_is(last, "metavariable");
    return last;
}

/* Every type a member is written for, each with every name it may have. */
static void rust_mm_add_all(RustMMExport *ex, const char *type_one, const char **types,
                            int type_count, const char *member_one, const char **members,
                            int member_count, bool any_member) {
    int nt = types ? type_count : (type_one ? 1 : 0);
    for (int i = 0; i < nt && !ex->failed; i++) {
        const char *type = types ? types[i] : type_one;
        if (any_member) {
            rust_mm_add(ex, type, NULL);
            continue;
        }
        int nm = members ? member_count : (member_one ? 1 : 0);
        for (int j = 0; j < nm && !ex->failed; j++) {
            rust_mm_add(ex, type, members ? members[j] : member_one);
        }
    }
}

/* The functions a token sequence holds directly, each `fn` and its name, written as
 * members of the type(s) given: a name the transcriber gives by a metavariable is what
 * it is bound to, a name it gives otherwise than by a word (a generated one) any. */
static void rust_mm_add_functions(RustMMExport *ex, TSNode tree, const char *type_one,
                                  const char **types, int type_count, const RustMMBinding *b) {
    RustMMTokens t = {0};
    rust_mm_tokens(ex, tree, &t);
    for (int i = 0; i + 1 < t.count && !ex->failed; i++) {
        if (!rust_mm_is(t.items[i], "fn")) {
            continue;
        }
        TSNode name = t.items[i + 1];
        if (rust_mm_is(name, "metavariable")) {
            const char *one = NULL;
            const char **many = NULL;
            int n = rust_mm_meta_names(ex, name, b, false, &one, &many);
            rust_mm_add_all(ex, type_one, types, type_count, one, many, n, false);
        } else if (rust_mm_word(ex->ctx, name)) {
            const char *member = rust_node_text(ex->ctx, name);
            if (!member) {
                ex->failed = true;
                return;
            }
            rust_mm_add_all(ex, type_one, types, type_count, member, NULL, 0, false);
        } else if (rust_mm_is(name, "token_tree") && rust_mm_first(ex->ctx, name) == '[') {
            rust_mm_add_all(ex, type_one, types, type_count, NULL, NULL, 0, true);
        }
    }
}

/* The `impl` blocks of a transcriber's token tree, at any depth, and their members. */
static void rust_mm_scan(RustMMExport *ex, TSNode tree, const RustMMBinding *b) {
    const RustLSPContext *ctx = ex->ctx;
    RustMMTokens t = {0};
    rust_mm_tokens(ex, tree, &t);
    for (int i = 0; i < t.count && !ex->failed; i++) {
        TSNode n = t.items[i];
        if (rust_mm_is(n, "token_tree")) {
            rust_mm_scan(ex, n, b);
            continue;
        }
        if (!rust_mm_is(n, "impl")) {
            continue;
        }
        /* The header: generics, then a trait and `for`, or the type, up to `where` or
         * the body, at angle depth 0. */
        int k = i + 1;
        int depth = 0;
        if (k < t.count && rust_mm_first(ctx, t.items[k]) == '<') {
            do {
                depth += rust_mm_angles(ctx, t.items[k]);
                k++;
            } while (k < t.count && depth > 0);
        }
        int type_from = k;
        int type_to = -1;
        int body = -1;
        depth = 0;
        for (; k < t.count; k++) {
            TSNode h = t.items[k];
            if (rust_mm_is(h, "token_tree")) {
                if (depth == 0 && rust_mm_first(ctx, h) == '{') {
                    body = k;
                    break;
                }
                continue;
            }
            if (depth == 0 && rust_mm_is(h, "for") &&
                !(k + 1 < t.count && rust_mm_first(ctx, t.items[k + 1]) == '<')) {
                type_from = k + 1;
                type_to = -1;
            } else if (depth == 0 && rust_mm_is(h, "where") && type_to < 0) {
                type_to = k;
            } else if (depth == 0 && rust_mm_spelled(ctx, h, ";")) {
                break;
            }
            depth += rust_mm_angles(ctx, h);
        }
        if (body < 0) {
            continue;
        }
        bool meta = false;
        TSNode type = rust_mm_type_name(ex, &t, type_from, type_to < 0 ? body : type_to, &meta);
        if (ts_node_is_null(type)) {
            continue;
        }
        if (meta) {
            const char *one = NULL;
            const char **many = NULL;
            int n = rust_mm_meta_names(ex, type, b, true, &one, &many);
            rust_mm_add_functions(ex, t.items[body], one, many, n, b);
        } else {
            const char *type_name = rust_node_text(ex->ctx, type);
            if (!type_name) {
                ex->failed = true;
                return;
            }
            rust_mm_add_functions(ex, t.items[body], type_name, NULL, 0, b);
        }
    }
}

/* The type an `impl` item is for, by the last name of its path; NULL when it names
 * none (a tuple, a reference to one, …). */
static const char *rust_mm_impl_type(RustLSPContext *ctx, TSNode impl) {
    TSNode type = ts_node_child_by_field_name(impl, "type", 4);
    for (int guard = 0; !ts_node_is_null(type) && guard < 8; guard++) {
        const char *kind = ts_node_type(type);
        if (strcmp(kind, "type_identifier") == 0) {
            return rust_node_text(ctx, type);
        }
        if (strcmp(kind, "scoped_type_identifier") == 0) {
            type = ts_node_child_by_field_name(type, "name", 4);
        } else if (strcmp(kind, "generic_type") == 0 || strcmp(kind, "reference_type") == 0) {
            type = ts_node_child_by_field_name(type, "type", 4);
        } else {
            return NULL;
        }
    }
    return NULL;
}

/* One invocation: the rules of the definition it sees, each transcriber read. The rules
 * the engine's matcher matches the invocation against are read with their bindings;
 * when it matches none, every rule is read and each metavariable may be any identifier
 * of the arguments. */
static void rust_mm_invocation(RustMMExport *ex, TSNode invocation, const char *impl_type) {
    RustLSPContext *ctx = ex->ctx;
    TSNode macro = ts_node_child_by_field_name(invocation, "macro", 5);
    if (!ts_node_is_null(macro) && rust_mm_is(macro, "scoped_identifier")) {
        macro = ts_node_child_by_field_name(macro, "name", 4);
    }
    if (ts_node_is_null(macro)) {
        return;
    }
    char *name = rust_node_text(ctx, macro);
    if (!name) {
        ex->failed = true;
        return;
    }
    RustMacroRule *visible = rust_visible_macro_definition(ctx, name, invocation);
    if (!visible) {
        return;
    }
    /* The arguments: the invocation's token tree, which the grammar gives no field, and
     * its text without the delimiters, as expansion matches it. */
    TSNode args = {0};
    PDXE_FOR_EACH_CHILD(it, invocation) {
        if (rust_mm_is(it.node, "token_tree")) {
            args = it.node;
            break;
        }
    }
    RustMMBinding unbound = {0};
    const char *inv = NULL;
    int inv_len = 0;
    if (!ts_node_is_null(args)) {
        rust_mm_collect_identifiers(ex, args, &unbound);
        char *text = pdxe_node_text(ctx->arena, args, ctx->source);
        if (!text) {
            ex->failed = true;
            return;
        }
        rust_macro_strip_outer(text, (int)strlen(text), &inv, &inv_len);
    }
    /* The definition's rules, from the name index rust_visible_macro_definition has just
     * brought up to date, as expansion reads them (issue 92): first whether the matcher
     * matches any, then each read. */
    bool scan = false;
#if defined(PDXE_ENABLE_TEST_SEAMS)
    const char *reference = getenv("PDX_ENGINE_TEST_LINEAR_LOOKUP_REFERENCE");
    scan = reference && reference[0];
#endif
    const PDXENameIndex *by_name = &ctx->macro_rules_by_name;
    bool any_matched = false;
    for (int pass = 0; pass < 2 && !ex->failed; pass++) {
        for (int i = scan ? 0 : pdxe_name_index_first(by_name, name);
             (scan ? i < ctx->macro_rules_count : i >= 0) && !ex->failed;
             i = scan ? i + 1 : pdxe_name_index_next(by_name, i)) {
            PDXE_LOOKUP_COUNT(PDXE_LC_NAME_COMPARES);
            RustMacroRule *r = ctx->macro_rules_arr[i];
            if (!r || strcmp(r->macro_name, name) != 0 ||
                r->definition_start_byte != visible->definition_start_byte ||
                r->scope_start_byte != visible->scope_start_byte ||
                r->scope_end_byte != visible->scope_end_byte ||
                ts_node_is_null(r->transcriber_node)) {
                continue;
            }
            MacroEnv env;
            memset(&env, 0, sizeof(env));
            bool matched = r->pattern_text && inv &&
                           macro_pattern_match(r->pattern_text, r->pattern_len, inv, inv_len, &env);
            if (pass == 0) {
                any_matched = any_matched || matched;
                continue;
            }
            if (any_matched && !matched) {
                continue;
            }
            RustMMBinding b = unbound;
            b.env = matched ? &env : NULL;
            rust_mm_scan(ex, r->transcriber_node, &b);
            if (impl_type) {
                /* In an impl body: the transcriber's own functions are the impl type's. */
                rust_mm_add_functions(ex, r->transcriber_node, impl_type, NULL, 0, &b);
            }
        }
    }
}

/* Every invocation of the tree, each knowing the impl type whose body it is directly
 * in, if any. */
static void rust_mm_walk(RustMMExport *ex, TSNode node, const char *impl_type) {
    PDXE_FOR_EACH_CHILD(it, node) {
        if (ex->failed) {
            break;
        }
        TSNode c = it.node;
        const char *kind = ts_node_type(c);
        if (strcmp(kind, "macro_invocation") == 0) {
            rust_mm_invocation(ex, c, impl_type);
        } else if (strcmp(kind, "impl_item") == 0) {
            TSNode body = ts_node_child_by_field_name(c, "body", 4);
            if (!ts_node_is_null(body)) {
                rust_mm_walk(ex, body, rust_mm_impl_type(ex->ctx, c));
            }
        } else if (ts_node_child_count(c) > 0 && strcmp(kind, "token_tree") != 0 &&
                   strcmp(kind, "macro_definition") != 0) {
            rust_mm_walk(ex, c, NULL);
        }
    }
}

static int rust_mm_cmp(const void *pa, const void *pb) {
    const PDXEMacroMember *a = (const PDXEMacroMember *)pa;
    const PDXEMacroMember *b = (const PDXEMacroMember *)pb;
    int c = strcmp(a->type_name, b->type_name);
    if (c != 0) {
        return c;
    }
    if (!a->member || !b->member) {
        return (a->member != NULL) - (b->member != NULL);
    }
    return strcmp(a->member, b->member);
}

void pdxe_rust_export_macro_members(PDXEArena *arena, PDXEFileResult *result, const char *source,
                                   int source_len, TSNode root) {
    if (!arena || !result || !source || ts_node_is_null(root)) {
        return;
    }
    RustLSPContext ctx;
    rust_lsp_init(&ctx, arena, source, source_len, NULL, result->module_qn, NULL);
    rust_collect_macro_rules(&ctx, root);
    if (ctx.macro_rules_count == 0) {
        return;
    }
    RustMMExport ex = {.ctx = &ctx, .out = &result->macro_members};
    rust_mm_walk(&ex, root, NULL);
    PDXEMacroMemberArray *arr = &result->macro_members;
    if (ex.failed) {
        /* A table not made whole is no table: lost work, counted where the allocation
         * failed. */
        memset(arr, 0, sizeof(*arr));
        return;
    }
    /* One entry per type and member, in byte order: what an invocation writes, never
     * how often. */
    if (arr->count > 1) {
        qsort(arr->items, (size_t)arr->count, sizeof(*arr->items), rust_mm_cmp);
        int kept = 1;
        for (int i = 1; i < arr->count; i++) {
            if (rust_mm_cmp(&arr->items[kept - 1], &arr->items[i]) != 0) {
                arr->items[kept++] = arr->items[i];
            }
        }
        arr->count = kept;
    }
}

/* Build the registry from the per-file `result->defs`, `result->impl_traits`,
 * and a Rust prelude seed. */
void pdxe_rust_build_local_registry(PDXEArena *arena, PDXETypeRegistry *reg, PDXEFileResult *result,
                                   const char *module_qn, TSNode root, const char *source) {

    pdxe_registry_init(reg, arena);
    pdxe_rust_stdlib_register(reg, arena);

    /* Phase A1: register all type containers first. Trait provenance on a
     * later method can then be canonicalized without declaration-order or
     * leaf-first guesses. */
    PDXEIdxMemo type_idx = {0};
    for (int i = 0; i < reg->type_count; i++) {
        if (reg->types[i].qualified_name) {
            pdxe_idxmemo_put_if_absent(&type_idx, arena, reg->types[i].qualified_name, i);
        }
    }
    for (int i = 0; i < result->defs.count; i++) {
        PDXEDefinition *d = &result->defs.items[i];
        if (!d->qualified_name || !d->name || !pdxe_label_is_type_like(d->label)) {
            continue;
        }
        PDXERegisteredType rt;
        memset(&rt, 0, sizeof(rt));
        rt.qualified_name = d->qualified_name;
        rt.short_name = d->name;
        rt.is_interface = (strcmp(d->label, "Interface") == 0 || strcmp(d->label, "Trait") == 0);
        pdxe_registry_add_type(reg, rt);
        pdxe_idxmemo_put_if_absent(&type_idx, arena, rt.qualified_name, reg->type_count - 1);
    }
    RustTraitNameIndex trait_names;
    rust_trait_name_index_build(&trait_names, arena, reg);

    /* Phase A2: register every Function/Method definition. */
    for (int i = 0; i < result->defs.count; i++) {
        PDXEDefinition *d = &result->defs.items[i];
        if (!d->qualified_name || !d->name)
            continue;

        if (d->label && (strcmp(d->label, "Function") == 0 || strcmp(d->label, "Method") == 0)) {
            PDXERegisteredFunc rf;
            memset(&rf, 0, sizeof(rf));
            rf.qualified_name = d->qualified_name;
            rf.short_name = d->name;
            rf.min_params = -1;
            if (d->is_abstract) {
                rf.flags |= PDXE_FUNC_FLAG_RUST_ABSTRACT;
            }
            if (d->impl_trait) {
                rf.flags |= PDXE_FUNC_FLAG_RUST_TRAIT_IMPL;
                rf.impl_trait_qn =
                    rust_canonical_impl_trait(arena, reg, &trait_names, d->impl_trait, module_qn);
                rf.rust_impl = rust_impl_facts_read(arena, reg, &trait_names, d->impl_bounds,
                                                    module_qn, rf.impl_trait_qn);
            }

            /* Build FUNC sig from return_types / param_types. */
            const PDXEType **ret_types = NULL;
            if (d->return_types) {
                int count = 0;
                while (d->return_types[count])
                    count++;
                if (count > 0) {
                    ret_types = (const PDXEType **)pdxe_arena_alloc(
                        arena, (count + 1) * sizeof(const PDXEType *));
                    for (int j = 0; j < count; j++) {
                        ret_types[j] =
                            rust_parse_return_type_text(arena, d->return_types[j], module_qn);
                    }
                    ret_types[count] = NULL;
                }
            } else if (d->return_type && d->return_type[0]) {
                ret_types = (const PDXEType **)pdxe_arena_alloc(arena, 2 * sizeof(const PDXEType *));
                ret_types[0] = rust_parse_return_type_text(arena, d->return_type, module_qn);
                ret_types[1] = NULL;
            }
            const PDXEType **param_types = NULL;
            const char **param_names = d->param_names;
            if (d->signature_param_types || d->signature_param_count > 0) {
                RustSignatureParamParserContext parser_ctx = {.module_qn = module_qn};
                param_types = pdxe_type_materialize_signature_params(
                    arena, d->signature_param_types, d->signature_param_count,
                    rust_signature_param_type_adapter, &parser_ctx);
                param_names = NULL;
            } else if (d->param_types) {
                int count = 0;
                while (d->param_types[count])
                    count++;
                if (count > 0) {
                    param_types = (const PDXEType **)pdxe_arena_alloc(
                        arena, (count + 1) * sizeof(const PDXEType *));
                    for (int j = 0; j < count; j++) {
                        param_types[j] =
                            rust_parse_return_type_text(arena, d->param_types[j], module_qn);
                    }
                    param_types[count] = NULL;
                }
            }
            rf.signature = pdxe_type_func(arena, param_names, param_types, ret_types);

            if (strcmp(d->label, "Method") == 0 && d->parent_class) {
                rf.receiver_type = d->parent_class;
                if (pdxe_idxmemo_get(&type_idx, rf.receiver_type) < 0) {
                    PDXERegisteredType auto_t;
                    memset(&auto_t, 0, sizeof(auto_t));
                    auto_t.qualified_name = rf.receiver_type;
                    const char *dot = strrchr(rf.receiver_type, '.');
                    auto_t.short_name = dot ? pdxe_arena_strdup(arena, dot + 1) : rf.receiver_type;
                    pdxe_registry_add_type(reg, auto_t);
                    pdxe_idxmemo_put_if_absent(&type_idx, arena, auto_t.qualified_name,
                                              reg->type_count - 1);
                }
            }

            pdxe_registry_add_func(reg, rf);
        }
    }

    /* Phase B: walk the AST to extract struct fields + record `impl Trait
     * for Type` linkage as embedded types. */
    if (!ts_node_is_null(root)) {
        PDXE_FOR_EACH_CHILD(top_it, root) {
            TSNode top = top_it.node;
            if (ts_node_is_null(top))
                continue;
            const char *tk = ts_node_type(top);

            if (strcmp(tk, "struct_item") == 0) {
                TSNode name_node = ts_node_child_by_field_name(top, "name", 4);
                TSNode body = ts_node_child_by_field_name(top, "body", 4);
                if (ts_node_is_null(name_node) || ts_node_is_null(body))
                    continue;
                char *tn = pdxe_node_text(arena, name_node, source);
                if (!tn || !tn[0])
                    continue;
                const char *type_qn = pdxe_arena_sprintf(arena, "%s.%s", module_qn, tn);

                /* Iterate field_declaration_list / ordered_field_declaration_list. */
                if (strcmp(ts_node_type(body), "field_declaration_list") == 0) {
                    const char *fld_names[64];
                    const PDXEType *fld_types[64];
                    int fld_count = 0;
                    PDXE_FOR_EACH_NAMED_CHILD(it2, body) {
                        if (!(fld_count < 63)) {
                            break;
                        }
                        TSNode fd = it2.node;
                        if (strcmp(ts_node_type(fd), "field_declaration") != 0)
                            continue;
                        TSNode fn = ts_node_child_by_field_name(fd, "name", 4);
                        TSNode ft = ts_node_child_by_field_name(fd, "type", 4);
                        char *fname = pdxe_node_text(arena, fn, source);
                        if (!fname)
                            continue;
                        /* Build a temporary context for parsing types. */
                        RustLSPContext tmp;
                        memset(&tmp, 0, sizeof(tmp));
                        tmp.arena = arena;
                        tmp.source = source;
                        tmp.source_len = (int)strlen(source);
                        tmp.registry = reg;
                        tmp.module_qn = module_qn;
                        const PDXEType *ft_t = rust_parse_type_node(&tmp, ft);
                        fld_names[fld_count] = fname;
                        fld_types[fld_count] = ft_t;
                        fld_count++;
                    }
                    if (fld_count > 0) {
                        /* Issue 92: the types named type_qn, from the index. */
                        for (int ti = pdxe_registry_type_first_by_qn(reg, type_qn); ti >= 0;
                             ti = pdxe_registry_type_next_by_qn(reg, ti)) {
                            if (reg->types[ti].qualified_name &&
                                strcmp(reg->types[ti].qualified_name, type_qn) == 0) {
                                const char **names = (const char **)pdxe_arena_alloc(
                                    arena, (fld_count + 1) * sizeof(const char *));
                                const PDXEType **types = (const PDXEType **)pdxe_arena_alloc(
                                    arena, (fld_count + 1) * sizeof(const PDXEType *));
                                for (int fi = 0; fi < fld_count; fi++) {
                                    names[fi] = fld_names[fi];
                                    types[fi] = fld_types[fi];
                                }
                                names[fld_count] = NULL;
                                types[fld_count] = NULL;
                                reg->types[ti].field_names = names;
                                reg->types[ti].field_types = types;
                                break;
                            }
                        }
                    }
                }
            }

            if (strcmp(tk, "trait_item") == 0) {
                TSNode name_node = ts_node_child_by_field_name(top, "name", 4);
                TSNode body = ts_node_child_by_field_name(top, "body", 4);
                if (ts_node_is_null(name_node))
                    continue;
                char *tn = pdxe_node_text(arena, name_node, source);
                if (!tn || !tn[0])
                    continue;
                const char *trait_qn = pdxe_arena_sprintf(arena, "%s.%s", module_qn, tn);

                /* Mark as interface and collect method names (issue 92: the types
                 * named trait_qn, from the index). */
                for (int ti = pdxe_registry_type_first_by_qn(reg, trait_qn); ti >= 0;
                     ti = pdxe_registry_type_next_by_qn(reg, ti)) {
                    if (!reg->types[ti].qualified_name)
                        continue;
                    if (strcmp(reg->types[ti].qualified_name, trait_qn) == 0) {
                        reg->types[ti].is_interface = true;
                        if (!ts_node_is_null(body)) {
                            const char *methods[64];
                            int mc = 0;
                            PDXE_FOR_EACH_NAMED_CHILD(it2, body) {
                                if (!(mc < 63)) {
                                    break;
                                }
                                TSNode item = it2.node;
                                const char *ik = ts_node_type(item);
                                if (strcmp(ik, "function_item") != 0 &&
                                    strcmp(ik, "function_signature_item") != 0)
                                    continue;
                                TSNode mn = ts_node_child_by_field_name(item, "name", 4);
                                if (ts_node_is_null(mn))
                                    continue;
                                char *mname = pdxe_node_text(arena, mn, source);
                                if (mname)
                                    methods[mc++] = mname;
                            }
                            if (mc > 0) {
                                const char **arr = (const char **)pdxe_arena_alloc(
                                    arena, (mc + 1) * sizeof(const char *));
                                for (int mi = 0; mi < mc; mi++)
                                    arr[mi] = methods[mi];
                                arr[mc] = NULL;
                                reg->types[ti].method_names = arr;
                            }
                        }
                        break;
                    }
                }
            }
        }
    }

    /* Phase B1: walk top-level free `function_item`s to harvest their
     * return types into the registry — `extract_defs` does not fill
     * `return_type` for Rust free functions either, so a let-binding
     * like `let v = pair();` would otherwise know nothing about pair's
     * return tuple. */
    if (!ts_node_is_null(root)) {
        RustLSPContext tmp;
        memset(&tmp, 0, sizeof(tmp));
        tmp.arena = arena;
        tmp.source = source;
        tmp.source_len = (int)strlen(source);
        tmp.registry = reg;
        tmp.module_qn = module_qn;

        PDXE_FOR_EACH_CHILD(kid_it, root) {
            TSNode top = kid_it.node;
            if (ts_node_is_null(top) || strcmp(ts_node_type(top), "function_item") != 0)
                continue;
            TSNode mn = ts_node_child_by_field_name(top, "name", 4);
            TSNode rtn = ts_node_child_by_field_name(top, "return_type", 11);
            if (ts_node_is_null(mn) || ts_node_is_null(rtn))
                continue;
            char *fname = pdxe_node_text(arena, mn, source);
            if (!fname)
                continue;
            const PDXEType *ret = rust_parse_type_node(&tmp, rtn);
            const char *fn_qn = pdxe_arena_sprintf(arena, "%s.%s", module_qn, fname);
            /* Issue 92: the funcs named fn_qn, from the index. */
            for (int k = pdxe_registry_func_first_by_qn(reg, fn_qn); k >= 0;
                 k = pdxe_registry_func_next_by_qn(reg, k)) {
                PDXERegisteredFunc *rf = &reg->funcs[k];
                if (!rf->qualified_name)
                    continue;
                if (rf->receiver_type)
                    continue; /* free fns only */
                if (strcmp(rf->qualified_name, fn_qn) != 0)
                    continue;
                const PDXEType **ret_arr =
                    (const PDXEType **)pdxe_arena_alloc(arena, 2 * sizeof(const PDXEType *));
                ret_arr[0] = ret;
                ret_arr[1] = NULL;
                rf->signature = pdxe_type_func_replace_returns(arena, rf->signature, ret_arr);
                break;
            }
        }
    }

    /* Phase A2: derive-macro synthesis.
     *
     * Real Rust code is saturated with `#[derive(Clone, Debug, …)]`.
     * Without expanding proc-macros we can still synthesize the trait
     * impl footprint that each well-known derive generates, so calls
     * like `x.clone()` / `format!("{:?}", x)` / `MyT::default()` on the
     * derived type actually resolve.
     *
     * We only synthesize the curated, high-frequency derives — anything
     * unknown is left alone (per the FOLLOWUP doc's "no false edge"
     * policy). Each synthesized impl:
     *   - registers a method (or static fn for `default`/`parse`) on
     *     the receiver type with the right short name and return type;
     *   - appends the trait's QN to the receiver's `embedded_types` so
     *     trait dispatch via `resolve_trait_method` walks it.
     */
    {
        /* Curated derive → (trait QN, [methods with sig sketch]) table. */
        struct DeriveMethod {
            const char *short_name;
            const char *return_type; /* QN or NULL for unknown */
            bool is_static;          /* no `self` (e.g. `default`, `parse`) */
        };
        struct DeriveImpl {
            const char *derive_name;
            const char *trait_qn;
            struct DeriveMethod methods[4]; /* NULL-terminated by empty short_name */
        };
        static const struct DeriveImpl derives[] = {
            {"Clone", "core.clone.Clone", {{"clone", NULL, false}, {NULL, NULL, false}}},
            {"Copy", "core.marker.Copy", {{NULL, NULL, false}}}, /* marker — no methods */
            {"Debug", "core.fmt.Debug", {{"fmt", NULL, false}, {NULL, NULL, false}}},
            {"Display", "core.fmt.Display", {{"fmt", NULL, false}, {NULL, NULL, false}}},
            {"Default", "core.default.Default", {{"default", NULL, true}, {NULL, NULL, false}}},
            {"PartialEq",
             "core.cmp.PartialEq",
             {{"eq", "bool", false}, {"ne", "bool", false}, {NULL, NULL, false}}},
            {"Eq", "core.cmp.Eq", {{NULL, NULL, false}}}, /* marker only */
            {"PartialOrd",
             "core.cmp.PartialOrd",
             {{"partial_cmp", NULL, false},
              {"lt", "bool", false},
              {"le", "bool", false},
              {NULL, NULL, false}}},
            {"Ord", "core.cmp.Ord", {{"cmp", NULL, false}, {NULL, NULL, false}}},
            {"Hash", "core.hash.Hash", {{"hash", "()", false}, {NULL, NULL, false}}},
            {"Send", "core.marker.Send", {{NULL, NULL, false}}},
            {"Sync", "core.marker.Sync", {{NULL, NULL, false}}},
            /* serde — extremely common. */
            {"Serialize", "serde.Serialize", {{"serialize", NULL, false}, {NULL, NULL, false}}},
            {"Deserialize",
             "serde.Deserialize",
             {{"deserialize", NULL, true}, {NULL, NULL, false}}},
            /* clap derive — synthesizes the Parser interface. */
            {"Parser",
             "clap.Parser",
             {{"parse", NULL, true},
              {"try_parse", NULL, true},
              {"parse_from", NULL, true},
              {"try_parse_from", NULL, true}}},
            {"Args", "clap.Args", {{NULL, NULL, false}}},
            {"Subcommand", "clap.Subcommand", {{NULL, NULL, false}}},
            {"ValueEnum", "clap.ValueEnum", {{NULL, NULL, false}}},
            /* thiserror — adds the Error impl. */
            {"Error", "core.error.Error", {{NULL, NULL, false}}},
        };
        const int derive_count = (int)(sizeof(derives) / sizeof(derives[0]));

        for (int i = 0; i < result->defs.count; i++) {
            PDXEDefinition *d = &result->defs.items[i];
            if (!d->qualified_name || !d->name)
                continue;
            /* `#[derive(...)]` rides on type-like defs — most often a struct or
             * enum (now labelled "Struct"/"Enum"), also type aliases. Accept the
             * whole type-like set so a derive on a struct is not dropped. */
            if (!pdxe_label_is_type_like(d->label))
                continue;
            if (!d->decorators)
                continue;

            /* Scan decorator strings for `#[derive(...)]`. */
            for (int di = 0; d->decorators[di]; di++) {
                const char *dec = d->decorators[di];
                const char *p = strstr(dec, "derive");
                if (!p)
                    continue;
                const char *lparen = strchr(p, '(');
                if (!lparen)
                    continue;
                const char *rparen = strchr(lparen, ')');
                if (!rparen)
                    continue;
                /* Now walk between the parens, splitting on comma. */
                const char *q = lparen + 1;
                while (q < rparen) {
                    while (q < rparen && (*q == ' ' || *q == ','))
                        q++;
                    /* Find the end of the identifier (may be qualified
                     * like `serde::Serialize`). We grab the trailing
                     * segment as the derive name. */
                    const char *tok_start = q;
                    while (q < rparen && *q != ',' && *q != ' ')
                        q++;
                    if (q == tok_start)
                        break;
                    /* Trailing-segment after the last `::`. */
                    const char *short_start = tok_start;
                    for (const char *r = tok_start; r < q - 1; r++) {
                        if (r[0] == ':' && r[1] == ':')
                            short_start = r + 2;
                    }
                    size_t name_len = (size_t)(q - short_start);
                    if (name_len == 0 || name_len > 64)
                        continue;
                    /* Look up in curated table. */
                    for (int di2 = 0; di2 < derive_count; di2++) {
                        const struct DeriveImpl *di_entry = &derives[di2];
                        size_t entry_len = strlen(di_entry->derive_name);
                        if (entry_len != name_len)
                            continue;
                        if (strncmp(di_entry->derive_name, short_start, name_len) != 0)
                            continue;

                        /* Found a matching curated derive. Register the
                         * trait QN as an embedded_type on the receiver
                         * AND synthesize the method entries. */
                        PDXERegisteredType *rt = NULL;
                        /* Issue 92: the first type of that name, from the index. */
                        for (int ti = pdxe_registry_type_first_by_qn(reg, d->qualified_name);
                             ti >= 0; ti = pdxe_registry_type_next_by_qn(reg, ti)) {
                            if (reg->types[ti].qualified_name &&
                                strcmp(reg->types[ti].qualified_name, d->qualified_name) == 0) {
                                rt = &reg->types[ti];
                                break;
                            }
                        }
                        if (!rt)
                            break;

                        /* Append trait QN to embedded_types. */
                        int existing = 0;
                        if (rt->embedded_types) {
                            while (rt->embedded_types[existing])
                                existing++;
                        }
                        const char **new_arr = (const char **)pdxe_arena_alloc(
                            arena, (existing + 2) * sizeof(const char *));
                        for (int k = 0; k < existing; k++) {
                            new_arr[k] = rt->embedded_types[k];
                        }
                        new_arr[existing] = di_entry->trait_qn;
                        new_arr[existing + 1] = NULL;
                        rt->embedded_types = new_arr;
                        /* Issue 109: the derive's relation; no header is known. */
                        PDXERustImplFacts *derived =
                            (PDXERustImplFacts *)pdxe_arena_alloc(arena, sizeof(*derived));
                        if (derived) {
                            memset(derived, 0, sizeof(*derived));
                            derived->trait_qn = di_entry->trait_qn;
                            derived->self_form = 'o';
                            derived->derived = true;
                            rust_registered_type_add_impl(arena, rt, derived);
                        }

                        /* Synthesize methods. Bound `mi < 4` BEFORE dereferencing
                         * methods[mi] so we never read methods[4] (OOB). */
                        for (int mi = 0; mi < 4 && di_entry->methods[mi].short_name; mi++) {
                            const struct DeriveMethod *dm = &di_entry->methods[mi];
                            PDXERegisteredFunc rf;
                            memset(&rf, 0, sizeof(rf));
                            rf.short_name = dm->short_name;
                            rf.qualified_name = pdxe_arena_sprintf(arena, "%s.%s", d->qualified_name,
                                                                  dm->short_name);
                            /* Static methods (default/parse) have no
                             * receiver; method calls treat them as
                             * static path lookups via UFCS. */
                            rf.receiver_type = d->qualified_name;
                            rf.min_params = -1;
                            rf.flags |= PDXE_FUNC_FLAG_RUST_TRAIT_IMPL | PDXE_FUNC_FLAG_RUST_DERIVED;
                            rf.impl_trait_qn = di_entry->trait_qn;
                            const PDXEType *ret_t = pdxe_type_unknown();
                            if (dm->return_type) {
                                if (strcmp(dm->return_type, "bool") == 0) {
                                    ret_t = pdxe_type_builtin(arena, "bool");
                                } else if (strcmp(dm->return_type, "()") == 0) {
                                    ret_t = pdxe_type_builtin(arena, "()");
                                }
                            } else if (dm->is_static) {
                                /* `default()`, `parse()` return Self. */
                                ret_t = pdxe_type_named(arena, d->qualified_name);
                            }
                            const PDXEType **ra = (const PDXEType **)pdxe_arena_alloc(
                                arena, 2 * sizeof(const PDXEType *));
                            ra[0] = ret_t;
                            ra[1] = NULL;
                            rf.signature = pdxe_type_func(arena, NULL, NULL, ra);
                            pdxe_registry_add_func(reg, rf);
                        }
                        break;
                    }
                }
            }
        }
    }

    /* Phase B2: walk impl bodies to harvest each method's return type
     * from the AST. The unified `extract_defs` extractor does not fill
     * `return_type` for Rust impl methods, so without this pass our
     * registered functions have no return-type signature and chained
     * method calls (`File::open().read()`) break. */
    if (!ts_node_is_null(root)) {
        PDXE_FOR_EACH_CHILD(top_it, root) {
            TSNode top = top_it.node;
            if (ts_node_is_null(top) || strcmp(ts_node_type(top), "impl_item") != 0)
                continue;
            TSNode type_node = ts_node_child_by_field_name(top, "type", 4);
            TSNode body = ts_node_child_by_field_name(top, "body", 4);
            if (ts_node_is_null(type_node) || ts_node_is_null(body))
                continue;
            char *type_name = pdxe_node_text(arena, type_node, source);
            if (!type_name || !type_name[0])
                continue;
            const char *type_qn = pdxe_arena_sprintf(arena, "%s.%s", module_qn, type_name);

            RustLSPContext tmp;
            memset(&tmp, 0, sizeof(tmp));
            tmp.arena = arena;
            tmp.source = source;
            tmp.source_len = (int)strlen(source);
            tmp.registry = reg;
            tmp.module_qn = module_qn;
            tmp.self_type_qn = type_qn;

            PDXE_FOR_EACH_CHILD(it2, body) {
                TSNode item = it2.node;
                if (ts_node_is_null(item) || !ts_node_is_named(item))
                    continue;
                if (strcmp(ts_node_type(item), "function_item") != 0)
                    continue;
                TSNode mn = ts_node_child_by_field_name(item, "name", 4);
                TSNode rtn = ts_node_child_by_field_name(item, "return_type", 11);
                if (ts_node_is_null(mn) || ts_node_is_null(rtn))
                    continue;
                char *mname = pdxe_node_text(arena, mn, source);
                if (!mname)
                    continue;
                const PDXEType *ret = rust_parse_type_node(&tmp, rtn);
                /* Substitute Self -> receiver type so chains work. */
                if (ret && ret->kind == PDXE_TYPE_NAMED &&
                    strcmp(ret->data.named.qualified_name, "Self") == 0) {
                    ret = pdxe_type_named(arena, type_qn);
                }
                /* Patch the registered function's signature: the first method of
                 * the type with that name, from the index (issue 92). */
                for (int k = pdxe_registry_func_first_by_method(reg, type_qn, mname); k >= 0;
                     k = pdxe_registry_func_next_by_method(reg, k)) {
                    PDXERegisteredFunc *rf = &reg->funcs[k];
                    if (!rf->receiver_type || !rf->short_name)
                        continue;
                    if (strcmp(rf->receiver_type, type_qn) != 0)
                        continue;
                    if (strcmp(rf->short_name, mname) != 0)
                        continue;
                    const PDXEType **ret_arr =
                        (const PDXEType **)pdxe_arena_alloc(arena, 2 * sizeof(const PDXEType *));
                    ret_arr[0] = ret;
                    ret_arr[1] = NULL;
                    rf->signature = pdxe_type_func_replace_returns(arena, rf->signature, ret_arr);
                    break;
                }
            }
        }
    }

    /* Phase C: encode `impl Trait for Type` as `embedded_types` on the
     * receiver type so trait dispatch can find them. */
    for (int i = 0; i < result->impl_traits.count; i++) {
        PDXEImplTrait *it = &result->impl_traits.items[i];
        if (!it->struct_name || !it->trait_name)
            continue;
        const char *recv_qn = it->struct_qn
                                  ? it->struct_qn
                                  : pdxe_arena_sprintf(arena, "%s.%s", module_qn, it->struct_name);
        const char *trait_qn =
            rust_canonical_impl_trait(arena, reg, &trait_names, it->trait_name, module_qn);
        if (!trait_qn) {
            continue;
        }

        PDXERegisteredType *rt = NULL;
        int32_t type_index = pdxe_idxmemo_get(&type_idx, recv_qn);
        if (type_index >= 0) {
            rt = &reg->types[type_index];
        }
        if (!rt) {
            PDXERegisteredType auto_t;
            memset(&auto_t, 0, sizeof(auto_t));
            auto_t.qualified_name = recv_qn;
            const char *dot = strrchr(recv_qn, '.');
            auto_t.short_name = dot ? dot + 1 : recv_qn;
            pdxe_registry_add_type(reg, auto_t);
            rt = &reg->types[reg->type_count - 1];
            pdxe_idxmemo_put_if_absent(&type_idx, arena, auto_t.qualified_name, reg->type_count - 1);
        }
        rust_registered_type_add_embedded(arena, rt, trait_qn);
        rust_registered_type_add_impl(arena, rt,
                                      rust_impl_facts_read(arena, reg, &trait_names,
                                                           it->impl_bounds, module_qn, trait_qn));
    }
}

void pdxe_run_rust_lsp_with_manifest(PDXEArena *arena, PDXEFileResult *result, const char *source,
                                    int source_len, TSNode root,
                                    const struct PDXECargoManifest *manifest) {
    if (!arena || !result || !source)
        return;
    const char *module_qn = result->module_qn ? result->module_qn : "rust";

    PDXETypeRegistry reg;
    pdxe_rust_build_local_registry(arena, &reg, result, module_qn, root, source);
    /* Finalize after all per-file adds so lookups during the walk
     * use the hash buckets. */
    pdxe_registry_finalize(&reg);

    RustLSPContext ctx;
    rust_lsp_init(&ctx, arena, source, source_len, &reg, module_qn, &result->resolved_calls);
    ctx.cargo_manifest = manifest;
    /* Let the resolver inject synthetic syntactic calls for operator/macro
     * desugaring so those recovered calls reach the CALLS-edge pipeline. */
    ctx.syn_calls = &result->calls;

    rust_collect_uses(&ctx, root);
    /* Bridge any extracted PDXEImports the unified extractor saw. */
    for (int i = 0; i < result->imports.count; i++) {
        PDXEImport *imp = &result->imports.items[i];
        if (imp->local_name && imp->module_path) {
            rust_lsp_add_use(&ctx, imp->local_name, imp->module_path);
        }
    }

    rust_lsp_process_file(&ctx, root);
}

void pdxe_run_rust_lsp(PDXEArena *arena, PDXEFileResult *result, const char *source, int source_len,
                      TSNode root) {
    pdxe_run_rust_lsp_with_manifest(arena, result, source, source_len, root, NULL);
}

/* ════════════════════════════════════════════════════════════════════
 * 12. Cross-file + batch
 * ════════════════════════════════════════════════════════════════════ */

extern const TSLanguage *tree_sitter_rust(void);

/* Populate + finalize a Rust cross-file type registry from `defs`. Shared by the
 * per-file resolver (pdxe_run_rust_lsp_cross_with_manifest) and the build-once shared
 * registry (pdxe_rust_build_cross_registry) so both produce a byte-identical registry.
 * `module_qn` is ONLY the fallback used to qualify a def's return type when that def
 * carries no def_module_qn; pass NULL for the shared build (all_defs always carry
 * def_module_qn — verified: 0 NULL across the C + Rust kernel corpora). */
/* Issue 105: a struct's fields, "name:type" pairs separated by "|" as the cross-file
 * pass folds them, each type read where the struct is written (`module_qn`). A field
 * whose pair cannot be read is left out; none leaves the type without fields. */
static void rust_registered_type_set_fields(PDXEArena *arena, PDXERegisteredType *rt,
                                            const char *field_defs, const char *module_qn) {
    if (!field_defs || !field_defs[0]) {
        return;
    }
    int count = 1;
    for (const char *p = field_defs; *p; p++) {
        count += *p == '|';
    }
    const char **names = (const char **)pdxe_arena_alloc(arena, (size_t)(count + 1) * sizeof(char *));
    const PDXEType **types =
        (const PDXEType **)pdxe_arena_alloc(arena, (size_t)(count + 1) * sizeof(PDXEType *));
    char *buf = pdxe_arena_strdup(arena, field_defs);
    if (!names || !types || !buf) {
        return;
    }
    int n = 0;
    for (char *pair = buf; pair;) {
        char *bar = strchr(pair, '|');
        if (bar) {
            *bar = '\0';
        }
        char *colon = strchr(pair, ':');
        if (colon && colon > pair && colon[1]) {
            *colon = '\0';
            names[n] = pair;
            types[n] = rust_parse_return_type_text(arena, colon + 1, module_qn);
            n++;
        }
        pair = bar ? bar + 1 : NULL;
    }
    names[n] = NULL;
    types[n] = NULL;
    if (n > 0) {
        rt->field_names = names;
        rt->field_types = types;
    }
}

static void rust_populate_cross_registry(PDXETypeRegistry *reg, PDXEArena *arena,
                                         PDXERustLSPDef *defs, int def_count,
                                         const char *module_qn) {
    pdxe_registry_init(reg, arena);
    pdxe_rust_stdlib_register(reg, arena);

    /* qn → (type index + 1), FIRST occurrence wins (mirrors the linear scans
     * this map replaces). Both in-loop registry probes below — the receiver
     * auto-registration check and the trait-linkage lookup — used to scan the
     * UNFINALIZED registry linearly (no buckets exist before finalize): the
     * checklist's lookup-in-registration-loop pattern. Invisible on small
     * per-file builds; on the shared all_defs build (~1.4M entries) those
     * scans were a constant ~63 s of the kernel run — and the sibling
     * null-filter files waited on the build once-guard for exactly that long,
     * which is why no resolution-side fix ever moved their wall time. Index,
     * not pointer, because reg->types reallocs as it grows. */
    PDXEIdxMemo type_idx = {0};
    for (int ti = 0; ti < reg->type_count; ti++) {
        const char *qn = reg->types[ti].qualified_name;
        if (qn) {
            pdxe_idxmemo_put_if_absent(&type_idx, arena, qn, ti);
        }
    }

    /* Register all type declarations before resolving raw impl-trait names.
     * This makes provenance independent of source/file ordering. */
    for (int i = 0; i < def_count; i++) {
        PDXERustLSPDef *d = &defs[i];
        if (!d->qualified_name || !d->short_name || !d->label)
            continue;
        if (!pdxe_label_is_type_like(d->label)) {
            continue;
        }
        PDXERegisteredType rt;
        memset(&rt, 0, sizeof(rt));
        rt.qualified_name = pdxe_arena_strdup(arena, d->qualified_name);
        rt.short_name = pdxe_arena_strdup(arena, d->short_name);
        rt.is_interface =
            d->is_interface || strcmp(d->label, "Trait") == 0 || strcmp(d->label, "Interface") == 0;
        rust_registered_type_set_fields(arena, &rt, d->rust_field_defs,
                                        d->def_module_qn ? d->def_module_qn : module_qn);
        pdxe_registry_add_type(reg, rt);
        pdxe_idxmemo_put_if_absent(&type_idx, arena, rt.qualified_name, reg->type_count - 1);
    }

    RustTraitNameIndex trait_names;
    rust_trait_name_index_build(&trait_names, arena, reg);

    /* Materialize independent impl relations first, including empty impls. */
    for (int i = 0; i < def_count; i++) {
        PDXERustLSPDef *d = &defs[i];
        if (!d->is_rust_impl_relation || !d->receiver_type || !d->trait_qn) {
            continue;
        }
        const char *def_mod = d->def_module_qn ? d->def_module_qn : module_qn;
        const char *trait_qn =
            rust_canonical_impl_trait(arena, reg, &trait_names, d->trait_qn, def_mod);
        if (!trait_qn) {
            continue;
        }
        int32_t type_index = pdxe_idxmemo_get(&type_idx, d->receiver_type);
        if (type_index < 0) {
            PDXERegisteredType auto_t;
            memset(&auto_t, 0, sizeof(auto_t));
            auto_t.qualified_name = pdxe_arena_strdup(arena, d->receiver_type);
            const char *dot = strrchr(d->receiver_type, '.');
            auto_t.short_name = dot ? pdxe_arena_strdup(arena, dot + 1) : auto_t.qualified_name;
            pdxe_registry_add_type(reg, auto_t);
            type_index = reg->type_count - 1;
            pdxe_idxmemo_put_if_absent(&type_idx, arena, auto_t.qualified_name, type_index);
        }
        rust_registered_type_add_embedded(arena, &reg->types[type_index], trait_qn);
        rust_registered_type_add_impl(arena, &reg->types[type_index],
                                      rust_impl_facts_read(arena, reg, &trait_names,
                                                           d->impl_bounds, def_mod, trait_qn));
    }

    for (int i = 0; i < def_count; i++) {
        PDXERustLSPDef *d = &defs[i];
        if (!d->qualified_name || !d->short_name || !d->label || d->is_rust_impl_relation)
            continue;
        const char *def_mod = d->def_module_qn ? d->def_module_qn : module_qn;

        if (strcmp(d->label, "Function") == 0 || strcmp(d->label, "Method") == 0) {
            PDXERegisteredFunc rf;
            memset(&rf, 0, sizeof(rf));
            rf.qualified_name = pdxe_arena_strdup(arena, d->qualified_name);
            rf.short_name = pdxe_arena_strdup(arena, d->short_name);
            rf.min_params = -1;
            if (d->is_abstract) {
                rf.flags |= PDXE_FUNC_FLAG_RUST_ABSTRACT;
            }
            if (d->trait_qn && d->trait_qn[0]) {
                rf.flags |= PDXE_FUNC_FLAG_RUST_TRAIT_IMPL;
                rf.impl_trait_qn =
                    rust_canonical_impl_trait(arena, reg, &trait_names, d->trait_qn, def_mod);
                rf.rust_impl = rust_impl_facts_read(arena, reg, &trait_names, d->impl_bounds,
                                                    def_mod, rf.impl_trait_qn);
            }

            /* Build sig from return_types text. */
            const PDXEType **ret_types = NULL;
            if (d->return_types && d->return_types[0]) {
                int count = 1;
                for (const char *p = d->return_types; *p; p++) {
                    if (*p == '|')
                        count++;
                }
                ret_types =
                    (const PDXEType **)pdxe_arena_alloc(arena, (count + 1) * sizeof(const PDXEType *));
                int idx = 0;
                char *buf = pdxe_arena_strdup(arena, d->return_types);
                char *start = buf;
                for (char *p = buf;; p++) {
                    if (*p == '|' || *p == '\0') {
                        char save = *p;
                        *p = '\0';
                        if (start[0]) {
                            ret_types[idx++] = rust_parse_return_type_text(arena, start, def_mod);
                        }
                        if (save == '\0')
                            break;
                        start = p + 1;
                    }
                }
                ret_types[idx] = NULL;
            } else if (d->rust_return_type && d->rust_return_type[0]) {
                /* Issue 105: an impl method's declared return type, one type. */
                ret_types = (const PDXEType **)pdxe_arena_alloc(arena, 2 * sizeof(const PDXEType *));
                if (ret_types) {
                    ret_types[0] = rust_parse_return_type_text(arena, d->rust_return_type, def_mod);
                    ret_types[1] = NULL;
                }
            }
            if (d->rust_is_async) {
                /* Issue 105: calling an `async` function gives a future of what it
                 * declares to return (`()` when it declares nothing); `.await` gives
                 * that. */
                const PDXEType *output = ret_types && ret_types[0]
                                             ? ret_types[0]
                                             : pdxe_type_builtin(arena, "()");
                const PDXEType **wrapped =
                    (const PDXEType **)pdxe_arena_alloc(arena, 2 * sizeof(const PDXEType *));
                if (wrapped && output) {
                    wrapped[0] = pdxe_type_template(arena, "core.future.Future", &output, 1);
                    wrapped[1] = NULL;
                    ret_types = wrapped;
                }
            }
            RustSignatureParamParserContext parser_ctx = {.module_qn = def_mod};
            const PDXEType **param_types = pdxe_type_materialize_signature_params(
                arena, d->signature_param_types, d->signature_param_count,
                rust_signature_param_type_adapter, &parser_ctx);
            rf.signature = pdxe_type_func(arena, NULL, param_types, ret_types);

            if (strcmp(d->label, "Method") == 0 && d->receiver_type && d->receiver_type[0]) {
                rf.receiver_type = pdxe_arena_strdup(arena, d->receiver_type);
                if (pdxe_idxmemo_get(&type_idx, rf.receiver_type) < 0) {
                    PDXERegisteredType auto_t;
                    memset(&auto_t, 0, sizeof(auto_t));
                    auto_t.qualified_name = rf.receiver_type;
                    const char *dot = strrchr(d->receiver_type, '.');
                    auto_t.short_name = dot ? pdxe_arena_strdup(arena, dot + 1) : rf.receiver_type;
                    pdxe_registry_add_type(reg, auto_t);
                    pdxe_idxmemo_put_if_absent(&type_idx, arena, auto_t.qualified_name,
                                              reg->type_count - 1);
                }
            }

            pdxe_registry_add_func(reg, rf);

            /* Method provenance also carries the type-level relation for
             * backwards-compatible direct PDXERustLSPDef callers. Production
             * extraction additionally emits an independent relation record. */
            if (rf.receiver_type && rf.impl_trait_qn) {
                PDXERegisteredType *rt = NULL;
                int32_t tix = pdxe_idxmemo_get(&type_idx, rf.receiver_type);
                if (tix >= 0) {
                    rt = &reg->types[tix];
                }
                if (rt) {
                    rust_registered_type_add_embedded(arena, rt, rf.impl_trait_qn);
                }
            }
        }
    }

    /* Finalise the cross-file registry now that all defs are added.
     * (type_idx is arena-owned — freed with the registry's arena.) */
    pdxe_registry_finalize(reg);
}

/* Resolve one Rust file against an ALREADY-built (per-file or shared) registry. */
static void rust_resolve_against_registry(PDXEArena *arena, const char *source, int source_len,
                                          const char *module_qn, const PDXETypeRegistry *reg,
                                          const char **import_names, const char **import_qns,
                                          int import_count, TSNode root,
                                          const struct PDXECargoManifest *manifest,
                                          PDXEResolvedCallArray *out,
                                          PDXECallArray *synthetic_calls) {
    RustLSPContext ctx;
    rust_lsp_init(&ctx, arena, source, source_len, reg, module_qn, out);
    ctx.cargo_manifest = manifest;
    ctx.syn_calls = synthetic_calls;
    rust_collect_uses(&ctx, root);
    for (int i = 0; i < import_count; i++) {
        if (import_names[i] && import_qns[i]) {
            rust_lsp_add_use(&ctx, import_names[i], import_qns[i]);
        }
    }
    rust_lsp_process_file(&ctx, root);
}

/* Tier-2: build the Rust cross registry ONCE from all project defs, sealed
 * read-only, and shared across every Rust file's resolve (mirrors C/py/cs/ts).
 * Converts the pipeline's PDXELSPDef into PDXERustLSPDef inline (same field copy as
 * pass_lsp_cross.c's pxc_lspdefs_to_rust, including proven trait provenance).
 * module_qn=NULL is byte-identical because all_defs always carry def_module_qn. */
PDXETypeRegistry *pdxe_rust_build_cross_registry(PDXEArena *arena, PDXELSPDef *defs, int def_count) {
    if (!arena)
        return NULL;
    PDXETypeRegistry *reg = (PDXETypeRegistry *)pdxe_arena_alloc(arena, sizeof(*reg));
    if (!reg)
        return NULL;
    PDXERustLSPDef *rdefs = NULL;
    if (def_count > 0) {
        rdefs = (PDXERustLSPDef *)pdxe_arena_alloc(arena, (size_t)def_count * sizeof(PDXERustLSPDef));
        if (!rdefs)
            return NULL;
        for (int i = 0; i < def_count; i++) {
            rdefs[i].qualified_name = defs[i].qualified_name;
            rdefs[i].short_name = defs[i].short_name;
            rdefs[i].label = defs[i].label;
            rdefs[i].receiver_type = defs[i].receiver_type;
            rdefs[i].def_module_qn = defs[i].def_module_qn;
            rdefs[i].return_types = defs[i].return_types;
            rdefs[i].embedded_types = defs[i].embedded_types;
            rdefs[i].field_defs = defs[i].field_defs;
            rdefs[i].method_names_str = defs[i].method_names_str;
            rdefs[i].signature_param_types = defs[i].signature_param_types;
            rdefs[i].signature_param_count = defs[i].signature_param_count;
            rdefs[i].trait_qn = defs[i].trait_qn;
            rdefs[i].is_interface = defs[i].is_interface;
            rdefs[i].is_rust_impl_relation = defs[i].is_rust_impl_relation;
            rdefs[i].is_abstract = defs[i].is_abstract;
            rdefs[i].impl_bounds = defs[i].impl_bounds;
            rdefs[i].rust_return_type = defs[i].rust_return_type;
            rdefs[i].rust_field_defs = defs[i].rust_field_defs;
            rdefs[i].rust_is_async = defs[i].rust_is_async;
        }
    }
    rust_populate_cross_registry(reg, arena, rdefs, def_count, /*module_qn=*/NULL);
    reg->read_only = true; /* seal: shared Tier-2 registry is read-only during resolve */
    return reg;
}

/* Cross-file Rust resolve using a pre-built shared registry (Tier-2). Skips the
 * per-file registry build; just parse + resolve. Mirrors pdxe_run_c_lsp_cross_with_registry. */
void pdxe_run_rust_lsp_cross_with_registry(PDXEArena *arena, const char *source, int source_len,
                                          const char *module_qn, const PDXETypeRegistry *reg,
                                          const char **import_names, const char **import_qns,
                                          int import_count, TSTree *cached_tree,
                                          const struct PDXECargoManifest *manifest,
                                          PDXEResolvedCallArray *out,
                                          PDXECallArray *synthetic_calls) {
    if (!source || source_len <= 0 || !out || !reg)
        return;
    TSParser *parser = NULL;
    TSTree *tree = cached_tree;
    bool owns_tree = false;
    if (!tree) {
        parser = ts_parser_new();
        if (!parser)
            return;
        ts_parser_set_language(parser, tree_sitter_rust());
        tree = ts_parser_parse_string(parser, NULL, source, source_len);
        owns_tree = true;
        if (!tree) {
            ts_parser_delete(parser);
            return;
        }
    }
    TSNode root = ts_tree_root_node(tree);
    rust_resolve_against_registry(arena, source, source_len, module_qn, reg, import_names,
                                  import_qns, import_count, root, manifest, out, synthetic_calls);
    if (owns_tree) {
        ts_tree_delete(tree);
        if (parser)
            ts_parser_delete(parser);
    }
}

void pdxe_run_rust_lsp_cross_with_manifest(PDXEArena *arena, const char *source, int source_len,
                                          const char *module_qn, PDXERustLSPDef *defs, int def_count,
                                          const char **import_names, const char **import_qns,
                                          int import_count, TSTree *cached_tree,
                                          const struct PDXECargoManifest *manifest,
                                          PDXEResolvedCallArray *out,
                                          PDXECallArray *synthetic_calls) {
    if (!source || source_len <= 0 || !out)
        return;

    TSParser *parser = NULL;
    TSTree *tree = cached_tree;
    bool owns_tree = false;
    if (!tree) {
        parser = ts_parser_new();
        if (!parser)
            return;
        ts_parser_set_language(parser, tree_sitter_rust());
        tree = ts_parser_parse_string(parser, NULL, source, source_len);
        owns_tree = true;
        if (!tree) {
            ts_parser_delete(parser);
            return;
        }
    }
    TSNode root = ts_tree_root_node(tree);

    /* Build registry from cross-file defs + stdlib (per-file). */
    PDXETypeRegistry reg;
    rust_populate_cross_registry(&reg, arena, defs, def_count, module_qn);

    rust_resolve_against_registry(arena, source, source_len, module_qn, &reg, import_names,
                                  import_qns, import_count, root, manifest, out, synthetic_calls);

    if (owns_tree) {
        ts_tree_delete(tree);
        if (parser)
            ts_parser_delete(parser);
    }
}

/* Manifest-free entry point. Preserves the pre-existing signature used by
 * the unit tests (test_rust_lsp.c) and the batch wrapper — delegates to
 * the manifest-aware variant with a NULL manifest. */
void pdxe_run_rust_lsp_cross(PDXEArena *arena, const char *source, int source_len,
                            const char *module_qn, PDXERustLSPDef *defs, int def_count,
                            const char **import_names, const char **import_qns, int import_count,
                            TSTree *cached_tree, PDXEResolvedCallArray *out) {
    pdxe_run_rust_lsp_cross_with_manifest(arena, source, source_len, module_qn, defs, def_count,
                                         import_names, import_qns, import_count, cached_tree, NULL,
                                         out, NULL);
}

void pdxe_batch_rust_lsp_cross(PDXEArena *arena, PDXEBatchRustLSPFile *files, int file_count,
                              PDXEResolvedCallArray *out) {
    if (!files || file_count <= 0 || !out)
        return;

    for (int f = 0; f < file_count; f++) {
        PDXEBatchRustLSPFile *file = &files[f];
        memset(&out[f], 0, sizeof(PDXEResolvedCallArray));
        if (!file->source || file->source_len <= 0)
            continue;

        PDXEArena file_arena;
        pdxe_arena_init(&file_arena);

        PDXEResolvedCallArray file_out;
        memset(&file_out, 0, sizeof(file_out));

        pdxe_run_rust_lsp_cross(&file_arena, file->source, file->source_len, file->module_qn,
                               file->defs, file->def_count, file->import_names, file->import_qns,
                               file->import_count, file->cached_tree, &file_out);

        if (file_out.count > 0) {
            out[f].count = file_out.count;
            out[f].items =
                (PDXEResolvedCall *)pdxe_arena_alloc(arena, file_out.count * sizeof(PDXEResolvedCall));
            for (int j = 0; j < file_out.count; j++) {
                PDXEResolvedCall *src = &file_out.items[j];
                PDXEResolvedCall *dst = &out[f].items[j];
                memset(dst, 0, sizeof(*dst));
                dst->caller_qn = src->caller_qn ? pdxe_arena_strdup(arena, src->caller_qn) : NULL;
                dst->callee_qn = src->callee_qn ? pdxe_arena_strdup(arena, src->callee_qn) : NULL;
                dst->strategy = src->strategy ? pdxe_arena_strdup(arena, src->strategy) : NULL;
                dst->confidence = src->confidence;
                dst->reason = src->reason ? pdxe_arena_strdup(arena, src->reason) : NULL;
                dst->kind = src->kind;
                dst->site_start_byte = src->site_start_byte;
                dst->site_end_byte = src->site_end_byte;
            }
        }

        pdxe_arena_destroy(&file_arena);
    }
}
