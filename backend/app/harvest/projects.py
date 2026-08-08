"""Managed projects: the top level of the workspace.

A project is a name plus a mode. "managed" means the refresh machinery clones
and tracks it; anything else means it is present but left alone. Streams hang
off a project, and repository sources hang off streams -- see streams.py.
"""
from __future__ import annotations

import logging
from typing import Optional

import psycopg2

from ..db import apply_updates, read_cursor, transaction

logger = logging.getLogger("praxevia.harvest.projects")

_PROJECT_COLUMNS = "id, name, display_name, mode, created_by, created_at, updated_at"


def add_project(
    name: str,
    display_name: str = "",
    mode: str = "managed",
    created_by: str = "system",
) -> Optional[dict]:
    """Register a project. None means the name is already taken."""
    try:
        with transaction(dict_rows=True) as cur:
            cur.execute(
                f"""
                INSERT INTO managed_projects (name, display_name, mode, created_by)
                VALUES (%s, %s, %s, %s)
                RETURNING {_PROJECT_COLUMNS}
                """,
                (name, display_name, mode, created_by),
            )
            row = cur.fetchone()
    except psycopg2.errors.UniqueViolation:
        logger.info("project name %r is already registered", name)
        return None
    return dict(row) if row else None


def edit_project(
    project_id: str,
    display_name: Optional[str] = None,
    mode: Optional[str] = None,
) -> bool:
    """Change a project's label or mode. Its name is its identity and is fixed.

    Renaming would change the registry key, and with it the names of the tables
    the indexer has already written -- effectively orphaning the index.
    """
    changes = []
    if display_name is not None:
        changes.append(("display_name = %s", display_name))
    if mode is not None:
        changes.append(("mode = %s", mode))
    if not changes:
        return False

    with transaction() as cur:
        return apply_updates(
            cur, "managed_projects", changes, project_id, raw=["updated_at = NOW()"]
        )


def remove_project(project_id: str) -> bool:
    """Delete a project, and by cascade its streams and their sources.

    Indexed content is not touched. The chunk tables outlive the project record
    and will simply stop being listed, which is recoverable; dropping them here
    would not be.
    """
    with transaction() as cur:
        cur.execute("DELETE FROM managed_projects WHERE id = %s", (project_id,))
        return cur.rowcount > 0


def fetch_project(project_id: str) -> Optional[dict]:
    """One project record."""
    with read_cursor(dict_rows=True) as cur:
        cur.execute(
            f"SELECT {_PROJECT_COLUMNS} FROM managed_projects WHERE id = %s",
            (project_id,),
        )
        row = cur.fetchone()
    return dict(row) if row else None


def list_projects_with_detail() -> list[dict]:
    """Every project with its streams, and every stream with its sources.

    Built for the admin screen, which renders the whole tree at once. The three
    levels are fetched with one query per level rather than one per parent row,
    then stitched together in memory -- with a handful of projects each holding
    a couple of streams, the nested-loop version issued dozens of round trips to
    assemble a single page.
    """
    with read_cursor(dict_rows=True) as cur:
        cur.execute(f"SELECT {_PROJECT_COLUMNS} FROM managed_projects ORDER BY name")
        projects = [dict(row) for row in cur.fetchall()]
        if not projects:
            return []

        cur.execute(
            """
            SELECT id, project_id, stream_name, enabled, last_refresh_at,
                   last_refresh_status, created_at
            FROM project_streams
            WHERE project_id = ANY(%s)
            ORDER BY stream_name
            """,
            ([project["id"] for project in projects],),
        )
        streams = [dict(row) for row in cur.fetchall()]

        sources_by_stream: dict = {}
        if streams:
            cur.execute(
                """
                SELECT rs.id, rs.stream_id, rs.source_mode, rs.credential_id,
                       rs.hosting_project_key, rs.slug_filter, rs.repo_url,
                       rs.repo_slug, rs.branch_strategy, rs.fixed_branch,
                       c.name AS credential_name
                FROM repository_sources rs
                LEFT JOIN credentials c ON c.id = rs.credential_id
                WHERE rs.stream_id = ANY(%s)
                ORDER BY rs.created_at
                """,
                ([stream["id"] for stream in streams],),
            )
            for row in cur.fetchall():
                source = dict(row)
                sources_by_stream.setdefault(source.pop("stream_id"), []).append(source)

    streams_by_project: dict = {}
    for stream in streams:
        stream["sources"] = sources_by_stream.get(stream["id"], [])
        streams_by_project.setdefault(stream.pop("project_id"), []).append(stream)

    for project in projects:
        project["streams"] = streams_by_project.get(project["id"], [])

    return projects
