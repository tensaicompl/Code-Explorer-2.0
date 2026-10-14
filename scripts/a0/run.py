#!/usr/bin/env python3
"""A0.1 and A0.2 end to end (decision 64): the structural-accuracy audit of the
Python and Rust goldens, with the PDX repository as a diagnostic corpus.

    run.py --work DIR --scip-python BIN --rust-analyzer BIN --rust-src DIR
           --toolchain DIR --self-sha SHA --resolver TEXT [--out bench/results/a0]

The goldens are read from `bench/repos.lock` at run time (their language, URL and
pinned commit) and their checkouts from `pdx-bench golden`'s cache
(`bench/.cache/repos/<language>/<name>`), which must hold the pinned commit. Every
tree the oracles index is an export of that commit into DIR, never the checkout
itself, and nothing the oracles need is installed anywhere but DIR: the Python
golden's dependencies from its own lock file into a virtual environment, the Rust
dependencies into DIR's own Cargo home. The PDX repository is exported at SHA and
indexed offline from the user's Cargo cache.

Steps for each corpus: the site dataset (`pdx-bench audit-sites`), the oracle's
SCIP index, the audit (`audit.py`), the result document (`report.py`). Then the
sanity fixtures, which must pass before any number is accepted. Every external
command is printed before it runs.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))

import report  # noqa: E402
import score  # noqa: E402


def run(cmd: list[str], *, cwd: Path | None = None, env: dict | None = None,
        stdout=None) -> None:
    print("+", " ".join(str(c) for c in cmd), flush=True)
    subprocess.run([str(c) for c in cmd], cwd=cwd, env=env, check=True, stdout=stdout)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def golden(language: str) -> dict:
    lock = tomllib.loads((ROOT / "bench/repos.lock").read_text())
    entries = [g for g in lock["golden"] if g["language"] == language]
    if len(entries) != 1:
        raise SystemExit(f"bench/repos.lock: {len(entries)} {language} goldens")
    g = entries[0]
    name = g["url"].rstrip("/").split("/")[-1]
    return {"language": language, "url": g["url"], "commit": g["commit"], "name": name,
            "checkout": ROOT / "bench/.cache/repos" / language / name}


def export(repo: Path, commit: str, dest: Path) -> None:
    head = subprocess.run(["git", "-C", str(repo), "rev-parse", "HEAD"], check=True,
                          capture_output=True, text=True).stdout.strip()
    if repo != ROOT and head != commit:
        raise SystemExit(f"{repo}: at {head}, not the pinned {commit}")
    if dest.exists():
        shutil.rmtree(dest)
    dest.mkdir(parents=True)
    archive = subprocess.run(["git", "-C", str(repo), "archive", commit], check=True,
                             capture_output=True).stdout
    subprocess.run(["tar", "-x", "-C", str(dest)], input=archive, check=True)


class Tools:
    def __init__(self, args):
        self.work = args.work.resolve()
        self.scip_python = args.scip_python.resolve()
        self.rust_analyzer = args.rust_analyzer.resolve()
        self.rust_src = args.rust_src.resolve()
        self.toolchain = args.toolchain.resolve()
        self.pdx_bench = ROOT / "target/debug/pdx-bench"

    def rust_env(self, cargo_home: Path | None, offline: bool) -> dict:
        env = {
            "PATH": f"{self.toolchain / 'bin'}:/usr/bin:/bin",
            "HOME": os.environ["HOME"],
            "LD_LIBRARY_PATH": str(self.toolchain / "lib"),
            "RUSTC": str(self.toolchain / "bin/rustc"),
            "CARGO": str(self.toolchain / "bin/cargo"),
            "RUST_SRC_PATH": str(self.rust_src),
            "CARGO_HOME": str(cargo_home or Path(os.environ["HOME"]) / ".cargo"),
            "CARGO_BUILD_JOBS": os.environ.get("CARGO_BUILD_JOBS", "6"),
            "CMAKE_BUILD_PARALLEL_LEVEL": os.environ.get("CMAKE_BUILD_PARALLEL_LEVEL", "6"),
        }
        if offline:
            env["CARGO_NET_OFFLINE"] = "true"
        return env

    def versions(self) -> dict:
        ra = subprocess.run([str(self.rust_analyzer), "--version"], check=True,
                            capture_output=True, text=True,
                            env=self.rust_env(None, True)).stdout.strip()
        sp = subprocess.run([str(self.scip_python), "--version"], check=True,
                            capture_output=True, text=True).stdout.strip()
        pkg = next(json.loads((d / "package.json").read_text())
                   for d in self.scip_python.parents if (d / "package.json").exists())
        lock = next((d / "package-lock.json" for d in self.scip_python.parents
                     if (d / "package-lock.json").exists()), None)
        integrity = None
        if lock is not None:
            entry = json.loads(lock.read_text())["packages"].get(
                "node_modules/@sourcegraph/scip-python", {})
            integrity = entry.get("integrity")
        return {"rust-analyzer": ra, "scip-python": sp,
                "scip-python_package": f"{pkg.get('name')}@{pkg.get('version')}",
                "scip-python_integrity": integrity}


def dataset(tools: Tools, tree: Path, out: Path) -> None:
    run([tools.pdx_bench, "audit-sites", "--root", tree, "--out", out])


def python_oracle(tools: Tools, tree: Path, project: str, version: str, out: Path) -> dict:
    """scip-python over `tree`, with the project's dependencies from its own lock."""
    env = dict(os.environ, UV_CACHE_DIR=str(tools.work / "uv-cache"))
    if (tree / "uv.lock").exists():
        run(["uv", "sync", "--frozen", "--no-dev", "--group", "tests", "--group", "typing",
             "--python", "/usr/bin/python3.12"], cwd=tree, env=env)
    else:
        run(["uv", "venv", "-q", "--python", "/usr/bin/python3.12", ".venv"], cwd=tree, env=env)
    run(["uv", "pip", "install", "-q", "--python", ".venv/bin/python", "pip==25.2"],
        cwd=tree, env=env)
    config = {"pythonVersion": "3.10" if (tree / "uv.lock").exists() else "3.12",
              "include": ["."], "exclude": ["**/.*", "**/__pycache__", "**/node_modules"]}
    (tree / "pyrightconfig.json").write_text(json.dumps(config, indent=2) + "\n")
    path = f"{tree / '.venv/bin'}:{Path(shutil.which('node')).parent}:/usr/bin:/bin"
    run([tools.scip_python, "index", "--project-name", project, "--project-version", version,
         "--output", out, "--quiet"], cwd=tree, env=dict(os.environ, PATH=path))
    return {"pyright_config": config,
            "lock_sha256": sha256(tree / "uv.lock") if (tree / "uv.lock").exists() else None}


