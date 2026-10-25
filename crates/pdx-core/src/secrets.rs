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
//! whose matching is linear in the input, and loops that read each byte a bounded
//! number of times: no detector backtracks over source. What the detectors match is
//! versioned by [`SECRET_DETECTOR_VERSION`]; a change to any of them bumps it.
//!
//! | Detector | Matches | Masks |
//! |---|---|---|
//! | [`Detector::PrivateKey`] | A PEM `BEGIN` marker of one of [`PRIVATE_KEY_LABELS`] (`PRIVATE KEY`, `RSA PRIVATE KEY`, `EC PRIVATE KEY`, `DSA PRIVATE KEY`, `OPENSSH PRIVATE KEY`, `ENCRYPTED PRIVATE KEY`, `PGP PRIVATE KEY BLOCK`), to the `END` marker with the same label, or to the end of the input if there is none | Every base64 byte between the markers (`A-Z a-z 0-9 + / =`), except the `n`, `r` or `t` of an escape after a backslash |
//! | [`Detector::BearerToken`] | `Bearer`, any case, at a word boundary, then spaces or tabs and a token of at least 16 of `A-Z a-z 0-9 - . _ ~ + /`, running on over those and `=` (its padding, and anything after it in the same run) | The token |
//! | [`Detector::CredentialAssignment`] | A key ending, in any case, with one of [`CREDENTIAL_KEYS`] (its words joined by `_`, `-` or nothing), an optional closing quote, then `=`, `:`, `:=` or `=>`, then a value, read as [below](#credential-assignment-values) | The value |
//! | [`Detector::UriPassword`] | `scheme://user:password@`: the scheme a letter, then letters, digits, `+`, `-` and `.`, recognised at the start of the input or after any byte that is not an ASCII letter or digit (punctuation and `_` included); or, before `://`, a bearer token as [`Detector::BearerToken`] finds one, which masking makes a scheme; read from the source and again with the other shapes masked, where any masked token before `://` is a scheme | The password |
//! | [`Detector::CloudAccessKeyId`] | `AKIA` or `ASIA` and 16 of `A-Z 0-9`, at word boundaries | All 20 bytes |
//! | [`Detector::JsonWebToken`] | Three base64url segments (`A-Z a-z 0-9 - _`) joined by `.`, the first two opening with `eyJ` (a JSON object's `{"`) and at least 8 bytes long, the third possibly empty, at the start of the input or after a byte that is not an ASCII letter or digit | The whole token |
//! | [`Detector::GitHubToken`] | `gh`, one of `o p s u r` and `_`, then at least 36 ASCII letters and digits; or `github_pat_`, 22 of them, `_` and at least 59 more; at the start of the input or after a byte that is not an ASCII letter or digit | Every byte after the prefix |
//!
//! A value that is wholly a placeholder (`${NAME}`, `$NAME`, `{{ … }}`) is a reference
//! to a secret, not one, and is left as it is: configuration resolution reads it.
//! Ranges found by different detectors may overlap; they are merged before masking,
//! so the result does not depend on the order the detectors run in.
//!
//! # Credential assignment values
//!
//! A value is read where its key is written, and never across the bounds of the string
//! literal the key is in (issue 96).
//!
//! - **Literals.** Each line's string literals are read in order, each closing on its
//!   line or being none: a `"` literal honours backslash escapes; `'` delimits one only
//!   in a language whose single quotes delimit strings
//!   ([`SINGLE_QUOTED_STRING_LANGUAGES`]; elsewhere `'x'` and `'\…'` are characters and
//!   any other `'` a lifetime or a label); a backquote only in Go, JavaScript and
//!   TypeScript ([`BACKQUOTED_STRING_LANGUAGES`]). A `'` between two lower-case ASCII
//!   letters is an apostrophe (`don't`), never a quote.
//! - **Outside a literal**, the value is the literal opening right after the separator
//!   (a backquoted one only without a `${` interpolation), or, in a language whose bare
//!   values are literals ([`BARE_VALUE_LANGUAGES`]), the bare value there: one that does
//!   not open with a quote, a bracket, a reference or a block indicator, up to
//!   whitespace or a delimiter, apostrophes included.
//! - **Inside a literal**, there is none when the literal ends at the separator: in
//!   `"token=" + decrypt(t) + "…"` the code between the literals stays (the destructive
//!   mask of issue 96). Otherwise it is a literal of the other quote closing within this
//!   one (`'{"token": "…"}'`), else this literal's own bytes: in a language whose bare
//!   values are literals, the bare value; elsewhere, after `=` only, a run ending at the
//!   literal's end or at `;`, `&`, `,`, a space or a tab (a connection string's
//!   `…;Password=…;`, a query's, an argument's), which neither opens with a reference, a
//!   format or a parameter (`$ { % ? : @ * =`) nor runs into code (`( ) [ ] { } < > | $
//!   # \`). After `:` a literal's bytes are a name (`"auth:token:refresh"`, a channel's or
//!   a key's), which contracts read, or prose; neither is masked.
//!
//! Keys are found in one pass: after a value, the search resumes past what its reading
//! read, so a key inside a value is part of it.
//!
//! # One reading
//!
//! Every detector reads the original bytes once (and a view of them, below); masking
//! happens once, after. Masked output is never read again: normalising it changes nothing because no detector can
//! find in it what it did not find in the source (issue 72), never because it is
//! rescanned until it stops changing. A detector whose match one byte of another's mask
//! could complete must find that match in the source itself. So detection has three
//! tiers, each reading a view of the source in which every byte the tiers before it
//! found is already the mask, as a second reading would see it:
//!
//! 1. The shapes that no mask completes: private keys, bearer tokens, cloud access key
//!    ids, JSON Web Tokens and GitHub tokens, from the source.
//! 2. URI passwords, from the source and from the view of tier 1: a masked token before
//!    `://` is a scheme (`<JSON Web Token>://u:p@h`, issues 72 and 96).
//! 3. Credential assignment values, from the view of tiers 1 and 2: a bare value runs on
//!    over a URI password found inside it, as it would run over that password's mask
//!    (issue 96).
//!
//! Literals are read from tier 3's view, and nothing a detector masks changes how they
//! read: no mask holds a quote a literal is delimited by, a backslash before one, or a
//! byte beside an apostrophe that a value does not also hold.

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
pub const PRIVATE_KEY_LABELS: [&str; 7] = [
    "PRIVATE KEY",
    "RSA PRIVATE KEY",
    "EC PRIVATE KEY",
    "DSA PRIVATE KEY",
    "OPENSSH PRIVATE KEY",
    "ENCRYPTED PRIVATE KEY",
    "PGP PRIVATE KEY BLOCK",
];

