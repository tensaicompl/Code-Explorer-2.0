#!/usr/bin/env python3
"""P2-13: compare the legacy graph's drawn calls with PDX's CALLS on the Python golden.

Triage only. The legacy graph is evidence about where the two differ, never an oracle
for which one is right, and nothing here tunes PDX.

    python3 scripts/compare-legacy.py                  run the legacy stack, then compare
    python3 scripts/compare-legacy.py --compare-only   compare what an earlier run left
    python3 scripts/compare-legacy.py --check-report   check the report's counts

What a run does:

1. Reads the one `[[golden]]` entry of `bench/repos.lock` whose language is Python,
   and the P2-12 cache for it: the checkout under `bench/.cache/repos/` (exactly the
   pin, clean, origin the lock's URL) and the segment under `bench/.cache/segments/`
   (that commit, structural). `make golden` fills both; nothing is fetched here.
2. Copies the checkout's tracked files, less those the segment records as redacted,
   to `bench/.cache/legacy-compare/codebase/pdx_compare/golden/`, so both sides read
   the same files.
3. Runs the legacy indexer through `legacy/docker-compose.yml` and its
   `docker-indexer` profile, as Compose project `pdx-p2-13`, with a generated override
   that gives PostgreSQL a volume of its own, mounts the staged source read-only,
   publishes no port and runs on the CPU, and an explicit environment file. Compose
   gets no host variable beyond PATH, HOME and Docker's own. The indexer builds its
   corpus, the graph builder writes the graph, and the raw call rows are exported;
   then the project is taken down, volumes and all.
4. Compares, at the level of unique source/target pairs (decision 46): the legacy
   graph's `calls` edges, each endpoint mapped to a PDX node by file, name, qualified
   name and start line or not at all, against the segment's drawn `CALLS` rows
   collapsed to pairs. `comparison/summary.json` holds the counts and
   `comparison/evidence.jsonl` one record per difference, with the deterministic
   sample of 20 marked.

Nothing here writes `bench/results/legacy-compare.md`: its explanations are written
by whoever reads the evidence. Nothing under `legacy/` is changed, and no model or API
is called.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
import os
import re
import shutil
import sqlite3
import subprocess
import sys
import tomllib
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from pathlib import Path

#: The bands whose edges are drawn (4.2.2), and so the only ones compared.
DRAWN_BANDS = ("precise", "typed", "import-guided", "inheritance-guided", "exact", "scoped")

#: The PDX kinds a legacy symbol node can be: the legacy extractor records a function
#: or a class definition, and PDX names a test function `Test`.
LEGACY_TYPE_KINDS = {
    "function": ("Function", "Method", "Test"),
    "class": ("Class", "Interface", "Struct", "Enum", "Test"),
}

#: The staged project and stream: neutral names, so no repository name is needed.
PROJECT = "pdx_compare"
STREAM = "golden"
COMPOSE_PROJECT = "pdx-p2-13"
PG_VOLUME = "pdx_p2_13_pgdata"
#: The legacy call table for that project and stream (its own naming rule).
LEGACY_CALLS_TABLE = f"calls_{PROJECT}_{STREAM}"

SAMPLE_TOTAL = 20
SAMPLE_PER_SIDE = 10

#: Variables Compose may see. Everything else of the host's stays out of it.
COMPOSE_ENV_KEEP = ("PATH", "HOME", "DOCKER_HOST", "DOCKER_CONTEXT", "DOCKER_CONFIG",
                    "DOCKER_CERT_PATH", "DOCKER_TLS_VERIFY", "XDG_RUNTIME_DIR")
#: Variables the legacy Compose file refuses to parse without, for a service this run
#: never starts. Given placeholder values in the generated environment file.
COMPOSE_REQUIRED_PLACEHOLDERS = ("LOGIN_USERNAME", "LOGIN_PASSWORD")

TAIL_LINES = 30


class CompareError(Exception):
    """A stop: the message says what failed and what to do."""


# --- paths ----------------------------------------------------------------------------


@dataclass(frozen=True)
class GoldenPin:
    language: str
    url: str
    commit: str
    repo: str

    @property
    def fetch_url(self) -> str:
        return f"https://{self.url}"


@dataclass(frozen=True)
class Paths:
    root: Path

    @classmethod
    def under(cls, root: Path) -> Paths:
        return cls(Path(root))

    lock = property(lambda self: self.root / "bench" / "repos.lock")
    legacy = property(lambda self: self.root / "legacy")
    compose_file = property(lambda self: self.legacy / "docker-compose.yml")
    cache = property(lambda self: self.root / "bench" / ".cache" / "legacy-compare")
    codebase = property(lambda self: self.cache / "codebase")
    staging = property(lambda self: self.codebase / PROJECT / STREAM)
    output = property(lambda self: self.cache / "output")
    comparison = property(lambda self: self.cache / "comparison")
    logs = property(lambda self: self.cache / "logs")
    override = property(lambda self: self.cache / "compose.override.yml")
    env_file = property(lambda self: self.cache / "compose.env")
    legacy_graph = property(lambda self: self.output / "legacy-graph.json")
    legacy_meta = property(lambda self: self.output / "legacy-meta.json")
    legacy_calls = property(lambda self: self.output / "legacy-calls.csv")
    staging_record = property(lambda self: self.cache / "staging.json")
    summary = property(lambda self: self.comparison / "summary.json")
    evidence = property(lambda self: self.comparison / "evidence.jsonl")
    report = property(lambda self: self.root / "bench" / "results" / "legacy-compare.md")

    def checkout(self, pin: GoldenPin) -> Path:
        return self.root / "bench" / ".cache" / "repos" / pin.language / pin.repo

    def segment(self, pin: GoldenPin) -> Path:
        return self.root / "bench" / ".cache" / "segments" / pin.language / pin.repo / "segment.db"

    def golden_json(self, pin: GoldenPin) -> Path:
        return self.root / "bench" / "golden" / pin.language / f"{pin.repo}.expected.json"


def python_golden(lock: dict) -> GoldenPin:
    """The lock's one Python golden. Anything but exactly one is a stop."""
    entries = [g for g in lock.get("golden", []) if g.get("language") == "python"]
    if len(entries) != 1:
        raise CompareError(f"bench/repos.lock has {len(entries)} Python goldens, not one")
    entry = entries[0]
    url, commit = str(entry.get("url", "")), str(entry.get("commit", ""))
    repo = url.rstrip("/").rsplit("/", 1)[-1]
    repo = repo[: -len(".git")] if repo.endswith(".git") else repo
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise CompareError("the Python golden's commit is not a full lower-case commit id")
    if not re.fullmatch(r"[A-Za-z0-9._-]+", repo) or repo in (".", ".."):
        raise CompareError("the Python golden's URL does not end in a usable repository name")
    return GoldenPin("python", url, commit, repo)


