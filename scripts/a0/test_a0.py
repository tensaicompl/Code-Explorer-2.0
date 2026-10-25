#!/usr/bin/env python3
"""Tests of A0.1's audit tooling (decision 64) that need no indexer: the SCIP reader,
the name tokens, rust-analyzer's tree dump, the verdict rules and 4.13.1's formulas
on a synthetic case, and `run.py`'s check of the executable it measures (issue 115).

    python3 scripts/a0/test_a0.py
"""

from __future__ import annotations

import json
import subprocess
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import audit  # noqa: E402
import rust_candidates  # noqa: E402
import scip  # noqa: E402
import score  # noqa: E402
import syntax  # noqa: E402


def varint(n: int) -> bytes:
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        if n:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def field(number: int, value) -> bytes:
    if isinstance(value, int):
        return varint(number << 3) + varint(value)
    data = value.encode() if isinstance(value, str) else value
    return varint(number << 3 | 2) + varint(len(data)) + data


class ScipReader(unittest.TestCase):
    def test_a_document_and_its_occurrences(self):
        packed = b"".join(varint(v) for v in (3, 4, 9))
        occ = field(1, packed) + field(2, "rust-analyzer cargo demo 0.1.0 f().") + field(3, 1)
        doc = field(1, "src/lib.rs") + field(2, occ) + field(4, "rust") + field(6, 1)
        meta = field(2, field(1, "tool") + field(2, "1.0"))
        index = scip.read_index(field(1, meta) + field(2, doc))
        self.assertEqual((index.tool_name, index.tool_version), ("tool", "1.0"))
        (d,) = index.documents
        self.assertEqual((d.relative_path, d.position_encoding), ("src/lib.rs", 1))
        (o,) = d.occurrences
        self.assertEqual((o.start_line, o.start_char, o.end_line, o.end_char), (3, 4, 3, 9))
        self.assertTrue(o.roles & scip.ROLE_DEFINITION)

    def test_package_and_short_form(self):
        sym = "rust-analyzer cargo core https://git.example/core fmt/Debug#fmt()."
        self.assertEqual(scip.package(sym), "core")
        self.assertEqual(scip.short(sym), "core fmt/Debug#fmt().")
        self.assertIsNone(scip.package("local 4"))
        self.assertEqual(scip.package("scheme manager my  pkg 1.0 x/"), "my pkg")

    def test_results_elide_string_literals_in_callees(self):
        import report
        self.assertEqual(report.callee_text('client\n    .get("a://b")\n    .send'),
                         'client .get("") .send')
        self.assertEqual(report.callee_text("f('x').g"), "f('x').g")
        self.assertEqual(report.callee_text("a // see a://b\n  .b /* c */ .c"), "a .b .c")


class NameTokens(unittest.TestCase):
    def test_the_last_name_outside_generic_arguments(self):
        cases = {
            ("self.app.route", "python"): "route",
            ("Vec::<u8>::new", "rust"): "new",
            ("foo::<T>", "rust"): "foo",
            ("WithRejection::<A, B>::from_request", "rust"): "from_request",
            ("x.go::<Vec<u8>>", "rust"): "go",
            ("super().__init__", "python"): "__init__",
        }
        for (text, lang), name in cases.items():
            s, e = syntax.callee_name_token(text.encode(), 100, lang)
            self.assertEqual(text.encode()[s - 100:e - 100].decode(), name, text)

    def test_rust_comments_and_literals_are_no_generic_syntax(self):
        """Issue 120 (a0-match-2): a comment's `<` opened generic arguments, so M1 took
        the name before it (`.layer` after `// <- Inner layer`). Comments, nested block
        comments and literals are skipped; a `//` in a string is no comment; generic
        arguments still hide their names."""
        cases = {
            "Router::new()\n    .route(\"/\", get(h))\n    // <- Inner layer\n    .layer": "layer",
            "app\n    /* <- Inner layer */\n    .with_state": "with_state",
            "app /* a /* <nested> */ < */ .with_state": "with_state",
            "app // trailing note\n    .layer::<Vec<u8>>": "layer",
            "client.get(\"a://b/<c>\").send": "send",
            "client.get(r#\"<\"// \"#).send": "send",
            "x.f('<').g": "g",
            "Foo::<'a, T>::new": "new",
            "x.go::<Vec<u8>>": "go",
        }
        for text, name in cases.items():
            s, e = syntax.callee_name_token(text.encode(), 100, "rust")
            self.assertEqual(text.encode()[s - 100:e - 100].decode(), name, text)

    def test_matching_and_metrics_versions(self):
        """Matching is a0-match-2 since issue 120; the metrics' definitions did not
        change, so they stay a0-metrics-2. Both are every result's provenance."""
        self.assertEqual(audit.ALGORITHM_VERSION, "a0-match-2")
        self.assertEqual(score.METRICS_VERSION, "a0-metrics-2")

    def test_python_calls_are_named_by_their_last_name(self):
        src = b"import json\n\ndef f(x):\n    json.dumps(x)\n    g()(1)\n"
        calls = syntax.python_calls(src)
        names = sorted(src[c.token[0]:c.token[1]].decode() for c in calls if c.token)
        self.assertEqual(names, ["dumps", "g"])
        self.assertEqual(sum(1 for c in calls if c.shape == "other"), 1)


