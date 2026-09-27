"""
Praxevia Explorer as MCP tools — the whole product, from a terminal agent.

Claude Code (or any MCP harness) talks to a running Praxevia Explorer backend
over HTTP and gets the three surfaces the dashboard has: the search index, the
architecture graph, and the Insight Advisor itself.

    PRAXEVIA_BACKEND_URL   required, e.g. http://127.0.0.1:3006
    PRAXEVIA_API_KEY       required, Bearer token — a key from the dashboard's
                           API-keys panel, a static key from API_KEYS, or the
                           session token returned by POST /api/login
    PRAXEVIA_PROJECT       optional default project, e.g. "myproject"
    PRAXEVIA_STREAM        optional default stream, e.g. "main"
    PRAXEVIA_CROSS_PROJECT optional, "true" searches every permitted project
    PRAXEVIA_VERIFY_SSL    optional, "false" only for a self-signed dev cert
    PRAXEVIA_TIMEOUT       optional seconds for ordinary calls (default 120)
    PRAXEVIA_CHAT_TIMEOUT  optional seconds for ask_codebase (default 600)
    PRAXEVIA_MAX_NODES     optional cap on nodes returned per graph call (200)

WHAT IS AND IS NOT EXPOSED. The eight indexed-search tools the chat agent itself
uses, the read-only graph endpoints, and the chat. Deliberately absent:
everything under /api/admin/* — stored credentials, access grants, reindex
triggers — and /api/api-keys. An agent in a terminal is the wrong place to
administer a deployment from, and a token scoped for reading should not become
one that can grant itself more.

PROJECT AND STREAM ARE PER CALL, with the environment as the default. One
deployment holds many indexed repositories; a server that can only ever see the
one named at startup makes the harness restart to change subject.

NOTHING IS EVER ADDRESSED BY REGISTRY KEY. `{project}_{stream}` is assembled by
each endpoint from its own arguments, and the two halves of the backend sanitise
differently — /api/tools with `[^a-z0-9]`, the graph router with `[^a-z0-9]+`,
which disagree on repeated separators. Sending project and stream and letting
each side key itself is the only way both stay correct. `catalog_projects` does
compare the two sides' keys, which is best-effort for exactly that reason, and
says so where it does it.

RESULTS ARE COMPACTED, NOT TRUNCATED SILENTLY. A level-3 graph fragment is up to
800 nodes, and pasting that verbatim into an agent's context buys nothing over
the fields it can act on. Nodes keep every field the graph carries; edges become
`source -type-> target`; and when this tool drops nodes past `max_nodes`, the
count it dropped is in the response next to the server's own `totalAvailable`.

CITATION MARKERS ARE LEFT ALONE. The advisor cites code as `[[path]]` or
`[[path:symbol]]`, which the dashboard turns into clickable chips by resolving
them against the loaded graph. Here they stay as written — `[[backend/app/
corpus/search.py:search_by_meaning]]` is already a precise reference to an agent holding a
filesystem, and a second resolver is a second thing to keep in step with the
graph's node ids.
"""

from __future__ import annotations

import json
import os
import sys
from typing import Any
from urllib.parse import quote

import httpx
from mcp.server.mcpserver import MCPServer

BACKEND_URL = os.environ.get("PRAXEVIA_BACKEND_URL", "").rstrip("/")
API_KEY = os.environ.get("PRAXEVIA_API_KEY", "")
DEFAULT_PROJECT = os.environ.get("PRAXEVIA_PROJECT", "")
DEFAULT_STREAM = os.environ.get("PRAXEVIA_STREAM", "")
CROSS_PROJECT = os.environ.get("PRAXEVIA_CROSS_PROJECT", "false").lower() == "true"
# Default ON. The value this replaced was a hardcoded verify=False on every
# call, which silently accepts any certificate for the life of the process.
VERIFY_SSL = os.environ.get("PRAXEVIA_VERIFY_SSL", "true").lower() not in (
    "false", "0", "no",
)
TIMEOUT = float(os.environ.get("PRAXEVIA_TIMEOUT", "120"))
CHAT_TIMEOUT = float(os.environ.get("PRAXEVIA_CHAT_TIMEOUT", "600"))
MAX_NODES = int(os.environ.get("PRAXEVIA_MAX_NODES", "200"))

mcp = MCPServer("praxevia")


