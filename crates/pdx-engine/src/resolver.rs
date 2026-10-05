//! Typed resolution across the files of a project.

use std::collections::BTreeMap;
use std::ffi::{CString, c_char};
use std::ptr::{self, NonNull};

use serde::{Deserialize, Serialize};

use pdx_engine_sys as sys;

use crate::convert;
use crate::engine::{Engine, RawResult, c_string, language_id};
use crate::error::{EngineError, SourceDifference, check};
use crate::model::{
    Call, CallArg, FileExtract, ImplTrait, ReadWrite, SourceDigest, Span, TypeRef, Usage,
    Visibility,
};

/// How a resolution was reached, in the interface's normalised vocabulary.
///
/// Exhaustive: the interface returns nothing else. Only [`Strategy::LspTyped`] with a
/// single candidate above the typed threshold is evidence on its own; the rest are
/// hints that seed a search without restricting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Strategy {
    /// `import_map`.
    ImportMap,
    /// `import_map_suffix`.
    ImportMapSuffix,
    /// `same_module`.
    SameModule,
    /// `qualified_suffix`.
    QualifiedSuffix,
    /// `unique_name`.
    UniqueName,
    /// `suffix_match`.
    SuffixMatch,
    /// `fuzzy_single`.
    FuzzySingle,
    /// `fuzzy_multi`.
    FuzzyMulti,
    /// `service_pattern`.
    ServicePattern,
    /// `lsp_typed`.
    LspTyped,
    /// `unknown`.
    Unknown,
}

impl Strategy {
    /// Every strategy, in the interface's order.
    pub const ALL: [Self; 11] = [
        Self::ImportMap,
        Self::ImportMapSuffix,
        Self::SameModule,
        Self::QualifiedSuffix,
        Self::UniqueName,
        Self::SuffixMatch,
        Self::FuzzySingle,
        Self::FuzzyMulti,
        Self::ServicePattern,
        Self::LspTyped,
        Self::Unknown,
    ];

    /// The interface's name for it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ImportMap => "import_map",
            Self::ImportMapSuffix => "import_map_suffix",
            Self::SameModule => "same_module",
            Self::QualifiedSuffix => "qualified_suffix",
            Self::UniqueName => "unique_name",
            Self::SuffixMatch => "suffix_match",
            Self::FuzzySingle => "fuzzy_single",
            Self::FuzzyMulti => "fuzzy_multi",
            Self::ServicePattern => "service_pattern",
            Self::LspTyped => "lsp_typed",
            Self::Unknown => "unknown",
        }
    }

    /// The strategy the interface names `name`, if it names one.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == name)
    }
}

/// Which call site a resolution answers: a file, and the index of the call in that
/// file's extraction. An index past the extraction's calls is a site typed resolution
/// found itself, described only by [`TypedResolution::site`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SiteRef {
    /// The file.
    pub rel_path: String,
    /// The call's index.
    pub call_index: u32,
}

/// One call site typed resolution answered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypedResolution {
    /// The site.
    pub site_ref: SiteRef,
    /// The site as extraction, or typed resolution, describes it.
    pub site: Call,
    /// The target's qualified name.
    pub target_qn: String,
    /// The file the target is in; `None` when it is in no file of the project, such as
    /// a built-in. The site is settled all the same and must not be resolved by name.
    pub target_rel_path: Option<String>,
    /// The engine's confidence.
    pub score: f64,
    /// How it was resolved, normalised.
    pub strategy: Strategy,
    /// How the engine says it resolved it, verbatim, for calibration only; never used to
    /// derive a band. `None` where the engine names nothing.
    pub engine_strategy: Option<String>,
    /// How many targets the engine weighed.
    pub candidates: u32,
}

/// Whether a run did all the work it should have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RunStatus {
    /// It did, which includes finding no answer for files that have none to find.
    Clean,
    /// Some work was skipped or lost. Every answer is sound, but a site without one may
    /// never have been asked about, so the absence of an answer is not evidence.
    Degraded,
}

