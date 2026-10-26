//! A binding the caller's own file (or, for a type, its module) makes is closer than an
//! import that binds no name of its own: such an import is no evidence for a namesake
//! elsewhere when one is defined here. Resolved through discovery, extraction, the
//! registry and Stage 3 alone, with no typed answer: typed resolution answers these
//! shapes itself when it can, so Stage 3's own reading is what is tested.

use std::fs;
use std::path::Path;

use pdx_core::bands::Band;
use pdx_core::config::PdxConfig;
use pdx_core::index::discover::discover;
use pdx_core::index::extract::{ExtractLimits, ExtractStage};
use pdx_core::resolve::registry::SymbolRegistry;
use pdx_core::resolve::stages::{Resolution, ResolveReport, resolve_with};
use pdx_engine::{ProjectResolution, RunHealth, RunStatus};

struct Resolved {
    _dir: tempfile::TempDir,
    reg: SymbolRegistry,
    report: ResolveReport,
}

fn build(files: &[(&str, &str)]) -> Resolved {
    let dir = tempfile::Builder::new()
        .prefix("pdx-import-shadowing-test-")
        .tempdir()
        .expect("a directory");
    for (path, content) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
        fs::write(&path, content).expect("a file");
    }
    let root: &Path = dir.path();
    let config = PdxConfig::load(root).expect("the configuration loads");
    let files = discover(root, &config).expect("discovery succeeds");
    let limits = ExtractLimits {
        requested_workers: 2,
        memory_budget_bytes: 64 << 20,
    };
    let report = ExtractStage::new(root, &config.secrets, limits)
        .run(&files)
        .expect("extraction succeeds");
    let reg = SymbolRegistry::build(root, &files, report).expect("the registry builds");
    let health = RunHealth {
        status: RunStatus::Clean,
        files: 0,
        files_resolved: 0,
        files_untyped: 0,
        files_empty: 0,
        files_over_budget: 0,
        files_source_unavailable: 0,
        files_not_reached: 0,
        pass_failures: 0,
    };
    let report = resolve_with(
        &reg,
        &ProjectResolution {
            resolutions: Vec::new(),
            health,
        },
    )
    .expect("resolution succeeds");
    Resolved {
        _dir: dir,
        reg,
        report,
    }
}

fn at<'r>(r: &'r Resolved, path: &str, callee: &str) -> &'r Resolution {
    let found: Vec<&Resolution> = r
        .report
        .resolutions
        .iter()
        .filter(|s| {
            s.site_ref.rel_path == path
                && r.reg
                    .text(path, &s.call(&r.reg).expect("a call").callee_text)
                    .is_some_and(|t| t == callee)
        })
        .map(|s| &s.resolution)
        .collect();
    assert_eq!(found.len(), 1, "{callee} in {path}: {found:?}");
    found[0]
}

/// Not drawn to a definition in `path`'s other file `elsewhere`.
fn not_drawn_to(r: &Resolved, got: &Resolution, elsewhere: &str) {
    if let Some(target) = got.target() {
        assert_ne!(target.path, elsewhere, "{got:?}");
    }
    let _ = r;
}

#[test]
fn a_c_files_own_static_function_shadows_an_included_headers_twin() {
    let r = build(&[
        ("y.h", "void helper(void);\n"),
        (
            "y.c",
            "#include \"y.h\"\n\nstatic void usage(void) {}\n\nvoid helper(void) { usage(); }\n",
        ),
        (
            "x.c",
            "#include \"y.h\"\n\nstatic void usage(void) {}\n\nint main(void) {\n    usage();\n    helper();\n    return 0;\n}\n",
        ),
    ]);
    let usage = at(&r, "x.c", "usage");
    not_drawn_to(&r, usage, "y.c");
    assert_ne!(usage.band(), Band::ImportGuided, "{usage:?}");
}

#[test]
fn a_python_def_shadows_a_star_import() {
    let r = build(&[
        ("pkg/__init__.py", ""),
        ("pkg/a.py", "def f():\n    return 1\n"),
        (
            "pkg/b.py",
            "from pkg.a import *\n\n\ndef f():\n    return 2\n\n\ndef g():\n    return f()\n",
        ),
    ]);
    let f = at(&r, "pkg/b.py", "f");
    not_drawn_to(&r, f, "pkg/a.py");
    assert_ne!(f.band(), Band::ImportGuided, "{f:?}");
}

#[test]
fn a_java_type_of_the_callers_package_shadows_a_wildcard_imports() {
    let r = build(&[
        (
            "src/main/java/com/lib/Util.java",
            "package com.lib;\n\npublic class Util {\n    public static int helper() { return 1; }\n}\n",
        ),
        (
            "src/main/java/com/app/Util.java",
            "package com.app;\n\npublic class Util {\n    public static int helper() { return 2; }\n}\n",
        ),
        (
            "src/main/java/com/app/Main.java",
            "package com.app;\n\nimport com.lib.*;\n\npublic class Main {\n    int run() { return Util.helper(); }\n}\n",
        ),
    ]);
    let helper = at(&r, "src/main/java/com/app/Main.java", "Util.helper");
    not_drawn_to(&r, helper, "src/main/java/com/lib/Util.java");
}
