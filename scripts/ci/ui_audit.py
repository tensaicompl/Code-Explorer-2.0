#!/usr/bin/env python3
"""The interface's dependency audit (P0-07, issue 100).

Runs `pnpm audit` over the interface's lockfile twice, production dependencies alone
and everything, and applies the policy:

- any advisory against a production dependency fails: that code ships;
- an advisory of `moderate` or above against a development dependency fails too: the
  build and test tools run on developers' machines and in CI;
- `low` development advisories are reported, not failed;
- an advisory is accepted only by an entry in `ui/audit-exceptions.json` naming it,
  with a reason and an expiry date; an expired entry or one without a reason fails.

    python3 scripts/ci/ui_audit.py [--root .]
    python3 scripts/ci/ui_audit.py --prod-json P --all-json A   (recorded reports)

Needs the network (the advisory database); CI's supply-chain job runs it, not `check`.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import subprocess
import sys
from pathlib import Path

FAILING_DEV = {"moderate", "high", "critical"}


def advisories(report: dict) -> dict[str, dict]:
    """pnpm's audit report as {advisory id: {severity, module, title}}."""
    out = {}
    for key, adv in (report.get("advisories") or {}).items():
        ident = adv.get("github_advisory_id") or str(key)
        out[ident] = {
            "severity": adv.get("severity", "unknown"),
            "module": adv.get("module_name", "?"),
            "title": adv.get("title", ""),
        }
    return out


def evaluate(prod: dict, everything: dict, exceptions: list[dict], today: dt.date) -> tuple[list[str], list[str]]:
    """(failures, notes) for the two reports under the exceptions in force on `today`."""
    failures: list[str] = []
    notes: list[str] = []
    accepted = {}
    for entry in exceptions:
        ident = entry.get("advisory", "")
        if not entry.get("reason", "").strip():
            failures.append(f"exception {ident or '?'} has no reason")
            continue
        try:
            expires = dt.date.fromisoformat(entry.get("expires", ""))
        except ValueError:
            failures.append(f"exception {ident} has no valid expiry date")
            continue
        if expires < today:
            failures.append(f"exception {ident} expired on {expires}")
            continue
        accepted[ident] = entry
    prod_ids = advisories(prod)
    for ident, adv in sorted(advisories(everything).items()):
        shipped = ident in prod_ids
        line = f"{ident} {adv['severity']} {adv['module']}: {adv['title']}"
        if ident in accepted:
            notes.append(f"accepted until {accepted[ident]['expires']}: {line}")
        elif shipped or adv["severity"] in FAILING_DEV:
            failures.append(("production " if shipped else "development ") + line)
        else:
            notes.append(f"development, reported: {line}")
    return failures, notes


def pnpm_audit(ui: Path, prod: bool) -> dict:
    cmd = ["pnpm", "audit", "--json"] + (["--prod"] if prod else [])
    run = subprocess.run(cmd, cwd=ui, capture_output=True, text=True)
    # pnpm exits non-zero when it finds advisories; a report is what matters.
    try:
        return json.loads(run.stdout)
    except json.JSONDecodeError:
        sys.exit(f"ui-audit: pnpm audit gave no report (exit {run.returncode}): {run.stderr.strip()[:400]}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=".")
    ap.add_argument("--prod-json")
    ap.add_argument("--all-json")
    args = ap.parse_args()
    ui = Path(args.root) / "ui"
    if args.prod_json and args.all_json:
        prod = json.loads(Path(args.prod_json).read_text())
        everything = json.loads(Path(args.all_json).read_text())
    else:
        prod = pnpm_audit(ui, prod=True)
        everything = pnpm_audit(ui, prod=False)
    exceptions_path = ui / "audit-exceptions.json"
    exceptions = json.loads(exceptions_path.read_text()) if exceptions_path.exists() else []
    failures, notes = evaluate(prod, everything, exceptions, dt.date.today())
    for line in notes:
        print(f"ui-audit: {line}")
    for line in failures:
        print(f"ui-audit: {line}", file=sys.stderr)
    if failures:
        return 1
    print("ui-audit: no unaccepted advisory")
    return 0


if __name__ == "__main__":
    sys.exit(main())
