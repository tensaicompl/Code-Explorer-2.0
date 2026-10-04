/*
 * Vendored subset: the import-target resolver and what it calls, extracted
 * verbatim from the reference's package-map source by
 * scripts/vendor/extract-functions.py. The rest of that source, which scans
 * manifests and discovers packages, is deliberately not here.
 */

#include "pipeline/pipeline.h"
#include "pipeline/pipeline_internal.h"
#include "foundation/compat.h"
#include "foundation/constants.h"
#include "foundation/hash_table.h"
#include "foundation/str_util.h"
#include "lost_work.h" /* allocations whose failure loses work are counted */
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifdef _WIN32
#endif

enum {
    PKGMAP_INIT_CAP = 16,
    PKGMAP_PATH_BUF = 1024,
    PKGMAP_LINE_BUF = 512,
    PKGMAP_HT_INIT = 64,
    PKGMAP_ITOA_BUF = 16,
    /* Hard ceiling on recursive directory descent. This is the
     * non-negotiable termination guarantee for the manifest walk: even
     * if the filesystem contains directory junctions / symlink cycles
     * (the documented reason the walk was once disabled on Windows),
     * descent stops at this depth so the walk can never hang. 64 is far
     * deeper than any real source tree. */
    PKGMAP_WALK_MAX_DEPTH = 64,
    /* String lengths for manifest parsing (avoid magic numbers in memcmp) */
    TOML_NAME_LEN = 4,      /* strlen("name") */
    TOML_NAME_SP = 5,       /* strlen("name ") */
    TOML_NAME_EQ = 5,       /* strlen("name=") */
    XML_PARENT_OPEN = 8,    /* strlen("<parent>") */
    XML_PARENT_CLOSE = 9,   /* strlen("</parent>") */
    XML_GROUP_OPEN = 9,     /* strlen("<groupId>") */
    XML_ARTIFACT_OPEN = 12, /* strlen("<artifactId>") */
};

/* Get the directory part of a relative path (without trailing slash).
 * Returns heap-allocated string. For "foo.json" returns "". */
static char *path_dirname(const char *rel_path) {
    const char *last = strrchr(rel_path, '/');
    if (!last) {
        return pdxe_counted_strdup("");
    }
    return pdxe_counted_strndup(rel_path, (size_t)(last - rel_path));
}

/* Check if a string ends with a suffix. */
static bool ends_with(const char *s, const char *suffix) {
    size_t slen = strlen(s);
    size_t suflen = strlen(suffix);
    if (suflen > slen) {
        return false;
    }
    return strcmp(s + slen - suflen, suffix) == 0;
}

/* Try slash-based prefix matching (Go: github.com/foo/bar/pkg/utils).
 * Returns heap QN or NULL. */
static char *resolve_slash_prefix(PDXEHashTable *map, const char *module_path) {
    char *buf = pdxe_counted_strdup(module_path);
    if (!buf) {
        return NULL;
    }
    for (char *slash = buf + strlen(buf) - SKIP_ONE; slash > buf; slash--) {
        if (*slash != '/') {
            continue;
        }
        *slash = '\0';
        const char *base_qn = (const char *)pdxe_ht_get(map, buf);
        if (!base_qn) {
            continue;
        }
        const char *subpath = module_path + (size_t)(slash - buf) + SKIP_ONE;
        char result[PKGMAP_PATH_BUF];
        snprintf(result, sizeof(result), "%s.%s", base_qn, subpath);
        /* Replace / with . in the appended part */
        for (char *c = result + strlen(base_qn) + SKIP_ONE; *c; c++) {
            if (*c == '/') {
                *c = '.';
            }
        }
        free(buf);
        return pdxe_counted_strdup(result);
    }
    free(buf);
    return NULL;
}

/* Try dot-based prefix matching (Java: com.myorg.pkg.Foo).
 * Returns heap QN or NULL. */
static char *resolve_dot_prefix(PDXEHashTable *map, const char *module_path,
                                const char *project_name) {
    char *buf = pdxe_counted_strdup(module_path);
    if (!buf) {
        return NULL;
    }
    for (char *dot = buf + strlen(buf) - SKIP_ONE; dot > buf; dot--) {
        if (*dot != '.') {
            continue;
        }
        *dot = '\0';
        const char *base_qn = (const char *)pdxe_ht_get(map, buf);
        if (!base_qn) {
            continue;
        }
        const char *subpath = module_path + (size_t)(dot - buf) + SKIP_ONE;
        char subpath_slashed[PKGMAP_PATH_BUF];
        snprintf(subpath_slashed, sizeof(subpath_slashed), "%s", subpath);
        for (char *c = subpath_slashed; *c; c++) {
            if (*c == '.') {
                *c = '/';
            }
        }
        char result[PKGMAP_PATH_BUF];
        snprintf(result, sizeof(result), "%s/%s", base_qn, subpath_slashed);
        free(buf);
        return pdxe_pipeline_fqn_module(project_name, result);
    }
    free(buf);
    return NULL;
}

