/*
 * The preprocessing library's ceilings and its #if arithmetic (docs/plan/ISSUES.md, issue
 * 87; decision 56; engine/patches/0019-bounded-deterministic-preprocessing.patch).
 *
 *     preprocessor_bounds
 *
 * Every ceiling of simplecpp::Budget is a count, so each is checked at its edge: a run is
 * first made with no ceiling to learn what it uses, then repeated with the ceiling at
 * that use (it completes), one less (it stops, naming that ceiling) and one more (it
 * completes):
 *
 *   - tokens and text macro expansion generates, temporaries included, and never the
 *     source's own tokens copied through;
 *   - the bytes stringify writes;
 *   - the tokens #if folding steps over, a ternary's restart from the front included;
 *   - the macros expanded, nested ones included, even those that create nothing;
 *   - how deeply expansions nest;
 *   - the padding of a forward jump: up to the ceiling it is newlines, past it a #line
 *     marker carrying the line.
 *
 * A run that passes a generation ceiling stops inside the expansion, before anything is
 * written out: its counts end one past the ceiling and nothing has been serialized.
 *
 * #if arithmetic is 64-bit two's complement, written out: sums, differences, products
 * and the least value divided by -1 wrap; negating the least value gives itself; -(-5)
 * is 5 (but - -5, one prefix operator directly before another, is still malformed and
 * false, the library's own rule); a shift by 64 or more gives 0, a right shift of a negative value is arithmetic,
 * and a negative count fails the condition, as a division by zero does.
 */

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

struct Run {
    std::string text;      /* the serialized output; empty when a ceiling stopped it */
    const char *stopped;   /* the ceiling passed, or nullptr */
    simplecpp::Budget used;
};

static Run run(const std::string &source, simplecpp::Budget budget) {
    Run result{std::string(), nullptr, simplecpp::Budget()};
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
    try {
        const simplecpp::BudgetScope scope(budget);
        simplecpp::preprocess(out, raw, files, cache, dui);
        result.text = out.stringify();
    } catch (const simplecpp::BudgetExhausted &exhausted) {
        result.stopped = exhausted.ceiling;
    }
    result.used = budget;
    return result;
}

static const char EXPANSION[] = "#define A one two three\n"
                                "#define B A A A\n"
                                "#define C B B\n"
                                "int v = C;\n";

static const char CONDITION[] = "#if ((1 + 2) * ((3)))\n"
                                "int yes;\n"
                                "#endif\n";

static const char CHAIN[] = "#define M0 end\n"
                            "#define M1 M0\n"
                            "#define M2 M1\n"
                            "#define M3 M2\n"
                            "#define M4 M3\n"
                            "int v = M4;\n";

/* One ceiling at its edge: `set` writes the ceiling into a Budget, `used` reads the use. */
template <class Set, class Used>
static void edge(const char *what, const char *source, const char *ceiling, Set set, Used used) {
    const Run free = run(source, simplecpp::Budget());
    CHECK(!free.stopped, "%s: a run with no ceiling stopped", what);
    const std::size_t n = used(free.used);
    CHECK(n > 1, "%s: the run used nothing", what);
    for (const long delta : {-1L, 0L, 1L}) {
        simplecpp::Budget budget;
        set(budget, static_cast<std::size_t>(static_cast<long>(n) + delta));
        const Run bounded = run(source, budget);
        if (delta < 0) {
            CHECK(bounded.stopped && std::string(bounded.stopped) == ceiling,
                  "%s: a ceiling of %zu (use %zu) did not stop it on %s", what, n - 1, n,
                  ceiling);
            CHECK(bounded.text.empty(), "%s: a stopped run left output", what);
        } else {
            CHECK(!bounded.stopped, "%s: a ceiling of %zu (use %zu) stopped it", what,
                  n + static_cast<std::size_t>(delta), n);
            CHECK(bounded.text == free.text, "%s: a ceiling it did not reach changed the output",
                  what);
        }
    }
}

