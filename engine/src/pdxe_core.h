#ifndef PDXE_H
#define PDXE_H

#include <stdint.h>
#include <stdbool.h>
#include "arena.h"
#include "tree_sitter/api.h"

/* Field lookups by NAME resolve the name with a linear strncmp scan over the
 * grammar's field table on every call -- 951 M strncmp calls on the Go corpus
 * (waste sanitizer, 2026-09-17) across ~1,200 call sites. Tree-sitter's own
 * implementation is exactly "field id for name, then child by field id"; this
 * caches the first half per thread (language, name) and keeps the second.
 * Every extractor includes this header, so every call site gets it. */
TSNode pdxe_ts_child_by_field_name(TSNode node, const char *name, uint32_t name_length);
/* Variadic: call sites spell the name and its length as ONE macro argument
 * (TS_FIELD("body") expands to "body", 4), which must expand before the call. */
#define ts_node_child_by_field_name(...) pdxe_ts_child_by_field_name(__VA_ARGS__)

// Language enum mirrors lang.Language in Go.
// Order must match lang_specs.c tables.
typedef enum {
    PDXE_LANG_GO = 0,
    PDXE_LANG_PYTHON,
    PDXE_LANG_JAVASCRIPT,
    PDXE_LANG_TYPESCRIPT,
    PDXE_LANG_TSX,
    PDXE_LANG_RUST,
    PDXE_LANG_JAVA,
    PDXE_LANG_CPP,
    PDXE_LANG_CSHARP,
    PDXE_LANG_PHP,
    PDXE_LANG_LUA,
    PDXE_LANG_SCALA,
    PDXE_LANG_KOTLIN,
    PDXE_LANG_RUBY,
    PDXE_LANG_C,
    PDXE_LANG_BASH,
    PDXE_LANG_ZIG,
    PDXE_LANG_ELIXIR,
    PDXE_LANG_HASKELL,
    PDXE_LANG_OCAML,
    PDXE_LANG_OBJC,
    PDXE_LANG_SWIFT,
    PDXE_LANG_DART,
    PDXE_LANG_PERL,
    PDXE_LANG_GROOVY,
    PDXE_LANG_ERLANG,
    PDXE_LANG_R,
    PDXE_LANG_HTML,
    PDXE_LANG_CSS,
    PDXE_LANG_SCSS,
    PDXE_LANG_YAML,
    PDXE_LANG_TOML,
    PDXE_LANG_HCL,
    PDXE_LANG_SQL,
    PDXE_LANG_DOCKERFILE,
    // New languages (v0.5 expansion)
    PDXE_LANG_CLOJURE,
    PDXE_LANG_FSHARP,
    PDXE_LANG_JULIA,
    PDXE_LANG_VIMSCRIPT,
    PDXE_LANG_NIX,
    PDXE_LANG_COMMONLISP,
    PDXE_LANG_ELM,
    PDXE_LANG_FORTRAN,
    PDXE_LANG_CUDA,
    PDXE_LANG_COBOL,
    PDXE_LANG_VERILOG,
    PDXE_LANG_EMACSLISP,
    PDXE_LANG_JSON,
    PDXE_LANG_XML,
    PDXE_LANG_MARKDOWN,
    PDXE_LANG_MAKEFILE,
    PDXE_LANG_CMAKE,
    PDXE_LANG_PROTOBUF,
    PDXE_LANG_GRAPHQL,
    PDXE_LANG_VUE,
    PDXE_LANG_SVELTE,
    PDXE_LANG_MESON,
    PDXE_LANG_GLSL,
    PDXE_LANG_INI,
    // Scientific/math languages
    PDXE_LANG_MATLAB,
    PDXE_LANG_LEAN,
    PDXE_LANG_FORM,
    PDXE_LANG_MAGMA,
    PDXE_LANG_WOLFRAM,
    PDXE_LANG_SOLIDITY,
    PDXE_LANG_TYPST,
    PDXE_LANG_GDSCRIPT,
    PDXE_LANG_GLEAM,
    PDXE_LANG_POWERSHELL,
    PDXE_LANG_PASCAL,
    PDXE_LANG_DLANG,
    PDXE_LANG_NIM,
    PDXE_LANG_SCHEME,
    PDXE_LANG_FENNEL,
    PDXE_LANG_FISH,
    PDXE_LANG_AWK,
    PDXE_LANG_ZSH,
    PDXE_LANG_TCL,
    PDXE_LANG_ADA,
    PDXE_LANG_AGDA,
    PDXE_LANG_RACKET,
    PDXE_LANG_ODIN,
    PDXE_LANG_RESCRIPT,
    PDXE_LANG_PURESCRIPT,
    PDXE_LANG_NICKEL,
    PDXE_LANG_CRYSTAL,
    PDXE_LANG_TEAL,
    PDXE_LANG_HARE,
    PDXE_LANG_PONY,
    PDXE_LANG_LUAU,
    PDXE_LANG_JANET,
    PDXE_LANG_SWAY,
    PDXE_LANG_NASM,
    PDXE_LANG_ASSEMBLY,
    PDXE_LANG_ASTRO,
    PDXE_LANG_BLADE,
    PDXE_LANG_JUST,
    PDXE_LANG_GOTEMPLATE,
    PDXE_LANG_TEMPL,
    PDXE_LANG_LIQUID,
    PDXE_LANG_JINJA2,
    PDXE_LANG_PRISMA,
    PDXE_LANG_HYPRLANG,
    PDXE_LANG_DOTENV,
    PDXE_LANG_DIFF,
    PDXE_LANG_WGSL,
    PDXE_LANG_KDL,
    PDXE_LANG_JSON5,
    PDXE_LANG_JSONNET,
    PDXE_LANG_RON,
    PDXE_LANG_THRIFT,
    PDXE_LANG_CAPNP,
    PDXE_LANG_PROPERTIES,
    PDXE_LANG_SSHCONFIG,
    PDXE_LANG_BIBTEX,
    PDXE_LANG_STARLARK,
    PDXE_LANG_BICEP,
    PDXE_LANG_CSV,
    PDXE_LANG_REQUIREMENTS,
    PDXE_LANG_HLSL,
    PDXE_LANG_VHDL,
    PDXE_LANG_SYSTEMVERILOG,
    PDXE_LANG_DEVICETREE,
    PDXE_LANG_LINKERSCRIPT,
    PDXE_LANG_GN,
    PDXE_LANG_KCONFIG,
    PDXE_LANG_BITBAKE,
    PDXE_LANG_SMALI,
    PDXE_LANG_TABLEGEN,
    PDXE_LANG_ISPC,
    PDXE_LANG_CAIRO,
    PDXE_LANG_MOVE,
    PDXE_LANG_SQUIRREL,
    PDXE_LANG_FUNC,
    PDXE_LANG_REGEX,
    PDXE_LANG_JSDOC,
    PDXE_LANG_RST,
    PDXE_LANG_BEANCOUNT,
    PDXE_LANG_MERMAID,
    PDXE_LANG_PUPPET,
    PDXE_LANG_PO,
    PDXE_LANG_GITATTRIBUTES,
    PDXE_LANG_GITIGNORE,
    PDXE_LANG_SLANG,
    PDXE_LANG_LLVM_IR,
    PDXE_LANG_SMITHY,
    PDXE_LANG_WIT,
    PDXE_LANG_TLAPLUS,
    PDXE_LANG_PKL,
    PDXE_LANG_GOMOD,
    PDXE_LANG_APEX,
    PDXE_LANG_SOQL,
    PDXE_LANG_SOSL,
    PDXE_LANG_KUSTOMIZE,            // kustomization.yaml — Kubernetes overlay tool
    PDXE_LANG_K8S,                  // Generic Kubernetes manifest (apiVersion: detected)
    PDXE_LANG_PINE,                 // Pine Script (TradingView indicator / strategy language)
    PDXE_LANG_QML,                  // Qt QML (Qt Modeling Language — declarative UI + embedded JS)
    PDXE_LANG_CFSCRIPT,             // CFML script dialect (.cfc components — Lucee/ColdFusion)
    PDXE_LANG_CFML,                 // CFML tag dialect (.cfm templates — Lucee/ColdFusion)
    PDXE_LANG_MOJO,                 // Mojo
    PDXE_LANG_OBJECTSCRIPT_UDL,     // InterSystems ObjectScript UDL (.cls class files)
    PDXE_LANG_OBJECTSCRIPT_ROUTINE, // InterSystems ObjectScript routine (.mac/.int/.rtn/.inc)
    PDXE_LANG_OBJECTSCRIPT_EXPORT,  // InterSystems Studio Export XML (<Export generator="Cache">)
    PDXE_LANG_ARKTS,    // ArkTS (HarmonyOS/OpenHarmony .ets — TypeScript superset + ArkUI)
    PDXE_LANG_PLSQL,    // Oracle PL/SQL
    PDXE_LANG_CHIALISP, // Chialisp (.clsp/.clib/.clinc — Chia smart-coin s-expression language)
    PDXE_LANG_COUNT
} PDXELanguage;

