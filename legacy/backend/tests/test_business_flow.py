"""
Tests for business-flow graph generation and serving.

Same strategy as test_graph_api.py: no credentials, auth injected by
monkeypatching the two callables the router is mounted with, and the store
pointed at a tmp_path.

The database is stubbed rather than run. `business_flow.py` uses Postgres only
for the job status row, and everything worth testing here — the synthesis, the
409 guard, the 404s, the bare-graph response shape — is independent of where
that row lives.

Run:
    cd backend && python3 -m pytest tests/test_business_flow.py -v
"""

from __future__ import annotations

import json
from datetime import datetime, timedelta, timezone

import pytest
from fastapi import FastAPI, HTTPException, Request
from fastapi.testclient import TestClient

from app.graph import business_flow
from app.graph.business_flow import _llm_cooldown_remaining, mount, registry_key
from app.graph.business_flow_synth import (
    MAX_STEPS_PER_FLOW,
    _flow_step_weight,
    synthesize_business_flow_deterministic,
)
from app.graph.store import DOMAIN_STORE, STORE

from test_graph_api import FIXTURE  # noqa: F401 — reuse the one fixture graph


@pytest.fixture
def graph_dir(tmp_path):
    key = registry_key("demo", "develop")
    (tmp_path / f"{key}.json").write_text(json.dumps(FIXTURE))
    (tmp_path / "domain").mkdir()
    return tmp_path


@pytest.fixture
def rows():
    """In-memory stand-in for the business_flow_graphs table."""
    return {}


@pytest.fixture
def client(graph_dir, rows, monkeypatch):
    monkeypatch.setattr(STORE, "_dir", str(graph_dir), raising=False)
    monkeypatch.setattr(DOMAIN_STORE, "_dir", str(graph_dir / "domain"), raising=False)
    STORE.invalidate()
    DOMAIN_STORE.invalidate()

    monkeypatch.setattr(business_flow, "ensure_table", lambda: None)
    monkeypatch.setattr(
        business_flow, "_row", lambda p, s: rows.get(registry_key(p, s))
    )
    monkeypatch.setattr(
        business_flow,
        "_set",
        lambda p, s, mode, status, message="": rows.__setitem__(
            registry_key(p, s),
            {"mode": mode, "status": status, "message": message,
             "updated_at": datetime.now(timezone.utc)},
        ),
    )

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
# Auth — the same three guarantees the graph API makes
# ---------------------------------------------------------------------------
def test_unauthenticated_is_rejected(client):
    assert client.get("/api/compat/domain-graph.json", params=Q).status_code == 401
    assert client.post("/api/graph/business-flow", json=Q).status_code == 401


def test_access_control_is_enforced(client):
    r = client.get(
        "/api/compat/domain-graph.json",
        params={"project": "forbidden", "stream": "develop"},
        headers=AUTH,
    )
    # 403 must win over 404: a user without access must not learn whether a
    # business-flow graph exists for that project.
    assert r.status_code == 403


# ---------------------------------------------------------------------------
# Serving
# ---------------------------------------------------------------------------
def test_missing_graph_is_404_so_the_client_degrades_to_null(client):
    r = client.get("/api/compat/domain-graph.json", params=Q, headers=AUTH)
    assert r.status_code == 404


def test_generated_graph_is_served_bare_not_enveloped(client, graph_dir):
    assert client.post(
        "/api/graph/business-flow", json={**Q, "mode": "deterministic"}, headers=AUTH
    ).status_code == 200

    r = client.get("/api/compat/domain-graph.json", params=Q, headers=AUTH)
    assert r.status_code == 200
    body = r.json()
    # App.tsx passes this straight to validateGraph, which requires `project` at
    # the top level. An envelope would fail validation with no clue why.
    assert "graph" not in body
    assert {"version", "project", "nodes", "edges", "layers", "tour"} <= set(body)


