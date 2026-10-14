#!/usr/bin/env python3
"""Tests for scripts/engine/differential.py: network-free, stdlib only, and independent
of both engines. The rows here are project-authored; the reference checkout is a local
synthetic Git repository, never the real one."""

import argparse
import contextlib
import copy
import importlib.util
import io
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

HERE = pathlib.Path(__file__).resolve().parent
REPO = HERE.parents[1]


def load_module():
    spec = importlib.util.spec_from_file_location("differential", HERE / "differential.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


D = load_module()
KINDS = D.kind_map()
P = "tmp-reference-project"
V = "__pdx__"


def definition(project, label="Function", qn="pkg.m.f", start=1, file="pkg/m.py"):
    row = {k: None for k, t in D.SCHEMA["definition"].items() if t in (D.S, D.L)}
    row.update({k: 0 for k, t in D.SCHEMA["definition"].items() if t == D.I})
    row.update({k: False for k, t in D.SCHEMA["definition"].items() if t == D.B})
    row.update({"c": "definition", "file": file, "label": label, "name": qn.rsplit(".", 1)[-1],
                "qn": f"{project}.{qn}", "def_file": file, "start_line": start, "end_line": start + 2})
    return row


def call(project, callee="g", caller="pkg.m.f", start=10, end=13, line=2):
    return {"c": "call", "file": "pkg/m.py", "caller": f"{project}.{caller}", "callee": callee,
            "line": line, "start": start, "end": end, "origin": 0, "first_string_arg": None,
            "second_arg_name": None, "args": [], "arg_count": 0, "loop_depth": 0,
            "branch_depth": 0, "is_method": False, "requires_lsp_resolution": False,
            "callee_is_locally_bound": False, "receiver_is_self_attribute": False}


def file_row(project, file="pkg/m.py", **kw):
    row = {"c": "file", "file": file, "module_qn": f"{project}.pkg.m", "namespace_name": None,
           "has_error": False, "is_test_file": False, "lsp_skipped": False,
           "walk_truncated": False, "exports": None}
    row.update(kw)
    return row


def typed(project, callee="pkg.m.g", strategy="lsp_direct", confidence="0.949999988"):
    return {"c": "typed", "file": "pkg/m.py", "caller": f"{project}.pkg.m.f",
            "callee": f"{project}.{callee}", "strategy": strategy, "confidence": confidence,
            "reason": None, "kind": 0, "start": 10, "end": 13, "origin": 0}


def importing(local="g", module="pkg.m.g"):
    return {"c": "import", "file": "pkg/m.py", "local_name": local, "module_path": module}


def surface(project, entries=None, reg=None, file="pkg/m.py"):
    lsp = entries if entries is not None else [
        {"qn": f"{project}.pkg.m.f", "sn": "f", "lb": "Function", "dm": f"{project}.pkg.m"},
        {"qn": f"{project}.pkg.m.C.p", "sn": "p", "lb": "Property", "dm": f"{project}.pkg.m"}]
    return {"c": "surface", "file": file,
            "json": json.dumps({"v": 1, "lsp": lsp, "reg": reg or []}, separators=(",", ":"))}


def side(project):
    return [file_row(project), definition(project), definition(project, "Property", "pkg.m.C.p", 5),
            call(project), call(project, start=20, end=23, line=3), importing(), typed(project),
            surface(project)]


def canon(rows, project):
    return D.canonical(rows, project, KINDS)


def differs(ref_rows, ven_rows):
    return D.compare(canon(ref_rows, P), canon(ven_rows, V))


class KindMap(unittest.TestCase):
    def test_the_table_is_the_interface_s(self):
        table, default = D.kind_map()
        self.assertEqual(default, "variable")
        self.assertEqual(table, {
            "Class": "class", "Interface": "interface", "Enum": "enum", "Struct": "struct",
            "Trait": "trait", "Type": "type_alias", "TypeAlias": "type_alias",
            "Function": "function", "Method": "method", "Constructor": "constructor",
            "Field": "field", "Property": "field", "Variable": "variable",
            "Constant": "variable", "Macro": "macro", "Module": "module", "Object": "class"})

    def test_a_table_in_another_shape_is_refused(self):
        with self.assertRaises(D.Failure):
            D.kind_map("static const char *normalise_kind(const char *label) {\n}\n")

    def test_the_project_sentinel_is_the_interface_s(self):
        self.assertEqual(D.vendored_project(), V)


class Rows(unittest.TestCase):
    def lines(self, rows):
        return "\n".join(json.dumps(r) for r in rows)

    def test_well_formed_rows_parse(self):
        rows = D.parse_rows(self.lines(side(V)), "vendored")
        self.assertEqual(len(rows), 8)

    def test_malformed_unknown_or_ill_typed_input_fails(self):
        cases = [
            "not json",
            json.dumps({"no": "category"}),
            json.dumps({"c": "future_row", "file": "a"}),
            json.dumps({**call(V), "extra": 1}),
            json.dumps({k: v for k, v in call(V).items() if k != "callee"}),
            json.dumps({**call(V), "start": "10"}),
            json.dumps({**call(V), "is_method": 0}),
            json.dumps({**typed(V), "confidence": "nonfinite"}),
            json.dumps(typed(V)).replace('"0.949999988"', "NaN"),
            json.dumps({"c": "missing", "file": "pkg/m.py"}),
            "",
        ]
        for case in cases:
            with self.subTest(case=case[:60]), self.assertRaises(D.Failure):
                D.parse_rows(case + "\n" + self.lines([file_row(V)]), "vendored")

    def test_a_row_of_the_other_side_fails(self):
        with self.assertRaises(D.Failure):
            D.parse_rows(json.dumps({"c": "project", "name": "x", "files": 1}), "vendored")
        cached = {"c": "cached_typed", "file": "a", "callee": None, "strategy": None,
                  "reason": None, "kind": 0, "start": 0, "end": 0}
        with self.assertRaises(D.Failure):
            D.parse_rows(json.dumps(cached), "reference")

    def test_numbers_are_kept_as_written(self):
        rows = D.parse_rows(self.lines([typed(V)]).replace('"0.949999988"', "0.949999988"),
                            "vendored")
        self.assertEqual(rows[0]["confidence"], "0.949999988")


class Canonical(unittest.TestCase):
    def test_equal_facts_give_equal_bytes_whatever_the_order_and_project(self):
        a = D.canonical_bytes(canon(side(P), P))
        b = D.canonical_bytes(canon(list(reversed(side(V))), V))
        self.assertEqual(a, b)
        self.assertTrue(a.endswith(b"\n") and not a.endswith(b"\n\n"))
        a.decode("ascii")
        self.assertEqual(D.canonical_bytes([]), b"")

    def test_only_the_project_wrapper_is_removed(self):
        self.assertEqual(D.unwrap(P, P), "@")
        self.assertEqual(D.unwrap(f"{P}.a.b", P), "@.a.b")
        self.assertEqual(D.unwrap(f"x.{P}.a", P), f"x.{P}.a")
        self.assertEqual(D.unwrap(f"{P}x.a", P), f"{P}x.a")
        self.assertEqual(D.unwrap("builtins.len", P), "builtins.len")
        self.assertEqual(D.unwrap({"a": [f"{P}.q", 1, None]}, P), {"a": ["@.q", 1, None]})

    def test_both_sides_take_the_interface_s_kind(self):
        text = "\n".join(canon(side(P), P))
        self.assertIn('"kind":"field","label":"Property"', text)
        self.assertIn('"kind":"field"', text.split('"part":"lsp"')[0] + text)

    def test_an_empty_surface_is_not_a_missing_one(self):
        empty = side(P)[:-1] + [surface(P, entries=[])]
        missing = side(V)[:-1]
        self.assertIn("surfaces", differs(empty, missing))
        lines = canon(empty, P)
        self.assertIn('{"c":"surface_file","file":"pkg/m.py","lsp":0,"reg":0}', lines)

    def test_a_surface_of_another_codec_fails(self):
        bad = side(P)[:-1] + [{"c": "surface", "file": "pkg/m.py",
                               "json": json.dumps({"v": 2, "lsp": [], "reg": []})}]
        with self.assertRaises(D.Failure):
            canon(bad, P)

    def test_a_path_that_is_not_repository_relative_fails(self):
        for path in ("/abs/m.py", "C:/m.py", "a\\b.py", "../m.py", "a//b.py"):
            with self.subTest(path=path), self.assertRaises(D.Failure):
                canon([file_row(P, file=path)], P)

    def test_rows_of_other_files_are_not_compared(self):
        extra = side(V) + [definition(V, file="go.mod")]
        files = {"pkg/m.py"}
        self.assertEqual(D.canonical(side(P), P, KINDS, files),
                         D.canonical(extra, V, KINDS, files))


class Mutations(unittest.TestCase):
    """Each substantive change to one side's rows is a difference; reordering is not."""

    def mutate(self, change):
        ven = copy.deepcopy(side(V))
        change(ven)
        return differs(side(P), ven)

    def test_identical_sides_do_not_differ(self):
        self.assertEqual(differs(side(P), side(V)), {})

    def test_a_dropped_definition_differs(self):
        self.assertIn("definitions", self.mutate(lambda v: v.pop(1)))

    def test_a_kind_mapping_on_one_side_differs(self):
        ref = canon(side(P), P)
        drifted = ({**KINDS[0], "Property": "variable"}, KINDS[1])
        ven = D.canonical(side(V), V, drifted)
        self.assertIn("definitions", D.compare(ref, ven))
        self.assertIn("surfaces", D.compare(ref, ven))

    def test_a_changed_call_differs(self):
        for field, value in (("callee", "h"), ("caller", f"{V}.pkg.m.other"), ("start", 11),
                             ("end", 14), ("line", 9)):
            with self.subTest(field=field):
                self.assertIn("calls", self.mutate(lambda v, f=field, x=value: v[3].update({f: x})))

    def test_a_changed_import_differs(self):
        self.assertIn("imports", self.mutate(lambda v: v[5].update({"local_name": "alias"})))
        self.assertIn("imports", self.mutate(lambda v: v[5].update({"module_path": "pkg.n.g"})))

    def test_a_dropped_surface_entry_differs_with_definitions_unchanged(self):
        result = self.mutate(lambda v: v.__setitem__(7, surface(V, entries=[
            {"qn": f"{V}.pkg.m.f", "sn": "f", "lb": "Function", "dm": f"{V}.pkg.m"}])))
        self.assertEqual(list(result), ["surfaces"])

    def test_two_logical_surfaces_with_one_hash_still_differ(self):
        # Equal hashes are diagnostics, never a verdict: the rows decide.
        ven = copy.deepcopy(side(V))
        ven[7] = surface(V, reg=[{"qn": f"{V}.pkg.m.C.x", "sn": "x", "lb": "Field"}])
        same_hash = lambda *a, **k: True  # noqa: E731
        original = D.surface_bytes_equal
        try:
            D.surface_bytes_equal = same_hash
            self.assertIn("surfaces", differs(side(P), ven))
        finally:
            D.surface_bytes_equal = original

    def test_a_changed_typed_answer_differs(self):
        self.assertIn("typed", self.mutate(lambda v: v[6].update({"callee": f"{V}.pkg.m.h"})))
        self.assertIn("typed", self.mutate(lambda v: v[6].update({"strategy": "lsp_method"})))
        self.assertIn("typed", self.mutate(lambda v: v[6].update({"confidence": "0.899999976"})))
        self.assertIn("typed", self.mutate(lambda v: v[6].update({"kind": 1})))

    def test_reordered_rows_do_not_differ(self):
        self.assertEqual(self.mutate(lambda v: v.reverse()), {})

    def test_a_duplicated_row_differs(self):
        self.assertIn("calls", self.mutate(lambda v: v.append(copy.deepcopy(v[3]))))
        self.assertIn("definitions", self.mutate(lambda v: v.append(copy.deepcopy(v[1]))))

    def test_a_category_one_side_lacks_differs_and_an_empty_one_is_counted(self):
        self.assertIn("typed", self.mutate(lambda v: v.pop(6)))
        counts = D.counts(canon([file_row(P), definition(P)], P))
        self.assertEqual(counts["typed"], 0)
        self.assertEqual(counts["definitions"], 1)


class Reports(unittest.TestCase):
    def test_a_difference_is_described_within_bounds(self):
        ref = [json.dumps({"c": "call", "n": i, "pad": "x" * 1000}) for i in range(50)]
        lines = D.describe("corpus", D.compare(ref, []))
        self.assertEqual(lines[0], "corpus: calls: 50 row(s) only the reference has, "
                                   "0 only the vendored engine has")
        shown = [l for l in lines if l.startswith("  reference only: {")]
        self.assertEqual(len(shown), D.SHOW)
        self.assertTrue(all(len(l) <= len("  reference only: ") + D.LINE + 3 for l in shown))
        self.assertIn("  reference only: and 30 more", lines)


class RatifiedRows:
    """One ratified divergence, taken from compare's output, and the file holding it."""

    CORPUS = "golden:lang/repo"
    PIN = "0" * 40

    def setUp(self):
        # The difference the records ratify, taken from compare's own output as the
        # live one is: one typed answer whose callee the vendored engine spells otherwise.
        ven = copy.deepcopy(side(V))
        ven[6]["callee"] = f"{V}.pkg.m.h"
        self.differences = differs(side(P), ven)
        (self.ref_row,), (self.ven_row,) = self.differences["typed"]
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)

    def record(self, **kw):
        record = {"id": "issue-1-typed-callee", "issue": 1, "decision": 66,
                  "corpus": self.CORPUS, "category": "typed", "reference_row": self.ref_row,
                  "vendored_row": self.ven_row, "reason": "the reference is known wrong"}
        record.update(kw)
        return record

    def judge(self, differences, records):
        return D.ratify(self.CORPUS, differences, records)

    def manifest(self, records, pin=None, raw=None):
        path = pathlib.Path(self.tmp.name) / "divergences.json"
        document = {"format": 1, "reference_commit": pin or self.PIN, "divergences": records}
        path.write_bytes(raw if raw is not None else D.divergences_bytes(document))
        return path

    def load(self, records, **kw):
        return D.load_divergences(self.PIN, {self.CORPUS, "fixture"}, self.manifest(records, **kw))

    def refused(self, records, message, **kw):
        with self.assertRaises(D.Failure) as caught:
            self.load(records, **kw)
        self.assertIn(message, str(caught.exception))


