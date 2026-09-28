/*
 * result_spill.c — see result_spill.h.
 *
 * Layout on disk, per parked result:
 *   spill_rec_hdr_t  (magic, block length, block base address at park time,
 *                     the PDXEFileResult header as it was in memory)
 *   block bytes      (the single compacted arena block)
 *
 * The header's pointers are meaningless on disk; the loader rebuilds the
 * arena at a new address and shifts every pointer by the delta
 * (pdxe_result_relocate, implemented on the compaction traversal so it sees
 * exactly the fields compaction copied).
 */

#include "result_spill.h"

#include "foundation/arena.h"
#include "foundation/compat_fs.h"
#include "foundation/compat_thread.h"
#include "foundation/constants.h"
#include "foundation/log.h"
#include "foundation/mem_core.h"
#include "foundation/platform.h" /* pdxe_fs_free_bytes: the disk ceiling below */

/* The park writes a result as an opaque image: the record header (a struct
 * copy) and the compacted arena block. Both carry padding bytes no code ever
 * wrote -- inside structs, between objects -- and MemorySanitizer tracks
 * that mark through the compaction's memcpy, so it refuses the fwrite of an
 * image that is read back whole and never interpreted byte by byte (CI MSan
 * lane on #2202: offset 4087, then 6714, of a 6,952-byte block). Under MSan
 * the image is declared defined right before the write; every other build
 * compiles this to nothing. */
#include "foundation/sanitized.h" /* __has_feature exists everywhere, cppcheck included */
#if __has_feature(memory_sanitizer)
#include <sanitizer/msan_interface.h>
#define SPILL_IMAGE_DEFINED(p, n) __msan_unpoison((p), (n))
#else
#define SPILL_IMAGE_DEFINED(p, n) ((void)0)
#endif

#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifdef _WIN32
#include <process.h>
#define SPILL_PID() ((long)_getpid())
#define SPILL_SEEK(fp, off) _fseeki64((fp), (long long)(off), SEEK_SET)
#else
#include <unistd.h>
#define SPILL_PID() ((long)getpid())
#define SPILL_SEEK(fp, off) fseeko((fp), (off_t)(off), SEEK_SET)
#endif

static const uint64_t SPILL_MAGIC = 0x5350494C4C524553ULL; /* "SPILLRES" */

typedef struct {
    uint64_t magic;
    uint64_t block_len;
    uint64_t old_base; /* (uintptr_t) of the block when parked */
    PDXEFileResult header;
} spill_rec_hdr_t;

typedef struct {
    FILE *fp;
    pdxe_mutex_t mu; /* reads seek; writes append -- one lock per file */
    char path[PDXE_SZ_1K];
    uint64_t end;  /* bytes written so far (append offset) */
    bool unlinked; /* already removed while open (POSIX); close must not retry */
} spill_file_t;

typedef struct {
    int writer;       /* -1 = empty */
    uint64_t offset;  /* record start in that writer's file */
    uint64_t rec_len; /* header + block */
    /* The file's namespace/package, kept in MEMORY while the result itself is
     * on disk. The namespace map that import resolution is built on is
     * assembled from every file at once, before any of them is reloaded: a
     * parked file used to contribute nothing to it, so spilling silently
     * changed how imports resolved (see pdxe_result_spill_namespace). One short
     * string per parked file is a rounding error against the result it
     * replaces. */
    char *namespace_name;
} spill_slot_t;

struct pdxe_result_spill {
    spill_file_t *files;
    int writers;
    spill_slot_t *slots;
    int slot_count;
    _Atomic int64_t parked;
    _Atomic int64_t bytes;
    _Atomic int64_t loads;
    int64_t byte_cap;         /* 0 = uncapped (the platform could not say) */
    _Atomic int cap_reported; /* reaching the cap is logged once, not per park */
};

/* Spilling trades a memory problem for a disk problem, so the disk gets a say.
 *
 * FLOOR: below this much free space, do not start spilling at all — filling the
 * user's disk is a worse failure than the memory pressure being relieved, since
 * it breaks everything else on the machine and not just this index.
 * SHARE: of what IS free, never take more than this fraction, so a runaway
 * index cannot consume the rest either.
 * Free space is read once, at open: it is this run's budget, not a moving
 * target to race against, and a failed write stays handled underneath. */
enum { SPILL_FREE_FLOOR_GB = 10, SPILL_FREE_SHARE_DIV = 2 };

/* Remove spill files left by runs that are no longer here. Only files this
 * module names (spill-<pid>-<writer>.bin) are touched, and on Windows an open
 * file refuses deletion, which is exactly the liveness check we want: a running
 * index keeps its own spill. POSIX runs unlink at open, so this only ever finds
 * files from before that behaviour, or from another platform's cache. */
