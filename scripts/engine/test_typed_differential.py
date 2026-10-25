#!/usr/bin/env python3
"""Tests for scripts/engine/typed-differential.py's own checks, without either engine:
a stand-in for resolve_dump answers as told, and each check must catch the answer it
exists for. Network-free, stdlib only."""

import os
import pathlib
import subprocess
import sys
import tempfile
import textwrap
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE / "typed-differential.py"

# A stand-in for resolve_dump: the rows of a fixture, changed as FAKE_DUMP says when it
# is asked for the reverse order or the cache.
FAKE = textwrap.dedent('''\
    import os, sys
    rows = ["CALLS\\tapp.run\\tlib.helper\\tlsp_direct\\t0.95",
            "IMPORTS\\tFILE:app.py\\tlib.helper\\thelper"]
    mode = os.environ.get("FAKE_DUMP", "")
    if "--reverse" in sys.argv and mode == "reverse":
        rows[0] = rows[0].replace("lib.helper", "lib.other")
    if "--cached" in sys.argv and mode == "cached":
        rows = rows[1:]
    print("\\n".join(rows))
''')

EXPECTED = ("CALLS\tapp.run\tlib.helper\tlsp_direct\n"
            "IMPORTS\tFILE:app.py\tlib.helper\thelper\n")


class TypedDifferential(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = pathlib.Path(self.tmp.name)
        self.dump = root / "fake_dump"
        self.dump.write_text(f"#!{sys.executable}\n" + FAKE)
        self.dump.chmod(0o755)
        self.fixture = root / "fixture"
        self.fixture.mkdir()
        (self.fixture / "expected.tsv").write_text(EXPECTED)

    def tearDown(self):
        self.tmp.cleanup()

    def run_check(self, mode="", asserts=None):
        if asserts is not None:
            (self.fixture / "assert.tsv").write_text(asserts)
        env = dict(os.environ, FAKE_DUMP=mode)
        return subprocess.run([sys.executable, str(SCRIPT), "--dump", str(self.dump),
                               str(self.fixture)], capture_output=True, text=True, env=env)

    def test_matching_answers_pass(self):
        done = self.run_check()
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)

    def test_answers_that_change_with_the_order_fail(self):
        done = self.run_check("reverse")
        self.assertEqual(done.returncode, 1)
        self.assertIn("added in reverse order", done.stdout)

    def test_answers_that_change_from_the_cache_fail(self):
        done = self.run_check("cached")
        self.assertEqual(done.returncode, 1)
        self.assertIn("come from a cache", done.stdout)

    def test_a_broken_assertion_fails(self):
        done = self.run_check(asserts="-\tCALLS\tapp.run\tlib.helper\t*\n")
        self.assertEqual(done.returncode, 1)
        self.assertIn("unexpected row", done.stdout)
        done = self.run_check(asserts="+\tCALLS\tapp.run\tlib.missing\t*\n")
        self.assertEqual(done.returncode, 1)
        self.assertIn("no row", done.stdout)

    def test_a_difference_from_the_recorded_answers_fails(self):
        (self.fixture / "expected.tsv").write_text(EXPECTED.replace("lib.helper\tlsp", "lib.x\tlsp"))
        done = self.run_check()
        self.assertEqual(done.returncode, 1)
        self.assertIn("missing", done.stdout)

    def test_a_ratified_divergence_is_accepted_exactly(self):
        # Ours has `lib.helper`; the recorded reference has `lib.x`. Ratifying exactly the
        # two one-sided rows passes; leaving one out, or naming one that no longer
        # differs, fails; a malformed line fails.
        (self.fixture / "expected.tsv").write_text(EXPECTED.replace("lib.helper\tlsp", "lib.x\tlsp"))
        both = ("+\tCALLS\tapp.run\tlib.helper\tlsp_direct\twhy ours\n"
                "-\tCALLS\tapp.run\tlib.x\tlsp_direct\twhy theirs\n")
        (self.fixture / "ratified.tsv").write_text(both)
        done = self.run_check()
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        (self.fixture / "ratified.tsv").write_text(both.splitlines(keepends=True)[0])
        done = self.run_check()
        self.assertEqual(done.returncode, 1)
        self.assertIn("missing", done.stdout)
        (self.fixture / "expected.tsv").write_text(EXPECTED)
        (self.fixture / "ratified.tsv").write_text(both)
        done = self.run_check()
        self.assertEqual(done.returncode, 1)
        self.assertIn("no longer occurs", done.stdout)
        (self.fixture / "ratified.tsv").write_text("+\tCALLS\tapp.run\n")
        done = self.run_check()
        self.assertEqual(done.returncode, 1)
        self.assertIn("malformed", done.stdout)


if __name__ == "__main__":
    unittest.main()