class Ratification(RatifiedRows, unittest.TestCase):
    """Decision 66: a corpus passes when the rows that differ (D) and the rows ratified
    for it (R) are equal multisets, every row exact. D1 to D10 are the decision's cases."""

    def test_d1_the_exact_ratified_difference_passes(self):
        records = self.load([self.record()])
        self.assertEqual(self.judge(self.differences, records), (["issue-1-typed-callee"], {}, []))

    def test_d2_a_difference_no_record_ratifies_fails(self):
        ven = copy.deepcopy(side(V))
        ven[6]["callee"] = f"{V}.pkg.m.h"
        ven.pop(1)
        differences = differs(side(P), ven)
        occurred, unexpected, stale = self.judge(differences, [self.record()])
        self.assertEqual((occurred, stale), (["issue-1-typed-callee"], []))
        self.assertEqual(list(unexpected), ["definitions"])
        lines = D.ratification_lines(self.CORPUS, occurred, unexpected, stale)
        self.assertEqual(lines[0], f"{self.CORPUS}: definitions: 1 row(s) only the reference "
                                   "has, 0 only the vendored engine has")

    def test_d3_a_ratification_that_no_longer_occurs_fails(self):
        occurred, unexpected, stale = self.judge({}, [self.record()])
        self.assertEqual((occurred, unexpected, stale), ([], {}, ["issue-1-typed-callee"]))
        self.assertIn("  ratified divergence no longer occurs: issue-1-typed-callee",
                      D.ratification_lines(self.CORPUS, occurred, unexpected, stale))

    def test_d4_a_wrong_reference_row_fails(self):
        wrong = self.ref_row.replace('"start":10', '"start":11')
        self.assertNotEqual(wrong, self.ref_row)
        occurred, unexpected, stale = self.judge(self.differences,
                                                 self.load([self.record(reference_row=wrong)]))
        self.assertEqual((occurred, stale), ([], ["issue-1-typed-callee"]))
        self.assertEqual(unexpected, self.differences)
        swapped = self.load([self.record(reference_row=self.ven_row, vendored_row=self.ref_row)])
        self.assertEqual(self.judge(self.differences, swapped),
                         ([], self.differences, ["issue-1-typed-callee"]))

    def test_d5_a_wrong_vendored_row_fails(self):
        wrong = self.ven_row.replace(f'"callee":"@.pkg.m.h"', '"callee":"@.pkg.m.k"')
        self.assertNotEqual(wrong, self.ven_row)
        occurred, unexpected, stale = self.judge(self.differences,
                                                 self.load([self.record(vendored_row=wrong)]))
        self.assertEqual((occurred, stale), ([], ["issue-1-typed-callee"]))
        self.assertEqual(unexpected, self.differences)

    def test_d6_the_wrong_corpus_fails(self):
        records = self.load([self.record(corpus="fixture")])
        # In the corpus that differs nothing is ratified, and in the one named, nothing occurs.
        self.assertEqual(self.judge(self.differences, records), ([], self.differences, []))
        self.assertEqual(D.ratify("fixture", {}, records), ([], {}, ["issue-1-typed-callee"]))
        self.refused([self.record(corpus="golden:lang/other")], "no corpus is named")
        self.refused([self.record(corpus="golden:*")], "no corpus is named")

    def test_d7_the_wrong_category_fails(self):
        self.refused([self.record(category="calls")],
                     "the reference row is a typed row, and the record says calls")
        self.refused([self.record(category="everything")], "no category is named")
        call_row = D.canonical([call(P)], P, KINDS)[0]
        self.refused([self.record(vendored_row=call_row)],
                     "the vendored row is a calls row, and the record says typed")

    def test_d8_multiplicity_is_kept(self):
        doubled = {"typed": ([self.ref_row] * 2, [self.ven_row] * 2)}
        occurred, unexpected, stale = self.judge(doubled, [self.record()])
        self.assertEqual((occurred, stale), (["issue-1-typed-callee"], []))
        self.assertEqual(unexpected, {"typed": ([self.ref_row], [self.ven_row])})
        twice = [self.record(), self.record(id="issue-1-typed-callee-again")]
        self.assertEqual(self.judge(self.differences, twice),
                         (["issue-1-typed-callee"], {}, ["issue-1-typed-callee-again"]))
        self.assertEqual(self.judge(doubled, twice),
                         (["issue-1-typed-callee", "issue-1-typed-callee-again"], {}, []))

    def test_d9_a_reference_pin_change_fails_until_revalidated(self):
        self.refused([self.record()], "must be revalidated", pin="1" * 40)
        self.assertEqual(len(self.load([self.record()])), 1)

    def test_d10_a_corpus_with_no_ratification_stays_strict(self):
        records = self.load([self.record(corpus="fixture")])
        self.assertEqual(self.judge({}, records), ([], {}, []))
        for differences in (self.differences, differs(side(P), side(V)[:-1])):
            self.assertEqual(self.judge(differences, records), ([], differences, []))
        self.assertEqual(self.judge(self.differences, []), ([], self.differences, []))

    def test_a_one_sided_divergence_names_its_absent_side(self):
        only_ref = {"typed": ([self.ref_row], [])}
        records = self.load([self.record(vendored_row=None)])
        self.assertEqual(self.judge(only_ref, records), (["issue-1-typed-callee"], {}, []))
        occurred, unexpected, stale = self.judge(self.differences, records)
        self.assertEqual((occurred, stale), (["issue-1-typed-callee"], []))
        self.assertEqual(unexpected, {"typed": ([], [self.ven_row])})
        self.refused([self.record(reference_row=None, vendored_row=None)], "explicit absence")
        self.refused([self.record(vendored_row=self.ref_row)], "two different rows")

    def test_nothing_widens_a_record(self):
        # No pattern, prefix, partial row or extra field: a row is one canonical row.
        self.refused([{**self.record(), "pattern": "*"}], "a record holds exactly")
        unsorted = json.dumps(dict(reversed(json.loads(self.ven_row).items())),
                              separators=(",", ":"))
        for row in ("*", self.ven_row[:-20], self.ven_row.replace(",", ", "), unsorted):
            self.refused([self.record(vendored_row=row)], "the vendored row is not a canonical row")
        fields = json.loads(self.ven_row)
        del fields["callee"]
        partial = json.dumps(fields, sort_keys=True, separators=(",", ":"))
        occurred, unexpected, stale = self.judge(
            self.differences, self.load([self.record(vendored_row=partial)]))
        self.assertEqual((occurred, stale), ([], ["issue-1-typed-callee"]))
        self.assertEqual(unexpected, self.differences)

    def test_the_file_is_held_to_one_form(self):
        record = self.record()
        document = {"format": 1, "reference_commit": self.PIN, "divergences": [record]}
        self.refused([], "canonical form", raw=json.dumps(document).encode())
        self.refused([], "nothing else", raw=b'{"format": 1}\n')
        self.refused([self.record(id="Issue 1")], "is not a record id")
        self.refused([self.record(issue="111")], "issue is not")
        self.refused([self.record(decision=True)], "decision is not")
        self.refused([self.record(reason=" ")], "no reason")
        self.refused([self.record(id="b"), self.record(id="a")], "unique and sorted")
        self.refused([self.record(), self.record()], "unique and sorted")

    def test_the_committed_ratifications_hold_against_the_pin_and_the_manifest(self):
        import tomllib
        with open(REPO / "bench/engine-differential.toml", "rb") as fh:
            manifest = tomllib.load(fh)
        names = ({c["name"] for c in manifest["fixture"]}
                 | {f"golden:{g['key']}" for g in manifest["golden"]})
        records = D.load_divergences(D.reference_pin(), names)
        for record in records:
            self.assertGreaterEqual(record["decision"], 66)


