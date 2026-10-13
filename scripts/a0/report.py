#!/usr/bin/env python3
"""A0.1 and A0.2's results (decision 64), from `audit.py`'s rows.

    report.py --audit DIR --corpus NAME --sites FILE --provenance JSON --out FILE
              [--dataset FILE.gz] [--mismatches FILE]

Writes the corpus's result document: provenance, 4.13.1's measures band by band
with every numerator and denominator, the recall breakdown, the false positives of
`exact` and `scoped` by category, overloads, the blocklist, Rust's macro token trees
and unknown receivers, A0.2's counterfactuals and representative examples (file,
line and byte span only: no source). Optionally appends the corpus's mismatches
(every false positive and every oracle relation not reproduced) to a JSON Lines
file, and writes the corpus's whole site dataset, gzip-compressed without a
timestamp so the bytes are deterministic.
"""

from __future__ import annotations

import argparse
import gzip
import io
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import audit  # noqa: E402
import scip  # noqa: E402
import score  # noqa: E402

EXAMPLES = 5


def load(audit_dir: Path, corpus: str):
    sites = [json.loads(x) for x in (audit_dir / f"{corpus}.sites.jsonl").open()]
    rels = [json.loads(x) for x in (audit_dir / f"{corpus}.relations.jsonl").open()]
    meta = json.loads((audit_dir / f"{corpus}.meta.json").read_text())
    return sites, rels, meta


_STRING = __import__("re").compile(r'"(?:[^"\\\n]|\\.)*"|\'(?:[^\'\\\n]|\\.){2,}\'')


_COMMENT = __import__("re").compile(r"//[^\n]*|/\*.*?\*/|#\s[^\n]*", __import__("re").S)


def callee_text(text: str | None) -> str | None:
    """A callee as results carry it: string literals' contents and comments elided (a
    chain's callee holds its earlier calls' arguments and comments) and whitespace
    runs collapsed."""
    if text is None:
        return None
    text = _STRING.sub(lambda m: m.group(0)[0] * 2, text)
    return " ".join(_COMMENT.sub(" ", text).split())


def where(row: dict) -> dict:
    token = row.get("token")
    return {"file": row["file"], "line": row.get("line"), "token": token,
            "span": row.get("span") or row.get("node")}


def line_of(row: dict, sources: audit.Sources) -> int | None:
    data = sources.get(row["file"])
    pos = (row.get("token") or row.get("span") or row.get("node") or [None])[0]
    if data is None or pos is None:
        return None
    return data.count(b"\n", 0, pos) + 1


def first(rows, n=EXAMPLES):
    return sorted(rows, key=lambda r: (r["file"], (r.get("token") or r.get("span")
                                                    or r.get("node") or [0])[0]))[:n]


def site_example(r: dict) -> dict:
    return {**where(r), "site": r["site"], "callee": callee_text(r.get("callee")), "band": r.get("band"),
            "pdx_target": r.get("target_qn"), "oracle_status": r.get("oracle_status"),
            "oracle_target": r.get("oracle_target_qn") or scip.short(r.get("oracle_symbol")),
            "reason": r.get("verdict_reason"), "receiver_proof": r.get("receiver_proof")}


def relation_example(r: dict, bucket: str, site: dict | None) -> dict:
    return {**where(r), "shape": r["shape"], "macro": r.get("macro"),
            "oracle_target": r.get("oracle_target_qn") or scip.short(r.get("oracle_symbol")),
            "bucket": bucket,
            "pdx_site": site["site"] if site else None,
            "pdx_band": site.get("band") if site else None,
            "pdx_target": site.get("target_qn") if site else None,
            "callee": callee_text(site.get("callee")) if site else None}


