#!/usr/bin/env python3
"""Tests for scripts/engine/differential.py: network-free, stdlib only, and independent
of both engines. The rows here are project-authored; the reference checkout is a local
synthetic Git repository, never the real one."""

import copy
import importlib.util
import json
import os
import pathlib
import subprocess
import tempfile
import unittest

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


if __name__ == "__main__":
    unittest.main()