def rust_oracle(tools: Tools, workspace: Path, out: Path, cargo_home: Path | None,
                offline: bool) -> dict:
    """rust-analyzer's SCIP over the Cargo workspace at `workspace`, all features."""
    config = tools.work / "rust-analyzer.json"
    config.write_text(json.dumps({"cargo": {"features": "all"}}) + "\n")
    env = tools.rust_env(cargo_home, offline)
    env["CARGO_TARGET_DIR"] = str(tools.work / ("target-" + out.stem))
    if not offline:
        run([env["CARGO"], "fetch", "--locked"], cwd=workspace, env=env)
    run([tools.rust_analyzer, "scip", ".", "--output", out, "--config-path", config],
        cwd=workspace, env=env, stdout=subprocess.DEVNULL)
    return {"features": "all", "cargo_lock_sha256": sha256(workspace / "Cargo.lock")}


def audit_corpus(tools: Tools, corpus: str, language: str, tree: Path, sites: Path,
                 scips: list[str]) -> None:
    cmd = [sys.executable, HERE / "audit.py", "--corpus", corpus, "--language", language,
           "--checkout", tree, "--sites", sites, "--out", tools.work / "out"]
    for s in scips:
        cmd += ["--scip", s]
    env = None
    if language == "rust":
        cmd += ["--rust-analyzer", tools.rust_analyzer]
        env = dict(os.environ, LD_LIBRARY_PATH=str(tools.toolchain / "lib"))
    run(cmd, env=env)


