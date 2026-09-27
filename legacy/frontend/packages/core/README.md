# `@prx/core` — browser-safe subset

Three files, ported unmodified from Architecture Insight:

| File | Why it is here |
|---|---|
| `types.ts` | The `KnowledgeGraph` / `GraphNode` / `GraphEdge` type model. 22 dashboard imports. |
| `schema.ts` | `validateGraph()` and the zod schemas. **The contract with the backend** — the graph builder and the graph API are both verified against this exact file, not a re-implementation. |
| `search.ts` | The Fuse.js index behind the search bar. |

## What was removed, and why

Upstream `@prx/core` also carries the tree-sitter analysis layer: 13 language
extractors, 12 non-code parsers, the plugin registry, the language and framework
registries, the analyzer, persistence and fingerprinting. That pulled in 12
tree-sitter grammar packages, `web-tree-sitter`, two workspace WASM packages and
`ignore`/`yaml` — none of which the dashboard imports.

**This project does its analysis in Python, in `indexer/graph_pass/`.** The
tree-sitter layer would have been dead weight in the frontend bundle and in every
`pnpm install`, and its two workspace WASM dependencies would have had to be
vendored to make the install resolve at all.

The dashboard imports exactly three subpaths — `./types` (22×), `./schema` (6×),
`./search` (2×) — which is what this package now exports.

## The removed code is not lost

The complete upstream package is preserved at **`frontend/reference/eai-core/`**,
outside the pnpm workspace so it is neither installed nor built. It is the port
source for plan §8.9 (Tier 2 parsers: SQL, Terraform, GraphQL, Protobuf, …), where
it would run as a Node sidecar rather than in the browser.

`indexer/graph_pass/sql_ddl.py` is the first such port and cites its origin by
line number.

## Rules

- `schema.ts` is the contract. If it changes, `indexer/graph_pass/builder.py` and
  `backend/app/graph/queries.py` must be re-verified against it — that is what
  caught the `summary: ""` defect (plan §4.16).
- Keep these three files free of Node built-ins. The upstream convention (`CLAUDE.md`
  in the earlier viewer) was that the dashboard must never import core's main entry point because it
  pulls in `node:fs`; here that risk is removed by construction, and it should stay
  removed.
