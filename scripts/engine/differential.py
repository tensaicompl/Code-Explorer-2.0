#!/usr/bin/env python3
"""The engine differential (P2-16, decision 50): the vendored engine against the
pinned, unmodified reference engine, row for row, on the same inputs.

    differential.py run --pdx-bench PATH [--dump PATH] [--corpus NAME]...
        the live differential, what make engine-differential runs (nightly only)
    differential.py canonical --project NAME <rows.jsonl>
        one side's rows, canonical, on standard output
    differential.py compare --reference ROWS --reference-project NAME
                            --vendored ROWS --vendored-project NAME
        two sides' rows compared, exit 1 when they differ

The corpora are bench/engine-differential.toml's: every typed-resolution fixture of
engine/tests/fixtures/resolve, and a bounded subset of every typed-tier golden
repository at its pinned commit. `pdx-bench differential-corpora` checks the manifest
against the language matrix and the lock, fetches the goldens as the golden harness
does, and hands the corpora over; nothing here fetches a repository.

For each corpus, both engines read the same bytes, staged outside the repository:

  - the vendored engine through engine/tests/differential_dump (built by make engine),
    which prints every row engine/tests/differential_rows.h defines after its
    cross-file typed pass, and each file's cross-file surface from a second run in
    which every file comes from its exported surface;
  - the reference engine through a build of its pinned checkout with an observation
    hook linked in (scripts/engine/differential_hook.c, its names and the rows
    header's translated back with the vendoring's own rename map, inverted), which
    prints the same rows when its calls pass starts; its surfaces are the ones it
    stores, read from its database, read-only.

Each side's rows are reduced to one canonical form (canonical()) and the two compared
as multisets, category by category: definitions, calls (call sites and callable
references), imports, surfaces and typed resolutions, and each file's own facts.
Canonicalisation removes only the project name the engine wraps every qualified name
in, and adds the interface's kind beside each engine label (engine/api/pdxe.c's
mapping, read from that file). Every other value is compared as written: positions,
callers, callees, multiplicity, scores (as the float's own nine digits). Any
difference fails, and there is no way to accept one.

The reference is configured, never edited: its process environment is emptied of its
own variables, it runs with an empty home and a configuration directory of the
harness's, in which every compared file's extension is given the language this
project's matrix gives the file, so both engines read every file in one language.
The comparison is of the explicit engine contract: the bytes, the language, the
repository-relative path and the metadata, every one given by an absolute path. Files
around an engine are no input, so each engine runs, for each corpus, from a working
directory of the harness's own, created empty for that run alone, outside the stage, the
corpus and the repository, and removed afterwards (decision 52). P2-16 ran both in the
staged corpus root, which is how issue 83 was found: the C and C++ preprocessing of each
read the files a source includes relative to the working directory. Since engine 7 ours
reads none; the reference still would, and from an empty directory finds none.
"""

import argparse
import collections
import contextlib
import hashlib
import json
import os
import pathlib
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile

REPO = pathlib.Path(__file__).resolve().parents[2]
RUN_DATA = REPO / "scripts/vendor/reference-run.txt"
RENAME_MAP = REPO / "scripts/vendor/rename-map.txt"
REFERENCES_LOCK = REPO / "bench/references.lock"
INTERFACE = REPO / "engine/api/pdxe.c"
ROWS_HEADER = REPO / "engine/tests/differential_rows.h"
HOOK_SOURCE = REPO / "scripts/engine/differential_hook.c"
DEFAULT_DUMP = REPO / "target/engine/tests/differential_dump"
EVIDENCE = REPO / "bench/.cache/engine-differential"
ENGINE_REF = "engine-ref"

# How many rows of a difference are shown each way, and how long each may be.
SHOW = 20
LINE = 400

# The reference's textual strategies (scripts/engine/typed-differential.py): a graph
# edge with one of these came from resolving by name, never from typed resolution, so
# none may appear among the typed answers compared here.
TEXTUAL = {
    "import_map", "import_map_suffix", "qualified_suffix", "same_module", "suffix_match",
    "unique_name", "fuzzy", "callee_suffix", "field_type_hint", "service_pattern",
}

# The categories, as reported, and the rows each holds.
CATEGORIES = {
    "files": ("file",),
    "definitions": ("definition",),
    "calls": ("call", "reference"),
    "imports": ("import",),
    "surfaces": ("surface", "surface_file"),
    "typed": ("typed",),
}

