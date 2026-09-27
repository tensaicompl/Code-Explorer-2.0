"""
Put a node of every type on the canvas and confirm it actually draws.

`chip_clickthrough.py` asserts on Overview CARD COUNTS, which can be perfectly
right while the nodes themselves fail to render. That is not a hypothetical here: records React Flow silently dropping every edge whose node lacked a
`<Handle>` — 1 of 130 edges mounted while the overview looked fine. A count is
not a pixel.

So this drills in, expands the clusters, and asserts that a `document:`,
`config:`, `service:`, `endpoint:` and `table:` node is each present in the DOM,
visible, non-zero-sized, and showing its own name rather than an empty box.
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
OUT = os.environ.get("E2E_SHOTS", tempfile.mkdtemp(prefix="praxevia-e2e-"))

TYPES = ["config", "document", "service", "endpoint", "table"]


def find_cases(graph_dir):
    """
    [(project, stream, layer_id, [types])] — the SMALLEST layer containing each
    type, discovered from the graphs that exist.

    Never a hardcoded project or layer: naming them leaks whatever the author
    was indexing, and a fixture that does not exist elsewhere makes this file
    silently assert nothing.
    """
    best = {}
    for name in sorted(os.listdir(graph_dir)) if os.path.isdir(graph_dir) else []:
        if not name.endswith(".json") or name.endswith(".meta.json"):
            continue
        project, stream = name[:-5].rsplit("_", 1)
        with open(os.path.join(graph_dir, name)) as fh:
            g = json.load(fh)
        by_id = {n["id"]: n for n in g["nodes"]}
        for layer in g["layers"]:
            size = len(layer["nodeIds"])
            present = {by_id[i]["type"] for i in layer["nodeIds"] if i in by_id}
            for t in TYPES:
                if t in present and (t not in best or size < best[t][0]):
                    best[t] = (size, project, stream, layer["id"])
    grouped = {}
    for t, (_size, project, stream, layer_id) in best.items():
        grouped.setdefault((project, stream, layer_id), []).append(t)
    return [(p, st, lid, sorted(ts)) for (p, st, lid), ts in grouped.items()]


CASES = find_cases(GRAPH_DIR)

failures: list[str] = []


def nodes_of_type(project, stream, layer_id, node_type):
    with open(f"{GRAPH_DIR}/{project}_{stream}.json") as fh:
        g = json.load(fh)
    by_id = {n["id"]: n for n in g["nodes"]}
    layer = next(l for l in g["layers"] if l["id"] == layer_id)
    return [by_id[i] for i in layer["nodeIds"] if by_id[i]["type"] == node_type]


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


def sign_in(page):
    page.goto(BASE_URL, wait_until="networkidle")
    page.locator("input[placeholder='Username']").fill(USERNAME)
    page.locator("input[placeholder='Password']").fill(PASSWORD)
    page.get_by_role("button", name="SIGN IN").click()
    page.wait_for_load_state("networkidle")
    page.wait_for_timeout(2500)
    if page.get_by_role("button", name="DON'T SHOW AGAIN").count():
        page.get_by_role("button", name="DON'T SHOW AGAIN").click()
        page.wait_for_timeout(400)


def switch_project(page, project, stream):
    current = page.locator("button[title*='switch project']").first
    if not current.inner_text().strip().startswith(f"{project}/"):
        current.click()
        page.wait_for_timeout(600)
        page.get_by_role("button", name=f"{project}/{stream}", exact=True).first.click()
        page.wait_for_load_state("networkidle")
    crumb = page.get_by_role("button", name="Project", exact=True)
    if crumb.count():
        crumb.first.click()
    wait_stable(page)


def drill_and_expand(page, layer_id):
    """Drill into the layer, then open every collapsed cluster inside it."""
    card = page.locator(f".react-flow__node[data-id='{layer_id}']")
    card.wait_for(state="visible", timeout=25000)
    card.click()
    wait_stable(page)
    # Containers arrive collapsed ("Cluster A · 5 items · click to open"); the
    # nodes only exist in the DOM once opened.
    for _ in range(6):
        clusters = page.locator(".react-flow__node[data-id^='container:']").all()
        if not clusters:
            break
        opened = False
        for c in clusters:
            if "click to open" in (c.inner_text() or ""):
                c.click()
                wait_stable(page)
                opened = True
        if not opened:
            break


with sync_playwright() as p:
    browser = p.chromium.launch(headless=True)
    page = browser.new_page(viewport={"width": 1600, "height": 1000})
    page.on("pageerror", lambda e: failures.append(f"PAGE ERROR: {e}"))
    sign_in(page)

    for project, stream, layer_id, types in CASES:
        switch_project(page, project, stream)
        drill_and_expand(page, layer_id)
        page.screenshot(path=f"{OUT}/render-{project}-{layer_id.split(':')[1]}.png")

        for node_type in types:
            wanted = nodes_of_type(project, stream, layer_id, node_type)
            if not wanted:
                failures.append(f"{project} {layer_id}: no {node_type} in the graph")
                continue
            drawn = []
            for node in wanted:
                loc = page.locator(
                    f".react-flow__node[data-id={json.dumps(node['id'])}]")
                if not loc.count() or not loc.first.is_visible():
                    continue
                box = loc.first.bounding_box()
                text = (loc.first.inner_text() or "")
                # The card truncates a long name with an ellipsis, so compare a
                # prefix. A markdown heading is routinely longer than the box.
                head = node["name"][:12]
                if not box or box["width"] < 10 or box["height"] < 10:
                    failures.append(f"{project} {node['id']}: mounted with no size")
                elif head not in text:
                    failures.append(
                        f"{project} {node['id']}: box drawn but its name "
                        f"({head!r}…) is not in {text[:60]!r}")
                else:
                    drawn.append(node)

            ok = bool(drawn)
            if not ok:
                failures.append(
                    f"{project} {layer_id}: NONE of the {len(wanted)} "
                    f"{node_type} nodes rendered")
            example = drawn[0]["name"] if drawn else "-"
            print(f"  {'PASS' if ok else 'FAIL'}  {project:5} {layer_id:16} "
                  f"{node_type:9} {len(drawn):3}/{len(wanted):3} drawn   "
                  f"e.g. {example[:40]!r}")

    browser.close()

print()
if failures:
    print(f"FAILURES ({len(failures)}):")
    for f in failures[:20]:
        print("  -", f)
    sys.exit(1)
print(f"Every node type renders. Screenshots: {OUT}")
