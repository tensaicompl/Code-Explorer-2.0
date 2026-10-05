//! Stage 3's resolution stages (specification 4.5): what each call site of the
//! repository resolves to, and how sure that is (4.2.2).
//!
//! Typed resolution runs over the whole current repository, every extracted file
//! added in path order with the registry's metadata; nothing of an earlier build's
//! answers is reused. Then every site is settled, in this order of precedence:
//!
//! 1. The generic-name blocklist (Appendix B.4, [`blocklist`]): `blocked`, whatever
//!    else is known.
//! 2. The engine's answer: `typed` when [`from_engine`] says so and its target is
//!    exactly one definition of the repository; `external` when its target is in no
//!    file of the project (issue 45). Any other answer is a hint, which adds its
//!    target to the candidates and never removes one.
//! 3. The lexical facts and bindings, which stop resolution by name: a name the
//!    source binds locally (`BLOCKED`, `BLOCKED_LOCALLY`, `LOCALLY_BOUND`) is
//!    `unresolved`; a name the file binds only through imports of external packages
//!    is `external`; a member call on a receiver nothing ties to a type
//!    (`UNRESOLVED_MEMBER`) is never resolved by its name alone: only the
//!    inheritance-guided stage, and the import-guided stage when the receiver is a
//!    name the file imports from the repository, may settle it.
//! 4. The Rust stages, each narrowing one sorted set of candidates: import-guided,
//!    inheritance-guided, exact, scoped ([`narrow`]).
//! 5. What is left: `candidate` with the narrowest set reached, or `unresolved`.
//!
//! A site that is only a question for typed resolution (`typed_only`), a callable
//! passed as a value (`is_reference`) and a site typed resolution found itself are
//! settled only by a typed or external answer; without one they are not call sites,
//! and are reported as unconfirmed rather than resolved by name.
//!
//! The candidates of a site are the definitions a call could invoke (functions,
//! methods, constructors, classes, structs and macros) of the caller's language
//! family (the language itself; TypeScript and JavaScript; C and C++), by the callee's
//! short name, by its whole text as a qualified name, and through the file's imports
//! under the name they bind it to; and whatever the engine's answer names. Nothing
//! here makes a graph identity: a target is a [`DefinitionRef`], a site a
//! [`SiteRef`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use pdx_engine::{
    Call, DefinitionKind, Engine, EngineError, LexicalFacts, ProjectResolution, ProjectResolver,
    RunHealth, SiteRef, Strategy, TypedResolution,
};

use crate::bands::{Band, EngineVerdict, from_engine};
use crate::index::extract::{ExtractError, FileOutcome, prepare_source};
use crate::languages::Language;

use super::blocklist;
use super::registry::{
    BaseResolution, DefinitionRef, ImportTarget, InternalTarget, NameProvenance, SymbolRegistry,
};

/// The engine's answer for a site, verbatim: kept beside the band for calibration
/// (4.2.2, rule 1), never in place of it.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineAnswer {
    /// The target's qualified name.
    pub target_qn: String,
    /// The file the target is in; `None` when it is in no file of the project.
    pub target_rel_path: Option<String>,
    /// The engine's score.
    pub score: f64,
    /// Its strategy, normalised; what [`from_engine`] reads.
    pub strategy: Strategy,
    /// Its strategy as the engine names it, verbatim.
    pub engine_strategy: Option<String>,
    /// How many targets it weighed.
    pub candidates: u32,
}

impl From<&TypedResolution> for EngineAnswer {
    fn from(r: &TypedResolution) -> Self {
        Self {
            target_qn: r.target_qn.clone(),
            target_rel_path: r.target_rel_path.clone(),
            score: r.score,
            strategy: r.strategy,
            engine_strategy: r.engine_strategy.clone(),
            candidates: r.candidates,
        }
    }
}

/// Why a [`Resolution`] cannot be made: it would say something no band means.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidResolution {
    /// Stage 3 assigns nine bands; `precise` and `contradicted` come from elsewhere.
    #[error("`{0}` is not a band Stage 3 assigns")]
    NotAssigned(Band),
    /// A drawn band has exactly one target and no candidates.
    #[error("a `{0}` resolution has one target and no candidates")]
    Drawn(Band),
    /// `candidate` has candidates and no target.
    #[error("a `candidate` resolution has candidates and no target")]
    Candidate,
    /// `external`, `blocked` and `unresolved` have neither.
    #[error("a `{0}` resolution has no target and no candidates")]
    Settled(Band),
    /// Candidates are sorted, each once.
    #[error("candidates are not sorted and distinct")]
    Unsorted,
}

/// What a site resolves to.
///
/// Only a shape its band means can be made: a drawn band has its one target and no
/// candidates; `candidate` has one or more candidates, sorted and distinct, and no
/// target; `external`, `blocked` and `unresolved` have neither.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolution {
    band: Band,
    target: Option<DefinitionRef>,
    candidates: Vec<DefinitionRef>,
    engine: Option<EngineAnswer>,
}

