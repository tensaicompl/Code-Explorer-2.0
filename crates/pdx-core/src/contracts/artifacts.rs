//! Artifact contracts from build manifests (4.7.1, issue 58).
//!
//! A manifest's own coordinate is an `Artifact` it provides (and, with a literal
//! version, an `ArtifactVersion`), owned by the manifest; each dependency is consumed
//! (with its declared version or range, verbatim, as an `ArtifactVersion` too). Keys
//! are `ecosystem:group:name` and `ecosystem:group:name@version`, the coordinate its
//! own namespace. Only literal declarations are read: a property this manifest does
//! not define, an interpolated string, a version catalog's alias or any other build
//! code is withheld, never evaluated. No package manager, build tool or registry is
//! run or contacted, and nothing outside the checkout is read.
//!
//! | Manifest | Provides | Consumes |
//! |---|---|---|
//! | `pom.xml` | `groupId` (or the parent's), `artifactId`, `version` (or the parent's), `${…}` from the same POM's properties | `dependencies` (not `dependencyManagement`, profiles or plugins) |
//! | `build.gradle(.kts)` | literal `group` and `version`, with `rootProject.name` from the same directory's `settings.gradle(.kts)` | literal `"g:a[:v]"` strings and `group:, name:, version:` maps of the dependency configurations |
//! | `package.json` | `name` (`@scope/name` is group `@scope`), `version` | `dependencies`, `devDependencies`, `peerDependencies`, `optionalDependencies` |
//! | `Cargo.toml` | `[package]` `name`, `version` | every dependency table (a renamed one by its `package`) |
//! | `go.mod` | `module` | `require` |
//! | `*.csproj` | `PackageId`, `Version` or `PackageVersion` | `PackageReference` |
//! | `pyproject.toml` | `[project]` (or Poetry's) `name`, `version` unless dynamic | PEP 508 requirements, Poetry's dependency tables |
//! | `setup.cfg` | `[metadata]` `name`, `version` | `[options]` `install_requires` |
//! | `alire.toml` | `name`, `version` | `[[depends-on]]` |
//! | `*.gpr` | `project Name is` | `with "other.gpr";` |

use std::collections::BTreeMap;

use crate::index::derive::DeriveError;
use crate::kinds::ContractKind;
use crate::model::ContractDirection;

use super::annotation::script_literal;
use super::identity::{Ecosystem, artifact_key, artifact_version_key};
use super::source::{self, Read};
use super::{
    Context, ContractIdentity, ContractObservation, ContractProblem, Found, extension, file_name,
};

/// One coordinate a manifest declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Coordinate {
    /// Its ecosystem.
    pub ecosystem: Ecosystem,
    /// Its group, `""` where the ecosystem has none.
    pub group: String,
    /// Its name.
    pub name: String,
    /// Its declared version or range, if literal.
    pub version: Option<String>,
    /// Whether the manifest provides it (its own) or consumes it (a dependency).
    pub provides: bool,
}

impl Coordinate {
    fn new(
        ecosystem: Ecosystem,
        group: &str,
        name: &str,
        version: Option<&str>,
        provides: bool,
    ) -> Self {
        Self {
            ecosystem,
            group: group.trim().to_owned(),
            name: name.trim().to_owned(),
            version: version
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_owned),
            provides,
        }
    }
}

/// Which manifest family a file is, by its name.
fn family(path: &str) -> Option<&'static str> {
    let name = file_name(path);
    Some(match name {
        "pom.xml" => "maven",
        "build.gradle" | "build.gradle.kts" => "gradle",
        "package.json" => "npm",
        "Cargo.toml" => "cargo",
        "go.mod" => "go",
        "pyproject.toml" => "pyproject",
        "setup.cfg" => "setup.cfg",
        "alire.toml" => "alire",
        _ => match extension(path) {
            Some("csproj") => "nuget",
            Some("gpr") => "gpr",
            _ => return None,
        },
    })
}

