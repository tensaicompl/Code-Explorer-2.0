"""
The guided walkthrough, derived from the graph rather than written by a model.

Learn mode has always rendered from `graph.tour`, and the builder has always
emitted `"tour": []` — so every project showed "No tour available" under a hint
telling the reader to generate one, with nothing anywhere that could. Upstream
filled that field with an LLM enrichment pass. This does not: the graph already
knows which areas are largest, which symbols nothing calls into, which are
called from everywhere, what the schema is and how the thing is configured, and
those are the questions a newcomer actually opens a codebase with.

DETERMINISTIC, like the rest of the builder. Same graph in, same tour out, no
model, no cost, no network, and nothing to fail at three in the morning when the
nightly refresh runs. Every ranking sorts by (-metric, id) so ties break the
same way on every machine.

NOTHING IS INVENTED. Each step's prose is assembled from counts and from doc
comments already in the graph. Where a claim depends on call resolution — which
is deliberately incomplete, because the resolver withholds edges it cannot
justify — the step says so rather than presenting a resolved subset as the whole
truth. A tour that quietly overstates what it knows is worse than no tour, since
the reader has no way to tell which half is real.

A STEP WITH AN EMPTY SUBJECT IS NOT EMITTED. A project with no SQL gets no
schema step, one with no dotenv gets no configuration step. The alternative —
a step reading "0 database tables" — is how a tour teaches someone that it is
not worth reading.

WHAT A STEP COSTS THE READER. `nodeIds` render as clickable pills, so they are
capped: eight names invite a click, forty are a wall. The cap is per step and
the ranking decides what survives it.

KNOWN BOUND: on a graph past FULL_GRAPH_MAX_NODES (30 000) the API ships a
file-level projection instead of every node, and a pill naming a function will
not resolve against it — the panel shows the id and selecting it finds nothing.
The coarse steps (layout, documentation, schema, configuration) are unaffected,
because their nodes survive the projection.
"""

from __future__ import annotations

import re
from collections import defaultdict

# Pills per step. Enough to be a starting point, few enough to read.
MAX_PILLS = 8
# Layers named in the layout step's prose.
MAX_NAMED_LAYERS = 6
# Nodes described individually inside a step's prose.
MAX_DESCRIBED = 4

# Types a `calls` edge can point at — the same set the builder resolves against,
# and the only nodes for which "nothing calls this" means anything.
CALLABLE_TYPES = {"function", "class", "module"}

# Composition, in the order a reader wants it: the containers first, then what
# they hold, then the things that are not code at all.
TYPE_LABELS = [
    ("module", "module", "modules"),
    ("file", "file", "files"),
    ("class", "class", "classes"),
    ("function", "function", "functions"),
    ("table", "database table", "database tables"),
    ("service", "service", "services"),
    ("endpoint", "endpoint", "endpoints"),
    ("document", "documentation section", "documentation sections"),
    ("config", "configuration variable", "configuration variables"),
]


# A test is an entry point — nothing calls it — but a newcomer asking where
# execution starts does not mean the test suite. Ranked below real roots rather
# than hidden, because in a repository that is mostly tests they ARE the answer.
_TEST_PATH = re.compile(
    r"(^|/)(tests?|specs?|__tests__|testing)(/|$)|(^|/)(test_|spec_)|"
    r"(_test|_spec|\.test|\.spec)\.[a-z]+$",
    re.IGNORECASE,
)


def _is_test(node: dict) -> bool:
    return bool(_TEST_PATH.search(node.get("filePath", "")))


def _plural(count: int, singular: str, plural: str) -> str:
    return f"{count:,} {singular if count == 1 else plural}"


def _summary_of(node: dict) -> str:
    """
    A node's doc comment, or "" when it has none.

    `_node()` defaults `summary` to the node's own name rather than an empty
    string (schema.ts rewrites a falsy summary and files an auto-corrected
    issue per node). So a summary equal to the name carries no information and
    must not be quoted back as though it were documentation.
    """
    summary = (node.get("summary") or "").strip()
    if summary == node.get("name"):
        return ""
    # A markdown summary can begin with the bullet of the list it was lifted
    # from, which reads as "— - Fetches workflow data" once inlined here.
    return re.sub(r"^[-*•\s]+", "", summary)


def _describe(node: dict, suffix: str = "") -> str:
    doc = _summary_of(node)
    line = f"- `{node['name']}`"
    if suffix:
        line += f" — {suffix}"
    if doc:
        line += f" — {doc}" if not suffix else f". {doc}"
    return line


def _step(title: str, description: str, node_ids: list[str]) -> dict:
    """
    One step, with its pill list capped — and the cap stated when it bites.

    A silent truncation reads as "these are all of them", which is the one thing
    a walkthrough must not imply. Everything else in this builder publishes what
    it dropped; so does this.
    """
    if len(node_ids) > MAX_PILLS:
        description += (
            f"\n\nShowing {MAX_PILLS} of {len(node_ids):,} — search by name for "
            f"the rest."
        )
    return {
        "order": 0,  # assigned once the surviving steps are known
        "title": title,
        "description": description,
        "nodeIds": node_ids[:MAX_PILLS],
    }


