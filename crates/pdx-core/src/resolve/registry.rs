//! The symbol registry: everything Stage 3 looks names up in, built once from Stage 1
//! and Stage 2 and read-only afterwards (specification 4.5, Stage 3).
//!
//! It holds every definition an extraction reported, indexed by short name, by
//! qualified name and by file; the modules of the language matrix's rules and what
//! is in each; each file's imports, with what they bind and where they lead; the
//! class hierarchy the extractions' base classes describe; the repository's module
//! metadata as the engine takes it; and which names reach a file only through
//! imports of external packages. It resolves no call and assigns no band: that is
//! the resolution stages', which read it.
//!
//! Identity is extraction-local: a definition is its file's path and its index in
//! that file's extraction, which is also how the extraction's own facts refer to it.
//! Every list the registry returns has a stated order (by path, then index), so no
//! answer depends on the order files arrived in, a hash's iteration, the filesystem
//! or the workers that extracted them. Names are kept exactly as the engine reported
//! them: nothing is case-folded here.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use pdx_engine::{
    CrateManifest, Definition, DefinitionKind, FileExtract, NamespaceEvidence, PackageEntry,
    ResolutionMetadata, namespace_evidence,
};

use crate::index::discover::{DiscoveredFile, Disposition};
use crate::index::extract::{ExtractError, ExtractReport, FileOutcome};
use crate::languages::Language;

use super::imports::{self, Lookup};
use super::metadata::dir_of;
use super::modules::{self, Layout, Membership};

/// A definition: the file it is in and its index in that file's extraction.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DefinitionRef {
    /// The file's path below the repository root.
    pub path: String,
    /// The definition's index in the file's extraction.
    pub index: u32,
}

/// An import: the file it is in and its index in that file's extraction.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImportRef {
    /// The file's path below the repository root.
    pub path: String,
    /// The import's index in the file's extraction.
    pub index: u32,
}

/// A module, as the language matrix's module rule for its language defines one.
///
/// Its name is in the language's own spelling (`com.acme.shop`, `geo::detail`,
/// `pkg.sub.tool`, `example.com/acme/pkg/foo`, `crate::a::b`, a directory, or a file's
/// path). The scope tells apart modules of the same name under different roots: a
/// Python source root, a `go.mod`'s directory, a crate's directory; "" where a
/// language's names are repository-wide. Not a graph identity: nodes for modules are
/// made later.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModuleKey {
    /// The matrix language.
    pub language: &'static str,
    /// The root the name is relative to.
    pub scope: String,
    /// The module's name; "" for a language's unnamed package or global namespace, or
    /// the repository root directory.
    pub name: String,
}

/// An internal place an import leads to.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InternalTarget {
    /// The module, when the target has exactly one.
    pub module: Option<ModuleKey>,
    /// The files, by path.
    pub files: Vec<String>,
    /// The name imported from the module, when the import names one inside it.
    pub member: Option<String>,
}

/// Where an import leads.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ImportTarget {
    /// Into the repository, to one place.
    Internal(InternalTarget),
    /// Into the repository, to several places the language's rules do not choose
    /// between.
    InternalCandidates(Vec<InternalTarget>),
    /// Outside the repository: a package, library or system file it does not hold.
    External,
    /// Into the repository by its own relative path, module path or alias, where
    /// nothing answers it.
    UnresolvedInternal,
    /// No evidence either way.
    Unclassified,
}

impl ImportTarget {
    /// Whether it leads into the repository, answered or not.
    pub fn is_internal(&self) -> bool {
        matches!(
            self,
            Self::Internal(_) | Self::InternalCandidates(_) | Self::UnresolvedInternal
        )
    }
}

/// One import of a file, as extracted, with what it binds and where it leads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportRecord {
    /// Which import.
    pub import: ImportRef,
    /// The module text the engine recorded, as written.
    pub module_text: String,
    /// The name the engine recorded the import binding, as written.
    pub imported_name: Option<String>,
    /// The alias the engine recorded, if any.
    pub alias: Option<String>,
    /// The local names it binds in the file, where its language's form makes them
    /// known; none for an import that brings in a namespace's names unlisted.
    pub bindings: Vec<String>,
    /// Where it leads.
    pub target: ImportTarget,
}

/// Where a name visible in a file comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameProvenance {
    /// The file defines it at its top level.
    Defined(Vec<DefinitionRef>),
    /// An import that leads into the repository binds it.
    InternalImport(Vec<ImportRef>),
    /// Only imports of external packages bind it: the name is external.
    ExternalImport(Vec<ImportRef>),
    /// Nothing proves where it comes from: no import binds it, or one that does leads
    /// nowhere known. Not evidence that it is external.
    Unknown,
}

