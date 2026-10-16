//! The 17 calls of the Rust golden (`bench/golden/rust/axum.expected.json`) that A0
//! found drawn to the wrong definition in a trusted band (issue 104): what each is
//! after R2, in the committed golden. Reads only the committed file: no fetch, no
//! index, no network. `make golden` checks the file against a real index of the pinned
//! repository.
//!
//! A drawn edge is `(src_qn, dst_qn, kind, band)`, so a wrong edge is checked absent
//! only where no other call of the same caller may draw it; where one does
//! (`derive_from_request` calls the crate root's `expand` too), the right edge is
//! checked present instead.

use std::path::Path;

use pdx_bench::golden::GoldenExpected;

fn axum() -> GoldenExpected {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/golden/rust/axum.expected.json");
    let bytes = std::fs::read(&path).expect("the committed golden");
    GoldenExpected::parse(&bytes).expect("a golden")
}

/// The bands of the edges from `src` to `dst` of `kind`.
fn bands(golden: &GoldenExpected, src: &str, dst: &str, kind: &str) -> Vec<String> {
    golden
        .drawn_edges
        .iter()
        .filter(|e| e.src_qn == src && e.dst_qn == dst && e.kind == kind)
        .map(|e| e.band.clone())
        .collect()
}

/// Whether any edge goes from `src` to `dst`, of any kind and band.
fn any_edge(golden: &GoldenExpected, src: &str, dst: &str) -> bool {
    golden
        .drawn_edges
        .iter()
        .any(|e| e.src_qn == src && e.dst_qn == dst)
}

#[test]
fn a_module_qualified_call_is_drawn_to_its_module() {
    // `from_request::expand`, `debug_handler::expand`, `typed_path::expand`
    // (`axum-macros/src/lib.rs:377, 426, 582, 638, 665`): typed, to the module the path
    // names, not the crate root's `expand`.
    let golden = axum();
    for (caller, target) in [
        ("derive_from_request", "from_request.mod.expand"),
        ("derive_from_request_parts", "from_request.mod.expand"),
        ("debug_handler", "debug_handler.expand"),
        ("debug_middleware", "debug_handler.expand"),
        ("derive_typed_path", "typed_path.expand"),
    ] {
        let src = format!("axum-macros.src.lib.{caller}");
        let dst = format!("axum-macros.src.{target}");
        assert_eq!(
            bands(&golden, &src, &dst, "CALLS"),
            ["typed"],
            "{src} -> {dst}"
        );
    }
}

#[test]
fn no_call_of_the_seventeen_is_drawn_to_its_wrong_definition() {
    // External (`multer::parse_boundary`, `hyper::upgrade::on`), candidate
    // (`PathDeserializationError::custom`), unresolved (the local
    // `find_first_not_double`), blocked (`Handler::call`) and not a call (the arm's
    // `params`): none draws the edge A0 found.
    let golden = axum();
    for (src, wrong) in [
        (
            "axum.src.extract.multipart.Multipart.from_request",
            "axum-extra.src.extract.multipart.parse_boundary",
        ),
        (
            "axum.src.extract.ws.http2#cfg(feature=http2)]",
            "axum.src.routing.method_routing.on",
        ),
        (
            "examples.http-proxy.src.main.proxy",
            "axum.src.routing.method_routing.on",
        ),
        (
            "axum.src.extract.path.de.MapDeserializer.next_value_seed",
            "axum.src.extract.connect_info.custom",
        ),
        (
            "axum.src.extract.path.de.KeyDeserializer.deserialize_any",
            "axum.src.extract.connect_info.custom",
        ),
        (
            "axum.src.routing.strip_prefix.capture_prefix_suffix",
            "axum-macros.src.typed_path.find_first_not_double",
        ),
        (
            "axum.src.handler.service.HandlerService.call",
            "axum.src.routing.method_routing.call",
        ),
        (
            "axum.src.extract.path.inner_path.InnerPath.from_request_parts",
            "axum.src.extract.path.inner_path.params",
        ),
    ] {
        assert!(!any_edge(&golden, src, wrong), "{src} -> {wrong}");
    }
}

#[test]
fn the_issue_104_leaves_are_no_trusted_edge() {
    // What R2 left to its children. The blanket impl the engine could not tell from the
    // type's own is no typed answer since issue 109: the type's own impl's header is not
    // proven for `TestExtractor`. Stage 3's `scoped` guess, still the wrong impl, draws
    // no edge since issue 102 (R3): the R3 acceptance case (`with_rejection.rs:196`), no
    // `CALLS` and no `TESTS` tuple. The impl a macro writes, which no extraction
    // represents, is withheld since issue 110: the macro may write another `from` for
    // `Body`, so the one the registry holds is drawn no more.
    let golden = axum();
    for kind in ["CALLS", "TESTS"] {
        assert_eq!(
            bands(
                &golden,
                "axum-extra.src.extract.with_rejection.extractor_rejection_is_transformed",
                "axum-extra.src.extract.with_rejection.WithRejection.from_request",
                kind
            ),
            Vec::<&str>::new(),
            "{kind}"
        );
    }
    assert_eq!(
        bands(
            &golden,
            "axum-core.src.ext_traits.request.extract_body_without_state",
            "axum-core.src.body.Body.from",
            "CALLS"
        ),
        Vec::<&str>::new()
    );
}
