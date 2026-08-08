"""
Business-flow synthesis — turn a codebase graph into domains -> flows -> steps.

WHAT THIS PRODUCES. A second graph, in the same schema as the codebase graph but
using the three DOMAIN node types (`domain`, `flow`, `step`) and the three domain
edge types (`contains_flow`, `flow_step`, `cross_domain`). The dashboard already
renders exactly this: `DomainGraphView.tsx` draws domain clusters joined by
`cross_domain` edges, and drills into one domain to show its flows and their
ordered steps. Nothing in the frontend needs to change to display the output.

DETERMINISTIC, like `layers.py` and `tour.py`. Same graph in, same flows out: no
model, no network, no cost, nothing to fail during a nightly refresh. Every
ranking sorts by an explicit tie-break so two runs on two machines agree.

NOTHING IS INVENTED. A domain is a layer the builder already detected. A flow is
an entry point the graph already contains. A step is a call the resolver already
justified. Where a name would have to be guessed at, the node's own name is used
verbatim rather than paraphrased into something that reads better and means less.
The honest limit of that policy is stated plainly: this pass finds the SHAPE of
the business flows, not their business VOCABULARY. It will tell you that
`create_order` calls `validate_cart` then `reserve_stock`; it will not tell you
that the domain is called "Order Management" unless a layer already said so. That
second half is what the LLM mode is for.

WHY LAYERS BECOME DOMAINS. `layers.py` already solves "which parts of this
codebase belong together", deterministically, and its answer is the one the rest
of the product already shows. Deriving a second, competing grouping here would
mean the domain view and the structural view disagreed about the same codebase,
which is worse than being coarse.

WHY CALL DEPTH IS CAPPED. A flow is a story about a request, and a story with 400
steps is not one. The caps below are the point at which a reader stops reading,
not a limit of the traversal.
"""

from __future__ import annotations


from collections import deque
from datetime import datetime, timezone

# A flow's steps are a narrative, not a call trace. These bound it to something
# a person reads rather than scrolls past.
MAX_FLOWS_PER_DOMAIN = 6
MAX_STEPS_PER_FLOW = 12
MAX_STEP_DEPTH = 4
# Domains with nothing in them are noise on the overview canvas.
MIN_NODES_PER_DOMAIN = 2

# Types a `calls` edge can meaningfully point at — the same set `tour.py` uses,
# and the only nodes for which "nothing calls this" identifies an entry point.
CALLABLE_TYPES = {"function", "class", "module"}
# Nodes that ARE an entry point by construction, no inference required.
ENTRY_TYPES = {"endpoint"}

# IDS ARE BUILT FROM SOURCE IDS VERBATIM, not slugged.
#
# The domain-graph convention is kebab-case ids, and following it here was a real
# bug: kebab-casing is lossy, and the source graph's ids are not. In `awp`,
# `function:src/platform/api/v1/tool_secrets.py:test_tool_connection` and
# `...:_test_tool_connection` differ only by a leading underscore, which any
# `[^a-z0-9]+ -> "-"` slug plus a trim collapses to one string. Two distinct
# functions became one step id, and `validateGraph` would have kept whichever it
# saw last and silently dropped the other node's edges.
#
# Source ids are already unique by construction, so composing them
# (`step:<flow id>:<node id>`) is unique by construction too. Ids are internal —
# the dashboard displays `name` — so nothing is lost by them being ugly, and
# correctness is not traded for cosmetics.


def _first_line(text: str, limit: int = 200) -> str:
    """First line of a summary, bounded — step cards show one line."""
    line = (text or "").strip().split("\n", 1)[0]
    return line[:limit]