/// What a run did with each file, and what it lost.
///
/// Every file is counted once, under the first of `files_untyped`, `files_empty`,
/// `files_not_reached`, `files_source_unavailable`, `files_over_budget` and
/// `files_resolved` that applies, so those six add up to `files`. Every count from
/// `files_not_reached` on is work lost, and any above zero makes the run degraded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunHealth {
    /// Clean or degraded.
    pub status: RunStatus,
    /// Files in the project.
    pub files: u32,
    /// Files typed resolution ran on.
    pub files_resolved: u32,
    /// Files in a language typed resolution does not cover, by design.
    pub files_untyped: u32,
    /// Files with no source, so nothing to resolve.
    pub files_empty: u32,
    /// Files extraction stopped at its node budget, which typed resolution skips.
    pub files_over_budget: u32,
    /// Files whose source typed resolution could not obtain.
    pub files_source_unavailable: u32,
    /// Files typed resolution stopped before reaching.
    pub files_not_reached: u32,
    /// Failures inside typed resolution that lost answers without skipping a file.
    pub pass_failures: u32,
}

impl RunHealth {
    /// Whether the run did all its work.
    pub fn is_clean(&self) -> bool {
        self.status == RunStatus::Clean
    }
}

/// What a completed run found, and how it went.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectResolution {
    /// The answers: for each file in the order added, within a file in call order. A
    /// degraded run reports every answer it did find.
    pub resolutions: Vec<TypedResolution>,
    /// Whether the run did all its work.
    pub health: RunHealth,
}

/// One package or module the repository declares: an import of `import_prefix`, or of
/// anything beneath it, resolves into the module whose entry is `entry_path`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageEntry {
    /// The import prefix.
    pub import_prefix: String,
    /// The module's entry, relative to the repository.
    pub entry_path: String,
}

/// One alias from a build configuration's path mappings, already split at its wildcard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathAlias {
    /// What precedes the wildcard in the alias.
    pub alias_prefix: String,
    /// What follows it.
    pub alias_suffix: String,
    /// What precedes the wildcard in the target.
    pub target_prefix: String,
    /// What follows it.
    pub target_suffix: String,
    /// Whether the alias has a wildcard; without one it matches exactly.
    pub has_wildcard: bool,
}

/// The aliases one configuration applies to the files beneath its directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AliasScope {
    /// The directory, relative to the repository; empty for the root.
    pub dir_prefix: String,
    /// The configuration's base URL, if it sets one.
    pub base_url: Option<String>,
    /// Its aliases.
    pub aliases: Vec<PathAlias>,
}

/// A dependency the root crate manifest declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrateDependency {
    /// Its name.
    pub name: String,
    /// For a dependency inside the repository, its path relative to the repository.
    pub path: Option<String>,
}

/// What the repository's root crate manifest declares, already parsed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrateManifest {
    /// The package's name, if the manifest declares a package.
    pub package_name: Option<String>,
    /// Whether it is a workspace root.
    pub is_workspace_root: bool,
    /// Its dependencies, in the manifest's order.
    pub dependencies: Vec<CrateDependency>,
    /// Its workspace members, as the manifest lists them.
    pub member_paths: Vec<String>,
}

/// What the repository declares about its own module structure. All optional: with
/// none, imports resolve as for a repository that declares nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionMetadata {
    /// Declared packages and modules.
    pub packages: Vec<PackageEntry>,
    /// Path alias scopes.
    pub alias_scopes: Vec<AliasScope>,
    /// The root crate manifest, if there is one.
    pub crate_manifest: Option<CrateManifest>,
}