class Evidence(unittest.TestCase):
    def test_incomplete_typed_work_is_no_evidence(self):
        for row in (file_row(V, lsp_skipped=True), file_row(V, walk_truncated=True)):
            with self.subTest(row=row), self.assertRaises(D.Failure):
                D.checks("vendored", [row, definition(V)], {"pkg/m.py"})

    def test_a_textual_strategy_among_typed_answers_fails(self):
        with self.assertRaises(D.Failure):
            D.checks("reference", [file_row(P), definition(P), typed(P, strategy="unique_name")],
                     {"pkg/m.py"})

    def test_a_missing_file_or_no_definition_fails(self):
        with self.assertRaises(D.Failure):
            D.checks("reference", [file_row(P), definition(P)], {"pkg/m.py", "pkg/n.py"})
        with self.assertRaises(D.Failure):
            D.checks("reference", [file_row(P)], {"pkg/m.py"})

    def test_the_vendored_cache_must_answer_as_fresh(self):
        rows = [typed(V)]
        cached = {"c": "cached_typed", "file": "pkg/m.py", "callee": f"{V}.pkg.m.g",
                  "strategy": "lsp_direct", "reason": None, "kind": 0, "start": 10, "end": 13}
        D.cached_consistent(rows + [cached])
        with self.assertRaises(D.Failure):
            D.cached_consistent(rows + [{**cached, "strategy": "lsp_method"}])
        with self.assertRaises(D.Failure):
            D.cached_consistent(rows)