# ---------------------------------------------------------------------------
# Generation
# ---------------------------------------------------------------------------
def test_generate_without_a_codebase_graph_is_404(client):
    r = client.post(
        "/api/graph/business-flow",
        json={"project": "nope", "stream": "develop"},
        headers=AUTH,
    )
    assert r.status_code == 404


def test_generate_rejects_an_unknown_mode(client):
    r = client.post(
        "/api/graph/business-flow", json={**Q, "mode": "magic"}, headers=AUTH
    )
    assert r.status_code == 400


def test_llm_mode_without_a_key_is_503_not_a_silent_fallback(client, monkeypatch):
    # Falling back to the deterministic pass would let a user judge the LLM mode
    # by the other one's output, with no way to tell.
    monkeypatch.setattr(business_flow, "ANTHROPIC_API_KEY", "")
    r = client.post("/api/graph/business-flow", json={**Q, "mode": "llm"}, headers=AUTH)
    assert r.status_code == 503
    assert "advisor" in r.json()["detail"].lower()


def test_llm_mode_runs_the_enrichment_and_persists_its_names(client, monkeypatch):
    monkeypatch.setattr(business_flow, "ANTHROPIC_API_KEY", "test-key")

    def fake_enrich(graph):
        for node in graph["nodes"]:
            if node["type"] == "domain":
                node["name"] = "Order Management"
        return graph, 1

    monkeypatch.setattr(business_flow, "enrich_business_flow_with_llm", fake_enrich)
    monkeypatch.setattr(business_flow, "record_event", lambda **kw: None)

    assert client.post(
        "/api/graph/business-flow", json={**Q, "mode": "llm"}, headers=AUTH
    ).status_code == 200

    body = client.get("/api/compat/domain-graph.json", params=Q, headers=AUTH).json()
    domains = [n for n in body["nodes"] if n["type"] == "domain"]
    assert domains and all(n["name"] == "Order Management" for n in domains)


def test_llm_failure_fails_the_job_rather_than_shipping_structural_names(
    client, rows, monkeypatch
):
    monkeypatch.setattr(business_flow, "ANTHROPIC_API_KEY", "test-key")

    def boom(graph):
        raise RuntimeError("upstream said no")

    monkeypatch.setattr(business_flow, "enrich_business_flow_with_llm", boom)

    client.post("/api/graph/business-flow", json={**Q, "mode": "llm"}, headers=AUTH)
    row = rows[registry_key("demo", "develop")]
    assert row["status"] == "failed"
    assert "upstream said no" in row["message"]
    # And nothing was written: a half-enriched graph must not replace a good one.
    r = client.get("/api/compat/domain-graph.json", params=Q, headers=AUTH)
    assert r.status_code == 404


def test_concurrent_generation_is_rejected(client, rows):
    rows[registry_key("demo", "develop")] = {
        "mode": "deterministic", "status": "generating", "message": "",
    }
    r = client.post("/api/graph/business-flow", json=Q, headers=AUTH)
    assert r.status_code == 409


def test_status_reports_a_graph_that_exists_on_disk_without_a_row(client):
    client.post("/api/graph/business-flow", json=Q, headers=AUTH)
    rows_before = client.get("/api/graph/business-flow", params=Q, headers=AUTH).json()
    assert rows_before["status"] == "ready"


# ---------------------------------------------------------------------------
# Synthesis
# ---------------------------------------------------------------------------
def test_synthesis_maps_layers_to_domains_and_calls_to_steps():
    out = synthesize_business_flow_deterministic(FIXTURE)
    kinds = {n["type"] for n in out["nodes"]}
    assert {"domain", "flow", "step"} <= kinds
    # The fixture has two layers, both above MIN_NODES_PER_DOMAIN.
    assert sum(1 for n in out["nodes"] if n["type"] == "domain") == 2
    assert out["layers"] == []
    assert out["tour"] == []