/// A set of files resolved together.
///
/// Files are added, with the repository's metadata if it has any, and the project is
/// run once, which consumes it. Every engine object the project depends on is owned
/// here and freed only after the project ends, so nothing the project reads can be
/// freed or reused under it. Dropping a resolver that was never run, after an error or
/// otherwise, frees everything.
pub struct ProjectResolver<'e> {
    engine: &'e Engine,
    project: NonNull<sys::pdxe_project>,
    /// Results the project resolves through; freed after the project ends.
    held: Vec<RawResult<'e>>,
    /// Definition counts by file, to check the sites the engine reports.
    definitions: BTreeMap<String, u32>,
    /// The first error that left the project unusable.
    broken: Option<EngineError>,
}

impl<'e> ProjectResolver<'e> {
    /// Begins a project on `engine`.
    ///
    /// # Errors
    ///
    /// The engine's failure to begin one.
    pub fn new(engine: &'e Engine) -> Result<Self, EngineError> {
        let mut out = ptr::null_mut();
        // SAFETY: the context is alive for 'e; `out` is a valid place to write.
        let status = unsafe { sys::pdxe_resolve_project_begin(engine.ctx(), &raw mut out) };
        check(status, "pdxe_resolve_project_begin", None)?;
        let project = NonNull::new(out)
            .ok_or_else(|| EngineError::Contract("the engine began no project".into()))?;
        engine.handle_opened();
        Ok(Self {
            engine,
            project,
            held: Vec::new(),
            definitions: BTreeMap::new(),
            broken: None,
        })
    }

    /// Adds a file from its extraction, which may have come from a cache or another
    /// process: the extraction's surface is what the file is resolved through, so the
    /// file is never extracted again. `source` is the file's source, which resolution
    /// parses again for the tree it walks; it must be exactly the bytes the extraction
    /// was taken from, and is checked against the extraction's length and digest.
    ///
    /// # Errors
    ///
    /// [`EngineError::SourceMismatch`] for any other source, even of the same length,
    /// and the engine's refusals, such as a path added twice. After an error the
    /// project cannot run.
    pub fn add_file(&mut self, extract: &FileExtract, source: &[u8]) -> Result<(), EngineError> {
        let result = self.add_file_inner(extract, source);
        self.note(result)
    }

    /// Extracts a file with this resolver's engine and adds it, resolving through the
    /// engine's own result rather than through a surface. Returns the extraction, the
    /// same as [`Engine::extract`] would.
    ///
    /// # Errors
    ///
    /// As for [`Engine::extract`] and [`ProjectResolver::add_file`].
    pub fn extract_and_add(
        &mut self,
        language: &str,
        rel_path: &str,
        source: &[u8],
    ) -> Result<FileExtract, EngineError> {
        let result = self.extract_and_add_inner(language, rel_path, source);
        self.note(result)
    }

    /// Supplies the repository's resolution metadata, at most once, before running.
    ///
    /// # Errors
    ///
    /// The engine's refusal, or a string containing a NUL byte. After an error the
    /// project cannot run.
    pub fn set_metadata(&mut self, metadata: &ResolutionMetadata) -> Result<(), EngineError> {
        let result = self.set_metadata_inner(metadata);
        self.note(result)
    }

    /// Resolves every file added, and ends the project.
    ///
    /// # Errors
    ///
    /// The first error any earlier call returned, or the run's own failure: a run that
    /// could not complete. A run that completes returns its answers with its health,
    /// degraded or not.
    pub fn run(self) -> Result<ProjectResolution, EngineError> {
        if let Some(error) = &self.broken {
            return Err(error.clone());
        }
        let p = self.project.as_ptr();
        // SAFETY: the project is alive and has not run (run consumes the resolver).
        check(
            unsafe { sys::pdxe_resolve_project_run(p) },
            "pdxe_resolve_project_run",
            None,
        )?;

        let mut health = sys::pdxe_run_health::default();
        // SAFETY: the project ran; `health` is a valid place to write.
        let status = unsafe { sys::pdxe_resolve_project_health(p, &raw mut health) };
        check(status, "pdxe_resolve_project_health", None)?;
        let health = run_health(health)?;

        let mut out = ptr::null();
        let mut n = 0u32;
        // SAFETY: the project ran; the out pointers are valid.
        let status = unsafe { sys::pdxe_resolve_project_results(p, &raw mut out, &raw mut n) };
        check(status, "pdxe_resolve_project_results", None)?;
        // SAFETY: the array and its strings live until the project ends, which is when
        // `self` is dropped, after everything has been copied.
        let resolutions = unsafe { convert::slice(out, n, "the resolutions") }?
            .iter()
            .map(|r| unsafe { self.resolution(r) })
            .collect::<Result<_, _>>()?;
        Ok(ProjectResolution {
            resolutions,
            health,
        })
    }