# Every field each raw row carries, with its type, on both sides.
S, B, I, N, L = "string", "bool", "int", "number", "list"
SCHEMA = {
    "file": {"file": S, "module_qn": S, "namespace_name": S, "has_error": B,
             "is_test_file": B, "lsp_skipped": B, "walk_truncated": B, "exports": L},
    "definition": {
        "file": S, "label": S, "name": S, "qn": S, "def_file": S, "start_line": I,
        "end_line": I, "signature": S, "return_type": S, "receiver": S, "docstring": S,
        "parent_class": S, "decorators": L, "base_classes": L, "param_names": L,
        "param_types": L, "signature_param_types": L, "signature_param_count": I,
        "return_types": L, "route_path": S, "route_method": S, "complexity": I,
        "cognitive": I, "loop_count": I, "loop_depth": I, "is_recursive": B,
        "param_count": I, "max_access_depth": I, "linear_scan_in_loop": I,
        "alloc_in_loop": I, "recursion_in_loop": B, "unguarded_recursion": B, "lines": I,
        "is_exported": B, "is_abstract": B, "is_test": B, "is_entry_point": B,
        "body_tokens": S, "impl_trait": S},
    "call": {
        "file": S, "caller": S, "callee": S, "line": I, "start": I, "end": I, "origin": I,
        "first_string_arg": S, "second_arg_name": S, "args": L, "arg_count": I,
        "loop_depth": I, "branch_depth": I, "is_method": B, "requires_lsp_resolution": B,
        "callee_is_locally_bound": B, "receiver_is_self_attribute": B},
    "reference": {
        "file": S, "caller": S, "name": S, "start": I, "end": I, "origin": I, "usage_kind": I,
        "may_be_call_reference": B, "semantic_reference_blocked": B,
        "semantic_reference_local_shadow": B, "is_member_access": B},
    "import": {"file": S, "local_name": S, "module_path": S},
    "typed": {"file": S, "caller": S, "callee": S, "strategy": S, "confidence": N,
              "reason": S, "kind": I, "start": I, "end": I, "origin": I},
    "surface": {"file": S, "json": S},
    "cached_typed": {"file": S, "callee": S, "strategy": S, "reason": S, "kind": I,
                     "start": I, "end": I},
    "project": {"name": S, "files": I},
}
# Fields that may be null.
NULLABLE = {S, L}
# Rows only one side writes: the vendored dump's cached-run answers, and the hook's
# project line.
VENDORED_ONLY = {"cached_typed"}
REFERENCE_ONLY = {"project"}


class Failure(Exception):
    """A run that cannot be compared, or a difference: never a skip."""


def fail(message):
    raise Failure(message)


# --- data the repository already holds -------------------------------------------


def run_field(key):
    for line in RUN_DATA.read_text().splitlines():
        if line.strip() and not line.startswith("#"):
            k, _, v = line.partition("\t")
            if k == key:
                return v
    fail(f"{RUN_DATA.name} has no {key}")


def reference_pin():
    import tomllib
    with open(REFERENCES_LOCK, "rb") as fh:
        for entry in tomllib.load(fh).get("reference", []):
            if entry.get("name") == ENGINE_REF:
                return entry["commit"]
    fail(f"{REFERENCES_LOCK.name} pins no {ENGINE_REF}")


def kind_map(source=None):
    """The interface's kind for each engine label, read from normalise_kind in
    engine/api/pdxe.c, so the harness can never map a label otherwise than the
    interface does; and the kind of a label with no entry."""
    text = (source if source is not None else INTERFACE.read_text())
    body = re.search(r"static const char \*normalise_kind\(const char \*label\) \{(.*?)\n\}",
                     text, re.S)
    if not body:
        fail("engine/api/pdxe.c has no normalise_kind")
    pairs = re.findall(r'\{"([A-Za-z]+)", "([a-z_]+)"\}', body.group(1))
    default = re.findall(r'return "([a-z_]+)";', body.group(1))
    if not pairs or not default or len(set(default)) != 1:
        fail("normalise_kind in engine/api/pdxe.c is not in the shape this harness reads")
    table = dict(pairs)
    if len(table) != len(pairs):
        fail("normalise_kind maps a label twice")
    return table, default[0]


def vendored_project(source=None):
    """The project name the interface computes every qualified name under."""
    text = (source if source is not None else INTERFACE.read_text())
    found = re.search(r'static const char PROJECT_SENTINEL\[\] = "([^"]+)";', text)
    if not found:
        fail("engine/api/pdxe.c has no PROJECT_SENTINEL")
    return found.group(1)


