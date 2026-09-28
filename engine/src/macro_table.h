#pragma once
#include <stddef.h>
#include "arena.h"

#define PDXE_MACRO_MAX_PARAMS 4
#define PDXE_MACRO_TABLE_CAP 4096

typedef struct {
    const char *name;
    int param_count;
    const char *param_names[PDXE_MACRO_MAX_PARAMS];
    const char *expansion;
    const char *resolved_callee;
} PDXEMacroEntry;

typedef struct PDXEMacroTable {
    PDXEMacroEntry entries[PDXE_MACRO_TABLE_CAP];
    int count;
    PDXEArena arena;
} PDXEMacroTable;

// Add an entry. Silently drops on overflow.
void pdxe_macro_table_add(PDXEMacroTable *t, PDXEArena *arena, const char *name, int param_count,
                         const char **param_names, const char *expansion,
                         const char *resolved_callee);

// Look up by name. Returns NULL if not found.
const PDXEMacroEntry *pdxe_macro_table_find(const PDXEMacroTable *t, const char *name);

// Parse a single .inc file content into the table (arena-allocated strings).
void pdxe_parse_inc_file(PDXEMacroTable *t, PDXEArena *arena, const char *content);

// Expand a macro call: substitute args into expansion text.
// Returns arena-allocated expanded text, or NULL if no expansion.
char *pdxe_macro_expand(PDXEArena *arena, const PDXEMacroEntry *entry, const char **args,
                       int arg_count);

// Extract a callee name from expanded text (looks for ##class(X).Method or $$Label^Routine).
// Returns arena-allocated "X.Method" or "Label^Routine", or NULL.
char *pdxe_macro_extract_callee(PDXEArena *arena, const char *expansion);

// Allocate and populate a new table with the hardcoded system macros.
// Caller owns the table (stack or heap).
void pdxe_macro_table_init_system(PDXEMacroTable *t);

// Destroy the arena inside t and free t itself. NULL-safe.
void pdxe_macro_table_free(PDXEMacroTable *t);
