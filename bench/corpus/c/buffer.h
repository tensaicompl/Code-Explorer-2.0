#ifndef CORPUS_BUFFER_H
#define CORPUS_BUFFER_H

#include <stddef.h>
#include <stdint.h>

#define BUFFER_MIN_CAPACITY 16
#define BUFFER_GROW(n) ((n) < BUFFER_MIN_CAPACITY ? BUFFER_MIN_CAPACITY : (n) * 2)
#define ARRAY_LEN(a) (sizeof(a) / sizeof((a)[0]))
#define LOG_CALL(fmt, ...) buffer_log(__FILE__, __LINE__, fmt, ##__VA_ARGS__)

typedef struct buffer {
    uint8_t *data;
    size_t len;
    size_t cap;
    unsigned owned : 1;
    unsigned frozen : 1;
} buffer_t;

typedef int (*buffer_visitor)(const uint8_t *chunk, size_t len, void *user);

union value {
    int64_t i;
    double d;
    const char *s;
};

int buffer_init(buffer_t *b, size_t capacity);
int buffer_append(buffer_t *b, const void *data, size_t len);
int buffer_visit(const buffer_t *b, size_t chunk, buffer_visitor visit, void *user);
void buffer_free(buffer_t *b);
void buffer_log(const char *file, int line, const char *fmt, ...);

#endif /* CORPUS_BUFFER_H */
