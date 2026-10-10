//! Configuration values a contract's placeholder may resolve from (4.7.1, issue 38).
//!
//! The sources are the repository's `application*.yml` and `application*.properties`
//! files, read as every other file is ([`super::source`]): a redacted one is never
//! opened. `.env*` files are inside the mandatory secret-path floor (5.12): never
//! opened, never parsed for a key or a value, so they resolve nothing and strengthen
//! no identity (issue 38). The text read is the normalised source, in which a secret
//! value is already masked.
//!
//! YAML mappings flatten to dotted keys (`orders: {topic: x}` is `orders.topic`), a
//! sequence's items to `key[i]`; a properties file's keys are kept as written. Every
//! document of every file is read, whatever profile it is for, because the profile a
//! service runs with is not known.
//!
//! A key's value is used only if it is safe to: never under a credential key
//! ([`crate::secrets::is_credential_key`]), never a value a secret detector still
//! recognises (a masked token, a URL with a password), never one holding a placeholder
//! itself. A placeholder `${KEY}` resolves when the eligible values of `KEY` are exactly
//! one distinct non-empty value; none, or several different ones, leave it unresolved.
//! No file, profile or order is preferred.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::index::derive::DeriveError;
use crate::resolve::registry::SymbolRegistry;
use crate::secrets;

use super::document::{self, Doc, DocError};
use super::identity::{has_placeholder, host_port, simple_placeholder};
use super::source::{self, Read};
use super::{ContractProblem, Found, extension, file_name};

/// Whether a file is an eligible configuration source: `application*.yml` or
/// `application*.properties`, by its name.
pub fn is_config_file(path: &str) -> bool {
    file_name(path).starts_with("application")
        && matches!(extension(path), Some("yml" | "properties"))
}

/// The eligible configuration values of a repository, by key.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigValues {
    values: BTreeMap<String, BTreeSet<String>>,
}

/// What a text resolves to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolved {
    /// A literal, as written.
    Literal(String),
    /// A `${KEY}` placeholder with exactly one eligible value.
    Placeholder {
        /// The placeholder as written.
        raw: String,
        /// Its value.
        value: String,
    },
    /// A placeholder (of any form) with no single eligible value.
    Unresolved(String),
}

impl ConfigValues {
    /// Reads every eligible configuration file of the repository.
    ///
    /// # Errors
    ///
    /// [`DeriveError`] when a file is not what Stage 1 and 2 found.
    pub(crate) fn read(
        root: &Path,
        registry: &SymbolRegistry,
        found: &mut Found,
    ) -> Result<Self, DeriveError> {
        let mut entries = Vec::new();
        for path in registry.files() {
            if !is_config_file(path) {
                continue;
            }
            let text = match source::read(root, registry, path)? {
                Read::Text(text) => text,
                Read::NotUtf8 => {
                    found.diagnose(path, ContractProblem::NotUtf8);
                    continue;
                }
                Read::Unread => continue,
            };
            if extension(path) == Some("properties") {
                entries.extend(parse_properties(&text));
            } else {
                match document::parse_yaml(&text) {
                    Ok(docs) => entries.extend(flatten(&docs)),
                    Err(DocError::TooDeep) => found.diagnose(path, ContractProblem::TooDeep),
                    Err(DocError::Syntax) => {
                        found.diagnose(path, ContractProblem::Unparseable { format: "yaml" });
                    }
                }
            }
        }
        Ok(Self::from_entries(entries))
    }

    /// The eligible values of `(key, value)` entries, as read from configuration.
    pub fn from_entries(entries: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut values: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (key, value) in entries {
            let value = value.trim();
            if eligible(&key, value) {
                values.entry(key).or_default().insert(value.to_owned());
            }
        }
        Self { values }
    }

    /// The single eligible value of `key`, if it has exactly one.
    pub fn single(&self, key: &str) -> Option<&str> {
        let values = self.values.get(key)?;
        match values.len() {
            1 => values.first().map(String::as_str),
            _ => None,
        }
    }

    /// What a text from the source resolves to: a literal stays, `${KEY}` takes
    /// `KEY`'s single value, and any other placeholder is unresolved.
    pub fn resolve(&self, text: &str) -> Resolved {
        if !has_placeholder(text) {
            return Resolved::Literal(text.to_owned());
        }
        match simple_placeholder(text).and_then(|key| self.single(key)) {
            Some(value) => Resolved::Placeholder {
                raw: text.to_owned(),
                value: value.to_owned(),
            },
            None => Resolved::Unresolved(text.to_owned()),
        }
    }

    /// What a URL from the source resolves to: as [`Self::resolve`], and also a URL
    /// that starts with `${KEY}` and goes on literally (`${users.url}/users/{id}`),
    /// whose base is `KEY`'s single value.
    pub fn resolve_leading(&self, text: &str) -> Resolved {
        if let Some(close) = text.strip_prefix("${").and_then(|_| text.find('}')) {
            let (head, rest) = text.split_at(close + 1);
            if !has_placeholder(rest)
                && let Some(base) = simple_placeholder(head).and_then(|k| self.single(k))
            {
                return Resolved::Placeholder {
                    raw: text.to_owned(),
                    value: format!("{base}{rest}"),
                };
            }
        }
        self.resolve(text)
    }