# --- the P2-12 checkout ---------------------------------------------------------------


def git_environment() -> dict:
    env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
    env.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1", GIT_TERMINAL_PROMPT="0")
    return env


def git(directory: Path, *args: str) -> str:
    result = subprocess.run(["git", "-C", str(directory), *args], env=git_environment(),
                            capture_output=True, stdin=subprocess.DEVNULL)
    if result.returncode != 0:
        raise CompareError(f"git {args[0]} failed in the golden checkout (exit {result.returncode})")
    return result.stdout.decode("utf-8", "replace")


def verify_checkout(checkout: Path, pin: GoldenPin) -> None:
    """The checkout is a real directory, exactly the pin, clean, from the lock's URL."""
    checkout = Path(checkout)
    if checkout.is_symlink() or not checkout.is_dir():
        raise CompareError("the golden checkout is missing or a symlink; run `make golden` "
                           "first to populate the P2-12 cache")
    dot_git = checkout / ".git"
    if dot_git.is_symlink() or not dot_git.is_dir():
        raise CompareError("the golden checkout has no real .git directory; run `make golden`")
    if git(checkout, "rev-parse", "HEAD").strip() != pin.commit:
        raise CompareError("the golden checkout is not at the pinned commit; run `make golden`")
    if git(checkout, "status", "--porcelain").strip():
        raise CompareError("the golden checkout is not clean; run `make golden`")
    if git(checkout, "remote", "get-url", "origin").strip() != pin.fetch_url:
        raise CompareError("the golden checkout's origin is not the lock's URL")


def tracked_entries(checkout: Path) -> list[tuple[str, str]]:
    """(mode, path) of every tracked entry, from the index, by path."""
    out = []
    for record in git(checkout, "ls-files", "-s", "-z").split("\0"):
        if record:
            meta, path = record.split("\t", 1)
            out.append((meta.split(" ", 1)[0], path))
    return sorted(out, key=lambda e: e[1])


def stage_checkout(checkout: Path, paths: Paths, redacted: set[str]) -> dict:
    """Copy the tracked files, less the redacted ones, to a fresh staging directory."""
    for directory in (paths.root / "bench" / ".cache", paths.cache):
        if directory.is_symlink():
            raise CompareError(f"{directory.relative_to(paths.root)} is a symlink; refused")
    if paths.codebase.is_symlink():
        raise CompareError("the staging directory is a symlink; refused")
    if paths.codebase.exists():
        shutil.rmtree(paths.codebase)
    staged, excluded, skipped = 0, [], []
    for mode, rel in tracked_entries(checkout):
        if rel in redacted:
            excluded.append(rel)
            continue
        if mode == "160000":  # a submodule: never populated
            skipped.append(rel)
            continue
        source, target = checkout / rel, paths.staging / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        if mode == "120000":
            os.symlink(os.readlink(source), target)
        else:
            shutil.copyfile(source, target, follow_symlinks=False)
            os.chmod(target, 0o755 if mode == "100755" else 0o644)
        staged += 1
    return {"files": staged, "excluded_redacted": excluded, "skipped_submodules": skipped}


# --- the new segment ------------------------------------------------------------------


@dataclass(frozen=True)
class PdxNode:
    node_id: str
    kind: str
    name: str
    qualified_name: str
    path: str | None
    start_line: int | None
    parent_id: str | None


@dataclass(frozen=True)
class PdxCall:
    edge_id: str
    src: str
    dst: str
    band: str
    site_id: str | None
    engine_score: float | None
    engine_strategy: str | None
    site_path: str | None
    site_line: int | None
    callee_text: str | None
    receiver_text: str | None


def open_segment(path: Path) -> sqlite3.Connection:
    """The segment, opened so that nothing can write it."""
    uri = Path(path).resolve().as_uri() + "?mode=ro&immutable=1"
    conn = sqlite3.connect(uri, uri=True)
    conn.execute("PRAGMA query_only = ON")
    return conn


def segment_meta(conn: sqlite3.Connection) -> dict:
    return dict(conn.execute("SELECT key, value FROM meta ORDER BY key"))


NODES_SQL = (
    "SELECT n.node_id, n.kind, n.name, n.qualified_name, f.path, n.start_line, n.parent_id "
    "FROM nodes n LEFT JOIN files f ON f.file_id = n.file_id ORDER BY n.node_id"
)

