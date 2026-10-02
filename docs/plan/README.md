# Plan tracking

The implementation plan for PDX 2.0 is a single external document. It is **not
committed**: it names the reference repositories that code is vendored or ported
from, and the provenance scanner fails the build on those names anywhere in the
tree except the third-party notices file. This directory holds the trackable
projection of that plan and nothing else.

The plan document itself is never edited to record progress. Progress lives here.

## Files

| File | Kind | Who writes it |
|---|---|---|
| `tasks.json` | derived | `scripts/plan/extract-plan.py` — regenerate, never edit |
| `TASKS.md` | derived | same; the readable task index |
| `amendments.json` | live | corrections the extractor applies to a task's projected text where a later record (a specification change, a decision, the specification itself) supersedes it; each names its record, and the extractor fails if the text it corrects is gone |
| `STATUS.md` | live | the driver, at the end of every task |
| `GATES.md` | live | the driver, when a gate is evaluated |
| `ISSUES.md` | live | anyone who hits a blocker, ambiguity, SCR or third-party problem |
| `DECISIONS.md` | live | the driver, whenever a decision rule is applied |
| `PROGRESS.md` | derived | `scripts/plan/plan-progress.py` — the roll-up |
| `../lint-exceptions.md` | live | whoever adds an inline lint suppression |

Derived files are reproducible from the plan; live files are the record of the
work and are never regenerated. Re-running the extractor leaves them alone.

## The loop

Per task:

1. Branch `task/<task-id>-<slug>` from `main`.
2. Implement exactly that task's deliverables — nothing from a later task.
3. Write the named acceptance tests; they fail first, pass after. `TASKS.md`
   lists the names the plan mandates for each task.
4. `make check` green: format, lint, build with warnings as errors, unit and
   integration tests, licence scan, provenance scan.
5. Mark the row in `STATUS.md`: status, merge commit, any deviation with its
   justification.
6. Commit as `<task-id>: <task title>`, fast-forward onto `main`.

Per phase, and at every session boundary:

```
python3 scripts/plan/plan-progress.py          # regenerate PROGRESS.md, validate
```

The validator fails on: a task missing from `STATUS.md`, an unknown status, a
task marked done with no commit, a task done before its dependencies or its
gate, and a blocked task with no entry in `ISSUES.md`.

At a gate, the driver — and only the driver — evaluates every criterion in
`GATES.md`, pastes the command output verbatim, and records the verdict. A
failing gate halts progression to the next phase.

A new session starts by reading `STATUS.md`, `ISSUES.md`, `DECISIONS.md` and
`PROGRESS.md`. No state lives in the conversation.

## Statuses

`todo`, `in-progress`, `done`, `blocked` — lower case, always. The upper-case
marker words are on the provenance scanner's deny-list, so an upper-cased status
would fail the build.

A blocked task does not stall the plan: record the issue and continue with the
next task that does not depend on it.

## Execution order

`Ord` in `TASKS.md` is the normative cross-phase order, which is **not** phase
order. P7-01…P7-06 run before P8-01…P8-10, then P7-07…P7-12, then P7-14, then
P8-11, then P9. Gate G5 is evaluated after P7-14 and G6 after P8-11, in that
order. There is no P7-13 and no P2-15.

Each task is classified by how it may be executed, from the plan's execution
appendix:

- `single-agent` — never fanned out: shared contracts, driver-only files, or work
  that needs one coherent model of the code.
- `fan-out-eligible` — may be split across subagents by the partition key given
  in `TASKS.md`; one partition per subagent, each in its own worktree.
- `mixed` — the code is driver-only, the fixtures may be fanned out.

Every fan-out task and every gate also needs a verifier: a subagent that did not
write the code re-runs the acceptance tests and checks the deliverables against
the diff before the driver merges. A verifier failure is a task failure.

## Regenerating

```
python3 scripts/plan/extract-plan.py <path-to-plan.md> --root .
python3 scripts/plan/plan-progress.py --root .
```

The first needs the external plan document; the second does not, so it can run in
CI. Both are idempotent.
