/*
 * C and C++ extraction reads nothing but the source it is given (docs/plan/ISSUES.md,
 * issue 83; decision 51; engine/patches/0015-source-only-preprocessing.patch).
 *
 *     c_family_input_isolation
 *
 * The preprocessed pass of C, C++ and CUDA expands the source's macros and conditionals.
 * It used to load every file the source #included, and every file a __has_include named,
 * from disk, relative to the process's working directory: what an extraction held then
 * depended on where the process ran and on files nobody gave the engine, which Stage 2
 * never masked, which may lie outside the repository, and which no cache key names. The
 * source buffer is the whole input now: an include directive is still a fact of the
 * source, and a macro the source itself defines is still expanded, but no target is
 * opened.
 *
 * Every check builds its files in a fresh temporary directory, extracts the same bytes
 * with the working directory in several places, and compares every row the engine
 * differential prints (tests/differential_rows.h) and the file's surface, byte for byte:
 *
 *   - working directory: the repository root, holding the headers the sources include;
 *     an unrelated directory holding other headers at the same paths; an empty one. C
 *     and C++; a C++ source also asks __has_include.
 *   - a secret header: a source including a header on disk whose every kind of content
 *     (a macro, a function, a string, a nested include) carries a marker. No row and no
 *     surface may hold the marker, the result must be the result without the header, and
 *     on Linux inotify must see no file under the directory opened while extracting: not
 *     the header, not the source.
 *   - outside the repository: a source reaching a header above the repository root, by a
 *     relative and by an absolute path.
 *   - the supplied bytes: a file on disk and the buffer given for its path differ, with
 *     the same length; the source includes itself, as the X-macro idiom does. The result
 *     must be the buffer's, as if no file were on disk.
 *   - the include facts and the source's own preprocessing are kept: each include
 *     directive is still an import, and calls that appear only once the source's own
 *     macros and conditionals are expanded are still found.
 *
 * CUDA runs the same preprocessing, but it cannot be extracted here: the interface names
 * no CUDA, the language matrix has none, and no CUDA grammar is vendored.
 */

#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#ifdef __linux__
#include <sys/inotify.h>
#endif

#include "pdxe.h"
#include "pdxe_core.h"
#include "shim_internal.h"

#include "differential_rows.h"

static int failures = 0;

#define CHECK(cond, ...)                                                                           \
    do {                                                                                           \
        if (!(cond)) {                                                                             \
            printf("FAIL ");                                                                       \
            printf(__VA_ARGS__);                                                                   \
            printf("\n");                                                                          \
            failures++;                                                                            \
        }                                                                                          \
    } while (0)

static void die(const char *what, const char *detail) {
    printf("FAIL %s%s%s (%s)\n", what, detail ? ": " : "", detail ? detail : "",
           strerror(errno));
    exit(1);
}

/* --- files ------------------------------------------------------------------------ */

static char start_dir[4096];

static char *join(const char *dir, const char *rel) {
    size_t n = strlen(dir) + strlen(rel) + 2;
    char *path = malloc(n);
    if (!path) {
        die("out of memory", NULL);
    }
    snprintf(path, n, "%s/%s", dir, rel);
    return path;
}

/* A fresh directory of its own below the temporary directory. */
static char *fresh_dir(const char *name) {
    const char *tmp = getenv("TMPDIR");
    char template_path[4096];
    snprintf(template_path, sizeof(template_path), "%s/pdxe-%s-XXXXXX", tmp && *tmp ? tmp : "/tmp",
             name);
    if (!mkdtemp(template_path)) {
        die("cannot make a directory", template_path);
    }
    char *dir = strdup(template_path);
    if (!dir) {
        die("out of memory", NULL);
    }
    return dir;
}

/* Writes `text` at dir/rel, making the directories between. */
static void put(const char *dir, const char *rel, const char *text) {
    char *path = join(dir, rel);
    for (char *p = path + strlen(dir) + 1; *p; p++) {
        if (*p == '/') {
            *p = '\0';
            if (mkdir(path, 0700) != 0 && errno != EEXIST) {
                die("cannot make a directory", path);
            }
            *p = '/';
        }
    }
    FILE *f = fopen(path, "wb");
    if (!f || fwrite(text, 1, strlen(text), f) != strlen(text) || fclose(f) != 0) {
        die("cannot write", path);
    }
    free(path);
}

