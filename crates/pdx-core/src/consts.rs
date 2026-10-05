//! Every constant the specification names, defined once.
//!
//! This module is the single source of truth. The interface mirrors the subset it
//! needs in `ui/src/consts.ts`, and `scripts/consts-sync.py` fails the build when
//! the two disagree.
//!
//! These are **defaults**. Where the server configuration lists an environment key
//! of the same meaning, that key overrides the default at run time; the constant is
//! what applies when nothing is set. Each such constant names its key below.
//!
//! Changing a value here changes the specification. The ordering of confidence
//! bands and the meaning of a schema version are fixed; a threshold may be tuned
//! only by a specification change request carrying new measurements.

// --- schema and format versions -------------------------------------------
//
// Independent integers. A reader refuses data whose major version differs from
// the one it was built against, and the server re-indexes rather than guessing.

/// Layout of a segment file, the immutable graph of one repository at one commit.
pub const SEGMENT_SCHEMA_VERSION: u32 = 2;

/// Layout of the control plane's relational schema.
pub const CONTROL_SCHEMA_VERSION: u32 = 1;

/// Version of the agent tool surface: names, inputs and outputs.
pub const MCP_TOOLS_VERSION: u32 = 1;

/// Wire format of a rendering tile.
pub const TILE_FORMAT_VERSION: u32 = 1;

/// Version of the language matrix: which languages exist, how they are detected, and
/// what each recognises as a test. 2 since the test rules are read as issue 31 decided:
/// TypeScript's `.test` and `.spec` on every extension, JavaScript's on its own, and a
/// directory rule at any depth.
pub const LANGUAGE_MATRIX_VERSION: u32 = 2;

/// Version of the extraction engine's behaviour, bumped when its output can change. 2
/// since extraction records each call's and possible reference's node-type path, which
/// it did not compute before (issue 46); 3 since it records every route binding a
/// definition's decorators or annotations declare, with the declaring node's position
/// and node-type path (issue 54).
pub const ENGINE_VERSION: u32 = 3;

/// Version of secret normalisation (5.12): which content detectors run, what each
/// matches, and how a match is masked. Bumped when any of the three changes, since the
/// bytes extracted change with them; it is part of the secret-policy digest, so every
/// extraction cached under the old behaviour stops matching. 2 since a bearer token
/// runs on over its `=` padding and what follows it (issue 44).
pub const SECRET_DETECTOR_VERSION: u32 = 2;

/// Layout of an entry of the extraction cache: the envelope that holds a cached
/// extraction and the key it was stored under. An entry of another version is never
/// read, only replaced. The cache is disposable and is not a segment. 2 since a
/// definition carries its base classes and an extraction its file's declared
/// namespace (issues 40 and 41), 3 since an extraction carries its `impl Trait for
/// Type` relations (issue 42), 4 since a call carries its node-type path and arguments
/// and a definition its decorators, parameter types and route (issues 46 and 47), 5
/// since a definition carries every route binding with its declaring node in place of
/// one route (issue 54).
pub const EXTRACT_CACHE_FORMAT_VERSION: u32 = 5;

// --- resolution ------------------------------------------------------------

/// Least engine score that may yield the `typed` band.
///
/// Below it, a single candidate is still only a candidate. Raising or lowering it
/// changes published accuracy, so it moves only with new measurements.
pub const TYPED_MIN_SCORE: f64 = 0.85;

// --- discovery -------------------------------------------------------------

/// Largest file that is parsed. Anything larger is recorded as skipped, with the
/// reason, so the gap is visible rather than silent.
///
/// Overridden per repository by the discovery configuration.
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

// --- extraction ------------------------------------------------------------

/// Most files one batch of Stage 2 holds, whatever the memory budget would allow.
///
/// Small batches spread over the workers and keep one isolated exchange short beside
/// its timeout. Fixed, so how files are batched never depends on the machine.
/// Unspecified by the plan; set under rule D17 (decision 22).
pub const EXTRACT_BATCH_MAX_FILES: u32 = 16;

/// Largest extraction cache entry that is read, in bytes.
///
/// An extraction of the largest file the index parses is far smaller; a larger entry
/// is not one this program wrote, and is a miss. Unspecified by the plan; set under
/// rule D17 (decision 23).
pub const EXTRACT_CACHE_MAX_ENTRY_BYTES: u64 = 1024 * 1024 * 1024;

// --- retention -------------------------------------------------------------