impl Resolution {
    /// A resolution, if its shape is one its band means.
    ///
    /// # Errors
    ///
    /// [`InvalidResolution`] for any other shape.
    pub fn new(
        band: Band,
        target: Option<DefinitionRef>,
        candidates: Vec<DefinitionRef>,
        engine: Option<EngineAnswer>,
    ) -> Result<Self, InvalidResolution> {
        match band {
            Band::Typed
            | Band::ImportGuided
            | Band::InheritanceGuided
            | Band::Exact
            | Band::Scoped => {
                if target.is_none() || !candidates.is_empty() {
                    return Err(InvalidResolution::Drawn(band));
                }
            }
            Band::Candidate => {
                if target.is_some() || candidates.is_empty() {
                    return Err(InvalidResolution::Candidate);
                }
                if !candidates.windows(2).all(|w| w[0] < w[1]) {
                    return Err(InvalidResolution::Unsorted);
                }
            }
            Band::External | Band::Blocked | Band::Unresolved => {
                if target.is_some() || !candidates.is_empty() {
                    return Err(InvalidResolution::Settled(band));
                }
            }
            Band::Precise | Band::Contradicted => return Err(InvalidResolution::NotAssigned(band)),
        }
        Ok(Self {
            band,
            target,
            candidates,
            engine,
        })
    }

    /// The band.
    pub fn band(&self) -> Band {
        self.band
    }

    /// The target, for a drawn band.
    pub fn target(&self) -> Option<&DefinitionRef> {
        self.target.as_ref()
    }

    /// The candidates, for `candidate`: sorted, each once.
    pub fn candidates(&self) -> &[DefinitionRef] {
        &self.candidates
    }

    /// The engine's answer, where it gave one.
    pub fn engine(&self) -> Option<&EngineAnswer> {
        self.engine.as_ref()
    }

    /// The engine's score, where it answered.
    pub fn engine_score(&self) -> Option<f64> {
        self.engine.as_ref().map(|e| e.score)
    }

    /// The engine's strategy as it names it, where it answered and named one.
    pub fn engine_strategy(&self) -> Option<&str> {
        self.engine.as_ref()?.engine_strategy.as_deref()
    }

    /// How many targets the engine weighed, where it answered.
    pub fn engine_candidates(&self) -> Option<u32> {
        self.engine.as_ref().map(|e| e.candidates)
    }
}

/// A call site and what it resolves to.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedSite {
    /// The site: its file and its index there.
    pub site_ref: SiteRef,
    /// The site as extraction (or, for a site typed resolution found, the engine)
    /// describes it.
    pub site: Call,
    /// What it resolves to.
    pub resolution: Resolution,
}

/// Why a site is not a call site.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Unconfirmed {
    /// A question for typed resolution (`typed_only`), which gave no typed or
    /// external answer: never resolved by name.
    TypedOnly,
    /// A callable passed as a value (`is_reference`), which typed resolution did not
    /// settle: an ordinary use of a name, never a call.
    Reference,
    /// A site typed resolution found itself, past the extraction's calls, which it
    /// did not settle.
    EngineSite,
}

/// A site that is not a call site, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct UnconfirmedSite {
    /// The site.
    pub site_ref: SiteRef,
    /// The site as described.
    pub site: Call,
    /// Why it is not a call site.
    pub reason: Unconfirmed,
    /// The engine's answer, a hint, if it gave one.
    pub engine: Option<EngineAnswer>,
}

/// What Stage 3's resolution found for a repository.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolveReport {
    /// Every call site, by path then index, and what it resolves to.
    pub resolutions: Vec<ResolvedSite>,
    /// Every site that is not a call site, by path then index.
    pub unconfirmed: Vec<UnconfirmedSite>,
    /// How typed resolution's run went. A degraded run's answers are sound, but a site
    /// without one may never have been asked about: the absence of an answer is not
    /// evidence, and nothing here treats it as such.
    pub engine_health: RunHealth,
}

