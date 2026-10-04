//! What an extracted import binds and where it leads, per language.
//!
//! The engine records an import as the text of the module it names and the local
//! name it binds, in each language's own shape. This module reads those shapes and
//! resolves the module into the repository, conservatively: an import is internal
//! only when a file or module of the repository answers it, external only when the
//! language's rules place it outside the repository, unresolved-internal when it
//! names the repository (relatively, or by the repository's own module path or
//! alias) and nothing there answers, and unclassified when there is no evidence
//! either way. Several equally valid internal answers stay several.

use std::collections::{BTreeMap, BTreeSet};

use pdx_engine::Import;

use crate::languages::Language;

use super::metadata::{dir_of, join};
use super::modules::{self, Layout, SCRIPT_EXTENSIONS, is_within};
use super::registry::{ImportTarget, InternalTarget, ModuleKey, Sources};

/// What import resolution looks things up in.
pub(crate) struct Lookup<'a> {
    pub(crate) sources: &'a Sources<'a>,
    pub(crate) layout: &'a Layout,
    /// Each file's modules.
    pub(crate) file_modules: &'a BTreeMap<String, Vec<ModuleKey>>,
    /// Each module's files.
    pub(crate) module_files: &'a BTreeMap<ModuleKey, BTreeSet<String>>,
    /// Declared namespaces by language and comparison key.
    pub(crate) namespaces: &'a BTreeMap<(&'static str, String), ModuleKey>,
}

impl Lookup<'_> {
    /// A target that is this file.
    fn file_target(&self, path: &str, member: Option<String>) -> InternalTarget {
        let module = match self.file_modules.get(path).map(Vec::as_slice) {
            Some([one]) => Some(one.clone()),
            _ => None,
        };
        InternalTarget {
            module,
            files: vec![path.to_owned()],
            member,
        }
    }

    /// A target that is this module.
    fn module_target(&self, module: &ModuleKey, member: Option<String>) -> InternalTarget {
        InternalTarget {
            module: Some(module.clone()),
            files: self
                .module_files
                .get(module)
                .map(|f| f.iter().cloned().collect())
                .unwrap_or_default(),
            member,
        }
    }
}

/// One answer, several, or what to say when there is none.
fn decide(mut found: Vec<InternalTarget>, otherwise: ImportTarget) -> ImportTarget {
    found.sort();
    found.dedup();
    match found.len() {
        0 => otherwise,
        1 => ImportTarget::Internal(found.remove(0)),
        _ => ImportTarget::InternalCandidates(found),
    }
}

/// The local names `import` binds and where it leads, for a file of `language` at
/// `path`.
pub(crate) fn interpret(
    lookup: &Lookup<'_>,
    language: &'static Language,
    path: &str,
    import: &Import,
) -> (Vec<String>, ImportTarget) {
    let text = import.module_text.as_str();
    let name = import.imported_name.as_deref();
    match language.id {
        "python" => (
            name.filter(|n| is_identifier(n))
                .map(str::to_owned)
                .into_iter()
                .collect(),
            python(lookup, language, path, text),
        ),
        "typescript" | "javascript" => (
            name.filter(|n| is_identifier(n))
                .map(str::to_owned)
                .into_iter()
                .collect(),
            script(lookup, path, text),
        ),
        "go" => (
            name.filter(|n| !matches!(*n, "_" | ".") && is_identifier(n))
                .map(str::to_owned)
                .into_iter()
                .collect(),
            go(lookup, text),
        ),
        "java" | "kotlin" => {
            // Kotlin's `import a.b.c as d` arrives as one text, alias included.
            let (module, binding) = match text.split_once(" as ") {
                Some((m, alias)) => (m.trim(), Some(alias.trim())),
                None => (text, name),
            };
            (
                binding
                    .filter(|n| is_identifier(n))
                    .map(str::to_owned)
                    .into_iter()
                    .collect(),
                declared(lookup, language.id, module, '.'),
            )
        }
        // A `using` names a namespace, whose names it does not list.
        "csharp" => (Vec::new(), declared(lookup, language.id, text, '.')),
        "php" => {
            if looks_like_path(text) {
                (Vec::new(), include(lookup, path, text))
            } else {
                (
                    name.filter(|n| is_identifier(n))
                        .map(str::to_owned)
                        .into_iter()
                        .collect(),
                    declared(lookup, language.id, text, '\\'),
                )
            }
        }
        "c" | "cpp" | "objc" | "protobuf" => (Vec::new(), include(lookup, path, text)),
        "rust" => rust(lookup, language, path, text),
        // No evidence: Perl's packages are not recorded, nor are the module systems of
        // the other structural languages resolved.
        _ => (Vec::new(), ImportTarget::Unclassified),
    }
}

