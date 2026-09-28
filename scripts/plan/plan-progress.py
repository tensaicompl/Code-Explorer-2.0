#!/usr/bin/env python3
"""Validate docs/plan/STATUS.md and regenerate docs/plan/PROGRESS.md.

Reads the extracted dataset (docs/plan/tasks.json) and the live status table
(docs/plan/STATUS.md). Does not need the plan document, so it runs in CI.

  python3 scripts/plan/plan-progress.py [--root .] [--check]

--check validates only and writes nothing. Exit codes: 0 consistent,
1 validation errors (listed on stderr).
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from datetime import date
from pathlib import Path

VALID = ("todo", "in-progress", "done", "blocked")
ROW_RE = re.compile(r"^\|\s*(?P<id>P\d-\d{2})\s*\|\s*(?P<status>[a-z-]*)\s*\|(?P<rest>.*)\|\s*$")


GATE_ROW = re.compile(r"^\|\s*(?P<id>G\d+a?)\s*\|[^|]*\|\s*(?P<state>[a-z ]+?)\s*\|")


def read_gate_verdicts(path: Path) -> set[str]:
    """Gates whose recorded verdict is passed.

    Read from the gate log rather than inferred from task status: a gate is a
    deliberate act with an evaluation behind it.
    """
    if not path.is_file():
        return set()
    passed: set[str] = set()
    for line in path.read_text().splitlines():
        m = GATE_ROW.match(line)
        if m and m.group("state").strip() == "passed":
            passed.add(m.group("id"))
    return passed


def read_status(path: Path) -> dict[str, dict[str, str]]:
    rows: dict[str, dict[str, str]] = {}
    for line in path.read_text().splitlines():
        m = ROW_RE.match(line)
        if not m:
            continue
        rest = [c.strip() for c in m.group("rest").split("|")]
        rows[m.group("id")] = {
            "status": m.group("status").strip(),
            "commit": rest[0] if rest else "",
            "deviations": rest[1] if len(rest) > 1 else "",
        }
    return rows


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=".")
    ap.add_argument("--check", action="store_true")
    args = ap.parse_args()

    root = Path(args.root).resolve()
    plan_dir = root / "docs/plan"
    data = json.loads((plan_dir / "tasks.json").read_text())
    tasks = data["tasks"]
    gates = data["gates"]
    order = data["execution_order"]
    status = read_status(plan_dir / "STATUS.md")
    errors: list[str] = []
    for t in tasks:
        if t["id"] not in status:
            errors.append(f"{t['id']}: absent from STATUS.md")
    for tid in status:
        if not any(t["id"] == tid for t in tasks):
            errors.append(f"{tid}: in STATUS.md but not in the plan")
    for tid, row in status.items():
        if row["status"] not in VALID:
            errors.append(f"{tid}: status {row['status']!r} is not one of {'/'.join(VALID)}")
        if row["status"] == "done" and not row["commit"]:
            errors.append(f"{tid}: done with no commit recorded")

    done = {tid for tid, r in status.items() if r["status"] == "done"}

    # A gate is passed only when it has been evaluated and the verdict recorded in
    # GATES.md. Finishing the task before a gate is not passing the gate: the whole
    # point of a gate is that someone ran its criteria and wrote down what happened.
    gate_done = read_gate_verdicts(plan_dir / "GATES.md")
    for g in gates:
        if g["id"] in gate_done and g["after"] not in done:
            errors.append(
                f"{g['id']}: recorded as passed, but {g['after']} is not done"
            )
    for t in tasks:
        if status.get(t["id"], {}).get("status") != "done":
            continue
        if t["depends_raw"].strip().lower().startswith("all"):
            outstanding = [x["id"] for x in tasks
                           if x["id"] != t["id"] and x["id"] not in done]
            if outstanding:
                errors.append(f"{t['id']}: done but {len(outstanding)} tasks are not done")
        for dep in t["depends"]:
            if dep.startswith("G"):
                if dep not in gate_done:
                    errors.append(f"{t['id']}: done but gate {dep} is not passed")
            elif dep in status and dep not in done:
                errors.append(f"{t['id']}: done but dependency {dep} is not done")

    issues_text = (plan_dir / "ISSUES.md").read_text() if (plan_dir / "ISSUES.md").exists() else ""
    for tid, row in status.items():
        if row["status"] == "blocked" and tid not in issues_text:
            errors.append(f"{tid}: blocked with no entry in ISSUES.md")

    # A make target standing in for unfinished work must be replaced by the task that
    # owns it; once that task is done, the stand-in is a check that silently skips.
    makefile = plan_dir.parent.parent / "Makefile"
    if makefile.exists():
        for target, owner in re.findall(r"pending-target\.sh\s+(\S+)\s+(P\d-\d{2})", makefile.read_text()):
            if status.get(owner, {}).get("status") == "done":
                errors.append(f"{owner}: done, but make {target} still skips as not yet implemented")

    if errors:
        for e in errors:
            print(f"error: {e}", file=sys.stderr)

    counts = {s: sum(1 for r in status.values() if r["status"] == s) for s in VALID}
    total = len(tasks)
    pct = 100 * counts["done"] // total if total else 0

    def deps_satisfied(t: dict) -> bool:
        # "Depends: all" (P9-07) means every other task must be done.
        if t["depends_raw"].strip().lower().startswith("all"):
            return all(x["id"] in done for x in tasks if x["id"] != t["id"])
        for dep in t["depends"]:
            if dep.startswith("G"):
                if dep not in gate_done:
                    return False
            elif dep not in done:
                return False
        return True

    ready = [t["id"] for t in sorted(tasks, key=lambda x: x["order"])
             if status.get(t["id"], {}).get("status") == "todo" and deps_satisfied(t)]

    out = [
        "# Progress roll-up",
        "",
        f"Generated by `scripts/plan/plan-progress.py` on {date.today().isoformat()} "
        "from `STATUS.md` and `tasks.json`. Do not edit by hand.",
        "",
        f"**{counts['done']} / {total} tasks done ({pct}%)** — "
        f"in-progress {counts['in-progress']} · blocked {counts['blocked']} · todo {counts['todo']}",
        "",
    ]
    if errors:
        out += [f"> Consistency errors: {len(errors)}. Run the script to list them.", ""]

    out += ["## Phases", "", "| Phase | Name | Done | Total | Gate | Gate status |", "|---|---|---|---|---|---|"]
    phase_names = {p["id"]: p["name"] for p in data["phases"]}
    for pid in sorted(phase_names):
        ptasks = [t for t in tasks if t["phase"] == pid]
        pdone = sum(1 for t in ptasks if t["id"] in done)
        pgates = [g["id"] for g in gates if any(t["id"] == g["after"] for t in ptasks)]
        gtxt = ", ".join(pgates) if pgates else "—"
        gstat = ", ".join("passed" if g in gate_done else "pending" for g in pgates) if pgates else "—"
        out.append(f"| {pid} | {phase_names[pid]} | {pdone} | {len(ptasks)} | {gtxt} | {gstat} |")
    out.append("")

    out += ["## Gates", "", "| Gate | After | Prerequisite tasks done | Status |", "|---|---|---|---|"]
    for g in gates:
        idx = order.index(g["after"]) if g["after"] in order else len(order)
        prereq = order[: idx + 1]
        pdone = sum(1 for tid in prereq if tid in done)
        out.append(
            f"| {g['id']} | `{g['after']}` | {pdone}/{len(prereq)} | "
            f"{'passed' if g['id'] in gate_done else 'pending'} |"
        )
    out.append("")

    out += ["## Next actionable", "",
            "Tasks whose dependencies are satisfied, in normative execution order.", ""]
    if ready:
        for tid in ready[:10]:
            t = next(x for x in tasks if x["id"] == tid)
            out.append(f"- `{tid}` ({t['size']}, {t['execution']}) — {t['title']}")
    else:
        out.append("_none — every task is in-progress, blocked or done_")
    out.append("")

    inprog = [tid for tid in order if status.get(tid, {}).get("status") == "in-progress"]
    blocked = [tid for tid in order if status.get(tid, {}).get("status") == "blocked"]
    if inprog:
        out += ["## In progress", ""] + [f"- `{t}`" for t in inprog] + [""]
    if blocked:
        out += ["## Blocked", "", "Each needs a numbered entry in `ISSUES.md`.", ""]
        out += [f"- `{t}`" for t in blocked] + [""]

    devs = [(tid, status[tid]["deviations"]) for tid in order
            if status.get(tid, {}).get("deviations")]
    if devs:
        out += ["## Recorded deviations", ""] + [f"- `{t}` — {d}" for t, d in devs] + [""]

    if not args.check:
        (plan_dir / "PROGRESS.md").write_text("\n".join(out))
        print(f"PROGRESS.md written: {counts['done']}/{total} done, {len(ready)} actionable")
    else:
        print(f"check: {counts['done']}/{total} done, {len(errors)} errors")

    sys.exit(1 if errors else 0)


if __name__ == "__main__":
    main()