#: Drawn CALLS only; the band list is DRAWN_BANDS, spelled out so the query is fixed.
CALLS_SQL = (
    "SELECT e.edge_id, e.src, e.dst, e.band, e.site_id, e.engine_score, e.engine_strategy, "
    "f.path, s.start_line, s.callee_text, s.receiver_text "
    "FROM edges e LEFT JOIN sites s ON s.site_id = e.site_id "
    "LEFT JOIN files f ON f.file_id = s.file_id "
    "WHERE e.kind = 'CALLS' AND e.band IN "
    "('precise', 'typed', 'import-guided', 'inheritance-guided', 'exact', 'scoped') "
    "ORDER BY e.edge_id"
)

SITES_SQL = (
    "SELECT s.site_id, f.path, s.start_line, s.site_kind, s.callee_text, s.receiver_text, "
    "e.kind, e.band, e.dst, c.band, c.reason, c.candidate_ids "
    "FROM sites s JOIN files f ON f.file_id = s.file_id "
    "LEFT JOIN edges e ON e.site_id = s.site_id "
    "LEFT JOIN candidates c ON c.site_id = s.site_id "
    "ORDER BY f.path, s.start_line, s.site_id, e.edge_id"
)


def read_pdx_nodes(conn: sqlite3.Connection) -> dict[str, PdxNode]:
    return {row[0]: PdxNode(*row) for row in conn.execute(NODES_SQL)}


def read_pdx_calls(conn: sqlite3.Connection) -> list[PdxCall]:
    return [PdxCall(*row) for row in conn.execute(CALLS_SQL)]


def read_pdx_sites(conn: sqlite3.Connection, nodes: dict[str, PdxNode]) -> dict:
    """Every site by (path, line), with what became of it: an edge or a candidate row."""
    out: dict[tuple[str, int], list[dict]] = defaultdict(list)
    for (site_id, path, line, site_kind, callee, receiver, ekind, eband, edst, cband,
         creason, cids) in conn.execute(SITES_SQL):
        record = {"site_id": site_id, "site_kind": site_kind, "callee_text": callee,
                  "receiver_text": receiver}
        if ekind is not None:
            record["edge"] = {"kind": ekind, "band": eband, "dst": qn_of(nodes, edst)}
        elif cband is not None:
            record["candidate"] = {
                "band": cband, "reason": creason,
                "candidates": sorted(qn_of(nodes, i) for i in json.loads(cids or "[]")),
            }
        out[(path, line)].append(record)
    return out


def qn_of(nodes: dict[str, PdxNode], node_id: str | None) -> str | None:
    node = nodes.get(node_id) if node_id else None
    return node.qualified_name if node else node_id


def pdx_pairs(calls: list[PdxCall]) -> dict[tuple[str, str], list[PdxCall]]:
    """Drawn CALLS rows collapsed to (source, destination): the comparison's unit."""
    pairs: dict[tuple[str, str], list[PdxCall]] = defaultdict(list)
    for call in calls:
        pairs[(call.src, call.dst)].append(call)
    return {pair: sorted(rows, key=lambda c: (c.site_line or 0, c.edge_id))
            for pair, rows in pairs.items()}


# --- the legacy graph -----------------------------------------------------------------


def legacy_nodes(graph: dict) -> dict[str, dict]:
    return {node["id"]: node for node in graph.get("nodes", [])}


def legacy_call_edges(graph: dict) -> list[tuple[str, str]]:
    """The graph's `calls` edges, one per emitted edge, in a fixed order."""
    return sorted((e["source"], e["target"]) for e in graph.get("edges", [])
                  if e.get("type") == "calls")


def read_legacy_calls(path: Path) -> list[dict]:
    with open(path, newline="", encoding="utf-8") as fh:
        rows = [dict(r) for r in csv.DictReader(fh)]
    for row in rows:
        row["caller_line"] = int(row["caller_line"]) if row.get("caller_line") else None
    return sorted(rows, key=lambda r: (r["caller_file"], r["caller_line"] or 0,
                                       r["caller_name"], r["callee_name"]))


# --- the crosswalk --------------------------------------------------------------------


@dataclass(frozen=True)
class Mapping:
    status: str  # unique | unmapped | ambiguous
    node_id: str | None
    rule: str
    candidates: tuple[str, ...] = ()


class Crosswalk:
    """Legacy node -> PDX node, by evidence only, never by order.

    A file maps by its path. A symbol's candidates are the PDX nodes of a compatible
    kind with its name in its file, never another file's. Several candidates are
    narrowed by the legacy qualified name (equal to the PDX one, or its suffix after
    a dot), then by an exactly equal start line, each only when that leaves one.
    Whatever is left of several is ambiguous; none is unmapped.
    """

    def __init__(self, nodes: dict[str, PdxNode]):
        self._files = {n.path: n.node_id for n in nodes.values() if n.kind == "File"}
        self._by_file_name: dict[tuple[str, str], list[PdxNode]] = defaultdict(list)
        for node in sorted(nodes.values(), key=lambda n: n.node_id):
            if node.kind != "File" and node.path is not None:
                self._by_file_name[(node.path, node.name)].append(node)

    def map(self, legacy: dict) -> Mapping:
        kind, path = legacy.get("type"), legacy.get("filePath")
        if kind == "file":
            node_id = self._files.get(path)
            return Mapping("unique", node_id, "file-path") if node_id else \
                Mapping("unmapped", None, "no-file")
        kinds = LEGACY_TYPE_KINDS.get(kind)
        if kinds is None:
            return Mapping("unmapped", None, f"legacy-type-{kind}")
        candidates = [n for n in self._by_file_name.get((path, legacy.get("name")), [])
                      if n.kind in kinds]
        if not candidates:
            return Mapping("unmapped", None, "no-candidate")
        if len(candidates) == 1:
            return Mapping("unique", candidates[0].node_id, "file-name")
        qualified = legacy.get("qualifiedName")
        if qualified:
            named = [n for n in candidates if n.qualified_name == qualified
                     or n.qualified_name.endswith("." + qualified)]
            if len(named) == 1:
                return Mapping("unique", named[0].node_id, "qualified-name")
            if named:
                candidates = named
        line = (legacy.get("lineRange") or [None])[0]
        if line is not None:
            at = [n for n in candidates if n.start_line == line]
            if len(at) == 1:
                return Mapping("unique", at[0].node_id, "start-line")
        return Mapping("ambiguous", None, "several-candidates",
                       tuple(sorted(n.node_id for n in candidates)))


