"""
Project sources for the dashboard — add a project, change where it comes from.

WHY THIS EXISTS ALONGSIDE THE ADMIN API. Praxevia already models this properly:
`managed_projects` -> `project_streams` -> `repository_sources`, driven by
`/api/admin/*` and guarded by `ensure_admin`. That surface is the right one
for an operator managing many teams. It is the wrong one for the dashboard,
where an ordinary user points Insight at a folder or a repository URL and
expects it to index. Rather than grant the dashboard admin rights or duplicate
the three-table model in the client, this keeps one flat row per
project+stream, and defers to the harvest machinery for the actual work.

LOCKING IS THE POINT, not a nicety. Changing where a project's code comes from
invalidates every chunk, symbol, call and graph node derived from the old
location. Serving the old graph while the new one is being built would show
answers about code that is no longer there, so a source change locks the
project until the rebuild finishes and the UI greys it out.
"""

from __future__ import annotations

import logging
import os
import re
import shlex
import subprocess
import threading
from typing import Callable

import psycopg2
from fastapi import APIRouter, HTTPException, Request
from pydantic import BaseModel

from .store import STORE

logger = logging.getLogger(__name__)

# How to run the indexer and the graph builder.
#
# Configurable because the indexer does NOT run in the API's environment: it
# needs sentence-transformers and torch, which the backend deliberately does not
# import — a 550 MB model must not be a prerequisite for serving a graph).
# In this deployment it runs in a container; elsewhere it may be a sibling
# virtualenv. `{project}` and `{stream}` are substituted.
INDEX_CMD = os.environ.get("INDEX_CMD", "python main.py {project} {stream}")
GRAPH_CMD = os.environ.get(
    "GRAPH_CMD", "python -m graph_pass.builder {project} {stream}"
)
INDEXER_DIR = os.environ.get(
    "INDEXER_DIR",
    os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "..", "indexer")),
)


def _run(template: str, project: str, stream: str) -> None:
    """Run one stage, raising CalledProcessError with output attached."""
    cmd = shlex.split(template.format(project=project, stream=stream))
    logger.info("Running %s (cwd=%s)", cmd, INDEXER_DIR)
    subprocess.run(
        cmd, cwd=INDEXER_DIR, env=os.environ.copy(), check=True,
        capture_output=True, timeout=7200,
    )

router = APIRouter(prefix="/api/graph", tags=["graph-sources"])

_verify: Callable[[Request], str] | None = None
_check_access: Callable[[str, str], None] | None = None

_SANITISE = re.compile(r"[^a-z0-9]+")
_GIT_URL = re.compile(r"^(https?://|git@|ssh://)", re.I)


def registry_key(project: str, stream: str) -> str:
    return f"{_SANITISE.sub('_', project.lower())}_{_SANITISE.sub('_', stream.lower())}"


def _conn():
    dsn = os.environ.get("DATABASE_URL")
    if not dsn:
        raise HTTPException(status_code=500, detail="DATABASE_URL is not configured")
    return psycopg2.connect(dsn)


def ensure_table() -> None:
    try:
        with _conn() as c, c.cursor() as cur:
            cur.execute("""
                CREATE TABLE IF NOT EXISTS project_sources (
                    registry_key TEXT PRIMARY KEY,
                    project      TEXT NOT NULL,
                    stream       TEXT NOT NULL,
                    path         TEXT NOT NULL,
                    kind         TEXT NOT NULL DEFAULT 'local',
                    status       TEXT NOT NULL DEFAULT 'ready',
                    message      TEXT NOT NULL DEFAULT '',
                    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
                )
            """)
            c.commit()
    except Exception:
        logger.exception("Could not ensure project_sources table")


class SourceBody(BaseModel):
    project: str
    stream: str = "develop"
    path: str


def classify(path: str) -> str:
    """`git` for anything that looks like a clone URL, otherwise `local`."""
    return "git" if _GIT_URL.match(path.strip()) else "local"


def _row(project: str, stream: str) -> dict | None:
    key = registry_key(project, stream)
    with _conn() as c, c.cursor() as cur:
        cur.execute(
            "SELECT path, kind, status, message FROM project_sources WHERE registry_key=%s",
            (key,),
        )
        r = cur.fetchone()
    if not r:
        return None
    return {"path": r[0], "kind": r[1], "status": r[2], "message": r[3]}


