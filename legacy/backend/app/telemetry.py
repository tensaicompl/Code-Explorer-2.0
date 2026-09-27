"""Recording what the service is used for.

An append-only event log plus a small table remembering which projects a user
has actually opened. Both are write-heavy and read rarely; the reading side
lives in telemetry_report.py.

Nothing here is allowed to break a request. Every write is wrapped and its
failure logged rather than raised, because the caller is always in the middle of
doing something the user asked for, and losing a usage statistic is not a reason
to fail their question.

Events expire. Thirty days is enough to answer "is this being used, by whom, for
what" without accumulating an indefinite record of individual activity.
"""
from __future__ import annotations

import logging
from datetime import datetime, timedelta

from .db import read_cursor, run_once, transaction

logger = logging.getLogger("praxevia.telemetry")

#: How long an event is kept before it is deleted.
RETENTION_DAYS = 30


@run_once
def ensure_events_schema() -> None:
    """Create the event table and its indexes.

    `user_identity` is added by ALTER rather than declared in the CREATE because
    it arrived after the table did, and an existing deployment needs to gain the
    column without losing its history.
    """
    with transaction() as cur:
        cur.execute(
            """
            CREATE TABLE IF NOT EXISTS usage_stats (
                id SERIAL PRIMARY KEY,
                event_type VARCHAR(50) NOT NULL,
                project VARCHAR(100) DEFAULT '',
                stream VARCHAR(100) DEFAULT '',
                model VARCHAR(100) DEFAULT '',
                session_id VARCHAR(100) DEFAULT '',
                created_at TIMESTAMP DEFAULT NOW()
            )
            """
        )
        cur.execute("DROP INDEX IF EXISTS idx_usage_stats_created")
        cur.execute(
            "CREATE INDEX IF NOT EXISTS usage_stats_created_idx ON usage_stats(created_at)"
        )
        cur.execute(
            "ALTER TABLE usage_stats ADD COLUMN IF NOT EXISTS user_identity VARCHAR(255) DEFAULT ''"
        )
        cur.execute("DROP INDEX IF EXISTS idx_usage_stats_user_identity")
        cur.execute(
            "CREATE INDEX IF NOT EXISTS usage_stats_user_identity_idx "
            "ON usage_stats(user_identity)"
        )
    logger.info("usage event table ready")


def record_event(
    event_type: str,
    project: str = "",
    stream: str = "",
    model: str = "",
    session_id: str = "",
    user_identity: str = "",
) -> None:
    """Append one usage event.

    Swallows its own failures on purpose -- see the module docstring. The name is
    deliberately unchanged from what callers already use, so moving this module
    stayed a one-line import edit for the graph package rather than a rename
    reaching into code that is otherwise left alone.
    """
    try:
        ensure_events_schema()
        with transaction() as cur:
            cur.execute(
                "INSERT INTO usage_stats "
                "(event_type, project, stream, model, session_id, user_identity) "
                "VALUES (%s, %s, %s, %s, %s, %s)",
                (event_type, project, stream, model, session_id, user_identity),
            )
    except Exception as exc:
        logger.warning("usage event %r not recorded: %s", event_type, exc)


def purge_old_events(days: int = RETENTION_DAYS) -> int:
    """Delete events past the retention window. Returns how many went."""
    cutoff = datetime.utcnow() - timedelta(days=days)
    try:
        with transaction() as cur:
            cur.execute("DELETE FROM usage_stats WHERE created_at < %s", (cutoff,))
            removed = cur.rowcount
    except Exception as exc:
        logger.warning("could not expire old usage events: %s", exc)
        return 0

    if removed:
        logger.info("expired %d usage event(s) older than %d days", removed, days)
    return removed


@run_once
def ensure_user_projects_schema() -> None:
    """Create the user/project association table.

    The pair is the primary key, which is what makes track_user_project safe to
    call on every request without accumulating duplicates.
    """
    with transaction() as cur:
        cur.execute(
            """
            CREATE TABLE IF NOT EXISTS user_projects (
                user_email VARCHAR(255) NOT NULL,
                project VARCHAR(100) NOT NULL,
                PRIMARY KEY (user_email, project)
            )
            """
        )
    logger.info("user/project association table ready")


def track_user_project(user_email: str, project: str) -> None:
    """Note that this user has worked on this project.

    Called on every relevant request; repeats are discarded by the conflict
    clause rather than checked for first.
    """
    try:
        ensure_user_projects_schema()
        with transaction() as cur:
            cur.execute(
                "INSERT INTO user_projects (user_email, project) "
                "VALUES (%s, %s) ON CONFLICT DO NOTHING",
                (user_email, project),
            )
    except Exception as exc:
        logger.warning("could not record %s working on %s: %s", user_email, project, exc)


def list_user_projects() -> list[dict]:
    """Every user/project pairing on record, for the admin overview."""
    ensure_user_projects_schema()
    try:
        with read_cursor() as cur:
            cur.execute(
                "SELECT user_email, project FROM user_projects "
                "ORDER BY user_email, project"
            )
            return [{"user_email": row[0], "project": row[1]} for row in cur.fetchall()]
    except Exception as exc:
        logger.warning("could not read user/project pairings: %s", exc)
        return []
