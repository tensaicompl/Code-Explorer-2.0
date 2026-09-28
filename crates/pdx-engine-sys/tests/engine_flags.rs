//! The engine this crate links was compiled under the engine's own warning policy.
//!
//! `CMake` records every compile it runs. Read from the build this crate made, on
//! whatever system runs the test, that record must show: warnings as errors for
//! every source; nothing switching all warnings off, except for the generated
//! grammar tables, which are silenced by design; the named exemptions on vendored
//! sources only, and none on the interface layer.

use std::path::Path;

/// One compile: the source it compiles and the arguments it was given.
struct Compile {
    file: String,
    args: Vec<String>,
}

fn compiles() -> Vec<Compile> {
    let path = Path::new(env!("PDXE_ENGINE_BUILD_DIR")).join("compile_commands.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e}; the engine build records its compiles",
            path.display()
        )
    });
    let entries: Vec<serde_json::Value> = serde_json::from_str(&text).expect("valid JSON");
    entries
        .iter()
        .map(|e| {
            let file = e["file"].as_str().expect("a file").replace('\\', "/");
            let args = match (e["command"].as_str(), e["arguments"].as_array()) {
                (Some(command), _) => command.split_whitespace().map(str::to_owned).collect(),
                (None, Some(args)) => args
                    .iter()
                    .map(|a| a.as_str().expect("an argument").to_owned())
                    .collect(),
                (None, None) => panic!("{file}: no command recorded"),
            };
            Compile { file, args }
        })
        .collect()
}

fn is_grammar(c: &Compile) -> bool {
    c.file
        .rsplit('/')
        .next()
        .is_some_and(|name| name.starts_with("grammar_"))
}

fn is_interface(c: &Compile) -> bool {
    c.file.contains("/engine/api/")
}

fn is_c(c: &Compile) -> bool {
    Path::new(&c.file)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("c"))
}

#[test]
fn the_engine_is_compiled_with_its_own_warning_policy() {
    let all = compiles();
    assert!(all.len() > 40, "only {} compiles recorded", all.len());
    assert!(
        all.iter().any(is_interface),
        "no interface source was compiled"
    );

    let mut problems = Vec::new();
    for c in &all {
        let has = |flag: &str| c.args.iter().any(|a| a == flag);
        if is_grammar(c) {
            continue; // generated parser tables, compiled with warnings off by design
        }
        if has("-w") {
            problems.push(format!(
                "{}: compiled with every warning switched off (-w)",
                c.file
            ));
        }
        for required in ["-Wall", "-Wextra", "-Werror"] {
            if !has(required) {
                problems.push(format!("{}: compiled without {required}", c.file));
            }
        }
        let exemptions: Vec<&String> = c
            .args
            .iter()
            .filter(|a| a.starts_with("-Wno-error"))
            .collect();
        if is_interface(c) && !exemptions.is_empty() {
            problems.push(format!(
                "{}: an interface source carries exemptions {exemptions:?}",
                c.file
            ));
        }
        if !is_interface(c) && is_c(c) && !has("-Wno-error=unused-function") {
            problems.push(format!(
                "{}: a vendored source is missing its named exemptions",
                c.file
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn the_engine_is_built_from_a_plain_path() {
    // A verbatim Windows path (\\?\D:\...) reaches the compiler as //?/D:/..., under
    // which the engine's relative includes do not resolve.
    for c in compiles() {
        assert!(
            !c.file.starts_with("//?/") && !c.file.starts_with(r"\\?\"),
            "{}: compiled through a verbatim path",
            c.file
        );
    }
}
