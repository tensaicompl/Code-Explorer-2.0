/*
 * mem_core.c — the allocation route. See mem_core.h for why it exists.
 */
#include "foundation/mem_core.h"
#include "foundation/mem.h"
#include "foundation/mem_events.h"

/* Ownership check for blocks handed back to the core (defined with pdxe_free). */
static void check_owned(const void *block, const char *op);

#include "foundation/constants.h"
#include "foundation/log.h"

#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Usable-size query, per platform.
 *
 * Deliberately NOT mi_usable_size: the mimalloc global override is off on
 * macOS (permanently — the two-level namespace aborts on cross-boundary
 * frees), so a pointer from plain malloc there is not a mimalloc block and
 * mi_usable_size would be undefined behaviour on it. Each platform's own query
 * is correct under whichever allocator is actually installed, including when
 * that allocator IS mimalloc via the Linux/MinGW override. */
#if defined(PDXE_BIND_TS_ALLOCATOR) && PDXE_BIND_TS_ALLOCATOR
#include <mimalloc.h>
#define PDXE_BACKING_MALLOC(n) mi_malloc(n)
#define PDXE_BACKING_CALLOC(n) mi_calloc(PDXE_ALLOC_ONE, n)
#define PDXE_BACKING_REALLOC(p, n) mi_realloc(p, n)
#define PDXE_BACKING_FREE(p) mi_free(p)
#define PDXE_USABLE_SIZE(p) mi_usable_size((void *)(p))
#elif defined(__APPLE__)
#include <malloc/malloc.h> /* malloc_size */
#define PDXE_USABLE_SIZE(p) malloc_size(p)
#elif defined(_WIN32)
#include <malloc.h> /* _msize */
#define PDXE_USABLE_SIZE(p) _msize((void *)(p))
#elif defined(__GLIBC__) || defined(__linux__)
#include <malloc.h> /* malloc_usable_size */
#define PDXE_USABLE_SIZE(p) malloc_usable_size((void *)(p))
#else
/* BSD and anything unknown: no portable query. Accounting then tracks the
 * REQUESTED size, which understates by the rounding. Understating is the safe
 * direction for a diagnostic (it never invents memory), and the alternative --
 * a per-block header -- costs 1.6 GB at kernel scale. */
#define PDXE_USABLE_SIZE_UNAVAILABLE 1
#endif

#ifndef PDXE_BACKING_MALLOC
#define PDXE_BACKING_MALLOC(n) malloc(n)
#define PDXE_BACKING_CALLOC(n) calloc(PDXE_ALLOC_ONE, n)
#define PDXE_BACKING_REALLOC(p, n) realloc(p, n)
#define PDXE_BACKING_FREE(p) free(p)
#endif

enum { MEM_CORE_REPORT_MIN = 64 };

typedef struct {
    atomic_size_t live_bytes;
    atomic_size_t live_blocks;
    atomic_size_t peak_bytes;
} mem_class_stats_t;

static mem_class_stats_t g_classes[PDXE_MEM_CLASS_COUNT];

static const char *const g_class_names[PDXE_MEM_CLASS_COUNT] = {
    "other",   "gbuf_node", "gbuf_edge", "gbuf_string", "gbuf_index", "extract",   "arena",
    "ts_tree", "semantic",  "dump",      "store",       "hash_table", "dyn_array",
};

const char *pdxe_mem_class_name(pdxe_mem_class_t cls) {
    if ((int)cls < 0 || (int)cls >= PDXE_MEM_CLASS_COUNT) {
        return "invalid";
    }
    return g_class_names[cls];
}

/* Out-of-range classes are folded into OTHER rather than rejected: a
 * mis-tagged allocation must still be freed correctly. Accounting accuracy is
 * worth less than not corrupting the heap. */
static mem_class_stats_t *class_slot(pdxe_mem_class_t cls) {
    if ((int)cls < 0 || (int)cls >= PDXE_MEM_CLASS_COUNT) {
        return &g_classes[PDXE_MEM_CLASS_OTHER];
    }
    return &g_classes[cls];
}

/* ── Accounting: thread-local deltas, shared atomics on flush ──────────
 * The hot path (every allocation and free on every worker) touches only
 * thread-local memory. The shared per-class counters see one flush per
 * MEM_FLUSH_BYTES / MEM_FLUSH_BLOCKS of change per thread, or an explicit
 * pdxe_mem_class_flush_thread() -- which every parallel-for worker calls when
 * its work item ends and every reader calls for its own thread first. With
 * one atomic per allocation, 18 workers on 18 cores bounced the same three
 * cache lines on every block: Kotlin CPU 38 -> 121 s for a smaller graph,
 * Go 177 -> 312 s (bench vs v0.10.8, 2026-09-14). A class's live figure can
 * lag a running worker by at most MEM_FLUSH_BYTES; the phase marks read
 * after the workers joined, so they are exact. Peaks are recorded at flush
 * and are low by at most threads x MEM_FLUSH_BYTES -- a diagnostic. */