def inverse_rename():
    """Our names back to the reference's: the vendoring's rename map, inverted. The map
    is one to one, so the inverse is too."""
    inverse = {}
    for line in RENAME_MAP.read_text().splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        theirs, _, ours = line.partition("\t")
        if not ours or ours in inverse:
            fail(f"{RENAME_MAP.name} is not a one-to-one map at {ours!r}")
        inverse[ours] = theirs
    return inverse


def translate(text, inverse):
    """A source written against the vendored engine, with every identifier the vendoring
    renamed given its reference name back, and the core header its reference name."""
    core = inverse.get("pdxe")
    if not core:
        fail("the rename map does not name the core prefix")
    text = text.replace('#include "pdxe_core.h"', f'#include "{core}.h"')
    pattern = re.compile(r"\b(" + "|".join(
        re.escape(t) for t in sorted(inverse, key=len, reverse=True)) + r")\b")
    return pattern.sub(lambda m: inverse[m.group(1)], text)


# --- rows --------------------------------------------------------------------------


def parse_rows(text, side):
    """One side's raw rows, every line checked against SCHEMA: an unknown category, an
    unknown or missing field, a value of the wrong type or a malformed line fails."""
    rows = []
    for number, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            fail(f"{side}: line {number} is empty")
        try:
            row = json.loads(line, parse_float=str, parse_constant=_constant)
        except ValueError as e:
            fail(f"{side}: line {number} is not a row: {e}")
        if not isinstance(row, dict) or not isinstance(row.get("c"), str):
            fail(f"{side}: line {number} has no category")
        category = row["c"]
        if category == "missing":
            fail(f"{side}: the engine produced no result for {row.get('file')}")
        schema = SCHEMA.get(category)
        if schema is None:
            fail(f"{side}: line {number}: unknown category {category!r}")
        if (side == "reference" and category in VENDORED_ONLY) or (
                side == "vendored" and category in REFERENCE_ONLY):
            fail(f"{side}: line {number}: category {category!r} belongs to the other side")
        fields = set(row) - {"c"}
        if fields != set(schema):
            fail(f"{side}: line {number}: {category} fields differ from the schema: "
                 f"missing {sorted(set(schema) - fields)}, unknown {sorted(fields - set(schema))}")
        for key, kind in schema.items():
            _check(side, number, key, kind, row[key])
        rows.append(row)
    return rows


def _constant(name):
    fail(f"a value is {name}, which no engine row may be")


def _check(side, number, key, kind, value):
    if value is None and kind in NULLABLE:
        return
    ok = {
        S: isinstance(value, str),
        B: isinstance(value, bool),
        I: isinstance(value, int) and not isinstance(value, bool),
        N: (isinstance(value, int) and not isinstance(value, bool)) or (
            isinstance(value, str) and re.fullmatch(r"-?[0-9][0-9.e+-]*", value) is not None),
        L: isinstance(value, list),
    }[kind]
    if not ok:
        if kind == N:
            fail(f"{side}: line {number}: {key} is not a finite number ({value!r})")
        fail(f"{side}: line {number}: {key} is not a {kind} ({value!r})")


def unwrap(value, project):
    """`value` with the project name the engine wraps qualified names in replaced by
    "@": exactly the name, or the name and a dot at the start. Nothing else changes."""
    if isinstance(value, str):
        if value == project:
            return "@"
        if value.startswith(project + "."):
            return "@" + value[len(project):]
        return value
    if isinstance(value, list):
        return [unwrap(v, project) for v in value]
    if isinstance(value, dict):
        return {k: unwrap(v, project) for k, v in value.items()}
    return value


def check_path(side, path):
    if not path or path.startswith("/") or "\\" in path or re.match(r"[A-Za-z]:", path) \
            or any(part in ("", ".", "..") for part in path.split("/")):
        fail(f"{side}: {path!r} is not a repository-relative path")