// --- Extraction result structs ---

/* One HTTP route binding a decorator or annotation declares for a definition, with
 * the source node that declares it. One fact per method and path the node binds (an
 * annotation listing two paths is two facts with one node). `ast_path` is the node
 * types from the lowest node holding both the definition and the declaring node (the
 * definition itself when the node is inside it) down to the declaring node,
 * NULL-terminated, the strings the grammar's own; NULL when they could not be
 * recorded. The span is the declaring node's, in the raw source. */
typedef struct {
    const char *method;      // upper-case HTTP method, or "ANY"
    const char *path;        // the route's path, joined to its class's prefix
    const char *callee_text; // the decorator's callee or the annotation's name, as written
    const char *source_text; // the declaring decorator call or annotation, as written
    const char **ast_path;   // see above
    uint32_t start_byte;     // the declaring node's span; end > start
    uint32_t end_byte;
} PDXERouteFact;

typedef struct {
    const char *name;           // short name
    const char *qualified_name; // project.path.name
    const char *label;          // "Function", "Method", "Class", "Variable", "Module"
    const char *file_path;      // relative path
    uint32_t start_line;
    uint32_t end_line;
    const char *signature;              // parameter text (NULL if none)
    const char *return_type;            // return type text (NULL if none)
    const char *receiver;               // Go method receiver (NULL if none)
    const char *docstring;              // leading doc comment (NULL if none)
    const char *parent_class;           // enclosing class QN for methods (NULL if none)
    const char **decorators;            // NULL-terminated array (NULL if none)
    const char **base_classes;          // NULL-terminated array (NULL if none)
    const char **param_names;           // NULL-terminated array (NULL if none)
    const char **param_types;           // NULL-terminated array (NULL if none)
    const char **signature_param_types; // ordered internal signature types; "?" means unknown
    int signature_param_count;          // number of entries in signature_param_types
    const char **return_types;          // NULL-terminated array (NULL if none)
    const char *route_path;   // HTTP route path from decorator (e.g., "/api/users") or NULL
    const char *route_method; // HTTP method from decorator (e.g., "POST") or NULL
    int complexity;           // cyclomatic complexity
    int cognitive;            // cognitive complexity (nesting-weighted)
    int loop_count;           // number of loop constructs in the body
    int loop_depth;           // max nested-loop depth (bottleneck proxy)
    bool is_recursive;        // body contains a direct self-call (seed for "recursive")
    int param_count;          // number of parameters (large = complexity smell)
    int max_access_depth;     // deepest chained member/subscript access (a.b.c.d)
    int linear_scan_in_loop;  // count of linear-scan calls (find/contains/indexOf) inside loops
    int alloc_in_loop;        // count of allocation/append calls inside loops
    bool recursion_in_loop;   // a self-call occurs inside a loop body
    bool unguarded_recursion; // recursive with no self-call guarded by a conditional
    int lines;                // body line count
    uint32_t *fingerprint;    // MinHash fingerprint (arena-allocated, K values) or NULL
    int fingerprint_k;        // number of hash values (PDXE_MINHASH_K or 0)
    bool is_exported;
    bool is_abstract;
    bool is_test;
    bool is_entry_point;
    const char *structural_profile; // AST structural profile (arena-allocated) or NULL
    const char *body_tokens; // space-separated raw identifier tokens from body (arena) or NULL
    /* Rust only: raw trait path from the exact `impl Trait for Type` block
     * that declared this method.  Kept at the tail so zero-initialised
     * callers in every other language remain ABI/source compatible. */
    const char *impl_trait;
    /* Every route binding the definition's decorators or annotations declare, each with
     * its declaring node (PDXERouteFact); NULL and 0 for none. route_path/route_method
     * above keep the first, as before. Kept at the tail, as impl_trait is. */
    PDXERouteFact *routes;
    int route_count;
} PDXEDefinition;

