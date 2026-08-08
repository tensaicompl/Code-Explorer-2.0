"""
Tests for the LOD graph API —, and part of's test strategy.

No credentials anywhere: identity is injected by monkeypatching the two auth
callables the router is mounted with. That is deliberate — it tests the graph
endpoints in isolation from the auth stack, while still proving that an
unauthenticated request is rejected (the one case that needs the real path).

Run:
    cd backend && python3 -m pytest tests/test_graph_api.py -v
"""

from __future__ import annotations

import json

import pytest
from fastapi import FastAPI, HTTPException, Request
from fastapi.testclient import TestClient

from app.graph import queries
from app.graph.router import mount, registry_key
from app.graph.store import STORE, GraphStore, LoadedGraph


# ---------------------------------------------------------------------------
# Fixture graph — small, but exercising every shape the queries care about.
# ---------------------------------------------------------------------------
def _node(nid, ntype, name, **extra):
    return {
        "id": nid, "type": ntype, "name": name,
        "summary": name, "tags": [], "complexity": "moderate", **extra,
    }


def _edge(src, tgt, etype, weight=1.0):
    return {"source": src, "target": tgt, "type": etype,
            "direction": "forward", "weight": weight}


FIXTURE = {
    "version": "1.0.0",
    "kind": "codebase",
    "project": {
        "name": "demo/develop", "languages": ["py"], "frameworks": [],
        "description": "", "analyzedAt": "2026-08-03T00:00:00Z", "gitCommitHash": "",
    },
    "nodes": [
        _node("module:src", "module", "src", filePath="src"),
        _node("file:src/a.py", "file", "a.py", filePath="src/a.py"),
        _node("function:src/a.py:alpha", "function", "alpha", filePath="src/a.py"),
        _node("function:src/a.py:beta", "function", "beta", filePath="src/a.py"),
        _node("module:tests", "module", "tests", filePath="tests"),
        _node("file:tests/t.py", "file", "t.py", filePath="tests/t.py"),
        _node("function:tests/t.py:test_alpha", "function", "test_alpha",
              filePath="tests/t.py"),
        _node("table:db/s.sql:users", "table", "users",
              filePath="db/s.sql", columns=["id", "email"], tags=["table"]),
    ],
    "edges": [
        _edge("module:src", "file:src/a.py", "contains"),
        _edge("file:src/a.py", "function:src/a.py:alpha", "contains"),
        _edge("file:src/a.py", "function:src/a.py:beta", "contains"),
        _edge("module:tests", "file:tests/t.py", "contains"),
        _edge("file:tests/t.py", "function:tests/t.py:test_alpha", "contains"),
        # cross-layer: tests -> src
        _edge("function:tests/t.py:test_alpha", "function:src/a.py:alpha", "calls", 0.5),
    ],
    "layers": [
        {"id": "layer:src", "name": "src", "description": "", "nodeIds": [
            "module:src", "file:src/a.py",
            "function:src/a.py:alpha", "function:src/a.py:beta",
            "table:db/s.sql:users",
        ]},
        {"id": "layer:tests", "name": "tests", "description": "", "nodeIds": [
            "module:tests", "file:tests/t.py", "function:tests/t.py:test_alpha",
        ]},
    ],
    "tour": [
        {"order": 1, "title": "How this codebase is laid out",
         "description": "Two areas.", "nodeIds": ["module:src", "module:tests"]},
        {"order": 2, "title": "Where execution starts",
         "description": "One root.",
         "nodeIds": ["function:tests/t.py:test_alpha",
                     "function:src/a.py:alpha"]},
    ],
}

SIDECAR = {"resolution": {"pctExactOfInternal": 62.9, "drawn": 1, "ambiguous": 0}}


@pytest.fixture
def graph_dir(tmp_path):
    key = registry_key("demo", "develop")
    (tmp_path / f"{key}.json").write_text(json.dumps(FIXTURE))
    (tmp_path / f"{key}.meta.json").write_text(json.dumps(SIDECAR))
    return tmp_path


