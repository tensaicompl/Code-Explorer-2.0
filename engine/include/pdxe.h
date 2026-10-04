/*
 * The engine's public interface.
 *
 * Everything the rest of this project uses from the engine passes through here.
 * Nothing above this header includes an engine source or knows an engine type: the
 * implementation translates between the engine's own model and this one, so the
 * vendored code can be refreshed without the callers noticing.
 *
 * Conventions that hold throughout:
 *
 *   - Offsets are byte offsets into the file's UTF-8 source. Lines and columns are
 *     1-based and are carried alongside, never derived by a caller.
 *   - Every string is UTF-8 and NUL-terminated, and is owned by the result that
 *     carries it until that result is freed. A caller that keeps a string past
 *     then must copy it.
 *   - Every function returns 0 on success and a negative code below on failure.
 *     Nothing here writes to standard output or standard error: this is a library
 *     inside a program that speaks a protocol on those streams. The engine's log is
 *     discarded whatever level the environment asks for. The one exception is the
 *     engine's own debugging switches (environment variables beginning
 *     PDX_ENGINE_LSP_ and PDX_ENGINE_MEM_), which print by design when set; nothing
 *     in this project sets them.
 */

/*
 * The guard is deliberately not PDXE_H. The vendoring rename gives the engine's own
 * core header that guard, and two headers sharing one means whichever is included
 * first silently empties the other: the engine's declarations simply vanish from any
 * file that included this one first. A test asserts the guards stay distinct.
 */
#ifndef PDXE_PUBLIC_INTERFACE_H
#define PDXE_PUBLIC_INTERFACE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* --- error codes --------------------------------------------------------- */

#define PDXE_OK 0
#define PDXE_E_NOMEM (-1)     /* allocation failed */
#define PDXE_E_LANG (-2)      /* language id not known to this build */
#define PDXE_E_TOOLARGE (-3)  /* input exceeds what the engine will parse */
#define PDXE_E_INVALID (-4)   /* a required argument was NULL or inconsistent */
#define PDXE_E_INTERNAL (-5)  /* the engine failed in a way the caller cannot act on */

/* --- lifecycle ----------------------------------------------------------- */

/*
 * One context per thread. The engine keeps parser state in thread-local storage,
 * so a context may not be shared between threads; parallelism belongs to the
 * caller, one context each.
 */
typedef struct pdxe_ctx pdxe_ctx;

int pdxe_init(pdxe_ctx **out);
void pdxe_shutdown(pdxe_ctx *ctx);

/* The engine's version, for cache keys and for reporting. Never NULL. */
const char *pdxe_version(void);

/*
 * Maps a language identifier to the engine's internal language number.
 * Returns a positive number for a known language and 0 for an unknown one, which
 * is not an error: a file in an unknown language is discovered and counted, not
 * parsed.
 */
int pdxe_language_id(const char *lang_id);

/* --- extraction ---------------------------------------------------------- */

/*
 * A position in the file. All zero means the position is not known. That is the case
 * for anything the engine found in C-family source after macro expansion, which is
 * positioned in text the caller never sees; it is reported without a position rather
 * than with a wrong one. It is also the case for every type reference, field access,
 * channel, configuration read and throw, whose positions the engine does not record:
 * those are placed by their scope alone.
 */
typedef struct {
    uint32_t start_byte, end_byte;
    uint32_t start_line, start_col;
    uint32_t end_line, end_col;
} pdxe_span;

/* Visibility of a definition, as the source declares it. */
enum pdxe_visibility {
    PDXE_VIS_UNKNOWN = 0,
    PDXE_VIS_PUBLIC = 1,
    PDXE_VIS_NON_PUBLIC = 2
};

/* The index that means "none": a definition at file scope, a call outside any. */
#define PDXE_NO_PARENT UINT32_MAX

/*
 * A definition: anything the graph gives a node of its own.
 *
 * `kind` is one of the normalised strings of the kind mapping, never an engine
 * kind: `class`, `interface`, `enum`, `struct`, `trait`, `type_alias`, `function`,
 * `method`, `constructor`, `field`, `variable`, `macro`, `module`. An engine kind
 * with no mapping becomes `variable`, and `engine_kind` keeps what it was so the
 * information is not lost.
 *
 * `parent_index` indexes the definition array of the same result, or
 * PDXE_NO_PARENT when the definition is at file scope.
 *
 * `base_classes` are the classes, interfaces and traits the definition names as its
 * bases, exactly as the engine recorded them: in its order, spelling and case,
 * unresolved. `n_base_classes` counts them; with none, `base_classes` is NULL.
 * Appended by a specification change; see docs/plan/ISSUES.md, issue 40.
 */
