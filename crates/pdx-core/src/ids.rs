//! Identities of repositories, nodes, evidence sites and edges (specification 4.2.1 and
//! its byte-level encoding).
//!
//! Every identity is a hash of the fields 4.2.1 names and nothing else: no line,
//! column or byte offset, no position in a file and no order of insertion, so an id
//! survives everything but a change to what it identifies. Ids are stored, so these
//! encodings are a stored format: fixed vectors in `tests/ids.rs` hold them.
//!
//! Strings are hashed as their UTF-8 bytes. The formulas separate fields with a NUL
//! byte, so no field may contain one: every function refuses a NUL rather than hash a
//! sequence that two different inputs could produce.

use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::kinds::{EdgeKind, NodeKind, SiteKind};

/// Why an identity could not be computed or parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdError {
    /// The named input contains a NUL byte.
    Nul(&'static str),
    /// The URL is not one 4.2.1 accepts as an HTTPS clone URL.
    NotAnHttpsCloneUrl(String),
    /// Estate identity was asked for a kind that belongs to one repository.
    NotEstateLevel(NodeKind),
    /// An identity's node-type path is empty, or one of its types is.
    EmptyAstPath,
    /// An ordinal of zero; ordinals count from 1.
    ZeroOrdinal,
    /// The string is not an identity of the named kind.
    Malformed(&'static str, String),
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nul(field) => write!(f, "{field} contains a NUL byte"),
            Self::NotAnHttpsCloneUrl(url) => write!(f, "{url:?} is not an HTTPS clone URL"),
            Self::NotEstateLevel(kind) => write!(f, "{kind} is not an estate-level kind"),
            Self::EmptyAstPath => f.write_str("a node-type path or one of its types is empty"),
            Self::ZeroOrdinal => f.write_str("ordinals count from 1"),
            Self::Malformed(what, text) => write!(f, "{text:?} is not a {what}"),
        }
    }
}

impl std::error::Error for IdError {}

/// Crockford's base32 alphabet, in the upper case its encoding writes.
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Length of a node, site or edge id: 26 symbols, the hash's first 130 bits.
pub const ID_LEN: usize = 26;

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

/// `parts` joined by NUL bytes and hashed.
fn sha256_nul_separated(parts: &[&str]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            hasher.update([0u8]);
        }
        hasher.update(part.as_bytes());
    }
    hasher.finalize().into()
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 0x0f)]));
    }
    out
}

