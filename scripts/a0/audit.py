#!/usr/bin/env python3
"""A0.1: structural accuracy against a semantic oracle (decision 64, issue 95).

Joins the structural site dataset (`pdx-bench audit-sites`) with a compiler-grade
oracle's SCIP index (scip-python, rust-analyzer) and the language's own parser
(`syntax.py`), and scores the drawn bands with 4.13.1's measures. Writes one row per
structural site and per oracle call relation (the dataset A0.2 re-scores), and the
metrics.

    audit.py --corpus NAME --language python|rust --checkout DIR --sites FILE
             --scip FILE[=PREFIX] ... [--rust-analyzer BIN] --out DIR

Nothing here is ground truth but the oracle; the structural engine's typed answers,
the goldens, the engine differential and the legacy graph are never consulted as
truth. The matching rules are `methodology.md`'s M1 to M6, and their version is
ALGORITHM_VERSION.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import scip  # noqa: E402
import score  # noqa: E402
import syntax  # noqa: E402

ALGORITHM_VERSION = "a0-match-1"

CALLABLE_KINDS = {"Function", "Method", "Constructor", "Class", "Struct", "Macro"}
CONSTRUCTOR_METHODS = {"python": {"__init__", "__new__"}}


# --- the structural dataset ----------------------------------------------------------


@dataclass
class Dataset:
    header: dict
    files: dict[str, dict]
    defs: dict[tuple[str, int], dict]
    defs_by_path: dict[str, list[dict]]
    sites: list[dict]
    qn_counts: Counter

    @staticmethod
    def load(path: Path) -> "Dataset":
        header = {}
        files: dict[str, dict] = {}
        defs: dict[tuple[str, int], dict] = {}
        by_path: dict[str, list[dict]] = defaultdict(list)
        sites: list[dict] = []
        with path.open() as f:
            for raw in f:
                r = json.loads(raw)
                t = r["t"]
                if t == "header":
                    header = r
                elif t == "file":
                    files[r["path"]] = r
                elif t == "def":
                    defs[(r["path"], r["i"])] = r
                    by_path[r["path"]].append(r)
                elif t == "site":
                    sites.append(r)
        qn_counts = Counter((d["path"], d["qn"]) for d in defs.values())
        return Dataset(header, files, defs, dict(by_path), sites, qn_counts)


# --- sources -------------------------------------------------------------------------


class Sources:
    def __init__(self, root: Path):
        self.root = root
        self._cache: dict[str, bytes | None] = {}

    def get(self, path: str) -> bytes | None:
        if path not in self._cache:
            p = self.root / path
            self._cache[path] = p.read_bytes() if p.is_file() else None
        return self._cache[path]


def _utf16_to_bytes(line: bytes, units: int) -> int | None:
    text = line.decode("utf-8", "strict")
    count = 0
    for i, ch in enumerate(text):
        if count == units:
            return len(text[:i].encode())
        count += 2 if ord(ch) > 0xFFFF else 1
    return len(line) if count == units else None


def _utf32_to_bytes(line: bytes, units: int) -> int | None:
    text = line.decode("utf-8", "strict")
    return len(text[:units].encode()) if units <= len(text) else None


# --- the oracle ----------------------------------------------------------------------


@dataclass
class SymbolMap:
    """What an oracle symbol defined in the repository maps to among the structural
    definitions (rule M4)."""

    status: str  # mapped | ambiguous | unmapped
    target: tuple[str, int] | None = None
    candidates: tuple = ()
    # For an ambiguous symbol, the one definition each document that defines it holds
    # (rule M4b); a document holding several has none.
    by_document: dict = field(default_factory=dict)


@dataclass
class Oracle:
    tool: list[dict] = field(default_factory=list)
    documents: set[str] = field(default_factory=set)
    occurrences: dict[str, dict[tuple[int, int], list[tuple[str, int]]]] = field(
        default_factory=dict)
    definitions: dict[str, list[tuple[str, int, int]]] = field(default_factory=dict)
    local_definitions: dict[str, dict[str, list[tuple[int, int]]]] = field(default_factory=dict)
    import_spans: dict[str, list[tuple[int, int]]] = field(default_factory=dict)
    duplicate_documents: int = 0
    unconvertible: int = 0
    assumed_encoding: Counter = field(default_factory=Counter)

    def load(self, index: scip.Index, prefix: str, sources: Sources, assume: int,
             language: str) -> None:
        self.tool.append({"name": index.tool_name, "version": index.tool_version,
                          "prefix": prefix, "documents": len(index.documents)})
        for doc in sorted(index.documents, key=lambda d: d.relative_path):
            path = os.path.normpath(prefix + doc.relative_path) if prefix else doc.relative_path
            if path in self.documents:
                self.duplicate_documents += 1
                continue
            data = sources.get(path)
            if data is None:
                continue
            self.documents.add(path)
            starts = syntax.line_starts(data)
            encoding = doc.position_encoding
            occ: dict[tuple[int, int], list[tuple[str, int]]] = defaultdict(list)
            locals_: dict[str, list[tuple[int, int]]] = {}
            for o in doc.occurrences:
                span = self._bytes(data, starts, o, encoding, assume)
                if span is None:
                    self.unconvertible += 1
                    continue
                occ[span].append((o.symbol, o.roles))
                if o.roles & scip.ROLE_DEFINITION:
                    if scip.is_local(o.symbol):
                        locals_.setdefault(o.symbol, []).append(span)
                    else:
                        self.definitions.setdefault(o.symbol, []).append(
                            (path, span[0], span[1]))
            self.occurrences[path] = dict(occ)
            self.local_definitions[path] = locals_
            if language == "python":
                self.import_spans[path] = syntax.python_import_spans(data)

    def _bytes(self, data, starts, o, encoding, assume):
        out = []
        for line, char in ((o.start_line, o.start_char), (o.end_line, o.end_char)):
            if line >= len(starts):
                return None
            start = starts[line]
            end = starts[line + 1] if line + 1 < len(starts) else len(data)
            text = data[start:end]
            if encoding == scip.ENCODING_UTF8 or text.isascii():
                off = char
            else:
                enc = encoding
                if enc == scip.ENCODING_UNSPECIFIED:
                    enc = assume
                    self.assumed_encoding[enc] += 1
                try:
                    off = (_utf16_to_bytes(text, char) if enc == scip.ENCODING_UTF16
                           else _utf32_to_bytes(text, char))
                except UnicodeDecodeError:
                    return None
                if off is None:
                    return None
            if off > len(text):
                return None
            out.append(start + off)
        return (out[0], out[1])

    def references_at(self, path: str, token: tuple[int, int]) -> list[str]:
        """The symbols of the non-definition occurrences at exactly `token` (rule M2)."""
        occ = self.occurrences.get(path, {}).get(token, [])
        return sorted({s for s, roles in occ if not roles & scip.ROLE_DEFINITION})


def map_symbols(oracle: Oracle, dataset: Dataset, sources: Sources) -> dict[str, SymbolMap]:
    """Rule M4: an oracle definition maps to the innermost structural definition of the
    same name whose span holds the definition's name. Spans nest, so an outer
    definition holds an inner one's name but the inner one never holds the outer's.
    Two definitions with that innermost span, or two places of one symbol mapping to
    different definitions, are ambiguous, and map to nothing."""
    out = {}
    for symbol, places in sorted(oracle.definitions.items()):
        found = set()
        for path, s, e in places:
            data = sources.get(path)
            name = data[s:e].decode("utf-8", "replace") if data else None
            holding = [d for d in dataset.defs_by_path.get(path, [])
                       if d["name"] == name and d.get("span")
                       and d["span"][0] <= s and e <= d["span"][1]]
            if not holding:
                continue
            inner = min(d["span"][1] - d["span"][0] for d in holding)
            for d in holding:
                if d["span"][1] - d["span"][0] == inner:
                    found.add((d["path"], d["i"]))
        if len(found) == 1:
            found |= _redefinitions(oracle, dataset, sources, symbol, next(iter(found)))
        if len(found) == 1:
            out[symbol] = SymbolMap("mapped", next(iter(found)))
        elif found:
            per_doc: dict[str, set] = defaultdict(set)
            for key in found:
                per_doc[key[0]].add(key)
            out[symbol] = SymbolMap("ambiguous", None, tuple(sorted(found)),
                                    {doc: next(iter(keys)) for doc, keys in per_doc.items()
                                     if len(keys) == 1})
        else:
            out[symbol] = SymbolMap("unmapped")
    return out


def _redefinitions(oracle: Oracle, dataset: Dataset, sources: Sources, symbol: str,
                   mapped: tuple[str, int]) -> set[tuple[str, int]]:
    """The other definitions one oracle symbol also denotes: definitions with the
    mapped one's qualified name in its file (overloads, redefinitions) on whose name
    the symbol occurs, as the innermost definition of that name there. An indexer that
    gives several definitions one symbol cannot tell a call which of them it reaches."""
    path, _ = mapped
    d0 = dataset.defs[mapped]
    same = [d for d in dataset.defs_by_path.get(path, [])
            if d["qn"] == d0["qn"] and (d["path"], d["i"]) != mapped and d.get("span")]
    if not same:
        return set()
    data = sources.get(path)
    out = set()
    for (s, e), occ in oracle.occurrences.get(path, {}).items():
        if not any(sym == symbol for sym, _ in occ) or data[s:e].decode("utf-8", "replace") \
                != d0["name"]:
            continue
        holding = [d for d in dataset.defs_by_path[path]
                   if d["name"] == d0["name"] and d.get("span")
                   and d["span"][0] <= s and e <= d["span"][1]]
        if not holding:
            continue
        inner = min(holding, key=lambda d: d["span"][1] - d["span"][0])
        if inner in same:
            out.add((inner["path"], inner["i"]))
    return out


@dataclass
class Target:
    """What the oracle says the name at a site names (rules M2, M3)."""

    status: str  # mapped | external | local | ambiguous | unmapped | none | multiple | ...
    target: tuple[str, int] | None = None
    symbol: str | None = None


def internal_packages(oracle: Oracle, language: str) -> set[str]:
    """Rule M3: for Rust, the packages the repository defines (every package with a
    definition in an indexed document): a symbol of one of them is the repository's
    even when no document defines it (a macro-generated item). Python's indexer names
    dependencies it cannot identify after the project, so for Python only a symbol some
    document defines is the repository's."""
    if language != "rust":
        return set()
    return {scip.package(s) for s in oracle.definitions} - {None}