DUMP = """SOURCE_FILE@0..40
  FN@0..40
    BLOCK_EXPR@8..40
      CALL_EXPR@10..18
        PATH_EXPR@10..16
          PATH@10..16
            PATH@10..11
              PATH_SEGMENT@10..11
                NAME_REF@10..11
                  IDENT@10..11 "A"
            COLON2@11..13 "::"
            PATH_SEGMENT@13..16
              NAME_REF@13..16
                IDENT@13..16 "new"
        ARG_LIST@16..18
      MACRO_CALL@20..38
        PATH@20..26
          PATH_SEGMENT@20..26
            NAME_REF@20..26
              IDENT@20..26 "assert"
        BANG@26..27 "!"
        TOKEN_TREE@27..38
          L_PAREN@27..28 "("
          IDENT@28..29 "B"
          COLON2@29..31 "::"
          IDENT@31..34 "run"
          TOKEN_TREE@34..36
            L_PAREN@34..35 "("
            R_PAREN@35..36 ")"
          R_PAREN@37..38 ")"
"""


class RustTree(unittest.TestCase):
    def test_calls_macro_invocations_and_token_tree_calls(self):
        data = b"fn f() { A::new() ; assert!(B::run() )}"
        calls, invocations = syntax.rust_calls(data, lambda _data: DUMP)
        by_shape = {c.shape: c for c in calls}
        self.assertEqual(by_shape["call"].token, (13, 16))
        self.assertEqual(by_shape["macro_tt"].token, (31, 34))
        # A token tree's call starts where its path does.
        self.assertEqual(by_shape["macro_tt"].node, (28, 36))
        self.assertEqual([(m.name, m.token) for m in invocations], [("assert", (20, 26))])


def site(**kw):
    base = {"status": "resolved", "band": "exact", "target": ["a.py", 1], "verdict": "correct",
            "is_reference": False, "file": "a.py", "site": 0}
    base.update(kw)
    return base