typedef struct {
    const char *name;
    const char *qualified_name;
    const char *kind;
    const char *engine_kind;
    const char *signature;
    const char *doc;
    pdxe_span span;
    pdxe_span body_span;
    uint32_t parent_index;
    uint8_t visibility;
    uint8_t is_test;
    uint8_t is_entry_point;
    uint32_t cyclomatic;
    uint32_t cognitive;
    uint32_t loop_depth;
    const char **base_classes;
    uint32_t n_base_classes;
} pdxe_definition;

/*
 * What the extractor saw of a name in its surroundings, for resolving it by name when
 * typed resolution has no answer. Each bit is one fact; none is a summary of others.
 *
 * On references and usages:
 *
 *   PDXE_LEX_EXPLICIT_REFERENCE  the source spells a callable reference as such
 *                                (a method reference, an address-of), rather than
 *                                passing a plain name a callable might be behind
 *   PDXE_LEX_MEMBER_ACCESS       the name is the member half of a selector; the
 *                                receiver before it has been removed
 *   PDXE_LEX_BLOCKED             a binding in the same code means the name is not the
 *                                module-level definition of that name; resolving it
 *                                by name must not reach a callable
 *   PDXE_LEX_BLOCKED_LOCALLY     that binding is in a local scope; resolving it by
 *                                name must not reach anything
 *
 * On calls:
 *
 *   PDXE_LEX_UNRESOLVED_MEMBER   a member call whose receiver the extractor could not
 *                                tie to anything: in Python, not self, cls or super()
 *                                and not rooted in an imported name; in JavaScript
 *                                and TypeScript, not this or super; in Perl, any
 *                                method call. Resolving it by a short name alone
 *                                fabricates edges, so the reference does not
 *   PDXE_LEX_LOCALLY_BOUND       a bare call whose callee name is a parameter of an
 *                                enclosing function, so it cannot be the module-level
 *                                definition of that name (Python)
 *   PDXE_LEX_SELF_ROOTED         a member call whose receiver is an attribute chain
 *                                rooted at self or cls but is not self or cls itself,
 *                                an object the class owns (Python)
 *
 * Appended by a specification change; see docs/plan/ISSUES.md, issue 17.
 */
enum pdxe_lexical_fact {
    PDXE_LEX_EXPLICIT_REFERENCE = 1,
    PDXE_LEX_MEMBER_ACCESS = 2,
    PDXE_LEX_BLOCKED = 4,
    PDXE_LEX_BLOCKED_LOCALLY = 8,
    PDXE_LEX_UNRESOLVED_MEMBER = 16,
    PDXE_LEX_LOCALLY_BOUND = 32,
    PDXE_LEX_SELF_ROOTED = 64
};

/*
 * A call site.
 *
 * `caller_index` indexes the definition array of the same result, or is
 * PDXE_NO_PARENT for a call at file scope.
 *
 * `is_reference` marks a callable passed as a value rather than invoked: a name the
 * engine saw used as a value where a callable could be meant, such as a function
 * handed to another as an argument. It is a call reference only if typed resolution
 * says so; without a typed resolution it is an ordinary use of a name, and a caller
 * must not turn it into a call. Such a site is reported here and not again among the
 * usages, so that no site is counted twice.
 *
 * `typed_only` marks a site that exists only as a question for typed resolution: an
 * operator the language turns into a method call, a protocol method it invokes
 * implicitly, a call the engine inferred rather than read. It is a call only if a
 * typed resolution names its target. A caller must never resolve it by name, since
 * the name it carries is not text the source contains. Appended by a specification
 * change; see docs/plan/ISSUES.md, issue 17.
 *
 * Calls come first in the order the engine found them, then references in the
 * order their names appear. That order is the index a typed resolution refers to.
 */
typedef struct {
    const char *callee_text;
    const char *receiver_text;
    uint32_t caller_index;
    pdxe_span span;
    uint8_t is_reference;
    uint8_t typed_only;
    uint16_t lexical;
} pdxe_call;

