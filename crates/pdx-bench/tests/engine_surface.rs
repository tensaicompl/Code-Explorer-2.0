//! A file's surface holds every fact the engine records for the file.
//!
//! Resolving a file from its cached surface must give the answers a fresh
//! extraction gives, which holds only while the surface carries every field the
//! resolver could read. The codec lists those fields by hand, so an engine refresh
//! that adds one would leave it silently incomplete. These tests read the engine's
//! structure definitions and the codec's tables and require them to agree, field
//! for field; the few fields a surface cannot carry are named below with the reason.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root resolves")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(repo_root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// Strips `//` and `/* */` comments.
fn without_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("/*") {
            rest = after.find("*/").map_or("", |i| &after[i + 2..]);
        } else if let Some(after) = rest.strip_prefix("//") {
            rest = after.find('\n').map_or("", |i| &after[i..]);
        } else {
            let c = rest.chars().next().expect("non-empty");
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// The member names of the structure whose typedef name is `name`.
fn struct_fields(header: &str, name: &str) -> BTreeSet<String> {
    let clean = without_comments(header);
    let end = clean
        .find(&format!("}} {name};"))
        .unwrap_or_else(|| panic!("no structure {name} in the engine header"));
    let start = clean[..end]
        .rfind("typedef struct")
        .expect("typedef precedes the body");
    let body = &clean[clean[start..end].find('{').expect("opening brace") + start + 1..end];
    body.split(';')
        .filter_map(|decl| {
            let decl = decl.trim();
            if decl.is_empty() {
                return None;
            }
            let decl = decl.split('[').next().expect("split yields one part");
            decl.rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .find(|s| !s.is_empty())
                .map(str::to_owned)
        })
        .collect()
}

/// The fields the codec encodes for `name`, from its FIELD and COUNTED entries.
fn codec_fields(codec: &str, name: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for macro_name in ["FIELD(", "COUNTED("] {
        for (i, _) in codec.match_indices(macro_name) {
            let args = &codec[i + macro_name.len()..];
            let args = &args[..args.find(')').expect("closing parenthesis")];
            let parts: Vec<&str> = args.split(',').map(str::trim).collect();
            if parts.first() == Some(&name) && parts.len() >= 3 {
                out.insert(parts[2].to_owned());
            }
        }
    }
    out
}

/// The result members the codec encodes as arrays, from its ARRAY entries.
fn codec_arrays(codec: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (i, _) in codec.match_indices("ARRAY(\"") {
        let args = &codec[i + "ARRAY(".len()..];
        let args = &args[..args.find(')').expect("closing parenthesis")];
        if let Some(member) = args.split(',').nth(1) {
            out.insert(member.trim().to_owned());
        }
    }
    out
}

const FACT_STRUCTURES: [&str; 16] = [
    "PDXEDefinition",
    "PDXERouteFact",
    "PDXECall",
    "PDXECallArg",
    "PDXEImport",
    "PDXEUsage",
    "PDXEThrow",
    "PDXEReadWrite",
    "PDXETypeRef",
    "PDXEEnvAccess",
    "PDXETypeAssign",
    "PDXEImplTrait",
    "PDXEResolvedCall",
    "PDXEStringRef",
    "PDXEInfraBinding",
    "PDXEChannel",
];

/// Members of the file result a surface does not carry, and why.
const NOT_IN_A_SURFACE: [(&str, &str); 6] = [
    (
        "arena",
        "the memory the result lives in; a rebuilt result has its own",
    ),
    (
        "cached_tree",
        "the parse tree; resolution parses the source again",
    ),
    (
        "source",
        "the retained source; resolution is handed the source",
    ),
    (
        "source_len",
        "the retained source; resolution is handed the source",
    ),
    (
        "owned_results",
        "parts of other files an aggregate result owns",
    ),
    (
        "owned_result_count",
        "parts of other files an aggregate result owns",
    ),
];

#[test]
fn every_field_of_every_fact_is_in_the_surface() {
    let header = read("engine/src/pdxe_core.h");
    let codec = without_comments(&read("engine/api/surface.c"));
    for name in FACT_STRUCTURES {
        let declared = struct_fields(&header, name);
        let encoded = codec_fields(&codec, name);
        assert!(
            !declared.is_empty(),
            "{name}: no fields found in the header"
        );
        let missing: Vec<_> = declared.difference(&encoded).collect();
        let unknown: Vec<_> = encoded.difference(&declared).collect();
        assert!(
            missing.is_empty() && unknown.is_empty(),
            "{name}: not encoded {missing:?}; encoded but not declared {unknown:?}"
        );
    }
}

#[test]
fn every_member_of_the_file_result_is_in_the_surface_or_excluded_for_a_reason() {
    let header = read("engine/src/pdxe_core.h");
    let codec = without_comments(&read("engine/api/surface.c"));
    let declared = struct_fields(&header, "PDXEFileResult");
    let mut covered = codec_fields(&codec, "PDXEFileResult");
    covered.extend(codec_arrays(&codec));
    covered.extend(NOT_IN_A_SURFACE.iter().map(|(m, _)| (*m).to_owned()));
    let missing: Vec<_> = declared.difference(&covered).collect();
    let unknown: Vec<_> = covered.difference(&declared).collect();
    assert!(
        missing.is_empty() && unknown.is_empty(),
        "file result: not in the surface {missing:?}; named but not declared {unknown:?}"
    );
}

#[test]
fn the_parser_reads_a_known_structure() {
    // The checks above are only as good as the parsing; pin it to a known answer.
    let header = read("engine/src/pdxe_core.h");
    let fields = struct_fields(&header, "PDXEImport");
    let expected: BTreeSet<String> = ["local_name", "module_path"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    assert_eq!(fields, expected);
}
