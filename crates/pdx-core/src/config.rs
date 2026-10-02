//! A repository's configuration: `pdx.toml` at the checkout's root (Appendix C).
//!
//! The file is repository-controlled, so it is untrusted input. It is read only from
//! the root, only if it is a regular file no larger than [`MAX_FILE_BYTES`], parsed
//! strictly (an unknown section or key is an error, never ignored) and validated
//! completely before anything uses it. Nothing in it is executed here: the precise
//! commands are stored as text for the precise band (4.6) to run in its sandbox.
//!
//! Every collection is ordered (`Vec` in the order given, or `BTreeMap`), so the
//! effective configuration can be hashed into a build's identity later.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use ignore::gitignore::GitignoreBuilder;
use serde::Deserialize;

use crate::consts::MAX_FILE_BYTES;
use crate::kinds::{EdgeKind, LayerRole};
use crate::languages::{self, Language};
use crate::vocabulary::vocabulary;

/// The configuration file's name, at the repository's root.
pub const CONFIG_FILE: &str = "pdx.toml";

/// The secret file patterns used when `[secrets] patterns` is not given (5.12).
pub const DEFAULT_SECRET_PATTERNS: [&str; 4] = ["*.pem", "*.key", ".env*", "*id_rsa*"];

/// `[precise] timeout_minutes` when not given (Appendix C).
pub const DEFAULT_PRECISE_TIMEOUT_MINUTES: u32 = 60;

/// Why a configuration could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file could not be read.
    #[error("{}: {source}", path.display())]
    Read {
        /// The file.
        path: PathBuf,
        /// What went wrong.
        #[source]
        source: std::io::Error,
    },
    /// The file is a symlink, a directory or anything but a regular file.
    #[error("{}: not a regular file", path.display())]
    NotARegularFile {
        /// The file.
        path: PathBuf,
    },
    /// The file is larger than any repository file may be.
    #[error("{}: {size} bytes, more than the {limit} a repository file may have", path.display())]
    TooLarge {
        /// The file.
        path: PathBuf,
        /// Its size.
        size: u64,
        /// The limit.
        limit: u64,
    },
    /// The file is not valid TOML, or not the shape Appendix C defines: an unknown
    /// section or key, or a value of the wrong type. The message names the key and
    /// the line.
    #[error("{}: {message}", path.display())]
    Syntax {
        /// The file.
        path: PathBuf,
        /// The parser's message.
        message: String,
    },
    /// A value is well formed but not acceptable.
    #[error("{}: `{key}`: {reason}", path.display())]
    Invalid {
        /// The file.
        path: PathBuf,
        /// The key, with its section.
        key: String,
        /// Why.
        reason: String,
    },
}

/// A repository's effective configuration: `pdx.toml` with every default applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PdxConfig {
    /// `[discover]`.
    pub discover: DiscoverConfig,
    /// `[languages]`.
    pub languages: LanguagesConfig,
    /// `[identity]`: parsed, applied by contract linking (4.7.1).
    pub identity: Option<IdentityConfig>,
    /// `[secrets]`.
    pub secrets: SecretsConfig,
    /// `[layers]`: parsed, applied by layer-role assignment (4.8.3).
    pub layers: LayersConfig,
    /// `[rules]`: parsed, evaluated by the architecture rules (4.12.2).
    pub rules: RulesConfig,
    /// `[precise]`: parsed, applied by the precise band (4.6).
    pub precise: PreciseConfig,
    /// `[server]`: parsed, used by the local binary to reach a server.
    pub server: Option<ServerConfig>,
}

/// `[discover]` (4.5 Stage 1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoverConfig {
    /// Whether directories named `vendor` are indexed. Default false.
    pub include_vendor: bool,
    /// More paths to exclude, in gitignore syntax, rooted at the repository's root;
    /// never a negation. Default none.
    pub extra_excludes: Vec<String>,
    /// Files larger than this are skipped, with the reason `size`. Default
    /// [`MAX_FILE_BYTES`].
    pub max_file_bytes: u64,
}

/// `[languages]`: extensions this repository adds to the language matrix.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LanguagesConfig {
    /// Extension to language: `.blade.php` → `php`. Every key is a suffix of a file's
    /// name that Appendix A does not already define; every language is one of the
    /// matrix's. Default none.
    pub extra: BTreeMap<String, &'static Language>,
}

