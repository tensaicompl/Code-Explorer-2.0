/*
 * Declarations the vendored resolution sources expect from the reference's
 * pipeline, extracted rather than vendored.
 *
 * The reference declares these in a header that also describes its indexing
 * pipeline: job queues, stores, passes, a graph buffer. None of that exists here,
 * and copying the header to obtain four declarations would drag in a subsystem
 * this project replaces. So the declarations the resolution sources actually use
 * are reproduced here, and nothing else is.
 *
 * Each is implemented by a vendored source of its own, so these are declarations
 * of code we have, not of code we are missing.
 */

#ifndef PDXE_PIPELINE_H
#define PDXE_PIPELINE_H

#include <stdbool.h>

#include "pdxe_core.h"

#ifdef __cplusplus
extern "C" {
#endif

/* --- fully qualified names ----------------------------------------------- */
/*
 * Implemented in the vendored name computation. Each returns storage the caller
 * frees.
 */

/* The qualified name of `name` defined in `rel_path` within `project`. */
char *pdxe_pipeline_fqn_compute(const char *project, const char *rel_path, const char *name);

/* The qualified name of the module that `rel_path` declares. */
char *pdxe_pipeline_fqn_module(const char *project, const char *rel_path);

/*
 * As above, but for languages where a directory is itself a module: pass true and
 * the path is read as the module, not as a file inside one.
 */
char *pdxe_pipeline_fqn_module_dir(const char *project, const char *rel_path, bool module_is_dir);

/* The qualified name of a directory within `project`. */
char *pdxe_pipeline_fqn_folder(const char *project, const char *rel_dir);

/*
 * An import written relative to the importing file (`./foo`, `../bar`, `.foo`, or a
 * bare local name such as `foo.h`), resolved against that file's path: a normalised
 * repository-relative path without extension, or NULL when the import is not
 * relative. Declared here because the import resolver calls it and C would otherwise
 * assume it returns an int, silently truncating the pointer on 64-bit targets.
 */
char *pdxe_pipeline_resolve_relative_import(const char *source_rel, const char *module_path);

/*
 * The language a file name implies, or the language count when none. The reference
 * declares it in its discovery header, which its pipeline header includes and the
 * registry relies on; discovery is not vendored, so the one declaration is here.
 */
PDXELanguage pdxe_language_for_filename(const char *filename);

/* --- the symbol registry -------------------------------------------------- */
/*
 * The registry the resolution sources build and query. Its declarations are
 * reproduced from the reference's pipeline header for the same reason as the name
 * computation above: the implementation is vendored, only the declarations were
 * in a header carrying a subsystem this project does not have.
 */

/* ── Function Registry ──────────────────────────────────────────── */

typedef struct pdxe_registry pdxe_registry_t;

typedef struct {
    const char *qualified_name; /* borrowed from registry */
    const char *strategy;       /* resolution strategy name */
    double confidence;          /* 0.0–1.0 */
    int candidate_count;
} pdxe_resolution_t;

/* Create/free a function registry. */
pdxe_registry_t *pdxe_registry_new(void);
void pdxe_registry_free(pdxe_registry_t *r);

/* Register a function/method/class. All strings are copied. */
void pdxe_registry_add(pdxe_registry_t *r, const char *name, const char *qualified_name,
                      const char *label);

/* Resolve a callee name using prioritized strategies.
 * import_map: NULL-terminated array of {local_name, resolved_qn} pairs, or NULL.
 * Returns result with qualified_name="" if unresolved.
 * Never returns a data relation (Table/View): relations are lineage-only
 * registry members and common table names (users, orders, config) collide with
 * code identifiers in every language, so the default resolve vetoes them
 * centrally instead of relying on per-consumer label checks. */
pdxe_resolution_t pdxe_registry_resolve(const pdxe_registry_t *r, const char *callee_name,
                                      const char *module_qn, const char **import_map_keys,
                                      const char **import_map_vals, int import_map_count);

/* Relation-permitting resolve for SQL FROM/JOIN lineage usages ONLY — the one
 * consumer allowed to bind Table/View targets. Uncached (the per-file resolve
 * cache stores the default variant's relation-vetoed answers). */
pdxe_resolution_t pdxe_registry_resolve_lineage(const pdxe_registry_t *r, const char *callee_name,
                                              const char *module_qn, const char **import_map_keys,
                                              const char **import_map_vals, int import_map_count);

/* Per-file memoization cache for is_import_reachable. Thread-local —
 * each resolve worker owns its own cache. Call _begin at the start
 * of resolve_file_calls (or any per-file resolve loop) and _end at
 * the end. The cache MUST be invalidated between files because
 * is_import_reachable's truth depends on the file's import_vals. */
void pdxe_registry_reach_cache_begin(int estimated_capacity);
void pdxe_registry_reach_cache_end(void);

/* Per-file import-map prefix → module-QN hash. Turns the linear
 * strcmp scan inside resolve_import_map into O(1). Keys/values are
 * BORROWED — caller must keep the import_map arrays alive for the
 * cache lifetime. Invalidate between files via _end. */
void pdxe_registry_import_map_cache_begin(const char **keys, const char **vals, int count);
void pdxe_registry_import_map_cache_end(void);

/* Per-file full-result cache for pdxe_registry_resolve. The same
 * callee_name appears in many call sites within a file; module_qn
 * is constant per file so each name resolves identically. First
 * lookup does the full strategy chain; repeats are O(1) hash hits.
 * This eliminates ~75% of the resolve-chain work on K8s where the
 * same names ("Get", "Add", "New", etc) appear hundreds of times. */
void pdxe_registry_resolve_cache_begin(int estimated_capacity);
void pdxe_registry_resolve_cache_end(void);

/* Check if a qualified name exists in the registry. */
bool pdxe_registry_exists(const pdxe_registry_t *r, const char *qn);

/* True if `name` is one of the curated Perl core builtins (perlfunc). Used by
 * the call-resolution passes to suppress generic-resolver CALLS edges from Perl
 * builtin invocations (push/shift/keys/...) to project subs that merely share
 * the name. Perl-scoped: callers gate on the file language. */
bool pdxe_perl_is_builtin(const char *name);

/* Decide whether a resolved Perl call edge is generic-resolver noise to drop
 * (#476): true only for Perl, only for a builtin/method call, and only when the
 * match used a weak short-name strategy — high-confidence same_module/import_map
 * matches are kept. Pure; unit-tested in test_registry.c. */
bool pdxe_perl_suppress_generic_match(bool is_perl, bool is_method, const char *callee_name,
                                     const char *strategy);

/* Decide whether a resolved member-call edge is weak-strategy noise to drop
 * (#592/#606/#1276): true only when the CALLER's per-language gate says the
 * guard applies (`enabled`), only for a member call with an unresolved receiver
 * (is_method), and only when the match used a weak short-name strategy
 * (suffix_match / unique_name / field_type_hint / fuzzy).
 * Explicit drop-list keeps every lsp_* / import / same-module / qualified match.
 * The language set lives at the call sites (pass_calls.c / pass_parallel.c) and
 * must be identical in both, or the sequential and parallel resolvers diverge.
 * Pure; unit-tested in test_registry.c. */
bool pdxe_suppress_weak_member_match(bool enabled, bool is_method, const char *strategy);

/* True if `name` is a method of a Python builtin type (str/bytes/list/dict/set/
 * file) or a builtin function seen as an attribute call. A language fact, kept
 * as a sorted table like the Perl builtins. */
bool pdxe_python_is_builtin_member(const char *name);

/* The member guard's exemption: a Python member call whose receiver is an
 * attribute chain rooted at self/cls (an object the class owns), whose callee has
 * exactly one project definition (strategy unique_name) and is not a builtin
 * type's own method keeps its edge. Combine at the call sites as
 * `suppress && !exempt`; both pass_calls.c and pass_parallel.c must do the same.
 * Pure; unit-tested in test_registry.c. */
bool pdxe_weak_member_unique_name_exempt(bool is_python, bool receiver_is_self_attribute,
                                        const char *callee_name, const char *strategy);

/* Bare-call counterpart of the guard above. True when a resolved BARE call edge
 * binds a callee that is shadowed by an enclosing parameter, and the match came
 * from a weak short-name strategy — so the edge is fabricated by construction
 * (`def f(run): run()` must not bind an unrelated `SatoriLive.run`). Shares the
 * member guard's drop-list, so lsp_* / import / same-module matches are kept.
 * Deliberately keyed on the SCOPE FACT, not on the callee's spelling. The
 * language set lives at the call sites (pass_calls.c / pass_parallel.c) and must
 * be identical in both, or the sequential and parallel resolvers diverge.
 * Pure; unit-tested in test_registry.c. */
bool pdxe_suppress_weak_local_binding_call(bool enabled, bool callee_is_locally_bound,
                                          const char *strategy);

/* #725: drop a suffix_match CALLS edge when the caller language and the
 * target file's language disagree. unique_name (candidates == 1) is #1572
 * and is left alone; same_module / import_map / lsp_* are kept. JS/TS/TSX
 * are one family so a .ts helper calling a .tsx function is not dropped.
 * Pure; unit-tested in test_registry.c. */
bool pdxe_suppress_cross_language_suffix_match(PDXELanguage caller_lang, const char *target_file_path,
                                              const char *strategy);

/* #1928: USAGE/WRITES/READS analog of the CALLS guard above. Reference edges
 * resolved by the short-name registry carry no import-closure evidence, so a
 * cross-language binding is a bare-name collision for EVERY strategy — drop
 * it whenever the caller's language and the target file's language disagree
 * (JS/TS family members and the C/C++ header family excepted). Pure;
 * unit-tested in test_registry.c. */
bool pdxe_suppress_cross_language_ref(PDXELanguage caller_lang, const char *target_file_path);

/* #1942: a bare (dot-less) Go reference can never denote a struct field —
 * field access is always a selector expression, and selector references
 * resolve on the LSP path. Drops a READS/WRITES/USAGE bind whose target is a
 * Field when the reference text carries no '.'. Go only: other OO languages
 * legitimately reference their own members bare inside method bodies. Pure;
 * unit-tested in test_registry.c. */
bool pdxe_go_suppress_bare_field_ref(bool is_go, bool is_member_access, const char *target_label);

/* Get the label of a qualified name, or NULL if not found. */
const char *pdxe_registry_label_of(const pdxe_registry_t *r, const char *qn);

/* Find all QNs with a given simple name. Sets *out and *count.
 * Caller does NOT free the array (owned by registry). */
int pdxe_registry_find_by_name(const pdxe_registry_t *r, const char *name, const char ***out,
                              int *count);

/* Return total number of entries. */
int pdxe_registry_size(const pdxe_registry_t *r);

/* Find all qualified names ending with ".suffix".
 * Sets *out to heap-allocated array of borrowed string pointers.
 * Caller must free(*out) but NOT the individual strings.
 * Returns count of matches. */
int pdxe_registry_find_ending_with(const pdxe_registry_t *r, const char *suffix, const char ***out);

/* Check if candidate QN's module prefix is reachable via any import value. */
bool pdxe_registry_is_import_reachable(const char *candidate_qn, const char **import_vals,
                                      int import_count);

/* Fuzzy resolve: match callee by bare function name (last segment after dots).
 * Returns result with ok=true if found, ok=false if not.
 * Lower confidence than Resolve (0.40 single, 0.30 multiple). */
typedef struct {
    pdxe_resolution_t result;
    bool ok;
} pdxe_fuzzy_result_t;

pdxe_fuzzy_result_t pdxe_registry_fuzzy_resolve(const pdxe_registry_t *r, const char *callee_name,
                                              const char *module_qn, const char **import_map_keys,
                                              const char **import_map_vals, int import_map_count);

const char *pdxe_confidence_band(double score);


#ifdef __cplusplus
}
#endif

#endif /* PDXE_PIPELINE_H */