# --- the comparison -------------------------------------------------------------------


@dataclass(frozen=True)
class Difference:
    """One compared pair. Sources and targets are legacy node ids on the legacy side,
    `qualified_name#node_id` on the PDX side."""

    side: str  # same | only-legacy | only-new
    source: str
    target: str
    reason: str = ""  # only-legacy: mapped-absent | endpoint-unmapped | endpoint-ambiguous
    bands: tuple[str, ...] = ()
    pdx_pair: tuple[str, str] | None = None

    @property
    def key(self) -> str:
        return f"{self.side}|{self.source}|{self.target}"


def pdx_descriptor(nodes: dict[str, PdxNode], node_id: str) -> str:
    return f"{qn_of(nodes, node_id)}#{node_id}"


@dataclass
class Comparison:
    legacy_graph: dict
    legacy_edges: list[tuple[str, str]]
    nodes: dict[str, PdxNode]
    pairs: dict[tuple[str, str], list[PdxCall]]
    raw_rows: int
    mappings: dict[str, Mapping]
    same: list[Difference] = field(default_factory=list)
    only_legacy: list[Difference] = field(default_factory=list)
    only_new: list[Difference] = field(default_factory=list)
    same_pdx_pairs: set = field(default_factory=set)

    def counts(self) -> dict:
        reasons = Counter(d.reason for d in self.only_legacy)
        return {
            "legacy_call_edges": len(self.legacy_edges),
            "pdx_raw_calls_rows": self.raw_rows,
            "pdx_unique_calls_pairs": len(self.pairs),
            "pdx_duplicate_rows_collapsed": self.raw_rows - len(self.pairs),
            "same": len(self.same),
            "same_pdx_pairs": len(self.same_pdx_pairs),
            "legacy_edges_sharing_a_pdx_pair": len(self.same) - len(self.same_pdx_pairs),
            "only_legacy": len(self.only_legacy),
            "only_legacy_mapped_absent": reasons["mapped-absent"],
            "only_legacy_endpoint_unmapped": reasons["endpoint-unmapped"],
            "only_legacy_endpoint_ambiguous": reasons["endpoint-ambiguous"],
            "only_new": len(self.only_new),
        }

    def mapping_counts(self) -> dict:
        references = [self.mappings[e] for edge in self.legacy_edges for e in edge]
        distinct = {e for edge in self.legacy_edges for e in edge}
        by_status = Counter(m.status for m in references)
        distinct_status = Counter(self.mappings[e].status for e in distinct)
        rules = Counter(self.mappings[e].rule for e in distinct)
        return {
            "legacy_endpoint_references": len(references),
            "endpoint_references_unique": by_status["unique"],
            "endpoint_references_unmapped": by_status["unmapped"],
            "endpoint_references_ambiguous": by_status["ambiguous"],
            "distinct_legacy_endpoints": len(distinct),
            "distinct_endpoints_unique": distinct_status["unique"],
            "distinct_endpoints_unmapped": distinct_status["unmapped"],
            "distinct_endpoints_ambiguous": distinct_status["ambiguous"],
            "distinct_endpoints_by_rule": dict(sorted(rules.items())),
            "fully_mapped_legacy_edges": sum(
                1 for s, t in self.legacy_edges
                if self.mappings[s].status == self.mappings[t].status == "unique"),
        }

    def _group(self, group: str) -> list[tuple[str, str]]:
        if group == "same":
            return sorted(self.same_pdx_pairs)
        if group == "only_new":
            return sorted(d.pdx_pair for d in self.only_new)
        return sorted(self.pairs)

    def band_participation(self, group: str = "all") -> dict:
        """How many pairs of the group hold at least one row of each band."""
        out = {band: 0 for band in DRAWN_BANDS}
        for pair in self._group(group):
            for band in {c.band for c in self.pairs[pair]}:
                out[band] += 1
        return out

    def band_sets(self, group: str = "all") -> dict:
        sets = Counter("+".join(sorted({c.band for c in self.pairs[p]})) for p in self._group(group))
        return dict(sorted(sets.items()))


def compare(graph: dict, nodes: dict[str, PdxNode], calls: list[PdxCall]) -> Comparison:
    lnodes = legacy_nodes(graph)
    edges = legacy_call_edges(graph)
    walk = Crosswalk(nodes)
    mappings = {nid: (walk.map(lnodes[nid]) if nid in lnodes
                      else Mapping("unmapped", None, "not-a-node"))
                for nid in sorted({e for edge in edges for e in edge})}
    pairs = pdx_pairs(calls)
    result = Comparison(graph, edges, nodes, pairs, len(calls), mappings)
    for source, target in edges:
        ms, mt = mappings[source], mappings[target]
        if ms.status == mt.status == "unique":
            pair = (ms.node_id, mt.node_id)
            if pair in pairs:
                bands = tuple(sorted({c.band for c in pairs[pair]}))
                result.same.append(Difference("same", source, target, bands=bands, pdx_pair=pair))
                result.same_pdx_pairs.add(pair)
            else:
                result.only_legacy.append(
                    Difference("only-legacy", source, target, "mapped-absent", pdx_pair=pair))
        else:
            statuses = {ms.status, mt.status}
            reason = "endpoint-unmapped" if "unmapped" in statuses else "endpoint-ambiguous"
            result.only_legacy.append(Difference("only-legacy", source, target, reason))
    for pair in sorted(pairs):
        if pair not in result.same_pdx_pairs:
            bands = tuple(sorted({c.band for c in pairs[pair]}))
            result.only_new.append(Difference(
                "only-new", pdx_descriptor(nodes, pair[0]), pdx_descriptor(nodes, pair[1]),
                bands=bands, pdx_pair=pair))
    counts = result.counts()
    assert counts["same"] + counts["only_legacy"] == counts["legacy_call_edges"]
    assert counts["same_pdx_pairs"] + counts["only_new"] == counts["pdx_unique_calls_pairs"]
    assert all(mappings[d.source].status == mappings[d.target].status == "unique"
               for d in result.same)
    return result