/* Argument captured from a call expression */
typedef struct {
    const char *expr;    // raw expression text ("payload.info", "MY_URL", "'hello'")
    const char *value;   // resolved string value or NULL (constant propagation)
    const char *keyword; // keyword name if keyword arg ("url", "topic_id"), NULL if positional
    int index;           // positional index (0-based)
} PDXECallArg;

#define PDXE_MAX_CALL_ARGS 8

/* Byte offsets are meaningful only within the source buffer that produced
 * them. C/C++/CUDA run both raw and preprocessed extraction passes, and those
 * buffers can contain unrelated occurrences at the same numeric span. */
typedef enum {
    PDXE_SOURCE_ORIGIN_RAW = 0,
    PDXE_SOURCE_ORIGIN_PREPROCESSED,
} PDXESourceOrigin;

typedef struct {
    const char *callee_name;       // raw callee text ("pkg.Func", "foo")
    const char *enclosing_func_qn; // QN of enclosing function (or module QN)
    const char *first_string_arg;  // first string literal argument (URL, topic, key) or NULL
    const char *second_arg_name;   // second argument identifier (handler ref) or NULL
    /* First arg_count captured arguments, arena-allocated on first capture;
     * NULL when arg_count == 0. Was an inline args[PDXE_MAX_CALL_ARGS] (256 of
     * the record's 320 bytes) -- the Go corpus census (2026-09-13) put 825k
     * calls at 251 MB with most of that empty slots. Readers index it exactly
     * as before; only `sizeof` changed. */
    PDXECallArg *args;
    int arg_count;                   // number of captured arguments (<= PDXE_MAX_CALL_ARGS)
    int loop_depth;                  // enclosing loop nesting at the call site
    int branch_depth;                // enclosing branch nesting at the call site
    int start_line;                  // 1-based source line of the call (for def range-match)
    uint32_t site_start_byte;        // exact AST occurrence span; end > start when present
    uint32_t site_end_byte;          // exclusive byte offset in the source file
    PDXESourceOrigin source_origin;   // raw source or C-family preprocessed buffer
    bool is_method;                  // method/member call with an UNRESOLVED receiver. Perl:
                                     // arrow/method call ($obj->m). TS/JS/TSX: member call
                                     // x.foo() whose receiver is not this/super. Python:
                                     // x.foo() where x is not self/cls/super() and is not
                                     // rooted in an imported name. Read by the weak-member
                                     // guard and by the pxc synthetic-carrier dedup key in
                                     // pass_lsp_cross.c. Default false.
    bool requires_lsp_resolution;    // synthetic semantic candidate (for example an implicit
                                     // C++ operator). Never fall back to textual resolution.
    bool callee_is_locally_bound;    // bare call foo() whose callee identifier is bound as a
                                     // parameter of an enclosing function, so it cannot be the
                                     // module-level foo. Python only today. Read by the
                                     // weak-local-binding guard. Default false.
    bool receiver_is_self_attribute; // Python member call whose receiver is an attribute
                                     // chain rooted at self/cls but not self/cls itself
                                     // (self.compiler.apply_converters()). An object the
                                     // class owns, not a parameter: read by the weak-member
                                     // guard's unique-name exemption. Default false.
    /* The syntax node types from the enclosing definition's node (the file's root
     * at file scope) down to the node the call is recorded at, NULL-terminated.
     * The strings are the language's own and static. NULL when the walk could not
     * record them. Kept at the tail, as impl_trait is, for zero-initialised callers. */
    const char **ast_path;
} PDXECall;

typedef struct {
    const char *local_name;  // local alias or name
    const char *module_path; // resolved module path / QN
} PDXEImport;

typedef enum {
    PDXE_USAGE_VALUE = 0,
    PDXE_USAGE_CALL_REFERENCE,
} PDXEUsageKind;

