/*
 * Implementations the engine expects and the vendored tree does not carry.
 *
 * The reuse map leaves out whole subsystems: the store that spills results to
 * disk, the memory instrumentation, the extractors performed in Rust, and the
 * helpers that reach for the operating system. Their callers remain, because
 * those callers do other work we want. This file answers each call in the way
 * that matches how the engine is used here.
 *
 * Every answer below is a decision, not a placeholder, and says which.
 */

#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

#if defined(__unix__) || defined(__APPLE__)
#include <unistd.h>
#endif

#include "lz4/lz4.h"
#include "pdxe_core.h"
#include "foundation/platform.h"

/* --- extractors performed elsewhere -------------------------------------- */
/*
 * Two extractors are not part of this engine: the graph they would contribute to
 * is built from the same files by other means, in a language better suited to
 * reading configuration. Their call sites stay so that the extraction walk keeps
 * its shape; they contribute nothing.
 */

void pdxe_extract_dbt(PDXEExtractCtx *ctx) {
    (void)ctx;
}

void pdxe_extract_k8s(PDXEExtractCtx *ctx) {
    (void)ctx;
}

/* --- results are never spilled to disk ----------------------------------- */
/*
 * The engine can park a result as an image on disk and read it back under memory
 * pressure. Here results live as long as the file that produced them and no
 * longer, and the caller decides how many files to hold at once, so nothing is
 * ever parked.
 *
 * Reporting no free space is how that is expressed: the caller reads zero as
 * unknown and leaves its spill budget at zero, which is the same decision arrived
 * at through the code path that already exists rather than through a new one.
 */

size_t pdxe_fs_free_bytes(const char *path) {
    (void)path;
    return 0;
}

/*
 * Relocating a parked result to a new base address is meaningless when nothing is
 * parked. Reached only through the read-back path, which the above disables.
 */
void pdxe_result_relocate(PDXEFileResult *result, const char *old_base, size_t len,
                          char *new_base) {
    (void)result;
    (void)old_base;
    (void)len;
    (void)new_base;
}

/* Compaction belongs to the same subsystem: there is no per-thread state to release. */
void pdxe_result_compact_release_thread(void) {
}

/* --- memory instrumentation ---------------------------------------------- */
/*
 * The engine can record where memory goes and flush that record per thread. The
 * header's hooks compile to nothing unless the feature is enabled, and it is not:
 * measuring the engine's allocator is not how this project measures anything.
 * These two are called outside those hooks, so they need a body.
 */

void pdxe_memev_flush_thread(void) {
}

void pdxe_memev_phase(const char *label) {
    (void)label;
}

/* --- helpers that reach for the operating system -------------------------- */

/*
 * Overwrites storage that held something sensitive.
 *
 * Written through a volatile pointer so the compiler may not treat it as a dead
 * store and remove it, which is the whole point and the reason this cannot be a
 * plain memset.
 */
void pdxe_secure_zero(void *buffer, size_t length) {
    if (buffer == NULL || length == 0) {
        return;
    }
    volatile unsigned char *p = (volatile unsigned char *)buffer;
    while (length-- > 0) {
        *p++ = 0;
    }
}

/*
 * Physical memory, in bytes, or zero when it cannot be determined.
 *
 * Callers treat zero as unknown and fall back to their own budget, so a platform
 * this does not cover degrades rather than fails.
 */
size_t pdxe_system_available_ram(void) {
#if defined(_SC_PHYS_PAGES) && defined(_SC_PAGESIZE)
    long pages = sysconf(_SC_PHYS_PAGES);
    long page_size = sysconf(_SC_PAGESIZE);
    if (pages > 0 && page_size > 0) {
        return (size_t)pages * (size_t)page_size;
    }
#endif
    return 0;
}

/*
 * Cores and memory.
 *
 * The distinction between performance and efficiency cores exists for one
 * platform's scheduler. Nothing here schedules by core type, so both counts are
 * the same number, and the field keeps its meaning for a caller that checks it.
 */
pdxe_system_info_t pdxe_system_info(void) {
    pdxe_system_info_t info;
    memset(&info, 0, sizeof(info));
    info.total_cores = pdxe_nprocs();
    if (info.total_cores < 1) {
        info.total_cores = 1;
    }
    info.perf_cores = info.total_cores;
    info.total_ram = pdxe_system_available_ram();
    return info;
}

/* --- compression ---------------------------------------------------------- */
/*
 * The matcher keeps its tables compressed. The compression library is vendored
 * alongside the engine, so this is a thin call through to it rather than a
 * decision: the engine's own wrapper lived in the store, which is not vendored.
 */

int pdxe_lz4_decompress(const char *src, int src_len, char *dst, int original_len) {
    if (src == NULL || dst == NULL || src_len <= 0 || original_len <= 0) {
        return -1;
    }
    return LZ4_decompress_safe(src, dst, src_len, original_len);
}