# --- the sample -----------------------------------------------------------------------


def sample_hash(difference: Difference) -> str:
    text = "\n".join((difference.side, difference.source, difference.target))
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def ranked(differences: list[Difference], side: str) -> list[Difference]:
    return sorted((d for d in differences if d.side == side),
                  key=lambda d: (sample_hash(d), d.source, d.target))


def select_sample(differences: list[Difference], total: int = SAMPLE_TOTAL,
                  per_side: int = SAMPLE_PER_SIDE) -> list[Difference]:
    """Up to `per_side` of each side by hash rank; a short side's slots go to the other."""
    legacy, new = ranked(differences, "only-legacy"), ranked(differences, "only-new")
    take_legacy, take_new = min(len(legacy), per_side), min(len(new), per_side)
    spare = total - take_legacy - take_new
    if spare > 0:
        extra_legacy = min(spare, len(legacy) - take_legacy)
        take_legacy += extra_legacy
        take_new += min(spare - extra_legacy, len(new) - take_new)
    return legacy[:take_legacy] + new[:take_new]


# --- evidence -------------------------------------------------------------------------


def legacy_node_record(node: dict | None, node_id: str) -> dict:
    if node is None:
        return {"id": node_id}
    return {key: node.get(key) for key in ("id", "type", "name", "qualifiedName", "filePath",
                                           "lineRange")}


def pdx_node_record(nodes: dict[str, PdxNode], node_id: str | None) -> dict | None:
    node = nodes.get(node_id) if node_id else None
    if node is None:
        return None
    return {"node_id": node.node_id, "kind": node.kind, "qualified_name": node.qualified_name,
            "path": node.path, "start_line": node.start_line}


def call_record(call: PdxCall) -> dict:
    return {"edge_id": call.edge_id, "band": call.band, "site_path": call.site_path,
            "site_line": call.site_line, "callee_text": call.callee_text,
            "receiver_text": call.receiver_text, "engine_strategy": call.engine_strategy,
            "engine_score": call.engine_score}


def matching_legacy_calls(legacy_calls_by_file: dict, source: dict | None,
                          target: dict | None) -> list[dict]:
    """The raw legacy call rows that could have made this edge: the caller's file, the
    callee's name, and for a symbol source its name as the scope path's last part."""
    if not source or not target:
        return []
    rows = legacy_calls_by_file.get(source.get("filePath"), [])
    callee = (target.get("name") or "").lower()
    out = []
    for row in rows:
        if (row["callee_name"] or "").lower() != callee:
            continue
        if source.get("type") != "file":
            last = (row["caller_name"] or "").split(".")[-1].lower()
            if last != (source.get("name") or "").lower():
                continue
        out.append(row)
    return out


def evidence_records(result: Comparison, legacy_calls: list[dict], pdx_sites: dict) -> list[dict]:
    lnodes = legacy_nodes(result.legacy_graph)
    by_file: dict[str, list[dict]] = defaultdict(list)
    by_line: dict[tuple[str, int], list[dict]] = defaultdict(list)
    for row in legacy_calls:
        by_file[row["caller_file"]].append(row)
        by_line[(row["caller_file"], row["caller_line"])].append(row)
    differences = result.only_legacy + result.only_new
    sample = {d.key: n for n, d in enumerate(select_sample(differences), 1)}
    records = []
    for side, items in (("only-legacy", result.only_legacy), ("only-new", result.only_new)):
        for rank, d in enumerate(ranked(items, side), 1):
            record = {"side": side, "key": d.key, "hash": sample_hash(d), "sample_rank": rank,
                      "sample_number": sample.get(d.key)}
            if side == "only-legacy":
                source, target = lnodes.get(d.source), lnodes.get(d.target)
                raw = matching_legacy_calls(by_file, source, target)
                record.update(
                    reason=d.reason,
                    legacy_source=legacy_node_record(source, d.source),
                    legacy_target=legacy_node_record(target, d.target),
                    mapping={end: {"status": m.status, "rule": m.rule,
                                   "node": pdx_node_record(result.nodes, m.node_id),
                                   "candidates": [pdx_node_record(result.nodes, c)
                                                  for c in m.candidates]}
                             for end, m in (("source", result.mappings[d.source]),
                                            ("target", result.mappings[d.target]))},
                    legacy_calls=raw,
                    pdx_sites_at_legacy_calls={
                        f"{r['caller_file']}:{r['caller_line']}":
                            pdx_sites.get((r["caller_file"], r["caller_line"]), [])
                        for r in raw},
                )
            else:
                rows = result.pairs[d.pdx_pair]
                record.update(
                    bands=list(d.bands),
                    pdx_source=pdx_node_record(result.nodes, d.pdx_pair[0]),
                    pdx_target=pdx_node_record(result.nodes, d.pdx_pair[1]),
                    calls=[call_record(c) for c in rows],
                    legacy_calls_at_pdx_sites={
                        f"{c.site_path}:{c.site_line}": by_line.get((c.site_path, c.site_line), [])
                        for c in rows if c.site_path is not None},
                )
            records.append(record)
    return records


