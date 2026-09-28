#ifndef PDXE_AC_H
#define PDXE_AC_H

#include <stdint.h>

// Forward declaration — full struct in ac.c
typedef struct PDXEAutomaton PDXEAutomaton;

// Input for batch LZ4 scanning.
typedef struct {
    const char *data;
    int compressed_len;
    int original_len;
} PDXELz4Entry;

// Output for batch LZ4 scanning.
typedef struct {
    int file_index;
    uint64_t bitmask;
} PDXELz4Match;

// Output for batch name scanning.
typedef struct {
    int name_index;
    int pattern_id;
} PDXEMatchResult;

// Build an Aho-Corasick automaton from patterns.
PDXEAutomaton *pdxe_ac_build(const char **patterns, const int *lengths, int count,
                           const uint8_t *alpha_map, int alpha_size);
void pdxe_ac_free(PDXEAutomaton *ac);

// Single-text scanning (returns bitmask of matched pattern IDs).
uint64_t pdxe_ac_scan_bitmask(const PDXEAutomaton *ac, const char *text, int text_len);

// LZ4-compressed scanning.
uint64_t pdxe_ac_scan_lz4_bitmask(const PDXEAutomaton *ac, const char *compressed, int compressed_len,
                                 int original_len);
int pdxe_ac_scan_lz4_batch(const PDXEAutomaton *ac, const PDXELz4Entry *entries, int num_entries,
                          PDXELz4Match *out_matches, int max_matches);

// Batch name scanning.
int pdxe_ac_scan_batch(const PDXEAutomaton *ac, const char *names_buf, const int *name_offsets,
                      const int *name_lengths, int num_names, PDXEMatchResult *out_matches,
                      int max_matches);

// Introspection.
int pdxe_ac_num_states(const PDXEAutomaton *ac);
int pdxe_ac_num_patterns(const PDXEAutomaton *ac);
int pdxe_ac_table_bytes(const PDXEAutomaton *ac);

#endif // PDXE_AC_H
