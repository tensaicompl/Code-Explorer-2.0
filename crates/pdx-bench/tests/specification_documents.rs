//! The specification documents exist, are marked normative, and cover every
//! section they are meant to cover.
//!
//! These documents are what later tasks are implemented against, so a missing or
//! unmarked one is a defect: an unmarked document invites a silent edit, and a
//! missing one sends an implementer back to a plan that is not in the repository.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The sections that must each have a document. The component map is a diagram
/// rather than a specification and is deliberately not among them.
const SECTIONS: [&str; 13] = [
    "4.2", "4.3", "4.4", "4.5", "4.6", "4.7", "4.8", "4.9", "4.10", "4.11", "4.12", "4.13", "4.14",
];

fn spec_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/spec")
        .canonicalize()
        .expect("the specification directory exists")
}

/// Every specification document, by path. The directory's own index is not one.
fn documents() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(spec_dir())
        .expect("the specification directory is readable")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .filter(|p| p.file_name().is_some_and(|n| n != "README.md"))
        .collect();
    out.sort();
    out
}

#[test]
fn every_specification_section_has_a_document() {
    let docs = documents();
    let found: BTreeSet<String> = docs
        .iter()
        .filter_map(|p| p.file_name()?.to_str())
        .filter_map(|n| n.split('-').next())
        .map(str::to_owned)
        .collect();
    let expected: BTreeSet<String> = SECTIONS.iter().map(|s| (*s).to_owned()).collect();

    let missing: Vec<&String> = expected.difference(&found).collect();
    let unexpected: Vec<&String> = found.difference(&expected).collect();
    assert!(
        missing.is_empty(),
        "sections without a document: {missing:?}"
    );
    assert!(
        unexpected.is_empty(),
        "documents for sections that should not have one: {unexpected:?}"
    );
    assert_eq!(
        docs.len(),
        SECTIONS.len(),
        "expected one document per section, found {}",
        docs.len()
    );
}

#[test]
fn every_document_is_marked_normative() {
    for path in documents() {
        let text = std::fs::read_to_string(&path).expect("document is readable");
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        assert!(text.starts_with("# "), "{name}: does not open with a title");
        assert!(
            text.contains("**Normative.**"),
            "{name}: is not marked normative, so nothing stops a silent edit"
        );
        assert!(
            text.contains("specification change request"),
            "{name}: does not say how it may be changed"
        );
        assert!(
            text.contains("is not committed"),
            "{name}: does not record that the plan it mirrors is outside the repository"
        );
    }
}

#[test]
fn every_document_names_the_section_it_mirrors() {
    for path in documents() {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let section = name.split('-').next().unwrap_or_default();
        let text = std::fs::read_to_string(&path).expect("document is readable");
        assert!(
            text.contains(&format!("section {section} of the implementation plan")),
            "{name}: does not say which section it mirrors"
        );
    }
}

#[test]
fn no_document_is_a_stub() {
    // A specification mirror that lost its detail is worse than none: it reads as
    // authoritative while sending the implementer to the wrong place.
    for path in documents() {
        let text = std::fs::read_to_string(&path).expect("document is readable");
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let words = text.split_whitespace().count();
        assert!(
            words >= 200,
            "{name}: only {words} words, which cannot be a faithful mirror"
        );
    }
}