/* --- what an extraction holds ------------------------------------------------------ */

static pdxe_ctx *ctx = NULL;

/* Every row and the surface of one extraction, as one string. */
static char *extract_in(const char *cwd, int lang, const char *rel, const char *source) {
    if (chdir(cwd) != 0) {
        die("cannot enter", cwd);
    }
    pdxe_file_result *r = NULL;
    int rc = pdxe_extract_file(ctx, lang, rel, (const uint8_t *)source, strlen(source), &r);
    if (chdir(start_dir) != 0) {
        die("cannot return to", start_dir);
    }
    if (rc != PDXE_OK || !r) {
        printf("FAIL %s: the extraction failed (%d)\n", rel, rc);
        exit(1);
    }
    const PDXEFileResult *engine = pdxe_result_engine(r);
    uint8_t *surface = NULL;
    size_t surface_len = 0;
    if (!engine || pdxe_surface_export(r, rel, &surface, &surface_len) != PDXE_OK) {
        printf("FAIL %s: no result or no surface\n", rel);
        exit(1);
    }
    FILE *o = tmpfile();
    if (!o) {
        die("no temporary file", NULL);
    }
    diff_rows_file(o, rel, engine);
    fputs("{\"c\":\"surface\",\"json\":", o);
    fwrite(surface, 1, surface_len, o);
    fputs("}\n", o);
    long n = ftell(o);
    char *text = malloc((size_t)n + 1);
    if (!text || fseek(o, 0, SEEK_SET) != 0 || fread(text, 1, (size_t)n, o) != (size_t)n) {
        die("cannot read the rows back", NULL);
    }
    text[n] = '\0';
    fclose(o);
    pdxe_surface_free(surface);
    pdxe_result_free(ctx, r);
    return text;
}

/* The first line where two extractions differ, both sides. */
static void show_difference(const char *a, const char *b) {
    int line = 1;
    while (*a && *a == *b) {
        if (*a == '\n') {
            line++;
        }
        a++;
        b++;
    }
    while (line > 1 && a[-1] != '\n') {
        a--;
        b--;
    }
    printf("     at row %d:\n       %.*s\n       %.*s\n", line, (int)strcspn(a, "\n"), a,
           (int)strcspn(b, "\n"), b);
}

static void check_same(const char *what, const char *expected, const char *got) {
    if (strcmp(expected, got) != 0) {
        printf("FAIL %s\n", what);
        show_difference(expected, got);
        failures++;
    }
}

/* Whether some row is a call of `callee` that only the expanded text holds. */
static int has_expanded_call(const char *rows, const char *callee) {
    char needle[256];
    snprintf(needle, sizeof(needle), "\"callee\":\"%s\",", callee);
    for (const char *line = rows; *line;) {
        const char *end = strchr(line, '\n');
        size_t n = end ? (size_t)(end - line) : strlen(line);
        if (strncmp(line, "{\"c\":\"call\"", 11) == 0) {
            const char *hit = strstr(line, needle);
            if (hit && hit < line + n && strstr(hit, "\"origin\":1,") &&
                strstr(hit, "\"origin\":1,") < line + n) {
                return 1;
            }
        }
        line = end ? end + 1 : line + n;
    }
    return 0;
}

/* --- what is opened ---------------------------------------------------------------- */

/* Files opened or read below the watched directories while extracting: inotify on
 * Linux, where every open of a file in a watched directory is an event. Elsewhere the
 * results' equality is the whole check. */
typedef struct {
    int fd;
} watch;

static watch watch_dirs(const char **dirs, int n) {
    watch w = {-1};
#ifdef __linux__
    w.fd = inotify_init1(IN_NONBLOCK | IN_CLOEXEC);
    if (w.fd < 0) {
        die("no inotify", NULL);
    }
    for (int i = 0; i < n; i++) {
        if (inotify_add_watch(w.fd, dirs[i], IN_OPEN | IN_ACCESS) < 0) {
            die("cannot watch", dirs[i]);
        }
    }
#else
    (void)dirs;
    (void)n;
#endif
    return w;
}