def _representative(layer: dict, index: dict, children: dict) -> str | None:
    """The node that best stands for a layer: the biggest container in it."""
    members = [index[i] for i in layer.get("nodeIds", []) if i in index]
    for wanted in ("module", "file"):
        candidates = [n for n in members if n["type"] == wanted]
        if candidates:
            return min(candidates, key=lambda n: (-children.get(n["id"], 0), n["id"]))["id"]
    return members[0]["id"] if members else None


def _layout_step(nodes, layers, by_type, index, children) -> dict | None:
    if not layers:
        return None

    ranked = sorted(layers, key=lambda l: (-len(l.get("nodeIds", [])), l["id"]))
    named = ", ".join(
        f"**{l['name']}** ({len(l.get('nodeIds', [])):,})"
        for l in ranked[:MAX_NAMED_LAYERS]
    )
    if len(ranked) > MAX_NAMED_LAYERS:
        named += f", and {len(ranked) - MAX_NAMED_LAYERS:,} more"

    composition = [
        _plural(len(by_type[key]), singular, plural)
        for key, singular, plural in TYPE_LABELS
        if by_type[key]
    ]

    description = (
        f"{_plural(len(nodes), 'node', 'nodes')} across "
        f"{_plural(len(layers), 'area', 'areas')}, largest first: {named}.\n\n"
        f"It is made of {', '.join(composition)}.\n\n"
        "Each pill below opens that area's biggest part. Start with whichever "
        "name means something to you."
    )

    reps = [r for l in ranked[:MAX_PILLS]
            if (r := _representative(l, index, children))]
    return _step("How this codebase is laid out", description, reps)


def _documentation_step(by_type, parent_type) -> dict | None:
    docs = by_type["document"]
    if not docs:
        return None

    def rank(node: dict) -> tuple:
        path = node.get("filePath", "").lower()
        # Whole documents before sections of one. A document contained by
        # another document is a heading inside it, and ranking on section count
        # alone promotes "Error Handling" over the README that holds it.
        nested = 1 if parent_type.get(node["id"]) == "document" else 0
        # Then the file the authors meant as the front door: a README, and the
        # shallower the better — README.md before agent/README.md.
        return (nested,
                0 if "readme" in path.rsplit("/", 1)[-1] else 1,
                path.count("/"),
                -len(node.get("sections", [])),
                node["id"])

    ordered = sorted(docs, key=rank)
    lines = [_describe(n) for n in ordered[:MAX_DESCRIBED]]
    description = (
        f"The project documents itself in "
        f"{_plural(len(docs), 'section', 'sections')}. "
        "Written by the people who wrote the code, so it says what they thought "
        "mattered:\n\n" + "\n".join(lines)
    )
    return _step("Read what the authors wrote", description,
                 [n["id"] for n in ordered])


def _entry_points_step(nodes, calls_in, calls_out, resolution) -> dict | None:
    starts = [
        n for n in nodes
        if n["type"] in CALLABLE_TYPES
        and calls_out[n["id"]] > 0
        and calls_in[n["id"]] == 0
    ]
    if not starts:
        return None

    ordered = sorted(
        starts, key=lambda n: (_is_test(n), -calls_out[n["id"]], n["id"])
    )
    lines = [
        _describe(n, f"calls {_plural(calls_out[n['id']], 'other symbol', 'other symbols')}")
        for n in ordered[:MAX_DESCRIBED]
    ]
    tests = sum(1 for n in starts if _is_test(n))
    note = ""
    if tests:
        note = (f" {_plural(tests, 'of them is a test', 'of them are tests')}, "
                f"ranked last here.")

    description = (
        f"{len(starts):,} symbols call other code but are themselves called by "
        "nothing this build could resolve — a main, a request handler, a command."
        f"{note} Execution starts somewhere in here.\n\n" + "\n".join(lines) +
        "\n\n" + _resolution_caveat(resolution)
    )
    return _step("Where execution starts", description,
                 [n["id"] for n in ordered])


def _busiest_step(nodes, calls_in, calls_out, used) -> dict | None:
    ranked = [
        n for n in nodes
        if n["type"] in CALLABLE_TYPES
        and n["id"] not in used
        and calls_in[n["id"]] > 0
    ]
    if not ranked:
        return None

    ordered = sorted(
        ranked,
        key=lambda n: (-(calls_in[n["id"]] + calls_out[n["id"]]), n["id"]),
    )
    lines = [
        _describe(n, f"called from {_plural(calls_in[n['id']], 'place', 'places')}")
        for n in ordered[:MAX_DESCRIBED]
    ]
    description = (
        "The most connected code in the repository. These are what the rest of "
        "it leans on, so they are where a change is felt furthest and where "
        "reading pays off soonest:\n\n" + "\n".join(lines)
    )
    return _step("What everything else depends on", description,
                 [n["id"] for n in ordered])