static void check_edges() {
    edge(
        "generated tokens", EXPANSION, "generated tokens",
        [](simplecpp::Budget &b, std::size_t n) { b.maxGeneratedTokens = n; },
        [](const simplecpp::Budget &b) { return b.generatedTokens; });
    edge(
        "generated bytes", EXPANSION, "generated bytes",
        [](simplecpp::Budget &b, std::size_t n) { b.maxGeneratedBytes = n; },
        [](const simplecpp::Budget &b) { return b.generatedBytes; });
    edge(
        "serialized bytes", EXPANSION, "serialized bytes",
        [](simplecpp::Budget &b, std::size_t n) { b.maxSerializedBytes = n; },
        [](const simplecpp::Budget &b) { return b.serializedBytes; });
    edge(
        "condition steps", CONDITION, "condition steps",
        [](simplecpp::Budget &b, std::size_t n) { b.maxConditionSteps = n; },
        [](const simplecpp::Budget &b) { return b.conditionSteps; });
    edge(
        "expansions", CHAIN, "expansions",
        [](simplecpp::Budget &b, std::size_t n) { b.maxExpansions = n; },
        [](const simplecpp::Budget &b) { return b.expansions; });
    edge(
        "expansion depth", CHAIN, "expansion depth",
        [](simplecpp::Budget &b, std::size_t n) { b.maxExpansionDepth = static_cast<unsigned>(n); },
        [](const simplecpp::Budget &b) { return static_cast<std::size_t>(b.deepestExpansion); });

    // The chain nests five deep, five expansions: M4, M3, M2, M1, M0.
    const Run chain = run(CHAIN, simplecpp::Budget());
    CHECK(chain.used.deepestExpansion == 5 && chain.used.expansions == 5,
          "the chain did not nest five deep in five expansions");
    // A macro that expands to nothing creates nothing, but each expansion is counted.
    const Run empty = run("#define E0\n#define E1 E0 E0\n#define E2 E1 E1\nint v = 1 E2;\n",
                          simplecpp::Budget());
    CHECK(empty.used.expansions == 7 && empty.used.generatedTokens == 0,
          "empty macros: %zu expansions, %zu tokens", empty.used.expansions,
          empty.used.generatedTokens);
    // Folding a ternary starts again from the front: with 100 tokens no fold removes
    // before 100 ternaries, every restart is counted.
    std::string ternaries = "#if ";
    for (int i = 0; i < 100; i++)
        ternaries += "\"s\" ";
    for (int i = 0; i < 100; i++)
        ternaries += "1 ? 1 : ";
    ternaries += "1\nint yes;\n#endif\n";
    const Run folded = run(ternaries, simplecpp::Budget());
    CHECK(folded.used.conditionSteps > 100 * 100,
          "100 ternaries after 100 tokens counted %zu steps", folded.used.conditionSteps);
    // The source's own tokens, copied through, are never generated.
    const Run plain = run("#define UNUSED 1\nint a = 1; int b = 2; int c = 3;\n", simplecpp::Budget());
    CHECK(plain.used.generatedTokens == 0 && plain.used.generatedBytes == 0,
          "a source with no expansion generated %zu tokens", plain.used.generatedTokens);
}

/* A doubling bomb stops inside the expansion, one token past its ceiling, before
 * anything is written out. */
static void check_stop_point() {
    std::string bomb = "#define D0 x\n";
    for (int i = 1; i <= 24; i++) {
        bomb += "#define D" + std::to_string(i) + " D" + std::to_string(i - 1) + " D" +
                std::to_string(i - 1) + "\n";
    }
    bomb += "int v = D24;\n";
    simplecpp::Budget budget;
    budget.maxGeneratedTokens = 1000;
    const Run bounded = run(bomb, budget);
    CHECK(bounded.stopped && std::string(bounded.stopped) == "generated tokens",
          "the bomb did not stop on generated tokens");
    CHECK(bounded.used.generatedTokens == 1001, "the bomb stopped at %zu tokens, not 1001",
          bounded.used.generatedTokens);
    CHECK(bounded.used.serializedBytes == 0, "the bomb was serialized (%zu bytes) before stopping",
          bounded.used.serializedBytes);
}

