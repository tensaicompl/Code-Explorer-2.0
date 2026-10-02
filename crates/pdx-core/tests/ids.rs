//! Identities (specification 4.2.1 and its byte-level encoding).
//!
//! The fixed vectors were computed outside this crate, by a separate implementation
//! (Python's `hashlib`, and `base64.b32encode` with its alphabet mapped to Crockford's),
//! never by the functions under test. They lock the hash inputs, their order, the
//! separators, the alphabet, the bit order, the case and the truncation: a change to any
//! of them renames every stored node and must fail here first.

use pdx_core::ids::{
    self, AstFingerprint, EdgeId, IdError, NodeId, NodeKey, RepoId, SiteId, SiteKey,
};
use pdx_core::kinds::{EdgeKind, NodeKind, SiteKind};
use pdx_core::model::{Node, Site, Span};
use proptest::prelude::*;

const REPO: &str = "ce63551447285fd4";
const JAVA_FILE: &str = "src/main/Inventory.java";

fn repo() -> RepoId {
    RepoId::parse(REPO).expect("a registered form")
}

fn method_key(disambiguator: &str) -> NodeKey {
    NodeKey::definition(
        &repo(),
        NodeKind::Method,
        JAVA_FILE,
        "com.acme.Inventory.add",
        disambiguator,
    )
}

fn method_id() -> NodeId {
    method_key("").node_id().expect("an id")
}

fn fingerprint() -> AstFingerprint {
    ids::ast_fingerprint(
        &[
            "method_declaration",
            "block",
            "expression_statement",
            "method_invocation",
        ],
        "add",
        "items",
        1,
    )
    .expect("a fingerprint")
}

// --- repo_id ---------------------------------------------------------------------

/// A URL from its scheme and the rest. Assembled rather than written, as the scanner's
/// own tests do, so this file carries no address for the provenance scan to take for a
/// network dependency: these are fixtures, on hosts reserved for examples (RFC 2606),
/// apart from this repository's own, which the scan allows.
fn url(scheme: &str, rest: &str) -> String {
    format!("{scheme}://{rest}")
}

fn https(rest: &str) -> String {
    url("https", rest)
}

#[test]
fn clone_urls_canonicalise() {
    for (input, canonical) in [
        (
            "https://github.com/tensaicompl/Code-Explorer-2.0.git".to_owned(),
            "https://github.com/tensaicompl/Code-Explorer-2.0".to_owned(),
        ),
        (
            "https://github.com/tensaicompl/Code-Explorer-2.0".to_owned(),
            "https://github.com/tensaicompl/Code-Explorer-2.0".to_owned(),
        ),
        // User information goes and the host is lowercased; the path keeps its case.
        (
            https("x-access-token:s3cr3t@Git.Example/Org/Repo.git"),
            https("git.example/Org/Repo"),
        ),
        (
            https("user@git.example/Org/Repo"),
            https("git.example/Org/Repo"),
        ),
        // The scheme in any case; a port as written.
        (
            url("HTTPS", "Git.Example:8443/Group/Sub/Project"),
            https("git.example:8443/Group/Sub/Project"),
        ),
        // The path is kept as written: a trailing slash, percent-encoding.
        (
            https("git.example/Org/Repo/"),
            https("git.example/Org/Repo/"),
        ),
        (
            https("git.example/Org/My%20Repo.git"),
            https("git.example/Org/My%20Repo"),
        ),
        // Only one trailing `.git` goes, and only at the end.
        (
            https("git.example/org/repo.git.git"),
            https("git.example/org/repo.git"),
        ),
        (
            https("git.example/org/repo.github"),
            https("git.example/org/repo.github"),
        ),
    ] {
        assert_eq!(ids::canonical_clone_url(&input), Ok(canonical), "{input}");
    }
}

