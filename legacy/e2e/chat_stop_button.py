"""
The chat's stop button must actually stop the answer.

A tool-using agent can spend a minute on one question, and the composer is
blocked for that whole minute — so a mistyped question used to cost a minute
before anything else could be asked. The send button becomes Stop while an
answer streams; this checks that it does, that clicking it ends the run, and
that the composer is usable immediately afterwards.

Asserts the behaviour, not the pixels: label and title flip, the request is
aborted (the reply carries the `Stopped` marker), `loading` clears, and a
follow-up question can be sent straight away.
"""
from __future__ import annotations

import os
import sys
import tempfile
from playwright.sync_api import sync_playwright

BASE_URL = os.environ.get("E2E_BASE_URL", "http://127.0.0.1:5173/")
USERNAME = os.environ.get("LOGIN_USERNAME", "dev")
PASSWORD = os.environ.get("LOGIN_PASSWORD", "devpass")
OUT = os.environ.get("E2E_SHOTS", tempfile.mkdtemp(prefix="praxevia-stop-"))

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

    send = page.get_by_role("button", name="Send")
    check(send.count() == 1, "idle: the button is Send")

    box = page.locator("textarea").last
    # Deliberately expensive: it must still be streaming when Stop is clicked.
    box.fill("List every function in this codebase and explain each one in detail.")
    box.press("Enter")

    # Wait for the button to become Stop rather than sleeping a fixed time.
    stop = page.get_by_role("button", name="Stop generating")
    try:
        stop.wait_for(state="visible", timeout=20000)
        check(True, "streaming: the button became Stop")
    except Exception:
        check(False, "streaming: the button became Stop", "(never appeared)")
        browser.close()
        sys.exit(1)

    check(stop.get_attribute("title") == "Stop generating (Esc)",
          "streaming: Stop advertises the Esc shortcut",
          f"title={stop.get_attribute('title')!r}")
    page.screenshot(path=f"{OUT}/streaming.png")

    page.wait_for_timeout(2500)   # let some answer arrive, so a partial survives
    stop.click()

    # The run must end promptly — the point of the feature is not waiting.
    back_to_send = page.get_by_role("button", name="Send")
    try:
        back_to_send.wait_for(state="visible", timeout=8000)
        check(True, "after Stop: the button is Send again")
    except Exception:
        check(False, "after Stop: the button is Send again", "(still Stop)")

    page.screenshot(path=f"{OUT}/stopped.png")
    body = page.inner_text("body")
    check("Stopped" in body, "after Stop: the transcript says it was stopped")

    # The whole point: ask again immediately.
    box.fill("Say OK.")
    box.press("Enter")
    try:
        page.get_by_role("button", name="Stop generating").wait_for(
            state="visible", timeout=20000)
        check(True, "a follow-up question can be sent right away")
    except Exception:
        check(False, "a follow-up question can be sent right away",
              "(composer still blocked)")
    # Leave nothing running behind us.
    if page.get_by_role("button", name="Stop generating").count():
        page.get_by_role("button", name="Stop generating").click()
        page.wait_for_timeout(1500)

    browser.close()

print()
if failures:
    print(f"FAILURES ({len(failures)}):")
    for f in failures:
        print("  -", f)
    sys.exit(1)
print(f"Stop button works. Screenshots: {OUT}")