/* A forward jump of up to the padding is newlines; one more line is a marker. */
static void check_padding() {
    // `int a` is on line 1, `int b` on line 12: a jump of 11 lines.
    const char *source = "int a;\n#line 12\nint b;\n";
    simplecpp::Budget padded;
    padded.maxLinePadding = 11;
    const Run at = run(source, padded);
    CHECK(at.used.lineMarkers == 0 && at.used.longestLineJump == 11 &&
              at.text.find("#line") == std::string::npos &&
              at.text == "int a ;\n\n\n\n\n\n\n\n\n\n\nint b ;",
          "a jump of the padding was not padded: %s", at.text.c_str());
    simplecpp::Budget marked;
    marked.maxLinePadding = 10;
    const Run past = run(source, marked);
    CHECK(past.used.lineMarkers == 1 && past.text == "int a ;\n#line 12 \"src/t.c\"\nint b ;",
          "a jump past the padding was not marked: %s", past.text.c_str());
    // A jump of four billion lines is one marker, a few bytes.
    simplecpp::Budget huge;
    huge.maxLinePadding = 4096;
    const Run far = run("int a;\n#line 4294967295\nint b;\n", huge);
    CHECK(far.text == "int a ;\n#line 4294967295 \"src/t.c\"\nint b ;",
          "a jump of four billion lines was not one marker: %s", far.text.c_str());
}

static std::string evaluates(const std::string &condition) {
    const Run r = run("#if " + condition + "\nint yes;\n#else\nint no;\n#endif\n", simplecpp::Budget());
    if (r.text.find("yes") != std::string::npos)
        return "true";
    if (r.text.find("no") != std::string::npos)
        return "false";
    return "error";
}

static void check_arithmetic() {
    static const struct {
        const char *condition;
        const char *result;
    } CASES[] = {
        {"9223372036854775807 + 1 == -9223372036854775807 - 1", "true"},
        {"(-9223372036854775807 - 1) - 1 == 9223372036854775807", "true"},
        {"4611686018427387904 * 2 == -9223372036854775807 - 1", "true"},
        {"(-9223372036854775807 - 1) * -1 == -9223372036854775807 - 1", "true"},
        {"(-9223372036854775807 - 1) / -1 == -9223372036854775807 - 1", "true"},
        {"(-9223372036854775807 - 1) % -1 == 0", "true"},
        {"-(-9223372036854775807 - 1) == -9223372036854775807 - 1", "true"},
        {"-(-5) == 5", "true"},
        {"-(-(5)) == 5", "true"},
        // The library folds prefix operators in one forward pass, so one directly before
        // another is never folded; the expression is left malformed and is false, as it
        // was before patch 0019 and is in the reference.
        {"- -5 == 5", "false"},
        {"~0 == -1", "true"},
        {"~(-9223372036854775807 - 1) == 9223372036854775807", "true"},
        {"(1 << 63) == -9223372036854775807 - 1", "true"},
        {"(1 << 62) == 4611686018427387904", "true"},
        {"(1 << 64) == 0", "true"},
        {"(1 << 200) == 0", "true"},
        {"(-1 << 1) == -2", "true"},
        {"(-8 >> 1) == -4", "true"},
        {"(-1 >> 63) == -1", "true"},
        {"(-1 >> 64) == 0", "true"},
        {"(16 >> 2) == 4", "true"},
        {"(-1 & 5) == 5", "true"},
        {"(-8 | 3) == -5", "true"},
        {"(-1 ^ 1) == -2", "true"},
        {"7 / 2 == 3 && -7 / 2 == -3 && -7 % 2 == -1", "true"},
        {"(1 << -1)", "error"},
        {"(1 >> -1)", "error"},
        {"1 / 0", "error"},
    };
    for (const auto &c : CASES) {
        const std::string got = evaluates(c.condition);
        CHECK(got == c.result, "#if %s is %s, not %s", c.condition, got.c_str(), c.result);
    }
}

int main() {
    check_edges();
    check_stop_point();
    check_padding();
    check_arithmetic();
    if (failures) {
        std::printf("%d failure(s)\n", failures);
        return 1;
    }
    std::printf("ok: every ceiling stops at its edge, and #if arithmetic is 64-bit and defined\n");
    return 0;
}
