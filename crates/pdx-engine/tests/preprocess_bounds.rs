//! C and C++ preprocessing is bounded by its source and defined the same on every
//! platform (docs/plan/ISSUES.md, issue 87; decision 56).
//!
//! A forward `#line` padded the expanded text with a newline for every line it skipped,
//! and macros that double expanded exponentially, so whether a file completed depended
//! on the machine. Now a far jump is one `#line` marker, and expansion is held to
//! ceilings counted from the source's length: passing one discards the expansion, the
//! file is extracted from its source alone, and the extraction loses work exactly once.
//! `#if` arithmetic is 64-bit two's complement, written out, and integer literals are
//! read at a fixed width, so no compiler or C++ library decides a value.
//!
//! Nothing here sets a variable or a switch, and every expectation is the same on Linux,
//! macOS and Windows: the Windows run of this test is the proof that Windows extracts as
//! Linux does.

use std::fmt::Write as _;

use pdx_engine::{Engine, FileExtract};

fn extract(language: &str, rel_path: &str, source: &[u8]) -> FileExtract {
    Engine::new()
        .unwrap()
        .extract(language, rel_path, source)
        .unwrap()
}

fn callees(extract: &FileExtract) -> Vec<&str> {
    extract
        .calls
        .iter()
        .map(|c| c.callee_text.as_str())
        .collect()
}

/// What an extraction found, by name and line: a directive's length moves byte offsets,
/// so they are left out.
fn facts(extract: &FileExtract) -> Vec<String> {
    let mut facts: Vec<String> = extract
        .definitions
        .iter()
        .map(|d| {
            format!(
                "def {} {:?} {:?}",
                d.qualified_name,
                d.kind,
                d.span.map(|s| s.start_line)
            )
        })
        .chain(extract.calls.iter().map(|c| {
            format!(
                "call {} from {:?} at {:?} reference {}",
                c.callee_text,
                c.caller,
                c.span.map(|s| s.start_line),
                c.is_reference
            )
        }))
        .chain(
            extract
                .usages
                .iter()
                .map(|u| format!("usage {u:?}").replace(char::is_numeric, "#")),
        )
        .collect();
    facts.sort();
    facts
}

fn far_line_source(line: &str) -> Vec<u8> {
    format!("#define KEEP 1\nint before(void);\n#line {line}\nvoid after(void) {{ callee(); }}\n")
        .into_bytes()
}

#[test]
fn a_far_line_jump_is_a_marker_and_never_a_fact() {
    for (language, path) in [("c", "src/far.c"), ("cpp", "src/far.cpp")] {
        let near = extract(language, path, &far_line_source("4"));
        assert_eq!(near.extraction_lost, 0);
        for line in ["400000000", "2147483647", "4294967295"] {
            let far = extract(language, path, &far_line_source(line));
            assert_eq!(far.extraction_lost, 0, "{language}: #line {line} lost work");
            assert!(
                callees(&far).contains(&"callee"),
                "{language}: #line {line}: {:?}",
                callees(&far)
            );
            assert_eq!(
                facts(&far),
                facts(&near),
                "{language}: #line {line} changed what was found"
            );
        }
    }
}

/// `D<k>` calls `leaf` 2^k times.
fn bomb(k: u32) -> Vec<u8> {
    let mut source = String::from("void before(void);\n#define D0 leaf();\n");
    for i in 1..=k {
        writeln!(source, "#define D{i} D{} D{}", i - 1, i - 1).unwrap();
    }
    writeln!(source, "void run(void) {{ before(); D{k} }}").unwrap();
    source.into_bytes()
}

#[test]
fn a_doubling_macro_expands_below_its_ceilings() {
    let below = extract("c", "src/bomb.c", &bomb(12));
    assert_eq!(below.extraction_lost, 0);
    let callees = callees(&below);
    assert!(
        callees.contains(&"leaf") && callees.contains(&"before"),
        "{callees:?}"
    );
}

#[test]
fn a_doubling_macro_above_its_ceilings_is_lost_work_once() {
    // 2^24 calls would be 67 million tokens: the expansion stops at its first step past
    // the budget, is discarded, and the file is extracted from its source alone.
    let above = extract("c", "src/bomb.c", &bomb(24));
    assert_eq!(above.extraction_lost, 1, "lost work is counted once");
    let callees = callees(&above);
    assert!(
        !callees.contains(&"leaf") && callees.contains(&"before"),
        "{callees:?}"
    );
    assert!(
        above.definitions.iter().any(|d| d.name == "run"),
        "the source's own definitions remain"
    );
    // The same on every run.
    assert_eq!(extract("c", "src/bomb.c", &bomb(24)), above);
}

#[test]
fn nesting_stops_at_its_ceiling_on_every_platform() {
    // F(F(...F(0)...)) recursed until the stack ran out, which is a crash on one
    // machine and not on another. 256 deep expands within half of the smallest stack an
    // extraction runs on (a Windows main thread's 1 MiB, in the isolated worker); 257 is
    // lost work.
    for (depth, lost) in [(256, 0), (257, 1)] {
        let source = format!(
            "#define F(x) (x + 1)\nint before(void);\nint v = {}0{};\n",
            "F(".repeat(depth),
            ")".repeat(depth)
        );
        let extract = extract("c", "src/nest.c", source.as_bytes());
        assert_eq!(extract.extraction_lost, lost, "{depth} deep");
    }
}