enum { MEM_FLUSH_BYTES = 256 * 1024, MEM_FLUSH_BLOCKS = 512 };

typedef struct {
    long bytes; /* signed: allocations add, frees subtract */
    long blocks;
} mem_delta_t;

static _Thread_local mem_delta_t tl_delta[PDXE_MEM_CLASS_COUNT];

/* live += delta, never wrapping below zero: a mismatched class on free (the
 * one way a caller can get this wrong) must not turn a small drift into a
 * colossal bogus number that looks like a leak. Returns the new value. */
static size_t apply_signed(atomic_size_t *counter, long delta) {
    if (delta >= 0) {
        return atomic_fetch_add_explicit(counter, (size_t)delta, memory_order_relaxed) +
               (size_t)delta;
    }
    size_t sub = (size_t)(-delta);
    size_t seen = atomic_load_explicit(counter, memory_order_relaxed);
    while (true) {
        size_t want = sub > seen ? 0 : seen - sub;
        if (atomic_compare_exchange_weak_explicit(counter, &seen, want, memory_order_relaxed,
                                                  memory_order_relaxed)) {
            return want;
        }
    }
}

static void class_flush_one(pdxe_mem_class_t cls) {
    mem_delta_t *d = &tl_delta[cls];
    if (d->bytes == 0 && d->blocks == 0) {
        return;
    }
    long bytes = d->bytes;
    long blocks = d->blocks;
    d->bytes = 0;
    d->blocks = 0;
    mem_class_stats_t *st = &g_classes[cls];
    size_t now = apply_signed(&st->live_bytes, bytes);
    (void)apply_signed(&st->live_blocks, blocks);
    if (bytes > 0) {
        /* Peak is best-effort under concurrency: racing writers can leave it
         * one flush low; that never changes a decision. */
        size_t seen = atomic_load_explicit(&st->peak_bytes, memory_order_relaxed);
        while (now > seen) {
            if (atomic_compare_exchange_weak_explicit(&st->peak_bytes, &seen, now,
                                                      memory_order_relaxed, memory_order_relaxed)) {
                break;
            }
        }
    }
}

void pdxe_mem_class_flush_thread(void) {
    pdxe_memev_flush_thread(); /* same seam: work-item end, thread end, every reader */
    for (int i = 0; i < PDXE_MEM_CLASS_COUNT; i++) {
        class_flush_one((pdxe_mem_class_t)i);
    }
}

static pdxe_mem_class_t class_index(pdxe_mem_class_t cls) {
    return ((int)cls < 0 || (int)cls >= PDXE_MEM_CLASS_COUNT) ? PDXE_MEM_CLASS_OTHER : cls;
}

static void class_add(pdxe_mem_class_t cls, size_t bytes, size_t blocks) {
    cls = class_index(cls);
    mem_delta_t *d = &tl_delta[cls];
    d->bytes += (long)bytes;
    d->blocks += (long)blocks;
    if (d->bytes >= MEM_FLUSH_BYTES || d->blocks >= MEM_FLUSH_BLOCKS) {
        class_flush_one(cls);
    }
}

static void class_sub(pdxe_mem_class_t cls, size_t bytes, size_t blocks) {
    cls = class_index(cls);
    mem_delta_t *d = &tl_delta[cls];
    d->bytes -= (long)bytes;
    d->blocks -= (long)blocks;
    if (d->bytes <= -MEM_FLUSH_BYTES || d->blocks <= -MEM_FLUSH_BLOCKS) {
        class_flush_one(cls);
    }
}

#ifdef PDXE_USABLE_SIZE_UNAVAILABLE
static size_t charge_size(const void *block, size_t requested) {
    (void)block;
    return requested;
}

size_t pdxe_mem_usable_size(const void *block) {
    (void)block;
    return 0;
}
#else
static size_t charge_size(const void *block, size_t requested) {
    size_t usable = PDXE_USABLE_SIZE(block);
    return usable ? usable : requested;
}

size_t pdxe_mem_usable_size(const void *block) {
    if (!block) {
        return 0;
    }
    return PDXE_USABLE_SIZE(block);
}
#endif

/* -- Waste-sanitizer hooks (mem_events.h) ---------------------------------
 * Compiled to nothing outside the `memwaste` flavour. The core names the
 * caller's site and the class; who REPORTS the event depends on whether an
 * observer sees the backing allocator:
 *   - backing is mimalloc called directly (PDXE_BIND_TS_ALLOCATOR): no observer
 *     ever sees it, the core reports everything itself;
 *   - backing is plain malloc and an observer is installed: the observer
 *     reports, the hint hands it the site and the class;
 *   - backing is plain malloc, no observer: the core reports.
 * Frees are reported BEFORE the block goes back (see mem_events.h). */