def relation_bucket(rel: dict, by_site: dict, policy: str = "C0"):
    sites = rel.get("pdx_sites") or []
    resolved = [s for s in sites if s["status"] == "resolved"]
    if not resolved:
        if sites:
            return "site_not_a_call_site", None
        return {"macro_tt": "site_missing_macro_token_tree",
                "macro_rules_body": "site_missing_macro_rules_body"}.get(
            rel["shape"], "site_missing"), None
    if len(resolved) > 1:
        return "site_ambiguous", None
    row = by_site[(rel["file"], resolved[0]["site"])]
    band, _target, verdict = score.edge_under(policy, row)
    if band is None and not row.get("materialized", True):
        return "resolution_not_materialized:" + (row.get("band") or "none"), row
    if band is None:
        return "resolution_not_drawn:" + (row.get("band") or "none"), row
    return ("drawn_correct" if verdict == "correct" else "drawn_wrong"), row


def build(corpus: str, sites: list[dict], rels: list[dict], meta: dict, defs: dict,
          provenance: dict, sources: audit.Sources) -> tuple[dict, list[dict]]:
    language = meta["language"]
    for r in sites + rels:
        r["line"] = line_of(r, sources)
    by_site = {(r["file"], r["site"]): r for r in sites}
    resolved = [r for r in sites if r["status"] == "resolved"]
    drawn = [r for r in resolved if r.get("band") in score.DRAWN and r.get("materialized", True)]

    # 4.13.1, all drawn edges, and calls alone.
    precision_all = score.precision_table(sites)
    precision_calls = score.precision_table(sites, kinds=("call",))
    precision_refs = score.precision_table(sites, kinds=("reference",))
    recall = score.recall_table(sites, rels)

    # Recall's breakdown, with examples.
    buckets: dict[str, list] = defaultdict(list)
    for rel in score.relations(rels):
        b, row = relation_bucket(rel, by_site)
        buckets[b].append((rel, row))
    extraction_loss = sum(len(v) for k, v in buckets.items()
                          if k.startswith("site_missing") or k == "site_not_a_call_site")
    resolution_loss = sum(len(v) for k, v in buckets.items()
                          if k.startswith("resolution_not_") or k == "drawn_wrong")
    fn_examples = {}
    for k, v in sorted(buckets.items()):
        if k == "drawn_correct":
            continue
        chosen = sorted(v, key=lambda x: (x[0]["file"], (x[0].get("token")
                                                         or x[0]["node"])[0]))[:EXAMPLES]
        fn_examples[k] = [relation_example(rel, k, row) for rel, row in chosen]

    # False positives, band by band; exact and scoped by category.
    fps = [r for r in drawn if r.get("verdict") == "false_positive"]
    fp_examples = {b: [site_example(r) for r in first([x for x in fps if x["band"] == b])]
                   for b in score.DRAWN}
    categories = {}
    for band in ("exact", "scoped"):
        primary = Counter()
        tags = Counter()
        examples = defaultdict(list)
        for r in fps:
            if r["band"] != band:
                continue
            t = score.fp_tags(r, defs)
            r["fp_category"] = t[0]
            r["fp_tags"] = t
            primary[t[0]] += 1
            tags.update(t)
            examples[t[0]].append(r)
        categories[band] = {
            "false_positives": sum(primary.values()),
            "primary": {c: primary[c] for c in score.FP_CATEGORIES},
            "tagged": {c: tags[c] for c in score.FP_CATEGORIES},
            "examples": {c: [site_example(r) for r in first(examples[c], 3)]
                         for c in score.FP_CATEGORIES if examples[c]},
        }

    # Overloads: relations whose target shares its qualified name in its file.
    overload_rel = [r for r in score.relations(rels) if r.get("oracle_target_overloaded")]
    ov = Counter(total=len(overload_rel))
    for rel in overload_rel:
        b, row = relation_bucket(rel, by_site)
        if row is None:
            continue
        ov["structural_site_exists"] += 1
        if row.get("engine_verdict") == "typed":
            ov["typed_answer_supplied"] += 1
            if row.get("engine_mapped", 0) > 1 and row.get("band") != "typed":
                ov["withheld_qn_maps_to_several_definitions"] += 1
        if row.get("band") in score.DRAWN:
            ov["drawn"] += 1
            ov["correct_overload_selected" if b == "drawn_correct" else
               "wrong_overload_selected"] += 1
    ambiguous_targets = sum(1 for r in rels if r.get("oracle_status") == "ambiguous")
    overloads = {"oracle_verifiable_overloaded_calls": dict(ov),
                 "oracle_calls_whose_target_is_ambiguous": ambiguous_targets,
                 "note": "an overloaded call is an oracle relation whose target shares its "
                         "qualified name with another definition of its file; a call whose "
                         "oracle symbol denotes several such definitions has no verdict"}

    # The blocklist.
    blocked = [r for r in resolved if r.get("blocklisted")]
    bl = Counter(sites=len(blocked))
    bl_examples = []
    for r in blocked:
        has_verdict = r.get("oracle_status") in ("mapped", "external", "local")
        bl["oracle_verifiable"] += has_verdict
        relation = r.get("oracle_status") == "mapped" and \
            (defs.get(tuple(r["oracle_target"]), {}).get("kind") in audit.CALLABLE_KINDS)
        bl["oracle_relation"] += relation
        if r.get("typed_target"):
            bl["typed_answer_suppressed_by_blocklist"] += 1
            v = r.get("typed_target_verdict")
            bl["suppressed_typed_answer_" + (v or "unverifiable")] += 1
            if v == "correct" and len(bl_examples) < EXAMPLES:
                bl_examples.append(site_example(r))
        else:
            bl["no_typed_answer_and_blocked"] += 1
            bl["no_typed_answer_and_blocked_with_oracle_relation"] += relation
        bl["final_band:" + str(r.get("band"))] += 1
    rec_c4 = score.recall_table(sites, rels, "C4")
    blocklist = {
        **dict(sorted(bl.items())),
        "edges_drawn": 0,
        "recall_c0": recall["recall"],
        "recall_c4": rec_c4["recall"],
        "relations_recovered_in_c4": rec_c4["drawn_correct"] - recall["drawn_correct"],
        "examples_correct_suppressed": bl_examples,
    }

    out = {
        "corpus": corpus,
        "language": language,
        "provenance": {
            **provenance,
            "oracle_tools": meta["oracle_tools"],
            "metrics_version": score.METRICS_VERSION,
            "site_matching_version": audit.ALGORITHM_VERSION,
            "dataset_format": meta["dataset_format"],
            "engine_health": meta["engine_health"],
        },
        "coverage": {
            "language_files": meta["language_files"],
            "language_files_indexed_by_oracle": meta["language_files_indexed"],
            "oracle_symbols": meta["symbols"],
            "oracle_unconvertible_occurrences": meta["oracle_unconvertible_occurrences"],
            "oracle_assumed_encoding": meta["oracle_assumed_encoding"],
            "parse_failures": meta["parse_failures"],
            "structural_call_sites": len(resolved),
            "structural_sites_not_call_sites": sum(1 for r in sites if r["status"] != "resolved"),
            "structural_call_sites_by_band": dict(sorted(Counter(r["band"] for r in resolved)
                                                         .items())),
            "structural_call_sites_without_name_token": sum(1 for r in resolved
                                                            if not r.get("token")),
            "structural_call_sites_not_materialized_by_band": dict(sorted(Counter(
                r["band"] for r in resolved if not r.get("materialized", True)).items())),
            "engine_sites": sum(1 for r in sites if r.get("engine_site")),
            "drawn_edges_by_kind": {
                "CALLS": sum(1 for r in drawn if not r.get("is_reference")),
                "CALL_REFERENCE": sum(1 for r in drawn if r.get("is_reference"))},
            "name_token_rules": dict(sorted(Counter(str(r.get("token_rule"))
                                                    for r in resolved).items())),
            "oracle_call_sites": len(rels),
            "oracle_call_sites_by_status": dict(sorted(Counter(r.get("oracle_status")
                                                               for r in rels).items())),
        },
        "precision": precision_all,
        "precision_calls_only": precision_calls,
        "precision_references_only": precision_refs,
        "unverifiable_drawn": dict(sorted(Counter(
            (r["band"] + ":" + str(r.get("verdict_reason"))) for r in drawn
            if r.get("verdict") == "unverifiable").items())),
        "recall": recall,
        "extraction_loss": extraction_loss,
        "resolution_loss": resolution_loss,
        "false_positive_examples": fp_examples,
        "false_negative_examples": fn_examples,
        "exact_scoped_false_positives": categories,
        "overloads": overloads,
        "blocklist": blocklist,
        "counterfactuals": score.counterfactuals(sites, rels),
    }
    if language == "rust":
        out["macros"] = macro_stats(meta, rels, by_site)
        out["receivers"] = receiver_stats(resolved)

    mismatches = []
    for r in fps:
        mismatches.append({"corpus": corpus, "kind": "false_positive", **site_example(r),
                           "category": r.get("fp_category"), "tags": r.get("fp_tags")})
    for b, items in sorted(buckets.items()):
        if b == "drawn_correct":
            continue
        for rel, row in items:
            mismatches.append({"corpus": corpus, "kind": "missing",
                               **relation_example(rel, b, row)})
    mismatches.sort(key=lambda m: (m["kind"], m["file"], (m.get("token") or m.get("span")
                                                          or [0])[0]))
    return out, mismatches


