/*
 * limits.h — Generous, env-configurable safety limits (Stage 2 / Track B4).
 *
 * Each knob has a generous default. Hitting a limit degrades to a *reported*
 * skip (surfaced via MCP/CLI/logfile), never a silent drop and never an
 * unbounded read (unbounded just trades a crash for an OOM/hang). Every limit
 * is env-overridable so an operator can tune it per-repo without a rebuild.
 */
#ifndef PDXE_LIMITS_H
#define PDXE_LIMITS_H

/* Result of an attempted per-file read, so callers can attribute a skip to the
 * right phase/reason instead of collapsing every failure into "read failed". */
typedef enum {
    PDXE_READ_OK = 0,    /* file read successfully */
    PDXE_READ_OPEN_FAIL, /* could not open (missing / permission) */
    PDXE_READ_EMPTY,     /* zero/negative size — benign, nothing to index */
    PDXE_READ_OVERSIZED, /* size exceeds pdxe_max_file_bytes() */
    PDXE_READ_OOM,       /* buffer allocation failed */
} pdxe_read_status_t;

/* Maximum size (bytes) of a single source file the indexer will read into
 * memory. Files larger than this are skipped-and-reported (phase "oversized"),
 * never silently dropped. Override with PDXE_MAX_FILE_BYTES (a positive integer
 * count of bytes). Default 512 MiB (raised from the historical 100 MB cap).
 *
 * The env var is read on each call — this is intentional: read_file() calls it
 * once per file (negligible), and reading fresh means a test / operator can
 * change the cap via setenv without a process restart or a stale memoized copy
 * leaking across runs. */
long pdxe_max_file_bytes(void);

/* Maximum variable-length path depth for the Cypher engine (the `*min..max`
 * hop ceiling). BOTH the explicit (`*1..N`) and unbounded (`*`, `*..m`) forms
 * are clamped to this, so `[:CALLS*1..1000000]` degrades to a WARN-and-cap
 * rather than an unbounded (cyclic-graph DoS) traversal. Override with
 * PDXE_CYPHER_MAX_DEPTH (a positive integer). Default 10. */
int pdxe_cypher_max_depth(void);

/* Maximum traversal depth for client-driven MCP graph tools (trace_call_path,
 * detect_changes): the client `depth` argument is WARN-clamped to this so an
 * arbitrarily large value cannot drive an unbounded BFS over the shared store.
 * Override with PDXE_MCP_MAX_DEPTH (a positive integer). Default 15. */
int pdxe_mcp_max_depth(void);

#endif /* PDXE_LIMITS_H */