#if defined(PDXE_MEMWASTE) && PDXE_MEMWASTE
static bool core_reports_frees(void) {
#if defined(PDXE_BIND_TS_ALLOCATOR) && PDXE_BIND_TS_ALLOCATOR
    return true;
#else
    return !pdxe_memev_observer_installed();
#endif
}
#define MEMEV_HINT(cls)                                              \
    do {                                                             \
        if (pdxe_memev_enabled() && !pdxe_memev_hint_pending()) {      \
            pdxe_memev_hint(__builtin_return_address(0), (int)(cls)); \
        }                                                            \
    } while (0)
#define MEMEV_ALLOCATED(block, bytes, flags)                                                  \
    do {                                                                                      \
        if (pdxe_memev_hint_pending()) {                                                       \
            pdxe_memev_alloc_ex((block), (bytes), (block) ? charge_size((block), (bytes)) : 0, \
                               NULL, (flags));                                                \
        }                                                                                     \
    } while (0)
#define MEMEV_REALLOCATED(old_block, grown, bytes)                                \
    do {                                                                          \
        if (pdxe_memev_hint_pending()) {                                           \
            pdxe_memev_realloc((old_block), (grown), (bytes),                      \
                              (grown) ? charge_size((grown), (bytes)) : 0, NULL); \
        }                                                                         \
    } while (0)
#define MEMEV_FREEING(block)                               \
    do {                                                   \
        if (pdxe_memev_enabled() && core_reports_frees()) { \
            pdxe_memev_free(block);                         \
        }                                                  \
    } while (0)
#else
#define MEMEV_HINT(cls) ((void)0)
#define MEMEV_ALLOCATED(block, bytes, flags) ((void)0)
#define MEMEV_REALLOCATED(old_block, grown, bytes) ((void)0)
#define MEMEV_FREEING(block) ((void)0)
#endif

void *pdxe_alloc(pdxe_mem_class_t cls, size_t bytes) {
    MEMEV_HINT(cls);
    void *block = PDXE_BACKING_MALLOC(bytes ? bytes : PDXE_ALLOC_ONE);
    MEMEV_ALLOCATED(block, bytes, 0);
    if (!block) {
        return NULL;
    }
    class_add(cls, charge_size(block, bytes), PDXE_ALLOC_ONE);
    return block;
}

void *pdxe_calloc(pdxe_mem_class_t cls, size_t bytes) {
    MEMEV_HINT(cls);
    void *block = PDXE_BACKING_CALLOC(bytes ? bytes : PDXE_ALLOC_ONE);
    MEMEV_ALLOCATED(block, bytes, PDXE_MEMEV_ZEROED);
    if (!block) {
        return NULL;
    }
    class_add(cls, charge_size(block, bytes), PDXE_ALLOC_ONE);
    return block;
}

void *pdxe_realloc(pdxe_mem_class_t cls, void *block, size_t bytes) {
    if (!block) {
        return pdxe_alloc(cls, bytes);
    }
    check_owned(block, "realloc");
    /* Measure BEFORE: after realloc the old block is gone and its size is
     * unknowable, so the decrement has to be computed first. */
    size_t old = charge_size(block, 0);
    MEMEV_HINT(cls);
    PDXE_MEMEV_BACKING(1);
    void *next = PDXE_BACKING_REALLOC(block, bytes ? bytes : PDXE_ALLOC_ONE);
    PDXE_MEMEV_BACKING(-1);
    MEMEV_REALLOCATED(block, next, bytes);
    if (!next) {
        return NULL; /* original intact and still charged - correct */
    }
    class_sub(cls, old, 0);
    class_add(cls, charge_size(next, bytes), 0);
    return next;
}

char *pdxe_mem_strdup(pdxe_mem_class_t cls, const char *s) {
    if (!s) {
        return NULL;
    }
    size_t len = strlen(s) + PDXE_ALLOC_ONE;
    MEMEV_HINT(cls); /* the site is OUR caller, not pdxe_alloc's */
    char *copy = (char *)pdxe_alloc(cls, len);
    if (!copy) {
        return NULL;
    }
    memcpy(copy, s, len);
    return copy;
}

/* A block that did not come from the backing allocator reached the core:
 * a cross-allocator free (a libc strdup handed to pdxe_free, which is mi_free
 * in the production build) that no libc-backed test build can see. Checked
 * where the backing is mimalloc and only under PDXE_MEM_PHASES=1 -- the proof
 * runs -- and fatal there: silent heap corruption is the alternative. */
