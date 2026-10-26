#!/usr/bin/env python3
"""P0-07's acceptance tests: actions pinned by digest, the supply-chain job failing on
an advisory, and coverage floors that only ratchet up. Network-free, stdlib only."""

import datetime as dt
import importlib.util
import json
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent


def load(name):
    spec = importlib.util.spec_from_file_location(name, HERE / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


pinned = load("actions_pinned")
audit = load("ui_audit")
coverage = load("coverage_floors")

SHA = "11d5960a326750d5838078e36cf38b85af677262"


def report(*advisories):
    """A pnpm audit report holding the given (id, severity, module) advisories."""
    return {"advisories": {str(i): {"github_advisory_id": ident, "severity": sev,
                                    "module_name": mod, "title": "t"}
                           for i, (ident, sev, mod) in enumerate(advisories)}}


class ActionsPinned(unittest.TestCase):
    def test_actions_pinned_by_digest(self):
        """The repository's own workflows pass, and the check refuses every unpinned form."""
        run = subprocess.run([sys.executable, str(HERE / "actions_pinned.py"), "--root", str(ROOT)],
                             capture_output=True, text=True)
        self.assertEqual(run.returncode, 0, run.stderr)
        good = f"      - uses: actions/checkout@{SHA} # v4.4.0\n"
        self.assertEqual(pinned.problems(good, "w.yml"), [])
        for bad in ("      - uses: actions/checkout@v4\n",
                    "      - uses: actions/checkout@main\n",
                    f"      - uses: actions/checkout@{SHA[:12]} # v4.4.0\n",
                    "        uses: org/action/sub@v1.2.3\n"):
            self.assertEqual(len(pinned.problems(bad, "w.yml")), 1, bad)

    def test_a_pin_names_its_release(self):
        self.assertEqual(len(pinned.problems(f"  - uses: actions/checkout@{SHA}\n", "w.yml")), 1)

    def test_local_and_digest_containers_pass(self):
        text = ("  - uses: ./.github/actions/setup\n"
                "  - uses: docker://alpine@sha256:" + "a" * 64 + "\n")
        self.assertEqual(pinned.problems(text, "w.yml"), [])


class SupplyChain(unittest.TestCase):
    TODAY = dt.date(2026, 10, 26)

    def test_supply_chain_job_fails_on_advisory(self):
        """CI's supply-chain job runs the advisory checks without a way to pass around
        them, and the interface's policy fails on an advisory it has not accepted."""
        ci = (ROOT / ".github/workflows/ci.yml").read_text()
        job = ci[ci.index("  supply-chain:"):]
        job = job[: re.search(r"\n  [a-z][a-z-]*:\n", job[1:]).start() + 1]
        self.assertIn("run: make deny", job)
        self.assertIn("run: make ui-audit", job)
        self.assertNotIn("continue-on-error", job)
        deny = (ROOT / "deny.toml").read_text()
        section = deny[deny.index("[advisories]"):]
        section = section[: section.index("\n[", 1)] if "\n[" in section[1:] else section
        self.assertNotRegex(section, r"(?m)^\s*ignore\s*=\s*\[\s*\"")
        self.assertIn('yanked = "deny"', section)
        failures, _ = audit.evaluate(report(("GHSA-a", "low", "react")),
                                     report(("GHSA-a", "low", "react")), [], self.TODAY)
        self.assertEqual(len(failures), 1)

    def test_development_advisories_fail_from_moderate(self):
        everything = report(("GHSA-m", "moderate", "vite"), ("GHSA-l", "low", "vite"))
        failures, notes = audit.evaluate(report(), everything, [], self.TODAY)
        self.assertEqual([f.split()[1] for f in failures], ["GHSA-m"])
        self.assertEqual(len(notes), 1)

    def test_an_exception_needs_a_reason_and_expires(self):
        everything = report(("GHSA-x", "high", "vite"))
        good = [{"advisory": "GHSA-x", "reason": "dev server only", "expires": "2026-11-01"}]
        self.assertEqual(audit.evaluate(report(), everything, good, self.TODAY)[0], [])
        expired = [dict(good[0], expires="2026-10-01")]
        self.assertTrue(audit.evaluate(report(), everything, expired, self.TODAY)[0])
        no_reason = [dict(good[0], reason=" ")]
        self.assertTrue(audit.evaluate(report(), everything, no_reason, self.TODAY)[0])


def summary(**crates):
    """An llvm-cov export summary with one source file per crate: {crate: (covered, count)}."""
    files = [{"filename": f"/w/crates/{c}/src/lib.rs",
              "summary": {"lines": {"covered": cov, "count": n}}} for c, (cov, n) in crates.items()]
    files.append({"filename": "/w/crates/a/tests/t.rs", "summary": {"lines": {"covered": 0, "count": 99}}})
    return {"data": [{"files": files}]}


class CoverageFloors(unittest.TestCase):
    def test_coverage_floor_ratchets(self):
        """A floor rises with the measurement, never falls, and a drop below it fails."""
        measured = coverage.per_crate(summary(a=(812, 1000), b=(500, 1000)))
        self.assertEqual(measured["a"], (812, 1000))  # tests/ are not measured code
        floors = coverage.ratchet(measured, {"a": 80.0, "b": 40.0})
        self.assertEqual(floors, {"a": 81.2, "b": 50.0})
        self.assertEqual(coverage.check(measured, floors), [])
        lower = coverage.per_crate(summary(a=(800, 1000), b=(500, 1000)))
        self.assertEqual(len(coverage.check(lower, floors)), 1)
        self.assertEqual(coverage.ratchet(lower, floors)["a"], 81.2)

    def test_a_new_crate_needs_a_floor(self):
        measured = coverage.per_crate(summary(a=(1, 2), c=(1, 2)))
        self.assertEqual(len(coverage.check(measured, {"a": 50.0})), 1)

    def test_the_command_ratchets_the_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "scripts/ci").mkdir(parents=True)
            (root / coverage.FLOORS).write_text(json.dumps({"floors": {"a": 90.0}}))
            path = root / "s.json"
            path.write_text(json.dumps(summary(a=(800, 1000))))
            cmd = [sys.executable, str(HERE / "coverage_floors.py"), str(path), "--root", str(root)]
            self.assertEqual(subprocess.run(cmd, capture_output=True).returncode, 1)
            subprocess.run(cmd + ["--ratchet"], check=True, capture_output=True)
            self.assertEqual(json.loads((root / coverage.FLOORS).read_text())["floors"]["a"], 90.0)


if __name__ == "__main__":
    unittest.main()
