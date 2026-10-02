//! Content secret normalisation (5.12), the second layer of secret handling.
//!
//! The first layer is discovery's: a file whose path matches a secret pattern is
//! `redacted` and never read. Every other file the engine extracts is normalised
//! first: each secret value the detectors below find is masked, byte for byte, with
//! [`MASK`], so a secret never reaches the engine, a cached extraction or anything
//! derived from them.
//!
//! Masking preserves length (issue 37). The bytes after a secret keep their offsets,
//! so spans, sites and identities taken from the normalised source are the source's
//! own; and carriage returns and line feeds are never masked, so every line keeps its
//! number and its ending. A key, a separator, a quote, a PEM marker line and the
//! indentation around a value stay as they were; only the value's bytes change.
//! `<REDACTED_SECRET>` remains how a masked value is described, not a substitution in
//! the parser's input.
//!
//! Detection works on bytes, never requiring a file to be UTF-8, with `regex::bytes`,
//! whose matching is linear in the input: no detector backtracks over source. What
//! the detectors match is versioned by [`SECRET_DETECTOR_VERSION`]; a change to any of
//! them bumps it.
//!
//! | Detector | Matches | Masks |
//! |---|---|---|
//! | [`Detector::PrivateKey`] | A PEM `BEGIN` marker of a `PRIVATE KEY`, `RSA PRIVATE KEY`, `EC PRIVATE KEY`, `DSA PRIVATE KEY` or `OPENSSH PRIVATE KEY`, to the `END` marker with the same label, or to the end of the input if there is none | Every base64 byte between the markers (`A-Z a-z 0-9 + / =`), except the `n`, `r` or `t` of an escape after a backslash |
//! | [`Detector::BearerToken`] | `Bearer`, any case, at a word boundary, then spaces or tabs and a token of at least 16 of `A-Z a-z 0-9 - . _ ~ + /` with any `=` padding | The token |
//! | [`Detector::CredentialAssignment`] | A key ending, in any case, with one of [`CREDENTIAL_KEYS`] (its words joined by `_`, `-` or nothing), an optional closing quote, then `=`, `:`, `:=` or `=>`, then a value | The value: inside its quotes when quoted; a bare value only in a language whose bare values are literals ([`BARE_VALUE_LANGUAGES`]) |
//! | [`Detector::UriPassword`] | `scheme://user:password@` | The password |
//! | [`Detector::CloudAccessKeyId`] | `AKIA` or `ASIA` and 16 of `A-Z 0-9`, at word boundaries | All 20 bytes |
//!
//! A value that is wholly a placeholder (`${NAME}`, `$NAME`, `{{ … }}`) is a reference
//! to a secret, not one, and is left as it is: configuration resolution reads it.
//! Ranges found by different detectors may overlap; they are merged before masking,
//! so the result does not depend on the order the detectors run in.

use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::LazyLock;

use regex::bytes::Regex;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

use crate::config::SecretsConfig;
use crate::consts::SECRET_DETECTOR_VERSION;

/// The byte a secret value's bytes are replaced with.
pub const MASK: u8 = b'X';

/// The key names an assignment's key ends with, in any case and with its words joined
/// by `_`, `-` or nothing, to be a credential.
pub const CREDENTIAL_KEYS: [&str; 16] = [
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
    "api_key",
    "api-key",
    "apikey",
    "access_key",
    "access-key",
    "access_key_id",
    "secret_key",
    "secret-key",
    "secret_access_key",
    "client_secret",
    "private_key",
];

/// The labels a PEM private-key block may carry.
pub const PRIVATE_KEY_LABELS: [&str; 5] = [
    "PRIVATE KEY",
    "RSA PRIVATE KEY",
    "EC PRIVATE KEY",
    "DSA PRIVATE KEY",
    "OPENSSH PRIVATE KEY",
];

/// Languages in which an unquoted assignment value is a literal string, and so is
/// masked like a quoted one. Elsewhere an unquoted value is code (a name, a call, an
/// expression) and is never masked: masking it would change what the engine extracts,
/// not hide a secret.
pub const BARE_VALUE_LANGUAGES: [&str; 5] =
    ["bash", "dockerfile", "markdown", "properties", "yaml"];

