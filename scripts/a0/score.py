"""A0.1's measures and A0.2's counterfactuals (decision 64), from `audit.py`'s rows.

4.13.1, exactly (METRICS_VERSION):

- precision = drawn edges whose site has an oracle verdict and whose target the
  oracle confirms / drawn edges whose site has an oracle verdict;
- recall = oracle call relations at parsed structural call sites that a drawn edge
  reproduces / oracle call relations at parsed structural call sites;
- oracle-verifiable fraction = drawn edges with an oracle verdict / drawn edges.

Every ratio is reported with its numerator and denominator. A counterfactual changes
only which edge a site draws; the oracle's verdicts and the relations are A0.1's,
fixed.
"""

from __future__ import annotations

from collections import Counter

METRICS_VERSION = "a0-metrics-1"

DRAWN = ("typed", "import-guided", "inheritance-guided", "exact", "scoped")
POLICIES = ("C0", "C1", "C2", "C3", "C4", "C5", "C6")
POLICY_TEXT = {
    "C0": "current behaviour",
    "C1": "`exact` not drawn",
    "C2": "`scoped` not drawn",
    "C3": "neither `exact` nor `scoped` drawn",
    "C4": "a valid typed answer is accepted before the generic-name blocklist",
    "C5": "a Rust call on an unproven member or static receiver does not fall through "
          "to `exact` or `scoped`",
    "C6": "C3, C4 and C5 together",
}

# A Rust receiver C5 treats as unproven (audit.receiver_proof).
UNPROVEN = ("path_unproven", "value_unproven")


def ratio(num: int, den: int) -> dict:
    return {"numerator": num, "denominator": den,
            "value": round(num / den, 6) if den else None}


def edge_under(policy: str, row: dict) -> tuple[str | None, list | None, str | None]:
    """(band, target, verdict) the site draws under `policy`, or (None, None, None)."""
    band = row.get("band")
    if row.get("status") != "resolved":
        return None, None, None
    # Stage 4 draws an edge only for a site it can persist (a position and a node-type
    # path); a site typed resolution found itself is counted in coverage alone.
    if not row.get("materialized", True):
        return None, None, None
    if policy in ("C4", "C6") and band == "blocked" and row.get("typed_target"):
        return "typed", row["typed_target"], row.get("typed_target_verdict")
    if band not in DRAWN:
        return None, None, None
    if policy in ("C1", "C3", "C6") and band == "exact":
        return None, None, None
    if policy in ("C2", "C3", "C6") and band == "scoped":
        return None, None, None
    if policy in ("C5", "C6") and band in ("exact", "scoped") and \
            row.get("receiver_proof") in UNPROVEN:
        return None, None, None
    return band, row["target"], row.get("verdict")


def precision_table(rows: list[dict], policy: str = "C0", kinds=("call", "reference")) -> dict:
    """Per band and for the union: drawn, verifiable, correct, false positives."""
    per: dict[str, Counter] = {b: Counter() for b in DRAWN}
    for r in rows:
        kind = "reference" if r.get("is_reference") else "call"
        if kind not in kinds:
            continue
        band, _target, verdict = edge_under(policy, r)
        if band is None:
            continue
        c = per[band]
        c["drawn"] += 1
        if verdict in ("correct", "false_positive"):
            c["verifiable"] += 1
            c[verdict] += 1
    out = {}
    union = Counter()
    for b in DRAWN:
        c = per[b]
        union.update(c)
        out[b] = _band_entry(c)
    out["union"] = _band_entry(union)
    return out


def _band_entry(c: Counter) -> dict:
    return {
        "drawn": c["drawn"],
        "oracle_verifiable": c["verifiable"],
        "correct": c["correct"],
        "false_positive": c["false_positive"],
        "precision": ratio(c["correct"], c["verifiable"]),
        "oracle_verifiable_fraction": ratio(c["verifiable"], c["drawn"]),
    }


def relations(rel_rows: list[dict]) -> list[dict]:
    """The oracle call relations: calls whose name the oracle resolves to one
    definition of the repository that is callable in the structural model."""
    return [r for r in rel_rows
            if r.get("oracle_status") == "mapped" and r.get("oracle_target_callable")]


