/**
 * The constants the interface needs, mirrored from the Rust definition.
 *
 * `crates/pdx-core/src/consts.rs` is the single source of truth. This file carries
 * only the subset the interface uses, and `scripts/consts-sync.py` fails the build
 * when a mirrored value here disagrees with the Rust one. Add a constant to
 * `scripts/consts-mirror.txt` to require it in both.
 *
 * Do not introduce a value here that has no Rust counterpart: the renderer and the
 * server would then disagree about the same budget.
 */

// --- schema and format versions -------------------------------------------

/** Wire format of a rendering tile. A tile of another version is refused. */
export const TILE_FORMAT_VERSION = 1;

/** Layout of a segment file, reported by the server and shown on the trust view. */
export const SEGMENT_SCHEMA_VERSION = 2;

/** Version of the agent tool surface, which the chat dock calls through. */
export const MCP_TOOLS_VERSION = 1;

// --- rendering budget ------------------------------------------------------
//
// The client honours these by dropping the deepest tiles until a frame fits, so a
// larger graph costs detail rather than frame rate.

/** Point primitives a frame may draw. */
export const FRAME_BUDGET_POINTS = 200_000;

/** Line primitives a frame may draw. */
export const FRAME_BUDGET_LINES = 300_000;

/** Labels a frame may draw, ranked by importance. */
export const FRAME_BUDGET_LABELS = 2_000;

/** Least on-screen size, in pixels, at which a label is drawn at all. */
export const LABEL_MIN_PX = 14;

/** Pixels of zoom over which one level fades out while its children fade in. */
export const FADE_BAND_PX = 8;

/** On-screen size, in pixels, at which a container opens to show its children. */
export const CONTAINER_OPEN_PX = 240;

/** Most nodes the neighbourhood view will lay out around a focus. */
export const NEIGHBOURHOOD_MAX_NODES = 20_000;

// --- agent surface ---------------------------------------------------------

/** Output budget a tool call assumes when none is given, in tokens. */
export const MCP_DEFAULT_BUDGET = 4_000;
