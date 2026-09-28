/*
 * The per-user language configuration the reference's language lookup consults
 * before its own tables. In this project there is none: the language matrix is
 * fixed, and a change to it is a specification change, not a setting. The shim
 * reports no configuration, so the lookup answers from its tables alone, exactly as
 * the reference does when no configuration file exists.
 */
#ifndef PDXE_USERCONFIG_FORWARD_H
#define PDXE_USERCONFIG_FORWARD_H

#include "pdxe_core.h"

typedef struct pdxe_userconfig pdxe_userconfig_t;

/* Always NULL here: no configuration is ever loaded. */
const pdxe_userconfig_t *pdxe_get_user_lang_config(void);

/* The language a configuration maps an extension to; never reached, as there is none. */
PDXELanguage pdxe_userconfig_lookup(const pdxe_userconfig_t *cfg, const char *ext);

#endif
