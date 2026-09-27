"""
Server-side graph API with level-of-detail —

WHY THIS EXISTS. The viewer this schema comes from ships the whole graph as one static knowledge-graph.json
fetched on load. Measured: 9.15 nodes and 57 call sites per file, so a
10,000-file codebase yields ~91,500 nodes and ~570,000 call sites — against a
documented dashboard design target of 100 nodes PER LAYER. Static JSON is not an
option, and no threshold closes a gap that size.

WHERE THE COST ACTUALLY LANDS — a sharper argument than raw counts.
Because detailLevel defaults to "file", only file nodes ever reach the renderer.
The dominant costs are payload and indexing, all on the main thread, all over the
FULL node set: fetch + JSON.parse, zod validateGraph(), new SearchEngine(graph.nodes)
(store.ts:367), and buildGraphIndexes (store.ts:75-96). ~95% of the payload is
parsed, validated and indexed to power search over data that is never drawn.

LOD CONTRACT — mirrors the two-stage container scheme the dashboard already uses, rather than inventing a second mental model:

    L0 layers    subsystem clusters + aggregated inter-layer edges   <=  60 nodes
    L1 modules   directories inside one layer                        <= 300
    L2 files     files inside one module                             <= 500
    L3 symbols   symbols in a file, or 1-2 hop neighbourhood         <= 800

L0 is capped at 60 because Overview lays out EVERY visible layer in ONE ELK call
with no lazy path.

RESPONSE SHAPE — verified, because the two obvious options both fail silently.
KnowledgeGraphSchema is a plain z.object(), so a new top-level key is stripped;
and ProjectMetaSchema (schema.ts:466-473) is ALSO plain with six fixed fields, so
hiding the envelope inside `project` fails identically. Use an HTTP envelope and
unwrap client-side before validateGraph():

    {"graph": {version, kind: "codebase", project, nodes, edges, layers, tour},
     "meta":  {truncated, totalAvailable, lod, resolution}}

`truncated` MUST surface in the UI. Silent truncation reads as "this is the whole
picture", which for a visualization is a correctness bug, not a UX nit.

EVERY response is a valid KnowledgeGraph fragment with layers populated and
tour: []. That keeps validateGraph() on the client path unchanged, which is what
lets the ported components work without modification.

AUTH: Praxevia's existing Bearer dependency and per-project access control. A user who
cannot chat about a project must not see its graph. That viewer's own auth — a single
process-lifetime shared secret (vite.config.ts:9-12) — is replaced, and TokenGate
is deleted.

COMPATIBILITY SHIMS — serve the five URLs the dashboard already fetches so
the port is a re-point, not a rewrite. /diff-overlay.json and /domain-graph.json
may 404; the client already degrades to null. But /knowledge-graph.json does NOT
check res.ok upstream (App.tsx:136-167), so a 403 surfaces to the user as
"Invalid knowledge graph" — add the guard.

PERFORMANCE: all aggregation in SQL. Precedent — trace_call_graph uses a recursive
CTE precisely because round-tripping was 30-50x slower. Materialise L0 and L1 into
graph_meta at index time; they change only when the index does.

Modules:
    router.py       /api/graph/* endpoints
    queries.py      LOD SQL: layers, modules, files, symbols, neighbourhood
    serializer.py   rows -> KnowledgeGraph fragments
    schema.py       Pydantic mirror of the KnowledgeGraph schema — the contract
    ddl.py          graph_nodes / graph_edges / graph_meta DDL
"""
