#!/usr/bin/env python3
"""Tests of A0.1's audit tooling (decision 64) that need no indexer: the SCIP reader,
the name tokens, rust-analyzer's tree dump, the verdict rules and 4.13.1's formulas
on a synthetic case.

    python3 scripts/a0/test_a0.py
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import audit  # noqa: E402
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
        p = score.precision_table(sites)["union"]
        self.assertEqual((p["drawn"], p["oracle_verifiable"], p["correct"]), (4, 3, 2))
        self.assertEqual(p["precision"]["value"], round(2 / 3, 6))
        self.assertEqual(p["oracle_verifiable_fraction"]["value"], 0.75)
        r = score.recall_table(sites, rels)
        self.assertEqual((r["recall"]["numerator"], r["recall"]["denominator"]), (2, 5))
        self.assertEqual(r["breakdown"]["site_missing_macro_token_tree"], 1)
        self.assertEqual(r["end_to_end_recall"]["denominator"], 6)
        c = score.counterfactuals(sites, rels)
        self.assertEqual(c["C1"]["false_positives_removed"], 1)
        self.assertEqual(c["C3"]["true_positives_lost"], 1)
        self.assertEqual(c["C4"]["true_positives_added"], 1)
        self.assertEqual(c["C4"]["recall"]["numerator"], 3)

    def test_a_site_stage_4_cannot_persist_draws_nothing(self):
        row = site(band="typed", materialized=False)
        self.assertEqual(score.edge_under("C0", row), (None, None, None))
        rel = {"file": "a.py", "shape": "macro_tt", "oracle_status": "mapped",
               "oracle_target_callable": True,
               "pdx_sites": [{"site": 0, "status": "resolved"}]}
        r = score.recall_table([row], [rel])
        self.assertEqual(r["breakdown"]["resolution_not_materialized:typed"], 1)
        self.assertEqual(score.precision_table([row])["union"]["drawn"], 0)

    def test_c5_only_drops_unproven_receivers_from_exact_and_scoped(self):
        rows = [site(band="exact", receiver_proof="value_unproven"),
                site(band="scoped", receiver_proof="path_bound"),
                site(band="typed", receiver_proof="value_unproven")]
        self.assertEqual([score.edge_under("C5", r)[0] for r in rows], [None, "scoped", "typed"])


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


if __name__ == "__main__":
    unittest.main(verbosity=1)
