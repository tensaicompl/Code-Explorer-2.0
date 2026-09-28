#ifndef PDXE_HELPERS_H
#define PDXE_HELPERS_H

#include "pdxe_core.h"

// Portable memmem: find first occurrence of `needle` (needle_len bytes) within
// `haystack` (haystack_len bytes). Returns a pointer into haystack, or NULL.
// Hand-rolled so it compiles identically on all platforms (GNU/BSD-only
// memmem is unavailable under msys2-clang on Windows).
void *pdxe_memmem(const void *haystack, size_t haystack_len, const void *needle, size_t needle_len);

// Extract text of a node from source. Returns arena-allocated string.
char *pdxe_node_text(PDXEArena *a, TSNode node, const char *source);

// Check if a string is a language keyword (should be skipped as callee/usage).
bool pdxe_is_keyword(const char *name, PDXELanguage lang);

// Check if a name is a builtin we mint a real graph node for, so a CALL to it
// must NOT be keyword-filtered out of call extraction (the LSP resolves it to
// the injected builtin node and forms a CALLS edge). Narrower than
// pdxe_is_keyword: it only covers builtins with a target node, so un-filtering
// them cannot produce a node-less / Module-sourced edge. The Python set MUST
// stay in sync with kPyBuiltinNodes in internal/pdxe/lsp/py_builtins.c.
bool pdxe_is_resolvable_builtin(const char *name, PDXELanguage lang);

// Classify a string literal as URL, config, or neither.
// Returns PDXE_STRREF_URL (0), PDXE_STRREF_CONFIG (1), or -1 for neither.
int pdxe_classify_string(const char *str, int len);

// Check if a name is exported per language convention.
bool pdxe_is_exported(const char *name, PDXELanguage lang);

// Check if a file is a test file based on path and language.
bool pdxe_is_test_file(const char *rel_path, PDXELanguage lang);

// Find the innermost enclosing function node by walking parent chain.
// Returns a null node if none found.
TSNode pdxe_find_enclosing_func(TSNode node, PDXELanguage lang);

// Get the QN of an enclosing function, or module_qn if none.
const char *pdxe_enclosing_func_qn(PDXEArena *a, TSNode node, PDXELanguage lang, const char *source,
                                  const char *project, const char *rel_path, const char *module_qn);

// Cached version: uses ctx->ef_cache to avoid repeated parent-chain walks.
const char *pdxe_enclosing_func_qn_cached(PDXEExtractCtx *ctx, TSNode node);

// Max declarator-chain descent depth for C/C++/CUDA/GLSL function-name
// resolution. Single source of truth — extract_defs.c's DECLARATOR_DEPTH_LIMIT
// is derived from this so the three extractors cannot drift.
#define PDXE_DECLARATOR_DEPTH_LIMIT 8

// Resolve the function-name node for a C/C++/CUDA/GLSL `function_definition`.
// Such nodes have no `name` field — the name is nested in the declarator chain
// (pointer/function/parenthesized/array declarators wrap it; out-of-line method
// definitions name it with a qualified_identifier). Descends the `declarator`
// field to the innermost name node and returns it, or a null node if none is
// found. Shared by the defs, calls, and unified extractors so all three agree on
// enclosing-function attribution — drift between private copies caused #438.
TSNode pdxe_resolve_c_declarator_name_node(TSNode func_node);

// Convert a resolved function/method name node to its name string, normalizing a
// C++ conversion-operator's `operator_cast` node (which spans the full
// "operator bool() const") down to "operator bool". Shared by the defs and
// unified extractors so the def name and call-scope QN agree.
// Also strips the surrounding quotes from a Nix quoted attrpath segment, so
// `"kebab-case" = a: a;` is named kebab-case rather than "kebab-case". Takes the
// language for that reason; every caller must pass ctx->language.
char *pdxe_func_name_node_text(PDXEArena *a, TSNode name_node, const char *source, PDXELanguage lang);

// ── Nix attrpath helpers ──
// A Nix binding's name is a PATH (`a.b.c = …`) whose segments may be quoted or
// interpolated. Shared by the defs and unified (call-scope) extractors so both
// derive the same name and the same scope prefix — divergence makes a CALLS edge
// name a source node that does not exist, and it is dropped at write.

// Strip one matching pair of surrounding double quotes, in place.
void pdxe_nix_strip_attr_quotes(char *text);