/// Whether `s` can be a name in code: letters, digits, `_` and `$`, not leading with a
/// digit.
fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

/// Whether an import's text is a path to a file rather than a module name.
fn looks_like_path(text: &str) -> bool {
    text.contains('/')
        || std::path::Path::new(text)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("php") || e.eq_ignore_ascii_case("inc"))
}

// --- Python -------------------------------------------------------------------------

/// A Python import. The engine writes `from P import N` as `P.N`, and `P` keeps its
/// leading dots, so `from . import x` reads as `..x`, which `from ..x import *` also
/// reads as: both readings are tried, the second naming a module and nothing else.
fn python(lookup: &Lookup<'_>, language: &Language, path: &str, text: &str) -> ImportTarget {
    let dots = text.chars().take_while(|c| *c == '.').count();
    let rest = &text[dots..];
    if dots == 0 {
        let found = python_lookup(lookup, rest, None, true);
        if !found.is_empty() {
            return decide(found, ImportTarget::External);
        }
        // A package of the repository's own that lacks the module named is not a
        // third-party package.
        let top = rest.split('.').next().unwrap_or(rest);
        let owned = lookup.module_files.keys().any(|k| {
            k.language == "python" && (k.name == top || k.name.starts_with(&format!("{top}.")))
        });
        return if owned {
            ImportTarget::UnresolvedInternal
        } else {
            ImportTarget::External
        };
    }
    let Some((scope, module)) = modules::python_module(lookup.layout, path, language) else {
        return ImportTarget::UnresolvedInternal;
    };
    let is_package = path.ends_with("/__init__.py") || path == "__init__.py";
    // The package a relative import starts from.
    let package: Vec<&str> = if is_package {
        module.split('.').collect()
    } else {
        let parts: Vec<&str> = module.split('.').collect();
        parts[..parts.len() - 1].to_vec()
    };
    let base = |level: usize| -> Option<String> {
        let keep = package.len().checked_sub(level.checked_sub(1)?)?;
        (keep > 0).then(|| package[..keep].join("."))
    };
    // Each reading, and whether its last part may be a member rather than a module:
    // with one part after the dots, the reading at that level is `from <dots>X import
    // *`, whose `X` is a module.
    let single = !rest.is_empty() && !rest.contains('.');
    let mut readings = Vec::new();
    if let Some(b) = base(dots) {
        let dotted = if rest.is_empty() {
            b
        } else {
            format!("{b}.{rest}")
        };
        readings.push((dotted, !single && !rest.is_empty()));
    }
    if dots >= 2
        && single
        && let Some(b) = base(dots - 1)
    {
        readings.push((format!("{b}.{rest}"), true));
    }
    let found = readings
        .iter()
        .flat_map(|(r, member)| python_lookup(lookup, r, Some(&scope), *member))
        .collect();
    decide(found, ImportTarget::UnresolvedInternal)
}