class Translation(unittest.TestCase):
    def test_our_names_go_back_to_the_reference_s(self):
        inverse = D.inverse_rename()
        ours = "pdxe_pipeline_pass_calls"
        self.assertIn(ours, inverse)
        translated = D.translate(f'#include "pdxe_core.h"\nint {ours}(void);\nPDXE_x y;\n', inverse)
        self.assertIn(f'#include "{inverse["pdxe"]}.h"', translated)
        self.assertIn(f"int {inverse[ours]}(void);", translated)
        self.assertNotIn(ours, translated)
        # Only whole identifiers: a longer name that merely contains one is unchanged.
        self.assertIn("PDXE_x", translated)

    def test_the_hook_and_rows_translate_completely(self):
        inverse = D.inverse_rename()
        for path in (D.HOOK_SOURCE, D.ROWS_HEADER):
            text = D.translate(path.read_text(), inverse)
            code = "\n".join(l for l in text.splitlines()
                             if not l.lstrip().startswith(("*", "/*", "//")))
            for token in ("pdxe_", "PDXE"):
                self.assertNotIn(token, code, f"{path.name} keeps {token}")


class Checkouts(unittest.TestCase):
    def git(self, cwd, *args):
        env = {"PATH": os.environ["PATH"], "GIT_CONFIG_GLOBAL": os.devnull,
               "GIT_CONFIG_NOSYSTEM": "1", "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@x",
               "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@x",
               "GIT_AUTHOR_DATE": "2026-01-01T00:00:00Z", "GIT_COMMITTER_DATE": "2026-01-01T00:00:00Z"}
        return subprocess.run(["git", "-C", str(cwd), *args], check=True, capture_output=True,
                              text=True, env=env).stdout.strip()

    def repo(self):
        tmp = tempfile.TemporaryDirectory()
        path = pathlib.Path(tmp.name)
        self.git(path, "init", "-q")
        (path / "a.txt").write_text("a\n")
        self.git(path, "add", "a.txt")
        self.git(path, "commit", "-qm", "one")
        first = self.git(path, "rev-parse", "HEAD")
        (path / "a.txt").write_text("b\n")
        self.git(path, "commit", "-qam", "two")
        return tmp, path, first, self.git(path, "rev-parse", "HEAD")

    def test_the_pinned_clean_checkout_is_accepted(self):
        tmp, path, _, head = self.repo()
        with tmp:
            D.check_checkout(path, head, "the reference")

    def test_another_commit_is_refused(self):
        tmp, path, first, _ = self.repo()
        with tmp, self.assertRaises(D.Failure):
            D.check_checkout(path, first, "the reference")

    def test_a_dirty_checkout_is_refused(self):
        tmp, path, _, head = self.repo()
        with tmp:
            (path / "untracked.c").write_text("int x;\n")
            with self.assertRaises(D.Failure):
                D.check_checkout(path, head, "the reference")
            (path / "untracked.c").unlink()
            (path / "a.txt").write_text("edited\n")
            with self.assertRaises(D.Failure):
                D.check_checkout(path, head, "the reference")

    def test_an_absent_checkout_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp, self.assertRaises(D.Failure):
            D.check_checkout(pathlib.Path(tmp) / "none", "0" * 40, "the reference")

    def test_build_and_stage_paths_must_be_outside_the_repository(self):
        self.assertFalse(D.outside(REPO / "target" / "ref", REPO))
        self.assertFalse(D.outside(REPO, REPO))
        self.assertTrue(D.outside("/tmp/pdx-reference-differential", REPO))

    def test_a_reference_build_inside_the_repository_is_refused_before_building(self):
        calls = []
        original_run, original_env = D.subprocess.run, os.environ.get("PDX_DIFFERENTIAL_BUILD")
        try:
            D.subprocess.run = lambda *a, **k: calls.append(a) or None
            for inside in (REPO / "target" / "reference", REPO):
                os.environ["PDX_DIFFERENTIAL_BUILD"] = str(inside)
                with self.subTest(inside=inside), self.assertRaises(D.Failure) as refused:
                    D.build_reference()
                self.assertIn("outside the repository", str(refused.exception))
        finally:
            D.subprocess.run = original_run
            if original_env is None:
                os.environ.pop("PDX_DIFFERENTIAL_BUILD", None)
            else:
                os.environ["PDX_DIFFERENTIAL_BUILD"] = original_env
        self.assertEqual(calls, [], "the build ran")

    def test_a_symlinked_corpus_file_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "real.py").write_text("x = 1\n")
            (root / "dir").mkdir()
            os.symlink(root / "real.py", root / "dir" / "link.py")
            self.assertEqual(D.safe_file(root, "real.py"), root / "real.py")
            with self.assertRaises(D.Failure):
                D.safe_file(root, "dir/link.py")
            with self.assertRaises(D.Failure):
                D.safe_file(root, "absent.py")



