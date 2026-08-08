"""Stored chat transcripts.

Presented as a single object rather than a set of free functions. The eight
operations here all share the same two tables, the same ownership rule and the
same one-time schema setup, and coordinating that through a module-level flag
left the relationship implicit. `ConversationStore` makes it explicit, and the
module-level `conversations` singleton means callers still get one shared
instance without having to pass it around.

Two invariants worth stating up front:

*Ownership is enforced in SQL, not in Python.* Every query that touches a
conversation carries `AND user_email = %s`. A caller cannot read, rename or
delete somebody else's transcript even if it guesses the id, and there is no
code path where forgetting a check would expose one -- the check is part of the
statement.

*Transcripts expire.* Anything untouched for the retention window is deleted on
startup. These are working notes about a codebase, not records anyone has
undertaken to keep.
"""
from __future__ import annotations

import logging
import uuid
from datetime import datetime, timedelta
from typing import List, Optional

from ..db import read_cursor, transaction

logger = logging.getLogger("praxevia.storage.conversations")

#: Conversations idle for longer than this are removed at startup.
RETENTION_DAYS = 30

#: Longest auto-derived title before it is cut short.
TITLE_MAX_CHARS = 80


class ConversationStore:
    """Create, read, amend and expire stored chat transcripts."""

    def __init__(self) -> None:
        self._schema_ready = False

    # -- schema ------------------------------------------------------------

    def ensure_schema(self) -> None:
        """Create both tables, then take the opportunity to expire old rows.

        Failure is logged rather than raised, and `_schema_ready` stays False so
        the next call tries again -- the usual cause is the database not being
        up yet, which resolves itself.
        """
        if self._schema_ready:
            return
        try:
            with transaction() as cur:
                cur.execute(
                    """
                    CREATE TABLE IF NOT EXISTS conversations (
                        id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                        user_email VARCHAR(255) NOT NULL,
                        title VARCHAR(500) NOT NULL DEFAULT '',
                        project VARCHAR(100) DEFAULT '',
                        stream VARCHAR(100) DEFAULT '',
                        model VARCHAR(100) DEFAULT '',
                        created_at TIMESTAMP DEFAULT NOW(),
                        updated_at TIMESTAMP DEFAULT NOW()
                    )
                    """
                )
                cur.execute("DROP INDEX IF EXISTS idx_conversations_user")
                cur.execute(
                    """
                    CREATE INDEX IF NOT EXISTS conversations_user_idx
                    ON conversations(user_email, updated_at DESC)
                    """
                )
                # ON DELETE CASCADE is what makes delete() a single statement:
                # removing the conversation takes its messages with it.
                cur.execute(
                    """
                    CREATE TABLE IF NOT EXISTS conversation_messages (
                        id SERIAL PRIMARY KEY,
                        conversation_id UUID NOT NULL
                            REFERENCES conversations(id) ON DELETE CASCADE,
                        role VARCHAR(20) NOT NULL,
                        content TEXT NOT NULL,
                        created_at TIMESTAMP DEFAULT NOW()
                    )
                    """
                )
                cur.execute("DROP INDEX IF EXISTS idx_conv_messages_conv")
                cur.execute(
                    """
                    CREATE INDEX IF NOT EXISTS conversation_messages_conv_idx
                    ON conversation_messages(conversation_id, id)
                    """
                )
        except Exception as exc:
            logger.warning("conversation tables not ready (%s); will retry", exc)
            return

        self._schema_ready = True
        logger.info("conversation tables ready")
        self.purge_expired()

    def purge_expired(self, days: int = RETENTION_DAYS) -> int:
        """Delete conversations idle for longer than the retention window.

        Returns how many went. Messages follow via the cascade.
        """
        cutoff = datetime.utcnow() - timedelta(days=days)
        try:
            with transaction() as cur:
                cur.execute("DELETE FROM conversations WHERE updated_at < %s", (cutoff,))
                removed = cur.rowcount
        except Exception as exc:
            logger.warning("could not expire old conversations: %s", exc)
            return 0

        if removed:
            logger.info("expired %d conversation(s) idle over %d days", removed, days)
        return removed

    # -- titles ------------------------------------------------------------

    @staticmethod
    def _title_from_first_message(text: str, max_len: int = TITLE_MAX_CHARS) -> str:
        """Condense an opening message into a one-line label.

        Whitespace is flattened so a multi-line paste does not become a
        multi-line title. Over-long text is cut at a word boundary when one
        falls in the back half of the limit; cutting at the first available
        space instead could leave a title of one word.
        """
        flattened = " ".join(text.split())
        if len(flattened) <= max_len:
            return flattened

        clipped = flattened[:max_len]
        boundary = clipped.rfind(" ")
        if boundary > max_len // 2:
            clipped = clipped[:boundary]
        return clipped + "..."

    # -- reads -------------------------------------------------------------

    def list(self, user_email: str, limit: int = 50, offset: int = 0) -> list[dict]:
        """One user's conversations, most recently touched first."""
        self.ensure_schema()
        with read_cursor(dict_rows=True) as cur:
            cur.execute(
                "SELECT id, title, project, stream, updated_at FROM conversations "
                "WHERE user_email = %s ORDER BY updated_at DESC LIMIT %s OFFSET %s",
                (user_email, limit, offset),
            )
            rows = cur.fetchall()

        return [
            {
                "id": str(row["id"]),
                "title": row["title"],
                "project": row["project"],
                "stream": row["stream"],
                "updated_at": row["updated_at"].isoformat(),
            }
            for row in rows
        ]

    def get(self, conversation_id: str, user_email: str) -> Optional[dict]:
        """One conversation with its messages in order, or None.

        None covers both "no such conversation" and "not yours" on purpose: the
        caller returns 404 either way, so a probe cannot distinguish an id that
        does not exist from one belonging to somebody else.
        """
        self.ensure_schema()
        with read_cursor(dict_rows=True) as cur:
            cur.execute(
                "SELECT id, title, project, stream, model, created_at, updated_at "
                "FROM conversations WHERE id = %s AND user_email = %s",
                (conversation_id, user_email),
            )
            header = cur.fetchone()
            if not header:
                return None

            cur.execute(
                "SELECT role, content FROM conversation_messages "
                "WHERE conversation_id = %s ORDER BY id",
                (conversation_id,),
            )
            messages = [{"role": r["role"], "content": r["content"]} for r in cur.fetchall()]

        return {
            "id": str(header["id"]),
            "title": header["title"],
            "project": header["project"],
            "stream": header["stream"],
            "model": header["model"] or "",
            "created_at": header["created_at"].isoformat(),
            "updated_at": header["updated_at"].isoformat(),
            "messages": messages,
        }

    # -- writes ------------------------------------------------------------

    def create(
        self,
        user_email: str,
        project: str = "",
        stream: str = "",
        model: str = "",
    ) -> dict:
        """Open an empty conversation. Its title is filled in on first append."""
        self.ensure_schema()
        conversation_id = str(uuid.uuid4())
        with transaction() as cur:
            cur.execute(
                "INSERT INTO conversations (id, user_email, project, stream, model) "
                "VALUES (%s, %s, %s, %s, %s) RETURNING id, title, created_at",
                (conversation_id, user_email, project, stream, model),
            )
            row = cur.fetchone()

        return {
            "id": str(row[0]),
            "title": row[1],
            "project": project,
            "stream": stream,
            "created_at": row[2].isoformat(),
        }

    def append_messages(
        self,
        conversation_id: str,
        user_email: str,
        messages: List[dict],
    ) -> Optional[dict]:
        """Add turns to a conversation, naming it if it is still untitled.

        Ownership check, inserts, title derivation and the timestamp bump all
        share one transaction: a partial application would leave a transcript
        with messages appended but no title and a stale `updated_at`, which
        sorts it to the wrong end of the user's list.
        """
        self.ensure_schema()
        with transaction(dict_rows=True) as cur:
            cur.execute(
                "SELECT id, title FROM conversations WHERE id = %s AND user_email = %s",
                (conversation_id, user_email),
            )
            existing = cur.fetchone()
            if not existing:
                return None

            for message in messages:
                cur.execute(
                    "INSERT INTO conversation_messages (conversation_id, role, content) "
                    "VALUES (%s, %s, %s)",
                    (conversation_id, message["role"], message["content"]),
                )

            if not existing["title"]:
                opener = next((m for m in messages if m["role"] == "user"), None)
                if opener:
                    cur.execute(
                        "UPDATE conversations SET title = %s WHERE id = %s",
                        (self._title_from_first_message(opener["content"]), conversation_id),
                    )

            cur.execute(
                "UPDATE conversations SET updated_at = NOW() WHERE id = %s "
                "RETURNING title, updated_at",
                (conversation_id,),
            )
            refreshed = cur.fetchone()

        return {
            "id": conversation_id,
            "title": refreshed["title"],
            "updated_at": refreshed["updated_at"].isoformat(),
        }

    def rename(self, conversation_id: str, user_email: str, title: str) -> bool:
        """Set a conversation's title by hand. False if it is not the caller's."""
        self.ensure_schema()
        with transaction() as cur:
            cur.execute(
                "UPDATE conversations SET title = %s WHERE id = %s AND user_email = %s",
                (title, conversation_id, user_email),
            )
            return cur.rowcount > 0

    def delete(self, conversation_id: str, user_email: str) -> bool:
        """Remove a conversation and, by cascade, its messages."""
        self.ensure_schema()
        with transaction() as cur:
            cur.execute(
                "DELETE FROM conversations WHERE id = %s AND user_email = %s",
                (conversation_id, user_email),
            )
            return cur.rowcount > 0


#: The shared instance. Import this rather than constructing your own -- the
#: schema latch lives on the instance, so a second store would re-run setup.
conversations = ConversationStore()
