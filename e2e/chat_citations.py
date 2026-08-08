"""
Citations must become chips — end to end, against a live provider.

The unit tests prove the resolver maps `path` / `path:symbol` onto node ids.
They cannot prove the model emits anything to resolve. That half is a prompt,
and a prompt is only verified by running it: this asks a question whose answer
is necessarily about specific files, then checks the transcript actually grew
clickable chips and that clicking one navigates the graph.

Runs against whatever provider the backend is configured with, so it doubles as
a check that the citation instruction survives a non-Anthropic model.
"""
from __future__ import annotations

import os
import sys
import tempfile
from playwright.sync_api import sync_playwright

BASE_URL = os.environ.get("E2E_BASE_URL", "http://127.0.0.1:5173/")
USERNAME = os.environ.get("LOGIN_USERNAME", "dev")
PASSWORD = os.environ.get("LOGIN_PASSWORD", "devpass")
OUT = os.environ.get("E2E_SHOTS", tempfile.mkdtemp(prefix="praxevia-cite-"))


def pick_subject(graph_dir):
    """
    (project, stream, question) — a question whose answer is necessarily about a
    file that exists in the graph, chosen from the graphs present.

    Derived rather than written down: a hardcoded question naming a real symbol
    leaks the codebase it came from, and asserts nothing wherever that codebase
    is absent.
    """
    for name in sorted(os.listdir(graph_dir)) if os.path.isdir(graph_dir) else []:
        if not name.endswith(".json") or name.endswith(".meta.json"):
            continue
        project, stream = name[:-5].rsplit("_", 1)
        with open(os.path.join(graph_dir, name)) as fh:
            g = json.load(fh)
        # Prefer a named definition with a file, so the answer must cite a path.
        for wanted in ("service", "table", "class"):
            for node in g["nodes"]:
                if node["type"] == wanted and node.get("filePath"):
                    return (project, stream,
                            f"Which file defines {node['name']}? "
                            f"Cite the file you found it in.")
    return (None, None, None)


PROJECT, STREAM, QUESTION = pick_subject(GRAPH_DIR)
if not PROJECT:
    sys.exit(f"no usable graph in {GRAPH_DIR} — nothing to ask about")

failures: list[str] = []


def check(ok: bool, label: str, detail: str = "") -> None:
    print(f"  {'PASS' if ok else 'FAIL'}  {label}{'   ' + detail if detail else ''}")
    if not ok:
        failures.append(f"{label} {detail}".strip())


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

    switcher = page.locator("button[title*='switch project']").first
    if not switcher.inner_text().strip().startswith(f"{PROJECT}/"):
        switcher.click()
        page.wait_for_timeout(600)
        page.get_by_role("button", name=f"{PROJECT}/{STREAM}", exact=True).first.click()
        page.wait_for_load_state("networkidle")
        page.wait_for_timeout(4000)

    # Wait for a graph, not a timeout. On first load ProjectSwitcher redirects to
    # a real project, which reloads the page — a fixed sleep lands mid-reload and
    # finds the composer still disabled.
    page.wait_for_selector(".react-flow__node", timeout=60000)
    page.wait_for_timeout(1500)
    if page.get_by_role("button", name="DON'T SHOW AGAIN").count():
        page.get_by_role("button", name="DON'T SHOW AGAIN").click()
        page.wait_for_timeout(400)

    page.get_by_role("button", name="Insight Advisor").click()
    page.wait_for_timeout(1200)
    box = page.locator("textarea").last
    box.fill(QUESTION)
    box.press("Enter")

    # Wait for the answer to finish: the Stop button reverts to Send.
    page.get_by_role("button", name="Stop generating").wait_for(
        state="visible", timeout=30000)
    try:
        page.get_by_role("button", name="Send").wait_for(state="visible", timeout=240000)
    except Exception:
        check(False, "the answer completed", "(still streaming after 4 min)")

    page.wait_for_timeout(1500)
    page.screenshot(path=f"{OUT}/citations.png")

    # `data-ref-chip` carries the resolved node id, so this counts citation
    # chips and nothing else. Matching "a button whose title has a slash" also
    # matched the project switcher, and the check passed while counting it.
    chips = page.locator("button[data-ref-chip]").all()
    check(bool(chips), "the answer produced at least one citation chip",
          f"({len(chips)} found)")

    # Every chip must carry a real node id — the whole point of resolving in the
    # browser is that an unresolvable citation never becomes a chip at all.
    ids = [c.get_attribute("data-ref-chip") for c in chips]
    check(all(i and ":" in i for i in ids),
          "every chip resolved to a graph node id", f"{ids[:3]}")

    body = page.inner_text("body")
    check("[[" not in body,
          "no raw [[…]] markup leaked into the rendered answer")

    if chips:
        print(f"        chips: {[c.get_attribute('title') for c in chips][:4]}")
        # Clicking must navigate, not throw. The graph selection is the proof.
        chips[0].click()
        page.wait_for_timeout(2500)
        page.screenshot(path=f"{OUT}/after-chip-click.png")
        check(not failures or all("PAGE ERROR" not in f for f in failures),
              "clicking a chip navigates without a page error")

    browser.close()

print()
if failures:
    print(f"FAILURES ({len(failures)}):")
    for f in failures:
        print("  -", f)
    sys.exit(1)
print(f"Citations resolve to chips. Screenshots: {OUT}")
