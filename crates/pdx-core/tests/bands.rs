//! Confidence bands (specification 4.2.2) and the engine's part in them (Appendix D.3).

use pdx_core::bands::{self, Band, CandidateBand, EngineVerdict, ResolverStage};
use pdx_core::consts::TYPED_MIN_SCORE;

/// The bands, as 4.2.2 lists them, in descending confidence.
const DESCENDING: [(&str, bool); 11] = [
    ("precise", true),
    ("typed", true),
    ("import-guided", true),
    ("inheritance-guided", true),
    ("exact", true),
    ("scoped", true),
    ("candidate", false),
    ("external", false),
    ("blocked", false),
    ("unresolved", false),
    ("contradicted", false),
];

fn band(name: &str) -> Band {
    Band::parse(name).unwrap_or_else(|| panic!("{name} is a band"))
}

#[test]
fn band_order_total() {
    let all: Vec<Band> = DESCENDING.iter().map(|(name, _)| band(name)).collect();
    assert_eq!(all.len(), 11);
    assert_eq!(Band::ALL, all.as_slice(), "Band::ALL is 4.2.2's order");
    for (i, a) in all.iter().enumerate() {
        // The exact place of each band: higher compares as more confident.
        assert_eq!(usize::from(a.confidence_rank()), 10 - i, "{a}");
        for (j, b) in all.iter().enumerate() {
            // Exactly one of <, =, > holds, and it is the listing's.
            assert_eq!(a.cmp(b), j.cmp(&i), "{a} against {b}");
            assert_eq!(a.partial_cmp(b), Some(a.cmp(b)));
            // Antisymmetry.
            assert_eq!(a.cmp(b).reverse(), b.cmp(a));
            for c in &all {
                // Transitivity.
                if a > b && b > c {
                    assert!(a > c, "{a} > {b} > {c}");
                }
            }
        }
    }
    let mut shuffled = all.clone();
    shuffled.reverse();
    shuffled.sort();
    shuffled.reverse();
    assert_eq!(shuffled, all, "sorting recovers 4.2.2's order");
    assert_eq!(all.iter().max(), Some(&Band::Precise));
    assert_eq!(all.iter().min(), Some(&Band::Contradicted));
}

#[test]
fn drawn_bands_are_exactly_the_six() {
    for (name, drawn) in DESCENDING {
        assert_eq!(band(name).is_drawn(), drawn, "{name}");
    }
    assert_eq!(Band::ALL.iter().filter(|b| b.is_drawn()).count(), 6);
}

#[test]
fn bands_serialise_as_their_names() {
    for (name, _) in DESCENDING {
        let b = band(name);
        assert_eq!(b.as_str(), name);
        assert_eq!(b.to_string(), name);
        assert_eq!(
            serde_json::to_string(&b).expect("json"),
            format!("\"{name}\"")
        );
        assert_eq!(
            serde_json::from_str::<Band>(&format!("\"{name}\"")).expect("json"),
            b
        );
    }
    // No other spelling is a band, and observation is not one.
    for not_a_band in [
        "observed",
        "import_guided",
        "ImportGuided",
        "Typed",
        "TYPED",
        "",
        "inheritance_guided",
    ] {
        assert_eq!(Band::parse(not_a_band), None, "{not_a_band}");
        assert!(serde_json::from_str::<Band>(&format!("\"{not_a_band}\"")).is_err());
    }
}

#[test]
fn candidate_bands_are_the_non_drawn_five() {
    let names: Vec<&str> = CandidateBand::ALL.iter().map(|b| b.as_str()).collect();
    assert_eq!(
        names,
        [
            "candidate",
            "external",
            "blocked",
            "unresolved",
            "contradicted"
        ]
    );
    for b in Band::ALL {
        let candidate = CandidateBand::try_from(*b);
        assert_eq!(candidate.is_ok(), !b.is_drawn(), "{b}");
        if let Ok(c) = candidate {
            assert_eq!(Band::from(c), *b);
            assert_eq!(
                serde_json::to_string(&c).expect("json"),
                serde_json::to_string(b).expect("json")
            );
        }
    }
    assert!(serde_json::from_str::<CandidateBand>("\"typed\"").is_err());
    assert!(serde_json::from_str::<CandidateBand>("\"precise\"").is_err());
}

/// Appendix D.3's strategies, written out here rather than taken from the engine or
/// from the code under test.
const STRATEGIES: [&str; 11] = [
    "import_map",
    "import_map_suffix",
    "same_module",
    "qualified_suffix",
    "unique_name",
    "suffix_match",
    "fuzzy_single",
    "fuzzy_multi",
    "service_pattern",
    "lsp_typed",
    "unknown",
];

