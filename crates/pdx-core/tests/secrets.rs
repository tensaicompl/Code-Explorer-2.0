//! Content secret normalisation (5.12, issue 37) and the secret-policy digest.
//!
//! Every secret here is synthetic: a value shaped like the real thing and carrying a
//! `PDXSYNTH` tag, assembled at run time so that no whole credential-shaped string is
//! written in the source for a scanner to mistake for a real one.

use std::collections::BTreeSet;
use std::path::Path;

use pdx_core::config::PdxConfig;
use pdx_core::consts::SECRET_DETECTOR_VERSION;
use pdx_core::secrets::{
    self, CREDENTIAL_KEYS, Detector, MASK, PRIVATE_KEY_LABELS, SecretPolicyDigest,
};
use proptest::prelude::*;

// --- synthetic secrets ---------------------------------------------------------------

/// A PEM private-key block with `label`, its body lines and `eol` line endings.
fn key_block(label: &str, body: &[&str], eol: &str) -> String {
    let mut out = format!("-----BEGIN {label}-----{eol}");
    for line in body {
        out.push_str(line);
        out.push_str(eol);
    }
    for part in ["-----END ", label, "-----", eol] {
        out.push_str(part);
    }
    out
}

/// Base64-looking key material with the synthetic tag.
const KEY_BODY: [&str; 2] = [
    "UERYU1lOVEhLRVlQREFZTE9BRDAwMDFQRFhTWU5USEtFWVBBWUxPQUQwMDAx",
    "UERYU1lOVEgvK0tFWT0=",
];

/// A synthetic access key id: the four-letter prefix and sixteen of `A-Z0-9`.
fn access_key_id(prefix: &str) -> String {
    format!("{prefix}{}", "PDXSYNTH00000001")
}

/// A synthetic bearer token, long enough to be detected.
fn bearer_token() -> String {
    ["pdxSynth", "BearerToken", ".0001"].concat()
}

/// A synthetic credential value.
fn value(n: u32) -> String {
    format!("pdxsynth-value-{n:04}")
}

/// `bytes` in base64url without padding, as a JSON Web Token's segments are written.
fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
        }
    }
    out
}

