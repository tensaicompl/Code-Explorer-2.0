#include "scope.h"
#include <string.h>

PDXEScope* pdxe_scope_push(PDXEArena* a, PDXEScope* current) {
    PDXEScope* scope = (PDXEScope*)pdxe_arena_alloc(a, sizeof(PDXEScope));
    if (!scope) {
        return current;
    }
    memset(scope, 0, sizeof(PDXEScope));
    scope->parent = current;
    scope->arena = a;
    return scope;
}

PDXEScope* pdxe_scope_pop(PDXEScope* scope) {
    if (!scope) {
        return NULL;
    }
    return scope->parent;
}

static PDXEScopeChunk* alloc_chunk(PDXEScope* scope) {
    if (!scope->arena) {
        return NULL;
    }
    PDXEScopeChunk* c = (PDXEScopeChunk*)pdxe_arena_alloc(scope->arena, sizeof(PDXEScopeChunk));
    if (!c) {
        return NULL;
    }
    memset(c, 0, sizeof(PDXEScopeChunk));
    c->next = scope->chunks;
    scope->chunks = c;
    return c;
}

/* Returns false when the binding could NOT be recorded in THIS frame.
 *
 * The failure that matters is arena exhaustion in alloc_chunk: the old void
 * form returned silently, so a caller that then consulted the scope CHAIN saw
 * the parent's binding for the same name and concluded the child had been
 * bound. For callable-value proof that is a fabricated identity -- the shadow
 * never took effect, yet the parent's callable looks like the child's. Callers
 * needing that distinction must use the checked form and consult the LOCAL
 * result, not a chain lookup. */
static bool pdxe_scope_bind_value(PDXEScope *scope, const char *name, const PDXEType *type,
                                 const char *callable_qn) {
    if (!scope || !name) {
        return false;
    }
    for (PDXEScopeChunk* c = scope->chunks; c != NULL; c = c->next) {
        for (int i = 0; i < c->used; i++) {
            if (c->bindings[i].name && strcmp(c->bindings[i].name, name) == 0) {
                c->bindings[i].type = type;
                c->bindings[i].callable_qn = callable_qn;
                return true;
            }
        }
    }
    PDXEScopeChunk* head = scope->chunks;
    if (!head || head->used >= PDXE_SCOPE_CHUNK_BINDINGS) {
        head = alloc_chunk(scope);
        if (!head) {
            return false; /* arena exhausted: the shadow did NOT take effect */
        }
    }
    head->bindings[head->used].name = name;
    head->bindings[head->used].type = type;
    head->bindings[head->used].callable_qn = callable_qn;
    head->used++;
    return true;
}

void pdxe_scope_bind(PDXEScope *scope, const char *name, const PDXEType *type) {
    (void)pdxe_scope_bind_value(scope, name, type, NULL);
}

bool pdxe_scope_bind_checked(PDXEScope *scope, const char *name, const PDXEType *type) {
    return pdxe_scope_bind_value(scope, name, type, NULL);
}

void pdxe_scope_bind_callable(PDXEScope *scope, const char *name, const PDXEType *type,
                             const char *callable_qn) {
    (void)pdxe_scope_bind_value(scope, name, type, callable_qn);
}

bool pdxe_scope_bind_callable_checked(PDXEScope *scope, const char *name, const PDXEType *type,
                                     const char *callable_qn) {
    return pdxe_scope_bind_value(scope, name, type, callable_qn);
}

const PDXEType* pdxe_scope_lookup(const PDXEScope* scope, const char* name) {
    if (!name) {
        return pdxe_type_unknown();
    }
    for (const PDXEScope* s = scope; s != NULL; s = s->parent) {
        for (PDXEScopeChunk* c = s->chunks; c != NULL; c = c->next) {
            for (int i = 0; i < c->used; i++) {
                if (c->bindings[i].name && strcmp(c->bindings[i].name, name) == 0) {
                    return c->bindings[i].type;
                }
            }
        }
    }
    return pdxe_type_unknown();
}

bool pdxe_scope_contains(const PDXEScope *scope, const char *name) {
    if (!name) {
        return false;
    }
    for (const PDXEScope *s = scope; s != NULL; s = s->parent) {
        for (const PDXEScopeChunk *c = s->chunks; c != NULL; c = c->next) {
            for (int i = 0; i < c->used; i++) {
                if (c->bindings[i].name && strcmp(c->bindings[i].name, name) == 0) {
                    return true;
                }
            }
        }
    }
    return false;
}

const char *pdxe_scope_lookup_callable(const PDXEScope *scope, const char *name) {
    if (!name) {
        return NULL;
    }
    for (const PDXEScope *s = scope; s != NULL; s = s->parent) {
        for (const PDXEScopeChunk *c = s->chunks; c != NULL; c = c->next) {
            for (int i = 0; i < c->used; i++) {
                if (c->bindings[i].name && strcmp(c->bindings[i].name, name) == 0) {
                    return c->bindings[i].callable_qn;
                }
            }
        }
    }
    return NULL;
}

bool pdxe_scope_update_callable(PDXEScope *scope, const char *name, const char *callable_qn) {
    if (!name) {
        return false;
    }
    for (PDXEScope *s = scope; s != NULL; s = s->parent) {
        for (PDXEScopeChunk *c = s->chunks; c != NULL; c = c->next) {
            for (int i = 0; i < c->used; i++) {
                if (c->bindings[i].name && strcmp(c->bindings[i].name, name) == 0) {
                    c->bindings[i].callable_qn = callable_qn;
                    return true;
                }
            }
        }
    }
    return false;
}