#[test]
fn nested_parentheses_in_an_if_are_lost_work_once() {
    // 80,000 nested parentheses in an `#if` were folded in quadratic time (9.2 s): the
    // folding steps are counted, and stop at the budget.
    let source = format!(
        "#if {}1{}\nint before(void);\n#endif\n",
        "(".repeat(80_000),
        ")".repeat(80_000)
    );
    let extract = extract("c", "src/parens.c", source.as_bytes());
    assert_eq!(extract.extraction_lost, 1);
}

#[test]
fn doubling_macros_that_create_nothing_are_lost_work_once() {
    // 2^40 expansions of a macro that expands to nothing would take days: the count of
    // expansions stops them, the same on every machine.
    let mut source = String::from("void before(void);\n#define D0\n");
    for i in 1..=40 {
        writeln!(source, "#define D{i} D{} D{}", i - 1, i - 1).unwrap();
    }
    source.push_str("void run(void) { before(); D40 }\n");
    let extract = extract("c", "src/empty.c", source.as_bytes());
    assert_eq!(extract.extraction_lost, 1);
    assert!(callees(&extract).contains(&"before"));
}

/// Conditions whose value is fixed by the arithmetic model, each choosing a call.
const ARITHMETIC: &[(&str, bool)] = &[
    // Sums, differences and products wrap modulo 2^64.
    ("9223372036854775807 + 1 == -9223372036854775807 - 1", true),
    (
        "(-9223372036854775807 - 1) - 1 == 9223372036854775807",
        true,
    ),
    ("4611686018427387904 * 2 == -9223372036854775807 - 1", true),
    (
        "(-9223372036854775807 - 1) * -1 == -9223372036854775807 - 1",
        true,
    ),
    // The least value divided by -1 is itself, its remainder 0; negating it gives it.
    (
        "(-9223372036854775807 - 1) / -1 == -9223372036854775807 - 1",
        true,
    ),
    ("(-9223372036854775807 - 1) % -1 == 0", true),
    (
        "-(-9223372036854775807 - 1) == -9223372036854775807 - 1",
        true,
    ),
    ("-(-5) == 5", true),
    ("~(-9223372036854775807 - 1) == 9223372036854775807", true),
    // Shifts: past 63 bits nothing is left; a negative value shifts right arithmetically.
    ("(1 << 63) == -9223372036854775807 - 1", true),
    ("(1 << 64) == 0", true),
    ("(-1 >> 63) == -1", true),
    ("(-8 >> 1) == -4", true),
    ("(-1 >> 64) == 0", true),
    ("(-8 | 3) == -5 && (-1 & 5) == 5 && (-1 ^ 1) == -2", true),
    ("-7 / 2 == -3 && -7 % 2 == -1", true),
    // Literals past the range are read at a fixed width, saturating at the bound.
    ("9223372036854775808 == 9223372036854775807", true),
    ("99999999999999999999999 == 9223372036854775807", true),
    ("0xffffffffffffffff == 9223372036854775807", true),
    ("0x10000000000000000 == 9223372036854775807", true),
    ("01777777777777777777777 == 9223372036854775807", true),
    ("0x7fffffffffffffff == 9223372036854775807", true),
    ("9223372036854775807 + 1 > 0", false),
];

#[test]
fn if_arithmetic_is_the_same_on_every_platform() {
    let mut source = String::new();
    let mut calls = String::new();
    for (i, (condition, _)) in ARITHMETIC.iter().enumerate() {
        write!(
            source,
            "#if {condition}\n#define C{i}() yes_{i}()\n#else\n#define C{i}() no_{i}()\n#endif\n"
        )
        .unwrap();
        write!(calls, "C{i}(); ").unwrap();
    }
    writeln!(source, "void chosen(void) {{ {calls}}}").unwrap();
    for (language, path) in [("c", "src/arith.c"), ("cpp", "src/arith.cpp")] {
        let extract = extract(language, path, source.as_bytes());
        assert_eq!(extract.extraction_lost, 0);
        let callees = callees(&extract);
        for (i, (condition, value)) in ARITHMETIC.iter().enumerate() {
            let (taken, other) = if *value {
                (format!("yes_{i}"), format!("no_{i}"))
            } else {
                (format!("no_{i}"), format!("yes_{i}"))
            };
            assert!(
                callees.contains(&taken.as_str()) && !callees.contains(&other.as_str()),
                "{language}: #if {condition} is not {value}: {callees:?}"
            );
        }
    }
}

#[test]
fn an_if_that_cannot_be_evaluated_fails_on_every_platform() {
    // A negative shift count fails the condition, as a division by zero does: the
    // library's own error, which keeps no expansion and loses no work.
    for condition in ["(1 << -1)", "(1 >> -1)", "1 / 0", "1 % 0"] {
        let source = format!(
            "#if {condition}\n#define PICK() yes()\n#else\n#define PICK() no()\n#endif\nvoid f(void) {{ PICK(); }}\n"
        );
        let extract = extract("c", "src/error.c", source.as_bytes());
        let callees = callees(&extract);
        assert!(
            !callees.contains(&"yes") && !callees.contains(&"no"),
            "#if {condition}: {callees:?}"
        );
        assert_eq!(extract.extraction_lost, 0, "#if {condition}");
    }
}
