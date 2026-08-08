"""Streams, and the repository sources that fill them.

A stream is a named line of development within a project -- the unit the indexer
builds one searchable corpus for. Its name is not decorative: git_ops.pick_branch
reads it when deciding which branch to check out, which is why the accepted
names are constrained rather than free text.

A source describes where a stream's content comes from. Wildcard sources
discover many repositories from a hosting project and filter by slug; explicit
sources name one repository outright.
"""
from __future__ import annotations

import logging
from typing import Optional

import psycopg2

from ..db import apply_updates, read_cursor, transaction

logger = logging.getLogger("praxevia.harvest.streams")

#: Stream names the branch-selection rules understand. Accepting an arbitrary
#: name would leave pick_branch with no rule to apply and every repository
#: falling through to main/master.
VALID_STREAM_NAMES = {"develop", "release"}

_SOURCE_COLUMNS = """
    id, stream_id, credential_id, source_mode, hosting_project_key,
    slug_filter, repo_url, repo_slug, branch_strategy, fixed_branch, created_at
"""


# ---------------------------------------------------------------------------
# Streams
# ---------------------------------------------------------------------------


def add_stream(project_id: str, stream_name: str) -> Optional[dict]:
    """Add a stream to a project. None means the project already has one.

    Raises ValueError for an unrecognised name -- a caller mistake, distinct
    from the duplicate case which is a legitimate outcome of a repeated click.
    """
    if stream_name not in VALID_STREAM_NAMES:
        raise ValueError(
            f"stream name must be one of: {', '.join(sorted(VALID_STREAM_NAMES))}"
        )

    try:
        with transaction(dict_rows=True) as cur:
            cur.execute(
                """
                INSERT INTO project_streams (project_id, stream_name)
                VALUES (%s, %s)
                RETURNING id, project_id, stream_name, enabled, created_at
                """,
                (project_id, stream_name),
            )
            row = cur.fetchone()
    except psycopg2.errors.UniqueViolation:
        logger.info("project %s already has a %r stream", project_id, stream_name)
        return None
    return dict(row) if row else None


def set_stream_enabled(stream_id: str, enabled: Optional[bool] = None) -> bool:
    """Turn a stream on or off.

    A disabled stream keeps its sources and its indexed content but drops out of
    scheduled refreshes and out of the searchable registry, so this is the
    reversible way to retire one.
    """
    if enabled is None:
        return False
    with transaction() as cur:
        return apply_updates(cur, "project_streams", [("enabled = %s", enabled)], stream_id)


def remove_stream(stream_id: str) -> bool:
    """Delete a stream and, by cascade, its sources."""
    with transaction() as cur:
        cur.execute("DELETE FROM project_streams WHERE id = %s", (stream_id,))
        return cur.rowcount > 0


def fetch_stream(stream_id: str) -> Optional[dict]:
    """One stream, joined to its project so callers get the names together."""
    with read_cursor(dict_rows=True) as cur:
        cur.execute(
            """
            SELECT ps.id, ps.project_id, ps.stream_name, ps.enabled,
                   ps.last_refresh_at, ps.last_refresh_status,
                   mp.name AS project_name, mp.mode AS project_mode
            FROM project_streams ps
            JOIN managed_projects mp ON mp.id = ps.project_id
            WHERE ps.id = %s
            """,
            (stream_id,),
        )
        row = cur.fetchone()
    return dict(row) if row else None


def list_active_streams() -> list[dict]:
    """Every stream a scheduled refresh should visit.

    Both conditions matter: the stream must be enabled, and its project must be
    in managed mode. A manual-mode project is one whose content someone places
    on the volume themselves, so cloning over it would destroy their work.
    """
    with read_cursor(dict_rows=True) as cur:
        cur.execute(
            """
            SELECT ps.id AS stream_id, ps.stream_name, ps.project_id,
                   mp.name AS project_name, mp.mode
            FROM project_streams ps
            JOIN managed_projects mp ON mp.id = ps.project_id
            WHERE ps.enabled = TRUE AND mp.mode = 'managed'
            ORDER BY mp.name, ps.stream_name
            """
        )
        return [dict(row) for row in cur.fetchall()]