def rust_only(src: Path, dest: Path) -> None:
    """The dataset with only Rust files' sites: every definition is kept, since a typed
    answer may name a definition in a file of another language."""
    rust = set()
    with src.open() as f, dest.open("w") as out:
        for line in f:
            r = json.loads(line)
            if r["t"] in ("header", "file", "def"):
                if r["t"] == "file" and r["language"] == "rust":
                    rust.add(r["path"])
                out.write(line)
            elif r["path"] in rust:
                out.write(line)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--work", required=True, type=Path)
    ap.add_argument("--scip-python", required=True, type=Path)
    ap.add_argument("--rust-analyzer", required=True, type=Path)
    ap.add_argument("--rust-src", required=True, type=Path)
    ap.add_argument("--toolchain", required=True, type=Path)
    ap.add_argument("--self-sha", required=True,
                    help="the commit of the PDX repository indexed as the diagnostic corpus")
    ap.add_argument("--resolver", required=True,
                    help="the resolver measured: the commit whose pdx-bench runs Stages 1 to 3, "
                         "as results should name it")
    ap.add_argument("--out", type=Path, default=ROOT / "bench/results/a0")
    args = ap.parse_args()
    tools = Tools(args)
    tools.work.mkdir(parents=True, exist_ok=True)
    (tools.work / "out").mkdir(exist_ok=True)
    versions = tools.versions()
    print(json.dumps(versions), flush=True)

    # The sanity fixtures first: no number is accepted unless every case passes.
    for language, corpus in (("python", "sanity-python"), ("rust", "sanity-rust")):
        tree = tools.work / corpus
        if tree.exists():
            shutil.rmtree(tree)
        shutil.copytree(ROOT / "bench/a0/sanity" / language, tree)
        sites = tools.work / f"{corpus}.sites.jsonl"
        dataset(tools, tree, sites)
        index = tools.work / f"{corpus}.scip"
        if language == "python":
            python_oracle(tools, tree, "a0-sanity", "0", index)
        else:
            env = tools.rust_env(None, True)
            run([env["CARGO"], "generate-lockfile", "--offline"], cwd=tree, env=env)
            rust_oracle(tools, tree, index, None, True)
        audit_corpus(tools, corpus, language, tree, sites, [str(index)])
        run([sys.executable, HERE / "sanity.py", "--fixture", ROOT / "bench/a0/sanity" / language,
             "--audit", tools.work / "out", "--corpus", corpus])

    provenance_common = {
        "resolver": args.resolver,
        "engine_version": 7,
        "oracle_versions": versions,
    }
    results = {}

    # The Python golden.
    py = golden("python")
    tree = tools.work / "python-golden"
    export(py["checkout"], py["commit"], tree)
    sites = tools.work / "python-golden.sites.jsonl"
    dataset(tools, py["checkout"], sites)
    index = tools.work / "python-golden.scip"
    setup = python_oracle(tools, tree, py["name"], py["commit"], index)
    audit_corpus(tools, "python-golden", "python", tree, sites, [str(index)])
    results["python"] = ("python-golden", sites, tree, {
        **provenance_common, "corpus_url": py["url"], "corpus_commit": py["commit"],
        "oracle": "scip-python", "oracle_setup": setup})

    # The Rust golden: its root workspace, and its examples workspace when it has one.
    rs = golden("rust")
    tree = tools.work / "rust-golden"
    export(rs["checkout"], rs["commit"], tree)
    sites = tools.work / "rust-golden.sites.jsonl"
    dataset(tools, rs["checkout"], sites)
    cargo_home = tools.work / "cargo-home"
    scips = []
    setups = {}
    for sub in ("", "examples"):
        ws = tree / sub
        if not (ws / "Cargo.toml").exists() or (sub and "[workspace]" not in
                                                 (ws / "Cargo.toml").read_text()):
            continue
        index = tools.work / f"rust-golden{'-' + sub if sub else ''}.scip"
        setups[sub or "."] = rust_oracle(tools, ws, index, cargo_home, False)
        scips.append(f"{index}={sub}/" if sub else str(index))
    audit_corpus(tools, "rust-golden", "rust", tree, sites, scips)
    results["rust"] = ("rust-golden", sites, tree, {
        **provenance_common, "corpus_url": rs["url"], "corpus_commit": rs["commit"],
        "oracle": "rust-analyzer scip", "oracle_setup": setups})

    # The PDX repository itself: diagnostic only.
    tree = tools.work / "pdx-self"
    export(ROOT, args.self_sha, tree)
    full = tools.work / "pdx-self.sites.jsonl"
    dataset(tools, tree, full)
    sites = tools.work / "pdx-self.rust.jsonl"
    rust_only(full, sites)
    index = tools.work / "pdx-self.scip"
    setup = rust_oracle(tools, tree, index, None, True)
    audit_corpus(tools, "pdx-self", "rust", tree, sites, [str(index)])
    results["pdx-self"] = ("pdx-self", sites, tree, {
        **provenance_common, "corpus_commit": args.self_sha,
        "oracle": "rust-analyzer scip", "oracle_setup": setup,
        "diagnostic_only": "cannot pass or fail G1b"})

    # The result set.
    args.out.mkdir(parents=True, exist_ok=True)
    mismatches = args.out / "mismatches.jsonl"
    mismatches.unlink(missing_ok=True)
    counterfactuals = {}
    for name, (corpus, sites, tree, provenance) in results.items():
        out = args.out / f"{name}.json"
        run([sys.executable, HERE / "report.py", "--audit", tools.work / "out",
             "--corpus", corpus, "--sites", sites, "--checkout", tree,
             "--provenance", json.dumps(provenance), "--out", out,
             "--mismatches", mismatches,
             "--dataset", tools.work / f"dataset-{name}.jsonl.gz"]
            # The diagnostic corpus's misses are summarised in its document only.
            + (["--false-positives-only"] if name == "pdx-self" else []))
        doc = json.loads(out.read_text())
        counterfactuals[name] = doc["counterfactuals"]
    (args.out / "counterfactuals.json").write_text(json.dumps({
        "policies": score.POLICY_TEXT,
        "metrics_version": score.METRICS_VERSION,
        "note": "A0.2 re-scores A0.1's one dataset per corpus; no resolver behaviour was "
                "changed. pdx-self is diagnostic only.",
        "corpora": counterfactuals}, indent=1, sort_keys=True) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