    /// Records the first error that leaves the project unusable.
    fn note<T>(&mut self, result: Result<T, EngineError>) -> Result<T, EngineError> {
        if let Err(error) = &result {
            self.broken.get_or_insert_with(|| error.clone());
        }
        result
    }

    fn add_file_inner(&mut self, extract: &FileExtract, source: &[u8]) -> Result<(), EngineError> {
        let difference = if source.len() as u64 != extract.source_len {
            Some(SourceDifference::Length {
                extracted: extract.source_len,
                given: source.len() as u64,
            })
        } else if SourceDigest::of(source) != extract.source_digest {
            Some(SourceDifference::Content)
        } else {
            None
        };
        if let Some(difference) = difference {
            return Err(EngineError::SourceMismatch {
                rel_path: extract.rel_path.clone(),
                difference,
            });
        }
        let lang = language_id(&extract.language)
            .ok_or_else(|| EngineError::UnknownLanguage(extract.language.clone()))?;
        let rebuilt = self.rebuild(extract)?;
        let surface = extract.surface.as_bytes();
        // SAFETY: the project is alive; the engine copies the bytes.
        let status = unsafe {
            sys::pdxe_surface_import(self.project.as_ptr(), surface.as_ptr(), surface.len())
        };
        check(status, "pdxe_surface_import", None)?;
        self.add(lang, &extract.language, &extract.rel_path, source, rebuilt)?;
        self.definitions
            .insert(extract.rel_path.clone(), count(extract.definitions.len())?);
        Ok(())
    }

    fn extract_and_add_inner(
        &mut self,
        language: &str,
        rel_path: &str,
        source: &[u8],
    ) -> Result<FileExtract, EngineError> {
        let raw = self.engine.extract_raw(language, rel_path, source)?;
        // The surface is exported before the project resolves through the result,
        // which the interface requires.
        let extract = raw.to_extract(language, rel_path, source)?;
        let lang = language_id(language)
            .ok_or_else(|| EngineError::UnknownLanguage(language.to_owned()))?;
        self.add(lang, language, rel_path, source, raw)?;
        self.definitions
            .insert(rel_path.to_owned(), count(extract.definitions.len())?);
        Ok(extract)
    }

    /// Adds a file the project will resolve through `result`, and keeps the result.
    fn add(
        &mut self,
        lang: i32,
        language: &str,
        rel_path: &str,
        source: &[u8],
        result: RawResult<'e>,
    ) -> Result<(), EngineError> {
        let path = c_string(rel_path, "a path")?;
        // SAFETY: the project and the result are alive; the engine copies the path and
        // source, and the result is kept alive here until the project ends.
        let status = unsafe {
            sys::pdxe_resolve_project_add_file(
                self.project.as_ptr(),
                lang,
                path.as_ptr(),
                source.as_ptr(),
                source.len(),
                result.as_ptr(),
            )
        };
        check(status, "pdxe_resolve_project_add_file", Some(language))?;
        self.held.push(result);
        Ok(())
    }