#[test]
fn clone_urls_outside_the_specification_are_refused() {
    for input in [
        "git@git.example:org/repo.git".to_owned(),
        url("ssh", "git@git.example/org/repo.git"),
        url("http", "git.example/org/repo.git"),
        url("git", "git.example/org/repo.git"),
        url("file", "/srv/git/repo.git"),
        https(""),
        https("/org/repo"),
        https("user@/org/repo"),
        https(":8443/org/repo"),
        https("git.example"),
        https("git.example/"),
        https("git.example/org/repo?ref=main"),
        https("git.example/org/repo#readme"),
        https("git.example/org/re\0po"),
        https("git.example/org/re po"),
        "git.example/org/repo".to_owned(),
        String::new(),
    ] {
        assert!(
            ids::canonical_clone_url(&input).is_err(),
            "{input:?} accepted"
        );
        assert!(ids::repo_id_from_url(&input).is_err(), "{input:?} accepted");
    }
}

#[test]
fn repo_id_fixed_vectors() {
    for (input, id) in [
        (
            "https://github.com/tensaicompl/Code-Explorer-2.0.git".to_owned(),
            "ce63551447285fd4",
        ),
        (
            https("x-access-token:s3cr3t@Git.Example/Org/Repo.git"),
            "2b2f33ba06c16e77",
        ),
        (
            url("HTTPS", "Git.Example:8443/Group/Sub/Project"),
            "fe6438f0e0cb550c",
        ),
    ] {
        assert_eq!(
            ids::repo_id_from_url(&input).map(|r| r.to_string()),
            Ok(id.to_owned()),
            "{input}"
        );
    }
    // A URL-derived id has the registered form: either can be parsed as the other.
    assert_eq!(
        RepoId::parse("ce63551447285fd4").map(|r| r.to_string()),
        Ok(REPO.to_owned())
    );
    for bad in [
        "CE63551447285FD4",
        "ce6355144728",
        "ce63551447285fd4a",
        "ce63551447285fdg",
        "estate",
        "",
    ] {
        assert!(RepoId::parse(bad).is_err(), "{bad:?}");
    }
}

// --- node_id ---------------------------------------------------------------------

#[test]
fn node_id_fixed_vectors() {
    let repo = repo();
    let cases = [
        (NodeKey::repo(&repo), "NP5899JPZZTZSSAGDXKHNYR17C"),
        (
            NodeKey::folder(&repo, "src/main"),
            "JA0W7H8DFATYPNGZS1754SD28P",
        ),
        (
            NodeKey::file(&repo, JAVA_FILE),
            "2M70XN97JJTJTTYPJ2PTZ4GJHP",
        ),
        (method_key(""), "FC0C2RXS73G18HY3HRG3C0EX2Z"),
        (method_key("0517abec"), "5YPXN04A1YH5MRHA8QENTERA8P"),
        (
            NodeKey::estate(NodeKind::ApiContract, "GET /orders/{id}").expect("an estate kind"),
            "FMXRAER9WFVM1JB93F13E9RTFT",
        ),
    ];
    for (key, id) in cases {
        assert_eq!(
            key.node_id().map(|n| n.to_string()),
            Ok(id.to_owned()),
            "{key:?}"
        );
    }
    // The primitive over the fields 4.2.1 names gives the same answers.
    assert_eq!(
        ids::node_id(
            REPO,
            NodeKind::Method,
            JAVA_FILE,
            "com.acme.Inventory.add",
            ""
        )
        .map(|n| n.to_string()),
        Ok("FC0C2RXS73G18HY3HRG3C0EX2Z".to_owned())
    );
}

#[test]
fn special_identity_inputs() {
    let repo = repo();
    let repo_key = NodeKey::repo(&repo);
    assert_eq!(
        (
            repo_key.repo.as_str(),
            repo_key.path.as_str(),
            repo_key.qualified_name.as_str()
        ),
        (REPO, "", "")
    );
    for key in [
        NodeKey::folder(&repo, "src/main"),
        NodeKey::file(&repo, JAVA_FILE),
    ] {
        assert_eq!(key.qualified_name, key.path, "{key:?}");
    }
    let estate = NodeKey::estate(NodeKind::Service, "orders").expect("an architecture kind");
    assert_eq!(
        (
            estate.repo.as_str(),
            estate.path.as_str(),
            estate.qualified_name.as_str()
        ),
        ("estate", "", "orders")
    );
    // Estate identity is for estate-level kinds only.
    for kind in [
        NodeKind::Repo,
        NodeKind::File,
        NodeKind::Method,
        NodeKind::Route,
    ] {
        assert_eq!(
            NodeKey::estate(kind, "x"),
            Err(IdError::NotEstateLevel(kind)),
            "{kind}"
        );
    }
}

