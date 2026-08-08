"""
/api/graph/* endpoints —

AUTH. Reuses `identify_caller` and `ensure_project_access` verbatim, injected
by `main.py` rather than imported, to avoid a circular import. That matters
beyond tidiness: a user who cannot chat about a project must not be able to see
its graph, and the only way to guarantee that is to run the same check, not a
parallel one. The earlier viewer's auth — a single process-lifetime shared secret
(vite.config.ts:9-12) — is not carried over.

RESPONSE ENVELOPE. Every endpoint returns

    {"graph": <valid KnowledgeGraph fragment>, "meta": {...}}

and NOT a bare graph. verified why: `KnowledgeGraphSchema` is a plain
`z.object()`, so a new top-level key is stripped; and `ProjectMetaSchema`
(schema.ts:466-473) is *also* plain with six fixed fields, so hiding the metadata
inside `project` fails identically and silently. The client unwraps `body.graph`
before calling `validateGraph()`.

COMPATIBILITY SHIMS are mounted at the paths the dashboard already
fetches, so porting it is a re-point rather than a rewrite.
"""

from __future__ import annotations

import re
from typing import Callable

from fastapi import APIRouter, HTTPException, Query, Request, Response

from . import queries
from .store import STORE

router = APIRouter(prefix="/api/graph", tags=["graph"])

# Injected by main.py at wiring time (see mount()).
_verify: Callable[[Request], str] | None = None
_check_access: Callable[[str, str], None] | None = None

_SANITISE = re.compile(r"[^a-z0-9]+")


def mount(app, verify_token, check_project_access) -> None:
    """Wire the router into the FastAPI app with the real auth functions."""
    global _verify, _check_access
    _verify = verify_token
    _check_access = check_project_access
    app.include_router(router)
    app.include_router(compat_router)


def registry_key(project: str, stream: str) -> str:
    return f"{_SANITISE.sub('_', project.lower())}_{_SANITISE.sub('_', stream.lower())}"


def _authorised_graph(request: Request, project: str, stream: str):
    """Verify identity, enforce per-project access, then load the graph."""
    if _verify is None or _check_access is None:  # pragma: no cover
        raise HTTPException(status_code=500, detail="graph router not mounted")

    identity = _verify(request)
    _check_access(identity, project)

    key = registry_key(project, stream)
    loaded = STORE.get(key)
    if loaded is None:
        # 404, not 500: "no graph has been built for this project yet" is an
        # ordinary state, and gives it a dedicated empty state in the UI.
        raise HTTPException(
            status_code=404,
            detail=f"No graph built for {project}/{stream}. Run the graph builder.",
        )
    return loaded


def _envelope(result) -> dict:
    graph, meta = result
    return {"graph": graph, "meta": meta}


@router.get("/layers")
def get_layers(request: Request, project: str, stream: str):
    """L0 — the default view. Subsystem clusters plus aggregated edges."""
    return _envelope(queries.overview(_authorised_graph(request, project, stream)))


@router.get("/layer/{layer_id:path}")
def get_layer(request: Request, layer_id: str, project: str, stream: str):
    """L1 — modules inside one layer."""
    loaded = _authorised_graph(request, project, stream)
    result = queries.layer_detail(loaded, layer_id)
    if result is None:
        raise HTTPException(status_code=404, detail=f"No such layer: {layer_id}")
    return _envelope(result)


@router.get("/module/{module_id:path}")
def get_module(request: Request, module_id: str, project: str, stream: str):
    """L2 — files inside one module."""
    loaded = _authorised_graph(request, project, stream)
    result = queries.module_detail(loaded, module_id)
    if result is None:
        raise HTTPException(status_code=404, detail=f"No such module: {module_id}")
    return _envelope(result)


@router.get("/file/{file_id:path}")
def get_file(request: Request, file_id: str, project: str, stream: str):
    """L3 — symbols declared in one file."""
    loaded = _authorised_graph(request, project, stream)
    result = queries.file_detail(loaded, file_id)
    if result is None:
        raise HTTPException(status_code=404, detail=f"No such file node: {file_id}")
    return _envelope(result)