/* Try backslash-based prefix matching (PHP PSR-4: App\\Controllers\\Foo).
 * Returns heap QN or NULL. */
static char *resolve_backslash_prefix(PDXEHashTable *map, const char *module_path,
                                      const char *project_name) {
    char *buf = pdxe_counted_strdup(module_path);
    if (!buf) {
        return NULL;
    }
    for (char *bs = buf + strlen(buf) - SKIP_ONE; bs > buf; bs--) {
        if (*bs != '\\') {
            continue;
        }
        *bs = '\0';
        char prefix[PKGMAP_PATH_BUF];
        snprintf(prefix, sizeof(prefix), "%s\\", buf);
        const char *base_dir = (const char *)pdxe_ht_get(map, prefix);
        if (!base_dir) {
            continue;
        }
        const char *subpath = module_path + (size_t)(bs - buf) + SKIP_ONE;
        char path_result[PKGMAP_PATH_BUF];
        snprintf(path_result, sizeof(path_result), "%s/%s", base_dir, subpath);
        for (char *c = path_result; *c; c++) {
            if (*c == '\\') {
                *c = '/';
            }
        }
        free(buf);
        return pdxe_pipeline_fqn_module(project_name, path_result);
    }
    free(buf);
    return NULL;
}

/* Path aliases come from tsconfig/jsconfig, which configure TypeScript and
 * JavaScript only: an import in any other language never goes through them. */
static bool path_aliases_apply_to(const char *source_rel) {
    const char *slash = strrchr(source_rel, '/');
    PDXELanguage lang = pdxe_language_for_filename(slash ? slash + 1 : source_rel);
    return lang == PDXE_LANG_TYPESCRIPT || lang == PDXE_LANG_TSX || lang == PDXE_LANG_JAVASCRIPT;
}

char *pdxe_pipeline_resolve_module(const pdxe_pipeline_ctx_t *ctx, const char *source_rel,
                                  const char *module_path) {
    if (!ctx || !module_path) {
        return pdxe_pipeline_fqn_module(ctx ? ctx->project_name : NULL, module_path);
    }

    /* 1. Try relative import resolution (existing logic) */
    char *resolved = pdxe_pipeline_resolve_relative_import(source_rel, module_path);
    if (resolved) {
        /* The relative resolver has already removed an explicit JS/TS file
         * extension.  Treat the remaining path as a module path verbatim so a
         * dotted extensionless basename such as `featureX.engine` is not
         * stripped a second time by pdxe_pipeline_fqn_module. */
        char *qn = pdxe_pipeline_fqn_folder(ctx->project_name, resolved);
        free(resolved);
        return qn;
    }

    /* 1b. Try build-tool path aliases (tsconfig/jsconfig paths today;
     *     other loaders can register here later). Independent of pkgmap. */
    if (ctx->path_aliases && source_rel && path_aliases_apply_to(source_rel)) {
        const pdxe_path_alias_map_t *amap =
            pdxe_path_alias_find_for_file(ctx->path_aliases, source_rel);
        if (amap) {
            char *aliased = pdxe_path_alias_resolve(amap, module_path);
            if (aliased) {
                char *qn = pdxe_pipeline_fqn_module(ctx->project_name, aliased);
                free(aliased);
                return qn;
            }
        }
    }

    /* 2. No pkgmap → fall through immediately */
    PDXEHashTable *pkgmap = pdxe_pipeline_get_pkgmap();
    if (!pkgmap) {
        return pdxe_pipeline_fqn_module(ctx->project_name, module_path);
    }

    /* 3. Exact lookup */
    const char *mapped_qn = (const char *)pdxe_ht_get(pkgmap, module_path);
    if (mapped_qn) {
        return pdxe_counted_strdup(mapped_qn);
    }

    /* 4. Prefix matching by separator type */
    char *result = resolve_slash_prefix(pkgmap, module_path);
    if (!result) {
        result = resolve_dot_prefix(pkgmap, module_path, ctx->project_name);
    }
    if (!result) {
        result = resolve_backslash_prefix(pkgmap, module_path, ctx->project_name);
    }
    if (result) {
        return result;
    }

    /* 5. Fallthrough to default resolution */
    return pdxe_pipeline_fqn_module(ctx->project_name, module_path);
}

/* Return the last path segment of an import string, recognizing the separators
 * used across languages: '.', '/', '\\', and "::".  Returns a pointer into
 * `path` (no allocation). */
static const char *import_last_segment(const char *path) {
    const char *seg = path;
    for (const char *p = path; *p; p++) {
        if (*p == '.' || *p == '/' || *p == '\\' || *p == ':') {
            seg = p + 1;
        }
    }
    return seg;
}

/* Derive a representative imported symbol name from a raw module/use path,
 * stripping language decorations so it can be matched against an in-graph node:
 *   - trailing alias " as X"      (Rust/Kotlin)
 *   - trailing glob "::*" / ".*"  (Rust/Kotlin/Java wildcard)
 *   - brace groups "{a, b, ...}"  (Rust grouped use) → first member
 * Writes the result into `out` (size `outsz`) and returns it, or NULL if none.
 * For grouped/braced forms the first listed symbol is used as the representative
 * (the tests only require at least one resolved IMPORTS edge per statement). */
