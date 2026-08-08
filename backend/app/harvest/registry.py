"""The workspace registry: which project/stream pairs exist and are searchable.

Owns the DDL for every table in the harvest package, for the same reason
warden/permissions.py owns warden's: these five tables reference each other
through foreign keys, so they have to be created in one place in one order.

`build_workspace_registry` is the bridge between what an operator configured and
what is actually on disk and in the index. It answers with a dict keyed by
"registry key" -- the project and stream names flattened into a single
identifier -- which is the same key the indexer uses to name its per-stream
tables. That correspondence is the whole reason the flattening rule lives here
and is applied consistently: get it wrong and the backend looks for a table the
indexer never wrote.
"""
from __future__ import annotations

import logging
import re
from typing import Dict

from ..config import CODEBASE_DIR
from ..db import read_cursor, run_once, transaction

logger = logging.getLogger("praxevia.harvest.registry")

#: Directory names that are never a project or a stream.
IGNORED_DIRS = {"scripts", "__pycache__", ".git", "node_modules"}

#: Prefix the indexer gives its per-stream chunk tables.
_CHUNK_TABLE_PREFIX = "code_chunks_"


def _slugify_table_name(name: str) -> str:
    """Flatten a project or stream name into an identifier-safe fragment.

    Everything outside [a-z0-9] collapses to an underscore. This must stay
    byte-for-byte in step with the indexer's own version of this rule, since the
    two independently derive the same table name from the same inputs.
    """
    return re.sub(r"[^a-z0-9]", "_", name.lower())


def registry_key_for(project: str, stream: str) -> str:
    """The key identifying one project/stream pair throughout the system."""
    return f"{_slugify_table_name(project)}_{_slugify_table_name(stream)}"


@run_once
def ensure_registry_schema() -> None:
    """Create the managed-project tables.

    Order matters: project_streams references managed_projects, and
    repository_sources references both project_streams and the credentials table
    that storage/credentials.py owns. The cascade rules differ on purpose --
    deleting a project should take its streams and sources with it, but deleting
    a credential should only detach it, leaving the source in place to be
    pointed at a new one.
    """
    with transaction() as cur:
        cur.execute(
            """
            CREATE TABLE IF NOT EXISTS managed_projects (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                name VARCHAR(100) NOT NULL UNIQUE,
                display_name VARCHAR(200) DEFAULT '',
                mode VARCHAR(20) NOT NULL DEFAULT 'managed',
                created_by VARCHAR(255) NOT NULL,
                created_at TIMESTAMP DEFAULT NOW(),
                updated_at TIMESTAMP DEFAULT NOW()
            )
            """
        )
        cur.execute(
            """
            CREATE TABLE IF NOT EXISTS project_streams (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                project_id UUID NOT NULL
                    REFERENCES managed_projects(id) ON DELETE CASCADE,
                stream_name VARCHAR(50) NOT NULL,
                enabled BOOLEAN NOT NULL DEFAULT TRUE,
                last_refresh_at TIMESTAMP,
                last_refresh_status VARCHAR(20),
                last_refresh_log TEXT,
                created_at TIMESTAMP DEFAULT NOW(),
                UNIQUE(project_id, stream_name)
            )
            """
        )
        cur.execute(
            """
            CREATE TABLE IF NOT EXISTS repository_sources (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                stream_id UUID NOT NULL
                    REFERENCES project_streams(id) ON DELETE CASCADE,
                credential_id UUID REFERENCES credentials(id) ON DELETE SET NULL,
                source_mode VARCHAR(20) NOT NULL,
                hosting_project_key VARCHAR(100),
                slug_filter VARCHAR(200),
                repo_url TEXT,
                repo_slug VARCHAR(200),
                branch_strategy VARCHAR(20) NOT NULL DEFAULT 'stream',
                fixed_branch VARCHAR(200),
                created_at TIMESTAMP DEFAULT NOW()
            )
            """
        )
        cur.execute(
            """
            CREATE TABLE IF NOT EXISTS refresh_schedule (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                cron_expression VARCHAR(50) NOT NULL,
                enabled BOOLEAN NOT NULL DEFAULT TRUE,
                last_run_at TIMESTAMP,
                created_by VARCHAR(255) NOT NULL,
                updated_at TIMESTAMP DEFAULT NOW()
            )
            """
        )
        cur.execute(
            """
            CREATE TABLE IF NOT EXISTS refresh_jobs (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                stream_id UUID REFERENCES project_streams(id) ON DELETE SET NULL,
                scope VARCHAR(20) NOT NULL DEFAULT 'global',
                trigger_type VARCHAR(20) NOT NULL,
                triggered_by VARCHAR(255),
                status VARCHAR(20) NOT NULL DEFAULT 'pending',
                phase VARCHAR(30),
                progress_pct INTEGER DEFAULT 0,
                repos_total INTEGER DEFAULT 0,
                repos_cloned INTEGER DEFAULT 0,
                repos_pulled INTEGER DEFAULT 0,
                repos_failed INTEGER DEFAULT 0,
                log TEXT DEFAULT '',
                started_at TIMESTAMP DEFAULT NOW(),
                finished_at TIMESTAMP
            )
            """
        )
        cur.execute(
            """
            CREATE INDEX IF NOT EXISTS refresh_jobs_stream_idx
            ON refresh_jobs(stream_id, started_at DESC)
            """
        )
    logger.info("workspace registry tables ready")