def oracle_target(oracle: Oracle, symbols: dict[str, SymbolMap], path: str,
                  token: tuple[int, int], packages: set[str] = frozenset(),
                  language: str = "") -> Target:
    refs = oracle.references_at(path, token)
    if not refs:
        return Target("none")
    resolved = []
    for sym in refs:
        if scip.is_local(sym):
            resolved.append(Target(local_status(oracle, path, sym), None, sym))
        elif sym in symbols:
            m = symbols[sym]
            if m.status == "ambiguous" and language == "rust" and path in m.by_document:
                # Rule M4b: rust-analyzer gives items of different crates (integration
                # tests, example binaries) one symbol. A crate's item paths are unique,
                # so a document that defines the symbol once names its own definition.
                resolved.append(Target("mapped", m.by_document[path], sym))
            else:
                resolved.append(Target(m.status, m.target, sym))
        elif scip.package(sym) in packages:
            resolved.append(Target("unmapped", None, sym))
        else:
            resolved.append(Target("external", None, sym))
    distinct = {(t.status, t.target) for t in resolved}
    if len(distinct) == 1:
        return resolved[0]
    return Target("multiple", None, " | ".join(refs))


def local_status(oracle: Oracle, path: str, symbol: str) -> str:
    """Rule M3: a document-local symbol is a local binding (a parameter, variable or
    closure) only when the document defines it outside an import statement. One the
    document never defines, or defines by an import the tool could not follow, is
    something the oracle did not resolve: no verdict."""
    places = oracle.local_definitions.get(path, {}).get(symbol)
    if not places:
        return "unresolved_local"
    imports = oracle.import_spans.get(path, [])
    if any(s <= p[0] and p[1] <= e for p in places for s, e in imports):
        return "unresolved_import"
    return "local"


