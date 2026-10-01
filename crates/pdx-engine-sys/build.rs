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
use std::path::{Component, Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let engine = engine_dir(&manifest);

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
        "CMAKE_GENERATOR",
        "MACOSX_DEPLOYMENT_TARGET",
        "PDX_WRITE_BINDINGS",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }

    link_engine(&engine);

    #[cfg(feature = "regenerate-bindings")]
    regenerate_bindings(&engine, &manifest);
}

/// The engine's directory, two levels above this crate, as a plain absolute path.
///
/// Built from the parts of the path rather than asked of the filesystem: on Windows
/// the filesystem's own answer is a verbatim path (`\\?\D:\...`), which `CMake` passes
/// to the compiler as `//?/D:/...`, and under that spelling the engine's sources
/// cannot reach each other through the relative includes they are written with.
fn engine_dir(manifest: &Path) -> PathBuf {
    let mut dir = PathBuf::new();
    for part in manifest.join("../../engine").components() {
        match part {
            Component::ParentDir => {
                dir.pop();
            }
            Component::CurDir => {}
            other => dir.push(other),
        }
    }
    let spelled = dir.to_string_lossy();
    assert!(
        !spelled.starts_with(r"\\?\") && !spelled.starts_with("//?/"),
        "the engine's path is a verbatim path ({spelled}); its sources' relative \
         includes do not resolve under one"
    );
    assert!(
        dir.join("CMakeLists.txt").is_file(),
        "no engine at {}; this crate builds from the repository",
        dir.display()
    );
    dir
}

/// Builds `libpdxe` with `CMake` and tells rustc how to link it.
fn link_engine(engine: &Path) {
    let target = env::var("TARGET").expect("set by cargo");
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    let mut config = cmake::Config::new(engine);
    // The engine's `CMake` project owns every compiler flag, its warning policy
    // included. Left to itself, the cmake crate passes the flags of its own compiler
    // probe as the base flags of every compile, and those switch all warnings off
    // (`-w`). Defining the base flags, empty, is what keeps them out.
    config
        .define("CMAKE_C_FLAGS", "")
        .define("CMAKE_CXX_FLAGS", "");

    if target_os == "windows" && target_env == "msvc" {
        // On Windows the engine is built with clang, for the MSVC ABI Rust uses;
        // Microsoft's own compiler is not a supported engine compiler. Named to
        // `CMake` directly, with the target stated rather than left to the default of
        // whichever clang is installed. Ninja, because the Visual Studio generators
        // always drive Microsoft's compiler.
        config
            .define("CMAKE_C_COMPILER", "clang")
            .define("CMAKE_CXX_COMPILER", "clang++")
            .define("CMAKE_C_COMPILER_TARGET", &target)
            .define("CMAKE_CXX_COMPILER_TARGET", &target);
        if env::var_os("CMAKE_GENERATOR").is_none() {
            // Keep going after a failed compile, so a build that fails reports every
            // failure at once rather than the first.
            config.generator("Ninja").build_arg("-k").build_arg("0");
        }
    }
    if target_os == "macos" {
        // The architecture and the oldest system are Rust's, so the objects link into
        // what rustc produces without a mismatch.
        let arch = if target_arch == "aarch64" {
            "arm64"
        } else {
            target_arch.as_str()
        };
        config.define("CMAKE_OSX_ARCHITECTURES", arch);
        let deployment = env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or_else(|_| {
            if target_arch == "aarch64" {
                "11.0"
            } else {
                "10.12"
            }
            .to_owned()
        });
        config.define("CMAKE_OSX_DEPLOYMENT_TARGET", deployment);
    }
    // The fault-injection switches, only when a test build asks (the feature's comment
    // in Cargo.toml says why).
    let seams = if cfg!(feature = "test-seams") {
        "ON"
    } else {
        "OFF"
    };
    let dst = config
        .define("PDXE_TEST_SEAMS", seams)
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

    // Where the build is, for the test that reads how the engine was compiled.
    println!("cargo:rustc-env=PDXE_ENGINE_BUILD_DIR={}", build.display());

    // What the engine's `CMake` project links it with, named again for rustc, which
    // cannot read that project. One translation unit is C++, the macro preprocessor,
    // so the C++ runtime is needed; with the MSVC toolchain the objects name their
    // runtimes themselves, and what remains are the system libraries the engine
    // calls on Windows: advapi32 for the access control the foundation puts on the
    // directories it creates, bcrypt for the random names it gives them.
    match (target_os.as_str(), target_env.as_str()) {
        ("macos" | "ios", _) => println!("cargo:rustc-link-lib=dylib=c++"),
        ("windows", "msvc") => {
            println!("cargo:rustc-link-lib=dylib=advapi32");
            println!("cargo:rustc-link-lib=dylib=bcrypt");
        }
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