/// A base a definition names, and what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaseRelation {
    /// The base as the engine recorded it.
    pub raw: String,
    /// The name looked up, from the raw text with any keyword and type arguments
    /// left out.
    pub name: String,
    /// What it resolved to.
    pub resolution: BaseResolution,
}

/// What a base names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BaseResolution {
    /// One type of the repository.
    Internal(DefinitionRef),
    /// Several, which nothing chooses between.
    Ambiguous(Vec<DefinitionRef>),
    /// A type imported from outside the repository.
    External(Vec<ImportRef>),
    /// Nothing found, and no import says it is from outside.
    Unresolved,
}

/// Why the registry could not be built. Names files by path, never their content.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    /// Stage 1 and Stage 2 do not describe the same files.
    #[error("{path}: discovery and extraction disagree: {detail}")]
    StageMismatch {
        /// The file.
        path: String,
        /// How.
        detail: &'static str,
    },
    /// An extraction refers to a definition it does not have.
    #[error("{path}: {what} names definition {index}, which the file does not have")]
    InvalidDefinitionIndex {
        /// The file.
        path: String,
        /// The fact that refers to it.
        what: &'static str,
        /// The index.
        index: u32,
    },
    /// A definition's parent is not a definition of the file, or its parents form a
    /// cycle.
    #[error("{path}: definition {index} has an invalid parent")]
    InvalidParent {
        /// The file.
        path: String,
        /// The definition.
        index: u32,
    },
    /// A metadata file could not be read.
    #[error("{path}: {source}")]
    MetadataRead {
        /// The file.
        path: String,
        /// Why.
        #[source]
        source: Box<ExtractError>,
    },
    /// A metadata file does not parse.
    #[error("{path}: {reason}")]
    MetadataParse {
        /// The file.
        path: String,
        /// Why.
        reason: String,
    },
    /// A metadata file parses but says something that cannot hold.
    #[error("{path}: {reason}")]
    InvalidMetadata {
        /// The file.
        path: String,
        /// Why.
        reason: String,
    },
    /// A file read again is not the one Stage 2 read.
    #[error("{0}: changed since extraction")]
    SourceChanged(String),
}

/// One file as Stage 1 found it and Stage 2 left it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileState {
    pub(crate) discovered: DiscoveredFile,
    pub(crate) outcome: FileOutcome,
}

/// The checkout and its files, by path.
pub(crate) struct Sources<'a> {
    pub(crate) root: &'a Path,
    pub(crate) files: BTreeMap<String, FileState>,
}

impl Sources<'_> {
    fn extract(&self, path: &str) -> Option<&FileExtract> {
        match &self.files.get(path)?.outcome {
            FileOutcome::Extracted { extract, .. } => Some(extract),
            _ => None,
        }
    }
}

/// The symbol registry of one repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymbolRegistry {
    files: BTreeMap<String, FileState>,
    by_name: BTreeMap<String, Vec<DefinitionRef>>,
    by_qualified_name: BTreeMap<String, Vec<DefinitionRef>>,
    by_file: BTreeMap<String, Vec<DefinitionRef>>,
    file_modules: BTreeMap<String, Vec<ModuleKey>>,
    definition_modules: BTreeMap<DefinitionRef, ModuleKey>,
    module_files: BTreeMap<ModuleKey, BTreeSet<String>>,
    module_definitions: BTreeMap<ModuleKey, Vec<DefinitionRef>>,
    imports: BTreeMap<String, Vec<ImportRecord>>,
    bases: BTreeMap<DefinitionRef, Vec<BaseRelation>>,
    derived: BTreeMap<DefinitionRef, Vec<DefinitionRef>>,
    pairs: BTreeMap<String, String>,
    metadata: ResolutionMetadata,
}