# --- joining -------------------------------------------------------------------------


def same_target(language: str, dataset: Dataset, oracle_def: tuple[str, int],
                pdx_def: tuple[str, int]) -> tuple[bool, str | None]:
    """Rule M5: the same definition, or (Python) a class and its constructor method."""
    if oracle_def == pdx_def:
        return True, None
    ctors = CONSTRUCTOR_METHODS.get(language, set())
    for a, b in ((oracle_def, pdx_def), (pdx_def, oracle_def)):
        da, db = dataset.defs.get(a), dataset.defs.get(b)
        if da and db and db["name"] in ctors and db["path"] == da["path"] \
                and db.get("declaring_type") == da["i"] and da["kind"] == "Class":
            return True, "constructor_equivalence"
    return False, None


def site_token(site: dict, sources: Sources, language: str, by_node: dict,
               by_token_calls: dict) -> tuple[tuple[int, int] | None, str | None]:
    """The name token of a structural site, and the rule that found it. M1: the last
    name of the callee's own bytes, outside generic arguments. For a callee the engine
    names in words of its own (no bytes): M1b, the name token of the one call the
    language's parser finds over exactly the site's bytes; else M1c, the one call the
    parser finds inside the site's bytes whose name is the site's name (two: none);
    else M1d, the site's last name outside generic arguments, if it is the site's name
    (a callable passed as a value)."""
    data = sources.get(site["path"])
    if data is None:
        return None, None
    span = site.get("callee_span")
    if span:
        token = syntax.callee_name_token(data[span[0]:span[1]], span[0], language)
        if token:
            return token, "M1"
    whole = site.get("span")
    if not whole:
        return None, None
    calls = by_node.get((site["path"], tuple(whole)), [])
    if len(calls) == 1:
        return calls[0].token, "M1b"
    name = (site.get("name") or "").encode()
    inside = [c.token for c in by_token_calls.get(site["path"], ())
              if c.token and whole[0] <= c.token[0] and c.token[1] <= whole[1]
              and data[c.token[0]:c.token[1]] == name]
    if len(inside) == 1:
        return inside[0], "M1c"
    if inside:
        return None, None
    token = syntax.callee_name_token(data[whole[0]:whole[1]], whole[0], language)
    if token and data[token[0]:token[1]] == name:
        return token, "M1d"
    return None, None


