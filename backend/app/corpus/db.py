"""Shared resources the search tools sit on: the embedder and the pool.

Both are process-wide singletons created on first use rather than at import.
The sentence-transformer model costs seconds to load and hundreds of megabytes
to hold, so a worker that never serves a semantic search never pays for it.

Connections come from a pool because the search tools run queries in parallel --
walking a call chain fans out across several at once -- and under that load
opening a fresh connection costs more than the query it carries. Borrowers must
give connections back; every caller does so in a finally block.
"""
from __future__ import annotations

import logging
import os
import threading
from typing import TYPE_CHECKING, Optional

import psycopg2
import psycopg2.extras
import psycopg2.pool

if TYPE_CHECKING:  # pragma: no cover
    from sentence_transformers import SentenceTransformer

from ..config import DATABASE_URL, EMBEDDING_MODEL

logger = logging.getLogger("praxevia.corpus.db")

#: Pool bounds. The floor keeps a couple of connections warm for the common
#: case; the ceiling is what stops a burst of parallel tool calls from opening
#: more connections than the server will accept.
_POOL_MIN_CONNECTIONS = 2
_POOL_MAX_CONNECTIONS = 20

_embedding_model: Optional["SentenceTransformer"] = None
_embedding_lock = threading.Lock()
_connection_pool: Optional[psycopg2.pool.SimpleConnectionPool] = None


def load_embedding_model() -> "SentenceTransformer":
    """Return the shared embedding model, loading it on first call.

    The sentence_transformers import is inside the function on purpose. It pulls
    in torch, and the graph half of this product needs neither torch nor the
    model -- hoisting the import to module scope would make a graph-only
    deployment carry a multi-gigabyte ML stack it never executes. A genuinely
    missing dependency still surfaces, loudly, the first time somebody runs a
    semantic search.

    Double-checked locking: the outer test avoids taking the lock once the model
    is loaded, the inner test covers two callers that both got past the outer
    one before either finished.
    """
    from sentence_transformers import SentenceTransformer

    global _embedding_model
    if _embedding_model is not None:
        return _embedding_model

    with _embedding_lock:
        if _embedding_model is None:
            # Pinned to CPU unless told otherwise, rather than letting the
            # library choose. Left to itself it picks the Apple Silicon GPU
            # backend, and encoding there from inside a worker thread killed the
            # whole process mid-request -- the log simply stopped, with a leaked
            # semaphore warning and no traceback, because nothing raised. Every
            # graph endpoint died with it.
            #
            # What this process embeds is one short query per search. Bulk
            # embedding happens in the indexer, which makes its own device
            # choice. Milliseconds here are not worth a server that falls over.
            device = os.environ.get("EMBEDDING_DEVICE", "cpu")
            logger.info("loading embedding model %s on %s", EMBEDDING_MODEL, device)
            _embedding_model = SentenceTransformer(
                EMBEDDING_MODEL, trust_remote_code=True, device=device
            )
            logger.info("embedding model ready")

    return _embedding_model


def _pool() -> psycopg2.pool.SimpleConnectionPool:
    """The shared pool, created on first use."""
    global _connection_pool
    if _connection_pool is None:
        _connection_pool = psycopg2.pool.SimpleConnectionPool(
            _POOL_MIN_CONNECTIONS, _POOL_MAX_CONNECTIONS, DATABASE_URL
        )
    return _connection_pool


def get_pooled_connection():
    """Borrow a connection. The caller must release it."""
    return _pool().getconn()


def release_pooled_connection(conn) -> None:
    """Give a connection back.

    Failures are swallowed deliberately. This runs in finally blocks on paths
    that may already be handling an exception, and a pool that has been closed
    or a connection it no longer recognises must not replace the original error
    with one about bookkeeping.
    """
    try:
        _pool().putconn(conn)
    except Exception:
        logger.debug("could not return a connection to the pool", exc_info=True)
