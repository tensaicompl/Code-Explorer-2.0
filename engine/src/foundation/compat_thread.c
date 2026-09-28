/*
 * compat_thread.c — Portable thread, mutex, and aligned allocation.
 *
 * POSIX: thin wrappers around pthreads and posix_memalign.
 * Windows: CreateThread, CRITICAL_SECTION, aligned allocation.
 */
#include "foundation/mem_events.h"
#include "foundation/constants.h"
#include "foundation/compat_thread.h"

#include "foundation/platform.h"
#include "foundation/sanitized.h" /* PDXE_SANITIZED — diagnostic stack floor */

#include <mimalloc.h> /* mi_thread_done at thread exit */

#include <errno.h> /* EINVAL — a refused stack-size hint, see pdxe_thread_create */
#include <pthread.h>
#include <stdlib.h>

/* Default 8MB stack for all threads. macOS ARM64 default is only 512KB,
 * which is too small for deep pipeline passes (configlink, etc.). */
#define PDXE_DEFAULT_STACK_SIZE ((size_t)8 * PDXE_SZ_1K * PDXE_SZ_1K)

#include <string.h>

/* Thread stacks are sized in code (explicitly by most callers, by the default
 * above otherwise), so RLIMIT_STACK does NOT reach them and a sanitizer lane
 * cannot size them with ulimit. MemorySanitizer's origin tracking inflates
 * every frame enough that deep parser recursion overflows the normal worker
 * stack, and without this hook that lane cannot run the parsing suites at all.
 *
 * The floor applies to EVERY thread — an earlier version only replaced the
 * DEFAULT, which silently did nothing for worker_pool/runtime/main, i.e. for
 * exactly the threads that overflow. Diagnostic builds only: the shipping
 * binary keeps its fixed, predictable stack sizes. */
static size_t pdxe_thread_stack_floor(size_t requested) {
#if PDXE_SANITIZED
    const char *env = getenv("PDX_ENGINE_THREAD_STACK_MB");
    if (env && env[0]) {
        char *end = NULL;
        unsigned long mb = strtoul(env, &end, 10);
        if (end && *end == '\0' && mb > 0 && mb <= 1024) {
            size_t floor_bytes = (size_t)mb * PDXE_SZ_1K * PDXE_SZ_1K;
            if (floor_bytes > requested) {
                return floor_bytes;
            }
        }
    }
#endif
    return requested;
}

static size_t pdxe_thread_default_stack_size(void) {
    return pdxe_thread_stack_floor(PDXE_DEFAULT_STACK_SIZE);
}

/* ── Thread ───────────────────────────────────────────────────── */

#ifdef _WIN32

typedef struct {
    void *(*fn)(void *);
    void *arg;
} win_thread_arg_t;

/* Release each thread's allocator heap at DLL_THREAD_DETACH.
 *
 * Doing this from the thread wrapper instead crashes: the wrapper returns
 * before the CRT's own thread teardown, and abandoning the heap there raced
 * with frees still in flight (daemon_ipc_wait_forever_is_interruptible
 * segfaulted, rc=139, reproducibly and only with the release enabled). A TLS
 * callback is the mechanism mimalloc itself uses under MSVC and runs after all
 * other thread cleanup, which is the only point where the heap is genuinely
 * unreferenced.
 *
 * Without any of this, a static MinGW link -- no DllMain, no TLS callback --
 * never releases a thread heap at all: 607 heaps after 300 requests, 170 MiB
 * held against a ~300 KiB live set, growing without bound (#581). POSIX gets
 * this from a pthread TSD destructor, which is why only Windows leaked.
 *
 * PDXE_MI_THREAD_DONE=0 disables it, so one binary can demonstrate both
 * behaviours rather than requiring a rebuild to establish causality. */
static bool thread_release_heap_enabled(void) {
    static int state = -1;
    if (state < 0) {
        char buf[PDXE_SZ_16];
        state =
            (pdxe_safe_getenv("PDX_ENGINE_MI_THREAD_DONE", buf, sizeof(buf), NULL) != NULL && buf[0] == '0')
                ? 0
                : 1;
    }
    return state == 1;
}

