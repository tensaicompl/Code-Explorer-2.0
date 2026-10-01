//! Extraction is a function of its input: the same source, path and language always
//! give the same extraction, field for field and byte for byte when serialised.

mod common;

use std::fmt::Write as _;

use pdx_engine::{Engine, FileExtract};
use proptest::prelude::*;
use proptest::test_runner::Config;

/// Three extractions of one input, two on one engine and one on a fresh engine, which
/// must all be equal, as must their serialised forms.
fn assert_deterministic(
    language: &str,
    rel_path: &str,
    source: &[u8],
) -> Result<(), TestCaseError> {
    let (first, second) = {
        let engine = Engine::new().expect("an engine");
        (
            engine.extract(language, rel_path, source),
            engine.extract(language, rel_path, source),
        )
    };
    let third = Engine::new()
        .expect("an engine")
        .extract(language, rel_path, source);
    // Any bytes extract: a source the grammar rejects is reported through the file's
    // status, never as an error, and an error here would also hide behind equality.
    prop_assert!(first.is_ok(), "{rel_path}: {:?}", first.as_ref().err());
    prop_assert_eq!(&first, &second, "twice on one engine");
    prop_assert_eq!(&first, &third, "on a fresh engine");
    if let Ok(extract) = &first {
        let bytes = |e: &FileExtract| postcard::to_stdvec(e).expect("serialisable");
        prop_assert_eq!(bytes(extract), bytes(third.as_ref().unwrap()));
    }
    Ok(())
}

/// A Python identifier that is never a keyword.
fn name() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "alpha", "beta", "gamma", "delta", "load", "save", "parse", "render", "item", "value",
    ])
    .prop_map(str::to_owned)
}

/// One statement, rendered at `indent`.
fn statement(indent: usize) -> impl Strategy<Value = String> {
    let pad = " ".repeat(indent);
    prop_oneof![
        (name(), name()).prop_map({
            let pad = pad.clone();
            move |(f, a)| format!("{pad}{f}({a})\n")
        }),
        (name(), name(), name()).prop_map({
            let pad = pad.clone();
            move |(v, o, m)| format!("{pad}{v} = {o}.{m}()\n")
        }),
        name().prop_map({
            let pad = pad.clone();
            move |e| format!("{pad}raise {}Error(\"{e}\")\n", capitalised(&e))
        }),
        name().prop_map({
            let pad = pad.clone();
            move |k| format!("{pad}{k} = os.environ.get(\"{}\")\n", k.to_uppercase())
        }),
        name().prop_map(move |v| format!("{pad}return {v}\n")),
    ]
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

/// A Python module: imports, then functions and classes calling one another.
fn python_module() -> impl Strategy<Value = String> {
    let function = (
        name(),
        prop::collection::vec(name(), 0..3),
        prop::collection::vec(statement(4), 1..6),
    )
        .prop_map(|(f, params, body)| {
            format!("def {f}({}):\n{}\n", params.join(", "), body.concat())
        });
    let class = (
        name(),
        prop::collection::vec((name(), prop::collection::vec(statement(8), 1..4)), 1..3),
    )
        .prop_map(|(c, methods)| {
            let mut text = format!("class {}:\n", capitalised(&c));
            for (m, body) in methods {
                writeln!(text, "    def {m}(self):\n{}", body.concat()).expect("in memory");
            }
            text + "\n"
        });
    (
        prop::collection::vec(name(), 0..3),
        prop::collection::vec(prop_oneof![function, class], 1..6),
    )
        .prop_map(|(imports, defs)| {
            let mut text = String::from("import os\n");
            for i in imports {
                writeln!(text, "from pkg.{i} import {i}").expect("in memory");
            }
            text + "\n" + &defs.concat()
        })
}

/// A smoke fixture, one per language of the matrix, with arbitrary bytes inserted,
/// removed or replaced: source the grammar may not accept, nor UTF-8 decode.
fn mutated_fixture() -> impl Strategy<Value = (String, String, Vec<u8>)> {
    let dir = common::fixtures().join("smoke");
    let mut fixtures: Vec<(String, String, Vec<u8>)> = std::fs::read_dir(&dir)
        .expect("the smoke fixtures")
        .map(|e| {
            let path = e.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let lang = name.split('.').next().unwrap().to_owned();
            (lang, name, std::fs::read(&path).unwrap())
        })
        .collect();
    fixtures.sort();
    (
        prop::sample::select(fixtures),
        prop::collection::vec((any::<prop::sample::Index>(), 0u8..3, any::<u8>()), 0..12),
    )
        .prop_map(|((lang, name, mut source), edits)| {
            for (at, op, byte) in edits {
                let i = at.index(source.len() + 1);
                match op {
                    0 => source.insert(i, byte),
                    1 if i < source.len() => {
                        source.remove(i);
                    }
                    _ if i < source.len() => source[i] = byte,
                    _ => source.push(byte),
                }
            }
            (lang, name, source)
        })
}

fn config() -> Config {
    Config {
        cases: 64,
        // A failing case is printed, minimised, rather than written into the tree.
        failure_persistence: None,
        ..Config::default()
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn extract_is_deterministic(module in python_module()) {
        assert_deterministic("python", "pkg/module.py", module.as_bytes())?;
    }

    #[test]
    fn extract_is_deterministic_on_any_bytes((language, name, source) in mutated_fixture()) {
        assert_deterministic(&language, &name, &source)?;
    }
}
