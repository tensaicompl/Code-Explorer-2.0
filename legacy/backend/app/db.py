"""Shared Postgres access: connections, cursors, and once-per-process DDL.

Every storage-backed module in this package used to open its own connection with
a private helper and close it on the happy path only -- an exception between
`connect()` and `close()` leaked the connection for the life of the worker.
Routing all of it through the context managers here means the connection is
returned whether the block succeeds, raises, or returns early.

`transaction()` is the one to reach for when writing: it commits only if the
body completes, so a failed multi-statement write rolls back as a unit instead
of leaving half of it applied.
"""
from __future__ import annotations

import functools
import logging
from contextlib import contextmanager

import psycopg2
import psycopg2.extras

from .config import DATABASE_URL

logger = logging.getLogger("praxevia.db")


@contextmanager
def get_connection():
    """Yield a connection and guarantee it is closed again."""
    conn = psycopg2.connect(DATABASE_URL)
    try:
        yield conn
    finally:
        conn.close()


@contextmanager
def read_cursor(dict_rows: bool = False):
    """Cursor for statements that only read. Nothing is committed.

    `dict_rows=True` selects RealDictCursor, so rows arrive as dicts rather than
    positional tuples -- worth it wherever the caller immediately builds a JSON
    response out of them.
    """
    factory = psycopg2.extras.RealDictCursor if dict_rows else None
    with get_connection() as conn:
        with conn.cursor(cursor_factory=factory) as cur:
            yield cur


@contextmanager
def transaction(dict_rows: bool = False):
    """Cursor for statements that write, committed once the block completes.

    Leaving the block by raising skips the commit, so the connection closes with
    the work rolled back. That is the intended behaviour for every caller here:
    a partially-applied schema migration or a permission grant recorded without
    its matching request update would both be worse than no change at all.
    """
    factory = psycopg2.extras.RealDictCursor if dict_rows else None
    with get_connection() as conn:
        with conn.cursor(cursor_factory=factory) as cur:
            yield cur
        conn.commit()


def apply_updates(cur, table: str, assignments, row_id, raw=()) -> bool:
    """Run a partial UPDATE built from only the fields a caller supplied.

    Several tables here are edited through "everything is Optional, None means
    leave alone" functions, and each was assembling its own SET clause from a
    pair of parallel lists. Doing it once removes four near-identical copies and
    with them the chance of the lists drifting out of step, which produces
    parameters bound to the wrong columns rather than an error.

    `assignments` is a sequence of (fragment, value) where the fragment carries
    exactly one placeholder -- so an accumulating column works as
    ("log = log || %s", text) alongside a plain ("status = %s", value). `raw`
    holds fragments with no parameter at all, such as "updated_at = NOW()".

    Returns whether any row changed. False with no assignments means the caller
    was handed nothing to do, which is not an error.
    """
    fragments = [fragment for fragment, _ in assignments]
    values = [value for _, value in assignments]
    if not fragments and not raw:
        return False

    fragments.extend(raw)
    cur.execute(
        f"UPDATE {table} SET {', '.join(fragments)} WHERE id = %s",
        [*values, row_id],
    )
    return cur.rowcount > 0


def run_once(fn):
    """Run a schema-setup routine at most once per process.

    Replaces the module-level `_TABLE_INIT_DONE` flag that each storage module
    used to carry its own copy of. Failure is deliberately not sticky: if the
    routine raises -- most likely because the database is not up yet -- the
    error is logged and the call is retried on the next invocation, matching how
    the hand-rolled guards behaved. Only a clean run marks the work done.
    """

    state = {"done": False}

    @functools.wraps(fn)
    def guarded(*args, **kwargs):
        if state["done"]:
            return None
        try:
            result = fn(*args, **kwargs)
        except Exception as exc:
            logger.error("schema setup %s did not complete (%s); will retry", fn.__name__, exc)
            return None
        state["done"] = True
        return result

    # Lets a test re-run a setup routine against a fresh database.
    guarded.reset = lambda: state.update(done=False)
    return guarded
