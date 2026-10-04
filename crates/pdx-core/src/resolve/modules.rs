//! Module membership: each `ModuleRule` of the language matrix, implemented from the
//! evidence that exists for it, and nothing guessed where it does not.
//!
//! The matrix says which rule a language follows; this module only implements the
//! rules. Where a rule's evidence is missing (a declaration the engine does not
//! record, a file outside every `go.mod` or crate), the module is `None`.

use std::collections::BTreeSet;

use pdx_engine::{
    AliasScope, CrateDependency, DefinitionKind, FileExtract, NamespaceEvidence, namespace_evidence,
};

use crate::languages::{Language, ModuleRule};

use super::metadata::{self, CargoManifest, dir_of, file_name, join};
use super::registry::{ModuleKey, RegistryError, Sources};

/// What the repository declares about its module layout, read once.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Layout {
    /// Directories that hold an `__init__.py`: Python packages.
    pub(crate) python_packages: BTreeSet<String>,
    /// Each `go.mod`'s directory and module path, by directory.
    pub(crate) go_modules: Vec<(String, String)>,
    /// Each crate manifest, by directory.
    pub(crate) crates: Vec<Crate>,
    /// The `tsconfig.json` alias scopes, nearest first as the engine orders them.
    pub(crate) alias_scopes: Vec<AliasScope>,
    /// The packages `package.json` files name, by name.
    pub(crate) npm_packages: Vec<NpmPackage>,
}

/// One `Cargo.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Crate {
    /// Its directory, "" for the root.
    pub(crate) dir: String,
    /// What it declares.
    pub(crate) manifest: CargoManifest,
    /// Its dependencies, with their repository paths.
    pub(crate) dependencies: Vec<CrateDependency>,
}

/// One package a `package.json` names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NpmPackage {
    /// Its name.
    pub(crate) name: String,
    /// Its directory.
    pub(crate) dir: String,
    /// The file it is imported as, or its directory when no entry it declares, nor an
    /// index, is a file of the repository.
    pub(crate) entry: String,
}

/// Reads the layout from the repository's metadata files.
pub(crate) fn layout(sources: &Sources<'_>) -> Result<Layout, RegistryError> {
    let mut layout = Layout::default();
    let parse_error = |path: &str, reason: String| RegistryError::MetadataParse {
        path: path.to_owned(),
        reason,
    };
    for path in sources.files.keys() {
        match file_name(path) {
            "__init__.py" => {
                layout.python_packages.insert(dir_of(path).to_owned());
            }
            "go.mod" => {
                if let Some(text) = metadata::read(sources, path)? {
                    let module = metadata::parse_go_mod(&text)
                        .ok_or_else(|| parse_error(path, "no `module` directive".into()))?;
                    layout.go_modules.push((dir_of(path).to_owned(), module));
                }
            }
            "tsconfig.json" => {
                if let Some(scope) = metadata::alias_scope(sources, path)? {
                    layout.alias_scopes.push(scope);
                }
            }
            "package.json" => {
                if let Some(text) = metadata::read(sources, path)? {
                    let (name, entries) =
                        metadata::parse_package_json(&text).map_err(|r| parse_error(path, r))?;
                    if let Some(name) = name {
                        let dir = dir_of(path).to_owned();
                        let entry = npm_entry(sources, &dir, &entries);
                        layout.npm_packages.push(NpmPackage { name, dir, entry });
                    }
                }
            }
            "Cargo.toml" => {
                if let Some(text) = metadata::read(sources, path)? {
                    let manifest =
                        metadata::parse_cargo(&text).map_err(|r| parse_error(path, r))?;
                    layout.crates.push(Crate {
                        dir: dir_of(path).to_owned(),
                        manifest,
                        dependencies: Vec::new(),
                    });
                }
            }
            _ => {}
        }
    }
    // Inherited dependencies take their path from the nearest workspace root above.
    let roots: Vec<(String, CargoManifest)> = layout
        .crates
        .iter()
        .filter(|c| c.manifest.is_workspace_root)
        .map(|c| (c.dir.clone(), c.manifest.clone()))
        .collect();
    for c in &mut layout.crates {
        let workspace = roots
            .iter()
            .filter(|(dir, _)| is_within(&c.dir, dir))
            .max_by_key(|(dir, _)| dir.len())
            .map(|(dir, m)| (dir.as_str(), m));
        c.dependencies = metadata::crate_dependencies(&c.dir, &c.manifest, workspace);
    }
    layout.alias_scopes.sort_by(|x, y| {
        y.dir_prefix
            .len()
            .cmp(&x.dir_prefix.len())
            .then_with(|| x.dir_prefix.cmp(&y.dir_prefix))
    });
    layout
        .npm_packages
        .sort_by(|x, y| x.name.cmp(&y.name).then_with(|| x.dir.cmp(&y.dir)));
    Ok(layout)
}

