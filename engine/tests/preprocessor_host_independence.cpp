/*
 * The preprocessing library's character constants and #line numbers do not depend on the
 * compiler or platform that built it (docs/plan/ISSUES.md, issue 86; decision 55;
 * engine/patches/0018-host-independent-literals-and-line.patch).
 *
 *     preprocessor_host_independence_signed_char
 *     preprocessor_host_independence_unsigned_char
 *
 * The library is built for this test twice from engine/src/preprocessor.cpp, once with
 * plain char signed (-fsigned-char, as on x86-64, Apple silicon and Windows) and once
 * with it unsigned (-funsigned-char, as on Linux for AArch64), and both builds must give
 * exactly the values below, written out here:
 *
 *   - characterLiteralToLL: an ordinary one-byte character constant has the value of a
 *     signed 8-bit char ('\x80' is -128, '\xff' is -1); a multi-character constant that
 *     of a 32-bit int; u'', u8'', U'' and L'' constants their code point, unsigned;
 *   - #if, #elif and macro expansion choose by those values;
 *   - #line N, #line N "file" and # N "file" set the line to the leading decimal digits
 *     of N, read at a fixed width, at most 4294967295: not by atol, whose C long is 32
 *     bits on Windows and 64 elsewhere. Lines are read from the tokens' locations,
 *     never from the expanded text, which the library pads with one newline per line a
 *     #line skips when no simplecpp::Budget limits the padding (issue 87).
 */

#include <climits>
#include <cstdio>
#include <sstream>
#include <string>
#include <vector>

#include "simplecpp/simplecpp.h"

static int failures = 0;

#define CHECK(cond, ...)                                                                           \
    do {                                                                                           \
        if (!(cond)) {                                                                             \
            std::printf("FAIL ");                                                                  \
            std::printf(__VA_ARGS__);                                                              \
            std::printf("\n");                                                                     \
            failures++;                                                                            \
        }                                                                                          \
    } while (0)

static const struct {
    const char *literal;
    long long value;
} LITERALS[] = {
    // One byte: a signed 8-bit char.
    {"'\\x00'", 0},
    {"'\\x01'", 1},
    {"'\\x7f'", 127},
    {"'\\x80'", -128},
    {"'\\xfe'", -2},
    {"'\\xff'", -1},
    {"'\\0'", 0},
    {"'\\200'", -128},
    {"'\\376'", -2},
    {"'\\377'", -1},
    {"'A'", 65},
    {"'\\n'", 10},
    {"'\x80'", -128},
    {"'\xff'", -1},
    // Several bytes: a 32-bit int, the first byte highest.
    {"'ab'", 0x6162},
    {"'abcd'", 0x61626364},
    {"'abcde'", 0x62636465},
    {"'\\xff\\xff'", 0xffff},
    {"'\\xff\\xff\\xff\\xff'", -1},
    {"'\\x80\\x00\\x00\\x00'", -2147483648LL},
    {"'\\x7f\\xff\\xff\\xff'", 2147483647},
    // Other kinds: the code point, unsigned.
    {"u'\\xff'", 255},
    {"U'\\xff'", 255},
    {"L'\\xff'", 255},
    {"u8'a'", 97},
    {"u'\\u00e9'", 233},
    {"L'\\U0001F600'", 128512},
    {"L'\\x10000'", 65536},
};

static std::string preprocess(const std::string &source) {
    std::istringstream stream(source);
    std::vector<std::string> files{"src/t.c"};
    simplecpp::TokenList raw(stream, files, files[0]);
    simplecpp::TokenList out(files);
    simplecpp::DUI dui;
    dui.std = "c11";
    dui.sourceOnly = true;
    dui.fixedDateTime = true;
    dui.fixedSizeOfTypes = true;
    simplecpp::FileDataCache cache;
    simplecpp::preprocess(out, raw, files, cache, dui);
    return out.stringify();
}

