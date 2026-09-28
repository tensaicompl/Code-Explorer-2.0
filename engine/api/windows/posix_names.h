/*
 * The POSIX names the engine's sources use, for the Microsoft C runtime.
 *
 * The reference builds for Windows against a runtime that provides these names
 * itself. Rust links against Microsoft's, which provides the same functions under
 * other names and one type not at all. This header, included ahead of every engine
 * source when building for that runtime (engine/CMakeLists.txt), supplies the names,
 * so the vendored sources compile as they are written.
 */
#ifndef PDXE_WINDOWS_POSIX_NAMES_H
#define PDXE_WINDOWS_POSIX_NAMES_H

#include <stddef.h>
#include <string.h>

/* A signed size. Microsoft's runtime has the concept but not the name. */
typedef ptrdiff_t ssize_t;

/* Case-insensitive comparison. */
#define strcasecmp _stricmp
#define strncasecmp _strnicmp

/* Re-entrant tokenising. Microsoft's strtok_s has exactly strtok_r's signature. */
#define strtok_r strtok_s

#endif /* PDXE_WINDOWS_POSIX_NAMES_H */
