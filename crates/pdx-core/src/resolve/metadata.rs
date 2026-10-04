//! The repository's own module metadata: `tsconfig.json`, `package.json`, `go.mod` and
//! `Cargo.toml`, read safely and parsed into the shapes the registry and the engine
//! both use, so the two never interpret a repository differently.
//!
//! A metadata file is read only if discovery found it a candidate: a redacted,
//! binary or oversized file is never opened and counts as absent. A file Stage 2
//! extracted is read again through [`prepare_source`] and must still be the bytes
//! Stage 2 identified; one it did not (`go.mod`, of no language) through
//! [`prepare_candidate`]. Either way the bytes parsed are normalised, never the
//! original, and nothing is fetched, installed or run.

use std::collections::{BTreeMap, BTreeSet};

use pdx_engine::{AliasScope, CrateDependency, PathAlias};

use crate::index::discover::Disposition;
use crate::index::extract::{FileOutcome, prepare_candidate, prepare_source};

use super::registry::{RegistryError, Sources};

/// A metadata file's text, or `None` when it is absent: never discovered, or not a
/// candidate.
pub(crate) fn read(sources: &Sources<'_>, path: &str) -> Result<Option<String>, RegistryError> {
    let Some(state) = sources.files.get(path) else {
        return Ok(None);
    };
    if state.discovered.disposition != Disposition::Candidate {
        return Ok(None);
    }
    let read_error = |source| RegistryError::MetadataRead {
        path: path.to_owned(),
        source: Box::new(source),
    };
    let prepared = match &state.outcome {
        FileOutcome::Extracted { extract, blob_sha } => {
            let prepared = prepare_source(sources.root, &state.discovered).map_err(read_error)?;
            if prepared.blob_sha != *blob_sha || prepared.digest != extract.source_digest {
                return Err(RegistryError::SourceChanged(path.to_owned()));
            }
            prepared
        }
        FileOutcome::EngineFailed { blob_sha, .. } | FileOutcome::EngineCrashed { blob_sha } => {
            let prepared = prepare_source(sources.root, &state.discovered).map_err(read_error)?;
            if prepared.blob_sha != *blob_sha {
                return Err(RegistryError::SourceChanged(path.to_owned()));
            }
            prepared
        }
        FileOutcome::UnknownLanguage => {
            prepare_candidate(sources.root, &state.discovered).map_err(read_error)?
        }
        // Too large for the memory budget, or not a candidate: never read.
        _ => return Ok(None),
    };
    String::from_utf8(prepared.bytes)
        .map(Some)
        .map_err(|_| RegistryError::MetadataParse {
            path: path.to_owned(),
            reason: "not UTF-8".into(),
        })
}