/// What each strategy means, from D.3 and 4.2.2: a typed result, a hint for a Rust
/// stage, or a diagnostic hint only.
fn verdict_for(strategy: &str) -> EngineVerdict {
    match strategy {
        "import_map" | "import_map_suffix" | "qualified_suffix" => {
            EngineVerdict::HintFor(ResolverStage::ImportGuided)
        }
        "same_module" => EngineVerdict::HintFor(ResolverStage::Scoped),
        "unique_name" => EngineVerdict::HintFor(ResolverStage::Exact),
        _ => EngineVerdict::Diagnostic,
    }
}

#[test]
fn from_engine() {
    let just_below = f64::from_bits(TYPED_MIN_SCORE.to_bits() - 1);
    let scores = [
        ("at the threshold", TYPED_MIN_SCORE),
        ("just below", just_below),
        ("zero", 0.0),
        ("one", 1.0),
        ("mid", 0.5),
        ("NaN", f64::NAN),
        ("infinite", f64::INFINITY),
        ("negative", -0.5),
        ("above one", 1.5),
    ];
    let mut rows = 0;
    for strategy in STRATEGIES {
        for (score_name, score) in scores {
            for candidates in [0u32, 1, 2, 7] {
                let got = bands::from_engine(score, strategy, candidates);
                // Only lsp_typed, with one candidate and a well-formed score at or above
                // the threshold, is typed. Everything else is a hint, whatever its score.
                let well_formed_typed = (TYPED_MIN_SCORE..=1.0).contains(&score);
                let expected = if strategy == "lsp_typed" && candidates == 1 && well_formed_typed {
                    EngineVerdict::Typed
                } else if strategy == "lsp_typed" {
                    EngineVerdict::Diagnostic
                } else {
                    verdict_for(strategy)
                };
                assert_eq!(
                    got, expected,
                    "{strategy}, score {score_name}, {candidates} candidate(s)"
                );
                // On its own, the engine draws nothing but typed: a hint the Rust stages
                // cannot narrow leaves the site a candidate (D.3).
                let band = if expected == EngineVerdict::Typed {
                    Band::Typed
                } else {
                    Band::Candidate
                };
                assert_eq!(
                    got.unnarrowed_band(),
                    band,
                    "{strategy}, score {score_name}, {candidates}"
                );
                rows += 1;
            }
        }
    }
    assert_eq!(rows, 11 * 9 * 4);

    // The cases 4.2.2 singles out, by name.
    assert_eq!(
        bands::from_engine(TYPED_MIN_SCORE, "lsp_typed", 1),
        EngineVerdict::Typed
    );
    assert_eq!(
        bands::from_engine(just_below, "lsp_typed", 1),
        EngineVerdict::Diagnostic
    );
    assert_eq!(
        bands::from_engine(0.99, "lsp_typed", 2),
        EngineVerdict::Diagnostic
    );
    assert_eq!(
        bands::from_engine(0.99, "lsp_typed", 0),
        EngineVerdict::Diagnostic
    );
    assert_eq!(
        bands::from_engine(1.0, "unique_name", 1),
        EngineVerdict::HintFor(ResolverStage::Exact)
    );
    assert_eq!(
        bands::from_engine(1.0, "suffix_match", 1),
        EngineVerdict::Diagnostic
    );
    // A strategy outside D.3's list, or a spelling of one in another case, is a
    // diagnostic hint and nothing more.
    for unlisted in ["LSP_TYPED", "lsp-typed", "typed", "", "precise", "exact"] {
        assert_eq!(
            bands::from_engine(1.0, unlisted, 1),
            EngineVerdict::Diagnostic,
            "{unlisted:?}"
        );
    }
    // No engine result is ever precise, nor any resolver band.
    for strategy in STRATEGIES {
        for candidates in [0, 1, 2] {
            let b = bands::from_engine(1.0, strategy, candidates).unnarrowed_band();
            assert!(
                matches!(b, Band::Typed | Band::Candidate),
                "{strategy}: {b}"
            );
        }
    }
}

#[test]
fn appendix_d3_is_the_engine_vocabulary() {
    // The engine's own list of strategies must be D.3's, so a strategy the engine
    // gains cannot reach the graph without a band decision here.
    let engine: Vec<&str> = pdx_engine::Strategy::ALL
        .iter()
        .map(|s| s.as_str())
        .collect();
    assert_eq!(engine, STRATEGIES);
}
