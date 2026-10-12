#ifndef PDXE_PREPROCESSOR_H
#define PDXE_PREPROCESSOR_H

#include <stddef.h>
#include <stdint.h>

/* #1989: build-system export macros (UE "<MOD>_API", CMake generate_export_header
 * "<lib>_EXPORT", ...) are defined empty on the real compile command line but are
 * invisible to tree-sitter, so `class MOD_API Foo` misparses with the macro token
 * as the type name. The preprocessed second pass predefines the conventional
 * export-macro-shaped identifiers found in a source file as empty, mirroring the
 * real build line. The candidate list is bounded. */
#define PDXE_EXPORT_MACRO_MAX 32
#define PDXE_EXPORT_MACRO_NAME_MAX 96

/* The ceilings of one file's preprocessing (docs/plan/ISSUES.md, issue 87; decision 56).
 * They are counts derived from the source's length and these constants alone, never a
 * time or the machine's memory, so every machine stops a file at the same point. The
 * byte budget is 16 bytes for every source byte, at least 256 KiB and at most 8 MiB
 * (pdxe_preprocess_budget_for_source_len). It bounds, each on its own:
 *   - the text of the tokens macro expansion creates, its temporaries included;
 *   - the expanded text written out, line padding and #line markers included;
 *   - the tokens #if and #elif folding steps over.
 * Macro expansion may also create at most PDXE_PREPROCESS_MAX_GENERATED_TOKENS tokens,
 * expand at most PDXE_PREPROCESS_MAX_EXPANSIONS macros (an empty one creates nothing,
 * but is work) and nest at most PDXE_PREPROCESS_MAX_EXPANSION_DEPTH deep, and a forward
 * #line jump of more than PDXE_PREPROCESS_MAX_LINE_PADDING lines is written as a #line
 * marker rather than a newline per line. Passing a ceiling is lost work: the expansion
 * is discarded and the file is extracted from its source alone. */
#define PDXE_PREPROCESS_EXPANSION_FACTOR 16
#define PDXE_PREPROCESS_MIN_EXPANDED_BYTES (256u * 1024u)
#define PDXE_PREPROCESS_MAX_EXPANDED_BYTES (8u * 1024u * 1024u)
#define PDXE_PREPROCESS_MAX_GENERATED_TOKENS 500000u
#define PDXE_PREPROCESS_MAX_EXPANSIONS 500000u
#define PDXE_PREPROCESS_MAX_EXPANSION_DEPTH 256u
#define PDXE_PREPROCESS_MAX_LINE_PADDING 4096u

#ifdef __cplusplus
extern "C" {
#endif

/* What one call of pdxe_preprocess_with_map used, and whether it passed a ceiling. */
typedef struct {
    size_t budget;              /* the byte and step budget of this source */
    size_t generated_tokens;
    size_t generated_bytes;
    size_t serialized_bytes;
    size_t condition_steps;
    size_t expansions;          /* macro expansions, nested ones included */
    size_t line_markers;        /* #line markers written in place of padding */
    unsigned int deepest_expansion;
    unsigned int longest_line_jump;
    int exhausted;              /* nonzero: a ceiling was passed, and NULL returned */
    const char *ceiling;        /* which, when exhausted */
} PDXEPreprocessStats;

/* The byte and step budget of a source of `source_len` bytes. */
size_t pdxe_preprocess_budget_for_source_len(int source_len);

/* What the last pdxe_preprocess_with_map on this thread used. */
const PDXEPreprocessStats *pdxe_preprocess_last_stats(void);

typedef struct {
    char *source;
    uint32_t *original_line_by_expanded_line; // 1-based; 0 means directive/unmapped.
    uint8_t *belongs_to_main_file;            // 1-based; true only for the original input file.
    int expanded_line_count;
} PDXEPreprocessedSource;

// Collect conventional export-macro-shaped identifiers (ALL_CAPS ending in
// _API/_EXPORT/_IMPORT/_DLLEXPORT/_DEPRECATED) from the source, up to max_out
// unique names. Returns the number of names stored in out.
int pdxe_export_macro_candidates(const char *source, int source_len,
                                char (*out)[PDXE_EXPORT_MACRO_NAME_MAX], int max_out);

// Preprocess C/C++ source: expand the macros and evaluate the conditionals it defines
// itself. No file is opened, so no #include is ever resolved.
// Returns malloc-allocated expanded source, or NULL if no expansion needed/on failure.
// extra_defines: NULL-terminated array of "NAME=VALUE" strings (can be NULL).
// include_paths: NULL-terminated array of directory paths (can be NULL), never searched.
// The returned string must be freed with pdxe_preprocess_free().
char *pdxe_preprocess(const char *source, int source_len, const char *filename,
                     const char **extra_defines, const char **include_paths, int cpp_mode);

// Preprocess and return source plus expanded-line -> original-line ownership map.
// Returns NULL if no expansion is needed, preprocessing fails or a ceiling is passed
// (pdxe_preprocess_last_stats says which).
// Free with pdxe_preprocessed_source_free().
PDXEPreprocessedSource *pdxe_preprocess_with_map(const char *source, int source_len,
                                               const char *filename, const char **extra_defines,
                                               const char **include_paths, int cpp_mode);

// Free preprocessed source returned by pdxe_preprocess.
void pdxe_preprocess_free(char *expanded);

// Free preprocessed source and line maps returned by pdxe_preprocess_with_map.
void pdxe_preprocessed_source_free(PDXEPreprocessedSource *pp);

#ifdef __cplusplus
}
#endif

#endif // PDXE_PREPROCESSOR_H
