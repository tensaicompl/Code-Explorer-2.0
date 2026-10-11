/*
 * The rows the engine differential compares (P2-16, decision 50), printed from one
 * file's extraction result as the cross-file typed pass left it: one JSON object per
 * line, written by the same code for both engines.
 *
 * This header is compiled twice: into tests/differential_dump.c for the vendored
 * engine, and, its names translated back with the vendoring's own rename map by
 * scripts/engine/differential.py, into the observation hook built with the pinned
 * reference engine (tests/differential_hook.c). So it reads only fields both engines'
 * structures have. The fields the patches here added (a site's node-type path and
 * owner, a definition's declaration occurrence and route facts) and the two analyses
 * patch 0002 removes (a definition's fingerprint and structural profile) are never
 * read.
 *
 *     {"c":"file", ...}       one per file: its module, namespace and walk health
 *     {"c":"definition", ...} one per definition
 *     {"c":"call", ...}       one per call extraction recorded, in order
 *     {"c":"reference", ...}  one per usage that is, or may be, a callable reference
 *     {"c":"import", ...}     one per import
 *     {"c":"typed", ...}      one per answer the typed passes wrote, resolved or not
 *
 * Strings are written byte for byte: printable ASCII as itself with '"' and '\'
 * escaped, every other byte as \u00XX of its value, so a file whose text is not valid
 * UTF-8 still gives a line any JSON reader accepts, and equal bytes give equal lines.
 * NULL is null. A float is written with nine significant digits, which give back the
 * same float, and a value that is not finite as the string "nonfinite", which the
 * harness refuses. Qualified names are written as the engine holds them, project
 * name included; the harness removes it.
 */

#ifndef PDX_DIFFERENTIAL_ROWS_H
#define PDX_DIFFERENTIAL_ROWS_H

#include <math.h>
#include <stdio.h>

static void dr_str(FILE *o, const char *s) {
    if (!s) {
        fputs("null", o);
        return;
    }
    fputc('"', o);
    for (const unsigned char *p = (const unsigned char *)s; *p; p++) {
        if (*p == '"' || *p == '\\') {
            fputc('\\', o);
            fputc(*p, o);
        } else if (*p >= 0x20 && *p < 0x7f) {
            fputc(*p, o);
        } else {
            fprintf(o, "\\u%04x", *p);
        }
    }
    fputc('"', o);
}

/* A NULL-terminated array, or null. */
static void dr_strv(FILE *o, const char **v) {
    if (!v) {
        fputs("null", o);
        return;
    }
    fputc('[', o);
    for (int i = 0; v[i]; i++) {
        if (i) {
            fputc(',', o);
        }
        dr_str(o, v[i]);
    }
    fputc(']', o);
}

/* A counted array, or null when it has no storage. */
static void dr_strn(FILE *o, const char **v, int n) {
    if (!v) {
        fputs("null", o);
        return;
    }
    fputc('[', o);
    for (int i = 0; i < n; i++) {
        if (i) {
            fputc(',', o);
        }
        dr_str(o, v[i]);
    }
    fputc(']', o);
}

static void dr_float(FILE *o, double v) {
    if (!isfinite(v)) {
        fputs("\"nonfinite\"", o);
        return;
    }
    fprintf(o, "%.9g", v);
}

static void dr_key(FILE *o, const char *key) {
    fprintf(o, ",\"%s\":", key);
}

static void dr_kstr(FILE *o, const char *key, const char *s) {
    dr_key(o, key);
    dr_str(o, s);
}

static void dr_kint(FILE *o, const char *key, long long v) {
    dr_key(o, key);
    fprintf(o, "%lld", v);
}

static void dr_kbool(FILE *o, const char *key, int v) {
    dr_key(o, key);
    fputs(v ? "true" : "false", o);
}

static void dr_begin(FILE *o, const char *category, const char *rel_path) {
    fprintf(o, "{\"c\":\"%s\"", category);
    dr_kstr(o, "file", rel_path);
}

static void dr_definition(FILE *o, const char *rel, const PDXEDefinition *d) {
    dr_begin(o, "definition", rel);
    dr_kstr(o, "label", d->label);
    dr_kstr(o, "name", d->name);
    dr_kstr(o, "qn", d->qualified_name);
    dr_kstr(o, "def_file", d->file_path);
    dr_kint(o, "start_line", d->start_line);
    dr_kint(o, "end_line", d->end_line);
    dr_kstr(o, "signature", d->signature);
    dr_kstr(o, "return_type", d->return_type);
    dr_kstr(o, "receiver", d->receiver);
    dr_kstr(o, "docstring", d->docstring);
    dr_kstr(o, "parent_class", d->parent_class);
    dr_key(o, "decorators");
    dr_strv(o, d->decorators);
    dr_key(o, "base_classes");
    dr_strv(o, d->base_classes);
    dr_key(o, "param_names");
    dr_strv(o, d->param_names);
    dr_key(o, "param_types");
    dr_strv(o, d->param_types);
    dr_key(o, "signature_param_types");
    dr_strn(o, d->signature_param_types, d->signature_param_count);
    dr_kint(o, "signature_param_count", d->signature_param_count);
    dr_key(o, "return_types");
    dr_strv(o, d->return_types);
    dr_kstr(o, "route_path", d->route_path);
    dr_kstr(o, "route_method", d->route_method);
    dr_kint(o, "complexity", d->complexity);
    dr_kint(o, "cognitive", d->cognitive);
    dr_kint(o, "loop_count", d->loop_count);
    dr_kint(o, "loop_depth", d->loop_depth);
    dr_kbool(o, "is_recursive", d->is_recursive);
    dr_kint(o, "param_count", d->param_count);
    dr_kint(o, "max_access_depth", d->max_access_depth);
    dr_kint(o, "linear_scan_in_loop", d->linear_scan_in_loop);
    dr_kint(o, "alloc_in_loop", d->alloc_in_loop);
    dr_kbool(o, "recursion_in_loop", d->recursion_in_loop);
    dr_kbool(o, "unguarded_recursion", d->unguarded_recursion);
    dr_kint(o, "lines", d->lines);
    dr_kbool(o, "is_exported", d->is_exported);
    dr_kbool(o, "is_abstract", d->is_abstract);
    dr_kbool(o, "is_test", d->is_test);
    dr_kbool(o, "is_entry_point", d->is_entry_point);
    dr_kstr(o, "body_tokens", d->body_tokens);
    dr_kstr(o, "impl_trait", d->impl_trait);
    fputs("}\n", o);
}