/// The digest's first [`ID_LEN`] symbols in Crockford's base32: the digest read as bits,
/// most significant first, five bits to a symbol.
fn crockford_id(digest: &[u8; 32]) -> String {
    let mut out = String::with_capacity(ID_LEN);
    let mut buffer: u32 = 0;
    let mut bits = 0u32;
    for byte in digest {
        buffer = (buffer << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 && out.len() < ID_LEN {
            bits -= 5;
            out.push(char::from(CROCKFORD[((buffer >> bits) & 0x1f) as usize]));
        }
        if out.len() == ID_LEN {
            break;
        }
        buffer &= (1 << bits) - 1;
    }
    out
}

fn no_nul(field: &'static str, value: &str) -> Result<(), IdError> {
    if value.contains('\0') {
        Err(IdError::Nul(field))
    } else {
        Ok(())
    }
}

fn is_crockford_id(text: &str) -> bool {
    text.len() == ID_LEN && text.bytes().all(|b| CROCKFORD.contains(&b))
}

macro_rules! crockford_id_type {
    ($(#[$meta:meta])* $name:ident, $what:literal) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// The id written `text`, which must be one: 26 symbols of Crockford's
            /// alphabet, in upper case.
            ///
            /// # Errors
            ///
            /// [`IdError::Malformed`] for anything else.
            pub fn parse(text: &str) -> Result<Self, IdError> {
                if is_crockford_id(text) {
                    Ok(Self(text.to_owned()))
                } else {
                    Err(IdError::Malformed($what, text.to_owned()))
                }
            }

            /// The id as its 26 symbols.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdError;

            fn try_from(text: String) -> Result<Self, IdError> {
                if is_crockford_id(&text) {
                    Ok(Self(text))
                } else {
                    Err(IdError::Malformed($what, text))
                }
            }
        }

        impl From<$name> for String {
            fn from(id: $name) -> String {
                id.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

crockford_id_type!(
    /// A node's identity (4.2.1).
    NodeId,
    "node id"
);
crockford_id_type!(
    /// An evidence site's identity (4.2.1).
    SiteId,
    "site id"
);
crockford_id_type!(
    /// An edge's identity (4.2.1).
    EdgeId,
    "edge id"
);

// --- repositories ------------------------------------------------------------------

/// A repository's identity: 16 lower-case hex characters.
///
/// **A repository's identity is the one it was registered with, not its URL.** A
/// server assigns it once, at random, and it never changes when the repository moves;
/// the local binary uses the server's whenever one is configured. Only a binary with
/// no server derives one from the clone URL ([`repo_id_from_url`]), and it records
/// that it did. Never derive an id for a repository that has a registered one.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RepoId(String);

impl RepoId {
    /// The registered identity written `text`: exactly 16 lower-case hex characters.
    ///
    /// # Errors
    ///
    /// [`IdError::Malformed`] for anything else, `"estate"` included: the estate is not
    /// a repository.
    pub fn parse(text: &str) -> Result<Self, IdError> {
        Self::try_from(text.to_owned())
    }

    /// The identity as its 16 characters.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RepoId {
    type Error = IdError;

    fn try_from(text: String) -> Result<Self, IdError> {
        if text.len() == 16 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            Ok(Self(text))
        } else {
            Err(IdError::Malformed("repo id", text))
        }
    }
}

impl From<RepoId> for String {
    fn from(id: RepoId) -> String {
        id.0
    }
}

impl fmt::Display for RepoId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The canonical form of an HTTPS clone URL (4.2.1): no user information, no trailing
/// `.git`, the host in lower case, and the path exactly as written.
///
/// The scheme is matched in any case and written `https://`; a port is kept as
/// written.
///
/// # Errors
///
/// [`IdError::NotAnHttpsCloneUrl`] for anything 4.2.1 does not describe: another scheme
/// (`http`, `ssh`, `git@host:path`), no host, no path, a query, a fragment, or a
/// space, control character or NUL anywhere.
pub fn canonical_clone_url(url: &str) -> Result<String, IdError> {
    let refuse = || IdError::NotAnHttpsCloneUrl(url.to_owned());
    if url.bytes().any(|b| b.is_ascii_control() || b == b' ') || url.contains(['?', '#']) {
        return Err(refuse());
    }
    let scheme_end = url.find("://").ok_or_else(refuse)?;
    if !url[..scheme_end].eq_ignore_ascii_case("https") {
        return Err(refuse());
    }
    let rest = &url[scheme_end + 3..];
    let path_start = rest.find('/').ok_or_else(refuse)?;
    let (authority, path) = rest.split_at(path_start);
    let host_and_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let path = path.strip_suffix(".git").unwrap_or(path);
    if host_and_port.is_empty() || host_and_port.starts_with(':') || path.len() <= 1 {
        return Err(refuse());
    }
    Ok(format!(
        "https://{}{path}",
        host_and_port.to_ascii_lowercase()
    ))
}

/// The identity of a repository with no registered one, derived from its clone URL:
/// `lowerhex(sha256(canonical_clone_url))[0..16]`.
///
/// **A fallback, for a local binary with no server only.** See [`RepoId`]: a
/// repository with a registered identity uses that, and an id derived here is
/// recorded as such (`repo_id_source = "url"`).
///
/// # Errors
///
/// [`IdError::NotAnHttpsCloneUrl`], from [`canonical_clone_url`].
pub fn repo_id_from_url(url: &str) -> Result<RepoId, IdError> {
    let canonical = canonical_clone_url(url)?;
    let digest = sha256(&[canonical.as_bytes()]);
    Ok(RepoId(lower_hex(&digest[..8])))
}

// --- nodes -------------------------------------------------------------------------

/// The repository field of estate-level identities (4.2.1).
pub const ESTATE: &str = "estate";

/// The identity of a node from the fields 4.2.1 names:
/// `base32-crockford(sha256(repo_id || 0x00 || kind || 0x00 || path || 0x00 || qualified_name || 0x00 || disambiguator))[0..26]`.
///
/// The fields are taken as given; [`NodeKey`]'s constructors supply the values 4.2.1
/// prescribes for repositories, folders, files and estate-level nodes.
///
/// # Errors
///
/// [`IdError::Nul`] if any field contains a NUL byte.
pub fn node_id(
    repo_id: &str,
    kind: NodeKind,
    path: &str,
    qualified_name: &str,
    disambiguator: &str,
) -> Result<NodeId, IdError> {
    no_nul("repo_id", repo_id)?;
    no_nul("path", path)?;
    no_nul("qualified_name", qualified_name)?;
    no_nul("disambiguator", disambiguator)?;
    let digest =
        sha256_nul_separated(&[repo_id, kind.as_str(), path, qualified_name, disambiguator]);
    Ok(NodeId(crockford_id(&digest)))
}

/// Everything a node's identity is computed from, and nothing else: no line, no
/// position, no order.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodeKey {
    /// The repository's id, or [`ESTATE`] for an estate-level node.
    pub repo: String,
    /// The node's kind.
    pub kind: NodeKind,
    /// The repository-relative POSIX path of the defining file; `""` for a repository
    /// or an estate-level node.
    pub path: String,
    /// The language's fully qualified name (Appendix B.1), `""` for a repository, the
    /// path for a folder or file, the `namespace_key` for an estate-level node.
    pub qualified_name: String,
    /// `""`, or for an overloaded callable its entry from [`overload_disambiguators`].
    pub disambiguator: String,
}

impl NodeKey {
    /// The repository's own node: path and qualified name empty.
    pub fn repo(repo: &RepoId) -> Self {
        Self::new(repo.as_str(), NodeKind::Repo, "", "", "")
    }

    /// A folder: its qualified name is its path.
    pub fn folder(repo: &RepoId, path: &str) -> Self {
        Self::new(repo.as_str(), NodeKind::Folder, path, path, "")
    }

    /// A file: its qualified name is its path.
    pub fn file(repo: &RepoId, path: &str) -> Self {
        Self::new(repo.as_str(), NodeKind::File, path, path, "")
    }

    /// A definition in a file: anything with a qualified name of its own.
    pub fn definition(
        repo: &RepoId,
        kind: NodeKind,
        path: &str,
        qualified_name: &str,
        disambiguator: &str,
    ) -> Self {
        Self::new(repo.as_str(), kind, path, qualified_name, disambiguator)
    }

    /// An estate-level node (a contract, artifact, system or context): repository
    /// `"estate"`, no path, and the contract's `namespace_key` or the architecture
    /// file's id as its qualified name, so the same contract has the same id in every
    /// segment that mentions it.
    ///
    /// # Errors
    ///
    /// [`IdError::NotEstateLevel`] for a kind that belongs to one repository.
    pub fn estate(kind: NodeKind, key: &str) -> Result<Self, IdError> {
        if kind.is_estate_level() {
            Ok(Self::new(ESTATE, kind, "", key, ""))
        } else {
            Err(IdError::NotEstateLevel(kind))
        }
    }

    fn new(
        repo: &str,
        kind: NodeKind,
        path: &str,
        qualified_name: &str,
        disambiguator: &str,
    ) -> Self {
        Self {
            repo: repo.to_owned(),
            kind,
            path: path.to_owned(),
            qualified_name: qualified_name.to_owned(),
            disambiguator: disambiguator.to_owned(),
        }
    }

    /// The node's id.
    ///
    /// # Errors
    ///
    /// [`IdError::Nul`] if a field contains a NUL byte.
    pub fn node_id(&self) -> Result<NodeId, IdError> {
        node_id(
            &self.repo,
            self.kind,
            &self.path,
            &self.qualified_name,
            &self.disambiguator,
        )
    }
}

/// The disambiguator of one overload: `lowerhex(sha256(normalised_signature))[0..8]`.
///
/// The signature must already be normalised (4.2.1, Appendix B.1): the parameter types
/// as declared, whitespace removed, generic arguments erased, default aliases expanded.
///
/// # Errors
///
/// [`IdError::Nul`] if the signature contains a NUL byte.
pub fn signature_disambiguator(normalised_signature: &str) -> Result<String, IdError> {
    no_nul("normalised_signature", normalised_signature)?;
    let digest = sha256(&[normalised_signature.as_bytes()]);
    Ok(lower_hex(&digest[..4]))
}

/// The disambiguators of a group of callables that share a kind, a path and a
/// qualified name, given their normalised signatures in file order, returned in the
/// same order.
///
/// A callable alone needs none (`""`). Overloads are told apart by their signatures'
/// hashes, so inserting one never renumbers the others. Only overloads with identical
/// normalised signatures, which dynamically typed languages allow, are numbered: each
/// gets `-k` appended, `k` counting from 1 in file order among those identical ones.
///
/// # Errors
///
/// [`IdError::Nul`] if a signature contains a NUL byte.
pub fn overload_disambiguators(signatures_in_file_order: &[&str]) -> Result<Vec<String>, IdError> {
    if signatures_in_file_order.len() < 2 {
        for signature in signatures_in_file_order {
            no_nul("normalised_signature", signature)?;
        }
        return Ok(vec![String::new(); signatures_in_file_order.len()]);
    }
    let mut out = Vec::with_capacity(signatures_in_file_order.len());
    for (i, signature) in signatures_in_file_order.iter().enumerate() {
        let hash = signature_disambiguator(signature)?;
        let identical = signatures_in_file_order
            .iter()
            .filter(|s| *s == signature)
            .count();
        if identical > 1 {
            let ordinal = 1 + signatures_in_file_order[..i]
                .iter()
                .filter(|s| *s == signature)
                .count();
            out.push(format!("{hash}-{ordinal}"));
        } else {
            out.push(hash);
        }
    }
    Ok(out)
}

// --- sites -------------------------------------------------------------------------

/// Where a site sits in its definition's syntax tree, independently of lines and
/// offsets: 64 lower-case hex characters (4.2.1).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AstFingerprint(String);

impl AstFingerprint {
    /// The fingerprint as its 64 characters, the form it is stored and hashed in.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for AstFingerprint {
    type Error = IdError;

    fn try_from(text: String) -> Result<Self, IdError> {
        if text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            Ok(Self(text))
        } else {
            Err(IdError::Malformed("ast fingerprint", text))
        }
    }
}

impl From<AstFingerprint> for String {
    fn from(fingerprint: AstFingerprint) -> String {
        fingerprint.0
    }
}

impl fmt::Display for AstFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A site's fingerprint:
/// `lowerhex(sha256(n || 0x00 || type_1 || 0x00 || … || type_n || 0x00 || callee_text || 0x00 || receiver_text || 0x00 || ordinal))`.
///
/// `node_type_path` is the engine's node types from the enclosing definition down to
/// the site; `callee_text` the site's text (the callee for a call) and `receiver_text`
/// its receiver, `""` for none; `ordinal` the position, counting from 1, of this
/// (path, texts) pair among the definition's sites with the same path and texts. `n`
/// and `ordinal` are written in decimal.
///
/// # Errors
///
/// [`IdError::EmptyAstPath`] for an empty path or node type, [`IdError::ZeroOrdinal`],
/// or [`IdError::Nul`].
pub fn ast_fingerprint(
    node_type_path: &[&str],
    callee_text: &str,
    receiver_text: &str,
    ordinal: u32,
) -> Result<AstFingerprint, IdError> {
    if node_type_path.is_empty() || node_type_path.iter().any(|t| t.is_empty()) {
        return Err(IdError::EmptyAstPath);
    }
    if ordinal == 0 {
        return Err(IdError::ZeroOrdinal);
    }
    for node_type in node_type_path {
        no_nul("node_type_path", node_type)?;
    }
    no_nul("callee_text", callee_text)?;
    no_nul("receiver_text", receiver_text)?;
    let count = node_type_path.len().to_string();
    let ordinal = ordinal.to_string();
    let mut parts: Vec<&str> = Vec::with_capacity(node_type_path.len() + 4);
    parts.push(&count);
    parts.extend_from_slice(node_type_path);
    parts.extend_from_slice(&[callee_text, receiver_text, &ordinal]);
    Ok(AstFingerprint(lower_hex(&sha256_nul_separated(&parts))))
}

/// Everything a site's identity is computed from, and nothing else: its byte span,
/// lines and columns are stored on the site and are no part of it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SiteKey {
    /// The repository-relative POSIX path of the site's file.
    pub file_path: String,
    /// The definition the site is in; `None` at the top of a file.
    pub enclosing_node_id: Option<NodeId>,
    /// What the site is.
    pub site_kind: SiteKind,
    /// Where it is in its definition.
    pub ast_fingerprint: AstFingerprint,
}

impl SiteKey {
    /// A site's key.
    pub fn new(
        file_path: &str,
        enclosing_node_id: Option<NodeId>,
        site_kind: SiteKind,
        ast_fingerprint: AstFingerprint,
    ) -> Self {
        Self {
            file_path: file_path.to_owned(),
            enclosing_node_id,
            site_kind,
            ast_fingerprint,
        }
    }

    /// The site's id:
    /// `base32-crockford(sha256(file_path || 0x00 || enclosing_node_id || 0x00 || site_kind || 0x00 || ast_fingerprint))[0..26]`,
    /// with `enclosing_node_id` `""` for a site outside every definition.
    ///
    /// # Errors
    ///
    /// [`IdError::Nul`] if the path contains a NUL byte.
    pub fn site_id(&self) -> Result<SiteId, IdError> {
        no_nul("file_path", &self.file_path)?;
        let enclosing = self.enclosing_node_id.as_ref().map_or("", NodeId::as_str);
        let digest = sha256_nul_separated(&[
            &self.file_path,
            enclosing,
            self.site_kind.as_str(),
            self.ast_fingerprint.as_str(),
        ]);
        Ok(SiteId(crockford_id(&digest)))
    }
}

// --- edges -------------------------------------------------------------------------

/// An edge's identity:
/// `base32-crockford(sha256(src_node_id || dst_node_id || edge_kind || site_id))[0..26]`,
/// concatenated without separators as 4.2.1 writes it, `site_id` `""` for an edge with
/// no site. The site is part of the identity because one pair can be linked from many
/// sites; the band, score, strategy, observation and weight are not, because they can
/// change while the fact stays the same.
pub fn edge_id(src: &NodeId, dst: &NodeId, kind: EdgeKind, site: Option<&SiteId>) -> EdgeId {
    let digest = sha256(&[
        src.as_str().as_bytes(),
        dst.as_str().as_bytes(),
        kind.as_str().as_bytes(),
        site.map_or("", SiteId::as_str).as_bytes(),
    ]);
    EdgeId(crockford_id(&digest))
}
