/*
 * Issue 92: the work typed resolution's lookups and the interface's translation do,
 * counted by kind in test builds (PDXE_ENABLE_TEST_SEAMS), so tests can assert how it
 * grows with a file, never how long it takes. Release builds count nothing: every
 * PDXE_LOOKUP_COUNT is empty there.
 *
 * The counts are the calling thread's: extraction and a project's run happen on one
 * thread from start to end, so a test resets them, does its work and reads them.
 */
#ifndef PDXE_LOOKUP_COUNTS_H
#define PDXE_LOOKUP_COUNTS_H

#include <stdbool.h>
#include <stdint.h>

typedef enum {
    PDXE_LC_REGISTRY_ENTRIES, /* registry entries a scan examined (a loop over funcs or types) */
    PDXE_LC_REGISTRY_PROBES,  /* index entries a lookup examined (bucket and chain steps) */
    PDXE_LC_QN_COMPARES,      /* qualified-name comparisons in the registry */
    PDXE_LC_NAME_COMPARES,    /* short-name and receiver comparisons in the registry */
    PDXE_LC_TYPE_SELF,        /* type lookups by qualified name in one registry */
    PDXE_LC_FUNC_SELF,        /* function lookups by qualified name in one registry */
    PDXE_LC_PY_CLASS_SCAN,    /* module-level statements examined for a Python class name */
    PDXE_LC_TS_CANDIDATES,    /* candidates a TypeScript symbol lookup inspected */
    PDXE_LC_ARG_CANDIDATES,   /* overloads scored by argument count or argument types */
    PDXE_LC_DEF_ENTRIES,      /* a file's definitions the interface's translation examined */
    PDXE_LC_IFACE_COMPARES,   /* qualified-name comparisons in the interface's translation */
    PDXE_LC_TS_USAGES,        /* usages a TypeScript lexical-shadow check inspected */
    PDXE_LC_ANSWER_ROWS,      /* resolver rows a call or reference's typed answer inspected */
    PDXE_LC_IMPORT_ITEMS,     /* import statements or items inspected for one import's binding */
    /* Issue 93: navigation of the syntax tree (node_iter.h). */
    PDXE_LC_NAV_INDEXED, /* children reached by position, each weighted by its position + 1 */
    PDXE_LC_NAV_CURSOR,  /* cursor steps: to a first child, a next sibling or a parent */
    PDXE_LC_NAV_PARENT,  /* parent lookups, weighted by the children passed on the way down */
    PDXE_LC_NAV_SIBLING, /* sibling lookups, weighted the same way */
    PDXE_LC_NAV_FIELD,   /* field lookups */
    PDXE_LC_NAV_LOST,    /* child walks cut short: a collection without memory, or a cursor
                          * that ran out before the count */
    PDXE_LC_COUNT
} pdxe_lookup_counter;

#if defined(PDXE_ENABLE_TEST_SEAMS) || defined(PDXE_LOOKUP_COUNTS_TEST_API)
/* Zero every count of this thread. */
void pdxe_lookup_counts_reset(void);
/* This thread's counts since its last reset, indexed by pdxe_lookup_counter. */
void pdxe_lookup_counts_read(uint64_t out[PDXE_LC_COUNT]);
#endif

/* Test builds can make a family of these indexes refuse their memory, as a failed
 * allocation would: PDX_ENGINE_TEST_REFUSE_LOOKUP_INDEXES is `registry` (the registries'
 * own), `scope` (the scope frames'), `resolver` (the per-language resolvers' and the
 * extraction's), `interface` (the interface's translation and answers) or `all`. The
 * refusal is counted as lost work, and the index degrades exactly as it does when its
 * memory is refused. Release builds never refuse. */
#if defined(PDXE_ENABLE_TEST_SEAMS)
bool pdxe_lookup_index_refused(const char *family);
#define PDXE_LOOKUP_INDEX_REFUSED(family) pdxe_lookup_index_refused(family)
#else
#define PDXE_LOOKUP_INDEX_REFUSED(family) false
#endif

#if defined(PDXE_ENABLE_TEST_SEAMS)
extern _Thread_local uint64_t pdxe_lookup_counts[PDXE_LC_COUNT];
#define PDXE_LOOKUP_COUNT(k) ((void)(pdxe_lookup_counts[(k)]++))
#define PDXE_LOOKUP_COUNT_N(k, n) ((void)(pdxe_lookup_counts[(k)] += (uint64_t)(n)))
#else
#define PDXE_LOOKUP_COUNT(k) ((void)0)
#define PDXE_LOOKUP_COUNT_N(k, n) ((void)0)
#endif

#endif /* PDXE_LOOKUP_COUNTS_H */
