#!/usr/bin/env python3
"""
Start the backend and the dashboard, run an end-to-end script against them, stop both.

    ../.venv/bin/python e2e/run.py e2e/chip_clickthrough.py

Self-contained on purpose: the browser tests need a real backend serving real
graphs, and a test you cannot run without remembering six environment variables
is a test nobody runs.

DEFAULTS MATCH, and every one is overridable from the environment.
`ANTHROPIC_API_KEY` is deliberately NOT among them — nothing here exercises the
chat, and a browser test must not be able to spend money.

Ports are 8099 / 5173, as If either is already busy this exits rather
than attaching, because attaching to somebody else's server is how a test passes
against the wrong build.
"""
from __future__ import annotations

import argparse
import os
import signal
import socket
import subprocess
import sys
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

BACKEND_PORT = int(os.environ.get("E2E_BACKEND_PORT", "8099"))
FRONTEND_PORT = int(os.environ.get("E2E_FRONTEND_PORT", "5173"))
STARTUP_TIMEOUT = int(os.environ.get("E2E_STARTUP_TIMEOUT", "90"))

# Everything resolves RELATIVE TO THE REPOSITORY, or comes from the environment.
# There is no path here belonging to whoever happened to write these tests: an
# absolute path baked into a default is a test that passes on one laptop and
# fails everywhere else, silently, by testing nothing.
#
# `--codebase` / `--indexer-dir` on the command line beat the environment, which
# beats these repo-relative defaults. `.env` is loaded first if present, so the
# ordinary case is that you configure the app once and the tests follow.
DEFAULTS = {
    "GRAPH_DIR": f"{REPO}/.graphs",
    "CODEBASE_DIR": f"{REPO}/codebase",
    "DATABASE_URL": "postgresql://prx@127.0.0.1:5434/prx",
    # Test-only credentials. The backend refuses to start without them, and it
    # is bound to localhost for the length of one test run.
    "LOGIN_USERNAME": "e2e",
    "LOGIN_PASSWORD": "e2e-local-only",
    # Where the indexer is invoked from — only used if a test triggers a
    # rebuild. Defaults to this repository, which holds indexer/.
    "INDEXER_DIR": REPO,
}


def load_dotenv(path: str) -> dict:
    """
    Minimal `.env` reader — no dependency, and it must not surprise anyone.

    Only `KEY=value` lines outside comments. Values are NOT expanded and quotes
    are stripped only when they wrap the whole value.
    """
    out: dict[str, str] = {}
    if not os.path.isfile(path):
        return out
    with open(path, encoding="utf-8") as fh:
        for raw in fh:
            line = raw.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            key, _, value = line.partition("=")
            key, value = key.strip(), value.strip()
            if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
                value = value[1:-1]
            if key and value:
                out[key] = value
    return out

ENV: dict = {}

VENV_PY = f"{REPO}/.venv/bin/python"
VITE = f"{REPO}/frontend/packages/dashboard/node_modules/.bin/vite"

SERVERS = [
    (
        "backend",
        [VENV_PY, "-m", "uvicorn", "app.main:app", "--port", str(BACKEND_PORT)],
        f"{REPO}/backend",
        BACKEND_PORT,
    ),
    (
        # --no-open, or every run pops a browser window on the developer's desktop.
        "dashboard",
        [VITE, "--no-open", "--port", str(FRONTEND_PORT)],
        f"{REPO}/frontend/packages/dashboard",
        FRONTEND_PORT,
    ),
]


def port_open(port: int) -> bool:
    with socket.socket() as s:
        s.settimeout(0.4)
        return s.connect_ex(("127.0.0.1", port)) == 0


