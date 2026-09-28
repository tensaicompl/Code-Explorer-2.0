/*
 * mem_events.h — the waste sanitizer: memory and CPU work that is correct but
 * unnecessary, attributed to the code that caused it.
 *
 * ASan, MSan, TSan and LSan find memory that is WRONG. Nothing in the tree
 * measured memory or work that is merely WASTED — allocator slack, blocks
 * bigger than what is written into them, churn an arena would have served,
 * realloc chains copying the same bytes again, duplicate strings, memory held
 * across phases that never touch it, the same string measured twice, the same
 * file opened twice. The memory-core rounds found such waste BY HAND; this
 * layer turns it into a ranked report.
 *
 * On 2026-09-17 a forwarding malloc-zone observer showed the Go indexing worker
 * making 92.3 million raw allocations OUTSIDE the memory core on macOS — 128 GB
 * requested cumulatively in a 41 s run — that no instrument in the tree saw.
 *
 * WHERE IT LISTENS
 *   - the memory core (mem_core.c) announces its own calls with a site + class;
 *   - platform observers see the malloc family underneath: the Linux and
 *     Windows --wrap shims, the macOS malloc-zone observer;
 *   - bound allocators that bypass both (SQLite, tree-sitter -> mimalloc) report
 *     directly;
 *   - containers (arena, dynamic array, hash table) report capacity vs use;
 *   - libc wrappers (mem_override_libc.c) report byte and system-call work;
 *   - the ACCESS lane (-fsanitize-coverage=trace-loads,trace-stores) reports
 *     reads and writes of every tracked block.
 *
 * COSTS NOTHING UNLESS ASKED FOR. Compiled in only under -DPDXE_MEMWASTE=1 (the
 * `memwaste` build flavour, `make MEMWASTE=1`) and dormant unless PDXE_MEMWASTE=1
 * is set. The access lane additionally needs -DPDXE_MEMWASTE_ACCESS=1
 * (`make MEMWASTE_ACCESS=1`). Release binaries contain none of it.
 *
 * NO PER-BLOCK HEADER (see mem_core.h: 16 bytes x ~100 M kernel blocks is
 * 1.6 GB). Block state lives in side tables backed by raw virtual memory, so
 * the layer never calls the allocator it is observing.
 *
 * DETERMINISM (O9). Every quantity a gate may read is a SUM over events —
 * per-site totals — or a snapshot taken at a phase mark after the workers
 * joined. Nothing here reads a clock.
 *
 * ENVIRONMENT
 *   PDXE_MEMWASTE=1          wake the layer
 *   PDXE_MEMWASTE_OUT=path   append JSON-lines dumps at every phase mark
 *   PDXE_MEMWASTE_FILL=0     do not fill new blocks with the never-written pattern
 *   PDXE_MEMWASTE_DUPES=0    skip the duplicate-content scan at phase marks
 */
#ifndef PDXE_MEM_EVENTS_H
#define PDXE_MEM_EVENTS_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

/* Class value for blocks that never passed through the memory core. */
#define PDXE_MEMEV_CLASS_RAW (-1)

/* Allocation flags. */
enum {
    PDXE_MEMEV_ZEROED = 1u,  /* calloc: the allocator zeroed it, never fill it */
    PDXE_MEMEV_FOREIGN = 2u, /* a system library allocating for itself: observe, never fill */
    PDXE_MEMEV_LIBRARY = 4u, /* allocated inside a wrapped library call, charged to its caller */
    /* The block already holds its content when it is reported (a strdup copy, a
     * realloc whose old block the layer never saw): observe, NEVER fill -- the
     * fill pattern would overwrite live data. */
    PDXE_MEMEV_WRITTEN = 8u,
};

/* Blocks at or below this size take part in the duplicate-content scan. */
enum { PDXE_MEMEV_DUP_MAX = 256 };

/* Freed by the allocating thread within this many of its own allocations:
 * churn an arena or a stack buffer would have served. */
enum { PDXE_MEMEV_CHURN_WINDOW = 64 };

/* True when the layer is compiled in AND PDXE_MEMWASTE=1. Cached after the
 * first call; safe from inside an allocator hook (it never allocates). */
bool pdxe_memev_enabled(void);

/* ── Allocation events ────────────────────────────────────────────────
 * `site` is a return address inside the code that asked for the memory.
 * `usable` is what the allocator actually handed out (0 when unknown: the
 * layer then charges `requested`). All return immediately when dormant. */
