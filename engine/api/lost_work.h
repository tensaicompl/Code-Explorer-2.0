/*
 * Work the engine could not do, counted where it happens.
 *
 * Typed resolution reaches deep into the vendored resolvers, which mostly answer a
 * failed allocation, or a work budget run dry, by doing less: an answer, a
 * definition, a candidate quietly goes missing. None of that code knows which project
 * it is working for, and threading a project through every allocation in it is not
 * possible. Every allocation, though, goes through the engine's memory core or one of
 * the counting functions below, and every work budget runs out in one place each. So
 * those places count, on the thread they run on, and a run takes the difference
 * between its start and its end into its own record (pdxe_run_health). The engine
 * runs a project on one thread from start to end, so the difference is that run's,
 * and no other's.
 *
 * Not every failure is counted: one that only makes the engine slower, or that falls
 * back to the complete answer, is not lost work, and such places allocate without
 * these functions on purpose. engine/patches/README.md and docs/plan/ISSUES.md
 * (issue 21) say which are which.
 */
#ifndef PDXE_LOST_WORK_H
#define PDXE_LOST_WORK_H

#include <stddef.h>
#include <stdint.h>

/* An allocation failed. The memory core calls this; so do the functions below. */
void pdxe_lost_allocation(void);

/* A work budget ran out, so work that could have produced evidence was not done. */
void pdxe_lost_work(void);

/* What this thread has lost so far. A run subtracts its start from its end. */
typedef struct {
    uint64_t allocations;
    uint64_t work;
} pdxe_losses_t;

pdxe_losses_t pdxe_losses(void);

/* The C library's allocators, counting their failures. For code that allocates
 * outside the memory core where a failure loses work. Memory from them is freed with
 * free() as usual. */
void *pdxe_counted_malloc(size_t size);
void *pdxe_counted_calloc(size_t count, size_t size);
void *pdxe_counted_realloc(void *block, size_t size);
char *pdxe_counted_strdup(const char *s);
char *pdxe_counted_strndup(const char *s, size_t len);

#endif /* PDXE_LOST_WORK_H */