@pytest.fixture
def client(graph_dir, monkeypatch):
    """App with the graph router mounted and auth stubbed to a fixed identity."""
    monkeypatch.setattr(STORE, "_dir", str(graph_dir), raising=False)
    STORE.invalidate()

    def fake_verify(request: Request) -> str:
        if request.headers.get("Authorization") != "Bearer test":
            raise HTTPException(status_code=401, detail="Unauthorized")
        return "tester@example.com"

    def fake_access(identity: str, project: str) -> None:
        if project == "forbidden":
            raise HTTPException(status_code=403, detail="No access to this project")

    app = FastAPI()
    mount(app, fake_verify, fake_access)
    return TestClient(app)


AUTH = {"Authorization": "Bearer test"}
Q = {"project": "demo", "stream": "develop"}


# ---------------------------------------------------------------------------
# Auth
# ---------------------------------------------------------------------------
def test_unauthenticated_is_rejected(client):
    assert client.get("/api/graph/layers", params=Q).status_code == 401


def test_access_control_is_enforced(client):
    r = client.get("/api/graph/layers",
                   params={"project": "forbidden", "stream": "develop"}, headers=AUTH)
    # 403 must win over 404: a user without access must not learn whether a
    # graph exists for that project.
    assert r.status_code == 403


def test_unbuilt_project_is_404_not_500(client):
    r = client.get("/api/graph/layers",
                   params={"project": "nope", "stream": "develop"}, headers=AUTH)
    assert r.status_code == 404


# ---------------------------------------------------------------------------
# The envelope and the schema invariants
# ---------------------------------------------------------------------------
def test_response_is_an_envelope_not_a_bare_graph(client):
    body = client.get("/api/graph/layers", params=Q, headers=AUTH).json()
    #: metadata cannot ride inside the graph — both KnowledgeGraphSchema and
    # ProjectMetaSchema are plain z.object() and strip unknown keys silently.
    assert set(body) == {"graph", "meta"}
    assert "truncated" in body["meta"]


@pytest.mark.parametrize("path,params", [
    ("/api/graph/layers", Q),
    ("/api/graph/layer/layer:src", Q),
    ("/api/graph/module/module:src", Q),
    ("/api/graph/file/file:src/a.py", Q),
    ("/api/graph/node/function:src/a.py:alpha/neighbourhood", Q),
])
def test_every_fragment_is_schema_shaped(client, path, params):
    g = client.get(path, params=params, headers=AUTH).json()["graph"]
    # validateGraph() treats a missing collection as FATAL (schema.ts:582).
    assert all(k in g for k in ("nodes", "edges", "layers", "tour"))
    assert g["kind"] == "codebase", "kind:'knowledge' switches renderer irreversibly"
    #: zero layers renders a blank but fully interactive canvas, silently.
    assert len(g["layers"]) > 0, f"{path} returned a fragment with no layers"
    #: a falsy summary triggers one auto-corrected issue per node.
    assert all(n["summary"] for n in g["nodes"])
    # Edges must not dangle — the dashboard indexes them by node id.
    ids = {n["id"] for n in g["nodes"]}
    assert all(e["source"] in ids and e["target"] in ids for e in g["edges"])


def test_layers_never_reference_absent_nodes(client):
    g = client.get("/api/graph/module/module:src", params=Q, headers=AUTH).json()["graph"]
    ids = {n["id"] for n in g["nodes"]}
    for layer in g["layers"]:
        assert set(layer["nodeIds"]) <= ids