impl SymbolRegistry {
    /// Builds the registry of the checkout at `root` from Stage 1's files and Stage
    /// 2's report on them. Repository metadata (`tsconfig.json`, `package.json`,
    /// `go.mod`, `Cargo.toml`) is read from the checkout, safely; nothing else is read.
    ///
    /// # Errors
    ///
    /// [`RegistryError`] when the two stages disagree on the files, an extraction
    /// refers to a definition it does not have, or metadata cannot be read or parsed
    /// or has changed since Stage 2.
    pub fn build(
        root: &Path,
        discovered: &[DiscoveredFile],
        extracted: ExtractReport,
    ) -> Result<Self, RegistryError> {
        let sources = Sources {
            root,
            files: pair_stages(discovered, extracted)?,
        };
        for (path, state) in &sources.files {
            if let FileOutcome::Extracted { extract, .. } = &state.outcome {
                validate(path, extract)?;
            }
        }
        let layout = modules::layout(&sources)?;
        let mut registry = Self {
            files: BTreeMap::new(),
            by_name: BTreeMap::new(),
            by_qualified_name: BTreeMap::new(),
            by_file: BTreeMap::new(),
            file_modules: BTreeMap::new(),
            definition_modules: BTreeMap::new(),
            module_files: BTreeMap::new(),
            module_definitions: BTreeMap::new(),
            imports: BTreeMap::new(),
            bases: BTreeMap::new(),
            derived: BTreeMap::new(),
            pairs: BTreeMap::new(),
            metadata: ResolutionMetadata::default(),
        };
        registry.index_definitions(&sources);
        registry.assign_modules(&sources, &layout);
        let namespaces = registry.declared_namespaces();
        registry.resolve_imports(&sources, &layout, &namespaces);
        registry.pair_headers(&sources);
        registry.resolve_bases(&sources);
        registry.metadata = registry.metadata_for_engine(&layout);
        registry.files = sources.files;
        Ok(registry)
    }

    fn index_definitions(&mut self, sources: &Sources<'_>) {
        for (path, state) in &sources.files {
            let FileOutcome::Extracted { extract, .. } = &state.outcome else {
                continue;
            };
            let refs: Vec<DefinitionRef> = (0..extract.definitions.len())
                .filter_map(|i| u32::try_from(i).ok())
                .map(|index| DefinitionRef {
                    path: path.clone(),
                    index,
                })
                .collect();
            for (d, r) in extract.definitions.iter().zip(&refs) {
                self.by_name
                    .entry(d.name.clone())
                    .or_default()
                    .push(r.clone());
                self.by_qualified_name
                    .entry(d.qualified_name.clone())
                    .or_default()
                    .push(r.clone());
            }
            self.by_file.insert(path.clone(), refs);
        }
        // Files arrive in path order and definitions in index order, so each list is
        // already in (path, index) order.
    }

    fn assign_modules(&mut self, sources: &Sources<'_>, layout: &Layout) {
        for (path, state) in &sources.files {
            let Some(language) = state.discovered.language else {
                continue;
            };
            let extract = sources.extract(path);
            let mut modules_here = BTreeSet::new();
            match modules::membership(layout, path, language, extract) {
                Membership::File(module) => {
                    if let Some(module) = module {
                        for r in self.by_file.get(path).into_iter().flatten() {
                            self.definition_modules.insert(r.clone(), module.clone());
                        }
                        modules_here.insert(module);
                    }
                }
                Membership::PerDefinition(per) => {
                    for (r, module) in self.by_file.get(path).into_iter().flatten().zip(per) {
                        if let Some(module) = module {
                            self.definition_modules.insert(r.clone(), module.clone());
                            modules_here.insert(module);
                        }
                    }
                }
            }
            for module in &modules_here {
                self.module_files
                    .entry(module.clone())
                    .or_default()
                    .insert(path.clone());
            }
            self.file_modules
                .insert(path.clone(), modules_here.into_iter().collect());
        }
        for (r, module) in &self.definition_modules {
            self.module_definitions
                .entry(module.clone())
                .or_default()
                .push(r.clone());
        }
    }

    /// Every declared package and namespace, by language and the key it is compared
    /// by.
    fn declared_namespaces(&self) -> BTreeMap<(&'static str, String), ModuleKey> {
        self.module_files
            .keys()
            .filter(|k| namespace_evidence(k.language) == NamespaceEvidence::FileDeclaration)
            .map(|k| {
                (
                    (k.language, imports::namespace_key(k.language, &k.name)),
                    k.clone(),
                )
            })
            .collect()
    }

