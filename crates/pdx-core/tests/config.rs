//! A repository's `pdx.toml` (Appendix C): its defaults, the full reference parsing,
//! and every refusal.

use std::fs;
use std::path::Path;

use pdx_core::config::{
    ArchitectureRule, ConfigError, CycleLevel, DEFAULT_PRECISE_TIMEOUT_MINUTES,
    DEFAULT_SECRET_PATTERNS, IdentityConfig, LayerRule, PdxConfig, PreciseFamily, RuleForm,
    ServerConfig,
};
use pdx_core::consts::MAX_FILE_BYTES;
use pdx_core::kinds::{EdgeKind, LayerRole};
use pdx_core::languages;

fn parse(text: &str) -> Result<PdxConfig, ConfigError> {
    PdxConfig::parse(text, Path::new("pdx.toml"))
}

fn refused(text: &str) -> String {
    match parse(text) {
        Ok(config) => panic!("accepted:\n{text}\nas {config:?}"),
        Err(error) => error.to_string(),
    }
}

fn root() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("pdx-config-test-")
        .tempdir()
        .expect("a directory")
}

// --- defaults ------------------------------------------------------------------------

#[test]
fn pdx_toml_defaults() {
    let config = PdxConfig::default();
    // [discover]: 4.5, and the size limit from consts.
    assert!(!config.discover.include_vendor);
    assert!(config.discover.extra_excludes.is_empty());
    assert_eq!(config.discover.max_file_bytes, MAX_FILE_BYTES);
    assert_eq!(config.discover.max_file_bytes, 2 * 1024 * 1024);
    // [languages]: nothing added.
    assert!(config.languages.extra.is_empty());
    // [secrets]: 5.12's patterns, held as a set in byte order.
    assert_eq!(
        config
            .secrets
            .patterns
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["*.key", "*.pem", "*id_rsa*", ".env*"]
    );
    assert_eq!(
        DEFAULT_SECRET_PATTERNS,
        ["*.pem", "*.key", ".env*", "*id_rsa*"]
    );
    // [layers] and [rules]: none.
    assert!(config.layers.rules.is_empty());
    assert!(config.rules.architecture.is_empty());
    // [precise]: off, no languages, an hour, no family settings.
    assert!(!config.precise.enabled);
    assert!(config.precise.languages.is_empty());
    assert_eq!(config.precise.timeout_minutes, 60);
    assert_eq!(DEFAULT_PRECISE_TIMEOUT_MINUTES, 60);
    assert_eq!(
        (
            &config.precise.java,
            &config.precise.ts,
            &config.precise.python,
            &config.precise.cxx
        ),
        (&None, &None, &None, &None)
    );
    for family in [
        PreciseFamily::Java,
        PreciseFamily::Ts,
        PreciseFamily::Python,
        PreciseFamily::Cxx,
    ] {
        assert_eq!(config.precise.timeout_for(family), 60);
    }
    // [identity] and [server]: absent.
    assert_eq!(config.identity, None);
    assert_eq!(config.server, None);

    // No pdx.toml, and an empty one, give exactly the defaults.
    let dir = root();
    assert_eq!(PdxConfig::load(dir.path()).expect("loads"), config);
    fs::write(dir.path().join("pdx.toml"), "").expect("a file");
    assert_eq!(PdxConfig::load(dir.path()).expect("loads"), config);
    assert_eq!(parse("# nothing but a comment\n").expect("parses"), config);
    // Sections present but empty are the defaults too.
    assert_eq!(
        parse("[discover]\n[languages]\n[secrets]\n[layers]\n[rules]\n[precise]\n")
            .expect("parses"),
        config
    );
}

// --- the reference -------------------------------------------------------------------