/// Manifest versions kept before their unreferenced segments may be collected.
///
/// Retention is this many versions **or** [`MANIFEST_RETENTION_DAYS`], whichever is
/// longer. Environment key: `PDX_MANIFEST_RETENTION`.
pub const MANIFEST_RETENTION: u32 = 30;

/// Days a manifest version is kept, the other half of the retention rule.
///
/// The specification states the rule but names no constant for this half; it is
/// defined here so the rule can be expressed in one place.
pub const MANIFEST_RETENTION_DAYS: u32 = 14;

/// Days an audit record is kept before export and deletion.
///
/// Environment key: `PDX_AUDIT_RETENTION_DAYS`.
pub const AUDIT_RETENTION_DAYS: u32 = 365;

// --- agent surface ---------------------------------------------------------

/// Output budget a tool call assumes when the caller names none, in tokens.
///
/// Enforced as four bytes per token. When the budget is reached, whole rows are
/// dropped from the end and the response says so; an identifier is never truncated.
pub const MCP_DEFAULT_BUDGET: u32 = 4_000;

/// Lifetime of a view handle, in seconds.
///
/// A handle binds a manifest and a permission set, so consistency never depends on
/// a transport session. Environment key: `PDX_VIEW_HANDLE_TTL`.
pub const VIEW_HANDLE_TTL: u64 = 24 * 60 * 60;

/// Requests per second allowed to one principal.
///
/// Environment key: `PDX_RATE_LIMIT_RPS`.
pub const RATE_LIMIT_RPS: u32 = 50;

// --- synchronisation -------------------------------------------------------

/// Seconds between permission synchronisations with the code host.
///
/// Membership also arrives by webhook; this interval is the floor that catches what
/// a webhook missed. Environment key: `PDX_PERMSYNC_INTERVAL`.
pub const PERMSYNC_INTERVAL: u64 = 15 * 60;

/// Seconds between polls of a code host, as the fallback when webhooks are absent.
///
/// Environment key: `PDX_POLL_INTERVAL`.
pub const POLL_INTERVAL: u64 = 10 * 60;

// --- storage ---------------------------------------------------------------

/// Bytes of local segment cache kept before the least recently used are evicted.
///
/// Environment key: `PDX_SEGMENT_CACHE_BYTES`.
pub const SEGMENT_CACHE_BYTES: u64 = 20 * 1024 * 1024 * 1024;

// --- rendering budget ------------------------------------------------------
//
// The frame budget is what lets the map stay interactive with no cap on the size
// of the graph: the client drops the deepest tiles until it fits, rather than
// refusing to draw a large system.

/// Point primitives a frame may draw.
pub const FRAME_BUDGET_POINTS: u32 = 200_000;

/// Line primitives a frame may draw.
pub const FRAME_BUDGET_LINES: u32 = 300_000;

/// Labels a frame may draw, ranked by importance.
pub const FRAME_BUDGET_LABELS: u32 = 2_000;

/// Least on-screen size, in pixels, at which a label is drawn at all.
pub const LABEL_MIN_PX: u32 = 14;

/// Pixels of zoom over which one level fades out while its children fade in.
pub const FADE_BAND_PX: u32 = 8;

/// On-screen size, in pixels, at which a container opens to show its children.
pub const CONTAINER_OPEN_PX: u32 = 240;

/// Most nodes the neighbourhood view will lay out around a focus.
pub const NEIGHBOURHOOD_MAX_NODES: u32 = 20_000;

/// Seed for every layout that uses randomness, so a layout is reproducible.
pub const LAYOUT_SEED: u64 = 42;

// --- history analytics -----------------------------------------------------

/// Days of history read when computing churn, co-change and ownership.
pub const HOTSPOT_WINDOW_DAYS: u32 = 180;

/// Most files a commit may touch and still count as evidence of co-change.
///
/// A sweeping commit couples everything it touches to everything else, which says
/// nothing about the code.
pub const COCHANGE_MAX_FILES: u32 = 20;

/// Fewest shared commits before two files are called co-changing.
pub const COCHANGE_MIN: u32 = 3;

// --- invariants -----------------------------------------------------------
//
// Asserted at compile time rather than in a test: these are properties of the
// values themselves, so a violation should stop the build wherever it is built,
// not wait for a test run. A failing assertion here names the rule it broke.

const _: () = assert!(
    TYPED_MIN_SCORE > 0.0 && TYPED_MIN_SCORE <= 1.0,
    "a score threshold outside zero to one could never be reached"
);

const _: () = assert!(
    FRAME_BUDGET_LINES > FRAME_BUDGET_POINTS,
    "the renderer assumes a graph has more edges to draw than nodes"
);