# ── plumbing ────────────────────────────────────────────────────────────────
#
# Every tool body below is a thin wrapper over one of these. They are ordinary
# functions rather than logic inside the decorated tools so that selftest.py can
# exercise them directly, and so a failure has one place to look.


def _require_config() -> None:
    if not BACKEND_URL or not API_KEY:
        raise RuntimeError(
            "PRAXEVIA_BACKEND_URL and PRAXEVIA_API_KEY must both be set "
            "(see mcp-server/README.md)."
        )


def _headers() -> dict[str, str]:
    return {"Authorization": f"Bearer {API_KEY}"}


def _timeout(read_seconds: float) -> httpx.Timeout:
    """
    Split, not one number for everything.

    A single `timeout=` applies to the connect as well, so any value large
    enough for an agentic answer is also how long a wrong hostname takes to
    fail.
    """
    return httpx.Timeout(connect=10.0, read=read_seconds, write=30.0, pool=10.0)


def _scope(project: str, stream: str) -> tuple[str, str]:
    """Resolve project/stream: the call's arguments first, then the environment."""
    resolved_project = (project or DEFAULT_PROJECT).strip()
    resolved_stream = (stream or DEFAULT_STREAM).strip()
    if not resolved_project:
        raise ValueError(
            "No project given and PRAXEVIA_PROJECT is not set. "
            "Call catalog_projects to see what this deployment holds, then pass "
            "project=… (and stream=…)."
        )
    if not resolved_stream:
        raise ValueError(
            "No stream given and PRAXEVIA_STREAM is not set. A project is "
            "indexed per stream — catalog_projects shows which streams exist."
        )
    return resolved_project, resolved_stream


def _explain(resp: httpx.Response) -> str:
    """Turn an HTTP failure into something the calling agent can act on."""
    try:
        detail = resp.json().get("detail", "")
    except Exception:  # noqa: BLE001 — a non-JSON error body is still an error
        detail = resp.text[:300]
    if resp.status_code == 401:
        return "Praxevia Explorer rejected the token (401). Check PRAXEVIA_API_KEY."
    if resp.status_code == 403:
        return f"No access to that project (403). {detail}"
    if resp.status_code == 404:
        return f"Not found (404). {detail}"
    return f"Praxevia Explorer returned {resp.status_code}. {detail}"


async def _request(
    method: str,
    path: str,
    *,
    params: dict | None = None,
    body: dict | None = None,
    read_timeout: float | None = None,
) -> Any:
    _require_config()
    async with httpx.AsyncClient(
        verify=VERIFY_SSL, timeout=_timeout(read_timeout or TIMEOUT)
    ) as client:
        resp = await client.request(
            method, f"{BACKEND_URL}{path}", params=params, json=body,
            headers=_headers(),
        )
    if resp.status_code >= 400:
        raise RuntimeError(_explain(resp))
    return resp.json()


def _dumps(payload: Any) -> str:
    return json.dumps(payload, indent=2, ensure_ascii=False)


async def _call_index_tool(
    tool_name: str, tool_input: dict, project: str, stream: str
) -> str:
    """Run one of the backend's indexed-search tools and return its JSON."""
    if CROSS_PROJECT:
        payload = {"tool_name": tool_name, "tool_input": tool_input,
                   "project": "", "stream": "", "cross_project": True}
    else:
        resolved_project, resolved_stream = _scope(project, stream)
        payload = {"tool_name": tool_name, "tool_input": tool_input,
                   "project": resolved_project, "stream": resolved_stream,
                   "cross_project": False}
    return _dumps(await _request("POST", "/api/tools", body=payload))


# ── graph shaping ───────────────────────────────────────────────────────────


def _compact_edges(edges: list[dict]) -> list[str]:
    """`source -type-> target`. The type is the part worth spending context on."""
    return [f"{e['source']} -{e.get('type', '?')}-> {e['target']}" for e in edges]


def _compact_node(node: dict) -> dict:
    """
    Everything the graph knows, minus the fields carrying no information.

    Not a whitelist: the node schema passes unknown keys through on purpose, so
    a field added by a future extractor — the documentation sections and table
    columns folded onto their parent are the current examples — reaches the
    agent without this file being edited to let it.
    """
    return {k: v for k, v in node.items() if v not in ([], "", None, {})}