#[test]
fn identity_inputs_refuse_nul() {
    assert!(matches!(
        ids::node_id(REPO, NodeKind::Method, "a\0b", "q", ""),
        Err(IdError::Nul(_))
    ));
    assert!(matches!(
        ids::node_id(REPO, NodeKind::Method, "a", "q\0", ""),
        Err(IdError::Nul(_))
    ));
    assert!(matches!(
        ids::node_id(REPO, NodeKind::Method, "a", "q", "\0"),
        Err(IdError::Nul(_))
    ));
    assert!(matches!(
        ids::node_id("ce6\0x", NodeKind::Method, "a", "q", ""),
        Err(IdError::Nul(_))
    ));
    assert!(matches!(
        ids::ast_fingerprint(&["call\0"], "f", "", 1),
        Err(IdError::Nul(_))
    ));
    assert!(matches!(
        ids::ast_fingerprint(&["call"], "f\0", "", 1),
        Err(IdError::Nul(_))
    ));
    assert!(matches!(
        ids::ast_fingerprint(&["call"], "f", "r\0", 1),
        Err(IdError::Nul(_))
    ));
    assert!(matches!(
        ids::signature_disambiguator("int\0"),
        Err(IdError::Nul(_))
    ));
    let site = SiteKey::new("a\0.py", None, SiteKind::Call, fingerprint());
    assert!(matches!(site.site_id(), Err(IdError::Nul(_))));
}

#[test]
fn ids_have_their_length_and_alphabet() {
    const CROCKFORD: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let ids = [
        method_id().to_string(),
        SiteKey::new(JAVA_FILE, Some(method_id()), SiteKind::Call, fingerprint())
            .site_id()
            .expect("an id")
            .to_string(),
        ids::edge_id(&method_id(), &method_id(), EdgeKind::Calls, None).to_string(),
    ];
    for id in &ids {
        assert_eq!(id.len(), 26, "{id}");
        assert!(id.chars().all(|c| CROCKFORD.contains(c)), "{id}");
    }
    for bad in [
        "FC0C2RXS73G18HY3HRG3C0EX2",
        "FC0C2RXS73G18HY3HRG3C0EX2ZZ",
        "fc0c2rxs73g18hy3hrg3c0ex2z",
        "FC0C2RXS73G18HY3HRG3C0EX2U",
        "FC0C2RXS73G18HY3HRG3C0EXIL",
    ] {
        assert!(NodeId::parse(bad).is_err(), "{bad}");
        assert!(SiteId::parse(bad).is_err(), "{bad}");
        assert!(EdgeId::parse(bad).is_err(), "{bad}");
    }
    assert!(NodeId::parse("FC0C2RXS73G18HY3HRG3C0EX2Z").is_ok());
}

proptest! {
    /// A node's identity is its key, and lines are no part of it: a node moved
    /// anywhere in its file keeps its id.
    #[test]
    fn node_id_ignores_lines(
        start in proptest::option::of(0u32..1_000_000),
        len in proptest::option::of(0u32..10_000),
        moved_start in proptest::option::of(0u32..1_000_000),
        moved_len in proptest::option::of(0u32..10_000),
        qn in "[a-zA-Z_][a-zA-Z0-9_.]{0,40}",
    ) {
        let key = NodeKey::definition(&repo(), NodeKind::Function, "src/lib.rs", &qn, "");
        let here = Node::new(&key, "f")
            .expect("a node")
            .with_lines(start, start.zip(len).map(|(s, l)| s + l));
        let there = Node::new(&key, "f")
            .expect("a node")
            .with_lines(moved_start, moved_start.zip(moved_len).map(|(s, l)| s + l));
        prop_assert_eq!((here.start_line, there.start_line), (start, moved_start));
        prop_assert_eq!(&here.node_id, &there.node_id);
        prop_assert_eq!(&here.node_id, &key.node_id().expect("an id"));
    }
}

