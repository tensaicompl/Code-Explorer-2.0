"""
Level-of-detail slicing —

Every function here returns a **complete, schema-valid KnowledgeGraph fragment**:
version, kind, project, nodes, edges, layers, tour. That is what lets the ported
dashboard components run unmodified — they call `validateGraph()` on whatever
arrives, and a fragment that is not a valid graph fails there rather than here.

TWO INVARIANTS EVERY FRAGMENT MUST HOLD, both learned the hard way:

  1. `layers` is never empty. A zero-layer graph renders a fully interactive,
     completely blank canvas with no error message. Each fragment carries
     the layer(s) its nodes belong to, so the invariant holds by construction.
  2. `summary` is never falsy. `schema.ts:292` rewrites a falsy summary to the
     node name and emits an `auto-corrected` issue per node — enough to bury a
     real warning in WarningBanner. The builder already guarantees this;
     nothing here may strip it.

Budgets come from They are caps on what is SENT, not on what exists;
`truncated` and `totalAvailable` tell the client the difference, and the client
is required to surface it. Silent truncation reads as "this is the whole
picture", which for a visualization is a correctness bug rather than a UX nit.
"""

from __future__ import annotations

import os

from .store import LoadedGraph

# The dashboard does its OWN level-of-detail. `drillIntoLayer` (store.ts:487) is
# pure client state — it sets `navigationLevel` and returns; it does not fetch.
# Every level below the overview is derived from the single graph already in the
# store, and `aggregateLayerEdges` (edgeAggregation.ts:21) recomputes the
# layer-to-layer connections from `graph.edges` on every render.
#
# So the graph handed to the dashboard must be COMPLETE. Serving it an L0
# fragment — module nodes only, edges filtered to that node set — leaves it
# nothing to aggregate: measured on a real 1,500-node index, 2,748 edges (1,050 of
# them `calls`, 215 crossing a layer boundary) reduced to **zero** surviving the
# fragment filter, which is exactly the disconnected-boxes overview a user sees.
#
# Above this many nodes the full graph stops being reasonable to ship, and
# `full_graph()` degrades to a file-level projection instead of silently
# truncating. 30k nodes is roughly a 3,000-file repo at the measured 9.4
# nodes/file.
FULL_GRAPH_MAX_NODES = int(os.environ.get("FULL_GRAPH_MAX_NODES", "30000"))

LOD_BUDGETS = {
    # L0 is capped hardest: Overview lays out EVERY visible layer in ONE ELK call
    # with no lazy path, so layer count must stay in the tens.
    "L0": 60,
    "L1": 300,
    "L2": 500,
    "L3": 800,
}

CONTAINER_TYPES = {"module", "file"}


def _fragment(loaded: LoadedGraph, nodes: list[dict], layers: list[dict],
              *, truncated: bool, total: int, lod: str) -> tuple[dict, dict]:
    """Assemble a fragment plus its out-of-band meta's HTTP envelope)."""
    node_ids = {n["id"] for n in nodes}
    # Containment inside the fragment, plus every semantic edge in the WHOLE
    # graph projected onto these nodes. Filtering raw edges to the node set
    # instead would leave every coarse view edgeless — `calls` is symbol-level,
    # so a fragment of modules or files contains none of them.
    edges = [
        e for e in loaded.graph.get("edges", [])
        if e["type"] == "contains" and e["source"] in node_ids and e["target"] in node_ids
    ] + lift_edges(loaded, node_ids)

    # Trim layer membership to the nodes actually present, then drop layers left
    # empty — a layer with no visible nodes is discarded by the dashboard anyway
    # (GraphView.tsx:283-298) and an all-empty set reproduces the blank canvas.
    trimmed = []
    for layer in layers:
        ids = [i for i in layer.get("nodeIds", []) if i in node_ids]
        if ids:
            trimmed.append({**layer, "nodeIds": ids})

    if not trimmed and nodes:
        # Should be unreachable, but a fragment with nodes and no layers is the
        # exact blank-canvas failure. Synthesise rather than ship it.
        trimmed = [{
            "id": "layer:all",
            "name": "All",
            "description": "",
            "nodeIds": sorted(node_ids),
        }]

    # NOTE the asymmetry: no nodes => no layers, deliberately. A truly empty
    # result (a search that matched nothing) must NOT be dressed up with an empty
    # layer — that would be a layer with zero nodeIds, which the dashboard drops
    # anyway (GraphView.tsx:283-298), reproducing the blank canvas by a longer
    # route. An empty fragment is the client's cue to render the empty
    # state, which is a different screen from a graph.

    graph = {
        "version": loaded.graph.get("version", "1.0.0"),
        "kind": "codebase",
        "project": loaded.project_meta,
        "nodes": nodes,
        "edges": edges,
        "layers": trimmed,
        "tour": [],
    }
    meta = {
        "lod": lod,
        "truncated": truncated,
        "totalAvailable": total,
        "returned": len(nodes),
        "resolution": loaded.sidecar.get("resolution", {}),
    }
    return graph, meta


