/*
 * The languages the interface's tests cover: the language matrix, and the one
 * variant of it (tsx) the interface names separately. One list for every C test, so
 * the tests cannot disagree about what the engine is expected to parse.
 *
 * The project's own registry of the matrix, with each language's extensions and rules,
 * is the language matrix task's (P1-07); this list is the tests' until then.
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
