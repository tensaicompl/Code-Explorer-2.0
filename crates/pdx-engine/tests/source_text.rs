//! Text an extraction takes from the file is a range of the source, held once, and a
//! nested definition's qualified name is given by its relation to an earlier one
//! (docs/plan/ISSUES.md, issue 90; decision 61).
//!
//! What is stored and sent is the record, which holds no source; an extraction is the
//! record bound to the exact bytes it was taken from. A record binds to no other source,
//! and a range or name that does not fit is refused, never clamped. Converting the
//! engine's result, serialising the record and reading it back keep every range a range:
//! in the four shapes issue 90 measured, and the two its audit found (C++ functions at
//! every level of nested namespaces, JavaScript functions as parameters' defaults), the
//! text an extraction owns and the record's encoded size grow with the source, never
//! with the square of the nesting.

use std::fmt::Write as _;
use std::sync::Arc;

use pdx_engine::{
    BindError, Engine, ExtractRecord, FileExtract, QualifiedName, SourceDigest, TextRef,
};

mod common;

fn extract(language: &str, rel_path: &str, source: &str) -> FileExtract {
    Engine::new()
        .unwrap()
        .extract(language, rel_path, source.as_bytes())
        .unwrap_or_else(|e| panic!("{rel_path}: {e}"))
}

/// Every text an extraction holds, with its record's own copy of it when owned.
fn texts(e: &FileExtract) -> Vec<&TextRef> {
    let mut out = Vec::new();
    for c in &e.calls {
        out.push(&c.callee_text);
        out.extend(c.args.iter().map(|a| &a.expr));
    }
    out.extend(e.usages.iter().map(|u| &u.name));
    for d in &e.definitions {
        out.extend(d.signature.as_ref());
        out.extend(d.routes.iter().filter_map(|r| r.source_text.as_ref()));
    }
    out
}

/// The bytes a record owns of the file's text: owned texts and qualified names.
fn owned_bytes(e: &FileExtract) -> usize {
    let text: usize = texts(e)
        .iter()
        .map(|t| match t {
            TextRef::Owned(s) => s.len(),
            TextRef::Source { .. } => 0,
        })
        .sum();
    let names: usize = e
        .definitions
        .iter()
        .map(|d| match &d.qualified_name {
            QualifiedName::Whole(s)
            | QualifiedName::Extended { rest: s, .. }
            | QualifiedName::Prefix { rest: s, .. } => s.len(),
            QualifiedName::Member { .. } => 0,
        })
        .sum();
    text + names
}

/// The record's encoded size without its surface and the sites' node-type paths: a path
/// grows with its site's depth, and a file's paths are bounded by their own budget
/// (issue 88), not by this representation. The surface is measured on its own.
fn encoded_text_size(e: &FileExtract) -> usize {
    let mut record = e.record().clone();
    for c in &mut record.calls {
        c.ast_path = pdx_engine::AstPath::default();
    }
    for d in &mut record.definitions {
        for r in &mut d.routes {
            r.ast_path = pdx_engine::AstPath::default();
        }
    }
    postcard::to_stdvec(&record).unwrap().len() - record.surface.as_bytes().len()
}

const MIXED_C: &str =
    "#define TWICE(v) twice((v) + 1)\nint g(int a) { return f(a, f(a, h(a))) + TWICE(a); }\n";
const MIXED_PY: &str =
    "class A:\n    class B:\n        def m(self):\n            return self.x.y(z)\n\n\nv = a.b.c\n";

#[test]
fn ranges_are_the_text_and_expanded_text_is_owned() {
    let c = extract("c", "src/g.c", MIXED_C);
    let outer = c
        .calls
        .iter()
        .find(|call| c.text(&call.callee_text) == "f" && call.args.len() == 2)
        .expect("the outer call");
    assert!(matches!(outer.callee_text, TextRef::Source { .. }));
    assert!(matches!(outer.args[1].expr, TextRef::Source { .. }));
    assert_eq!(c.text(&outer.args[1].expr), "f(a, h(a))");
    let expanded = c
        .calls
        .iter()
        .find(|call| c.text(&call.callee_text) == "twice")
        .expect("the expanded call");
    assert_eq!(expanded.callee_text, TextRef::Owned("twice".into()));
    assert_eq!(expanded.args[0].expr, TextRef::Owned("( a ) + 1".into()));

    let py = extract("python", "app/a.py", MIXED_PY);
    let chain = py
        .usages
        .iter()
        .find(|u| py.text(&u.name) == "a.b.c")
        .expect("the attribute chain");
    assert!(matches!(chain.name, TextRef::Source { .. }));
    let names = py.qualified_names();
    let m = py
        .definitions
        .iter()
        .position(|d| d.name == "m")
        .expect("the method");
    assert_eq!(names.get(m), "app.a.A.B.m");
    assert_eq!(py.qualified_name(m), "app.a.A.B.m");
    assert_eq!(py.qualified_name_parts(m).concat(), "app.a.A.B.m");
    assert!(matches!(
        py.definitions[m].qualified_name,
        QualifiedName::Member { .. }
    ));
}

