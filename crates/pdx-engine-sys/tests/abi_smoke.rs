//! The engine through its Rust bindings.
//!
//! `abi_smoke` is the C test of the same name, run through the bindings: every
//! language of the matrix, one fixture each, must extract, parse, and yield a
//! definition besides its module. A typed resolution across files is also run, to
//! show that the project interface and its nested structures cross the boundary
//! intact.

use std::ffi::{CStr, CString, c_char};
use std::path::{Path, PathBuf};
use std::ptr;

use pdx_engine_sys as sys;

/// The language matrix, and the one variant of it the interface names separately.
const LANGUAGES: [&str; 32] = [
    "java",
    "kotlin",
    "scala",
    "typescript",
    "tsx",
    "javascript",
    "python",
    "go",
    "c",
    "cpp",
    "csharp",
    "rust",
    "php",
    "perl",
    "ada",
    "bash",
    "ruby",
    "swift",
    "objc",
    "groovy",
    "lua",
    "sql",
    "protobuf",
    "graphql",
    "yaml",
    "json",
    "toml",
    "hcl",
    "dockerfile",
    "markdown",
    "xml",
    "properties",
];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../engine/tests/fixtures")
}

/// The one fixture named after `lang`, as `<lang>.<ext>`.
fn fixture_for(lang: &str) -> PathBuf {
    let dir = fixtures().join("smoke");
    std::fs::read_dir(&dir)
        .expect("the smoke fixtures are readable")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.split_once('.'))
                .is_some_and(|(stem, _)| stem == lang)
        })
        .unwrap_or_else(|| panic!("no smoke fixture for {lang}"))
}

/// A string the engine owns, read as UTF-8.
///
/// # Safety
///
/// `s` is NULL or points to a NUL-terminated string that outlives the returned
/// borrow.
unsafe fn text<'a>(s: *const c_char) -> &'a str {
    if s.is_null() {
        return "";
    }
    // SAFETY: the caller guarantees `s` is a live NUL-terminated string.
    unsafe { CStr::from_ptr(s) }
        .to_str()
        .expect("the engine returns UTF-8")
}

/// A context for one test, shut down when it is dropped.
struct Context(*mut sys::pdxe_ctx);

impl Context {
    fn new() -> Self {
        let mut ctx = ptr::null_mut();
        // SAFETY: `ctx` is a valid place for the engine to write a context pointer.
        assert_eq!(unsafe { sys::pdxe_init(&raw mut ctx) }, sys::PDXE_OK);
        assert!(!ctx.is_null());
        Self(ctx)
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: the context came from pdxe_init and is shut down once.
        unsafe { sys::pdxe_shutdown(self.0) };
    }
}

fn language_id(lang: &str) -> i32 {
    let name = CString::new(lang).expect("no NUL in a language name");
    // SAFETY: `name` is a valid C string for the duration of the call.
    unsafe { sys::pdxe_language_id(name.as_ptr()) }
}

#[test]
fn abi_smoke() {
    let ctx = Context::new();
    // SAFETY: pdxe_version returns a static string.
    assert!(!unsafe { text(sys::pdxe_version()) }.is_empty());
    let parsed = i32::try_from(sys::PDXE_FILE_PARSED).expect("a small status value");
    for lang in LANGUAGES {
        let id = language_id(lang);
        assert!(id > 0, "{lang}: not a language this build knows");
        let path = fixture_for(lang);
        let bytes = std::fs::read(&path).expect("the fixture is readable");
        let rel = CString::new(path.file_name().unwrap().to_string_lossy().into_owned()).unwrap();
        let mut result = ptr::null_mut();
        // SAFETY: every pointer is valid for the call; `result` receives a result
        // the caller owns and frees below.
        let rc = unsafe {
            sys::pdxe_extract_file(
                ctx.0,
                id,
                rel.as_ptr(),
                bytes.as_ptr(),
                bytes.len(),
                &raw mut result,
            )
        };
        assert_eq!(rc, sys::PDXE_OK, "{lang}: extraction failed");
        // SAFETY: a successful extraction leaves `result` pointing at a result that
        // lives until pdxe_result_free, and its arrays hold `n_*` elements.
        let r = unsafe { &*result };
        assert_eq!(r.status, parsed, "{lang}: did not parse cleanly");
        // SAFETY: as above; `defs` holds `n_defs` definitions.
        let defs = unsafe { std::slice::from_raw_parts(r.defs, r.n_defs as usize) };
        // SAFETY: each kind is a string the result owns.
        let own = defs
            .iter()
            .filter(|d| unsafe { text(d.kind) } != "module")
            .count();
        assert!(own > 0, "{lang}: no definition besides the module");
        // SAFETY: the result came from pdxe_extract_file and is freed once.
        unsafe { sys::pdxe_result_free(ctx.0, result) };
    }
}