def surface_rows(side, row, project, kinds):
    """The logical rows of one file's cross-file surface: one per entry of its "lsp" and
    "reg" arrays, and one saying how many each holds, so an empty surface is never
    taken for a missing one."""
    try:
        doc = json.loads(row["json"], parse_float=str, parse_constant=_constant)
    except ValueError as e:
        fail(f"{side}: the surface of {row['file']} is not JSON: {e}")
    if not isinstance(doc, dict) or set(doc) != {"v", "lsp", "reg"} or doc["v"] != 1:
        fail(f"{side}: the surface of {row['file']} is not the version-1 codec's")
    out = [{"c": "surface_file", "file": row["file"], "lsp": len(doc["lsp"]),
            "reg": len(doc["reg"])}]
    for part in ("lsp", "reg"):
        if not isinstance(doc[part], list):
            fail(f"{side}: the surface of {row['file']} has no {part} list")
        for entry in doc[part]:
            if not isinstance(entry, dict):
                fail(f"{side}: a surface entry of {row['file']} is not an object")
            canonical = {"c": "surface", "file": row["file"], "part": part,
                         "entry": unwrap(entry, project)}
            label = entry.get("lb")
            if label is not None:
                canonical["kind"] = kinds[0].get(label, kinds[1])
            out.append(canonical)
    return out


def canonical(rows, project, kinds, files=None, side="side"):
    """One side's rows in canonical form: a sorted list of lines, each one row as JSON
    with sorted keys, no spaces and only ASCII. `files` limits the rows to those
    files; the engines index nothing else that is compared."""
    lines = []
    for row in rows:
        category = row["c"]
        if category in VENDORED_ONLY or category in REFERENCE_ONLY:
            continue
        check_path(side, row["file"])
        if files is not None and row["file"] not in files:
            continue
        if category == "surface":
            produced = surface_rows(side, row, project, kinds)
        else:
            produced = [unwrap(row, project)]
            if category == "definition":
                produced[0]["kind"] = kinds[0].get(row["label"], kinds[1])
        for item in produced:
            lines.append(json.dumps(item, sort_keys=True, separators=(",", ":"),
                                    ensure_ascii=True))
    return sorted(lines)


def canonical_bytes(lines):
    return ("\n".join(lines) + "\n").encode("ascii") if lines else b""


def category_of(line):
    c = json.loads(line)["c"]
    for name, members in CATEGORIES.items():
        if c in members:
            return name
    fail(f"no category holds {c!r}")


def counts(lines):
    found = collections.Counter(category_of(line) for line in lines)
    return {name: found.get(name, 0) for name in CATEGORIES}


def compare(reference, vendored):
    """Each category's rows only one side has, as multisets: {} when identical."""
    ref = collections.Counter(reference)
    ven = collections.Counter(vendored)
    out = {}
    for name in CATEGORIES:
        only_ref = sorted(l for l in (ref - ven).elements() if category_of(l) == name)
        only_ven = sorted(l for l in (ven - ref).elements() if category_of(l) == name)
        if only_ref or only_ven:
            out[name] = (only_ref, only_ven)
    return out


def describe(corpus, differences):
    """A bounded account of a difference: never more than SHOW rows each way, each cut
    at LINE characters."""
    out = []
    for name, (only_ref, only_ven) in differences.items():
        out.append(f"{corpus}: {name}: {len(only_ref)} row(s) only the reference has, "
                   f"{len(only_ven)} only the vendored engine has")
        for label, rows in (("reference only", only_ref), ("vendored only", only_ven)):
            for line in rows[:SHOW]:
                out.append(f"  {label}: {line[:LINE]}{'...' if len(line) > LINE else ''}")
            if len(rows) > SHOW:
                out.append(f"  {label}: and {len(rows) - SHOW} more")
    return out


def checks(side, rows, files):
    """What makes one side's rows parity evidence at all: every compared file present,
    typed work complete, no textual strategy among the typed answers, at least one
    definition."""
    present = {r["file"] for r in rows if r["c"] == "file"}
    missing = sorted(set(files) - present)
    if missing:
        fail(f"{side}: no result for {', '.join(missing[:5])}")
    for r in rows:
        if r["c"] == "file" and r["file"] in files and (r["lsp_skipped"] or r["walk_truncated"]):
            fail(f"{side}: {r['file']}'s typed work is incomplete (lsp_skipped "
                 f"{r['lsp_skipped']}, walk_truncated {r['walk_truncated']}), so it is no "
                 "parity evidence")
        if r["c"] == "typed" and r["strategy"] in TEXTUAL:
            fail(f"{side}: a typed answer carries the textual strategy {r['strategy']}")
    if not any(r["c"] == "definition" and r["file"] in files for r in rows):
        fail(f"{side}: no definition at all; the corpus proves nothing")