# ---------------------------------------------------------------------------
# Edge lifting
#
# `calls` edges are symbol-to-symbol. Any view that shows something coarser than
# a symbol — modules in a layer, files in a module — therefore has no edges of
# its own and renders as disconnected boxes. Projecting each endpoint up the
# `contains` tree until it lands on a node that IS being shown is what turns
# "863 functions call each other somewhere" into "this module depends on that
# one".
# ---------------------------------------------------------------------------


def _parent_map(loaded: LoadedGraph) -> dict[str, str]:
    """child id -> parent id, from the `contains` tree. Cached on the graph."""
    cached = getattr(loaded, "_parent_map_cache", None)
    if cached is not None:
        return cached
    parents: dict[str, str] = {}
    for e in loaded.graph.get("edges", []):
        if e["type"] == "contains":
            # First parent wins. The builder emits a single containment parent
            # per node; if that ever stops being true, a stable choice beats a
            # last-writer-wins one that reshuffles between loads.
            parents.setdefault(e["target"], e["source"])
    loaded._parent_map_cache = parents  # type: ignore[attr-defined]
    return parents


def _lift(node_id: str, allowed: set[str], parents: dict[str, str]) -> str | None:
    """Walk up the containment chain to the nearest ancestor that is on screen."""
    seen: set[str] = set()
    cur: str | None = node_id
    while cur is not None and cur not in allowed:
        if cur in seen:
            return None  # a containment cycle; refuse rather than spin
        seen.add(cur)
        cur = parents.get(cur)
    return cur


def lift_edges(loaded: LoadedGraph, allowed: set[str]) -> list[dict]:
    """
    Every semantic edge in the graph, projected onto the visible node set.

    `contains` is excluded: lifting it produces a self-loop by construction, and
    the containment structure is already expressed by the node hierarchy.

    Deduplicated per (source, target, type). The number of underlying calls goes
    into `description`, NOT a new field — `GraphEdgeSchema` is a plain
    `z.object()` and silently strips unknown keys R1). `description` is an
    optional string it actually permits.
    """
    parents = _parent_map(loaded)
    agg: dict[tuple[str, str, str], int] = {}

    for e in loaded.graph.get("edges", []):
        if e["type"] == "contains":
            continue
        src = _lift(e["source"], allowed, parents)
        dst = _lift(e["target"], allowed, parents)
        if src is None or dst is None or src == dst:
            continue
        key = (src, dst, e["type"])
        agg[key] = agg.get(key, 0) + 1

    out: list[dict] = []
    for (src, dst, etype), count in agg.items():
        out.append({
            "source": src,
            "target": dst,
            "type": etype,
            "direction": "forward",
            # weight is z.number().min(0).max(1) — a normalised confidence, not a
            # count. Saturating at 10 underlying calls keeps thick edges thick
            # without letting one hot pair flatten everything else.
            "weight": min(1.0, count / 10.0),
            "description": f"{count} {etype}" if count > 1 else etype,
        })
    return out


def lift_tour(loaded: LoadedGraph, allowed: set[str]) -> list[dict]:
    """
    The walkthrough, with its pills projected onto the visible node set.

    The tour names symbols; the file-level projection has no symbol nodes. Left
    alone, every pill on a large repository would name a node that is not in the
    payload — the panel would render bare ids and clicking one would select
    nothing. Lifting each to its nearest visible container is the same move
    `lift_edges` makes, and for the same reason: the step still points somewhere
    real, one level coarser.

    The step text says so, because a pill that reads `app.py` where the prose
    named a function is otherwise just wrong.
    """
    parents = _parent_map(loaded)
    lifted_steps = []
    for step in loaded.graph.get("tour", []):
        original = step.get("nodeIds", [])
        ids: list[str] = []
        for node_id in original:
            target = _lift(node_id, allowed, parents)
            if target and target not in ids:
                ids.append(target)
        out = {**step, "nodeIds": ids}
        if ids != original:
            out["description"] = (
                step.get("description", "")
                + "\n\nThis repository is too large to ship every symbol, so the "
                  "components below name the files that contain them."
            )
        lifted_steps.append(out)
    return lifted_steps


