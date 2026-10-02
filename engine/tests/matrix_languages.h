/*
 * The languages the interface's tests cover: the language matrix, and the one
 * variant of it (tsx) the interface names separately. One list for every C test, so
 * the tests cannot disagree about what the engine is expected to parse.
 *
 * The matrix itself, with each language's extensions and rules, is pdx_core::languages.
 * C tests cannot read it, so this list stays: the matrix's ids, and the engine grammars
 * named otherwise than their language (tsx). A Rust test in that crate keeps the two
 * equal (engine_test_languages_are_the_matrix_and_its_dialects).
 */
#ifndef PDXE_TEST_MATRIX_LANGUAGES_H
#define PDXE_TEST_MATRIX_LANGUAGES_H

static const char *const MATRIX_LANGUAGES[] = {
    "java",       "kotlin",   "scala",   "typescript", "tsx",  "javascript", "python", "go",
    "c",          "cpp",      "csharp",  "rust",       "php",  "perl",       "ada",    "bash",
    "ruby",       "swift",    "objc",    "groovy",     "lua",  "sql",        "protobuf",
    "graphql",    "yaml",     "json",    "toml",       "hcl",  "dockerfile", "markdown",
    "xml",        "properties",
};
enum { N_MATRIX_LANGUAGES = sizeof(MATRIX_LANGUAGES) / sizeof(MATRIX_LANGUAGES[0]) };

#endif /* PDXE_TEST_MATRIX_LANGUAGES_H */