typedef struct {
    const char *ref_name;          // referenced identifier
    const char *enclosing_func_qn; // QN of enclosing function (or module QN)
    /* Fixed-width fields grouped so the record packs to 40 bytes (was 48; the
     * Go corpus holds 4.68M of these). Field meanings unchanged. */
    uint32_t lexical_scope_id;       // extraction-local scope instance; never graph identity
    uint32_t site_start_byte;        // exact reference-token span; end > start when present
    uint32_t site_end_byte;          // exclusive byte offset in the source file
    PDXEUsageKind kind;               // ordinary USAGE or explicit callable reference
    PDXESourceOrigin source_origin;   // raw source or C-family preprocessed buffer
    bool may_be_call_reference;      // syntactic candidate; exact LSP proof may upgrade its edge
    bool semantic_reference_blocked; // lexical evidence blocks only unproven textual fallback
    bool semantic_reference_local_shadow; // blocker belongs to a non-module lexical scope
    bool is_member_access;                // token is the member half of a selector/attribute
                                          // (Go x.f — field_identifier). The extractor strips
                                          // the receiver, so this is the only surviving record
                                          // of selector shape (#1962). Default false.
    /* As PDXECall.ast_path, recorded only for a usage that may be a callable
     * reference (kind CALL_REFERENCE, or VALUE with may_be_call_reference);
     * NULL for every other usage. */
    const char **ast_path;
} PDXEUsage;

typedef struct {
    const char *exception_name;    // exception class/type name
    const char *enclosing_func_qn; // QN of enclosing function
} PDXEThrow;

typedef struct {
    const char *var_name;          // variable name
    const char *enclosing_func_qn; // QN of enclosing function
    bool is_write;                 // true = write, false = read
    bool is_member_access;         // var_name is the field half of a selector/member LHS
                                   // (`t.err = x` → "err"); the receiver is stripped here,
                                   // so this is the only record of selector shape (#1962)
} PDXEReadWrite;

typedef struct {
    const char *type_name;         // referenced type/class name
    const char *enclosing_func_qn; // QN of enclosing function
} PDXETypeRef;

typedef struct {
    const char *env_key;           // environment variable key
    const char *enclosing_func_qn; // QN of enclosing function
} PDXEEnvAccess;

typedef struct {
    const char *var_name;          // variable being assigned
    const char *type_name;         // class/type name of RHS constructor
    const char *enclosing_func_qn; // QN of enclosing function
} PDXETypeAssign;

// String reference: URL, config key, or async target found in source.
// Extracted from string literals during AST walk.
typedef enum {
    PDXE_STRREF_URL = 0,    // REST path or full URL
    PDXE_STRREF_CONFIG = 1, // config file path or env var key
} PDXEStringRefKind;

typedef struct {
    const char *value;             // the string literal content
    const char *enclosing_func_qn; // QN of enclosing function
    const char *key_path;          // dotted key path from YAML/JSON nesting (NULL if flat)
    PDXEStringRefKind kind;         // URL, CONFIG
} PDXEStringRef;

/* Infrastructure binding: topic/queue → endpoint URL.
 * Extracted from YAML/HCL/JSON subscription/scheduler configs.
 * Used by pass_route_nodes to connect async Route nodes to handler services. */
typedef struct {
    const char *source_name; // topic, queue, or schedule name
    const char *target_url;  // push_endpoint, uri, or http_target URL
    const char *broker;      // "pubsub", "cloud_tasks", "cloud_scheduler", "sqs", "kafka"
} PDXEInfraBinding;

/* Pub/sub channel participation.  One record per emit() or on()/addListener()
 * call detected in source — the receiver (e.g. Socket.IO client, EventEmitter
 * instance) is intentionally NOT identified; matching is by channel_name
 * across files, which captures the common pattern of one logical bus per
 * service.  Transport disambiguates Socket.IO vs EventEmitter vs future
 * detectors (Kafka, Cloud Pub/Sub, etc.). */
typedef enum {
    PDXE_CHANNEL_EMIT = 0,
    PDXE_CHANNEL_LISTEN = 1,
} PDXEChannelDirection;

typedef struct {
    const char *channel_name;      // literal channel name (e.g. "user.created")
    const char *transport;         // "socketio", "event_emitter", ...
    const char *enclosing_func_qn; // QN of the function containing the emit/on call
    PDXEChannelDirection direction;
} PDXEChannel;

// Rust: impl Trait for Struct
typedef struct {
    const char *trait_name;  // trait name (raw text)
    const char *struct_name; // struct/type name (raw text)
    /* Exact extracted QN of the implementing type.  Unlike struct_name this
     * does not need a later leaf-name guess, and the relation exists even for
     * an empty `impl Trait for Type {}` block. */
    const char *struct_qn;
} PDXEImplTrait;

typedef enum {
    PDXE_RESOLVED_INVOCATION = 0,
    PDXE_RESOLVED_CALL_REFERENCE,
} PDXEResolvedKind;

// LSP-resolved invocation/reference: high-confidence type-aware resolution.
typedef struct {
    const char *caller_qn;         // enclosing function QN
    const char *callee_qn;         // resolved target QN (fully qualified)
    const char *strategy;          // "lsp_type_dispatch", "lsp_direct", etc.
    float confidence;              // 0.90-0.95
    const char *reason;            // diagnostic label for unresolved calls (NULL if resolved)
    PDXEResolvedKind kind;          // invocation (CALLS) or explicit callable reference
    uint32_t site_start_byte;      // exact source occurrence; end > start when present
    uint32_t site_end_byte;        // exclusive byte offset in the source file
    PDXESourceOrigin source_origin; // raw source or C-family preprocessed buffer
} PDXEResolvedCall;

typedef struct {
    PDXEResolvedCall *items;
    int count;
    int cap;
} PDXEResolvedCallArray;

// Growable arrays used during extraction.
typedef struct {
    PDXEDefinition *items;
    int count;
    int cap;
} PDXEDefArray;

typedef struct {
    PDXECall *items;
    int count;
    int cap;
} PDXECallArray;

typedef struct {
    PDXEImport *items;
    int count;
    int cap;
} PDXEImportArray;

typedef struct {
    PDXEUsage *items;
    int count;
    int cap;
} PDXEUsageArray;

typedef struct {
    PDXEThrow *items;
    int count;
    int cap;
} PDXEThrowArray;

typedef struct {
    PDXEReadWrite *items;
    int count;
    int cap;
} PDXERWArray;