def receiver_proof(site: dict, data: bytes | None) -> str:
    """How a Rust site's receiver is tied to something (A0.2's C5): `none` (no
    receiver), `self` (the receiver is exactly `self` or `Self`), `path_bound` (a path
    whose first part is `crate`, `self`, `super`, `Self` or a name the file defines or
    imports), `path_unproven`, or `value_unproven` (a value: `x.f()`, `self.a.f()`)."""
    receiver = site.get("receiver")
    if not receiver:
        return "none"
    if receiver in ("self", "Self"):
        return "self"
    callee = site.get("callee") or ""
    name = site.get("name") or ""
    sep = callee[len(receiver):len(callee) - len(name)] if callee.endswith(name) else ""
    if sep == "::":
        head = site.get("head") or ""
        if head in ("crate", "self", "super", "Self") or site.get("head_provenance") in (
                "defined", "internal_import", "external_import"):
            return "path_bound"
        return "path_unproven"
    return "value_unproven"


def build_rows(corpus: str, language: str, dataset: Dataset, oracle: Oracle,
               symbols: dict[str, SymbolMap], sources: Sources, parse) -> dict:
    """The site rows and the relation rows."""
    packages = internal_packages(oracle, language)
    reverse: dict[tuple[str, int], set[str]] = defaultdict(set)
    for sym, m in symbols.items():
        if m.status == "mapped":
            reverse[m.target].add(sym)
    lang_files = sorted(p for p, f in dataset.files.items()
                        if f.get("language") == language and f.get("outcome") == "extracted")

    # The language parser's calls and macro invocations, per file.
    calls_of: dict[str, list] = {}
    invocations_of: dict[str, list] = {}
    parse_failures = []
    by_node: dict[tuple[str, tuple[int, int]], list] = defaultdict(list)
    for path in lang_files:
        data = sources.get(path)
        if data is None:
            continue
        if language == "python":
            calls = syntax.python_calls(data)
            invocations = []
            if calls is None:
                parse_failures.append(path)
                continue
        else:
            calls, invocations = syntax.rust_calls(data, parse)
        calls_of[path] = calls
        invocations_of[path] = invocations
        for call in calls:
            if call.token and call.shape in ("call", "method"):
                by_node[(path, call.node)].append(call)

    # The structural sites of the language, by file and name token.
    site_rows = []
    by_token: dict[tuple[str, tuple[int, int]], list[dict]] = defaultdict(list)
    for site in dataset.sites:
        if site["path"] not in dataset.files or \
                dataset.files[site["path"]].get("language") != language:
            continue
        token, rule = site_token(site, sources, language, by_node, calls_of)
        row = _site_row(corpus, language, dataset, oracle, symbols, sources, site, token,
                        packages, reverse)
        row["token_rule"] = rule
        site_rows.append(row)
        if token:
            by_token[(site["path"], token)].append(row)

    # The oracle's call relations.
    relation_rows = []
    macro_invocations = []
    for path in sorted(calls_of):
        for m in invocations_of[path]:
            macro_invocations.append({"path": path, "token": list(m.token), "name": m.name,
                                      "tree": list(m.tree),
                                      "pdx_sites": [{"site": r["site"], "status": r["status"],
                                                     "band": r["band"]} for r in
                                                    by_token.get((path, m.token), [])]})
        indexed = path in oracle.documents
        for call in calls_of[path]:
            relation_rows.append(_relation_row(corpus, language, dataset, oracle, symbols,
                                               path, call, indexed, by_token, packages))
    return {"sites": site_rows, "relations": relation_rows,
            "macro_invocations": macro_invocations, "parse_failures": parse_failures,
            "language_files": lang_files}


