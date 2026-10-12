/*
 * What scanner_state reaches of four grammar scanners (docs/plan/ISSUES.md, issue 91).
 *
 * Each scanner's source is compiled into the test a second time, its entry points
 * renamed so they do not clash with the engine's, so the test drives the scanner's own
 * state and static helpers: its pushes, its ceilings, its serializer and deserializer.
 * `serialize` writes into the buffer it is given, which must be at least
 * unit_serialization_buffer_size() bytes, as the parser's is.
 */
#ifndef PDXE_TEST_SCANNER_STATE_HOOKS_H
#define PDXE_TEST_SCANNER_STATE_HOOKS_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

/* The parser's TREE_SITTER_SERIALIZATION_BUFFER_SIZE, as the scanners see it. */
size_t unit_serialization_buffer_size(void);

/* YAML: indentation entries above the root, a type and a length each. */
size_t unit_yaml_header_bytes(void);
size_t unit_yaml_entry_bytes(void);
size_t unit_yaml_max_entries(void);
void *unit_yaml_new(void);
void unit_yaml_free(void *scanner);
unsigned unit_yaml_serialize(void *scanner, char *buffer);
void unit_yaml_deserialize(void *scanner, const char *buffer, unsigned length);
size_t unit_yaml_entries(void *scanner);
/* Opens one entry as the scanner does (refused past the ceiling), or, `forced`, past it. */
bool unit_yaml_push(void *scanner, bool forced);

/* Markdown: open blocks. */
size_t unit_md_header_bytes(void);
size_t unit_md_block_bytes(void);
size_t unit_md_max_blocks(void);
void *unit_md_new(void);
void unit_md_free(void *scanner);
unsigned unit_md_serialize(void *scanner, char *buffer);
void unit_md_deserialize(void *scanner, const char *buffer, unsigned length);
size_t unit_md_blocks(void *scanner);
bool unit_md_push(void *scanner, bool forced);

/* Kotlin: open string delimiters, one byte each. */
size_t unit_kt_max_delimiters(void);
void *unit_kt_new(void);
void unit_kt_free(void *scanner);
unsigned unit_kt_serialize(void *scanner, char *buffer);
void unit_kt_deserialize(void *scanner, const char *buffer, unsigned length);
size_t unit_kt_delimiters(void *scanner);
bool unit_kt_push(void *scanner, bool forced);
/* Pops one delimiter: the scanner's own stack_pop, on an empty stack too. */
void unit_kt_pop(void *scanner);
/* The scanner's string-start scan over `text`: whether it claims STRING_START. */
bool unit_kt_scan_string_start(void *scanner, const char *text);

/* Python: open string delimiters (one byte each, at most 255) and indentation levels
 * above the first (two bytes each). */
size_t unit_py_fixed_bytes(void);
size_t unit_py_indent_bytes(void);
void *unit_py_new(void);
void unit_py_free(void *scanner);
unsigned unit_py_serialize(void *scanner, char *buffer);
void unit_py_deserialize(void *scanner, const char *buffer, unsigned length);
size_t unit_py_delimiters(void *scanner);
size_t unit_py_indents(void *scanner); /* levels above the first */
bool unit_py_push_delimiter(void *scanner, bool forced);
bool unit_py_push_indent(void *scanner, bool forced);

#endif