def _set(project: str, stream: str, path: str, status: str, message: str = "") -> None:
    key = registry_key(project, stream)
    with _conn() as c, c.cursor() as cur:
        cur.execute(
            """
            INSERT INTO project_sources (registry_key, project, stream, path, kind, status, message)
            VALUES (%s,%s,%s,%s,%s,%s,%s)
            ON CONFLICT (registry_key) DO UPDATE SET
                path=EXCLUDED.path, kind=EXCLUDED.kind, status=EXCLUDED.status,
                message=EXCLUDED.message, updated_at=NOW()
            """,
            (key, project, stream, path, classify(path), status, message),
        )
        c.commit()


def _rebuild(project: str, stream: str, path: str) -> None:
    """
    Index the source, then rebuild its graph. Runs on a worker thread.

    Failures are recorded on the row rather than raised: this runs detached from
    any request, and a project stuck in `indexing` forever with no explanation
    is worse than one that says why it stopped.
    """
    key = registry_key(project, stream)
    try:
        _set(project, stream, path, "indexing", "Indexing source…")
        _run(INDEX_CMD, project, stream)

        _set(project, stream, path, "indexing", "Building graph…")
        _run(GRAPH_CMD, project, stream)

        # The store caches per project+stream; a rebuilt graph the API keeps
        # serving from memory is the same staleness this whole module exists to
        # prevent.
        STORE.invalidate(key)
        _set(project, stream, path, "ready", "")
        logger.info("Rebuild finished for %s", key)
    except subprocess.CalledProcessError as e:
        tail = (e.stderr or b"").decode("utf-8", "replace")[-400:]
        _set(project, stream, path, "failed", tail or "Indexing failed")
        logger.error("Rebuild failed for %s: %s", key, tail)
    except Exception as e:  # noqa: BLE001 — detached thread, nothing to raise to
        _set(project, stream, path, "failed", str(e)[:400])
        logger.exception("Rebuild failed for %s", key)


def mount(app, verify_token, check_project_access) -> None:
    global _verify, _check_access
    _verify, _check_access = verify_token, check_project_access
    ensure_table()
    app.include_router(router)


def _auth(request: Request, project: str) -> None:
    if _verify is None or _check_access is None:  # pragma: no cover
        raise HTTPException(status_code=500, detail="sources router not mounted")
    _check_access(_verify(request), project)


@router.get("/source")
def get_source(request: Request, project: str, stream: str = "develop"):
    """Where this project's code comes from, and whether it is mid-rebuild."""
    _auth(request, project)
    row = _row(project, stream)
    if row is None:
        # Not yet recorded: fall back to the conventional on-disk location so
        # the panel shows something true rather than an empty field.
        base = os.environ.get("CODEBASE_DIR", "")
        guess = os.path.join(base, project, stream) if base else ""
        return {"path": guess, "kind": "local", "status": "ready", "message": "",
                "recorded": False}
    return {**row, "recorded": True}


@router.put("/source")
def put_source(request: Request, body: SourceBody):
    """
    Point a project at a new path or repository, and rebuild it.

    Returns immediately with `status: "indexing"`. The client polls GET /source
    and keeps the project greyed out until it reads `ready` or `failed`.
    """
    _auth(request, body.project)
    path = body.path.strip()
    if not path:
        raise HTTPException(status_code=400, detail="Path must not be empty")

    kind = classify(path)
    if kind == "local" and not os.path.isdir(path):
        raise HTTPException(
            status_code=400,
            detail=f"No such directory on the server: {path}",
        )

    current = _row(body.project, body.stream)
    if current and current["status"] == "indexing":
        raise HTTPException(
            status_code=409,
            detail="This project is already being rebuilt. Wait for it to finish.",
        )

    _set(body.project, body.stream, path, "indexing", "Queued…")
    threading.Thread(
        target=_rebuild, args=(body.project, body.stream, path), daemon=True,
    ).start()
    return {"path": path, "kind": kind, "status": "indexing", "message": "Queued…"}
