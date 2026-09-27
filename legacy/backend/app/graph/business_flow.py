"""
Business-flow graphs — generate one for a project, and serve it.

WHY A SECOND ARTIFACT RATHER THAN MORE NODES IN THE FIRST. The dashboard fetches
the codebase graph and the business-flow graph as two documents and holds them in
two store slots (`graph` and `domainGraph`), switching between them with
`viewMode`. Folding domain/flow/step nodes into the codebase graph would put them
on the structural canvas, where they are not what anyone asked to see, and would
make "regenerate the flows" mean "rebuild the whole graph".

`emit.py` states the storage rule this follows: "key by project + stream + KIND
from day one. The earlier viewer had one graph slot per project and its satellite skills
overwrite each other — do not inherit that."

JOB SHAPE COPIED FROM `sources.py`. A background thread plus a status row the
client polls, not SSE and not an in-memory dict. In-memory would be wrong for a
reason that is easy to miss: `docker-entrypoint.sh` runs uvicorn with
`--workers 2`, so the worker that answers the status poll is often not the worker
running the job. The row is the only place both can see.
"""

from __future__ import annotations

import json
import logging
import os
import re
import threading
from datetime import datetime, timedelta, timezone
from typing import Callable

import psycopg2
from fastapi import APIRouter, HTTPException, Request
from pydantic import BaseModel

from ..config import ANTHROPIC_API_KEY, CLAUDE_MODEL
from ..telemetry import record_event
from .business_flow_llm import enrich_business_flow_with_llm
from .business_flow_synth import synthesize_business_flow_deterministic
from .store import DOMAIN_STORE, STORE

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api/graph", tags=["graph-business-flow"])

_verify: Callable[[Request], str] | None = None
_check_access: Callable[[str, str], None] | None = None

_SANITISE = re.compile(r"[^a-z0-9]+")

VALID_MODES = ("deterministic", "llm")

