/*
 * The reference declares its language lookup in its discovery header, beside file
 * discovery, which this project does in Rust. Only the lookup is vendored
 * (src/resolve/language_lookup.c), so this declares only the lookup.
 */
#ifndef PDXE_DISCOVER_FORWARD_H
#define PDXE_DISCOVER_FORWARD_H

#include "pdxe_core.h"

/* The language an extension such as ".go" names, or the language count when none. */
PDXELanguage pdxe_language_for_extension(const char *ext);

/* The language a file name implies, special names first, or the language count. */
PDXELanguage pdxe_language_for_filename(const char *filename);

#endif