/// The modules a dotted name answers: the module itself, else, when `member` allows,
/// its parent with the last part as a member. Within one scope when given.
fn python_lookup(
    lookup: &Lookup<'_>,
    dotted: &str,
    scope: Option<&str>,
    member: bool,
) -> Vec<InternalTarget> {
    let modules_named = |name: &str| -> Vec<&ModuleKey> {
        lookup
            .module_files
            .keys()
            .filter(|k| k.language == "python" && k.name == name)
            .filter(|k| scope.is_none_or(|s| k.scope == s))
            .collect()
    };
    let exact = modules_named(dotted);
    if !exact.is_empty() {
        return exact
            .into_iter()
            .map(|k| lookup.module_target(k, None))
            .collect();
    }
    if !member {
        return Vec::new();
    }
    match dotted.rsplit_once('.') {
        Some((parent, member)) => modules_named(parent)
            .into_iter()
            .map(|k| lookup.module_target(k, Some(member.to_owned())))
            .collect(),
        None => Vec::new(),
    }
}

// --- TypeScript and JavaScript ------------------------------------------------------

/// A TypeScript or JavaScript import: relative to the file, through the nearest
/// `tsconfig.json`'s aliases and base URL as the engine applies them, or a package of
/// the repository's own; anything else is a package from outside.
fn script(lookup: &Lookup<'_>, path: &str, spec: &str) -> ImportTarget {
    if spec == "." || spec == ".." || spec.starts_with("./") || spec.starts_with("../") {
        return match join(dir_of(path), spec) {
            Some(p) => decide(script_files(lookup, &p), ImportTarget::UnresolvedInternal),
            None => ImportTarget::UnresolvedInternal,
        };
    }
    if let Some(scope) = lookup
        .layout
        .alias_scopes
        .iter()
        .find(|s| is_within(path, &s.dir_prefix))
    {
        for alias in &scope.aliases {
            let target = if alias.has_wildcard {
                spec.strip_prefix(alias.alias_prefix.as_str())
                    .and_then(|r| r.strip_suffix(alias.alias_suffix.as_str()))
                    .filter(|_| spec.len() >= alias.alias_prefix.len() + alias.alias_suffix.len())
                    .map(|wild| format!("{}{wild}{}", alias.target_prefix, alias.target_suffix))
            } else {
                (spec == alias.alias_prefix).then(|| alias.target_prefix.clone())
            };
            if let Some(target) = target {
                return decide(
                    script_files(lookup, &target),
                    ImportTarget::UnresolvedInternal,
                );
            }
        }
        if let Some(base) = &scope.base_url
            && !spec.starts_with('@')
            && spec.contains('/')
            && let Some(p) = join(if base == "." { "" } else { base }, spec)
        {
            let found = script_files(lookup, &p);
            if !found.is_empty() {
                return decide(found, ImportTarget::External);
            }
        }
    }
    for package in &lookup.layout.npm_packages {
        if spec == package.name {
            return decide(
                script_files(lookup, &package.entry),
                ImportTarget::UnresolvedInternal,
            );
        }
        if let Some(sub) = spec.strip_prefix(&format!("{}/", package.name)) {
            return match join(&package.dir, sub) {
                Some(p) => decide(script_files(lookup, &p), ImportTarget::UnresolvedInternal),
                None => ImportTarget::UnresolvedInternal,
            };
        }
    }
    ImportTarget::External
}