def cached_consistent(rows):
    """The vendored engine answers the same from its surfaces as fresh."""
    def key(r):
        return (r["file"], r["callee"], r["strategy"], r["reason"], r["kind"], r["start"],
                r["end"])
    fresh = collections.Counter(key(r) for r in rows if r["c"] == "typed")
    cached = collections.Counter(key(r) for r in rows if r["c"] == "cached_typed")
    if fresh != cached:
        fail(f"vendored: {sum((fresh - cached).values())} answer(s) only fresh, "
             f"{sum((cached - fresh).values())} only from surfaces: the vendored engine's "
             "own cache path changes its answers")


def surface_bytes_equal(ref_rows, ref_project, ven_rows, ven_project, files):
    """Whether each file's surface is the same bytes on both sides once the project
    name is replaced: a codec representation check, reported, never a verdict."""
    def digests(rows, project):
        out = {}
        for r in rows:
            if r["c"] == "surface" and r["file"] in files:
                text = r["json"].replace(f'"{project}.', '"@.').replace(f'"{project}"', '"@"')
                out[r["file"]] = hashlib.sha256(text.encode("latin-1")).hexdigest()
        return out
    return digests(ref_rows, ref_project) == digests(ven_rows, ven_project)


# --- the live run --------------------------------------------------------------------


def isolated_git(args, cwd):
    env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "GIT_TERMINAL_PROMPT": "0",
           "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull, "LC_ALL": "C"}
    done = subprocess.run(["git", "-C", str(cwd)] + args, capture_output=True, text=True,
                          env=env, stdin=subprocess.DEVNULL, timeout=600)
    if done.returncode != 0:
        fail(f"git {args[0]} failed in {cwd}: {done.stderr.strip()[:300]}")
    return done.stdout


def check_checkout(path, commit, what):
    """`path` is a checkout at exactly `commit`, clean."""
    if not (path / ".git").is_dir():
        fail(f"{what} is not fetched at {path}")
    head = isolated_git(["rev-parse", "HEAD"], path).strip()
    if head != commit:
        fail(f"{what} is at {head}, not the pinned {commit}")
    if isolated_git(["status", "--porcelain", "--untracked-files=all"], path).strip():
        fail(f"{what} is not clean at {path}")


def outside(path, *roots):
    """`path` (resolved) lies in none of `roots`."""
    p = pathlib.Path(path).resolve()
    return not any(p == r.resolve() or r.resolve() in p.parents for r in roots)


def reference_checkout():
    return pathlib.Path(os.environ.get("PDX_REF_DIR", "/tmp/ref")) / ENGINE_REF


def build_reference():
    """The reference engine built from its pinned checkout with the observation hook,
    by scripts/engine/reference-build.sh, outside the repository and the checkout.
    Returns the binary."""
    build = pathlib.Path(os.environ.get("PDX_DIFFERENTIAL_BUILD",
                                        "/tmp/pdx-reference-differential"))
    if not outside(build, REPO, reference_checkout()):
        fail(f"the reference must be built outside the repository and its checkout, not {build}")
    inverse = inverse_rename()
    hook = translate(HOOK_SOURCE.read_text(), inverse)
    rows = translate(ROWS_HEADER.read_text(), inverse)
    digest = hashlib.sha256((hook + "\0" + rows).encode()).hexdigest()[:16]
    hook_dir = pathlib.Path(tempfile.gettempdir()) / f"pdx-differential-hook-{digest}"
    hook_dir.mkdir(mode=0o700, exist_ok=True)
    (hook_dir / "hook.c").write_text(hook)
    (hook_dir / "differential_rows.h").write_text(rows)
    target = inverse.get("pdxe_pipeline_pass_calls")
    if not target:
        fail("the rename map does not name the calls pass")
    extra = " ".join(filter(None, [os.environ.get("PDX_REFERENCE_EXTRA_FLAGS", ""),
                                   f"-Wl,--wrap={target}", str(hook_dir / "hook.c")]))
    env = dict(os.environ, PDX_REFERENCE_BUILD=str(build), PDX_REFERENCE_EXTRA_FLAGS=extra)
    done = subprocess.run([str(REPO / "scripts/engine/reference-build.sh")], env=env,
                          capture_output=True, text=True, stdin=subprocess.DEVNULL)
    if done.returncode != 0:
        fail("the reference engine did not build:\n" + done.stderr.strip()[-2000:])
    return pathlib.Path(done.stdout.strip().splitlines()[-1])