def preflight(env: dict) -> None:
    missing = [p for p in (VENV_PY, VITE) if not os.path.exists(p)]
    if missing:
        sys.exit(f"missing: {missing}\n"
                 f"  venv:  python3 -m venv .venv && .venv/bin/pip install -r "
                 f"backend/requirements.txt playwright\n"
                 f"  vite:  cd frontend && pnpm install && pnpm build:core")
    if not os.path.isdir(env["CODEBASE_DIR"]):
        sys.exit(f"CODEBASE_DIR does not exist: {env['CODEBASE_DIR']}\n"
                 f"  Point it at the directory holding your source trees as\n"
                 f"  {{project}}/{{stream}}/ subdirectories. Any of:\n"
                 f"    --codebase /path/to/codebase\n"
                 f"    CODEBASE_DIR=/path/to/codebase {sys.argv[0]} …\n"
                 f"    CODEBASE_DIR=… in {REPO}/.env  (copy .env.example)")
    if not os.path.isdir(env["GRAPH_DIR"]) or not os.listdir(env["GRAPH_DIR"]):
        sys.exit(f"no graphs in {env['GRAPH_DIR']} — build one first:\n"
                 f"  cd indexer && DATABASE_URL=… GRAPH_DIR=$PWD/../.graphs \\\n"
                 f"    ../.venv/bin/python -m graph_pass.builder <project> <stream>\n"
                 f"  or set GRAPH_DIR / --graphs to where they already are.")
    for name, _cmd, _cwd, port in SERVERS:
        if port_open(port):
            sys.exit(f"port {port} is already in use — stop the {name} you have "
                     f"running, or set E2E_{name.upper()}_PORT. Refusing to test "
                     f"against a server this script did not start.")


def build_env(args: argparse.Namespace) -> dict:
    """
    Resolve configuration: CLI > process environment > .env > repo-relative default.

    Paths are made absolute, because the servers run with their own cwd and a
    relative path would resolve differently for each of them.
    """
    dotenv = load_dotenv(os.path.join(REPO, ".env"))
    resolved = {}
    for key, fallback in DEFAULTS.items():
        resolved[key] = os.environ.get(key) or dotenv.get(key) or fallback
    if args.codebase:
        resolved["CODEBASE_DIR"] = args.codebase
    if args.graphs:
        resolved["GRAPH_DIR"] = args.graphs
    if args.indexer_dir:
        resolved["INDEXER_DIR"] = args.indexer_dir
    for key in ("CODEBASE_DIR", "GRAPH_DIR", "INDEXER_DIR"):
        resolved[key] = os.path.abspath(os.path.expanduser(resolved[key]))
    return {**os.environ, **dotenv, **resolved}


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Start the backend and dashboard, run a script, stop both.",
        epilog="Config precedence: these flags > environment > .env > "
               "repo-relative defaults.")
    parser.add_argument("--codebase", metavar="DIR",
                        help="directory holding {project}/{stream}/ source trees "
                             "(default: <repo>/codebase)")
    parser.add_argument("--graphs", metavar="DIR",
                        help="where built graphs live (default: <repo>/.graphs)")
    parser.add_argument("--indexer-dir", metavar="DIR",
                        help="working directory for indexer invocations "
                             "(default: the repository root)")
    parser.add_argument("script", help="the e2e script to run")
    parser.add_argument("script_args", nargs=argparse.REMAINDER,
                        help="arguments passed through to the script")
    args = parser.parse_args()

    global ENV
    ENV = build_env(args)
    preflight(ENV)

    procs = []
    try:
        for name, cmd, cwd, port in SERVERS:
            print(f"  starting {name} on {port}…", flush=True)
            procs.append(subprocess.Popen(cmd, cwd=cwd, env=ENV,
                                          stdout=subprocess.DEVNULL,
                                          stderr=subprocess.STDOUT,
                                          start_new_session=True))
            deadline = time.time() + STARTUP_TIMEOUT
            while not port_open(port):
                if time.time() > deadline:
                    return f"  {name} did not open port {port} in {STARTUP_TIMEOUT}s"
                if procs[-1].poll() is not None:
                    return f"  {name} exited with {procs[-1].returncode}"
                time.sleep(0.5)
        print("  servers ready\n", flush=True)
        return subprocess.call([VENV_PY, args.script, *args.script_args],
                               env=ENV)
    finally:
        for p in procs:
            try:
                # The whole group: vite spawns children that outlive the parent
                # and keep 5173 bound, so the next run's preflight would refuse.
                os.killpg(os.getpgid(p.pid), signal.SIGTERM)
            except (ProcessLookupError, PermissionError):
                pass
        for p in procs:
            try:
                p.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(os.getpgid(p.pid), signal.SIGKILL)


if __name__ == "__main__":
    result = main()
    if isinstance(result, str):
        print(result, file=sys.stderr)
        raise SystemExit(1)
    raise SystemExit(result)