#if defined(PDXE_BIND_TS_ALLOCATOR) && PDXE_BIND_TS_ALLOCATOR
static void check_owned(const void *block, const char *op) {
    if (!pdxe_mem_phases_enabled() || mi_is_in_heap_region(block)) {
        return;
    }
    pdxe_log_error("mem.core.foreign_block", "op", op);
    abort();
}
#else
static void check_owned(const void *block, const char *op) {
    (void)block;
    (void)op;
}
#endif

void pdxe_free(pdxe_mem_class_t cls, void *block) {
    if (!block) {
        return;
    }
    check_owned(block, "free");
    class_sub(cls, charge_size(block, 0), PDXE_ALLOC_ONE);
    MEMEV_FREEING(block);
    PDXE_BACKING_FREE(block);
}

void pdxe_mem_class_add_external(pdxe_mem_class_t cls, size_t bytes) {
    class_add(cls, bytes, 0);
}

void pdxe_mem_class_remove_external(pdxe_mem_class_t cls, size_t bytes) {
    class_sub(cls, bytes, 0);
}

size_t pdxe_mem_class_live_bytes(pdxe_mem_class_t cls) {
    pdxe_mem_class_flush_thread();
    return atomic_load_explicit(&class_slot(cls)->live_bytes, memory_order_relaxed);
}

size_t pdxe_mem_class_live_blocks(pdxe_mem_class_t cls) {
    pdxe_mem_class_flush_thread();
    return atomic_load_explicit(&class_slot(cls)->live_blocks, memory_order_relaxed);
}

size_t pdxe_mem_class_peak_bytes(pdxe_mem_class_t cls) {
    pdxe_mem_class_flush_thread();
    return atomic_load_explicit(&class_slot(cls)->peak_bytes, memory_order_relaxed);
}

size_t pdxe_mem_tracked_live_bytes(void) {
    pdxe_mem_class_flush_thread();
    size_t total = 0;
    for (int i = 0; i < PDXE_MEM_CLASS_COUNT; i++) {
        total += atomic_load_explicit(&g_classes[i].live_bytes, memory_order_relaxed);
    }
    return total;
}

void pdxe_mem_class_reset_peaks(void) {
    pdxe_mem_class_flush_thread();
    for (int i = 0; i < PDXE_MEM_CLASS_COUNT; i++) {
        size_t live = atomic_load_explicit(&g_classes[i].live_bytes, memory_order_relaxed);
        atomic_store_explicit(&g_classes[i].peak_bytes, live, memory_order_relaxed);
    }
}

int pdxe_mem_class_report_json(char *out, size_t size) {
    pdxe_mem_class_flush_thread();
    if (!out || size < MEM_CORE_REPORT_MIN) {
        return 0;
    }
    int order[PDXE_MEM_CLASS_COUNT];
    int n = 0;
    for (int i = 0; i < PDXE_MEM_CLASS_COUNT; i++) {
        if (atomic_load_explicit(&g_classes[i].peak_bytes, memory_order_relaxed) > 0) {
            order[n++] = i;
        }
    }
    if (n == 0) {
        return 0;
    }
    /* Insertion sort: n is at most PDXE_MEM_CLASS_COUNT. */
    for (int i = 1; i < n; i++) {
        int key = order[i];
        size_t kv = atomic_load_explicit(&g_classes[key].live_bytes, memory_order_relaxed);
        int j = i - 1;
        while (j >= 0 &&
               atomic_load_explicit(&g_classes[order[j]].live_bytes, memory_order_relaxed) < kv) {
            order[j + 1] = order[j];
            j--;
        }
        order[j + 1] = key;
    }

    int written = snprintf(out, size, "[");
    for (int i = 0; i < n && written > 0 && (size_t)written < size; i++) {
        int idx = order[i];
        int add = snprintf(out + written, size - (size_t)written,
                           "%s{\"class\":\"%s\",\"live_bytes\":%zu,\"live_blocks\":%zu,"
                           "\"peak_bytes\":%zu}",
                           i ? "," : "", g_class_names[idx],
                           atomic_load_explicit(&g_classes[idx].live_bytes, memory_order_relaxed),
                           atomic_load_explicit(&g_classes[idx].live_blocks, memory_order_relaxed),
                           atomic_load_explicit(&g_classes[idx].peak_bytes, memory_order_relaxed));
        if (add < 0 || (size_t)(written + add) >= size) {
            return 0; /* truncated: a partial JSON array is worse than none */
        }
        written += add;
    }
    if ((size_t)written + PDXE_ALLOC_ONE >= size) {
        return 0;
    }
    out[written++] = ']';
    out[written] = '\0';
    return written;
}

void pdxe_mem_class_log(const char *tag) {
    pdxe_mem_class_flush_thread();
    char report[PDXE_SZ_1K];
    if (pdxe_mem_class_report_json(report, sizeof(report)) <= 0) {
        return;
    }
    pdxe_log_info("mem.classes", "tag", tag ? tag : "-", "classes", report);
}