/// A content detector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Detector {
    /// PEM private-key blocks.
    PrivateKey,
    /// `Bearer` tokens.
    BearerToken,
    /// Credential assignments in source and configuration.
    CredentialAssignment,
    /// Passwords in URI user information.
    UriPassword,
    /// Cloud access key identifiers.
    CloudAccessKeyId,
}

impl Detector {
    /// Every detector, in the order normalisation runs them; the order changes
    /// nothing.
    pub const ALL: [Self; 5] = [
        Self::PrivateKey,
        Self::BearerToken,
        Self::CredentialAssignment,
        Self::UriPassword,
        Self::CloudAccessKeyId,
    ];

    /// The ranges of `bytes`, a file of `language`, this detector finds secret.
    fn find(self, bytes: &[u8], language: &str, out: &mut Vec<Range<usize>>) {
        match self {
            Self::PrivateKey => private_keys(bytes, out),
            Self::BearerToken => groups(&BEARER, bytes, out),
            Self::CredentialAssignment => {
                let pattern = if BARE_VALUE_LANGUAGES.contains(&language) {
                    &ASSIGNMENT_ANY_VALUE
                } else {
                    &ASSIGNMENT_QUOTED_VALUE
                };
                values(pattern, bytes, out);
            }
            Self::UriPassword => values(&URI_PASSWORD, bytes, out),
            Self::CloudAccessKeyId => groups(&ACCESS_KEY_ID, bytes, out),
        }
    }
}

/// The secret ranges every detector finds in `bytes`, a file of `language` (the
/// matrix's id), sorted and merged.
pub fn secret_ranges(bytes: &[u8], language: &str) -> Vec<Range<usize>> {
    secret_ranges_with(&Detector::ALL, bytes, language)
}

/// The secret ranges the given detectors find, sorted and merged: the same whatever
/// order they are given in.
pub fn secret_ranges_with(
    detectors: &[Detector],
    bytes: &[u8],
    language: &str,
) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    for detector in detectors {
        detector.find(bytes, language, &mut found);
    }
    merge(found)
}

/// Masks every secret value in `bytes`, a file of `language`, in place. Returns how
/// many bytes were masked. The length never changes, and neither does any carriage
/// return or line feed.
pub fn normalise_in_place(bytes: &mut [u8], language: &str) -> usize {
    let ranges = secret_ranges(bytes, language);
    mask(bytes, &ranges)
}

/// Masks `ranges` of `bytes`, leaving line endings. Returns how many bytes changed
/// to the mask.
fn mask(bytes: &mut [u8], ranges: &[Range<usize>]) -> usize {
    let mut masked = 0;
    for range in ranges {
        for b in &mut bytes[range.clone()] {
            if *b != b'\r' && *b != b'\n' {
                *b = MASK;
                masked += 1;
            }
        }
    }
    masked
}

/// Sorts ranges and merges those that overlap or touch.
fn merge(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.retain(|r| r.start < r.end);
    ranges.sort_unstable_by_key(|r| (r.start, r.end));
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("a detector's pattern compiles")
}

/// The opening marker of a private key; group 1 is its label.
static PRIVATE_KEY_BEGIN: LazyLock<Regex> = LazyLock::new(|| {
    let labels: Vec<String> = PRIVATE_KEY_LABELS
        .iter()
        .map(|l| regex::escape(l))
        .collect();
    regex(&format!("(?-u)-----BEGIN ({})-----", labels.join("|")))
});

/// `Bearer` and its token, group 1.
static BEARER: LazyLock<Regex> =
    LazyLock::new(|| regex(r"(?-u)(?i)\bbearer[ \t]+([A-Za-z0-9\-._~+/]{16,}=*)"));

/// An access key identifier, group 1.
static ACCESS_KEY_ID: LazyLock<Regex> =
    LazyLock::new(|| regex(r"(?-u)\b((?:AKIA|ASIA)[A-Z0-9]{16})\b"));

