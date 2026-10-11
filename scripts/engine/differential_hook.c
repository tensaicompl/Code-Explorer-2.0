/*
 * The reference engine's side of the engine differential (P2-16, decision 50): an
 * observation point, linked into a build of the pinned, unmodified reference engine,
 * that prints the rows engine/tests/differential_rows.h defines for every file of the
 * run, at the moment the reference's calls pass starts: after its cross-file typed
 * pass has written its answers into each file's result, and before anything turns
 * them into graph edges. The rows are exactly what differential_dump prints for the
 * vendored engine after its typed pass.
 *
 * Nothing here is compiled into this project, and nothing in the reference is
 * edited. scripts/engine/differential.py translates this file's names, and the rows
 * header's, back to the reference's with the vendoring's own rename map
 * (scripts/vendor/rename-map.txt, inverted), and the reference's own build compiles
 * the translation into its binary, outside the repository and the reference's
 * checkout, with the linker told to send the calls pass's entry here first
 * (--wrap). This then hands over to the pass unchanged.
 *
 * The rows go to the file PDX_DIFFERENTIAL_ROWS names; without it, nothing is
 * written. A file the run has no result for is a row saying so, which the harness
 * refuses.
 */

#include <stdio.h>
#include <stdlib.h>

#include "pdxe_core.h"
#include "pipeline/pipeline_internal.h"

#include "differential_rows.h"

#define DIFFERENTIAL_WRAPPED(name) DIFFERENTIAL_WRAPPED_(name)
#define DIFFERENTIAL_WRAPPED_(name) __wrap_##name
#define DIFFERENTIAL_REAL(name) DIFFERENTIAL_REAL_(name)
#define DIFFERENTIAL_REAL_(name) __real_##name

int DIFFERENTIAL_REAL(pdxe_pipeline_pass_calls)(pdxe_pipeline_ctx_t *ctx,
                                                const pdxe_file_info_t *files, int file_count);
int DIFFERENTIAL_WRAPPED(pdxe_pipeline_pass_calls)(pdxe_pipeline_ctx_t *ctx,
                                                   const pdxe_file_info_t *files,
                                                   int file_count);

int DIFFERENTIAL_WRAPPED(pdxe_pipeline_pass_calls)(pdxe_pipeline_ctx_t *ctx,
                                                   const pdxe_file_info_t *files,
                                                   int file_count) {
    const char *path = getenv("PDX_DIFFERENTIAL_ROWS");
    if (path && path[0]) {
        FILE *o = fopen(path, "wx");
        if (!o) {
            fprintf(stderr, "differential hook: cannot create the rows file\n");
            exit(3);
        }
        fputs("{\"c\":\"project\",\"name\":", o);
        dr_str(o, ctx ? ctx->project_name : NULL);
        fprintf(o, ",\"files\":%d}\n", file_count);
        for (int i = 0; i < file_count; i++) {
            const PDXEFileResult *r = ctx && ctx->result_cache ? ctx->result_cache[i] : NULL;
            if (r) {
                diff_rows_file(o, files[i].rel_path, r);
            } else {
                dr_begin(o, "missing", files[i].rel_path);
                fputs("}\n", o);
            }
        }
        if (fclose(o) != 0) {
            fprintf(stderr, "differential hook: cannot write the rows file\n");
            exit(3);
        }
    }
    return DIFFERENTIAL_REAL(pdxe_pipeline_pass_calls)(ctx, files, file_count);
}