class Measures(unittest.TestCase):
    def test_oracle_metrics_formula(self):
        """4.13.1 on a synthetic case: precision over verifiable drawn edges, recall over
        relations at parsed sites, the oracle-verifiable fraction over drawn edges."""
        sites = [
            site(site=0, band="typed", verdict="correct"),
            site(site=1, band="exact", verdict="false_positive"),
            site(site=2, band="exact", verdict="unverifiable"),
            site(site=3, band="scoped", verdict="correct"),
            site(site=4, band="candidate", target=None, verdict=None),
            site(site=5, band="blocked", target=None, verdict=None,
                 typed_target=["a.py", 2], typed_target_verdict="correct"),
        ]
        rel = lambda i, shape="call": {"file": "a.py", "shape": shape,  # noqa: E731
                                       "oracle_status": "mapped", "oracle_target_callable": True,
                                       "pdx_sites": [{"site": i, "status": "resolved"}]}
        rels = [rel(0), rel(1), rel(3), rel(4), rel(5),
                {"file": "a.py", "shape": "macro_tt", "oracle_status": "mapped",
                 "oracle_target_callable": True, "pdx_sites": []}]
        table = score.precision_table(sites)
        # The formula, over every band that resolved to one definition, weak evidence
        # included: the union as it was before issue 102.
        p = table["union_with_evidence"]
        self.assertEqual((p["drawn"], p["oracle_verifiable"], p["correct"]), (4, 3, 2))
        self.assertEqual(p["precision"]["value"], round(2 / 3, 6))
        self.assertEqual(p["oracle_verifiable_fraction"]["value"], 0.75)
        # Drawn edges only since issue 102: `typed`. Weak evidence is measured per band.
        p = table["union"]
        self.assertEqual((p["drawn"], p["oracle_verifiable"], p["correct"]), (1, 1, 1))
        self.assertEqual(p["oracle_verifiable_fraction"]["value"], 1.0)
        self.assertEqual((table["exact"]["drawn"], table["exact"]["false_positive"]), (2, 1))
        self.assertEqual((table["scoped"]["drawn"], table["scoped"]["correct"]), (1, 1))
        r = score.recall_table(sites, rels)
        self.assertEqual((r["recall"]["numerator"], r["recall"]["denominator"]), (1, 5))
        self.assertEqual(r["breakdown"]["resolution_not_drawn:exact"], 1)
        self.assertEqual(r["breakdown"]["resolution_not_drawn:scoped"], 1)
        self.assertEqual(r["weak_evidence"], {"exact": 1, "exact:false_positive": 1,
                                              "scoped": 1, "scoped:correct": 1})
        self.assertEqual(r["breakdown"]["site_missing_macro_token_tree"], 1)
        self.assertEqual(r["end_to_end_recall"]["denominator"], 6)
        c = score.counterfactuals(sites, rels)
        # C7, the policy before issue 102, draws weak evidence again.
        self.assertEqual(c["C7"]["recall"]["numerator"], 2)
        self.assertEqual(c["C7"]["true_positives_added"], 1)
        self.assertEqual(c["C7"]["false_positives_added"], 1)
        self.assertEqual(c["C7"]["drawn"], 4)
        for same in ("C1", "C2", "C3", "C5"):
            self.assertEqual(c[same]["false_positives_removed"], 0, same)
            self.assertEqual(c[same]["recall"], c["C0"]["recall"], same)
        self.assertEqual(c["C4"]["true_positives_added"], 1)
        self.assertEqual(c["C4"]["recall"]["numerator"], 2)

    def test_a_site_stage_4_cannot_persist_draws_nothing(self):
        row = site(band="typed", materialized=False)
        self.assertEqual(score.edge_under("C0", row), (None, None, None))
        rel = {"file": "a.py", "shape": "macro_tt", "oracle_status": "mapped",
               "oracle_target_callable": True,
               "pdx_sites": [{"site": 0, "status": "resolved"}]}
        r = score.recall_table([row], [rel])
        self.assertEqual(r["breakdown"]["resolution_not_materialized:typed"], 1)
        self.assertEqual(score.precision_table([row])["union"]["drawn"], 0)

    def test_weak_evidence_draws_only_under_c7(self):
        # Issue 102: `exact` and `scoped` draw nothing, whatever the receiver; C7 is the
        # policy before it, which drew them.
        rows = [site(band="exact", receiver_proof="value_unproven"),
                site(band="scoped", receiver_proof="path_bound"),
                site(band="typed", receiver_proof="value_unproven")]
        for policy in ("C0", "C1", "C2", "C3", "C4", "C5", "C6"):
            self.assertEqual([score.edge_under(policy, r)[0] for r in rows],
                             [None, None, "typed"], policy)
        self.assertEqual([score.edge_under("C7", r)[0] for r in rows],
                         ["exact", "scoped", "typed"])
        # Measured all the same, on the candidate each keeps.
        self.assertEqual([score.evidence_under(r)[0] for r in rows], ["exact", "scoped", None])


class SiteRows(unittest.TestCase):
    def test_a_site_row_keeps_the_datasets_one_based_line(self):
        """The dataset's line is the interface's span line, 1-based: a site on the
        third line of its file is on line 3, not 4."""
        import tempfile
        with tempfile.TemporaryDirectory() as d:
            src = b"a = 1\nb = 2\nrun_it()\n"
            Path(d, "m.py").write_bytes(src)
            start = src.index(b"run_it")
            site = {"path": "m.py", "i": 0, "status": "resolved", "band": "unresolved",
                    "span": [start, start + 8], "line": 3, "callee": "run_it",
                    "callee_span": [start, start + 6], "name": "run_it", "caller": None,
                    "engine": None, "target": None}
            files = {"m.py": {"path": "m.py", "language": "python", "outcome": "extracted",
                              "source_len": len(src)}}
            ds = audit.Dataset({}, files, {}, {}, [site], None)
            row = audit._site_row("c", "python", ds, audit.Oracle(), {}, audit.Sources(Path(d)),
                                  site, (start, start + 6), set(), {})
            self.assertEqual(row["line"], 3)

    def test_weak_evidence_is_measured_on_its_one_candidate(self):
        """Issue 102: an `exact` or `scoped` site has no target and keeps one
        candidate; the row measures that candidate and is not drawn."""
        import tempfile
        with tempfile.TemporaryDirectory() as d:
            src = b"def run_it():\n    pass\n\nrun_it()\n"
            Path(d, "m.py").write_bytes(src)
            start = src.rindex(b"run_it")
            defs = {("m.py", 1): {"path": "m.py", "i": 1, "qn": "m.run_it", "name": "run_it",
                                  "kind": "Function"}}
            files = {"m.py": {"path": "m.py", "language": "python", "outcome": "extracted",
                              "source_len": len(src)}}
            for band, drawn in (("exact", False), ("scoped", False)):
                site = {"path": "m.py", "i": 0, "status": "resolved", "band": band,
                        "span": [start, start + 8], "line": 4, "callee": "run_it",
                        "callee_span": [start, start + 6], "name": "run_it", "caller": None,
                        "engine": None, "target": None, "candidates": [["m.py", 1]]}
                ds = audit.Dataset({}, files, defs, {}, [site], None)
                row = audit._site_row("c", "python", ds, audit.Oracle(), {},
                                      audit.Sources(Path(d)), site, (start, start + 6),
                                      set(), {})
                self.assertEqual(row["target"], ["m.py", 1], band)
                self.assertEqual(row["drawn"], drawn, band)
            site["band"], site["target"], site["candidates"] = "typed", ["m.py", 1], []
            row = audit._site_row("c", "python", ds, audit.Oracle(), {},
                                  audit.Sources(Path(d)), site, (start, start + 6), set(), {})
            self.assertEqual((row["target"], row["drawn"]), (["m.py", 1], True))


