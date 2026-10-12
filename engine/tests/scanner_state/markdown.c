/* The Markdown grammar's scanner, compiled into scanner_state with its entry points
 * renamed (issue 91): the test drives the scanner's own block stack and serializer. */
#define tree_sitter_markdown_external_scanner_create unit_md_create_
#define tree_sitter_markdown_external_scanner_destroy unit_md_destroy_
#define tree_sitter_markdown_external_scanner_scan unit_md_scan_
#define tree_sitter_markdown_external_scanner_serialize unit_md_serialize_
#define tree_sitter_markdown_external_scanner_deserialize unit_md_deserialize_
#include "../../grammars/markdown/scanner.c"

#include <stdlib.h>

#include "hooks.h"

size_t unit_md_header_bytes(void) { return MD_SER_HEADER_BYTES; }
size_t unit_md_block_bytes(void) { return sizeof(Block); }
size_t unit_md_max_blocks(void) { return MD_MAX_OPEN_BLOCKS; }
void *unit_md_new(void) { return unit_md_create_(); }
void unit_md_free(void *scanner) { unit_md_destroy_(scanner); }
unsigned unit_md_serialize(void *scanner, char *buffer) { return unit_md_serialize_(scanner, buffer); }
void unit_md_deserialize(void *scanner, const char *buffer, unsigned length) {
    unit_md_deserialize_(scanner, buffer, length);
}
size_t unit_md_blocks(void *scanner) { return ((Scanner *)scanner)->open_blocks.size; }

bool unit_md_push(void *scanner, bool forced) {
    Scanner *s = (Scanner *)scanner;
    /* Distinct values, so a round trip that loses or reorders a block shows. */
    Block b = (Block)(s->open_blocks.size % (ANONYMOUS + 1));
    if (!forced) {
        return push_block(s, b);
    }
    if (s->open_blocks.size == s->open_blocks.capacity) {
        size_t capacity = s->open_blocks.capacity ? s->open_blocks.capacity * 2 : 8;
        Block *grown = (Block *)realloc(s->open_blocks.items, capacity * sizeof(Block));
        if (!grown) {
            return false;
        }
        s->open_blocks.items = grown;
        s->open_blocks.capacity = capacity;
    }
    s->open_blocks.items[s->open_blocks.size++] = b;
    return true;
}