pub(crate) fn observe(context: &Context<'_>, found: &mut Found) -> Result<(), DeriveError> {
    let registry = context.registry;
    for path in registry.files() {
        let Some(family) = family(path) else {
            continue;
        };
        let text = match source::read(context.root, registry, path)? {
            Read::Text(text) => text,
            Read::NotUtf8 => {
                found.diagnose(path, ContractProblem::NotUtf8);
                continue;
            }
            Read::Unread => continue,
        };
        let coordinates = match family {
            "maven" => maven(&text),
            "gradle" => {
                let settings = settings_name(context, path)?;
                Some(gradle(&text, settings.as_deref()))
            }
            "npm" => npm(&text),
            "cargo" => cargo(&text),
            "go" => Some(go_mod(&text)),
            "pyproject" => pyproject(&text),
            "setup.cfg" => Some(setup_cfg(&text)),
            "alire" => alire(&text),
            "nuget" => nuget(&text),
            _ => gpr(&text),
        };
        let Some(coordinates) = coordinates else {
            let format = match family {
                "maven" | "nuget" => "xml",
                "npm" => "json",
                "cargo" | "pyproject" | "alire" => "toml",
                other => other,
            };
            found.diagnose(path, ContractProblem::Unparseable { format });
            continue;
        };
        let owner = context.graph.file_node(path)?.clone();
        for coordinate in coordinates {
            for observation in observations(&coordinate, path) {
                found.observe(if coordinate.provides {
                    observation.with_owner(owner.clone())
                } else {
                    observation
                });
            }
        }
    }
    Ok(())
}

/// A coordinate's `Artifact`, and its `ArtifactVersion` when it has a version.
fn observations(coordinate: &Coordinate, path: &str) -> Vec<ContractObservation> {
    let Some(key) = artifact_key(coordinate.ecosystem, &coordinate.group, &coordinate.name) else {
        return Vec::new();
    };
    let direction = if coordinate.provides {
        ContractDirection::Provides
    } else {
        ContractDirection::Consumes
    };
    let raw = match &coordinate.version {
        Some(v) => format!("{}:{}:{v}", coordinate.group, coordinate.name),
        None => format!("{}:{}", coordinate.group, coordinate.name),
    };
    let base = |kind, key: String| {
        ContractObservation::new(kind, key.clone(), direction, path, raw.clone())
            .with_identities(vec![ContractIdentity::Exact(key)])
            .with_fact("ecosystem", coordinate.ecosystem.as_str())
            .with_fact("group", coordinate.group.clone())
            .with_fact("name", coordinate.name.clone())
    };
    let mut out = vec![base(ContractKind::Artifact, key.clone())];
    if let Some(version) = &coordinate.version
        && let Some(versioned) = artifact_version_key(&key, version)
    {
        out.push(
            base(ContractKind::ArtifactVersion, versioned).with_fact("version", version.clone()),
        );
    }
    out
}

// --- Maven --------------------------------------------------------------------------

fn xml(text: &str) -> Option<roxmltree::Document<'_>> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: false,
        ..roxmltree::ParsingOptions::default()
    };
    roxmltree::Document::parse_with_options(text, options).ok()
}

fn child<'a, 'b>(node: roxmltree::Node<'a, 'b>, name: &str) -> Option<roxmltree::Node<'a, 'b>> {
    node.children()
        .find(|c| c.is_element() && c.tag_name().name() == name)
}