def corpora(pdx_bench, names):
    """Every corpus, from pdx-bench differential-corpora, which checks the manifest and
    fetches the goldens it names."""
    done = subprocess.run([str(pdx_bench), "differential-corpora"], cwd=REPO,
                          capture_output=True, text=True, stdin=subprocess.DEVNULL)
    if done.returncode != 0:
        fail("pdx-bench differential-corpora failed:\n" + done.stderr.strip()[-2000:])
    doc = json.loads(done.stdout)
    if doc.get("format") != 1:
        fail("pdx-bench differential-corpora answered in another format")
    found = doc["corpora"]
    if names:
        unknown = sorted(set(names) - {c["name"] for c in found})
        if unknown:
            fail(f"no corpus is named {', '.join(unknown)}")
        found = [c for c in found if c["name"] in names]
    return found


def safe_file(root, rel):
    """`root/rel`, a regular file reached through real directories only."""
    path = root
    for part in rel.split("/"):
        path = path / part
        if path.is_symlink():
            fail(f"{root}: {rel} passes through a symlink")
    if not path.is_file():
        fail(f"{root}: {rel} is not a regular file")
    return path


def stage(corpus, stage_root):
    """The corpus's files and its support files, copied byte for byte into a fresh
    directory of the stage, which nothing else uses."""
    root = pathlib.Path(corpus["root"])
    target = stage_root / re.sub(r"[^A-Za-z0-9_.-]", "-", corpus["name"])
    if target.exists():
        shutil.rmtree(target)
    target.mkdir(parents=True)
    for rel in [f["path"] for f in corpus["files"]] + corpus["support"]:
        source = safe_file(root, rel)
        dest = target / rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, dest)
    return target


@contextlib.contextmanager
def hermetic_cwd(side, *inputs):
    """The working directory of one engine's run of one corpus: created empty for that
    run alone in the system's temporary directory, outside every input and the
    repository, and removed afterwards. Nothing an engine finds by a relative path is
    then an input; only what it is given by absolute path is (decision 52)."""
    path = pathlib.Path(tempfile.mkdtemp(prefix=f"pdx-differential-{side}-cwd-")).resolve()
    try:
        if any(path.iterdir()) or not outside(path, REPO, *inputs):
            fail(f"{path} is not an empty working directory of the harness's own")
        yield path
    finally:
        shutil.rmtree(path, ignore_errors=True)


def explicit(*paths):
    """Every path an engine is given is absolute: it runs elsewhere."""
    for path in paths:
        if not pathlib.Path(path).is_absolute():
            fail(f"an engine input is not an absolute path: {path}")


def run_vendored(dump, corpus, staged, work):
    listing = work / "vendored.list"
    listing.write_text("".join(f"{f['engine_language']}\t{f['path']}\n" for f in corpus["files"]))
    env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": str(work / "home")}
    explicit(dump, listing, staged)
    with hermetic_cwd("vendored", staged, pathlib.Path(corpus["root"]), work) as cwd:
        done = subprocess.run([str(dump), str(listing), str(staged)], capture_output=True,
                              cwd=cwd, env=env, stdin=subprocess.DEVNULL, timeout=1800)
    if done.returncode != 0:
        fail("the vendored engine failed: " + done.stderr.decode("latin-1").strip()[-1000:])
    return done.stdout.decode("ascii")


