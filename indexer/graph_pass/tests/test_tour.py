"""
The deterministic tour, against graphs built to trip it.

    cd indexer && ../.venv/bin/python -m pytest graph_pass/tests -q

WHY THESE EXIST. A tour is prose, and wrong prose is the one output nobody
double-checks — it reads fluently whatever it says. The claims worth defending
are the ones a reader would act on: that a step naming four symbols is not
quietly hiding four hundred, that "nothing calls this" is stated as what it
actually is, and that a step whose subject does not exist is not emitted saying
"0 database tables".

Determinism is asserted by SHUFFLING the input, not by calling twice. Calling
twice passes even if the output depends on dictionary insertion order, which is
exactly the bug that makes a nightly rebuild produce a different tour from the
same commit.

The schema itself is checked elsewhere and better: `builtGraphSchema.test.ts`
runs the dashboard's own `validateGraph` over every file in `.graphs/`, so a
tour step that violates TourStepSchema fails there against the real contract
rather than against a copy of it kept in step by hand.
"""

from __future__ import annotations

import os
import random
import sys

import pytest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))

from graph_pass.tour import MAX_PILLS, build_tour  # noqa: E402


def node(node_id: str, node_type: str, name: str, **extra) -> dict:
    """A node shaped like builder._node — summary defaults to the name."""
    return {"id": node_id, "type": node_type, "name": name,
            "summary": extra.pop("summary", name), "tags": [],
            "complexity": "moderate", **extra}


def edge(source: str, target: str, edge_type: str) -> dict:
    return {"source": source, "target": target, "type": edge_type,
            "direction": "forward", "weight": 1.0}


def layer(slug: str, name: str, node_ids: list[str]) -> dict:
    return {"id": f"layer:{slug}", "name": name, "description": "",
            "nodeIds": node_ids}


@pytest.fixture
def graph() -> tuple[list[dict], list[dict], list[dict]]:
    """A small graph with one of everything the tour can talk about."""
    nodes = [
        node("module:src", "module", "src"),
        node("file:src/app.py", "file", "app.py", filePath="src/app.py"),
        node("function:src/app.py:main", "function", "main",
             filePath="src/app.py", summary="Entry point."),
        node("function:src/app.py:helper", "function", "helper",
             filePath="src/app.py"),
        node("function:src/app.py:shared", "function", "shared",
             filePath="src/app.py"),
        node("file:tests/test_app.py", "file", "test_app.py",
             filePath="tests/test_app.py"),
        node("function:tests/test_app.py:test_main", "function", "test_main",
             filePath="tests/test_app.py"),
        node("file:README.md", "file", "README.md", filePath="README.md"),
        node("document:README.md:Project", "document", "Project",
             filePath="README.md", summary="What this is.", sections=["A", "B"]),
        node("document:README.md:Project.Details", "document", "Details",
             filePath="README.md", summary="Fine print."),
        node("table:db/schema.sql:orders", "table", "orders",
             filePath="db/schema.sql", columns=["id", "total"]),
        node("config:.env:API_URL", "config", "API_URL", filePath=".env"),
    ]
    edges = [
        edge("module:src", "file:src/app.py", "contains"),
        edge("file:src/app.py", "function:src/app.py:main", "contains"),
        edge("file:README.md", "document:README.md:Project", "contains"),
        # Details is a heading INSIDE Project, which is what must rank it lower.
        edge("document:README.md:Project", "document:README.md:Project.Details",
             "contains"),
        edge("function:src/app.py:main", "function:src/app.py:helper", "calls"),
        edge("function:src/app.py:main", "function:src/app.py:shared", "calls"),
        edge("function:src/app.py:helper", "function:src/app.py:shared", "calls"),
        edge("function:tests/test_app.py:test_main", "function:src/app.py:main",
             "calls"),
        edge("function:tests/test_app.py:test_main", "function:src/app.py:helper",
             "calls"),
    ]
    layers = [
        layer("src", "src", ["module:src", "file:src/app.py"]),
        layer("db", "db", ["table:db/schema.sql:orders"]),
    ]
    return nodes, edges, layers


def titles(tour: list[dict]) -> list[str]:
    return [s["title"] for s in tour]


def step_named(tour: list[dict], fragment: str) -> dict:
    return next(s for s in tour if fragment.lower() in s["title"].lower())


# ── shape ───────────────────────────────────────────────────────────────────

def test_every_step_is_well_formed(graph):
    tour = build_tour(*graph)
    assert tour
    for i, step in enumerate(tour, start=1):
        assert step["order"] == i, "order must be 1..n with no gaps"
        assert step["title"].strip()
        assert step["description"].strip()
        assert isinstance(step["nodeIds"], list)
        assert all(isinstance(n, str) for n in step["nodeIds"])
        assert len(step["nodeIds"]) <= MAX_PILLS


def test_pills_name_nodes_that_exist(graph):
    nodes, _, _ = graph
    ids = {n["id"] for n in nodes}
    for step in build_tour(*graph):
        assert set(step["nodeIds"]) <= ids, step["title"]


# ── determinism ─────────────────────────────────────────────────────────────

def test_shuffled_input_yields_an_identical_tour(graph):
    """
    The claim the whole module rests on: same graph in, same tour out.

    Shuffled rather than repeated, because a repeated call passes even when the
    result depends on insertion order — and the builder's node order is not
    guaranteed to survive a re-index that touches one file.
    """
    nodes, edges, layers = graph
    baseline = build_tour(nodes, edges, layers)
    rng = random.Random(20260805)
    for _ in range(8):
        shuffled_nodes = nodes[:]
        shuffled_edges = edges[:]
        shuffled_layers = layers[:]
        rng.shuffle(shuffled_nodes)
        rng.shuffle(shuffled_edges)
        rng.shuffle(shuffled_layers)
        assert build_tour(shuffled_nodes, shuffled_edges, shuffled_layers) == baseline