static void spill_sweep_orphans(const char *spill_dir) {
    pdxe_dir_t *dir = pdxe_opendir(spill_dir);
    if (!dir) {
        return;
    }
    int removed = 0;
    pdxe_dirent_t *entry;
    while ((entry = pdxe_readdir(dir)) != NULL) {
        if (strncmp(entry->name, "spill-", 6) != 0) {
            continue;
        }
        size_t len = strlen(entry->name);
        if (len < 5 || strcmp(entry->name + len - 4, ".bin") != 0) {
            continue;
        }
        char path[PDXE_SZ_1K];
        if (snprintf(path, sizeof(path), "%s/%s", spill_dir, entry->name) >= (int)sizeof(path)) {
            continue;
        }
        if (pdxe_unlink(path) == 0) {
            removed++;
        }
    }
    pdxe_closedir(dir);
    if (removed > 0) {
        char removed_text[PDXE_SZ_16];
        snprintf(removed_text, sizeof(removed_text), "%d", removed);
        pdxe_log_info("mem.spill.orphans_removed", "count", removed_text, "dir", spill_dir);
    }
}

pdxe_result_spill_t *pdxe_result_spill_open(const char *dir, int writers, int slots) {
    if (!dir || !dir[0] || writers <= 0 || slots <= 0) {
        return NULL;
    }
    char spill_dir[PDXE_SZ_1K];
    if (snprintf(spill_dir, sizeof(spill_dir), "%s/spill", dir) >= (int)sizeof(spill_dir)) {
        return NULL;
    }
    if (!pdxe_mkdir_p(spill_dir, 0700)) {
        pdxe_log_warn("mem.spill.open_failed", "dir", spill_dir, "reason", "mkdir");
        return NULL;
    }
    /* Reclaim what earlier runs could not. A run killed outright (OOM killer,
     * SIGKILL, power loss) runs no cleanup, and on Windows its spill files
     * survive; POSIX runs unlink-at-open below and leaves nothing. Deleting is
     * the liveness test: an open file cannot be removed on Windows, so a
     * concurrent run's spill refuses and is left alone. */
    spill_sweep_orphans(spill_dir);
    /* Ask the disk BEFORE trading memory for it. Refusing here leaves the
     * caller in the memory path it was already in (back-pressure, then its own
     * budget decision) — the same outcome as a disk that fills mid-run, minus
     * the filled disk. */
    size_t free_bytes = pdxe_fs_free_bytes(spill_dir);
    int64_t byte_cap = 0;
    if (free_bytes > 0) {
        size_t floor_bytes = (size_t)SPILL_FREE_FLOOR_GB * 1024 * 1024 * 1024;
        if (free_bytes < floor_bytes) {
            char free_mb[PDXE_SZ_32];
            snprintf(free_mb, sizeof(free_mb), "%zu", free_bytes / (1024 * 1024));
            pdxe_log_warn("mem.spill.refused", "reason", "low_disk", "free_mb", free_mb, "detail",
                         "not spilling onto a nearly full disk; memory relief continues through "
                         "back-pressure instead");
            return NULL;
        }
        byte_cap = (int64_t)(free_bytes / SPILL_FREE_SHARE_DIV);
    }
    pdxe_result_spill_t *sp = pdxe_calloc(PDXE_MEM_CLASS_OTHER, sizeof(*sp));
    if (!sp) {
        return NULL;
    }
    sp->files = pdxe_calloc(PDXE_MEM_CLASS_OTHER, (size_t)writers * sizeof(spill_file_t));
    sp->slots = pdxe_calloc(PDXE_MEM_CLASS_OTHER, (size_t)slots * sizeof(spill_slot_t));
    if (!sp->files || !sp->slots) {
        pdxe_result_spill_close(sp);
        return NULL;
    }
    sp->writers = writers;
    sp->slot_count = slots;
    sp->byte_cap = byte_cap;
    for (int i = 0; i < slots; i++) {
        sp->slots[i].writer = -1;
    }
    for (int w = 0; w < writers; w++) {
        spill_file_t *f = &sp->files[w];
        snprintf(f->path, sizeof(f->path), "%s/spill-%ld-%d.bin", spill_dir, SPILL_PID(), w);
        f->fp = pdxe_fopen(f->path, "w+b");
        if (!f->fp) {
            pdxe_log_warn("mem.spill.open_failed", "path", f->path, "reason", "fopen");
            pdxe_result_spill_close(sp);
            return NULL;
        }
        pdxe_mutex_init(&f->mu);
#ifndef _WIN32
        /* Unlink while we hold it open: the data stays reachable through this
         * handle, and the space comes back the moment the process ends — a
         * clean exit, an abort, or a SIGKILL from the OOM killer alike. A
         * killed run cannot leave gigabytes of spill behind because there is no
         * name left to leave it under. Windows refuses to unlink an open file,
         * so it keeps the close-path unlink and the sweep below. */
        if (pdxe_unlink(f->path) == 0) {
            f->unlinked = true;
        }
#endif
    }
    pdxe_log_info("mem.spill.open", "dir", spill_dir, "writers", writers > 0 ? "yes" : "no");
    return sp;
}