    /// An engine result rebuilt from an extraction's arrays, as a cache keeps them.
    fn rebuild(&self, e: &FileExtract) -> Result<RawResult<'e>, EngineError> {
        let mut strings = Strings::default();
        let defs = e
            .definitions
            .iter()
            .map(|d| {
                Ok(sys::pdxe_definition {
                    name: strings.required(&d.name)?,
                    qualified_name: strings.required(&d.qualified_name)?,
                    kind: strings.required(d.kind.as_str())?,
                    engine_kind: strings.required(&d.engine_kind)?,
                    signature: strings.optional(d.signature.as_deref())?,
                    doc: strings.optional(d.doc.as_deref())?,
                    span: c_span(d.span),
                    body_span: c_span(d.body_span),
                    parent_index: c_index(d.parent),
                    visibility: c_visibility(d.visibility)?,
                    is_test: u8::from(d.is_test),
                    is_entry_point: u8::from(d.is_entry_point),
                    cyclomatic: d.cyclomatic,
                    cognitive: d.cognitive,
                    loop_depth: d.loop_depth,
                    base_classes: strings.array(&d.base_classes)?,
                    n_base_classes: count(d.base_classes.len())?,
                    decorators: strings.array(&d.decorators)?,
                    n_decorators: count(d.decorators.len())?,
                    signature_param_types: strings.array(&d.signature_param_types)?,
                    n_signature_param_types: count(d.signature_param_types.len())?,
                    route_path: strings.optional(d.route_path.as_deref())?,
                    route_method: strings.optional(d.route_method.as_deref())?,
                })
            })
            .collect::<Result<Vec<_>, EngineError>>()?;
        let calls = e
            .calls
            .iter()
            .map(|c| c_call(c, &mut strings))
            .collect::<Result<Vec<_>, EngineError>>()?;
        let imports = e
            .imports
            .iter()
            .map(|i| {
                Ok(sys::pdxe_import {
                    module_text: strings.required(&i.module_text)?,
                    imported_name: strings.optional(i.imported_name.as_deref())?,
                    alias: strings.optional(i.alias.as_deref())?,
                    span: c_span(i.span),
                })
            })
            .collect::<Result<Vec<_>, EngineError>>()?;
        let usages = e
            .usages
            .iter()
            .map(|u| c_usage(u, &mut strings))
            .collect::<Result<Vec<_>, EngineError>>()?;
        let types = e
            .type_refs
            .iter()
            .map(|t| c_type_ref(t, &mut strings))
            .collect::<Result<Vec<_>, EngineError>>()?;
        let rws = e
            .read_writes
            .iter()
            .map(|w| c_read_write(w, &mut strings))
            .collect::<Result<Vec<_>, EngineError>>()?;
        let impls = e
            .impl_traits
            .iter()
            .map(|t| c_impl_trait(t, &mut strings))
            .collect::<Result<Vec<_>, EngineError>>()?;