# One successful LLM run per project per day.
#
# THIS IS THE SPEND CONTROL, and it is deliberately the only one. The whole
# deployment shares a single ANTHROPIC_API_KEY (docker-compose.yml), so without
# a limit any user on any project could spend the same budget in a loop. A
# cooldown is a better fit than a token ceiling here: business flows change on
# the timescale of a codebase, not a coffee break, so a second run within the
# day would buy a differently-worded answer to the same question.
#
# It gates on SUCCESSFUL runs only. A failed run left nothing to protect, and
# locking someone out for a day because the key was misconfigured would be
# hostile. Deterministic mode is never gated — it costs nothing.
LLM_COOLDOWN_HOURS = float(os.environ.get("BUSINESS_FLOW_LLM_COOLDOWN_HOURS", "24"))


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
                CREATE TABLE IF NOT EXISTS business_flow_graphs (
                    registry_key TEXT PRIMARY KEY,
                    project      TEXT NOT NULL,
                    stream       TEXT NOT NULL,
                    mode         TEXT NOT NULL DEFAULT 'deterministic',
                    status       TEXT NOT NULL DEFAULT 'idle',
                    message      TEXT NOT NULL DEFAULT '',
                    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
                )
            """)
            c.commit()
    except Exception:
        logger.exception("Could not ensure business_flow_graphs table")


class GenerateBody(BaseModel):
    project: str
    stream: str = "develop"
    mode: str = "deterministic"


def _row(project: str, stream: str) -> dict | None:
    key = registry_key(project, stream)
    with _conn() as c, c.cursor() as cur:
        cur.execute(
            "SELECT mode, status, message, updated_at FROM business_flow_graphs "
            "WHERE registry_key=%s",
            (key,),
        )
        r = cur.fetchone()
    if not r:
        return None
    return {"mode": r[0], "status": r[1], "message": r[2], "updated_at": r[3]}


def _set(project: str, stream: str, mode: str, status: str, message: str = "") -> None:
    key = registry_key(project, stream)
    with _conn() as c, c.cursor() as cur:
        cur.execute(
            """
            INSERT INTO business_flow_graphs (registry_key, project, stream, mode, status, message)
            VALUES (%s,%s,%s,%s,%s,%s)
            ON CONFLICT (registry_key) DO UPDATE SET
                mode=EXCLUDED.mode, status=EXCLUDED.status,
                message=EXCLUDED.message, updated_at=NOW()
            """,
            (key, project, stream, mode, status, message),
        )
        c.commit()


def _llm_cooldown_remaining(row: dict | None) -> timedelta | None:
    """
    How long until this project may run LLM mode again, or None if it may now.

    Reads `updated_at` off the status row rather than keeping a separate ledger:
    the row is already written on every state change, already shared between
    uvicorn workers, and already the thing the client polls. A second table
    would be a second source of truth for the same fact.
    """
    if row is None or row.get("mode") != "llm" or row.get("status") != "ready":
        return None
    last = row.get("updated_at")
    if last is None:
        return None
    if last.tzinfo is None:  # defensive: TIMESTAMPTZ should always be aware
        last = last.replace(tzinfo=timezone.utc)
    elapsed = datetime.now(timezone.utc) - last
    cooldown = timedelta(hours=LLM_COOLDOWN_HOURS)
    return cooldown - elapsed if elapsed < cooldown else None


def _write_atomically(path: str, graph: dict) -> None:
    """
    Write the graph to a temp file in the same directory, then rename over.

    A regeneration that fails halfway must not destroy the graph the user is
    currently looking at. `os.replace` is atomic within a filesystem, so a reader
    sees either the old complete document or the new one, never a truncated one.
    Writing the temp file beside the target rather than in /tmp keeps it on the
    same filesystem, which is what makes the rename atomic rather than a copy.
    """
    os.makedirs(os.path.dirname(path), exist_ok=True)
    tmp = f"{path}.tmp-{os.getpid()}"
    with open(tmp, "w") as fh:
        json.dump(graph, fh)
    os.replace(tmp, path)


def _generate(project: str, stream: str, mode: str) -> None:
    """
    Synthesise the business-flow graph, then persist it. Runs on a worker thread.

    Failures are recorded on the row rather than raised: this runs detached from
    any request, and a project stuck in `generating` forever with no explanation
    is worse than one that says why it stopped.
    """
    key = registry_key(project, stream)
    try:
        loaded = STORE.get(key)
        if loaded is None:
            raise RuntimeError(
                f"No codebase graph for {project}/{stream} — build that first."
            )

        _set(project, stream, mode, "generating", "Deriving flows from the call graph…")
        graph = synthesize_business_flow_deterministic(loaded.graph)

        message = ""
        if mode == "llm":
            # The deterministic pass runs first in BOTH modes: it produces the
            # structure, and the model only renames it. That ordering is what
            # keeps an LLM run from being able to invent a process.
            _set(project, stream, mode, "generating", "Naming domains and flows…")
            graph, renamed = enrich_business_flow_with_llm(graph)
            record_event(
                event_type="business_flow_llm",
                project=project,
                stream=stream,
                model=CLAUDE_MODEL,
            )
            if renamed == 0:
                message = "The model returned no confident names; structural names kept."

        _write_atomically(DOMAIN_STORE.path_for(key), graph)

        counts = {t: sum(1 for n in graph["nodes"] if n["type"] == t)
                  for t in ("domain", "flow", "step")}
        _set(project, stream, mode, "ready", message)
        logger.info("Business-flow graph generated for %s (%s): %s", key, mode, counts)
    except Exception as e:  # noqa: BLE001 — detached thread, nothing to raise to
        _set(project, stream, mode, "failed", str(e)[:400])
        logger.exception("Business-flow generation failed for %s", key)


def mount(app, verify_token, check_project_access) -> None:
    global _verify, _check_access
    _verify, _check_access = verify_token, check_project_access
    ensure_table()
    app.include_router(router)
    app.include_router(compat_router)


def _auth(request: Request, project: str) -> None:
    if _verify is None or _check_access is None:  # pragma: no cover
        raise HTTPException(status_code=500, detail="business-flow router not mounted")
    _check_access(_verify(request), project)


@router.get("/business-flow")
def get_business_flow_status(request: Request, project: str, stream: str = "develop"):
    """Whether a business-flow graph exists for this project+stream, and its mode."""
    _auth(request, project)
    row = _row(project, stream)
    if row is None:
        # Never generated. `exists` is answered from disk rather than the row so
        # a graph produced outside this endpoint still shows up.
        exists = os.path.exists(DOMAIN_STORE.path_for(registry_key(project, stream)))
        return {"mode": None, "status": "ready" if exists else "idle", "message": ""}
    return row


@router.post("/business-flow")
def post_business_flow(request: Request, body: GenerateBody):
    """
    Generate the business-flow graph for one project+stream, in the background.

    Returns immediately with `status: "generating"`. The client polls GET
    /business-flow and refetches /api/compat/domain-graph.json once it reads
    `ready` — the same contract as PUT /source.
    """
    _auth(request, body.project)

    if body.mode not in VALID_MODES:
        raise HTTPException(
            status_code=400,
            detail=f"mode must be one of {', '.join(VALID_MODES)}",
        )
    if body.mode == "llm" and not ANTHROPIC_API_KEY:
        # 503 rather than 500: the deployment is missing configuration, and the
        # deterministic mode still works. Saying so is more useful than an API
        # error surfacing from a worker thread minutes later.
        raise HTTPException(
            status_code=503,
            detail="LLM mode needs ANTHROPIC_API_KEY — the same key the advisor uses. "
                   "Deterministic mode still works.",
        )

    current = _row(body.project, body.stream)
    if body.mode == "llm":
        remaining = _llm_cooldown_remaining(current)
        if remaining is not None:
            hours = remaining.total_seconds() / 3600
            raise HTTPException(
                status_code=429,
                detail=(
                    f"Business flows were already generated with the model for this "
                    f"project today. Try again in {hours:.1f}h, or use deterministic "
                    f"mode, which has no limit."
                ),
                headers={"Retry-After": str(int(remaining.total_seconds()))},
            )

    key = registry_key(body.project, body.stream)
    if STORE.get(key) is None:
        raise HTTPException(
            status_code=404,
            detail=f"No codebase graph built for {body.project}/{body.stream} yet.",
        )

    if current and current["status"] == "generating":
        raise HTTPException(
            status_code=409,
            detail="A business-flow graph is already being generated for this project.",
        )

    _set(body.project, body.stream, body.mode, "generating", "Queued…")
    threading.Thread(
        target=_generate, args=(body.project, body.stream, body.mode), daemon=True,
    ).start()
    return {"mode": body.mode, "status": "generating", "message": "Queued…"}


# The URL App.tsx has always fetched. It answered 404 for the whole of v1
# because nothing produced the artifact behind it; the fetch already degrades to
# `null`, so this route lighting up is the entire client-side change needed.
compat_router = APIRouter(prefix="/api/compat", tags=["graph-compat-business-flow"])


@compat_router.get("/domain-graph.json")
def compat_domain_graph(request: Request, project: str, stream: str):
    """
    Serve the business-flow graph, BARE rather than enveloped.

    `/api/graph/*` wraps responses as `{"graph": ..., "meta": ...}`, but the
    compat routes exist to keep App.tsx's existing fetches working unchanged, and
    that fetch passes the body straight to `validateGraph`. An envelope here
    would fail validation on a missing `project` field.
    """
    _auth(request, project)
    loaded = DOMAIN_STORE.get(registry_key(project, stream))
    if loaded is None:
        raise HTTPException(
            status_code=404,
            detail=f"No business-flow graph for {project}/{stream}.",
        )
    return loaded.graph