def _schema_step(by_type) -> dict | None:
    tables, services, endpoints = (by_type["table"], by_type["service"],
                                   by_type["endpoint"])
    if not (tables or services or endpoints):
        return None

    parts = []
    if tables:
        parts.append(_plural(len(tables), "database table", "database tables"))
    if services:
        parts.append(_plural(len(services), "service", "services"))
    if endpoints:
        parts.append(_plural(len(endpoints), "endpoint", "endpoints"))

    ordered = (
        sorted(tables, key=lambda n: (-len(n.get("columns", [])), n["id"]))
        + sorted(services, key=lambda n: n["id"])
        + sorted(endpoints, key=lambda n: n["id"])
    )
    lines = []
    for node in ordered[:MAX_DESCRIBED]:
        held = node.get("columns") or node.get("fields") or []
        lines.append(_describe(
            node, _plural(len(held), "column", "columns") if held else ""
        ))

    description = (
        f"The shapes this code moves around: {', '.join(parts)}. Read these "
        "before the code that manipulates them — a schema is the shortest "
        "description of what a system is actually about:\n\n" + "\n".join(lines)
    )
    return _step("The data it works with", description,
                 [n["id"] for n in ordered])


def _configuration_step(by_type) -> dict | None:
    config = by_type["config"]
    if not config:
        return None

    ordered = sorted(config, key=lambda n: n["id"])
    description = (
        f"{_plural(len(config), 'setting', 'settings')} the deployment supplies "
        "from the environment. They are what changes between one running copy "
        "of this system and another.\n\n"
        "Names only: values are redacted before anything is indexed, so no "
        "secret from a `.env` reaches the index or this tour."
    )
    return _step("How it is configured", description,
                 [n["id"] for n in ordered])


def _closing_step() -> dict:
    return _step(
        "Where to go from here",
        "That is the shape of it. From here:\n\n"
        "- Click any node for its summary, neighbours and source\n"
        "- Search by name in the box at the top\n"
        "- Ask the Insight Advisor anything the graph does not answer — it "
        "searches the index itself and cites what it finds\n\n"
        "The graph draws only the edges it can justify, so a missing arrow "
        "means unproven, not absent.",
        [],
    )


def _resolution_caveat(resolution: dict) -> str:
    """
    State how much of the call graph is actually drawn.

    "Nothing calls this" is only as true as call resolution is complete, and the
    resolver deliberately withholds what it cannot justify. Publishing the rate
    beside the claim is the difference between a fact and an overstatement.
    """
    sites = resolution.get("callSites") or 0
    resolved = (resolution.get("exact") or 0) + (resolution.get("scoped") or 0)
    if not sites:
        return ("Calls are resolved conservatively: an ambiguous call site is "
                "counted, not drawn.")
    return (
        f"Read that as *no resolved caller*, not *no caller*: "
        f"{resolved / sites:.0%} of the {sites:,} call sites in this repository "
        f"resolved to a single target, and the ambiguous rest are counted "
        f"rather than guessed at."
    )


def build_tour(nodes: list[dict], edges: list[dict], layers: list[dict],
               resolution: dict | None = None) -> list[dict]:
    """
    Assemble the guided walkthrough for one graph.

    Returns TourStep dicts — order, title, description, nodeIds — ready for the
    `tour` collection. Steps whose subject does not exist in this graph are
    omitted, so the result is between one and seven steps long.
    """
    index = {n["id"]: n for n in nodes}
    by_type: dict[str, list[dict]] = defaultdict(list)
    for node in nodes:
        by_type[node["type"]].append(node)

    calls_in: dict[str, int] = defaultdict(int)
    calls_out: dict[str, int] = defaultdict(int)
    children: dict[str, int] = defaultdict(int)
    parent_type: dict[str, str] = {}
    for edge in edges:
        if edge["type"] == "calls":
            calls_out[edge["source"]] += 1
            calls_in[edge["target"]] += 1
        elif edge["type"] == "contains":
            children[edge["source"]] += 1
            owner = index.get(edge["source"])
            if owner:
                parent_type[edge["target"]] = owner["type"]

    steps: list[dict] = []
    used: set[str] = set()

    def add(step: dict | None) -> None:
        if step is None:
            return
        steps.append(step)
        used.update(step["nodeIds"])

    add(_layout_step(nodes, layers, by_type, index, children))
    add(_documentation_step(by_type, parent_type))
    add(_entry_points_step(nodes, calls_in, calls_out, resolution or {}))
    # After the entry points, so the two steps do not name the same symbols
    # twice — what starts execution and what everything leans on are different
    # questions, and a repeated pill teaches neither.
    add(_busiest_step(nodes, calls_in, calls_out, used))
    add(_schema_step(by_type))
    add(_configuration_step(by_type))
    if steps:
        add(_closing_step())

    for position, step in enumerate(steps, start=1):
        step["order"] = position
    return steps
