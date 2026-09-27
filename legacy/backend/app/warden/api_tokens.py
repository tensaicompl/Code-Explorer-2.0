"""Long-lived API tokens that identify a user without an interactive sign-in.

These are what the MCP surface and any scripted client authenticate with. Only
the SHA-256 digest of a token is stored, so the plaintext exists exactly once,
in the response that created it -- there is no recovery path, only reissue.

The stored `key_prefix` is the first few visible characters, kept purely so the
token list can show which token is which. It is not used to find a token.

Named api_tokens rather than api_keys because "key" is already spoken for in
this codebase by SSH keys, which live in harvest/ssh.py and are a different kind
of secret entirely.
"""
from __future__ import annotations

import hashlib
import logging
import secrets
import uuid
from typing import Optional

from ..db import read_cursor, run_once, transaction

logger = logging.getLogger("praxevia.warden.api_tokens")

#: Prefix on every newly issued token.
API_TOKEN_PREFIX = "prx-"

#: Prefix used before this naming pass. Tokens carrying it are still accepted:
#: lookup is by digest, so the prefix only decides whether a bearer string is
#: worth a database round-trip at all. Dropping it would invalidate every token
#: issued to date without warning. Retiring it is a separate decision, to be
#: made once existing tokens have been reissued.
LEGACY_TOKEN_PREFIX = "cce-"

_ACCEPTED_PREFIXES = (API_TOKEN_PREFIX, LEGACY_TOKEN_PREFIX)


def looks_like_api_token(token: str) -> bool:
    """Whether this bearer string is shaped like one of our tokens.

    Callers use this to decide whether to attempt a token lookup. Keeping the
    test here means the set of accepted prefixes is defined once; a guard that
    compared against a single imported constant would silently lock out every
    legacy token the moment the prefix changed.
    """
    return token.startswith(_ACCEPTED_PREFIXES)


def _hash_token(raw_token: str) -> str:
    """The digest stored in place of the token itself."""
    return hashlib.sha256(raw_token.encode()).hexdigest()


@run_once
def ensure_api_token_schema() -> None:
    """Create the token table and the indexes the lookup path depends on."""
    with transaction() as cur:
        cur.execute(
            """
            CREATE TABLE IF NOT EXISTS api_keys (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                user_email VARCHAR(255) NOT NULL,
                name VARCHAR(100) NOT NULL,
                key_prefix VARCHAR(20) NOT NULL,
                key_hash VARCHAR(64) NOT NULL UNIQUE,
                created_at TIMESTAMP DEFAULT NOW()
            )
            """
        )
        # Replaces the earlier idx_* index names; dropping the old one first
        # avoids ending up with two indexes covering the same column.
        cur.execute("DROP INDEX IF EXISTS idx_api_keys_hash")
        cur.execute("CREATE INDEX IF NOT EXISTS api_keys_hash_idx ON api_keys(key_hash)")
        cur.execute("DROP INDEX IF EXISTS idx_api_keys_user")
        cur.execute("CREATE INDEX IF NOT EXISTS api_keys_user_idx ON api_keys(user_email)")
    logger.info("api token table ready")


def issue_api_token(user_email: str, name: str) -> dict:
    """Mint a token for a user.

    The returned `raw_token` is the only time the caller will ever see it. Every
    later read returns the prefix alone.
    """
    ensure_api_token_schema()
    raw_token = API_TOKEN_PREFIX + secrets.token_hex(20)
    visible_prefix = raw_token[:12]
    token_id = str(uuid.uuid4())

    with transaction() as cur:
        cur.execute(
            "INSERT INTO api_keys (id, user_email, name, key_prefix, key_hash) "
            "VALUES (%s, %s, %s, %s, %s) RETURNING created_at",
            (token_id, user_email, name, visible_prefix, _hash_token(raw_token)),
        )
        created_at = cur.fetchone()[0]

    return {
        "id": token_id,
        "name": name,
        "key_prefix": visible_prefix,
        "raw_key": raw_token,
        "created_at": created_at.isoformat(),
    }


def list_api_tokens(user_email: str) -> list[dict]:
    """A user's tokens, newest first. Digests are never returned."""
    ensure_api_token_schema()
    with read_cursor(dict_rows=True) as cur:
        cur.execute(
            "SELECT id, name, key_prefix, created_at FROM api_keys "
            "WHERE user_email = %s ORDER BY created_at DESC",
            (user_email,),
        )
        rows = cur.fetchall()

    return [
        {
            "id": str(row["id"]),
            "name": row["name"],
            "key_prefix": row["key_prefix"],
            "created_at": row["created_at"].isoformat(),
        }
        for row in rows
    ]


def revoke_api_token(token_id: str, user_email: str) -> bool:
    """Delete one token, scoped to its owner.

    The owner is part of the WHERE clause on purpose: without it, knowing any
    token's id would be enough to revoke somebody else's.
    """
    ensure_api_token_schema()
    with transaction() as cur:
        cur.execute(
            "DELETE FROM api_keys WHERE id = %s AND user_email = %s",
            (token_id, user_email),
        )
        return cur.rowcount > 0


def resolve_api_token(raw_token: str) -> Optional[str]:
    """The email behind a token, or None if it is unknown or revoked."""
    ensure_api_token_schema()
    with read_cursor() as cur:
        cur.execute(
            "SELECT user_email FROM api_keys WHERE key_hash = %s",
            (_hash_token(raw_token),),
        )
        row = cur.fetchone()
    return row[0] if row else None


def resolve_api_token_with_name(raw_token: str) -> Optional[dict]:
    """As resolve_api_token, plus the token's label -- used by usage reporting
    so events can be attributed to a specific token rather than just a user."""
    ensure_api_token_schema()
    with read_cursor() as cur:
        cur.execute(
            "SELECT user_email, name FROM api_keys WHERE key_hash = %s",
            (_hash_token(raw_token),),
        )
        row = cur.fetchone()
    return {"user_email": row[0], "name": row[1]} if row else None