# ---------------------------------------------------------------------------
# LOD behaviour
# ---------------------------------------------------------------------------
def test_overview_returns_real_layers_with_real_representatives(client):
    """
    L0 sends the graph's REAL layers and REAL module nodes.

    An earlier version invented one `agg:layer:*` node per layer inside a single
    synthetic layer. It validated cleanly and rendered as one box for the whole
    project, because `useOverviewGraph` builds one cluster PER LAYER
    (GraphView.tsx:300). Synthetic ids also leak into navigation and citations,
    where nothing can resolve them.
    """
    body = client.get("/api/graph/layers", params=Q, headers=AUTH).json()
    g = body["graph"]
    assert {n["id"] for n in g["nodes"]} == {"module:src", "module:tests"}
    assert {l["id"] for l in g["layers"]} == {"layer:src", "layer:tests"}
    assert body["meta"]["truncated"] is False


def test_overview_lifts_symbol_calls_into_visible_dependencies(client):
    """
    The regression that produced a screen of disconnected boxes.

    `calls` edges are symbol-to-symbol, so filtering them to a node set made of
    modules keeps none of them — measured 0 of 2,748 on a real index.
    Lifting each endpoint up the `contains` tree turns those calls into the
    module-to-module dependency this level exists to show.
    """
    g = client.get("/api/graph/layers", params=Q, headers=AUTH).json()["graph"]
    assert g["edges"], "overview must not be edgeless — that is the bug"
    assert any(e["source"] == "module:tests" and e["target"] == "module:src"
               for e in g["edges"])


def test_full_graph_keeps_every_node_and_edge(client):
    """
    The dashboard does its own level-of-detail client-side, so /api/compat must
    hand it everything. Serving a fragment here is what left it nothing to
    aggregate.
    """
    g = client.get("/api/compat/knowledge-graph.json", params=Q, headers=AUTH).json()
    assert len(g["nodes"]) == len(FIXTURE["nodes"])
    assert len(g["edges"]) == len(FIXTURE["edges"])
    assert len(g["layers"]) == len(FIXTURE["layers"])
    # And the client's own aggregation must find the cross-layer link.
    ids = {n["id"] for n in g["nodes"]}
    n2l = {i: l["id"] for l in g["layers"] for i in l["nodeIds"] if i in ids}
    crossing = {
        tuple(sorted((n2l[e["source"]], n2l[e["target"]])))
        for e in g["edges"]
        if e["source"] in n2l and e["target"] in n2l
        and n2l[e["source"]] != n2l[e["target"]]
    }
    assert crossing, "aggregateLayerEdges would draw nothing"


def test_full_graph_carries_the_tour(client):
    """Learn mode reads `graph.tour`; a stripped one is the empty state again."""
    g = client.get("/api/compat/knowledge-graph.json", params=Q, headers=AUTH).json()
    assert g["tour"] == FIXTURE["tour"]


def test_the_file_projection_lifts_the_tour_rather_than_dropping_it(client, monkeypatch):
    """
    Past the node ceiling the payload has no symbols — so neither may the tour.

    Dropping it is how the largest repository in a deployment, the one whose
    newcomer most needs a walkthrough, gets "No tour available". Lifting each
    pill to its containing file keeps every one of them pointing at a node that
    is actually in the response.
    """
    monkeypatch.setattr(queries, "FULL_GRAPH_MAX_NODES", 0)
    g = client.get("/api/compat/knowledge-graph.json", params=Q, headers=AUTH).json()
    ids = {n["id"] for n in g["nodes"]}

    assert len(g["tour"]) == len(FIXTURE["tour"])
    for step in g["tour"]:
        assert step["nodeIds"], step["title"]
        assert set(step["nodeIds"]) <= ids, "a pill naming an absent node"

    # The two functions live in different files, so neither collapses into the
    # other — and the step says why it is naming files.
    starts = g["tour"][1]
    assert starts["nodeIds"] == ["file:tests/t.py", "file:src/a.py"]
    assert "files that contain them" in starts["description"]

    # A step that already named visible nodes is left exactly as it was.
    assert g["tour"][0] == FIXTURE["tour"][0]


def test_drilling_into_a_layer_returns_its_modules(client):
    g = client.get("/api/graph/layer/layer:src", params=Q, headers=AUTH).json()["graph"]
    assert [n["id"] for n in g["nodes"]] == ["module:src"]