def run_reference(binary, corpus, staged, work):
    """The reference indexes the staged corpus. Returns its hook's rows with its stored
    surfaces added, and its project name."""
    rows_path = work / "reference.rows"
    cache = work / "reference-cache"
    home = work / "home"
    config = work / "config"
    for d in (cache, config):
        d.mkdir(mode=0o700)
    language = {}
    for f in corpus["files"]:
        suffix = "." + f["path"].rsplit(".", 1)[-1] if "." in f["path"].rsplit("/", 1)[-1] else ""
        if not suffix or language.setdefault(suffix, f["engine_language"]) != f["engine_language"]:
            fail(f"{corpus['name']}: {f['path']}'s extension cannot carry one language")
    settings = config / run_field("language_config")
    settings.parent.mkdir(parents=True, mode=0o700)
    settings.write_text(json.dumps({run_field("language_config_key"): language}, sort_keys=True))
    prefix = run_field("environment_prefix")
    env = {k: v for k, v in os.environ.items()
           if k in ("PATH", "LANG", "LC_ALL") and not k.startswith(prefix)}
    env.update({"HOME": str(home), "XDG_CONFIG_HOME": str(config),
                run_field("cache_variable"): str(cache), "PDX_DIFFERENTIAL_ROWS": str(rows_path)})
    args = [str(binary)] + run_field("index_arguments").split() + [
        json.dumps({"repo_path": str(staged)})]
    explicit(binary, staged, rows_path, cache, home, config)
    with hermetic_cwd("reference", staged, pathlib.Path(corpus["root"]), work) as cwd:
        done = subprocess.run(args, cwd=cwd, env=env, capture_output=True,
                              stdin=subprocess.DEVNULL, timeout=1800)
    if done.returncode != 0:
        fail("the reference failed: " + done.stderr.decode("latin-1").strip()[-1000:])
    if not rows_path.is_file():
        fail("the reference's calls pass never ran: no rows (a run of over 50 files takes "
             "its parallel path, which the hook does not observe)")
    text = rows_path.read_text(encoding="ascii")
    first = json.loads(text.splitlines()[0]) if text else {}
    if first.get("c") != "project":
        fail("the reference's rows do not begin with its project")
    project = first["name"]
    graphs = [p for p in cache.glob("*.db") if not p.name.startswith("_")]
    if len(graphs) != 1:
        fail(f"expected one graph from the reference, found {len(graphs)}")
    db = sqlite3.connect(f"file:{graphs[0]}?mode=ro", uri=True)
    try:
        db.text_factory = bytes
        # Beside the project, the reference may record "<project>::missed", its
        # coverage bookkeeping for regions it could not parse; that is no engine row
        # (the parse state is compared in each file's own row).
        names = sorted(r[0].decode() for r in db.execute("select name from projects"))
        if project not in names or any(n not in (project, project + "::missed") for n in names):
            fail(f"the reference's graph is of {names}, not {project}")
        surfaces = db.execute("select rel_path, defs_json from lsp_surface where project = ? "
                              "order by rel_path", (project,)).fetchall()
    finally:
        db.close()
    for rel, defs_json in surfaces:
        text += json.dumps({"c": "surface", "file": rel.decode("latin-1"),
                            "json": defs_json.decode("latin-1")}, ensure_ascii=True) + "\n"
    return text, project


def evidence_dir(name):
    """bench/.cache/engine-differential/<name>, real directories only."""
    path = REPO / "bench/.cache"
    for part in ("engine-differential", re.sub(r"[^A-Za-z0-9_.-]", "-", name)):
        path = path / part
        if path.is_symlink() or (path.exists() and not path.is_dir()):
            fail(f"{path} is not a directory of the harness's")
        path.mkdir(exist_ok=True)
    return path


def run_corpus(corpus, binary, dump, kinds, stage_root):
    work = stage_root / ("work-" + re.sub(r"[^A-Za-z0-9_.-]", "-", corpus["name"]))
    if work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True)
    (work / "home").mkdir(mode=0o700)
    if corpus["kind"] == "golden":
        check_checkout(pathlib.Path(corpus["root"]), corpus["commit"], corpus["name"])
    staged = stage(corpus, stage_root)
    files = {f["path"] for f in corpus["files"]}
    ven_text = run_vendored(dump, corpus, staged, work)
    ref_text, ref_project = run_reference(binary, corpus, staged, work)
    ven_rows = parse_rows(ven_text, "vendored")
    ref_rows = parse_rows(ref_text, "reference")
    ven_project = vendored_project()
    checks("vendored", ven_rows, files)
    checks("reference", ref_rows, files)
    cached_consistent(ven_rows)
    ref_lines = canonical(ref_rows, ref_project, kinds, files, "reference")
    ven_lines = canonical(ven_rows, ven_project, kinds, files, "vendored")
    evidence = evidence_dir(corpus["name"])
    (evidence / "reference.canonical").write_bytes(canonical_bytes(ref_lines))
    (evidence / "vendored.canonical").write_bytes(canonical_bytes(ven_lines))
    differences = compare(ref_lines, ven_lines)
    (evidence / "differences.txt").write_text(
        "\n".join(describe(corpus["name"], differences)) + "\n" if differences else "")
    same_bytes = surface_bytes_equal(ref_rows, ref_project, ven_rows, ven_project, files)
    if corpus["kind"] == "golden":
        check_checkout(pathlib.Path(corpus["root"]), corpus["commit"], corpus["name"])
    shutil.rmtree(work)
    shutil.rmtree(staged)
    return counts(ref_lines), counts(ven_lines), differences, same_bytes