const _: () = assert!(
    FRAME_BUDGET_LABELS < FRAME_BUDGET_POINTS,
    "labels are the scarcest primitive; more of them than nodes is meaningless"
);

const _: () = assert!(
    CONTAINER_OPEN_PX > LABEL_MIN_PX * 2,
    "a container that opens before its own label is legible cannot be navigated"
);

const _: () = assert!(
    MANIFEST_RETENTION > 0 && MANIFEST_RETENTION_DAYS > 0,
    "retention must keep something"
);

const _: () = assert!(
    AUDIT_RETENTION_DAYS >= 365,
    "an audit trail is kept for a year at least"
);

const _: () = assert!(
    MAX_FILE_BYTES > 0,
    "a maximum file size of zero would skip every file"
);

const _: () = assert!(
    EXTRACT_BATCH_MAX_FILES > 0,
    "a batch must be able to hold a file"
);

const _: () = assert!(
    EXTRACT_CACHE_MAX_ENTRY_BYTES > MAX_FILE_BYTES,
    "an entry is never refused for being the size of the file it extracts"
);

#[cfg(test)]
mod doc_tests {
    //! Every constant is documented, checked against the source text itself.
    //!
    //! The compiler's own missing-documentation warning covers the public items,
    //! and this asserts the same thing independently: a specification value written
    //! as a bare number tells a reader nothing about what it governs, and these
    //! files are read far more often than they are written.

    use std::path::Path;

    /// Names of the constants in a source file, paired with whether a doc comment
    /// precedes them.
    fn documented(source: &str, marker: &str) -> Vec<(String, bool)> {
        let lines: Vec<&str> = source.lines().collect();
        let mut out = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let Some(rest) = line.strip_prefix(marker) else {
                continue;
            };
            let name: String = rest
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
                .collect();
            if name.is_empty() {
                continue;
            }
            let has_doc = i > 0 && lines[i - 1].trim_start().starts_with("///");
            out.push((name, has_doc));
        }
        out
    }

    #[test]
    fn consts_have_docs() {
        let source = include_str!("consts.rs");
        let found = documented(source, "pub const");
        assert!(
            found.len() >= 25,
            "only {} constants found; the parser is not seeing the file",
            found.len()
        );
        let undocumented: Vec<&str> = found
            .iter()
            .filter(|(_, ok)| !ok)
            .map(|(name, _)| name.as_str())
            .collect();
        assert!(
            undocumented.is_empty(),
            "constants without a doc comment: {undocumented:?}"
        );
    }

    #[test]
    fn consts_have_docs_detects_a_missing_one() {
        // The check above is only worth having if it can fail.
        let found = documented("pub const UNDOCUMENTED: u32 = 1;\n", "pub const");
        assert_eq!(found, vec![("UNDOCUMENTED".to_owned(), false)]);
    }

    #[test]
    fn the_interface_mirror_is_documented_too() {
        let ts = include_str!("../../../ui/src/consts.ts");
        let undocumented: Vec<String> = ts
            .lines()
            .enumerate()
            .filter_map(|(i, line)| {
                let rest = line.strip_prefix("export const")?;
                let name: String = rest
                    .trim_start()
                    .chars()
                    .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
                    .collect();
                let previous = ts
                    .lines()
                    .nth(i.saturating_sub(1))
                    .unwrap_or("")
                    .trim_start();
                // A single-line doc block, or the closing line of a multi-line one.
                let documented = previous.starts_with("/**") || previous.starts_with('*');
                (!name.is_empty() && !documented).then_some(name)
            })
            .collect();
        assert!(
            undocumented.is_empty(),
            "mirrored constants without a doc comment: {undocumented:?}"
        );
    }

    #[test]
    fn the_mirror_list_is_not_empty_and_every_entry_is_real() {
        let list = include_str!("../../../scripts/consts-mirror.txt");
        let source = include_str!("consts.rs");
        let names: Vec<&str> = list
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        assert!(!names.is_empty(), "the mirror list is empty");
        for name in names {
            assert!(
                source.contains(&format!("pub const {name}")),
                "{name} is required in both definitions but is not defined here"
            );
        }
    }

    /// The source file lives where the other tooling expects it.
    #[test]
    fn the_source_of_truth_is_where_the_tooling_looks() {
        let here = Path::new(file!());
        assert!(
            here.ends_with("consts.rs"),
            "this test moved away from the constants it checks"
        );
    }
}
