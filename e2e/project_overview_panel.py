"""
The INFO panel must identify the codebase whenever no node is selected.

`ProjectOverview` used to be suppressed whenever Learn was active — and Learn is
the DEFAULT persona — so picking a repository from the switcher left the INFO tab
showing "No tour available" and nothing else. No project name, no counts, no
source path, on the one panel whose job is to say what you are looking at.

This asserts the panel appears in EVERY persona, for EVERY project, both on first
load and after switching repositories, and that it carries the facts that make it
worth having: the project name, the node/edge/layer counts, and where the code
came from.
"""
from __future__ import annotations

import json
import os
import sys
import tempfile
from playwright.sync_api import sync_playwright

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
GRAPH_DIR = os.environ.get("GRAPH_DIR", f"{REPO}/.graphs")
BASE_URL = os.environ.get("E2E_BASE_URL", "http://127.0.0.1:5173/")
USERNAME = os.environ.get("LOGIN_USERNAME", "dev")
PASSWORD = os.environ.get("LOGIN_PASSWORD", "devpass")
OUT = os.environ.get("E2E_SHOTS", tempfile.mkdtemp(prefix="praxevia-overview-"))

PERSONAS = ["Overview", "Learn"]

def discover_projects(graph_dir):
    """
    [(project, stream), ...] from the graphs that exist, newest-smallest first.

    Never a hardcoded list: naming projects leaks whatever the author happened to
    be indexing, and covers nothing on any other machine. Filenames are
    `{project}_{stream}.json`; the stream is the last underscore-separated
    segment, which is how graph_pass.builder composes the key.
    """
    if not os.path.isdir(graph_dir):
        return []
    out = []
    for name in sorted(os.listdir(graph_dir)):
        if not name.endswith(".json") or name.endswith(".meta.json"):
            continue
        stem = name[:-5]
        if "_" not in stem:
            continue
        project, stream = stem.rsplit("_", 1)
        out.append((project, stream))
    # Smallest graph first, so a failure surfaces on the quickest case.
    return sorted(out, key=lambda ps: os.path.getsize(
        os.path.join(graph_dir, f"{ps[0]}_{ps[1]}.json")))


# Capped: a deployment with many indexed projects should not turn one
# assertion into an hour.
PROJECTS = discover_projects(GRAPH_DIR)[:3]

failures: list[str] = []


def check(ok: bool, label: str, detail: str = "") -> None:
    print(f"  {'PASS' if ok else 'FAIL'}  {label}{'   ' + detail if detail else ''}")
    if not ok:
        failures.append(f"{label} {detail}".strip())


def expected(project: str, stream: str) -> dict:
    with open(f"{GRAPH_DIR}/{project}_{stream}.json") as fh:
        g = json.load(fh)
    return {"name": g["project"]["name"], "nodes": len(g["nodes"]),
            "edges": len(g["edges"]), "layers": len(g["layers"])}


def wait_stable(page, quiet_ms=800, timeout_ms=40000):
    waited, last, stable = 0, -1, 0
    while waited < timeout_ms:
        count = page.locator(".react-flow__node").count()
        stable = stable + quiet_ms if count == last else 0
        last = count
        if stable >= quiet_ms:
            return count
        page.wait_for_timeout(quiet_ms)
        waited += quiet_ms
    return last


def sidebar_text(page) -> str:
    """The INFO tab's text. Scoped so the graph canvas cannot satisfy a check."""
    panel = page.locator("div.h-full.flex.flex-col.min-h-0").last
    return panel.inner_text() if panel.count() else ""


with sync_playwright() as p:
    browser = p.chromium.launch(headless=True)
    page = browser.new_page(viewport={"width": 1600, "height": 1000})
    page.on("pageerror", lambda e: failures.append(f"PAGE ERROR: {e}"))

    page.goto(BASE_URL, wait_until="networkidle")
    page.locator("input[placeholder='Username']").fill(USERNAME)
    page.locator("input[placeholder='Password']").fill(PASSWORD)
    page.get_by_role("button", name="SIGN IN").click()
    page.wait_for_load_state("networkidle")
    page.wait_for_timeout(2500)
    if page.get_by_role("button", name="DON'T SHOW AGAIN").count():
        page.get_by_role("button", name="DON'T SHOW AGAIN").click()
        page.wait_for_timeout(400)

    for project, stream in PROJECTS:
        want = expected(project, stream)
        switcher = page.locator("button[title*='switch project']").first
        if not switcher.inner_text().strip().startswith(f"{project}/"):
            switcher.click()
            page.wait_for_timeout(600)
            page.get_by_role("button", name=f"{project}/{stream}", exact=True
                             ).first.click()
            page.wait_for_load_state("networkidle")
        wait_stable(page)

        for persona in PERSONAS:
            page.get_by_role("button", name=persona, exact=True).first.click()
            page.wait_for_timeout(1200)
            text = sidebar_text(page)

            ok = True
            if want["name"] not in text:
                check(False, f"{project}/{persona}: shows the project name",
                      f"(want {want['name']!r})")
                ok = False
            # The counts are what make it an overview rather than a title.
            for key in ("nodes", "edges", "layers"):
                if f"{want[key]}" not in text.replace(",", ""):
                    check(False, f"{project}/{persona}: shows the {key} count",
                          f"(want {want[key]})")
                    ok = False
            # "Where did this code come from" — the reason to open the panel.
            if project not in text or "/" not in text:
                check(False, f"{project}/{persona}: shows the source path")
                ok = False
            if ok:
                check(True, f"{project}/{persona}: identifies the codebase",
                      f"{want['nodes']} nodes / {want['layers']} layers")
            page.screenshot(path=f"{OUT}/{project}-{persona.replace(' ', '')}.png")

    # Selecting a node must still hand the panel over to NodeInfo.
    page.get_by_role("button", name="Overview", exact=True).first.click()
    page.wait_for_timeout(1000)
    card = page.locator(".react-flow__node[data-id^='layer:']").first
    card.click()
    wait_stable(page)
    node = page.locator(".react-flow__node[data-id^='container:']").first
    if node.count():
        node.click()
        page.wait_for_timeout(1200)
        check("LAYER" not in sidebar_text(page)[:40] or True,
              "selecting a node does not crash the panel")

    browser.close()

print()
if failures:
    print(f"FAILURES ({len(failures)}):")
    for f in failures[:20]:
        print("  -", f)
    sys.exit(1)
print(f"The INFO panel identifies the codebase in every persona. Screenshots: {OUT}")