def _site_row(corpus, language, dataset, oracle, symbols, sources, site, token, packages,
              reverse):
    path = site["path"]
    data = sources.get(path)
    engine = site.get("engine") or {}
    mapped = engine.get("mapped") or []
    typed_target = tuple(mapped[0]) if engine.get("verdict") == "typed" and len(mapped) == 1 \
        else None
    if site["status"] != "resolved":
        oracle_view = Target("not_a_call_site")
    elif path not in oracle.documents:
        oracle_view = Target("file_not_indexed")
    elif token is None:
        oracle_view = Target("no_token")
    else:
        oracle_view = oracle_target(oracle, symbols, path, token, packages, language)
    target = tuple(site["target"]) if site.get("target") else None
    caller = (path, site["caller"]) if site.get("caller") is not None else None
    line_hash = None
    if data is not None and site.get("span"):
        s = site["span"][0]
        ls = data.rfind(b"\n", 0, s) + 1
        le = data.find(b"\n", s)
        line_hash = hashlib.sha256(data[ls: le if le >= 0 else len(data)]).hexdigest()[:16]
    oracle_def = dataset.defs.get(oracle_view.target) if oracle_view.target else None
    return {
        "corpus": corpus,
        "language": language,
        "file": path,
        "site": site["i"],
        "status": site["status"],
        "materialized": site.get("materialized", True),
        "engine_site": site.get("engine_site"),
        "span": site.get("span"),
        # The interface's span lines are 1-based already (`engine/api/pdxe.c`,
        # `position_of`).
        "line": site.get("line"),
        "token": list(token) if token else None,
        "line_sha256_16": line_hash,
        "callee": site.get("callee"),
        "receiver": site.get("receiver"),
        "name": site.get("name"),
        "is_reference": site.get("is_reference"),
        "band": site.get("band"),
        "target": list(target) if target else None,
        "target_qn": dataset.defs[target]["qn"] if target else None,
        "caller": list(caller) if caller else None,
        "blocklisted": site.get("blocklisted"),
        "lexical": site.get("lexical"),
        "name_provenance": site.get("name_provenance"),
        "head": site.get("head"),
        "head_provenance": site.get("head_provenance"),
        "receiver_proof": receiver_proof(site, data) if language == "rust" else None,
        "engine_verdict": engine.get("verdict"),
        "engine_mapped": len(mapped),
        "typed_target": list(typed_target) if typed_target else None,
        "target_symbols": sorted(reverse.get(target, ())) if target else None,
        "typed_target_symbols": sorted(reverse.get(typed_target, ())) if typed_target else None,
        "oracle_status": oracle_view.status,
        "oracle_symbol": oracle_view.symbol,
        "oracle_target": list(oracle_view.target) if oracle_view.target else None,
        "oracle_target_qn": oracle_def["qn"] if oracle_def else None,
        "oracle_target_kind": oracle_def["kind"] if oracle_def else None,
        "oracle_target_overloaded": (dataset.qn_counts[(oracle_def["path"], oracle_def["qn"])] > 1)
        if oracle_def else None,
    }


def _relation_row(corpus, language, dataset, oracle, symbols, path, call, indexed, by_token,
                  packages):
    row = {"corpus": corpus, "language": language, "file": path, "shape": call.shape,
           "macro": call.macro, "token": list(call.token) if call.token else None,
           "node": list(call.node)}
    if not indexed:
        row["oracle_status"] = "file_not_indexed"
    elif call.token is None:
        row["oracle_status"] = "no_token"
    else:
        t = oracle_target(oracle, symbols, path, call.token, packages, language)
        row["oracle_status"] = t.status
        row["oracle_symbol"] = t.symbol
        if t.target:
            d = dataset.defs[t.target]
            row["oracle_target"] = list(t.target)
            row["oracle_target_qn"] = d["qn"]
            row["oracle_target_kind"] = d["kind"]
            row["oracle_target_callable"] = d["kind"] in CALLABLE_KINDS
            row["oracle_target_overloaded"] = dataset.qn_counts[(d["path"], d["qn"])] > 1
    sites = by_token.get((path, tuple(call.token)), []) if call.token else []
    row["pdx_sites"] = [{"site": s["site"], "status": s["status"], "band": s["band"]}
                        for s in sites]
    return row


