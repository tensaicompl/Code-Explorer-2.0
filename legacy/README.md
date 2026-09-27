# Superseded implementation — read-only reference

Everything in this directory is the previous implementation. It is kept as a
reference while the replacement is built, and it is deleted in the retirement
task of the final phase. Treat it as read-only.

**Do not** fix bugs here, add features here, import from here into the new tree,
or copy code out of it. What carries forward carries forward as behaviour, not as
source: read the concept, then write the replacement fresh against its own
specification.

| Path | What it was |
|---|---|
| `indexer/` | Extraction, chunking, embeddings, and the deterministic graph builder — node identity, edge resolution and its confidence policy, layer assignment, and the route, protobuf, DDL, dotenv and markdown extractors |
| `backend/` | The API: level-of-detail graph endpoints, search tools, repository registry and sync worker, permissions and identity, credential and conversation storage, the business-flow loop |
| `frontend/` | The web interface: graph canvas, layer and container views, panels, search, chat dock |
| `mcp-server/` | The agent surface, as a proxy over the API |
| `e2e/` | The browser verification suite |
| `docker-compose.yml`, `.env.example`, `.mcp.json.example` | How that stack was run |

The parts that carry forward as behaviour, and where the replacement specifies
them, are listed in the plan's inventory of the starting point. In short: the
confidence policy of withholding what cannot be justified while counting
everything, node identity that does not depend on line numbers, level-of-detail
delivery so a browser never receives a whole graph, evidence attached to every
answer, unattended and deterministic operation with no model in the loop, and
the registry that pulls from Git hosts on a schedule and on webhook.

Two directories here are not first-party work: the interface code was ported from
an MIT-licensed project. See `THIRD_PARTY_NOTICES.md` at the repository root
before copying anything anywhere.

One task still runs this stack: the comparison that diffs the superseded graph
builder against the replacement on a golden repository, for triage. It uses
`docker-compose.yml` in this directory with the indexer profile.