/// A synthetic JSON Web Token: a JSON header, JSON claims and a signature, each tagged.
fn json_web_token() -> String {
    [
        base64url(br#"{"alg":"HS256","typ":"PDXSYNTH"}"#),
        base64url(br#"{"sub":"pdxsynth-0001"}"#),
        base64url(b"pdxsynth-signature-0001"),
    ]
    .join(".")
}

/// A synthetic GitHub token of `kind` (`p`, `o`, `u`, `s` or `r`): its prefix and its
/// body of 36 letters and digits.
fn github_token(kind: char) -> (String, String) {
    (format!("gh{kind}_"), format!("PDXSYNTH{}", "0".repeat(28)))
}

/// A synthetic fine-grained GitHub token: its prefix and its body, 22 letters and
/// digits, `_` and 59 more.
fn fine_grained_github_token() -> (String, String) {
    (
        ["github", "_pat_"].concat(),
        format!("PDXSYNTH{}_{}", "1".repeat(14), "2".repeat(59)),
    )
}

/// `source` normalised as `language`.
fn normalised(source: &str, language: &str) -> String {
    let mut bytes = source.as_bytes().to_vec();
    secrets::normalise_in_place(&mut bytes, language);
    String::from_utf8(bytes).expect("masking keeps UTF-8 valid")
}

/// `text` with every occurrence of each of `secrets` replaced by the mask, which is
/// what normalisation must produce when those are the values it should find.
fn with_masked(text: &str, secrets: &[&str]) -> String {
    let mut out = text.to_owned();
    for s in secrets {
        out = out.replace(s, &"X".repeat(s.len()));
    }
    out
}

fn assert_lengths_and_line_endings_kept(original: &[u8], normalised: &[u8]) {
    assert_eq!(normalised.len(), original.len());
    for (i, (a, b)) in original.iter().zip(normalised).enumerate() {
        if matches!(a, b'\r' | b'\n') || matches!(b, b'\r' | b'\n') {
            assert_eq!(a, b, "a line ending moved at {i}");
        } else if a != b {
            assert_eq!(
                *b, MASK,
                "byte {i} changed to something other than the mask"
            );
        }
    }
}

// --- the policy digest -----------------------------------------------------------

fn digest_of(config: &str) -> SecretPolicyDigest {
    let config = PdxConfig::parse(config, Path::new("pdx.toml")).expect("a valid config");
    SecretPolicyDigest::of(&config.secrets)
}

#[test]
fn secret_policy_digest_fixed_vector() {
    // sha256 of the canonical form, computed independently (Python `hashlib`, which
    // first reproduced detectors 1 to 4's values):
    //   {"detector_version":5,"patterns":["*.key","*.pem","*id_rsa*",".env*"]}
    assert_eq!(SECRET_DETECTOR_VERSION, 5);
    let default = SecretPolicyDigest::of(&PdxConfig::default().secrets);
    assert_eq!(
        default.to_string(),
        "6aa1dd655b7d45a69c1cecc0e83ca105fd01c098f79ede4e0fa43eb2d83f3b99"
    );
    // A repository adding patterns: the effective set is the default and its own,
    //   {"detector_version":5,"patterns":["*.key","*.p12","*.pem","*id_rsa*",".env*",
    //    "config/credentials.yml"]}
    // Detector 4's was 92d35bc7…3be7, detector 3's e4eeb594…75d0, detector 2's
    // 76c75e1c…6bf3.
    let custom = digest_of("[secrets]\npatterns = [\"config/credentials.yml\", \"*.p12\"]\n");
    assert_eq!(
        custom.to_string(),
        "79ad82419b2bb1d6b67721a4c578aa4cc6b65909d2f65b88245e14ec59d0eba7"
    );
    // Order and repetition mean nothing, and a default restated changes nothing.
    assert_eq!(
        digest_of(
            "[secrets]\npatterns = [\"*.p12\", \"config/credentials.yml\", \"*.p12\", \"*.pem\"]\n"
        ),
        custom
    );
    assert_eq!(digest_of("[secrets]\npatterns = [\"*.pem\"]\n"), default);
}

#[test]
fn detector_version_enters_secret_policy_digest() {
    // The same patterns under every earlier detector version, each a policy builds
    // were made under, computed independently: version 1, before issue 44; 2, before
    // issue 72; 3, before issue 73; 4, before issue 96.
    //   {"detector_version":1,"patterns":["*.key","*.pem","*id_rsa*",".env*"]}
    //   … and so on to 4.
    let patterns = PdxConfig::default().secrets.patterns;
    let versions: Vec<SecretPolicyDigest> = (1..=5)
        .map(|v| SecretPolicyDigest::for_policy(v, &patterns))
        .collect();
    let (v1, v2, v3, v4, v5) = (
        versions[0],
        versions[1],
        versions[2],
        versions[3],
        versions[4],
    );
    assert_eq!(v5, SecretPolicyDigest::of(&PdxConfig::default().secrets));
    assert_eq!(
        v1.to_string(),
        "b9e1601f738d0ef897c6d316535f86fb31ecae8f6ab50676678a91209dd21b0c"
    );
    assert_eq!(
        v2.to_string(),
        "2d31e6c1829c821c29d11db44835995830ea3df0819a22b3736ffbf071ef07e8"
    );
    assert_eq!(
        v3.to_string(),
        "de4ad6db03d6d5ed4acb6db7119afc5cc7f54db7e434df08f0ab5f26fe487776"
    );
    assert_eq!(
        v4.to_string(),
        "05f97f160819d54c5aba9054efa8af77190d043feb6f03baec871b2b0c9d6904"
    );
    // Every version its own policy: an extraction cached under any earlier detector
    // is a miss.
    for (i, a) in versions.iter().enumerate() {
        for b in &versions[i + 1..] {
            assert_ne!(a, b);
        }
    }
    // And no pattern at all is a policy too, distinct from the default's.
    assert_ne!(SecretPolicyDigest::for_policy(1, &BTreeSet::new()), v1);
}

// --- the detectors -----------------------------------------------------------------

#[test]
fn private_keys_lose_their_material_and_keep_their_markers() {
    for label in PRIVATE_KEY_LABELS {
        for eol in ["\n", "\r\n"] {
            let block = key_block(label, &KEY_BODY, eol);
            let source = format!("key: |{eol}{}", block.replace(eol, &format!("{eol}  ")));
            let out = normalised(&source, "yaml");
            assert_eq!(out, with_masked(&source, &KEY_BODY), "{label} {eol:?}");
            assert!(out.contains(&format!("-----BEGIN {label}-----")));
            assert!(out.contains(&format!("-----END {label}-----")));
        }
    }
    // Escaped in a string literal: the escapes stay, the material goes.
    let (begin, end) = (
        format!("-----BEGIN {}-----", PRIVATE_KEY_LABELS[1]),
        format!("-----END {}-----", PRIVATE_KEY_LABELS[1]),
    );
    let source = format!(
        "KEY = \"{begin}\\n{}\\n{}\\n{end}\\n\"\n",
        KEY_BODY[0], KEY_BODY[1]
    );
    assert_eq!(
        normalised(&source, "python"),
        with_masked(&source, &KEY_BODY)
    );
    // A marker of another kind of key does not end this one.
    let other = key_block(PRIVATE_KEY_LABELS[0], &KEY_BODY, "\n").replace(
        &format!("-----END {}-----", PRIVATE_KEY_LABELS[0]),
        &format!("-----END {}-----", PRIVATE_KEY_LABELS[2]),
    );
    assert!(!normalised(&other, "yaml").contains(KEY_BODY[1]));
}

#[test]
fn bearer_tokens_are_masked_and_the_word_is_kept() {
    let token = bearer_token();
    for scheme in ["Bearer", "bearer", "BEARER"] {
        let source = format!("headers = {{\"Authorization\": \"{scheme} {token}\"}}\n");
        assert_eq!(
            normalised(&source, "python"),
            with_masked(&source, &[&token]),
            "{scheme}"
        );
    }
    // Words after `Bearer` in code and prose are not tokens.
    for source in [
        "class Bearer extends Base {}\n",
        "// a Bearer token goes here\n",
        "const auth = `Bearer ${token}`;\n",
    ] {
        assert_eq!(normalised(source, "typescript"), source);
    }
}

#[test]
fn bearer_masking_is_idempotent_after_padding() {
    // The case the offsets property found (issue 44): a token, `=`, then more token
    // bytes. Padding must not end the token, or the bytes after it reach the engine
    // and a second pass, reading the masked `=` as a token byte, masks them then.
    let token = bearer_token();
    for tail in ["=A", "==abc", "=", "=a=b"] {
        let source = format!("Authorization: Bearer {token}{tail}\n");
        let once = normalised(&source, "yaml");
        assert_eq!(
            once,
            with_masked(&source, &[&format!("{token}{tail}")]),
            "{tail}"
        );
        assert_eq!(normalised(&once, "yaml"), once, "{tail}");
    }
    // What follows the run is left alone.
    let source = format!("Authorization: Bearer {token}= next\n");
    assert_eq!(
        normalised(&source, "yaml"),
        with_masked(&source, &[&format!("{token}=")])
    );
}

#[test]
fn credential_assignments_mask_the_value_only() {
    let v = |n| value(n);
    // Source code: quoted values, whatever the separator.
    let source = format!(
        "API_KEY = \"{}\"\nconfig = {{'client_secret': '{}'}}\nauth := Auth{{Password: \"{}\"}}\nlet {} = \"{}\";\n$opts = ['passwd' => '{}'];\n",
        v(1),
        v(2),
        v(3),
        CREDENTIAL_KEYS[4],
        v(4),
        v(5),
    );
    let all = [v(1), v(2), v(3), v(4), v(5)];
    let refs: Vec<&str> = all.iter().map(String::as_str).collect();
    assert_eq!(normalised(&source, "python"), with_masked(&source, &refs));

    // YAML: quoted and bare.
    let yaml = format!(
        "database:\n  password: {}\n  secret_key: \"{}\"\n  user: admin\n",
        v(6),
        v(7)
    );
    assert_eq!(
        normalised(&yaml, "yaml"),
        with_masked(&yaml, &[&v(6), &v(7)])
    );

    // JSON.
    let json = format!(
        "{{\"accessKey\": \"{}\", \"apiKey\" : \"{}\", \"name\": \"svc\"}}\n",
        v(8),
        v(9)
    );
    assert_eq!(
        normalised(&json, "json"),
        with_masked(&json, &[&v(8), &v(9)])
    );

    // Properties and INI-style.
    let properties = format!(
        "spring.datasource.password={}\naws.secret-key = {}\nhost=localhost\n",
        v(10),
        v(11)
    );
    assert_eq!(
        normalised(&properties, "properties"),
        with_masked(&properties, &[&v(10), &v(11)])
    );
}

#[test]
fn what_is_not_a_literal_credential_is_left_alone() {
    for (source, language) in [
        // A name, a call or an expression in code is not a secret.
        ("password = current_password\n", "python"),
        ("token = os.environ[\"TOKEN\"]\n", "python"),
        ("const secret = await vault.read(path);\n", "typescript"),
        // Comparisons and keys that only contain a credential word.
        ("if password == \"\": pass\n", "python"),
        ("password_hint: remember\ntokenizer: basic\n", "yaml"),
        // References to a secret, not secrets.
        ("password: ${DB_PASSWORD}\n", "yaml"),
        ("password: \"{{ .Values.db.password }}\"\n", "yaml"),
        ("password=$DB_PASSWORD\n", "properties"),
        // An empty value.
        ("password: \"\"\n", "yaml"),
    ] {
        assert_eq!(normalised(source, language), source, "{language}: {source}");
    }
}

#[test]
fn uri_passwords_are_masked_and_the_rest_kept() {
    let scheme = "postgres";
    let password = value(12);
    let source = format!("url = \"{scheme}://svc_user:{password}@db.internal:5432/app\"\n");
    let out = normalised(&source, "python");
    assert_eq!(out, with_masked(&source, &[&password]));
    assert!(out.contains("svc_user:") && out.contains("@db.internal:5432/app"));
    // No user information, nothing to mask.
    let plain = format!("url = \"{scheme}://db.internal:5432/app\"\n");
    assert_eq!(normalised(&plain, "python"), plain);
    // A password that is wholly a placeholder is a reference, not a secret.
    let reference = format!("url = \"{scheme}://svc_user:${{DB_PASSWORD}}@db.internal/app\"\n");
    assert_eq!(normalised(&reference, "python"), reference);
}

/// The range of the one occurrence of `needle` in `haystack`.
fn range_of(haystack: &str, needle: &str) -> std::ops::Range<usize> {
    assert_eq!(
        haystack.matches(needle).count(),
        1,
        "{needle} in {haystack}"
    );
    let start = haystack.find(needle).expect("present");
    start..start + needle.len()
}

#[test]
fn bearer_adjacent_uri_password_is_idempotent() {
    // The minimal input of CI run 38098740924, attempt 1 (issue 72), as YAML:
    // `Bearer <token>_mysql://u:<password>@h`. `_` is a bearer-token byte, so the token
    // runs on over `_mysql`; `_` is no scheme byte, so `mysql` is a scheme. Under
    // detector 2 the URI detector missed it (a word boundary is not where a scheme
    // starts), and masking the token made `XXXX…://` a URI a second pass then found.
    let token = format!("{}_mysql", bearer_token());
    let password = value(22);
    let source = format!("Authorization: Bearer {token}://u:{password}@h");
    let bytes = source.as_bytes();

    // Detection first: on the original bytes, each detector by itself finds its own.
    assert_eq!(
        secrets::secret_ranges_with(&[Detector::BearerToken], bytes, "yaml"),
        vec![range_of(&source, &token)]
    );
    assert_eq!(
        secrets::secret_ranges_with(&[Detector::UriPassword], bytes, "yaml"),
        vec![range_of(&source, &password)]
    );
    assert_eq!(
        secrets::secret_ranges(bytes, "yaml"),
        vec![range_of(&source, &token), range_of(&source, &password)]
    );

    // One pass masks both secrets and keeps the scheme separator, the user, `:`, `@`
    // and the host.
    let once = normalised(&source, "yaml");
    assert_eq!(
        once,
        format!(
            "Authorization: Bearer {}://u:{}@h",
            "X".repeat(token.len()),
            "X".repeat(password.len())
        )
    );
    assert!(!once.contains(&bearer_token()) && !once.contains(&password));
    assert_lengths_and_line_endings_kept(bytes, once.as_bytes());
    // A second pass changes nothing.
    assert_eq!(normalised(&once, "yaml"), once);

    // The same with a line ending after it, in every language the property uses.
    for eol in ["\n", "\r\n", "\r"] {
        for language in ["yaml", "python", "json", "properties"] {
            let source = format!("{source}{eol}");
            let once = normalised(&source, language);
            assert_eq!(
                once,
                with_masked(&source, &[&token, &password]),
                "{language} {eol:?}"
            );
            assert_lengths_and_line_endings_kept(source.as_bytes(), once.as_bytes());
            assert_eq!(normalised(&once, language), once, "{language} {eol:?}");
        }
    }
}

#[test]
fn uri_password_boundary_is_security_conservative() {
    let password = value(40);
    // A scheme (a letter, then letters, digits, `+`, `-` and `.`) is recognised at the
    // start of the input or right after any byte that is not an ASCII letter or digit
    // (decision 43): punctuation of every kind, `_`, and bytes outside ASCII.
    for before in [
        "", " ", "\t", "\n", "_", "-", "+", ".", "/", ":", "=", "~", "\"", "'", "(", "[", ",", "@",
        "1.", "/foo.", "\u{e9}",
    ] {
        for scheme in [
            "mysql", "postgres", "https", "foo+bar", "foo.bar", "foo-bar", "s3", "a1b2",
        ] {
            let source = format!("{before}{scheme}://u:{password}@h\n");
            assert_eq!(
                secrets::secret_ranges_with(&[Detector::UriPassword], source.as_bytes(), "yaml"),
                vec![range_of(&source, &password)],
                "{source:?}"
            );
            let once = normalised(&source, "python");
            assert_eq!(once, with_masked(&source, &[&password]), "{source:?}");
            assert_eq!(normalised(&once, "python"), once, "{source:?}");
        }
    }

    // The byte before `mysql` alone decides when nothing earlier can start a scheme:
    // after a leading `9` none can, so `9<byte>mysql://…` is a URI password exactly
    // when the byte is no ASCII letter or digit.
    let delimiters: [(&str, &[u8], bool); 15] = [
        ("start", b"", true),
        ("ASCII letter", b"9x", false),
        ("ASCII digit", b"91", false),
        ("_", b"9_", true),
        ("-", b"9-", true),
        ("+", b"9+", true),
        (".", b"9.", true),
        ("/", b"9/", true),
        ("space", b"9 ", true),
        ("quote", b"9\"", true),
        ("colon", b"9:", true),
        ("equals", b"9=", true),
        ("tilde", b"9~", true),
        ("non-ASCII", b"9\xff", true),
        ("letters and a digit", b"0abc9", false),
    ];
    for (category, prefix, masks) in delimiters {
        let source = [prefix, b"mysql://u:", password.as_bytes(), b"@h\n"].concat();
        let at = prefix.len() + "mysql://u:".len();
        let found = secrets::secret_ranges_with(&[Detector::UriPassword], &source, "yaml");
        let expected: Vec<_> = masks
            .then_some(at..at + password.len())
            .into_iter()
            .collect();
        assert_eq!(found, expected, "{category}");
        let mut once = source.clone();
        secrets::normalise_in_place(&mut once, "yaml");
        let mut again = once.clone();
        secrets::normalise_in_place(&mut again, "yaml");
        assert_eq!(again, once, "{category}");
    }

    // A scheme right after the `@` that ends another URI's match: the byte before it
    // is no letter or digit, though the other match took it.
    let (first, second) = (value(41), value(42));
    let source = format!("a://u:{first}@b://u:{second}@h\n");
    assert_eq!(
        normalised(&source, "python"),
        with_masked(&source, &[&first, &second])
    );

    // Not a URI password: no `://`, no `@`, no `:` between user and password, no
    // scheme or one that would start with a digit or a sign, or a putative `mysql`
    // right after a letter or digit with no letter-led scheme before it. (At the
    // start of the input, `xmysql://` and `abc9mysql://` are schemes of their own,
    // masked as detector 2 masked them.)
    for source in [
        format!("mysql:/u:{password}@h\n"),
        format!("mysql//u:{password}@h\n"),
        format!("mysql://u:{password}\n"),
        format!("mysql://u{password}@h\n"),
        format!("://u:{password}@h\n"),
        format!(" ://u:{password}@h\n"),
        format!("9://u:{password}@h\n"),
        format!("-://u:{password}@h\n"),
        format!("9mysql://u:{password}@h\n"),
        format!("9xmysql://u:{password}@h\n"),
        format!("0abc9mysql://u:{password}@h\n"),
    ] {
        assert_eq!(normalised(&source, "python"), source, "{source:?}");
    }
}

/// Detector 2's URI password pattern and its placeholder exemption, kept only as an
/// oracle: what detector 2 found, which the detector must still find (issue 73). It is
/// never used to work out what the detector itself finds.
fn detector_two_uri_passwords(bytes: &[u8]) -> Vec<std::ops::Range<usize>> {
    static PATTERN: std::sync::LazyLock<regex::bytes::Regex> = std::sync::LazyLock::new(|| {
        regex::bytes::Regex::new(
            r#"(?-u)(?i)\b[a-z][a-z0-9+.\-]*://[^\s/?#@"'<>:]*:([^\s/?#@"'<>]+)@"#,
        )
        .expect("detector 2's pattern")
    });
    static PLACEHOLDER: std::sync::LazyLock<regex::bytes::Regex> = std::sync::LazyLock::new(|| {
        regex::bytes::Regex::new(r"(?-u)\A(?:\$\{[^}]*\}|\$[A-Za-z_][A-Za-z0-9_]*|\{\{[^}]*\}\})\z")
            .expect("detector 2's placeholder")
    });
    PATTERN
        .captures_iter(bytes)
        .filter_map(|c| c.get(1))
        .filter(|m| !PLACEHOLDER.is_match(m.as_bytes()))
        .map(|m| m.range())
        .collect()
}

/// The URI password ranges the detector finds.
fn uri_passwords(bytes: &[u8]) -> Vec<std::ops::Range<usize>> {
    secrets::secret_ranges_with(&[Detector::UriPassword], bytes, "yaml")
}

/// The ranges of `old` that `new` does not have.
fn lost(
    old: &[std::ops::Range<usize>],
    new: &[std::ops::Range<usize>],
) -> Vec<std::ops::Range<usize>> {
    old.iter().filter(|r| !new.contains(r)).cloned().collect()
}

#[test]
fn uri_detector_preserves_detector_two_coverage() {
    let password = value(70);
    let token = bearer_token();
    // Detector 2 found these and detector 3 lost them: a scheme after punctuation that
    // can also sit in a scheme. They must never be lost again.
    for before in ["-", "+", ".", "1.", "/", ":", " ", "/foo.", ""] {
        let source = format!("{before}mysql://u:{password}@h\n");
        let old = detector_two_uri_passwords(source.as_bytes());
        assert_eq!(old, vec![range_of(&source, &password)], "{source:?}");
        assert_eq!(uri_passwords(source.as_bytes()), old, "{source:?}");
        assert_eq!(
            normalised(&source, "yaml"),
            with_masked(&source, &[&password]),
            "{source:?}"
        );
    }
    // Detector 3's additions, kept: `_` before a scheme, and a bearer token before
    // `://` (issue 72), each masked where detector 2 may not have found it.
    let digits = "0".repeat(16);
    for source in [
        format!("_mysql://u:{password}@h\n"),
        format!("Authorization: Bearer {token}_mysql://u:{password}@h\n"),
        format!("Authorization: Bearer {token}_9mysql://u:{password}@h\n"),
        format!("Authorization: Bearer {token}_-mysql://u:{password}@h\n"),
        format!("Authorization: Bearer {token}_://u:{password}@h\n"),
        format!("Authorization: Bearer {token}=://u:{password}@h\n"),
        format!("Authorization: Bearer {digits}://u:{password}@h\n"),
    ] {
        let new = uri_passwords(source.as_bytes());
        assert_eq!(new, vec![range_of(&source, &password)], "{source:?}");
        assert!(lost(&detector_two_uri_passwords(source.as_bytes()), &new).is_empty());
    }
    // Every byte before a scheme, after nothing, after a digit and after a letter:
    // nothing detector 2 found is lost.
    let mut cases = 0;
    for prefix in [&b""[..], b"9", b"ab", b"x."] {
        for byte in 0..=u8::MAX {
            let source = [prefix, &[byte], b"mysql://u:", password.as_bytes(), b"@h"].concat();
            let old = detector_two_uri_passwords(&source);
            assert!(
                lost(&old, &uri_passwords(&source)).is_empty(),
                "{:?}",
                String::from_utf8_lossy(&source)
            );
            cases += 1;
        }
    }
    assert_eq!(cases, 4 * 256);
}

/// A source in which two detectors' secrets meet: its language, the two detectors and
/// the secrets planted in it.
struct Meeting {
    source: String,
    language: &'static str,
    detectors: [Detector; 2],
    planted: Vec<String>,
}

/// Sources that put two detectors' secrets against each other.
fn meetings() -> Vec<Meeting> {
    use Detector::{BearerToken, CloudAccessKeyId, CredentialAssignment, UriPassword};
    let (token, id, password) = (bearer_token(), access_key_id("ASIA"), value(50));
    let digits = "0".repeat(16);
    let meet = |source: String, language, detectors, planted: &[&str]| Meeting {
        source,
        language,
        detectors,
        planted: planted.iter().map(|s| (*s).to_owned()).collect(),
    };
    let bearer_uri = [BearerToken, UriPassword];
    vec![
        // A bearer token running into `://`: masked, the token is a scheme, so the
        // password after it is found on the original bytes, whatever the token ends
        // with (issue 72).
        meet(
            format!("Authorization: Bearer {token}_mysql://u:{password}@h\n"),
            "yaml",
            bearer_uri,
            &[&token, &password],
        ),
        meet(
            format!("Authorization: Bearer {token}_9mysql://u:{password}@h\n"),
            "yaml",
            bearer_uri,
            &[&token, &password],
        ),
        meet(
            format!("Authorization: Bearer {token}_-mysql://u:{password}@h\r\n"),
            "json",
            bearer_uri,
            &[&token, &password],
        ),
        meet(
            format!("Authorization: Bearer {token}_://u:{password}@h\n"),
            "python",
            bearer_uri,
            &[&token, &password],
        ),
        meet(
            format!("Authorization: Bearer {token}=://u:{password}@h\n"),
            "properties",
            bearer_uri,
            &[&token, &password],
        ),
        meet(
            format!("Authorization: bearer\t{token}~1mysql://u:{password}@h\n"),
            "yaml",
            bearer_uri,
            &[&token, &password],
        ),
        meet(
            format!("Authorization: Bearer {digits}://u:{password}@h\n"),
            "yaml",
            bearer_uri,
            &[&digits, &password],
        ),
        // A credential assignment whose value is a bearer header.
        meet(
            format!("token: \"Bearer {token}\"\n"),
            "yaml",
            [CredentialAssignment, BearerToken],
            &[&token],
        ),
        meet(
            format!("auth_token=Bearer {token}\n"),
            "properties",
            [CredentialAssignment, BearerToken],
            &[&token],
        ),
        // A credential assignment whose value is a URI with a password.
        meet(
            format!("password: mysql://u:{password}@h\n"),
            "yaml",
            [CredentialAssignment, UriPassword],
            &[&password],
        ),
        meet(
            format!("db_password = \"postgres://u:{password}@h/app\"\n"),
            "python",
            [CredentialAssignment, UriPassword],
            &[&password],
        ),
        // An access key id as a URI's user, and as what stands before `://`.
        meet(
            format!("url: s3://{id}:{password}@bucket\n"),
            "yaml",
            [CloudAccessKeyId, UriPassword],
            &[&id, &password],
        ),
        meet(
            format!("{id}://u:{password}@h\n"),
            "properties",
            [CloudAccessKeyId, UriPassword],
            &[&id, &password],
        ),
    ]
    .into_iter()
    .chain(issue_96_meetings())
    .collect()
}

/// Sources that put issue 96's detectors' secrets against the others'.
fn issue_96_meetings() -> Vec<Meeting> {
    use Detector::{BearerToken, CredentialAssignment, GitHubToken, JsonWebToken, UriPassword};
    let password = value(50);
    let (jwt, (gh_prefix, gh_body)) = (json_web_token(), github_token('p'));
    let meet = |source: String, language, detectors, planted: &[&str]| Meeting {
        source,
        language,
        detectors,
        planted: planted.iter().map(|s| (*s).to_owned()).collect(),
    };
    vec![
        // A bare value whose URI password holds a delimiter: the value runs on over the
        // password, as it would over its mask (issue 96).
        meet(
            format!("{}: s://u:{password};x@h\n", CREDENTIAL_KEYS[0]),
            "yaml",
            [CredentialAssignment, UriPassword],
            &[&password, "s://u:", ";x@h"],
        ),
        // A JSON Web Token as a bearer token, as a value and after a GitHub prefix.
        meet(
            format!("Authorization: Bearer {jwt}\n"),
            "yaml",
            [JsonWebToken, BearerToken],
            &[&jwt],
        ),
        meet(
            format!("{} = \"{jwt}\"\n", CREDENTIAL_KEYS[4]),
            "python",
            [JsonWebToken, CredentialAssignment],
            &[&jwt],
        ),
        meet(
            format!("{gh_prefix}{jwt}\n"),
            "markdown",
            [GitHubToken, JsonWebToken],
            &[&jwt],
        ),
        // A GitHub token as a value and as a URI's password.
        meet(
            format!("{}: {gh_prefix}{gh_body}\n", CREDENTIAL_KEYS[4]),
            "yaml",
            [GitHubToken, CredentialAssignment],
            &[&gh_body],
        ),
        meet(
            format!("remote: s://x:{gh_prefix}{gh_body}@git.example/r\n"),
            "yaml",
            [GitHubToken, UriPassword],
            &[&gh_body],
        ),
    ]
}

#[test]
fn cross_detector_boundaries_are_idempotent() {
    // Each source puts two detectors' secrets against each other; every planted secret
    // must be gone after one pass, which a second pass then leaves alone.
    for Meeting {
        source,
        language,
        detectors,
        planted,
    } in &meetings()
    {
        let language = *language;
        let bytes = source.as_bytes();
        for detector in detectors {
            assert!(
                !secrets::secret_ranges_with(&[*detector], bytes, language).is_empty(),
                "{detector:?} finds nothing in {source:?}"
            );
        }
        // The merged ranges are the same in every order of the seven detectors.
        let expected = secrets::secret_ranges(bytes, language);
        let mut order = Detector::ALL.to_vec();
        permute(&mut order, 0, &mut |order| {
            assert_eq!(
                secrets::secret_ranges_with(order, bytes, language),
                expected,
                "{order:?} {source:?}"
            );
        });
        let once = normalised(source, language);
        for secret in planted {
            assert!(!once.contains(secret), "{secret} survived in {once:?}");
        }
        assert_lengths_and_line_endings_kept(bytes, once.as_bytes());
        assert_eq!(normalised(&once, language), once, "{source:?}");
    }
    // The bearer clause is the bearer detector's shape exactly: no bearer token (too
    // short, or `bearer` inside a word), no clause, and nothing changes on either pass.
    let (token, password) = (bearer_token(), value(50));
    for source in [
        format!("Authorization: Bearer short_9mysql://u:{password}@h\n"),
        format!("xBearer {token}_9mysql://u:{password}@h\n"),
    ] {
        assert_eq!(normalised(&source, "yaml"), source, "{source:?}");
    }
}

#[test]
fn uri_detector_covers_detector_two_on_the_corpus() {
    // What detector 2 found in ordinary sources and in the sanitizer corpus, the
    // detector still finds: it may find more, never less (issue 73).
    let p = value(60);
    // The web schemes assembled at run time, as the provenance scan requires.
    let (web, plain) = (["ht", "tps"].concat(), ["ht", "tp"].concat());
    let ordinary = [
        format!("url = \"postgres://svc_user:{p}@db.internal:5432/app\"\n"),
        format!("DATABASE_URL=mysql://root:{p}@localhost/db\n"),
        format!("{{\"dsn\": \"redis://:{p}@cache:6379/0\"}}\n"),
        format!("jdbc:mysql://app:{p}@db/x\n"),
        format!("git+ssh://deploy:{p}@git.example/repo.git\n"),
        format!("see {web}://user:{p}@git.example/a and ftp://u:{p}@files\n"),
        format!("(amqp://guest:{p}@broker)\n"),
        format!("mongodb+srv://u:{p}@cluster/db?retryWrites=true\n"),
        format!("a://u:{p}@b://u:{p}@h\n"),
        format!("{plain}://git.example/path\n"),
    ];
    for source in &ordinary {
        let old = detector_two_uri_passwords(source.as_bytes());
        assert!(
            lost(&old, &uri_passwords(source.as_bytes())).is_empty(),
            "{source:?}"
        );
    }
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/corpus");
    let (mut files, mut old_ranges, mut new_ranges, mut lost_ranges, mut added) = (0, 0, 0, 0, 0);
    for language in std::fs::read_dir(&corpus).expect("the corpus") {
        for file in std::fs::read_dir(language.expect("an entry").path()).expect("a language") {
            let bytes = std::fs::read(file.expect("an entry").path()).expect("a file");
            let (old, new) = (detector_two_uri_passwords(&bytes), uri_passwords(&bytes));
            lost_ranges += lost(&old, &new).len();
            added += lost(&new, &old).len();
            old_ranges += old.len();
            new_ranges += new.len();
            files += 1;
        }
    }
    eprintln!(
        "bench/corpus: {files} files, detector 2 {old_ranges} URI password ranges, \
         detector {SECRET_DETECTOR_VERSION} {new_ranges}, lost {lost_ranges}, added {added}"
    );
    assert!(files > 50, "{files} corpus files");
    assert_eq!(lost_ranges, 0);
}

#[test]
fn cloud_access_key_ids_are_masked() {
    for prefix in ["AKIA", "ASIA"] {
        let id = access_key_id(prefix);
        let source = format!("aws_access_key_id = {id}\nregion = eu-west-1\n");
        assert_eq!(
            normalised(&source, "properties"),
            with_masked(&source, &[&id])
        );
        let in_code = format!("const ID = '{id}';\n");
        assert_eq!(
            normalised(&in_code, "javascript"),
            with_masked(&in_code, &[&id])
        );
    }
    // Wrong shape: too short, lower case, or part of a longer word.
    for source in [
        "AKIAPDXSYNTH0001\n".to_owned(),
        access_key_id("akia"),
        format!("X{}", access_key_id("AKIA")),
    ] {
        assert_eq!(normalised(&source, "properties"), source);
    }
}

#[test]
fn non_utf8_source_is_normalised_byte_for_byte() {
    let secret = value(13);
    let mut original = vec![0xff, 0xfe, b'\n'];
    original.extend_from_slice(format!("password: \"{secret}\"\r\n").as_bytes());
    original.extend_from_slice(&[0x80, 0xc3, 0x28, b'\n']);
    let mut bytes = original.clone();
    let masked = secrets::normalise_in_place(&mut bytes, "yaml");
    assert_eq!(masked, secret.len());
    assert_lengths_and_line_endings_kept(&original, &bytes);
    assert!(!bytes.windows(secret.len()).any(|w| w == secret.as_bytes()));
    assert_eq!(&bytes[..3], &original[..3]);
    assert_eq!(&bytes[bytes.len() - 4..], &original[original.len() - 4..]);
}

#[test]
fn secret_detector_overlap_is_order_independent() {
    // One quoted value holding a bearer token that is an access key id, and a URI
    // whose password is another: four detectors' ranges overlap.
    let id = access_key_id("AKIA");
    let scheme = "https";
    let source =
        format!("token = \"Bearer {id}\"\nendpoint = \"{scheme}://svc:{id}@api.internal/\"\n")
            .into_bytes();

    let single: Vec<Vec<std::ops::Range<usize>>> = Detector::ALL
        .iter()
        .map(|d| secrets::secret_ranges_with(&[*d], &source, "python"))
        .collect();
    let overlapping = |a: &[std::ops::Range<usize>], b: &[std::ops::Range<usize>]| {
        a.iter()
            .any(|x| b.iter().any(|y| x.start < y.end && y.start < x.end))
    };
    // Each detector by name, never by its place in `Detector::ALL`.
    let by = |d: Detector| secrets::secret_ranges_with(&[d], &source, "python");
    let (assignment, bearer, uri, cloud) = (
        by(Detector::CredentialAssignment),
        by(Detector::BearerToken),
        by(Detector::UriPassword),
        by(Detector::CloudAccessKeyId),
    );
    assert!(
        overlapping(&assignment, &bearer)
            && overlapping(&bearer, &cloud)
            && overlapping(&uri, &cloud)
    );

    let expected = secrets::secret_ranges(&source, "python");
    let mut expected_bytes = source.clone();
    secrets::normalise_in_place(&mut expected_bytes, "python");
    // Every order of the seven detectors.
    let mut order = Detector::ALL.to_vec();
    let mut orders = 0;
    permute(&mut order, 0, &mut |order| {
        orders += 1;
        assert_eq!(
            secrets::secret_ranges_with(order, &source, "python"),
            expected,
            "{order:?}"
        );
    });
    assert_eq!(orders, 5040);
    // The merged ranges are disjoint, sorted, and cover every single detector's.
    assert!(expected.windows(2).all(|w| w[0].end < w[1].start));
    for ranges in &single {
        for r in ranges {
            assert!(
                expected
                    .iter()
                    .any(|e| e.start <= r.start && r.end <= e.end)
            );
        }
    }
    assert!(!String::from_utf8_lossy(&expected_bytes).contains(&id));
}

fn permute(items: &mut Vec<Detector>, k: usize, visit: &mut dyn FnMut(&[Detector])) {
    if k == items.len() {
        visit(items);
        return;
    }
    for i in k..items.len() {
        items.swap(k, i);
        permute(items, k + 1, visit);
        items.swap(k, i);
    }
}

/// Bytes a bearer token may hold, every one of its punctuation bytes among them: `_`,
/// `~`, `/` and `=` hold no scheme, digits and `+ - .` start none.
const TOKEN_BYTES: &[u8] = b"aZ09-._~+/=";

/// Source made of pieces, some of them secrets, joined by line endings of either kind.
/// Short runs of bearer-token bytes and a URI without a scheme let a secret's piece run
/// into another's, as issue 72's did.
fn pieces() -> impl Strategy<Value = Vec<u8>> {
    let piece = prop_oneof![
        proptest::collection::vec(any::<u8>(), 0..24),
        proptest::collection::vec(prop::sample::select(TOKEN_BYTES.to_vec()), 1..4),
        Just(format!("://u:{}@h", value(23)).into_bytes()),
        Just(format!("password: \"{}\"", value(20)).into_bytes()),
        Just(format!("{}: {}", CREDENTIAL_KEYS[13], value(21)).into_bytes()),
        Just(format!("Authorization: Bearer {}", bearer_token()).into_bytes()),
        Just(access_key_id("ASIA").into_bytes()),
        Just(key_block(PRIVATE_KEY_LABELS[4], &KEY_BODY, "\r\n").into_bytes()),
        Just(format!("{}://u:{}@h", "mysql", value(22)).into_bytes()),
        Just(json_web_token().into_bytes()),
        Just(github_token('s').0.into_bytes()),
        Just(github_token('s').1.into_bytes()),
        Just(format!("\"Server=db;{}={};\"", CREDENTIAL_KEYS[2], value(24)).into_bytes()),
        Just(format!("\"{}=\" + f(p) + \"&x\"", CREDENTIAL_KEYS[0]).into_bytes()),
        Just(format!("'{{\"{}\": \"{}\"}}'", CREDENTIAL_KEYS[4], value(25)).into_bytes()),
        Just(format!("{}=", CREDENTIAL_KEYS[0]).into_bytes()),
        Just(b"don't".to_vec()),
        // Quotes, escapes, delimiters and apostrophes' neighbours, so that literals open,
        // close and fail to close around every other piece.
        proptest::collection::vec(prop::sample::select(QUOTING_BYTES.to_vec()), 1..6),
        Just(b"\r\n".to_vec()),
        Just(b"\n".to_vec()),
        Just(b"\r".to_vec()),
    ];
    proptest::collection::vec(piece, 0..12).prop_map(|p| p.concat())
}

/// Bytes that decide how literals read: every quote, a backslash, the delimiters a
/// value inside a literal ends at, a separator, and lower-case letters for apostrophes.
const QUOTING_BYTES: &[u8] = b"'\"`\\;&, =:{$atX";

proptest! {
    #[test]
    fn secret_normalisation_preserves_offsets(
        source in pieces(),
        language in prop::sample::select(vec![
            "yaml", "python", "json", "properties", "markdown", "bash", "javascript",
            "go", "java", "csharp", "rust", "c",
        ]),
    ) {
        let mut bytes = source.clone();
        let masked = secrets::normalise_in_place(&mut bytes, language);
        // The same length; every line ending where it was; every other byte either
        // itself or the mask; and as many masked as the count says.
        prop_assert_eq!(bytes.len(), source.len());
        for (a, b) in source.iter().zip(&bytes) {
            if matches!(a, b'\r' | b'\n') {
                prop_assert_eq!(a, b);
            } else {
                prop_assert!(a == b || *b == MASK);
            }
        }
        let changed = source.iter().zip(&bytes).filter(|(a, b)| a != b).count();
        prop_assert!(changed <= masked);
        // Masking what is already masked changes nothing more.
        let mut again = bytes.clone();
        secrets::normalise_in_place(&mut again, language);
        prop_assert_eq!(again, bytes);
    }
}

/// A connection-string-shaped piece: something before it, a scheme, `://`, a user,
/// `:`, a password, `@`, a host and something after. Every part may be malformed.
fn uri_shaped() -> impl Strategy<Value = Vec<u8>> {
    let around = prop_oneof![
        proptest::collection::vec(any::<u8>(), 0..6),
        proptest::collection::vec(prop::sample::select(b"aZ09-+._/:~ @".to_vec()), 0..4),
        Just(b"Bearer ".to_vec()),
        Just(format!("Bearer {}", bearer_token()).into_bytes()),
        Just(b"Bearer 0000000000000000".to_vec()),
    ];
    let scheme = prop_oneof![
        Just(b"mysql".to_vec()),
        proptest::collection::vec(prop::sample::select(b"aZm9+.-_~/".to_vec()), 0..6),
    ];
    let part = proptest::collection::vec(prop::sample::select(b"uZ09-._~%!$:@/ {}".to_vec()), 0..6);
    (around.clone(), scheme, part.clone(), part, around).prop_map(
        |(before, scheme, user, password, after)| {
            [
                before,
                scheme,
                b"://".to_vec(),
                user,
                b":".to_vec(),
                password,
                b"@h".to_vec(),
                after,
            ]
            .concat()
        },
    )
}

proptest! {
    #[test]
    fn uri_detector_never_loses_a_detector_two_password(
        pieces in proptest::collection::vec(uri_shaped(), 1..4),
    ) {
        // Detector 2's ranges come from its own pattern, never from the detector.
        let source = pieces.concat();
        let old = detector_two_uri_passwords(&source);
        let missing = lost(&old, &uri_passwords(&source));
        prop_assert!(missing.is_empty(), "{:?} lost {:?}", String::from_utf8_lossy(&source), missing);
    }
}

#[test]
fn secret_normalisation_preserves_offsets_in_a_mixed_file() {
    let original = format!(
        "{}password: \"{}\"\r\nauth: Bearer {}\n\nid: {}\r\n",
        key_block(PRIVATE_KEY_LABELS[3], &KEY_BODY, "\r\n"),
        value(30),
        bearer_token(),
        access_key_id("AKIA"),
    )
    .into_bytes();
    let mut bytes = original.clone();
    secrets::normalise_in_place(&mut bytes, "yaml");
    assert_lengths_and_line_endings_kept(&original, &bytes);
    let text = String::from_utf8(bytes).unwrap();
    for planted in [
        KEY_BODY[0],
        KEY_BODY[1],
        &value(30),
        &bearer_token(),
        &access_key_id("AKIA"),
    ] {
        assert!(!text.contains(planted), "{planted} survived");
    }
}

// --- issue 96 ------------------------------------------------------------------------

/// Asserts that `source`, as `language`, normalises to itself with exactly `secrets`
/// masked, keeps its length and line endings, and normalises no further.
fn assert_masks(source: &str, language: &str, secrets: &[&str]) {
    let once = normalised(source, language);
    assert_eq!(once, with_masked(source, secrets), "{language}: {source:?}");
    assert_lengths_and_line_endings_kept(source.as_bytes(), once.as_bytes());
    assert_eq!(
        normalised(&once, language),
        once,
        "{language}: {source:?} twice"
    );
}

#[test]
fn json_web_tokens_are_masked_wherever_they_stand() {
    let jwt = json_web_token();
    assert!(jwt.starts_with("eyJ") && jwt.matches('.').count() == 2);
    for (source, language) in [
        (format!("session = decode(\"{jwt}\")\n"), "python"),
        (format!("// sample: {jwt}\r\n"), "java"),
        (format!("{jwt}\n"), "markdown"),
        (
            format!("headers = {{ Authorization: 'Bearer {jwt}' }};\n"),
            "javascript",
        ),
        (format!("seen={jwt}.\n"), "properties"),
    ] {
        assert_masks(&source, language, &[&jwt]);
    }
    // An unsigned token keeps its empty third segment, and is still masked.
    let unsigned = jwt.rsplit_once('.').expect("three segments").0.to_owned() + ".";
    assert_masks(&format!("t({unsigned})\n"), "python", &[&unsigned]);
    // Not a token: one segment, segments shorter than 8 bytes, `eyJ` inside a word.
    let short = ["eyJabcd", "eyJefgh", "ij"].join(".");
    for source in [
        format!("{}\n", jwt.split('.').next().expect("a header")),
        format!("{short}\n"),
        format!("x{jwt}\n"),
    ] {
        assert_eq!(normalised(&source, "python"), source, "{source:?}");
    }
}

#[test]
fn github_tokens_are_masked_after_their_prefix() {
    for kind in ['p', 'o', 'u', 's', 'r'] {
        let (prefix, body) = github_token(kind);
        let token = format!("{prefix}{body}");
        for (source, language) in [
            (format!("client = Client(\"{token}\")\n"), "python"),
            (format!("# {token}\r\n"), "yaml"),
            (format!("run: echo {token}|login\n"), "yaml"),
            (format!("BUILD_{token}\n"), "properties"),
        ] {
            assert_masks(&source, language, &[&body]);
            assert!(normalised(&source, language).contains(&prefix));
        }
    }
    let (prefix, body) = fine_grained_github_token();
    assert_masks(
        &format!("const t = \"{prefix}{body}\";\n"),
        "javascript",
        &[&body],
    );
    // Wrong shape: a body too short, an unknown kind, a prefix inside a word.
    let (prefix, body) = github_token('p');
    for source in [
        format!("{prefix}{}\n", &body[..35]),
        format!("ghx_{body}\n"),
        format!("x{prefix}{body}\n"),
    ] {
        assert_eq!(normalised(&source, "python"), source, "{source:?}");
    }
}

#[test]
fn encrypted_and_pgp_private_keys_are_masked() {
    assert_eq!(
        PRIVATE_KEY_LABELS[5..],
        ["ENCRYPTED PRIVATE KEY", "PGP PRIVATE KEY BLOCK"]
    );
    // PGP armour carries headers and a checksum line: all of it between the markers goes,
    // but for the punctuation and blanks.
    let block = key_block(
        PRIVATE_KEY_LABELS[6],
        &["Version: PDXSYNTH 1", "", KEY_BODY[0], "=PDXS"],
        "\n",
    );
    let out = normalised(&block, "markdown");
    assert_lengths_and_line_endings_kept(block.as_bytes(), out.as_bytes());
    assert!(out.contains("XXXXXXX: XXXXXXXX X\n\n"), "{out}");
    // The checksum line's `=` is a base64 byte too.
    assert!(out.contains("\nXXXXX\n-----END"), "{out}");
    assert!(!out.contains(KEY_BODY[0]));
    assert!(out.starts_with(&format!("-----BEGIN {}-----\n", PRIVATE_KEY_LABELS[6])));
    assert!(out.ends_with(&format!("-----END {}-----\n", PRIVATE_KEY_LABELS[6])));
}

#[test]
fn connection_string_passwords_are_masked() {
    let v = value(80);
    let (password, pwd) = (CREDENTIAL_KEYS[0], CREDENTIAL_KEYS[2]);
    let cased = "Password";
    for (source, language) in [
        // Issue 96's class 3: a C# connection string ending CRLF.
        (
            format!("var cs = \"Server=db;User Id=sa;{cased}={v};\";\r\n"),
            "csharp",
        ),
        (
            format!("String url = \"jdbc:sqlserver://db;user=sa;{password}={v}\";\n"),
            "java",
        ),
        (
            format!("{{\"Db\": \"Data Source=x; {cased} = {v}; Timeout=3\"}}\n"),
            "json",
        ),
        (
            format!("dsn = 'host=db user=app {password}={v} sslmode=require'\n"),
            "python",
        ),
        (
            format!("auto c = \"Driver={{X}};Uid=app;{pwd}={v};\";\n"),
            "cpp",
        ),
        (format!("cs = \"Server=db;{cased}='{v}';\"\n"), "csharp"),
        (format!("q = \"?user=a&{password}={v}&mode=x\"\n"), "python"),
    ] {
        assert_masks(&source, language, &[&v]);
    }
    // References, formats, parameters and interpolations inside a literal are code.
    for (source, language) in [
        (
            format!("cs = \"Server=db;{cased}=${{DB_PW}};\";\n"),
            "csharp",
        ),
        (format!("cs = \"Server=db;{cased}=%s;\" % pw\n"), "python"),
        (format!("cs = f\"Server=db;{cased}={{pw}};\"\n"), "python"),
        (format!("cs = $\"Server=db;{cased}={{pw}};\";\n"), "csharp"),
        (
            format!("q = \"SELECT 1 WHERE {password}=? AND id=:id\"\n"),
            "python",
        ),
        (
            format!("q = \"UPDATE u SET {password}=crypt($1) WHERE id=1\"\n"),
            "python",
        ),
        (format!("s = \"{password}=#{{pw}};\"\n"), "ruby"),
    ] {
        assert_eq!(
            normalised(&source, language),
            source,
            "{language}: {source:?}"
        );
    }
}

#[test]
fn a_literal_ending_at_its_separator_keeps_the_code_after_it() {
    // Issue 96's class 4, the destructive mask: the quote after the separator closes
    // the key's own literal, so what follows it is code, never a value. Detector 4 read
    // that quote as a value's opening one and masked ` + decrypt(pw) + `.
    let (password, token) = (CREDENTIAL_KEYS[0], CREDENTIAL_KEYS[4]);
    for (source, language) in [
        (
            format!("url = \"{password}=\" + decrypt(pw) + \"&mode=x\"\n"),
            "python",
        ),
        (
            format!("const q = '?user=a&{password}=' + encode(p) + '&x=1';\n"),
            "javascript",
        ),
        (format!("$q = \"{token}: \" . $p . \"!\";\n"), "php"),
        (
            format!("String s = \"{password}=\" + read(p) + \";\";\r\n"),
            "java",
        ),
        (
            format!("cs = \"Server=db;{password}=\" + pw + \";\";\n"),
            "csharp",
        ),
        (
            format!("curl -d \"{password}=\"$(read_pw)\" -X\"\n"),
            "bash",
        ),
    ] {
        assert_eq!(
            normalised(&source, language),
            source,
            "{language}: {source:?}"
        );
    }
    // A value in the literal before the code is still masked, and one in a literal of
    // its own after the code too.
    let (v, w) = (value(81), value(82));
    assert_masks(
        &format!("url = \"{password}={v}&mode=\" + m + \"&{token}={w}\"\n"),
        "python",
        &[&v, &w],
    );
}

#[test]
fn values_stay_inside_their_literal() {
    let (password, v) = (CREDENTIAL_KEYS[0], value(83));
    for (source, language) in [
        (format!("body = '{{\"{password}\": \"{v}\"}}'\n"), "python"),
        (format!("body = \"{{'{password}': '{v}'}}\"\n"), "python"),
        (
            format!("db.exec(\"UPDATE u SET {password}='{v}' WHERE id=1\");\n"),
            "java",
        ),
        (
            format!("args = [\"--{password}={v}\", \"--verbose\"]\n"),
            "python",
        ),
        (format!("cmd: \"login {password}={v}\"\n"), "yaml"),
    ] {
        assert_masks(&source, language, &[&v]);
    }
    // A literal of the other quote that would close past this literal's end is none.
    let source = format!("s = '{password}: \"' + v + '\"'\n");
    assert_eq!(normalised(&source, "python"), source);
    // After `:`, a literal's bytes in code are a name a contract may read (a channel, a
    // key) or prose, never a value.
    let token = CREDENTIAL_KEYS[4];
    for (source, language) in [
        (
            format!("bus.publish(\"auth:{token}:refresh\", e);\n"),
            "java",
        ),
        (
            format!("redis.get(`session:{token}:${{id}}`);\n"),
            "typescript",
        ),
        (format!("print(\"{password}: wrong\")\n"), "python"),
    ] {
        assert_eq!(
            normalised(&source, language),
            source,
            "{language}: {source:?}"
        );
    }
}

#[test]
fn apostrophes_are_no_quotes() {
    let (password, token, v) = (CREDENTIAL_KEYS[0], CREDENTIAL_KEYS[4], value(84));
    // An apostrophe in a comment or in prose opens no literal the value would be in.
    assert_masks(
        &format!("# don't commit this: {password} = '{v}'\n"),
        "python",
        &[&v],
    );
    assert_masks(
        &format!("Don't use {password}: {v} in production.\n"),
        "markdown",
        &[&v],
    );
    // A bare value runs on over an apostrophe, and a quoted one closes past it.
    let bare = format!("don't-{v}");
    assert_masks(&format!("{password}: {bare}\n"), "yaml", &[&bare]);
    let quoted = format!("it's-{v}");
    assert_masks(&format!("{token} = '{quoted}'\n"), "python", &[&quoted]);
    // A lifetime is no quote either.
    assert_masks(
        &format!("fn f<'a>(x: &'a str) {{ let {token} = \"{v}\"; }}\n"),
        "rust",
        &[&v],
    );
}

#[test]
fn character_literals_are_no_strings() {
    let (token, v) = (CREDENTIAL_KEYS[4], value(85));
    // A quote in a character literal opens no string, so what follows is read as code:
    // were `'"'`'s quote a string's, `c;` would be a value inside it.
    let source = format!("if (c == '\"') {{ {token} = c; }} log(\"x\");\n");
    assert_eq!(normalised(&source, "java"), source);
    let source = format!("if (c == '\\\"') {{ {token} = c; }} log(\"x\");\n");
    assert_eq!(normalised(&source, "c"), source);
    assert_masks(
        &format!("let q = '\"'; let {token} = \"{v}\";\n"),
        "rust",
        &[&v],
    );
    // A character is no value: in a language whose single quotes delimit characters,
    // `'x'` is never masked.
    let source = format!("char {token} = 'x';\n");
    assert_eq!(normalised(&source, "c"), source);
}

#[test]
fn backquoted_values_where_backquotes_delimit_strings() {
    let (password, v) = (CREDENTIAL_KEYS[0], value(86));
    assert_masks(&format!("{password} := `{v}`\n"), "go", &[&v]);
    assert_masks(&format!("const {password} = `{v}`;\n"), "javascript", &[&v]);
    // A template with an interpolation is code; a backquote elsewhere quotes a name or
    // a command.
    for (source, language) in [
        (
            format!("const {password} = `${{prefix}}{v}`;\n"),
            "javascript",
        ),
        (format!("val {password} = `{v}`\n"), "kotlin"),
        (format!("{password}=`{v}`\n"), "bash"),
    ] {
        assert_eq!(
            normalised(&source, language),
            source,
            "{language}: {source:?}"
        );
    }
}

#[test]
fn a_password_after_a_masked_token_is_found_at_once() {
    // The offsets property's shrunk case: a JSON Web Token running on over `ghs_` into
    // `://`. Before `://` stands `_`, so there is no scheme and no password; masked, the
    // token is a run of `X`, a scheme, and a second pass found the password. The
    // password is read over the token's mask as well, so the first pass finds it.
    let (jwt, password) = (json_web_token(), value(89));
    let (gh_prefix, gh_body) = github_token('s');
    let fine = fine_grained_github_token();
    for (source, secrets) in [
        (
            format!("{jwt}ghs_://u:{password}@h\n"),
            vec![format!("{jwt}ghs_"), password.clone()],
        ),
        // Scheme bytes after the token's run are the scheme's too, though none of them
        // starts one.
        (
            format!("{jwt}_+9://u:{password}@h\n"),
            vec![format!("{jwt}_"), password.clone()],
        ),
        // A token whose body opens with a digit is no scheme until it is masked.
        (
            format!("{gh_prefix}9{gh_body}://u:{password}@h\n"),
            vec![format!("9{gh_body}"), password.clone()],
        ),
        (
            format!("{}2{}://u:{password}@h\n", fine.0, &fine.1[1..]),
            vec![format!("2{}", &fine.1[1..]), password.clone()],
        ),
    ] {
        let refs: Vec<&str> = secrets.iter().map(String::as_str).collect();
        assert_masks(&source, "yaml", &refs);
        // The URI detector alone finds no password: only the token's mask makes one.
        assert!(
            secrets::secret_ranges_with(&[Detector::UriPassword], source.as_bytes(), "yaml")
                .is_empty(),
            "{source:?}"
        );
    }
}

#[test]
fn a_value_runs_on_over_what_another_detector_found() {
    // Detector 4 was not idempotent here: the bare value ended at the `;` inside the
    // URI's password, the URI detector masked the password, and a second pass read a
    // value running on over that mask to `@h`.
    let password = CREDENTIAL_KEYS[0];
    let uri = format!("s://u:{};x@h", value(87));
    assert_masks(&format!("{password}: {uri}\n"), "yaml", &[&uri]);
    assert_masks(&format!("{password}={uri} next\n"), "properties", &[&uri]);
}

/// Detector 4's credential assignment, kept only as an oracle: what it masked, which
/// the detector must still mask, but for the reads issue 96 removed. It is never used
/// to work out what the detector itself finds.
fn detector_four_assignment_values(bytes: &[u8], language: &str) -> Vec<std::ops::Range<usize>> {
    use regex::bytes::Regex;
    use std::sync::LazyLock;
    fn prefix() -> String {
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
    const QUOTED: &str = r#""((?:[^"\\\r\n]|\\[^\r\n])*)"|'([^'\r\n]*)'"#;
    const BARE: &str = r#"([^\s"'`#,;(){}\[\]<>|&*$\\][^\s"'`#,;(){}\[\]<>\\]*)"#;
    static QUOTED_ONLY: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(&format!("{}(?:{QUOTED})", prefix())).expect("quoted"));
    static ANY: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(&format!("{}(?:{QUOTED}|{BARE})", prefix())).expect("any"));
    static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?-u)\A(?:\$\{[^}]*\}|\$[A-Za-z_][A-Za-z0-9_]*|\{\{[^}]*\}\})\z")
            .expect("placeholder")
    });
    let pattern = if secrets::BARE_VALUE_LANGUAGES.contains(&language) {
        &ANY
    } else {
        &QUOTED_ONLY
    };
    pattern
        .captures_iter(bytes)
        .filter_map(|c| c.iter().skip(1).flatten().next())
        .filter(|m| !PLACEHOLDER.is_match(m.as_bytes()))
        .map(|m| m.range())
        .collect()
}