#[test]
fn a_record_survives_serialisation_and_binds_to_its_source_again() {
    for (language, path, source) in [("c", "src/g.c", MIXED_C), ("python", "app/a.py", MIXED_PY)] {
        let fresh = extract(language, path, source);
        let bytes = common::record_bytes(&fresh);
        // The record carries ranges, not the source.
        assert!(
            !bytes
                .windows(source.len().min(24))
                .any(|w| w == &source.as_bytes()[..source.len().min(24)]),
            "{path}: the source was serialised"
        );
        let record: ExtractRecord = postcard::from_bytes(&bytes).unwrap();
        // Bound to a fresh copy of the same bytes, as a cache hit binds it: the same
        // extraction, every text the same.
        let bound = record.bind(Arc::from(source.as_bytes())).unwrap();
        assert_eq!(bound, fresh, "{path}");
        for (a, b) in texts(&bound).iter().zip(texts(&fresh)) {
            assert_eq!(bound.text(a), fresh.text(b), "{path}");
        }
        for i in 0..fresh.definitions.len() {
            assert_eq!(bound.qualified_name(i), fresh.qualified_name(i), "{path}");
        }
    }
}

#[test]
fn a_record_binds_only_to_its_own_source() {
    let fresh = extract("python", "app/a.py", MIXED_PY);
    let record = || fresh.record().clone();
    // Another length.
    let longer = format!("{MIXED_PY} ");
    assert_eq!(
        record().bind(Arc::from(longer.as_bytes())),
        Err(BindError::Length {
            extracted: MIXED_PY.len() as u64,
            given: longer.len() as u64,
        })
    );
    // The same length, other bytes: never bound, whatever its ranges would read.
    let other = MIXED_PY.replace("self.x.y", "self.q.y");
    assert_eq!(other.len(), MIXED_PY.len());
    assert_eq!(
        record().bind(Arc::from(other.as_bytes())),
        Err(BindError::Digest)
    );
    // A record whose digest says another source is refused even by that source's
    // length.
    let mut forged = record();
    forged.source_digest = SourceDigest::of(other.as_bytes());
    assert_eq!(
        forged.bind(Arc::from(MIXED_PY.as_bytes())),
        Err(BindError::Digest)
    );
}

#[test]
fn a_record_whose_ranges_or_names_do_not_fit_is_refused() {
    let fresh = extract("python", "app/a.py", MIXED_PY);
    let len = MIXED_PY.len() as u64;
    let source = || Arc::<[u8]>::from(MIXED_PY.as_bytes());
    let end = u32::try_from(len).unwrap();
    for (what, bad, expected) in [
        (
            "past the source",
            TextRef::Source {
                start: 0,
                end: end + 1,
            },
            BindError::Range {
                start: 0,
                end: end + 1,
                len,
            },
        ),
        (
            "starting past its end",
            TextRef::Source { start: 5, end: 4 },
            BindError::Range {
                start: 5,
                end: 4,
                len,
            },
        ),
    ] {
        let mut call = fresh.record().clone();
        call.calls[0].callee_text = bad.clone();
        assert_eq!(
            call.bind(source()),
            Err(expected.clone()),
            "a callee {what}"
        );
        let mut usage = fresh.record().clone();
        usage.usages[0].name = bad.clone();
        assert_eq!(
            usage.bind(source()),
            Err(expected.clone()),
            "a usage {what}"
        );
        let mut argument = fresh.record().clone();
        let with_args = argument
            .calls
            .iter()
            .position(|c| !c.args.is_empty())
            .expect("a call with an argument");
        argument.calls[with_args].args[0].expr = bad;
        assert_eq!(argument.bind(source()), Err(expected), "an argument {what}");
    }
    // An empty range at the end is the empty text, within the source.
    let mut empty = fresh.record().clone();
    empty.calls[0].callee_text = TextRef::Source { start: end, end };
    assert!(empty.bind(source()).is_ok());
    // A name given by itself, or by a later definition.
    let m = fresh
        .definitions
        .iter()
        .position(|d| d.name == "m")
        .unwrap();
    for base in [m, m + 1] {
        let mut name = fresh.record().clone();
        let base = u32::try_from(base).unwrap();
        name.definitions[m].qualified_name = QualifiedName::Member { base };
        assert_eq!(
            name.bind(source()),
            Err(BindError::QualifiedName { index: m, base })
        );
    }
}

