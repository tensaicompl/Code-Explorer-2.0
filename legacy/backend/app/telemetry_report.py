"""Turning the event log into the numbers the admin dashboard shows.

One read-only aggregation, kept apart from the write path in telemetry.py
because it is the opposite kind of code: several multi-clause queries run
occasionally, against a table that is otherwise only ever appended to.

A note on the identity markers. Events are attributed through a `user_identity`
string which encodes *how* the caller authenticated as well as who they were:

    __apikey__:<client>                a statically configured integration key
    __userkey__:<token-name>:<email>   a token a user issued for themselves

These prefixes are historical and are parsed rather than joined, because no
table maps them back to anything. They are left as they are on purpose:
rewriting them would orphan the attribution on every event already recorded, and
there is no migration path that could repair it.
"""
from __future__ import annotations

import logging
from datetime import datetime, timedelta

from .db import read_cursor
from .telemetry import RETENTION_DAYS, ensure_events_schema, purge_old_events

logger = logging.getLogger("praxevia.telemetry_report")

#: Prefixes marking a non-human caller, and the report section each feeds.
_STATIC_KEY_MARKER = "__apikey__:"
_USER_KEY_MARKER = "__userkey__:"


def _empty_report() -> dict:
    """The report's shape, so a failed query still returns something renderable.

    The dashboard reads every one of these keys unconditionally. Returning a
    partial dict on error would turn a database hiccup into a broken page.
    """
    return {
        "daily": [],
        "totals": {"messages": 0, "tool_calls": 0, "sessions": 0},
        "by_project": {},
        "by_model": {},
        "by_api_key": {},
        "by_user_api_key": {},
    }


def _collect_daily(cur, cutoff, report: dict) -> None:
    """Per-day message, tool-call and session counts, and their totals."""
    cur.execute(
        """
        SELECT DATE(created_at) AS day,
               COUNT(*) FILTER (WHERE event_type = 'chat_message') AS messages,
               COUNT(*) FILTER (WHERE event_type = 'tool_call') AS tool_calls,
               COUNT(DISTINCT session_id) FILTER (
                   WHERE event_type = 'chat_message' AND session_id != ''
               ) AS sessions
        FROM usage_stats
        WHERE created_at >= %s
        GROUP BY DATE(created_at)
        ORDER BY day
        """,
        (cutoff,),
    )
    for row in cur.fetchall():
        report["daily"].append(
            {
                "day": row["day"].isoformat(),
                "messages": row["messages"],
                "tool_calls": row["tool_calls"],
                "sessions": row["sessions"],
            }
        )
        for field in ("messages", "tool_calls", "sessions"):
            report["totals"][field] += row[field]


def _collect_breakdown(cur, cutoff, column: str) -> dict:
    """Message counts grouped by one column, busiest first.

    The column name is interpolated rather than bound because it is a column
    identifier, not a value -- and it comes only from the fixed call sites
    below, never from a request.
    """
    cur.execute(
        f"""
        SELECT {column} AS bucket, COUNT(*) AS count
        FROM usage_stats
        WHERE created_at >= %s AND event_type = 'chat_message' AND {column} != ''
        GROUP BY {column}
        ORDER BY count DESC
        """,
        (cutoff,),
    )
    return {row["bucket"]: row["count"] for row in cur.fetchall()}


def _collect_by_identity(cur, cutoff, marker: str) -> list:
    """Message and tool-call counts for identities carrying a given marker.

    Both machine-caller sections of the report ask exactly this question of
    exactly this shape, differing only in the prefix, so the query is written
    once and the two callers differ in how they unpack the identity string.
    """
    cur.execute(
        """
        SELECT user_identity,
               COUNT(*) FILTER (WHERE event_type = 'chat_message') AS messages,
               COUNT(*) FILTER (WHERE event_type = 'tool_call') AS tool_calls
        FROM usage_stats
        WHERE created_at >= %s AND user_identity LIKE %s
        GROUP BY user_identity
        ORDER BY (COUNT(*) FILTER (WHERE event_type = 'chat_message')
                + COUNT(*) FILTER (WHERE event_type = 'tool_call')) DESC
        """,
        (cutoff, marker + "%"),
    )
    return cur.fetchall()


def build_usage_report(days: int = RETENTION_DAYS) -> dict:
    """Aggregate recent activity for the admin dashboard.

    Expiring old events first is deliberate: this is the only regularly-invoked
    entry point in the module, so it doubles as the retention sweep rather than
    requiring a separate scheduled job for a single DELETE.

    Query failures are logged and the report is returned as far as it got. A
    dashboard showing some panels populated and others empty is more useful than
    an error page.
    """
    ensure_events_schema()
    purge_old_events(days)

    report = _empty_report()
    cutoff = datetime.utcnow() - timedelta(days=days)

    try:
        with read_cursor(dict_rows=True) as cur:
            _collect_daily(cur, cutoff, report)
            report["by_project"] = _collect_breakdown(cur, cutoff, "project")
            report["by_model"] = _collect_breakdown(cur, cutoff, "model")

            for row in _collect_by_identity(cur, cutoff, _STATIC_KEY_MARKER):
                client = row["user_identity"].replace(_STATIC_KEY_MARKER, "", 1)
                report["by_api_key"][client] = {
                    "messages": row["messages"],
                    "tool_calls": row["tool_calls"],
                }

            for row in _collect_by_identity(cur, cutoff, _USER_KEY_MARKER):
                # Encoded as "<token-name>:<owner-email>"; the owner may be
                # absent on events recorded before that was included.
                remainder = row["user_identity"].replace(_USER_KEY_MARKER, "", 1)
                separator = remainder.find(":")
                token_name = remainder[:separator] if separator >= 0 else remainder
                owner = remainder[separator + 1:] if separator >= 0 else ""
                report["by_user_api_key"][token_name] = {
                    "messages": row["messages"],
                    "tool_calls": row["tool_calls"],
                    "owner": owner,
                }
    except Exception as exc:
        logger.warning("usage report is incomplete: %s", exc)

    return report
