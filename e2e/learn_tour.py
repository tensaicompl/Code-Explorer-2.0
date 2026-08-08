"""
Learn mode must offer a walkthrough, and the walkthrough must go somewhere.

For as long as Learn has existed the panel read "No tour available", under a
hint telling the reader to generate one from their knowledge graph — with
nothing anywhere in the product that could. The builder now derives a tour from
the graph it just built, so this asserts the thing a user does with it: open
Learn, start the tour, walk every step, click a component it names, and land on
that node.

Asserts behaviour, not prose. Titles and wording are the builder's business and
are covered by `graph_pass/tests/test_tour.py`; what matters here is that a tour
exists for every project, that its length matches what was built rather than
whatever the panel felt like rendering, that no step is blank, and that the
pills are live.

Projects come from GRAPH_DIR. Naming one would leak whatever the author happened
to be indexing and would cover nothing anywhere else.
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
OUT = os.environ.get("E2E_SHOTS", tempfile.mkdtemp(prefix="praxevia-tour-"))

failures: list[str] = []


def check(ok: bool, label: str, detail: str = "") -> None:
    print(f"  {'PASS' if ok else 'FAIL'}  {label}{'   ' + detail if detail else ''}")
    if not ok:
        failures.append(f"{label} {detail}".strip())


def discover_projects(graph_dir: str) -> list[tuple[str, str]]:
    """[(project, stream), ...] from the graphs that exist, smallest first."""
    if not os.path.isdir(graph_dir):
        return []
    out = []
    for name in sorted(os.listdir(graph_dir)):
        if not name.endswith(".json") or name.endswith(".meta.json"):
            continue
        stem = name[:-5]
        if "_" not in stem:
            continue
        out.append(tuple(stem.rsplit("_", 1)))
    return sorted(out, key=lambda ps: os.path.getsize(
        os.path.join(graph_dir, f"{ps[0]}_{ps[1]}.json")))


def built_graph(project: str, stream: str) -> tuple[list[dict], dict[str, str]]:
    """The tour as built, and id -> name for the nodes its pills point at."""
    with open(f"{GRAPH_DIR}/{project}_{stream}.json") as fh:
        graph = json.load(fh)
    return graph["tour"], {n["id"]: n["name"] for n in graph["nodes"]}


def wait_stable(page, quiet_ms=800, timeout_ms=40000) -> int:
    """ELK lays out asynchronously; a fixed sleep reads a half-drawn canvas."""
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
    """The active tab's text. Scoped so the canvas cannot satisfy a check."""
    panel = page.locator("div.h-full.flex.flex-col.min-h-0").last
    return panel.inner_text() if panel.count() else ""


def open_tour_tab(page) -> None:
    """
    Select the TOUR tab.

    The tour used to be appended below the project overview inside INFO, and
    ProjectOverview carried a second start button of its own. It is now a tab
    beside INFO and FILES, shown only in Learn, with one way in — so every
    assertion about tour content has to say which tab it is looking at.
    """
    tab = page.get_by_role("button", name="Tour", exact=True)
    if tab.count():
        tab.first.click()
        page.wait_for_timeout(500)


# Capped: a deployment with many projects should not turn one assertion into
# an hour. Smallest first, so a failure surfaces on the quickest case.
PROJECTS = discover_projects(GRAPH_DIR)[:3]

