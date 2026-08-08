"""Which users may see which projects.

Also owns the DDL for the whole warden package. All three authorization tables
are created together by `ensure_authorization_tables()` because they are useless
apart: a pending request in `access_requests` is meaningless without a
`project_permissions` row to promote it into, and `admin_users` decides who is
allowed to do the promoting. One entry point, called once at startup, keeps that
ordering explicit instead of leaving it to whichever module happened to be
imported first.

Reads deliberately fail soft. If the database is unreachable, a permission check
returns False and a listing returns empty -- the caller then denies access. The
opposite choice, treating an outage as permission granted, would turn a database
blip into an authorization bypass.
"""
from __future__ import annotations

import logging
from typing import Optional

from ..config import ADMIN_EMAILS
from ..db import read_cursor, run_once, transaction

logger = logging.getLogger("praxevia.warden.permissions")


# ---------------------------------------------------------------------------
# Schema
# ---------------------------------------------------------------------------


def _create_permissions_table(cur) -> None:
    cur.execute(
        """
        CREATE TABLE IF NOT EXISTS project_permissions (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            user_email VARCHAR(255) NOT NULL,
            project VARCHAR(100) NOT NULL,
            granted_by VARCHAR(255) NOT NULL,
            granted_at TIMESTAMP DEFAULT NOW(),
            UNIQUE(user_email, project)
        )
        """
    )
    # Indexes were originally created under idx_* names. They are dropped by the
    # old name first so a deployment that predates the rename ends up with one
    # index per column rather than two identical ones.
    cur.execute("DROP INDEX IF EXISTS idx_project_permissions_user")
    cur.execute(
        """
        CREATE INDEX IF NOT EXISTS project_permissions_user_idx
        ON project_permissions(user_email)
        """
    )
    cur.execute("DROP INDEX IF EXISTS idx_project_permissions_project")
    cur.execute(
        """
        CREATE INDEX IF NOT EXISTS project_permissions_project_idx
        ON project_permissions(project)
        """
    )


def _create_requests_table(cur) -> None:
    cur.execute(
        """
        CREATE TABLE IF NOT EXISTS access_requests (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            user_email VARCHAR(255) NOT NULL,
            project VARCHAR(100) NOT NULL,
            status VARCHAR(20) NOT NULL DEFAULT 'pending',
            reviewed_by VARCHAR(255),
            reviewed_at TIMESTAMP,
            created_at TIMESTAMP DEFAULT NOW(),
            UNIQUE(user_email, project)
        )
        """
    )
    cur.execute("DROP INDEX IF EXISTS idx_access_requests_status")
    cur.execute(
        """
        CREATE INDEX IF NOT EXISTS access_requests_status_idx
        ON access_requests(status, created_at DESC)
        """
    )
    cur.execute("DROP INDEX IF EXISTS idx_access_requests_user")
    cur.execute(
        """
        CREATE INDEX IF NOT EXISTS access_requests_user_idx
        ON access_requests(user_email)
        """
    )


def _collapse_legacy_request_constraint(cur) -> None:
    """Move access_requests from a three-column uniqueness rule to (user, project).

    Earlier deployments keyed uniqueness on three columns, which let the same
    user accumulate several rows for one project as its status changed. The new
    rule is one row per user per project, so duplicates have to go before the
    constraint can be added -- the delete below keeps the newest row of each
    group and drops the rest.

    Detection is by shape rather than by name: the old constraint was
    auto-named, so its identifier differs between installations.
    """
    cur.execute(
        """
        SELECT 1 FROM pg_constraint
        WHERE conrelid = 'access_requests'::regclass
          AND contype = 'u'
          AND array_length(conkey, 1) = 3
        """
    )
    if not cur.fetchone():
        return

    cur.execute(
        """
        DELETE FROM access_requests a
        USING access_requests b
        WHERE a.user_email = b.user_email
          AND a.project = b.project
          AND a.id != b.id
          AND (a.created_at < b.created_at
               OR (a.created_at = b.created_at AND a.id < b.id))
        """
    )
    cur.execute(
        """
        SELECT conname FROM pg_constraint
        WHERE conrelid = 'access_requests'::regclass
          AND contype = 'u'
          AND array_length(conkey, 1) = 3
        """
    )
    stale = cur.fetchone()
    if stale:
        cur.execute(f"ALTER TABLE access_requests DROP CONSTRAINT {stale[0]}")
    cur.execute(
        """
        ALTER TABLE access_requests
        ADD CONSTRAINT uq_access_requests_user_project UNIQUE (user_email, project)
        """
    )
    logger.info("access_requests now keyed on (user_email, project)")


