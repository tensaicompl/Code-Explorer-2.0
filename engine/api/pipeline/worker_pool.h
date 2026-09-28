/*
 * The reference spreads work across a pool of its own. Parallelism here belongs
 * to the caller, which runs one engine context per thread, so the loop below runs
 * in order and the shape of the call site is preserved.
 */
#ifndef PDXE_WORKER_POOL_FORWARD_H
#define PDXE_WORKER_POOL_FORWARD_H

#include <stdbool.h>
#include <stddef.h>

/* Visits one index. The signature is the reference's, unchanged. */
typedef void (*pdxe_parallel_fn)(int idx, void *ctx);

/*
 * Field for field the reference's layout, in the reference's order. A caller may
 * initialise this positionally, so an extra or reordered field here would compile
 * and silently assign a value to the wrong member.
 */
typedef struct {
    int max_workers;     /* ignored: every loop runs in order */
    bool force_pthreads; /* ignored: kept because callers set it */
} pdxe_parallel_for_opts_t;

void pdxe_parallel_for(int count, pdxe_parallel_fn fn, void *ctx, pdxe_parallel_for_opts_t opts);

#endif /* PDXE_WORKER_POOL_FORWARD_H */
