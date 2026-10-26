#!/usr/bin/env python3
"""How much of what ships is vetted by an audit rather than an exemption (P0-07).

Decision 83: by G7 the shipped dependency closure is covered by recorded audits or
trusted imports, and exemptions remain only for build- and test-only dependencies.
This reports progress toward that; it fails nothing until G7 makes it a criterion.

The shipped closure is every third-party crate in the normal dependency graph of the
`pdx` binary with the enterprise feature (the widest build that ships): build and
dev dependencies are excluded, proc macros are included (their output ships).

    python3 scripts/ci/vet_shipped.py [--root .]
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


def shipped(root: Path) -> set[tuple[str, str]]:
    run = subprocess.run(
        ["cargo", "tree", "--locked", "-p", "pdx", "--features", "enterprise", "-e", "normal",
         "--prefix", "none", "--no-dedupe", "--format", "{p}"],
        cwd=root, capture_output=True, text=True, check=True,
    )
    out = set()
    for line in run.stdout.splitlines():
        parts = line.split()
        if len(parts) < 2 or not parts[1].startswith("v"):
            continue
        if "(" in line and ("/" in line.split("(", 1)[1]):
            continue  # a path dependency: one of ours
        out.add((parts[0], parts[1][1:]))
    return out


def vet(root: Path) -> dict:
    run = subprocess.run(["cargo", "vet", "--locked", "--output-format=json"],
                         cwd=root, capture_output=True, text=True)
    try:
        return json.loads(run.stdout)
    except json.JSONDecodeError:
        sys.exit(f"vet-shipped: cargo vet gave no report: {run.stderr.strip()[:400]}")


def classify(closure: set[tuple[str, str]], report: dict) -> dict[str, list[tuple[str, str]]]:
    def keys(field):
        return {(e["name"], e["version"]) for e in report.get(field, [])}
    full, partial, exempt = keys("vetted_fully"), keys("vetted_partially"), keys("vetted_with_exemptions")
    out = {"audited": [], "partial": [], "exempted": [], "unknown": []}
    for crate in sorted(closure):
        if crate in full:
            out["audited"].append(crate)
        elif crate in partial:
            out["partial"].append(crate)
        elif crate in exempt:
            out["exempted"].append(crate)
        else:
            out["unknown"].append(crate)
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=".")
    args = ap.parse_args()
    root = Path(args.root)
    closure = shipped(root)
    groups = classify(closure, vet(root))
    total = len(closure)
    audited = len(groups["audited"])
    share = 100.0 * audited / total if total else 100.0
    print(f"vet-shipped: {total} shipped crates; audited or imported {audited} ({share:.1f}%), "
          f"partly audited {len(groups['partial'])}, exempted {len(groups['exempted'])}, "
          f"not in cargo vet's report {len(groups['unknown'])}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