/* The names opened since the watch began, or 0 with none. */
static int opened_since(watch w, const char *what) {
    int events = 0;
#ifdef __linux__
    char buf[4096] __attribute__((aligned(__alignof__(struct inotify_event))));
    for (;;) {
        ssize_t got = read(w.fd, buf, sizeof(buf));
        if (got <= 0) {
            break;
        }
        for (char *p = buf; p < buf + got;) {
            struct inotify_event *e = (struct inotify_event *)p;
            printf("FAIL %s: the engine opened %s\n", what, e->len ? e->name : "a watched directory");
            events++;
            p += sizeof(*e) + e->len;
        }
    }
    close(w.fd);
#else
    (void)w;
    (void)what;
#endif
    return events;
}

/* --- the checks --------------------------------------------------------------------- */

static const char C_SOURCE[] = "#include \"app.h\"\n"
                               "#include \"../include/shared.h\"\n"
                               "#define LOCAL_TWICE(x) local_twice_target(x)\n"
                               "int run(void) { return HEADER_CALL(1) + SHARED_CALL(2) + "
                               "LOCAL_TWICE(3); }\n";

static const char CPP_SOURCE[] = "#if __has_include(\"app.h\")\n"
                                 "#define PICK() has_header_branch()\n"
                                 "#else\n"
                                 "#define PICK() no_header_branch()\n"
                                 "#endif\n"
                                 "#include \"app.h\"\n"
                                 "#include \"../include/shared.h\"\n"
                                 "int run2() { return PICK() + HEADER_CALL(3) + SHARED_CALL(4); }\n";

/* The same bytes from the repository root, an unrelated directory with other headers at
 * the same paths, and an empty directory: one result. */
static void check_working_directory(void) {
    char *repo = fresh_dir("repo");
    char *other = fresh_dir("other");
    char *empty = fresh_dir("empty");
    put(repo, "src/app.h",
        "#define HEADER_CALL(x) header_target_repo(x)\n"
        "int header_defined_fn(void) { return header_inner_call(); }\n");
    put(repo, "include/shared.h", "#define SHARED_CALL(x) shared_target_repo(x)\n");
    put(other, "src/app.h", "#define HEADER_CALL(x) header_target_other(x)\n");
    put(other, "include/shared.h", "#define SHARED_CALL(x) shared_target_other(x)\n");

    struct {
        const char *name;
        int lang;
        const char *rel;
        const char *source;
    } cases[] = {
        {"C", pdxe_language_id("c"), "src/app.c", C_SOURCE},
        {"C++", pdxe_language_id("cpp"), "src/app.cpp", CPP_SOURCE},
    };
    for (size_t i = 0; i < sizeof(cases) / sizeof(cases[0]); i++) {
        char *in_empty = extract_in(empty, cases[i].lang, cases[i].rel, cases[i].source);
        char *in_repo = extract_in(repo, cases[i].lang, cases[i].rel, cases[i].source);
        char *in_other = extract_in(other, cases[i].lang, cases[i].rel, cases[i].source);
        char what[256];
        snprintf(what, sizeof(what), "%s: the repository root's headers changed the result",
                 cases[i].name);
        check_same(what, in_empty, in_repo);
        snprintf(what, sizeof(what), "%s: an unrelated directory's headers changed the result",
                 cases[i].name);
        check_same(what, in_empty, in_other);
        for (const char *const *marker =
                 (const char *const[]){"header_target", "shared_target", "header_inner_call",
                                       "header_defined_fn", NULL};
             *marker; marker++) {
            CHECK(!strstr(in_repo, *marker) && !strstr(in_other, *marker),
                  "%s: a header's %s reached the result", cases[i].name, *marker);
        }
        CHECK(!has_expanded_call(in_repo, "has_header_branch"),
              "%s: __has_include found a header on disk", cases[i].name);
        if (cases[i].source == C_SOURCE) {
            CHECK(has_expanded_call(in_repo, "local_twice_target"),
                  "%s: the source's own macro was not expanded", cases[i].name);
        } else {
            CHECK(has_expanded_call(in_repo, "no_header_branch"),
                  "%s: __has_include must find no header", cases[i].name);
        }
        free(in_empty);
        free(in_repo);
        free(in_other);
    }
    free(repo);
    free(other);
    free(empty);
}