def cmd_run(args):
    pin = reference_pin()
    checkout = reference_checkout()
    check_checkout(checkout, pin, "the reference engine")
    dump = pathlib.Path(args.dump).resolve()
    if not dump.is_file():
        fail(f"no differential dump at {dump}: run make engine")
    kinds = kind_map()
    found = corpora(args.pdx_bench, args.corpus)
    binary = build_reference()
    stage_root = pathlib.Path(os.environ.get("PDX_DIFFERENTIAL_STAGE",
                                             "/tmp/pdx-engine-differential")).resolve()
    if not outside(stage_root, REPO, checkout):
        fail(f"the stage must be outside the repository and the reference, not {stage_root}")
    stage_root.mkdir(parents=True, exist_ok=True)
    totals = {"fixture": collections.Counter(), "golden": collections.Counter()}
    vtotals = {"fixture": collections.Counter(), "golden": collections.Counter()}
    languages = collections.defaultdict(list)
    failed = []
    for corpus in found:
        try:
            ref, ven, differences, same_bytes = run_corpus(corpus, binary, dump, kinds,
                                                           stage_root)
        except Failure as e:
            failed.append(corpus["name"])
            print(f"FAIL {corpus['name']}: {e}")
            continue
        totals[corpus["kind"]].update(ref)
        vtotals[corpus["kind"]].update(ven)
        summary = ", ".join(f"{name} {ref[name]}" + ("" if ref[name] == ven[name] else
                                                     f"/{ven[name]}") for name in CATEGORIES)
        if differences:
            failed.append(corpus["name"])
            print(f"DIFFERS {corpus['name']} ({corpus['language']}): {summary}")
            print("\n".join(describe(corpus["name"], differences)))
        else:
            note = "" if same_bytes else "; surface bytes differ, rows identical"
            print(f"ok {corpus['name']} ({corpus['language']}): {summary}{note}")
            languages[(corpus["kind"], corpus["language"])].append(corpus["name"])
    check_checkout(checkout, pin, "the reference engine, after the run,")
    for kind in ("fixture", "golden"):
        names = [c for c in found if c["kind"] == kind]
        if not names:
            continue
        langs = sorted({c["language"] for c in names})
        print(f"{kind}s: {len(names)} corpora, {len(langs)} languages, "
              f"{sum(len(c['files']) for c in names)} files")
        for name in CATEGORIES:
            print(f"  {name:12} reference {totals[kind][name]:7}  vendored {vtotals[kind][name]:7}")
    print(f"mismatches {len(failed)}")
    if failed:
        print(f"engine differential: {len(failed)} corpus/corpora differ or failed; full "
              f"evidence under bench/.cache/engine-differential/")
        return 1
    print("engine differential: identical")
    return 0


# --- the offline commands -----------------------------------------------------------


def cmd_canonical(args):
    kinds = kind_map()
    rows = parse_rows(pathlib.Path(args.rows).read_text(encoding="ascii"), args.side)
    sys.stdout.buffer.write(canonical_bytes(canonical(rows, args.project, kinds, None, args.side)))
    return 0


def cmd_compare(args):
    kinds = kind_map()
    ref_rows = parse_rows(pathlib.Path(args.reference).read_text(encoding="ascii"), "reference")
    ven_rows = parse_rows(pathlib.Path(args.vendored).read_text(encoding="ascii"), "vendored")
    files = {r["file"] for r in ref_rows + ven_rows if r["c"] == "file"}
    checks("reference", ref_rows, files)
    checks("vendored", ven_rows, files)
    cached_consistent(ven_rows)
    ref = canonical(ref_rows, args.reference_project, kinds, None, "reference")
    ven = canonical(ven_rows, args.vendored_project, kinds, None, "vendored")
    differences = compare(ref, ven)
    if differences:
        print("\n".join(describe("compare", differences)))
        return 1
    c = counts(ref)
    print("identical: " + ", ".join(f"{name} {c[name]}" for name in CATEGORIES))
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="command", required=True)
    run = sub.add_parser("run")
    run.add_argument("--pdx-bench", required=True)
    run.add_argument("--dump", default=str(DEFAULT_DUMP))
    run.add_argument("--corpus", action="append", default=[])
    can = sub.add_parser("canonical")
    can.add_argument("--project", required=True)
    can.add_argument("--side", default="vendored", choices=["reference", "vendored"])
    can.add_argument("rows")
    cmp_ = sub.add_parser("compare")
    cmp_.add_argument("--reference", required=True)
    cmp_.add_argument("--reference-project", required=True)
    cmp_.add_argument("--vendored", required=True)
    cmp_.add_argument("--vendored-project", required=True)
    args = ap.parse_args(argv)
    try:
        return {"run": cmd_run, "canonical": cmd_canonical, "compare": cmd_compare}[
            args.command](args)
    except Failure as e:
        print(f"engine differential: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