impl LanguagesConfig {
    /// The language of a file in this repository: the longest configured extension
    /// its name ends with, case-sensitively, and otherwise exactly what
    /// [`languages::detect`] says. The matrix itself is never changed.
    pub fn detect(
        &self,
        path: &str,
        content: &[u8],
        sibling_exists: impl Fn(&str) -> bool,
    ) -> Option<&'static Language> {
        let name = path.rsplit('/').next().unwrap_or(path);
        let configured = self
            .extra
            .iter()
            .filter(|(extension, _)| name.ends_with(extension.as_str()))
            .max_by_key(|(extension, _)| extension.len());
        match configured {
            Some((_, language)) => Some(language),
            None => languages::detect(path, content, sibling_exists),
        }
    }
}

/// `[identity]`: the namespace identity of this repository's contracts (4.7.1).
/// Parsed and kept; contract linking applies it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct IdentityConfig {
    /// Host names the repository's services answer to.
    pub hostnames: Vec<String>,
    /// Brokers its channels live on.
    pub brokers: Vec<String>,
    /// Data sources its tables live in.
    pub datasources: Vec<String>,
}

/// `[secrets]` (5.12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretsConfig {
    /// Files whose path matches are indexed as `redacted`, their content never read.
    /// Gitignore glob syntax, no negation; a pattern without `/` matches a file name in
    /// any directory. Given, it replaces the default, [`DEFAULT_SECRET_PATTERNS`].
    pub patterns: Vec<String>,
}

/// `[layers]`: layer rules for this repository, the schema of `pdx-arch.yaml`'s
/// `layer_rules` without `repo` (4.8.3). Parsed only; assignment applies them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayersConfig {
    /// The rules, in order: the first that matches wins.
    pub rules: Vec<LayerRule>,
}

/// One layer rule: modules and classes under paths matching `path_glob` have `role`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerRule {
    /// `match.path_glob`.
    pub path_glob: String,
    /// `role`.
    pub role: LayerRole,
}

/// `[rules]`: architecture rules for this repository, the schema of
/// `pdx-arch.yaml`'s `architecture_rules` (4.12.2). Parsed only; analytics evaluates
/// them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RulesConfig {
    /// `architecture`: the rules, ids unique.
    pub architecture: Vec<ArchitectureRule>,
}

/// One architecture rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchitectureRule {
    /// Its id.
    pub id: String,
    /// What it says.
    pub form: RuleForm,
}

vocabulary! {
    /// The level a cycle rule applies at (4.12.3).
    pub enum CycleLevel {
        /// Modules, within a repository.
        Module => "module",
        /// Services, across the link layer.
        Service => "service",
        /// Bounded contexts.
        Context => "context",
    }
}

/// The four forms of architecture rule, exactly (4.12.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleForm {
    /// `forbid {from_role, to_role, edge}`.
    ForbidRoles {
        /// Edges from this role.
        from_role: LayerRole,
        /// To this role.
        to_role: LayerRole,
        /// Of this kind.
        edge: EdgeKind,
    },
    /// `forbid {from_context, to_context}`.
    ForbidContexts {
        /// From this context.
        from_context: String,
        /// To this one.
        to_context: String,
    },
    /// `forbid_cycles {level}`.
    ForbidCycles {
        /// At this level.
        level: CycleLevel,
    },
    /// `require {from_role, to_role_any_of}`.
    Require {
        /// Edges from this role.
        from_role: LayerRole,
        /// Must reach one of these.
        to_role_any_of: Vec<LayerRole>,
    },
}

/// `[precise]` (4.6.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreciseConfig {
    /// Whether precise indexing runs. Default false.
    pub enabled: bool,
    /// The languages it runs for, each a matrix language, none twice. Default none.
    pub languages: Vec<&'static Language>,
    /// How long a precise job may run, unless its family says otherwise. Default
    /// [`DEFAULT_PRECISE_TIMEOUT_MINUTES`].
    pub timeout_minutes: u32,
    /// `[precise.java]`.
    pub java: Option<PreciseFamilyConfig>,
    /// `[precise.ts]`.
    pub ts: Option<PreciseFamilyConfig>,
    /// `[precise.python]`.
    pub python: Option<PreciseFamilyConfig>,
    /// `[precise.cxx]`.
    pub cxx: Option<PreciseFamilyConfig>,
}