/// Appendix C, with every precise family of 4.6.2 filled in, the four rule forms of
/// 4.12.2, and a command that would do harm if anything ran it.
fn reference(marker: &Path) -> String {
    format!(
        r#"[discover]
include_vendor = false
extra_excludes = ["generated/**"]
max_file_bytes = 2097152

[languages]
extra = {{ ".blade.php" = "php" }}

[identity]                # 4.7.1 namespace identity for this repo's contracts (optional)
hostnames = ["fdp-service"]
brokers = ["kafka-main"]
datasources = ["pg-fdp"]

[secrets]
patterns = ["*.pem", "*.key", ".env*", "*id_rsa*"]

[layers]                 # same schema as pdx-arch.yaml layer_rules, repo-scoped
rules = [ {{ match = {{ path_glob = "src/main/java/**/web/**" }}, role = "api" }} ]

[rules]                  # same schema as pdx-arch.yaml architecture_rules, repo-scoped
architecture = [
  {{ id = "no-api-to-persistence", forbid = {{ from_role = "api", to_role = "persistence", edge = "CALLS" }} }},
  {{ id = "no-billing-to-flight", forbid = {{ from_context = "billing", to_context = "flight-data" }} }},
  {{ id = "no-cycles-between-contexts", forbid_cycles = {{ level = "context" }} }},
  {{ id = "api-reaches-service", require = {{ from_role = "api", to_role_any_of = ["service", "domain"] }} }},
]

[precise]
enabled = true
languages = ["java", "typescript", "python", "cpp"]
timeout_minutes = 60

[precise.java]
build_cmd = "mvn -q -DskipTests package"

[precise.ts]
install_cmd = "npm ci --ignore-scripts"

[precise.python]
install_cmd = "{}"

[precise.cxx]
compdb_cmd = "cmake -S . -B build -DCMAKE_EXPORT_COMPILE_COMMANDS=ON"
timeout_minutes = 90

[server]                 # local pdx only
url = "pdx.example.internal"
token_ref = "env:PDX_TOKEN"    # or "keyring"
"#,
        toml_escaped(&harmful(marker))
    )
}

/// A command that would leave a mark, and worse, if anything ran it.
fn harmful(marker: &Path) -> String {
    format!("touch '{}'; rm -rf ~", marker.display())
}

