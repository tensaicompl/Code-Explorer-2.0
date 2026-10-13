#!/usr/bin/env python3
"""A0.1's oracle sanity check (decision 64): before any whole-repository number is
accepted, the oracle mapping must give the expected answer for every case of the
synthetic fixtures in `bench/a0/sanity/`.

Each case is a line of a fixture marked `A0: <case> -> <expected>`, where the
expected answer is a structural qualified name (the oracle must resolve the call on
that line to that definition), `external`, `none` (no oracle verdict), `ambiguous`
(the oracle's target maps to more than one definition, or to none), or `macro` (a
macro invocation, which is no call). The case's name is the call's name token on
that line: the token checked is the line's call whose name the case names, or, for
`macro`, the line's macro invocation. Where the structural engine has a site for the
call, its name token must be the oracle call's (rule M1).

    sanity.py --fixture DIR --audit DIR --corpus NAME
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

MARK = re.compile(r"(?:#|//) A0: (\w+) -> (\S+)")

# The callee each case names on its line.
NAMES = {
    "plain_local": "local_helper",
    "imported": "compute_total",
    "constructor": "Widget",
    "method": "render",
    "static": None,  # build_default (Python), new (Rust)
    "same_short_name": "draw",
    "overload": None,  # convert (Python), area (Rust)
    "external": None,  # dumps (Python), new (Rust)
    "recursive": "factorial",
    "alias": "total_alias",
    "unresolved_receiver": "render",
    "macro_token_tree": "compute_total",
    "macro_invocation": "twice",
    "duplicate_symbol": "helper",
}
LANGUAGE_NAMES = {
    ("python", "static"): "build_default",
    ("rust", "static"): "new",
    ("python", "overload"): "convert",
    ("rust", "overload"): "area",
    ("python", "external"): "dumps",
    ("rust", "external"): "new",
}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--fixture", required=True, type=Path)
    ap.add_argument("--audit", required=True, type=Path)
    ap.add_argument("--corpus", required=True)
    args = ap.parse_args()
    meta = json.loads((args.audit / f"{args.corpus}.meta.json").read_text())
    language = meta["language"]
    rels = [json.loads(x) for x in (args.audit / f"{args.corpus}.relations.jsonl").open()]
    sites = [json.loads(x) for x in (args.audit / f"{args.corpus}.sites.jsonl").open()]
    failures = 0
    checked = 0
    for path in sorted(p for p in args.fixture.rglob("*") if p.suffix in (".py", ".rs")):
        rel = path.relative_to(args.fixture).as_posix()
        data = path.read_bytes()
        starts = [0] + [i + 1 for i, b in enumerate(data) if b == 0x0A]
        for lineno, line in enumerate(data.decode().splitlines(), 1):
            m = MARK.search(line)
            if not m:
                continue
            case, expected = m.group(1), m.group(2)
            name = NAMES.get(case) or LANGUAGE_NAMES.get((language, case))
            lo, hi = starts[lineno - 1], starts[lineno] if lineno < len(starts) else len(data)
            checked += 1
            if expected == "macro":
                found = [mi for mi in meta["macro_invocations"]
                         if mi["path"] == rel and lo <= mi["token"][0] < hi and mi["name"] == name]
                ok = len(found) == 1
                print(f"{'PASS' if ok else 'FAIL'} {rel}:{lineno} {case}: macro invocation "
                      f"`{name}` {'recorded' if ok else 'missing'}")
                failures += not ok
                continue
            cands = [r for r in rels if r["file"] == rel and r.get("token")
                     and lo <= r["token"][0] < hi
                     and data[r["token"][0]:r["token"][1]].decode() == name]
            if len(cands) != 1:
                print(f"FAIL {rel}:{lineno} {case}: {len(cands)} oracle calls named `{name}`")
                failures += 1
                continue
            r = cands[0]
            status = r.get("oracle_status")
            if expected in ("external", "none"):
                ok = status == expected
                got = status
            elif expected == "ambiguous":
                ok = status in ("ambiguous", "unmapped", "multiple")
                got = status
            else:
                ok = status == "mapped" and r.get("oracle_target_qn") == expected
                got = r.get("oracle_target_qn") or status
            # The structural site at the same call must take the same name token.
            token_ok = True
            for s in r.get("pdx_sites", []):
                row = next(x for x in sites if x["file"] == rel and x["site"] == s["site"])
                token_ok &= row["token"] == r["token"]
            shape = r["shape"]
            print(f"{'PASS' if ok and token_ok else 'FAIL'} {rel}:{lineno} {case} ({shape}): "
                  f"expected {expected}, oracle {got}; structural sites {len(r['pdx_sites'])}"
                  f"{'' if token_ok else ', token disagrees'}")
            failures += not (ok and token_ok)
    print(f"sanity: {checked - failures}/{checked} cases pass")
    return 1 if failures or not checked else 0


if __name__ == "__main__":
    sys.exit(main())