/// The script files a path names: the file itself, then with each extension left
/// off (a `.js` name also as the `.ts` it is compiled from), then a directory's
/// index. Every file found at the first of those that finds any.
fn script_files(lookup: &Lookup<'_>, p: &str) -> Vec<InternalTarget> {
    let exists = |f: &str| lookup.sources.files.contains_key(f);
    let found = |files: Vec<String>| -> Vec<InternalTarget> {
        files.iter().map(|f| lookup.file_target(f, None)).collect()
    };
    if SCRIPT_EXTENSIONS.iter().any(|e| p.ends_with(e)) && exists(p) {
        return found(vec![p.to_owned()]);
    }
    let compiled_from: [(&str, &[&str]); 4] = [
        (".js", &[".ts", ".tsx"]),
        (".jsx", &[".tsx"]),
        (".mjs", &[".mts"]),
        (".cjs", &[".cts"]),
    ];
    for (js, ts) in compiled_from {
        if let Some(stem) = p.strip_suffix(js) {
            let files: BTreeSet<String> = ts
                .iter()
                .map(|t| format!("{stem}{t}"))
                .filter(|f| exists(f))
                .collect();
            if !files.is_empty() {
                return found(files.into_iter().collect());
            }
        }
    }
    let with_ext: Vec<String> = SCRIPT_EXTENSIONS
        .iter()
        .map(|e| format!("{p}{e}"))
        .filter(|f| exists(f))
        .collect();
    if !with_ext.is_empty() {
        return found(with_ext);
    }
    let index: Vec<String> = SCRIPT_EXTENSIONS
        .iter()
        .filter_map(|e| join(p, &format!("index{e}")))
        .filter(|f| exists(f))
        .collect();
    found(index)
}

// --- Go -------------------------------------------------------------------------

/// A Go import: inside the repository when a `go.mod` of it owns the import path's
/// prefix, the package then being the directory below that module's root.
fn go(lookup: &Lookup<'_>, spec: &str) -> ImportTarget {
    let owner = lookup
        .layout
        .go_modules
        .iter()
        .filter(|(_, m)| spec == m || spec.starts_with(&format!("{m}/")))
        .max_by_key(|(_, m)| m.len());
    let Some((dir, module)) = owner else {
        return ImportTarget::External;
    };
    let below = spec[module.len()..].trim_start_matches('/');
    let Some(target_dir) = join(dir, below) else {
        return ImportTarget::UnresolvedInternal;
    };
    let files: Vec<String> = lookup
        .sources
        .files
        .iter()
        .filter(|(p, s)| {
            dir_of(p) == target_dir && s.discovered.language.is_some_and(|l| l.id == "go")
        })
        .map(|(p, _)| p.clone())
        .collect();
    let key = files
        .first()
        .and_then(|f| modules::go_module(lookup.layout, f));
    match key {
        Some(key) => ImportTarget::Internal(InternalTarget {
            module: Some(key),
            files,
            member: None,
        }),
        None => ImportTarget::UnresolvedInternal,
    }
}

// --- declared packages and namespaces -----------------------------------------------

/// The key a declared namespace is compared by: PHP's are case-insensitive.
pub(crate) fn namespace_key(language: &str, name: &str) -> String {
    let name = name.trim_start_matches('\\');
    if language == "php" {
        name.to_ascii_lowercase()
    } else {
        name.to_owned()
    }
}

/// An import of a declared package or namespace (Java, Kotlin, C#, PHP): the
/// namespace it names, or the one its last part is a member of. Every file of the
/// language says what it declares, so a name no file declares is from outside.
fn declared(lookup: &Lookup<'_>, language: &'static str, text: &str, sep: char) -> ImportTarget {
    let text = text.trim_start_matches(sep);
    let mut found = Vec::new();
    if let Some(module) = lookup
        .namespaces
        .get(&(language, namespace_key(language, text)))
    {
        found.push(lookup.module_target(module, None));
    }
    if let Some((parent, member)) = text.rsplit_once(sep)
        && let Some(module) = lookup
            .namespaces
            .get(&(language, namespace_key(language, parent)))
    {
        found.push(lookup.module_target(module, Some(member.to_owned())));
    }
    // A static import names a member of a type: `q.Util.helper` imports `helper` of
    // the type `Util` that the package `q` defines. Only the package's files that
    // define such a type are the target.
    if found.is_empty()
        && let Some((parent, member)) = text.rsplit_once(sep)
        && let Some((package, ty)) = parent.rsplit_once(sep)
        && let Some(module) = lookup
            .namespaces
            .get(&(language, namespace_key(language, package)))
    {
        let files: Vec<String> = lookup
            .module_files
            .get(module)
            .into_iter()
            .flatten()
            .filter(|f| {
                lookup.sources.extract(f).is_some_and(|e| {
                    e.definitions
                        .iter()
                        .any(|d| d.name == ty && modules::is_type(d.kind))
                })
            })
            .cloned()
            .collect();
        if !files.is_empty() {
            found.push(InternalTarget {
                module: Some(module.clone()),
                files,
                member: Some(member.to_owned()),
            });
        }
    }
    decide(found, ImportTarget::External)
}