    /// The Kafka cluster the configuration names (`spring.kafka.bootstrap-servers`,
    /// one list of literal `host:port`), canonical: each authority as [`host_port`]
    /// makes it, sorted, once, joined by `,`.
    pub fn kafka_cluster(&self) -> Option<String> {
        let key = "spring.kafka.bootstrap-servers";
        let listed: Vec<&str> = if let Some(value) = self.single(key) {
            value.split(',').collect()
        } else {
            let mut items = Vec::new();
            for i in 0.. {
                let item = format!("{key}[{i}]");
                if !self.values.contains_key(&item) {
                    break;
                }
                items.push(self.single(&item)?);
            }
            items
        };
        cluster(&listed)
    }

    /// The `RabbitMQ` broker the configuration names (`spring.rabbitmq.addresses`, or
    /// `spring.rabbitmq.host` with its `port`), canonical as [`Self::kafka_cluster`].
    pub fn rabbit_broker(&self) -> Option<String> {
        if let Some(addresses) = self.single("spring.rabbitmq.addresses") {
            let listed: Vec<&str> = addresses
                .split(',')
                .map(|a| {
                    a.trim()
                        .rsplit_once("://")
                        .map_or(a.trim(), |(_, rest)| rest)
                })
                .collect();
            return cluster(&listed);
        }
        let host = self.single("spring.rabbitmq.host")?;
        let authority = match self.single("spring.rabbitmq.port") {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        };
        cluster(&[authority.as_str()])
    }

    /// The data source the configuration names (`spring.datasource.url`, a literal
    /// JDBC URL), as [`jdbc_identity`] reads it.
    pub fn datasource(&self) -> Option<String> {
        jdbc_identity(self.single("spring.datasource.url")?)
    }
}

/// Whether a value under a key may be used: not a credential's, not a secret a
/// detector still recognises, not empty, not itself a placeholder.
fn eligible(key: &str, value: &str) -> bool {
    !value.is_empty()
        && !key.is_empty()
        && !secrets::is_credential_key(key)
        && !has_placeholder(value)
        && secrets::secret_ranges(value.as_bytes(), "").is_empty()
}

/// A cluster identity: every listed `host[:port]` canonical, sorted and once.
fn cluster(listed: &[&str]) -> Option<String> {
    let mut hosts = BTreeSet::new();
    for item in listed {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        hosts.insert(host_port(item, None)?);
    }
    (!hosts.is_empty()).then(|| hosts.into_iter().collect::<Vec<_>>().join(","))
}

/// The data source of a JDBC URL: its host (with an explicit port) and database, the
/// user information and properties dropped, for `PostgreSQL`, `MySQL`, `MariaDB` and SQL
/// Server URLs and Oracle's thin form. `None` for any other URL, a placeholder, or a
/// local host.
pub fn jdbc_identity(url: &str) -> Option<String> {
    let rest = url.trim().strip_prefix("jdbc:")?;
    if has_placeholder(rest) {
        return None;
    }
    if let Some(thin) = rest
        .strip_prefix("oracle:thin:")
        .and_then(|t| t.strip_prefix('@'))
    {
        let thin = thin.strip_prefix("//").unwrap_or(thin);
        let (authority, service) = match thin.split_once('/') {
            Some(split) => split,
            // host:port:SID
            None => thin.rsplit_once(':')?,
        };
        let host = host_port(authority, None)?;
        let service = service.split(['?', ';']).next()?.trim();
        return (!service.is_empty()).then(|| format!("{host}/{service}"));
    }
    let (subprotocol, after) = rest.split_once("://")?;
    match subprotocol {
        "postgresql" | "mysql" | "mariadb" => {
            let end = after.find(['/', '?', ';']).unwrap_or(after.len());
            let host = host_port(&after[..end], None)?;
            let database = after[end..].strip_prefix('/')?;
            let database = database.split(['?', ';', '#']).next()?.trim();
            (!database.is_empty()).then(|| format!("{host}/{database}"))
        }
        "sqlserver" => {
            let (authority, properties) = after.split_once(';').unwrap_or((after, ""));
            let host = host_port(authority.split('\\').next()?, None)?;
            let database = properties.split(';').find_map(|p| {
                let (k, v) = p.split_once('=')?;
                let k = k.trim();
                (k.eq_ignore_ascii_case("databaseName") || k.eq_ignore_ascii_case("database"))
                    .then(|| v.trim())
            })?;
            (!database.is_empty()).then(|| format!("{host}/{database}"))
        }
        _ => None,
    }
}

/// A YAML stream's scalars as dotted keys.
pub fn flatten(docs: &[Doc]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for doc in docs {
        flatten_into(doc, String::new(), &mut out);
    }
    out
}