def evidence_lines(result: Comparison, legacy_calls: list[dict], pdx_sites: dict) -> list[str]:
    """The evidence file's lines: one JSON object per difference, by side and rank."""
    return [json.dumps(r, sort_keys=True, ensure_ascii=False)
            for r in evidence_records(result, legacy_calls, pdx_sites)]


# --- summary --------------------------------------------------------------------------


SEGMENT_META_KEYS = ("commit_sha", "profile", "repo_id", "repo_url", "engine_version",
                     "schema_version", "language_matrix_version")


def golden_cross_check(golden_json: Path, result: Comparison) -> dict:
    """The committed golden's CALLS, collapsed by qualified names, beside the segment's."""
    golden = json.loads(Path(golden_json).read_text(encoding="utf-8"))
    golden_pairs = {(e["src_qn"], e["dst_qn"]) for e in golden["drawn_edges"]
                    if e["kind"] == "CALLS"}
    golden_rows = sum(1 for e in golden["drawn_edges"] if e["kind"] == "CALLS")
    segment_qn_pairs = {(qn_of(result.nodes, s), qn_of(result.nodes, d)) for s, d in result.pairs}
    return {
        "golden_calls_rows": golden_rows,
        "golden_unique_qn_pairs": len(golden_pairs),
        "segment_unique_qn_pairs": len(segment_qn_pairs),
        "segment_unique_node_pairs": len(result.pairs),
        "node_pairs_sharing_a_qn_pair": len(result.pairs) - len(segment_qn_pairs),
        "qn_pair_sets_equal": golden_pairs == segment_qn_pairs,
    }


def compare_outputs(paths: Paths, segment: Path, golden_json: Path | None,
                    staging: dict | None = None) -> dict:
    """Compare what the legacy run left with the segment; write summary and evidence."""
    for required in (paths.legacy_graph, paths.legacy_meta, paths.legacy_calls):
        if not required.is_file():
            raise CompareError(f"{required.relative_to(paths.root)} is missing; run without "
                               "--compare-only first")
    graph = json.loads(paths.legacy_graph.read_text(encoding="utf-8"))
    sidecar = json.loads(paths.legacy_meta.read_text(encoding="utf-8"))
    legacy_calls = read_legacy_calls(paths.legacy_calls)
    conn = open_segment(segment)
    try:
        meta = segment_meta(conn)
        nodes = read_pdx_nodes(conn)
        calls = read_pdx_calls(conn)
        sites = read_pdx_sites(conn, nodes)
    finally:
        conn.close()
    result = compare(graph, nodes, calls)
    lines = evidence_lines(result, legacy_calls, sites)
    sample = select_sample(result.only_legacy + result.only_new)
    counts = result.counts()
    participation = {g: result.band_participation(g) for g in ("all", "same", "only_new")}
    summary = {
        "task": "P2-13",
        "segment": {key: meta.get(key) for key in SEGMENT_META_KEYS},
        "legacy": {
            "nodes": len(graph.get("nodes", [])),
            "edges": len(graph.get("edges", [])),
            "edge_types": dict(sorted(Counter(e.get("type") for e in graph.get("edges", []))
                                      .items())),
            "call_edges": counts["legacy_call_edges"],
            "raw_call_rows": len(legacy_calls),
            "sidecar": {key: sidecar[key] for key in sorted(sidecar)},
        },
        "same": counts["same"],
        "only_legacy": counts["only_legacy"],
        "only_new": counts["only_new"],
        "counts": counts,
        "mapping": result.mapping_counts(),
        "band_participation": participation,
        "band_sets": {g: result.band_sets(g) for g in ("all", "same", "only_new")},
        "typed": {"same_pairs_with_typed": participation["same"]["typed"],
                  "only_new_pairs_with_typed": participation["only_new"]["typed"],
                  "all_pairs_with_typed": participation["all"]["typed"]},
        "sample": [{"number": n, "side": d.side, "key": d.key, "hash": sample_hash(d)}
                   for n, d in enumerate(sample, 1)],
        "golden_cross_check": golden_cross_check(golden_json, result) if golden_json else None,
        "staging": staging,
    }
    paths.comparison.mkdir(parents=True, exist_ok=True)
    paths.summary.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n",
                             encoding="utf-8")
    paths.evidence.write_text("".join(line + "\n" for line in lines), encoding="utf-8")
    return summary


REPORT_COUNT_LINE = re.compile(r"^(same|only-legacy|only-new)[ \t]+([0-9]+)[ \t]*$", re.M)


def report_count_mismatches(text: str, summary: dict) -> list[str]:
    """The report's `same`, `only-legacy` and `only-new` lines against the summary."""
    found: dict[str, int] = {}
    for name, value in REPORT_COUNT_LINE.findall(text):
        found.setdefault(name, int(value))
    out = []
    for name in ("same", "only-legacy", "only-new"):
        expected = summary[name.replace("-", "_")]
        if found.get(name) != expected:
            out.append(f"{name}: report {found.get(name)}, summary {expected}")
    return out


# --- the legacy stack -----------------------------------------------------------------


def compose_environment() -> dict:
    return {k: os.environ[k] for k in COMPOSE_ENV_KEEP if k in os.environ}


def compose_args(paths: Paths, *rest: str) -> list[str]:
    return ["docker", "compose", "-p", COMPOSE_PROJECT, "--project-directory", str(paths.legacy),
            "-f", str(paths.compose_file), "-f", str(paths.override),
            "--env-file", str(paths.env_file), "--profile", "docker-indexer", *rest]