/// Languages in which an unquoted assignment value is a literal string, and so is
/// masked like a quoted one. Elsewhere an unquoted value is code (a name, a call, an
/// expression) and is never masked: masking it would change what the engine extracts,
/// not hide a secret.
pub const BARE_VALUE_LANGUAGES: [&str; 5] =
    ["bash", "dockerfile", "markdown", "properties", "yaml"];

/// Languages whose single quotes delimit strings. Elsewhere `'` opens a character, a
/// lifetime or a label, and no value is single-quoted.
pub const SINGLE_QUOTED_STRING_LANGUAGES: [&str; 17] = [
    "bash",
    "dockerfile",
    "groovy",
    "javascript",
    "lua",
    "markdown",
    "perl",
    "php",
    "properties",
    "protobuf",
    "python",
    "ruby",
    "sql",
    "toml",
    "typescript",
    "xml",
    "yaml",
];

/// Languages whose backquotes delimit strings: Go's raw strings, JavaScript's and
/// TypeScript's templates. Elsewhere a backquote quotes a name or a command, and no
/// value is backquoted.
pub const BACKQUOTED_STRING_LANGUAGES: [&str; 3] = ["go", "javascript", "typescript"];

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
    /// JSON Web Tokens, wherever they stand.
    JsonWebToken,
    /// GitHub access tokens, wherever they stand.
    GitHubToken,
}

impl Detector {
    /// Every detector, in the order normalisation runs them; the order changes
    /// nothing.
    pub const ALL: [Self; 7] = [
        Self::PrivateKey,
        Self::BearerToken,
        Self::CredentialAssignment,
        Self::UriPassword,
        Self::CloudAccessKeyId,
        Self::JsonWebToken,
        Self::GitHubToken,
    ];

