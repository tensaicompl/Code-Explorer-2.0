"""
The MCP server, exercised over MCP — not by importing its functions.

    .venv/bin/python mcp-server/selftest.py

Runs against a Praxevia Explorer backend you already have running, obtaining a
token the same way any other client would. It launches the server as a
subprocess and speaks the protocol to it, because that is the only path that
also covers the parts a direct import skips: the process starting at all, the
tool schemas FastMCP derives from the signatures, and the transport.

    PRAXEVIA_BACKEND_URL   default http://127.0.0.1:<E2E_BACKEND_PORT or 8099>
    PRAXEVIA_API_KEY       optional; without it, LOGIN_USERNAME/LOGIN_PASSWORD
                           are exchanged for a session token via /api/login
    GRAPH_DIR              where built graphs live — the test subject is
                           discovered from it, never named here
    PRAXEVIA_SELFTEST_CHAT set to 1 to include ask_codebase, which spends real
                           tokens on whichever provider the backend is
                           configured with; skipped by default and reported as
                           skipped, not as passed

`e2e/run.py` can start a backend for it — `e2e/run.py mcp-server/selftest.py` —
but it also starts the dashboard and refuses to run while port 5173 is taken,
which is the usual state on a machine where the app is up. Against a backend
that is already running, invoke this directly.
"""

from __future__ import annotations

import asyncio
import json
import os
import sys

import httpx
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
SERVER = os.path.join(HERE, "praxevia_mcp_server.py")
GRAPH_DIR = os.environ.get("GRAPH_DIR", f"{REPO}/.graphs")
BACKEND_URL = os.environ.get(
    "PRAXEVIA_BACKEND_URL",
    f"http://127.0.0.1:{os.environ.get('E2E_BACKEND_PORT', '8099')}",
).rstrip("/")

EXPECTED_TOOLS = {
    "catalog_projects", "graph_overview", "graph_expand", "graph_search",
    "ask_codebase", "browse_source_tree", "search_by_meaning", "find_files_by_name",
    "read_source_excerpt", "regex_search_source", "lookup_symbol_usage",
    "walk_call_chain", "find_subsystem_dependencies",
}

failures: list[str] = []
skipped: list[str] = []


def check(ok: bool, label: str, detail: str = "") -> None:
    print(f"  {'PASS' if ok else 'FAIL'}  {label}{'   ' + detail if detail else ''}")
    if not ok:
        failures.append(f"{label} {detail}".strip())


def skip(label: str, why: str) -> None:
    print(f"  SKIP  {label}   {why}")
    skipped.append(f"{label} — {why}")


def discover_project(graph_dir: str) -> tuple[str, str] | None:
    """
    The smallest built graph, as (project, stream).

    Never a hardcoded name: naming a project leaks whatever the author happened
    to be indexing and covers nothing on anyone else's machine. Filenames are
    `{project}_{stream}.json`, which is how graph_pass.builder composes the key.
    """
    if not os.path.isdir(graph_dir):
        return None
    found = []
    for name in sorted(os.listdir(graph_dir)):
        if not name.endswith(".json") or name.endswith(".meta.json"):
            continue
        stem = name[:-5]
        if "_" not in stem:
            continue
        found.append((*stem.rsplit("_", 1),
                      os.path.getsize(os.path.join(graph_dir, name))))
    if not found:
        return None
    project, stream, _size = min(found, key=lambda f: f[2])
    return project, stream


def get_token() -> str:
    key = os.environ.get("PRAXEVIA_API_KEY", "")
    if key:
        return key
    username = os.environ.get("LOGIN_USERNAME", "")
    password = os.environ.get("LOGIN_PASSWORD", "")
    if not username or not password:
        sys.exit("Set PRAXEVIA_API_KEY, or LOGIN_USERNAME and LOGIN_PASSWORD "
                 "for this backend.")
    resp = httpx.post(f"{BACKEND_URL}/api/login",
                      json={"username": username, "password": password},
                      timeout=15.0)
    if resp.status_code != 200:
        sys.exit(f"Could not sign in to {BACKEND_URL}: "
                 f"{resp.status_code} {resp.text[:200]}")
    return resp.json()["token"]