static void NTAPI pdxe_thread_detach_callback(PVOID handle, DWORD reason, PVOID reserved) {
    (void)handle;
    (void)reserved;
    if (reason == DLL_THREAD_DETACH) {
        pdxe_memev_thread_end(); /* waste-sanitizer thread state; no-op outside that build */
    }
    if (reason == DLL_THREAD_DETACH && thread_release_heap_enabled()) {
        mi_thread_done();
    }
}

/* Park the callback in .CRT$XLB, the table the loader walks. The linker only
 * emits a TLS directory when _tls_used is referenced, so anchor it. */
extern const IMAGE_TLS_DIRECTORY64 _tls_used;
static const void *const pdxe_tls_anchor __attribute__((used)) = &_tls_used;
__attribute__((section(".CRT$XLB"), used)) PIMAGE_TLS_CALLBACK pdxe_thread_detach_tls_cb =
    pdxe_thread_detach_callback;

static DWORD WINAPI win_thread_wrapper(LPVOID lpParam) {
    win_thread_arg_t *a = (win_thread_arg_t *)lpParam;
    void *(*fn)(void *) = a->fn;
    void *arg = a->arg;
    free(a);
    fn(arg);
    /* Release this thread's allocator heap before the thread dies.
     *
     * On POSIX a pthread TSD destructor calls this for us, which is why POSIX
     * stays flat. A static MinGW link has no DllMain and registers no TLS
     * callback, so nothing runs at thread exit and every thread leaves its
     * thread-heap behind for the life of the process: 607 heaps after 300
     * requests, 170 MiB committed against a live set of ~300 KiB, growing
     * without bound (#581). The heaps are invisible to mi_heap_visit -- which
     * only sees the main and calling heaps -- which is why the growth looked
     * like it belonged to no allocator at all. */
    return 0;
}

int pdxe_thread_create(pdxe_thread_t *t, size_t stack_size, void *(*fn)(void *), void *arg) {
    if (stack_size == 0) {
        stack_size = pdxe_thread_default_stack_size();
    } else {
        stack_size = pdxe_thread_stack_floor(stack_size);
    }
    win_thread_arg_t *a = (win_thread_arg_t *)malloc(sizeof(win_thread_arg_t));
    if (!a) {
        return PDXE_NOT_FOUND;
    }
    a->fn = fn;
    a->arg = arg;
    t->handle = CreateThread(NULL, stack_size, win_thread_wrapper, a, 0, NULL);
    if (!t->handle) {
        free(a);
        return PDXE_NOT_FOUND;
    }
    return 0;
}

int pdxe_thread_join(pdxe_thread_t *t) {
    if (WaitForSingleObject(t->handle, INFINITE) != WAIT_OBJECT_0) {
        return PDXE_NOT_FOUND;
    }
    CloseHandle(t->handle);
    t->handle = NULL;
    return 0;
}

int pdxe_thread_detach(pdxe_thread_t *t) {
    if (t->handle) {
        CloseHandle(t->handle);
        t->handle = NULL;
    }
    return 0;
}

#else /* POSIX */

int pdxe_thread_create(pdxe_thread_t *t, size_t stack_size, void *(*fn)(void *), void *arg) {
    if (stack_size == 0) {
        stack_size = pdxe_thread_default_stack_size();
    } else {
        stack_size = pdxe_thread_stack_floor(stack_size);
    }
    pthread_attr_t attr;
    pthread_attr_init(&attr);
    pthread_attr_setstacksize(&attr, stack_size);
    int rc = pthread_create(&t->handle, &attr, fn, arg);
    pthread_attr_destroy(&attr);
    if (rc == EINVAL) {
        /* glibc carves the static TLS block out of the thread's own stack
         * allocation, so a small REQUESTED stack stops being legal the moment
         * the image's TLS grows — no warning where the growth happens, only
         * EINVAL here, from then on. That is exactly how the 64 KB
         * parent-death watchdog stopped starting once this image's TLS passed
         * it (PR #2233): the worker then refused to index without containment
         * and SIGKILLed its own group, so every venue reported nothing but
         * "killed (signal 9)".
         * A stack size is a hint about how much this thread needs; the platform
         * refusing the hint is not a reason to fail to create the thread. Fall
         * back to the default stack, which always has room for the TLS block. */
        pthread_attr_t fallback;
        pthread_attr_init(&fallback);
        pthread_attr_setstacksize(&fallback, pdxe_thread_default_stack_size());
        rc = pthread_create(&t->handle, &fallback, fn, arg);
        pthread_attr_destroy(&fallback);
    }
    return rc;
}

