//! C and C++ character constants and `#line` numbers mean the same on every platform
//! (docs/plan/ISSUES.md, issue 86; decision 55).
//!
//! An ordinary one-byte character constant is a signed 8-bit char, whatever the
//! compiler's plain char (unsigned on Linux for `AArch64`), and a `#line` number is its
//! leading decimal digits read at a fixed width, at most 4294967295, whatever the C
//! `long` (32 bits on Windows). The expectations are the same on Linux, macOS and
//! Windows: the Windows run of this test is the proof that Windows extracts as Linux
//! does, where it read `#line 2147483648` as 2147483647 before.

use pdx_engine::Engine;

/// Calls chosen by `#if`, `#elif` and macro expansion on character constants.
const LITERALS: &[u8] = b"#if '\\xff' < 0
#define BRANCH() signed_branch()
#else
#define BRANCH() unsigned_branch()
#endif
#if '\\x80' > 0
#define LOW() low_positive()
#elif '\\x80' == -128 && '\\377' == -1
#define LOW() low_is_minus_128()
#endif
#if 'ab' == 0x6162 && '\\xff\\xff\\xff\\xff' == -1
#define MULTI() multi_is_int32()
#endif
void selected(void) { BRANCH(); LOW(); MULTI(); }
";

/// Calls chosen by the line `#line` gives. Each large directive names a file of its own,
/// so the engine never pads its expanded text to the line (issue 87), and the source's
/// own lines resume at 20.
fn lines(rel_path: &str) -> Vec<u8> {
    format!(
        "#define KEEP 1
#line 2147483648 \"wide.c\"
#if __LINE__ == 2147483648
#define WIDE_LINE() line_exact()
#elif __LINE__ == 2147483647
#define WIDE_LINE() line_long_saturated()
#endif
#line 4294967296 \"huge.c\"
#if __LINE__ == 4294967295
#define HUGE_LINE() line_clamped()
#elif __LINE__ == 0
#define HUGE_LINE() line_wrapped()
#elif __LINE__ == 2147483647
#define HUGE_LINE() line_long_saturated()
#endif
#line 20 \"{rel_path}\"
void lines(void) {{ WIDE_LINE(); HUGE_LINE(); }}
"
    )
    .into_bytes()
}

fn callees(language: &str, rel_path: &str, source: &[u8]) -> Vec<String> {
    let engine = Engine::new().unwrap();
    let extract = engine.extract(language, rel_path, source).unwrap();
    extract.calls.into_iter().map(|c| c.callee_text).collect()
}

#[test]
fn character_constants_take_the_signed_char_rule_on_every_platform() {
    for (language, path) in [("c", "src/chars.c"), ("cpp", "src/chars.cpp")] {
        let callees = callees(language, path, LITERALS);
        for expected in ["signed_branch", "low_is_minus_128", "multi_is_int32"] {
            assert!(
                callees.iter().any(|c| c == expected),
                "{language}: {expected} is not called: {callees:?}"
            );
        }
        for other in ["unsigned_branch", "low_positive"] {
            assert!(
                !callees.iter().any(|c| c == other),
                "{language}: {other} is called: {callees:?}"
            );
        }
    }
}

#[test]
fn line_numbers_take_the_fixed_rule_on_every_platform() {
    for (language, path) in [("c", "src/lines.c"), ("cpp", "src/lines.cpp")] {
        let callees = callees(language, path, &lines(path));
        for expected in ["line_exact", "line_clamped"] {
            assert!(
                callees.iter().any(|c| c == expected),
                "{language}: {expected} is not called: {callees:?}"
            );
        }
        for other in ["line_long_saturated", "line_wrapped"] {
            assert!(
                !callees.iter().any(|c| c == other),
                "{language}: {other} is called: {callees:?}"
            );
        }
    }
}