typedef struct {
    const char *module_text;
    const char *imported_name;
    const char *alias;
    pdxe_span span;
} pdxe_import;

/* A use of a name as a value. `lexical` as for calls. */
typedef struct {
    const char *name;
    uint32_t scope_index;
    pdxe_span span;
    uint16_t lexical;
} pdxe_usage;

typedef struct {
    const char *type_text;
    uint32_t scope_index;
    pdxe_span span;
} pdxe_type_ref;

typedef struct {
    const char *field_text;
    uint32_t scope_index;
    uint8_t is_write;
    pdxe_span span;
} pdxe_rw;

/* Publish or subscribe participation. `is_listen` is 0 for emit, 1 for listen. */
typedef struct {
    const char *channel_text;
    uint8_t is_listen;
    pdxe_span span;
} pdxe_channel;

/* An environment or configuration key the file reads. */
typedef struct {
    const char *key;
    pdxe_span span;
} pdxe_env_access;

typedef struct {
    const char *message;
    pdxe_span span;
} pdxe_diag;

/*
 * An exception a definition raises: a throw or raise statement. `exception_text` is
 * the exception's type as the source spells it; finding the definition it names is
 * resolution's work, not extraction's. `scope_index` as for usages.
 *
 * The engine also reads exceptions a method declares, but only where the language's
 * grammar names the declaration as a field, which none of the matrix's grammars does:
 * Java's `throws` clause is not reported. Appended by a specification change; see
 * docs/plan/ISSUES.md, issue 25.
 */
typedef struct {
    const char *exception_text;
    uint32_t scope_index;
    pdxe_span span;
} pdxe_throw;

/* `status`: 0 parsed, 1 partial (the tree carries errors), 2 failed. */
enum pdxe_file_status {
    PDXE_FILE_PARSED = 0,
    PDXE_FILE_PARTIAL = 1,
    PDXE_FILE_FAILED = 2
};

typedef struct {
    int status;
    pdxe_definition *defs;
    uint32_t n_defs;
    pdxe_call *calls;
    uint32_t n_calls;
    pdxe_import *imports;
    uint32_t n_imports;
    pdxe_usage *usages;
    uint32_t n_usages;
    pdxe_type_ref *types;
    uint32_t n_types;
    pdxe_rw *rws;
    uint32_t n_rws;
    pdxe_channel *channels;
    uint32_t n_channels;
    pdxe_env_access *envs;
    uint32_t n_envs;
    pdxe_diag *diags;
    uint32_t n_diags;
    /* Appended by a specification change; see docs/plan/ISSUES.md, issue 25. */
    pdxe_throw *throws;
    uint32_t n_throws;
    /*
     * 1 when the extractor stopped walking the file at its node budget: what it found
     * up to that point is reported, the rest of the file is not, and typed resolution
     * skips the file. The budget is off unless the environment sets one. Appended by a
     * specification change; see docs/plan/ISSUES.md, issue 25.
     */
    uint8_t truncated;
    /*
     * Work extraction lost on the file: allocations that failed and work budgets that
     * ran out while it was extracted (api/lost_work.h). 0 when nothing was lost. It is
     * the count the file's resolution surface carries, which a project resolving the
     * file counts as its own loss. A result rebuilt from a cache has 0. Appended by a
     * specification change; see docs/plan/ISSUES.md, issue 26.
     */
    uint32_t extraction_lost;
    /*
     * The package or namespace the file declares, exactly as written in its first
     * declaration, or NULL when it declares none. Of the language matrix's languages,
     * the engine reads it for Java and Kotlin (`package`), C# (`namespace`, block or
     * file-scoped) and PHP (`namespace`), and it is NULL for every other, whatever the
     * source says. A result rebuilt from a cache has none. Appended by a specification
     * change; see docs/plan/ISSUES.md, issue 41.
     */
    const char *declared_namespace;
} pdxe_file_result;

int pdxe_extract_file(pdxe_ctx *ctx, int lang, const char *rel_path, const uint8_t *bytes,
                      size_t len, pdxe_file_result **out);

void pdxe_result_free(pdxe_ctx *ctx, pdxe_file_result *r);