# ── a step with no subject is not emitted ───────────────────────────────────

def test_a_graph_with_nothing_in_it_yields_no_tour():
    """Not one closing step addressed to nobody — no tour at all."""
    assert build_tour([], [], []) == []


def test_steps_without_a_subject_are_omitted():
    """A project with no SQL must not get a step reading "0 database tables"."""
    nodes = [
        node("module:src", "module", "src"),
        node("file:src/app.py", "file", "app.py", filePath="src/app.py"),
    ]
    layers = [layer("src", "src", ["module:src", "file:src/app.py"])]
    tour = build_tour(nodes, [], layers)

    assert "How this codebase is laid out" in titles(tour)
    for absent in ("authors wrote", "execution starts", "depends on",
                   "data it works with", "configured"):
        assert not any(absent in t.lower() for t in titles(tour)), absent
    for step in tour:
        assert " 0 " not in step["description"]


def test_the_full_graph_produces_every_step(graph):
    assert titles(build_tour(*graph)) == [
        "How this codebase is laid out",
        "Read what the authors wrote",
        "Where execution starts",
        "What everything else depends on",
        "The data it works with",
        "How it is configured",
        "Where to go from here",
    ]


# ── the claims each step makes ──────────────────────────────────────────────

def test_entry_points_are_uncalled_callers_and_tests_rank_last(graph):
    step = step_named(build_tour(*graph), "execution starts")
    # `main` is called by the test, so the only true root here is the test.
    assert step["nodeIds"] == ["function:tests/test_app.py:test_main"]
    assert "1 of them is a test" in step["description"]
    # `shared` is called and calls nothing — the opposite of an entry point.
    assert "function:src/app.py:shared" not in step["nodeIds"]


def test_entry_points_state_how_much_of_the_call_graph_resolved(graph):
    """
    "Nothing calls this" is only as true as resolution is complete.

    The resolver withholds what it cannot justify, so the step has to publish
    the rate beside the claim rather than presenting a subset as the whole.
    """
    step = step_named(
        build_tour(*graph, {"callSites": 1000, "exact": 400, "scoped": 100}),
        "execution starts",
    )
    assert "50%" in step["description"]  # (400 exact + 100 scoped) / 1000
    assert "no resolved caller" in step["description"]
    assert "1,000 call sites" in step["description"]

    # With no statistics at all it must still qualify the claim, not drop it.
    bare = step_named(build_tour(*graph), "execution starts")
    assert "ambiguous" in bare["description"]


def test_the_busiest_step_does_not_repeat_the_entry_points(graph):
    tour = build_tour(*graph)
    starts = set(step_named(tour, "execution starts")["nodeIds"])
    busiest = set(step_named(tour, "depends on")["nodeIds"])
    assert starts and busiest
    assert not (starts & busiest)


def test_the_busiest_step_ranks_by_connections(graph):
    """
    Total connections, most first — and a tie broken by id, not by luck.

    `main` and `helper` both sit on three call edges; `shared` on two. The tie
    is what matters here: it must resolve the same way on every machine, which
    is why the id is part of the sort key rather than a leftover order.
    """
    step = step_named(build_tour(*graph), "depends on")
    assert step["nodeIds"] == [
        "function:src/app.py:helper",   # 3 edges, sorts before main on id
        "function:src/app.py:main",     # 3 edges
        "function:src/app.py:shared",   # 2 edges
    ]


def test_documentation_prefers_a_whole_document_to_a_heading_inside_one(graph):
    step = step_named(build_tour(*graph), "authors wrote")
    assert step["nodeIds"][0] == "document:README.md:Project"
    assert "What this is." in step["description"]


def test_a_summary_that_is_only_the_name_is_not_quoted_back(graph):
    """
    `_node` defaults summary to the node's own name (schema.ts rewrites a falsy
    one and files an issue per node). Repeating it would read as documentation.
    """
    step = step_named(build_tour(*graph), "data it works with")
    assert "`orders` — 2 columns" in step["description"]
    assert "orders — orders" not in step["description"]


def test_the_schema_step_counts_what_a_table_holds(graph):
    step = step_named(build_tour(*graph), "data it works with")
    assert "1 database table" in step["description"]
    assert "2 columns" in step["description"]


def test_configuration_names_settings_and_says_values_are_redacted(graph):
    step = step_named(build_tour(*graph), "configured")
    assert step["nodeIds"] == ["config:.env:API_URL"]
    assert "redacted" in step["description"]


# ── caps are stated, never silent ───────────────────────────────────────────

def test_a_truncated_pill_list_says_so():
    """A silent cap reads as "these are all of them", which is the one lie."""
    nodes = [node("module:src", "module", "src")]
    ids = ["module:src"]
    for i in range(30):
        node_id = f"config:.env:VAR_{i:02d}"
        nodes.append(node(node_id, "config", f"VAR_{i:02d}", filePath=".env"))
        ids.append(node_id)
    tour = build_tour(nodes, [], [layer("src", "src", ids)])

    step = step_named(tour, "configured")
    assert len(step["nodeIds"]) == MAX_PILLS
    assert f"Showing {MAX_PILLS} of 30" in step["description"]


def test_an_untruncated_list_says_nothing_about_showing(graph):
    step = step_named(build_tour(*graph), "configured")
    assert "Showing" not in step["description"]