def _flow_step_weight(index: int, total: int) -> float:
    """
    Encode a step's 1-based position within a flow's `total` steps.

    `weight` is `z.number().min(0).max(1)` (schema.ts:448) — a normalised
    position, not a slot number. The previous rule rounded to one decimal and
    stepped by 0.1, which works for at most ten steps: an eleventh needs an
    eleventh increment, lands on 1.1, and `autoFixGraph` clamps it back onto the
    tenth step's 1.0 (schema.ts:374-377). Two steps, one weight, order lost.

    `index / (total + 1)` never rounds and is strictly between 0 and 1 for every
    index in [1, total], so it never reaches that clamp and never collides, for
    any total. The renderer recovers position by RANK — sorting a flow's
    `flow_step` edges by ascending weight — so this denominator is an
    implementation detail it never has to agree with. It only has to agree that
    the sequence increases.
    """
    return index / (total + 1)


def _node(node_id: str, node_type: str, name: str, summary: str, **extra) -> dict:
    """
    One graph node with every field `GraphNodeSchema` requires.

    `summary`, `tags` and `complexity` are REQUIRED by the schema, not optional
    (schema.ts:433-435) — a node missing any of them is dropped wholesale by
    `validateGraph`, silently taking its edges with it via the referential
    -integrity check. Defaulting them here is what keeps that from happening one
    forgotten keyword argument at a time.
    """
    return {
        "id": node_id,
        "type": node_type,
        "name": name,
        "summary": summary,
        "tags": extra.pop("tags", []),
        "complexity": extra.pop("complexity", "moderate"),
        **extra,
    }


def _edge(source: str, target: str, edge_type: str, weight: float, **extra) -> dict:
    """One graph edge with every field `GraphEdgeSchema` requires."""
    return {
        "source": source,
        "target": target,
        "type": edge_type,
        "direction": "forward",
        "weight": weight,
        **extra,
    }


def _index(graph: dict) -> tuple[dict, dict, dict, dict]:
    """Return (nodes_by_id, calls_out, calls_in, declared_by) for the source graph."""
    nodes_by_id = {n["id"]: n for n in graph.get("nodes", [])}
    calls_out: dict[str, list[str]] = {}
    calls_in: dict[str, list[str]] = {}
    declared_by: dict[str, str] = {}
    for e in graph.get("edges", []):
        src, tgt = e.get("source"), e.get("target")
        if src not in nodes_by_id or tgt not in nodes_by_id or src == tgt:
            continue
        etype = e.get("type")
        if etype == "calls":
            calls_out.setdefault(src, []).append(tgt)
            calls_in.setdefault(tgt, []).append(src)
        elif etype == "contains" and nodes_by_id[src].get("type") in CALLABLE_TYPES:
            # An HTTP route is CONTAINED BY its handler function — that is how
            # http_routes.py reports it, via parent_name. A file also contains
            # the route, which is why only callable parents are recorded: the
            # handler is where the flow's steps actually begin.
            declared_by.setdefault(tgt, src)
    return nodes_by_id, calls_out, calls_in, declared_by


def _domain_of_node(graph: dict, nodes_by_id: dict) -> tuple[list[dict], dict]:
    """
    Map the source graph's layers onto domains, and each node onto its domain.

    FIRST-MATCHING-LAYER WINS, mirroring `store.py`'s `layer_of_node` and the
    dashboard's own `nodeIdToLayerId` (store.ts:75-96). A node in two layers gets
    one domain, and it is the same one every consumer already picked.
    """
    domains: list[dict] = []
    domain_of: dict[str, str] = {}
    seen_domains: set[str] = set()

    for layer in graph.get("layers", []):
        members = [nid for nid in layer.get("nodeIds", []) if nid in nodes_by_id]
        if len(members) < MIN_NODES_PER_DOMAIN:
            continue
        domain_id = f"domain:{layer.get('id') or layer.get('name', '')}"
        if domain_id in seen_domains:
            continue
        seen_domains.add(domain_id)
        domains.append(
            _node(
                domain_id,
                "domain",
                layer.get("name") or layer.get("id", "Domain"),
                layer.get("description") or f"{len(members)} nodes in this area of the codebase.",
                tags=["layer-derived"],
                domainMeta={"entities": [], "businessRules": [], "crossDomainInteractions": []},
            )
        )
        for nid in members:
            domain_of.setdefault(nid, domain_id)

    return domains, domain_of