        let mut out = ptr::null_mut();
        // SAFETY: every array holds its count and every string is alive in `strings`
        // for the call; the engine copies all of them.
        let status = unsafe {
            sys::pdxe_result_build(
                self.engine.ctx(),
                defs.as_ptr(),
                count(defs.len())?,
                calls.as_ptr(),
                count(calls.len())?,
                imports.as_ptr(),
                count(imports.len())?,
                usages.as_ptr(),
                count(usages.len())?,
                types.as_ptr(),
                count(types.len())?,
                rws.as_ptr(),
                count(rws.len())?,
                if impls.is_empty() {
                    ptr::null()
                } else {
                    impls.as_ptr()
                },
                count(impls.len())?,
                &raw mut out,
            )
        };
        check(status, "pdxe_result_build", None)?;
        RawResult::adopt(self.engine, out)
    }

    fn set_metadata_inner(&mut self, m: &ResolutionMetadata) -> Result<(), EngineError> {
        let mut strings = Strings::default();
        let packages = m
            .packages
            .iter()
            .map(|p| {
                Ok(sys::pdxe_package_entry {
                    import_prefix: strings.required(&p.import_prefix)?,
                    entry_path: strings.required(&p.entry_path)?,
                })
            })
            .collect::<Result<Vec<_>, EngineError>>()?;
        let aliases = m
            .alias_scopes
            .iter()
            .map(|s| {
                s.aliases
                    .iter()
                    .map(|a| {
                        Ok(sys::pdxe_path_alias {
                            alias_prefix: strings.required(&a.alias_prefix)?,
                            alias_suffix: strings.required(&a.alias_suffix)?,
                            target_prefix: strings.required(&a.target_prefix)?,
                            target_suffix: strings.required(&a.target_suffix)?,
                            has_wildcard: u8::from(a.has_wildcard),
                        })
                    })
                    .collect::<Result<Vec<_>, EngineError>>()
            })
            .collect::<Result<Vec<_>, EngineError>>()?;
        let scopes = m
            .alias_scopes
            .iter()
            .zip(&aliases)
            .map(|(s, a)| {
                Ok(sys::pdxe_alias_scope {
                    dir_prefix: strings.required(&s.dir_prefix)?,
                    base_url: strings.optional(s.base_url.as_deref())?,
                    aliases: a.as_ptr(),
                    n_aliases: count(a.len())?,
                })
            })
            .collect::<Result<Vec<_>, EngineError>>()?;
        let manifest = match &m.crate_manifest {
            None => None,
            Some(c) => {
                let dependencies = c
                    .dependencies
                    .iter()
                    .map(|d| {
                        Ok(sys::pdxe_crate_dependency {
                            name: strings.required(&d.name)?,
                            path: strings.optional(d.path.as_deref())?,
                        })
                    })
                    .collect::<Result<Vec<_>, EngineError>>()?;
                let members = c
                    .member_paths
                    .iter()
                    .map(|p| strings.required(p))
                    .collect::<Result<Vec<_>, EngineError>>()?;
                Some((c, dependencies, members))
            }
        };
        let c_manifest = match &manifest {
            None => None,
            Some((c, dependencies, members)) => Some(sys::pdxe_crate_manifest {
                package_name: strings.optional(c.package_name.as_deref())?,
                is_workspace_root: u8::from(c.is_workspace_root),
                dependencies: dependencies.as_ptr(),
                n_dependencies: count(dependencies.len())?,
                member_paths: members.as_ptr(),
                n_members: count(members.len())?,
            }),
        };
        let metadata = sys::pdxe_resolution_metadata {
            packages: packages.as_ptr(),
            n_packages: count(packages.len())?,
            alias_scopes: scopes.as_ptr(),
            n_alias_scopes: count(scopes.len())?,
            crate_manifest: c_manifest.as_ref().map_or(ptr::null(), ptr::from_ref),
        };
        // SAFETY: the project is alive and every array and string the metadata points to
        // lives until the call returns; the engine copies everything.
        let status = unsafe {
            sys::pdxe_resolve_project_set_metadata(self.project.as_ptr(), &raw const metadata)
        };
        check(status, "pdxe_resolve_project_set_metadata", None)
    }

    /// One resolution, copied.
    ///
    /// # Safety
    ///
    /// `r` and its strings are alive for the call.
    unsafe fn resolution(&self, r: &sys::pdxe_resolution) -> Result<TypedResolution, EngineError> {
        let sys::pdxe_resolution {
            rel_path,
            call_index,
            target_qualified_name,
            target_rel_path,
            score,
            strategy,
            candidates,
            engine_strategy,
            site,
        } = *r;
        // SAFETY (all reads below): the caller guarantees the strings are alive.
        let rel_path = unsafe { convert::required(rel_path, "a resolution's file") }?;
        let strategy_name = unsafe { convert::required(strategy, "a resolution's strategy") }?;
        let strategy = Strategy::parse(&strategy_name).ok_or_else(|| {
            EngineError::Contract(format!("`{strategy_name}` is not a resolution strategy"))
        })?;
        let site = unsafe { convert::call(&site, self.definitions.get(&rel_path).copied()) }?;
        Ok(TypedResolution {
            site_ref: SiteRef {
                rel_path,
                call_index,
            },
            site,
            target_qn: unsafe {
                convert::required(target_qualified_name, "a resolution's target")
            }?,
            target_rel_path: unsafe { convert::optional(target_rel_path) },
            score,
            strategy,
            engine_strategy: unsafe { convert::optional(engine_strategy) },
            candidates,
        })
    }
}