def _indexed_keys(cur) -> set[str]:
    """Registry keys the indexer has actually produced a chunk table for.

    One catalogue query answers this for every project at once. Asking per
    project -- which is what this used to do, on a fresh connection each time --
    made registry construction cost two connections per stream.
    """
    cur.execute(
        "SELECT table_name FROM information_schema.tables WHERE table_name LIKE %s",
        (_CHUNK_TABLE_PREFIX + "%",),
    )
    return {row[0][len(_CHUNK_TABLE_PREFIX):] for row in cur.fetchall()}


def _chunk_count(cur, registry_key: str) -> int:
    """How many chunks are indexed for one stream, or 0 if unreadable."""
    try:
        cur.execute(f'SELECT COUNT(*) FROM "{_CHUNK_TABLE_PREFIX}{registry_key}"')
        return cur.fetchone()[0]
    except Exception:
        return 0


def _entry(project: str, stream: str, path, mode: str, indexed: bool, chunks: int) -> dict:
    return {
        "project": project,
        "stream": stream,
        "codebase_dir": path,
        "indexed": indexed,
        "chunk_count": chunks,
        "mode": mode,
    }


def build_workspace_registry() -> Dict[str, dict]:
    """Assemble every searchable project/stream pair.

    Two sources, in priority order. Configured projects come from the database
    and are authoritative. Directories found under CODEBASE_DIR that no
    configuration mentions are then added as "legacy" entries, so source that
    predates the managed-project tables -- or was placed on the volume by hand --
    stays searchable instead of disappearing.

    A database failure is logged and yields the filesystem view alone rather
    than raising: serving the projects that can be seen beats serving none.
    """
    registry: Dict[str, dict] = {}

    try:
        with read_cursor(dict_rows=True) as cur:
            indexed = _indexed_keys(cur)
            cur.execute(
                """
                SELECT mp.name AS project_name, mp.mode, ps.stream_name
                FROM managed_projects mp
                JOIN project_streams ps ON ps.project_id = mp.id
                WHERE ps.enabled = TRUE
                ORDER BY mp.name, ps.stream_name
                """
            )
            configured = cur.fetchall()

            for row in configured:
                key = registry_key_for(row["project_name"], row["stream_name"])
                is_indexed = key in indexed
                registry[key] = _entry(
                    row["project_name"],
                    row["stream_name"],
                    CODEBASE_DIR / row["project_name"] / row["stream_name"],
                    row["mode"],
                    is_indexed,
                    _chunk_count(cur, key) if is_indexed else 0,
                )
    except Exception:
        logger.exception("could not read configured projects; falling back to disk only")
        indexed = set()

    if not CODEBASE_DIR.exists():
        return registry

    try:
        with read_cursor() as cur:
            for project_dir in sorted(CODEBASE_DIR.iterdir()):
                if not project_dir.is_dir() or project_dir.name in IGNORED_DIRS:
                    continue
                for stream_dir in sorted(project_dir.iterdir()):
                    if not stream_dir.is_dir() or stream_dir.name in IGNORED_DIRS:
                        continue
                    key = registry_key_for(project_dir.name, stream_dir.name)
                    if key in registry:
                        continue
                    is_indexed = key in indexed
                    registry[key] = _entry(
                        project_dir.name,
                        stream_dir.name,
                        stream_dir,
                        "legacy",
                        is_indexed,
                        _chunk_count(cur, key) if is_indexed else 0,
                    )
    except Exception:
        logger.exception("could not scan %s for unmanaged projects", CODEBASE_DIR)

    return registry