def test_synthesis_emits_no_duplicate_ids_and_no_dangling_edges():
    out = synthesize_business_flow_deterministic(FIXTURE)
    ids = [n["id"] for n in out["nodes"]]
    assert len(ids) == len(set(ids))
    known = set(ids)
    for e in out["edges"]:
        assert e["source"] in known
        assert e["target"] in known


def test_synthesis_keeps_every_weight_inside_the_schema_bound():
    out = synthesize_business_flow_deterministic(FIXTURE)
    for e in out["edges"]:
        assert 0.0 <= e["weight"] <= 1.0


def test_step_weights_increase_strictly_within_each_flow():
    out = synthesize_business_flow_deterministic(FIXTURE)
    per_flow: dict[str, list[float]] = {}
    for e in out["edges"]:
        if e["type"] == "flow_step":
            per_flow.setdefault(e["source"], []).append(e["weight"])
    assert per_flow, "fixture should produce at least one flow"
    for flow, weights in per_flow.items():
        assert weights == sorted(weights), flow
        assert len(set(weights)) == len(weights), flow


@pytest.mark.parametrize("total", [1, 3, 5, 10, 11, 15, 40])
def test_flow_step_weight_never_collides_or_leaves_the_bound(total):
    """
    The old rule rounded to 0.1 and collided past ten steps. This one must not,
    at any size — that is the whole reason it exists.
    """
    weights = [_flow_step_weight(i, total) for i in range(1, total + 1)]
    assert len(set(weights)) == total
    assert weights == sorted(weights)
    assert all(0.0 < w < 1.0 for w in weights)


def test_a_flow_never_exceeds_the_step_cap():
    out = synthesize_business_flow_deterministic(FIXTURE)
    counts: dict[str, int] = {}
    for e in out["edges"]:
        if e["type"] == "flow_step":
            counts[e["source"]] = counts.get(e["source"], 0) + 1
    assert all(c <= MAX_STEPS_PER_FLOW for c in counts.values())


def test_synthesis_of_an_empty_graph_does_not_raise():
    out = synthesize_business_flow_deterministic(
        {"project": {}, "nodes": [], "edges": [], "layers": [], "tour": []}
    )
    assert out["nodes"] == []
    assert out["edges"] == []
    # Still a schema-valid document: `project` is required and all six of its
    # fields are non-optional.
    assert set(out["project"]) == {
        "name", "languages", "frameworks", "description", "analyzedAt", "gitCommitHash",
    }


# ---------------------------------------------------------------------------
# LLM enrichment — the pure parts, which is where the defensive rules live
# ---------------------------------------------------------------------------
from app.graph.business_flow_llm import _extract_json, _merge, _pack  # noqa: E402


def test_extract_json_survives_a_fenced_or_chatty_reply():
    assert _extract_json('{"a": 1}') == {"a": 1}
    assert _extract_json('```json\n{"a": 1}\n```') == {"a": 1}
    assert _extract_json('Sure! Here you go:\n{"a": 1}\nHope that helps.') == {"a": 1}
    with pytest.raises(ValueError):
        _extract_json("no json at all")


def test_merge_ignores_ids_the_model_invented():
    graph = synthesize_business_flow_deterministic(FIXTURE)
    before = len(graph["nodes"])
    graph, applied = _merge(graph, {"domain:does-not-exist": {"name": "Ghost"}})
    assert applied == 0
    assert len(graph["nodes"]) == before


def test_merge_keeps_deterministic_values_for_ids_the_model_omitted():
    graph = synthesize_business_flow_deterministic(FIXTURE)
    original = {n["id"]: n["name"] for n in graph["nodes"]}
    graph, _ = _merge(graph, {})
    assert {n["id"]: n["name"] for n in graph["nodes"]} == original