static const char *import_candidate_symbol(const char *module_path, char *out, size_t outsz) {
    if (!module_path || !module_path[0]) {
        return NULL;
    }
    char buf[1024];
    snprintf(buf, sizeof(buf), "%s", module_path);

    /* Brace group: `prefix::{a, b}` → take first member `a`. */
    char *brace = strchr(buf, '{');
    if (brace) {
        char *first = brace + 1;
        char *end = first;
        while (*end && *end != ',' && *end != '}') {
            end++;
        }
        *end = '\0';
        /* Trim whitespace. */
        while (*first == ' ' || *first == '\t') {
            first++;
        }
        char *t = first + strlen(first);
        while (t > first && (t[-1] == ' ' || t[-1] == '\t')) {
            *--t = '\0';
        }
        if (first[0] && strcmp(first, "self") != 0) {
            snprintf(out, outsz, "%s", first);
            return out;
        }
        /* `{self, ...}` → fall back to the path before the brace group. */
        *brace = '\0';
    }

    /* Strip trailing " as <alias>". */
    char *as = strstr(buf, " as ");
    if (as) {
        *as = '\0';
    }

    /* Strip trailing glob and separator noise. */
    size_t len = strlen(buf);
    while (len > 0 && (buf[len - 1] == '*' || buf[len - 1] == ':' || buf[len - 1] == '.' ||
                       buf[len - 1] == '/' || buf[len - 1] == '\\' || buf[len - 1] == ' ')) {
        buf[--len] = '\0';
    }
    if (!buf[0]) {
        return NULL;
    }
    const char *seg = import_last_segment(buf);
    if (!seg || !seg[0] || strcmp(seg, "*") == 0) {
        return NULL;
    }
    snprintf(out, outsz, "%s", seg);
    return out;
}

/* True for node labels that represent an importable definition (so a symbol-name
 * fallback does not link to, e.g., a Variable or Field). */
static bool import_targetable_label(const char *label) {
    if (!label) {
        return false;
    }
    static const char *ok[] = {"Class", "Interface", "Function", "Method", "Module", "Struct",
                               "Enum",  "Trait",     "Type",     "File",   NULL};
    for (const char **l = ok; *l; l++) {
        if (strcmp(*l, label) == 0) {
            return true;
        }
    }
    return false;
}

/* #1934: whether the name-guess import fallbacks — Strategy 1b (sibling file,
 * whose label filter admits symbols) and Strategy 3 (symbol name) — may run
 * for imports from this language. A Go import path names a package — never a
 * function, method
 * or field — and every correct Go import resolves in Strategy 1 (module path
 * → the package's Folder node); when that misses the import is external and
 * the correct result is NO edge. The fallback instead bound the last path
 * segment to an arbitrary same-named project symbol (`import "os/exec"` → a
 * test harness's exec() method, two imports → a Makefile target). Languages
 * whose import genuinely can name a member (Python `from m import f`, Java
 * `import com.example.Foo`, Rust `use crate::ops::helper`) keep it. */
bool pdxe_import_symbol_fallback_allowed(PDXELanguage lang) {
    return lang != PDXE_LANG_GO;
}

static const char *path_leaf(const char *path) {
    const char *leaf = path;
    for (const char *p = path; p && *p; p++) {
        if (*p == '/' || *p == '\\') {
            leaf = p + 1;
        }
    }
    return leaf;
}

static bool is_header_include(const char *path) {
    if (!path || !path[0]) {
        return false;
    }
    static const char *header_exts[] = {".h", ".hh", ".hpp", ".hxx", ".inc", ".inl", ".ipp", NULL};
    for (const char **ext = header_exts; *ext; ext++) {
        size_t path_len = strlen(path);
        size_t ext_len = strlen(*ext);
        if (path_len > ext_len && strcmp(path + path_len - ext_len, *ext) == 0) {
            return true;
        }
    }
    return false;
}

static bool is_c_family_source(const char *source_rel) {
    if (!source_rel || !source_rel[0]) {
        return false;
    }
    static const char *exts[] = {".c", ".cc", ".cpp", ".cxx", ".c++",
                                 ".h", ".hh", ".hpp", ".hxx", NULL};
    for (const char **ext = exts; *ext; ext++) {
        size_t path_len = strlen(source_rel);
        size_t ext_len = strlen(*ext);
        if (path_len > ext_len && strcmp(source_rel + path_len - ext_len, *ext) == 0) {
            return true;
        }
    }
    return false;
}

/* Directory depth of a repo-relative path: how many directories sit above it. */
static int include_path_depth(const char *path) {
    int depth = 0;
    for (const char *p = path; *p; p++) {
        if (*p == '/' || *p == '\\') {
            depth++;
        }
    }
    return depth;
}