/// Whether `path` is `dir` or below it ("" holds everything).
pub(crate) fn is_within(path: &str, dir: &str) -> bool {
    dir.is_empty() || path == dir || path.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
}

/// The extensions a TypeScript or JavaScript import may leave off, in the order a
/// resolver tries them.
pub(crate) const SCRIPT_EXTENSIONS: [&str; 9] = [
    ".ts", ".tsx", ".d.ts", ".js", ".jsx", ".mts", ".cts", ".mjs", ".cjs",
];

/// The first of a package's declared entries that is a file of the repository, then
/// its index, else its directory.
fn npm_entry(sources: &Sources<'_>, dir: &str, entries: &[String]) -> String {
    for entry in entries {
        if let Some(path) = join(dir, entry)
            && sources.files.contains_key(&path)
        {
            return path;
        }
    }
    for base in ["index", "src/index"] {
        for ext in SCRIPT_EXTENSIONS {
            if let Some(path) = join(dir, &format!("{base}{ext}"))
                && sources.files.contains_key(&path)
            {
                return path;
            }
        }
    }
    dir.to_owned()
}

/// The file's path without the matrix extension of its language.
fn stem<'p>(path: &'p str, language: &Language) -> &'p str {
    language
        .extensions
        .iter()
        .filter(|ext| path.ends_with(*ext))
        .max_by_key(|ext| ext.len())
        .map_or(path, |ext| &path[..path.len() - ext.len()])
}

/// Where a file's module comes from.
pub(crate) enum Membership {
    /// One module, or none, for the whole file.
    File(Option<ModuleKey>),
    /// A module, or none, for each definition, in the extraction's order.
    PerDefinition(Vec<Option<ModuleKey>>),
}

/// The module or modules of a file under its language's rule.
pub(crate) fn membership(
    layout: &Layout,
    path: &str,
    language: &'static Language,
    extract: Option<&FileExtract>,
) -> Membership {
    let key = |scope: &str, name: String| ModuleKey {
        language: language.id,
        scope: scope.to_owned(),
        name,
    };
    match language.module_rule {
        ModuleRule::Directory
        | ModuleRule::DirectoryWithPathMappings { .. }
        | ModuleRule::DirectoryPairingHeaders => {
            Membership::File(Some(key("", dir_of(path).to_owned())))
        }
        ModuleRule::File | ModuleRule::FileSectionsAsDocs | ModuleRule::FileKeysOnly => {
            Membership::File(Some(key("", path.to_owned())))
        }
        ModuleRule::DirectoryUnderModulePath { .. } => Membership::File(go_module(layout, path)),
        ModuleRule::DottedPath { .. } => Membership::File(
            python_module(layout, path, language).map(|(scope, name)| key(&scope, name)),
        ),
        ModuleRule::ModTree { crate_roots } => {
            Membership::File(rust_module(layout, path, crate_roots, language))
        }
        ModuleRule::Declaration(_) => {
            let engine_language = language.engine_language(path);
            match (namespace_evidence(engine_language), extract) {
                (_, None) | (NamespaceEvidence::Unrecorded, _) => Membership::File(None),
                (NamespaceEvidence::FileDeclaration, Some(e)) => Membership::File(Some(key(
                    "",
                    e.declared_namespace
                        .as_deref()
                        .map_or_else(String::new, |n| n.trim_start_matches('\\').to_owned()),
                ))),
                (NamespaceEvidence::QualifiedNames, Some(e)) => Membership::PerDefinition(
                    nesting(e, true)
                        .into_iter()
                        .map(|n| n.map(|n| key("", n)))
                        .collect(),
                ),
            }
        }
        ModuleRule::ModuleClassNesting => match extract {
            Some(e) => Membership::PerDefinition(
                nesting(e, false)
                    .into_iter()
                    .map(|n| n.map(|n| key("", n)))
                    .collect(),
            ),
            None => Membership::File(None),
        },
        ModuleRule::UnitName => {
            Membership::File(extract.and_then(unit_name).map(|name| key("", name)))
        }
    }
}