impl Drop for ProjectResolver<'_> {
    fn drop(&mut self) {
        // SAFETY: the project came from pdxe_resolve_project_begin and is ended once. It
        // ends before the results it reads are freed, which happens when `held` drops
        // after this body.
        unsafe { sys::pdxe_resolve_project_end(self.project.as_ptr()) };
        self.engine.handle_closed();
    }
}

/// A run's health, typed.
fn run_health(h: sys::pdxe_run_health) -> Result<RunHealth, EngineError> {
    let sys::pdxe_run_health {
        status,
        files,
        files_resolved,
        files_untyped,
        files_empty,
        files_over_budget,
        files_source_unavailable,
        files_not_reached,
        pass_failures,
    } = h;
    let status = match u32::try_from(status) {
        Ok(sys::PDXE_RUN_CLEAN) => RunStatus::Clean,
        Ok(sys::PDXE_RUN_DEGRADED) => RunStatus::Degraded,
        _ => {
            return Err(EngineError::Contract(format!(
                "{status} is not a run status"
            )));
        }
    };
    let counted = [
        files_resolved,
        files_untyped,
        files_empty,
        files_over_budget,
        files_source_unavailable,
        files_not_reached,
    ]
    .iter()
    .map(|&n| u64::from(n))
    .sum::<u64>();
    if counted != u64::from(files) {
        return Err(EngineError::Contract(format!(
            "a run counted {counted} file outcomes for {files} files"
        )));
    }
    let lost = files_over_budget + files_source_unavailable + files_not_reached + pass_failures;
    if (lost > 0) != (status == RunStatus::Degraded) {
        return Err(EngineError::Contract(
            "a run's status disagrees with what it lost".into(),
        ));
    }
    Ok(RunHealth {
        status,
        files,
        files_resolved,
        files_untyped,
        files_empty,
        files_over_budget,
        files_source_unavailable,
        files_not_reached,
        pass_failures,
    })
}

/// C strings, arrays of them, and arrays of arguments, kept alive while the engine
/// reads them.
#[derive(Default)]
struct Strings(
    Vec<CString>,
    Vec<Vec<*const c_char>>,
    Vec<Vec<sys::pdxe_call_arg>>,
);

impl Strings {
    fn required(&mut self, s: &str) -> Result<*const c_char, EngineError> {
        let c = c_string(s, "a string")?;
        let p = c.as_ptr();
        self.0.push(c);
        Ok(p)
    }

    /// An array of these strings, or NULL when there are none, as the interface's
    /// counted string arrays are.
    fn array(&mut self, items: &[String]) -> Result<*mut *const c_char, EngineError> {
        if items.is_empty() {
            return Ok(ptr::null_mut());
        }
        let mut pointers = items
            .iter()
            .map(|s| self.required(s))
            .collect::<Result<Vec<_>, _>>()?;
        // The vector's buffer does not move when it is kept below.
        let p = pointers.as_mut_ptr();
        self.1.push(pointers);
        Ok(p)
    }

    fn optional(&mut self, s: Option<&str>) -> Result<*const c_char, EngineError> {
        s.map_or(Ok(ptr::null()), |s| self.required(s))
    }