/* Total order among nodes whose file path ends with the include path. The
 * by-name hits arrive in node-registration order, which under parallel
 * extraction is the workers' merge order and differs run to run; taking the
 * first hit made `#include <linux/device.h>` target include/linux/device.h in
 * one index of the kernel and tools/virtio/linux/device.h in the next (5,821
 * IMPORTS edges moved, and every CALLS edge resolved through those files'
 * import maps moved with them). The include names a file, so a File node
 * outranks a symbol declared in it; among files the least nested path wins
 * (include/ over tools/virtio/), then the smaller path, then the smaller QN
 * — a function of the candidate set alone (O9). */
static bool include_target_outranks(const pdxe_gbuf_node_t *cand, const pdxe_gbuf_node_t *best) {
    if (!best) {
        return true;
    }
    bool cand_file = strcmp(cand->label, "File") == 0;
    bool best_file = strcmp(best->label, "File") == 0;
    if (cand_file != best_file) {
        return cand_file;
    }
    int cd = include_path_depth(cand->file_path);
    int bd = include_path_depth(best->file_path);
    if (cd != bd) {
        return cd < bd;
    }
    int by_path = strcmp(cand->file_path, best->file_path);
    if (by_path != 0) {
        return by_path < 0;
    }
    return cand->qualified_name && best->qualified_name &&
           strcmp(cand->qualified_name, best->qualified_name) < 0;
}

static const pdxe_gbuf_node_t *resolve_exact_file_node(const pdxe_pipeline_ctx_t *ctx,
                                                      const char *file_path,
                                                      const char *source_file_qn) {
    if (!ctx || !file_path || !file_path[0]) {
        return NULL;
    }

    const char *leaf = path_leaf(file_path);
    if (!leaf || !leaf[0]) {
        return NULL;
    }

    char stem[PKGMAP_PATH_BUF];
    snprintf(stem, sizeof(stem), "%s", leaf);
    char *last_dot = strrchr(stem, '.');
    if (last_dot && last_dot != stem) {
        *last_dot = '\0';
    }

    const char *names[2] = {stem[0] ? stem : NULL, leaf};
    const pdxe_gbuf_node_t *best = NULL;
    for (int ni = 0; ni < 2; ni++) {
        const char *name = names[ni];
        if (!name || !name[0]) {
            continue;
        }

        const pdxe_gbuf_node_t **hits = NULL;
        int hit_count = 0;
        if (pdxe_gbuf_find_by_name(ctx->gbuf, name, &hits, &hit_count) != 0 || !hits) {
            continue;
        }

        for (int i = 0; i < hit_count; i++) {
            const pdxe_gbuf_node_t *cand = hits[i];
            if (!cand || !cand->file_path) {
                continue;
            }

            /* Tie-breaker: Ensure the candidate's actual file_path ends with the
             * specific include path requested (e.g., 'a/util.h' instead of just 'util.h'). */
            if (!ends_with(cand->file_path, file_path)) {
                continue;
            }

            if (!cand->label || !import_targetable_label(cand->label)) {
                continue;
            }
            if (source_file_qn && cand->qualified_name &&
                strcmp(cand->qualified_name, source_file_qn) == 0) {
                continue;
            }
            if (include_target_outranks(cand, best)) {
                best = cand;
            }
        }
    }
    return best;
}

static const pdxe_gbuf_node_t *resolve_header_include(const pdxe_pipeline_ctx_t *ctx,
                                                     const char *source_rel,
                                                     const char *source_file_qn,
                                                     const char *module_path) {
    if (!is_c_family_source(source_rel) || !is_header_include(module_path)) {
        return NULL;
    }

    const char *base = module_path;
    if (base[0] == '.' && base[1] == '/') {
        base += 2;
    }

    const pdxe_gbuf_node_t *exact = resolve_exact_file_node(ctx, base, source_file_qn);
    if (exact) {
        return exact;
    }

    if (source_rel && source_rel[0]) {
        char *dir = path_dirname(source_rel);
        if (dir) {
            char candidate[PKGMAP_PATH_BUF];
            if (dir[0]) {
                snprintf(candidate, sizeof(candidate), "%s/%s", dir, base);
            } else {
                snprintf(candidate, sizeof(candidate), "%s", base);
            }
            free(dir);
            exact = resolve_exact_file_node(ctx, candidate, source_file_qn);
            if (exact) {
                return exact;
            }
        }
    }

    return NULL;
}

/* Resolve a sibling-file import: a bare path/name (no leading "./") that names
 * a file relative to the importer's directory.  This covers build/markup
 * grammars whose import string is a sibling filename or directory rather than a
 * dotted module path:
 *   - SCSS  `@use 'vars'`              → sibling `_vars.scss` (partial underscore)
 *   - Just  `import 'common.just'`     → sibling `common.just`
 *   - BitBake `require mypackage.inc`  → sibling `mypackage.inc`
 *   - Meson `subdir('lib')`            → `lib/meson.build`
 *   - func  `#include "utils.fc"`      → sibling `utils.fc`
 *   - Pony  `use "util"`               → sibling `util.pony`
 * Builds a path relative to source_rel's directory, then looks up the resulting
 * File/Module-node QN (extension is stripped by fqn_module).  Returns a borrowed
 * node or NULL.  Several filename conventions are tried in turn. */