/// Whether a definition kind declares a type.
pub(crate) fn is_type(kind: DefinitionKind) -> bool {
    matches!(
        kind,
        DefinitionKind::Class
            | DefinitionKind::Interface
            | DefinitionKind::Struct
            | DefinitionKind::Trait
            | DefinitionKind::Enum
            | DefinitionKind::TypeAlias
    )
}

/// The file's own module definition's qualified name: the prefix of every other.
fn module_qn(extract: &FileExtract) -> Option<&str> {
    extract
        .definitions
        .iter()
        .find(|d| d.kind == DefinitionKind::Module && d.name == extract.rel_path)
        .map(|d| d.qualified_name.as_str())
}

/// Each definition's nesting, read from its qualified name: what lies between the
/// file's module and the definition's own name, each part split at `::` as well and
/// joined with `::`. With `namespaces_only`, the types that enclose a definition are
/// left out, so a member is in its outermost type's namespace. `None` for the file's
/// module definition and wherever the qualified name does not have that shape.
fn nesting(extract: &FileExtract, namespaces_only: bool) -> Vec<Option<String>> {
    let Some(module) = module_qn(extract) else {
        return vec![None; extract.definitions.len()];
    };
    let raw = |qn: &str, name: &str| -> Option<Vec<String>> {
        let rest = qn.strip_prefix(module)?.strip_prefix('.')?;
        let enclosing = if rest == name {
            ""
        } else {
            rest.strip_suffix(name)?.strip_suffix('.')?
        };
        Some(
            enclosing
                .split('.')
                .flat_map(|part| part.split("::"))
                .filter(|p| !p.is_empty())
                .map(str::to_owned)
                .collect(),
        )
    };
    extract
        .definitions
        .iter()
        .map(|d| {
            if d.kind == DefinitionKind::Module && d.name == extract.rel_path {
                return None;
            }
            if !namespaces_only {
                return raw(&d.qualified_name, &d.name).map(|p| p.join("::"));
            }
            // The outermost type whose qualified name encloses this definition's.
            let outer = extract
                .definitions
                .iter()
                .filter(|t| {
                    is_type(t.kind)
                        && t.qualified_name != d.qualified_name
                        && d.qualified_name
                            .strip_prefix(t.qualified_name.as_str())
                            .is_some_and(|r| r.starts_with('.'))
                })
                .min_by_key(|t| t.qualified_name.len());
            match outer {
                Some(t) => raw(&t.qualified_name, &t.name).map(|p| p.join("::")),
                None => raw(&d.qualified_name, &d.name).map(|p| p.join("::")),
            }
        })
        .collect()
}

