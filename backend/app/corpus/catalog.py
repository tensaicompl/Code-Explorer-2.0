"""What has been indexed, and how to address it.

The catalogue answers "which project/stream corpora exist, are they indexed, and
how large are they". It does not own project configuration -- harvest/registry.py
does. It only discovers which of those projects actually have searchable content,
and caches that answer because every single tool call needs it.

The cache is time-bounded rather than invalidated. Going stale costs at most one
refresh interval of reporting a project as unindexed just after its first index
run, which is a better trade than threading invalidation calls through every
write path in the application.
"""
from __future__ import annotations

import logging
import re
import time
from pathlib import Path
from typing import Dict

from ..config import CODEBASE_DIR
from .db import get_pooled_connection, release_pooled_connection

logger = logging.getLogger("praxevia.corpus.catalog")

#: File types the indexer treats as source. Used when a tool needs to decide
#: whether a path is worth reading or reporting.
SOURCE_EXTENSIONS = {
    ".ads", ".adb", ".ada",
    ".c", ".h", ".hh", ".cc",
    ".py",
    ".pl", ".pm",
    ".sh", ".bash", ".ksh",
    ".cpp", ".hpp",
    ".rb",
    ".idl", ".incl",
    ".sql",
    ".gpr",
    ".json", ".yaml", ".yml", ".cfg", ".conf", ".ini",
    ".java", ".kt", ".scala", ".ts", ".js", ".tsx", ".jsx",
    ".properties", ".gradle", ".pom",
}

#: Directory names that are build output or tooling, never project content.
IGNORED_DIRS = {"scripts", "__pycache__", ".git", "target", "build", "node_modules"}

#: Prefix the indexer gives each corpus's chunk table.
_CHUNK_TABLE_PREFIX = "code_chunks_"

#: How long a catalogue snapshot is trusted before it is rebuilt.
_REGISTRY_TTL_SECONDS = 30.0

#: Every addressable corpus, keyed by registry key.
#:
#: Mutated in place, never rebound -- see refresh_corpus_registry. Other modules
#: import this name directly, and rebinding it here would leave them holding the
#: previous dict forever.
CORPUS_REGISTRY: Dict[str, dict] = {}

_last_refreshed: float = 0.0


def sanitize_registry_key(name: str) -> str:
    """Flatten a project or stream name into an identifier-safe fragment.

    Must stay identical to the indexer's version of this rule and to
    harvest.registry's: all three independently derive the same table name from
    the same inputs, and a divergence means the backend queries a table the
    indexer never wrote.
    """
    return re.sub(r"[^a-z0-9]", "_", name.lower())


def project_label(registry_key: str) -> str:
    """Render a registry key as the "project/stream" form people and models use.

    Falls back to the raw key when the corpus is not catalogued, so a label is
    always produced rather than a blank that would read as a missing project.
    """
    corpus = CORPUS_REGISTRY.get(registry_key, {})
    return f"{corpus.get('project', registry_key)}/{corpus.get('stream', '')}"


def _corpus_table_exists(registry_key: str) -> bool:
    """Whether the indexer has produced a chunk table for this corpus."""
    conn = None
    try:
        conn = get_pooled_connection()
        cur = conn.cursor()
        cur.execute(
            "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = %s)",
            (f"{_CHUNK_TABLE_PREFIX}{registry_key}",),
        )
        exists = cur.fetchone()[0]
        cur.close()
        return exists
    except Exception as exc:
        logger.warning("could not check whether %s is indexed: %s", registry_key, exc)
        return False
    finally:
        if conn:
            release_pooled_connection(conn)


def _corpus_chunk_count(registry_key: str) -> int:
    """How many chunks this corpus holds; 0 if it cannot be counted."""
    conn = None
    try:
        conn = get_pooled_connection()
        cur = conn.cursor()
        cur.execute(f'SELECT COUNT(*) FROM "{_CHUNK_TABLE_PREFIX}{registry_key}"')
        count = cur.fetchone()[0]
        cur.close()
        return count
    except Exception:
        return 0
    finally:
        if conn:
            release_pooled_connection(conn)


def discover_indexed_corpora() -> Dict[str, dict]:
    """Build the catalogue by walking the source volume.

    The fallback path, used when the configured-project tables cannot be read.
    It sees only what is on disk, so it cannot report a project's mode -- but a
    catalogue built from directories is better than none.
    """
    if not CODEBASE_DIR.exists():
        return {}

    found: Dict[str, dict] = {}
    for project_dir in sorted(CODEBASE_DIR.iterdir()):
        if not project_dir.is_dir() or project_dir.name in IGNORED_DIRS:
            continue
        for stream_dir in sorted(project_dir.iterdir()):
            if not stream_dir.is_dir() or stream_dir.name in IGNORED_DIRS:
                continue

            key = (
                f"{sanitize_registry_key(project_dir.name)}_"
                f"{sanitize_registry_key(stream_dir.name)}"
            )
            indexed = _corpus_table_exists(key)
            found[key] = {
                "project": project_dir.name,
                "stream": stream_dir.name,
                "codebase_dir": stream_dir,
                "indexed": indexed,
                "chunk_count": _corpus_chunk_count(key) if indexed else 0,
            }

    return found


def refresh_corpus_registry() -> None:
    """Rebuild the catalogue from configuration, falling back to the disk scan.

    CORPUS_REGISTRY is cleared and repopulated rather than reassigned. Several
    modules do `from .catalog import CORPUS_REGISTRY`, which binds the object
    itself -- rebinding the name here would update only this module's view and
    leave every reader looking at a dict that never changes again.
    """
    global _last_refreshed
    try:
        from ..harvest import build_workspace_registry

        rebuilt = build_workspace_registry()
    except Exception:
        logger.warning("could not read configured projects; cataloguing from disk instead")
        rebuilt = discover_indexed_corpora()

    CORPUS_REGISTRY.clear()
    CORPUS_REGISTRY.update(rebuilt)
    _last_refreshed = time.time()
    logger.info("catalogue holds %d corpora", len(CORPUS_REGISTRY))


def ensure_corpus_registry() -> Dict[str, dict]:
    """The catalogue, rebuilt first if it is empty or has gone stale."""
    if not CORPUS_REGISTRY or (time.time() - _last_refreshed) > _REGISTRY_TTL_SECONDS:
        refresh_corpus_registry()
    return CORPUS_REGISTRY


def get_corpus_summary(registry_key: str) -> str:
    """A short description of one corpus, for injecting into a system prompt.

    Deliberately terse. This text is prepended to every conversation about the
    project, so it buys orientation -- what the thing is called, how much of it
    is indexed, what the top-level structure looks like -- at the smallest token
    cost that still answers those questions. The module list is capped for the
    same reason.
    """
    corpus = CORPUS_REGISTRY.get(registry_key)
    if not corpus:
        return ""

    codebase_dir: Path = corpus["codebase_dir"]
    if not codebase_dir.exists():
        return ""

    top_level = sorted(
        entry.name
        for entry in codebase_dir.iterdir()
        if entry.is_dir() and not entry.name.startswith(".")
    )
    shown = top_level[:30]

    lines = [
        f"Project: {corpus.get('project', registry_key)}, Stream: {corpus.get('stream', '')}",
        f"Indexed chunks: {corpus.get('chunk_count', 0):,}",
        f"Top-level modules ({len(top_level)}): {', '.join(shown)}",
    ]
    if len(top_level) > len(shown):
        lines[-1] += f" ... (+{len(top_level) - len(shown)} more)"
    return "\n".join(lines)