    /// The ranges of `bytes` this detector finds by their shape alone. A URI password
    /// is read after them, over them, and a credential assignment's value after every
    /// shape ([`secret_ranges_with`]).
    fn shapes(self, bytes: &[u8], out: &mut Vec<Range<usize>>) {
        match self {
            Self::PrivateKey => private_keys(bytes, out),
            Self::BearerToken => groups(&BEARER, bytes, out),
            // Read over the other shapes ([`secret_ranges_with`]).
            Self::CredentialAssignment | Self::UriPassword => {}
            Self::CloudAccessKeyId => groups(&ACCESS_KEY_ID, bytes, out),
            Self::JsonWebToken => after_boundary(&JSON_WEB_TOKEN, bytes, out),
            Self::GitHubToken => after_boundary(&GITHUB_TOKEN, bytes, out),
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
    let mut shapes = Vec::new();
    for detector in detectors {
        detector.shapes(bytes, &mut shapes);
    }
    let mut shapes = merge(shapes);
    if detectors.contains(&Detector::UriPassword) {
        // A password is read from the source, and from the source as a second reading
        // would see it: a masked token before `://` is a scheme there.
        let mut found = shapes.clone();
        uri_passwords(bytes, &mut found);
        if !shapes.is_empty() {
            let mut view = bytes.to_vec();
            mask(&mut view, &shapes);
            uri_passwords(&view, &mut found);
        }
        shapes = merge(found);
    }
    if !detectors.contains(&Detector::CredentialAssignment) {
        return shapes;
    }
    let mut found = shapes.clone();
    assignment_values(
        &View {
            bytes,
            masked: &shapes,
        },
        language,
        &mut found,
    );
    merge(found)
}

/// Whether a configuration key names a credential: it ends, in any case and with its
/// words joined by `_`, `-` or nothing, with one of [`CREDENTIAL_KEYS`], exactly the
/// keys [`Detector::CredentialAssignment`] masks the value of. A dotted key
/// (`spring.datasource.password`) is judged by its whole text, so by its last part.
pub fn is_credential_key(key: &str) -> bool {
    CREDENTIAL_KEY.is_match(key.as_bytes())
}

/// A key ending with a credential key, as [`assignment_prefix`] reads one.
static CREDENTIAL_KEY: LazyLock<Regex> = LazyLock::new(|| {
    let keys: Vec<String> = CREDENTIAL_KEYS
        .iter()
        .map(|k| {
            k.split(['_', '-'])
                .map(regex::escape)
                .collect::<Vec<_>>()
                .join("[_-]?")
        })
        .collect();
    regex(&format!(r"(?-u)(?i)(?:{})\z", keys.join("|")))
});

/// Masks every secret value in `bytes`, a file of `language`, in place. Returns how
/// many bytes were masked. The length never changes, and neither does any carriage
/// return or line feed.
pub fn normalise_in_place(bytes: &mut [u8], language: &str) -> usize {
    // The detectors inspect these original bytes once; their ranges are merged before
    // any byte is changed. Never mask and detect again (issue 72).
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

/// A bearer token: at least 16 token bytes, then any run of token bytes and `=`. It runs
/// on over `=` and what follows it in the same run, so padding never ends a match
/// early: masking turns `=` into a token byte, and a token that stopped there would
/// grow on a second pass (issue 44).
const BEARER_TOKEN: &str = r"[A-Za-z0-9\-._~+/]{16,}[A-Za-z0-9\-._~+/=]*";

/// `Bearer` and its token, group 1.
static BEARER: LazyLock<Regex> =
    LazyLock::new(|| regex(&format!(r"(?-u)(?i)\bbearer[ \t]+({BEARER_TOKEN})")));

/// An access key identifier, group 1.
static ACCESS_KEY_ID: LazyLock<Regex> =
    LazyLock::new(|| regex(r"(?-u)\b((?:AKIA|ASIA)[A-Z0-9]{16})\b"));

/// A JSON Web Token, group 1, after the byte before it. Each segment runs to the end of
/// its base64url run, so a mask beside one never lengthens it: no mask starts right
/// after a base64url byte or a `.`.
static JSON_WEB_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"(?-u)(?:\A|[^A-Za-z0-9])(eyJ[A-Za-z0-9_\-]{5,}\.eyJ[A-Za-z0-9_\-]{5,}\.[A-Za-z0-9_\-]*)",
    )
});

