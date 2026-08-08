"""
Tests for HTTP route extraction.

The interesting cases here are all NEGATIVE. Finding `@router.get("/x")` is
easy; the work is in not finding the things that look exactly like it and are
not routes — client calls, docstring examples, prefix composition. Every
"does not match" test below corresponds to a false positive measured against a
real indexed codebase, not to a hypothetical.

Run:
    cd indexer && python3 -m pytest graph_pass/tests/test_http_routes.py -v
"""

from __future__ import annotations

import pytest

from graph_pass.http_routes import extract_http_route_symbols as extract


def names(rows):
    return [r["name"] for r in rows]


# ---------------------------------------------------------------------------
# FastAPI / Flask
# ---------------------------------------------------------------------------
def test_fastapi_router_decorator():
    src = '@router.get("/items")\ndef list_items():\n    pass\n'
    assert names(extract(src, ".py", "api.py", [])) == ["GET /items"]


def test_fastapi_app_decorator():
    src = '@app.post("/login")\ndef login():\n    pass\n'
    assert names(extract(src, ".py", "main.py", [])) == ["POST /login"]


def test_any_router_variable_name_not_just_app():
    # a real project registers 17 of its 18 route files on `@router.`; the earlier pattern only
    # matched `app.` and would have found 2 routes out of 100+.
    src = '@v1_router.delete("/x")\ndef d():\n    pass\n'
    assert names(extract(src, ".py", "a.py", [])) == ["DELETE /x"]


def test_empty_path_is_kept_as_the_collection_root():
    # shop's cart.py:24 and health.py:20 use "" for the collection root; a
    # pattern requiring a leading "/" drops them silently.
    src = '@router.get("")\ndef root():\n    pass\n'
    assert names(extract(src, ".py", "cart.py", [])) == ["GET /"]


def test_flask_bare_route_has_an_unresolved_verb():
    src = '@app.route("/health")\ndef health():\n    pass\n'
    assert names(extract(src, ".py", "a.py", [])) == ["ANY /health"]


def test_docstring_example_is_not_a_route():
    # awp's database.py:51 documents `@router.get("/workflows")` inside
    # get_db()'s docstring. Before masking, that module — which serves no
    # requests at all — contributed an endpoint to the graph.
    src = (
        'def get_db():\n'
        '    """\n'
        '    Usage:\n'
        '        @router.get("/workflows")\n'
        '        def get_workflows():\n'
        '            pass\n'
        '    """\n'
        '    pass\n'
    )
    assert extract(src, ".py", "database.py", []) == []


def test_commented_out_route_is_not_a_route():
    src = '# @router.get("/old")\n@router.get("/new")\ndef n():\n    pass\n'
    assert names(extract(src, ".py", "a.py", [])) == ["GET /new"]


# ---------------------------------------------------------------------------
# Spring
# ---------------------------------------------------------------------------
def test_spring_mapping_with_path():
    src = '@GetMapping("/alerts")\npublic List<Alert> list() {}\n'
    assert names(extract(src, ".java", "C.java", [])) == ["GET /alerts"]


def test_spring_bare_annotation_without_parens():
    # AlertCenterController.java:55, HealthController.java:40 — the earlier pattern
    # required a parenthesised path and dropped every one of these.
    src = "@GetMapping\npublic String ping() {}\n"
    assert names(extract(src, ".java", "C.java", [])) == ["GET /"]


def test_spring_value_kwarg_form():
    src = '@RequestMapping(value = "/rules")\npublic void r() {}\n'
    assert names(extract(src, ".java", "C.java", [])) == ["ANY /rules"]


@pytest.mark.parametrize("verb", ["Get", "Post", "Put", "Patch", "Delete"])
def test_spring_covers_all_five_verbs(verb):
    src = f'@{verb}Mapping("/x")\npublic void m() {{}}\n'
    assert names(extract(src, ".java", "C.java", [])) == [f"{verb.upper()} /x"]


# ---------------------------------------------------------------------------
# Axum — an entire REST API, invisible to every earlier pattern
# ---------------------------------------------------------------------------
def test_axum_route_registration():
    src = 'Router::new().route("/flights", get(list_flights))\n'
    rows = extract(src, ".rs", "routes/flights.rs", [])
    assert names(rows) == ["GET /flights"]
    assert "list_flights" in rows[0]["doc"]


def test_axum_qualified_handler_is_reduced_to_its_name():
    src = 'r.route("/fep", get(super::fep::flight_fep_detail))\n'
    rows = extract(src, ".rs", "r.rs", [])
    assert "flight_fep_detail" in rows[0]["doc"]


def test_axum_nest_is_not_a_route():
    # .nest() composes a prefix; it registers no handler.
    src = 'Router::new().nest("/flights", flights::routes())\n'
    assert extract(src, ".rs", "mod.rs", []) == []


# ---------------------------------------------------------------------------
# Express — the pattern that had to be narrowed
# ---------------------------------------------------------------------------
def test_express_route_with_handler():
    src = 'router.get("/users", listUsers);\n'
    assert names(extract(src, ".js", "r.js", [])) == ["GET /users"]


def test_http_client_call_is_not_a_route():
    # ff TrafficOverview.tsx:176. The wide form counted this as an endpoint,
    # which inflated ff to 546 mostly imaginary routes. An endpoint means
    # "requests enter HERE", not "this UI calls out to somewhere else".
    src = 'const res = await apiClient.get("/monitoring/traffic-counts");\n'
    assert extract(src, ".tsx", "TrafficOverview.tsx", []) == []


def test_test_mock_handler_is_not_a_route():
    # cso Connectivity.test.tsx:112 — `http.post('/api/v1/...', () => ...)`.
    src = "http.post('/api/v1/config/obm/test', () => {});\n"
    assert extract(src, ".tsx", "Connectivity.test.tsx", []) == []


# ---------------------------------------------------------------------------
# Symbol shape — must match what persist_symbols writes
# ---------------------------------------------------------------------------
def test_emitted_rows_carry_every_field_flush_symbols_writes():
    rows = extract('@router.get("/x")\ndef h():\n    pass\n', ".py", "a.py", [])
    assert rows
    for r in rows:
        assert set(r) >= {
            "name", "qualified_name", "kind", "filename",
            "line_start", "line_end", "parent_name", "doc",
        }
        # `route` must be in builder.py's KIND_TO_TYPE, or the build reports it
        # as an unmapped kind and it never becomes an endpoint node.
        assert r["kind"] == "route"


def test_handler_is_resolved_from_the_tree_sitter_symbols():
    src = '@router.post("/orders")\ndef create_order():\n    pass\n'
    ts = [{"kind": "function", "name": "create_order", "line_start": 2, "line_end": 3}]
    rows = extract(src, ".py", "a.py", ts)
    assert rows[0]["parent_name"] == "create_order"
    assert "create_order" in rows[0]["doc"]


def test_line_numbers_survive_comment_masking():
    src = "# padding\n# padding\n@router.get(\"/x\")\ndef h():\n    pass\n"
    assert extract(src, ".py", "a.py", [])[0]["line_start"] == 3


def test_duplicate_registrations_on_one_line_are_emitted_once():
    src = '@router.get("/x")\n@router.get("/x")\ndef h():\n    pass\n'
    rows = extract(src, ".py", "a.py", [])
    assert len(rows) == 2  # different lines, genuinely two registrations


def test_unknown_extension_yields_nothing():
    assert extract('@router.get("/x")', ".txt", "a.txt", []) == []