/// The unit a file holds: the one package-like definition at its top level, as the
/// engine names it. `None` when there is not exactly one.
fn unit_name(extract: &FileExtract) -> Option<String> {
    let mut units = extract.definitions.iter().filter(|d| {
        d.parent.is_none()
            && matches!(d.kind, DefinitionKind::Class | DefinitionKind::Module)
            && d.name != extract.rel_path
    });
    let unit = units.next()?;
    units.next().is_none().then(|| unit.name.clone())
}

/// A Python file's module: its scope (the directory above its outermost package)
/// and its dotted name. A file outside any package is a top-level module of its own
/// directory.
pub(crate) fn python_module(
    layout: &Layout,
    path: &str,
    language: &Language,
) -> Option<(String, String)> {
    let module_stem = file_name(stem(path, language));
    let mut dir = dir_of(path).to_owned();
    let mut parts: Vec<String> = Vec::new();
    while layout.python_packages.contains(&dir) {
        let (parent, name) = match dir.rfind('/') {
            Some(at) => (dir[..at].to_owned(), dir[at + 1..].to_owned()),
            None => (String::new(), dir.clone()),
        };
        if name.is_empty() {
            break; // the root itself holds __init__.py: not a package name
        }
        parts.push(name);
        dir = parent;
    }
    parts.reverse();
    if module_stem != "__init__" {
        parts.push(module_stem.to_owned());
    }
    if parts.is_empty() {
        return None; // an __init__.py at the root
    }
    Some((dir, parts.join(".")))
}

/// A Go file's package: the module path of the nearest `go.mod` above it, then its
/// directory below that.
pub(crate) fn go_module(layout: &Layout, path: &str) -> Option<ModuleKey> {
    let dir = dir_of(path);
    let (mod_dir, module) = layout
        .go_modules
        .iter()
        .filter(|(d, _)| is_within(dir, d))
        .max_by_key(|(d, _)| d.len())?;
    let below = if mod_dir.is_empty() {
        dir
    } else {
        dir.strip_prefix(mod_dir.as_str())?.trim_start_matches('/')
    };
    Some(ModuleKey {
        language: "go",
        scope: mod_dir.clone(),
        name: if below.is_empty() {
            module.clone()
        } else {
            format!("{module}/{below}")
        },
    })
}

/// A Rust file's module in its crate's tree, from the conventional layout: the crate
/// root (`src/lib.rs` or `src/main.rs`) is `crate`, `src/a.rs` and `src/a/mod.rs` are
/// `crate::a`, `src/a/b.rs` is `crate::a::b`. A binary, test, example or benchmark
/// target's root file is a crate of its own, scoped by its path. Anything else is in
/// no module.
pub(crate) fn rust_module(
    layout: &Layout,
    path: &str,
    crate_roots: &[&str],
    language: &'static Language,
) -> Option<ModuleKey> {
    let krate = layout
        .crates
        .iter()
        .filter(|c| c.manifest.package_name.is_some() && is_within(path, &c.dir))
        .max_by_key(|c| c.dir.len())?;
    let below = if krate.dir.is_empty() {
        path
    } else {
        path.strip_prefix(krate.dir.as_str())?
            .trim_start_matches('/')
    };
    let key = |scope: &str, name: String| ModuleKey {
        language: language.id,
        scope: scope.to_owned(),
        name,
    };
    // Targets with a root file of their own.
    for target in ["src/bin/", "tests/", "examples/", "benches/"] {
        if let Some(rest) = below.strip_prefix(target)
            && !rest.contains('/')
        {
            return Some(key(path, "crate".to_owned()));
        }
    }
    let rel = below.strip_prefix("src/")?;
    if crate_roots.contains(&rel) {
        return Some(key(&krate.dir, "crate".to_owned()));
    }
    let rel = stem(rel, language);
    let mut parts: Vec<&str> = rel.split('/').collect();
    if parts.last() == Some(&"mod") {
        parts.pop();
    }
    if parts.is_empty() || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    Some(key(&krate.dir, format!("crate::{}", parts.join("::"))))
}