/// Why resolution could not complete.
#[derive(Debug, thiserror::Error)]
pub enum ResolutionError {
    /// Typed resolution could not be set up or run, or refused what it was given (a
    /// file's extraction, its relations included, that does not cross the interface).
    #[error("typed resolution failed: {0}")]
    Engine(#[from] EngineError),
    /// A file could not be read again.
    #[error("{path}: {source}")]
    SourceRead {
        /// The file.
        path: String,
        /// Why.
        #[source]
        source: Box<ExtractError>,
    },
    /// A file read again is not the one Stage 2 extracted.
    #[error("{0}: changed since extraction")]
    SourceChanged(String),
    /// The engine answered one site twice, differently.
    #[error("{}:{}: two different answers for one site", site.rel_path, site.call_index)]
    DuplicateEngineAnswer {
        /// The site.
        site: SiteRef,
    },
    /// The engine answered a site the extractions do not have.
    #[error("{}:{}: {detail}", site.rel_path, site.call_index)]
    ImpossibleEngineSite {
        /// The site.
        site: SiteRef,
        /// What is wrong with it.
        detail: &'static str,
    },
    /// The registry does not hold what a site refers to.
    #[error("{path}: {detail}")]
    Registry {
        /// The file.
        path: String,
        /// What is missing.
        detail: &'static str,
    },
    /// A resolution was about to be made in a shape its band does not mean.
    #[error("{0}")]
    Invalid(#[from] InvalidResolution),
}

/// Resolves every call site of the repository at `root`: typed resolution over the
/// whole repository ([`typed_resolution`]), then the stages ([`resolve_with`]).
///
/// # Errors
///
/// As for [`typed_resolution`] and [`resolve_with`].
pub fn resolve(root: &Path, registry: &SymbolRegistry) -> Result<ResolveReport, ResolutionError> {
    let typed = typed_resolution(root, registry)?;
    resolve_with(registry, &typed)
}

/// Runs typed resolution over every extracted file of the repository, in path order,
/// with the registry's metadata. Each file's source is read again as Stage 2 read it,
/// and must be the bytes Stage 2 extracted; it is handed to the engine and dropped,
/// not kept.
///
/// # Errors
///
/// [`ResolutionError::SourceChanged`] when a file is not what Stage 2 extracted,
/// [`ResolutionError::SourceRead`] when it cannot be read, and
/// [`ResolutionError::Engine`] when typed resolution fails. A run that completes
/// degraded is not an error: its health says so.
pub fn typed_resolution(
    root: &Path,
    registry: &SymbolRegistry,
) -> Result<ProjectResolution, ResolutionError> {
    let engine = Engine::new()?;
    let mut project = ProjectResolver::new(&engine)?;
    project.set_metadata(registry.resolution_metadata())?;
    for path in registry.files() {
        let Some(FileOutcome::Extracted { extract, blob_sha }) = registry.outcome(path) else {
            continue;
        };
        let discovered = registry
            .discovered(path)
            .ok_or_else(|| ResolutionError::Registry {
                path: path.to_owned(),
                detail: "an extracted file the registry did not discover",
            })?;
        let prepared =
            prepare_source(root, discovered).map_err(|source| ResolutionError::SourceRead {
                path: path.to_owned(),
                source: Box::new(source),
            })?;
        if prepared.blob_sha != *blob_sha || prepared.digest != extract.source_digest {
            return Err(ResolutionError::SourceChanged(path.to_owned()));
        }
        project.add_file(extract, &prepared.bytes)?;
    }
    Ok(project.run()?)
}

/// Settles every site of the registry's extractions with typed resolution's answers
/// `typed`, which must be one answer per site, for sites the extractions have.
///
/// # Errors
///
/// [`ResolutionError::DuplicateEngineAnswer`] for a site answered twice differently,
/// [`ResolutionError::ImpossibleEngineSite`] for an answer about a file with no
/// extraction or a site the extraction does not describe, and
/// [`ResolutionError::Registry`] when a site's file has no language.
pub fn resolve_with(
    registry: &SymbolRegistry,
    typed: &ProjectResolution,
) -> Result<ResolveReport, ResolutionError> {
    let answers = answers_by_site(registry, &typed.resolutions)?;
    let resolver = Resolver { registry };
    let mut resolutions = Vec::new();
    let mut unconfirmed = Vec::new();
    let mut record = |site_ref: SiteRef, site: &Call, outcome: Outcome| match outcome {
        Outcome::Resolved(resolution) => resolutions.push(ResolvedSite {
            site_ref,
            site: site.clone(),
            resolution,
        }),
        Outcome::Unconfirmed(reason, engine) => unconfirmed.push(UnconfirmedSite {
            site_ref,
            site: site.clone(),
            reason,
            engine,
        }),
    };
    for path in registry.files() {
        let Some(extract) = registry.extract(path) else {
            continue;
        };
        for (index, call) in extract.calls.iter().enumerate() {
            let site_ref = SiteRef {
                rel_path: path.to_owned(),
                call_index: u32::try_from(index).map_err(|_| ResolutionError::Registry {
                    path: path.to_owned(),
                    detail: "more calls than an index can count",
                })?,
            };
            let answer = answers.get(&site_ref).copied();
            let outcome = resolver.site(&site_ref, call, answer, false)?;
            record(site_ref, call, outcome);
        }
        // Sites typed resolution found past the extraction's calls, in index order.
        let past = SiteRef {
            rel_path: path.to_owned(),
            call_index: u32::try_from(extract.calls.len()).unwrap_or(u32::MAX),
        };
        for (site_ref, answer) in answers.range(past..) {
            if site_ref.rel_path != path {
                break;
            }
            let outcome = resolver.site(site_ref, &answer.site, Some(answer), true)?;
            record(site_ref.clone(), &answer.site, outcome);
        }
    }
    Ok(ResolveReport {
        resolutions,
        unconfirmed,
        engine_health: typed.health,
    })
}

/// One answer per site, checked against the extractions.
fn answers_by_site<'t>(
    registry: &SymbolRegistry,
    answers: &'t [TypedResolution],
) -> Result<BTreeMap<SiteRef, &'t TypedResolution>, ResolutionError> {
    let mut by_site: BTreeMap<SiteRef, &TypedResolution> = BTreeMap::new();
    for answer in answers {
        let site = &answer.site_ref;
        let Some(extract) = registry.extract(&site.rel_path) else {
            return Err(ResolutionError::ImpossibleEngineSite {
                site: site.clone(),
                detail: "an answer for a file with no extraction",
            });
        };
        if let Some(call) = usize::try_from(site.call_index)
            .ok()
            .and_then(|i| extract.calls.get(i))
            && call.callee_text != answer.site.callee_text
        {
            return Err(ResolutionError::ImpossibleEngineSite {
                site: site.clone(),
                detail: "an answer for a call the extraction names differently",
            });
        }
        match by_site.get(site) {
            None => {
                by_site.insert(site.clone(), answer);
            }
            Some(earlier) if *earlier == answer => {}
            Some(_) => {
                return Err(ResolutionError::DuplicateEngineAnswer { site: site.clone() });
            }
        }
    }
    Ok(by_site)
}