static const pdxe_gbuf_node_t *resolve_sibling_file(const pdxe_pipeline_ctx_t *ctx,
                                                   const char *source_rel,
                                                   const char *source_file_qn,
                                                   const char *module_path) {
    if (!module_path || !module_path[0]) {
        return NULL;
    }
    /* Directory of the importing file (empty for repo-root files). */
    char *dir = path_dirname(source_rel ? source_rel : "");
    if (!dir) {
        return NULL;
    }

    /* Candidate relative paths, in priority order. */
    char cands[5][PKGMAP_PATH_BUF];
    int ncand = 0;
    const char *base = module_path;
    /* Skip a leading "./". */
    if (base[0] == '.' && base[1] == '/') {
        base += 2;
    }
    /* 1. Direct sibling: dir/<module_path>. */
    snprintf(cands[ncand++], PKGMAP_PATH_BUF, "%s%s%s", dir, dir[0] ? "/" : "", base);
    /* 2. SCSS partial: dir/[subdir/]_<basename>.scss (underscore-prefixed). */
    {
        const char *slash = strrchr(base, '/');
        const char *bn = slash ? slash + 1 : base;
        char *dpart = slash ? pdxe_counted_strndup(base, (size_t)(slash - base)) : pdxe_counted_strdup("");
        if (dpart && bn[0] != '_') {
            snprintf(cands[ncand++], PKGMAP_PATH_BUF, "%s%s%s%s_%s.scss", dir, dir[0] ? "/" : "",
                     dpart[0] ? dpart : "", dpart[0] ? "/" : "", bn);
        }
        free(dpart);
    }
    /* 3. Meson subdir: dir/<module_path>/meson.build. */
    snprintf(cands[ncand++], PKGMAP_PATH_BUF, "%s%s%s/meson.build", dir, dir[0] ? "/" : "", base);
    /* 4. Basename sibling: dir/<basename(module_path)>.  Covers include paths
     *    that carry a non-relative prefix (Hyprlang `source = ~/.config/.../x.conf`,
     *    absolute include paths) but reference a file sitting beside the importer. */
    {
        const char *slash = strrchr(base, '/');
        if (slash && slash[1]) {
            snprintf(cands[ncand++], PKGMAP_PATH_BUF, "%s%s%s", dir, dir[0] ? "/" : "", slash + 1);
        }
    }

    const pdxe_gbuf_node_t *found = NULL;
    for (int i = 0; i < ncand; i++) {
        char *qn = pdxe_pipeline_fqn_module(ctx->project_name, cands[i]);
        if (!qn) {
            continue;
        }
        const pdxe_gbuf_node_t *n = pdxe_gbuf_find_by_qn(ctx->gbuf, qn);
        free(qn);
        if (n && import_targetable_label(n->label) &&
            (!source_file_qn || !n->qualified_name ||
             strcmp(n->qualified_name, source_file_qn) != 0)) {
            found = n;
            break;
        }
    }
    free(dir);
    return found;
}

