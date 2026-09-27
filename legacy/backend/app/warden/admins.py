"""The platform admin roster.

Membership here is what lets a caller reach the /api/admin routes at all. It is
a flat list with no per-project scoping: an admin is an admin everywhere. That
is a deliberate simplification, not an oversight -- scoped administration would
need its own table and a policy for who may delegate what.

Note that the privileged non-human identities (the shared password session and
static API keys) never appear in this table. They are recognised upstream in the
request-guard layer and bypass this check entirely.

The table is created by permissions.ensure_authorization_tables().
"""
from __future__ import annotations

import logging

from ..db import read_cursor, transaction

logger = logging.getLogger("praxevia.warden.admins")


def is_platform_admin(user_email: str) -> bool:
    """True when this address is on the roster.

    Fails closed: an unreachable database denies the check rather than granting
    it, so an outage cannot open the admin surface.
    """
    try:
        with read_cursor() as cur:
            cur.execute(
                "SELECT 1 FROM admin_users WHERE user_email = %s",
                (user_email.lower(),),
            )
            return cur.fetchone() is not None
    except Exception as exc:
        logger.error("admin check failed for %s: %s", user_email, exc)
        return False


def grant_admin(user_email: str, added_by: str = "system") -> bool:
    """Add someone to the roster. False means they were already on it."""
    try:
        with transaction() as cur:
            cur.execute(
                "INSERT INTO admin_users (user_email, added_by) VALUES (%s, %s) "
                "ON CONFLICT DO NOTHING",
                (user_email.lower(), added_by.lower()),
            )
            return cur.rowcount > 0
    except Exception as exc:
        logger.error("could not add admin %s: %s", user_email, exc)
        return False


def revoke_admin(user_email: str) -> bool:
    """Remove someone from the roster. False means they were not on it.

    There is no guard against removing the last admin. Recovery in that case is
    to set ADMIN_EMAILS and restart against an empty table, or to insert a row
    directly.
    """
    try:
        with transaction() as cur:
            cur.execute(
                "DELETE FROM admin_users WHERE user_email = %s",
                (user_email.lower(),),
            )
            return cur.rowcount > 0
    except Exception as exc:
        logger.error("could not remove admin %s: %s", user_email, exc)
        return False


def list_platform_admins() -> list[dict]:
    """The full roster, with timestamps pre-formatted for JSON."""
    try:
        with read_cursor(dict_rows=True) as cur:
            cur.execute(
                "SELECT user_email, added_by, created_at FROM admin_users "
                "ORDER BY user_email"
            )
            rows = [dict(r) for r in cur.fetchall()]
    except Exception as exc:
        logger.error("could not list admins: %s", exc)
        return []

    for row in rows:
        if row.get("created_at"):
            row["created_at"] = row["created_at"].isoformat()
    return rows
