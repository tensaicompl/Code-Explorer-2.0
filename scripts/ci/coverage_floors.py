#!/usr/bin/env python3
"""Per-crate line coverage against floors that only ratchet up (P0-07).

Reads the JSON summary `cargo llvm-cov` writes (`--json --summary-only`), sums the
lines of each crate's own sources (`crates/<crate>/src/`; tests and benches are not
measured code), and compares each crate with its floor in
`scripts/ci/coverage-floors.json`.

    python3 scripts/ci/coverage_floors.py <summary.json> [--root .]            check
    python3 scripts/ci/coverage_floors.py <summary.json> --ratchet [--root .]  raise

A crate below its floor fails; so does a measured crate with no floor. `--ratchet`
writes floors at the measured value rounded down to a tenth, and never lowers one:
coverage may be added, never given back without someone changing the file by hand,
in a reviewed commit that says why.
"""

from __future__ import annotations

import argparse
import json
import math
import re
import sys
from pathlib import Path

FLOORS = "scripts/ci/coverage-floors.json"
CRATE_SRC = re.compile(r"(?:^|/)crates/(?P<crate>[^/]+)/src/")


def per_crate(summary: dict) -> dict[str, tuple[int, int]]:
    """{crate: (covered lines, lines)} from llvm-cov's export summary."""
    totals: dict[str, list[int]] = {}
    for data in summary.get("data", []):
        for f in data.get("files", []):
            m = CRATE_SRC.search(f.get("filename", "").replace("\\", "/"))
            if not m:
                continue
            lines = f.get("summary", {}).get("lines", {})
            t = totals.setdefault(m.group("crate"), [0, 0])
            t[0] += int(lines.get("covered", 0))
            t[1] += int(lines.get("count", 0))
    return {c: (v[0], v[1]) for c, v in totals.items() if v[1] > 0}


def percent(covered: int, count: int) -> float:
    return 100.0 * covered / count


def check(measured: dict[str, tuple[int, int]], floors: dict[str, float]) -> list[str]:
    failures = []
    for crate, (covered, count) in sorted(measured.items()):
        value = percent(covered, count)
        if crate not in floors:
            failures.append(f"{crate}: {value:.1f}% measured and no floor recorded")
        elif value + 1e-9 < floors[crate]:
            failures.append(f"{crate}: {value:.1f}% is below its floor of {floors[crate]:.1f}%")
    for crate in sorted(set(floors) - set(measured)):
        failures.append(f"{crate}: has a floor but was not measured")
    return failures


def ratchet(measured: dict[str, tuple[int, int]], floors: dict[str, float]) -> dict[str, float]:
    """Floors raised to the measured values (rounded down to a tenth), never lowered."""
    out = dict(floors)
    for crate, (covered, count) in measured.items():
        value = math.floor(percent(covered, count) * 10) / 10
        out[crate] = max(out.get(crate, 0.0), value)
    return dict(sorted(out.items()))


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("summary")
    ap.add_argument("--root", default=".")
    ap.add_argument("--ratchet", action="store_true")
    args = ap.parse_args()
    measured = per_crate(json.loads(Path(args.summary).read_text()))
    path = Path(args.root) / FLOORS
    floors = json.loads(path.read_text())["floors"] if path.exists() else {}
    for crate, (covered, count) in sorted(measured.items()):
        print(f"coverage: {crate}: {percent(covered, count):.1f}% ({covered}/{count} lines)")
    if args.ratchet:
        new = ratchet(measured, floors)
        path.write_text(json.dumps({"floors": new}, indent=2) + "\n")
        print(f"coverage: floors written to {FLOORS}")
        return 0
    failures = check(measured, floors)
    for line in failures:
        print(f"coverage: {line}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
