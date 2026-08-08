"""
Tier-3 LLM enrichment — OPTIONAL. NEVER on the critical path.

Adds summary, tags, complexity, concept nodes, and authored layer names and
descriptions. The graph is fully usable without it: Tier 1/2 emit summary="",
tags=[], complexity="moderate", which is schema-valid and makes the panels degrade
to structure-only. Do NOT relax the schema to make these optional — that forks the
contract with every ported component.

SCOPE FOR v1 (decision D3): MODULES AND LAYERS ONLY — roughly 1,500 nodes instead
of ~91,500. Two orders of magnitude cheaper, and it covers where summaries are
actually read. Generate file- and symbol-level summaries ON DEMAND when a node is
opened, then cache; that turns a batch cost into a marginal one.

Resumable, budget-capped, idempotent. Only re-summarise nodes whose underlying
file hash changed.

Reuse Praxevia's Anthropic client, model allow-list and API-key handling. Do NOT reuse
the earlier viewer's chat proxy — its default model id is invalid.

Enrichment writes go ON NODES. GraphNodeSchema is .passthrough(), the only place
unknown fields survive validation; edges and the graph root silently strip them R1/R2/R3).
"""
