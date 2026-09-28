//! Builds the engine and links it.
//!
//! The engine is built by the same `CMake` project `make engine` uses
//! (`engine/CMakeLists.txt`), so there is one description of how it compiles, not
//! two. Only the library is built here; its test programs are built by `make`.
//!
//! With the `regenerate-bindings` feature the bindings are generated again from the
//! public header. `make bindgen` sets `PDX_WRITE_BINDINGS`, and the result replaces
//! `src/bindings.rs`; any other build with the feature compares the result with the
//! committed file and fails if they differ, so bindings that no longer match the
//! header cannot go unnoticed.

use std::env;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let engine = manifest
        .join("../../engine")
        .canonicalize()
        .expect("the engine directory sits two levels above this crate");

    for watched in [
        "CMakeLists.txt",
        "include",
        "api",
        "src",
        "vendored",
        "grammars",
    ] {
        println!("cargo:rerun-if-changed={}", engine.join(watched).display());
    }
    for var in [
        "CC",
        "CXX",
        "CFLAGS",
        "CXXFLAGS",
        "CMAKE_GENERATOR",
        "PDX_WRITE_BINDINGS",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }

    link_engine(&engine);

    #[cfg(feature = "regenerate-bindings")]
    regenerate_bindings(&engine, &manifest);
}

/// Builds `libpdxe` with `CMake` and tells rustc how to link it.
fn link_engine(engine: &Path) {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    let mut config = cmake::Config::new(engine);
    if target_os == "windows" && target_env == "msvc" {
        // On Windows the engine is built with clang, for the MSVC ABI Rust uses;
        // Microsoft's own compiler is not a supported engine compiler. Ninja, because
        // the Visual Studio generators always drive Microsoft's compiler.
        let mut c = cc::Build::new();
        c.compiler("clang");
        let mut cxx = cc::Build::new();
        cxx.cpp(true).compiler("clang++");
        config.init_c_cfg(c).init_cxx_cfg(cxx);
        if env::var_os("CMAKE_GENERATOR").is_none() {
            config.generator("Ninja");
        }
    }
    let dst = config
        .define("PDXE_BUILD_TESTS", "OFF")
        // Always optimised: the engine is a dependency, not what is being debugged,
        // and an unoptimised parser is slow enough to distort every test above it.
        .profile("Release")
        .build_target("pdxe")
        .build();

    // The archive lands in the build directory, or in a per-configuration directory
    // beneath it under a multi-configuration generator.
    let build = dst.join("build");
    let dir = [build.join("Release"), build.clone()]
        .into_iter()
        .find(|d| d.join("libpdxe.a").is_file() || d.join("pdxe.lib").is_file())
        .unwrap_or_else(|| panic!("the engine built, but no library is in {}", build.display()));
    println!("cargo:rustc-link-search=native={}", dir.display());
    println!("cargo:rustc-link-lib=static=pdxe");

    // One translation unit is C++, the macro preprocessor, so the C++ runtime is
    // needed. With the MSVC toolchain the objects name their runtime themselves.
    match (target_os.as_str(), target_env.as_str()) {
        ("macos" | "ios", _) => println!("cargo:rustc-link-lib=dylib=c++"),
        ("windows", "msvc") => {}
        ("linux" | "android", _) => {
            println!("cargo:rustc-link-lib=dylib=stdc++");
            println!("cargo:rustc-link-lib=dylib=m");
            println!("cargo:rustc-link-lib=dylib=pthread");
        }
        _ => println!("cargo:rustc-link-lib=dylib=stdc++"),
    }
}

/// The Rust type of each constant the header defines as a macro. Status codes are
/// what the functions return, so they share its type; the no-parent marker is an
/// index, compared with index fields, so it shares theirs. Left to itself bindgen
/// picks a type from each value, which types `PDXE_OK` unsigned and the error codes
/// signed.
#[cfg(feature = "regenerate-bindings")]
#[derive(Debug)]
struct MacroTypes;

#[cfg(feature = "regenerate-bindings")]
impl bindgen::callbacks::ParseCallbacks for MacroTypes {
    fn int_macro(&self, name: &str, _value: i64) -> Option<bindgen::callbacks::IntKind> {
        use bindgen::callbacks::IntKind;
        match name {
            "PDXE_NO_PARENT" => Some(IntKind::U32),
            n if n == "PDXE_OK" || n.starts_with("PDXE_E_") => Some(IntKind::Int),
            _ => None,
        }
    }
}

/// Generates the bindings from the public header, then writes or checks them.
#[cfg(feature = "regenerate-bindings")]
fn regenerate_bindings(engine: &Path, manifest: &Path) {
    let header = engine.join("include/pdxe.h");
    let bindings = bindgen::Builder::default()
        .header(header.to_string_lossy())
        // The header's comments are its documentation; carry them over.
        .clang_arg("-fparse-all-comments")
        .allowlist_function("pdxe_.*")
        .allowlist_type("pdxe_.*")
        .allowlist_var("PDXE_.*")
        .translate_enum_integer_types(true)
        // The header's constants are unique by their own prefix; keep their names.
        .prepend_enum_name(false)
        .parse_callbacks(Box::new(MacroTypes))
        .derive_default(true)
        .rust_edition(bindgen::RustEdition::Edition2024)
        .formatter(bindgen::Formatter::Rustfmt)
        .raw_line(
            "// Generated by bindgen from engine/include/pdxe.h. Do not edit: change the \
             header and run `make bindgen`.",
        )
        .generate()
        .expect("bindgen could not read the public header");
    let generated = bindings.to_string();

    let committed_path = manifest.join("src/bindings.rs");
    if env::var_os("PDX_WRITE_BINDINGS").is_some() {
        std::fs::write(&committed_path, &generated).expect("write src/bindings.rs");
        return;
    }
    let committed = std::fs::read_to_string(&committed_path).unwrap_or_default();
    assert!(
        committed == generated,
        "src/bindings.rs does not match engine/include/pdxe.h; run `make bindgen` and \
         commit the result"
    );
}
