# Praxevia Explorer over MCP

Use Praxevia Explorer from Claude Code, or any other MCP-speaking agent, without
leaving the terminal. The server is a thin client: it talks to a **running**
Praxevia Explorer backend over HTTP and exposes what that backend already
serves — the search index, the architecture graph, and the Insight Advisor.

> Copyright © 2026 Praxevia. All rights reserved. Proprietary and confidential.
> See [`../LICENSE`](../LICENSE).

---

## Requirements

- A Praxevia Explorer backend you can reach, with at least one indexed project
- A token for it (below)
- Python 3.11+

```bash
pip install -r mcp-server/requirements.txt
```

## A token

Any Bearer token the backend accepts. In order of preference:

1. **A static key** — an entry in the backend's `API_KEYS`, as
   `key:client-name` (comma-separated for several). Stable across restarts,
   which is what a registered MCP server wants, and the right choice on a
   deployment using the shared sign-in.

   ```bash
   # in the backend's .env, then restart it
   API_KEYS=prx-<something-long-and-random>:claude-code
   ```

2. **A session token** — what `POST /api/login` returns for the dashboard
   credentials. Nothing to configure, but it changes every time the backend
   restarts and the registration has to be updated with it. `selftest.py` uses
   this one.

   ```bash
   curl -s -X POST http://127.0.0.1:3006/api/login \
     -H 'Content-Type: application/json' \
     -d '{"username":"...","password":"..."}'
   ```

3. **A per-user key** — `POST /api/api-keys`, named and revocable, and the only
   kind that records who is asking. Available only where the deployment is
   behind Entra SSO: creating one requires an SSO identity, which the shared
   sign-in is not.

## Register it with Claude Code

Copy [`../.mcp.json.example`](../.mcp.json.example) to `.mcp.json` in whichever
repository you want to explore from, and fill in the two absolute paths.
`.mcp.json` is gitignored here — it holds a token, so keep it out of any
repository you commit.

Claude Code expands `${VAR}` in that file, so the token can stay in your shell
environment rather than on disk:

```json
"PRAXEVIA_API_KEY": "${PRAXEVIA_API_KEY}"
```

Or register it without a file:

```bash
claude mcp add praxevia \
  -e PRAXEVIA_BACKEND_URL=http://127.0.0.1:3006 \
  -e PRAXEVIA_API_KEY=… \
  -- /path/to/praxevia-explorer/.venv/bin/python \
     /path/to/praxevia-explorer/mcp-server/praxevia_mcp_server.py
```

Then, in a session: `/mcp` lists what it exposes.

## Configuration

| Variable | Required | Purpose |
|---|---|---|
| `PRAXEVIA_BACKEND_URL` | yes | e.g. `http://127.0.0.1:3006` |
| `PRAXEVIA_API_KEY` | yes | Bearer token, as above |
| `PRAXEVIA_PROJECT` | no | Default project, when you mostly ask about one |
| `PRAXEVIA_STREAM` | no | Default stream (branch), e.g. `main` |
| `PRAXEVIA_CROSS_PROJECT` | no | `true` searches every project you may see |
| `PRAXEVIA_VERIFY_SSL` | no | `false` only for a self-signed development cert |
| `PRAXEVIA_TIMEOUT` | no | Seconds for ordinary calls (default 120) |
| `PRAXEVIA_CHAT_TIMEOUT` | no | Seconds for `ask_codebase` (default 600) |
| `PRAXEVIA_MAX_NODES` | no | Cap on nodes per graph call (default 200) |

`PRAXEVIA_PROJECT` and `PRAXEVIA_STREAM` are only defaults. Every tool takes
`project` and `stream` arguments, so one registered server covers every
repository the deployment holds.

These belong in `.mcp.json`, not in the backend's `.env` — they configure this
client, and the backend never reads them.

## The tools

**Orientation**

- `catalog_projects` — what this deployment holds, and for each whether it is
  `indexed` (search and chat work) and whether it `hasGraph` (the graph tools
  work). The two are built separately and legitimately disagree.
- `graph_overview` — what a codebase is: counts, composition by node and edge
  type, languages, the commit it was built from, where the source came from, and
  the edge-resolution statistics.

**The architecture graph**

- `graph_expand` — one tool for every level. No id gives the layers; a
  `layer:` id gives its modules; `module:` its files; `file:` its symbols;
  anything else the neighbourhood of that node. Node ids are stable across
  re-indexing.
- `graph_search` — find nodes by name or path, to get ids for `graph_expand`.

**Ask**

- `ask_codebase` — the Insight Advisor. Agentic: it runs its own searches and
  answers from what it finds. Slower than a single search, and better when the
  question needs several.

**Indexed search** — the same eight tools the advisor runs for itself, for when
you already know the shape of the search you want:

`browse_source_tree`, `search_by_meaning`, `find_files_by_name`, `read_source_excerpt`,
`regex_search_source`, `lookup_symbol_usage`, `walk_call_chain`,
`find_subsystem_dependencies`.

### Documentation, and citations

Graph nodes carry their own documentation, and `graph_expand` returns it:
`summary` is the symbol's doc comment, and `sections`, `columns` and `fields`
hold the headings, table columns and message fields that are folded onto a
parent node rather than exploded into nodes of their own.

`ask_codebase` answers cite code as `[[path]]` or `[[path:symbol]]`. The
dashboard turns those into clickable chips; here they are left exactly as
written, because each one already names a real file and symbol.

### What is not exposed

Nothing under `/api/admin/*` — stored credentials, access grants, reindex
triggers — and not `/api/api-keys`. A terminal agent is the wrong place to
administer a deployment from, and a token for reading should not be able to
grant itself more.

## Verify

```bash
.venv/bin/python mcp-server/selftest.py
```

Launches the server and speaks MCP to it, against a backend you already have
running. It discovers its test subject from `GRAPH_DIR` rather than naming a
project. `PRAXEVIA_SELFTEST_CHAT=1` includes `ask_codebase`, which spends real
tokens; without it that one check reports as skipped, not as passed.

## When it does not work

| Symptom | Cause |
|---|---|
| Server exits at startup | `PRAXEVIA_BACKEND_URL` or `PRAXEVIA_API_KEY` unset — it says so on stderr, which the harness logs |
| `rejected the token (401)` | Wrong or revoked key |
| `No graph built for …` | Indexed, but the graph builder has not run for it |
| `not found in registry` | A graph exists but nothing is indexed — search and chat need the index |
| `ask_codebase` fails | The backend has no `ANTHROPIC_API_KEY`; the graph and search tools do not need one |