/// A GitHub token's bytes after its prefix (group 1, or 2 for a fine-grained token),
/// after the byte before it. The prefix stays: a token's kind is no secret, and a mask
/// that ran over it would let a second reading find another token after it.
static GITHUB_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"(?-u)(?:\A|[^A-Za-z0-9])(?:gh[opsur]_([A-Za-z0-9]{36,})|github_pat_([A-Za-z0-9]{22}_[A-Za-z0-9]{59,}))",
    )
});

/// A password in a URI's user information, group 1 (issues 72 and 73). What stands
/// before `://` is either
///
/// - a scheme (a letter, then letters, digits, `+`, `-` and `.`) recognised at the
///   start of the input or right after any byte that is not an ASCII letter or digit.
///   This is a secret detector's boundary, not a URI lexer's: punctuation before a
///   connection string (`-`, `+`, `.`, `_`, `/`, `:` …) never stops its password being
///   masked, so everything detector 2's word boundary found is still found, and `_`
///   too. After a letter or digit no scheme starts, and masking makes a letter, so a
///   mask never opens a boundary. The byte before the scheme is matched, never
///   captured; or
/// - a bearer token, [`BEARER`]'s exactly: masked, it is a run of [`MASK`], itself a
///   scheme, so a password after it must be found now, on the original bytes, or a
///   second pass would find it.
static URI_PASSWORD: LazyLock<Regex> = LazyLock::new(|| {
    regex(&format!(
        r#"(?-u)(?:(?:\A|[^A-Za-z0-9])[A-Za-z][A-Za-z0-9+.\-]*|(?i:\bbearer)[ \t]+{BEARER_TOKEN})://[^\s/?#@"'<>:]*:([^\s/?#@"'<>]+)@"#
    ))
});

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

/// A credential key, its separator and the blanks after it: where a value starts.
static ASSIGNMENT: LazyLock<Regex> = LazyLock::new(|| regex(&assignment_prefix()));

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

/// The secret group of every match of a pattern that opens with the byte before its
/// secret, or the input's start, matched but never captured. The next search starts at
/// a match's end, where that byte may be the next match's.
fn after_boundary(pattern: &Regex, bytes: &[u8], out: &mut Vec<Range<usize>>) {
    let mut from = 0;
    while let Some(caps) = pattern.captures_at(bytes, from) {
        if let Some(secret) = caps.iter().skip(1).flatten().next() {
            out.push(secret.range());
        }
        from = caps.get(0).expect("the whole match").end();
    }
}

/// The password of every URI with one, unless it is a placeholder.
fn uri_passwords(bytes: &[u8], out: &mut Vec<Range<usize>>) {
    let mut from = 0;
    while let Some(caps) = URI_PASSWORD.captures_at(bytes, from) {
        let whole = caps.get(0).expect("the whole match");
        let password = caps.get(1).expect("the password");
        if !PLACEHOLDER.is_match(password.as_bytes()) {
            out.push(password.range());
        }
        // A match ends with its `@`, which is no letter or digit, so a scheme may
        // start right after it: the next search starts on it, where the pattern can
        // read it as the byte before a scheme.
        from = whole.end() - 1;
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

// --- credential assignment values -----------------------------------------------

/// The source as a second reading would see it: every byte a shape found is already
/// the mask. Line endings are never masked, so they read as themselves.
struct View<'a> {
    bytes: &'a [u8],
    /// The shapes' ranges, sorted, disjoint.
    masked: &'a [Range<usize>],
}

impl View<'_> {
    fn len(&self) -> usize {
        self.bytes.len()
    }

    /// The byte at `i`, which is in the source.
    fn at(&self, i: usize) -> u8 {
        let b = self.bytes[i];
        let k = self.masked.partition_point(|r| r.start <= i);
        if k > 0 && i < self.masked[k - 1].end && b != b'\r' && b != b'\n' {
            MASK
        } else {
            b
        }
    }

    /// The byte at `i`, if the source has one.
    fn get(&self, i: usize) -> Option<u8> {
        (i < self.len()).then(|| self.at(i))
    }

    /// Whether the `'` at `i` stands between two lower-case ASCII letters: an
    /// apostrophe, never a quote. The mask is upper case, so masking never makes one.
    fn is_apostrophe(&self, i: usize) -> bool {
        self.at(i) == b'\''
            && i > 0
            && self.at(i - 1).is_ascii_lowercase()
            && self.get(i + 1).is_some_and(|b| b.is_ascii_lowercase())
    }
}