/// `rel`, taken from the directory `base`, as a path below the repository root:
/// `.` and `..` applied, `/`-separated, "" for the root. `None` for a path that leaves
/// the repository or is absolute.
pub(crate) fn join(base: &str, rel: &str) -> Option<String> {
    if rel.starts_with('/') || rel.starts_with('\\') || rel.contains(':') {
        return None;
    }
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for part in rel.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

/// The directory of a repository path: "" for a file at the root.
pub(crate) fn dir_of(path: &str) -> &str {
    path.rfind('/').map_or("", |at| &path[..at])
}

/// The last segment of a repository path.
pub(crate) fn file_name(path: &str) -> &str {
    path.rfind('/').map_or(path, |at| &path[at + 1..])
}

// --- JSON with comments ---------------------------------------------------------

/// JSON with comments (`//` and `/* */`) and trailing commas, as `tsconfig.json` may
/// be written, turned into strict JSON: comments become spaces, and a comma followed
/// only by whitespace or comments before `}` or `]` is dropped. Strings are copied
/// as they are, escapes included.
///
/// # Errors
///
/// An unterminated string or block comment.
pub(crate) fn strip_jsonc(text: &str) -> Result<String, String> {
    let bytes = text.as_bytes();
    // Pass one: comments out.
    let mut plain = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let start = i;
                i += 1;
                loop {
                    match bytes.get(i) {
                        None => return Err("an unterminated string".into()),
                        Some(b'\\') => i += 2,
                        Some(b'"') => {
                            i += 1;
                            break;
                        }
                        Some(_) => i += 1,
                    }
                }
                plain.extend_from_slice(bytes.get(start..i).ok_or("an unterminated string")?);
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                plain.push(b' ');
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let end = text[i + 2..].find("*/").ok_or("an unterminated comment")?;
                i += 2 + end + 2;
                plain.push(b' ');
            }
            b => {
                plain.push(b);
                i += 1;
            }
        }
    }
    // Pass two: trailing commas out.
    let mut out = Vec::with_capacity(plain.len());
    let mut i = 0;
    while i < plain.len() {
        match plain[i] {
            b'"' => {
                let start = i;
                i += 1;
                while i < plain.len() && plain[i] != b'"' {
                    i += if plain[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
                out.extend_from_slice(&plain[start..i.min(plain.len())]);
            }
            b',' => {
                let next = plain[i + 1..]
                    .iter()
                    .find(|b| !b.is_ascii_whitespace())
                    .copied();
                if !matches!(next, Some(b'}' | b']')) {
                    out.push(b',');
                }
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| "not UTF-8".into())
}

// --- tsconfig.json --------------------------------------------------------------

/// What one `tsconfig.json` says that resolution needs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TsConfig {
    /// The configurations it extends, as written.
    pub(crate) extends: Vec<String>,
    /// `compilerOptions.baseUrl`.
    pub(crate) base_url: Option<String>,
    /// `compilerOptions.paths`: each alias and its first target.
    pub(crate) paths: Option<Vec<(String, String)>>,
}

/// Parses a `tsconfig.json`.
pub(crate) fn parse_tsconfig(text: &str) -> Result<TsConfig, String> {
    let json: serde_json::Value =
        serde_json::from_str(&strip_jsonc(text)?).map_err(|e| e.to_string())?;
    let object = json.as_object().ok_or("not an object")?;
    let extends = match object.get("extends") {
        None => Vec::new(),
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or("`extends` holds a non-string")
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err("`extends` is neither a string nor a list".into()),
    };
    let options = match object.get("compilerOptions") {
        None => None,
        Some(v) => Some(v.as_object().ok_or("`compilerOptions` is not an object")?),
    };
    let base_url = match options.and_then(|o| o.get("baseUrl")) {
        None => None,
        Some(v) => Some(v.as_str().ok_or("`baseUrl` is not a string")?.to_owned()),
    };
    let paths = match options.and_then(|o| o.get("paths")) {
        None => None,
        Some(v) => {
            let map = v.as_object().ok_or("`paths` is not an object")?;
            let mut out = Vec::new();
            for (alias, targets) in map {
                let first = targets
                    .as_array()
                    .ok_or("a `paths` entry is not a list")?
                    .first()
                    .map(|t| t.as_str().ok_or("a `paths` target is not a string"))
                    .transpose()?;
                if let Some(first) = first {
                    out.push((alias.clone(), first.to_owned()));
                }
            }
            Some(out)
        }
    };
    Ok(TsConfig {
        extends,
        base_url,
        paths,
    })
}

/// The alias scope of the `tsconfig.json` at `config_path`, its `extends` chain
/// followed within the repository (a configuration extended from a package is not
/// read), or `None` when it sets neither a base URL nor paths.
pub(crate) fn alias_scope(
    sources: &Sources<'_>,
    config_path: &str,
) -> Result<Option<AliasScope>, RegistryError> {
    // The options in force, each with the directory of the configuration that set it.
    let mut base_url: Option<(String, String)> = None;
    let mut paths: Option<(String, Vec<(String, String)>)> = None;
    // Children override what they extend, so the chain is applied from its far end.
    let mut chain = Vec::new();
    let mut seen = BTreeSet::new();
    collect_chain(sources, config_path, &mut seen, &mut chain)?;
    for (path, config) in chain.into_iter().rev() {
        let dir = dir_of(&path).to_owned();
        if let Some(b) = config.base_url {
            base_url = Some((dir.clone(), b));
        }
        if let Some(p) = config.paths {
            paths = Some((dir, p));
        }
    }
    if base_url.is_none() && paths.is_none() {
        return Ok(None);
    }
    let invalid = |reason: &str| RegistryError::InvalidMetadata {
        path: config_path.to_owned(),
        reason: reason.to_owned(),
    };
    let base_dir = match &base_url {
        Some((dir, url)) => {
            Some(join(dir, url).ok_or_else(|| invalid("`baseUrl` leaves the repository"))?)
        }
        None => None,
    };
    let mut aliases = Vec::new();
    if let Some((dir, entries)) = paths {
        // Targets are taken from the base URL, or without one from the directory of
        // the configuration that sets `paths`.
        let from = base_dir.clone().unwrap_or(dir);
        for (alias, target) in entries {
            if let Some(a) = path_alias(&from, &alias, &target) {
                aliases.push(a);
            }
        }
    }
    sort_aliases(&mut aliases);
    Ok(Some(AliasScope {
        dir_prefix: dir_of(config_path).to_owned(),
        base_url: base_dir.map(|b| if b.is_empty() { ".".to_owned() } else { b }),
        aliases,
    }))
}

/// The configuration at `path` and those it extends, nearest first.
fn collect_chain(
    sources: &Sources<'_>,
    path: &str,
    seen: &mut BTreeSet<String>,
    chain: &mut Vec<(String, TsConfig)>,
) -> Result<(), RegistryError> {
    if !seen.insert(path.to_owned()) {
        return Ok(()); // a cycle: each configuration counts once
    }
    let Some(text) = read(sources, path)? else {
        return Ok(());
    };
    let config = parse_tsconfig(&text).map_err(|reason| RegistryError::MetadataParse {
        path: path.to_owned(),
        reason,
    })?;
    let extends = config.extends.clone();
    chain.push((path.to_owned(), config));
    for base in extends {
        if !base.starts_with('.') {
            continue; // from a package, outside the repository
        }
        let Some(target) = join(dir_of(path), &base) else {
            continue;
        };
        let is_json = std::path::Path::new(&target)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"));
        let target = if sources.files.contains_key(&target) || is_json {
            target
        } else {
            format!("{target}.json")
        };
        collect_chain(sources, &target, seen, chain)?;
    }
    Ok(())
}

/// One `paths` entry as the engine takes it, the target made repository-relative.
/// `None` for an alias or target with more than one wildcard, which TypeScript
/// refuses too, or a target outside the repository.
fn path_alias(from: &str, alias: &str, target: &str) -> Option<PathAlias> {
    let split = |s: &str| -> Option<(String, String, bool)> {
        match s.matches('*').count() {
            0 => Some((s.to_owned(), String::new(), false)),
            1 => {
                let at = s.find('*')?;
                Some((s[..at].to_owned(), s[at + 1..].to_owned(), true))
            }
            _ => None,
        }
    };
    let (alias_prefix, alias_suffix, alias_wild) = split(alias)?;
    let (target_prefix, target_suffix, target_wild) = split(target)?;
    if alias_wild != target_wild {
        return None;
    }
    // The part before the wildcard: its directory made repository-relative, and any
    // partial last segment kept as written.
    let target_prefix = if alias_wild {
        let (dir, partial) = match target_prefix.rfind('/') {
            Some(at) => (&target_prefix[..at], &target_prefix[at + 1..]),
            None => ("", target_prefix.as_str()),
        };
        let dir = join(from, dir)?;
        if dir.is_empty() {
            partial.to_owned()
        } else {
            format!("{dir}/{partial}")
        }
    } else {
        join(from, &target_prefix)?
    };
    Some(PathAlias {
        alias_prefix,
        alias_suffix,
        target_prefix,
        target_suffix,
        has_wildcard: alias_wild,
    })
}

/// Aliases in the engine's order: the longest prefix first, ties by content.
pub(crate) fn sort_aliases(aliases: &mut [PathAlias]) {
    aliases.sort_by(|x, y| {
        y.alias_prefix
            .len()
            .cmp(&x.alias_prefix.len())
            .then_with(|| x.alias_prefix.cmp(&y.alias_prefix))
            .then_with(|| x.alias_suffix.cmp(&y.alias_suffix))
            .then_with(|| x.target_prefix.cmp(&y.target_prefix))
            .then_with(|| x.target_suffix.cmp(&y.target_suffix))
    });
}

// --- package.json ---------------------------------------------------------------

/// What a `package.json` names: the package, and the entries it declares, in the
/// order a resolver prefers them.
pub(crate) fn parse_package_json(text: &str) -> Result<(Option<String>, Vec<String>), String> {
    let json: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let object = json.as_object().ok_or("not an object")?;
    let name = object
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let entries = ["types", "typings", "module", "main"]
        .iter()
        .filter_map(|k| object.get(*k).and_then(|v| v.as_str()).map(str::to_owned))
        .collect();
    Ok((name, entries))
}

// --- go.mod ---------------------------------------------------------------------

/// The module path a `go.mod` declares.
pub(crate) fn parse_go_mod(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.split("//").next().unwrap_or("").trim();
        let rest = line.strip_prefix("module")?;
        if !rest.starts_with(char::is_whitespace) {
            return None;
        }
        let path = rest.trim().trim_matches('"').trim_matches('`');
        (!path.is_empty()).then(|| path.to_owned())
    })
}

