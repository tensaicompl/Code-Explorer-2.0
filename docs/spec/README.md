# Specifications

Normative documents. Each mirrors one section of the implementation plan, which is
not committed to this repository, and each is the authority that later work is
implemented against: an implementer reads the document, not the plan.

Changing any of them requires a specification change request recorded in
`docs/plan/ISSUES.md`. Where a change affects a stored format, the relevant version
constant in `crates/pdx-core/src/consts.rs` is bumped in the same commit.

| Document | Covers |
|---|---|
| `4.2-identity-and-bands.md` | Node, site and edge identity; the confidence bands; the node and edge kinds |
| `4.3-segment-file.md` | The immutable per-repository graph file, its schema and its determinism |
| `4.4-control-plane.md` | The relational schema, the publication transaction and retention |
| `4.5-indexing-pipeline.md` | The six stages from discovery to publication, and the resolution order |
| `4.6-precise-band.md` | Compiler-backed indexes, their sandbox, and the merge into a structural graph |
| `4.7-contracts-and-links.md` | Contract extraction per repository and the cross-repository link layer |
| `4.8-architecture-model.md` | The six levels, component detection, layer roles and annotations |
| `4.9-agent-surface.md` | The tools, their budgets and cursors, view handles, hooks and the local overlay |
| `4.10-access-control-and-audit.md` | Roles, repository permissions, the enforcement point, tokens and the audit trail |
| `4.11-interface.md` | The views, the frame budget, semantic zoom, tiles and layout |
| `4.12-analytics.md` | Metrics, rules, cycles, blast radius, drift, hotspots, ownership, duplication, freshness |
| `4.13-trust-and-benchmark.md` | How accuracy is measured and published, and what a human must review |
| `4.14-deployment-and-release.md` | Artefacts, supply chain, versioning and the outbound network policy |

The component map that opens the plan's architecture part is a diagram rather than
a specification and has no document here.
