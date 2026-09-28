/*
 * Stands in for the POSIX header on Microsoft's C runtime, which has none. The one
 * source that includes it outside its Windows branch uses nothing from it; a use
 * added later fails to compile rather than resolving to something wrong.
 */
#ifndef PDXE_WINDOWS_UNISTD_H
#define PDXE_WINDOWS_UNISTD_H
#endif