fn child_text<'a>(node: roxmltree::Node<'a, '_>, name: &str) -> Option<&'a str> {
    child(node, name)?
        .text()
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

/// A POM's coordinate and dependencies; `None` for XML that cannot be read.
pub fn maven(text: &str) -> Option<Vec<Coordinate>> {
    let document = xml(text)?;
    let project = document.root_element();
    if project.tag_name().name() != "project" {
        return Some(Vec::new());
    }
    let parent = child(project, "parent");
    let group =
        child_text(project, "groupId").or_else(|| parent.and_then(|p| child_text(p, "groupId")));
    let artifact = child_text(project, "artifactId");
    let version =
        child_text(project, "version").or_else(|| parent.and_then(|p| child_text(p, "version")));
    let mut properties: BTreeMap<String, String> = BTreeMap::new();
    if let Some(list) = child(project, "properties") {
        for p in list.children().filter(roxmltree::Node::is_element) {
            if let Some(value) = p.text() {
                properties.insert(p.tag_name().name().to_owned(), value.trim().to_owned());
            }
        }
    }
    let mut builtins = |key: &str, value: Option<&str>| {
        if let Some(value) = value {
            properties.insert(key.to_owned(), value.to_owned());
        }
    };
    builtins("project.groupId", group);
    builtins("project.artifactId", artifact);
    builtins("project.version", version);
    builtins(
        "project.parent.version",
        parent.and_then(|p| child_text(p, "version")),
    );
    builtins(
        "project.parent.groupId",
        parent.and_then(|p| child_text(p, "groupId")),
    );
    let expand = |value: Option<&str>| value.and_then(|v| substitute(v, &properties));
    let mut out = Vec::new();
    if let (Some(group), Some(artifact)) = (expand(group), expand(artifact)) {
        out.push(Coordinate::new(
            Ecosystem::Maven,
            &group,
            &artifact,
            expand(version).as_deref(),
            true,
        ));
    }
    if let Some(dependencies) = child(project, "dependencies") {
        for dependency in dependencies
            .children()
            .filter(|c| c.tag_name().name() == "dependency")
        {
            let (Some(group), Some(artifact)) = (
                expand(child_text(dependency, "groupId")),
                expand(child_text(dependency, "artifactId")),
            ) else {
                continue;
            };
            let version = expand(child_text(dependency, "version"));
            out.push(Coordinate::new(
                Ecosystem::Maven,
                &group,
                &artifact,
                version.as_deref(),
                false,
            ));
        }
    }
    Some(out)
}

/// `${name}` replaced from the same POM's properties; `None` when any is not defined
/// there (a parent's, a profile's, the environment's).
fn substitute(value: &str, properties: &BTreeMap<String, String>) -> Option<String> {
    let mut text = value.to_owned();
    for _ in 0..8 {
        let Some(start) = text.find("${") else {
            return Some(text);
        };
        let end = start + text[start..].find('}')?;
        let replacement = properties.get(&text[start + 2..end])?;
        text.replace_range(start..=end, replacement);
    }
    None
}

// --- Gradle -------------------------------------------------------------------------

/// `rootProject.name` from the `settings.gradle(.kts)` beside a build file.
fn settings_name(context: &Context<'_>, path: &str) -> Result<Option<String>, DeriveError> {
    let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
    for name in ["settings.gradle", "settings.gradle.kts"] {
        let settings = if dir.is_empty() {
            name.to_owned()
        } else {
            format!("{dir}/{name}")
        };
        if let Read::Text(text) = source::read(context.root, context.registry, &settings)? {
            let names: Vec<String> = text
                .lines()
                .filter_map(|l| assignment(l, "rootProject.name"))
                .collect();
            if let [name] = names.as_slice() {
                return Ok(Some(name.clone()));
            }
        }
    }
    Ok(None)
}

/// A line's literal `name = "value"` (or `'value'`), if it is one.
fn assignment(line: &str, name: &str) -> Option<String> {
    let rest = line.trim().strip_prefix(name)?.trim_start();
    let value = rest.strip_prefix('=')?.trim();
    script_literal(value)
}

/// The dependency configurations whose literal declarations are read.
const CONFIGURATIONS: [&str; 15] = [
    "implementation",
    "api",
    "compileOnly",
    "runtimeOnly",
    "testImplementation",
    "testRuntimeOnly",
    "testCompileOnly",
    "annotationProcessor",
    "kapt",
    "ksp",
    "compile",
    "runtime",
    "testCompile",
    "developmentOnly",
    "classpath",
];

/// A Gradle build file's literal coordinate and dependencies.
pub fn gradle(text: &str, root_name: Option<&str>) -> Vec<Coordinate> {
    let mut groups = Vec::new();
    let mut versions = Vec::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.split("//").next().unwrap_or("").trim();
        if let Some(g) = assignment(line, "group") {
            groups.push(g);
        }
        if let Some(v) = assignment(line, "version") {
            versions.push(v);
        }
        let Some(config) = CONFIGURATIONS.iter().find(|c| {
            line.strip_prefix(**c)
                .is_some_and(|r| r.starts_with([' ', '(', '\t']))
        }) else {
            continue;
        };
        let mut arguments = line[config.len()..].trim();
        arguments = arguments
            .strip_prefix('(')
            .and_then(|a| a.strip_suffix(')'))
            .unwrap_or(arguments)
            .trim();
        for wrapper in ["platform(", "enforcedPlatform("] {
            if let Some(inner) = arguments
                .strip_prefix(wrapper)
                .and_then(|a| a.strip_suffix(')'))
            {
                arguments = inner.trim();
            }
        }
        if let Some(notation) = script_literal(arguments) {
            let parts: Vec<&str> = notation.split(':').collect();
            if let [group, name, rest @ ..] = parts.as_slice()
                && rest.len() <= 2
                && !group.is_empty()
                && !name.is_empty()
            {
                out.push(Coordinate::new(
                    Ecosystem::Maven,
                    group,
                    name,
                    rest.first().copied(),
                    false,
                ));
            }
        } else if let Some(map) = gradle_map(arguments) {
            out.push(map);
        }
    }
    if let (Some(name), [group]) = (root_name, groups.as_slice()) {
        let version = match versions.as_slice() {
            [v] => Some(v.as_str()),
            _ => None,
        };
        out.push(Coordinate::new(
            Ecosystem::Maven,
            group,
            name,
            version,
            true,
        ));
    }
    out
}