/*
 * Rebuilds a result from parts held in a cache, so that a file whose content has
 * not changed is never extracted again. The arrays are copied, strings included; the
 * caller keeps its own. The rebuilt result describes the file; to resolve the file
 * it is added to a project together with its surface (pdxe_surface_import). Its
 * channel, configuration, diagnostic and throw arrays are empty, its status is parsed,
 * it is not truncated and it reports no lost work: those parts are the cache's to keep,
 * and resolution reads what it needs of them from the surface. A definition's base
 * classes are copied with it, array and strings; a definition with bases and no array,
 * or with a NULL base, is refused. It declares no namespace.
 */
int pdxe_result_build(pdxe_ctx *ctx, const pdxe_definition *defs, uint32_t n_defs,
                      const pdxe_call *calls, uint32_t n_calls, const pdxe_import *imports,
                      uint32_t n_imports, const pdxe_usage *usages, uint32_t n_usages,
                      const pdxe_type_ref *types, uint32_t n_types, const pdxe_rw *rws,
                      uint32_t n_rws, pdxe_file_result **out);

/* --- typed resolution across files --------------------------------------- */

/*
 * A set of files resolved together: added one by one, with the repository's
 * metadata, run once, and read for its resolutions until it ends.
 */
typedef struct pdxe_project pdxe_project;

int pdxe_resolve_project_begin(pdxe_ctx *ctx, pdxe_project **out);

/*
 * Adds one file: its path, language and source, and the result the caller has for it.
 * Resolution parses the source again for the tree it walks, which is parse-only cost;
 * it never extracts a file the caller supplied a result for.
 *
 *   - A result pdxe_extract_file returned is resolved through directly. Resolution
 *     writes into it and the project puts it back as it was when the project ends;
 *     until then the result must stay alive and may belong to no other project.
 *   - A result pdxe_result_build returned, from a cache, is resolved through the
 *     file's surface, which must be imported before the run.
 *   - With no result, the file is extracted when the run starts, unless its surface
 *     was imported, which is then used instead.
 *
 * Each path may be added once. The files' answers do not depend on the order they
 * are added in; adding them in path order keeps even the order of the results fixed.
 */
int pdxe_resolve_project_add_file(pdxe_project *p, int lang, const char *rel_path,
                                  const uint8_t *bytes, size_t len, const pdxe_file_result *r);

/* --- resolution metadata ------------------------------------------------- */
/*
 * What the repository declares about its own module structure, supplied by the
 * caller. The engine resolves imports; it does not discover packages. Reading
 * manifests and build configuration is the caller's job, and what arrives here is
 * already resolved: plain mappings, with repository-relative paths and no need for
 * the engine to touch the filesystem. See docs/plan/ISSUES.md, issue 15.
 *
 * All of it is optional. With none, imports that need it resolve as if the
 * repository declared nothing, which is the correct answer for a repository that
 * does not.
 */

/*
 * One package or module the repository declares. An import naming `import_prefix`,
 * or naming anything beneath it, resolves into the module whose entry is
 * `entry_path`:
 *
 *   - a Go module:        "example.com/acme"  ->  ""  (the module root)
 *   - an npm package:     "@acme/lib"         ->  "packages/lib/src/index.ts"
 *   - a declared package: "com.acme.core"     ->  "core/src/main/java/com/acme/core"
 *
 * Prefixes are matched exactly first, then by the longest leading run of path
 * segments, under whichever separator the import uses.
 */
typedef struct {
    const char *import_prefix;
    const char *entry_path;
} pdxe_package_entry;

/*
 * One alias from a build configuration's path mappings, already split at its
 * wildcard. An alias mapping "@/" followed by a wildcard to "src/" followed by a
 * wildcard becomes alias prefix "@/", target prefix "src/", both suffixes empty,
 * has_wildcard 1. An alias without a wildcard matches exactly.
 */
typedef struct {
    const char *alias_prefix;
    const char *alias_suffix;
    const char *target_prefix;
    const char *target_suffix;
    uint8_t has_wildcard;
} pdxe_path_alias;

/*
 * The aliases one configuration applies to the files beneath its directory. A file
 * takes the aliases of its nearest enclosing scope. `dir_prefix` is repository-
 * relative, "" for the root; `base_url` may be NULL.
 */
typedef struct {
    const char *dir_prefix;
    const char *base_url;
    const pdxe_path_alias *aliases;
    uint32_t n_aliases;
} pdxe_alias_scope;