def macro_stats(meta: dict, rels: list[dict], by_site: dict) -> dict:
    inv = meta["macro_invocations"]
    tt = [r for r in rels if r["shape"] == "macro_tt"]
    body = [r for r in rels if r["shape"] == "macro_rules_body"]
    c = Counter()
    for r in tt:
        c["call_text_in_token_trees"] += 1
        visible = r.get("oracle_status") not in ("none", "file_not_indexed", "no_token")
        c["visible_to_oracle"] += visible
        relation = r.get("oracle_status") == "mapped" and r.get("oracle_target_callable")
        c["oracle_relations"] += bool(relation)
        sites = [s for s in r.get("pdx_sites") or [] if s["status"] == "resolved"]
        if sites:
            row = by_site[(r["file"], sites[0]["site"])]
            if row.get("materialized", True):
                c["with_structural_call_site"] += 1
            else:
                c["with_engine_site_only"] += 1
                c["engine_site_band:" + str(row.get("band"))] += 1
            if row.get("band") in score.DRAWN and row.get("materialized", True):
                c["with_drawn_edge"] += 1
                c["drawn_correct"] += row.get("verdict") == "correct"
        elif r.get("pdx_sites"):
            c["with_unconfirmed_site_only"] += 1
        else:
            c["invisible_to_structural_calls"] += 1
            c["invisible_with_oracle_relation"] += bool(relation)
    return {
        "macro_invocations": len(inv),
        "macro_invocations_with_structural_site": sum(1 for m in inv if m["pdx_sites"]),
        "macro_invocations_by_band": dict(sorted(Counter(
            s["band"] or s["status"] for m in inv for s in m["pdx_sites"]).items())),
        "token_tree_calls": dict(sorted(c.items())),
        "macro_rules_body_call_text": len(body),
        "macro_rules_body_oracle_relations": sum(
            1 for r in body if r.get("oracle_status") == "mapped"
            and r.get("oracle_target_callable")),
    }