@router.get("/node/{node_id:path}/neighbourhood")
def get_neighbourhood(
    request: Request,
    node_id: str,
    project: str,
    stream: str,
    hops: int = Query(1, ge=1, le=3),
):
    """L3 focused — used by citation chips that name a node not yet loaded."""
    loaded = _authorised_graph(request, project, stream)
    result = queries.neighbourhood(loaded, node_id, hops)
    if result is None:
        raise HTTPException(status_code=404, detail=f"No such node: {node_id}")
    return _envelope(result)


@router.get("/search")
def get_search(
    request: Request,
    project: str,
    stream: str,
    q: str,
    limit: int = Query(50, ge=1, le=200),
):
    """Search the whole index, not just whatever the client currently holds."""
    loaded = _authorised_graph(request, project, stream)
    return _envelope(queries.search(loaded, q, limit))


@router.get("/meta")
def get_meta(request: Request, project: str, stream: str):
    """Counts, resolution statistics, and project metadata."""
    loaded = _authorised_graph(request, project, stream)
    g = loaded.graph
    return {
        "project": loaded.project_meta,
        "counts": {
            "nodes": len(g.get("nodes", [])),
            "edges": len(g.get("edges", [])),
            "layers": len(g.get("layers", [])),
        },
        "nodeTypes": _count_by(g.get("nodes", []), "type"),
        "edgeTypes": _count_by(g.get("edges", []), "type"),
        # Surfaced in the UI so a user can calibrate how far to trust the edges.
        # A resolution rate nobody can see is a resolution rate nobody accounts for.
        "resolution": loaded.sidecar.get("resolution", {}),
        "budgets": queries.LOD_BUDGETS,
    }


@router.get("/projects")
def get_projects(request: Request):
    """Which project+stream combinations have a built graph."""
    if _verify is None:  # pragma: no cover
        raise HTTPException(status_code=500, detail="graph router not mounted")
    _verify(request)
    return {"graphs": STORE.available()}


def _count_by(items: list[dict], field: str) -> dict[str, int]:
    out: dict[str, int] = {}
    for i in items:
        out[i.get(field, "?")] = out.get(i.get(field, "?"), 0) + 1
    return dict(sorted(out.items(), key=lambda kv: -kv[1]))


# ---------------------------------------------------------------------------
# Compatibility shims — the file names the dashboard already fetches.
#
# Mounted under /api/compat rather than at the web root. Serving them at the root
# means the dev server has to special-case five bare *.json paths ahead of its own
# static handler, and a Vite regex proxy key that fails to match degrades to a
# silent 403 from the fs allow-list — a confusing failure with no server-side trace.
# One /api prefix covers everything and needs no proxy rule of its own.
# ---------------------------------------------------------------------------
compat_router = APIRouter(prefix="/api/compat", tags=["graph-compat"])


@compat_router.get("/knowledge-graph.json")
def compat_graph(request: Request, project: str, stream: str):
    """
    The dashboard's primary fetch. Serves the COMPLETE graph.

    It used to serve the L0 fragment, which was wrong in a way that only a
    screenshot revealed: the dashboard implements its own level-of-detail
    entirely client-side (`drillIntoLayer`, store.ts:487, is a state write with
    no fetch) and recomputes layer-to-layer connections from `graph.edges` on
    every render. Handing it a pre-sliced fragment left it with 26 nodes and no
    edges, so every layer rendered as an unconnected box.

    Returns a BARE graph, not the envelope — this path exists to keep
    `App.tsx`'s existing fetch working unchanged during the port. New code should
    use /api/graph/layers and read `meta`.

    Served pre-compressed when the client accepts it (§P5). 4.6 % of the raw
    size on a real 5,900-node index, computed once per loaded graph rather than per
    request, and scoped to this endpoint so the chat's SSE stream is untouched.
    """
    loaded = _authorised_graph(request, project, stream)
    graph, _meta = queries.full_graph(loaded)

    # A client that does not advertise gzip gets plain JSON. Browsers always
    # advertise it; `curl` without --compressed does not, and handing it a
    # gzip body it never asked for would be a protocol violation, not an
    # optimisation.
    if "gzip" not in request.headers.get("accept-encoding", "").lower():
        return graph

    return Response(
        content=loaded.gzipped(f"full:{project}:{stream}", graph),
        media_type="application/json",
        headers={"Content-Encoding": "gzip", "Vary": "Accept-Encoding"},
    )