def test_file_detail_returns_its_symbols(client):
    g = client.get("/api/graph/file/file:src/a.py", params=Q, headers=AUTH).json()["graph"]
    names = sorted(n["name"] for n in g["nodes"])
    assert names == ["a.py", "alpha", "beta"]


def test_neighbourhood_crosses_layers(client):
    g = client.get("/api/graph/node/function:src/a.py:alpha/neighbourhood",
                   params={**Q, "hops": 1}, headers=AUTH).json()["graph"]
    ids = {n["id"] for n in g["nodes"]}
    assert "function:tests/t.py:test_alpha" in ids, "caller from another layer"
    assert "file:src/a.py" in ids, "containing file"


def test_budget_truncates_and_reports_it(client, monkeypatch):
    monkeypatch.setitem(queries.LOD_BUDGETS, "L0", 1)
    body = client.get("/api/graph/layers", params=Q, headers=AUTH).json()
    assert len(body["graph"]["nodes"]) == 1
    # Silent truncation reads as "this is the whole picture" — a correctness bug
    # for a visualization, so the flag is not optional.
    assert body["meta"]["truncated"] is True
    assert body["meta"]["totalAvailable"] == 2


# ---------------------------------------------------------------------------
# Search and meta
# ---------------------------------------------------------------------------
def test_search_prefers_name_matches_over_path_matches(client):
    g = client.get("/api/graph/search", params={**Q, "q": "alpha"},
                   headers=AUTH).json()["graph"]
    assert g["nodes"][0]["name"] == "alpha"


def test_empty_result_carries_no_layers_so_the_client_shows_an_empty_state(client):
    """
    The one place the "always >= 1 layer" rule is deliberately NOT applied.

    A search that matched nothing must return an empty fragment, not a fragment
    containing an empty layer: an empty layer is dropped by the dashboard anyway
    (GraphView.tsx:283-298) and reproduces the blank canvas by a longer route.
    Zero nodes is the client's cue to render the empty state instead.
    """
    for q in ("  ", "zzz-no-such-symbol-zzz"):
        body = client.get("/api/graph/search", params={**Q, "q": q}, headers=AUTH).json()
        assert body["graph"]["nodes"] == []
        assert body["graph"]["layers"] == []
        assert all(k in body["graph"] for k in ("nodes", "edges", "layers", "tour"))


def test_meta_exposes_resolution_statistics(client):
    m = client.get("/api/graph/meta", params=Q, headers=AUTH).json()
    assert m["counts"]["nodes"] == 8
    assert m["nodeTypes"]["function"] == 3
    assert m["nodeTypes"]["table"] == 1
    #: a resolution rate nobody can see is one nobody accounts for.
    assert m["resolution"]["pctExactOfInternal"] == 62.9


# ---------------------------------------------------------------------------
# Store
# ---------------------------------------------------------------------------
def test_store_survives_a_corrupt_graph(tmp_path):
    (tmp_path / "bad_develop.json").write_text("{ this is not json")
    store = GraphStore(str(tmp_path))
    # A corrupt graph must not take the API down — the chat product is unaffected.
    assert store.get("bad_develop") is None


def test_loaded_graph_indexes_first_layer_wins():
    loaded = LoadedGraph(graph=FIXTURE)
    loaded.build_indexes()
    assert loaded.layer_of_node["module:src"] == "layer:src"
    assert len(loaded.nodes_by_id) == len(FIXTURE["nodes"])


# ---------------------------------------------------------------------------
# Pre-compression (§P5)
# ---------------------------------------------------------------------------
def test_graph_is_gzipped_when_the_client_accepts_it(client):
    r = client.get(
        "/api/compat/knowledge-graph.json", params=Q,
        headers={**AUTH, "Accept-Encoding": "gzip"},
    )
    assert r.status_code == 200
    # TestClient transparently decodes, so assert on the header and on the fact
    # that the decoded body is still the graph.
    assert r.headers.get("content-encoding") == "gzip"
    assert r.headers.get("vary") == "Accept-Encoding"
    assert len(r.json()["nodes"]) == len(FIXTURE["nodes"])