typedef struct {
    PDXETypeRef *items;
    int count;
    int cap;
} PDXETypeRefArray;

typedef struct {
    PDXEEnvAccess *items;
    int count;
    int cap;
} PDXEEnvAccessArray;

typedef struct {
    PDXETypeAssign *items;
    int count;
    int cap;
} PDXETypeAssignArray;

typedef struct {
    PDXEStringRef *items;
    int count;
    int cap;
} PDXEStringRefArray;

typedef struct {
    PDXEInfraBinding *items;
    int count;
    int cap;
} PDXEInfraBindingArray;

typedef struct {
    PDXEImplTrait *items;
    int count;
    int cap;
} PDXEImplTraitArray;

typedef struct {
    PDXEChannel *items;
    int count;
    int cap;
} PDXEChannelArray;

// Full extraction result for one file.
typedef struct PDXEFileResult {
    PDXEArena arena; // owns local memory; composites may also retain child arenas below

    PDXEDefArray defs;
    PDXECallArray calls;
    PDXEImportArray imports;
    PDXEUsageArray usages;
    PDXEThrowArray throws;
    PDXERWArray rw;
    PDXETypeRefArray type_refs;
    PDXEEnvAccessArray env_accesses;
    PDXETypeAssignArray type_assigns;
    PDXEImplTraitArray impl_traits;       // Rust: impl Trait for Struct pairs
    PDXEResolvedCallArray resolved_calls; // LSP-resolved invocations/references (high confidence)
    PDXEStringRefArray string_refs;       // URL/config string literals from AST
    PDXEInfraBindingArray infra_bindings; // topic→URL pairs from IaC configs
    PDXEChannelArray channels;            // Socket.IO / EventEmitter pub/sub participation

    const char *module_qn;      // module qualified name
    const char *namespace_name; // declared namespace/package (Java/Kotlin/C#/PHP), NULL if none
    const char **exports;       // NULL-terminated (NULL if none)
    const char **constants;     // NULL-terminated (NULL if none)
    const char **global_vars;   // NULL-terminated (NULL if none)
    const char **macros;        // NULL-terminated, C/C++ only (NULL if none)

    bool has_error;
    const char *error_msg;
    /* Best-effort parse-coverage signal (experimental). parse_incomplete is true
     * when the parse tree contains tree-sitter ERROR/MISSING nodes — constructs
     * in those regions are silently absent from the graph. error_ranges is a
     * compact "start-end,start-end" list of 1-based line ranges (arena-owned) or
     * NULL. This only marks what we can DETECT: the absence of a flag is NOT a
     * completeness guarantee. Callers should treat a flagged file as "prefer
     * grep here", never treat an unflagged file as provably complete. */
    bool parse_incomplete;
    /* True when the ranges cover so much of the file that they are no longer
     * useful advice — one range over 80% of the line count. The file WAS
     * indexed, but pointing a reader at almost every line tells them nothing,
     * so the report says "read the source" instead of listing the range.
     *
     * Its main customers are non-C languages. The refinement that narrows a
     * whole-file range using the preprocessed parse only runs for C, C++ and
     * CUDA, so a Python, Java or Ruby file whose root node is ERROR still
     * reports 1-N.
     *
     * Note the naming: this field and the phase string it produces are both
     * `parse_unusable`. The older `parse_incomplete` field emits the phase
     * `parse_partial` instead. That mismatch is historical, not deliberate —
     * do not copy it. */
    bool parse_unusable;
    const char *error_ranges;
    int error_region_count;
    bool is_test_file;
    int imports_count;
    TSTree *cached_tree; // retained parse tree (caller frees via pdxe_free_tree)
    /* The parse alone used more than its share of the per-file budget: the
     * per-file LSP walk and the cross-file resolve skip this file (its
     * unified-extractor defs stay). Set by pdxe_engine_extract_file_ex, honoured by
     * pdxe_pxc_dispatch_file -- one site for every language. */
    bool lsp_skipped;
    /* The unified walk stopped at its CPU budget: defs/calls/usages found up
     * to that point are kept, the rest of the file is not walked. Implies
     * lsp_skipped. */
    bool walk_truncated;
    /* Size of this file's parse tree, and how much of it the unified walk got
     * through. Reported for a truncated or LSP-skipped file so the coverage
     * report says how much of it is missing, instead of leaving the gap
     * silent. */
    uint32_t tree_nodes;
    uint32_t walk_nodes_visited;
    PDXELanguage cached_lang; // language of cached tree (for parser selection)

    // Retained source bytes — copied into `arena` by the parallel
    // extract pass so the fused cross-file LSP step in resolve_worker
    // can run without re-reading the file from disk. NULL when the
    // file exceeded the per-file (100 MB) or total (2 GB) retention
    // cap; in that case the cross-file LSP step is skipped for this
    // file (defs/calls already extracted are unaffected).
    const char *source;
    int source_len;

    // Composite extraction results (currently ObjectScript Studio Export)
    // retain their per-unit results so shallow-copied carrier strings remain
    // valid for the composite's full lifetime. Owned and recursively released
    // by pdxe_free_result(); ordinary single-file results leave these zeroed.
    struct PDXEFileResult **owned_results;
    int owned_result_count;
} PDXEFileResult;

// --- Enclosing function cache ---
// Avoids repeated parent-chain walks for nodes within the same function body.
// Each entry records a function's byte range and its precomputed QN.
#define EFC_SIZE 64 // power of 2 for fast modulo

typedef struct {
    uint32_t start_byte;
    uint32_t end_byte;
    const char *qn;
} EFCEntry;

typedef struct {
    EFCEntry entries[EFC_SIZE];
    int count;
} EFCache;

// --- Extraction context passed to sub-extractors ---

