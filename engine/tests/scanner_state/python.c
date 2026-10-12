/* The Python grammar's scanner, compiled into scanner_state with its entry points
 * renamed (issue 91): the test drives the scanner's own delimiter and indentation
 * stacks, their ceilings and its serializer. */
#define tree_sitter_python_external_scanner_create unit_py_create_
#define tree_sitter_python_external_scanner_destroy unit_py_destroy_
#define tree_sitter_python_external_scanner_scan unit_py_scan_
#define tree_sitter_python_external_scanner_serialize unit_py_serialize_
#define tree_sitter_python_external_scanner_deserialize unit_py_deserialize_
#include "../../grammars/python/scanner.c"

#include "hooks.h"

size_t unit_py_fixed_bytes(void) { return PY_SER_FIXED_BYTES; }
size_t unit_py_indent_bytes(void) { return PY_SER_INDENT_BYTES; }
void *unit_py_new(void) { return unit_py_create_(); }
void unit_py_free(void *scanner) { unit_py_destroy_(scanner); }
unsigned unit_py_serialize(void *scanner, char *buffer) { return unit_py_serialize_(scanner, buffer); }
void unit_py_deserialize(void *scanner, const char *buffer, unsigned length) {
    unit_py_deserialize_(scanner, buffer, length);
}
size_t unit_py_delimiters(void *scanner) { return ((Scanner *)scanner)->delimiters.size; }
size_t unit_py_indents(void *scanner) { return ((Scanner *)scanner)->indents.size - 1; }

bool unit_py_push_delimiter(void *scanner, bool forced) {
    Scanner *s = (Scanner *)scanner;
    if (!forced && !room_for_delimiter(s)) {
        return false;
    }
    Delimiter delimiter = new_delimiter();
    set_end_character(&delimiter, s->delimiters.size % 2 ? '\'' : '"');
    array_push(&s->delimiters, delimiter);
    return true;
}

bool unit_py_push_indent(void *scanner, bool forced) {
    Scanner *s = (Scanner *)scanner;
    if (!forced && !room_for_indent(s)) {
        return false;
    }
    array_push(&s->indents, (uint16_t)(s->indents.size * 3));
    return true;
}