def recall_table(site_rows: list[dict], rel_rows: list[dict], policy: str = "C0") -> dict:
    """Recall at parsed structural call sites, and where the rest of the oracle's
    relations went (never mixing a missing site with a missing resolution)."""
    by_site = {(r["file"], r["site"]): r for r in site_rows}
    buckets = Counter()
    for rel in relations(rel_rows):
        sites = rel.get("pdx_sites") or []
        resolved = [s for s in sites if s["status"] == "resolved"]
        if not resolved:
            if sites:
                buckets["site_not_a_call_site"] += 1
            elif rel["shape"] == "macro_tt":
                buckets["site_missing_macro_token_tree"] += 1
            elif rel["shape"] == "macro_rules_body":
                buckets["site_missing_macro_rules_body"] += 1
            else:
                buckets["site_missing"] += 1
            continue
        if len(resolved) > 1:
            buckets["site_ambiguous"] += 1
            continue
        row = by_site[(rel["file"], resolved[0]["site"])]
        band, target, verdict = edge_under(policy, row)
        buckets["at_parsed_site"] += 1
        if band is None and not row.get("materialized", True):
            buckets["resolution_not_materialized:" + (row.get("band") or "none")] += 1
        elif band is None:
            buckets["resolution_not_drawn:" + (row.get("band") or "none")] += 1
        elif verdict == "correct":
            buckets["drawn_correct"] += 1
        else:
            buckets["drawn_wrong"] += 1
    unmapped = Counter()
    for rel in rel_rows:
        status = rel.get("oracle_status")
        if status == "mapped" and not rel.get("oracle_target_callable"):
            unmapped["target_not_callable_in_structural_model"] += 1
        elif status in ("ambiguous", "unmapped", "multiple"):
            unmapped["oracle_target_" + status] += 1
        elif status in ("external", "local", "none", "no_token", "file_not_indexed",
                        "unresolved_local", "unresolved_import"):
            unmapped["not_a_relation:" + status] += 1
    total = sum(1 for _ in relations(rel_rows))
    return {
        "oracle_relations": total,
        "at_parsed_structural_sites": buckets["at_parsed_site"],
        "drawn_correct": buckets["drawn_correct"],
        "recall": ratio(buckets["drawn_correct"], buckets["at_parsed_site"]),
        "end_to_end_recall": ratio(buckets["drawn_correct"], total),
        "breakdown": dict(sorted(buckets.items())),
        "oracle_calls_outside_relations": dict(sorted(unmapped.items())),
    }


def counterfactuals(site_rows: list[dict], rel_rows: list[dict]) -> dict:
    out = {}
    base = {(r["file"], r["site"]): edge_under("C0", r) for r in site_rows}
    for p in POLICIES:
        prec = precision_table(site_rows, p)["union"]
        rec = recall_table(site_rows, rel_rows, p)
        removed_fp = lost_tp = added_tp = added_fp = 0
        for r in site_rows:
            b0 = base[(r["file"], r["site"])]
            b1 = edge_under(p, r)
            v0 = b0[2] if b0[0] else None
            v1 = b1[2] if b1[0] else None
            if b0[0] and not b1[0]:
                if v0 == "false_positive":
                    removed_fp += 1
                elif v0 == "correct":
                    lost_tp += 1
            elif b1[0] and not b0[0]:
                if v1 == "correct":
                    added_tp += 1
                elif v1 == "false_positive":
                    added_fp += 1
        out[p] = {
            "policy": POLICY_TEXT[p],
            "drawn": prec["drawn"],
            "oracle_verifiable": prec["oracle_verifiable"],
            "precision": prec["precision"],
            "oracle_verifiable_fraction": prec["oracle_verifiable_fraction"],
            "recall": rec["recall"],
            "false_positives_removed": removed_fp,
            "true_positives_lost": lost_tp,
            "true_positives_added": added_tp,
            "false_positives_added": added_fp,
            "typed": precision_table(site_rows, p)["typed"]["precision"],
        }
    return out


FP_CATEGORIES = (
    "self_edge",
    "external_receiver_mistaken_for_local",
    "constructor_collision",
    "hierarchy_evidence_missing",
    "import_provenance_failure",
    "unknown_or_unproven_receiver",
    "same_short_name_on_unrelated_type",
    "module_or_package_uniqueness_only",
    "other",
)


def fp_tags(row: dict, defs: dict) -> list[str]:
    """Every category an exact or scoped false positive fits; the first is primary."""
    tags = []
    target = tuple(row["target"])
    caller = tuple(row["caller"]) if row.get("caller") else None
    receiver = row.get("receiver")
    tdef = defs.get(target, {})
    if caller == target:
        tags.append("self_edge")
    if row.get("oracle_status") in ("external", "local"):
        tags.append("external_receiver_mistaken_for_local")
    if tdef.get("kind") in ("Class", "Struct", "Constructor") or \
            tdef.get("name") in ("new", "__init__", "__new__", "default"):
        tags.append("constructor_collision")
    if receiver in ("self", "Self", "cls", "super()", "super", "this"):
        tags.append("hierarchy_evidence_missing")
    if row.get("name_provenance") in ("internal_import", "external_import") or \
            row.get("head_provenance") in ("internal_import", "external_import"):
        tags.append("import_provenance_failure")
    if receiver and receiver not in ("self", "Self", "cls", "super()", "super", "this") and \
            (row.get("receiver_proof") in UNPROVEN or row.get("language") == "python"):
        tags.append("unknown_or_unproven_receiver")
    odef = defs.get(tuple(row["oracle_target"])) if row.get("oracle_target") else None
    if odef and odef.get("name") == tdef.get("name"):
        tags.append("same_short_name_on_unrelated_type")
    if row.get("band") == "scoped":
        tags.append("module_or_package_uniqueness_only")
    if not tags:
        tags.append("other")
    return [c for c in FP_CATEGORIES if c in tags]
