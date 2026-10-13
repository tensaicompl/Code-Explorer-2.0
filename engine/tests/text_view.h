/*
 * Text and qualified names as the interface gives them, spelled out for a test (issue
 * 90, decision 61).
 *
 * A pdxe_text is a range of the source or owned text, and a definition's qualified
 * name is given by its relation to an earlier one's; a test reads them as strings.
 * Each function returns a NUL-terminated string the test does not free: the last
 * TV_RING are kept in a ring of buffers, reachable until the process exits, so a test
 * may hold several at once (one printf of a few, a comparison of two).
 */

#ifndef PDX_TEST_TEXT_VIEW_H
#define PDX_TEST_TEXT_VIEW_H

#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "pdxe.h"

enum { TV_RING = 16 };

static inline char *tv_buffer(size_t len) {
    static char *ring[TV_RING];
    static size_t caps[TV_RING];
    static unsigned next;
    unsigned slot = next++ % TV_RING;
    if (caps[slot] < len + 1) {
        char *grown = (char *)realloc(ring[slot], len + 1);
        if (!grown) {
            abort();
        }
        ring[slot] = grown;
        caps[slot] = len + 1;
    }
    return ring[slot];
}

/* The text: a range's bytes of `source` (`len` long), owned text as it is, NULL for none.
 * A malformed text, which a test must never see from the engine, aborts. */
static inline const char *tv_text(const pdxe_text *t, const uint8_t *source, size_t len) {
    switch (t->kind) {
    case PDXE_TEXT_NONE:
        return NULL;
    case PDXE_TEXT_OWNED:
        return t->owned;
    case PDXE_TEXT_SOURCE: {
        if (t->start > t->end || t->end > len || !source) {
            abort();
        }
        char *out = tv_buffer(t->end - t->start);
        memcpy(out, source + t->start, t->end - t->start);
        out[t->end - t->start] = '\0';
        return out;
    }
    default:
        abort();
    }
}

/* Whether the text is exactly `s`. */
static inline bool tv_text_is(const pdxe_text *t, const uint8_t *source, size_t len, const char *s) {
    const char *text = tv_text(t, source, len);
    return text && s && strcmp(text, s) == 0;
}

/* Definition `i`'s qualified name, spelled out; NULL when it cannot be. */
static inline const char *tv_qn(const pdxe_file_result *r, uint32_t i) {
    size_t len = 0;
    if (pdxe_qualified_name(r->defs, r->n_defs, i, NULL, 0, &len) != PDXE_OK) {
        return NULL;
    }
    char *out = tv_buffer(len);
    if (pdxe_qualified_name(r->defs, r->n_defs, i, out, len + 1, &len) != PDXE_OK) {
        return NULL;
    }
    return out;
}

#endif