def receiver_stats(resolved: list[dict]) -> dict:
    out = {}
    for proof in ("none", "self", "path_bound", "path_unproven", "value_unproven"):
        rows = [r for r in resolved if r.get("receiver_proof") == proof]
        bands = Counter(r.get("band") for r in rows)
        es = [r for r in rows if r.get("band") in ("exact", "scoped")]
        verif = [r for r in es if r.get("verdict") in ("correct", "false_positive")]
        ok = sum(1 for r in verif if r["verdict"] == "correct")
        out[proof] = {"call_sites": len(rows), "by_band": dict(sorted(bands.items(), key=str)),
                      "exact_or_scoped_drawn": len(es),
                      "exact_or_scoped_precision": score.ratio(ok, len(verif))}
    return out


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--audit", required=True, type=Path)
    ap.add_argument("--corpus", required=True)
    ap.add_argument("--sites", required=True, type=Path)
    ap.add_argument("--checkout", required=True, type=Path)
    ap.add_argument("--provenance", required=True)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--dataset", type=Path)
    ap.add_argument("--mismatches", type=Path)
    ap.add_argument("--false-positives-only", action="store_true",
                    help="append only the false positives to --mismatches")
    args = ap.parse_args()
    sites, rels, meta = load(args.audit, args.corpus)
    ds = audit.Dataset.load(args.sites)
    out, mismatches = build(args.corpus, sites, rels, meta, ds.defs,
                            json.loads(args.provenance), audit.Sources(args.checkout))
    args.out.write_text(json.dumps(out, indent=1, sort_keys=True) + "\n")
    if args.mismatches:
        with args.mismatches.open("a") as f:
            for m in mismatches:
                if args.false_positives_only and m["kind"] != "false_positive":
                    continue
                compact = {k: v for k, v in m.items() if v is not None}
                if compact.get("token"):
                    compact.pop("span", None)
                f.write(json.dumps(compact, sort_keys=True) + "\n")
    if args.dataset:
        buf = io.BytesIO()
        with gzip.GzipFile(fileobj=buf, mode="wb", mtime=0, filename="") as z:
            for kind, rows in (("site", sites), ("relation", rels)):
                for r in rows:
                    z.write((json.dumps({"kind": kind, **dataset_row(r, kind)},
                                        sort_keys=True) + "\n").encode())
        args.dataset.write_bytes(buf.getvalue())


