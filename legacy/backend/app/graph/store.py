"""
Server-side graph store —

DELIBERATE v1 SIMPLIFICATION, stated rather than hidden.
------------------------------------------------------
Plan specifies materialising L0/L1 into `graph_meta` at index time and
slicing with SQL. This module instead loads the builder's `knowledge-graph.json`
once per project+stream and slices it in memory.

Why that is legitimate rather than a shortcut:

  *'s argument is that THE BROWSER cannot hold the whole graph. It says
    nothing about the server, which has orders of magnitude more memory and
    amortises one load across every request and every user.
  * The LOD contract the dashboard sees is identical either way. Swapping the
    backing store later changes only this file — `queries.py` and `router.py`
    consume the interface, not the storage.
  * It removes `graph_pass/emit.py` and a new table pair from the critical path
    to a working application, which is what the current phase is for.

Where it stops being adequate, honestly:

  * A 10,000-file repo is ~94,000 nodes ≈ 80–100 MB of JSON per project+stream. Six such indexes would be ~600 MB resident. At that point the
    materialised-SQL design in is required, not optional.
  * Rebuilding after a re-index is a process restart or an explicit cache bust,
    not a transactional update.

The upgrade path is a `GraphSource` swap; `LOD_BUDGETS` and the response
envelope do not change.
"""

from __future__ import annotations

import gzip
import json
import logging
import os
import threading
from dataclasses import dataclass, field

logger = logging.getLogger(__name__)

# Where the builder writes. One file per project+stream.
GRAPH_DIR = os.environ.get("GRAPH_DIR", "/data/graphs")


@dataclass
class LoadedGraph:
    """A parsed graph plus the indexes every LOD query needs."""

    graph: dict
    sidecar: dict = field(default_factory=dict)
    # mtime of the graph file this was loaded from, used by GraphStore.get() to
    # notice a rebuild. 0.0 means "never stamped" and always reloads.
    mtime: float = 0.0
    nodes_by_id: dict[str, dict] = field(default_factory=dict)
    layer_of_node: dict[str, str] = field(default_factory=dict)
    nodes_by_layer: dict[str, list[str]] = field(default_factory=dict)
    edges_by_source: dict[str, list[dict]] = field(default_factory=dict)
    edges_by_target: dict[str, list[dict]] = field(default_factory=dict)

    def build_indexes(self) -> None:
        self.nodes_by_id = {n["id"]: n for n in self.graph.get("nodes", [])}

        for layer in self.graph.get("layers", []):
            ids = layer.get("nodeIds", [])
            self.nodes_by_layer[layer["id"]] = ids
            for nid in ids:
                # First-matching-layer wins, mirroring the dashboard's own
                # nodeIdToLayerId semantics (store.ts:75-96). Filtering there
                # uses a separate every-layer index; navigation uses this one.
                self.layer_of_node.setdefault(nid, layer["id"])

        for e in self.graph.get("edges", []):
            self.edges_by_source.setdefault(e["source"], []).append(e)
            self.edges_by_target.setdefault(e["target"], []).append(e)

    @property
    def project_meta(self) -> dict:
        return self.graph.get("project", {})

    def gzipped(self, cache_key: str, payload: dict) -> bytes:
        """
        Serialise and gzip `payload` ONCE per cache_key, memoised on this load.

        Takes the payload rather than compressing `self.graph` directly, because
        the bytes on the wire are whatever `queries.full_graph()` produced —
        which past FULL_GRAPH_MAX_NODES is a file-level projection, not the
        graph as stored. Compressing the wrong object would ship a body that
        does not match its own `meta`.

        Measured on a real 5,900-node index: 4,593,409 bytes of JSON become 209,099 —
        4.6 %, a 22x reduction. On loopback that saves nothing (69 ms either
        way); over any real link it is the difference between ~3.7 s and ~0.2 s.

        Compressing here rather than with `GZipMiddleware` is deliberate and is
        about the chat, not about speed. The middleware wraps EVERY response,
        including `/api/chat`, which is Server-Sent Events (`main.py:397`,
        `text/event-stream`). Gzip buffers until it has a block to emit, so
        per-token events would arrive batched and the tiny `: keepalive` frames
        would vanish into the buffer — a visible regression in the chat to buy a
        transfer win on a different endpoint. Doing it at load time touches the
        graph endpoints only, and costs zero CPU per request.
        """
        cache: dict[str, bytes] = getattr(self, "_gzip_cache", None) or {}
        hit = cache.get(cache_key)
        if hit is not None:
            return hit

        raw = json.dumps(payload, separators=(",", ":")).encode()
        # Level 6, not 9: on this payload 9 buys under 2 % extra for roughly
        # triple the time, and this runs inside the first request that touches
        # the graph.
        blob = gzip.compress(raw, compresslevel=6)
        cache[cache_key] = blob
        self._gzip_cache = cache  # type: ignore[attr-defined]
        logger.info(
            "Pre-compressed %s: %d -> %d bytes (%.1f%%)",
            cache_key, len(raw), len(blob), len(blob) / len(raw) * 100,
        )
        return blob