/// One construct of each shape issue 90 measured, nested `n` deep.
fn shape(which: usize, n: usize) -> (&'static str, &'static str, String) {
    match which {
        0 => (
            "c",
            "src/g.c",
            format!(
                "int g(int a) {{ return {}a{}; }}\n",
                "f(a, ".repeat(n),
                ")".repeat(n)
            ),
        ),
        1 => (
            "javascript",
            "web/m.js",
            format!("function m() {{ return x{}; }}\n", ".f()".repeat(n)),
        ),
        2 => (
            "python",
            "app/h.py",
            format!("def h():\n    v = a{}\n    return f(v)\n", ".b".repeat(n)),
        ),
        3 => (
            "java",
            "src/main/java/p/C.java",
            format!(
                "package p;\n{}void m() {{ f(); }}{}\n",
                (0..n).fold(String::new(), |mut s, i| {
                    let _ = write!(s, "class C{i} {{ ");
                    s
                }),
                "}".repeat(n)
            ),
        ),
        4 => (
            "cpp",
            "src/n.cpp",
            format!(
                "{}{}\n",
                (0..n).fold(String::new(), |mut s, i| {
                    let _ = write!(s, "namespace n{i} {{ int f{i}() {{ return 0; }} ");
                    s
                }),
                "}".repeat(n)
            ),
        ),
        _ => (
            "javascript",
            "web/d.js",
            format!(
                "{}1{}\n",
                (0..n).fold(String::new(), |mut s, i| {
                    let _ = write!(s, "function f{i}(a = ");
                    s
                }),
                ") {}".repeat(n)
            ),
        ),
    }
}

#[test]
fn retained_text_grows_with_the_source_not_the_nesting() {
    // Twice as deep is twice the source: text held once at most doubles, where nested
    // copies would quadruple. Four copies of a construct 40, 80 and 160 deep.
    for which in 0..6 {
        let mut owned = Vec::new();
        let mut encoded = Vec::new();
        let mut surfaces = Vec::new();
        let mut sources = Vec::new();
        for depth in [40, 80, 160] {
            let (language, path, one) = shape(which, depth);
            let source = one.repeat(4);
            let e = extract(language, path, &source);
            assert_eq!(e.extraction_lost, 0, "{path} at {depth}");
            owned.push(owned_bytes(&e));
            encoded.push(encoded_text_size(&e));
            surfaces.push(e.surface.as_bytes().len());
            sources.push(source.len());
        }
        let (_, path, _) = shape(which, 1);
        for step in 1..3 {
            assert!(
                owned[step] <= 3 * owned[step - 1] + 64,
                "{path}: owned bytes {owned:?} for sources {sources:?}"
            );
            assert!(
                encoded[step] <= 3 * encoded[step - 1],
                "{path}: encoded records {encoded:?} for sources {sources:?}"
            );
            // A call nested in a call's argument or a chain's link has a node-type path
            // as deep as it is: the surface of those two shapes holds the paths.
            assert!(
                which < 2 || surfaces[step] <= 3 * surfaces[step - 1],
                "{path}: surfaces {surfaces:?} for sources {sources:?}"
            );
        }
        assert!(
            owned[2] <= sources[2],
            "{path}: {} bytes owned for {} of source",
            owned[2],
            sources[2]
        );
    }
}

#[test]
fn names_in_nested_namespaces_share_their_prefix() {
    // C++ functions at every level of nested namespaces, which are no definitions: each
    // name keeps the prefix of an earlier function's it shares and adds the rest.
    let source = "namespace a { int f() { return 0; }\n\
                  namespace b { int g() { return 0; } int g2() { return 0; }\n\
                  namespace c { int h() { return 0; } } } }\n\
                  namespace a { int f2() { return 0; } }\n";
    let e = extract("cpp", "src/n.cpp", source);
    let names = e.qualified_names();
    let at = |name: &str| {
        e.definitions
            .iter()
            .position(|d| d.name == name)
            .unwrap_or_else(|| panic!("no {name}"))
    };
    for (name, qn, base, keep, rest) in [
        ("g", "src.n.a.b.g", "f", 7, ".b.g"),
        ("g2", "src.n.a.b.g2", "g", 9, ".g2"),
        ("h", "src.n.a.b.c.h", "g", 9, ".c.h"),
        ("f2", "src.n.a.f2", "f", 7, ".f2"),
    ] {
        let i = at(name);
        assert_eq!(
            e.definitions[i].qualified_name,
            QualifiedName::Prefix {
                base: u32::try_from(at(base)).unwrap(),
                keep,
                rest: rest.into()
            },
            "{name}"
        );
        assert_eq!(names.get(i), qn, "{name}");
        assert_eq!(e.qualified_name(i), qn, "{name}");
        assert_eq!(e.qualified_name_parts(i).concat(), qn, "{name}");
    }
    // Serialised and bound again, the same names.
    let record: ExtractRecord = postcard::from_bytes(&common::record_bytes(&e)).unwrap();
    let bound = record.bind(Arc::from(source.as_bytes())).unwrap();
    assert_eq!(bound, e);
    assert_eq!(bound.qualified_names(), names);
}