/// `group: 'g', name: 'a', version: 'v'` (Groovy) or `group = "g", name = "a"`
/// (Kotlin), literals only.
fn gradle_map(arguments: &str) -> Option<Coordinate> {
    let mut fields: BTreeMap<&str, String> = BTreeMap::new();
    for part in super::annotation::split_top(arguments)? {
        let (key, value) = part.split_once(':').or_else(|| part.split_once('='))?;
        fields.insert(key.trim(), script_literal(value.trim())?);
    }
    Some(Coordinate::new(
        Ecosystem::Maven,
        fields.get("group")?,
        fields.get("name")?,
        fields.get("version").map(String::as_str),
        false,
    ))
}

// --- npm ----------------------------------------------------------------------------

/// A version spec that is not a version: a path, a URL, a repository, a workspace.
fn non_registry(spec: &str) -> bool {
    spec.contains(':') || spec.contains('/')
}

/// `package.json`'s coordinate and dependencies; `None` for JSON that cannot be read.
pub fn npm(text: &str) -> Option<Vec<Coordinate>> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let split = |name: &str| -> Option<(String, String)> {
        match name.strip_prefix('@') {
            Some(scoped) => {
                let (scope, package) = scoped.split_once('/')?;
                Some((format!("@{scope}"), package.to_owned()))
            }
            None => Some((String::new(), name.to_owned())),
        }
    };
    let mut out = Vec::new();
    if let Some(name) = value.get("name").and_then(serde_json::Value::as_str)
        && let Some((group, package)) = split(name)
    {
        let version = value.get("version").and_then(serde_json::Value::as_str);
        out.push(Coordinate::new(
            Ecosystem::Npm,
            &group,
            &package,
            version,
            true,
        ));
    }
    for table in [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ] {
        let Some(entries) = value.get(table).and_then(serde_json::Value::as_object) else {
            continue;
        };
        for (name, spec) in entries {
            let Some((group, package)) = split(name) else {
                continue;
            };
            let version = spec.as_str().filter(|s| !non_registry(s));
            out.push(Coordinate::new(
                Ecosystem::Npm,
                &group,
                &package,
                version,
                false,
            ));
        }
    }
    Some(out)
}

// --- Cargo --------------------------------------------------------------------------