def _create_admins_table(cur) -> None:
    cur.execute(
        """
        CREATE TABLE IF NOT EXISTS admin_users (
            user_email VARCHAR(255) PRIMARY KEY,
            added_by VARCHAR(255) NOT NULL DEFAULT 'system',
            created_at TIMESTAMP DEFAULT NOW()
        )
        """
    )


def _seed_admins_from_env(cur) -> None:
    """Populate the admin roster from ADMIN_EMAILS, but only while it is empty.

    This is a bootstrap for a brand-new deployment, not a sync. Once anyone has
    been made an admin through the API, the env var stops being consulted --
    otherwise removing an admin in the UI would silently undo itself on the next
    restart.
    """
    cur.execute("SELECT COUNT(*) FROM admin_users")
    if cur.fetchone()[0] or not ADMIN_EMAILS.strip():
        return

    seeded = 0
    for raw in ADMIN_EMAILS.split(","):
        email = raw.strip().lower()
        if not email:
            continue
        cur.execute(
            "INSERT INTO admin_users (user_email, added_by) VALUES (%s, 'system') "
            "ON CONFLICT DO NOTHING",
            (email,),
        )
        seeded += cur.rowcount
    if seeded:
        logger.info("bootstrapped %d admin account(s) from ADMIN_EMAILS", seeded)


@run_once
def ensure_authorization_tables() -> None:
    """Create every table the warden package depends on, once per process."""
    with transaction() as cur:
        _create_permissions_table(cur)
        _create_requests_table(cur)
        _collapse_legacy_request_constraint(cur)
        _create_admins_table(cur)
        _seed_admins_from_env(cur)
    logger.info("authorization tables ready")


# ---------------------------------------------------------------------------
# Checks
# ---------------------------------------------------------------------------


def user_can_access_project(user_email: str, project: str) -> bool:
    """True only when an explicit grant exists for this exact pair."""
    try:
        with read_cursor() as cur:
            cur.execute(
                "SELECT 1 FROM project_permissions WHERE user_email = %s AND project = %s",
                (user_email.lower(), project),
            )
            return cur.fetchone() is not None
    except Exception as exc:
        logger.error("access check failed for %s on %s: %s", user_email, project, exc)
        return False


def list_permitted_projects(user_email: str) -> list[str]:
    """Project names this user has been granted, for building their menu."""
    try:
        with read_cursor() as cur:
            cur.execute(
                "SELECT DISTINCT project FROM project_permissions "
                "WHERE user_email = %s ORDER BY project",
                (user_email.lower(),),
            )
            return [row[0] for row in cur.fetchall()]
    except Exception as exc:
        logger.error("could not list projects for %s: %s", user_email, exc)
        return []


# ---------------------------------------------------------------------------
# Grants
# ---------------------------------------------------------------------------


def grant_project_permission(user_email: str, project: str, granted_by: str) -> bool:
    """Grant access. False means the grant already existed, not that it failed."""
    try:
        with transaction() as cur:
            cur.execute(
                """
                INSERT INTO project_permissions (user_email, project, granted_by)
                VALUES (%s, %s, %s) ON CONFLICT (user_email, project) DO NOTHING
                """,
                (user_email.lower(), project, granted_by.lower()),
            )
            return cur.rowcount > 0
    except Exception as exc:
        logger.error("could not grant %s access to %s: %s", user_email, project, exc)
        return False


def revoke_project_permission(user_email: str, project: str) -> bool:
    """Withdraw access. False means there was nothing to withdraw."""
    try:
        with transaction() as cur:
            cur.execute(
                "DELETE FROM project_permissions WHERE user_email = %s AND project = %s",
                (user_email.lower(), project),
            )
            return cur.rowcount > 0
    except Exception as exc:
        logger.error("could not revoke %s access to %s: %s", user_email, project, exc)
        return False


def list_permissions(project: Optional[str] = None) -> list[dict]:
    """Every grant on record, narrowed to one project when asked.

    Timestamps are converted to ISO strings here so the admin routes can hand
    the rows straight to the JSON encoder.
    """
    try:
        with read_cursor(dict_rows=True) as cur:
            if project:
                cur.execute(
                    """
                    SELECT user_email, project, granted_by, granted_at
                    FROM project_permissions WHERE project = %s
                    ORDER BY user_email
                    """,
                    (project,),
                )
            else:
                cur.execute(
                    """
                    SELECT user_email, project, granted_by, granted_at
                    FROM project_permissions ORDER BY project, user_email
                    """
                )
            rows = [dict(r) for r in cur.fetchall()]
    except Exception as exc:
        logger.error("could not list permissions: %s", exc)
        return []

    for row in rows:
        if row.get("granted_at"):
            row["granted_at"] = row["granted_at"].isoformat()
    return rows