// True when an attrpath segment contains a `${...}` interpolation and therefore
// has no statically knowable name.
bool pdxe_nix_attr_is_interpolated(TSNode attr);

// The leaf segment of an attrpath — the name. Null node for an empty attrpath.
TSNode pdxe_nix_attrpath_last_attr(TSNode attrpath);

// The scope prefix of an attrpath: all segments but the leaf, quote-stripped and
// dot-joined, so `a.b.fn = …` qualifies identically to `a = { b = { fn = …; }; }`.
// NULL for a single-segment path, or when a leading segment is interpolated.
const char *pdxe_nix_attrpath_scope(PDXEArena *a, TSNode attrpath, const char *source);

// True when a Nix `binding`'s value is an attribute set — the binding names a
// scope rather than defining a value. Excludes let-bindings and lambda values.
bool pdxe_nix_binding_is_attrset_scope(TSNode node);

// The scope QN contributed by a Nix `binding` whose value is an attribute set.
// Called by BOTH extract_defs.c and extract_unified.c, which carry separate
// compute_class_qn implementations — sharing this makes a def/call-scope QN
// mismatch (which silently drops the CALLS edge) structurally impossible.
const char *pdxe_nix_binding_scope_qn(PDXEExtractCtx *ctx, TSNode node, const char *saved_enclosing);

// The QN-relative name of a Nix binding — its attrpath scope joined to `name`.
// Callers prepend the enclosing attrset scope (or the module QN), so a dotted
// attrpath and an enclosing attrset compose into one qualified name.
const char *pdxe_nix_qn_name(PDXEArena *a, TSNode func_node, const char *source, const char *name);

// Resolve a function/method definition node's NAME node across all ~130 grammars
// (generic `name` field, arrow→declarator, C/C++ declarator chain, plus the many
// per-language quirks: Fortran subroutine, SCSS mixin, SQL create_function, R,
// PowerShell, Ada, the Lisp/FP family, etc.). Defined in extract_defs.c. Shared by
// the defs, calls, and unified extractors so all three agree on enclosing-function
// naming — drift between private copies caused the Module-mis-attribution of
// gap #3 (and #438 for the C-declarator case).
TSNode pdxe_resolve_func_name(TSNode node, PDXELanguage lang);

// C++/CUDA out-of-line method definition (`void Foo::bar() {...}`): return the
// immediate enclosing class name ("Foo") from the qualified declarator, or NULL
// for a plain free function. Defined in extract_defs.c. Shared so the unified
// (call-scope) extractor computes the SAME class-qualified enclosing QN as the
// def extractor — drift dropped the class qualifier from in-body calls (#554/#621).
char *pdxe_cpp_out_of_line_parent_class(PDXEArena *a, TSNode node, const char *source);

// Find a child node by kind string.
TSNode pdxe_find_child_by_kind(TSNode parent, const char *kind);

// Find every child node matching `kind` (unlike pdxe_find_child_by_kind,
// which stops at the first match). Writes up to `max` nodes into `out`;
// returns how many were found. Repeated sibling nodes of the same kind are
// common in some grammars (e.g. C#/PHP stack each `[Attr]` as its own
// attribute_list child) — see #1692.
int pdxe_find_children_by_kind(TSNode parent, const char *kind, TSNode *out, int max);

/* --- Lisp-family shared gates ---------------------------------------------
 * The defs, calls and unified extractors each walk the same generic `list`
 * node and must agree on what it means. The predicates below therefore live
 * here rather than being copied per translation unit: a private copy in each
 * drifts, and the drift is silent — defs and call-scope simply stop describing
 * the same tree.
 */

/* True when any ancestor list of `node` is headed by a quote symbol
 * (`q` / `quote` / `qq`) — its contents are DATA, not code, so no def and no
 * call may be minted from them. Bounded ancestor walk; the arena is used only
 * for the head-text reads. */
bool pdxe_lisp_node_in_quote(PDXEArena *a, TSNode node, const char *source);

/* The `want`-th named child of `node`, skipping `comment` nodes. Comments are
 * named in the s-expression grammars and so occupy named-child indices: a
 * comment between a def head and its name shifts every later index by one.
 * Definition extraction and call-scope attribution MUST use this same skipping
 * rule or they desynchronise on exactly the files that carry doc comments. */
TSNode pdxe_lisp_named_child_skip_comments(TSNode node, uint32_t want);