const pdxe_gbuf_node_t *pdxe_pipeline_resolve_import_node(const pdxe_pipeline_ctx_t *ctx,
                                                        const char *source_rel,
                                                        const char *source_file_qn,
                                                        const PDXEImport *imp,
                                                        PDXEHashTable *namespace_map) {
    if (!ctx || !imp || !imp->module_path) {
        return NULL;
    }

    /* Prefer exact header-file nodes for C/C++ includes so same-stem source or
     * module nodes do not steal the edge target. */
    const pdxe_gbuf_node_t *header_target =
        resolve_header_include(ctx, source_rel, source_file_qn, imp->module_path);
    if (header_target) {
        return header_target;
    }

    /* Strategy 1: module-path resolution → existing node (Python/TS/Go).
     * No label filter here: directory-module languages (Go/Java packages)
     * legitimately resolve straight to a Folder node -- that's the intended,
     * correct import target, not a collision. The Folder-collision problem
     * (#767) only shows up downstream, in Strategy 4's retry-with-truncated-
     * path loop, which re-enters resolve_module with a DIFFERENT, shortened
     * string that the original import never named. */
    char *target_qn = pdxe_pipeline_resolve_module(ctx, source_rel, imp->module_path);
    const pdxe_gbuf_node_t *target = target_qn ? pdxe_gbuf_find_by_qn(ctx->gbuf, target_qn) : NULL;
    free(target_qn);
    if (target) {
        /* Python/TS from-import of a member: module_path is often
         * "pkg.mod.symbol" while resolve_module lands on the Module node
         * "pkg.mod". Prefer the member Function/Class when it exists —
         * required for `from M import f as g` so CALLS can import_map to f
         * (local_name g ≠ f). */
        if (target->label &&
            (strcmp(target->label, "Module") == 0 || strcmp(target->label, "File") == 0)) {
            char symbuf[256];
            const char *sym = import_candidate_symbol(imp->module_path, symbuf, sizeof(symbuf));
            if (sym && sym[0] && strcmp(sym, "*") != 0) {
                const char *mod_tail =
                    import_last_segment(target->qualified_name ? target->qualified_name : "");
                if (!mod_tail || strcmp(mod_tail, sym) != 0) {
                    char member_qn[PDXE_SZ_512];
                    snprintf(member_qn, sizeof(member_qn), "%s.%s", target->qualified_name, sym);
                    const pdxe_gbuf_node_t *member = pdxe_gbuf_find_by_qn(ctx->gbuf, member_qn);
                    if (member && import_targetable_label(member->label) &&
                        strcmp(member->label, "Module") != 0 &&
                        strcmp(member->label, "File") != 0) {
                        return member;
                    }
                }
            }
        }
        return target;
    }

    /* Name-guess fallbacks below (Strategy 1b sibling-file, Strategy 3
     * symbol-name) are gated per importing-file language — see
     * pdxe_import_symbol_fallback_allowed (#1934). */
    const char *src_base = source_rel ? source_rel : "";
    for (const char *pb = src_base; *pb; pb++) {
        if (*pb == '/' || *pb == '\\') {
            src_base = pb + SKIP_ONE;
        }
    }
    const bool symbol_fallback_allowed =
        pdxe_import_symbol_fallback_allowed(pdxe_language_for_filename(src_base));

    /* Strategy 1b: sibling-file resolution for build/markup grammars whose
     * import string is a sibling filename or directory (SCSS partials, Just/
     * BitBake/func includes, Meson subdir, Pony use). Its label filter admits
     * symbols too, so for Go it re-creates the Strategy-3 bug one directory
     * closer (`os/exec` → a same-package exec() method) — gated the same. */
    if (symbol_fallback_allowed) {
        const pdxe_gbuf_node_t *sib =
            resolve_sibling_file(ctx, source_rel, source_file_qn, imp->module_path);
        if (sib) {
            return sib;
        }
    }

    /* Strategy 2: namespace map.  `using App.Utils`, `import com.example.Foo`,
     * `use App\Utils\Helper` name a NAMESPACE (or a member of it) that the
     * path-based QN cannot express.  Try the full module path and progressively
     * shorter prefixes (dropping the trailing member segment) so both a bare
     * namespace import and a member import resolve to the declaring file. */
    if (namespace_map) {
        /* Normalize separators to '.' for namespace keys (PHP uses '\\').
         * Strip decorations first so `com.example.Util as U`, `crate::ops::*`
         * and `App\Utils\{A, B}` reduce to a clean dotted path. */
        char norm[1024];
        snprintf(norm, sizeof(norm), "%s", imp->module_path);
        char *brace = strchr(norm, '{');
        if (brace) {
            *brace = '\0'; /* drop the group; the prefix is the namespace */
        }
        char *as = strstr(norm, " as ");
        if (as) {
            *as = '\0';
        }
        for (char *p = norm; *p; p++) {
            if (*p == '\\' || *p == ':' || *p == '/') {
                *p = '.';
            }
        }
        /* Strip trailing glob/separator noise. */
        size_t nl = strlen(norm);
        while (nl > 0 && (norm[nl - 1] == '*' || norm[nl - 1] == '.' || norm[nl - 1] == ' ')) {
            norm[--nl] = '\0';
        }
        /* Collapse any "::"-induced empty segments ("a..b" → "a.b"). */
        for (;;) {
            char *dd = strstr(norm, "..");
            if (!dd) {
                break;
            }
            memmove(dd, dd + 1, strlen(dd + 1) + 1);
        }
        for (;;) {
            /* The map value is a '\n'-delimited list of __file__ QNs declaring
             * this namespace. Return the FIRST that is not the importing file,
             * so a same-package wildcard import (`import com.example.*` from a
             * file that is itself in com.example) resolves to a sibling — and
             * deterministically, independent of file-iteration order across
             * platforms (amd64 vs arm64). */
            const char *list = (const char *)pdxe_ht_get(namespace_map, norm);
            for (const char *seg = list; seg && *seg;) {
                const char *eol = strchr(seg, '\n');
                size_t len = eol ? (size_t)(eol - seg) : strlen(seg);
                char qbuf[1024];
                if (len > 0 && len < sizeof(qbuf)) {
                    memcpy(qbuf, seg, len);
                    qbuf[len] = '\0';
                    const pdxe_gbuf_node_t *n = pdxe_gbuf_find_by_qn(ctx->gbuf, qbuf);
                    if (n && (!source_file_qn || strcmp(n->qualified_name, source_file_qn) != 0)) {
                        return n;
                    }
                }
                seg = eol ? eol + 1 : NULL;
            }
            char *dot = strrchr(norm, '.');
            if (!dot) {
                break;
            }
            *dot = '\0';
        }
    }

    /* Strategy 3: symbol-name fallback.  Derive a representative imported
     * symbol (handling alias / glob / grouped forms) and match it against an
     * in-graph definition of the same simple name in another file
     * (Rust `helper`, Java `Util`, Kotlin grouped, ...).
     * Gated per importing-file language, like Strategy 1b above (#1934). */
    char symbuf[256];
    /* Prefer the clean candidate from the module path; the local_name may be an
     * alias (Rust `as h`, Kotlin `as U`) that names no real symbol. */
    const char *seg = import_candidate_symbol(imp->module_path, symbuf, sizeof(symbuf));
    if (!seg && imp->local_name && imp->local_name[0] && strcmp(imp->local_name, "*") != 0) {
        seg = imp->local_name;
    }
    /* Build the candidate symbol list: the derived symbol plus, when the import
     * is a dotted member path like `com.example.Config.DEFAULT`, the enclosing
     * type segments (`Config`).  This resolves object/class-member imports to
     * the declaring type when the leaf member isn't an importable node. */
    const char *cands[8];
    int ncands = 0;
    if (seg && seg[0] && strcmp(seg, "*") != 0) {
        cands[ncands++] = seg;
    }
    {
        /* Strip decorations, normalize separators to '.', and collect the
         * trailing path segments (last first) as fallback candidates. */
        char mp[1024];
        snprintf(mp, sizeof(mp), "%s", imp->module_path);
        char *br = strchr(mp, '{');
        if (br) {
            *br = '\0';
        }
        char *as3 = strstr(mp, " as ");
        if (as3) {
            *as3 = '\0';
        }
        for (char *p = mp; *p; p++) {
            if (*p == '\\' || *p == ':' || *p == '/') {
                *p = '.';
            }
        }
        /* Walk segments from the end. */
        char *end = mp + strlen(mp);
        while (end > mp && ncands < 8) {
            char *dot = NULL;
            for (char *p = end - 1; p >= mp; p--) {
                if (*p == '.') {
                    dot = p;
                    break;
                }
            }
            const char *s = dot ? dot + 1 : mp;
            if (s[0] && strcmp(s, "*") != 0) {
                bool dup = false;
                for (int k = 0; k < ncands; k++) {
                    if (strcmp(cands[k], s) == 0) {
                        dup = true;
                        break;
                    }
                }
                if (!dup) {
                    cands[ncands++] = s;
                }
            }
            if (!dot) {
                break;
            }
            *dot = '\0';
            end = dot;
        }
        for (int ci = 0; symbol_fallback_allowed && ci < ncands; ci++) {
            const pdxe_gbuf_node_t **hits = NULL;
            int n = 0;
            if (pdxe_gbuf_find_by_name(ctx->gbuf, cands[ci], &hits, &n) == 0 && hits) {
                /* Deterministic winner: hits[] is in node-registration order,
                 * which under parallel extraction varies run to run — taking
                 * the FIRST targetable hit made the same import resolve to
                 * different same-named nodes across runs (xfs: 360 flickering
                 * IMPORTS diff lines between two MT runs). Pick the candidate
                 * with the lexicographically smallest qualified name instead:
                 * stable, content-derived, identical for ST and MT. */
                const pdxe_gbuf_node_t *best = NULL;
                for (int i = 0; i < n; i++) {
                    const pdxe_gbuf_node_t *cand = hits[i];
                    if (!cand || !import_targetable_label(cand->label)) {
                        continue;
                    }
                    if (source_file_qn && cand->qualified_name &&
                        strcmp(cand->qualified_name, source_file_qn) == 0) {
                        continue; /* self */
                    }
                    if (!best || (cand->qualified_name && best->qualified_name &&
                                  strcmp(cand->qualified_name, best->qualified_name) < 0)) {
                        best = cand;
                    }
                }
                if (best) {
                    return best;
                }
            }
        }
    }

    /* Strategy 4: crate-relative module path → File/Module node.  Rust glob
     * `use crate::ops::*` names a module, not a symbol; strip the glob and the
     * `crate::`/`self::`/`super::` prefix, convert `::`→`/`, then resolve the
     * remaining path (and successive prefixes) to a Module/File node. */
    {
        char mp[1024];
        snprintf(mp, sizeof(mp), "%s", imp->module_path);
        char *brace = strchr(mp, '{');
        if (brace) {
            *brace = '\0';
        }
        char *as2 = strstr(mp, " as ");
        if (as2) {
            *as2 = '\0';
        }
        /* Convert "::" → "/" (drop the doubled colon cleanly). */
        char clean[1024] = ""; /* first byte defined even on the zero-write path */
        size_t ci = 0;
        for (const char *p = mp; *p && ci + 1 < sizeof(clean); p++) {
            if (*p == ':') {
                if (ci > 0 && clean[ci - 1] == '/') {
                    continue; /* collapse "::" → single "/" */
                }
                clean[ci++] = '/';
            } else if (*p == '*' || *p == ' ') {
                continue;
            } else {
                clean[ci++] = *p;
            }
        }
        clean[ci] = '\0';
        /* Drop trailing slashes. */
        while (ci > 0 && clean[ci - 1] == '/') {
            clean[--ci] = '\0';
        }
        /* Strip a leading crate-relative prefix. */
        const char *body = clean;
        static const char *prefixes[] = {"crate/", "self/", "super/", NULL};
        for (const char **pf = prefixes; *pf; pf++) {
            size_t pl = strlen(*pf);
            if (strncmp(body, *pf, pl) == 0) {
                body += pl;
                break;
            }
        }
        if (body[0]) {
            char work[1024];
            snprintf(work, sizeof(work), "%s", body);
            for (;;) {
                char *rqn = pdxe_pipeline_resolve_module(ctx, source_rel, work);
                const pdxe_gbuf_node_t *n = rqn ? pdxe_gbuf_find_by_qn(ctx->gbuf, rqn) : NULL;
                free(rqn);
                if (n && import_targetable_label(n->label) &&
                    (!source_file_qn || !n->qualified_name ||
                     strcmp(n->qualified_name, source_file_qn) != 0)) {
                    return n;
                }
                char *sl = strrchr(work, '/');
                if (!sl) {
                    break;
                }
                *sl = '\0';
            }
        }
    }

    return NULL;
}