// --- includes -------------------------------------------------------------------

/// A file include (C, C++, Objective-C, protobuf, PHP's `require`): beside the
/// including file, else from the repository root, else the one file whose path ends
/// with it. A relative include that finds nothing is the repository's own; any other
/// names a file the checkout does not hold.
fn include(lookup: &Lookup<'_>, path: &str, spec: &str) -> ImportTarget {
    let exists = |f: &str| lookup.sources.files.contains_key(f);
    for candidate in [join(dir_of(path), spec), join("", spec)]
        .into_iter()
        .flatten()
    {
        if exists(&candidate) {
            return ImportTarget::Internal(lookup.file_target(&candidate, None));
        }
    }
    let suffix = format!("/{}", spec.trim_start_matches("./"));
    let found: Vec<InternalTarget> = lookup
        .sources
        .files
        .keys()
        .filter(|f| f.ends_with(&suffix) || *f == spec)
        .map(|f| lookup.file_target(f, None))
        .collect();
    let relative = spec.starts_with("./") || spec.starts_with("../");
    decide(
        found,
        if relative {
            ImportTarget::UnresolvedInternal
        } else {
            ImportTarget::External
        },
    )
}

// --- Rust -----------------------------------------------------------------------

/// The crates of the standard distribution.
const RUST_STANDARD: [&str; 5] = ["std", "core", "alloc", "proc_macro", "test"];

/// A `use`: its bindings, from a brace group if it has one, and where its path
/// leads.
fn rust(
    lookup: &Lookup<'_>,
    language: &'static Language,
    path: &str,
    text: &str,
) -> (Vec<String>, ImportTarget) {
    let text = text.trim().trim_start_matches("::");
    let (prefix, bindings) = if let Some(open) = text.find('{') {
        let prefix = text[..open].trim_end_matches("::").trim();
        let group = text[open + 1..]
            .trim_end()
            .strip_suffix('}')
            .unwrap_or(&text[open + 1..]);
        let mut bindings = Vec::new();
        group_bindings(prefix, group, &mut bindings);
        (prefix, bindings)
    } else {
        let (p, alias) = match text.split_once(" as ") {
            Some((p, a)) => (p.trim(), Some(a.trim())),
            None => (text, None),
        };
        let last = p.rsplit("::").next().unwrap_or(p);
        let binding = alias.unwrap_or(last);
        let bindings = if binding == "*" || binding == "_" || binding.is_empty() {
            Vec::new()
        } else {
            vec![binding.to_owned()]
        };
        (p, bindings)
    };
    let segments: Vec<&str> = prefix.split("::").filter(|s| !s.is_empty()).collect();
    (bindings, rust_path(lookup, language, path, &segments))
}

