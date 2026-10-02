/*
 * Work the engine could not do, counted on the thread it happened on. See
 * lost_work.h.
 */

#include "lost_work.h"

#include <stdlib.h>
#include <string.h>

/* Per thread: each thread runs its own projects, one at a time. */
static _Thread_local pdxe_losses_t losses;

void pdxe_lost_allocation(void) {
    losses.allocations++;
}

void pdxe_lost_work(void) {
    losses.work++;
}

pdxe_losses_t pdxe_losses(void) {
    return losses;
}

void *pdxe_counted_malloc(size_t size) {
    void *p = malloc(size);
    if (!p) {
        pdxe_lost_allocation();
    }
    return p;
}

void *pdxe_counted_calloc(size_t count, size_t size) {
    void *p = calloc(count, size);
    if (!p && count && size) {
        pdxe_lost_allocation();
    }
    return p;
}

void *pdxe_counted_realloc(void *block, size_t size) {
    void *p = realloc(block, size);
    if (!p && size) {
        pdxe_lost_allocation();
    }
    return p;
}

char *pdxe_counted_strdup(const char *s) {
    if (!s) {
        return NULL;
    }
    return pdxe_counted_strndup(s, strlen(s));
}

char *pdxe_counted_strndup(const char *s, size_t len) {
    if (!s) {
        return NULL;
    }
    char *copy = (char *)malloc(len + 1);
    if (!copy) {
        pdxe_lost_allocation();
        return NULL;
    }
    memcpy(copy, s, len);
    copy[len] = '\0';
    return copy;
}