/* The namespace names themselves, so a caller that has parked some results on
 * disk can still contribute their namespaces (see
 * pdxe_result_spill_namespace). A file missing from this map does not fail to
 * resolve -- it resolves DIFFERENTLY, through the looser fallback, which is why
 * an incomplete map changed edge counts in both directions rather than only
 * losing edges. */
PDXEHashTable *pdxe_pipeline_namespace_map_build_names(const char *project_name,
                                                     const char *const *namespaces,
                                                     const char *const *rels, int count) {
    PDXEHashTable *map = NULL;
    for (int i = 0; i < count; i++) {
        const char *namespace_name = namespaces[i];
        if (!namespace_name || !namespace_name[0] || !rels[i]) {
            continue;
        }
        if (!map) {
            map = pdxe_ht_create(PDXE_SZ_64);
            if (!map) {
                return NULL;
            }
        }
        char *file_qn = pdxe_pipeline_fqn_compute(project_name, rels[i], "__file__");
        if (!file_qn) {
            continue;
        }
        /* Normalize the namespace key to dot-separated form so it matches the
         * dot-normalized lookups in pdxe_pipeline_resolve_import_node (PHP uses
         * '\\', some grammars '::' or '/'). */
        char *key = pdxe_counted_strdup(namespace_name);
        if (!key) {
            free(file_qn);
            continue;
        }
        for (char *p = key; *p; p++) {
            if (*p == '\\' || *p == ':' || *p == '/') {
                *p = '.';
            }
        }
        /* Store ALL files declaring a namespace as a '\n'-delimited list so the
         * resolver can pick a non-importer sibling (see resolve loop). The hash
         * table does not copy keys, so the strdup'd key is owned by the map and
         * freed in ns_map_free_entry. */
        if (!pdxe_ht_has(map, key)) {
            pdxe_ht_set(map, key, file_qn); /* map owns key + file_qn */
        } else {
            /* Append to the existing list. Re-key with the STORED key pointer
             * (not our fresh strdup) so the map's key pointer never changes —
             * otherwise Verstable would adopt the new key and our free(key)
             * below would free the live key (use-after-free). */
            const char *stored_key = pdxe_ht_get_key(map, key);
            const char *cur = (const char *)pdxe_ht_get(map, key);
            char *combined = NULL;
            if (stored_key && cur) {
                size_t need = strlen(cur) + 1 + strlen(file_qn) + 1;
                combined = pdxe_counted_malloc(need);
                if (combined) {
                    snprintf(combined, need, "%s\n%s", cur, file_qn);
                    void *prev = pdxe_ht_set(map, stored_key, combined);
                    free(prev); /* old value string */
                }
            }
            free(key);     /* our fresh strdup — never stored */
            free(file_qn); /* content copied into combined */
        }
    }
    return map;
}