    fn resolve_imports(
        &mut self,
        sources: &Sources<'_>,
        layout: &Layout,
        namespaces: &BTreeMap<(&'static str, String), ModuleKey>,
    ) {
        let lookup = Lookup {
            sources,
            layout,
            file_modules: &self.file_modules,
            module_files: &self.module_files,
            namespaces,
        };
        let mut all = BTreeMap::new();
        for (path, state) in &sources.files {
            let (Some(language), FileOutcome::Extracted { extract, .. }) =
                (state.discovered.language, &state.outcome)
            else {
                continue;
            };
            let records = extract
                .imports
                .iter()
                .enumerate()
                .filter_map(|(i, import)| Some((u32::try_from(i).ok()?, import)))
                .map(|(index, import)| {
                    let (bindings, target) = imports::interpret(&lookup, language, path, import);
                    ImportRecord {
                        import: ImportRef {
                            path: path.clone(),
                            index,
                        },
                        module_text: import.module_text.clone(),
                        imported_name: import.imported_name.clone(),
                        alias: import.alias.clone(),
                        bindings,
                        target,
                    }
                })
                .collect();
            all.insert(path.clone(), records);
        }
        self.imports = all;
    }

    /// C headers and sources paired by directory and basename.
    fn pair_headers(&mut self, sources: &Sources<'_>) {
        for (path, state) in &sources.files {
            let Some(language) = state.discovered.language else {
                continue;
            };
            if !matches!(
                language.module_rule,
                crate::languages::ModuleRule::DirectoryPairingHeaders
            ) {
                continue;
            }
            let Some(stem) = path.strip_suffix(".h") else {
                continue;
            };
            let sources_of_pair: Vec<&String> = language
                .extensions
                .iter()
                .filter(|e| **e != ".h")
                .filter_map(|e| {
                    sources
                        .files
                        .get_key_value(&format!("{stem}{e}"))
                        .map(|(k, _)| k)
                })
                .collect();
            if let [source] = sources_of_pair.as_slice() {
                self.pairs.insert(path.clone(), (*source).clone());
                self.pairs.insert((*source).clone(), path.clone());
            }
        }
    }

    fn resolve_bases(&mut self, sources: &Sources<'_>) {
        let mut bases = BTreeMap::new();
        for (path, refs) in &self.by_file {
            let Some(extract) = sources.extract(path) else {
                continue;
            };
            for (r, d) in refs.iter().zip(&extract.definitions) {
                let mut relations = Vec::new();
                let mut seen = BTreeSet::new();
                for raw in &d.base_classes {
                    if !seen.insert(raw.as_str()) {
                        continue; // the engine can report one base twice
                    }
                    for name in base_names(raw) {
                        let resolution = self.resolve_base(sources, r, &name);
                        relations.push(BaseRelation {
                            raw: raw.clone(),
                            name,
                            resolution,
                        });
                    }
                }
                if !relations.is_empty() {
                    bases.insert(r.clone(), relations);
                }
            }
        }
        let mut derived: BTreeMap<DefinitionRef, BTreeSet<DefinitionRef>> = BTreeMap::new();
        for (r, relations) in &bases {
            for relation in relations {
                if let BaseResolution::Internal(base) = &relation.resolution {
                    derived.entry(base.clone()).or_default().insert(r.clone());
                }
            }
        }
        self.bases = bases;
        self.derived = derived
            .into_iter()
            .map(|(k, v)| (k, v.into_iter().collect()))
            .collect();
    }

    /// What one base names: an exact qualified name; a type of the same name in the
    /// derived definition's module; a type an import binding the name leads to (or
    /// external, if the import is); a type of that name unique in the repository.
    /// The first of these that finds anything decides; several are ambiguous.
    fn resolve_base(
        &self,
        sources: &Sources<'_>,
        derived: &DefinitionRef,
        name: &str,
    ) -> BaseResolution {
        let Some(language) = sources
            .files
            .get(&derived.path)
            .and_then(|s| s.discovered.language)
        else {
            return BaseResolution::Unresolved;
        };
        let is_type_here = |r: &DefinitionRef| -> bool {
            r != derived
                && definition_of(sources, r).is_some_and(|d| modules::is_type(d.kind))
                && sources
                    .files
                    .get(&r.path)
                    .and_then(|s| s.discovered.language)
                    .is_some_and(|l| compatible(l, language))
        };
        let decide = |found: Vec<DefinitionRef>| -> Option<BaseResolution> {
            let mut found = found;
            found.sort();
            found.dedup();
            match found.len() {
                0 => None,
                1 => Some(BaseResolution::Internal(found.remove(0))),
                _ => Some(BaseResolution::Ambiguous(found)),
            }
        };
        let (qualifier, short) = split_qualified(name);
        // 1. The name is a qualified name.
        let exact: Vec<DefinitionRef> =
            [name.to_owned(), name.replace("::", ".").replace('\\', ".")]
                .iter()
                .flat_map(|n| self.by_qualified_name.get(n).into_iter().flatten())
                .filter(|r| is_type_here(r))
                .cloned()
                .collect();
        if let Some(found) = decide(exact) {
            return found;
        }
        // 2. A type of that name in the same module.
        if qualifier.is_none()
            && let Some(module) = self.definition_modules.get(derived)
        {
            let same: Vec<DefinitionRef> = self
                .by_name
                .get(short)
                .into_iter()
                .flatten()
                .filter(|r| is_type_here(r) && self.definition_modules.get(*r) == Some(module))
                .cloned()
                .collect();
            if let Some(found) = decide(same) {
                return found;
            }
        }
        // 3. An import binds it.
        if let Some(found) =
            self.base_through_imports(sources, derived, qualifier, short, &is_type_here)
        {
            return found;
        }
        // 4. A type of that name unique in the repository.
        let anywhere: Vec<DefinitionRef> = self
            .by_name
            .get(short)
            .into_iter()
            .flatten()
            .filter(|r| is_type_here(r))
            .cloned()
            .collect();
        decide(anywhere).unwrap_or(BaseResolution::Unresolved)
    }

    /// Stage 3 of [`Self::resolve_base`]: the types the imports that bind the base's
    /// name (or its qualifier's first part) lead to; external when every such import
    /// is; unresolved when they lead nowhere known. `None` when no import binds it.
    fn base_through_imports(
        &self,
        sources: &Sources<'_>,
        derived: &DefinitionRef,
        qualifier: Option<&str>,
        short: &str,
        is_type_here: &dyn Fn(&DefinitionRef) -> bool,
    ) -> Option<BaseResolution> {
        let bound = qualifier.map_or(short, |q| q.split(['.', ':', '\\']).next().unwrap_or(q));
        let binding: Vec<&ImportRecord> = self
            .imports
            .get(&derived.path)
            .into_iter()
            .flatten()
            .filter(|i| i.bindings.iter().any(|b| b == bound))
            .collect();
        if binding.is_empty() {
            return None;
        }
        let mut found = Vec::new();
        for record in &binding {
            let targets: Vec<&InternalTarget> = match &record.target {
                ImportTarget::Internal(t) => vec![t],
                ImportTarget::InternalCandidates(ts) => ts.iter().collect(),
                _ => Vec::new(),
            };
            for target in targets {
                // The type's name: the member the import names, for a name the import
                // binds directly.
                let wanted = match (&target.member, qualifier) {
                    (Some(member), None) => member.as_str(),
                    _ => short,
                };
                for file in &target.files {
                    found.extend(
                        self.by_file
                            .get(file)
                            .into_iter()
                            .flatten()
                            .filter(|r| is_type_here(r))
                            .filter(|r| definition_of(sources, r).is_some_and(|d| d.name == wanted))
                            .cloned(),
                    );
                }
            }
        }
        found.sort();
        found.dedup();
        Some(match found.len() {
            1 => BaseResolution::Internal(found.remove(0)),
            n if n > 1 => BaseResolution::Ambiguous(found),
            _ => {
                let external: Vec<ImportRef> = binding
                    .iter()
                    .filter(|i| i.target == ImportTarget::External)
                    .map(|i| i.import.clone())
                    .collect();
                if external.len() == binding.len() {
                    BaseResolution::External(external)
                } else {
                    BaseResolution::Unresolved
                }
            }
        })
    }

    /// The metadata the engine's resolver takes, from the same layout the registry
    /// resolves imports with.
    fn metadata_for_engine(&self, layout: &Layout) -> ResolutionMetadata {
        let mut packages = Vec::new();
        for (dir, module) in &layout.go_modules {
            packages.push(PackageEntry {
                import_prefix: module.clone(),
                entry_path: dir.clone(),
            });
        }
        for package in &layout.npm_packages {
            packages.push(PackageEntry {
                import_prefix: package.name.clone(),
                entry_path: package.entry.clone(),
            });
        }
        for (module, files) in &self.module_files {
            if module.name.is_empty()
                || namespace_evidence(module.language) != NamespaceEvidence::FileDeclaration
            {
                continue;
            }
            let dirs: BTreeSet<&str> = files.iter().map(|f| dir_of(f)).collect();
            for dir in dirs {
                packages.push(PackageEntry {
                    import_prefix: module.name.clone(),
                    entry_path: dir.to_owned(),
                });
            }
        }
        packages.sort_by(|x, y| {
            x.import_prefix
                .cmp(&y.import_prefix)
                .then_with(|| x.entry_path.cmp(&y.entry_path))
        });
        packages.dedup();
        let crate_manifest =
            layout
                .crates
                .iter()
                .find(|c| c.dir.is_empty())
                .map(|c| CrateManifest {
                    package_name: c.manifest.package_name.clone(),
                    is_workspace_root: c.manifest.is_workspace_root,
                    dependencies: c.dependencies.clone(),
                    member_paths: c.manifest.members.clone(),
                });
        ResolutionMetadata {
            packages,
            alias_scopes: layout.alias_scopes.clone(),
            crate_manifest,
        }
    }

    // --- files and extractions ----------------------------------------------------

    /// Every file, by path.
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    /// A file's language, as discovery assigned it.
    pub fn language(&self, path: &str) -> Option<&'static Language> {
        self.files.get(path)?.discovered.language
    }