/// `text` as the inside of a TOML basic string: a Windows path's backslashes escaped.
fn toml_escaped(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

#[test]
fn the_appendix_c_reference_parses() {
    let dir = root();
    let marker = dir.path().join("ran");
    let config = parse(&reference(&marker)).expect("the reference parses");

    assert!(!config.discover.include_vendor);
    assert_eq!(config.discover.extra_excludes, ["generated/**"]);
    assert_eq!(config.discover.max_file_bytes, MAX_FILE_BYTES);
    assert_eq!(
        config.languages.extra.get(".blade.php").map(|l| l.id),
        Some("php")
    );
    assert_eq!(
        config.identity,
        Some(IdentityConfig {
            hostnames: vec!["fdp-service".to_owned()],
            brokers: vec!["kafka-main".to_owned()],
            datasources: vec!["pg-fdp".to_owned()],
        })
    );
    assert_eq!(
        config.secrets.patterns,
        PdxConfig::default().secrets.patterns
    );
    assert_eq!(
        config.layers.rules,
        [LayerRule {
            path_glob: "src/main/java/**/web/**".to_owned(),
            role: LayerRole::Api
        }]
    );
    assert_eq!(
        config.rules.architecture,
        [
            ArchitectureRule {
                id: "no-api-to-persistence".to_owned(),
                form: RuleForm::ForbidRoles {
                    from_role: LayerRole::Api,
                    to_role: LayerRole::Persistence,
                    edge: EdgeKind::Calls
                },
            },
            ArchitectureRule {
                id: "no-billing-to-flight".to_owned(),
                form: RuleForm::ForbidContexts {
                    from_context: "billing".to_owned(),
                    to_context: "flight-data".to_owned()
                },
            },
            ArchitectureRule {
                id: "no-cycles-between-contexts".to_owned(),
                form: RuleForm::ForbidCycles {
                    level: CycleLevel::Context
                }
            },
            ArchitectureRule {
                id: "api-reaches-service".to_owned(),
                form: RuleForm::Require {
                    from_role: LayerRole::Api,
                    to_role_any_of: vec![LayerRole::Service, LayerRole::Domain]
                },
            },
        ]
    );
    assert!(config.precise.enabled);
    let ids: Vec<&str> = config.precise.languages.iter().map(|l| l.id).collect();
    assert_eq!(ids, ["java", "typescript", "python", "cpp"]);
    // Commands are kept exactly as written, and none was run.
    let command = |family| {
        config
            .precise
            .family(family)
            .and_then(|f| f.command.as_deref())
    };
    assert_eq!(
        command(PreciseFamily::Java),
        Some("mvn -q -DskipTests package")
    );
    assert_eq!(command(PreciseFamily::Ts), Some("npm ci --ignore-scripts"));
    assert_eq!(
        command(PreciseFamily::Python),
        Some(harmful(&marker).as_str())
    );
    assert_eq!(
        command(PreciseFamily::Cxx),
        Some("cmake -S . -B build -DCMAKE_EXPORT_COMPILE_COMMANDS=ON")
    );
    assert!(!marker.exists(), "parsing ran a command");
    assert_eq!(
        config.server,
        Some(ServerConfig {
            url: "pdx.example.internal".to_owned(),
            token_ref: "env:PDX_TOKEN".to_owned()
        })
    );
}

#[test]
fn precise_timeouts_are_global_with_family_overrides() {
    // 4.6.2's own example: `timeout_minutes = 90` after `[precise.cxx]` is the C++
    // family's, not the global one.
    let config = parse(
        "[precise]\nenabled = true\nlanguages = [\"cpp\"]\n[precise.cxx]\ncompdb_cmd = \"cmake -S . -B build\"\ntimeout_minutes = 90\n",
    )
    .expect("parses");
    assert_eq!(config.precise.timeout_minutes, 60);
    assert_eq!(config.precise.timeout_for(PreciseFamily::Cxx), 90);
    assert_eq!(config.precise.timeout_for(PreciseFamily::Java), 60);
    let config = parse("[precise]\ntimeout_minutes = 30\n[precise.java]\ntimeout_minutes = 45\n")
        .expect("parses");
    assert_eq!(config.precise.timeout_minutes, 30);
    assert_eq!(config.precise.timeout_for(PreciseFamily::Java), 45);
    assert_eq!(config.precise.timeout_for(PreciseFamily::Ts), 30);
}

// --- [languages] ---------------------------------------------------------------------

#[test]
fn extra_compound_extension_wins() {
    let config =
        parse("[languages]\nextra = { \".component.rs\" = \"python\" }\n").expect("parses");
    let detect = |path: &str| config.languages.detect(path, b"", |_| false).map(|l| l.id);
    assert_eq!(detect("ui/widget.component.rs"), Some("python"));
    assert_eq!(detect("ui/widget.rs"), Some("rust"));
    // The matrix itself is untouched.
    assert_eq!(
        languages::detect("ui/widget.component.rs", b"", |_| false).map(|l| l.id),
        Some("rust")
    );
}

#[test]
fn extra_language_target_must_exist() {
    let message = refused("[languages]\nextra = { \".mylang\" = \"mylang\" }\n");
    assert!(
        message.contains("languages.extra") && message.contains("mylang"),
        "{message}"
    );
}

#[test]
fn extra_cannot_override_builtin_extension() {
    for (extension, language) in [
        (".rs", "python"),
        (".tsx", "javascript"),
        (".h", "rust"),
        (".dockerfile", "yaml"),
    ] {
        let message = refused(&format!(
            "[languages]\nextra = {{ \"{extension}\" = \"{language}\" }}\n"
        ));
        assert!(
            message.contains("already defines"),
            "{extension}: {message}"
        );
    }
}

#[test]
fn extra_matching_is_case_sensitive() {
    let config = parse("[languages]\nextra = { \".mjsx\" = \"javascript\" }\n").expect("parses");
    let detect = |path: &str| config.languages.detect(path, b"", |_| false).map(|l| l.id);
    assert_eq!(detect("a.mjsx"), Some("javascript"));
    assert_eq!(detect("a.MJSX"), None);
}

#[test]
fn tsx_cannot_be_extra_target() {
    let message = refused("[languages]\nextra = { \".jsx2\" = \"tsx\" }\n");
    assert!(message.contains("tsx"), "{message}");
    refused("[precise]\nlanguages = [\"tsx\"]\n");
}

#[test]
fn extra_extension_keys_are_suffixes() {
    for key in ["rs2", ".", "a/.x", ".x\\\\y", ".*", ".x?", ".[ab]"] {
        refused(&format!(
            "[languages]\nextra = {{ \"{key}\" = \"python\" }}\n"
        ));
    }
}

// --- refusals ----------------------------------------------------------------------

#[test]
fn malformed_configuration_is_refused() {
    let cases = [
        ("[discover\n", "pdx.toml"),
        ("[discovr]\n", "discovr"),
        ("[discover]\ninclude_vendr = true\n", "include_vendr"),
        ("[discover]\nmax_file_bytes = 0\n", "max_file_bytes"),
        ("[discover]\nmax_file_bytes = -1\n", "max_file_bytes"),
        (
            "[discover]\nextra_excludes = [\"!keep/**\"]\n",
            "discover.extra_excludes",
        ),
        (
            "[discover]\nextra_excludes = [\"src/[z-a]\"]\n",
            "discover.extra_excludes",
        ),
        ("[secrets]\npatterns = [\"a{b\"]\n", "secrets.patterns"),
        (
            "[discover]\nextra_excludes = [\"\"]\n",
            "discover.extra_excludes",
        ),
        ("[secrets]\npatterns = [\"!*.pem\"]\n", "secrets.patterns"),
        ("[secrets]\npattern = [\"*.pem\"]\n", "pattern"),
        (
            "[layers]\nrules = [ { match = { path_glob = \"src/**\" }, role = \"web\" } ]\n",
            "web",
        ),
        (
            "[layers]\nrules = [ { match = { path_glob = \"src/[a\" }, role = \"api\" } ]\n",
            "path_glob",
        ),
        (
            "[layers]\nrules = [ { match = { path = \"src/**\" }, role = \"api\" } ]\n",
            "path",
        ),
        (
            "[rules]\narchitecture = [ { id = \"r\", forbid = { from_role = \"api\", to_role = \"domain\", edge = \"calls\" } } ]\n",
            "calls",
        ),
        (
            "[rules]\narchitecture = [ { id = \"r\", forbid = { from_role = \"api\", to_context = \"billing\" } } ]\n",
            "rules.architecture[0].forbid",
        ),
        (
            "[rules]\narchitecture = [ { id = \"r\", forbid = { from_role = \"api\", to_role = \"domain\", edge = \"CALLS\" }, require = { from_role = \"api\", to_role_any_of = [\"service\"] } } ]\n",
            "exactly one",
        ),
        (
            "[rules]\narchitecture = [ { id = \"r\" } ]\n",
            "exactly one",
        ),
        (
            "[rules]\narchitecture = [ { id = \"r\", forbid_cycles = { level = \"module\" } }, { id = \"r\", forbid_cycles = { level = \"context\" } } ]\n",
            "another rule",
        ),
        (
            "[rules]\narchitecture = [ { id = \"r\", forbid_cycles = { level = \"galaxy\" } } ]\n",
            "galaxy",
        ),
        (
            "[rules]\narchitecture = [ { id = \"r\", require = { from_role = \"api\", to_role_any_of = [] } } ]\n",
            "to_role_any_of",
        ),
        (
            "[precise]\ntimeout_minutes = 0\n",
            "precise.timeout_minutes",
        ),
        (
            "[precise.cxx]\ntimeout_minutes = 0\n",
            "precise.cxx.timeout_minutes",
        ),
        (
            "[precise.typscript]\ninstall_cmd = \"npm ci\"\n",
            "typscript",
        ),
        (
            "[precise.java]\ninstall_cmd = \"mvn package\"\n",
            "install_cmd",
        ),
        (
            "[precise.ts]\ninstall_cmd = \"   \"\n",
            "precise.ts.install_cmd",
        ),
        (
            "[precise.ts]\ninstall_cmd = \"npm\\u0000ci\"\n",
            "precise.ts.install_cmd",
        ),
        ("[precise]\nlanguages = [\"java\", \"java\"]\n", "twice"),
        ("[precise]\nlanguages = [\"cobol\"]\n", "cobol"),
        (
            "[identity]\nhostnames = [\"a\", \"a\"]\n",
            "identity.hostnames",
        ),
        (
            "[server]\nurl = \"pdx.internal\"\ntoken_ref = \"plaintext-token\"\n",
            "server.token_ref",
        ),
        ("unknown_top_level = 1\n", "unknown_top_level"),
    ];
    for (text, mentioned) in cases {
        let message = refused(text);
        assert!(
            message.contains(mentioned),
            "{text:?} refused, but the message does not name {mentioned:?}: {message}"
        );
        assert!(message.contains("pdx.toml"), "{message}");
    }
}

#[test]
fn only_a_regular_root_file_of_bounded_size_is_read() {
    // Nested pdx.toml files are not configuration.
    let dir = root();
    fs::create_dir_all(dir.path().join("sub")).expect("a directory");
    fs::write(dir.path().join("sub/pdx.toml"), "this is not toml [").expect("a file");
    assert_eq!(
        PdxConfig::load(dir.path()).expect("loads"),
        PdxConfig::default()
    );

    // A directory where the file should be.
    let dir = root();
    fs::create_dir_all(dir.path().join("pdx.toml")).expect("a directory");
    assert!(matches!(
        PdxConfig::load(dir.path()),
        Err(ConfigError::NotARegularFile { .. })
    ));

    // Larger than any repository file may be: refused before it is read.
    let dir = root();
    let mut big = vec![b'#'; usize::try_from(MAX_FILE_BYTES).expect("small enough")];
    big.push(b'\n');
    fs::write(dir.path().join("pdx.toml"), big).expect("a file");
    assert!(matches!(
        PdxConfig::load(dir.path()),
        Err(ConfigError::TooLarge { .. })
    ));

    // Not UTF-8.
    let dir = root();
    fs::write(dir.path().join("pdx.toml"), b"[discover]\n# \xff\n").expect("a file");
    assert!(matches!(
        PdxConfig::load(dir.path()),
        Err(ConfigError::Syntax { .. })
    ));
}

#[cfg(unix)]
#[test]
fn a_symlinked_configuration_is_refused() {
    let dir = root();
    let outside = root();
    fs::write(
        outside.path().join("elsewhere.toml"),
        "[discover]\ninclude_vendor = true\n",
    )
    .expect("a file");
    std::os::unix::fs::symlink(
        outside.path().join("elsewhere.toml"),
        dir.path().join("pdx.toml"),
    )
    .expect("a symlink");
    assert!(matches!(
        PdxConfig::load(dir.path()),
        Err(ConfigError::NotARegularFile { .. })
    ));
}

// --- [secrets] ---------------------------------------------------------------------

fn secret_patterns(text: &str) -> Vec<String> {
    parse(text)
        .expect("parses")
        .secrets
        .patterns
        .into_iter()
        .collect()
}

#[test]
fn configured_secret_patterns_extend_the_mandatory_ones() {
    assert_eq!(
        secret_patterns("[secrets]\npatterns = [\"*.secret\"]\n"),
        ["*.key", "*.pem", "*.secret", "*id_rsa*", ".env*"]
    );
    // An empty list, or one repeating a mandatory pattern, removes nothing.
    let mandatory = ["*.key", "*.pem", "*id_rsa*", ".env*"];
    assert_eq!(secret_patterns("[secrets]\npatterns = []\n"), mandatory);
    assert_eq!(
        secret_patterns("[secrets]\npatterns = [\".env*\", \"*.pem\"]\n"),
        mandatory
    );
    assert_eq!(secret_patterns(""), mandatory);
}

#[test]
fn secret_pattern_order_and_duplicates_do_not_change_effective_policy() {
    let a = parse("[secrets]\npatterns = [\"*.secret\", \"credentials/**\"]\n").expect("parses");
    let b = parse("[secrets]\npatterns = [\"credentials/**\", \"*.secret\", \"*.secret\"]\n")
        .expect("parses");
    assert_eq!(a.secrets, b.secrets);
    assert_eq!(a, b);
    assert_eq!(
        a.secrets
            .patterns
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "*.key",
            "*.pem",
            "*.secret",
            "*id_rsa*",
            ".env*",
            "credentials/**"
        ]
    );
}