/// `Cargo.toml`'s package and dependencies; `None` for TOML that cannot be read.
pub fn cargo(text: &str) -> Option<Vec<Coordinate>> {
    let table: toml::Table = text.parse().ok()?;
    let mut out = Vec::new();
    if let Some(package) = table.get("package").and_then(toml::Value::as_table)
        && let Some(name) = package.get("name").and_then(toml::Value::as_str)
    {
        let version = package.get("version").and_then(toml::Value::as_str);
        out.push(Coordinate::new(Ecosystem::Cargo, "", name, version, true));
    }
    let mut tables: Vec<&toml::Table> = Vec::new();
    for name in ["dependencies", "dev-dependencies", "build-dependencies"] {
        tables.extend(table.get(name).and_then(toml::Value::as_table));
    }
    if let Some(workspace) = table.get("workspace").and_then(toml::Value::as_table) {
        tables.extend(
            workspace
                .get("dependencies")
                .and_then(toml::Value::as_table),
        );
    }
    if let Some(targets) = table.get("target").and_then(toml::Value::as_table) {
        for target in targets.values().filter_map(toml::Value::as_table) {
            for name in ["dependencies", "dev-dependencies", "build-dependencies"] {
                tables.extend(target.get(name).and_then(toml::Value::as_table));
            }
        }
    }
    for dependencies in tables {
        for (name, spec) in dependencies {
            let (crate_name, version) = match spec {
                toml::Value::String(v) => (name.as_str(), Some(v.as_str())),
                toml::Value::Table(t) => (
                    t.get("package")
                        .and_then(toml::Value::as_str)
                        .unwrap_or(name),
                    t.get("version").and_then(toml::Value::as_str),
                ),
                _ => continue,
            };
            out.push(Coordinate::new(
                Ecosystem::Cargo,
                "",
                crate_name,
                version,
                false,
            ));
        }
    }
    Some(out)
}

// --- Go -----------------------------------------------------------------------------

/// `go.mod`'s module and requirements.
pub fn go_mod(text: &str) -> Vec<Coordinate> {
    let mut out = Vec::new();
    let mut in_require = false;
    for line in text.lines() {
        let line = line.split("//").next().unwrap_or("").trim();
        if in_require {
            if line == ")" {
                in_require = false;
            } else if let Some(c) = requirement(line) {
                out.push(c);
            }
            continue;
        }
        if let Some(module) = line.strip_prefix("module ") {
            let module = module.trim().trim_matches('"');
            if !module.is_empty() {
                out.push(Coordinate::new(Ecosystem::Go, "", module, None, true));
            }
        } else if let Some(rest) = line.strip_prefix("require") {
            let rest = rest.trim();
            if rest == "(" {
                in_require = true;
            } else if let Some(c) = requirement(rest) {
                out.push(c);
            }
        }
    }
    out
}

fn requirement(line: &str) -> Option<Coordinate> {
    let mut parts = line.split_whitespace();
    let path = parts.next()?.trim_matches('"');
    let version = parts.next()?;
    Some(Coordinate::new(
        Ecosystem::Go,
        "",
        path,
        Some(version),
        false,
    ))
}

// --- NuGet --------------------------------------------------------------------------

/// A project file's package and references; `None` for XML that cannot be read.
pub fn nuget(text: &str) -> Option<Vec<Coordinate>> {
    let document = xml(text)?;
    let literal = |v: &str| (!v.contains("$(")).then(|| v.trim().to_owned());
    let mut ids = Vec::new();
    let mut versions = Vec::new();
    let mut out = Vec::new();
    for node in document.descendants().filter(roxmltree::Node::is_element) {
        match node.tag_name().name() {
            "PackageId" => ids.extend(node.text().and_then(literal)),
            "Version" | "PackageVersion"
                if node
                    .parent_element()
                    .is_some_and(|p| p.tag_name().name() == "PropertyGroup") =>
            {
                versions.extend(node.text().and_then(literal));
            }
            "PackageReference" => {
                let Some(name) = node.attribute("Include").and_then(literal) else {
                    continue;
                };
                let version = node
                    .attribute("Version")
                    .map(str::to_owned)
                    .or_else(|| child_text(node, "Version").map(str::to_owned))
                    .and_then(|v| literal(&v));
                out.push(Coordinate::new(
                    Ecosystem::Nuget,
                    "",
                    &name,
                    version.as_deref(),
                    false,
                ));
            }
            _ => {}
        }
    }
    if let [id] = ids.as_slice() {
        let version = match versions.as_slice() {
            [v] => Some(v.as_str()),
            _ => None,
        };
        out.push(Coordinate::new(Ecosystem::Nuget, "", id, version, true));
    }
    Some(out)
}