# A stand-in for an engine: it records where it ran, what that directory held, and what
# it could read there by the corpus's relative paths, then leaves a file behind.
STAND_IN = """import json, os, sqlite3, sys
seen = {}
for rel in ("src/header.h", "src/x.h"):
    try:
        with open(rel) as fh:
            seen[rel] = fh.read()
    except OSError:
        seen[rel] = None
record = {"side": SIDE, "cwd": os.getcwd(), "listing": sorted(os.listdir(".")),
          "seen": seen, "args": sys.argv[1:]}
with open(LOG, "a") as fh:
    fh.write(json.dumps(record) + "\\n")
with open("left-behind-" + SIDE, "w") as fh:
    fh.write("x")
if SIDE == "reference":
    with open(os.environ["PDX_DIFFERENTIAL_ROWS"], "x") as fh:
        fh.write(json.dumps({"c": "project", "name": "p", "files": 0}) + "\\n")
    db = sqlite3.connect(os.path.join(os.environ[CACHE], "p.db"))
    db.execute("create table projects (name text)")
    db.execute("insert into projects values ('p')")
    db.execute("create table lsp_surface (project text, rel_path text, defs_json text)")
    db.commit()
    db.close()
else:
    print("{}")
"""


class RunVerdicts(RatifiedRows, unittest.TestCase):
    """The live run's verdict and wording under decision 66, the engines stood in for: a
    corpus with ratified divergences passes without being called identical, and an
    unexpected or stale one fails the run."""

    def run_with(self, results, records, pin=None):
        """cmd_run over corpora whose differences are `results`, ratified by `records`,
        with the reference pinned at `pin` (the file's own by default)."""
        root = pathlib.Path(self.tmp.name).resolve()
        dump = root / "dump"
        dump.write_text("")
        found = [{"name": name, "kind": "golden", "language": "lang", "files": []}
                 for name in results]
        every = set(results) | {"fixture"}
        empty = {name: 0 for name in D.CATEGORIES}
        patches = {
            "reference_pin": lambda: pin or self.PIN,
            "reference_checkout": lambda: root / "checkout",
            "check_checkout": lambda *a: None,
            "kind_map": lambda: KINDS,
            "corpora": lambda bench, names: (found, every),
            "build_reference": lambda: root / "binary",
            "run_corpus": lambda corpus, *a: (self.ran.append(corpus["name"]) or
                                              (empty, empty, results[corpus["name"]], True)),
            "DIVERGENCES": self.manifest(records),
        }
        saved = {name: getattr(D, name) for name in patches}
        self.ran = []
        out = io.StringIO()
        try:
            for name, value in patches.items():
                setattr(D, name, value)
            with mock.patch.dict(os.environ, {"PDX_DIFFERENTIAL_STAGE": str(root / "stage")}), \
                    contextlib.redirect_stdout(out):
                code = D.cmd_run(argparse.Namespace(dump=str(dump), pdx_bench="bench",
                                                    corpus=[]))
        finally:
            for name, value in saved.items():
                setattr(D, name, value)
        return code, out.getvalue()

    def test_ratified_divergences_pass_without_being_called_identical(self):
        code, out = self.run_with({self.CORPUS: self.differences, "golden:x/y": {}},
                                  [self.record()])
        self.assertEqual(code, 0, out)
        self.assertIn(f"ok {self.CORPUS} (lang): ", out)
        self.assertIn("; 1 ratified divergence (decision 66): issue-1-typed-callee", out)
        self.assertIn("corpora 2: 2 passed, 1 identical, 1 with 1 ratified divergence, "
                      "0 unexpected divergences, 0 stale ratifications, 0 differ or failed", out)
        self.assertIn("engine differential: passed, 1 identical and 1 with only ratified "
                      "divergences (decision 66)", out)
        self.assertNotIn("engine differential: identical", out)

    def test_identical_corpora_are_identical(self):
        code, out = self.run_with({self.CORPUS: {}, "golden:x/y": {}}, [])
        self.assertEqual(code, 0, out)
        self.assertIn("corpora 2: 2 passed, 2 identical, 0 with 0 ratified divergences", out)
        self.assertTrue(out.endswith("engine differential: identical\n"), out)

    def test_an_unexpected_difference_fails_the_run(self):
        code, out = self.run_with({self.CORPUS: self.differences, "golden:x/y": {}}, [])
        self.assertEqual(code, 1, out)
        self.assertIn(f"DIFFERS {self.CORPUS} (lang)", out)
        self.assertIn(f"{self.CORPUS}: typed: 1 row(s) only the reference has, 1 only the "
                      "vendored engine has", out)
        self.assertIn("2 unexpected divergences, 0 stale ratifications, 1 differ or failed", out)

    def test_a_stale_ratification_fails_the_run(self):
        code, out = self.run_with({self.CORPUS: {}, "golden:x/y": {}}, [self.record()])
        self.assertEqual(code, 1, out)
        self.assertIn("  ratified divergence no longer occurs: issue-1-typed-callee", out)
        self.assertIn("0 unexpected divergences, 1 stale ratification, 1 differ or failed", out)

    def test_a_pin_change_fails_before_any_run(self):
        with self.assertRaises(D.Failure) as caught:
            self.run_with({self.CORPUS: self.differences}, [self.record()], pin="1" * 40)
        self.assertIn("must be revalidated", str(caught.exception))
        self.assertEqual(self.ran, [])
        # An empty file is validated against a pin as well.
        with self.assertRaises(D.Failure):
            self.run_with({self.CORPUS: {}}, [], pin="1" * 40)
        self.assertEqual(self.ran, [])
        self.run_with({self.CORPUS: {}}, [])
        self.assertEqual(self.ran, [self.CORPUS])