class CommittedResults(unittest.TestCase):
    def test_r3_acceptance_sites_are_weak_evidence_not_drawn(self):
        """Issue 102 (R3): the committed results hold the issue-109 site and issue 118's
        four as wrong weak evidence, and no corpus has a drawn false positive."""
        path = Path(__file__).resolve().parents[2] / "bench/results/a0/mismatches.jsonl"
        rows = [json.loads(line) for line in path.read_text().splitlines()]
        self.assertFalse([r for r in rows if r["kind"] == "false_positive"])
        cases = [
            ("rust-golden", "axum-extra/src/extract/with_rejection.rs", 196, "scoped"),
            ("pdx-self", "crates/pdx-bench/src/bench/run.rs", 918, "exact"),
            ("pdx-self", "crates/pdx-bench/src/bench/run.rs", 929, "exact"),
            ("pdx-self", "crates/pdx-bench/tests/bench_run.rs", 213, "exact"),
            ("pdx-self", "crates/pdx-bench/tests/bench_run.rs", 227, "exact"),
        ]
        for corpus, file, line, band in cases:
            found = [r for r in rows if (r["corpus"], r["file"], r["line"]) == (corpus, file, line)
                     and r["kind"] != "missing"]
            self.assertEqual([(r["kind"], r["band"]) for r in found],
                             [("wrong_weak_evidence", band)], (corpus, file, line))


class Issue107ASites(unittest.TestCase):
    """The Rust golden's 56 relations with no structural site after R3
    (`bench/a0/issue-107a-sites.json`): 30 path and 24 method turbofish calls the engine
    did not extract (issue 107A), and 2 calls with a comment before their dot whose sites
    exist, missed by M1 (issue 120)."""

    ROOT = Path(__file__).resolve().parents[2]

    def evidence(self):
        return json.loads((self.ROOT / "bench/a0/issue-107a-sites.json").read_text())

    def test_the_classes(self):
        doc = self.evidence()
        from collections import Counter
        counts = Counter(r["class"] for r in doc["relations"])
        self.assertEqual(counts, {"path_turbofish": 30, "method_turbofish": 24,
                                  "comment_before_dot": 2})
        self.assertEqual({c: v["count"] for c, v in doc["classes"].items()}, dict(counts))
        keys = [(r["file"], tuple(r["token"])) for r in doc["relations"]]
        self.assertEqual(len(set(keys)), 56)

    def test_against_the_committed_results(self):
        """Measured before R4 (engine 12, a0-match-1), the 56 are exactly the Rust
        golden's `site_missing` relations; measured after it (engine 13 or later,
        a0-match-2), none of them is missing a site."""
        results = json.loads((self.ROOT / "bench/results/a0/rust.json").read_text())
        provenance = results["provenance"]
        rows = [json.loads(line) for line in
                (self.ROOT / "bench/results/a0/mismatches.jsonl").read_text().splitlines()]
        missing = {(r["file"], tuple(r["token"])): r for r in rows
                   if r["corpus"] == "rust-golden" and r.get("bucket", "").startswith("site_")}
        evidence = {(r["file"], tuple(r["token"])): r for r in self.evidence()["relations"]}
        if provenance["engine_version"] < 13:
            self.assertEqual(provenance["site_matching_version"], "a0-match-1")
            site_missing = {k for k, r in missing.items() if r["bucket"] == "site_missing"}
            self.assertEqual(site_missing, set(evidence))
            for key, r in evidence.items():
                self.assertEqual((missing[key]["line"], missing[key]["oracle_target"],
                                  missing[key]["shape"]),
                                 (r["line"], r["oracle_target"], r["shape"]), key)
        else:
            self.assertEqual(provenance["site_matching_version"], "a0-match-2")
            self.assertEqual(set(evidence) & set(missing), set())