def _compact_fragment(envelope: dict, max_nodes: int) -> dict:
    """Reshape a {graph, meta} response into something worth reading."""
    graph = envelope.get("graph", {})
    meta = envelope.get("meta", {})
    nodes = graph.get("nodes", [])
    kept = nodes[:max_nodes]
    kept_ids = {n["id"] for n in kept}

    out: dict[str, Any] = {
        "project": graph.get("project", {}),
        "levelOfDetail": meta.get("lod", ""),
        "returnedByServer": meta.get("returned", len(nodes)),
        "totalAvailable": meta.get("totalAvailable", len(nodes)),
        "truncatedByServer": meta.get("truncated", False),
        "layers": [
            {"id": l["id"], "name": l.get("name", ""),
             "nodes": len(l.get("nodeIds", []))}
            for l in graph.get("layers", [])
        ],
        "nodes": [_compact_node(n) for n in kept],
        # An edge whose other end was dropped would name a node that is not in
        # the response — confusing rather than useful, so it goes with it.
        "edges": _compact_edges([
            e for e in graph.get("edges", [])
            if e["source"] in kept_ids and e["target"] in kept_ids
        ]),
    }
    if len(nodes) > len(kept):
        out["omittedByMaxNodes"] = len(nodes) - len(kept)
        out["hint"] = (
            f"{len(nodes) - len(kept)} more nodes were returned by the server "
            f"and dropped here. Raise max_nodes, or narrow with graph_search."
        )
    return out


# ── discovery ───────────────────────────────────────────────────────────────


@mcp.tool()
async def catalog_projects() -> str:
    """
    List every project and stream on this Praxevia Explorer, and what each has.

    Two things are indexed separately and can disagree, so both are reported:
    `indexed` (the Postgres search index, which backs the search tools and the
    chat) and `hasGraph` (a graph has been built, which the graph tools need).
    Indexed with no graph yet is an ordinary state — the graph tools report
    not-found for it until the graph builder has run.
    """
    _require_config()
    chat_side = await _request("GET", "/api/projects")
    try:
        graph_side = await _request("GET", "/api/graph/projects")
        built = set(graph_side.get("graphs", []))
    except RuntimeError:
        # The backend mounts the graph API defensively and runs without it.
        # That must not take the project list down with it.
        built = set()

    projects = []
    seen = set()
    for entry in chat_side.get("projects", []):
        key = entry.get("registry_key", "")
        seen.add(key)
        projects.append({
            "project": entry.get("name", ""),
            "stream": entry.get("stream", ""),
            "indexed": entry.get("index_count", 0) > 0,
            "chunks": entry.get("index_count", 0),
            "hasGraph": key in built,
            "hasSource": entry.get("has_codebase", False),
            "hasAccess": entry.get("has_access", True),
        })

    # A graph can exist for something the chat registry has never seen — the
    # index was dropped, or the graph was built elsewhere and copied in.
    #
    # BEST-EFFORT, and the only key comparison in this file. The two sides
    # sanitise a project name differently (see the module docstring), so a name
    # containing repeated separators can appear here as an extra row as well as
    # a `hasGraph: false` on its real one. Both rows are still true statements
    # about a key that exists; neither is used to address anything.
    for key in sorted(built - seen):
        projects.append({
            "registryKey": key,
            "indexed": False,
            "hasGraph": True,
            "note": "A graph exists but nothing is indexed under this key. "
                    "The graph tools work; search and chat do not.",
        })

    return _dumps({
        "projects": projects,
        "defaults": {"project": DEFAULT_PROJECT or None,
                     "stream": DEFAULT_STREAM or None},
        "crossProject": CROSS_PROJECT,
    })


# ── the architecture graph ──────────────────────────────────────────────────


@mcp.tool()
async def graph_overview(project: str = "", stream: str = "") -> str:
    """
    What this codebase is: size, composition, and where its code came from.

    Returns node and edge counts, the breakdown by node type and edge type, the
    languages detected, the commit the graph was built from, and the repository
    URL or path the source was taken from.

    Also returns edge-resolution statistics. Call resolution is ambiguous in
    real code, and Praxevia Explorer withholds the edges it cannot justify
    rather than guessing; those numbers say how much of the call graph is
    actually drawn, and are worth reading before treating a missing edge as
    evidence that nothing calls something.
    """
    resolved_project, resolved_stream = _scope(project, stream)
    params = {"project": resolved_project, "stream": resolved_stream}
    meta = await _request("GET", "/api/graph/meta", params=params)

    # Where the code came from is the one thing a reader always wants and the
    # graph itself does not record — it lives with the project's source row.
    try:
        source = await _request("GET", "/api/graph/source", params=params)
    except RuntimeError:
        source = {}

    return _dumps({
        "project": meta.get("project", {}),
        "source": {
            "location": source.get("path", ""),
            "kind": source.get("kind", ""),
            "status": source.get("status", ""),
            "recorded": source.get("recorded", False),
        },
        "counts": meta.get("counts", {}),
        "nodeTypes": meta.get("nodeTypes", {}),
        "edgeTypes": meta.get("edgeTypes", {}),
        "edgeResolution": meta.get("resolution", {}),
    })


