/* A growable byte buffer: macros, conditional compilation, function pointers. */
#include "buffer.h"

#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if defined(_WIN32)
#define PLATFORM "windows"
#elif defined(__APPLE__)
#define PLATFORM "darwin"
#else
#define PLATFORM "posix"
#endif

#ifndef BUFFER_DEBUG
#define BUFFER_DEBUG 0
#endif

static const char *const NAMES[] = {"zero", "one", "two", "tr\xc3\xa8s", "four"};

static const struct {
    const char *key;
    union value v;
} DEFAULTS[] = {
    {.key = "limit", .v = {.i = 1024}},
    {.key = "ratio", .v = {.d = 0.75}},
    {.key = "name", .v = {.s = "buf\t\"quoted\"\n"}},
};

int buffer_init(buffer_t *b, size_t capacity) {
    if (!b) {
        return -1;
    }
    memset(b, 0, sizeof(*b));
    b->cap = BUFFER_GROW(capacity);
    b->data = malloc(b->cap);
    b->owned = 1;
    return b->data ? 0 : -1;
}

int buffer_append(buffer_t *b, const void *data, size_t len) {
    if (b->frozen) {
        return -1;
    }
    if (b->len + len > b->cap) {
        size_t cap = b->cap;
        while (cap < b->len + len) {
            cap = BUFFER_GROW(cap);
        }
        uint8_t *grown = realloc(b->data, cap);
        if (!grown) {
            return -1;
        }
        b->data = grown;
        b->cap = cap;
    }
    memcpy(b->data + b->len, data, len);
    b->len += len;
#if BUFFER_DEBUG
    LOG_CALL("appended %zu bytes on %s", len, PLATFORM);
#endif
    return 0;
}

int buffer_visit(const buffer_t *b, size_t chunk, buffer_visitor visit, void *user) {
    for (size_t at = 0; at < b->len; at += chunk) {
        size_t n = b->len - at < chunk ? b->len - at : chunk;
        int rc = visit(b->data + at, n, user);
        if (rc != 0) {
            return rc;
        }
    }
    return 0;
}

void buffer_free(buffer_t *b) {
    if (b && b->owned) {
        free(b->data);
    }
}

void buffer_log(const char *file, int line, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    fprintf(stderr, "%s:%d: ", file, line);
    vfprintf(stderr, fmt, ap);
    va_end(ap);
}

static int count_bytes(const uint8_t *chunk, size_t len, void *user) {
    (void)chunk;
    *(size_t *)user += len;
    return 0;
}

size_t buffer_demo(void) {
    buffer_t b;
    size_t seen = 0;
    if (buffer_init(&b, ARRAY_LEN(NAMES)) != 0) {
        return 0;
    }
    for (size_t i = 0; i < ARRAY_LEN(NAMES); i++) {
        buffer_append(&b, NAMES[i], strlen(NAMES[i]));
    }
    buffer_visit(&b, 4, count_bytes, &seen);
    buffer_free(&b);
    return seen + (size_t)DEFAULTS[0].v.i;
}