class RustCandidateClassification(unittest.TestCase):
    """Issue 105 (R5): the Rust golden's relations at a structural site with no drawn
    edge, classified (`scripts/a0/rust_candidates.py`), before and after R5
    (`bench/results/a0/rust-candidate-classification.json` and
    `rust-candidate-relations.jsonl`). The summaries are their rows': nothing in them is
    written by hand."""

    ROOT = Path(__file__).resolve().parents[2] / "bench/results/a0"

    def load(self):
        doc = json.loads((self.ROOT / "rust-candidate-classification.json").read_text())
        rows = {}
        for line in (self.ROOT / "rust-candidate-relations.jsonl").read_text().splitlines():
            r = json.loads(line)
            rows.setdefault(r.pop("state"), []).append(r)
        return doc, rows

    def test_each_summary_is_its_rows(self):
        doc, rows = self.load()
        self.assertEqual(doc["classes"], list(rust_candidates.CLASSES))
        self.assertEqual(set(doc["states"]), set(rows))
        for label, state in doc["states"].items():
            self.assertEqual(rust_candidates.summarise(rows[label]), state["summary"], label)
            undrawn = {k: v for k, v in state["population"].items()
                       if k not in ("no_site", "site_ambiguous") and not k.startswith("drawn_")}
            self.assertEqual(sum(undrawn.values()), len(rows[label]), label)

    def test_a_relations_primary_class_is_its_first(self):
        _, rows = self.load()
        order = {c: i for i, c in enumerate(rust_candidates.CLASSES)}
        for label, state_rows in rows.items():
            keys = [(r["file"], tuple(r["token"])) for r in state_rows]
            self.assertEqual(len(set(keys)), len(keys), label)
            for r in state_rows:
                self.assertTrue(r["classes"], r)
                self.assertEqual(r["primary"], min(r["classes"], key=order.__getitem__), r)

    def test_r5_draws_nothing_wrong(self):
        doc, _ = self.load()
        for label, state in doc["states"].items():
            self.assertNotIn("drawn_wrong", state["population"], label)


class BlocklistAccounting(unittest.TestCase):
    def test_only_a_blocked_site_counts_as_suppressed(self):
        """A blocklisted name Stage 3 draws as typed (issue 103) is an accepted typed
        answer, not a suppressed one."""
        import report
        base = {"status": "resolved", "blocklisted": True, "file": "a.py", "site": 0,
                "oracle_status": "mapped", "oracle_target": ["a.py", 1], "is_reference": False,
                "typed_target_verdict": "correct", "materialized": True}
        rows = [dict(base, site=0, band="blocked", typed_target=["a.py", 1]),
                dict(base, site=1, band="typed", typed_target=["a.py", 1], target=["a.py", 1],
                     verdict="correct"),
                dict(base, site=2, band="blocked", typed_target=None)]
        defs = {("a.py", 1): {"kind": "Function", "name": "get", "qn": "a.get", "path": "a.py",
                              "i": 1}}
        meta = {"language": "python", "oracle_tools": [], "dataset_format": 1,
                "engine_health": "", "language_files": 1, "language_files_indexed": 1,
                "symbols": {}, "oracle_unconvertible_occurrences": 0,
                "oracle_assumed_encoding": {}, "parse_failures": [], "macro_invocations": []}

        class NoSources:
            def get(self, _path):
                return None
        out, _ = report.build("c", rows, [], meta, defs, {}, NoSources())
        bl = out["blocklist"]
        self.assertEqual(bl["sites"], 3)
        self.assertEqual(bl["typed_answer_suppressed_by_blocklist"], 1)
        self.assertEqual(bl["typed_answer_accepted_before_blocklist"], 1)
        self.assertEqual(bl["no_typed_answer_and_blocked"], 1)
        self.assertEqual(bl["final_band:blocked"], 2)
        self.assertEqual(bl["edges_drawn"], 1)


class Verdicts(unittest.TestCase):
    def setUp(self):
        defs = {
            ("m.py", 0): {"path": "m.py", "i": 0, "name": "C", "kind": "Class", "qn": "m.C",
                          "declaring_type": None},
            ("m.py", 1): {"path": "m.py", "i": 1, "name": "__init__", "kind": "Method",
                          "qn": "m.C.__init__", "declaring_type": 0},
        }
        self.ds = audit.Dataset({}, {}, defs, {}, [], None)

    def test_a_class_and_its_constructor_are_one_target(self):
        row = {"oracle_status": "mapped", "oracle_target": ["m.py", 0]}
        self.assertEqual(audit.site_verdict("python", self.ds, row, ["m.py", 1], [])[0],
                         "correct")
        self.assertEqual(audit.site_verdict("rust", self.ds, row, ["m.py", 1], [])[0],
                         "false_positive")

    def test_unverifiable_unless_the_target_is_provably_another_symbol(self):
        row = {"oracle_status": "unmapped", "oracle_symbol": "s1"}
        self.assertEqual(audit.site_verdict("rust", self.ds, row, ["m.py", 0], [])[0],
                         "unverifiable")
        self.assertEqual(audit.site_verdict("rust", self.ds, row, ["m.py", 0], ["s2"]),
                         ("false_positive", "other_target_unmapped"))
        self.assertEqual(audit.site_verdict("rust", self.ds, row, ["m.py", 0], ["s1"])[0],
                         "unverifiable")
        for status in ("none", "unresolved_local", "unresolved_import", "multiple"):
            self.assertEqual(audit.site_verdict("rust", self.ds, {"oracle_status": status},
                                                ["m.py", 0], ["s2"])[0], "unverifiable")

    def test_receiver_proof(self):
        def proof(callee, receiver, name, head=None, prov=None):
            return audit.receiver_proof({"callee": callee, "receiver": receiver, "name": name,
                                         "head": head, "head_provenance": prov}, b"")
        self.assertEqual(proof("f", None, "f"), "none")
        self.assertEqual(proof("self.f", "self", "f"), "self")
        self.assertEqual(proof("self.a.f", "self.a", "f"), "value_unproven")
        self.assertEqual(proof("crate::m::f", "crate::m", "f", "crate"), "path_bound")
        self.assertEqual(proof("T::f", "T", "f", "T", "defined"), "path_bound")
        self.assertEqual(proof("T::f", "T", "f", "T", "unknown"), "path_unproven")

    def test_false_positive_categories_put_self_edges_first(self):
        row = {"target": ["m.py", 1], "caller": ["m.py", 1], "band": "scoped",
               "oracle_status": "external", "receiver": "self", "language": "python"}
        tags = score.fp_tags(row, self.ds.defs)
        self.assertEqual(tags[0], "self_edge")
        self.assertIn("module_or_package_uniqueness_only", tags)