// --- narrowing -------------------------------------------------------------------

/// A stage's evidence: given the current candidates, the ones it supports.
pub type Evidence<'a> = Box<dyn FnMut(&[DefinitionRef]) -> Vec<DefinitionRef> + 'a>;

/// One narrowing stage: the band it assigns, and the candidates of the current set its
/// evidence supports.
pub struct NarrowingStage<'a> {
    /// The band a site gets when this stage leaves exactly one candidate.
    pub band: Band,
    /// The candidates of the current set the stage has evidence for. Anything it names
    /// outside the current set is ignored: a stage never adds a candidate.
    pub evidence: Evidence<'a>,
}

/// What narrowing reached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Narrowed {
    /// A stage left exactly one candidate: the site has its band.
    Resolved {
        /// The stage's band.
        band: Band,
        /// The candidate.
        target: DefinitionRef,
    },
    /// No stage left exactly one: the narrowest set reached, sorted, perhaps empty.
    Remaining(Vec<DefinitionRef>),
}

/// Narrows `initial` through `stages`, in order (4.5 Stage 3): the first stage whose
/// evidence leaves exactly one candidate resolves the site; one that leaves several
/// makes them the set every later stage sees; one with no evidence leaves the set as
/// it was, so a stage that does not apply never erases candidates another source
/// established.
pub fn narrow(initial: Vec<DefinitionRef>, stages: Vec<NarrowingStage<'_>>) -> Narrowed {
    let mut current = initial;
    current.sort();
    current.dedup();
    for mut stage in stages {
        let mut subset: Vec<DefinitionRef> = (stage.evidence)(&current)
            .into_iter()
            .filter(|c| current.binary_search(c).is_ok())
            .collect();
        subset.sort();
        subset.dedup();
        match subset.len() {
            0 => {}
            1 => {
                return Narrowed::Resolved {
                    band: stage.band,
                    target: subset.remove(0),
                };
            }
            _ => current = subset,
        }
    }
    Narrowed::Remaining(current)
}

// --- one site --------------------------------------------------------------------

enum Outcome {
    Resolved(Resolution),
    Unconfirmed(Unconfirmed, Option<EngineAnswer>),
}

/// The names a lexical fact binds in the source, so that resolving them by name must
/// not reach a definition of the repository.
const LOCALLY_BOUND: [LexicalFacts; 3] = [
    LexicalFacts::BLOCKED,
    LexicalFacts::BLOCKED_LOCALLY,
    LexicalFacts::LOCALLY_BOUND,
];

/// Where a call's receiver points in the caller's type hierarchy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Receiver {
    /// No receiver, in a language where a bare name is a function, not a member.
    Bare,
    /// The current object or type: `this`, `self`, `$this`, `Self`, `cls`, or a bare
    /// name in a language where a bare name inside a type is a member.
    Current,
    /// The current type's bases: `super`, `super()`, `base`, `parent`.
    Super,
    /// Anything else: a value whose type is not proven here.
    Other,
}

/// Languages in which a bare name inside a type names a member of the type first.
const IMPLICIT_RECEIVER: [&str; 8] = [
    "java", "kotlin", "csharp", "cpp", "scala", "groovy", "swift", "ruby",
];