/*
 * A dependency the repository's root crate manifest declares: its name, and for a
 * dependency that lives in the repository, its repository-relative path, else NULL.
 */
typedef struct {
    const char *name;
    const char *path;
} pdxe_crate_dependency;

/*
 * What the repository's root crate manifest declares, already parsed: the package's
 * name, whether it is a workspace root, its dependencies, and its workspace members
 * as the paths the manifest lists them by. A member is known to the resolver by the
 * last segment of its path, which the engine derives itself, as it does when it reads
 * a manifest.
 */
typedef struct {
    const char *package_name;
    uint8_t is_workspace_root;
    const pdxe_crate_dependency *dependencies;
    uint32_t n_dependencies;
    const char *const *member_paths;
    uint32_t n_members;
} pdxe_crate_manifest;

typedef struct {
    const pdxe_package_entry *packages;
    uint32_t n_packages;
    const pdxe_alias_scope *alias_scopes;
    uint32_t n_alias_scopes;
    /* The root crate manifest, or NULL when the repository has none. */
    const pdxe_crate_manifest *crate_manifest;
} pdxe_resolution_metadata;

/*
 * Supplies the repository's resolution metadata. Call before running, at most once.
 * Everything is copied, so the caller may free its own structures on return.
 *
 * The order packages and aliases arrive in does not matter: the engine orders them
 * itself, most specific first, and breaks every tie by content, so the same metadata
 * always resolves the same way regardless of how the caller happened to assemble it.
 * A crate manifest is taken in the order the manifest lists things, as the engine's
 * own reader would take it, and like that reader it keeps the first 256 dependencies
 * and the first 64 members.
 */
int pdxe_resolve_project_set_metadata(pdxe_project *p, const pdxe_resolution_metadata *m);

/*
 * Resolves every file added, once per project. Fails with PDXE_E_INVALID when a file
 * added with a cache-built result has no surface, when a surface belongs to no file
 * or to a file of another language, and when run a second time. A run that returns 0
 * completed; whether it did all its work is pdxe_resolve_project_health's to say.
 */
int pdxe_resolve_project_run(pdxe_project *p);

/*
 * How a completed run went.
 *
 * Clean: typed resolution did all the work it should have. That includes finding no
 * answer at all, for files that have none to find.
 *
 * Degraded: some of the work was skipped or lost. Every answer reported is sound, but
 * a site without one may be a site whose question was never asked, so the absence of
 * an answer is not evidence of anything.
 *
 * A run that could not complete is neither: pdxe_resolve_project_run returned an
 * error instead.
 */
enum pdxe_run_status {
    PDXE_RUN_CLEAN = 0,
    PDXE_RUN_DEGRADED = 1
};

/*
 * What a completed run did with each file, and what it lost.
 *
 * Every file is counted once, under the first of these that applies:
 *
 *   files_untyped             its language has no typed resolution, by design
 *   files_empty               it has no source, so there is nothing to resolve
 *   files_not_reached         typed resolution stopped before it reached the file
 *   files_source_unavailable  typed resolution could not obtain its source
 *   files_over_budget         extraction stopped at its node budget, and typed
 *                             resolution skips such a file
 *   files_resolved            typed resolution ran on it
 *
 * so the six add up to `files`. `pass_failures` counts work typed resolution lost
 * without skipping a file: every allocation that failed, wherever in the run it failed,
 * and every per-file work budget a resolver ran out of, each of which leaves answers
 * unfound; the same, counted during the extraction of each file, since part of typed
 * resolution happens there; the project's definitions not collected; and the pass's
 * account of its files disagreeing with the interface's. An allocation that fails
 * where it only costs time, or where the complete answer is reached another way, is
 * not counted. Each of the counts from files_not_reached to pass_failures is work
 * lost: any of them above zero makes the run degraded. All of them are counted from
 * the run's own state, never from its log.
 *
 * Appended by a specification change; see docs/plan/ISSUES.md, issue 21.
 */
typedef struct {
    int status;
    uint32_t files;
    uint32_t files_resolved;
    uint32_t files_untyped;
    uint32_t files_empty;
    uint32_t files_over_budget;
    uint32_t files_source_unavailable;
    uint32_t files_not_reached;
    uint32_t pass_failures;
} pdxe_run_health;

/* The health of a run that returned 0. Fails with PDXE_E_INVALID before then. */
int pdxe_resolve_project_health(const pdxe_project *p, pdxe_run_health *out);