with sync_playwright() as p:
    browser = p.chromium.launch(headless=True)
    page = browser.new_page(viewport={"width": 1600, "height": 1000})
    page.on("pageerror", lambda e: failures.append(f"PAGE ERROR: {e}"))

    page.goto(BASE_URL, wait_until="networkidle")
    page.locator("input[placeholder='Username']").fill(USERNAME)
    page.locator("input[placeholder='Password']").fill(PASSWORD)
    page.get_by_role("button", name="SIGN IN").click()
    page.wait_for_selector(".react-flow__node", timeout=60000)
    page.wait_for_timeout(1500)
    if page.get_by_role("button", name="DON'T SHOW AGAIN").count():
        page.get_by_role("button", name="DON'T SHOW AGAIN").click()
        page.wait_for_timeout(400)

    for project, stream in PROJECTS:
        steps, node_names = built_graph(project, stream)
        switcher = page.locator("button[title*='switch project']").first
        if not switcher.inner_text().strip().startswith(f"{project}/"):
            switcher.click()
            page.wait_for_timeout(600)
            page.get_by_role("button", name=f"{project}/{stream}", exact=True
                             ).first.click()
            page.wait_for_load_state("networkidle")
        wait_stable(page)

        # Learn is the default persona, but say so rather than depend on it.
        page.get_by_role("button", name="Learn", exact=True).first.click()
        page.wait_for_timeout(900)

        check(bool(steps), f"{project}: the builder produced a tour",
              f"({len(steps)} steps)")
        if not steps:
            continue

        # Learn adds a TOUR tab; Overview must not have one.
        check(page.get_by_role("button", name="Tour", exact=True).count() == 1,
              f"{project}: Learn adds the TOUR tab")

        # INFO answers "what am I looking at" and nothing else now. The tour
        # used to be offered twice — once by ProjectOverview's own button and
        # once by the panel stacked beneath it — so assert the duplicate is
        # gone rather than merely that the new one works.
        page.get_by_role("button", name="Info", exact=True).first.click()
        page.wait_for_timeout(500)
        info = sidebar_text(page)
        check("Start Guided Tour" not in info and "Start Tour" not in info,
              f"{project}: INFO no longer offers to start the tour")

        open_tour_tab(page)
        text = sidebar_text(page)
        check("No tour available" not in text,
              f"{project}: Learn no longer says there is no tour")
        check(f"{len(steps)} steps" in text,
              f"{project}: the panel offers every built step",
              f"(built {len(steps)})")

        start = page.get_by_role("button", name="Start Tour")
        if not start.count():
            check(False, f"{project}: the tour can be started", "(no button)")
            continue
        start.click()
        page.wait_for_timeout(700)

        blank, walked = [], 0
        for i, step in enumerate(steps, start=1):
            body = sidebar_text(page)
            if f"{i} / {len(steps)}" not in body:
                check(False, f"{project}: step {i} reports its position",
                      f"(want '{i} / {len(steps)}')")
                break
            if step["title"] not in body:
                check(False, f"{project}: step {i} shows its title",
                      f"({step['title']!r})")
                break
            # A step whose prose failed to render is the empty state again,
            # wearing a title.
            after_title = body.split(step["title"], 1)[1].strip()
            if len(after_title) < 40:
                blank.append(step["title"])
            walked += 1
            if i < len(steps):
                page.get_by_role("button", name="Next").click()
                page.wait_for_timeout(600)

        check(walked == len(steps), f"{project}: every step is reachable",
              f"({walked} of {len(steps)})")
        check(not blank, f"{project}: no step renders empty", str(blank))
        page.screenshot(path=f"{OUT}/{project}-tour.png")

        # The pills are the point: a step names components, and clicking one
        # has to select it. Walk back to the first step that has any.
        with_pills = next((n for n, s in enumerate(steps, start=1)
                           if s["nodeIds"]), None)
        if with_pills:
            page.get_by_role("button", name="Exit Tour").click()
            page.wait_for_timeout(500)
            page.get_by_role("button", name="Start Tour").click()
            page.wait_for_timeout(600)
            for _ in range(with_pills - 1):
                page.get_by_role("button", name="Next").click()
                page.wait_for_timeout(500)

            step = steps[with_pills - 1]
            # The pill carries the node's NAME, which is not the tail of its id
            # — a module node is named for its directory, not its last segment.
            wanted_id = step["nodeIds"][0]
            wanted = node_names.get(wanted_id, wanted_id)
            target = page.get_by_role("button", name=wanted, exact=True).first
            if target.count():
                target.click()
                page.wait_for_timeout(900)
                # Selecting a node switches to INFO — that is the pill's whole
                # purpose, and NodeInfo lives there. The tour is not lost: it
                # stays on its own tab, on the same step.
                check(page.get_by_role("heading", name=wanted).count() > 0,
                      f"{project}: a referenced component opens its node",
                      f"({wanted!r})")
                open_tour_tab(page)
                check(f"{with_pills} / {len(steps)}" in sidebar_text(page),
                      f"{project}: the tour resumes on the step it was on",
                      f"(step {with_pills})")
            else:
                check(False, f"{project}: the step's components are clickable",
                      f"({wanted!r} not found)")
            open_tour_tab(page)
            page.get_by_role("button", name="Exit Tour").click()
            page.wait_for_timeout(400)

    browser.close()

print()
if failures:
    print(f"FAILURES ({len(failures)}):")
    for f in failures:
        print("  -", f)
    sys.exit(1)
print(f"Learn mode walks a real tour. Screenshots: {OUT}")
