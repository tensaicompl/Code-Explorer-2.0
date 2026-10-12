/* The Kotlin grammar's scanner, compiled into scanner_state with its entry points
 * renamed (issue 91): the test drives the scanner's own delimiter stack, its string
 * start and its serializer. */
#define tree_sitter_kotlin_external_scanner_create unit_kt_create_
#define tree_sitter_kotlin_external_scanner_destroy unit_kt_destroy_
#define tree_sitter_kotlin_external_scanner_scan unit_kt_scan_
#define tree_sitter_kotlin_external_scanner_serialize unit_kt_serialize_
#define tree_sitter_kotlin_external_scanner_deserialize unit_kt_deserialize_
#include "../../grammars/kotlin/scanner.c"

#include "hooks.h"

size_t unit_kt_max_delimiters(void) { return TREE_SITTER_SERIALIZATION_BUFFER_SIZE; }
void *unit_kt_new(void) { return unit_kt_create_(); }
void unit_kt_free(void *scanner) { unit_kt_destroy_(scanner); }
unsigned unit_kt_serialize(void *scanner, char *buffer) { return unit_kt_serialize_(scanner, buffer); }
void unit_kt_deserialize(void *scanner, const char *buffer, unsigned length) {
    unit_kt_deserialize_(scanner, buffer, length);
}
size_t unit_kt_delimiters(void *scanner) { return ((Stack *)scanner)->size; }

bool unit_kt_push(void *scanner, bool forced) {
    Stack *stack = (Stack *)scanner;
    bool triple = stack->size % 2 != 0;
    if (!forced) {
        return stack_push(stack, '"', triple);
    }
    array_push(stack, (Delimiter)(triple ? '"' + 1 : '"'));
    return true;
}

void unit_kt_pop(void *scanner) { (void)stack_pop((Stack *)scanner); }

/* A lexer over a NUL-terminated string. */
typedef struct {
    TSLexer lexer;
    const char *text;
    size_t pos;
} UnitLexer;

static void unit_advance(TSLexer *lexer, bool skip) {
    UnitLexer *u = (UnitLexer *)lexer;
    (void)skip;
    if (u->text[u->pos]) {
        u->pos++;
    }
    lexer->lookahead = (unsigned char)u->text[u->pos];
}
static void unit_mark_end(TSLexer *lexer) { (void)lexer; }
static uint32_t unit_get_column(TSLexer *lexer) { return (uint32_t)((UnitLexer *)lexer)->pos; }
static bool unit_at_range_start(const TSLexer *lexer) {
    (void)lexer;
    return false;
}
static bool unit_eof(const TSLexer *lexer) { return ((const UnitLexer *)lexer)->text[((const UnitLexer *)lexer)->pos] == 0; }

bool unit_kt_scan_string_start(void *scanner, const char *text) {
    UnitLexer u;
    memset(&u, 0, sizeof u);
    u.lexer.advance = unit_advance;
    u.lexer.mark_end = unit_mark_end;
    u.lexer.get_column = unit_get_column;
    u.lexer.is_at_included_range_start = unit_at_range_start;
    u.lexer.eof = unit_eof;
    u.text = text;
    u.lexer.lookahead = (unsigned char)text[0];
    return scan_string_start(&u.lexer, (Stack *)scanner);
}