@mcp.tool()
async def graph_expand(
    node_id: str = "",
    hops: int = 1,
    max_nodes: int = 0,
    project: str = "",
    stream: str = "",
) -> str:
    """
    Walk the architecture graph. One tool for every level of it.

    node_id decides what you get, by its prefix:
      ""                 the top level — the layers this codebase divides into
      "layer:<name>"     the modules in that layer
      "module:<path>"    the files in that module
      "file:<path>"      the symbols declared in that file
      anything else      the neighbourhood of that node: what it calls, what
                         calls it, what contains it, out to `hops` (1-3)

    Node ids are stable across re-indexing and carry no line numbers, so an id
    from one call stays valid in the next one and in a citation.

    Nodes come back with everything the graph knows about them, which is where
    the documentation lives: `summary` is the symbol's own doc comment, and
    `sections`, `columns` and `fields` carry the headings, table columns and
    message fields that are folded onto their parent rather than exploded into
    nodes of their own.

    hops applies to the neighbourhood form only. max_nodes defaults to
    PRAXEVIA_MAX_NODES.
    """
    resolved_project, resolved_stream = _scope(project, stream)
    params: dict[str, Any] = {"project": resolved_project,
                              "stream": resolved_stream}
    cap = max_nodes if max_nodes > 0 else MAX_NODES
    ident = quote(node_id, safe="/")  # ids hold slashes; the routes take them raw

    if not node_id:
        path = "/api/graph/layers"
    elif node_id.startswith("layer:"):
        path = f"/api/graph/layer/{ident}"
    elif node_id.startswith("module:"):
        path = f"/api/graph/module/{ident}"
    elif node_id.startswith("file:"):
        path = f"/api/graph/file/{ident}"
    else:
        path = f"/api/graph/node/{ident}/neighbourhood"
        params["hops"] = max(1, min(hops, 3))

    try:
        envelope = await _request("GET", path, params=params)
    except RuntimeError as first_error:
        if not node_id or "404" not in str(first_error):
            raise
        # A container with no detail view of its own — a module holding no files
        # directly, a file node that only ever appears as a target — is still a
        # real node with real edges. Falling back beats reporting "not found"
        # for something the graph plainly contains.
        params["hops"] = max(1, min(hops, 3))
        envelope = await _request(
            "GET", f"/api/graph/node/{ident}/neighbourhood", params=params
        )

    result = _compact_fragment(envelope, cap)
    if node_id:
        focus = next(
            (n for n in envelope.get("graph", {}).get("nodes", [])
             if n["id"] == node_id),
            None,
        )
        if focus:
            result["focus"] = _compact_node(focus)
    return _dumps(result)


@mcp.tool()
async def graph_search(
    query: str,
    limit: int = 50,
    max_nodes: int = 0,
    project: str = "",
    stream: str = "",
) -> str:
    """
    Find nodes by name or path anywhere in the architecture graph.

    Matches node names and file paths, exact name matches first. Use it to turn
    a name you have into node ids you can pass to graph_expand — it searches the
    whole graph, not just some previously loaded part of it.

    For "which code does X" rather than "where is the thing called X", use
    search_by_meaning: this is a substring match, not a match on meaning.
    """
    resolved_project, resolved_stream = _scope(project, stream)
    envelope = await _request("GET", "/api/graph/search", params={
        "project": resolved_project, "stream": resolved_stream,
        "q": query, "limit": max(1, min(limit, 200)),
    })
    return _dumps(_compact_fragment(
        envelope, max_nodes if max_nodes > 0 else MAX_NODES
    ))


# ── the Insight Advisor ─────────────────────────────────────────────────────