def _flow_roots(members: list[str], nodes_by_id: dict, calls_out: dict, calls_in: dict) -> list[str]:
    """
    Pick the nodes a domain's flows start from.

    Two tiers, in order of how much inference each requires:

    1. `endpoint` nodes. These ARE entry points — a gRPC rpc or a GraphQL field
       is one by definition, and once HTTP route extraction lands its routes
       arrive here too. No inference at all.
    2. Callables nothing calls. If a function in this layer has no incoming
       `calls` edge but does call others, something outside the resolved graph
       reaches it — a framework, a CLI, a test, an unresolved dynamic dispatch.
       That is the best available structural evidence for "this is where control
       enters", and it is evidence, not a guess.

    Tier 2 is only consulted when tier 1 is empty, so a repo with real endpoints
    never has them diluted by heuristics. Ranking is by outgoing-call count
    descending — a root that calls nothing produces a one-step flow, which is a
    fact about the graph, not a story — with the node id as an explicit
    tie-break so the ordering is stable across runs and machines.
    """
    endpoints = sorted(
        nid for nid in members if nodes_by_id[nid].get("type") in ENTRY_TYPES
    )
    if endpoints:
        return endpoints[:MAX_FLOWS_PER_DOMAIN]

    candidates = [
        nid
        for nid in members
        if nodes_by_id[nid].get("type") in CALLABLE_TYPES
        and not calls_in.get(nid)
        and calls_out.get(nid)
    ]
    candidates.sort(key=lambda nid: (-len(calls_out.get(nid, [])), nid))
    return candidates[:MAX_FLOWS_PER_DOMAIN]


def _walk(root: str, calls_out: dict, nodes_by_id: dict) -> list[str]:
    """
    Breadth-first walk from a flow's root along resolved `calls` edges.

    Breadth-first rather than depth-first because a flow reads as "what this
    entry point does, in order", and BFS keeps the first-level actions adjacent
    instead of diving to the bottom of the first branch and coming back. Callees
    are visited in sorted order so the walk is reproducible.

    The root is step 1. Nodes already visited are not revisited, so a cycle
    terminates rather than looping.
    """
    order: list[str] = []
    seen = {root}
    queue: deque[tuple[str, int]] = deque([(root, 0)])

    while queue and len(order) < MAX_STEPS_PER_FLOW:
        nid, depth = queue.popleft()
        order.append(nid)
        if depth >= MAX_STEP_DEPTH:
            continue
        for callee in sorted(set(calls_out.get(nid, []))):
            if callee in seen or callee not in nodes_by_id:
                continue
            seen.add(callee)
            queue.append((callee, depth + 1))

    return order


