/*
 * The reference keeps its cross-file resolution under a pipeline directory and
 * its sources address each other through that path. This project keeps them
 * beside the rest of the engine, so this forwards the name to where the file
 * actually is, rather than rewriting the includes inside code we do not edit.
 */
#ifndef PDXE_PIPELINE_PASS_LSP_CROSS_FORWARD_H
#define PDXE_PIPELINE_PASS_LSP_CROSS_FORWARD_H
#include "resolve/pass_lsp_cross.h"
#endif