void pdxe_memev_alloc(void *block, size_t requested, size_t usable, void *site);
void pdxe_memev_alloc_ex(void *block, size_t requested, size_t usable, void *site, unsigned flags);
void pdxe_memev_free(void *block);
/* A free of a block another allocator made (Windows: the C runtime or a system
 * DLL allocating where --wrap cannot reach). Counted apart from untracked
 * frees: its allocation was never observable, so it says what the layer cannot
 * see, not that the layer missed something it should have seen. */
void pdxe_memev_free_foreign(void *block);
/* One event for a realloc: the old block's record moves to `grown`, the copy
 * the allocator had to make is charged to the ORIGINAL allocation site. */
void pdxe_memev_realloc(void *old_block, void *grown, size_t requested, size_t usable, void *site);

/* ── Core <-> observer handshake ──────────────────────────────────────
 * The memory core and a platform observer can both see one allocation. The
 * core announces itself first; the observer consumes the hint and the event
 * carries the CORE's caller as site and the class attached. When nothing
 * consumed the hint, the core reports the event itself:
 *
 *   pdxe_memev_hint(site, cls);
 *   block = backing_malloc(n);
 *   if (pdxe_memev_hint_pending()) {
 *       pdxe_memev_alloc(block, n, usable, NULL);
 *   }
 *
 * FREES HAVE NO HANDSHAKE, on purpose: a free event must be recorded BEFORE
 * the block goes back to the allocator, or another thread can be handed the
 * same address, record its allocation, and have it erased by the late free.
 * Where an observer sees the backing allocator it reports frees; everywhere
 * else the core reports them itself, first. */
void pdxe_memev_hint(void *site, int mem_class);
bool pdxe_memev_hint_pending(void);
void pdxe_memev_set_observer_installed(bool installed);
bool pdxe_memev_observer_installed(void);

/* Load address of the main image, written into every report header: sites are
 * raw addresses in a randomised process, and offline symbolisation needs the
 * base they are relative to. 0 = unknown. */
void pdxe_memev_set_image_base(uintptr_t base);

/* ── Merge points ─────────────────────────────────────────────────────
 * Per-thread counters are pushed to the shared tables here: work-item end,
 * thread end, before any reader. */
void pdxe_memev_flush_thread(void);

/* This thread is ending: merge its counters and release its state now. POSIX
 * does this from a pthread key destructor. A static MinGW link runs no FLS or
 * key destructors at thread exit (the same reason compat_thread.c releases the
 * mimalloc thread heap from a loader TLS callback, #581), so Windows threads
 * left their 22 MB state behind -- 71 of them by the similarity pass on the
 * Go corpus, then commit exhaustion (2026-09-17). Safe to call twice. */
void pdxe_memev_thread_end(void);

/* A pipeline phase boundary. Blocks allocated from now on carry the new phase;
 * the live set is snapshotted (retained-across-phases, duplicate content) and
 * a dump is appended to PDXE_MEMWASTE_OUT. Called after the workers joined. */
void pdxe_memev_phase(const char *label);

/* ── Process role and exit ────────────────────────────────────────────
 * The index worker leaves with _Exit (no exit handlers: multi-GB teardown is
 * skipped), which also skips every at-exit writer -- the waste dump and clang's
 * profile runtime. pdxe_memev_process_exit writes both first; it is a no-op when
 * neither lane is compiled in or active.
 * pdxe_memev_process_role names this process's execution-count profile
 * <PDXE_PROFILE_DIR>/<role>-<pid>.profraw, so the scaling lane can compare the
 * worker's work without the daemon and CLI processes mixed in. */
void pdxe_memev_process_role(const char *role);
void pdxe_memev_process_exit(void);

/* ── Access lane: what the compiler cannot see ─────────────────────────
 * The access lane learns about reads and writes from compiler callbacks, and a
 * library call is not compiled with them: a block filled by memcpy or read(2)
 * would look never written. The library wrappers report those ranges here.
 * While the allocator itself runs (a realloc copying the old block), its reads
 * and writes are not the program's: the shims bracket every backing call that
 * can copy. Both vanish outside the access lane. */
#if defined(PDXE_MEMWASTE_ACCESS) && PDXE_MEMWASTE_ACCESS
void pdxe_memev_access(const void *p, size_t n, bool store, void *pc);
void pdxe_memev_backing(int delta);
#define PDXE_MEMEV_ACCESS(p, n, store, pc) pdxe_memev_access((p), (n), (store), (pc))
#define PDXE_MEMEV_BACKING(delta) pdxe_memev_backing(delta)
#else
#define PDXE_MEMEV_ACCESS(p, n, store, pc) ((void)0)
#define PDXE_MEMEV_BACKING(delta) ((void)0)
#endif