def compose_override(paths: Paths) -> str:
    q = json.dumps
    return "\n".join([
        "# Generated by scripts/compare-legacy.py (P2-13). The legacy stack, isolated: its",
        "# own database volume, the staged source read-only, no host port, the CPU only.",
        "services:",
        "  postgres:",
        "    volumes: !override",
        f"      - {q(f'{PG_VOLUME}:/var/lib/postgresql/data')}",
        "    ports: !reset []",
        "    mem_limit: 1g",
        "  indexer:",
        "    environment:",
        "      - INDEXER_DEVICE=cpu",
        "    volumes: !override",
        f"      - {q(f'{paths.codebase}:/data/codebase:ro')}",
        f"      - {q(f'{paths.output}:/out')}",
        "    mem_limit: 5g",
        "volumes:",
        f"  {PG_VOLUME}: {{}}",
        "",
    ])


def compose_env_text() -> str:
    lines = ["# Generated by scripts/compare-legacy.py (P2-13). Only what the legacy Compose",
             "# file needs to parse; the service that reads these is never started."]
    lines += [f"{name}=unused" for name in COMPOSE_REQUIRED_PLACEHOLDERS]
    return "\n".join(lines) + "\n"


def tail(text: str) -> str:
    return "\n".join(text.splitlines()[-TAIL_LINES:])[-4000:]


def run_step(category: str, args: list[str], paths: Paths, log: str, timeout: int,
             stdout_to: Path | None = None) -> str:
    """Run one step, its output to a log; a failure stops with a bounded tail."""
    log_path = paths.logs / log
    with open(log_path, "wb") as log_fh:
        try:
            result = subprocess.run(args, env=compose_environment(), stdin=subprocess.DEVNULL,
                                    stdout=subprocess.PIPE,
                                    stderr=subprocess.PIPE if stdout_to else subprocess.STDOUT,
                                    timeout=timeout)
        except FileNotFoundError:
            raise CompareError(f"{category}: `{args[0]}` is not installed") from None
        except subprocess.TimeoutExpired:
            raise CompareError(f"{category}: timed out after {timeout}s; see "
                               f"{log_path.relative_to(paths.root)}") from None
        output = result.stdout.decode("utf-8", "replace")
        if stdout_to is not None:
            stdout_to.write_bytes(result.stdout)
            log_fh.write(result.stderr)
            output = result.stderr.decode("utf-8", "replace")
        else:
            log_fh.write(result.stdout)
    if result.returncode != 0:
        raise CompareError(f"{category} failed (exit {result.returncode}); the end of "
                           f"{log_path.relative_to(paths.root)}:\n{tail(output)}")
    return output


def check_compose_config(paths: Paths) -> None:
    """The effective configuration is the isolated one, or nothing starts."""
    result = subprocess.run(compose_args(paths, "config", "--format", "json", "postgres",
                                         "indexer"),
                            env=compose_environment(), capture_output=True,
                            stdin=subprocess.DEVNULL)
    if result.returncode != 0:
        raise CompareError("docker compose config failed (exit "
                           f"{result.returncode}):\n{tail(result.stderr.decode('utf-8', 'replace'))}")
    config = json.loads(result.stdout)  # parsed only; never printed or logged
    services = config.get("services", {})
    problems = []
    if set(services) != {"postgres", "indexer"}:
        problems.append(f"services {sorted(services)}")
    pg = services.get("postgres", {})
    pg_mounts = [m for m in pg.get("volumes", []) if m.get("target") == "/var/lib/postgresql/data"]
    if not (len(pg_mounts) == 1 and pg_mounts[0].get("type") == "volume"
            and pg_mounts[0].get("source") == PG_VOLUME and len(pg.get("volumes", [])) == 1):
        problems.append("postgres storage is not the isolated volume")
    if pg.get("ports"):
        problems.append("postgres publishes a host port")
    ix = services.get("indexer", {})
    mounts = {m.get("target"): m for m in ix.get("volumes", [])}
    code = mounts.get("/data/codebase", {})
    out = mounts.get("/out", {})
    if set(mounts) != {"/data/codebase", "/out"}:
        problems.append(f"indexer mounts {sorted(mounts)}")
    if not (code.get("type") == "bind" and code.get("source") == str(paths.codebase)
            and code.get("read_only")):
        problems.append("the indexer's /data/codebase is not the staged source, read-only")
    if not (out.get("type") == "bind" and out.get("source") == str(paths.output)):
        problems.append("the indexer's /out is not the output directory")
    if (ix.get("environment") or {}).get("INDEXER_DEVICE") != "cpu":
        problems.append("the indexer is not set to the CPU")
    if problems:
        raise CompareError("the effective Compose configuration is not isolated; nothing was "
                           "started: " + "; ".join(problems))