    /// What Stage 2 did with a file.
    pub fn outcome(&self, path: &str) -> Option<&FileOutcome> {
        self.files.get(path).map(|s| &s.outcome)
    }

    /// A file's extraction, if it was extracted.
    pub fn extract(&self, path: &str) -> Option<&FileExtract> {
        match self.outcome(path)? {
            FileOutcome::Extracted { extract, .. } => Some(extract),
            _ => None,
        }
    }

    /// A definition.
    pub fn definition(&self, r: &DefinitionRef) -> Option<&Definition> {
        self.extract(&r.path)?
            .definitions
            .get(usize::try_from(r.index).ok()?)
    }

    // --- definitions --------------------------------------------------------------

    /// Every definition with this short name, by path then index.
    pub fn by_name(&self, name: &str) -> &[DefinitionRef] {
        self.by_name.get(name).map_or(&[], Vec::as_slice)
    }

    /// Every definition with exactly this qualified name, by path then index: several
    /// when a name is overloaded or declared twice, never one chosen among them.
    pub fn by_qualified_name(&self, qualified_name: &str) -> &[DefinitionRef] {
        self.by_qualified_name
            .get(qualified_name)
            .map_or(&[], Vec::as_slice)
    }

    /// Every definition of a file, by index.
    pub fn by_file(&self, path: &str) -> &[DefinitionRef] {
        self.by_file.get(path).map_or(&[], Vec::as_slice)
    }