/// A precise indexer family's settings: the command its family documents, stored as
/// written and never run here, and a timeout overriding `[precise] timeout_minutes`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PreciseFamilyConfig {
    /// The family's command: `build_cmd` for `java`, `install_cmd` for `ts` and
    /// `python`, `compdb_cmd` for `cxx`. Repository-controlled code (4.6.2).
    pub command: Option<String>,
    /// The family's own timeout.
    pub timeout_minutes: Option<u32>,
}

/// The precise indexer families `[precise.<family>]` configures (4.6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PreciseFamily {
    /// `scip-java`: Java, Kotlin, Scala.
    Java,
    /// `scip-typescript`: TypeScript, JavaScript.
    Ts,
    /// `scip-python`.
    Python,
    /// `scip-clang`: C, C++.
    Cxx,
}

impl PreciseConfig {
    /// A family's settings, if it has any.
    pub fn family(&self, family: PreciseFamily) -> Option<&PreciseFamilyConfig> {
        match family {
            PreciseFamily::Java => self.java.as_ref(),
            PreciseFamily::Ts => self.ts.as_ref(),
            PreciseFamily::Python => self.python.as_ref(),
            PreciseFamily::Cxx => self.cxx.as_ref(),
        }
    }

    /// How long a family's precise job may run: its own timeout, or the global one.
    pub fn timeout_for(&self, family: PreciseFamily) -> u32 {
        self.family(family)
            .and_then(|f| f.timeout_minutes)
            .unwrap_or(self.timeout_minutes)
    }
}

/// `[server]`: the server a local binary talks to. Parsed and kept.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    /// The server's URL.
    pub url: String,
    /// Where the token is: `env:<VARIABLE>` or `keyring`.
    pub token_ref: String,
}

impl Default for PdxConfig {
    fn default() -> Self {
        Self {
            discover: DiscoverConfig {
                include_vendor: false,
                extra_excludes: Vec::new(),
                max_file_bytes: MAX_FILE_BYTES,
            },
            languages: LanguagesConfig::default(),
            identity: None,
            secrets: SecretsConfig {
                patterns: DEFAULT_SECRET_PATTERNS
                    .iter()
                    .map(|p| (*p).to_owned())
                    .collect(),
            },
            layers: LayersConfig::default(),
            rules: RulesConfig::default(),
            precise: PreciseConfig {
                enabled: false,
                languages: Vec::new(),
                timeout_minutes: DEFAULT_PRECISE_TIMEOUT_MINUTES,
                java: None,
                ts: None,
                python: None,
                cxx: None,
            },
            server: None,
        }
    }
}

impl PdxConfig {
    /// The configuration of the repository checked out at `root`: its `pdx.toml`, or
    /// the defaults when it has none. Only the root is looked at.
    ///
    /// # Errors
    ///
    /// [`ConfigError`] for a `pdx.toml` that is not a regular file, is larger than
    /// [`MAX_FILE_BYTES`], cannot be read, or does not parse or validate.
    pub fn load(root: &Path) -> Result<Self, ConfigError> {
        let path = root.join(CONFIG_FILE);
        let metadata = match path.symlink_metadata() {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(source) => return Err(ConfigError::Read { path, source }),
        };
        if !metadata.is_file() {
            return Err(ConfigError::NotARegularFile { path });
        }
        let too_large = |size| ConfigError::TooLarge {
            path: path.clone(),
            size,
            limit: MAX_FILE_BYTES,
        };
        if metadata.len() > MAX_FILE_BYTES {
            return Err(too_large(metadata.len()));
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&path)
            .and_then(|file| file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes))
            .map_err(|source| ConfigError::Read {
                path: path.clone(),
                source,
            })?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(too_large(bytes.len() as u64));
        }
        let text = String::from_utf8(bytes).map_err(|_| ConfigError::Syntax {
            path: path.clone(),
            message: "not UTF-8".to_owned(),
        })?;
        Self::parse(&text, &path)
    }

    /// The configuration `text` gives; `path` names it in errors. Empty text gives the
    /// defaults.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Syntax`] or [`ConfigError::Invalid`].
    pub fn parse(text: &str, path: &Path) -> Result<Self, ConfigError> {
        let raw: RawConfig = toml::from_str(text).map_err(|e| ConfigError::Syntax {
            path: path.to_path_buf(),
            message: e.to_string().trim_end().to_owned(),
        })?;
        let checker = Checker { path };
        checker.config(raw)
    }
}

