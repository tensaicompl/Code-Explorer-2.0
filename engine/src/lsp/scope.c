#include "scope.h"
#include "../lookup_counts.h"
#include <string.h>

static uint64_t scope_name_hash(const char *s) {
    uint64_t h = 1469598103934665603ULL;
    for (; *s; s++) {
        h = (h ^ (unsigned char)*s) * 1099511628211ULL;
    }
    return h;
}

/* The slot of `name` (hashed h) in a frame's index: where it is, or the empty slot it
 * would take. */
static int scope_index_slot(const PDXEScope *frame, const char *name, uint64_t h) {
    int mask = frame->index_cap - 1;
    for (int at = (int)(h & (uint64_t)mask);; at = (at + 1) & mask) {
        const PDXEScopeIndexSlot *slot = &frame->index[at];
        if (!slot->binding || (slot->hash == h && strcmp(slot->binding->name, name) == 0)) {
            return at;
        }
    }
}

PDXEVarBinding *pdxe_scope_frame_find(const PDXEScope *frame, const char *name) {
    if (!frame || !name) {
        return NULL;
    }
    if (frame->index) {
        PDXE_LOOKUP_COUNT(PDXE_LC_REGISTRY_PROBES);
        return frame->index[scope_index_slot(frame, name, scope_name_hash(name))].binding;
    }
    /* Not yet indexed: at most one chunk of bindings. */
    for (PDXEScopeChunk *c = frame->chunks; c != NULL; c = c->next) {
        for (int i = 0; i < c->used; i++) {
            PDXE_LOOKUP_COUNT(PDXE_LC_NAME_COMPARES);
            if (c->bindings[i].name && strcmp(c->bindings[i].name, name) == 0) {
                return &c->bindings[i];
            }
        }
    }
    return NULL;
}

/* Grows the frame's index so it holds one more name, indexing every binding the frame
 * has when it is first built. False when it cannot be allocated. */
static bool scope_index_reserve(PDXEScope *frame) {
    if (frame->count + 1 <= PDXE_SCOPE_CHUNK_BINDINGS) {
        return true; /* small frames are scanned: one chunk at most */
    }
    if (frame->index && 2 * (frame->count + 1) <= frame->index_cap) {
        return true;
    }
    if (frame->index_cap > (1 << 28) || !frame->arena || PDXE_LOOKUP_INDEX_REFUSED("scope")) {
        return false;
    }
    int cap = frame->index_cap ? 2 * frame->index_cap : 4 * PDXE_SCOPE_CHUNK_BINDINGS;
    PDXEScopeIndexSlot *index =
        (PDXEScopeIndexSlot *)pdxe_arena_alloc(frame->arena, (size_t)cap * sizeof(*index));
    if (!index) {
        return false;
    }
    memset(index, 0, (size_t)cap * sizeof(*index));
    PDXEScope grown = *frame;
    grown.index = index;
    grown.index_cap = cap;
    for (PDXEScopeChunk *c = frame->chunks; c != NULL; c = c->next) {
        for (int i = 0; i < c->used; i++) {
            if (c->bindings[i].name) {
                uint64_t h = scope_name_hash(c->bindings[i].name);
                int at = scope_index_slot(&grown, c->bindings[i].name, h);
                index[at].hash = h;
                index[at].binding = &c->bindings[i];
            }
        }
    }
    frame->index = index;
    frame->index_cap = cap;
    return true;
}

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
    PDXEVarBinding *existing = pdxe_scope_frame_find(scope, name);
    if (existing) {
        existing->type = type;
        existing->callable_qn = callable_qn;
        return true;
    }
    /* Issue 92: an index that cannot grow fails the bind, as arena exhaustion does:
     * the frame's index stays complete for every binding it holds. */
    if (!scope_index_reserve(scope)) {
        return false; /* arena exhausted: the shadow did NOT take effect */
    }
    PDXEScopeChunk* head = scope->chunks;
    if (!head || head->used >= PDXE_SCOPE_CHUNK_BINDINGS) {
        head = alloc_chunk(scope);
        if (!head) {
            return false; /* arena exhausted: the shadow did NOT take effect */
        }
    }
    PDXEVarBinding *b = &head->bindings[head->used];
    b->name = name;
    b->type = type;
    b->callable_qn = callable_qn;
    b->written_next = NULL;
    b->written = false;
    head->used++;
    scope->count++;
    if (scope->index) {
        uint64_t h = scope_name_hash(name);
        int at = scope_index_slot(scope, name, h);
        scope->index[at].hash = h;
        scope->index[at].binding = b;
    }
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
        const PDXEVarBinding *b = pdxe_scope_frame_find(s, name);
        if (b) {
            return b->type;
        }
    }
    return pdxe_type_unknown();
}

bool pdxe_scope_contains(const PDXEScope *scope, const char *name) {
    if (!name) {
        return false;
    }
    for (const PDXEScope *s = scope; s != NULL; s = s->parent) {
        if (pdxe_scope_frame_find(s, name)) {
            return true;
        }
    }
    return false;
}

const char *pdxe_scope_lookup_callable(const PDXEScope *scope, const char *name) {
    if (!name) {
        return NULL;
    }
    for (const PDXEScope *s = scope; s != NULL; s = s->parent) {
        const PDXEVarBinding *b = pdxe_scope_frame_find(s, name);
        if (b) {
            return b->callable_qn;
        }
    }
    return NULL;
}

bool pdxe_scope_update_callable(PDXEScope *scope, const char *name, const char *callable_qn) {
    if (!name) {
        return false;
    }
    for (PDXEScope *s = scope; s != NULL; s = s->parent) {
        PDXEVarBinding *b = pdxe_scope_frame_find(s, name);
        if (b) {
            b->callable_qn = callable_qn;
            return true;
        }
    }
    return false;
}