/* Convenience for callers whose results are all in memory (the sequential
 * definitions pass). A caller that can SPILL must use the _names variant and
 * fill the parked slots from pdxe_result_spill_namespace, or its map silently
 * loses those files. */
PDXEHashTable *pdxe_pipeline_namespace_map_build(const char *project_name,
                                               PDXEFileResult *const *results,
                                               const char *const *rels, int count) {
    const char **names = pdxe_calloc(PDXE_MEM_CLASS_OTHER, (size_t)count * sizeof(char *));
    if (!names) {
        return NULL;
    }
    for (int i = 0; i < count; i++) {
        names[i] = results[i] ? results[i]->namespace_name : NULL;
    }
    PDXEHashTable *map = pdxe_pipeline_namespace_map_build_names(project_name, names, rels, count);
    pdxe_free(PDXE_MEM_CLASS_OTHER, names);
    return map;
}

static void ns_map_free_entry(const char *key, void *value, void *ud) {
    (void)ud;
    free((void *)key); /* strdup'd in pdxe_pipeline_namespace_map_build */
    free(value);
}

void pdxe_pipeline_namespace_map_free(PDXEHashTable *map) {
    if (!map) {
        return;
    }
    pdxe_ht_foreach(map, ns_map_free_entry, NULL);
    pdxe_ht_free(map);
}
