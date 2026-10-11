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
    // first reproduced detector 2's values below):
    //   {"detector_version":3,"patterns":["*.key","*.pem","*id_rsa*",".env*"]}
    assert_eq!(SECRET_DETECTOR_VERSION, 3);
    let default = SecretPolicyDigest::of(&PdxConfig::default().secrets);
    assert_eq!(
        default.to_string(),
        "de4ad6db03d6d5ed4acb6db7119afc5cc7f54db7e434df08f0ab5f26fe487776"
    );
    // A repository adding patterns: the effective set is the default and its own,
    //   {"detector_version":3,"patterns":["*.key","*.p12","*.pem","*id_rsa*",".env*",
    //    "config/credentials.yml"]}
    let custom = digest_of("[secrets]\npatterns = [\"config/credentials.yml\", \"*.p12\"]\n");
    assert_eq!(
        custom.to_string(),
        "e4eeb594585a1f1b469d6c7deddf7316829a60afbf9eaa2811b35054773275d0"
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
    // The same patterns under the earlier detector versions, computed independently:
    // version 1, before issue 44, and version 2, before issue 72:
    //   {"detector_version":1,"patterns":["*.key","*.pem","*id_rsa*",".env*"]}
    //   {"detector_version":2,"patterns":["*.key","*.pem","*id_rsa*",".env*"]}
    let patterns = PdxConfig::default().secrets.patterns;
    let v1 = SecretPolicyDigest::for_policy(1, &patterns);
    let v2 = SecretPolicyDigest::for_policy(2, &patterns);
    let v3 = SecretPolicyDigest::for_policy(3, &patterns);
    assert_eq!(v3, SecretPolicyDigest::of(&PdxConfig::default().secrets));
    assert_eq!(
        v1.to_string(),
        "b9e1601f738d0ef897c6d316535f86fb31ecae8f6ab50676678a91209dd21b0c"
    );
    assert_eq!(
        v2.to_string(),
        "2d31e6c1829c821c29d11db44835995830ea3df0819a22b3736ffbf071ef07e8"
    );
    // Every version its own policy: an extraction cached under detector 2 is a miss.
    assert_ne!(v1, v2);
    assert_ne!(v2, v3);
    assert_ne!(v1, v3);
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
fn uri_scheme_boundary_uses_scheme_grammar_not_word_boundary() {
    let password = value(40);
    // A scheme starts at the start of the input or right after any byte no scheme
    // holds: `_` (a regex word byte, the case detector 2 missed), `~`, `/` and the
    // rest. Schemes hold letters, then letters, digits, `+`, `-` and `.`.
    for before in ["", " ", "_", "~", "/", "=", "\"", "'", "(", ":", "\t", "\n"] {
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
    // A scheme right after the `@` that ends another URI's match: the byte before it
    // is no scheme byte, though the other match took it.
    let (first, second) = (value(41), value(42));
    let source = format!("a://u:{first}@b://u:{second}@h\n");
    assert_eq!(
        normalised(&source, "python"),
        with_masked(&source, &[&first, &second])
    );
    // Not a URI password: no `://`, no `@`, no `:` between user and password, no
    // scheme, or a scheme that would start with a digit, with no letter-led scheme
    // before it.
    for source in [
        format!("mysql:/u:{password}@h\n"),
        format!("mysql//u:{password}@h\n"),
        format!("mysql://u:{password}\n"),
        format!("mysql://u{password}@h\n"),
        format!("://u:{password}@h\n"),
        format!(" ://u:{password}@h\n"),
        format!("9mysql://u:{password}@h\n"),
        format!("1.mysql://u:{password}@h\n"),
    ] {
        assert_eq!(normalised(&source, "python"), source, "{source:?}");
    }
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
        // The merged ranges are the same in every order of the five detectors.
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
fn uri_boundary_change_leaves_ordinary_sources_alone() {
    // Detector 2's URI pattern, kept here to compare: where a scheme starts at a word
    // boundary that is also a scheme boundary, detector 3 finds exactly what it found.
    let detector_2 = regex::bytes::Regex::new(
        r#"(?-u)(?i)\b[a-z][a-z0-9+.\-]*://[^\s/?#@"'<>:]*:([^\s/?#@"'<>]+)@"#,
    )
    .expect("a pattern");
    let old = |bytes: &[u8]| -> Vec<std::ops::Range<usize>> {
        detector_2
            .captures_iter(bytes)
            .filter_map(|c| c.get(1).map(|m| m.range()))
            .collect()
    };
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
        assert_eq!(
            secrets::secret_ranges_with(&[Detector::UriPassword], source.as_bytes(), "yaml"),
            old(source.as_bytes()),
            "{source:?}"
        );
    }
    // And over the sanitizer corpus, every file of every language.
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/corpus");
    let mut files = 0;
    for language in std::fs::read_dir(&corpus).expect("the corpus") {
        for file in std::fs::read_dir(language.expect("an entry").path()).expect("a language") {
            let bytes = std::fs::read(file.expect("an entry").path()).expect("a file");
            assert_eq!(
                secrets::secret_ranges_with(&[Detector::UriPassword], &bytes, "yaml"),
                old(&bytes)
            );
            files += 1;
        }
    }
    assert!(files > 50, "{files} corpus files");
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
    // Every order of the five detectors.
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
    assert_eq!(orders, 120);
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
        Just(b"\r\n".to_vec()),
        Just(b"\n".to_vec()),
        Just(b"\r".to_vec()),
    ];
    proptest::collection::vec(piece, 0..12).prop_map(|p| p.concat())
}

proptest! {
    #[test]
    fn secret_normalisation_preserves_offsets(
        source in pieces(),
        language in prop::sample::select(vec!["yaml", "python", "json", "properties"]),
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
