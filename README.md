# Praxevia Explorer

Understand a codebase by looking at it. Praxevia Explorer indexes source
repositories, builds a typed architecture graph from that index, and puts the
graph on the main surface with an agentic chat beside it.

> **Copyright © 2026 Praxevia. All rights reserved. Proprietary and confidential.**
> No use, copying, deployment or distribution without Praxevia's written approval.
> See [`LICENSE`](./LICENSE) and [`NOTICE`](./NOTICE). Keep the repository private;
> do not publish.

---

## What it does

- **Indexes** a repository into Postgres: chunks and embeddings for semantic
  search, plus symbols, calls, imports and inheritance.
- **Builds a graph deterministically** from that index — no model in the loop, so
  it runs unattended on a schedule and produces the same graph twice. Files,
  modules, classes, functions, database tables, protobuf services and RPCs,
  configuration variables and documentation all become typed nodes.
- **Writes its own walkthrough.** Learn mode's guided tour is derived from the
  built graph — how the code is laid out, what the authors documented, where
  execution starts, what everything depends on, the schema, the configuration —
  so every project has one the moment it is indexed, with no model involved.
- **Draws only edges it can justify.** Call resolution is ambiguous in real code;
  ambiguous and low-precision call sites are counted and withheld rather than
  guessed at, and the statistics are published alongside the graph.
- **Serves the graph with level of detail**, so a large repository stays
  interactive instead of shipping one enormous payload to the browser.
- **Answers questions about the code** in a chat dock that runs its own search
  tools against the index and cites what it found as chips that jump to the node.
- **Serves all of that over MCP**, so Claude Code and other terminal agents can
  search the index, walk the graph and ask the advisor without a browser.

Supported languages: Ada, Bash, C, C++, Java, JavaScript, Kotlin, Perl, Python,
Ruby, Rust, Scala, TypeScript, plus SQL/DDL, Protocol Buffers, Markdown and
dotenv.

---

## Requirements

- Docker with Compose
- 8 GB RAM for the indexer (it loads a local embedding model)
- Optional: an Anthropic-compatible API key for the chat; everything else works
  without one

---

## Quick start

```bash
# 1. Configure. LOGIN_USERNAME and LOGIN_PASSWORD are REQUIRED — the backend
#    refuses to start without them, and there is no default.
cp .env.example .env

# 2. Put the source trees to analyse under codebase/{project}/{stream}/
mkdir -p codebase/myproject/main
rsync -a --exclude=.git --exclude=node_modules /path/to/repo/ codebase/myproject/main/

# 3. Index. The first run builds the image and downloads the embedding model.
docker compose --profile docker-indexer up --build indexer

# 4. Build the graph the dashboard reads
docker compose run --rm \
  -e GRAPH_DIR=/data/graphs -v "$PWD/.graphs:/data/graphs" \
  indexer python -m graph_pass.builder myproject main

# 5. Run it
docker compose up -d
```

Then open the dashboard and sign in with the credentials from your `.env`.

Indexing is CPU-bound and slow on a first pass over a large repository;
subsequent runs are incremental and fast. On Apple silicon,
`indexer/run_corpus_native.sh` uses the GPU and is substantially quicker.

---

## Configuration

Every setting lives in `.env` — see [`.env.example`](./.env.example), which
documents each one. The essentials:

| Variable | Purpose |
|---|---|
| `LOGIN_USERNAME`, `LOGIN_PASSWORD` | Required. Sign-in for the dashboard. |
| `CODEBASE_DIR` | Where `{project}/{stream}/` source trees live. |
| `GRAPH_DIR` | Where built graphs are written and read. |
| `ANTHROPIC_API_KEY` | Optional. Enables the chat dock. |
| `ANTHROPIC_BASE_URL` | Optional. Point the chat at any compatible provider. |

**The sign-in is a single shared credential, not an identity system.** Do not
expose the service to an untrusted network without putting real authentication
in front of it.

---

## Layout

```
indexer/            Extraction, chunking and embedding.
  graph_pass/       The deterministic graph builder: node identity, edge
                    resolution and its confidence policy, layer assignment,
                    and the SQL / protobuf / markdown / dotenv extractors.
backend/            API, agentic chat and its search tools, the level-of-detail
                    graph API, project sources, access control, scheduler.
frontend/
  packages/core/    Graph data model, schema and search engine.
  packages/dashboard/  Graph canvas, panels, chat dock.
mcp-server/         MCP surface — the same product from a terminal agent.
e2e/                Browser tests. `python e2e/run.py --help`.
```

---

## From a terminal agent

Claude Code and other MCP harnesses can use a running deployment directly:
`ask_codebase` for the Insight Advisor, `graph_expand` and `graph_search` for
the architecture graph, and the eight indexed-search tools the advisor itself
runs. Copy `.mcp.json.example` to `.mcp.json` in whichever repository you want
to explore from — see [`mcp-server/README.md`](./mcp-server/README.md).

---

## Development

```bash
# Backend
cd backend && python -m pytest tests/ -q

# Graph builder and extractors
cd indexer && python -m pytest graph_pass/tests -q

# Dashboard
cd frontend/packages/dashboard && pnpm install && pnpm build:core
./node_modules/.bin/vitest run

# Browser tests — starts both servers itself, then stops them
python e2e/run.py --codebase /path/to/codebase e2e/chip_clickthrough.py
```

### Things that are easy to get wrong

- **A graph must emit at least one layer.** Zero layers renders an interactive
  but completely empty canvas with no error. The builder asserts this and fails
  loudly rather than shipping a blank screen.
- **Node ids must be stable across re-indexing.** No line numbers in ids;
  overloads are disambiguated by a deterministic ordinal. Id churn breaks
  selection, navigation history and every saved link.
- **Never draw an ambiguous edge.** A drawn edge is a fact users believe. Emit
  the confident bands, withhold the rest, publish the statistics.
- **A new symbol kind must be added to the builder's kind→type map.** Anything
  unmapped falls through to a default and quietly mislabels its nodes; the
  builder reports unmapped kinds and exits non-zero.
- **Rebuild the indexer image after changing it.** Compose builds it rather than
  mounting it, so an edit has no effect until you rebuild.