int pdxe_thread_join(pdxe_thread_t *t) {
    int rc = pthread_join(t->handle, NULL);
    if (rc == 0) {
        memset(&t->handle, 0, sizeof(t->handle));
    }
    return rc;
}

int pdxe_thread_detach(pdxe_thread_t *t) {
    int rc = pthread_detach(t->handle);
    if (rc == 0) {
        memset(&t->handle, 0, sizeof(t->handle));
    }
    return rc;
}

#endif

/* ── Mutex ────────────────────────────────────────────────────── */

#ifdef _WIN32

void pdxe_mutex_init(pdxe_mutex_t *m) {
    InitializeCriticalSection(&m->cs);
}

void pdxe_mutex_lock(pdxe_mutex_t *m) {
#if defined(PDXE_MEMWASTE) && PDXE_MEMWASTE
    if (pdxe_memev_enabled()) {
        bool contended = !TryEnterCriticalSection(&m->cs);
        if (contended) {
            EnterCriticalSection(&m->cs);
        }
        pdxe_work_note(PDXE_WORK_MUTEX, __builtin_return_address(0), 0, contended ? 1 : 0, 0);
        return;
    }
#endif
    EnterCriticalSection(&m->cs);
}

void pdxe_mutex_unlock(pdxe_mutex_t *m) {
    LeaveCriticalSection(&m->cs);
}

void pdxe_mutex_destroy(pdxe_mutex_t *m) {
    DeleteCriticalSection(&m->cs);
}

#else /* POSIX */

void pdxe_mutex_init(pdxe_mutex_t *m) {
    pthread_mutex_init(&m->mtx, NULL);
}

void pdxe_mutex_lock(pdxe_mutex_t *m) {
#if defined(PDXE_MEMWASTE) && PDXE_MEMWASTE
    /* pthread_mutex_lock itself counts in this flavour; going through the same
     * lock with our caller as the site keeps the attribution and counts once. */
    (void)pdxe_memev_mutex_lock(&m->mtx, __builtin_return_address(0));
#else
    pthread_mutex_lock(&m->mtx);
#endif
}

void pdxe_mutex_unlock(pdxe_mutex_t *m) {
    pthread_mutex_unlock(&m->mtx);
}

void pdxe_mutex_destroy(pdxe_mutex_t *m) {
    pthread_mutex_destroy(&m->mtx);
}

#endif

/* ── Aligned allocation ───────────────────────────────────────── */

#ifdef _WIN32

int pdxe_aligned_alloc(void **ptr, size_t alignment, size_t size) {
#if defined(PDXE_MEM_GLOBAL_OVERRIDE) && PDXE_MEM_GLOBAL_OVERRIDE
    *ptr = mi_malloc_aligned(size, alignment);
#else
    *ptr = _aligned_malloc(size, alignment);
#endif
    return *ptr ? 0 : -1;
}

void pdxe_aligned_free(void *ptr) {
#if defined(PDXE_MEM_GLOBAL_OVERRIDE) && PDXE_MEM_GLOBAL_OVERRIDE
    mi_free(ptr);
#else
    _aligned_free(ptr);
#endif
}

#else /* POSIX */

int pdxe_aligned_alloc(void **ptr, size_t alignment, size_t size) {
    return posix_memalign(ptr, alignment, size);
}

void pdxe_aligned_free(void *ptr) {
    free(ptr);
}

#endif