// Module-level string constant map (for constant propagation)
#define PDXE_MAX_STRING_CONSTANTS 256
typedef struct {
    const char *names[PDXE_MAX_STRING_CONSTANTS];
    const char *values[PDXE_MAX_STRING_CONSTANTS];
    bool is_url_builder[PDXE_MAX_STRING_CONSTANTS];
    int count;
} PDXEStringConstantMap;

// Forward declaration: ObjectScript macro table (defined in macro_table.h).
typedef struct PDXEMacroTable PDXEMacroTable;

// Method-return-type table for ObjectScript variable type inference. Populated
// from definition nodes (method QN -> declared return type) so a later
// `Set x = obj.Method()` can resolve x's class.
#define PDXE_RETURN_TYPE_TABLE_CAP 2048

typedef struct {
    const char *method_qn;
    const char *return_type;
} PDXEReturnTypeEntry;

typedef struct {
    PDXEReturnTypeEntry entries[PDXE_RETURN_TYPE_TABLE_CAP];
    int count;
} PDXEReturnTypeTable;

typedef struct {
    PDXEArena *arena;
    /* Scratch for AST traversal, owned by the pdxe_engine_extract_file_ex call that
     * built this context and destroyed when it returns. Nothing a
     * PDXEFileResult points at may be allocated here: `arena` is the result's
     * own, and it outlives extraction by the whole pipeline (#1997). NULL in a
     * context built without one, in which case the stacks fall back to
     * `arena`. */
    PDXEArena *scratch;
    PDXEFileResult *result;
    const char *source;
    int source_len;
    PDXELanguage language;
    const char *project;
    const char *rel_path;
    const char *module_qn;
    TSNode root;
    EFCache ef_cache;                            // enclosing function cache
    const char *enclosing_class_qn;              // for nested class QN computation
    PDXEStringConstantMap string_constants;       // module-level NAME = "value" pairs
    const PDXEMacroTable *macro_table;            // ObjectScript $$$macro table (NULL if none)
    const PDXEReturnTypeTable *return_type_table; // ObjectScript method return types (NULL if none)
    /* Set by extract_class_variables around its extract_var_names calls, so a
     * class-body variable def records which class declares it (parent_class)
     * without changing its module-level qualified name. NULL elsewhere. */
    const char *var_parent_class;
    /* Per-file walk budget in VISITED NODES (0 = unbounded). The unified cursor
     * walk stops once it is spent, so no single file can hold a worker for
     * minutes: a 23 MB single-expression C# test file cost 346 s in usage
     * stamping alone (tree-sitter's ts_node_parent descends from the root,
     * quadratic on a deep tree; 2026-09-14). What was extracted before the stop
     * is kept, and the file is named in the coverage report. Counted in nodes
     * rather than CPU time so that the same file always stops at the same node
     * — see PDXE_WALK_MAX_NODES_DEFAULT for what a clock did here. */
    uint32_t walk_budget_nodes;
    bool walk_budget_exhausted;
    /* How many nodes the unified walk actually visited (whether or not it ran
     * out of budget) — the measurement the budget has to be expressed in. */
    uint32_t walk_nodes_visited;
} PDXEExtractCtx;

// --- Public API ---

// Bind third-party allocators (tree-sitter, sqlite3) to mimalloc as
// defense-in-depth, so they never depend on the fragile MI_OVERRIDE symbol
// override (#424). MUST be called as the very first statement of main(), before
// any sqlite3_open*/sqlite3_initialize (SQLITE_CONFIG_MALLOC returns
// SQLITE_MISUSE once sqlite has initialized).
// Idempotent (static guard); intended for single-threaded startup. pdxe_engine_init()
// also calls it so non-main entry points (pipeline passes) still get the binds.
// In the test build (no PDXE_BIND_TS_ALLOCATOR) this is a no-op.
void pdxe_alloc_init(void);
/* SQLite allocates from a dedicated mimalloc heap per thread while on; the
 * index worker turns it on (its default heap holds the graph). Off elsewhere:
 * a thread-per-connection daemon would pin connection-lifetime blocks to
 * dead threads. The switch exists in every build; it changes nothing where
 * the allocator binds are compiled out. */
void pdxe_sqlite_dedicated_heap(bool on);

// Initialize the library. Call once at startup. Returns 0 on success.
int pdxe_engine_init(void);

// True when rel_path is in the crash-quarantine set — the newline-delimited list
// of files (PDXE_INDEX_QUARANTINE_FILE) the crash supervisor pinned as crashers
// during its single-threaded recovery re-run. Loaded once, lazily; read-only
// after load. pdxe_engine_extract_file short-circuits such files to an empty result so no
// pass can crash on them; the pipeline extract loops call this to also REPORT the
// skip as phase="crash". Always false (cheap no-op) when the env var is unset.
bool pdxe_index_is_quarantined(const char *rel_path);

// Phase a quarantined file was pinned under: "crash" (a fault signal) or "hang"
// (killed for making no progress). Returns NULL when rel_path is not quarantined.
// Drives the same lazy once-load as pdxe_index_is_quarantined. Used by the pipeline
// extract loops to report the skip's phase in skipped[] (falls back to "crash").
const char *pdxe_index_quarantine_phase(const char *rel_path);

// Crash-supervisor marker journal (parallel-safe): appends "S <rel_path>" /
// "D <rel_path>" to PDXE_INDEX_MARKER_FILE. Files with an S but no D form the
// parent's crash/hang suspect set. No-ops when the env var is unset.
// pdxe_engine_extract_file journals its own start/done; long-running per-file phases
// (cross-LSP resolve) call these around their per-file work so a hang there
// is attributed to the RIGHT file instead of a stale extraction marker.
void pdxe_index_mark_start(const char *rel_path);
void pdxe_index_mark_done(const char *rel_path);