    /// An array of these arguments, or NULL when there are none.
    fn args(&mut self, args: &[CallArg]) -> Result<*const sys::pdxe_call_arg, EngineError> {
        if args.is_empty() {
            return Ok(ptr::null());
        }
        let array = args
            .iter()
            .map(|a| {
                Ok(sys::pdxe_call_arg {
                    expr: self.required(&a.expr)?,
                    value: self.optional(a.value.as_deref())?,
                    keyword: self.optional(a.keyword.as_deref())?,
                    index: a.index,
                })
            })
            .collect::<Result<Vec<_>, EngineError>>()?;
        // The vector's buffer does not move when it is kept below.
        let p = array.as_ptr();
        self.2.push(array);
        Ok(p)
    }
}

fn c_span(s: Option<Span>) -> sys::pdxe_span {
    s.map_or_else(sys::pdxe_span::default, |s| sys::pdxe_span {
        start_byte: s.start_byte,
        end_byte: s.end_byte,
        start_line: s.start_line,
        start_col: s.start_col,
        end_line: s.end_line,
        end_col: s.end_col,
    })
}

fn c_visibility(v: Visibility) -> Result<u8, EngineError> {
    let code = match v {
        Visibility::Unknown => sys::PDXE_VIS_UNKNOWN,
        Visibility::Public => sys::PDXE_VIS_PUBLIC,
        Visibility::NonPublic => sys::PDXE_VIS_NON_PUBLIC,
    };
    u8::try_from(code)
        .map_err(|_| EngineError::Contract(format!("visibility {code} is not a byte")))
}

fn c_index(i: Option<u32>) -> u32 {
    i.unwrap_or(sys::PDXE_NO_PARENT)
}

fn c_call(c: &Call, strings: &mut Strings) -> Result<sys::pdxe_call, EngineError> {
    Ok(sys::pdxe_call {
        callee_text: strings.required(&c.callee_text)?,
        receiver_text: strings.optional(c.receiver_text.as_deref())?,
        caller_index: c_index(c.caller),
        span: c_span(c.span),
        is_reference: u8::from(c.is_reference),
        typed_only: u8::from(c.typed_only),
        lexical: c.lexical.bits(),
        ast_path: strings.array(&c.ast_path)?,
        n_ast_path: count(c.ast_path.len())?,
        args: strings.args(&c.args)?,
        n_args: count(c.args.len())?,
    })
}

fn c_usage(u: &Usage, strings: &mut Strings) -> Result<sys::pdxe_usage, EngineError> {
    Ok(sys::pdxe_usage {
        name: strings.required(&u.name)?,
        scope_index: c_index(u.scope),
        span: c_span(u.span),
        lexical: u.lexical.bits(),
    })
}

fn c_type_ref(t: &TypeRef, strings: &mut Strings) -> Result<sys::pdxe_type_ref, EngineError> {
    Ok(sys::pdxe_type_ref {
        type_text: strings.required(&t.type_text)?,
        scope_index: c_index(t.scope),
        span: c_span(t.span),
    })
}

fn c_read_write(w: &ReadWrite, strings: &mut Strings) -> Result<sys::pdxe_rw, EngineError> {
    Ok(sys::pdxe_rw {
        field_text: strings.required(&w.field_text)?,
        scope_index: c_index(w.scope),
        is_write: u8::from(w.is_write),
        span: c_span(w.span),
    })
}

fn c_impl_trait(t: &ImplTrait, strings: &mut Strings) -> Result<sys::pdxe_impl_trait, EngineError> {
    Ok(sys::pdxe_impl_trait {
        trait_name: strings.required(&t.trait_name)?,
        struct_name: strings.required(&t.struct_name)?,
        struct_qn: strings.required(&t.struct_qn)?,
    })
}

/// A length as the interface counts it.
fn count(n: usize) -> Result<u32, EngineError> {
    u32::try_from(n).map_err(|_| EngineError::TooLarge)
}