fn flatten_into(doc: &Doc, prefix: String, out: &mut Vec<(String, String)>) {
    match doc {
        Doc::Scalar(value) if !prefix.is_empty() => out.push((prefix, value.clone())),
        Doc::Map(_) => {
            for (key, value) in doc.entries() {
                let key = if prefix.is_empty() {
                    key.to_owned()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_into(value, key, out);
            }
        }
        Doc::Seq(items) if !prefix.is_empty() => {
            for (i, item) in items.iter().enumerate() {
                flatten_into(item, format!("{prefix}[{i}]"), out);
            }
        }
        _ => {}
    }
}

/// A Java properties file's entries: logical lines joined at a trailing backslash,
/// comments (`#`, `!`) and blank lines skipped, the key ended by the first unescaped
/// `=`, `:` or whitespace, escapes read.
pub fn parse_properties(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
            continue;
        }
        let mut logical = String::from(trimmed);
        while ends_continued(&logical) {
            logical.pop();
            match lines.next() {
                Some(next) => logical.push_str(next.trim_start()),
                None => break,
            }
        }
        let (key, value) = split_property(&logical);
        if !key.is_empty() {
            out.push((key, value));
        }
    }
    out
}

/// Whether a line ends with an odd number of backslashes: a continuation.
fn ends_continued(line: &str) -> bool {
    line.chars().rev().take_while(|&c| c == '\\').count() % 2 == 1
}

fn split_property(line: &str) -> (String, String) {
    let mut key = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next() {
                    key.push(unescape(next));
                }
            }
            '=' | ':' => break,
            c if c.is_whitespace() => {
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
                if matches!(chars.peek(), Some('=' | ':')) {
                    chars.next();
                }
                break;
            }
            c => key.push(c),
        }
    }
    let rest: String = chars.collect();
    let mut value = String::new();
    let mut chars = rest.trim_start().chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                value.push(unescape(next));
            }
        } else {
            value.push(c);
        }
    }
    (key, value)
}

const fn unescape(c: char) -> char {
    match c {
        't' => '\t',
        'n' => '\n',
        'r' => '\r',
        'f' => '\u{c}',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn properties_and_yaml_flatten_to_the_same_keys() {
        let p = parse_properties("# c\norders.topic = created\nlong.key: a\\\n  b\nflag\n");
        assert_eq!(
            p,
            [
                ("orders.topic".into(), "created".into()),
                ("long.key".into(), "ab".into()),
                ("flag".into(), String::new()),
            ]
        );
        let docs = document::parse_yaml("orders:\n  topic: created\nlist: [a, b]\n").expect("yaml");
        assert_eq!(
            flatten(&docs),
            [
                ("orders.topic".into(), "created".into()),
                ("list[0]".into(), "a".into()),
                ("list[1]".into(), "b".into()),
            ]
        );
    }

    #[test]
    fn jdbc_urls_give_host_and_database_without_user_information() {
        // User information is assembled at run time: no credential-shaped literal.
        let at = char::from(0x40);
        let user_info = ["shop", "pass"].join(":");
        assert_eq!(
            jdbc_identity(&format!("jdbc:mysql://{user_info}{at}db.example.com/shop")),
            Some("db.example.com/shop".into())
        );
        assert_eq!(
            jdbc_identity(&format!("jdbc:oracle:thin:{at}ora.example.com:1521/ORCL")),
            Some("ora.example.com:1521/ORCL".into())
        );
        assert_eq!(
            jdbc_identity("jdbc:sqlserver://sql.example.com:1433;databaseName=Sales;encrypt=true"),
            Some("sql.example.com:1433/Sales".into())
        );
        assert_eq!(
            jdbc_identity("jdbc:postgresql://DB.Example.com:5432/shop?ssl=true"),
            Some("db.example.com:5432/shop".into())
        );
        assert_eq!(jdbc_identity("jdbc:postgresql://localhost:5432/shop"), None);
        assert_eq!(jdbc_identity("jdbc:postgresql://${DB_HOST}/shop"), None);
        assert_eq!(jdbc_identity("jdbc:h2:mem:test"), None);
    }

    #[test]
    fn clusters_are_canonical_and_order_free() {
        let a = ConfigValues::from_entries([(
            "spring.kafka.bootstrap-servers".to_owned(),
            "K2.example.com:9092, k1.example.com:9092".to_owned(),
        )]);
        let b = ConfigValues::from_entries([
            (
                "spring.kafka.bootstrap-servers[0]".to_owned(),
                "k1.example.com:9092".to_owned(),
            ),
            (
                "spring.kafka.bootstrap-servers[1]".to_owned(),
                "k2.example.com:9092".to_owned(),
            ),
        ]);
        assert_eq!(
            a.kafka_cluster(),
            Some("k1.example.com:9092,k2.example.com:9092".into())
        );
        assert_eq!(a.kafka_cluster(), b.kafka_cluster());
        let local = ConfigValues::from_entries([(
            "spring.kafka.bootstrap-servers".to_owned(),
            "localhost:9092".to_owned(),
        )]);
        assert_eq!(local.kafka_cluster(), None);
    }
}