static void dr_call(FILE *o, const char *rel, const PDXECall *c) {
    dr_begin(o, "call", rel);
    dr_kstr(o, "caller", c->enclosing_func_qn);
    dr_kstr(o, "callee", c->callee_name);
    dr_kint(o, "line", c->start_line);
    dr_kint(o, "start", c->site_start_byte);
    dr_kint(o, "end", c->site_end_byte);
    dr_kint(o, "origin", (int)c->source_origin);
    dr_kstr(o, "first_string_arg", c->first_string_arg);
    dr_kstr(o, "second_arg_name", c->second_arg_name);
    dr_key(o, "args");
    fputc('[', o);
    for (int i = 0; c->args && i < c->arg_count; i++) {
        const PDXECallArg *a = &c->args[i];
        fputs(i ? ",{" : "{", o);
        fputs("\"index\":", o);
        fprintf(o, "%d", a->index);
        dr_kstr(o, "expr", a->expr);
        dr_kstr(o, "value", a->value);
        dr_kstr(o, "keyword", a->keyword);
        fputc('}', o);
    }
    fputc(']', o);
    dr_kint(o, "arg_count", c->arg_count);
    dr_kint(o, "loop_depth", c->loop_depth);
    dr_kint(o, "branch_depth", c->branch_depth);
    dr_kbool(o, "is_method", c->is_method);
    dr_kbool(o, "requires_lsp_resolution", c->requires_lsp_resolution);
    dr_kbool(o, "callee_is_locally_bound", c->callee_is_locally_bound);
    dr_kbool(o, "receiver_is_self_attribute", c->receiver_is_self_attribute);
    fputs("}\n", o);
}

static void dr_reference(FILE *o, const char *rel, const PDXEUsage *u) {
    dr_begin(o, "reference", rel);
    dr_kstr(o, "caller", u->enclosing_func_qn);
    dr_kstr(o, "name", u->ref_name);
    dr_kint(o, "start", u->site_start_byte);
    dr_kint(o, "end", u->site_end_byte);
    dr_kint(o, "origin", (int)u->source_origin);
    dr_kint(o, "usage_kind", (int)u->kind);
    dr_kbool(o, "may_be_call_reference", u->may_be_call_reference);
    dr_kbool(o, "semantic_reference_blocked", u->semantic_reference_blocked);
    dr_kbool(o, "semantic_reference_local_shadow", u->semantic_reference_local_shadow);
    dr_kbool(o, "is_member_access", u->is_member_access);
    fputs("}\n", o);
}

static void dr_import(FILE *o, const char *rel, const PDXEImport *i) {
    dr_begin(o, "import", rel);
    dr_kstr(o, "local_name", i->local_name);
    dr_kstr(o, "module_path", i->module_path);
    fputs("}\n", o);
}

static void dr_typed(FILE *o, const char *rel, const PDXEResolvedCall *t) {
    dr_begin(o, "typed", rel);
    dr_kstr(o, "caller", t->caller_qn);
    dr_kstr(o, "callee", t->callee_qn);
    dr_kstr(o, "strategy", t->strategy);
    dr_key(o, "confidence");
    dr_float(o, t->confidence);
    dr_kstr(o, "reason", t->reason);
    dr_kint(o, "kind", (int)t->kind);
    dr_kint(o, "start", t->site_start_byte);
    dr_kint(o, "end", t->site_end_byte);
    dr_kint(o, "origin", (int)t->source_origin);
    fputs("}\n", o);
}

/* Every row of one file's result. */
static void diff_rows_file(FILE *o, const char *rel, const PDXEFileResult *r) {
    dr_begin(o, "file", rel);
    dr_kstr(o, "module_qn", r->module_qn);
    dr_kstr(o, "namespace_name", r->namespace_name);
    dr_kbool(o, "has_error", r->has_error);
    dr_kbool(o, "is_test_file", r->is_test_file);
    dr_kbool(o, "lsp_skipped", r->lsp_skipped);
    dr_kbool(o, "walk_truncated", r->walk_truncated);
    dr_key(o, "exports");
    dr_strv(o, r->exports);
    fputs("}\n", o);
    for (int i = 0; i < r->defs.count; i++) {
        dr_definition(o, rel, &r->defs.items[i]);
    }
    for (int i = 0; i < r->calls.count; i++) {
        dr_call(o, rel, &r->calls.items[i]);
    }
    for (int i = 0; i < r->usages.count; i++) {
        const PDXEUsage *u = &r->usages.items[i];
        if (u->kind != PDXE_USAGE_VALUE || u->may_be_call_reference) {
            dr_reference(o, rel, u);
        }
    }
    for (int i = 0; i < r->imports.count; i++) {
        dr_import(o, rel, &r->imports.items[i]);
    }
    for (int i = 0; i < r->resolved_calls.count; i++) {
        dr_typed(o, rel, &r->resolved_calls.items[i]);
    }
}

#endif /* PDX_DIFFERENTIAL_ROWS_H */