class GraphStore:
    """Thread-safe lazy loader keyed by registry key ('<project>_<stream>')."""

    def __init__(self, graph_dir: str = GRAPH_DIR) -> None:
        self._dir = graph_dir
        self._cache: dict[str, LoadedGraph] = {}
        self._lock = threading.Lock()

    def path_for(self, key: str) -> str:
        return os.path.join(self._dir, f"{key}.json")

    def sidecar_path_for(self, key: str) -> str:
        return os.path.join(self._dir, f"{key}.meta.json")

    def available(self) -> list[str]:
        if not os.path.isdir(self._dir):
            return []
        return sorted(
            f[:-5]
            for f in os.listdir(self._dir)
            if f.endswith(".json") and not f.endswith(".meta.json")
        )

    def get(self, key: str) -> LoadedGraph | None:
        """
        Return the loaded graph, or None when no graph has been built yet.

        VALIDATED BY MTIME, not cached once. The cache used to be write-once:
        the only thing that could ever evict an entry was `invalidate()`, and
        the sole caller of that is `sources.py`'s `_rebuild()` — which runs
        in-process. `docker-entrypoint.sh` starts uvicorn with `--workers 2`,
        so a rebuild triggered through one worker left the OTHER worker
        serving its cached copy forever, with nothing anywhere to correct it.
        A user then saw a fresh or a stale graph depending on which worker
        answered — indistinguishable from the request having failed to take
        effect at all.

        One `os.stat` per request is the cheapest fix that works across
        processes without a shared signal: the file on disk is already the
        thing every worker agrees on, so its mtime is the invalidation
        channel. It also makes graphs written by anything other than
        `_rebuild()` — the indexer sidecar, a manual `graph_pass.builder`
        run — visible without a restart, which they previously were not.
        """
        path = self.path_for(key)
        try:
            mtime = os.stat(path).st_mtime
        except OSError:
            return None

        with self._lock:
            cached = self._cache.get(key)
            if cached is not None and cached.mtime == mtime:
                return cached

        try:
            with open(path) as fh:
                graph = json.load(fh)
        except (OSError, json.JSONDecodeError) as e:
            # A corrupt graph must not take the whole API down — the chat half of
            # the product is unaffected by it.
            logger.error("Failed to load graph %s: %s", path, e)
            return None

        sidecar: dict = {}
        sc_path = self.sidecar_path_for(key)
        if os.path.exists(sc_path):
            try:
                with open(sc_path) as fh:
                    sidecar = json.load(fh)
            except (OSError, json.JSONDecodeError):
                logger.warning("Ignoring unreadable sidecar %s", sc_path)

        loaded = LoadedGraph(graph=graph, sidecar=sidecar, mtime=mtime)
        loaded.build_indexes()

        with self._lock:
            self._cache[key] = loaded
        logger.info(
            "Loaded graph %s: %d nodes, %d edges, %d layers",
            key,
            len(graph.get("nodes", [])),
            len(graph.get("edges", [])),
            len(graph.get("layers", [])),
        )
        return loaded

    def invalidate(self, key: str | None = None) -> None:
        """Drop cached graphs. Call after a re-index."""
        with self._lock:
            if key is None:
                self._cache.clear()
            else:
                self._cache.pop(key, None)


STORE = GraphStore()

# Business-flow (domain) graphs live in a SUBDIRECTORY of GRAPH_DIR rather than
# beside the codebase graphs as `{key}.domain.json`.
#
# `available()` lists every `*.json` that is not a `*.meta.json` and strips the
# extension, and it is what backs `/api/graph/projects`. A sibling
# `myproj_main.domain.json` would therefore surface as a PROJECT called
# "myproj_main.domain" in the switcher. A subdirectory is invisible to that
# listing (`domain` does not end in `.json`), so the same GraphStore class
# serves both with no special-casing and no filter that a future filename could
# slip past.
DOMAIN_STORE = GraphStore(os.path.join(GRAPH_DIR, "domain"))
