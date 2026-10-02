//! Confidence bands (specification 4.2.2), and what the engine's answer contributes to
//! one (Appendix D.3).
//!
//! A band is what every consumer reads: graph, tiles, agent tools, blast radius. The
//! engine's own score, strategy and candidate count are kept verbatim beside it for
//! calibration (`Edge::engine_*`), never in place of it.

use std::cmp::Ordering;

use crate::consts::TYPED_MIN_SCORE;
use crate::vocabulary::vocabulary;

vocabulary! {
    /// How sure the graph is of an edge (4.2.2). Eleven, in descending confidence.
    ///
    /// Observation at run time is not a band: it is a flag beside one, and never
    /// changes it.
    pub enum Band {
        /// Confirmed by a compiler-backed index.
        Precise => "precise",
        /// Engine type resolution reached exactly one target.
        Typed => "typed",
        /// Name resolution restricted by the file's imports reached exactly one target.
        ImportGuided => "import-guided",
        /// Resolved through the receiver's class hierarchy to exactly one target.
        InheritanceGuided => "inheritance-guided",
        /// The name has exactly one definition in the repository.
        Exact => "exact",
        /// Exactly one definition in the same module or package.
        Scoped => "scoped",
        /// More than one candidate survives, or the engine's score is too low.
        Candidate => "candidate",
        /// The target is outside the repository.
        External => "external",
        /// The callee's name is on the generic-name blocklist.
        Blocked => "blocked",
        /// No candidate.
        Unresolved => "unresolved",
        /// A drawn edge a precise index says does not exist.
        Contradicted => "contradicted",
    }
}

impl Band {
    /// The band's place in 4.2.2's order: 10 for `precise`, the most confident, down to
    /// 0 for `contradicted`. Bands compare by it, so a higher band is a more confident
    /// one, whatever order the values are declared in.
    pub const fn confidence_rank(self) -> u8 {
        match self {
            Self::Precise => 10,
            Self::Typed => 9,
            Self::ImportGuided => 8,
            Self::InheritanceGuided => 7,
            Self::Exact => 6,
            Self::Scoped => 5,
            Self::Candidate => 4,
            Self::External => 3,
            Self::Blocked => 2,
            Self::Unresolved => 1,
            Self::Contradicted => 0,
        }
    }

    /// Whether edges of this band are drawn: in the graph, tiles, traversals and blast
    /// radius. The rest are reachable through coverage and candidates only.
    pub const fn is_drawn(self) -> bool {
        matches!(
            self,
            Self::Precise
                | Self::Typed
                | Self::ImportGuided
                | Self::InheritanceGuided
                | Self::Exact
                | Self::Scoped
        )
    }
}

impl Ord for Band {
    fn cmp(&self, other: &Self) -> Ordering {
        self.confidence_rank().cmp(&other.confidence_rank())
    }
}

impl PartialOrd for Band {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

vocabulary! {
    /// The bands a non-drawn call site can have, which are the only ones the
    /// `candidates` table accepts (4.3). A drawn band cannot be one.
    pub enum CandidateBand {
        /// `candidate`.
        Candidate => "candidate",
        /// `external`.
        External => "external",
        /// `blocked`.
        Blocked => "blocked",
        /// `unresolved`.
        Unresolved => "unresolved",
        /// `contradicted`.
        Contradicted => "contradicted",
    }
}

impl From<CandidateBand> for Band {
    fn from(band: CandidateBand) -> Self {
        match band {
            CandidateBand::Candidate => Self::Candidate,
            CandidateBand::External => Self::External,
            CandidateBand::Blocked => Self::Blocked,
            CandidateBand::Unresolved => Self::Unresolved,
            CandidateBand::Contradicted => Self::Contradicted,
        }
    }
}

impl TryFrom<Band> for CandidateBand {
    /// The band, which is drawn.
    type Error = Band;

    fn try_from(band: Band) -> Result<Self, Band> {
        match band {
            Band::Candidate => Ok(Self::Candidate),
            Band::External => Ok(Self::External),
            Band::Blocked => Ok(Self::Blocked),
            Band::Unresolved => Ok(Self::Unresolved),
            Band::Contradicted => Ok(Self::Contradicted),
            drawn => Err(drawn),
        }
    }
}

/// A Rust resolver stage an engine strategy is a hint for (Appendix D.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResolverStage {
    /// The import-guided stage.
    ImportGuided,
    /// The scoped stage.
    Scoped,
    /// The exact stage.
    Exact,
}

/// What the engine's answer for a site contributes to its band.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EngineVerdict {
    /// The engine resolved the site by type: its band is `typed`.
    Typed,
    /// A hint for that Rust stage. It seeds the stage's candidates and never restricts
    /// them; the band is the stage's to decide, not the engine's.
    HintFor(ResolverStage),
    /// A diagnostic hint only.
    Diagnostic,
}

impl EngineVerdict {
    /// The site's band when no Rust stage narrows it to one target: `typed` for a typed
    /// verdict, `candidate` for any hint (Appendix D.3). The engine alone never yields
    /// any other band.
    pub const fn unnarrowed_band(self) -> Band {
        match self {
            Self::Typed => Band::Typed,
            Self::HintFor(_) | Self::Diagnostic => Band::Candidate,
        }
    }
}

/// What an engine resolution means for its site's band (Appendix D.3, 4.2.2).
///
/// `strategy` is the normalised strategy the engine reports, `candidates` how many
/// targets it considered and `score` its confidence, which is between 0 and 1.
///
/// Only `lsp_typed` with exactly one candidate and a score of at least
/// [`TYPED_MIN_SCORE`] is typed. `import_map`, `import_map_suffix` and
/// `qualified_suffix` are hints for the import-guided stage, `same_module` for the
/// scoped stage and `unique_name` for the exact stage, which alone decide those bands.
/// Everything else is a diagnostic hint: `suffix_match`, `fuzzy_single`,
/// `fuzzy_multi`, `service_pattern`, `unknown`, an `lsp_typed` answer short of the
/// conditions, and any string outside D.3's list. A score that is not a number
/// between 0 and 1 is taken as no evidence, so a malformed answer can never be typed.
pub fn from_engine(score: f64, strategy: &str, candidates: u32) -> EngineVerdict {
    match strategy {
        "lsp_typed" if candidates == 1 && (TYPED_MIN_SCORE..=1.0).contains(&score) => {
            EngineVerdict::Typed
        }
        "import_map" | "import_map_suffix" | "qualified_suffix" => {
            EngineVerdict::HintFor(ResolverStage::ImportGuided)
        }
        "same_module" => EngineVerdict::HintFor(ResolverStage::Scoped),
        "unique_name" => EngineVerdict::HintFor(ResolverStage::Exact),
        _ => EngineVerdict::Diagnostic,
    }
}