def full_graph(loaded: LoadedGraph) -> tuple[dict, dict]:
    """
    The whole graph, which is what the ported dashboard actually needs.

    Degrades rather than truncates: past FULL_GRAPH_MAX_NODES the symbol layer is
    dropped and its call edges are lifted onto the surviving file nodes, so the
    view stays connected and honest instead of becoming a big empty one.
    `meta.projection` names which of the two you got.
    """
    nodes = loaded.graph.get("nodes", [])
    total = len(nodes)

    if total <= FULL_GRAPH_MAX_NODES:
        graph = {
            "version": loaded.graph.get("version", "1.0.0"),
            "kind": "codebase",
            "project": loaded.project_meta,
            "nodes": nodes,
            "edges": loaded.graph.get("edges", []),
            "layers": loaded.graph.get("layers", []),
            "tour": loaded.graph.get("tour", []),
        }
        meta = {
            "lod": "full",
            "projection": "symbol",
            "truncated": False,
            "totalAvailable": total,
            "returned": total,
            "resolution": loaded.sidecar.get("resolution", {}),
        }
        return graph, meta

    kept = [n for n in nodes if n["type"] in CONTAINER_TYPES]
    allowed = {n["id"] for n in kept}
    edges = [
        e for e in loaded.graph.get("edges", [])
        if e["type"] == "contains" and e["source"] in allowed and e["target"] in allowed
    ] + lift_edges(loaded, allowed)

    layers = []
    for layer in loaded.graph.get("layers", []):
        ids = [i for i in layer.get("nodeIds", []) if i in allowed]
        if ids:
            layers.append({**layer, "nodeIds": ids})

    graph = {
        "version": loaded.graph.get("version", "1.0.0"),
        "kind": "codebase",
        "project": loaded.project_meta,
        "nodes": kept,
        "edges": edges,
        "layers": layers,
        # NOT []. Dropping it here is how a large repository — the one whose
        # newcomer most needs a walkthrough — lands back on "No tour available".
        "tour": lift_tour(loaded, allowed),
    }
    meta = {
        "lod": "full",
        "projection": "file",
        "truncated": True,
        "totalAvailable": total,
        "returned": len(kept),
        "resolution": loaded.sidecar.get("resolution", {}),
    }
    return graph, meta


def overview(loaded: LoadedGraph) -> tuple[dict, dict]:
    """
    L0 — the default view.

    Returns the graph's REAL layers plus one representative node per layer, and
    lets the dashboard build the clusters itself. That is the correction to an
    earlier version which pre-aggregated into synthetic `agg:` nodes inside one
    synthetic layer: `useOverviewGraph` renders one cluster PER LAYER
    (GraphView.tsx:240-384), so a single synthetic layer collapsed the whole
    project into one box. Send layers, not clusters.

    Module nodes are the representatives because they are the coarsest real
    nodes that exist, so no synthetic ids leak into navigation or citations.
    """
    layers = loaded.graph.get("layers", [])
    budget = LOD_BUDGETS["L0"]
    shown = layers[:budget]
    truncated = len(layers) > budget

    nodes: list[dict] = []
    trimmed_layers: list[dict] = []
    for layer in shown:
        ids = layer.get("nodeIds", [])
        members = [loaded.nodes_by_id[i] for i in ids if i in loaded.nodes_by_id]
        reps = [n for n in members if n["type"] == "module"]
        if not reps:
            reps = [n for n in members if n["type"] == "file"][:1]
        if not reps:
            reps = members[:1]
        if not reps:
            continue  # an empty layer would be dropped by the dashboard anyway
        nodes.extend(reps)
        trimmed_layers.append({**layer, "nodeIds": [n["id"] for n in reps]})

    # Lifted, not filtered. Representatives are module nodes and `calls` edges
    # are symbol-level, so a raw filter yields zero edges every time — measured
    # 0 of 2,748 there. Lifting turns those calls into the module-to-module
    # dependencies this level is supposed to show.
    node_ids = {n["id"] for n in nodes}
    edges = lift_edges(loaded, node_ids)

    graph = {
        "version": loaded.graph.get("version", "1.0.0"),
        "kind": "codebase",
        "project": loaded.project_meta,
        "nodes": nodes,
        "edges": edges,
        "layers": trimmed_layers,
        "tour": [],
    }
    meta = {
        "lod": "L0",
        "truncated": truncated,
        "totalAvailable": len(layers),
        "returned": len(trimmed_layers),
        "resolution": loaded.sidecar.get("resolution", {}),
    }
    return graph, meta