bool pdxe_result_spill_has(const pdxe_result_spill_t *sp, int slot) {
    return sp && slot >= 0 && slot < sp->slot_count && sp->slots[slot].writer >= 0;
}

bool pdxe_result_spill_park(pdxe_result_spill_t *sp, int writer, int slot, PDXEFileResult *result) {
    if (!sp || !result || writer < 0 || writer >= sp->writers || slot < 0 ||
        slot >= sp->slot_count || sp->slots[slot].writer >= 0) {
        return false;
    }
    /* Only a compacted result is one block with every pointer inside it; a
     * result that owns sub-results (embedded languages) has more arenas than
     * that and stays in memory. A retained parse tree is a re-parse cache,
     * not data: it is dropped with the in-memory result below. */
    if (result->arena.nblocks != 1 || result->owned_result_count != 0) {
        return false;
    }
    /* The disk ceiling. Refusing a park leaves the result in memory, which is
     * exactly where it would be without spill at all — the caller's
     * back-pressure and budget keep deciding from there. What this prevents is
     * an index quietly consuming the rest of the user's free space. */
    if (sp->byte_cap > 0) {
        int64_t want = (int64_t)(sizeof(spill_rec_hdr_t) + result->arena.used);
        if (atomic_load_explicit(&sp->bytes, memory_order_relaxed) + want > sp->byte_cap) {
            if (atomic_exchange_explicit(&sp->cap_reported, 1, memory_order_relaxed) == 0) {
                char cap_mb[PDXE_SZ_32];
                snprintf(cap_mb, sizeof(cap_mb), "%lld", (long long)(sp->byte_cap / (1024 * 1024)));
                pdxe_log_warn("mem.spill.cap_reached", "cap_mb", cap_mb, "detail",
                             "spill is at its share of free disk; results stay in memory and "
                             "back-pressure takes over");
            }
            return false;
        }
    }
    spill_file_t *f = &sp->files[writer];
    spill_rec_hdr_t hdr;
    memset(&hdr, 0, sizeof(hdr));
    hdr.magic = SPILL_MAGIC;
    hdr.block_len = result->arena.used; /* bytes actually written into the block */
    hdr.old_base = (uint64_t)(uintptr_t)result->arena.blocks[0];
    hdr.header = *result;
    hdr.header.cached_tree = NULL; /* never on disk: the loader gets no tree */
    SPILL_IMAGE_DEFINED(&hdr, sizeof(hdr));
    SPILL_IMAGE_DEFINED(result->arena.blocks[0], hdr.block_len);
    pdxe_mutex_lock(&f->mu);
    uint64_t offset = f->end;
    bool ok = SPILL_SEEK(f->fp, offset) == 0 && fwrite(&hdr, sizeof(hdr), 1, f->fp) == 1 &&
              (hdr.block_len == 0 || fwrite(result->arena.blocks[0], hdr.block_len, 1, f->fp) == 1);
    if (ok) {
        f->end = offset + sizeof(hdr) + hdr.block_len;
    }
    pdxe_mutex_unlock(&f->mu);
    if (!ok) {
        pdxe_log_warn("mem.spill.write_failed", "path", f->path);
        return false;
    }
    sp->slots[slot].writer = writer;
    sp->slots[slot].offset = offset;
    sp->slots[slot].rec_len = sizeof(hdr) + hdr.block_len;
    /* Copied BEFORE the result is freed below: the namespace map is built from
     * all files at once, long before this slot is loaded back. */
    if (result->namespace_name && result->namespace_name[0]) {
        sp->slots[slot].namespace_name =
            pdxe_mem_strdup(PDXE_MEM_CLASS_OTHER, result->namespace_name);
    }
    atomic_fetch_add_explicit(&sp->parked, 1, memory_order_relaxed);
    atomic_fetch_add_explicit(&sp->bytes, (int64_t)(sizeof(hdr) + hdr.block_len),
                              memory_order_relaxed);
    pdxe_free_result(result);
    return true;
}