#[test]
fn a_prefix_that_is_not_one_its_base_adds_is_refused() {
    let source = "namespace a { int f() { return 0; }\nnamespace b { int g() { return 0; } } }\n";
    let fresh = extract("cpp", "src/n.cpp", source);
    let g = fresh
        .definitions
        .iter()
        .position(|d| d.name == "g")
        .unwrap();
    let f = u32::try_from(
        fresh
            .definitions
            .iter()
            .position(|d| d.name == "f")
            .unwrap(),
    )
    .unwrap();
    let QualifiedName::Prefix { keep, .. } = fresh.definitions[g].qualified_name else {
        panic!(
            "g is not a prefix: {:?}",
            fresh.definitions[g].qualified_name
        );
    };
    assert_eq!(keep, 7, "src.n.a");
    let src = || Arc::<[u8]>::from(source.as_bytes());
    for (what, base, keep, rest) in [
        ("a keep that is no `.`", f, 6, ".b.g"),
        ("a keep at the base's length", f, 9, ".b.g"),
        ("a keep past the base", f, 40, ".b.g"),
        ("a keep in the base's own base's bytes", f, 5, ".b.g"),
        ("a keep of nothing", f, 0, ".b.g"),
        ("a rest after no `.`", f, 7, "b.g"),
    ] {
        let mut record = fresh.record().clone();
        record.definitions[g].qualified_name = QualifiedName::Prefix {
            base,
            keep,
            rest: rest.into(),
        };
        let bound = record.bind(src());
        assert!(
            matches!(bound, Err(BindError::Prefix { index, .. }) if index == g),
            "{what}: {bound:?}"
        );
    }
    let mut later = fresh.record().clone();
    later.definitions[g].qualified_name = QualifiedName::Prefix {
        base: u32::try_from(g).unwrap(),
        keep: 7,
        rest: ".b.g".into(),
    };
    assert!(matches!(
        later.bind(src()),
        Err(BindError::QualifiedName { .. })
    ));
    // A parameter list must lie in the source too.
    let mut params = fresh.record().clone();
    params.definitions[g].signature = Some(TextRef::Source {
        start: 0,
        end: u32::try_from(source.len()).unwrap() + 1,
    });
    assert!(matches!(params.bind(src()), Err(BindError::Range { .. })));
}

#[test]
fn parameter_lists_are_ranges_and_expanded_ones_owned() {
    let js = "function f0(a = function f1(b = function f2() {}) {}) {}\n";
    let e = extract("javascript", "web/d.js", js);
    for (name, params) in [
        ("f0", "(a = function f1(b = function f2() {}) {})"),
        ("f1", "(b = function f2() {})"),
        ("f2", "()"),
    ] {
        let d = e.definitions.iter().find(|d| d.name == name).unwrap();
        let signature = d.signature.as_ref().unwrap();
        assert!(matches!(signature, TextRef::Source { .. }), "{name}");
        assert_eq!(e.text(signature), params, "{name}");
    }
    // A method only the macro-expanded text gives (the raw parse misreads its class)
    // holds the expanded text's parameter list; a function the raw parse reads holds its
    // range.
    let cpp = "#define PANEL_API\n\nint measure_panel() { return 2; }\n\n\
               class PANEL_API Panel : public Base {\n public:\n  \
               int size() const { return measure_panel(); }\n};\n";
    let e = extract("cpp", "src/panel.cpp", cpp);
    let size = e.definitions.iter().find(|d| d.name == "size").unwrap();
    assert_eq!(size.signature, Some(TextRef::Owned("( )".into())));
    let measure = e
        .definitions
        .iter()
        .find(|d| d.name == "measure_panel")
        .unwrap();
    assert!(matches!(measure.signature, Some(TextRef::Source { .. })));
    assert_eq!(e.text(measure.signature.as_ref().unwrap()), "()");
}