#[test]
fn typed_resolution_crosses_the_boundary() {
    let ctx = Context::new();
    let dir = fixtures().join("resolve/python_cross_file_calls");
    let files = ["app/models.py", "app/service.py"];
    let python = language_id("python");

    let mut project = ptr::null_mut();
    // SAFETY: `project` receives a project that is ended below.
    assert_eq!(
        unsafe { sys::pdxe_resolve_project_begin(ctx.0, &raw mut project) },
        sys::PDXE_OK
    );
    // The sources and results must outlive the project, which reads them.
    let sources: Vec<Vec<u8>> = files
        .iter()
        .map(|f| std::fs::read(dir.join(f)).expect("the fixture is readable"))
        .collect();
    let names: Vec<CString> = files.iter().map(|f| CString::new(*f).unwrap()).collect();
    let mut results = Vec::new();
    for (name, source) in names.iter().zip(&sources) {
        let mut result = ptr::null_mut();
        // SAFETY: every pointer is valid for the call.
        let rc = unsafe {
            sys::pdxe_extract_file(
                ctx.0,
                python,
                name.as_ptr(),
                source.as_ptr(),
                source.len(),
                &raw mut result,
            )
        };
        assert_eq!(rc, sys::PDXE_OK);
        // SAFETY: the result stays alive until after the project ends.
        let rc = unsafe {
            sys::pdxe_resolve_project_add_file(
                project,
                python,
                name.as_ptr(),
                source.as_ptr(),
                source.len(),
                result,
            )
        };
        assert_eq!(rc, sys::PDXE_OK);
        results.push(result);
    }
    // SAFETY: the project is live and has not run.
    assert_eq!(
        unsafe { sys::pdxe_resolve_project_run(project) },
        sys::PDXE_OK
    );

    let mut out = ptr::null();
    let mut n = 0u32;
    // SAFETY: the project has run; the array it returns lives until it ends.
    assert_eq!(
        unsafe { sys::pdxe_resolve_project_results(project, &raw mut out, &raw mut n) },
        sys::PDXE_OK
    );
    // SAFETY: as above.
    let resolutions = unsafe { std::slice::from_raw_parts(out, n as usize) };
    let found = resolutions.iter().any(|r| {
        // SAFETY: every string belongs to the project or to a result it read, all
        // alive until the project ends.
        unsafe {
            text(r.rel_path) == "app/service.py"
                && text(r.target_qualified_name) == "app.models.make_user"
                && text(r.target_rel_path) == "app/models.py"
                && text(r.strategy) == "lsp_typed"
                && text(r.engine_strategy) == "lsp_callable_alias"
                && text(r.site.callee_text) == "make_user"
                && r.site.is_reference == 0
                && r.candidates == 1
        }
    });
    assert!(
        found,
        "the cross-file call to make_user was not resolved through the bindings"
    );

    // SAFETY: the project ends before the results it read are freed.
    unsafe { sys::pdxe_resolve_project_end(project) };
    for result in results {
        // SAFETY: each result came from pdxe_extract_file and is freed once.
        unsafe { sys::pdxe_result_free(ctx.0, result) };
    }
}