PDXEFileResult *pdxe_result_spill_load(const pdxe_result_spill_t *sp, int slot) {
    if (!pdxe_result_spill_has(sp, slot)) {
        return NULL;
    }
    const spill_slot_t *s = &sp->slots[slot];
    spill_file_t *f = &sp->files[s->writer];
    spill_rec_hdr_t hdr;
    pdxe_mutex_lock(&f->mu);
    bool ok = SPILL_SEEK(f->fp, s->offset) == 0 && fread(&hdr, sizeof(hdr), 1, f->fp) == 1 &&
              hdr.magic == SPILL_MAGIC;
    PDXEFileResult *r = NULL;
    if (ok) {
        r = pdxe_result_alloc();
        if (r) {
            *r = hdr.header;
            r->cached_tree = NULL;
            memset(&r->arena, 0, sizeof(r->arena));
            pdxe_arena_init_exact(&r->arena, hdr.block_len ? (size_t)hdr.block_len : 1);
            if (r->arena.nblocks == 1 &&
                (hdr.block_len == 0 ||
                 fread(r->arena.blocks[0], (size_t)hdr.block_len, 1, f->fp) == 1)) {
                r->arena.used = (size_t)hdr.block_len;
                r->arena.total_alloc = (size_t)hdr.block_len;
            } else {
                ok = false;
            }
        } else {
            ok = false;
        }
    }
    pdxe_mutex_unlock(&f->mu);
    if (!ok) {
        pdxe_log_warn("mem.spill.read_failed", "path", f->path);
        if (r) {
            pdxe_free_result(r);
        }
        return NULL;
    }
    pdxe_result_relocate(r, (const char *)(uintptr_t)hdr.old_base, (size_t)hdr.block_len,
                        r->arena.blocks[0]);
    atomic_fetch_add_explicit((_Atomic int64_t *)&sp->loads, 1, memory_order_relaxed);
    return r;
}

bool pdxe_result_spill_peek_header(const pdxe_result_spill_t *sp, int slot, PDXEFileResult *out) {
    if (!out || !pdxe_result_spill_has(sp, slot)) {
        return false;
    }
    const spill_slot_t *s = &sp->slots[slot];
    spill_file_t *f = &sp->files[s->writer];
    spill_rec_hdr_t hdr;
    pdxe_mutex_lock(&f->mu);
    bool ok = SPILL_SEEK(f->fp, s->offset) == 0 && fread(&hdr, sizeof(hdr), 1, f->fp) == 1 &&
              hdr.magic == SPILL_MAGIC;
    pdxe_mutex_unlock(&f->mu);
    if (ok) {
        *out = hdr.header;
    }
    return ok;
}

void pdxe_result_spill_peek_counts(const pdxe_result_spill_t *sp, int slot, int *defs, int *impls) {
    PDXEFileResult hdr;
    bool ok = pdxe_result_spill_peek_header(sp, slot, &hdr);
    if (defs) {
        *defs = ok ? hdr.defs.count : 0;
    }
    if (impls) {
        *impls = ok ? hdr.impl_traits.count : 0;
    }
}

void pdxe_result_spill_stats(const pdxe_result_spill_t *sp, int64_t *parked, int64_t *bytes,
                            int64_t *loads) {
    if (parked) {
        *parked = sp ? atomic_load_explicit(&sp->parked, memory_order_relaxed) : 0;
    }
    if (bytes) {
        *bytes = sp ? atomic_load_explicit(&sp->bytes, memory_order_relaxed) : 0;
    }
    if (loads) {
        *loads = sp ? atomic_load_explicit(&sp->loads, memory_order_relaxed) : 0;
    }
}

const char *pdxe_result_spill_namespace(const pdxe_result_spill_t *sp, int slot) {
    if (!sp || !sp->slots || slot < 0 || slot >= sp->slot_count) {
        return NULL;
    }
    return sp->slots[slot].namespace_name;
}

void pdxe_result_spill_close(pdxe_result_spill_t *sp) {
    if (!sp) {
        return;
    }
    if (sp->files) {
        for (int w = 0; w < sp->writers; w++) {
            spill_file_t *f = &sp->files[w];
            if (f->fp) {
                (void)fclose(f->fp);
                if (!f->unlinked) { /* POSIX already removed it at open */
                    (void)pdxe_unlink(f->path);
                }
                pdxe_mutex_destroy(&f->mu);
            }
        }
        pdxe_free(PDXE_MEM_CLASS_OTHER, sp->files);
    }
    if (sp->slots) {
        for (int i = 0; i < sp->slot_count; i++) {
            pdxe_free(PDXE_MEM_CLASS_OTHER, sp->slots[i].namespace_name);
        }
    }
    pdxe_free(PDXE_MEM_CLASS_OTHER, sp->slots);
    pdxe_free(PDXE_MEM_CLASS_OTHER, sp);
}
