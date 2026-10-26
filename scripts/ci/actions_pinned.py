#!/usr/bin/env python3
"""Every action a workflow uses is pinned by commit digest (P0-07, issue 100).

A tag or a branch can be moved to other code after it was reviewed; a commit digest
cannot. Each `uses:` must name a full 40-hex commit, followed by a comment with the
release it is (`# v4.4.0`), so that an update is a reviewed change of both. Local
actions (`./...`) and container actions pinned by `@sha256:` digest are allowed.

    python3 scripts/ci/actions_pinned.py [--root .]

Exit 0 when every reference is pinned; 1 with one line per offending reference.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

USES = re.compile(r"^\s*(?:-\s+)?uses:\s*(?P<ref>\S+)(?P<rest>.*)$")
PINNED = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_./-]+@[0-9a-f]{40}$")
CONTAINER = re.compile(r"^docker://\S+@sha256:[0-9a-f]{64}$")
RELEASE_COMMENT = re.compile(r"^\s+#\s*v\d+(?:\.\d+)*\S*\s*$")


def problems(text: str, name: str) -> list[str]:
    """The unpinned references in one workflow's text, as `name:line: reason` strings."""
    out = []
    for number, line in enumerate(text.splitlines(), 1):
        m = USES.match(line)
        if not m:
            continue
        ref = m.group("ref").strip("'\"")
        rest = m.group("rest")
        if ref.startswith("./"):
            continue
        if CONTAINER.match(ref):
            continue
        if not PINNED.match(ref):
            out.append(f"{name}:{number}: {ref} is not pinned by a 40-hex commit digest")
        elif not RELEASE_COMMENT.match(rest):
            out.append(f"{name}:{number}: {ref} has no `# v<release>` comment naming what it pins")
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=".")
    args = ap.parse_args()
    workflows = sorted((Path(args.root) / ".github" / "workflows").glob("*.y*ml"))
    if not workflows:
        print("actions-pinned: no workflows found", file=sys.stderr)
        return 1
    found = []
    for path in workflows:
        found += problems(path.read_text(), path.name)
    for line in found:
        print(f"actions-pinned: {line}", file=sys.stderr)
    if found:
        return 1
    print(f"actions-pinned: every action pinned by digest ({len(workflows)} workflows)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