// --- Python -------------------------------------------------------------------------

/// A PEP 508 requirement's name and version specifier (extras and markers dropped; a
/// direct reference has no version).
fn pep508(requirement: &str) -> Option<(String, Option<String>)> {
    let requirement = requirement.split(';').next()?.trim();
    let end = requirement
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')))
        .unwrap_or(requirement.len());
    let name = &requirement[..end];
    if name.is_empty() {
        return None;
    }
    let mut rest = requirement[end..].trim();
    if rest.starts_with('[') {
        rest = rest[rest.find(']')? + 1..].trim();
    }
    if rest.starts_with('@') {
        return Some((name.to_owned(), None));
    }
    let spec = rest.trim_start_matches('(').trim_end_matches(')').trim();
    Some((name.to_owned(), (!spec.is_empty()).then(|| spec.to_owned())))
}

/// `pyproject.toml`'s project and requirements; `None` for TOML that cannot be read.
pub fn pyproject(text: &str) -> Option<Vec<Coordinate>> {
    let table: toml::Table = text.parse().ok()?;
    let mut out = Vec::new();
    let string =
        |t: &toml::Table, k: &str| t.get(k).and_then(toml::Value::as_str).map(str::to_owned);
    if let Some(project) = table.get("project").and_then(toml::Value::as_table) {
        let dynamic: Vec<&str> = project
            .get("dynamic")
            .and_then(toml::Value::as_array)
            .map(|a| a.iter().filter_map(toml::Value::as_str).collect())
            .unwrap_or_default();
        if let Some(name) = string(project, "name") {
            let version = string(project, "version").filter(|_| !dynamic.contains(&"version"));
            out.push(Coordinate::new(
                Ecosystem::Pypi,
                "",
                &name,
                version.as_deref(),
                true,
            ));
        }
        let mut requirements: Vec<&toml::Value> = Vec::new();
        requirements.extend(
            project
                .get("dependencies")
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten(),
        );
        if let Some(optional) = project
            .get("optional-dependencies")
            .and_then(toml::Value::as_table)
        {
            for list in optional.values().filter_map(toml::Value::as_array) {
                requirements.extend(list);
            }
        }
        for requirement in requirements.iter().filter_map(|v| v.as_str()) {
            if let Some((name, version)) = pep508(requirement) {
                out.push(Coordinate::new(
                    Ecosystem::Pypi,
                    "",
                    &name,
                    version.as_deref(),
                    false,
                ));
            }
        }
    }
    if let Some(poetry) = table
        .get("tool")
        .and_then(|t| t.get("poetry"))
        .and_then(toml::Value::as_table)
    {
        if let Some(name) = string(poetry, "name") {
            out.push(Coordinate::new(
                Ecosystem::Pypi,
                "",
                &name,
                string(poetry, "version").as_deref(),
                true,
            ));
        }
        let mut tables: Vec<&toml::Table> = Vec::new();
        for key in ["dependencies", "dev-dependencies"] {
            tables.extend(poetry.get(key).and_then(toml::Value::as_table));
        }
        if let Some(groups) = poetry.get("group").and_then(toml::Value::as_table) {
            for group in groups.values() {
                tables.extend(group.get("dependencies").and_then(toml::Value::as_table));
            }
        }
        for dependencies in tables {
            for (name, spec) in dependencies {
                if name == "python" {
                    continue;
                }
                let version = match spec {
                    toml::Value::String(v) => Some(v.as_str()),
                    toml::Value::Table(t) => t.get("version").and_then(toml::Value::as_str),
                    _ => None,
                };
                out.push(Coordinate::new(Ecosystem::Pypi, "", name, version, false));
            }
        }
    }
    Some(out)
}