/* A header on disk the engine was not given: nothing of it, and it is never opened. */
static void check_secret_header(void) {
    char *repo = fresh_dir("secret");
    char *empty = fresh_dir("secret-empty");
    put(repo, "src/secret_header.h",
        "#include \"ambient_marker_nested.h\"\n"
        "#define SOME_MACRO ambient_marker_call(\"AMBIENT_MARKER_STRING\")\n"
        "static const char *ambient_marker_value = \"AMBIENT_MARKER_VALUE\";\n"
        "int ambient_marker_def(void) { return ambient_marker_inner(\"AMBIENT_MARKER_ARG\"); }\n"
        "struct ambient_marker_type { int ambient_marker_field; };\n");
    put(repo, "src/ambient_marker_nested.h",
        "#define NESTED_MARKER ambient_marker_nested_call()\n"
        "int ambient_marker_nested_def(void) { return NESTED_MARKER; }\n");
    static const char source[] = "#include \"secret_header.h\"\n"
                                 "#define LOCAL_FLAG 1\n"
                                 "\n"
                                 "int f(void) {\n"
                                 "    return SOME_MACRO;\n"
                                 "}\n";
    put(repo, "src/vault.c", source);
    char *src_dir = join(repo, "src");
    const char *watched[] = {repo, src_dir};

    char *without = extract_in(empty, pdxe_language_id("c"), "src/vault.c", source);
    watch w = watch_dirs(watched, 2);
    char *with = extract_in(repo, pdxe_language_id("c"), "src/vault.c", source);
    int opened = opened_since(w, "a secret header");
    CHECK(opened == 0, "a secret header: %d files under the repository were opened", opened);
    check_same("a secret header: the header on disk changed the result", without, with);
    CHECK(!strstr(with, "ambient_marker") && !strstr(with, "AMBIENT_MARKER"),
          "a secret header: the header's content reached the result");
    CHECK(strstr(with, "\"module_path\":\"secret_header.h\"") != NULL,
          "a secret header: the include directive is no longer an import");
    free(without);
    free(with);
    free(src_dir);
    free(repo);
    free(empty);
}

/* A header above the repository root, reached by a relative and an absolute path. */
static void check_outside_repository(void) {
    char *root = fresh_dir("outside");
    char *empty = fresh_dir("outside-empty");
    char *repo = join(root, "repo");
    char *outside = join(root, "outside");
    put(root, "outside/external.h",
        "#define EXTERNAL_CALL(x) outside_marker_relative(x)\n"
        "int outside_marker_def(void) { return outside_marker_inner(); }\n");
    put(root, "outside/external_abs.h", "#define ABSOLUTE_CALL(x) outside_marker_absolute(x)\n");
    char source[8192];
    snprintf(source, sizeof(source),
             "#include \"../../outside/external.h\"\n"
             "#include \"%s/external_abs.h\"\n"
             "#define LOCAL_CALL(x) local_inside(x)\n"
             "int use(void) { return EXTERNAL_CALL(1) + ABSOLUTE_CALL(2) + LOCAL_CALL(3); }\n",
             outside);
    put(root, "repo/src/use.c", source);
    const char *watched[] = {outside};

    char *without = extract_in(empty, pdxe_language_id("c"), "src/use.c", source);
    watch w = watch_dirs(watched, 1);
    char *with = extract_in(repo, pdxe_language_id("c"), "src/use.c", source);
    int opened = opened_since(w, "outside the repository");
    CHECK(opened == 0, "outside the repository: %d files were opened", opened);
    check_same("outside the repository: a header above the root changed the result", without,
               with);
    CHECK(!strstr(with, "outside_marker"),
          "outside the repository: an outside header's content reached the result");
    CHECK(has_expanded_call(with, "local_inside"),
          "outside the repository: the source's own macro was not expanded");
    free(without);
    free(with);
    free(repo);
    free(outside);
    free(root);
    free(empty);
}

/* The buffer given for a path is the source, whatever the file at that path holds, even
 * when the source includes itself. */
