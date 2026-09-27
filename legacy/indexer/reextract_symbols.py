#!/usr/bin/env python3
"""
Re-run symbol extraction over an ALREADY-INDEXED project, without re-embedding.

WHY THIS HAS TO EXIST. `index_project_stream` decides what to do by comparing file
hashes: unchanged files are skipped, and when nothing changed at all it logs
"nothing to re-index, skipping" and returns (build_corpus.py:1596). That is correct for
its own job — the files really have not changed — but it means CHANGING AN
EXTRACTOR has no effect whatsoever. Adding `http_routes.py` emitted exactly zero
route symbols for every already-indexed project, silently, because no file
content moved.

There is already a symbol-only path that does the right work
(`_run_symbol_only_pass`, build_corpus.py:1455) — it re-extracts every file from the
existing hash list and never touches embeddings. But it only fires when the
symbols table is completely empty (build_corpus.py:1588). This script makes that
reachable deliberately: clear the derived rows, then let the existing pass
rebuild them.

WHAT IT COSTS AND WHAT IT RISKS. Symbols, calls and imports are DERIVED data —
this script regenerates exactly what it deletes, from source files it does not
touch. Chunks and embeddings, the expensive half, are left alone: on the largest
indexed project this is a couple of minutes rather than the hour a full re-index
would take. If it is interrupted midway the symbols table is left short, and
re-running finishes the job.

Usage:
    cd indexer
    DATABASE_URL=... CODEBASE_DIR=... python reextract_symbols.py <project> <stream>
    python reextract_symbols.py --all
"""

from __future__ import annotations

import argparse
import logging
import os
import sys

import psycopg2

import build_corpus as indexer

logging.basicConfig(
    level=logging.INFO, format="%(asctime)s [%(levelname)s] %(message)s"
)
logger = logging.getLogger("reextract")


def _clear_derived(conn, key: str) -> dict[str, int]:
    """Delete symbols/calls/imports for one project+stream. Returns row counts."""
    removed: dict[str, int] = {}
    cur = conn.cursor()
    for prefix in ("symbols", "calls", "imports"):
        table = f"{prefix}_{key}"
        try:
            cur.execute(f'SELECT count(*) FROM "{table}"')
            removed[table] = cur.fetchone()[0]
            cur.execute(f'DELETE FROM "{table}"')
        except psycopg2.Error:
            # A table that predates this version, or a project that never had
            # one. Nothing to clear, and not a reason to stop.
            conn.rollback()
            removed[table] = 0
    conn.commit()
    cur.close()
    return removed


def _require_tree_sitter() -> None:
    """
    Refuse to run without the tree-sitter grammars.

    This clears symbols BEFORE re-extracting, so an environment that cannot
    parse code does not fail loudly — it succeeds, quietly, having replaced a
    complete symbol table with whatever the regex extractors alone could find.
    Running it once in a venv missing `tree_sitter` took shop from 489 symbols
    and 1,125 call edges to 305 symbols and ZERO calls, and the only sign was a
    warning line in the middle of the log.

    `main._load_treesitter_parser` swallows the ImportError per language by design — that
    is right for the indexer, where a missing grammar should degrade one
    language rather than stop the run. It is wrong here, where the destructive
    half has already happened by the time the warning appears.
    """
    try:
        import tree_sitter  # noqa: F401
    except ImportError as e:
        raise SystemExit(
            f"tree-sitter is not installed in this interpreter ({sys.executable}): {e}\n"
            "Re-extracting without it would DELETE every function, class and call "
            "edge and replace them with nothing.\n"
            "Install the indexer's requirements first:\n"
            f"    {sys.executable} -m pip install -r indexer/requirements.txt"
        )


def reextract(project: str, stream: str) -> int:
    key = indexer.registry_key(project, stream) if hasattr(indexer, "registry_key") \
        else f"{project}_{stream}"
    project_dir = os.path.join(indexer.CODEBASE_DIR, project, stream)
    if not os.path.isdir(project_dir):
        logger.error("No such codebase directory: %s", project_dir)
        return 1

    conn = psycopg2.connect(os.environ["DATABASE_URL"])
    table_name = f"code_chunks_{key}"
    hashes = indexer.load_indexed_hashes(conn, table_name)
    if not hashes:
        logger.error(
            "%s/%s has no indexed chunks — run the full indexer first, there is "
            "nothing to re-extract against.", project, stream,
        )
        conn.close()
        return 1

    before = _clear_derived(conn, key)
    logger.info("Cleared derived rows for %s: %s", key, before)

    indexer._run_symbol_only_pass(conn, key, project_dir, hashes)

    cur = conn.cursor()
    cur.execute(f'SELECT kind, count(*) FROM "symbols_{key}" GROUP BY kind ORDER BY 2 DESC')
    counts = dict(cur.fetchall())
    cur.close()
    conn.close()
    logger.info("Re-extracted %s: %s", key, counts)
    logger.info("routes found: %d", counts.get("route", 0))
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[1])
    ap.add_argument("project", nargs="?")
    ap.add_argument("stream", nargs="?", default="develop")
    ap.add_argument("--all", action="store_true",
                    help="every project/stream the indexer can discover")
    args = ap.parse_args()

    if not os.environ.get("DATABASE_URL"):
        logger.error("DATABASE_URL is not set")
        return 2

    _require_tree_sitter()

    if args.all:
        combos = indexer.discover_project_streams()
        logger.info("Re-extracting %d combos: %s", len(combos), combos)
        rc = 0
        for project, stream in combos:
            rc |= reextract(project, stream)
        return rc

    if not args.project:
        ap.error("give a project and stream, or --all")
    return reextract(args.project, args.stream)


if __name__ == "__main__":
    sys.exit(main())