// Extract all data from one file. Caller must call pdxe_free_result().
// source must remain valid for the duration of the call.
// timeout_micros: per-file parse timeout in microseconds (0 = no timeout).
/* Compact a finished result: copy everything reachable from it -- every
 * record array at exact count, every string once (interned by content within
 * the file), the retained source -- into one exact-size arena, and destroy the
 * working arena the extractors wrote into. Measured on the Go corpus
 * (2026-09-13): 14.8 GB written per index, 3.4 GB reachable; the rest was
 * node-text copies and abandoned array generations no one could free because
 * the result owned the arena. Call once, after the last per-file write and
 * before the result is cached for later passes. Later appends into the arena
 * still work (growth restarts at the default block). A composite's owned
 * per-unit results are released: after the deep copy nothing points at them.
 * On allocation failure the result is left exactly as it was. */
void pdxe_result_compact(PDXEFileResult *result);

/* The working arena the extractors write into, per worker thread. Extraction
 * takes it (rewound, pages still mapped) instead of allocating a fresh arena
 * per file; compaction returns it instead of destroying it. Reusing the same
 * addresses directly is what stops the purge/re-commit churn that kept a
 * kernel worker at 15 GB resident with 4-5 GB charged. An arena that grew past
 * PDXE_WORK_ARENA_KEEP_BYTES (one giant file) is destroyed, not kept. */
enum { PDXE_WORK_ARENA_KEEP_BYTES = 16 * 1024 * 1024 };
void pdxe_work_arena_take(PDXEArena *into);
void pdxe_work_arena_give(PDXEArena *from);
/* Drop this thread's kept working arena (end of an extraction pass). */
void pdxe_work_arena_release(void);
/* True on a thread that has given a working arena back, i.e. a pipeline
 * worker whose pdxe_work_arena_release is guaranteed to run: only such a thread
 * may keep per-thread scratch between files. */
bool pdxe_work_arena_keeping(void);
/* Let this thread keep its per-file extraction scratch between the files of a
 * sequential loop; the caller must call pdxe_work_arena_release on the same
 * thread when the loop ends (every return path). */
void pdxe_work_arena_keep_begin(void);
/* Free the compaction scratch this thread kept (pdxe_work_arena_release calls it). */
void pdxe_result_compact_release_thread(void);

PDXEFileResult *pdxe_engine_extract_file(const char *source, int source_len, PDXELanguage language,
                                const char *project, const char *rel_path, int64_t timeout_micros,
                                const char **extra_defines, // NULL-terminated, or NULL
                                const char **include_paths  // NULL-terminated, or NULL
);

// Pipeline-internal variant of pdxe_engine_extract_file() carrying ObjectScript
// per-project tables (macro table + method-return-type table). The public
// pdxe_engine_extract_file() is a thin wrapper that passes NULL, NULL for both.
PDXEFileResult *pdxe_engine_extract_file_ex(
    const char *source, int source_len, PDXELanguage language, const char *project,
    const char *rel_path, int64_t timeout_micros,
    const char **extra_defines,                 // NULL-terminated, or NULL
    const char **include_paths,                 // NULL-terminated, or NULL
    const PDXEMacroTable *macro_table,           // ObjectScript macros, or NULL
    const PDXEReturnTypeTable *return_type_table // OS return types, or NULL
);

// Free all memory associated with a result.
void pdxe_free_result(PDXEFileResult *result);

/* Allocate an empty result; pdxe_free_result releases it. */
PDXEFileResult *pdxe_result_alloc(void);

/* Release a composite result's per-unit results (the owner of that array). */
void pdxe_result_release_owned(PDXEFileResult *result);

// Free only the cached tree from a result (caller retained it for reuse).
void pdxe_free_tree(PDXEFileResult *result);

// Free a standalone TSTree pointer (for Go layer cleanup).
void pdxe_free_tree_ptr(TSTree *tree);

// Reset the thread-local parser's internal state, releasing slab-allocated
// subtrees. Must be called BEFORE pdxe_slab_reset_thread() so the slab rebuild
// doesn't corrupt live parser state.
void pdxe_reset_thread_parser(void);

// Destroy the thread-local parser. Call on worker thread exit.
void pdxe_destroy_thread_parser(void);

// This thread's reusable tree cursor, reset to `node`. Child walks run once per
// visited node; a cursor per walk was a malloc + free of its stack each time
// (49.8 M on the Go corpus, waste sanitizer 2026-09-17). Valid until the next
// call on this thread; released with the thread parser.
TSTreeCursor *pdxe_thread_cursor(TSNode node);

// Reusable cursors for RECURSIVE walks: one per recursion depth on this thread,
// so a walk that recurses from inside its child loop never shares a cursor with
// its own callers. A slot already in use (another walker nested on this thread)
// hands out a private cursor instead; release deletes it. Released with the
// thread parser.
typedef struct {
    TSTreeCursor *cursor;
    TSTreeCursor private_cursor;
    int slot; /* -1: private */
} pdxe_cursor_lease_t;
TSTreeCursor *pdxe_cursor_acquire(pdxe_cursor_lease_t *lease, int depth, TSNode node);
void pdxe_cursor_release(pdxe_cursor_lease_t *lease);

// Shutdown the library. Call once at exit.
void pdxe_engine_shutdown(void);

// Profiling: get accumulated parse/extraction times and file count.
typedef struct {
    uint64_t *parse_ns;
    uint64_t *extract_ns;
    uint64_t *files;
} pdxe_profile_out_t;
void pdxe_get_profile(pdxe_profile_out_t out);
uint64_t pdxe_get_lsp_ns(void);
uint64_t pdxe_get_preprocess_ns(void);
uint64_t pdxe_get_files_preprocessed(void);
void pdxe_reset_profile(void);