def synthesize_business_flow_deterministic(graph: dict) -> dict:
    """
    Build the business-flow graph from a codebase graph, without a model.

    Returns a document in the same schema the dashboard already validates and
    renders. `layers` and `tour` are empty by design: the domain view has its own
    two-level navigation (domain overview, then flows and steps within one
    domain) and reads neither.
    """
    nodes_by_id, calls_out, calls_in, declared_by = _index(graph)
    domains, domain_of = _domain_of_node(graph, nodes_by_id)

    members_by_domain: dict[str, list[str]] = {}
    for nid, did in domain_of.items():
        members_by_domain.setdefault(did, []).append(nid)

    nodes: list[dict] = list(domains)
    edges: list[dict] = []

    for domain in domains:
        did = domain["id"]
        members = sorted(members_by_domain.get(did, []))
        for root in _flow_roots(members, nodes_by_id, calls_out, calls_in):
            root_node = nodes_by_id[root]
            flow_id = f"flow:{root}"

            # An endpoint node carries no `calls` edges of its own — it is a
            # route registration, not a caller. Walking from it alone produced a
            # one-step flow that said only "this URL exists". Starting the walk
            # at the HANDLER, with the route itself as step 1, is what turns an
            # endpoint into an actual process.
            handler = declared_by.get(root)
            if handler is not None and not calls_out.get(root):
                walk = [root] + _walk(handler, calls_out, nodes_by_id)
                walk = walk[:MAX_STEPS_PER_FLOW]
            else:
                walk = _walk(root, calls_out, nodes_by_id)
            entry_type = "http" if root_node.get("type") in ENTRY_TYPES else "manual"

            nodes.append(
                _node(
                    flow_id,
                    "flow",
                    root_node.get("name") or root,
                    f"Starts at {root_node.get('name') or root} and reaches "
                    f"{len(walk) - 1} further step(s).",
                    tags=["call-derived"],
                    domainMeta={
                        "entryPoint": root_node.get("name") or root,
                        "entryType": entry_type,
                    },
                )
            )
            edges.append(_edge(did, flow_id, "contains_flow", 1.0))

            total = len(walk)
            for i, nid in enumerate(walk, start=1):
                src = nodes_by_id[nid]
                step_id = f"step:{root}:{nid}"
                step = _node(
                    step_id,
                    "step",
                    src.get("name") or nid,
                    _first_line(src.get("summary"))
                    or f"{src.get('type', 'node')} {src.get('name') or nid}.",
                    tags=[src.get("type", "node")],
                )
                # Carried through so clicking a step opens the real file. The
                # source node is the only place this is known.
                if src.get("filePath"):
                    step["filePath"] = src["filePath"]
                if isinstance(src.get("lineRange"), list) and len(src["lineRange"]) == 2:
                    step["lineRange"] = src["lineRange"]
                nodes.append(step)
                edges.append(
                    _edge(flow_id, step_id, "flow_step", _flow_step_weight(i, total))
                )

    edges.extend(_cross_domain_edges(domain_of, calls_out, {d["id"] for d in domains}))

    project = dict(graph.get("project") or {})
    return {
        "version": "1.0.0",
        "project": {
            "name": str(project.get("name") or "Project"),
            "languages": list(project.get("languages") or []),
            "frameworks": list(project.get("frameworks") or []),
            "description": "Business flows derived from the codebase graph.",
            "analyzedAt": datetime.now(timezone.utc).isoformat(),
            "gitCommitHash": str(project.get("gitCommitHash") or ""),
        },
        "nodes": nodes,
        "edges": edges,
        "layers": [],
        "tour": [],
    }


def _cross_domain_edges(domain_of: dict, calls_out: dict, domain_ids: set[str]) -> list[dict]:
    """
    One edge per ordered pair of domains that call into each other.

    Weight carries how much traffic crosses, normalised against the busiest pair
    so the heaviest coupling is 1.0 and the schema's `[0, 1]` bound holds without
    a clamp. The count goes in `description`, which `DomainGraphView` renders as
    the edge label — a number a reader can act on, rather than a line thickness
    they have to compare by eye.
    """
    counts: dict[tuple[str, str], int] = {}
    for src, targets in calls_out.items():
        src_domain = domain_of.get(src)
        if src_domain is None:
            continue
        for tgt in targets:
            tgt_domain = domain_of.get(tgt)
            if tgt_domain is None or tgt_domain == src_domain:
                continue
            counts[(src_domain, tgt_domain)] = counts.get((src_domain, tgt_domain), 0) + 1

    if not counts:
        return []

    busiest = max(counts.values())
    return [
        _edge(
            src,
            tgt,
            "cross_domain",
            round(n / busiest, 4),
            description=f"{n} call(s)",
        )
        for (src, tgt), n in sorted(counts.items())
        if src in domain_ids and tgt in domain_ids
    ]
