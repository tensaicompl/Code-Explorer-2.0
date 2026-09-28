/*
 * Stands in for the POSIX threads header on Microsoft's C runtime, which has none.
 * On Windows the engine's thread layer uses the operating system's own threads and
 * calls nothing from this header; one source includes it outside its Windows branch
 * all the same. It declares nothing, so a real use fails to compile rather than
 * resolving to something wrong.
 */
#ifndef PDXE_WINDOWS_PTHREAD_H
#define PDXE_WINDOWS_PTHREAD_H
#endif
