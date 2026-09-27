"""
Tier 1 graph builder — THE novel component of this project.

Reads the existing search index (symbols_*, calls_*) and emits a
KnowledgeGraph with NO LLM involvement and NO parsing — Tier 1 is pure SQL over
tables the indexer already fills. (Tier 2's file parsers are a separate concern
and are where decision D7's Node sidecar applies; this module needs neither.)

Usage:
    DATABASE_URL=... python -m graph_pass.builder <project> <stream> [-o out.json]

NODE ID CONVENTIONS — CodeViewer, diff overlays, citation chips and
navigation history all key on these, so they must be stable across re-indexes:
    file:<rel-path>                function:<rel-path>:<name>
    module:<dir-path>              class:<rel-path>:<name>

NO LINE NUMBERS IN IDS. A symbol that moves must keep its id. Overloads are
disambiguated first by parent (Class.method), and only then by a positional
ordinal — parent-based disambiguation is stable under edits, positional is not,
so it is the last resort rather than the first.

Tier 1/2 emit summary="", tags=[], complexity="moderate" — schema-valid, and the
panels degrade to structure-only. Do NOT relax the schema to make these optional;
that forks the contract with every ported component.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from collections import defaultdict
from datetime import datetime, timezone

import psycopg2
import psycopg2.extras

try:  # importable as a module or runnable as a script
    from .layers import assign_layers
    from .resolver import DRAWABLE, LOW_PRECISION_EXTENSIONS, CalleeResolver
    from .tour import build_tour
except ImportError:  # pragma: no cover
    from layers import assign_layers
    from resolver import DRAWABLE, LOW_PRECISION_EXTENSIONS, CalleeResolver
    from tour import build_tour

# Upstream symbols_*.kind vocabulary is exactly {package, procedure, function,
# class, constructor, method}. sql_ddl.py adds {table, view, index, column,
# references} on top; the declarative extractors add the rest.
CLASS_KINDS = {"class", "package", "message", "enum"}
TABLE_KINDS = {"table", "view"}

# THE KIND -> NODE TYPE MAP. Read this before adding an extractor.
#
# This used to be three inline branches — TABLE_KINDS, anything containing
# "class", and everything else -> `function`. That default is what made Rust's
# `impl_item` and `mod_item` arrive as functions, and it would have
# turned every protobuf message and every markdown heading into a function too:
# the graph would look populated, the type counts would look plausible, and
# DOCS / CONFIG / INFRA would still filter nothing.
#
# So the mapping is now a table, and a kind that is in NEITHER this table NOR
# FUNCTION_KINDS is reported at the end of the build (see unmapped_kinds). A new
# extractor whose kinds were never wired up is loud, not invisible.
#
# The node type decides which toolbar chip a node answers to —
# NODE_TYPE_TO_CATEGORY, GraphView.tsx:82:
#     file/function/class/module/concept -> CODE      config    -> CONFIG
#     document                           -> DOCS      table/endpoint/schema -> DATA
#     service/resource/pipeline          -> INFRA     domain/flow/step      -> DOMAIN
KIND_TO_TYPE = {
    **{k: "class" for k in CLASS_KINDS},
    **{k: "table" for k in TABLE_KINDS},
    "service": "service",     # protobuf service  -> INFRA
    "rpc": "endpoint",        # protobuf rpc      -> DATA
    "route": "endpoint",      # HTTP/REST route   -> DATA
    "document": "document",   # markdown heading  -> DOCS
    "env_var": "config",      # dotenv variable   -> CONFIG
    # Rust `mod_item`. Found by the unmapped-kinds report the first time it ran:
    # 1 779 of ff's nodes were Rust modules drawn as functions, which is the
    # trap still live in the data rather than a new one. Node ids for
    # those change from `function:` to `module:` on the next build — a one-time
    # correction, and the reason ids are rebuilt from source rather than stored.
    "module": "module",
}

# Kinds that legitimately fall through to the `function` default. Anything else
# reaching the default is a wiring bug, not a callable.
FUNCTION_KINDS = {"function", "procedure", "method", "constructor"}
DEFAULT_NODE_TYPE = "function"

# Kinds that are metadata on their owner rather than nodes of their own, mapped
# to the node field they fold into. A 40-column table must cost one graph node,
# not forty — and the same argument applies to a 30-field protobuf message
# and to the level-3 headings of a long README.
FOLDED_KINDS = {
    "column": "columns",
    "field": "fields",
    "enum_value": "fields",
    "subheading": "sections",
}

# Not nodes: a reference from one definition to another. Each becomes a
# depends_on edge, and each resolves against ITS OWN candidate set — the value
# here is the set of symbol kinds a reference of that kind may point at.
#
# THE SETS MUST NOT BE MERGED. A foreign key means a table; a protobuf type
# reference means a message. Measured on ff: 26 message names also name a Rust
# struct — the generated code for those very messages — and 16 table names also
# name a class or a message. One shared namespace would draw rpc -> Rust-struct
# edges, and drop the correct ones as ambiguous. A wrong edge on a graph is a
# fact users believe.
REFERENCE_KINDS = {
    "references": {"table", "view"},    # SQL foreign key
    "type_ref": {"message", "enum"},    # protobuf rpc payload
}
# Indexes are metadata on their table, not graph nodes.
NON_NODE_KINDS = set(FOLDED_KINDS) | set(REFERENCE_KINDS) | {"index"}

# Node types that can own members. When one file declares two things with the
# same name, a `parent_name` naming it means the one that can hold members.
CONTAINER_TYPES = {"class", "table", "service", "document", "module"}

# Node types a `calls` edge may point AT. Everything else — documents, config
# variables, tables, protobuf services and rpcs — is a node that nothing calls,
# and leaving it in the resolver's pool both invents edges and destroys real
# ones. See the CalleeResolver construction below for the measured cost.
CALLABLE_TARGET_TYPES = {"function", "class", "module"}

SANITISE = re.compile(r"[^a-z0-9]+")


def registry_key(project: str, stream: str) -> str:
    return f"{SANITISE.sub('_', project.lower())}_{SANITISE.sub('_', stream.lower())}"


def _node(node_id: str, node_type: str, name: str, **extra) -> dict:
    """
    A schema-valid node. summary/tags/complexity are required, not optional.

    summary defaults to the node NAME, not "". The plan originally specified an
    empty string, which turned out to be wrong when actually run: schema.ts:292
    treats a falsy summary as missing, rewrites it to the name anyway, and emits
    an `auto-corrected` issue per node. On a 1,249-node graph that is 1,249
    entries in WarningBanner on every load — enough to bury a real warning.

    So do explicitly what the validator would do. Tier 3 replaces it with an
    authored summary; until then the name is the honest placeholder, and it
    is exactly the fallback the earlier viewer chose.
    """
    return {
        "id": node_id,
        "type": node_type,
        "name": name,
        "summary": name,
        "tags": [],
        "complexity": "moderate",
        **extra,
    }


def _edge(source: str, target: str, edge_type: str, weight: float) -> dict:
    """
    A schema-valid edge. NOTHING may be added beyond these five keys —
    GraphEdgeSchema is a plain z.object() and silently strips unknown fields on
    the dashboard's first load, with no error. Confidence bands go in
    the sidecar, not here.
    """
    return {
        "source": source,
        "target": target,
        "type": edge_type,
        "direction": "forward",
        "weight": weight,
    }


def build_graph(conn, project: str, stream: str) -> tuple[dict, dict]:
    """Return (knowledge_graph, sidecar). The sidecar holds anything zod strips."""
    key = registry_key(project, stream)
    cur = conn.cursor(cursor_factory=psycopg2.extras.RealDictCursor)

    cur.execute(
        f'SELECT name, qualified_name, kind, filename, line_start, line_end, '
        f'parent_name, COALESCE(doc, \'\') AS doc '
        f'FROM "symbols_{key}" ORDER BY filename, line_start, name'
    )
    symbols = [dict(r) for r in cur.fetchall()]

    cur.execute(
        f'SELECT caller_name, caller_file, caller_line, callee_name FROM "calls_{key}"'
    )
    calls = [dict(r) for r in cur.fetchall()]

    # Imports are optional: a project indexed before has no such table,
    # and the resolver simply never fires its import-guided band.
    try:
        cur.execute(f'SELECT filename, module, kind, owner FROM "imports_{key}"')
        imports = [dict(r) for r in cur.fetchall()]
    except Exception:
        conn.rollback()
        imports = []

    cur.execute(f'SELECT DISTINCT filename FROM "code_chunks_{key}"')
    all_files = sorted({r["filename"] for r in cur.fetchall()})
    file_ids = set(all_files)

    nodes: list[dict] = []
    edges: list[dict] = []
    node_ids_by_path: dict[str, list[str]] = defaultdict(list)

    # ---- file nodes -------------------------------------------------------
    for path in all_files:
        nid = f"file:{path}"
        nodes.append(_node(nid, "file", path.rsplit("/", 1)[-1], filePath=path))
        node_ids_by_path[path].append(nid)

    # ---- module nodes (directories) ---------------------------------------
    for d in sorted({p.rsplit("/", 1)[0] for p in all_files if "/" in p}):
        nid = f"module:{d}"
        nodes.append(_node(nid, "module", d.rsplit("/", 1)[-1], filePath=d))
        node_ids_by_path[d + "/"].append(nid)

    for path in all_files:
        if "/" in path:
            edges.append(
                _edge(f"module:{path.rsplit('/', 1)[0]}", f"file:{path}", "contains", 1.0)
            )

    # ---- folded kinds and references are not emitted as nodes ----
    # {(file, owner_name): {"columns": [...], "fields": [...], "sections": [...]}}
    folded_of: dict[tuple[str, str], dict[str, list[str]]] = defaultdict(
        lambda: defaultdict(list)
    )
    references: list[tuple[str, str, str, str]] = []  # (kind, file, from, to)
    unmapped_kinds: dict[str, int] = defaultdict(int)

    for s in symbols:
        kind = (s["kind"] or "").lower()
        if kind in FOLDED_KINDS and s["parent_name"]:
            folded_of[(s["filename"], s["parent_name"])][FOLDED_KINDS[kind]].append(
                s["name"]
            )
        elif kind in REFERENCE_KINDS and s["parent_name"]:
            references.append((kind, s["filename"], s["parent_name"], s["name"]))

    # ---- symbol nodes -----------------------------------------------------
    seen_ids: dict[str, int] = defaultdict(int)
    symbol_node_id: dict[int, str] = {}
    # Where a reference can land, PER REFERENCE KIND: name -> {node ids}. A set,
    # not a first-wins dict, because two files declaring the same table or
    # message name is an AMBIGUOUS target, and the non-negotiable is that an
    # ambiguous edge is not drawn at all. Resolution requires exactly one.
    ref_targets: dict[str, dict[str, set[str]]] = {
        k: defaultdict(set) for k in REFERENCE_KINDS
    }
    symbol_node_type: dict[int, str] = {}

    for idx, s in enumerate(symbols):
        kind = (s["kind"] or "").lower()
        if kind in NON_NODE_KINDS:
            continue

        node_type = KIND_TO_TYPE.get(kind, DEFAULT_NODE_TYPE)
        if kind not in KIND_TO_TYPE and kind not in FUNCTION_KINDS:
            unmapped_kinds[kind] += 1

        base = f"{s['parent_name']}.{s['name']}" if s["parent_name"] else s["name"]
        nid = f"{node_type}:{s['filename']}:{base}"

        seen_ids[nid] += 1
        if seen_ids[nid] > 1:
            nid = f"{nid}#{seen_ids[nid]}"

        symbol_node_id[idx] = nid
        symbol_node_type[idx] = node_type

        extra = {"filePath": s["filename"]}
        if s["line_start"] and s["line_end"] and s["line_end"] >= s["line_start"]:
            extra["lineRange"] = [s["line_start"], s["line_end"]]

        # Node metadata is the ONLY place unknown fields survive validation —
        # GraphNodeSchema is .passthrough(); edges and the root strip them (R1).
        for field, values in folded_of.get((s["filename"], s["name"]), {}).items():
            if values:
                extra[field] = values

        if s.get("qualified_name") and s["qualified_name"] != s["name"]:
            extra["qualifiedName"] = s["qualified_name"]

        for ref_kind, target_kinds in REFERENCE_KINDS.items():
            if kind in target_kinds:
                ref_targets[ref_kind][s["name"]].add(nid)
                if s.get("qualified_name"):
                    ref_targets[ref_kind][s["qualified_name"]].add(nid)

        if node_type == "table":
            extra["tags"] = ["view"] if kind == "view" else ["table"]

        # The author's own doc comment becomes the summary (D3, solution 2).
        # Free, offline, and better than a generated one: whoever wrote the
        # function knew what it was for. `_node` keeps the name as the fallback,
        # which stays truthful when there is no doc.
        if s.get("doc"):
            extra["summary"] = s["doc"]

        nodes.append(_node(nid, node_type, s["name"], **extra))
        node_ids_by_path[s["filename"]].append(nid)

        if s["filename"] in file_ids:
            edges.append(_edge(f"file:{s['filename']}", nid, "contains", 0.9))

    # ---- (file, name) -> the node that owns that name ----------------------
    # Used for both reference sources and containment owners. When a file
    # declares a class and a function with the same name, the container wins:
    # a `parent_name` naming `Foo` means the thing that can hold members.
    node_by_file_name: dict[tuple[str, str], str] = {}
    for idx in sorted(
        symbol_node_id,
        key=lambda i: (symbol_node_type[i] not in CONTAINER_TYPES, i),
    ):
        node_by_file_name.setdefault(
            (symbols[idx]["filename"], symbols[idx]["name"]), symbol_node_id[idx]
        )

    # ---- references -> depends_on edges foreign keys, rpc types) --
    # A reference's SOURCE is always declared in the same file as the reference
    # itself: a FOREIGN KEY sits inside its CREATE TABLE, an rpc's request type
    # sits inside the rpc. Its TARGET may be anywhere, or nowhere.
    ambiguous_refs: dict[str, int] = defaultdict(int)
    for ref_kind, file, from_name, to_name in references:
        src = node_by_file_name.get((file, from_name))
        if not src:
            continue
        targets = ref_targets[ref_kind]
        # Try the reference exactly as written, then its last segment — a FK may
        # say `public.users` where the table was declared bare, and a proto rpc
        # may say `tact.types.b2b.Filter` where the message is `Filter`.
        candidates = targets.get(to_name) or targets.get(to_name.rsplit(".", 1)[-1])
        if not candidates:
            # Points outside this repo, or at something never declared. Skip
            # silently rather than inventing a node for it.
            continue
        if len(candidates) > 1:
            # Two declarations answer to this name. Drawing either would be a
            # coin flip presented as a fact — draw neither, and report it.
            #
            # This is stricter than shipped: FK resolution used to take the
            # first declaration by line order. That is the same coin flip with
            # the result hidden, and the whole point of is that a drawn edge
            # is believed. The count is published rather than swallowed.
            ambiguous_refs[ref_kind] += 1
            continue
        tgt = next(iter(candidates))
        if src != tgt:
            edges.append(_edge(src, tgt, "depends_on", 0.8))

    # ---- container -> member containment via parent_name -------------------
    # kind alone cannot express this: Python methods are recorded as 'function'
    # (measured: 851 function vs 12 method against 228 classes,, and Ada
    # line spans are frequently zero-length, so line containment is unusable.
    #
    # Every emitted node may own members, not only classes: a protobuf service
    # owns its rpcs, a level-1 markdown heading owns its level-2 headings, and a
    # nested message owns the message inside it. Restricting the lookup to
    # CLASS_KINDS, as this did, would have left every rpc and every sub-heading
    # attached to its file and to nothing else.
    for idx, s in enumerate(symbols):
        # Folded kinds and references have no node id, by construction.
        if not s["parent_name"] or idx not in symbol_node_id:
            continue
        owner = node_by_file_name.get((s["filename"], s["parent_name"]))
        if owner and owner != symbol_node_id[idx]:
            edges.append(_edge(owner, symbol_node_id[idx], "contains", 0.9))

    # ---- call edges -------------------------------------------------------
    # Only symbols that became nodes are resolution targets — folded columns and
    # FK references must never be the endpoint of a `calls` edge.
    #
    # AND ONLY CALLABLE ONES. A markdown heading called "Configuration" or a
    # variable called `LOG_LEVEL` is a node, but nothing calls it. Left in the
    # pool, a callee matching only a heading would resolve `exact` and draw a
    # FALSE edge from code into a README, and one matching both a function and a
    # heading turns ambiguous.
    #
    # MEASURED on awp when documents and config first entered the pool: ambiguous
    # call sites 15 -> 159 and reported resolution 62.9 % -> 58.5 %, while DRAWN
    # edges stayed at 1 315. So the observed cost was the published number and
    # 144 real call sites suppressed as ties — not, on this data, a false edge.
    # That is luck, not safety: one heading sharing a function's name is all it
    # takes, and's rule is that a drawn edge is believed.
    resolver = CalleeResolver(
        [
            {**s, "_idx": i}
            for i, s in enumerate(symbols)
            if i in symbol_node_id and symbol_node_type[i] in CALLABLE_TARGET_TYPES
        ],
        node_id_of=lambda s: symbol_node_id[s["_idx"]],
        imports=imports,
    )

    # A call's source: the symbol in caller_file whose name matches the last
    # segment of caller_name. caller_name is the FULL scope path while
    # qualified_name carries only one parent level, so they diverge at depth >= 3
    # — fall back to the file node rather than guessing.
    by_file_name: dict[tuple[str, str], list[str]] = defaultdict(list)
    for idx, s in enumerate(symbols):
        if idx in symbol_node_id:
            by_file_name[(s["filename"], s["name"].lower())].append(symbol_node_id[idx])

    call_edges: dict[tuple[str, str], float] = defaultdict(float)
    skipped_low_precision = 0

    for c in calls:
        caller_file = c["caller_file"]
        if os.path.splitext(caller_file)[1].lower() in LOW_PRECISION_EXTENSIONS:
            skipped_low_precision += 1
            continue

        target, band = resolver.resolve(
            c["callee_name"], caller_file, c.get("caller_name") or "",
        )
        if band not in DRAWABLE or not target:
            continue

        last = (c["caller_name"] or "").split(".")[-1].lower()
        owners = by_file_name.get((caller_file, last), [])
        if len(owners) == 1:
            source = owners[0]
        elif caller_file in file_ids:
            source = f"file:{caller_file}"
        else:
            continue

        if source == target:
            continue

        # Collapse multiplicities: one edge per pair, weight by call volume.
        call_edges[(source, target)] += 1.0

    max_calls = max(call_edges.values(), default=1.0)
    for (src, tgt), count in call_edges.items():
        edges.append(_edge(src, tgt, "calls", round(min(count / max_calls, 1.0), 4)))

    # ---- layers (MANDATORY — asserts internally, ---------------------
    layers = assign_layers(dict(node_ids_by_path))

    # The Learn panel's walkthrough. Derived from the graph that was just built,
    # so it needs the resolution statistics too — a step claiming "nothing calls
    # this" has to say how much of the call graph was resolvable at all.
    resolution = resolver.stats.as_dict()
    tour = build_tour(nodes, edges, layers, resolution)

    graph = {
        "version": "1.0.0",
        "kind": "codebase",  # never "knowledge" — that switch is sticky
        "project": {
            "name": f"{project}/{stream}",
            # `.env` splits to an empty extension (see chunking.classify_ext),
            # which would put a blank entry in the language list.
            "languages": sorted(
                {
                    ext
                    for p in all_files
                    if (ext := os.path.splitext(p)[1].lstrip("."))
                }
            ),
            "frameworks": [],
            "description": "",
            "analyzedAt": datetime.now(timezone.utc).isoformat(),
            "gitCommitHash": _git_hash(),
        },
        "nodes": nodes,
        "edges": edges,
        "layers": layers,
        # Required collection; empty is valid and absent is fatal. It is no
        # longer empty — see tour.py.
        "tour": tour,
    }

    node_types: dict[str, int] = defaultdict(int)
    for n in nodes:
        node_types[n["type"]] += 1

    sidecar = {
        "resolution": resolution,
        "skippedLowPrecisionCallSites": skipped_low_precision,
        "ambiguousReferences": dict(sorted(ambiguous_refs.items())),
        # Per-type counts, so "the DOCS filter is empty" is answerable from the
        # build output instead of by loading the UI and clicking.
        "nodeTypes": dict(sorted(node_types.items())),
        # Kinds that fell through KIND_TO_TYPE to the `function` default without
        # being callable. Empty is the only correct value; anything else is an
        # extractor whose kinds were never wired up, and its nodes are lying
        # about what they are.
        "unmappedKinds": dict(sorted(unmapped_kinds.items())),
        "counts": {
            "nodes": len(nodes),
            "edges": len(edges),
            "layers": len(layers),
            "files": len(all_files),
            "symbols": len(symbols),
            # A tour that silently came out empty is the "No tour available"
            # empty state again, reported nowhere. Count it like everything else.
            "tourSteps": len(tour),
        },
    }
    return graph, sidecar


def _git_hash() -> str:
    try:
        return subprocess.check_output(
            ["git", "rev-parse", "HEAD"], stderr=subprocess.DEVNULL, text=True
        ).strip()
    except Exception:
        return ""


def main() -> int:
    ap = argparse.ArgumentParser(description="Tier 1 graph builder")
    ap.add_argument("project")
    ap.add_argument("stream")
    ap.add_argument(
        "-o", "--out", default=None,
        help="default: $GRAPH_DIR/<project>_<stream>.json — the layout backend/app/"
             "graph/store.py reads. Override only for ad-hoc inspection.",
    )
    ap.add_argument("--sidecar", default=None,
                    help="default: alongside --out as <name>.meta.json")
    args = ap.parse_args()

    # Default to the shared location so builder output and API input cannot
    # drift apart. GRAPH_DIR is the one knob both sides read.
    key = registry_key(args.project, args.stream)
    graph_dir = os.environ.get("GRAPH_DIR", "/data/graphs")
    if args.out is None:
        os.makedirs(graph_dir, exist_ok=True)
        args.out = os.path.join(graph_dir, f"{key}.json")
    if args.sidecar is None:
        base = args.out[:-5] if args.out.endswith(".json") else args.out
        args.sidecar = f"{base}.meta.json"

    dsn = os.environ.get("DATABASE_URL")
    if not dsn:
        print("DATABASE_URL is not set", file=sys.stderr)
        return 2

    with psycopg2.connect(dsn) as conn:
        graph, sidecar = build_graph(conn, args.project, args.stream)

    os.makedirs(os.path.dirname(args.out) or ".", exist_ok=True)
    with open(args.out, "w") as fh:
        json.dump(graph, fh, indent=2)
    with open(args.sidecar, "w") as fh:
        json.dump(sidecar, fh, indent=2)

    r, c = sidecar["resolution"], sidecar["counts"]
    print(f"{args.out}: {c['nodes']} nodes, {c['edges']} edges, "
          f"{c['layers']} layers, {c['tourSteps']} tour steps")
    print(
        f"  resolution: {r['drawn']} edges drawn from {r['internalCandidates']} "
        f"internal candidates ({r['pctExactOfInternal']}% exact); "
        f"{r['ambiguous']} ambiguous + {r['blocked']} blocked NOT drawn; "
        f"{r['external']} external"
    )
    print("  node types: " + "  ".join(
        f"{t}={n}" for t, n in sidecar["nodeTypes"].items()
    ))
    if sidecar["ambiguousReferences"]:
        print("  references NOT drawn — the name matched more than one "
              "declaration: " + "  ".join(
                  f"{k}={n}" for k, n in sidecar["ambiguousReferences"].items()
              ))
    if sidecar["unmappedKinds"]:
        # Loud, and non-zero exit, because the alternative is a graph where a
        # protobuf message is labelled a function and nothing looks wrong.
        print(
            "  UNMAPPED SYMBOL KINDS — these became `function` nodes by default: "
            + ", ".join(f"{k}={n}" for k, n in sidecar["unmappedKinds"].items())
            + "\n  Add them to KIND_TO_TYPE (or FUNCTION_KINDS if they really are "
              "callables) in graph_pass/builder.py.",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