/* True for a Chialisp definition-form head. Deliberately separate from the
 * Clojure/Scheme def-head set: Chialisp shares the generic `list` kind, and
 * treating `mod`/`defconstant` as defs must not leak into the other lisps (in
 * Scheme `(mod x y)` is a call). Excludes the expression-local binding forms
 * (`let`, `assign`, `lambda`), which would fragment call attribution, and
 * excludes `export`/`namespace`, which NAME an already-defined function rather
 * than defining one. */
bool pdxe_chialisp_is_def_head(const char *t);

// Check if node kind matches a set of types (NULL-terminated array of strings).
bool pdxe_kind_in_set(TSNode node, const char **types);

/* Namespace/module declarations that extend a qualified-name scope without
 * turning their children into class methods. Shared by definition and unified
 * walks so TS/TSX scope attribution cannot drift. */
bool pdxe_is_namespace_scope_kind(PDXELanguage lang, const char *kind);

// Free the calling thread's pdxe_kind_in_set bitset cache (call at thread/process
// teardown so the thread-local cache is not reported as a leak).
void pdxe_kind_in_set_free_cache(void);

// Check if node has an ancestor of the given kind, within max_depth levels.
bool pdxe_has_ancestor_kind(TSNode node, const char *kind, int max_depth);

// Count nodes of given kinds in subtree (for complexity metric).
int pdxe_count_branching(TSNode node, const char **branching_types);

// Per-function structural complexity, computed in a single AST walk.
typedef struct {
    int cyclomatic;       // branching-node count (matches def.complexity)
    int cognitive;        // nesting-weighted flow-break count (Campbell-style approximation)
    int loop_count;       // total loop constructs in the body
    int loop_depth;       // maximum nested-loop depth — structural bottleneck proxy
    int max_access_depth; // deepest chained member/subscript access (a.b.c.d → 4) — structure smell
} pdxe_complexity_t;

// Compute the metrics above in one traversal of `node`'s subtree.
// `branching_types` is the language's branching node-type set.
void pdxe_compute_complexity(TSNode node, const char **branching_types, pdxe_complexity_t *out);

// Is `kind` a loop construct node type? Language-agnostic curated set (for/while/
// do/foreach/repeat/loop variants). Exposed so the unified walk can track loop
// nesting at call sites without re-deriving the set.
bool pdxe_is_loop_node_type(const char *kind);

// Is this a module-level node? (not nested inside function/class body)
bool pdxe_is_module_level(TSNode node, PDXELanguage lang);

// Same check, but the node's PARENT is supplied directly — avoids the
// O(n) ts_node_parent rescan. Use at call sites iterating a known
// parent's children (the common case). `parent` is the parent of the
// node being classified.
bool pdxe_is_module_level_p(TSNode parent, PDXELanguage lang);

// --- FQN computation ---

// Compute qualified name: project.rel_path_parts.name
char *pdxe_fqn_compute(PDXEArena *a, const char *project, const char *rel_path, const char *name);

// Module QN (file without name): project.rel_path_parts
char *pdxe_fqn_module(PDXEArena *a, const char *project, const char *rel_path);

// Language-aware module QN. For directory-module languages (Java package, Go
// package) the module is derived from the CONTAINING DIRECTORY (the filename
// stem is NOT baked in): `Outer.java` at root -> "proj", `myapp/db/conn.go` ->
// "proj.myapp.db". For every OTHER language this returns exactly what
// pdxe_fqn_module returns (no behavior change).
char *pdxe_fqn_module_source_lang(PDXEArena *a, const char *project, const char *rel_path,
                                 PDXELanguage lang);

// Language-aware symbol QN. For directory-module languages this is the
// directory-based module + "." + name (so a top-level class `Outer` in
// `Outer.java` is "proj.Outer", not "proj.Outer.Outer"). For every other
// language this is exactly pdxe_fqn_compute (no behavior change).
char *pdxe_fqn_compute_source_lang(PDXEArena *a, const char *project, const char *rel_path,
                                  const char *name, PDXELanguage lang);

// Folder QN: project.dir_parts
char *pdxe_fqn_folder(PDXEArena *a, const char *project, const char *rel_dir);

/* Flatten a JS/TS `template_string` node into plain text: string fragments are
 * kept verbatim and each ${...} substitution becomes the "{}" placeholder, so
 * client-side URLs built from template literals share the canonical parameter
 * shape that server-side route paths already use. NULL when empty/oversized. */
const char *pdxe_template_string_text(PDXEArena *a, TSNode node, const char *source);

#endif // PDXE_HELPERS_H
