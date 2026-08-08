"""
Click every toolbar category chip in a real browser and check what leaves the canvas.

Plan recorded the per-chip counts as "verified structurally (layer membership
+ the two frontend maps), NOT by clicking the chips in a browser". This closes that
gap. For every project and every chip it reads each Overview layer card's node
count, clicks the chip off, reads the counts again, and asserts the drop is EXACTLY
the number of nodes in that layer whose type maps to that category — then clicks it
back on and asserts the counts return.

Expectations come from .graphs/*.json at run time, so this compares the running UI
against the builder's own output rather than against numbers typed by hand.

WHY OVERVIEW AND NOT A DRILL-IN. The layer view renders collapsed containers
("Cluster A · 5 items"), so individual node ids are only in the DOM once a cluster
is expanded, and re-deriving containers after a filter change collapses them again.
The Overview card count is the same filtered node set, stated as a number, without
that moving part.
"""
import json
import os
import re
import sys
import tempfile
from playwright.sync_api import sync_playwright

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
GRAPH_DIR = os.environ.get("GRAPH_DIR", f"{REPO}/.graphs")
BASE_URL = os.environ.get("E2E_BASE_URL", "http://127.0.0.1:5173/")
USERNAME = os.environ.get("LOGIN_USERNAME", "dev")
PASSWORD = os.environ.get("LOGIN_PASSWORD", "devpass")
# Screenshots are evidence, not output: keep them out of the repo.
OUT = os.environ.get("E2E_SHOTS", tempfile.mkdtemp(prefix="praxevia-e2e-"))

# Mirrors NODE_TYPE_TO_CATEGORY, GraphView.tsx:82.
CATEGORY = {
    **{t: "CODE" for t in ("file", "function", "class", "module", "concept")},
    "config": "CONFIG",
    "document": "DOCS",
    **{t: "INFRA" for t in ("service", "resource", "pipeline")},
    **{t: "DATA" for t in ("table", "endpoint", "schema")},
    **{t: "DOMAIN" for t in ("domain", "flow", "step")},
}
CHIPS = ["CODE", "CONFIG", "DOCS", "INFRA", "DATA"]

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


PROJECTS = discover_projects(GRAPH_DIR)

failures: list[str] = []
checked = 0


def graph(project, stream):
    with open(f"{GRAPH_DIR}/{project}_{stream}.json") as fh:
        return json.load(fh)


def expected_counts(project, stream):
    """{layer_id: {chip: n}} straight from the built graph."""
    g = graph(project, stream)
    by_id = {n["id"]: n for n in g["nodes"]}
    out = {}
    for layer in g["layers"]:
        per = dict.fromkeys(CHIPS, 0)
        for nid in layer["nodeIds"]:
            cat = CATEGORY.get(by_id[nid]["type"])
            if cat in per:
                per[cat] += 1
        out[layer["id"]] = per
    return out


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


def wait_stable(page, quiet_ms=800, timeout_ms=40000):
    """
    Block until the canvas stops changing.

    ELK lays out asynchronously and React Flow mounts as it goes, so a fixed
    sleep reads a half-drawn canvas — which is how an earlier version of this
    script "measured" 5 of 13 nodes and called it a failure.
    """
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


def switch_project(page, project, stream):
    current = page.locator("button[title*='switch project']").first
    if not current.inner_text().strip().startswith(f"{project}/"):
        current.click()
        page.wait_for_timeout(600)
        page.get_by_role("button", name=f"{project}/{stream}", exact=True).first.click()
        page.wait_for_load_state("networkidle")
    # A stale drill-in would make every count below belong to the wrong view.
    crumb = page.get_by_role("button", name="Project", exact=True)
    if crumb.count():
        crumb.first.click()
    wait_stable(page)


def layer_counts(page):
    """{layer_id: node count} as the Overview cards report it right now."""
    counts = {}
    for n in page.locator(".react-flow__node").all():
        nid = n.get_attribute("data-id") or ""
        if not nid.startswith("layer:"):
            continue
        # `files?` — LayerClusterNode.tsx:87 pluralises, so a layer filtered down
        # to one node reads "1 file". Matching only "files" made that layer look
        # like it had vanished, which is a bug in this script, not in the app.
        m = re.search(r"(\d[\d\s,]*)\s+files?\b", n.inner_text() or "")
        if m:
            counts[nid] = int(re.sub(r"[^\d]", "", m.group(1)))
    return counts


def chip(page, name):
    """
    The toolbar chip for a category.

    Keyed on the title attribute: the button's text is a colour dot plus a label,
    and its title flips between "Hide X nodes" and "Show X nodes" as it toggles.
    """
    n = name.title()
    return page.locator(
        f"button[title='Hide {n} nodes'], button[title='Show {n} nodes']"
    ).first


if not PROJECTS:
    sys.exit(f"no graphs in {GRAPH_DIR} — nothing to click through")

with sync_playwright() as p:
    browser = p.chromium.launch(headless=True)
    page = browser.new_page(viewport={"width": 1600, "height": 1000})
    page.on("pageerror", lambda e: failures.append(f"PAGE ERROR: {e}"))
    sign_in(page)

    for project, stream in PROJECTS:
        switch_project(page, project, stream)
        want = expected_counts(project, stream)
        base = layer_counts(page)
        print(f"\n{project}: {len(base)} layer cards, {sum(base.values())} nodes")

        # The card counts must agree with the built graph before anything is
        # clicked, or every delta below is measured from the wrong baseline.
        for layer_id, n in base.items():
            total = sum(want.get(layer_id, {}).values())
            if n != total:
                failures.append(f"{project} {layer_id}: baseline card says {n}, "
                                f"graph says {total}")

        for name in CHIPS:
            title_before = chip(page, name).get_attribute("title")
            chip(page, name).click()
            wait_stable(page)
            title_after = chip(page, name).get_attribute("title")
            hidden = layer_counts(page)
            page.screenshot(path=f"{OUT}/chip-{project}-{name}.png")
            chip(page, name).click()
            wait_stable(page)
            restored = layer_counts(page)

            ok = True
            total_expected = sum(want[l].get(name, 0) for l in base)
            total_dropped = sum(base[l] - hidden.get(l, 0) for l in base)

            for layer_id, before in base.items():
                # A layer emptied by the filter loses its card entirely
                # (visibleLayers drops empty layers, GraphView.tsx:283-298).
                dropped = before - hidden.get(layer_id, 0)
                if dropped != want[layer_id].get(name, 0):
                    failures.append(
                        f"{project} {layer_id} {name}: card dropped {dropped}, "
                        f"graph says {want[layer_id].get(name, 0)}")
                    ok = False
            if restored != base:
                failures.append(f"{project} {name}: toggling back did not restore "
                                f"({restored} vs {base})")
                ok = False
            if (title_before, title_after) != (f"Hide {name.title()} nodes",
                                               f"Show {name.title()} nodes"):
                failures.append(f"{project} {name}: chip title went "
                                f"{title_before!r} -> {title_after!r}")
                ok = False

            checked += 1
            verdict = "PASS" if ok else "FAIL"
            note = "" if total_expected else "   (nothing of this type in this project)"
            print(f"  {verdict}  {name:7} hid {total_dropped:6} of "
                  f"{sum(base.values()):6}   graph says {total_expected:6}{note}")

    browser.close()

print()
if failures:
    print(f"FAILURES ({len(failures)}):")
    for f in failures[:30]:
        print("  -", f)
    sys.exit(1)
print(f"All {checked} chip click-throughs passed across {len(PROJECTS)} "
      f"projects. Screenshots: {OUT}")
