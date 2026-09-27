#!/usr/bin/env python3
"""Fails when the two constant definitions disagree.

The Rust module is the source of truth; the interface mirrors the subset it needs.
A budget that differs between the renderer and the server is a defect that no test
of either side alone would catch, so it is checked here.

  python3 scripts/consts-sync.py [--root .]

Exit codes: 0 in agreement, 1 a disagreement (listed on stderr).
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

RUST = "crates/pdx-core/src/consts.rs"
TS = "ui/src/consts.ts"
MIRROR = "scripts/consts-mirror.txt"

RUST_CONST = re.compile(
    r"^pub const (?P<name>[A-Z][A-Z0-9_]*)\s*:\s*(?P<type>[a-z0-9]+)\s*=\s*(?P<value>[^;]+);",
    re.M,
)
TS_CONST = re.compile(
    r"^export const (?P<name>[A-Z][A-Z0-9_]*)\s*=\s*(?P<value>[^;]+);",
    re.M,
)


def evaluate(expr: str) -> float:
    """Evaluates a numeric literal expression, which is all these files contain."""
    cleaned = expr.replace("_", "").strip()
    # Digits and arithmetic only: no names, no calls, nothing to evaluate but maths.
    if not re.fullmatch(r"[0-9.+\-*/() ]+", cleaned):
        raise ValueError(f"not a numeric expression: {expr}")
    # Only arithmetic on literals: no names, no calls.
    return float(eval(cleaned, {"__builtins__": {}}, {}))  # noqa: S307


def parse(path: Path, pattern: re.Pattern[str]) -> dict[str, tuple[str, float]]:
    text = path.read_text()
    out: dict[str, tuple[str, float]] = {}
    for m in pattern.finditer(text):
        raw = m.group("value").strip()
        try:
            out[m.group("name")] = (raw, evaluate(raw))
        except ValueError as exc:
            sys.exit(f"error: {path}: {m.group('name')}: {exc}")
    return out


def docs_of(path: Path, marker: str) -> set[str]:
    """Names carrying a doc comment on the line or block directly above them."""
    documented: set[str] = set()
    lines = path.read_text().splitlines()
    for i, line in enumerate(lines):
        m = re.match(rf"^{marker} (?P<name>[A-Z][A-Z0-9_]*)", line)
        if not m:
            continue
        j = i - 1
        while j >= 0 and (lines[j].strip().startswith(("///", "*", "/**", "*/", "//!"))
                          or lines[j].strip() == ""):
            if lines[j].strip().startswith(("///", "/**", "*")):
                documented.add(m.group("name"))
                break
            if lines[j].strip() == "":
                break
            j -= 1
    return documented


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=".")
    args = ap.parse_args()
    root = Path(args.root).resolve()

    rust_path, ts_path, mirror_path = root / RUST, root / TS, root / MIRROR
    for p in (rust_path, ts_path, mirror_path):
        if not p.is_file():
            sys.exit(f"error: {p} is missing")

    rust = parse(rust_path, RUST_CONST)
    ts = parse(ts_path, TS_CONST)
    mirrored = [
        line.strip()
        for line in mirror_path.read_text().splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    ]

    errors: list[str] = []

    for name in mirrored:
        if name not in rust:
            errors.append(f"{name}: required in both, absent from {RUST}")
        if name not in ts:
            errors.append(f"{name}: required in both, absent from {TS}")
        if name in rust and name in ts and rust[name][1] != ts[name][1]:
            errors.append(
                f"{name}: {RUST} says {rust[name][0]} ({rust[name][1]:g}), "
                f"{TS} says {ts[name][0]} ({ts[name][1]:g})"
            )

    # A value in the interface with no counterpart would let the two sides disagree
    # about the same budget without anything noticing.
    for name in ts:
        if name not in rust:
            errors.append(f"{name}: defined in {TS} with no counterpart in {RUST}")
        elif name not in mirrored:
            errors.append(f"{name}: mirrored in {TS} but not listed in {MIRROR}")

    # Every constant carries a doc comment: these are specification values, and a
    # bare number tells a reader nothing about what it governs.
    for name in sorted(rust):
        if name not in docs_of(rust_path, "pub const"):
            errors.append(f"{name}: no doc comment in {RUST}")
    for name in sorted(ts):
        if name not in docs_of(ts_path, "export const"):
            errors.append(f"{name}: no doc comment in {TS}")

    if errors:
        for e in errors:
            print(f"error: {e}", file=sys.stderr)
        print(f"consts sync: {len(errors)} disagreement(s)", file=sys.stderr)
        sys.exit(1)

    print(
        f"consts sync: in agreement ({len(rust)} defined, {len(mirrored)} mirrored)"
    )


if __name__ == "__main__":
    main()