/// The names a brace group binds: each item's alias or last segment, `self` binding
/// the group's own last segment, a glob binding none; nested groups too.
fn group_bindings(prefix: &str, group: &str, out: &mut Vec<String>) {
    let mut depth = 0;
    let mut start = 0;
    let mut items = Vec::new();
    for (i, c) in group.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                items.push(&group[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    items.push(&group[start..]);
    for item in items.into_iter().map(str::trim).filter(|i| !i.is_empty()) {
        if let Some(open) = item.find('{') {
            let inner = item[open + 1..]
                .trim_end()
                .strip_suffix('}')
                .unwrap_or(&item[open + 1..]);
            group_bindings(item[..open].trim_end_matches("::"), inner, out);
            continue;
        }
        let (p, alias) = match item.split_once(" as ") {
            Some((p, a)) => (p.trim(), Some(a.trim())),
            None => (item, None),
        };
        let binding = match (alias, p) {
            (Some(a), _) => a,
            (None, "self") => prefix.rsplit("::").next().unwrap_or(prefix),
            (None, p) => p.rsplit("::").next().unwrap_or(p),
        };
        if !matches!(binding, "*" | "_" | "") {
            out.push(binding.to_owned());
        }
    }
}

/// Where a `use` path leads from the file at `path`.
fn rust_path(
    lookup: &Lookup<'_>,
    language: &'static Language,
    path: &str,
    segments: &[&str],
) -> ImportTarget {
    let Some(first) = segments.first() else {
        return ImportTarget::Unclassified;
    };
    if RUST_STANDARD.contains(first) {
        return ImportTarget::External;
    }
    let roots = match language.module_rule {
        crate::languages::ModuleRule::ModTree { crate_roots } => crate_roots,
        _ => &[],
    };
    let Some(here) = modules::rust_module(lookup.layout, path, roots, language) else {
        return ImportTarget::Unclassified;
    };
    let module = |scope: &str, name: &str| ModuleKey {
        language: language.id,
        scope: scope.to_owned(),
        name: name.to_owned(),
    };
    // The module the path starts from, and the segments after it.
    let (scope, base, rest): (String, String, &[&str]) = match *first {
        "crate" => (here.scope.clone(), "crate".into(), &segments[1..]),
        "self" => (here.scope.clone(), here.name.clone(), &segments[1..]),
        "super" => {
            let supers = segments.iter().take_while(|s| **s == "super").count();
            let mut base: Vec<&str> = here.name.split("::").collect();
            if base.len() <= supers {
                return ImportTarget::UnresolvedInternal;
            }
            base.truncate(base.len() - supers);
            (here.scope.clone(), base.join("::"), &segments[supers..])
        }
        other => {
            let normalised = other.replace('-', "_");
            let krate = lookup.layout.crates.iter().find(|c| c.dir == here.scope);
            let dependency = krate.and_then(|c| {
                c.dependencies
                    .iter()
                    .find(|d| d.name.replace('-', "_") == normalised)
            });
            let member = lookup.layout.crates.iter().find(|c| {
                c.manifest
                    .package_name
                    .as_deref()
                    .is_some_and(|n| n.replace('-', "_") == normalised)
            });
            if let Some(d) = dependency {
                match d
                    .path
                    .as_deref()
                    .and_then(|p| lookup.layout.crates.iter().find(|c| c.dir == p))
                {
                    Some(c) => (c.dir.clone(), "crate".into(), &segments[1..]),
                    None if d.path.is_some() => return ImportTarget::UnresolvedInternal,
                    None => return ImportTarget::External,
                }
            } else if let Some(c) = member {
                (c.dir.clone(), "crate".into(), &segments[1..])
            } else if lookup
                .module_files
                .contains_key(&module(&here.scope, &format!("{}::{other}", here.name)))
            {
                (here.scope.clone(), here.name.clone(), segments)
            } else if lookup
                .module_files
                .contains_key(&module(&here.scope, &format!("crate::{other}")))
            {
                (here.scope.clone(), "crate".into(), segments)
            } else {
                return ImportTarget::Unclassified;
            }
        }
    };
    let joined = |parts: &[&str]| -> String {
        std::iter::once(base.as_str())
            .chain(parts.iter().copied())
            .collect::<Vec<_>>()
            .join("::")
    };
    let whole = module(&scope, &joined(rest));
    if lookup.module_files.contains_key(&whole) {
        return ImportTarget::Internal(lookup.module_target(&whole, None));
    }
    if let Some((member, parent)) = rest.split_last() {
        let parent = module(&scope, &joined(parent));
        if lookup.module_files.contains_key(&parent) {
            return ImportTarget::Internal(
                lookup.module_target(&parent, Some((*member).to_owned())),
            );
        }
    }
    ImportTarget::UnresolvedInternal
}