class ExecutableGuard(unittest.TestCase):
    """Issue 115: `run.py` measures with the pdx-bench it runs only once that executable
    has reported the source's versions, and records the executed engine's version."""

    # Every version the source declares that the executable reports, and where.
    DECLARED = {
        "ENGINE_VERSION": "crates/pdx-core/src/consts.rs",
        "EXTRACT_CACHE_FORMAT_VERSION": "crates/pdx-core/src/consts.rs",
        "LANGUAGE_MATRIX_VERSION": "crates/pdx-core/src/consts.rs",
        "SECRET_DETECTOR_VERSION": "crates/pdx-core/src/consts.rs",
        "SEGMENT_SCHEMA_VERSION": "crates/pdx-core/src/consts.rs",
        "PROTOCOL": "crates/pdx-engine/src/isolate.rs",
    }

    def setUp(self):
        import contextlib
        import io
        import tempfile
        import run
        self.run = run
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(__import__("shutil").rmtree, self.tmp)
        # A checkout: the real files declaring the versions, at a root of the test's own.
        self.root = self.tmp / "checkout"
        self.declared = {}
        for name, rel in self.DECLARED.items():
            (self.root / rel).parent.mkdir(parents=True, exist_ok=True)
            text = (run.ROOT / rel).read_text()
            (self.root / rel).write_text(text)
            prefix = f"pub const {name}: u32 = "
            (line,) = [x for x in text.splitlines() if x.startswith(prefix)]
            self.declared[name] = int(line.removeprefix(prefix).rstrip(";"))
        self.bench = self.root / run.EXECUTABLE
        self.calls = self.tmp / "calls.log"
        self.report = self.tmp / "report.json"
        self.executable(self.built_from_source())
        # Oracles that record being run and fail: the first thing run after the guard.
        self.bin = self.tmp / "bin"
        self.bin.mkdir()
        for tool in ("rust-analyzer", "scip-python"):
            self.script(self.bin / tool, f'echo "{tool} $@" >> "{self.calls}"\nexit 1\n')
        self.out = self.tmp / "results"
        self.out.mkdir()
        (self.out / "rust.json").write_text("the results before this run\n")
        self.quiet = contextlib.redirect_stdout(io.StringIO())

    def script(self, path: Path, body: str) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("#!/bin/sh\n" + body)
        path.chmod(0o755)

    def built_from_source(self, **stale) -> dict:
        """The report of an executable built from the checkout's source, or with the
        versions given in place of the source's (an executable not rebuilt since)."""
        v = {**self.declared, **stale}
        return {"format": 1, "engine_version": str(v["ENGINE_VERSION"]),
                "pdx_core": {k: v[k] for k in (
                    "ENGINE_VERSION", "EXTRACT_CACHE_FORMAT_VERSION",
                    "LANGUAGE_MATRIX_VERSION", "SECRET_DETECTOR_VERSION",
                    "SEGMENT_SCHEMA_VERSION")},
                "worker_protocol": v["PROTOCOL"]}

    def executable(self, report: dict | None, exit_code: int = 0) -> None:
        if report is not None:
            self.report.write_text(json.dumps(report))
        self.script(self.bench,
                    f'echo "pdx-bench $@" >> "{self.calls}"\n'
                    f'if [ "$1" = versions ]; then cat "{self.report}"; exit {exit_code}; fi\n'
                    'exit 3\n')

    def bump(self, name: str) -> None:
        """A source change of `name` with no rebuild."""
        path = self.root / self.DECLARED[name]
        line = f"pub const {name}: u32 = {self.declared[name]};"
        text = path.read_text()
        self.assertEqual(text.count(line), 1)
        path.write_text(text.replace(line, f"pub const {name}: u32 = {self.declared[name] + 1};"))

    def main(self):
        """`run.py` over the test's checkout, with every tool the test's own."""
        argv = ["--work", str(self.tmp / "work"), "--scip-python", str(self.bin / "scip-python"),
                "--rust-analyzer", str(self.bin / "rust-analyzer"),
                "--rust-src", str(self.tmp / "rust-src"), "--toolchain", str(self.tmp / "tc"),
                "--self-sha", "0" * 40, "--resolver", "test", "--out", str(self.out)]
        saved = self.run.ROOT
        self.run.ROOT = self.root
        try:
            with self.quiet:
                return self.run.main(argv)
        finally:
            self.run.ROOT = saved

    def ran(self) -> list[str]:
        return self.calls.read_text().splitlines() if self.calls.exists() else []

    def refused(self) -> str:
        with self.assertRaises(SystemExit) as e:
            self.main()
        return str(e.exception.code)

    def assert_nothing_measured_or_written(self):
        self.assertEqual(self.ran(), ["pdx-bench versions"])
        self.assertFalse((self.tmp / "work").exists())
        self.assertEqual(sorted(p.name for p in self.out.iterdir()), ["rust.json"])
        self.assertEqual((self.out / "rust.json").read_text(), "the results before this run\n")

    def test_a1_an_executable_built_from_the_source_proceeds(self):
        with self.quiet:
            executable = self.run.Executable(self.bench, self.root)
        self.assertEqual(executable.versions, self.built_from_source())
        # Past the guard, run.py goes on to the oracles' versions, which fail here.
        with self.assertRaises(subprocess.CalledProcessError):
            self.main()
        self.assertEqual(self.ran()[-2:], ["pdx-bench versions", "rust-analyzer --version"])

    def test_a2_a_stale_executable_is_refused_before_any_corpus(self):
        e = self.declared["ENGINE_VERSION"]
        self.executable(self.built_from_source(ENGINE_VERSION=e - 1))
        message = self.refused()
        self.assertEqual(message, (
            f"A0 refused: source ENGINE_VERSION={e} but pdx-bench reports engine_version={e - 1}; "
            f"source ENGINE_VERSION={e} but pdx-bench reports pdx_core.ENGINE_VERSION={e - 1}. "
            "Rebuild the executable (cargo build -p pdx-bench) before running the audit."))
        self.assert_nothing_measured_or_written()

    def test_a3_a_refusal_writes_and_promotes_nothing(self):
        for report, code in ((self.built_from_source(SEGMENT_SCHEMA_VERSION=0), 0),
                             (None, 1), ({"format": 2}, 0)):
            with self.subTest(report=report, code=code):
                self.calls.unlink(missing_ok=True)
                if report is None:
                    self.report.write_text("")
                self.executable(report, code)
                self.assertTrue(self.refused().startswith("A0 refused: "))
                self.assert_nothing_measured_or_written()
        self.bench.unlink()
        self.calls.unlink(missing_ok=True)
        self.assertIn("does not exist", self.refused())
        self.assertEqual(self.ran(), [])
        self.assertFalse((self.tmp / "work").exists())

    def test_a4_the_provenance_is_the_executed_engines(self):
        with self.quiet:
            executable = self.run.Executable(self.bench, self.root)
        version = self.declared["ENGINE_VERSION"]
        saved = self.run.ROOT
        self.run.ROOT = self.root
        try:
            provenance = self.run.common_provenance("r", executable, {"oracle": "v"})
            self.assertEqual(provenance, {"resolver": "r", "engine_version": version,
                                          "oracle_versions": {"oracle": "v"}})
            # Read from what the executable reported, never the source again.
            self.bump("ENGINE_VERSION")
            self.report.write_text(json.dumps(self.built_from_source(ENGINE_VERSION=version + 7)))
            provenance = self.run.common_provenance("r", executable, {})
            self.assertEqual(provenance["engine_version"], version)
        finally:
            self.run.ROOT = saved
        # And an executable rebuilt after its check measures nothing more.
        self.script(self.bench, self.bench.read_text().removeprefix("#!/bin/sh\n") + "# rebuilt\n")
        tools = type("Tools", (), {"executable": executable, "pdx_bench": self.bench})()
        self.calls.unlink(missing_ok=True)
        with self.assertRaises(SystemExit) as e:
            self.run.dataset(tools, self.root, self.tmp / "sites.jsonl")
        self.assertIn("changed after its versions were checked", str(e.exception.code))
        self.assertEqual(self.ran(), [])

    def test_a5_a_source_bump_without_a_rebuild_is_caught(self):
        fields = {name: [f"pdx_core.{name}"] for name in self.DECLARED}
        fields["ENGINE_VERSION"].insert(0, "engine_version")
        fields["PROTOCOL"] = ["worker_protocol"]
        for name, reported in fields.items():
            with self.subTest(name=name):
                self.setUp()
                self.bump(name)
                message = self.refused()
                v = self.declared[name]
                self.assertEqual(message, "A0 refused: " + "; ".join(
                    f"source {name}={v + 1} but pdx-bench reports {f}={v}" for f in reported)
                    + ". Rebuild the executable (cargo build -p pdx-bench) before running "
                    "the audit.")
                self.assert_nothing_measured_or_written()

    def test_the_checkouts_own_versions_are_read(self):
        self.assertEqual({(name, rel) for name, rel, _ in self.run.VERSIONS},
                         set(self.DECLARED.items()))
        for name, rel in self.DECLARED.items():
            self.assertEqual(self.run.source_version(self.run.ROOT, rel, name),
                             self.declared[name])

    def test_results_are_written_only_with_the_executable_checked_and_together(self):
        with self.quiet:
            executable = self.run.Executable(self.bench, self.root)
        work = self.tmp / "work"
        work.mkdir()
        tools = type("Tools", (), {"executable": executable, "work": work,
                                   "pdx_bench": self.bench})()
        results = {name: (name, self.tmp / "sites", self.tmp, {})
                   for name in ("python", "rust", "pdx-self")}
        reports = []
        fail = [0]
        rebuild = [0]

        def rebuilt():
            self.script(self.bench,
                        self.bench.read_text().removeprefix("#!/bin/sh\n") + "# rebuilt\n")

        def report(cmd, **_):
            cmd = [str(c) for c in cmd]
            reports.append(cmd)
            if len(reports) == fail[0]:
                raise subprocess.CalledProcessError(1, cmd)
            if len(reports) == rebuild[0]:
                rebuilt()
            Path(cmd[cmd.index("--out") + 1]).write_text(json.dumps({"counterfactuals": {}}))

        saved = self.run.run
        self.run.run = report
        try:
            # A report that fails part way promotes nothing.
            fail[0] = 2
            with self.assertRaises(subprocess.CalledProcessError):
                self.run.write_results(tools, results, self.out)
            self.assertEqual(sorted(p.name for p in self.out.iterdir()), ["rust.json"])
            self.assertEqual((self.out / "rust.json").read_text(),
                             "the results before this run\n")
            # Every report written: all promoted.
            fail[0], reports[:] = 0, []
            self.run.write_results(tools, results, self.out)
            self.assertEqual(len(reports), 3)
            self.assertEqual(sorted(p.name for p in self.out.iterdir()), [
                "counterfactuals.json", "mismatches.jsonl", "pdx-self.json", "python.json",
                "rust.json"])
            self.assertEqual(json.loads((self.out / "rust.json").read_text()),
                             {"counterfactuals": {}})
            # An executable rebuilt while the reports are written: nothing is promoted.
            before = {p.name: p.read_bytes() for p in self.out.iterdir()}
            rebuild[0], reports[:] = 2, []
            with self.assertRaises(SystemExit):
                self.run.write_results(tools, results, self.out)
            self.assertEqual(len(reports), 3)
            self.assertEqual({p.name: p.read_bytes() for p in self.out.iterdir()}, before)
            # Rebuilt before: no report runs.
            rebuilt()
            reports[:] = []
            with self.assertRaises(SystemExit):
                self.run.write_results(tools, results, self.out)
            self.assertEqual(reports, [])
            self.assertEqual({p.name: p.read_bytes() for p in self.out.iterdir()}, before)
        finally:
            self.run.run = saved

    def test_results_are_promoted_together(self):
        staged = self.tmp / "staged"
        staged.mkdir()
        for name in ("rust.json", "python.json"):
            (staged / name).write_text(f"new {name}\n")
        # A copy that fails replaces nothing.
        copy = self.run.shutil.copyfile
        copied = []

        def failing(src, dst):
            copied.append(src)
            if len(copied) == 2:
                raise OSError("no space")
            return copy(src, dst)

        self.run.shutil.copyfile = failing
        try:
            with self.assertRaises(OSError):
                self.run.promote(staged, self.out)
        finally:
            self.run.shutil.copyfile = copy
        self.assertEqual(sorted(p.name for p in self.out.iterdir()), ["rust.json"])
        self.assertEqual((self.out / "rust.json").read_text(), "the results before this run\n")
        self.run.promote(staged, self.out)
        self.assertEqual(sorted(p.name for p in self.out.iterdir()),
                         ["python.json", "rust.json"])
        self.assertEqual((self.out / "rust.json").read_text(), "new rust.json\n")


if __name__ == "__main__":
    unittest.main(verbosity=1)