/// A password in a URI's user information, group 1.
static URI_PASSWORD: LazyLock<Regex> =
    LazyLock::new(|| regex(r#"(?-u)(?i)\b[a-z][a-z0-9+.\-]*://[^\s/?#@"'<>:]*:([^\s/?#@"'<>]+)@"#));

/// A credential key, an optional closing quote and a separator. A key's words may be
/// joined by `_`, `-` or nothing, so `access_key` is also `access-key`, `accesskey`
/// and `accessKey`.
fn assignment_prefix() -> String {
    let keys: Vec<String> = CREDENTIAL_KEYS
        .iter()
        .map(|k| {
            k.split(['_', '-'])
                .map(regex::escape)
                .collect::<Vec<_>>()
                .join("[_-]?")
        })
        .collect();
    format!(
        r#"(?-u)(?i)[A-Za-z0-9_.\-]*(?:{})["']?[ \t]*(?::=|=>|[:=])[ \t]*"#,
        keys.join("|")
    )
}

/// A double-quoted value (group 1) or a single-quoted one (group 2).
const QUOTED_VALUE: &str = r#""((?:[^"\\\r\n]|\\[^\r\n])*)"|'([^'\r\n]*)'"#;

/// A bare value (group 3): one that does not open with a quote, a bracket, a
/// reference or a block indicator, up to whitespace or a delimiter.
const BARE_VALUE: &str = r#"([^\s"'`#,;(){}\[\]<>|&*$\\][^\s"'`#,;(){}\[\]<>\\]*)"#;

static ASSIGNMENT_QUOTED_VALUE: LazyLock<Regex> =
    LazyLock::new(|| regex(&format!("{}(?:{QUOTED_VALUE})", assignment_prefix())));

static ASSIGNMENT_ANY_VALUE: LazyLock<Regex> = LazyLock::new(|| {
    regex(&format!(
        "{}(?:{QUOTED_VALUE}|{BARE_VALUE})",
        assignment_prefix()
    ))
});

/// A value that is wholly a reference to a secret rather than one.
static PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| regex(r"(?-u)\A(?:\$\{[^}]*\}|\$[A-Za-z_][A-Za-z0-9_]*|\{\{[^}]*\}\})\z"));

/// The first capture group of every match.
fn groups(pattern: &Regex, bytes: &[u8], out: &mut Vec<Range<usize>>) {
    for caps in pattern.captures_iter(bytes) {
        if let Some(m) = caps.get(1) {
            out.push(m.range());
        }
    }
}

/// The value group of every match, whichever of them matched, unless the value is a
/// placeholder.
fn values(pattern: &Regex, bytes: &[u8], out: &mut Vec<Range<usize>>) {
    for caps in pattern.captures_iter(bytes) {
        if let Some(m) = caps.iter().skip(1).flatten().next()
            && !PLACEHOLDER.is_match(m.as_bytes())
        {
            out.push(m.range());
        }
    }
}

/// The base64 bytes of every private-key block.
fn private_keys(bytes: &[u8], out: &mut Vec<Range<usize>>) {
    let mut from = 0;
    while from < bytes.len() {
        let Some(caps) = PRIVATE_KEY_BEGIN.captures_at(bytes, from) else {
            break;
        };
        let begin = caps.get(0).expect("the whole match");
        let label = caps.get(1).expect("the label").as_bytes();
        let mut end_marker = b"-----END ".to_vec();
        end_marker.extend_from_slice(label);
        end_marker.extend_from_slice(b"-----");
        let (payload_end, next) = match find(&bytes[begin.end()..], &end_marker) {
            Some(at) => (begin.end() + at, begin.end() + at + end_marker.len()),
            None => (bytes.len(), bytes.len()),
        };
        base64_runs(bytes, begin.end()..payload_end, out);
        from = next;
    }
}

/// The runs of base64 bytes in `range`, the letter of a `\n`, `\r` or `\t` escape
/// excepted: an escape is the string's syntax, never key material.
fn base64_runs(bytes: &[u8], range: Range<usize>, out: &mut Vec<Range<usize>>) {
    let mut run: Option<usize> = None;
    for i in range.clone() {
        let b = bytes[i];
        let escape_letter =
            i > range.start && bytes[i - 1] == b'\\' && matches!(b, b'n' | b'r' | b't');
        let secret =
            !escape_letter && (b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='));
        match (secret, run) {
            (true, None) => run = Some(i),
            (false, Some(start)) => {
                out.push(start..i);
                run = None;
            }
            _ => {}
        }
    }
    if let Some(start) = run {
        out.push(start..range.end);
    }
}

/// The first position of `needle` in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// The identity of the effective secret policy: a SHA-256 over its canonical form,
/// which is the compact JSON object
///
/// ```text
/// {"detector_version":<SECRET_DETECTOR_VERSION>,"patterns":[<effective patterns>]}
/// ```
///
/// with the keys in that order, no whitespace, and the patterns as JSON strings in
/// byte order, each once (the effective set of `[secrets] patterns`, defaults
/// included). Two policies that redact and mask alike have the same digest; nothing
/// else (a path, a time, the repository's content, the machine) enters it. Extraction
/// caches key on it, and the segment's build key will.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SecretPolicyDigest([u8; 32]);

impl SecretPolicyDigest {
    /// The digest of the policy `secrets` configures, under this build's detectors.
    pub fn of(secrets: &SecretsConfig) -> Self {
        Self::for_policy(SECRET_DETECTOR_VERSION, &secrets.patterns)
    }

    /// The digest of the policy with these detectors and these effective patterns.
    ///
    /// # Panics
    ///
    /// Never: a number and a set of strings always serialise.
    pub fn for_policy(detector_version: u32, patterns: &BTreeSet<String>) -> Self {
        #[derive(Serialize)]
        struct Canonical<'a> {
            detector_version: u32,
            patterns: &'a BTreeSet<String>,
        }
        let json = serde_json::to_vec(&Canonical {
            detector_version,
            patterns,
        })
        .expect("a policy serialises");
        Self(sha2::Sha256::digest(&json).into())
    }

    /// The digest's bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Display for SecretPolicyDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl std::fmt::Debug for SecretPolicyDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SecretPolicyDigest({self})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A private-key block's opening and closing markers, assembled here so that no
    /// whole marker is written in the source.
    fn markers(label: &str) -> (String, String) {
        (
            format!("-----BEGIN {label}-----"),
            format!("-----END {label}-----"),
        )
    }

    #[test]
    fn ranges_merge_when_they_overlap_or_touch() {
        assert_eq!(
            merge(vec![5..9, 0..2, 1..3, 9..10, 12..12]),
            vec![0..3, 5..10]
        );
    }

    #[test]
    fn a_key_without_an_end_is_masked_to_the_end() {
        let (begin, _) = markers(PRIVATE_KEY_LABELS[0]);
        let mut bytes = format!("{begin}\nABCD\nEFGH").into_bytes();
        normalise_in_place(&mut bytes, "yaml");
        assert_eq!(bytes, format!("{begin}\nXXXX\nXXXX").into_bytes());
    }

    #[test]
    fn escapes_inside_a_key_keep_their_letter() {
        let (begin, end) = markers(PRIVATE_KEY_LABELS[2]);
        let mut bytes = format!(r#""{begin}\nAbC+/=\n{end}""#).into_bytes();
        normalise_in_place(&mut bytes, "python");
        assert_eq!(bytes, format!(r#""{begin}\nXXXXXX\n{end}""#).into_bytes());
    }

    #[test]
    fn placeholders_are_left_alone() {
        for value in ["${DB_PASSWORD}", "$DB_PASSWORD", "{{ .Values.password }}"] {
            let source = format!("{}: \"{value}\"\n", CREDENTIAL_KEYS[0]);
            let mut bytes = source.clone().into_bytes();
            normalise_in_place(&mut bytes, "yaml");
            assert_eq!(bytes, source.as_bytes(), "{value}");
        }
    }

    #[test]
    fn bare_values_are_masked_only_where_they_are_literals() {
        let source = b"password = current_password\n";
        let mut code = source.to_vec();
        normalise_in_place(&mut code, "python");
        assert_eq!(code, source, "a name in code is not a secret");
        let mut config = b"password = sample\n".to_vec();
        normalise_in_place(&mut config, "properties");
        assert_eq!(config, b"password = XXXXXX\n");
    }
}