def run_legacy(paths: Paths) -> None:
    """Build the corpus and the graph with the isolated legacy stack, then take it down."""
    for directory in (paths.output, paths.logs):
        directory.mkdir(parents=True, exist_ok=True)
    for stale in (paths.legacy_graph, paths.legacy_meta, paths.legacy_calls):
        if stale.exists():
            stale.unlink()
    paths.override.write_text(compose_override(paths), encoding="utf-8")
    paths.env_file.write_text(compose_env_text(), encoding="utf-8")
    run_step("docker --version", ["docker", "--version"], paths, "preflight-docker.log", 60)
    run_step("docker compose version", ["docker", "compose", "version"], paths,
             "preflight-compose.log", 60)
    run_step("docker info", ["docker", "info", "--format", "{{.ServerVersion}}"], paths,
             "preflight-info.log", 60)
    check_compose_config(paths)
    user = f"{os.getuid()}:{os.getgid()}"
    try:
        run_step("compose down (before)", compose_args(paths, "down", "-v", "--remove-orphans"),
                 paths, "down-before.log", 300)
        print("compare-legacy: building the legacy indexer image", file=sys.stderr)
        run_step("legacy indexer image build",
                 compose_args(paths, "--progress", "plain", "build", "indexer"),
                 paths, "build.log", 7200)
        run_step("isolated postgres", compose_args(paths, "up", "-d", "--wait", "postgres"),
                 paths, "postgres.log", 600)
        print("compare-legacy: building the legacy corpus", file=sys.stderr)
        corpus = run_step("legacy corpus (build_corpus.py)",
                          compose_args(paths, "run", "--rm", "--no-deps", "-T", "indexer",
                                       "python", "build_corpus.py", PROJECT, STREAM),
                          paths, "build-corpus.log", 7200)
        if f"=== Indexing project: {PROJECT} / stream: {STREAM} ===" not in corpus:
            raise CompareError("build_corpus.py exited 0 without indexing the staged project")
        print("compare-legacy: building the legacy graph", file=sys.stderr)
        run_step("legacy graph (graph_pass.builder)",
                 compose_args(paths, "run", "--rm", "--no-deps", "-T", "--user", user, "indexer",
                              "python", "-m", "graph_pass.builder", PROJECT, STREAM,
                              "-o", "/out/legacy-graph.json", "--sidecar", "/out/legacy-meta.json"),
                 paths, "graph-builder.log", 1800)
        run_step("legacy call export",
                 compose_args(paths, "exec", "-T", "postgres", "psql", "-U", "prx", "-d", "prx",
                              "-v", "ON_ERROR_STOP=1", "--csv", "-c",
                              "SELECT caller_name, caller_file, caller_line, callee_name "
                              f'FROM "{LEGACY_CALLS_TABLE}" '
                              "ORDER BY caller_file, caller_line, caller_name, callee_name"),
                 paths, "calls-export.log", 600, stdout_to=paths.legacy_calls)
    finally:
        try:
            run_step("compose down", compose_args(paths, "down", "-v", "--remove-orphans"),
                     paths, "down.log", 600)
        except CompareError as error:
            print(f"compare-legacy: cleanup: {error}", file=sys.stderr)


# --- the command ----------------------------------------------------------------------


def repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def print_summary(summary: dict) -> None:
    c, m = summary["counts"], summary["mapping"]
    lines = [
        f"segment: commit {summary['segment']['commit_sha']}, engine "
        f"{summary['segment']['engine_version']}, schema {summary['segment']['schema_version']}",
        f"legacy calls edges {c['legacy_call_edges']}; PDX CALLS rows {c['pdx_raw_calls_rows']}, "
        f"unique pairs {c['pdx_unique_calls_pairs']} ({c['pdx_duplicate_rows_collapsed']} rows "
        "collapsed)",
        f"same {summary['same']}",
        f"only-legacy {summary['only_legacy']} (mapped but absent {c['only_legacy_mapped_absent']}, "
        f"endpoint unmapped {c['only_legacy_endpoint_unmapped']}, endpoint ambiguous "
        f"{c['only_legacy_endpoint_ambiguous']})",
        f"only-new {summary['only_new']}",
        f"endpoints: {m['legacy_endpoint_references']} references, {m['endpoint_references_unique']}"
        f" unique, {m['endpoint_references_unmapped']} unmapped, "
        f"{m['endpoint_references_ambiguous']} ambiguous; {m['fully_mapped_legacy_edges']} edges "
        "fully mapped",
        f"sample: {len(summary['sample'])} differences",
    ]
    print("\n".join(lines))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="P2-13: compare the legacy graph's calls with PDX's CALLS on the Python "
                    "golden (triage only).")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--compare-only", action="store_true",
                      help="reuse the legacy outputs an earlier run left; start no container")
    mode.add_argument("--check-report", action="store_true",
                      help="check bench/results/legacy-compare.md's counts against the summary")
    args = parser.parse_args(argv)
    paths = Paths.under(repo_root())
    try:
        if args.check_report:
            if not paths.summary.is_file() or not paths.report.is_file():
                raise CompareError("the summary or the report is missing")
            problems = report_count_mismatches(paths.report.read_text(encoding="utf-8"),
                                               json.loads(paths.summary.read_text()))
            for problem in problems:
                print(f"compare-legacy: {problem}", file=sys.stderr)
            print("report counts: " + ("mismatch" if problems else "match the summary"))
            return 1 if problems else 0
        with open(paths.lock, "rb") as fh:
            pin = python_golden(tomllib.load(fh))
        checkout, segment = paths.checkout(pin), paths.segment(pin)
        if not segment.is_file():
            raise CompareError("the Python golden's segment is missing; run `make golden` first "
                               "to populate the P2-12 cache")
        verify_checkout(checkout, pin)
        conn = open_segment(segment)
        try:
            meta = segment_meta(conn)
            redacted = {row[0] for row in conn.execute(
                "SELECT path FROM files WHERE status = 'redacted' ORDER BY path")}
        finally:
            conn.close()
        if meta.get("commit_sha") != pin.commit or meta.get("profile") != "structural":
            raise CompareError("the cached segment is not the pin's structural segment; run "
                               "`make golden`")
        staging = None
        if not args.compare_only:
            staging = stage_checkout(checkout, paths, redacted)
            paths.staging_record.write_text(json.dumps(staging, indent=2, sort_keys=True) + "\n",
                                            encoding="utf-8")
            run_legacy(paths)
        elif paths.staging_record.is_file():
            staging = json.loads(paths.staging_record.read_text(encoding="utf-8"))
        summary = compare_outputs(paths, segment, paths.golden_json(pin), staging)
    except CompareError as error:
        print(f"compare-legacy: {error}", file=sys.stderr)
        return 1
    print_summary(summary)
    return 0


if __name__ == "__main__":
    sys.exit(main())