    // --- modules ----------------------------------------------------------------

    /// Every module, in order.
    pub fn modules(&self) -> impl Iterator<Item = &ModuleKey> {
        self.module_files.keys()
    }

    /// A file's modules: none when its rule has no evidence, one for most languages,
    /// several for a file whose definitions sit in different namespaces.
    pub fn modules_of_file(&self, path: &str) -> &[ModuleKey] {
        self.file_modules.get(path).map_or(&[], Vec::as_slice)
    }

    /// A file's module, when it has exactly one.
    pub fn module_of_file(&self, path: &str) -> Option<&ModuleKey> {
        match self.modules_of_file(path) {
            [one] => Some(one),
            _ => None,
        }
    }

    /// A definition's module.
    pub fn module_of_definition(&self, r: &DefinitionRef) -> Option<&ModuleKey> {
        self.definition_modules.get(r)
    }

    /// A module's files, by path.
    pub fn files_in_module(&self, module: &ModuleKey) -> impl Iterator<Item = &str> {
        self.module_files
            .get(module)
            .into_iter()
            .flatten()
            .map(String::as_str)
    }

    /// A module's definitions, by path then index.
    pub fn definitions_in_module(&self, module: &ModuleKey) -> &[DefinitionRef] {
        self.module_definitions
            .get(module)
            .map_or(&[], Vec::as_slice)
    }

    /// The source or header a C file is paired with: the file of the same directory
    /// and basename, when there is exactly one.
    pub fn paired_file(&self, path: &str) -> Option<&str> {
        self.pairs.get(path).map(String::as_str)
    }

    // --- imports ------------------------------------------------------------------

    /// A file's imports, one record per import extracted, in extraction order.
    pub fn imports_of(&self, path: &str) -> &[ImportRecord] {
        self.imports.get(path).map_or(&[], Vec::as_slice)
    }

    /// One import.
    pub fn import(&self, r: &ImportRef) -> Option<&ImportRecord> {
        self.imports_of(&r.path).get(usize::try_from(r.index).ok()?)
    }

