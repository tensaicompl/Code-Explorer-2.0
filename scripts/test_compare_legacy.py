#!/usr/bin/env python3
"""Network-free tests of scripts/compare-legacy.py (P2-13).

Nothing here needs Docker, the network, the embedding model, PostgreSQL or a golden
checkout: segments are built from the real schema in a temporary directory, legacy
graphs are written inline, and every subprocess the script could start is made to
fail the test.

    python3 scripts/test_compare_legacy.py
"""

import importlib.util
import json
import os
import random
import re
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
SCHEMA = ROOT / "crates" / "pdx-core" / "src" / "segment" / "schema.sql"


def load_module():
    spec = importlib.util.spec_from_file_location("compare_legacy", HERE / "compare-legacy.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


cl = load_module()


# --- fixtures -------------------------------------------------------------------------


class Segment:
    """A segment file with the real schema and only the rows a test writes."""

    def __init__(self, directory):
        self.path = Path(directory) / "segment.db"
        self.db = sqlite3.connect(self.path)
        # The full-text index is no input to the comparison, and not every Python build
        # has the module it needs; every other statement is the schema's own.
        schema = re.sub(r"CREATE VIRTUAL TABLE[^;]*;", "", SCHEMA.read_text())
        self.db.executescript(schema)
        self.meta(commit_sha="c" * 40, profile="structural", repo_id="r1", repo_url="u",
                  engine_version="4", schema_version="3", language_matrix_version="2")
        self._edges = 0
        self._sites = 0

    def meta(self, **values):
        for key, value in values.items():
            self.db.execute("INSERT OR REPLACE INTO meta VALUES (?, ?)", (key, value))

    def file(self, path, status="parsed"):
        file_id = "file:" + path
        self.db.execute(
            "INSERT INTO files (file_id, path, language, status, size_bytes) VALUES (?,?,?,?,?)",
            (file_id, path, "python", status, 1),
        )
        self.db.execute(
            "INSERT INTO nodes (node_id, kind, name, qualified_name, file_id) VALUES (?,?,?,?,?)",
            (file_id, "File", path.rsplit("/", 1)[-1], path, file_id),
        )
        return file_id

    def node(self, node_id, kind, name, qn, path, line, parent=None):
        self.db.execute(
            "INSERT INTO nodes (node_id, kind, name, qualified_name, file_id, parent_id, "
            "start_line, end_line) VALUES (?,?,?,?,?,?,?,?)",
            (node_id, kind, name, qn, "file:" + path, parent or "file:" + path, line, line + 1),
        )
        return node_id

    def edge(self, src, dst, kind="CALLS", band="typed", path=None, line=1, callee="f",
             score=None, strategy=None):
        self._edges += 1
        site_id = None
        if path is not None:
            self._sites += 1
            site_id = f"site{self._sites}"
            self.db.execute(
                "INSERT INTO sites (site_id, file_id, enclosing_node_id, site_kind, "
                "ast_fingerprint, start_byte, end_byte, start_line, start_col, end_line, "
                "end_col, callee_text, receiver_text) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
                (site_id, "file:" + path, src, "call", "fp", 0, 1, line, 1, line, 2, callee,
                 None),
            )
        self.db.execute(
            "INSERT INTO edges (edge_id, src, dst, kind, band, engine_score, engine_strategy, "
            "site_id) VALUES (?,?,?,?,?,?,?,?)",
            (f"edge{self._edges}", src, dst, kind, band, score, strategy, site_id),
        )

    def close(self):
        self.db.commit()
        self.db.close()
        return self.path


def legacy_graph(nodes, edges):
    """A legacy graph in the builder's output shape: nodes, then typed edges."""
    return {
        "nodes": [dict(n) for n in nodes],
        "edges": [
            {"source": s, "target": t, "type": kind, "direction": "forward", "weight": 1.0}
            for s, t, kind in edges
        ],
    }


def lfile(path):
    return {"id": f"file:{path}", "type": "file", "name": path.rsplit("/", 1)[-1],
            "filePath": path}


def lsym(kind, path, name, line, qualified=None):
    node = {"id": f"{kind}:{path}:{qualified or name}", "type": kind, "name": name,
            "filePath": path, "lineRange": [line, line + 1]}
    if qualified:
        node["qualifiedName"] = qualified
    return node


def no_subprocess(*args, **kwargs):
    raise AssertionError(f"a subprocess was started: {args!r}")


# --- the new graph --------------------------------------------------------------------


class NewGraph(unittest.TestCase):
    def test_segment_reader_takes_drawn_calls_only(self):
        with tempfile.TemporaryDirectory() as tmp:
            s = Segment(tmp)
            s.file("a.py")
            a = s.node("A", "Function", "a", "a.a", "a.py", 3)
            b = s.node("B", "Function", "b", "a.b", "a.py", 9)
            s.edge(a, b, path="a.py", line=4, callee="b", score=0.9, strategy="lsp_direct")
            s.edge(a, b, kind="CALL_REFERENCE", path="a.py", line=5)
            s.edge(a, b, kind="TESTS", path="a.py", line=6)
            s.edge(a, b, kind="DEFINES_ROUTE", path="a.py", line=7)
            s.edge(a, b, band="candidate", path="a.py", line=8)
            conn = cl.open_segment(s.close())
            nodes = cl.read_pdx_nodes(conn)
            calls = cl.read_pdx_calls(conn)
        self.assertEqual(len(calls), 1)
        call = calls[0]
        self.assertEqual((call.src, call.dst, call.band), ("A", "B", "typed"))
        self.assertEqual((call.site_line, call.callee_text), (4, "b"))
        self.assertEqual((call.engine_score, call.engine_strategy), (0.9, "lsp_direct"))
        self.assertEqual(nodes["A"].path, "a.py")
        self.assertEqual(nodes["B"].qualified_name, "a.b")
        self.assertEqual(nodes["B"].start_line, 9)

    def test_pdx_rows_collapse_to_unique_pairs(self):
        with tempfile.TemporaryDirectory() as tmp:
            s = Segment(tmp)
            s.file("a.py")
            a = s.node("A", "Function", "a", "a.a", "a.py", 1)
            b = s.node("B", "Function", "b", "a.b", "a.py", 9)
            s.edge(a, b, band="typed", path="a.py", line=2)
            s.edge(a, b, band="exact", path="a.py", line=3)
            conn = cl.open_segment(s.close())
            calls = cl.read_pdx_calls(conn)
        pairs = cl.pdx_pairs(calls)
        self.assertEqual(len(calls), 2)
        self.assertEqual(len(pairs), 1)
        self.assertEqual(sorted(c.band for c in pairs[("A", "B")]), ["exact", "typed"])

    def test_segment_is_opened_read_only(self):
        with tempfile.TemporaryDirectory() as tmp:
            s = Segment(tmp)
            s.file("a.py")
            s.node("A", "Function", "a", "a.a", "a.py", 1)
            path = s.close()
            before = path.read_bytes()
            conn = cl.open_segment(path)
            cl.read_pdx_nodes(conn)
            cl.read_pdx_calls(conn)
            cl.segment_meta(conn)
            with self.assertRaises(sqlite3.OperationalError):
                conn.execute("INSERT INTO meta VALUES ('x', 'y')")
            conn.close()
            self.assertEqual(path.read_bytes(), before)


# --- the legacy graph -----------------------------------------------------------------


class LegacyGraph(unittest.TestCase):
    def test_legacy_graph_takes_calls_only(self):
        graph = legacy_graph(
            [lfile("a.py"), lsym("function", "a.py", "f", 1), lsym("function", "a.py", "g", 5)],
            [("file:a.py", "function:a.py:f", "contains"),
             ("function:a.py:f", "function:a.py:g", "calls"),
             ("function:a.py:g", "function:a.py:f", "depends_on")],
        )
        self.assertEqual(cl.legacy_call_edges(graph), [("function:a.py:f", "function:a.py:g")])


# --- the crosswalk --------------------------------------------------------------------


def crosswalk(build):
    with tempfile.TemporaryDirectory() as tmp:
        s = Segment(tmp)
        build(s)
        conn = cl.open_segment(s.close())
        return cl.Crosswalk(cl.read_pdx_nodes(conn))


class Mapping(unittest.TestCase):
    def test_legacy_file_node_maps_by_path(self):
        def build(s):
            s.file("pkg/a.py")
            s.file("other/a.py")
        walk = crosswalk(build)
        mapped = walk.map(lfile("pkg/a.py"))
        self.assertEqual((mapped.status, mapped.node_id), ("unique", "file:pkg/a.py"))
        self.assertEqual(walk.map(lfile("pkg/b.py")).status, "unmapped")

    def test_legacy_symbol_maps_by_file_name_and_qualified_name(self):
        def build(s):
            s.file("m.py")
            s.node("RUN_A", "Method", "run", "m.A.run", "m.py", 3)
            s.node("RUN_B", "Method", "run", "m.B.run", "m.py", 13)
        walk = crosswalk(build)
        mapped = walk.map(lsym("function", "m.py", "run", 40, qualified="B.run"))
        self.assertEqual((mapped.status, mapped.node_id), ("unique", "RUN_B"))

    def test_symbol_never_maps_across_files(self):
        def build(s):
            s.file("x.py")
            s.file("y.py")
            s.node("FOO_Y", "Function", "foo", "y.foo", "y.py", 1)
        walk = crosswalk(build)
        self.assertEqual(walk.map(lsym("function", "x.py", "foo", 1)).status, "unmapped")

    def test_exact_start_line_disambiguates(self):
        def build(s):
            s.file("m.py")
            s.node("F1", "Function", "f", "m.f", "m.py", 3)
            s.node("F2", "Function", "f", "m.f", "m.py", 20)
        walk = crosswalk(build)
        mapped = walk.map(lsym("function", "m.py", "f", 20))
        self.assertEqual((mapped.status, mapped.node_id), ("unique", "F2"))

    def test_ambiguous_legacy_node_is_not_guessed(self):
        def build(s):
            s.file("m.py")
            s.node("F1", "Function", "f", "m.f", "m.py", 3)
            s.node("F2", "Function", "f", "m.f", "m.py", 20)
        walk = crosswalk(build)
        # Neither start line matches, and the nearest one is no evidence.
        for line in (4, 19, 21):
            mapped = walk.map(lsym("function", "m.py", "f", line))
            self.assertEqual((mapped.status, mapped.node_id), ("ambiguous", None))
            self.assertEqual(sorted(mapped.candidates), ["F1", "F2"])

    def test_symbol_kinds_must_be_compatible(self):
        def build(s):
            s.file("m.py")
            s.node("VAR", "Variable", "f", "m.f", "m.py", 1)
            s.node("FUN", "Function", "f", "m.f", "m.py", 5)
            s.node("CLS", "Class", "C", "m.C", "m.py", 9)
            s.node("TEST", "Test", "test_x", "m.test_x", "m.py", 12)
        walk = crosswalk(build)
        self.assertEqual(walk.map(lsym("function", "m.py", "f", 99)).node_id, "FUN")
        self.assertEqual(walk.map(lsym("class", "m.py", "C", 99)).node_id, "CLS")
        self.assertEqual(walk.map(lsym("function", "m.py", "test_x", 99)).node_id, "TEST")


# --- the comparison -------------------------------------------------------------------


def comparison(shuffle_seed=None):
    """same 1, only-legacy 3 (one absent, one unmapped, one ambiguous), only-new 2."""
    with tempfile.TemporaryDirectory() as tmp:
        s = Segment(tmp)
        s.file("m.py")
        s.node("A", "Function", "a", "m.a", "m.py", 1)
        s.node("B", "Function", "b", "m.b", "m.py", 10)
        s.node("C", "Function", "c", "m.c", "m.py", 20)
        s.node("D1", "Function", "d", "m.d", "m.py", 30)
        s.node("D2", "Function", "d", "m.d", "m.py", 40)
        s.edge("A", "B", band="typed", path="m.py", line=2)      # same
        s.edge("A", "B", band="exact", path="m.py", line=3)      # same pair, second site
        s.edge("B", "C", band="typed", path="m.py", line=11)     # only-new
        s.edge("C", "A", band="exact", path="m.py", line=21)     # only-new
        conn = cl.open_segment(s.close())
        nodes = cl.read_pdx_nodes(conn)
        calls = cl.read_pdx_calls(conn)
    lnodes = [lfile("m.py"), lsym("function", "m.py", "a", 1), lsym("function", "m.py", "b", 10),
              lsym("function", "m.py", "c", 20), lsym("function", "m.py", "d", 35),
              lsym("function", "m.py", "gone", 50)]
    ledges = [("function:m.py:a", "function:m.py:b", "calls"),     # same
              ("function:m.py:c", "function:m.py:b", "calls"),     # mapped, absent
              ("function:m.py:a", "function:m.py:gone", "calls"),  # unmapped target
              ("function:m.py:b", "function:m.py:d", "calls"),     # ambiguous target
              ("file:m.py", "function:m.py:a", "contains")]
    if shuffle_seed is not None:
        rng = random.Random(shuffle_seed)
        rng.shuffle(lnodes)
        rng.shuffle(ledges)
        calls = list(calls)
        rng.shuffle(calls)
    return cl.compare(legacy_graph(lnodes, ledges), nodes, calls)


class Compare(unittest.TestCase):
    def test_pair_accounting(self):
        result = comparison()
        counts = result.counts()
        self.assertEqual(counts["legacy_call_edges"], 4)
        self.assertEqual(counts["pdx_raw_calls_rows"], 4)
        self.assertEqual(counts["pdx_unique_calls_pairs"], 3)
        self.assertEqual(counts["pdx_duplicate_rows_collapsed"], 1)
        self.assertEqual((counts["same"], counts["only_legacy"], counts["only_new"]), (1, 3, 2))
        self.assertEqual(counts["only_legacy_mapped_absent"], 1)
        self.assertEqual(counts["only_legacy_endpoint_unmapped"], 1)
        self.assertEqual(counts["only_legacy_endpoint_ambiguous"], 1)
        self.assertEqual(counts["same"] + counts["only_legacy"], counts["legacy_call_edges"])
        self.assertEqual(counts["same_pdx_pairs"] + counts["only_new"],
                         counts["pdx_unique_calls_pairs"])
        endpoints = result.mapping_counts()
        self.assertEqual(endpoints["legacy_endpoint_references"], 8)
        self.assertEqual(endpoints["fully_mapped_legacy_edges"], 2)
        self.assertEqual(sorted(d.target for d in result.only_new),
                         ["m.a#A", "m.c#C"])

    def test_unmapped_legacy_edge_is_kept_as_only_legacy(self):
        result = comparison()
        unmapped = [d for d in result.only_legacy if d.reason == "endpoint-unmapped"]
        self.assertEqual(len(unmapped), 1)
        self.assertEqual(unmapped[0].target, "function:m.py:gone")

    def test_band_sets_are_kept_per_pair(self):
        result = comparison()
        self.assertEqual(result.same[0].bands, ("exact", "typed"))
        self.assertEqual(result.band_participation("same")["typed"], 1)
        self.assertEqual(result.band_participation("only_new")["typed"], 1)
        self.assertEqual(result.band_participation("only_new")["exact"], 1)

    def test_evidence_is_byte_deterministic(self):
        outputs = []
        for seed in (None, 1, 2, 3):
            result = comparison(seed)
            outputs.append(cl.evidence_lines(result, [], {}))
        self.assertTrue(all(o == outputs[0] for o in outputs))
        self.assertEqual(len(outputs[0]), 5)


# --- the sample -----------------------------------------------------------------------


def differences(only_legacy, only_new):
    out = [cl.Difference("only-legacy", f"legacy-src-{i}", f"legacy-dst-{i}")
           for i in range(only_legacy)]
    out += [cl.Difference("only-new", f"new-src-{i}", f"new-dst-{i}") for i in range(only_new)]
    return out


class Sample(unittest.TestCase):
    def test_sampling_is_deterministic(self):
        items = differences(40, 40)
        first = cl.select_sample(items)
        for seed in range(5):
            shuffled = list(items)
            random.Random(seed).shuffle(shuffled)
            self.assertEqual(cl.select_sample(shuffled), first)

    def test_sample_ranks_by_hash_not_input_order(self):
        items = differences(40, 0)
        chosen = [d.key for d in cl.select_sample(items)]
        by_hash = sorted(items, key=lambda d: cl.sample_hash(d))[:20]
        self.assertEqual(chosen, [d.key for d in by_hash])

    def test_sample_balance(self):
        def sides(sample):
            return (sum(d.side == "only-legacy" for d in sample),
                    sum(d.side == "only-new" for d in sample))
        self.assertEqual(sides(cl.select_sample(differences(15, 15))), (10, 10))
        self.assertEqual(sides(cl.select_sample(differences(3, 30))), (3, 17))
        self.assertEqual(sides(cl.select_sample(differences(30, 3))), (17, 3))
        self.assertEqual(sides(cl.select_sample(differences(4, 5))), (4, 5))
        self.assertEqual(sides(cl.select_sample(differences(0, 0))), (0, 0))


# --- the pin, the checkout and the stack ---------------------------------------------


class Pin(unittest.TestCase):
    def test_python_golden_is_read_from_the_lock(self):
        lock = {"lock_version": 1,
                "golden": [{"language": "go", "url": "git.example/x/gor", "commit": "a" * 40},
                           {"language": "python", "url": "git.example/x/pyr.git",
                            "commit": "b" * 40}],
                "scale": [{"name": "s", "url": "git.example/x/py2", "commit": "c" * 40}]}
        pin = cl.python_golden(lock)
        self.assertEqual((pin.repo, pin.commit), ("pyr", "b" * 40))
        self.assertEqual(pin.fetch_url, "https://git.example/x/pyr.git")
        for golden in ([], [lock["golden"][1], lock["golden"][1]]):
            with self.assertRaises(cl.CompareError):
                cl.python_golden({"lock_version": 1, "golden": golden})

    def test_checkout_must_be_the_clean_pin(self):
        env = dict(os.environ, GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1",
                   GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@git.example",
                   GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@git.example")
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "repo"
            repo.mkdir()

            def git(*args):
                return subprocess.run(["git", "-C", str(repo), *args], env=env, check=True,
                                      capture_output=True, text=True).stdout.strip()

            git("init", "-q")
            (repo / "a.py").write_text("x = 1\n")
            git("add", "a.py")
            git("commit", "-qm", "one")
            git("remote", "add", "origin", "https://git.example/x/pyr")
            head = git("rev-parse", "HEAD")
            pin = cl.GoldenPin("python", "git.example/x/pyr", head, "pyr")
            cl.verify_checkout(repo, pin)
            with self.assertRaises(cl.CompareError):
                cl.verify_checkout(repo, cl.GoldenPin("python", "git.example/x/pyr", "d" * 40,
                                                      "pyr"))
            with self.assertRaises(cl.CompareError):
                cl.verify_checkout(repo, cl.GoldenPin("python", "git.example/x/other", head,
                                                      "other"))
            (repo / "b.py").write_text("y = 2\n")
            with self.assertRaises(cl.CompareError):
                cl.verify_checkout(repo, pin)
            (repo / "b.py").unlink()
            link = Path(tmp) / "link"
            link.symlink_to(repo)
            with self.assertRaises(cl.CompareError):
                cl.verify_checkout(link, pin)


class Stack(unittest.TestCase):
    def test_compose_command_uses_the_isolated_project(self):
        paths = cl.Paths.under(Path("/w"))
        args = cl.compose_args(paths, "config")
        self.assertEqual(args[:2], ["docker", "compose"])
        self.assertIn(str(Path("/w/legacy/docker-compose.yml")), args)
        self.assertIn(str(paths.override), args)
        self.assertEqual(args[args.index("-p") + 1], "pdx-p2-13")
        self.assertEqual(args[args.index("--profile") + 1], "docker-indexer")
        self.assertEqual(args[args.index("--env-file") + 1], str(paths.env_file))
        self.assertEqual(args[-1], "config")
        # The legacy file comes first, so the override is the one that wins.
        self.assertLess(args.index(str(Path("/w/legacy/docker-compose.yml"))),
                        args.index(str(paths.override)))

    def test_override_isolates_storage_and_source(self):
        paths = cl.Paths.under(Path("/w"))
        text = cl.compose_override(paths)
        self.assertIn("pdx_p2_13_pgdata:/var/lib/postgresql/data", text)
        self.assertIn(f"{paths.codebase}:/data/codebase:ro", text)
        self.assertIn(f"{paths.output}:/out", text)
        self.assertIn("ports: !reset []", text)
        self.assertIn("INDEXER_DEVICE=cpu", text)
        self.assertNotIn("pgdata:/var/lib/postgresql/data\n", text.replace(
            "pdx_p2_13_pgdata:/var/lib/postgresql/data", ""))

    def test_compose_environment_carries_no_host_variables(self):
        with mock.patch.dict(os.environ, {"ANTHROPIC_API_KEY": "x", "PATH": "/bin"}):
            env = cl.compose_environment()
        self.assertNotIn("ANTHROPIC_API_KEY", env)
        self.assertEqual(env["PATH"], "/bin")

    def test_compare_only_starts_no_process(self):
        with tempfile.TemporaryDirectory() as tmp:
            s = Segment(tmp)
            s.file("m.py")
            s.node("A", "Function", "a", "m.a", "m.py", 1)
            s.node("B", "Function", "b", "m.b", "m.py", 10)
            s.edge("A", "B", path="m.py", line=2)
            segment = s.close()
            paths = cl.Paths.under(Path(tmp) / "w")
            paths.output.mkdir(parents=True)
            paths.legacy_graph.write_text(json.dumps(legacy_graph(
                [lsym("function", "m.py", "a", 1), lsym("function", "m.py", "b", 10)],
                [("function:m.py:a", "function:m.py:b", "calls")])))
            paths.legacy_meta.write_text(json.dumps({"resolution": {"exact": 1}}))
            paths.legacy_calls.write_text("caller_name,caller_file,caller_line,callee_name\n"
                                          "a,m.py,2,b\n")
            with mock.patch.object(subprocess, "run", no_subprocess), \
                    mock.patch.object(subprocess, "Popen", no_subprocess):
                summary = cl.compare_outputs(paths, segment, golden_json=None)
            self.assertEqual((summary["same"], summary["only_legacy"], summary["only_new"]),
                             (1, 0, 0))
            self.assertTrue(paths.summary.exists() and paths.evidence.exists())

    def test_help_exits_zero_without_docker(self):
        result = subprocess.run([sys.executable, str(HERE / "compare-legacy.py"), "--help"],
                                env={"PATH": ""}, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--compare-only", result.stdout)


class Report(unittest.TestCase):
    def test_report_counts_are_checked_against_the_summary(self):
        summary = {"same": 3, "only_legacy": 4, "only_new": 5}
        good = "intro\n```text\nsame         3\nonly-legacy  4\nonly-new     5\n```\nmore\n"
        self.assertEqual(cl.report_count_mismatches(good, summary), [])
        bad = good.replace("only-new     5", "only-new     6")
        self.assertEqual(len(cl.report_count_mismatches(bad, summary)), 1)
        self.assertEqual(len(cl.report_count_mismatches("no block\n", summary)), 3)


if __name__ == "__main__":
    unittest.main(verbosity=1)