static void check_supplied_bytes(void) {
    char *repo = fresh_dir("supplied");
    char *empty = fresh_dir("supplied-empty");
    static const char on_disk[] = "#define OWN() disk_own_target()\n"
                                  "#ifndef SELF_ONCE\n"
                                  "#define SELF_ONCE\n"
                                  "#include \"self.c\"\n"
                                  "int use(void) { return PICKED() + OWN(); }\n"
                                  "#else\n"
                                  "#define PICKED() disk_only_target()\n"
                                  "#endif\n";
    static const char supplied[] = "#define OWN() supp_own_target()\n"
                                   "#ifndef SELF_ONCE\n"
                                   "#define SELF_ONCE\n"
                                   "#include \"self.c\"\n"
                                   "int use(void) { return PICKED() + OWN(); }\n"
                                   "#else\n"
                                   "#define PICKED() supp_only_target()\n"
                                   "#endif\n";
    _Static_assert(sizeof(on_disk) == sizeof(supplied), "the same length");
    put(repo, "src/self.c", on_disk);
    char *src_dir = join(repo, "src");
    const char *watched[] = {src_dir};

    char *without = extract_in(empty, pdxe_language_id("c"), "src/self.c", supplied);
    watch w = watch_dirs(watched, 1);
    char *with = extract_in(repo, pdxe_language_id("c"), "src/self.c", supplied);
    int opened = opened_since(w, "the supplied bytes");
    CHECK(opened == 0, "the supplied bytes: %d files were opened", opened);
    check_same("the supplied bytes: the file on disk changed the result", without, with);
    CHECK(!strstr(with, "disk_"), "the supplied bytes: the file on disk's content was read");
    CHECK(has_expanded_call(with, "supp_own_target"),
          "the supplied bytes: the buffer's own macro was not expanded");
    free(without);
    free(with);
    free(src_dir);
    free(repo);
    free(empty);
}

/* What the source itself defines is still preprocessed, and its includes are facts. */
static void check_source_preprocessing(void) {
    char *empty = fresh_dir("local");
    static const char source[] = "#include \"local.h\"\n"
                                 "#include <stdio.h>\n"
                                 "#define FEATURE_LEVEL 2\n"
                                 "#define LOCAL_CALL(x) local_target(x)\n"
                                 "#if FEATURE_LEVEL > 1\n"
                                 "#define CHOOSE() chosen_target()\n"
                                 "#else\n"
                                 "#define CHOOSE() rejected_target()\n"
                                 "#endif\n"
                                 "#ifdef LOCAL_CALL\n"
                                 "#define WRAPPED() wrapped_target()\n"
                                 "#endif\n"
                                 "#ifndef NOT_DEFINED_HERE\n"
                                 "#define GUARDED() guarded_target()\n"
                                 "#endif\n"
                                 "int h(void) { return LOCAL_CALL(1) + CHOOSE() + WRAPPED() + "
                                 "GUARDED(); }\n";
    struct {
        const char *name;
        int lang;
        const char *rel;
    } cases[] = {
        {"C", pdxe_language_id("c"), "src/local.c"},
        {"C++", pdxe_language_id("cpp"), "src/local.cpp"},
    };
    for (size_t i = 0; i < sizeof(cases) / sizeof(cases[0]); i++) {
        char *rows = extract_in(empty, cases[i].lang, cases[i].rel, source);
        for (const char *const *callee =
                 (const char *const[]){"local_target", "chosen_target", "wrapped_target",
                                       "guarded_target", NULL};
             *callee; callee++) {
            CHECK(has_expanded_call(rows, *callee), "%s: the source's own %s was not expanded",
                  cases[i].name, *callee);
        }
        CHECK(!has_expanded_call(rows, "rejected_target"),
              "%s: a branch the source's own condition rejects was expanded", cases[i].name);
        CHECK(strstr(rows, "\"module_path\":\"local.h\"") != NULL,
              "%s: a quoted include is no longer an import", cases[i].name);
        CHECK(strstr(rows, "\"module_path\":\"stdio.h\"") != NULL,
              "%s: a system include is no longer an import", cases[i].name);
        free(rows);
    }
    free(empty);
}

int main(void) {
    if (!getcwd(start_dir, sizeof(start_dir))) {
        die("no working directory", NULL);
    }
    /* Engine 7 is the first whose C-family extraction reads nothing but its source: what
     * an earlier engine cached may hold an ambient header's content, and its cache keys
     * must not be this engine's. */
    CHECK(atoi(pdxe_version()) >= 7, "engine %s predates source-only preprocessing",
          pdxe_version());
    if (pdxe_init(&ctx) != PDXE_OK) {
        printf("FAIL the engine did not start\n");
        return 1;
    }
    check_working_directory();
    check_secret_header();
    check_outside_repository();
    check_supplied_bytes();
    check_source_preprocessing();
    pdxe_shutdown(ctx);
    if (failures) {
        printf("%d failure(s)\n", failures);
        return 1;
    }
    printf("ok: C and C++ extraction read nothing but the source given\n");
    return 0;
}