@mcp.tool()
async def ask_codebase(
    question: str,
    project: str = "",
    stream: str = "",
    reasoning_mode: str = "low-level",
    model: str = "",
    timeout_s: int = 0,
) -> str:
    """
    Ask Praxevia Explorer's Insight Advisor about the codebase.

    The advisor is agentic: it runs its own searches over the index — semantic
    search, grep, symbol lookup, call tracing — and answers from what it finds.
    Worth using over the individual search tools when a question needs several
    searches whose shape you would otherwise have to work out yourself, and when
    a synthesised answer beats raw results. It can take a minute or more,
    because it really is running those searches.

    reasoning_mode: "low-level" for code detail, "high-level" for architecture.
    model: leave empty for the backend's configured default.

    Answers cite code as [[path]] or [[path:symbol]], left exactly as written —
    each one names a real file, and a symbol within it where it has one.
    """
    body: dict[str, Any] = {
        "message": question,
        # No history. The harness holding this conversation is the thread; a
        # second one kept here would drift from it, and be the one that is wrong.
        "history": [],
        "model": model,
        "diagram_mode": "mermaid",
        "reasoning_mode": reasoning_mode,
        "user_role": "general",
    }
    if CROSS_PROJECT:
        body.update({"project": "", "stream": "", "cross_project": True})
    else:
        resolved_project, resolved_stream = _scope(project, stream)
        body.update({"project": resolved_project, "stream": resolved_stream,
                     "cross_project": False})

    _require_config()
    answer: list[str] = []
    tools_used: list[str] = []
    usage: dict = {}
    used_model = ""

    async with httpx.AsyncClient(
        verify=VERIFY_SSL, timeout=_timeout(float(timeout_s or CHAT_TIMEOUT))
    ) as client:
        async with client.stream(
            "POST", f"{BACKEND_URL}/api/chat", json=body, headers=_headers()
        ) as resp:
            if resp.status_code >= 400:
                await resp.aread()
                raise RuntimeError(_explain(resp))
            async for line in resp.aiter_lines():
                # Heartbeats are SSE comments (": keepalive"), not data frames.
                # Anything that is not a data frame is not ours to parse.
                if not line.startswith("data:"):
                    continue
                try:
                    event = json.loads(line[5:].strip())
                except json.JSONDecodeError:
                    continue
                kind, data = event.get("type"), event.get("data", {})
                if kind == "content_delta":
                    answer.append(data.get("text", ""))
                elif kind == "tool_use":
                    tools_used.append(data.get("tool_name", "?"))
                elif kind == "message_start":
                    used_model = data.get("model", "")
                elif kind == "message_end":
                    usage = data.get("usage", {})
                elif kind == "error":
                    # These arrive INSIDE a 200 response: the backend catches the
                    # exception and yields it as an event, so the status code is
                    # no signal at all. Without this the tool would return an
                    # empty answer and report success.
                    raise RuntimeError(
                        "Insight Advisor failed: "
                        f"{data.get('message', 'unknown error')}"
                    )

    counts: dict[str, int] = {}
    for name in tools_used:
        counts[name] = counts.get(name, 0) + 1

    return _dumps({
        "answer": "".join(answer),
        "model": used_model,
        "searchesRun": counts,
        "usage": usage,
    })


# ── indexed search ──────────────────────────────────────────────────────────
#
# The same eight tools the in-app assistant runs for itself. Reach for these when
# you already know the shape of the search you want; reach for ask_codebase when
# you do not.
#
# Parameter names here are this server's own vocabulary. The dict keys passed to
# _call_index_tool are the backend's wire contract and are deliberately NOT the
# same strings -- renaming a parameter is a local change, renaming a key is a
# protocol change. `project` and `stream` keep their names throughout because
# they are product vocabulary shared with the REST API and the dashboard.


@mcp.tool()
async def browse_source_tree(
    subdir: str = "", levels: int = 3, project: str = "", stream: str = ""
) -> str:
    """
    List the directory tree of the indexed source.

    A good first call for "how is this laid out". subdir is relative to the
    source root (empty for the root); levels is how far to expand (1-4).
    """
    return await _call_index_tool(
        "browse_source_tree", {"path": subdir, "depth": levels}, project, stream
    )


@mcp.tool()
async def search_by_meaning(
    description: str, limit: int = 20, project: str = "", stream: str = ""
) -> str:
    """
    Search the codebase by meaning, using vector embeddings over indexed chunks.

    Describe what the code does -- "where retries are configured", "how sessions
    expire" -- rather than naming an identifier. For an exact name,
    regex_search_source or lookup_symbol_usage is faster and more precise.
    """
    return await _call_index_tool(
        "search_by_meaning", {"query": description, "max_results": limit},
        project, stream,
    )


