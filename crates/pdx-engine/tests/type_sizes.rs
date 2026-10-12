//! C and C++ preprocessing's `sizeof` in `#if` has one fixed data model, the same on
//! every platform (docs/plan/ISSUES.md, issue 85; decision 54).
//!
//! The engine used to size types as the platform it was built for does: `long` is 8
//! bytes on Linux and macOS and 4 on Windows, `long double` 16 on x86-64 and 8 on Apple
//! silicon and Windows, so a source whose `#if` asks one extracted differently on each.
//! Each test here expects the fixed model's answers on every platform: nothing below
//! depends on the platform it runs on, so the Windows run proves Windows takes the same
//! branch as Linux.
//!
//! Other platforms are also simulated, in child processes: the test seam
//! `PDX_ENGINE_TEST_HOST_SIZES` gives the engine the sizes a build there would have read.

mod common;

use std::fmt::Write as _;
use std::path::PathBuf;

use pdx_engine::Engine;

const HOST_SIZES: &str = "PDX_ENGINE_TEST_HOST_SIZES";
const OUT: &str = "PDX_TYPE_SIZES_OUT";

/// Calls that depend on the branches an `#if` and an `#elif` on `sizeof` take.
const MODEL: &[u8] = b"int model_lp64(void);
#if sizeof(long) == 8 && sizeof(long double) == 16 && sizeof(char *) == 8
#define MODEL() model_lp64()
#elif sizeof(long) == 4
#define MODEL() model_llp64()
#else
#define MODEL() model_other()
#endif
#if sizeof(long double) == 16
#define WIDE() wide_sixteen()
#elif sizeof(long double) == 8
#define WIDE() wide_eight()
#endif
int f(void) { return MODEL() + WIDE(); }
";

/// Every extraction of the model source, C and C++, as text.
fn extractions() -> String {
    let engine = Engine::new().unwrap();
    let mut out = String::new();
    for language in ["c", "cpp"] {
        let path = format!("src/model.{language}");
        let extract = engine.extract(language, &path, MODEL).unwrap();
        writeln!(out, "{extract:?}").unwrap();
    }
    out
}

fn callees(language: &str) -> Vec<String> {
    let engine = Engine::new().unwrap();
    let extract = engine
        .extract(language, &format!("src/model.{language}"), MODEL)
        .unwrap();
    extract.calls.into_iter().map(|c| c.callee_text).collect()
}

#[test]
fn sizeof_in_if_takes_the_fixed_models_branches_on_every_platform() {
    for language in ["c", "cpp"] {
        let callees = callees(language);
        for expected in ["model_lp64", "wide_sixteen"] {
            assert!(
                callees.iter().any(|c| c == expected),
                "{language}: {expected} is not called: {callees:?}"
            );
        }
        for other in ["model_llp64", "model_other", "wide_eight"] {
            assert!(
                !callees.iter().any(|c| c == other),
                "{language}: {other} is called: {callees:?}"
            );
        }
    }
}

#[test]
fn host_type_sizes_cannot_change_an_extraction() {
    const TEST: &str = "host_type_sizes_cannot_change_an_extraction";
    if let Some(out) = std::env::var_os(OUT) {
        // A child: extract as a build with the simulated sizes would.
        std::fs::write(PathBuf::from(out), extractions()).unwrap();
        return;
    }
    let own = extractions();
    let dir = tempfile_dir();
    for (name, sizes) in [
        ("64-bit Windows", "long=4,long int=4,long double=8"),
        ("Apple silicon", "long double=8"),
        (
            "a 32-bit build",
            "long=4,long int=4,long double=12,char *=4,int *=4,long *=4,long int *=4",
        ),
    ] {
        let out = dir.join(name.replace(' ', "-"));
        assert!(common::rerun_in_child(
            TEST,
            &[(HOST_SIZES, sizes), (OUT, out.to_str().unwrap())]
        ));
        assert_eq!(
            std::fs::read_to_string(&out).unwrap(),
            own,
            "{name}'s sizes changed the extraction"
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// A fresh directory for the children's answers.
fn tempfile_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdx-type-sizes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