// --- the file as written ---------------------------------------------------------------

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct RawConfig {
    discover: RawDiscover,
    languages: RawLanguages,
    identity: Option<IdentityConfig>,
    secrets: RawSecrets,
    layers: RawLayers,
    rules: RawRules,
    precise: RawPrecise,
    server: Option<ServerConfig>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct RawDiscover {
    include_vendor: Option<bool>,
    extra_excludes: Option<Vec<String>>,
    max_file_bytes: Option<u64>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct RawLanguages {
    extra: BTreeMap<String, String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct RawSecrets {
    patterns: Option<Vec<String>>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct RawLayers {
    rules: Vec<RawLayerRule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLayerRule {
    #[serde(rename = "match")]
    matches: RawLayerMatch,
    role: LayerRole,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLayerMatch {
    path_glob: String,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct RawRules {
    architecture: Vec<RawRule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    id: String,
    forbid: Option<RawForbid>,
    forbid_cycles: Option<RawCycles>,
    require: Option<RawRequire>,
}

/// `forbid` in either of its two forms; which one is decided by the keys present.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawForbid {
    from_role: Option<LayerRole>,
    to_role: Option<LayerRole>,
    edge: Option<EdgeKind>,
    from_context: Option<String>,
    to_context: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCycles {
    level: CycleLevel,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRequire {
    from_role: LayerRole,
    to_role_any_of: Vec<LayerRole>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct RawPrecise {
    enabled: Option<bool>,
    languages: Option<Vec<String>>,
    timeout_minutes: Option<u32>,
    java: Option<RawJava>,
    ts: Option<RawInstall>,
    python: Option<RawInstall>,
    cxx: Option<RawCxx>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawJava {
    build_cmd: Option<String>,
    timeout_minutes: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInstall {
    install_cmd: Option<String>,
    timeout_minutes: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCxx {
    compdb_cmd: Option<String>,
    timeout_minutes: Option<u32>,
}

// --- validation ----------------------------------------------------------------------

struct Checker<'a> {
    path: &'a Path,
}

impl Checker<'_> {
    fn invalid(&self, key: impl Into<String>, reason: impl Into<String>) -> ConfigError {
        ConfigError::Invalid {
            path: self.path.to_path_buf(),
            key: key.into(),
            reason: reason.into(),
        }
    }

    fn config(&self, raw: RawConfig) -> Result<PdxConfig, ConfigError> {
        let defaults = PdxConfig::default();
        let discover = DiscoverConfig {
            include_vendor: raw
                .discover
                .include_vendor
                .unwrap_or(defaults.discover.include_vendor),
            extra_excludes: self.patterns(
                "discover.extra_excludes",
                raw.discover.extra_excludes.unwrap_or_default(),
            )?,
            max_file_bytes: match raw.discover.max_file_bytes {
                Some(0) => {
                    return Err(self.invalid("discover.max_file_bytes", "must be greater than 0"));
                }
                Some(n) => n,
                None => defaults.discover.max_file_bytes,
            },
        };
        let secrets = SecretsConfig {
            patterns: match raw.secrets.patterns {
                Some(patterns) => self.patterns("secrets.patterns", patterns)?,
                None => defaults.secrets.patterns,
            },
        };
        Ok(PdxConfig {
            discover,
            languages: self.languages(raw.languages)?,
            identity: raw.identity.map(|i| self.identity(i)).transpose()?,
            secrets,
            layers: self.layers(raw.layers)?,
            rules: self.rules(raw.rules)?,
            precise: self.precise(raw.precise, &defaults.precise)?,
            server: raw.server.map(|s| self.server(s)).transpose()?,
        })
    }

    /// Gitignore patterns that only exclude: each must compile, none may negate.
    fn patterns(&self, key: &str, patterns: Vec<String>) -> Result<Vec<String>, ConfigError> {
        let mut builder = GitignoreBuilder::new("");
        for pattern in &patterns {
            if pattern.trim().is_empty() || pattern.contains('\0') {
                return Err(self.invalid(key, format!("{pattern:?} is not a pattern")));
            }
            if pattern.starts_with('!') {
                return Err(self.invalid(
                    key,
                    format!("{pattern:?} is a negation; these patterns only exclude"),
                ));
            }
            builder
                .add_line(None, pattern)
                .map_err(|e| self.invalid(key, format!("{pattern:?}: {e}")))?;
        }
        builder
            .build()
            .map_err(|e| self.invalid(key, e.to_string()))?;
        Ok(patterns)
    }

    fn languages(&self, raw: RawLanguages) -> Result<LanguagesConfig, ConfigError> {
        let mut extra = BTreeMap::new();
        for (extension, id) in raw.extra {
            let key = format!("languages.extra.\"{extension}\"");
            let well_formed = extension.len() > 1
                && extension.starts_with('.')
                && !extension.contains(['/', '\\', '\0', '*', '?', '[', ']']);
            if !well_formed {
                return Err(self.invalid(key, "an extension is a `.` and at least one more character, with no path or glob characters"));
            }
            if languages::all()
                .iter()
                .any(|l| l.extensions.contains(&extension.as_str()))
            {
                return Err(self.invalid(key, "Appendix A already defines this extension; [languages] extra only adds new ones"));
            }
            let Some(language) = languages::by_id(&id) else {
                return Err(self.invalid(key, format!("{id:?} is not a language of the matrix")));
            };
            extra.insert(extension, language);
        }
        Ok(LanguagesConfig { extra })
    }

    fn identity(&self, raw: IdentityConfig) -> Result<IdentityConfig, ConfigError> {
        for (key, values) in [
            ("identity.hostnames", &raw.hostnames),
            ("identity.brokers", &raw.brokers),
            ("identity.datasources", &raw.datasources),
        ] {
            self.distinct_names(key, values)?;
        }
        Ok(raw)
    }

    fn distinct_names(&self, key: &str, values: &[String]) -> Result<(), ConfigError> {
        let mut seen = BTreeSet::new();
        for value in values {
            if value.trim().is_empty() || value.contains('\0') {
                return Err(self.invalid(key, format!("{value:?} is not a name")));
            }
            if !seen.insert(value) {
                return Err(self.invalid(key, format!("{value:?} is listed twice")));
            }
        }
        Ok(())
    }

    fn layers(&self, raw: RawLayers) -> Result<LayersConfig, ConfigError> {
        let mut rules = Vec::with_capacity(raw.rules.len());
        for (i, rule) in raw.rules.into_iter().enumerate() {
            let glob = rule.matches.path_glob;
            globset::Glob::new(&glob).map_err(|e| {
                self.invalid(format!("layers.rules[{i}].match.path_glob"), e.to_string())
            })?;
            if glob.contains('\0') {
                return Err(self.invalid(
                    format!("layers.rules[{i}].match.path_glob"),
                    "contains a NUL",
                ));
            }
            rules.push(LayerRule {
                path_glob: glob,
                role: rule.role,
            });
        }
        Ok(LayersConfig { rules })
    }

    fn rules(&self, raw: RawRules) -> Result<RulesConfig, ConfigError> {
        let mut ids = BTreeSet::new();
        let mut architecture = Vec::with_capacity(raw.architecture.len());
        for (i, rule) in raw.architecture.into_iter().enumerate() {
            let key = format!("rules.architecture[{i}]");
            if rule.id.trim().is_empty() || rule.id.contains('\0') {
                return Err(self.invalid(format!("{key}.id"), "a rule needs an id"));
            }
            if !ids.insert(rule.id.clone()) {
                return Err(self.invalid(
                    format!("{key}.id"),
                    format!("{:?} is the id of another rule", rule.id),
                ));
            }
            let form = match (rule.forbid, rule.forbid_cycles, rule.require) {
                (Some(f), None, None) => self.forbid(&key, f)?,
                (None, Some(c), None) => RuleForm::ForbidCycles { level: c.level },
                (None, None, Some(r)) => {
                    if r.to_role_any_of.is_empty() {
                        return Err(self.invalid(
                            format!("{key}.require.to_role_any_of"),
                            "must name at least one role",
                        ));
                    }
                    RuleForm::Require {
                        from_role: r.from_role,
                        to_role_any_of: r.to_role_any_of,
                    }
                }
                _ => {
                    return Err(self.invalid(
                        key,
                        "a rule has exactly one of `forbid`, `forbid_cycles` and `require`",
                    ));
                }
            };
            architecture.push(ArchitectureRule { id: rule.id, form });
        }
        Ok(RulesConfig { architecture })
    }

    /// `forbid {from_role, to_role, edge}` or `forbid {from_context, to_context}`, and
    /// nothing in between.
    fn forbid(&self, key: &str, f: RawForbid) -> Result<RuleForm, ConfigError> {
        match f {
            RawForbid {
                from_role: Some(from_role),
                to_role: Some(to_role),
                edge: Some(edge),
                from_context: None,
                to_context: None,
            } => Ok(RuleForm::ForbidRoles {
                from_role,
                to_role,
                edge,
            }),
            RawForbid {
                from_role: None,
                to_role: None,
                edge: None,
                from_context: Some(from),
                to_context: Some(to),
            } => {
                for (field, value) in [("from_context", &from), ("to_context", &to)] {
                    if value.trim().is_empty() || value.contains('\0') {
                        return Err(
                            self.invalid(format!("{key}.forbid.{field}"), "not a context id")
                        );
                    }
                }
                Ok(RuleForm::ForbidContexts {
                    from_context: from,
                    to_context: to,
                })
            }
            _ => Err(self.invalid(
                format!("{key}.forbid"),
                "either `from_role`, `to_role` and `edge`, or `from_context` and `to_context`",
            )),
        }
    }

    fn command(&self, key: &str, command: Option<String>) -> Result<Option<String>, ConfigError> {
        match command {
            Some(c) if c.trim().is_empty() => Err(self.invalid(key, "an empty command")),
            Some(c) if c.contains('\0') => Err(self.invalid(key, "a command with a NUL")),
            other => Ok(other),
        }
    }

    fn timeout(&self, key: &str, minutes: Option<u32>) -> Result<Option<u32>, ConfigError> {
        match minutes {
            Some(0) => Err(self.invalid(key, "must be at least 1 minute")),
            other => Ok(other),
        }
    }

    fn family(
        &self,
        name: &str,
        command_key: &str,
        command: Option<String>,
        timeout: Option<u32>,
    ) -> Result<PreciseFamilyConfig, ConfigError> {
        Ok(PreciseFamilyConfig {
            command: self.command(&format!("precise.{name}.{command_key}"), command)?,
            timeout_minutes: self.timeout(&format!("precise.{name}.timeout_minutes"), timeout)?,
        })
    }

    fn precise(
        &self,
        raw: RawPrecise,
        defaults: &PreciseConfig,
    ) -> Result<PreciseConfig, ConfigError> {
        let mut languages = Vec::new();
        for id in raw.languages.unwrap_or_default() {
            let Some(language) = languages::by_id(&id) else {
                return Err(self.invalid(
                    "precise.languages",
                    format!("{id:?} is not a language of the matrix"),
                ));
            };
            if languages.contains(&language) {
                return Err(self.invalid("precise.languages", format!("{id:?} is listed twice")));
            }
            languages.push(language);
        }
        Ok(PreciseConfig {
            enabled: raw.enabled.unwrap_or(defaults.enabled),
            languages,
            timeout_minutes: self
                .timeout("precise.timeout_minutes", raw.timeout_minutes)?
                .unwrap_or(defaults.timeout_minutes),
            java: raw
                .java
                .map(|f| self.family("java", "build_cmd", f.build_cmd, f.timeout_minutes))
                .transpose()?,
            ts: raw
                .ts
                .map(|f| self.family("ts", "install_cmd", f.install_cmd, f.timeout_minutes))
                .transpose()?,
            python: raw
                .python
                .map(|f| self.family("python", "install_cmd", f.install_cmd, f.timeout_minutes))
                .transpose()?,
            cxx: raw
                .cxx
                .map(|f| self.family("cxx", "compdb_cmd", f.compdb_cmd, f.timeout_minutes))
                .transpose()?,
        })
    }

    fn server(&self, raw: ServerConfig) -> Result<ServerConfig, ConfigError> {
        if raw.url.trim().is_empty() || raw.url.contains('\0') {
            return Err(self.invalid("server.url", "not a URL"));
        }
        let token_ref_ok = raw.token_ref == "keyring"
            || raw
                .token_ref
                .strip_prefix("env:")
                .is_some_and(|v| !v.is_empty() && !v.contains('\0'));
        if !token_ref_ok {
            return Err(self.invalid("server.token_ref", "must be `env:<VARIABLE>` or `keyring`"));
        }
        Ok(raw)
    }
}