/// A string literal on one line: the positions of its opening and its closing quote.
#[derive(Clone, Copy, Debug)]
struct Literal {
    open: usize,
    close: usize,
}

/// Every string literal of the source, in order, as [the module](self) reads them.
fn literals(view: &View, language: &str) -> Vec<Literal> {
    let single = SINGLE_QUOTED_STRING_LANGUAGES.contains(&language);
    let back = BACKQUOTED_STRING_LANGUAGES.contains(&language);
    let mut out = Vec::new();
    // For each quote, the end of a line on which one of its kind closed no literal: no
    // later one on that line closes one either, so none is searched for twice.
    let mut unclosed_until = [0; 3];
    let mut i = 0;
    while i < view.len() {
        let b = view.at(i);
        let kind = match b {
            b'"' => Some(0),
            b'\'' if single => Some(1),
            b'`' if back => Some(2),
            _ => None,
        };
        match kind {
            Some(k) if i >= unclosed_until[k] && !view.is_apostrophe(i) => {
                match closing_quote(view, i) {
                    Ok(close) => {
                        out.push(Literal { open: i, close });
                        i = close + 1;
                        continue;
                    }
                    Err(line_end) => unclosed_until[k] = line_end,
                }
            }
            None if b == b'\'' => {
                if let Some(end) = character(view, i) {
                    i = end;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// The closing quote of the literal opening at `open`, or the end of its line if it
/// has none. A `"` literal's backslash escapes the byte after it; an apostrophe closes
/// nothing.
fn closing_quote(view: &View, open: usize) -> Result<usize, usize> {
    let quote = view.at(open);
    let mut j = open + 1;
    while j < view.len() {
        match view.at(j) {
            b'\r' | b'\n' => return Err(j),
            b'\\' if quote == b'"' => match view.get(j + 1) {
                Some(b'\r' | b'\n') | None => return Err(j + 1),
                Some(_) => j += 2,
            },
            b if b == quote && !view.is_apostrophe(j) => return Ok(j),
            _ => j += 1,
        }
    }
    Err(view.len())
}

/// The end of the character literal opening at `open` in a language whose single
/// quotes delimit characters: `'x'`, or an escape `'\…'` of at most a dozen bytes.
fn character(view: &View, open: usize) -> Option<usize> {
    match view.get(open + 1)? {
        b'\'' | b'\r' | b'\n' => None,
        b'\\' => {
            if matches!(view.get(open + 2)?, b'\r' | b'\n') {
                return None;
            }
            for j in open + 3..view.len().min(open + 14) {
                match view.at(j) {
                    b'\'' => return Some(j + 1),
                    b'\r' | b'\n' => return None,
                    _ => {}
                }
            }
            None
        }
        _ => (view.get(open + 2)? == b'\'').then_some(open + 3),
    }
}

/// The literal `at` is inside of, between its quotes.
fn literal_around(literals: &[Literal], at: usize) -> Option<Literal> {
    let k = literals.partition_point(|l| l.open < at);
    let literal = *literals.get(k.checked_sub(1)?)?;
    (at < literal.close).then_some(literal)
}

/// The literal opening at `at`.
fn literal_opening(literals: &[Literal], at: usize) -> Option<Literal> {
    let k = literals.partition_point(|l| l.open < at);
    literals.get(k).filter(|l| l.open == at).copied()
}

/// What reading a value found: the value, if there is one to mask, and where the
/// reading ended, past which the next key is searched for.
struct Read {
    value: Option<Range<usize>>,
    end: usize,
}

impl Read {
    const fn none(at: usize) -> Self {
        Self {
            value: None,
            end: at,
        }
    }
}

/// Every credential assignment's value, read as
/// [the module](self#credential-assignment-values) describes.
fn assignment_values(view: &View, language: &str, out: &mut Vec<Range<usize>>) {
    let bare = BARE_VALUE_LANGUAGES.contains(&language);
    let mut literals = None;
    let mut from = 0;
    while let Some(m) = ASSIGNMENT.find_at(view.bytes, from) {
        let literals = literals.get_or_insert_with(|| self::literals(view, language));
        let at = m.end();
        let separator = (m.start()..at)
            .rev()
            .find(|&i| !matches!(view.bytes[i], b' ' | b'\t'))
            .expect("a match ends with its separator and blanks");
        let read = match literal_around(literals, separator) {
            // `"token=" + decrypt(t) + "…"`: the literal ends; what follows is code.
            Some(literal) if at == literal.close => Read::none(at),
            Some(literal) => match inner_literal(view, at, literal) {
                Some(read) => read,
                None if bare => bare_value(view, at),
                None if view.bytes[separator] == b'=' => literal_value(view, at, literal),
                None => Read::none(at),
            },
            None => match literal_opening(literals, at) {
                Some(literal) => quoted_value(view, literal),
                None if bare => bare_value(view, at),
                None => Read::none(at),
            },
        };
        if let Some(value) = read.value
            && !value.is_empty()
            && !PLACEHOLDER.is_match(&view.bytes[value.clone()])
        {
            out.push(value);
        }
        from = m.end().max(read.end);
    }
}

/// A literal's contents, unless it is a template with an interpolation, which is code.
fn quoted_value(view: &View, literal: Literal) -> Read {
    let contents = literal.open + 1..literal.close;
    let interpolated = view.at(literal.open) == b'`'
        && contents
            .clone()
            .any(|i| view.at(i) == b'$' && view.get(i + 1) == Some(b'{'));
    Read {
        value: (!interpolated).then_some(contents),
        end: literal.close + 1,
    }
}

/// The contents of a literal of the other quote opening at `at` and closing within
/// `outer`: `'{"token": "…"}'`.
fn inner_literal(view: &View, at: usize, outer: Literal) -> Option<Read> {
    let quote = view.at(at);
    if !matches!(quote, b'"' | b'\'') || quote == view.at(outer.open) {
        return None;
    }
    let mut j = at + 1;
    while j < outer.close {
        match view.at(j) {
            b'\\' if quote == b'"' => j += 2,
            b if b == quote && !view.is_apostrophe(j) => {
                return Some(Read {
                    value: Some(at + 1..j),
                    end: j + 1,
                });
            }
            _ => j += 1,
        }
    }
    None
}

/// Whether `b` may continue a bare value.
fn in_bare_value(b: u8) -> bool {
    !b" \t\r\n\x0b\x0c\"'`#,;(){}[]<>\\".contains(&b)
}

/// A bare value at `at`: one that does not open with a quote, a bracket, a reference or
/// a block indicator, up to whitespace or a delimiter. An apostrophe is part of it.
fn bare_value(view: &View, at: usize) -> Read {
    match view.get(at) {
        Some(b) if in_bare_value(b) && !b"|&*$".contains(&b) => {}
        _ => return Read::none(at),
    }
    let mut end = at + 1;
    while end < view.len() && (in_bare_value(view.at(end)) || view.is_apostrophe(end)) {
        end += 1;
    }
    Read {
        value: Some(at..end),
        end,
    }
}

/// Whether `b` may continue a value inside a literal.
fn in_literal_value(b: u8) -> bool {
    !b" \t\r\n\x0b\x0c\"'`\\;&,(){}[]<>|$#".contains(&b)
}

/// A value inside `literal` at `at`, after `=`, in a language whose bare values are
/// code: a run ending at the literal's end or at `;`, `&`, `,`, a space or a tab, which
/// opens with no reference, format or parameter and runs into no code. An apostrophe is
/// part of it.
fn literal_value(view: &View, at: usize, literal: Literal) -> Read {
    let first = view.at(at);
    if !in_literal_value(first) || b"%?:@*={".contains(&first) {
        return Read::none(at);
    }
    let mut end = at + 1;
    while end < literal.close && (in_literal_value(view.at(end)) || view.is_apostrophe(end)) {
        end += 1;
    }
    let delimited =
        end == literal.close || matches!(view.at(end), b';' | b'&' | b',' | b' ' | b'\t');
    Read {
        value: delimited.then_some(at..end),
        end,
    }
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