async def call(session: ClientSession, tool: str, args: dict) -> tuple[bool, str]:
    """Return (ok, text). A tool error is a result, not an exception."""
    result = await session.call_tool(tool, args)
    text = "".join(getattr(c, "text", "") for c in result.content)
    return not result.is_error, text


async def main() -> int:
    project_stream = discover_project(GRAPH_DIR)
    if project_stream is None:
        sys.exit(f"No graphs in {GRAPH_DIR} — build one before running this.")
    project, stream = project_stream
    scope = {"project": project, "stream": stream}

    print(f"\nbackend: {BACKEND_URL}")
    print(f"subject: {project}/{stream}\n")

    params = StdioServerParameters(
        command=sys.executable,
        args=[SERVER],
        # Deliberately WITHOUT PRAXEVIA_PROJECT/PRAXEVIA_STREAM: every call below
        # passes its own scope, which is the behaviour that replaced the old
        # server's single hardcoded project.
        env={
            "PATH": os.environ.get("PATH", ""),
            "PRAXEVIA_BACKEND_URL": BACKEND_URL,
            "PRAXEVIA_API_KEY": get_token(),
        },
    )

    async with stdio_client(params) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()

            listed = await session.list_tools()
            names = {t.name for t in listed.tools}
            check(names == EXPECTED_TOOLS, "every tool is exposed, and no others",
                  f"missing={sorted(EXPECTED_TOOLS - names)} "
                  f"extra={sorted(names - EXPECTED_TOOLS)}"
                  if names != EXPECTED_TOOLS else "")
            described = [t.name for t in listed.tools if not (t.description or "").strip()]
            check(not described, "every tool carries a description", str(described))

            # ── discovery ───────────────────────────────────────────────────
            ok, text = await call(session, "catalog_projects", {})
            check(ok, "catalog_projects succeeds", text[:200] if not ok else "")
            if ok:
                data = json.loads(text)
                mine = [p for p in data["projects"]
                        if p.get("project") == project and p.get("stream") == stream]
                check(bool(mine), f"catalog_projects finds {project}/{stream}")
                if mine:
                    check(mine[0]["hasGraph"] is True,
                          "catalog_projects reports the built graph",
                          json.dumps(mine[0]))
                    check("indexed" in mine[0],
                          "catalog_projects separates indexed from hasGraph")

            # ── the graph ───────────────────────────────────────────────────
            ok, text = await call(session, "graph_overview", scope)
            check(ok, "graph_overview succeeds", text[:200] if not ok else "")
            if ok:
                data = json.loads(text)
                check(data["counts"].get("nodes", 0) > 0,
                      "graph_overview reports node counts", json.dumps(data["counts"]))
                check(bool(data["project"].get("name")),
                      "graph_overview names the project", str(data["project"].get("name")))
                check("location" in data["source"],
                      "graph_overview says where the code came from",
                      json.dumps(data["source"]))
                check("edgeResolution" in data,
                      "graph_overview publishes edge-resolution statistics")

            ok, text = await call(session, "graph_expand", scope)
            check(ok, "graph_expand('') returns the top level",
                  text[:200] if not ok else "")
            layer_id = ""
            if ok:
                top = json.loads(text)
                check(bool(top["layers"]), "the top level has layers")
                check(top["levelOfDetail"] == "L0", "the top level is L0",
                      top["levelOfDetail"])
                if top["layers"]:
                    layer_id = top["layers"][0]["id"]

            if layer_id:
                ok, text = await call(session, "graph_expand",
                                      {**scope, "node_id": layer_id})
                check(ok, f"graph_expand descends into {layer_id}",
                      text[:200] if not ok else "")
                if ok:
                    check(bool(json.loads(text)["nodes"]),
                          "a layer expands to nodes")

            # A file node, then a symbol inside it: the two remaining routes.
            ok, text = await call(session, "graph_search",
                                  {**scope, "query": ".", "limit": 200})
            check(ok, "graph_search succeeds", text[:200] if not ok else "")
            file_id = symbol_id = ""
            if ok:
                hits = json.loads(text)["nodes"]
                check(bool(hits), "graph_search returns nodes")
                file_id = next((n["id"] for n in hits
                                if n["id"].startswith("file:")), "")
                symbol_id = next((n["id"] for n in hits
                                  if not n["id"].startswith(("file:", "module:",
                                                             "layer:"))), "")

            if file_id:
                ok, text = await call(session, "graph_expand",
                                      {**scope, "node_id": file_id})
                check(ok, "graph_expand opens a file node",
                      text[:200] if not ok else "")

            if symbol_id:
                ok, text = await call(session, "graph_expand",
                                      {**scope, "node_id": symbol_id, "hops": 2})
                check(ok, "graph_expand walks a symbol's neighbourhood",
                      text[:200] if not ok else "")
                if ok:
                    data = json.loads(text)
                    check(data.get("focus", {}).get("id") == symbol_id,
                          "the neighbourhood identifies its focus node",
                          str(data.get("focus", {}).get("id")))
                    # The documentation an agent came for. Not every node has a
                    # doc comment, so this asserts the fields survive the
                    # compaction, not that this symbol happens to be documented.
                    check(set(data["focus"]) >= {"id", "type", "name"},
                          "focus nodes keep their fields",
                          json.dumps(data["focus"])[:200])
                    check(all("-> " in e for e in data["edges"]),
                          "edges render as source -type-> target",
                          str(data["edges"][:2]))

            # ── the errors an agent will actually hit ───────────────────────
            ok, text = await call(session, "graph_overview", {})
            check(not ok and "catalog_projects" in text,
                  "no project and no default says how to find one", text[:160])

            ok, text = await call(session, "graph_overview",
                                  {"project": "no-such-project", "stream": "main"})
            check(not ok and "404" in text,
                  "an unknown project reports not-found, not a stack trace",
                  text[:160])

            # ── indexed search ──────────────────────────────────────────────
            # Needs the Postgres index, which is a separate thing from the graph
            # and legitimately absent. Distinguish that from a broken tool.
            ok, text = await call(session, "find_files_by_name",
                                  {**scope, "name_glob": "*"})
            if not ok and "registry" in text.lower():
                skip("indexed search tools",
                     f"{project}/{stream} has a graph but is not in the search "
                     f"index")
            else:
                check(ok, "find_files_by_name reaches the index",
                      text[:200] if not ok else "")
                if ok:
                    check("files" in text or "matches" in text or "count" in text,
                          "find_files_by_name returns a result body", text[:160])
                ok, text = await call(session, "browse_source_tree", {**scope, "levels": 1})
                check(ok, "browse_source_tree reaches the source tree",
                      text[:200] if not ok else "")

            # ── the advisor ─────────────────────────────────────────────────
            if os.environ.get("PRAXEVIA_SELFTEST_CHAT") == "1":
                ok, text = await call(session, "ask_codebase", {
                    **scope,
                    "question": "In one sentence, what is this codebase for?",
                    "timeout_s": 300,
                })
                check(ok, "ask_codebase returns an answer", text[:300] if not ok else "")
                if ok:
                    data = json.loads(text)
                    check(bool(data["answer"].strip()),
                          "the answer is not empty", json.dumps(data)[:200])
                    check(isinstance(data.get("searchesRun"), dict),
                          "the searches the advisor ran are reported",
                          json.dumps(data.get("searchesRun")))
            else:
                skip("ask_codebase", "set PRAXEVIA_SELFTEST_CHAT=1 — it spends "
                                     "tokens on the configured provider")

    print()
    if skipped:
        print(f"SKIPPED ({len(skipped)}):")
        for s in skipped:
            print("  -", s)
    if failures:
        print(f"FAILURES ({len(failures)}):")
        for f in failures:
            print("  -", f)
        return 1
    print("MCP server works.")
    return 0


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