# ---------------------------------------------------------------------------
# Repository sources
# ---------------------------------------------------------------------------


def attach_repository_source(
    stream_id: str,
    source_mode: str,
    credential_id: Optional[str] = None,
    hosting_project_key: str = "",
    slug_filter: str = "",
    repo_url: str = "",
    repo_slug: str = "",
    branch_strategy: str = "stream",
    fixed_branch: str = "",
) -> Optional[dict]:
    """Point a stream at a place to fetch source from.

    An empty credential_id is stored as NULL rather than as "": the column is a
    foreign key, and an empty string is not a valid UUID.
    """
    with transaction(dict_rows=True) as cur:
        cur.execute(
            f"""
            INSERT INTO repository_sources
                (stream_id, credential_id, source_mode, hosting_project_key,
                 slug_filter, repo_url, repo_slug, branch_strategy, fixed_branch)
            VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s)
            RETURNING {_SOURCE_COLUMNS}
            """,
            (
                stream_id,
                credential_id or None,
                source_mode,
                hosting_project_key,
                slug_filter,
                repo_url,
                repo_slug,
                branch_strategy,
                fixed_branch,
            ),
        )
        row = cur.fetchone()
    return dict(row) if row else None


def edit_repository_source(
    source_id: str,
    credential_id: Optional[str] = None,
    hosting_project_key: Optional[str] = None,
    slug_filter: Optional[str] = None,
    repo_url: Optional[str] = None,
    repo_slug: Optional[str] = None,
    branch_strategy: Optional[str] = None,
    fixed_branch: Optional[str] = None,
) -> bool:
    """Change a source in place.

    `source_mode` is absent by design: switching a wildcard source to an
    explicit one leaves half its columns meaningless, so that is a detach and a
    fresh attach rather than an edit.
    """
    # (column, what the caller passed, what to store). Passing None means "not
    # supplied" and skips the column entirely; credential_id differs only in
    # that an explicit empty string is stored as NULL, which detaches the
    # credential rather than writing an invalid UUID.
    optional = [
        ("credential_id", credential_id, credential_id or None),
        ("hosting_project_key", hosting_project_key, hosting_project_key),
        ("slug_filter", slug_filter, slug_filter),
        ("repo_url", repo_url, repo_url),
        ("repo_slug", repo_slug, repo_slug),
        ("branch_strategy", branch_strategy, branch_strategy),
        ("fixed_branch", fixed_branch, fixed_branch),
    ]
    changes = [
        (f"{column} = %s", stored) for column, given, stored in optional if given is not None
    ]
    if not changes:
        return False

    with transaction() as cur:
        return apply_updates(cur, "repository_sources", changes, source_id)


def detach_repository_source(source_id: str) -> bool:
    """Remove a source. Already-cloned content on disk is left alone."""
    with transaction() as cur:
        cur.execute("DELETE FROM repository_sources WHERE id = %s", (source_id,))
        return cur.rowcount > 0


def list_repository_sources(stream_id: str) -> list[dict]:
    """Every source feeding a stream, with its credential's connection details.

    The join is what lets the sync worker build a provider without a second
    lookup -- it needs the source type, base URL and TLS setting that live on
    the credential, not just its id.
    """
    with read_cursor(dict_rows=True) as cur:
        cur.execute(
            """
            SELECT rs.*, c.name AS credential_name, c.source_type,
                   c.base_url, c.ssl_verify
            FROM repository_sources rs
            LEFT JOIN credentials c ON c.id = rs.credential_id
            WHERE rs.stream_id = %s
            ORDER BY rs.created_at
            """,
            (stream_id,),
        )
        return [dict(row) for row in cur.fetchall()]