// --- overloads -------------------------------------------------------------------

#[test]
fn disambiguator_fixed_vectors() {
    assert_eq!(
        ids::signature_disambiguator("String,int").as_deref(),
        Ok("0517abec")
    );
    assert_eq!(
        ids::signature_disambiguator("Item").as_deref(),
        Ok("652bcc3a")
    );
    assert_eq!(ids::signature_disambiguator("").as_deref(), Ok("e3b0c442"));
    // Alone, a callable needs no disambiguator.
    assert_eq!(
        ids::overload_disambiguators(&["String,int"]),
        Ok(vec![String::new()])
    );
    assert_eq!(
        ids::overload_disambiguators(&["String,int", "Item"]),
        Ok(vec!["0517abec".to_owned(), "652bcc3a".to_owned()])
    );
}

#[test]
fn identical_signatures_take_their_file_order() {
    // Identical normalised signatures, possible in dynamically typed languages, are
    // told apart by their order among the identical ones, counted from 1; a distinct
    // signature in the same group is unaffected.
    let group = ["", "Item", "", ""];
    assert_eq!(
        ids::overload_disambiguators(&group),
        Ok(vec![
            "e3b0c442-1".to_owned(),
            "652bcc3a".to_owned(),
            "e3b0c442-2".to_owned(),
            "e3b0c442-3".to_owned()
        ])
    );
    // A distinct overload inserted among them renumbers none of them.
    assert_eq!(
        ids::overload_disambiguators(&["", "String,int", "Item", "", ""]),
        Ok(vec![
            "e3b0c442-1".to_owned(),
            "0517abec".to_owned(),
            "652bcc3a".to_owned(),
            "e3b0c442-2".to_owned(),
            "e3b0c442-3".to_owned(),
        ])
    );
    assert_eq!(ids::overload_disambiguators(&[]), Ok(vec![]));
}

proptest! {
    /// Overloads with distinct signatures keep their ids, in whatever order the file
    /// declares them, when another distinct overload is inserted anywhere among them.
    #[test]
    fn overload_insert_does_not_renumber(
        (declared, inserted) in proptest::collection::btree_set("[A-Za-z][A-Za-z0-9,<>\\[\\]]{0,24}", 3..12)
            .prop_flat_map(|signatures| {
                // The new overload is one of the generated signatures, held back; the
                // rest are declared in an arbitrary order, not a sorted one.
                let mut all: Vec<String> = signatures.into_iter().collect();
                let inserted = all.pop().expect("at least three");
                (Just(all).prop_shuffle(), Just(inserted))
            }),
        at in 0usize..16,
    ) {
        let n = declared.len();
        let ids_of = |sigs: &[String]| -> Vec<(String, NodeId)> {
            let refs: Vec<&str> = sigs.iter().map(String::as_str).collect();
            let disambiguators = ids::overload_disambiguators(&refs).expect("disambiguators");
            sigs.iter().zip(disambiguators).map(|(sig, d)| (sig.clone(), method_key(&d).node_id().expect("an id"))).collect()
        };
        let before = ids_of(&declared);
        let mut after_sigs = declared.clone();
        after_sigs.insert(at.min(n), inserted.clone());
        let after = ids_of(&after_sigs);
        for (sig, id) in &before {
            let now = after.iter().find(|(s, _)| s == sig).map(|(_, id)| id);
            prop_assert_eq!(now, Some(id), "{} was renumbered", sig);
        }
        let new_id = after.iter().find(|(s, _)| *s == inserted).map(|(_, id)| id.clone()).expect("inserted");
        prop_assert!(before.iter().all(|(_, id)| *id != new_id));
    }
}

// --- sites -----------------------------------------------------------------------