fn receiver_of(language: &str, receiver: Option<&str>) -> Receiver {
    let Some(receiver) = receiver else {
        return if IMPLICIT_RECEIVER.contains(&language) {
            Receiver::Current
        } else {
            Receiver::Bare
        };
    };
    match (language, receiver) {
        ("python", "self" | "cls")
        | ("rust", "self" | "Self")
        | ("swift" | "ruby" | "objc", "self")
        | ("php", "$this" | "self" | "static")
        | ("perl", "$self")
        | (
            "java" | "kotlin" | "scala" | "groovy" | "csharp" | "cpp" | "typescript" | "javascript",
            "this",
        ) => Receiver::Current,
        ("python", "super()")
        | ("csharp", "base")
        | ("php", "parent")
        | (
            "java" | "kotlin" | "scala" | "groovy" | "swift" | "ruby" | "objc" | "typescript"
            | "javascript",
            "super",
        ) => Receiver::Super,
        _ => Receiver::Other,
    }
}

/// A callee's receiver, if it has one, and its short name: split at the last `.`,
/// `->`, `::` or `\`. A text that would leave no name (an operator such as
/// `operator->`) is all name.
///
/// The one reading of a callee's text: resolution looks the name up with it, and a
/// site's identity (4.2.1) hashes the same name and receiver.
pub fn split_callee(text: &str) -> (Option<&str>, &str) {
    let at = ["->", "::", ".", "\\"]
        .iter()
        .filter_map(|sep| text.rfind(sep).map(|i| (i, sep.len())))
        .max_by_key(|(i, _)| *i);
    match at {
        Some((i, len)) if i + len < text.len() && i > 0 => (Some(&text[..i]), &text[i + len..]),
        _ => (None, text),
    }
}

/// A receiver's parts, split at every `.`, `->`, `::` and `\\`.
fn parts_of(receiver: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut rest = receiver;
    loop {
        let next = ["->", "::", ".", "\\"]
            .iter()
            .filter_map(|sep| rest.find(sep).map(|i| (i, sep.len())))
            .min_by_key(|(i, _)| *i);
        let Some((i, len)) = next else {
            parts.push(rest);
            return parts;
        };
        parts.push(&rest[..i]);
        rest = &rest[i + len..];
    }
}

/// The first part of a receiver: what a name in the file binds.
fn head_of(receiver: &str) -> &str {
    parts_of(receiver).first().copied().unwrap_or(receiver)
}

/// The parts of a receiver after its first.
fn tail_of(receiver: &str) -> Vec<&str> {
    parts_of(receiver).into_iter().skip(1).collect()
}

/// Whether a definition is something a call can invoke.
fn callable(kind: DefinitionKind) -> bool {
    matches!(
        kind,
        DefinitionKind::Function
            | DefinitionKind::Method
            | DefinitionKind::Constructor
            | DefinitionKind::Class
            | DefinitionKind::Struct
            | DefinitionKind::Macro
    )
}

/// Whether a name in one language can name a definition in another: the same
/// language, TypeScript and JavaScript, or C and C++.
fn same_family(a: &str, b: &str) -> bool {
    a == b
        || (matches!(a, "typescript" | "javascript") && matches!(b, "typescript" | "javascript"))
        || (matches!(a, "c" | "cpp") && matches!(b, "c" | "cpp"))
}

struct Resolver<'r> {
    registry: &'r SymbolRegistry,
}

/// What one site's stages need to know about it.
struct SiteContext<'s> {
    path: &'s str,
    language: &'static Language,
    receiver: Option<&'s str>,
    name: &'s str,
    role: Receiver,
    caller: Option<DefinitionRef>,
    current_type: Option<DefinitionRef>,
}