/// `setup.cfg`'s metadata and `install_requires`.
pub fn setup_cfg(text: &str) -> Vec<Coordinate> {
    let mut section = String::new();
    let mut values: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    let mut current: Option<(String, String)> = None;
    for line in text.lines() {
        if line.trim_start().starts_with(['#', ';']) || line.trim().is_empty() {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            if let Some(key) = &current {
                values
                    .entry(key.clone())
                    .or_default()
                    .push(line.trim().to_owned());
            }
            continue;
        }
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            name.trim().clone_into(&mut section);
            current = None;
        } else if let Some((key, value)) = line.split_once('=').or_else(|| line.split_once(':')) {
            let key = (section.clone(), key.trim().to_owned());
            let entry = values.entry(key.clone()).or_default();
            if !value.trim().is_empty() {
                entry.push(value.trim().to_owned());
            }
            current = Some(key);
        }
    }
    let get = |s: &str, k: &str| values.get(&(s.to_owned(), k.to_owned()));
    let mut out = Vec::new();
    if let Some([name]) = get("metadata", "name").map(Vec::as_slice) {
        let version = match get("metadata", "version").map(Vec::as_slice) {
            Some([v]) if !v.starts_with("attr:") && !v.starts_with("file:") => Some(v.as_str()),
            _ => None,
        };
        out.push(Coordinate::new(Ecosystem::Pypi, "", name, version, true));
    }
    for requirement in get("options", "install_requires").into_iter().flatten() {
        if let Some((name, version)) = pep508(requirement) {
            out.push(Coordinate::new(
                Ecosystem::Pypi,
                "",
                &name,
                version.as_deref(),
                false,
            ));
        }
    }
    out
}

// --- Ada ----------------------------------------------------------------------------

/// `alire.toml`'s crate and dependencies; `None` for TOML that cannot be read.
pub fn alire(text: &str) -> Option<Vec<Coordinate>> {
    let table: toml::Table = text.parse().ok()?;
    let mut out = Vec::new();
    if let Some(name) = table.get("name").and_then(toml::Value::as_str) {
        let version = table.get("version").and_then(toml::Value::as_str);
        out.push(Coordinate::new(Ecosystem::Alire, "", name, version, true));
    }
    for depends in table
        .get("depends-on")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
    {
        for (name, spec) in depends.as_table().into_iter().flatten() {
            out.push(Coordinate::new(
                Ecosystem::Alire,
                "",
                name,
                spec.as_str(),
                false,
            ));
        }
    }
    Some(out)
}