#[test]
fn ast_fingerprint_fixed_vector() {
    assert_eq!(
        fingerprint().as_str(),
        "b72a1edb5ebc15504aff07917b0760bc15da4cdc57e725700662a10a2d9e755f"
    );
    // The ordinal counts from 1; a path is never empty, and neither is a node type.
    assert_eq!(
        ids::ast_fingerprint(&["call"], "f", "", 0),
        Err(IdError::ZeroOrdinal)
    );
    assert_eq!(
        ids::ast_fingerprint(&[], "f", "", 1),
        Err(IdError::EmptyAstPath)
    );
    assert_eq!(
        ids::ast_fingerprint(&["call", ""], "f", "", 1),
        Err(IdError::EmptyAstPath)
    );
    // Where the path ends is part of the fingerprint: a type moved into the text is a
    // different site.
    assert_ne!(
        ids::ast_fingerprint(&["a", "b"], "c", "", 1),
        ids::ast_fingerprint(&["a"], "b", "c", 1)
    );
}

#[test]
fn site_id_fixed_vectors() {
    let enclosed = SiteKey::new(JAVA_FILE, Some(method_id()), SiteKind::Call, fingerprint());
    assert_eq!(
        enclosed.site_id().map(|s| s.to_string()),
        Ok("8Q0S4A5X7HM4PN0YV3FCEAFV2V".to_owned())
    );
    let top_level = SiteKey::new(JAVA_FILE, None, SiteKind::Import, fingerprint());
    assert_eq!(
        top_level.site_id().map(|s| s.to_string()),
        Ok("VPJH78E48GY402DQW6XET6F6XM".to_owned())
    );
}

proptest! {
    /// A site's identity is its key; where it sits in the file is not.
    #[test]
    fn site_id_ignores_lines(
        start_byte in 0u32..5_000_000,
        len in 0u32..5_000,
        start_line in 1u32..100_000,
        start_col in 0u32..500,
        moved_byte in 0u32..5_000_000,
        moved_line in 1u32..100_000,
        moved_col in 0u32..500,
    ) {
        let key = SiteKey::new(JAVA_FILE, Some(method_id()), SiteKind::Call, fingerprint());
        let span = |byte: u32, line: u32, col: u32| Span {
            start_byte: byte, end_byte: byte + len, start_line: line, start_col: col, end_line: line, end_col: col + len,
        };
        let file = NodeKey::file(&repo(), JAVA_FILE).node_id().expect("an id");
        let here = Site::new(&key, file.clone(), span(start_byte, start_line, start_col)).expect("a site");
        let there = Site::new(&key, file, span(moved_byte, moved_line, moved_col)).expect("a site");
        prop_assert_eq!(&here.site_id, &there.site_id);
        prop_assert_eq!(&here.site_id, &key.site_id().expect("an id"));
    }

    /// The same call, reached by another path through the syntax tree, is another
    /// site: the path changes the fingerprint and the fingerprint the id.
    #[test]
    fn site_id_changes_with_ast_path(
        path in proptest::collection::vec("[a-z_]{1,24}", 1..8),
        other in proptest::collection::vec("[a-z_]{1,24}", 1..8),
    ) {
        prop_assume!(path != other);
        let at = |types: &[String]| {
            let types: Vec<&str> = types.iter().map(String::as_str).collect();
            let fingerprint = ids::ast_fingerprint(&types, "add", "items", 1).expect("a fingerprint");
            (fingerprint.clone(), SiteKey::new(JAVA_FILE, Some(method_id()), SiteKind::Call, fingerprint).site_id().expect("an id"))
        };
        let (fingerprint, id) = at(&path);
        let (other_fingerprint, other_id) = at(&other);
        prop_assert_ne!(fingerprint, other_fingerprint);
        prop_assert_ne!(id, other_id);
    }
}

// --- edges -----------------------------------------------------------------------