/* A wrapped library call (fopen, fread, opendir...) is running on this thread.
 * Memory the library allocates for itself meanwhile -- a FILE buffer, a DIR
 * stream -- is charged to `site` (the wrapper's caller) with PDXE_MEMEV_LIBRARY:
 * it is counted like any memory, but the access lane leaves it alone, because
 * only the uninstrumented library ever reads it. Nestable; the outermost site
 * wins. */
void pdxe_memev_library_enter(void *site);
void pdxe_memev_library_leave(void);
void *pdxe_memev_library_site(void);

/* ── CPU work ─────────────────────────────────────────────────────────
 * Work is COUNTED, never timed: a count is a function of the code and the
 * input, so it can gate. Semantics of the counters per kind:
 *
 *   kind              bytes             aux1                aux2
 *   memcpy..strncmp   bytes processed   -                   -
 *   read, write,
 *   pread, pwrite,
 *   fread, fwrite     bytes transferred tiny calls (<512 B) -
 *   open, stat,
 *   fopen, opendir    -                 failures            -
 *   readdir           -                 -                   -
 *   mutex             -                 contended           -
 *   parallel_for      items             imbalance items     workers
 *   pool_ops          events by workers imbalance events    workers
 *   ht_get            -                 misses              -
 *   ht_set            -                 table growths       -
 *   dead_write        bytes never read  blocks              -
 *   ct_arena,
 *   ct_dyn_array,
 *   ct_hash_table     capacity bytes    used bytes          growths
 *
 * `repeats` counts the same work done again, where the answer cannot have
 * changed: strlen of the same pointer holding the same bytes (recent, same
 * thread), open/stat of the same path (process-wide), a hash-table lookup of
 * the same key TEXT in the same table with no write to that table in between
 * (recent, same thread). A reused buffer holding a different string is not a
 * repeat.
 *
 * `peak` is the largest single note: the biggest call for byte work and I/O,
 * and for containers the largest capacity one instance left unused. Containers
 * report at every reset, so `bytes` and `aux1` sum over USES (a per-thread arena
 * rewound for 28,000 files reports 28,000 times); `peak` is the memory figure. */
typedef enum {
    PDXE_WORK_MEMCPY = 0,
    PDXE_WORK_MEMMOVE,
    PDXE_WORK_MEMSET,
    PDXE_WORK_MEMCMP,
    PDXE_WORK_STRLEN,
    PDXE_WORK_STRCMP,
    PDXE_WORK_STRNCMP,
    PDXE_WORK_READ,
    PDXE_WORK_WRITE,
    PDXE_WORK_PREAD,
    PDXE_WORK_PWRITE,
    PDXE_WORK_OPEN,
    PDXE_WORK_STAT,
    PDXE_WORK_FOPEN,
    PDXE_WORK_FREAD,
    PDXE_WORK_FWRITE,
    PDXE_WORK_OPENDIR,
    PDXE_WORK_READDIR,
    PDXE_WORK_MUTEX,
    PDXE_WORK_PARALLEL_FOR,
    PDXE_WORK_POOL_OPS,
    PDXE_WORK_HT_GET,
    PDXE_WORK_HT_SET,
    PDXE_WORK_DEAD_WRITE,
    PDXE_WORK_CT_ARENA,
    PDXE_WORK_CT_DYN_ARRAY,
    PDXE_WORK_CT_HASH_TABLE,
    PDXE_WORK_KIND_COUNT
} pdxe_work_kind_t;

enum { PDXE_WORK_TINY_IO = 512 };

const char *pdxe_work_kind_name(pdxe_work_kind_t kind);
void pdxe_work_note(pdxe_work_kind_t kind, void *site, uint64_t bytes, uint64_t aux1, uint64_t aux2);
void pdxe_work_note_strlen(void *site, const char *s, size_t len);

/* Events (allocations and work notes) this thread has produced so far. A pool
 * compares its workers by this: a pool over N work SLOTS whose workers each pull
 * their own items shows its real imbalance only in the work done. */
uint64_t pdxe_memev_thread_ops(void);

#if defined(PDXE_MEMWASTE) && PDXE_MEMWASTE && !defined(_WIN32)
/* The one counting lock behind every pthread_mutex_lock in the flavour
 * (mem_override_libc.c): try first, count a contended acquisition at `site`,
 * then block. pdxe_mutex_lock passes its own caller as the site. */
int pdxe_memev_mutex_lock(void *pthread_mutex, void *site);
#endif
void pdxe_work_note_path(pdxe_work_kind_t kind, void *site, const char *path, bool failed);
/* `generation` is the table's write counter: a lookup repeats only when the
 * table has not been written since the same key was last looked up. */
