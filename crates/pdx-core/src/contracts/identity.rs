//! Contract identity (4.7.1, issue 58): the normalised matching keys, the namespace
//! identities they are scoped by, and the one encoding of `namespace_key` every
//! contract id is computed over.
//!
//! A contract's id is 4.2.1's estate-level node id over its `namespace_key`, and
//! nothing else: [`contract_id`] is `NodeKey::estate(kind, namespace_key)`. Owner,
//! direction, raw form, file and line are never part of it.
//!
//! `namespace_key` is, for a contract whose identity is established (`exact` or
//! `declared`), compact JSON with exactly the members `identity` and `key`, in that
//! order, escaped as a route's qualified name is (4.2.1, rule 12); for an unresolved
//! identity, `unresolved:<repo_id>:<key>`, so unresolved contracts never meet across
//! repositories (D28); and for an artifact, whose coordinate is unique in its
//! ecosystem's registry, the key itself. The identity's strength is not in it, so an
//! `exact` and a `declared` observation of one identity are one contract.

use serde::Serialize;

use crate::ids::{IdError, NodeId, NodeKey, RepoId};
use crate::kinds::ContractKind;
use crate::model::IdentityStrength;

/// Where a contract's namespace comes from (4.7.1).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ContractIdentity {
    /// The source or configuration describing the endpoint names it: a URL's host, a
    /// specification's literal server, a literal JDBC host and database, a literal
    /// broker. Also an artifact's coordinate, unique in its registry.
    Exact(String),
    /// The repository's `pdx.toml [identity]` declares it.
    Declared(String),
    /// Nothing proves one.
    Unresolved,
}

impl ContractIdentity {
    /// The identity's strength.
    pub fn strength(&self) -> IdentityStrength {
        match self {
            Self::Exact(_) => IdentityStrength::Exact,
            Self::Declared(_) => IdentityStrength::Declared,
            Self::Unresolved => IdentityStrength::Unresolved,
        }
    }
}

/// The members of a resolved `namespace_key`, in their order.
#[derive(Serialize)]
struct Namespace<'a> {
    identity: &'a str,
    key: &'a str,
}

/// Whether a contract kind is an artifact, whose coordinate is its own namespace.
pub const fn is_artifact(kind: ContractKind) -> bool {
    matches!(kind, ContractKind::Artifact | ContractKind::ArtifactVersion)
}

/// A contract's `namespace_key` (issue 58).
///
/// - an artifact: the key, whatever the identity;
/// - `exact` or `declared`: `{"identity":…,"key":…}`, compact;
/// - `unresolved`: `unresolved:<repo_id>:<key>`.
pub fn namespace_key(
    kind: ContractKind,
    identity: &ContractIdentity,
    key: &str,
    repo: &RepoId,
) -> String {
    if is_artifact(kind) {
        return key.to_owned();
    }
    match identity {
        ContractIdentity::Exact(name) | ContractIdentity::Declared(name) => {
            serde_json::to_string(&Namespace {
                identity: name,
                key,
            })
            .unwrap_or_default()
        }
        ContractIdentity::Unresolved => format!("unresolved:{}:{key}", repo.as_str()),
    }
}

/// A contract's id: 4.2.1's estate node id of its kind over its `namespace_key`.
///
/// # Errors
///
/// [`IdError`] for a key holding a NUL, which no id may hash.
pub fn contract_id(kind: ContractKind, namespace_key: &str) -> Result<NodeId, IdError> {
    NodeKey::estate(kind.node_kind(), namespace_key)?.node_id()
}

// --- API keys ----------------------------------------------------------------------

/// An HTTP method token, upper-cased; `None` for anything that is not one.
pub fn http_method(raw: &str) -> Option<String> {
    let method = raw.trim();
    (!method.is_empty() && method.bytes().all(|b| b.is_ascii_alphabetic()))
        .then(|| method.to_ascii_uppercase())
}

/// An API contract's key: the method upper-cased, a space, and [`api_path`].
pub fn api_key(method: &str, path_or_url: &str) -> Option<String> {
    Some(format!(
        "{} {}",
        http_method(method)?,
        api_path(path_or_url)?
    ))
}

/// The path template of a URL or path (4.7.1, issue 58).
///
/// Scheme, authority and user information, query and fragment are removed; the path
/// begins with `/`; every path parameter, whatever its syntax (`{id}`, `{id:[0-9]+}`,
/// `:id`, `<id>`, `<int:id>`), is `{}`; trailing slashes are removed except from `/`.
/// The case of the path and its percent-encoding are kept as written, and dot
/// segments are not resolved. `None` for an empty input or one holding whitespace or
/// a control character.
pub fn api_path(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let mut rest = raw;
    if let Some(after) = strip_scheme(rest) {
        rest = after.find('/').map_or("", |i| &after[i..]);
    } else if let Some(after) = rest.strip_prefix("//") {
        rest = after.find('/').map_or("", |i| &after[i..]);
    }
    let end = rest.find(['?', '#']).unwrap_or(rest.len());
    let path = &rest[..end];
    let mut out = String::with_capacity(path.len() + 1);
    if !path.starts_with('/') {
        out.push('/');
    }
    out.push_str(&normalise_parameters(path)?);
    while out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    Some(out)
}