@mcp.tool()
async def find_files_by_name(
    name_glob: str,
    path_substring: str = "",
    limit: int = 50,
    project: str = "",
    stream: str = "",
) -> str:
    """
    Find files by filename pattern, e.g. "main.py", "*.sql", "*Test.java".

    path_substring further narrows to paths containing that text.
    """
    return await _call_index_tool(
        "find_files_by_name",
        {"filename_pattern": name_glob, "path_contains": path_substring,
         "max_results": limit},
        project, stream,
    )


@mcp.tool()
async def read_source_excerpt(
    path: str,
    from_line: int = 1,
    to_line: int = 0,
    subprogram: str = "",
    project: str = "",
    stream: str = "",
) -> str:
    """
    Read a file from the indexed source, up to 500 lines per call.

    path is relative to the source root. Give from_line/to_line for a range, or
    subprogram to pull out a single named routine instead of a line range.
    """
    return await _call_index_tool(
        "read_source_excerpt",
        {"file_path": path, "start_line": from_line,
         "end_line": to_line, "procedure_name": subprogram},
        project, stream,
    )


@mcp.tool()
async def regex_search_source(
    regex: str,
    extension: str = "",
    path_substring: str = "",
    limit: int = 200,
    context: int = 0,
    project: str = "",
    stream: str = "",
) -> str:
    """
    Regex search across the real source files, case-insensitive.

    regex is a Python regular expression. extension limits to one file type
    (".py", ".java"); path_substring to paths containing that text; context adds
    surrounding lines the way grep -C does.
    """
    return await _call_index_tool(
        "regex_search_source",
        {"pattern": regex, "file_extension": extension,
         "path_contains": path_substring, "max_results": limit,
         "context_lines": context},
        project, stream,
    )


@mcp.tool()
async def lookup_symbol_usage(
    symbol: str, symbol_kind: str = "", project: str = "", stream: str = ""
) -> str:
    """
    Where a symbol is defined, who calls it, and what it calls -- one hop.

    symbol accepts a bare or a qualified name ("save", "UserService.save").
    symbol_kind optionally narrows: function, procedure, class, method, package.
    For chains longer than one hop use walk_call_chain rather than calling this
    repeatedly.
    """
    return await _call_index_tool(
        "lookup_symbol_usage", {"symbol_name": symbol, "kind": symbol_kind},
        project, stream,
    )


@mcp.tool()
async def walk_call_chain(
    symbol: str,
    direction: str = "callers",
    depth_limit: int = 6,
    path_filter: str = "",
    limit: int = 500,
    project: str = "",
    stream: str = "",
) -> str:
    """
    Follow call chains to or from a symbol across many hops.

    direction: "callers" (what reaches this), "callees" (what this reaches), or
    "both". depth_limit is the hop ceiling (1-10). path_filter is a SQL ILIKE
    pattern on file paths, e.g. "%auth%". Answers "what breaks if I change this"
    in a single call.
    """
    return await _call_index_tool(
        "walk_call_chain",
        {"symbol_name": symbol, "direction": direction,
         "max_depth": depth_limit, "file_filter": path_filter,
         "max_results": limit},
        project, stream,
    )


@mcp.tool()
async def find_subsystem_dependencies(
    subsystem_path: str, project: str = "", stream: str = ""
) -> str:
    """
    Cross-module dependencies for one subsystem directory, grouped by module.

    subsystem_path is a directory relative to the source root, e.g.
    "backend/api". Returns what it depends on and what depends on it -- the
    coupling of one subsystem, for a dependency diagram or an impact assessment.
    """
    return await _call_index_tool(
        "find_subsystem_dependencies", {"module_path": subsystem_path},
        project, stream,
    )


if __name__ == "__main__":
    # Checked eagerly as well as per call: a misconfigured server that starts and
    # then fails every tool is diagnosed by reading the harness's logs, while one
    # that refuses to start says so where it was registered.
    if not BACKEND_URL or not API_KEY:
        print(
            "ERROR: PRAXEVIA_BACKEND_URL and PRAXEVIA_API_KEY must both be set.\n"
            "See mcp-server/README.md for the whole configuration.",
            file=sys.stderr,
        )
        sys.exit(1)
    mcp.run()