#[test]
fn edge_id_fixed_vectors() {
    let src = method_id();
    let dst = NodeKey::definition(
        &repo(),
        NodeKind::Method,
        "src/main/Store.java",
        "com.acme.Store.save",
        "",
    )
    .node_id()
    .expect("an id");
    assert_eq!(dst.as_str(), "639NYMS5S3YSVEMV5N1G4T7HEQ");
    let site = SiteKey::new(JAVA_FILE, Some(src.clone()), SiteKind::Call, fingerprint())
        .site_id()
        .expect("an id");
    assert_eq!(
        ids::edge_id(&src, &dst, EdgeKind::Calls, Some(&site)).as_str(),
        "04K6NBGNS5YV1FH28A5J5KK36A"
    );
    assert_eq!(
        ids::edge_id(&src, &dst, EdgeKind::ChangesWith, None).as_str(),
        "DWNCJWMCV866WG7RJNMDFCQRNP"
    );
    // Direction is identity: the reverse edge is another edge.
    assert_ne!(
        ids::edge_id(&dst, &src, EdgeKind::Calls, Some(&site)),
        ids::edge_id(&src, &dst, EdgeKind::Calls, Some(&site))
    );
}

#[test]
fn id_types_serialise_as_their_strings() {
    let id = method_id();
    assert_eq!(
        serde_json::to_string(&id).expect("json"),
        "\"FC0C2RXS73G18HY3HRG3C0EX2Z\""
    );
    assert_eq!(
        serde_json::from_str::<NodeId>("\"FC0C2RXS73G18HY3HRG3C0EX2Z\"").expect("json"),
        id
    );
    assert!(serde_json::from_str::<NodeId>("\"not an id\"").is_err());
    assert_eq!(
        serde_json::to_string(&repo()).expect("json"),
        "\"ce63551447285fd4\""
    );
    assert!(serde_json::from_str::<RepoId>("\"estate\"").is_err());
    assert_eq!(
        serde_json::to_string(&fingerprint()).expect("json"),
        "\"b72a1edb5ebc15504aff07917b0760bc15da4cdc57e725700662a10a2d9e755f\""
    );
    let site = SiteKey::new(JAVA_FILE, Some(id.clone()), SiteKind::Call, fingerprint())
        .site_id()
        .expect("an id");
    assert_eq!(
        serde_json::to_string(&site).expect("json"),
        "\"8Q0S4A5X7HM4PN0YV3FCEAFV2V\""
    );
    let edge = ids::edge_id(&id, &id, EdgeKind::Calls, Some(&site));
    assert_eq!(
        serde_json::from_str::<EdgeId>(&serde_json::to_string(&edge).expect("json")).expect("json"),
        edge
    );
    assert_eq!(format!("{id}"), "FC0C2RXS73G18HY3HRG3C0EX2Z");
}

#[test]
fn the_specification_carries_these_vectors() {
    // 4.2.1 prints reference vectors so that a second implementation can check itself
    // against the specification alone. Every one of them is one this file checks the
    // code against, so the two cannot drift apart.
    let spec = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/spec/4.2-identity-and-bands.md"
    ))
    .expect("the identity specification");
    let vectors = spec
        .split("Reference vectors")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .expect("4.2.1's reference vectors");
    for value in [
        "ce63551447285fd4",
        "2b2f33ba06c16e77",
        "fe6438f0e0cb550c",
        "NP5899JPZZTZSSAGDXKHNYR17C",
        "2M70XN97JJTJTTYPJ2PTZ4GJHP",
        "FC0C2RXS73G18HY3HRG3C0EX2Z",
        "5YPXN04A1YH5MRHA8QENTERA8P",
        "FMXRAER9WFVM1JB93F13E9RTFT",
        "0517abec",
        "652bcc3a",
        "e3b0c442",
        "b72a1edb5ebc15504aff07917b0760bc15da4cdc57e725700662a10a2d9e755f",
        "8Q0S4A5X7HM4PN0YV3FCEAFV2V",
        "VPJH78E48GY402DQW6XET6F6XM",
        "639NYMS5S3YSVEMV5N1G4T7HEQ",
        "04K6NBGNS5YV1FH28A5J5KK36A",
        "DWNCJWMCV866WG7RJNMDFCQRNP",
    ] {
        assert!(
            vectors.contains(&format!("`{value}`")),
            "4.2.1 does not print {value}"
        );
    }
    let printed = vectors.matches("\n| ").count();
    assert_eq!(printed, 15, "4.2.1 prints a vector this test does not know");
}