/// `scheme://` removed, if the text starts with one.
fn strip_scheme(text: &str) -> Option<&str> {
    let at = text.find("://")?;
    let scheme = &text[..at];
    let valid = scheme
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    valid.then(|| &text[at + 3..])
}

/// Every path parameter as `{}`: a braced or angle-bracketed parameter wherever it is
/// (nested braces in a pattern included), and a segment that starts with `:`.
fn normalise_parameters(path: &str) -> Option<String> {
    let mut out = String::with_capacity(path.len());
    for (i, segment) in path.split('/').enumerate() {
        if i > 0 {
            out.push('/');
        }
        if segment.starts_with(':') && segment.len() > 1 {
            out.push_str("{}");
            continue;
        }
        let mut chars = segment.chars().peekable();
        while let Some(c) = chars.next() {
            let close = match c {
                '{' => '}',
                '<' => '>',
                _ => {
                    out.push(c);
                    continue;
                }
            };
            let mut depth = 1usize;
            for inner in chars.by_ref() {
                if inner == c {
                    depth += 1;
                } else if inner == close {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            if depth != 0 {
                // An unbalanced parameter: not a template this can read.
                return None;
            }
            out.push_str("{}");
        }
    }
    Some(out)
}

// --- Authorities --------------------------------------------------------------------

/// The canonical authority of an absolute URL, as an API or broker identity: its host
/// lower-cased with one terminal dot removed and its user information dropped, and an
/// explicit port kept unless it is the scheme's default (`http` 80, `https` 443).
/// `None` for no host, a templated or placeholder host, or a loopback or unspecified
/// address, which names no shared service.
pub fn url_authority(url: &str) -> Option<String> {
    let url = url.trim();
    let after = strip_scheme(url)?;
    let scheme = url[..url.len() - after.len() - 3].to_ascii_lowercase();
    let end = after.find(['/', '?', '#']).unwrap_or(after.len());
    host_port(&after[..end], Some(&scheme))
}

/// The canonical form of `host[:port]`, user information dropped, the scheme's default
/// port omitted when the scheme is known; `None` as for [`url_authority`].
pub fn host_port(authority: &str, scheme: Option<&str>) -> Option<String> {
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    if authority.is_empty()
        || authority
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '{' | '}' | '$' | '%'))
    {
        return None;
    }
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let close = rest.find(']')?;
        let host = &authority[..close + 2];
        let port = rest[close + 1..].strip_prefix(':');
        if !rest[close + 1..].is_empty() && port.is_none() {
            return None;
        }
        (host.to_owned(), port)
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_owned(), Some(p)),
            None => (authority.to_owned(), None),
        }
    };
    let mut host = host.to_ascii_lowercase();
    if host.ends_with('.') && host.len() > 1 {
        host.pop();
    }
    if host.is_empty() || (host.contains(':') && !host.starts_with('[')) || is_local(&host) {
        return None;
    }
    let port = match port {
        Some(p) if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) => return None,
        Some(p) => Some(p),
        None => None,
    };
    let default = match scheme {
        Some("http" | "ws") => Some("80"),
        Some("https" | "wss") => Some("443"),
        _ => None,
    };
    Some(match port {
        Some(p) if Some(p) != default => format!("{host}:{p}"),
        _ => host,
    })
}

/// A loopback or unspecified host: it names the machine the code runs on, never a
/// service another repository could share.
fn is_local(host: &str) -> bool {
    host == "localhost"
        || host.ends_with(".localhost")
        || host == "0.0.0.0"
        || host == "[::]"
        || host == "[::1]"
        || host
            .strip_prefix("127.")
            .is_some_and(|rest| rest.split('.').count() == 3)
}

/// A declared identity from `pdx.toml [identity]`, canonical as an authority is: the
/// host lower-cased, one terminal dot removed, a port kept as written. Declared names
/// are the owner's statement, so a local name is kept.
pub fn declared_name(raw: &str) -> Option<String> {
    let mut name = raw.trim().to_ascii_lowercase();
    if name.ends_with('.') && name.len() > 1 {
        name.pop();
    }
    (!name.is_empty() && !name.contains('\0')).then_some(name)
}

// --- Channels -----------------------------------------------------------------------

