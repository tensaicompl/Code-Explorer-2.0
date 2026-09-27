#!/usr/bin/env python3
"""Extract the trackable plan dataset from the (uncommitted) implementation plan.

The plan document itself is delivered outside the repository and is never
committed: it names reference repositories, which the provenance scanner
forbids inside the tree. This script reads it from an external path and emits
only provenance-free tracking artefacts under docs/plan/.

Usage:
  python3 scripts/plan/extract-plan.py <path-to-plan.md> [--root .]

Emits (overwriting):
  docs/plan/tasks.json   authoritative extracted dataset (ids, deps, order, tests)
  docs/plan/TASKS.md     human-readable task index

Live files (STATUS.md, GATES.md, ISSUES.md, DECISIONS.md, lint-exceptions.md) are
created once if absent and never overwritten: they carry the record of the work.
  docs/plan/TASKS.md     human-readable task index

"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

TASK_RE = re.compile(
    r"^\*\*(?P<id>P\d-\d{2})\s+—\s+(?P<title>.+?)\.\*\*\s+"
    r"Depends:\s*(?P<depends>.*?)\.\s+Size\s+(?P<size>[SML])\.",
    re.S,
)
PHASE_RE = re.compile(r"^### Phase (?P<id>P\d) — (?P<name>.+)$")
TASK_START_RE = re.compile(r"^\*\*P\d-\d{2}\s+—")
GATE_ROW_RE = re.compile(r"^\|\s*\*\*(?P<id>G\d+a?)\*\*\s*\|\s*(?P<after>.*?)\s*\|\s*(?P<crit>.*?)\s*\|\s*$")

# Normative cross-phase execution order (Part 6 preamble).
ORDER_SPEC = [
    ("P0", None), ("P1", None), ("P2", None), ("P3", None), ("P4", None),
    ("P5", None), ("P6", None),
    ("P7", (1, 6)), ("P8", (1, 10)), ("P7", (7, 12)), ("P7", (14, 14)),
    ("P8", (11, 11)), ("P9", None),
]

GATE_AFTER = {
    "G0": "P0-06", "G1a": "P1-07", "G1": "P2-16", "G2": "P3-09", "G3": "P4-14",
    "G4": "P6-08", "G5": "P7-14", "G6": "P8-11", "G7": "P9-07",
}


# The provenance deny-list lives in a data file, not in this source file: a
# literal reference name here would itself fail the provenance scan. Prefer the
# scanner's own list once P0-04 has created it.
DENYLIST_CANDIDATES = (
    "scripts/provenance-denylist.txt",
    "scripts/plan/redactions.txt",
)


def load_denylist(root: Path) -> list[str]:
    for rel in DENYLIST_CANDIDATES:
        path = root / rel
        if path.exists():
            return [
                line.strip()
                for line in path.read_text().splitlines()
                if line.strip() and not line.lstrip().startswith("#")
            ]
    sys.exit("no provenance deny-list found: " + " or ".join(DENYLIST_CANDIDATES))


REDACTION_MARK = "(attribution details in the plan)"
_redaction_re: re.Pattern[str] | None = None
_redaction_count = 0


def init_redaction(root: Path) -> None:
    global _redaction_re
    _redaction_re = re.compile("|".join(load_denylist(root)), re.I)


def redact(text: str) -> str:
    """Remove reference identifiers from prose bound for the tree.

    A reference is usually named inside a parenthetical aside, so the whole aside
    is dropped rather than leaving a hole in the middle of a sentence. Anything
    outside parentheses falls back to a token-level replacement.
    """
    global _redaction_count
    if _redaction_re is None:
        raise RuntimeError("init_redaction() must run before redact()")
    out = text

    def drop_aside(m: re.Match[str]) -> str:
        global _redaction_count
        if _redaction_re.search(m.group(0)):
            _redaction_count += 1
            return REDACTION_MARK
        return m.group(0)

    # Innermost-first, so nested asides collapse into the outer one.
    for _ in range(3):
        out, n = re.subn(r"\(([^()]*)\)", drop_aside, out)
        if not n:
            break
    out, n = _redaction_re.subn("the upstream reference named in the plan", out)
    _redaction_count += n
    return out

def slice_between(text: str, start: str, end: str) -> str:
    a = text.index(start)
    b = text.index(end, a + len(start))
    return text[a:b]


def expand_depends(raw: str) -> list[str]:
    """Return concrete task/gate ids from a Depends: string."""
    out: list[str] = []
    for lo, hi in re.findall(r"(P\d-\d{2})\.\.(P\d-\d{2})", raw):
        phase = lo.split("-")[0]
        for n in range(int(lo.split("-")[1]), int(hi.split("-")[1]) + 1):
            out.append(f"{phase}-{n:02d}")
    stripped = re.sub(r"P\d-\d{2}\.\.P\d-\d{2}", "", raw)
    out += re.findall(r"P\d-\d{2}", stripped)
    out += re.findall(r"\bG\d+a?\b", stripped)
    seen: list[str] = []
    for item in out:
        if item not in seen:
            seen.append(item)
    return seen


def acceptance_tests(text: str) -> list[str]:
    """Identifier-shaped backticked tokens are the test names the plan mandates."""
    names = []
    for tok in re.findall(r"`([^`]+)`", text):
        if re.fullmatch(r"[a-z0-9]+(?:_[a-z0-9]+)+", tok):
            if tok not in names:
                names.append(tok)
    return names


def first_sentence(text: str, limit: int = 240) -> str:
    flat = " ".join(redact(text).split())
    flat = re.sub(r"\s+", " ", flat)
    if len(flat) <= limit:
        return flat
    return flat[: limit - 1].rstrip() + "…"


def parse_tasks(plan: str) -> tuple[list[dict], dict[str, str]]:
    part6 = slice_between(plan, "## Part 6 — Work breakdown", "## Part 7 — Gates")
    lines = part6.splitlines()

    phases: dict[str, str] = {}
    blocks: list[tuple[str, list[str]]] = []
    phase = None
    current: list[str] | None = None

    for line in lines:
        m = PHASE_RE.match(line)
        if m:
            phases[m.group("id")] = m.group("name").strip()
            phase = m.group("id")
            current = None
            continue
        if TASK_START_RE.match(line):
            current = [line]
            blocks.append((phase, current))
            continue
        if line.startswith("**Gate") or line.startswith("### ") or line.startswith("---"):
            current = None
            continue
        if current is not None:
            current.append(line)

    tasks: list[dict] = []
    for phase, block in blocks:
        body = "\n".join(block)
        m = TASK_RE.match(body)
        if not m:
            sys.exit(f"unparsed task block: {block[0][:80]}")
        deliv = re.search(r"Deliverables[^:]*:(?P<d>.*?)(?:\nAcceptance:|$)", body, re.S)
        accept = re.search(r"Acceptance:(?P<a>.*)$", body, re.S)
        deliv_text = deliv.group("d").strip() if deliv else ""
        accept_text = accept.group("a").strip() if accept else ""
        tasks.append(
            {
                "id": m.group("id"),
                "phase": phase,
                "title": m.group("title").strip(),
                "size": m.group("size"),
                "depends_raw": m.group("depends").strip(),
                "depends": expand_depends(m.group("depends")),
                "deliverables": first_sentence(deliv_text, 400),
                "acceptance": first_sentence(accept_text, 400),
                "acceptance_tests": acceptance_tests(accept_text),
            }
        )
    return tasks, phases


def parse_gates(plan: str) -> list[dict]:
    part7 = slice_between(plan, "## Part 7 — Gates", "## Part 8 — Decision table")
    gates = []
    for line in part7.splitlines():
        m = GATE_ROW_RE.match(line)
        if not m:
            continue
        gid = m.group("id").replace(" ", "")
        crit = m.group("crit").strip()
        parts = [redact(p.strip()) for p in re.split(r";\s+", crit) if p.strip()]
        gates.append(
            {
                "id": gid,
                "after": GATE_AFTER.get(gid, m.group("after").strip()),
                "after_raw": m.group("after").strip(),
                "criteria": parts,
            }
        )
    return gates


def parse_fanout(plan: str) -> tuple[set[str], set[str], set[str]]:
    appj = plan[plan.index("## Appendix J"):]
    k3 = slice_between(appj, "### K.3", "### K.4")
    k4 = appj[appj.index("### K.4"):]
    k4 = k4[: k4.index("### K.5")]
    # K.3 is a table: only the first column names the task. Ids in the Notes
    # column are prerequisites ("after P7-03 is merged"), not fan-out targets.
    k3_task_col = "\n".join(
        line.split("|")[1] for line in k3.splitlines()
        if line.startswith("|") and len(line.split("|")) > 2
    )
    fan: set[str] = set()
    for lo, hi in re.findall(r"(P\d-\d{2})\s*…\s*(P\d-\d{2})", k3_task_col):
        for n in range(int(lo.split("-")[1]), int(hi.split("-")[1]) + 1):
            fan.add(f"{lo.split('-')[0]}-{n:02d}")
    fan |= set(re.findall(r"P\d-\d{2}", re.sub(r"P\d-\d{2}\s*…\s*P\d-\d{2}", "", k3_task_col)))
    single: set[str] = set()
    for lo, hi in re.findall(r"(P\d-\d{2})\s*…\s*(P\d-\d{2})", k4):
        for n in range(int(lo.split("-")[1]), int(hi.split("-")[1]) + 1):
            single.add(f"{lo.split('-')[0]}-{n:02d}")
    single |= set(re.findall(r"P\d-\d{2}", re.sub(r"P\d-\d{2}\s*…\s*P\d-\d{2}", "", k4)))
    # K.4 names one phase in prose ("all of P4") rather than by task id.
    whole_phases = set(re.findall(r"all of (P\d)\b", k4))
    return fan, single, whole_phases


def execution_order(tasks: list[dict]) -> list[str]:
    by_phase: dict[str, list[str]] = {}
    for t in tasks:
        by_phase.setdefault(t["phase"], []).append(t["id"])
    order: list[str] = []
    for phase, rng in ORDER_SPEC:
        ids = by_phase.get(phase, [])
        if rng is None:
            picked = ids
        else:
            lo, hi = rng
            picked = [i for i in ids if lo <= int(i.split("-")[1]) <= hi]
        for i in picked:
            if i not in order:
                order.append(i)
    missing = [t["id"] for t in tasks if t["id"] not in order]
    if missing:
        sys.exit(f"tasks absent from the normative order: {missing}")
    return order


def write_tasks_json(root: Path, tasks: list[dict], phases: dict[str, str], gates: list[dict],
                     order: list[str], fan: set[str], single: set[str],
                     whole_phases: set[str], plan_name: str) -> None:
    for t in tasks:
        t["order"] = order.index(t["id"]) + 1
        is_single = t["id"] in single or t["phase"] in whole_phases
        if t["id"] in fan and is_single:
            # K.3 and K.4 both name it: the code is driver-only, the fixtures fan out.
            t["execution"] = "mixed"
        elif t["id"] in fan:
            t["execution"] = "fan-out-eligible"
        elif is_single:
            t["execution"] = "single-agent"
        else:
            t["execution"] = "unclassified"
        t["gate_after"] = next((g["id"] for g in gates if g["after"] == t["id"]), None)
    payload = {
        "source_document": plan_name,
        "source_committed": False,
        "task_count": len(tasks),
        "phases": [{"id": p, "name": n} for p, n in sorted(phases.items())],
        "gates": gates,
        "execution_order": order,
        "tasks": sorted(tasks, key=lambda t: t["order"]),
    }
    (root / "docs/plan/tasks.json").write_text(json.dumps(payload, indent=2) + "\n")


def write_tasks_md(root: Path, tasks: list[dict], phases: dict[str, str], gates: list[dict],
                   plan_name: str) -> None:
    by_order = sorted(tasks, key=lambda t: t["order"])
    out = [
        "# Task index — PDX 2.0",
        "",
        f"Derived from `{plan_name}` (Part 6, Part 7, Appendix J) by",
        "`scripts/plan/extract-plan.py`. Read-only: regenerate rather than edit.",
        "Live status lives in `STATUS.md`; gate results in `GATES.md`.",
        "",
        f"{len(tasks)} tasks in {len(phases)} phases, {len(gates)} gates.",
        "",
        "`Ord` is the position in the normative cross-phase execution order, which is",
        "not phase order: P7-01…P7-06 run before P8-01…P8-10, then P7-07…P7-12, P7-14,",
        "then P8-11, then P9.",
        "",
    ]
    for pid in sorted(phases):
        ptasks = [t for t in by_order if t["phase"] == pid]
        sizes = {s: sum(1 for t in ptasks if t["size"] == s) for s in "SML"}
        out += [
            f"## {pid} — {phases[pid]}",
            "",
            f"{len(ptasks)} tasks (S {sizes['S']} · M {sizes['M']} · L {sizes['L']})",
            "",
            "| Ord | Task | Title | Size | Depends | Execution | Tests |",
            "|---|---|---|---|---|---|---|",
        ]
        for t in ptasks:
            dep = ", ".join(t["depends"]) if t["depends"] else "—"
            out.append(
                f"| {t['order']} | `{t['id']}` | {t['title']} | {t['size']} | {dep} "
                f"| {t['execution']} | {len(t['acceptance_tests'])} |"
            )
        out.append("")
        for t in ptasks:
            out += [f"### {t['id']} — {t['title']}", ""]
            out.append(f"- **Size** {t['size']} · **Depends** {t['depends_raw'] or '—'} "
                       f"· **Order** {t['order']} · **Execution** {t['execution']}")
            if t["gate_after"]:
                out.append(f"- **Gate** {t['gate_after']} is evaluated after this task")
            out.append(f"- **Deliverables** {t['deliverables']}")
            out.append(f"- **Acceptance** {t['acceptance']}")
            if t["acceptance_tests"]:
                out.append("- **Named tests** " + ", ".join(f"`{n}`" for n in t["acceptance_tests"]))
            out.append("")
    (root / "docs/plan/TASKS.md").write_text("\n".join(out))


def write_gates_md(root: Path, gates: list[dict], plan_name: str) -> bool:
    """Live file: gate verdicts are recorded in it, so never overwrite."""
    path = root / "docs/plan/GATES.md"
    if path.exists():
        return False
    out = [
        "# Gate evaluations",
        "",
        f"Criteria derived from `{plan_name}` Part 7 by `scripts/plan/extract-plan.py`.",
        "A gate is evaluated by the driver only, after the task named in *After*.",
        "Every criterion must hold; a failing gate halts progression to the next phase.",
        "",
        "Record each evaluation below the gate's checklist with the command output",
        "pasted verbatim, the date, and the resulting verdict.",
        "",
        "Performance thresholds in G1 and G4 missed by ≤ 2 points are recorded with the",
        "actual number and an SCR; a larger miss halts. Correctness and security floors",
        "are never redefined — G7 blocks the release until they hold.",
        "",
        "| Gate | After | Status | Evaluated |",
        "|---|---|---|---|",
    ]
    for g in gates:
        out.append(f"| {g['id']} | `{g['after']}` | pending | |")
    out.append("")
    for g in gates:
        out += [f"## {g['id']} — after `{g['after']}`", "", "Status: **pending**", "", "Criteria:", ""]
        for c in g["criteria"]:
            out.append(f"- [ ] {c}")
        out += ["", "Evaluation log:", "", "_not yet evaluated_", ""]
    path.write_text("\n".join(out))
    return True


def write_status_md(root: Path, tasks: list[dict]) -> bool:
    path = root / "docs/plan/STATUS.md"
    if path.exists():
        return False
    out = [
        "# Implementation status",
        "",
        "One row per task of the implementation plan, in normative execution order.",
        "Statuses: `todo`, `in-progress`, `done`, `blocked`. Keep them lowercase — the",
        "provenance scanner fails the build on the upper-case marker strings.",
        "",
        "Updated by the driver at the end of every task, per the task protocol: mark the",
        "task, record the merge commit, and note any deviation with its justification.",
        "A blocked task needs a numbered entry in `ISSUES.md`; a task that applied a",
        "decision rule needs one in `DECISIONS.md`.",
        "",
        "Regenerate the roll-up after editing: `python3 scripts/plan/plan-progress.py`",
        "",
        "| Task | Status | Commit | Deviations |",
        "|---|---|---|---|",
    ]
    for t in sorted(tasks, key=lambda t: t["order"]):
        out.append(f"| {t['id']} | todo | | |")
    out.append("")
    path.write_text("\n".join(out))
    return True


def parse_decisions(plan: str) -> list[tuple[str, str, str]]:
    part8 = slice_between(plan, "## Part 8 — Decision table", "## Appendix A")
    rows = []
    for line in part8.splitlines():
        cells = [c.strip() for c in line.split("|")]
        if len(cells) >= 5 and re.fullmatch(r"D\d+", cells[1]):
            rows.append((cells[1], cells[2], cells[3]))
    rows.sort(key=lambda r: int(r[0][1:]))
    return rows


def write_decisions_md(root: Path, decisions: list[tuple[str, str, str]], plan_name: str) -> bool:
    path = root / "docs/plan/DECISIONS.md"
    if path.exists():
        return False
    out = [
        "# Decisions",
        "",
        "One line per decision taken under the plan's decision table, with the rule",
        "applied. Append only; never rewrite an earlier entry. A decision that changes a",
        "normative specification is not a decision — it is a specification change request",
        "and belongs in `ISSUES.md`.",
        "",
        "| # | Date | Task | Rule | Decision taken |",
        "|---|---|---|---|---|",
        "",
        f"## Rule reference (extracted from `{plan_name}` Part 8)",
        "",
        "When something is not specified, apply the first matching rule, record it above,",
        "and continue.",
        "",
        "| Rule | Situation | Decision |",
        "|---|---|---|",
    ]
    for rid, situation, decision in decisions:
        out.append(f"| {rid} | {situation} | {decision} |")
    out.append("")
    path.write_text("\n".join(out))
    return True


def write_issues_md(root: Path) -> bool:
    path = root / "docs/plan/ISSUES.md"
    if path.exists():
        return False
    path.write_text("\n".join([
        "# Issues",
        "",
        "Numbered log of blockers, ambiguities, specification change requests and",
        "third-party problems. Unfinished work is recorded here with a task reference,",
        "never as a marker comment in code — the provenance scanner fails the build on",
        "those markers.",
        "",
        "Open an entry when: an acceptance test cannot pass without changing a normative",
        "specification; a required third-party component is unavailable, unlicensed for",
        "our allow-list, or fails the provenance scan; or a gate fails. Then continue with",
        "the next task that does not depend on the halted one.",
        "",
        "Types: `blocker`, `ambiguity`, `scr` (specification change request),",
        "`third-party`. States: `open`, `resolved`, `withdrawn`.",
        "",
        "| # | Date | Task | Type | State | Summary | Resolution |",
        "|---|---|---|---|---|---|---|",
        "",
        "## Entries",
        "",
        "_none_",
        "",
    ]))
    return True


def write_lint_exceptions_md(root: Path) -> bool:
    path = root / "docs/lint-exceptions.md"
    if path.exists():
        return False
    path.write_text("\n".join([
        "# Lint exceptions",
        "",
        "Every inline lint suppression in the tree, with its justification. A suppression",
        "that is not listed here is a build failure; a suppression without a one-line",
        "justification on the same line as the code is also a failure.",
        "",
        "| File | Line | Rule suppressed | Justification |",
        "|---|---|---|---|",
        "",
        "_none_",
        "",
    ]))
    return True


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("plan")
    ap.add_argument("--root", default=".")
    args = ap.parse_args()

    root = Path(args.root).resolve()
    plan_path = Path(args.plan)
    init_redaction(root)
    plan = plan_path.read_text(encoding="utf-8")
    plan_name = plan_path.name

    tasks, phases = parse_tasks(plan)
    gates = parse_gates(plan)
    fan, single, whole_phases = parse_fanout(plan)
    order = execution_order(tasks)

    write_tasks_json(root, tasks, phases, gates, order, fan, single, whole_phases, plan_name)
    write_tasks_md(root, tasks, phases, gates, plan_name)
    fresh = write_status_md(root, tasks)
    made = [name for name, created in (
        ("GATES.md", write_gates_md(root, gates, plan_name)),
        ("DECISIONS.md", write_decisions_md(root, parse_decisions(plan), plan_name)),
        ("ISSUES.md", write_issues_md(root)),
        ("lint-exceptions.md", write_lint_exceptions_md(root)),
    ) if created]

    print(f"tasks: {len(tasks)}  phases: {len(phases)}  gates: {len(gates)}")
    print(f"provenance redactions applied: {_redaction_count}")
    counts: dict[str, int] = {}
    for t in tasks:
        counts[t["execution"]] = counts.get(t["execution"], 0) + 1
    print("execution classes: " + ", ".join(f"{k} {v}" for k, v in sorted(counts.items())))
    print("STATUS.md: created" if fresh else "STATUS.md: left untouched (live file)")
    print("live files created: " + (", ".join(made) if made else "none (all present)"))


if __name__ == "__main__":
    main()
