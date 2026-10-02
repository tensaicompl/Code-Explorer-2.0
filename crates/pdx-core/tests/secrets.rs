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
    // sha256 of the canonical form, computed independently:
    //   {"detector_version":1,"patterns":["*.key","*.pem","*id_rsa*",".env*"]}
    assert_eq!(SECRET_DETECTOR_VERSION, 1);
    let default = SecretPolicyDigest::of(&PdxConfig::default().secrets);
    assert_eq!(
        default.to_string(),
        "b9e1601f738d0ef897c6d316535f86fb31ecae8f6ab50676678a91209dd21b0c"
    );
    // A repository adding patterns: the effective set is the default and its own,
    //   {"detector_version":1,"patterns":["*.key","*.p12","*.pem","*id_rsa*",".env*",
    //    "config/credentials.yml"]}
    let custom = digest_of("[secrets]\npatterns = [\"config/credentials.yml\", \"*.p12\"]\n");
    assert_eq!(
        custom.to_string(),
        "eaaa970c8085c957cd2e5e5f285e7937bc48a5731a6dedd0ecd8ef749ebaae7d"
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
    // The same patterns under detector version 2, computed independently:
    //   {"detector_version":2,"patterns":["*.key","*.pem","*id_rsa*",".env*"]}
    let patterns = PdxConfig::default().secrets.patterns;
    let v1 = SecretPolicyDigest::for_policy(1, &patterns);
    let v2 = SecretPolicyDigest::for_policy(2, &patterns);
    assert_eq!(v1, SecretPolicyDigest::of(&PdxConfig::default().secrets));
    assert_eq!(
        v2.to_string(),
        "2d31e6c1829c821c29d11db44835995830ea3df0819a22b3736ffbf071ef07e8"
    );
    assert_ne!(v1, v2);
    // And no pattern at all is a policy too, distinct from both.
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
    let (assignment, bearer, uri, cloud) = (&single[2], &single[1], &single[3], &single[4]);
    assert!(
        overlapping(assignment, bearer) && overlapping(bearer, cloud) && overlapping(uri, cloud)
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

/// Source made of pieces, some of them secrets, joined by line endings of either kind.
fn pieces() -> impl Strategy<Value = Vec<u8>> {
    let piece = prop_oneof![
        proptest::collection::vec(any::<u8>(), 0..24),
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