/// The simple placeholder form 4.7.1 resolves, `${KEY}`, and its key; `None` for any
/// other text, a default (`${KEY:default}`) or a nested placeholder included.
pub fn simple_placeholder(text: &str) -> Option<&str> {
    let key = text.strip_prefix("${")?.strip_suffix('}')?;
    let valid = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '[' | ']'));
    valid.then_some(key)
}

/// Whether a text holds a placeholder of any form.
pub fn has_placeholder(text: &str) -> bool {
    text.contains("${")
}

/// The key of an unresolved channel destination: `unresolved:` and the text as written.
pub fn unresolved_channel_key(raw: &str) -> String {
    format!("unresolved:{raw}")
}

// --- Tables -------------------------------------------------------------------------

/// A table's key (4.7.1, issue 58): each part unquoted (`"…"`, `` `…` ``, `[…]`) and
/// lower-cased, and the schema dropped when it is `public`. Another schema is kept.
/// `None` for an empty part or anything that is not an identifier.
pub fn table_key(raw: &str) -> Option<String> {
    let parts = identifier_parts(raw)?;
    let parts: Vec<&str> = match parts.as_slice() {
        [schema, table] if schema == "public" => vec![table.as_str()],
        _ => parts.iter().map(String::as_str).collect(),
    };
    Some(parts.join("."))
}

/// A column's key (issue 58): its table's key, `.`, and the column unquoted and
/// lower-cased.
pub fn column_key(table: &str, column: &str) -> Option<String> {
    let table = table_key(table)?;
    let column = identifier_parts(column)?;
    let [column] = column.as_slice() else {
        return None;
    };
    Some(format!("{table}.{column}"))
}

/// A dotted SQL identifier's parts, unquoted and lower-cased.
pub fn identifier_parts(raw: &str) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    let mut chars = raw.trim().chars().peekable();
    loop {
        let part = match chars.peek() {
            Some(&open @ ('"' | '`' | '[')) => {
                chars.next();
                let close = if open == '[' { ']' } else { open };
                let mut text = String::new();
                loop {
                    match chars.next() {
                        Some(c) if c == close => {
                            if close != ']' && chars.peek() == Some(&close) {
                                chars.next();
                                text.push(c);
                            } else {
                                break;
                            }
                        }
                        Some(c) => text.push(c),
                        None => return None,
                    }
                }
                text
            }
            Some(_) => {
                let mut text = String::new();
                while let Some(&c) = chars.peek() {
                    if c == '.' {
                        break;
                    }
                    if !(c.is_alphanumeric() || matches!(c, '_' | '$')) {
                        return None;
                    }
                    text.push(c);
                    chars.next();
                }
                text
            }
            None => return None,
        };
        if part.is_empty() || part.chars().any(char::is_control) {
            return None;
        }
        parts.push(part.to_lowercase());
        match chars.next() {
            None => return Some(parts),
            Some('.') => {}
            Some(_) => return None,
        }
    }
}

// --- Artifacts ----------------------------------------------------------------------

/// An artifact's ecosystem, as its coordinate names it (issue 58).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ecosystem {
    /// Maven and Gradle.
    Maven,
    /// npm.
    Npm,
    /// Cargo.
    Cargo,
    /// Go modules.
    Go,
    /// `NuGet`.
    Nuget,
    /// `PyPI`.
    Pypi,
    /// Alire.
    Alire,
    /// GNAT project files.
    Gpr,
}

impl Ecosystem {
    /// The ecosystem's name in a coordinate.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Maven => "maven",
            Self::Npm => "npm",
            Self::Cargo => "cargo",
            Self::Go => "go",
            Self::Nuget => "nuget",
            Self::Pypi => "pypi",
            Self::Alire => "alire",
            Self::Gpr => "gpr",
        }
    }
}

/// An artifact's key: `ecosystem:group:name`, the group empty where the ecosystem has
/// none. `None` for an empty name or a part holding a `:` the coordinate could not be
/// read back from, or a control character.
pub fn artifact_key(ecosystem: Ecosystem, group: &str, name: &str) -> Option<String> {
    let bad = |s: &str| s.contains(':') || s.chars().any(|c| c.is_control() || c.is_whitespace());
    (!name.is_empty() && !bad(group) && !bad(name))
        .then(|| format!("{}:{group}:{name}", ecosystem.as_str()))
}

/// An artifact version's key: the artifact's, `@`, and the version as declared, a
/// range's syntax kept verbatim. `None` for an empty version or one holding a control
/// character.
pub fn artifact_version_key(artifact_key: &str, version: &str) -> Option<String> {
    let version = version.trim();
    (!version.is_empty() && !version.chars().any(char::is_control))
        .then(|| format!("{artifact_key}@{version}"))
}
