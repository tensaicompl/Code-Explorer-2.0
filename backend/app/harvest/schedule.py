"""When refreshes run, and the record of each one that did.

Two related concerns. The *schedule* is a single row saying how often a global
refresh should happen; cadence.py reads it and arms a timer from it. A *job* is
one refresh attempt, carrying live progress so the admin screen can show a bar
and a log while it runs.

Job rows are written to constantly during a pass -- once per repository -- which
is why `record_refresh_progress` accepts every field as optional and updates
only what changed. The log column is appended to rather than replaced, so a
progress write never truncates what came before.
"""
from __future__ import annotations

import logging
from typing import Optional

from ..db import apply_updates, read_cursor, transaction

logger = logging.getLogger("praxevia.harvest.schedule")

_ACTIVE_STATUSES = ("pending", "running")


# ---------------------------------------------------------------------------
# Schedule
# ---------------------------------------------------------------------------


def get_refresh_schedule() -> Optional[dict]:
    """The current schedule, or None if one was never configured.

    The table can technically hold several rows; the most recently updated one
    wins. Keeping the older rows costs nothing and leaves a trail of what the
    schedule used to be.
    """
    with read_cursor(dict_rows=True) as cur:
        cur.execute("SELECT * FROM refresh_schedule ORDER BY updated_at DESC LIMIT 1")
        row = cur.fetchone()
    return dict(row) if row else None


def set_refresh_schedule(cron_expression: str, enabled: bool, created_by: str) -> dict:
    """Store the schedule, updating the existing row if there is one.

    The cron expression is not validated here. cadence.py parses it when arming
    the timer and reports what it makes of it, which keeps one parser in the
    system rather than two that could disagree.
    """
    existing = get_refresh_schedule()
    with transaction(dict_rows=True) as cur:
        if existing:
            cur.execute(
                """
                UPDATE refresh_schedule
                SET cron_expression = %s, enabled = %s, updated_at = NOW()
                WHERE id = %s
                RETURNING *
                """,
                (cron_expression, enabled, existing["id"]),
            )
        else:
            cur.execute(
                """
                INSERT INTO refresh_schedule (cron_expression, enabled, created_by)
                VALUES (%s, %s, %s) RETURNING *
                """,
                (cron_expression, enabled, created_by),
            )
        row = cur.fetchone()
    return dict(row) if row else {}


# ---------------------------------------------------------------------------
# Jobs
# ---------------------------------------------------------------------------


def refresh_in_progress(scope: Optional[str] = None, stream_id: Optional[str] = None) -> bool:
    """Whether a refresh is already pending or running.

    Advisory only. Two callers checking simultaneously can both see False and
    both proceed; the consequence is duplicated cloning, which is wasteful but
    harmless, since fetching the same repository twice converges on the same
    checkout.
    """
    with read_cursor() as cur:
        if stream_id:
            cur.execute(
                "SELECT EXISTS (SELECT 1 FROM refresh_jobs "
                "WHERE stream_id = %s AND status IN %s)",
                (stream_id, _ACTIVE_STATUSES),
            )
        elif scope:
            cur.execute(
                "SELECT EXISTS (SELECT 1 FROM refresh_jobs "
                "WHERE scope = %s AND status IN %s)",
                (scope, _ACTIVE_STATUSES),
            )
        else:
            cur.execute(
                "SELECT EXISTS (SELECT 1 FROM refresh_jobs WHERE status IN %s)",
                (_ACTIVE_STATUSES,),
            )
        return cur.fetchone()[0]


def start_refresh_job(
    stream_id: Optional[str],
    scope: str,
    trigger_type: str,
    triggered_by: str = "",
) -> str:
    """Open a job row and return its id. The caller drives it from there."""
    with transaction() as cur:
        cur.execute(
            """
            INSERT INTO refresh_jobs (stream_id, scope, trigger_type, triggered_by, status)
            VALUES (%s, %s, %s, %s, 'pending')
            RETURNING id
            """,
            (stream_id, scope, trigger_type, triggered_by),
        )
        return str(cur.fetchone()[0])


def record_refresh_progress(
    job_id: str,
    status: Optional[str] = None,
    phase: Optional[str] = None,
    progress_pct: Optional[int] = None,
    repos_total: Optional[int] = None,
    repos_cloned: Optional[int] = None,
    repos_pulled: Optional[int] = None,
    repos_failed: Optional[int] = None,
    log_append: Optional[str] = None,
    finished: bool = False,
) -> None:
    """Update whichever parts of a running job have moved.

    Called once per repository during a large refresh, so it deliberately writes
    the smallest possible UPDATE rather than rewriting the whole row.
    """
    optional = [
        ("status", status),
        ("phase", phase),
        ("progress_pct", progress_pct),
        ("repos_total", repos_total),
        ("repos_cloned", repos_cloned),
        ("repos_pulled", repos_pulled),
        ("repos_failed", repos_failed),
    ]
    changes = [(f"{column} = %s", value) for column, value in optional if value is not None]
    if log_append is not None:
        # Concatenate in SQL: reading the log back to append in Python would
        # lose whatever a concurrent write added in between.
        changes.append(("log = log || %s", log_append))

    raw = ["finished_at = NOW()"] if finished else []
    if not changes and not raw:
        return

    with transaction() as cur:
        apply_updates(cur, "refresh_jobs", changes, job_id, raw=raw)


def fetch_refresh_job(job_id: str) -> Optional[dict]:
    """One job, including its accumulated log."""
    with read_cursor(dict_rows=True) as cur:
        cur.execute("SELECT * FROM refresh_jobs WHERE id = %s", (job_id,))
        row = cur.fetchone()
    return dict(row) if row else None


def list_refresh_job_history(stream_id: Optional[str] = None, limit: int = 50) -> list[dict]:
    """Recent jobs, newest first, for the whole system or for one stream."""
    with read_cursor(dict_rows=True) as cur:
        if stream_id:
            cur.execute(
                "SELECT * FROM refresh_jobs WHERE stream_id = %s "
                "ORDER BY started_at DESC LIMIT %s",
                (stream_id, limit),
            )
        else:
            cur.execute(
                "SELECT * FROM refresh_jobs ORDER BY started_at DESC LIMIT %s",
                (limit,),
            )
        return [dict(row) for row in cur.fetchall()]


def record_stream_refresh_result(
    stream_id: str, status: str, log: Optional[str] = None
) -> None:
    """Stamp a stream with how its last refresh went.

    Denormalised onto the stream so the admin list can show each stream's state
    without joining to the job history.
    """
    with transaction() as cur:
        cur.execute(
            """
            UPDATE project_streams
            SET last_refresh_at = NOW(), last_refresh_status = %s, last_refresh_log = %s
            WHERE id = %s
            """,
            (status, log, stream_id),
        )