/// A GNAT project file's name and the projects it withs; `None` for one that names no
/// project.
pub fn gpr(text: &str) -> Option<Vec<Coordinate>> {
    let mut out = Vec::new();
    let mut project = None;
    for line in text.lines() {
        let line = line.split("--").next().unwrap_or("").trim();
        if let Some(rest) = line.strip_prefix("with ") {
            for item in rest.trim_end_matches(';').split(',') {
                let Some(file) = script_literal(item.trim()) else {
                    continue;
                };
                let stem = file_name(&file);
                let stem = stem.strip_suffix(".gpr").unwrap_or(stem);
                out.push(Coordinate::new(Ecosystem::Gpr, "", stem, None, false));
            }
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        if project.is_none()
            && let Some(at) = words.iter().position(|w| w.eq_ignore_ascii_case("project"))
            && words
                .get(at + 2)
                .is_some_and(|w| w.eq_ignore_ascii_case("is"))
            && words[..at].iter().all(|w| {
                ["library", "aggregate", "abstract"]
                    .iter()
                    .any(|k| w.eq_ignore_ascii_case(k))
            })
        {
            project = Some(words[at + 1].to_owned());
        }
    }
    let project = project?;
    out.push(Coordinate::new(Ecosystem::Gpr, "", &project, None, true));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(coordinates: &[Coordinate]) -> Vec<String> {
        coordinates
            .iter()
            .map(|c| {
                let key = artifact_key(c.ecosystem, &c.group, &c.name).expect("a key");
                let side = if c.provides { "provides" } else { "consumes" };
                match &c.version {
                    Some(v) => format!("{side} {key}@{v}"),
                    None => format!("{side} {key}"),
                }
            })
            .collect()
    }

    #[test]
    fn maven_substitutes_its_own_properties_only() {
        let pom = "<project><parent><groupId>com.acme</groupId><version>2.0</version></parent><artifactId>shop</artifactId><properties><lib.version>1.4</lib.version></properties><dependencies><dependency><groupId>org.x</groupId><artifactId>lib</artifactId><version>${lib.version}</version></dependency><dependency><groupId>org.y</groupId><artifactId>other</artifactId><version>${from.parent}</version></dependency></dependencies><dependencyManagement><dependencies><dependency><groupId>org.z</groupId><artifactId>managed</artifactId><version>9</version></dependency></dependencies></dependencyManagement></project>";
        assert_eq!(
            keys(&maven(pom).expect("parsed")),
            [
                "provides maven:com.acme:shop@2.0",
                "consumes maven:org.x:lib@1.4",
                "consumes maven:org.y:other"
            ]
        );
        assert_eq!(maven("<project><unclosed></project>"), None);
        assert_eq!(maven("<!DOCTYPE p [<!ENTITY e \"x\">]><project/>"), None);
    }

    #[test]
    fn gradle_reads_literals_and_withholds_code() {
        let build = "group = 'com.acme'\nversion = \"1.0\"\ndependencies {\n  implementation 'org.x:lib:1.2'\n  implementation(\"org.y:kt:2.0\")\n  implementation \"org.z:dyn:$v\"\n  implementation libs.catalog\n  api group: 'org.m', name: 'mapped', version: '3'\n  implementation(platform(\"org.b:bom:4\"))\n}\n";
        assert_eq!(
            keys(&gradle(build, Some("shop"))),
            [
                "consumes maven:org.x:lib@1.2",
                "consumes maven:org.y:kt@2.0",
                "consumes maven:org.m:mapped@3",
                "consumes maven:org.b:bom@4",
                "provides maven:com.acme:shop@1.0",
            ]
        );
        assert_eq!(
            keys(&gradle(build, None)).len(),
            4,
            "no name: no provided coordinate"
        );
    }

    #[test]
    fn npm_cargo_go_python_and_ada() {
        let npm = npm(r#"{"name":"@acme/web","version":"1.0.0","dependencies":{"lodash":"^4.17.21","local":"file:../x"}}"#).expect("json");
        assert_eq!(
            keys(&npm),
            [
                "provides npm:@acme:web@1.0.0",
                "consumes npm::local",
                "consumes npm::lodash@^4.17.21"
            ]
        );
        let cargo = cargo("[package]\nname = \"svc\"\nversion = \"0.1.0\"\n[dependencies]\nserde = \"1\"\nrenamed = { package = \"real\", version = \"2\" }\nlocal = { path = \"../l\" }\n").expect("toml");
        assert_eq!(
            keys(&cargo),
            [
                "provides cargo::svc@0.1.0",
                "consumes cargo::local",
                "consumes cargo::real@2",
                "consumes cargo::serde@1"
            ]
        );
        let go = go_mod(
            "module example.com/acme/svc\n\ngo 1.22\n\nrequire (\n  github.com/a/b v1.2.3 // indirect\n)\nrequire example.com/c v0.1.0\n",
        );
        assert_eq!(
            keys(&go),
            [
                "provides go::example.com/acme/svc",
                "consumes go::github.com/a/b@v1.2.3",
                "consumes go::example.com/c@v0.1.0"
            ]
        );
        let py = pyproject("[project]\nname = \"svc\"\ndynamic = [\"version\"]\ndependencies = [\"requests[socks]>=2.31 ; python_version>'3.8'\", \"pkg @ https://api.example.com/y.whl\"]\n").expect("toml");
        assert_eq!(
            keys(&py),
            [
                "provides pypi::svc",
                "consumes pypi::requests@>=2.31",
                "consumes pypi::pkg"
            ]
        );
        let cfg = setup_cfg(
            "[metadata]\nname = svc\nversion = 1.2\n[options]\ninstall_requires =\n    flask>=2\n    click\n",
        );
        assert_eq!(
            keys(&cfg),
            [
                "provides pypi::svc@1.2",
                "consumes pypi::flask@>=2",
                "consumes pypi::click"
            ]
        );
        let gpr = gpr("with \"lib/common.gpr\";\nlibrary project Shop is\n   for Source_Dirs use (\"src\");\nend Shop;\n").expect("gpr");
        assert_eq!(keys(&gpr), ["consumes gpr::common", "provides gpr::Shop"]);
    }
}