// --- Cargo.toml -----------------------------------------------------------------

/// What a `Cargo.toml` declares that resolution needs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CargoManifest {
    /// `[package] name`.
    pub(crate) package_name: Option<String>,
    /// Whether it has a `[workspace]`.
    pub(crate) is_workspace_root: bool,
    /// `[workspace] members`, as listed.
    pub(crate) members: Vec<String>,
    /// Its dependencies of every kind, by name, each with its `path` (relative to the
    /// manifest) when it has one, and whether it is inherited from the workspace.
    pub(crate) dependencies: BTreeMap<String, (Option<String>, bool)>,
    /// `[workspace.dependencies]`, by name, with their `path`.
    pub(crate) workspace_dependencies: BTreeMap<String, Option<String>>,
}

/// Parses a `Cargo.toml`.
pub(crate) fn parse_cargo(text: &str) -> Result<CargoManifest, String> {
    let table: toml::Table = toml::from_str(text).map_err(|e| e.to_string().trim().to_owned())?;
    let mut m = CargoManifest {
        package_name: table
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .map(str::to_owned),
        is_workspace_root: table.contains_key("workspace"),
        ..CargoManifest::default()
    };
    if let Some(ws) = table.get("workspace").and_then(|w| w.as_table()) {
        if let Some(members) = ws.get("members").and_then(|m| m.as_array()) {
            m.members = members
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect();
        }
        if let Some(deps) = ws.get("dependencies").and_then(|d| d.as_table()) {
            for (name, spec) in deps {
                let path = spec.get("path").and_then(|p| p.as_str()).map(str::to_owned);
                m.workspace_dependencies.insert(name.clone(), path);
            }
        }
    }
    let mut tables: Vec<&toml::Table> = Vec::new();
    for kind in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(t) = table.get(kind).and_then(|d| d.as_table()) {
            tables.push(t);
        }
    }
    if let Some(targets) = table.get("target").and_then(|t| t.as_table()) {
        for target in targets.values().filter_map(|t| t.as_table()) {
            for kind in ["dependencies", "dev-dependencies", "build-dependencies"] {
                if let Some(t) = target.get(kind).and_then(|d| d.as_table()) {
                    tables.push(t);
                }
            }
        }
    }
    for deps in tables {
        for (name, spec) in deps {
            let path = spec.get("path").and_then(|p| p.as_str()).map(str::to_owned);
            let inherited = spec
                .get("workspace")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false);
            m.dependencies
                .entry(name.clone())
                .or_insert((path, inherited));
        }
    }
    Ok(m)
}