class HermeticRuns(unittest.TestCase):
    """Both engines run from a fresh, empty working directory of the harness's own, one
    per engine per corpus, never the corpus root or the caller's: only explicit inputs
    reach an engine (decision 52)."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = pathlib.Path(self.tmp.name).resolve()
        self.root = root
        self.log = root / "runs.jsonl"
        self.dump = self.stand_in("vendored")
        self.binary = self.stand_in("reference")
        self.stage_root = root / "stage"
        self.stage_root.mkdir()
        self.corpus_root = root / "corpus"
        self.ambient = root / "ambient"
        for base, text in ((self.corpus_root, "corpus"), (self.ambient, "ambient")):
            for rel in ("src/a.c", "src/header.h", "src/x.h"):
                (base / rel).parent.mkdir(parents=True, exist_ok=True)
                (base / rel).write_text(f"/* {text} */\n")

    def tearDown(self):
        self.tmp.cleanup()

    def stand_in(self, side):
        path = self.root / f"stand_in_{side}"
        path.write_text(f"#!{sys.executable}\nSIDE = {side!r}\nLOG = {str(self.log)!r}\n"
                        f"CACHE = {D.run_field('cache_variable')!r}\n" + STAND_IN)
        path.chmod(0o755)
        return path

    def corpus(self, name):
        return {"name": name, "root": str(self.corpus_root), "kind": "fixture",
                "language": "c", "files": [{"path": "src/a.c", "engine_language": "c"}],
                "support": ["src/header.h", "src/x.h"]}

    def run_both(self, name):
        """One corpus through both engines' runs, as run_corpus makes them."""
        corpus = self.corpus(name)
        work = self.stage_root / f"work-{name}"
        work.mkdir()
        (work / "home").mkdir(mode=0o700)
        staged = D.stage(corpus, self.stage_root)
        D.run_vendored(self.dump, corpus, staged, work)
        D.run_reference(self.binary, corpus, staged, work)
        return staged

    def runs(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def test_neither_engine_runs_in_the_corpus_root(self):
        staged = self.run_both("a")
        for run in self.runs():
            cwd = pathlib.Path(run["cwd"])
            self.assertTrue(D.outside(cwd, staged, self.corpus_root, self.stage_root, REPO),
                            run)
            self.assertIsNone(run["seen"]["src/header.h"], run)

    def test_files_around_the_caller_are_no_input(self):
        # The corpus and the caller's directory hold conflicting src/x.h: an engine must
        # read neither by a relative path.
        before = os.getcwd()
        os.chdir(self.ambient)
        try:
            self.run_both("a")
        finally:
            os.chdir(before)
        for run in self.runs():
            self.assertEqual(run["seen"], {"src/header.h": None, "src/x.h": None}, run)
            self.assertNotEqual(pathlib.Path(run["cwd"]), self.ambient)

    def test_every_run_starts_in_a_fresh_empty_directory(self):
        # Each stand-in leaves a file behind: no later run, of either engine or another
        # corpus, may find it, and the harness removes each directory after its run.
        self.run_both("a")
        self.run_both("b")
        runs = self.runs()
        self.assertEqual([r["side"] for r in runs], ["vendored", "reference"] * 2)
        for run in runs:
            self.assertEqual(run["listing"], [], run)
            self.assertFalse(pathlib.Path(run["cwd"]).exists(), run)
        self.assertEqual(len({r["cwd"] for r in runs}), 4)

    def test_both_engines_are_isolated_alike(self):
        self.run_both("a")
        vendored, reference = self.runs()
        for run in (vendored, reference):
            cwd = pathlib.Path(run["cwd"])
            self.assertEqual(run["listing"], [], run)
            self.assertTrue(D.outside(cwd, self.root, REPO), run)
            self.assertEqual(cwd.parent, pathlib.Path(tempfile.gettempdir()).resolve(), run)
        self.assertNotEqual(vendored["cwd"], reference["cwd"])

    def test_the_explicit_inputs_are_absolute_and_unchanged(self):
        staged = self.run_both("a")
        vendored, reference = self.runs()
        listing, directory = vendored["args"]
        self.assertTrue(pathlib.Path(listing).is_absolute())
        self.assertEqual(directory, str(staged))
        self.assertEqual(json.loads(reference["args"][-1]), {"repo_path": str(staged)})
        for rel in ("src/a.c", "src/header.h", "src/x.h"):
            self.assertEqual((staged / rel).read_bytes(), (self.corpus_root / rel).read_bytes())
        self.assertEqual(sorted(p.name for p in staged.rglob("*")),
                         ["a.c", "header.h", "src", "x.h"])
        self.assertEqual(sorted(p.name for p in self.corpus_root.rglob("*")),
                         ["a.c", "header.h", "src", "x.h"])


if __name__ == "__main__":
    unittest.main()
