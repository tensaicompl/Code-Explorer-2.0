/* The YAML grammar's scanner, compiled into scanner_state with its entry points renamed
 * (issue 91): the test drives the scanner's own pushes, ceiling and serializer. */
#define tree_sitter_yaml_external_scanner_create unit_yaml_create_
#define tree_sitter_yaml_external_scanner_destroy unit_yaml_destroy_
#define tree_sitter_yaml_external_scanner_scan unit_yaml_scan_
#define tree_sitter_yaml_external_scanner_serialize unit_yaml_serialize_
#define tree_sitter_yaml_external_scanner_deserialize unit_yaml_deserialize_
#include "../../grammars/yaml/scanner.c"

#include "hooks.h"

size_t unit_serialization_buffer_size(void) { return TREE_SITTER_SERIALIZATION_BUFFER_SIZE; }

size_t unit_yaml_header_bytes(void) { return YAML_SER_HEADER_BYTES; }
size_t unit_yaml_entry_bytes(void) { return YAML_SER_ENTRY_BYTES; }
size_t unit_yaml_max_entries(void) { return YAML_MAX_IND_ENTRIES; }
void *unit_yaml_new(void) { return unit_yaml_create_(); }
void unit_yaml_free(void *scanner) { unit_yaml_destroy_(scanner); }
unsigned unit_yaml_serialize(void *scanner, char *buffer) { return unit_yaml_serialize_(scanner, buffer); }
void unit_yaml_deserialize(void *scanner, const char *buffer, unsigned length) {
    unit_yaml_deserialize_(scanner, buffer, length);
}
size_t unit_yaml_entries(void *scanner) { return ((Scanner *)scanner)->ind_typ_stk.size - 1; }

bool unit_yaml_push(void *scanner, bool forced) {
    Scanner *s = (Scanner *)scanner;
    if (!forced && !ind_has_room(s)) {
        return false;
    }
    /* Distinct values, so a round trip that loses or reorders an entry shows. */
    int16_t n = (int16_t)s->ind_typ_stk.size;
    push_ind(s, (int16_t)(n % 2 ? IND_MAP : IND_SEQ), n);
    return true;
}