def layer_detail(loaded: LoadedGraph, layer_id: str) -> tuple[dict, dict] | None:
    """L1 — module nodes inside one layer."""
    layer = next(
        (l for l in loaded.graph.get("layers", []) if l["id"] == layer_id), None
    )
    if layer is None:
        return None

    ids = layer.get("nodeIds", [])
    modules = [
        loaded.nodes_by_id[i]
        for i in ids
        if i in loaded.nodes_by_id and loaded.nodes_by_id[i]["type"] == "module"
    ]
    # A layer with no module nodes (a flat directory) still has to render, so
    # fall back to its files rather than returning an empty fragment.
    if not modules:
        modules = [
            loaded.nodes_by_id[i]
            for i in ids
            if i in loaded.nodes_by_id and loaded.nodes_by_id[i]["type"] == "file"
        ]

    budget = LOD_BUDGETS["L1"]
    return _fragment(
        loaded, modules[:budget], [layer],
        truncated=len(modules) > budget, total=len(modules), lod="L1",
    )


def module_detail(loaded: LoadedGraph, module_id: str) -> tuple[dict, dict] | None:
    """L2 — files contained by one module."""
    if module_id not in loaded.nodes_by_id:
        return None

    children = [
        loaded.nodes_by_id[e["target"]]
        for e in loaded.edges_by_source.get(module_id, [])
        if e["type"] == "contains" and e["target"] in loaded.nodes_by_id
    ]
    nodes = [loaded.nodes_by_id[module_id]] + children
    budget = LOD_BUDGETS["L2"]
    layers = _layers_for(loaded, [n["id"] for n in nodes])
    return _fragment(
        loaded, nodes[:budget], layers,
        truncated=len(nodes) > budget, total=len(nodes), lod="L2",
    )


def file_detail(loaded: LoadedGraph, file_id: str) -> tuple[dict, dict] | None:
    """L3 — symbols declared in one file."""
    if file_id not in loaded.nodes_by_id:
        return None

    children = [
        loaded.nodes_by_id[e["target"]]
        for e in loaded.edges_by_source.get(file_id, [])
        if e["type"] == "contains" and e["target"] in loaded.nodes_by_id
    ]
    nodes = [loaded.nodes_by_id[file_id]] + children
    budget = LOD_BUDGETS["L3"]
    layers = _layers_for(loaded, [n["id"] for n in nodes])
    return _fragment(
        loaded, nodes[:budget], layers,
        truncated=len(nodes) > budget, total=len(nodes), lod="L3",
    )


def neighbourhood(loaded: LoadedGraph, node_id: str, hops: int = 1) -> tuple[dict, dict] | None:
    """L3 focused — the n-hop neighbourhood of one node, in both directions."""
    if node_id not in loaded.nodes_by_id:
        return None

    frontier = {node_id}
    seen = {node_id}
    for _ in range(max(1, min(hops, 3))):
        nxt: set[str] = set()
        for nid in frontier:
            for e in loaded.edges_by_source.get(nid, []):
                nxt.add(e["target"])
            for e in loaded.edges_by_target.get(nid, []):
                nxt.add(e["source"])
        nxt -= seen
        seen |= nxt
        frontier = nxt
        if not frontier:
            break

    nodes = [loaded.nodes_by_id[i] for i in seen if i in loaded.nodes_by_id]
    budget = LOD_BUDGETS["L3"]
    layers = _layers_for(loaded, [n["id"] for n in nodes])
    return _fragment(
        loaded, nodes[:budget], layers,
        truncated=len(nodes) > budget, total=len(nodes), lod="L3",
    )


def search(loaded: LoadedGraph, query: str, limit: int = 50) -> tuple[dict, dict]:
    """Server-side node search across the whole graph, not just the loaded view."""
    q = (query or "").lower().strip()
    if not q:
        return _fragment(loaded, [], [], truncated=False, total=0, lod="search")

    hits = [
        n for n in loaded.graph.get("nodes", [])
        if q in n["name"].lower() or q in n.get("filePath", "").lower()
    ]
    # Prefer a name match over an incidental path match, then shorter names —
    # an exact symbol beats a file that merely contains the substring.
    hits.sort(key=lambda n: (q not in n["name"].lower(), len(n["name"])))
    layers = _layers_for(loaded, [n["id"] for n in hits[:limit]])
    return _fragment(
        loaded, hits[:limit], layers,
        truncated=len(hits) > limit, total=len(hits), lod="search",
    )


def _layers_for(loaded: LoadedGraph, node_ids: list[str]) -> list[dict]:
    """The layer objects that any of these nodes belong to."""
    wanted = {loaded.layer_of_node.get(i) for i in node_ids}
    wanted.discard(None)
    return [l for l in loaded.graph.get("layers", []) if l["id"] in wanted]