void pdxe_work_note_ht(pdxe_work_kind_t kind, void *site, const void *table, const char *key,
                      uint64_t generation, bool hit);

/* Containers report capacity against use when they are freed or reset.
 * `site` is where the container was created. */
static inline void pdxe_memev_container(pdxe_work_kind_t kind, void *site, uint64_t capacity,
                                       uint64_t used, uint64_t growths) {
    pdxe_work_note(kind, site, capacity, used, growths);
}

/* A dynamic array being freed: capacity against count, in bytes. The site is
 * the function that freed it (the helper is never inlined, so its return
 * address lands there). Growths are the doublings from 8 slots to `cap`. */
void pdxe_memev_da_note(size_t cap_bytes, size_t used_bytes, size_t cap_slots);

/* ── Report ───────────────────────────────────────────────────────────*/
typedef struct {
    uint64_t allocs;          /* alloc events seen */
    uint64_t frees;           /* free events matched to a tracked block */
    uint64_t untracked_frees; /* frees of blocks the layer never saw */
    uint64_t foreign_frees;   /* frees of blocks another allocator made */
    uint64_t reallocs;
    uint64_t requested_bytes;    /* cumulative */
    uint64_t usable_bytes;       /* cumulative */
    uint64_t realloc_copy_bytes; /* bytes the allocator had to move */
    uint64_t live_bytes;         /* usable bytes outstanding now */
    uint64_t live_blocks;
    uint64_t raw_bytes; /* cumulative usable bytes never tagged by the core */
    uint64_t sites;     /* distinct allocation sites */
    /* Records the fixed tables could not hold. A profiler that silently loses
     * records produces false confidence; these make the blind spot a number. */
    uint64_t site_table_full;
    uint64_t pointer_table_full;
    uint64_t work_table_full;
} pdxe_memev_totals_t;

void pdxe_memev_totals(pdxe_memev_totals_t *out);

typedef struct {
    uintptr_t site;
    uint64_t allocs;
    uint64_t frees;
    uint64_t reallocs;
    uint64_t requested_bytes;
    uint64_t usable_bytes;
    uint64_t realloc_copy_bytes;
    uint64_t live_bytes;
    uint64_t live_blocks;
    uint64_t short_lived;
    uint64_t raw_bytes;
    /* fill-pattern scan at free */
    uint64_t scanned_blocks;
    uint64_t never_written_bytes;  /* bytes still holding the fill pattern */
    uint64_t over_requested_bytes; /* requested beyond the last written byte */
    uint64_t zero_untouched_bytes; /* calloc: trailing zero bytes (upper bound) */
    /* phase snapshot (the most recent phase mark) */
    uint64_t retained_bytes; /* live, allocated in an EARLIER phase */
    uint64_t dup_bytes;      /* live small blocks identical to another */
    uint64_t dup_blocks;
    /* access lane */
    uint64_t acc_untouched_blocks;
    uint64_t acc_untouched_bytes;
    uint64_t acc_dead_blocks; /* written, never read */
    uint64_t acc_dead_bytes;
    uint64_t acc_uninit_reads;  /* read before any write, not calloc */
    uint64_t acc_idle_bytes;    /* last accessed in an earlier phase than freed */
    uint64_t acc_opaque_blocks; /* written (fill scan) with no callback: no verdict possible */
} pdxe_memev_site_t;

size_t pdxe_memev_sites(pdxe_memev_site_t *out, size_t max);

typedef struct {
    uintptr_t site;
    pdxe_work_kind_t kind;
    uint64_t calls;
    uint64_t bytes;
    uint64_t aux1;
    uint64_t aux2;
    uint64_t repeats;
    uint64_t peak;
} pdxe_work_row_t;

size_t pdxe_work_rows(pdxe_work_row_t *out, size_t max);

/* Append the report as JSON lines to `path`: one header object, one object per
 * allocation site, one per work row. Raw addresses; symbolisation is offline
 * (scripts/memwaste-report.py). `why` labels the dump. */
bool pdxe_memev_dump(const char *path, const char *why);

/* Test seams. Never called in production. */
void pdxe_memev_reset_for_tests(void);
void pdxe_memev_force_for_tests(bool on);
/* Tests that hand the layer REAL memory turn the fill pattern and the duplicate
 * scan on (reset turns fill off: most tests use synthetic addresses). */
void pdxe_memev_scan_for_tests(bool fill, bool dupes);

#endif /* PDXE_MEM_EVENTS_H */