def dataset_row(r: dict, kind: str) -> dict:
    """Section 10's fields: no source text beyond the callee's own name and path."""
    if kind == "site":
        verdict = r.get("verdict") if r.get("band") in score.DRAWN else None
        return {
            "corpus": r["corpus"], "language": r["language"], "file": r["file"],
            "span": r.get("span"), "token": r.get("token"), "line": r.get("line"),
            "line_sha256_16": r.get("line_sha256_16"), "site": f"{r['file']}#{r['site']}",
            "status": r["status"], "callee": callee_text(r.get("callee")),
            "receiver": callee_text(r.get("receiver")),
            "band": r.get("band"), "pdx_target": r.get("target_qn"),
            "pdx_target_path": r["target"][0] if r.get("target") else None,
            "oracle_verifiable": verdict in ("correct", "false_positive"),
            "oracle_status": r.get("oracle_status"),
            "oracle_symbol": scip.short(r.get("oracle_symbol")),
            "oracle_target": r.get("oracle_target_qn"),
            "oracle_target_path": r["oracle_target"][0] if r.get("oracle_target") else None,
            "verdict": verdict, "reason": r.get("verdict_reason"),
            "category": r.get("fp_category"), "blocklisted": r.get("blocklisted"),
            "typed_answer": r.get("typed_target") is not None,
            "typed_answer_verdict": r.get("typed_target_verdict"),
            "receiver_proof": r.get("receiver_proof"),
        }
    return {
        "corpus": r["corpus"], "language": r["language"], "file": r["file"],
        "span": r.get("node"), "token": r.get("token"), "line": r.get("line"),
        "shape": r["shape"], "macro": r.get("macro"),
        "oracle_status": r.get("oracle_status"), "oracle_target": r.get("oracle_target_qn"),
        "oracle_target_path": r["oracle_target"][0] if r.get("oracle_target") else None,
        "relation": r.get("oracle_status") == "mapped" and bool(r.get("oracle_target_callable")),
        "pdx_sites": [f"{r['file']}#{s['site']}" for s in r.get("pdx_sites") or []],
    }


if __name__ == "__main__":
    main()