/*
 * One resolved call site.
 *
 * `strategy` is exhaustive and the caller maps each to a confidence band:
 * `import_map`, `import_map_suffix`, `same_module`, `qualified_suffix`,
 * `unique_name`, `suffix_match`, `fuzzy_single`, `fuzzy_multi`, `service_pattern`,
 * `lsp_typed`, `unknown`. Only `lsp_typed` with a single candidate above the
 * threshold is evidence on its own; the rest are hints that seed a search without
 * restricting it.
 *
 * `call_index` indexes the calls of the file's extraction result. Typed resolution
 * can also find call sites extraction did not report, such as an operator on a type
 * that defines it; those are numbered on from the end of that array, are always
 * `typed_only`, and are described only by `site`.
 *
 * A site is resolved once or not at all, and only when the typed pass gives exactly
 * one answer for it, joined to the site the way the reference joins them.
 *
 * For a call, an answer is reported only when its target is something the project
 * knows: a definition in one of its files, or a built-in the engine supplies for the
 * language. When the typed pass names anything else, the reference treats the call
 * as having no typed answer, and so does this: nothing is reported and the caller
 * resolves the call by other means. For a reference, an answer is reported whatever
 * it names, because the reference lets a typed answer settle a reference even when
 * its target is nowhere in the project.
 *
 * `target_rel_path` is NULL when the target is in no file of the project: a built-in,
 * or, for a reference, something outside it. The site is settled all the same, and
 * must not be resolved by name instead.
 */
typedef struct {
    const char *rel_path;
    uint32_t call_index;
    const char *target_qualified_name;
    const char *target_rel_path;
    double score;
    const char *strategy;
    uint32_t candidates;
    /*
     * The engine's own name for how it resolved the call, verbatim, for calibrating
     * the bands against what actually produced them. Several engine strategies share
     * one normalised `strategy`; this keeps which one it was. Never used to derive a
     * band. Appended by a specification change; see docs/plan/ISSUES.md, issue 16.
     */
    const char *engine_strategy;
    /*
     * The call site itself, as extraction describes it at `call_index`, or as typed
     * resolution found it when the index is past the end of extraction's calls.
     * Appended by a specification change; see docs/plan/ISSUES.md, issue 17.
     */
    pdxe_call site;
} pdxe_resolution;

/*
 * The resolutions, for each file in the order added and within a file in call order.
 * The array and every string it points to belong to the project and last until
 * pdxe_resolve_project_end; a site's strings may be the caller's own result's, which
 * is one more reason that result must outlive the project.
 */
int pdxe_resolve_project_results(pdxe_project *p, const pdxe_resolution **out, uint32_t *n);

/* Frees the project, first putting back every caller's result it resolved through. */
void pdxe_resolve_project_end(pdxe_project *p);

/* --- the cross-file surface ---------------------------------------------- */

/*
 * A file's surface: everything typed resolution reads from the file's extraction, as
 * bytes the extraction cache keeps beside the file's other parts. Opaque to the caller.
 * Its strings are the engine's byte for byte, so it is JSON in shape but not strict
 * JSON when the file's text is not valid UTF-8.
 *
 * Typed resolution runs over the whole repository on every build, so its answers
 * never depend on an earlier build. A file whose content has not changed is not
 * extracted again for it: its surface is imported instead and resolution gives the
 * same answers it would give from a fresh extraction. The surface therefore holds the
 * file's own facts only, never anything computed from other files, and it names the
 * path and language it was taken for. Equal extractions give equal bytes.
 *
 * Export takes a result pdxe_extract_file returned, and only while no project is
 * resolving through it; the bytes are the caller's, freed with pdxe_surface_free.
 * Import copies what it needs, so the caller may free the bytes on return. It refuses
 * bytes this version did not write, and a second surface for one path. Every surface
 * imported must belong to a file added to the project, in the same language, by the
 * time the project runs.
 */
int pdxe_surface_export(const pdxe_file_result *r, const char *rel_path, uint8_t **out,
                        size_t *out_len);
int pdxe_surface_import(pdxe_project *p, const uint8_t *bytes, size_t len);
void pdxe_surface_free(uint8_t *bytes);

#ifdef __cplusplus
}
#endif

#endif /* PDXE_PUBLIC_INTERFACE_H */
