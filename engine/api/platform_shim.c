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

#include "pdxe_core.h"
#include "foundation/mem_events.h"
#include "foundation/platform.h"
#include "foundation/secure_random.h"

#ifdef _WIN32
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <bcrypt.h> /* BCryptGenRandom */
#include <limits.h> /* ULONG_MAX */
#endif

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
 * Two of its calls sit outside those hooks, so they need a body: the flush, and the
 * end of a thread, which only the Windows thread-exit callback reports.
 */

void pdxe_memev_flush_thread(void) {
}

#ifdef _WIN32
void pdxe_memev_thread_end(void) {
}
#endif

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

#ifdef _WIN32
/*
 * Fills a buffer from the operating system's cryptographic random source.
 *
 * Only Windows asks: there the foundation names the temporary directories and
 * files it creates itself, from random characters, where other systems have
 * mkdtemp and mkstemp do it. A name drawn from a predictable source could be
 * claimed by another process first, so it must come from this one. Returns false,
 * with the buffer unspecified, if the source fails.
 */
bool pdxe_secure_random(void *buffer, size_t length) {
    if (buffer == NULL && length != 0) {
        return false;
    }
    unsigned char *cursor = (unsigned char *)buffer;
    while (length > 0) {
        /* The call takes a 32-bit length; a larger request is filled in parts. A
         * negative status is a failure. */
        ULONG chunk = length > ULONG_MAX ? ULONG_MAX : (ULONG)length;
        if (BCryptGenRandom(NULL, cursor, chunk, BCRYPT_USE_SYSTEM_PREFERRED_RNG) < 0) {
            return false;
        }
        cursor += chunk;
        length -= chunk;
    }
    return true;
}
#endif