#if defined(PDXE_KOTLIN_DEDUP_TEST_API) && PDXE_KOTLIN_DEDUP_TEST_API
// Test-build-only operation counter for Kotlin operator-carrier deduplication.
// Production builds do not expose or retain this instrumentation.
void pdxe_kotlin_operator_dedup_test_reset(void);
uint64_t pdxe_kotlin_operator_dedup_test_comparisons(void);
#endif

#if defined(PDXE_CALL_REFERENCE_LOOKUP_TEST_API) && PDXE_CALL_REFERENCE_LOOKUP_TEST_API
// Test-build-only work counter for resolving a node's field role while
// classifying value references. Production builds retain no instrumentation.
void pdxe_usage_field_lookup_test_reset(void);
uint64_t pdxe_usage_field_lookup_test_work(void);
uint64_t pdxe_usage_slow_parent_fallback_test_count(void);
#endif

// Toggle C/C++ preprocessor Macro-node extraction (#375). The pipeline enables
// it only for full/advanced index modes (it dominates extraction on macro-dense
// codebases). Default ON. Set before extraction; read-only during.
void pdxe_set_macro_extraction(int enabled);
int pdxe_macro_extraction_enabled(void);

// --- Internal helpers used by extractors ---

// Growable array push functions (arena-allocated, no individual free needed).
void pdxe_defs_push(PDXEDefArray *arr, PDXEArena *a, PDXEDefinition def);
void pdxe_calls_push(PDXECallArray *arr, PDXEArena *a, PDXECall call);
void pdxe_imports_push(PDXEImportArray *arr, PDXEArena *a, PDXEImport imp);
void pdxe_usages_push(PDXEUsageArray *arr, PDXEArena *a, PDXEUsage usage);
void pdxe_throws_push(PDXEThrowArray *arr, PDXEArena *a, PDXEThrow thr);
void pdxe_rw_push(PDXERWArray *arr, PDXEArena *a, PDXEReadWrite rw);
void pdxe_typerefs_push(PDXETypeRefArray *arr, PDXEArena *a, PDXETypeRef tr);
void pdxe_envaccess_push(PDXEEnvAccessArray *arr, PDXEArena *a, PDXEEnvAccess ea);
void pdxe_typeassign_push(PDXETypeAssignArray *arr, PDXEArena *a, PDXETypeAssign ta);
void pdxe_stringref_push(PDXEStringRefArray *arr, PDXEArena *a, PDXEStringRef sr);
void pdxe_infrabinding_push(PDXEInfraBindingArray *arr, PDXEArena *a, PDXEInfraBinding ib);
void pdxe_impltrait_push(PDXEImplTraitArray *arr, PDXEArena *a, PDXEImplTrait it);
void pdxe_resolvedcall_push(PDXEResolvedCallArray *arr, PDXEArena *a, PDXEResolvedCall rc);
void pdxe_channels_push(PDXEChannelArray *arr, PDXEArena *a, PDXEChannel ch);

// --- Sub-extractor entry points ---

void pdxe_extract_definitions(PDXEExtractCtx *ctx);
/* Internal companion for embedded-language trees that contribute definitions
 * to an existing host-file Module rather than minting a second Module. */
void pdxe_extract_definitions_without_module(PDXEExtractCtx *ctx);
// dbt lineage for Jinja-templated SQL models: emits a Model def plus one usage
// per ref()/source() call. No-op unless the file parses as SQL and actually
// contains a dbt builtin call. Defined in extract_dbt.c.
void pdxe_extract_dbt(PDXEExtractCtx *ctx);
void pdxe_extract_imports(PDXEExtractCtx *ctx);
void pdxe_extract_usages(PDXEExtractCtx *ctx);
void pdxe_extract_semantic(PDXEExtractCtx *ctx);
void pdxe_extract_type_refs(PDXEExtractCtx *ctx);
void pdxe_extract_env_accesses(PDXEExtractCtx *ctx);
void pdxe_extract_type_assigns(PDXEExtractCtx *ctx);
void pdxe_extract_channels(PDXEExtractCtx *ctx);

// Single-pass unified extraction (replaces the 7 calls above except defs+imports).
void pdxe_extract_unified(PDXEExtractCtx *ctx);

// K8s / Kustomize semantic extractor (called when language is PDXE_LANG_K8S or PDXE_LANG_KUSTOMIZE).
void pdxe_extract_k8s(PDXEExtractCtx *ctx);

// --- Label predicates ---

// True when `label` names a TYPE-LIKE container definition — a node that can own
// methods/fields, be a base/embedded type, satisfy/declare an interface, and be a
// target of name→type resolution. The canonical set is:
//   Class, Struct, Interface, Enum, Type, Trait.
// Single source of truth for every type-resolution / registry-seeding /
// INHERITS·IMPLEMENTS / LSP-type-registrar consumer, so adding a new type-like
// label (e.g. "Struct" for Rust/Go/Swift/D structs) updates them all at once
// instead of scattering `|| strcmp(label,"Struct")==0` across the tree.
// `label` may be NULL (returns false). Defined in helpers.c.
bool pdxe_label_is_type_like(const char *label);

// True for data-relation labels (Table, View — SQL DDL). Relations resolve as
// lineage targets only: registry members, but never type-like and never valid
// CALLS/THROWS/READS/WRITES targets. `label` may be NULL. Defined in helpers.c.
bool pdxe_label_is_relation(const char *label);

// True for labels admitted to the cross-file name registry: Function, Method,
// every type-like container, Variable, Field, and the relation labels. Single
// source of truth for registry seeding — the full (pass_definitions.c),
// parallel (pass_parallel.c) and incremental (pipeline_incremental.c) pipelines
// all seed through this predicate so their registries never diverge.
// `label` may be NULL (returns false). Defined in helpers.c.
bool pdxe_label_is_registry_symbol(const char *label);

#endif // PDXE_H