/// The ranges of `old` that no range of `new`, sorted and merged, covers whole.
fn uncovered(
    old: &[std::ops::Range<usize>],
    new: &[std::ops::Range<usize>],
) -> Vec<std::ops::Range<usize>> {
    old.iter()
        .filter(|r| !new.iter().any(|n| n.start <= r.start && r.end <= n.end))
        .cloned()
        .collect()
}

#[test]
fn assignment_detector_covers_detector_four_but_for_issue_96() {
    // Everything detector 4 masked, detector 5 masks: it may mask more, never less,
    // but for what issue 96 removed: a value read past the literal its key is in (the
    // destructive mask) and a character read as a string.
    let (k, v) = (CREDENTIAL_KEYS[0], value(88));
    let ordinary = [
        (format!("{k} = \"{v}\"\n"), "python"),
        (format!("{k}: '{v}'\r\n"), "yaml"),
        (format!("{k}: {v}\n"), "yaml"),
        (format!("{{\"{k}\": \"{v}\", \"n\": 1}}\n"), "json"),
        (format!("db.{k}={v}\n"), "properties"),
        (format!("ENV {k}={v}\n"), "dockerfile"),
        (format!("export {k}=\"{v}\"\n"), "bash"),
        (format!("$c = ['{k}' => '{v}'];\n"), "php"),
        (format!("cfg.{k} := \"{v}\"\n"), "go"),
        (format!("var {k} = \"a\\\"{v}\\\"b\";\n"), "csharp"),
        (format!("body = '{{\"{k}\": \"{v}\"}}'\n"), "python"),
        (format!("q = \"SET {k}='{v}'\"\n"), "java"),
        (format!("{k}: it's {v}\n"), "yaml"),
        (format!("{k} = 'it''s'\n"), "sql"),
    ];
    for (source, language) in &ordinary {
        let old = detector_four_assignment_values(source.as_bytes(), language);
        assert!(!old.is_empty(), "{source:?}");
        let new = secrets::secret_ranges(source.as_bytes(), language);
        assert!(
            uncovered(&old, &new).is_empty(),
            "{language}: {source:?} lost {:?}",
            uncovered(&old, &new)
        );
    }
    // What it no longer masks is exactly what issue 96 removed.
    for (source, language) in [
        (
            format!("url = \"{k}=\" + decrypt(pw) + \"&mode=x\"\n"),
            "python",
        ),
        (format!("char {k} = 'x';\n"), "c"),
    ] {
        let old = detector_four_assignment_values(source.as_bytes(), language);
        let new = secrets::secret_ranges(source.as_bytes(), language);
        assert!(!uncovered(&old, &new).is_empty(), "{source:?}");
    }
    // And on the sanitizer corpus, nothing.
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/corpus");
    let (mut files, mut old_ranges, mut lost_ranges) = (0, 0, Vec::new());
    for language in std::fs::read_dir(&corpus).expect("the corpus") {
        let language = language.expect("an entry").path();
        let id = language
            .file_name()
            .and_then(|n| n.to_str())
            .expect("a language")
            .to_owned();
        for file in std::fs::read_dir(&language).expect("a language") {
            let path = file.expect("an entry").path();
            let bytes = std::fs::read(&path).expect("a file");
            let old = detector_four_assignment_values(&bytes, &id);
            let new = secrets::secret_ranges(&bytes, &id);
            for r in uncovered(&old, &new) {
                lost_ranges.push(format!(
                    "{}:{}",
                    path.display(),
                    String::from_utf8_lossy(&bytes[r])
                ));
            }
            old_ranges += old.len();
            files += 1;
        }
    }
    eprintln!(
        "bench/corpus: {files} files, detector 4 {old_ranges} assignment values, lost {}",
        lost_ranges.len()
    );
    assert!(files > 50, "{files} corpus files");
    assert!(lost_ranges.is_empty(), "{lost_ranges:#?}");
}
