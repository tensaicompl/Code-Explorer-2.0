"""The request-and-review path a user takes to gain access to a project.

A request is a durable record of someone asking, plus the decision that was
made. Approving one both closes the request and creates the matching grant, and
those two writes share a transaction on purpose -- see `approve_request`.

The tables themselves are created by permissions.ensure_authorization_tables().
"""
from __future__ import annotations

import logging
from typing import Optional

from ..db import read_cursor, transaction

logger = logging.getLogger("praxevia.warden.requests")

_TIMESTAMP_FIELDS = ("reviewed_at", "created_at")

_REQUEST_COLUMNS = """
    id, user_email, project, status, reviewed_by, reviewed_at, created_at
"""


def _as_json_row(row: dict) -> dict:
    """Make one request row safe to hand to the JSON encoder.

    UUIDs and datetimes both survive psycopg2 as native Python objects that the
    encoder refuses, and every one of the listing functions below needs the same
    treatment, so it lives here rather than three times over.
    """
    out = dict(row)
    if out.get("id") is not None:
        out["id"] = str(out["id"])
    for field in _TIMESTAMP_FIELDS:
        if out.get(field):
            out[field] = out[field].isoformat()
    return out


def submit_access_request(user_email: str, project: str) -> dict:
    """Record a pending request, replacing any earlier one for the same pair.

    Deleting first is what makes re-requesting work: the table allows one row
    per user per project, so a previously denied request would otherwise block
    the user from ever asking again.

    Unlike the read paths in this package this one propagates its exception --
    the caller is a POST handler that must not report success for a request it
    failed to store.
    """
    user_email = user_email.lower()
    try:
        with transaction(dict_rows=True) as cur:
            cur.execute(
                "DELETE FROM access_requests WHERE user_email = %s AND project = %s",
                (user_email, project),
            )
            cur.execute(
                """
                INSERT INTO access_requests (user_email, project, status)
                VALUES (%s, %s, 'pending')
                RETURNING id, user_email, project, status, created_at
                """,
                (user_email, project),
            )
            row = dict(cur.fetchone())
    except Exception as exc:
        logger.error("could not record %s's request for %s: %s", user_email, project, exc)
        raise
    return _as_json_row(row)


def list_requests(status: Optional[str] = None) -> list[dict]:
    """Every request, newest first, optionally narrowed to one status."""
    try:
        with read_cursor(dict_rows=True) as cur:
            if status:
                cur.execute(
                    f"SELECT {_REQUEST_COLUMNS} FROM access_requests "
                    "WHERE status = %s ORDER BY created_at DESC",
                    (status,),
                )
            else:
                cur.execute(
                    f"SELECT {_REQUEST_COLUMNS} FROM access_requests "
                    "ORDER BY created_at DESC"
                )
            rows = cur.fetchall()
    except Exception as exc:
        logger.error("could not list access requests: %s", exc)
        return []
    return [_as_json_row(r) for r in rows]


def list_requests_for_user(user_email: str) -> list[dict]:
    """One user's own request history, for showing them where they stand."""
    try:
        with read_cursor(dict_rows=True) as cur:
            cur.execute(
                f"SELECT {_REQUEST_COLUMNS} FROM access_requests "
                "WHERE user_email = %s ORDER BY created_at DESC",
                (user_email.lower(),),
            )
            rows = cur.fetchall()
    except Exception as exc:
        logger.error("could not list requests for %s: %s", user_email, exc)
        return []
    return [_as_json_row(r) for r in rows]


def approve_request(request_id: str, admin_email: str) -> bool:
    """Approve a pending request and grant the access it asked for.

    Both writes share one transaction deliberately. Calling the grant helper in
    permissions.py instead would open a second connection, and a failure between
    the two would leave a request marked approved with no permission behind it
    -- the user would be told they have access and then be refused it.

    The UPDATE is guarded on `status = 'pending'`, so a request that was already
    decided returns False rather than being re-approved.
    """
    admin_email = admin_email.lower()
    try:
        with transaction() as cur:
            cur.execute(
                """
                UPDATE access_requests
                SET status = 'approved', reviewed_by = %s, reviewed_at = NOW()
                WHERE id = %s AND status = 'pending'
                RETURNING user_email, project
                """,
                (admin_email, request_id),
            )
            decided = cur.fetchone()
            if not decided:
                return False
            user_email, project = decided
            cur.execute(
                """
                INSERT INTO project_permissions (user_email, project, granted_by)
                VALUES (%s, %s, %s) ON CONFLICT (user_email, project) DO NOTHING
                """,
                (user_email, project, admin_email),
            )
        return True
    except Exception as exc:
        logger.error("could not approve request %s: %s", request_id, exc)
        return False


def deny_request(request_id: str, admin_email: str) -> bool:
    """Refuse a pending request. No permission row is touched."""
    try:
        with transaction() as cur:
            cur.execute(
                """
                UPDATE access_requests
                SET status = 'denied', reviewed_by = %s, reviewed_at = NOW()
                WHERE id = %s AND status = 'pending'
                """,
                (admin_email.lower(), request_id),
            )
            return cur.rowcount > 0
    except Exception as exc:
        logger.error("could not deny request %s: %s", request_id, exc)
        return False
