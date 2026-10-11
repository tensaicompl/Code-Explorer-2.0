#!/usr/bin/env python3
"""Tests for scripts/owner-census.py: network-free, stdlib only, and independent of the
engine program it runs (the program's own counting is engine/tests/owner_occurrence.c's
subject)."""

import importlib.util
import os
import sqlite3
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent


def load_module():
    spec = importlib.util.spec_from_file_location("owner_census", HERE / "owner-census.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


census = load_module()


def segment(rows):
    """A minimal segment holding only the files table the script reads."""
    handle, path = tempfile.mkstemp(suffix=".db")
    os.close(handle)
    db = sqlite3.connect(path)
    db.execute("CREATE TABLE files (path TEXT, language TEXT, status TEXT)")
    db.executemany("INSERT INTO files VALUES (?, ?, ?)", rows)
    db.commit()
    db.close()
    return path


class ParsedFiles(unittest.TestCase):
    def test_lists_parsed_and_partial_files_in_path_order(self):
        path = segment(
            [
                ("src/b.py", "python", "parsed"),
                ("src/a.py", "python", "partial"),
                ("vendor/c.py", "python", "failed"),
                ("secret.env", "unknown", "redacted"),
                ("big.bin", "unknown", "skipped"),
            ]
        )
        try:
            self.assertEqual(
                census.parsed_files(path), [("python", "src/a.py"), ("python", "src/b.py")]
            )
        finally:
            os.remove(path)


class EngineLanguage(unittest.TestCase):
    def test_tsx_files_are_read_with_the_tsx_grammar(self):
        self.assertEqual(census.engine_language("typescript", "src/view.tsx"), "tsx")

    def test_every_other_file_is_read_as_its_language(self):
        self.assertEqual(census.engine_language("typescript", "src/view.ts"), "typescript")
        self.assertEqual(census.engine_language("javascript", "src/view.jsx"), "javascript")
        self.assertEqual(census.engine_language("cpp", "src/a.tsx.cc"), "cpp")


class Main(unittest.TestCase):
    def test_feeds_the_census_program_the_segment_listing(self):
        path = segment([("src/view.tsx", "typescript", "parsed"), ("src/a.py", "python", "parsed")])
        try:
            done = subprocess.CompletedProcess([], 0)
            with mock.patch.object(census.os, "access", return_value=True), mock.patch.object(
                census.subprocess, "run", return_value=done
            ) as run:
                self.assertEqual(census.main(["--list", path, "/checkout"]), 0)
            command = run.call_args.args[0]
            self.assertEqual(command, [census.CENSUS, "--list", "/checkout"])
            self.assertEqual(
                run.call_args.kwargs["input"], "python\tsrc/a.py\ntsx\tsrc/view.tsx\n"
            )
        finally:
            os.remove(path)

    def test_occurrences_mode_is_passed_on(self):
        path = segment([("src/a.py", "python", "parsed")])
        try:
            done = subprocess.CompletedProcess([], 0)
            with mock.patch.object(census.os, "access", return_value=True), mock.patch.object(
                census.subprocess, "run", return_value=done
            ) as run:
                census.main(["--occurrences", path, "/checkout"])
            self.assertEqual(run.call_args.args[0], [census.CENSUS, "--occurrences", "/checkout"])
        finally:
            os.remove(path)

    def test_refuses_without_the_census_program(self):
        with mock.patch.object(census.os, "access", return_value=False):
            with self.assertRaises(SystemExit) as stop:
                census.main(["segment.db", "/checkout"])
        self.assertIn("make engine", str(stop.exception))


if __name__ == "__main__":
    unittest.main()
