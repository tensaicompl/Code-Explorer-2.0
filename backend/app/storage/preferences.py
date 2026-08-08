"""Per-user UI state.

One JSONB blob per user, stored and returned verbatim. Nothing here inspects or
validates the contents: the shape is agreed between the dashboard and
schemas.PreferencesPayload, and keeping this layer incurious about it means a
new setting needs no change at all on the way through.

`store_preferences` REPLACES the stored blob rather than merging into it. That
is deliberate -- merging here would make it impossible to ever clear a setting.
The route handler reads the current values, overlays the incoming partial
object, and passes the result down, so the endpoint behaves as a merge even
though this function does not.
"""
from __future__ import annotations

import logging

import psycopg2.extras

from ..db import read_cursor, run_once, transaction

logger = logging.getLogger("praxevia.storage.preferences")


@run_once
def ensure_preferences_schema() -> None:
    """Create the preferences table.

    `user_email` is the primary key, so the upsert in store_preferences has a
    conflict target and each user can only ever have one row.
    """
    with transaction() as cur:
        cur.execute(
            """
            CREATE TABLE IF NOT EXISTS user_preferences (
                user_email VARCHAR(255) PRIMARY KEY,
                preferences JSONB NOT NULL DEFAULT '{}',
                updated_at TIMESTAMP DEFAULT NOW()
            )
            """
        )
    logger.info("preferences table ready")


def load_preferences(user_email: str) -> dict:
    """This user's stored settings, or an empty dict if they have none yet.

    An empty dict is the correct answer for a new user, not an error -- the
    dashboard falls back to its own defaults for anything absent.
    """
    ensure_preferences_schema()
    with read_cursor() as cur:
        cur.execute(
            "SELECT preferences FROM user_preferences WHERE user_email = %s",
            (user_email,),
        )
        row = cur.fetchone()
    return row[0] if row else {}


def store_preferences(user_email: str, preferences: dict) -> dict:
    """Write the settings blob, creating the row if this is the user's first.

    Returns what the database now holds rather than what was passed in, so the
    caller echoes back the stored truth.
    """
    ensure_preferences_schema()
    with transaction() as cur:
        cur.execute(
            """
            INSERT INTO user_preferences (user_email, preferences, updated_at)
            VALUES (%s, %s, NOW())
            ON CONFLICT (user_email)
            DO UPDATE SET preferences = EXCLUDED.preferences, updated_at = NOW()
            RETURNING preferences
            """,
            (user_email, psycopg2.extras.Json(preferences)),
        )
        row = cur.fetchone()
    return row[0] if row else preferences