/// A manifest's dependencies as the engine takes them: each by name, with its path
/// relative to the repository when it lives inside it. An inherited dependency takes
/// the path the workspace root declares for it.
pub(crate) fn crate_dependencies(
    manifest_dir: &str,
    manifest: &CargoManifest,
    workspace: Option<(&str, &CargoManifest)>,
) -> Vec<CrateDependency> {
    manifest
        .dependencies
        .iter()
        .map(|(name, (path, inherited))| {
            let path = match (path, inherited, workspace) {
                (Some(p), _, _) => join(manifest_dir, p),
                (None, true, Some((root_dir, root))) => root
                    .workspace_dependencies
                    .get(name)
                    .and_then(Option::as_deref)
                    .and_then(|p| join(root_dir, p)),
                _ => None,
            };
            CrateDependency {
                name: name.clone(),
                path,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonc_comments_and_trailing_commas() {
        let text = r#"{
  // a comment
  "compilerOptions": {
    "baseUrl": "./", /* the root */
    "paths": { "@/*": ["src/*",], "slashes": ["x//y"], },
  },
  "s": "a // not a comment, \" /* nor this */",
}"#;
        let json: serde_json::Value = serde_json::from_str(&strip_jsonc(text).unwrap()).unwrap();
        assert_eq!(json["compilerOptions"]["baseUrl"], "./");
        assert_eq!(json["compilerOptions"]["paths"]["@/*"][0], "src/*");
        assert_eq!(json["compilerOptions"]["paths"]["slashes"][0], "x//y");
        assert_eq!(json["s"], "a // not a comment, \" /* nor this */");
        assert!(strip_jsonc("{ /* open").is_err());
        assert!(strip_jsonc("{ \"open").is_err());
    }

    #[test]
    fn paths_join_within_the_repository_only() {
        assert_eq!(join("web", "./src"), Some("web/src".into()));
        assert_eq!(join("web/app", "../lib/x"), Some("web/lib/x".into()));
        assert_eq!(join("", "."), Some(String::new()));
        assert_eq!(join("", ".."), None);
        assert_eq!(join("a", "/etc"), None);
        assert_eq!(join("a", "C:/x"), None);
    }

    #[test]
    fn go_mod_module_paths() {
        assert_eq!(
            parse_go_mod("// head\nmodule example.com/acme // trailing\n\ngo 1.22\n"),
            Some("example.com/acme".into())
        );
        assert_eq!(
            parse_go_mod("module \"example.com/q\"\n"),
            Some("example.com/q".into())
        );
        assert_eq!(parse_go_mod("modules x\n"), None);
        assert_eq!(parse_go_mod("go 1.22\n"), None);
    }

    #[test]
    fn path_aliases_split_at_their_wildcard() {
        assert_eq!(
            path_alias("", "@/*", "src/*"),
            Some(PathAlias {
                alias_prefix: "@/".into(),
                alias_suffix: String::new(),
                target_prefix: "src/".into(),
                target_suffix: String::new(),
                has_wildcard: true,
            })
        );
        let exact = path_alias("web", "config", "./config/index.ts").unwrap();
        assert_eq!(exact.target_prefix, "web/config/index.ts");
        assert!(!exact.has_wildcard);
        assert_eq!(path_alias("", "a/*/*", "b/*/*"), None);
        assert_eq!(path_alias("", "@x", "../../out"), None);
    }
}