    /// Where a name visible in a file comes from: the file's own top-level
    /// definitions first, then the imports that bind it. A name is external only when
    /// every import that binds it is an import of an external package and nothing in
    /// the file or the repository's imports offers it otherwise.
    pub fn name_provenance(&self, path: &str, name: &str) -> NameProvenance {
        let defined: Vec<DefinitionRef> = self
            .by_file(path)
            .iter()
            .filter(|r| {
                self.definition(r).is_some_and(|d| {
                    d.name == name
                        && d.parent.is_none()
                        && !(d.kind == DefinitionKind::Module && d.name == path)
                })
            })
            .cloned()
            .collect();
        if !defined.is_empty() {
            return NameProvenance::Defined(defined);
        }
        let binding: Vec<&ImportRecord> = self
            .imports_of(path)
            .iter()
            .filter(|i| i.bindings.iter().any(|b| b == name))
            .collect();
        if binding.is_empty() {
            return NameProvenance::Unknown;
        }
        let internal: Vec<ImportRef> = binding
            .iter()
            .filter(|i| i.target.is_internal())
            .map(|i| i.import.clone())
            .collect();
        if !internal.is_empty() {
            return NameProvenance::InternalImport(internal);
        }
        if binding.iter().all(|i| i.target == ImportTarget::External) {
            return NameProvenance::ExternalImport(
                binding.iter().map(|i| i.import.clone()).collect(),
            );
        }
        NameProvenance::Unknown
    }

    /// Whether a name visible in a file is external ([`Self::name_provenance`]).
    pub fn is_external_name(&self, path: &str, name: &str) -> bool {
        matches!(
            self.name_provenance(path, name),
            NameProvenance::ExternalImport(_)
        )
    }

    /// The names that reach a file only through imports of external packages.
    pub fn external_names(&self, path: &str) -> BTreeSet<&str> {
        self.imports_of(path)
            .iter()
            .flat_map(|i| i.bindings.iter())
            .filter(|b| self.is_external_name(path, b))
            .map(String::as_str)
            .collect()
    }

    // --- hierarchy ----------------------------------------------------------------

    /// The bases a definition names, in the engine's order, each once.
    pub fn direct_bases(&self, r: &DefinitionRef) -> &[BaseRelation] {
        self.bases.get(r).map_or(&[], Vec::as_slice)
    }

    /// The definitions that name this one as a base, by path then index.
    pub fn direct_derived(&self, r: &DefinitionRef) -> &[DefinitionRef] {
        self.derived.get(r).map_or(&[], Vec::as_slice)
    }

    /// Every internal type a definition inherits from, nearest first, each once, in
    /// the order its bases name them: breadth first, a type already seen skipped, so
    /// a cycle ends rather than repeats.
    pub fn ancestors(&self, r: &DefinitionRef) -> Vec<DefinitionRef> {
        let mut seen = BTreeSet::from([r.clone()]);
        let mut queue = VecDeque::from([r.clone()]);
        let mut out = Vec::new();
        while let Some(next) = queue.pop_front() {
            for relation in self.direct_bases(&next) {
                if let BaseResolution::Internal(base) = &relation.resolution
                    && seen.insert(base.clone())
                {
                    out.push(base.clone());
                    queue.push_back(base.clone());
                }
            }
        }
        out
    }

    /// The type a member is declared in: the nearest enclosing definition that is a
    /// type.
    pub fn declaring_type(&self, r: &DefinitionRef) -> Option<DefinitionRef> {
        let extract = self.extract(&r.path)?;
        let mut seen = BTreeSet::new();
        let mut at = extract
            .definitions
            .get(usize::try_from(r.index).ok()?)?
            .parent;
        while let Some(parent) = at {
            if !seen.insert(parent) {
                return None; // a cycle, which validation refuses; never followed
            }
            let d = extract.definitions.get(usize::try_from(parent).ok()?)?;
            if modules::is_type(d.kind) {
                return Some(DefinitionRef {
                    path: r.path.clone(),
                    index: parent,
                });
            }
            at = d.parent;
        }
        None
    }

    // --- engine metadata ------------------------------------------------------------

    /// The repository's module metadata as the engine's resolver takes it: Go
    /// modules, packages named by `package.json` files and declared packages and
    /// namespaces, each to its entry; the `tsconfig.json` alias scopes; the root crate
    /// manifest. The same that the registry resolved imports with.
    pub fn resolution_metadata(&self) -> &ResolutionMetadata {
        &self.metadata
    }
}

/// A definition of the files being registered.
fn definition_of<'s>(sources: &'s Sources<'_>, r: &DefinitionRef) -> Option<&'s Definition> {
    sources
        .extract(&r.path)?
        .definitions
        .get(usize::try_from(r.index).ok()?)
}

/// Whether a base in one language can name a type in another: the same language,
/// or TypeScript and JavaScript.
fn compatible(a: &Language, b: &Language) -> bool {
    a.id == b.id
        || (matches!(a.id, "typescript" | "javascript")
            && matches!(b.id, "typescript" | "javascript"))
}