def test_graph_is_plain_json_when_the_client_does_not_accept_gzip(client):
    """
    A client that never advertised gzip must not be handed a gzip body. Browsers
    always advertise it; `curl` without --compressed does not.
    """
    r = client.get(
        "/api/compat/knowledge-graph.json", params=Q,
        headers={**AUTH, "Accept-Encoding": "identity"},
    )
    assert r.status_code == 200
    assert "content-encoding" not in r.headers
    assert len(r.json()["nodes"]) == len(FIXTURE["nodes"])


def test_compression_is_memoised_not_recomputed(client):
    """Two identical requests must reuse one compressed blob."""
    from app.graph.router import registry_key
    from app.graph.store import STORE

    h = {**AUTH, "Accept-Encoding": "gzip"}
    first = client.get("/api/compat/knowledge-graph.json", params=Q, headers=h)
    loaded = STORE.get(registry_key("demo", "develop"))
    cache = getattr(loaded, "_gzip_cache", {})
    assert len(cache) == 1, "first request should populate exactly one entry"
    blob = next(iter(cache.values()))

    second = client.get("/api/compat/knowledge-graph.json", params=Q, headers=h)
    assert next(iter(getattr(loaded, "_gzip_cache").values())) is blob
    assert first.json() == second.json()


def test_sse_chat_is_not_touched_by_compression(client):
    """
    The reason compression lives in the store rather than in GZipMiddleware:
    the middleware wraps every response, including the chat's text/event-stream,
    where gzip's block buffering batches per-token events. Nothing outside the
    graph endpoints may acquire a Content-Encoding from this change.
    """
    r = client.get(
        "/api/graph/projects", headers={**AUTH, "Accept-Encoding": "gzip"},
    )
    assert r.status_code == 200
    assert "content-encoding" not in r.headers


# ---------------------------------------------------------------------------
# Cache staleness
# ---------------------------------------------------------------------------
def test_rebuilt_graph_is_picked_up_without_an_explicit_invalidate(client, graph_dir):
    """
    The cache used to be write-once, evicted only by `STORE.invalidate()` — and
    the sole caller of that is `sources.py`'s `_rebuild()`, which runs in one
    process. `docker-entrypoint.sh` starts uvicorn with `--workers 2`, so a
    rebuild triggered through one worker left the other serving its cached copy
    with nothing anywhere to correct it: the same URL returned fresh or stale
    data depending on which worker answered.

    Validating against the file's mtime fixes that across processes, and also
    picks up graphs written by the indexer sidecar or a manual builder run,
    which previously needed a restart to become visible.
    """
    import os

    key = registry_key("demo", "develop")
    path = graph_dir / f"{key}.json"

    first = client.get("/api/compat/knowledge-graph.json", params=Q, headers=AUTH)
    assert first.status_code == 200
    assert first.json()["project"]["name"] == "demo/develop"

    rebuilt = json.loads(json.dumps(FIXTURE))
    rebuilt["project"]["name"] = "demo/develop (rebuilt)"
    path.write_text(json.dumps(rebuilt))
    # Force a distinct mtime: the write above can land inside the filesystem's
    # timestamp granularity and look unchanged.
    stamp = os.stat(path).st_mtime + 10
    os.utime(path, (stamp, stamp))

    second = client.get("/api/compat/knowledge-graph.json", params=Q, headers=AUTH)
    assert second.json()["project"]["name"] == "demo/develop (rebuilt)"


def test_unchanged_graph_is_served_from_cache(client):
    """The mtime check must not turn every request into a full reload."""
    key = registry_key("demo", "develop")
    first = STORE.get(key)
    second = STORE.get(key)
    assert first is second