def test_merge_rejects_an_entry_type_outside_the_schema_enum():
    graph = synthesize_business_flow_deterministic(FIXTURE)
    flow = next(n for n in graph["nodes"] if n["type"] == "flow")
    graph, _ = _merge(graph, {flow["id"]: {"entryType": "telepathy"}})
    node = next(n for n in graph["nodes"] if n["id"] == flow["id"])
    # DomainMetaSchema's entryType is a closed z.enum — an invented value would
    # fail validation for the whole node, taking its edges with it.
    assert node.get("domainMeta", {}).get("entryType") != "telepathy"


def test_merge_rejects_non_string_names():
    graph = synthesize_business_flow_deterministic(FIXTURE)
    dom = next(n for n in graph["nodes"] if n["type"] == "domain")
    graph, applied = _merge(graph, {dom["id"]: {"name": 42, "summary": None}})
    node = next(n for n in graph["nodes"] if n["id"] == dom["id"])
    assert applied == 0
    assert isinstance(node["name"], str)
    assert isinstance(node["summary"], str)


def test_pack_mentions_every_domain_and_flow_id_the_reply_is_keyed_by():
    graph = synthesize_business_flow_deterministic(FIXTURE)
    packed = _pack(graph)
    for node in graph["nodes"]:
        if node["type"] in ("domain", "flow"):
            assert node["id"] in packed


# ---------------------------------------------------------------------------
# LLM cooldown — the only spend control, so it gets real coverage
# ---------------------------------------------------------------------------
def _row_at(mode, status, age_hours):
    return {
        "mode": mode, "status": status, "message": "",
        "updated_at": datetime.now(timezone.utc) - timedelta(hours=age_hours),
    }


def test_cooldown_blocks_a_second_llm_run_within_the_window():
    assert _llm_cooldown_remaining(_row_at("llm", "ready", 1)) is not None


def test_cooldown_expires_after_the_window():
    assert _llm_cooldown_remaining(_row_at("llm", "ready", 25)) is None


def test_cooldown_does_not_apply_to_a_failed_run():
    # Locking someone out for a day because the key was misconfigured would be
    # hostile, and a failed run left nothing to protect.
    assert _llm_cooldown_remaining(_row_at("llm", "failed", 1)) is None


def test_cooldown_does_not_apply_to_deterministic_runs():
    assert _llm_cooldown_remaining(_row_at("deterministic", "ready", 1)) is None


def test_cooldown_ignores_a_project_that_never_ran():
    assert _llm_cooldown_remaining(None) is None


def test_cooldown_tolerates_a_naive_timestamp():
    row = _row_at("llm", "ready", 1)
    row["updated_at"] = row["updated_at"].replace(tzinfo=None)
    assert _llm_cooldown_remaining(row) is not None


def test_second_llm_request_within_the_day_is_429(client, rows, monkeypatch):
    monkeypatch.setattr(business_flow, "ANTHROPIC_API_KEY", "test-key")
    monkeypatch.setattr(business_flow, "enrich_business_flow_with_llm",
                        lambda g: (g, 1))
    monkeypatch.setattr(business_flow, "record_event", lambda **kw: None)

    first = client.post("/api/graph/business-flow", json={**Q, "mode": "llm"},
                        headers=AUTH)
    assert first.status_code == 200

    second = client.post("/api/graph/business-flow", json={**Q, "mode": "llm"},
                         headers=AUTH)
    assert second.status_code == 429
    assert "Retry-After" in second.headers
    assert "deterministic" in second.json()["detail"].lower()


def test_deterministic_stays_available_during_the_llm_cooldown(client, rows, monkeypatch):
    monkeypatch.setattr(business_flow, "ANTHROPIC_API_KEY", "test-key")
    monkeypatch.setattr(business_flow, "enrich_business_flow_with_llm",
                        lambda g: (g, 1))
    monkeypatch.setattr(business_flow, "record_event", lambda **kw: None)

    client.post("/api/graph/business-flow", json={**Q, "mode": "llm"}, headers=AUTH)
    r = client.post("/api/graph/business-flow",
                    json={**Q, "mode": "deterministic"}, headers=AUTH)
    assert r.status_code == 200