/* The line of the token `marker` in `source`, from its location alone. */
static long long line_of(const std::string &source, const char *marker) {
    std::istringstream stream(source);
    std::vector<std::string> files{"src/t.c"};
    simplecpp::TokenList raw(stream, files, files[0]);
    for (const simplecpp::Token *tok = raw.cfront(); tok; tok = tok->next) {
        if (tok->str() == marker) {
            return tok->location.line;
        }
    }
    return -1;
}

static void check_literals() {
    for (const auto &entry : LITERALS) {
        long long got = 0;
        try {
            got = simplecpp::characterLiteralToLL(entry.literal);
        } catch (const std::exception &e) {
            CHECK(false, "%s: %s", entry.literal, e.what());
            continue;
        }
        CHECK(got == entry.value, "%s is %lld, not %lld", entry.literal, got, entry.value);
    }
}

static void check_branches() {
    const std::string text = preprocess("#if '\\xff' < 0\n"
                                        "#define BRANCH() signed_branch()\n"
                                        "#else\n"
                                        "#define BRANCH() unsigned_branch()\n"
                                        "#endif\n"
                                        "#if '\\x80' > 0\n"
                                        "#define LOW() low_positive()\n"
                                        "#elif '\\x80' == -128\n"
                                        "#define LOW() low_is_minus_128()\n"
                                        "#endif\n"
                                        "void selected(void) { BRANCH(); LOW(); }\n");
    CHECK(text.find("signed_branch") != std::string::npos &&
              text.find("unsigned_branch") == std::string::npos,
          "#if '\\xff' < 0 did not take the signed branch:\n%s", text.c_str());
    CHECK(text.find("low_is_minus_128") != std::string::npos &&
              text.find("low_positive") == std::string::npos,
          "#elif '\\x80' == -128 was not taken:\n%s", text.c_str());
}

static const struct {
    const char *number;
    long long line;
} LINES[] = {
    {"0", 0},
    {"1", 1},
    {"42", 42},
    {"100000", 100000},
    {"2147483646", 2147483646},
    {"2147483647", 2147483647},
    {"2147483648", 2147483648LL},
    {"4294967294", 4294967294LL},
    {"4294967295", 4294967295LL},
    {"4294967296", 4294967295LL},
    {"9223372036854775807", 4294967295LL},
    {"9223372036854775808", 4294967295LL},
    {"999999999999999999999999", 4294967295LL},
    {"0x10", 0},
    {"1e5", 1},
};

static void check_lines() {
    for (const auto &entry : LINES) {
        const std::string forms[] = {
            std::string("#line ") + entry.number + "\nint here;\n",
            std::string("#line ") + entry.number + " \"other.c\"\nint here;\n",
            std::string("# ") + entry.number + " \"other.c\"\nint here;\n",
        };
        for (const std::string &source : forms) {
            const long long got = line_of(source, "here");
            CHECK(got == entry.line, "%s: the next line is %lld, not %lld",
                  source.substr(0, source.find('\n')).c_str(), got, entry.line);
        }
    }
    // Through the preprocessor, where each directive names another file, so that the
    // library, with no Budget here, writes a #line marker rather than pad: __LINE__ is
    // the same number.
    const std::string text =
        preprocess("#line 4294967296 \"huge.c\"\nint at = __LINE__;\n#line 2147483648 \"wide.c\"\n"
                   "int at2 = __LINE__;\n");
    CHECK(text.find("at = 4294967295") != std::string::npos &&
              text.find("at2 = 2147483648") != std::string::npos,
          "__LINE__ after #line is not the fixed number:\n%s", text.c_str());
}

int main() {
#ifdef PDXE_TEST_UNSIGNED_CHAR
    CHECK(static_cast<char>(0xff) > 0, "this build's char is not unsigned");
    const char *build = "unsigned";
#else
    CHECK(static_cast<char>(0xff) < 0, "this build's char is not signed");
    const char *build = "signed";
#endif
    check_literals();
    check_branches();
    check_lines();
    if (failures) {
        std::printf("%d failure(s) with plain char %s\n", failures, build);
        return 1;
    }
    std::printf("ok: literals and #line take the project's values with plain char %s\n", build);
    return 0;
}