@compat_router.get("/meta.json")
def compat_meta(request: Request, project: str, stream: str):
    loaded = _authorised_graph(request, project, stream)
    return {"project": loaded.project_meta}


@compat_router.get("/config.json")
def compat_config(request: Request):
    if _verify is None:  # pragma: no cover
        raise HTTPException(status_code=500, detail="graph router not mounted")
    _verify(request)
    return {"autoUpdate": False, "outputLanguage": "en"}


# The Vite middleware this replaces refused anything over 1 MB; keep that
# ceiling, because it is a *preview* pane and a megabyte of one file is already
# past the point where showing more helps anybody.
VIEWER_MAX_BYTES = 1024 * 1024
# `read_source_excerpt` defaults to MAX_FILE_READ_LINES (500) when end_line is
# unset — right for an agent quoting an excerpt, silently wrong for a viewer
# that is supposed to show the file. Ask for the whole thing explicitly.
VIEWER_MAX_LINES = 1_000_000

_LANGUAGE_BY_EXT = {
    "bash": "bash", "c": "c", "cc": "cpp", "cpp": "cpp", "cs": "csharp",
    "css": "css", "go": "go", "h": "c", "hpp": "cpp", "html": "markup",
    "java": "java", "js": "javascript", "json": "json", "jsx": "jsx",
    "kt": "kotlin", "md": "markdown", "mjs": "javascript", "pl": "perl",
    "proto": "protobuf", "py": "python", "rb": "ruby", "rs": "rust",
    "scala": "scala", "sh": "bash", "sql": "sql", "ts": "typescript",
    "tsx": "tsx", "txt": "text", "yaml": "yaml", "yml": "yaml",
}


@compat_router.get("/file-content.json")
def compat_file_content(request: Request, project: str, stream: str, path: str):
    """
    Source for the code viewer.

    Upstream served this from the Vite dev server's own middleware, gated on a
    one-time process token and reading `.insight/knowledge-graph.json`. Neither
    survives here: the client authenticates with the backend's session token,
    and graphs live in GRAPH_DIR as `{project}_{stream}.json`. So the viewer
    could not load a file under ANY circumstances — 403 on the token mismatch,
    and 404 on the graph lookup even when handed the right token. `dataUrl` in
    App.tsx moved every other fetch to /api/compat; this one was left behind.

    Reads through the same `read_source_excerpt` the advisor uses, so the path
    guards — confined to CODEBASE_DIR, no traversal — are the ones already
    exercised by /api/tools rather than a second copy free to drift out of step.
    """
    # Identity and per-project access, and 404 for a project with no graph —
    # the same gate every other /api/graph endpoint runs.
    _authorised_graph(request, project, stream)

    from ..corpus import read_source_excerpt

    result = read_source_excerpt(registry_key(project, stream), path, 1, VIEWER_MAX_LINES)
    if result.get("error"):
        raise HTTPException(status_code=404, detail=result["error"])

    content = result.get("content", "")
    size = len(content.encode("utf-8"))
    if size > VIEWER_MAX_BYTES:
        raise HTTPException(status_code=413, detail="File is too large to preview")

    ext = path.rsplit(".", 1)[-1].lower() if "." in path else ""
    total = result.get("total_lines_in_file", 0)
    returned = len(content.splitlines()) if content else 0
    return {
        "path": path,
        "language": _LANGUAGE_BY_EXT.get(ext, "text"),
        "content": content,
        "sizeBytes": size,
        "lineCount": returned,
        # Published rather than hidden: a viewer that quietly drops the tail of
        # a file is worse than one that says it did.
        "truncated": total > returned,
        "totalLines": total,
    }