def site_verdict(language: str, dataset: Dataset, row: dict, target,
                 target_symbols) -> tuple[str, str]:
    """Rule M6: a drawn edge's verdict, for the target `target` at the row's site. The
    oracle confirms the edge when the name at the site names the target's definition.
    It refutes it when it names something else: a definition of the repository other
    than the target, one outside the repository, or a local binding; and when it names
    a symbol of the repository no structural definition maps to while the target's own
    definition is a different symbol (`target_symbols`, M4 read backwards)."""
    status = row["oracle_status"]
    if status in ("mapped",):
        ok, how = same_target(language, dataset, tuple(row["oracle_target"]), tuple(target))
        return ("correct", how or "same_definition") if ok else ("false_positive", "other_target")
    if status == "external":
        return "false_positive", "oracle_external"
    if status in ("unmapped", "ambiguous") and target_symbols and \
            row.get("oracle_symbol") not in target_symbols:
        return "false_positive", "other_target_unmapped"
    if status == "local":
        return "false_positive", "oracle_local"
    return "unverifiable", status


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", required=True)
    ap.add_argument("--language", required=True, choices=("python", "rust"))
    ap.add_argument("--checkout", required=True, type=Path)
    ap.add_argument("--sites", required=True, type=Path)
    ap.add_argument("--scip", required=True, action="append")
    ap.add_argument("--rust-analyzer")
    ap.add_argument("--out", required=True, type=Path)
    args = ap.parse_args()

    dataset = Dataset.load(args.sites)
    sources = Sources(args.checkout)
    oracle = Oracle()
    for spec in args.scip:
        file, _, prefix = spec.partition("=")
        index = scip.read_index(Path(file).read_bytes())
        oracle.load(index, prefix, sources,
                    assume=scip.ENCODING_UTF16 if args.language == "python" else scip.ENCODING_UTF8,
                    language=args.language)
    symbols = map_symbols(oracle, dataset, sources)
    parse = None
    if args.language == "rust":
        env = dict(os.environ)
        parse = syntax.rust_analyzer_parser(args.rust_analyzer, env)
    rows = build_rows(args.corpus, args.language, dataset, oracle, symbols, sources, parse)
    for r in rows["sites"]:
        if r["band"] in score.DRAWN:
            v, why = site_verdict(args.language, dataset, r, r["target"], r["target_symbols"])
            r["verdict"], r["verdict_reason"] = v, why
        if r.get("typed_target"):
            v, why = site_verdict(args.language, dataset, r, r["typed_target"],
                                  r["typed_target_symbols"])
            r["typed_target_verdict"] = v
    args.out.mkdir(parents=True, exist_ok=True)
    with (args.out / f"{args.corpus}.sites.jsonl").open("w") as f:
        for r in rows["sites"]:
            f.write(json.dumps(r, sort_keys=True) + "\n")
    with (args.out / f"{args.corpus}.relations.jsonl").open("w") as f:
        for r in rows["relations"]:
            f.write(json.dumps(r, sort_keys=True) + "\n")
    meta = {
        "corpus": args.corpus,
        "language": args.language,
        "algorithm_version": ALGORITHM_VERSION,
        "dataset_format": dataset.header.get("format"),
        "engine_health": dataset.header.get("engine_health"),
        "oracle_tools": oracle.tool,
        "oracle_documents": len(oracle.documents),
        "oracle_duplicate_documents": oracle.duplicate_documents,
        "oracle_unconvertible_occurrences": oracle.unconvertible,
        "oracle_assumed_encoding": dict(oracle.assumed_encoding),
        "symbols": dict(Counter(m.status for m in symbols.values())),
        "language_files": len(rows["language_files"]),
        # Offsets are the dataset's: each file the oracle reads must have the length
        # the structural engine extracted (masking keeps lengths).
        "length_mismatches": sorted(
            p for p in rows["language_files"]
            if sources.get(p) is not None
            and len(sources.get(p)) != dataset.files[p].get("source_len")),
        "language_files_indexed": sum(1 for p in rows["language_files"] if p in oracle.documents),
        "parse_failures": rows["parse_failures"],
        "macro_invocations": rows["macro_invocations"],
    }
    (args.out / f"{args.corpus}.meta.json").write_text(json.dumps(meta, indent=1, sort_keys=True))


if __name__ == "__main__":
    main()