impl Resolver<'_> {
    fn site(
        &self,
        site_ref: &SiteRef,
        call: &Call,
        answer: Option<&TypedResolution>,
        engine_site: bool,
    ) -> Result<Outcome, ResolutionError> {
        let path = site_ref.rel_path.as_str();
        let language = self
            .registry
            .language(path)
            .ok_or_else(|| ResolutionError::Registry {
                path: path.to_owned(),
                detail: "a file with calls has no language",
            })?;
        let (receiver, name) = split_callee(&call.callee_text);
        let engine = answer.map(EngineAnswer::from);
        let mapped = answer.map(|a| self.engine_targets(a)).unwrap_or_default();
        let engine_external = answer.is_some_and(|a| a.target_rel_path.is_none());
        let typed_target = answer
            .filter(|a| {
                from_engine(a.score, a.strategy.as_str(), a.candidates) == EngineVerdict::Typed
            })
            .and_then(|_| match mapped.as_slice() {
                [one] => Some(one.clone()),
                _ => None,
            });

        // Not a call site unless typed resolution settles it.
        let question = if engine_site {
            Some(Unconfirmed::EngineSite)
        } else if call.typed_only {
            Some(Unconfirmed::TypedOnly)
        } else if call.is_reference {
            Some(Unconfirmed::Reference)
        } else {
            None
        };
        if let Some(reason) = question
            && !engine_external
            && typed_target.is_none()
        {
            return Ok(Outcome::Unconfirmed(reason, engine));
        }
        let made = |band, target, candidates| -> Result<Outcome, ResolutionError> {
            Ok(Outcome::Resolved(Resolution::new(
                band,
                target,
                candidates,
                engine.clone(),
            )?))
        };

        // 1. The blocklist, before anything else.
        if blocklist::is_blocked(language.id, name) {
            return made(Band::Blocked, None, Vec::new());
        }
        // 2. The engine's answer: a target in no file of the project, or a typed one.
        if engine_external {
            return made(Band::External, None, Vec::new());
        }
        if let Some(target) = typed_target {
            return made(Band::Typed, Some(target), Vec::new());
        }
        // 3. to 5. By name.
        let resolution = self.by_name(path, language, call, receiver, name, mapped)?;
        Ok(Outcome::Resolved(Resolution {
            engine,
            ..resolution
        }))
    }

    /// A site with no blocklisted name and no typed or external answer, settled by name:
    /// the lexical facts and bindings that stop it, then the Rust stages over its
    /// candidates, then what is left. The engine's hint targets are among the
    /// candidates; its answer is attached by the caller.
    fn by_name(
        &self,
        path: &str,
        language: &'static Language,
        call: &Call,
        receiver: Option<&str>,
        name: &str,
        hinted: Vec<DefinitionRef>,
    ) -> Result<Resolution, ResolutionError> {
        let made = |band, target, candidates| Resolution::new(band, target, candidates, None);
        // 3. What stops resolution by name.
        if LOCALLY_BOUND.iter().any(|f| call.lexical.contains(*f)) {
            return Ok(made(Band::Unresolved, None, Vec::new())?);
        }
        let role = receiver_of(language.id, receiver);
        if receiver.is_none() || role == Receiver::Other {
            let head = receiver.map_or(name, head_of);
            if matches!(
                self.registry.name_provenance(path, head),
                NameProvenance::ExternalImport(_)
            ) {
                return Ok(made(Band::External, None, Vec::new())?);
            }
        }
        // 4. The Rust stages.
        let caller = call.caller.map(|index| DefinitionRef {
            path: path.to_owned(),
            index,
        });
        let current_type = caller.as_ref().and_then(|c| self.current_type(c));
        let context = SiteContext {
            path,
            language,
            receiver,
            name,
            role,
            caller,
            current_type,
        };
        let named = self.named(&context);
        let mut universe: BTreeSet<DefinitionRef> = named.iter().cloned().collect();
        universe.extend(self.by_qualified_text(&context, &call.callee_text));
        universe.extend(self.through_aliases(&context));
        universe.extend(hinted);
        // A member call on a receiver the extractor could not tie to anything is not
        // resolved by its name alone: exact and scoped do not run. Import-guided runs
        // only when the receiver is a name the file's own imports bind into the
        // repository (a module or type imported under it), which follows the import
        // rather than guessing a value's type.
        let arbitrary_member = call.lexical.contains(LexicalFacts::UNRESOLVED_MEMBER);
        let receiver_imported = receiver.is_some_and(|r| {
            matches!(
                self.registry.name_provenance(path, head_of(r)),
                NameProvenance::InternalImport(_)
            )
        });
        let mut stages = Vec::new();
        if !arbitrary_member || receiver_imported {
            stages.push(NarrowingStage {
                band: Band::ImportGuided,
                evidence: Box::new(|current: &[DefinitionRef]| {
                    self.through_imports(&context, current)
                }),
            });
        }
        stages.push(NarrowingStage {
            band: Band::InheritanceGuided,
            evidence: Box::new(|current: &[DefinitionRef]| {
                self.through_hierarchy(&context, current)
            }),
        });
        if !arbitrary_member {
            stages.push(NarrowingStage {
                band: Band::Exact,
                evidence: Box::new(|current: &[DefinitionRef]| match named.as_slice() {
                    [one] if current.contains(one) => vec![one.clone()],
                    _ => Vec::new(),
                }),
            });
            stages.push(NarrowingStage {
                band: Band::Scoped,
                evidence: Box::new(|current: &[DefinitionRef]| self.in_module(&context, current)),
            });
        }
        // 5. What is left.
        match narrow(universe.into_iter().collect(), stages) {
            Narrowed::Resolved { band, target } => Ok(made(band, Some(target), Vec::new())?),
            Narrowed::Remaining(candidates) if !candidates.is_empty() => {
                Ok(made(Band::Candidate, None, candidates)?)
            }
            Narrowed::Remaining(_) => Ok(made(Band::Unresolved, None, Vec::new())?),
        }
    }

    /// The definitions of the repository the engine's answer names: in its file, with
    /// exactly its qualified name. None when its target is in no file of the project.
    fn engine_targets(&self, answer: &TypedResolution) -> Vec<DefinitionRef> {
        let Some(path) = &answer.target_rel_path else {
            return Vec::new();
        };
        self.registry
            .by_qualified_name(&answer.target_qn)
            .iter()
            .filter(|r| &r.path == path)
            .cloned()
            .collect()
    }

    /// The type a call is made from: the caller itself when it is a type, else the type
    /// it is declared in.
    fn current_type(&self, caller: &DefinitionRef) -> Option<DefinitionRef> {
        let definition = self.registry.definition(caller)?;
        if matches!(
            definition.kind,
            DefinitionKind::Class
                | DefinitionKind::Interface
                | DefinitionKind::Struct
                | DefinitionKind::Trait
                | DefinitionKind::Enum
        ) {
            return Some(caller.clone());
        }
        self.registry.declaring_type(caller)
    }

    /// Whether a definition is one a call in this file can invoke by name.
    fn invocable(&self, context: &SiteContext<'_>, r: &DefinitionRef) -> bool {
        self.registry
            .definition(r)
            .is_some_and(|d| callable(d.kind))
            && self
                .registry
                .language(&r.path)
                .is_some_and(|l| same_family(context.language.id, l.id))
    }

    /// The definitions of the callee's short name.
    fn named(&self, context: &SiteContext<'_>) -> Vec<DefinitionRef> {
        self.registry
            .by_name(context.name)
            .iter()
            .filter(|r| self.invocable(context, r))
            .cloned()
            .collect()
    }

    /// The definitions whose qualified name is the callee's whole text.
    fn by_qualified_text(&self, context: &SiteContext<'_>, text: &str) -> Vec<DefinitionRef> {
        if context.receiver.is_none() {
            return Vec::new();
        }
        let dotted = text
            .replace("::", ".")
            .replace("->", ".")
            .replace('\\', ".");
        [text.to_owned(), dotted]
            .iter()
            .flat_map(|qn| self.registry.by_qualified_name(qn).iter())
            .filter(|r| self.invocable(context, r))
            .cloned()
            .collect()
    }

    /// The internal targets an import leads to.
    fn targets(target: &ImportTarget) -> Vec<&InternalTarget> {
        match target {
            ImportTarget::Internal(t) => vec![t],
            ImportTarget::InternalCandidates(ts) => ts.iter().collect(),
            _ => Vec::new(),
        }
    }

    /// Whether `r` is in a file an import target leads to, or in the source a header it
    /// leads to is paired with.
    fn in_target(&self, target: &InternalTarget, r: &DefinitionRef) -> bool {
        target
            .files
            .iter()
            .any(|f| *f == r.path || self.registry.paired_file(f) == Some(r.path.as_str()))
    }

    /// Definitions a bare call reaches under another name: the member an import binds
    /// to the callee's name (`from a import f as g`, `import a.b.C as D`).
    fn through_aliases(&self, context: &SiteContext<'_>) -> Vec<DefinitionRef> {
        if context.receiver.is_some() {
            return Vec::new();
        }
        let mut found = Vec::new();
        for import in self.registry.imports_of(context.path) {
            if !import.bindings.iter().any(|b| b == context.name) {
                continue;
            }
            for target in Self::targets(&import.target) {
                let Some(member) = target.member.as_deref().filter(|m| *m != context.name) else {
                    continue;
                };
                for file in &target.files {
                    found.extend(
                        self.registry
                            .by_file(file)
                            .iter()
                            .filter(|r| {
                                self.registry
                                    .definition(r)
                                    .is_some_and(|d| d.name == member && callable(d.kind))
                            })
                            .cloned(),
                    );
                }
            }
        }
        found
    }

    /// Import-guided: the candidates the file's imports make visible under the call's
    /// name. A bare name is visible through an import that binds it (as the member it
    /// imports, under an alias or not), or through one that binds no name of its own
    /// (a wildcard, a namespace, an include) when the candidate is at the top of its
    /// file, unless the call is inside a type in a language where a bare name there is
    /// a member. A receiver is visible through an import that binds it, as the module
    /// or type the import names, or through one that binds no name when the receiver is
    /// the candidate's type.
    fn through_imports(
        &self,
        context: &SiteContext<'_>,
        current: &[DefinitionRef],
    ) -> Vec<DefinitionRef> {
        current
            .iter()
            .filter(|c| self.import_visible(context, c))
            .cloned()
            .collect()
    }

    fn import_visible(&self, context: &SiteContext<'_>, c: &DefinitionRef) -> bool {
        let Some(definition) = self.registry.definition(c) else {
            return false;
        };
        let declaring = self
            .registry
            .declaring_type(c)
            .and_then(|t| self.registry.definition(&t).map(|d| d.name.clone()));
        let inside_type =
            context.current_type.is_some() && IMPLICIT_RECEIVER.contains(&context.language.id);
        for import in self.registry.imports_of(context.path) {
            let targets = Self::targets(&import.target);
            if targets.is_empty() {
                continue;
            }
            match context.receiver {
                None => {
                    if import.bindings.iter().any(|b| b == context.name) {
                        if targets.iter().any(|t| {
                            self.in_target(t, c)
                                && definition.name == t.member.as_deref().unwrap_or(context.name)
                        }) {
                            return true;
                        }
                    } else if import.bindings.is_empty()
                        && !inside_type
                        && declaring.is_none()
                        && definition.name == context.name
                        && targets.iter().any(|t| self.in_target(t, c))
                    {
                        return true;
                    }
                }
                Some(receiver) => {
                    if definition.name != context.name {
                        continue;
                    }
                    let head = head_of(receiver);
                    let tail = tail_of(receiver);
                    if import.bindings.iter().any(|b| b == head) {
                        let fits = |t: &&InternalTarget| {
                            self.in_target(t, c)
                                && match (t.member.as_deref(), tail.as_slice()) {
                                    // The receiver is the module: a function at its top,
                                    // or a member of a type named after it.
                                    (None, []) => declaring.is_none(),
                                    (None, [ty]) => declaring.as_deref() == Some(*ty),
                                    // The receiver is the type the import names.
                                    (Some(member), []) => declaring.as_deref() == Some(member),
                                    _ => false,
                                }
                        };
                        if targets.iter().any(fits) {
                            return true;
                        }
                    } else if import.bindings.is_empty()
                        && tail.is_empty()
                        && declaring.as_deref() == Some(receiver)
                        && targets.iter().any(|t| self.in_target(t, c))
                    {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Inheritance-guided: the candidates declared in the hierarchy of the type the call
    /// is made from, when the receiver is that type (or its bases, for `super`). The
    /// hierarchy is the type, its internal bases and, for Rust, the internal traits it
    /// implements directly (issue 42), level by level and each type once. Where no type
    /// on the way has more than one base, the nearest level declaring a candidate
    /// decides; otherwise every candidate the hierarchy declares stays.
    fn through_hierarchy(
        &self,
        context: &SiteContext<'_>,
        current: &[DefinitionRef],
    ) -> Vec<DefinitionRef> {
        let Some(start) = (match context.role {
            Receiver::Current => Some(0),
            Receiver::Super => Some(1),
            Receiver::Bare | Receiver::Other => None,
        }) else {
            return Vec::new();
        };
        let Some(current_type) = &context.current_type else {
            return Vec::new();
        };
        let levels = self.hierarchy(current_type);
        let declared_in = |c: &DefinitionRef| self.registry.declaring_type(c);
        let mut linear = true;
        let mut everywhere = Vec::new();
        let mut nearest: Option<Vec<DefinitionRef>> = None;
        for level in levels.iter().skip(start) {
            if level.len() > 1 {
                linear = false;
            }
            let here: Vec<DefinitionRef> = current
                .iter()
                .filter(|c| declared_in(c).is_some_and(|t| level.contains(&t)))
                .cloned()
                .collect();
            if !here.is_empty() && nearest.is_none() && linear {
                nearest = Some(here.clone());
            }
            everywhere.extend(here);
        }
        nearest.unwrap_or(everywhere)
    }

    /// A type's hierarchy, level by level: the type, then its internal bases and the
    /// internal traits it implements, then theirs; each type once, so a cycle ends.
    fn hierarchy(&self, root: &DefinitionRef) -> Vec<Vec<DefinitionRef>> {
        let mut seen = BTreeSet::from([root.clone()]);
        let mut levels = vec![vec![root.clone()]];
        loop {
            let mut next = Vec::new();
            for ty in levels.last().into_iter().flatten() {
                let bases =
                    self.registry
                        .direct_bases(ty)
                        .iter()
                        .filter_map(|b| match &b.resolution {
                            BaseResolution::Internal(base) => Some(base.clone()),
                            _ => None,
                        });
                let traits = self.registry.implemented_traits(ty).iter().cloned();
                for parent in bases.chain(traits) {
                    if seen.insert(parent.clone()) {
                        next.push(parent);
                    }
                }
            }
            if next.is_empty() {
                return levels;
            }
            levels.push(next);
        }
    }

    /// Scoped: the candidates in the module the call is made from: the caller's module
    /// when it has one, else the file's when it has exactly one. No module, no
    /// evidence: a directory is a module only where the language matrix says so.
    fn in_module(
        &self,
        context: &SiteContext<'_>,
        current: &[DefinitionRef],
    ) -> Vec<DefinitionRef> {
        let module = context
            .caller
            .as_ref()
            .and_then(|c| self.registry.module_of_definition(c))
            .or_else(|| self.registry.module_of_file(context.path));
        let Some(module) = module else {
            return Vec::new();
        };
        current
            .iter()
            .filter(|c| self.registry.module_of_definition(c) == Some(module))
            .cloned()
            .collect()
    }
}