/// A name's qualifier and short name, at its last `.`, `::` or `\`.
fn split_qualified(name: &str) -> (Option<&str>, &str) {
    let at = [
        name.rfind("::").map(|i| (i, 2)),
        name.rfind('.').map(|i| (i, 1)),
        name.rfind('\\').map(|i| (i, 1)),
    ]
    .into_iter()
    .flatten()
    .max_by_key(|(i, _)| *i);
    match at {
        Some((i, len)) => (Some(&name[..i]), &name[i + len..]),
        None => (None, name),
    }
}

/// The names a raw base names: a leading `extends`, `implements` or `with` left out,
/// a list split, type arguments and constructor arguments left off.
fn base_names(raw: &str) -> Vec<String> {
    let mut text = raw.trim().to_owned();
    for keyword in [" implements ", " with "] {
        text = text.replace(keyword, ",");
    }
    text.split(',')
        .map(|part| {
            let mut p = part.trim();
            for keyword in ["extends ", "implements ", "with "] {
                if let Some(rest) = p.strip_prefix(keyword) {
                    p = rest.trim();
                }
            }
            let end = p.find(['<', '(', '[', ' ']).unwrap_or(p.len());
            p[..end].trim().to_owned()
        })
        .filter(|p| !p.is_empty())
        .collect()
}

/// Pairs each discovered file with what Stage 2 did with it.
fn pair_stages(
    discovered: &[DiscoveredFile],
    extracted: ExtractReport,
) -> Result<BTreeMap<String, FileState>, RegistryError> {
    let mut found: BTreeMap<String, DiscoveredFile> = BTreeMap::new();
    for file in discovered {
        if found.insert(file.path.clone(), file.clone()).is_some() {
            return Err(RegistryError::StageMismatch {
                path: file.path.clone(),
                detail: "discovered twice",
            });
        }
    }
    let mut files = BTreeMap::new();
    for e in extracted.files {
        let Some(d) = found.remove(&e.path) else {
            return Err(RegistryError::StageMismatch {
                path: e.path,
                detail: "extracted but not discovered, or extracted twice",
            });
        };
        if d.language.map(|l| l.id) != e.language.map(|l| l.id) {
            return Err(RegistryError::StageMismatch {
                path: e.path,
                detail: "a different language in each stage",
            });
        }
        if let FileOutcome::Extracted { extract, .. } = &e.outcome
            && (extract.rel_path != e.path
                || d.disposition != Disposition::Candidate
                || d.language
                    .is_none_or(|l| extract.language != l.engine_language(&e.path)))
        {
            return Err(RegistryError::StageMismatch {
                path: e.path,
                detail: "an extraction of another file, or of a file not extracted",
            });
        }
        files.insert(
            e.path,
            FileState {
                discovered: d,
                outcome: e.outcome,
            },
        );
    }
    if let Some(path) = found.into_keys().next() {
        return Err(RegistryError::StageMismatch {
            path,
            detail: "discovered but not in the extraction report",
        });
    }
    Ok(files)
}

/// Every definition index an extraction's facts use is one of its definitions, and
/// no definition is its own ancestor.
fn validate(path: &str, extract: &FileExtract) -> Result<(), RegistryError> {
    let n = extract.definitions.len();
    let check = |what: &'static str, index: Option<u32>| -> Result<(), RegistryError> {
        match index {
            Some(i) if usize::try_from(i).map_or(true, |i| i >= n) => {
                Err(RegistryError::InvalidDefinitionIndex {
                    path: path.to_owned(),
                    what,
                    index: i,
                })
            }
            _ => Ok(()),
        }
    };
    for c in &extract.calls {
        check("a call's caller", c.caller)?;
    }
    for u in &extract.usages {
        check("a usage's scope", u.scope)?;
    }
    for t in &extract.type_refs {
        check("a type reference's scope", t.scope)?;
    }
    for t in &extract.throws {
        check("a throw's scope", t.scope)?;
    }
    for rw in &extract.read_writes {
        check("a field access's scope", rw.scope)?;
    }
    for (i, d) in extract.definitions.iter().enumerate() {
        let index = u32::try_from(i).unwrap_or(u32::MAX);
        let invalid = || RegistryError::InvalidParent {
            path: path.to_owned(),
            index,
        };
        let mut seen = BTreeSet::from([i]);
        let mut at = d.parent;
        while let Some(parent) = at {
            let p = usize::try_from(parent).map_err(|_| invalid())?;
            if p >= n || !seen.insert(p) {
                return Err(invalid());
            }
            at = extract.definitions.get(p).and_then(|d| d.parent);
        }
    }
    Ok(())
}
