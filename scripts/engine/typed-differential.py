#!/usr/bin/env python3
"""Compares our typed resolution with the reference engine's, fixture by fixture.

Each fixture is a directory of source files. For each one, what our engine decided
(by engine/tests/resolve_dump) must equal what the reference decided, as recorded in
the fixture's expected.tsv:

    typed-differential.py [--dump PATH] <fixture>...              compare with expected.tsv
    typed-differential.py --reference [--dump PATH] <fixture>...  also run the reference
                                                                  and compare with it
    typed-differential.py --reference --update <fixture>...       rewrite expected.tsv from
                                                                  the reference

The expected files are the reference's output, so the check runs anywhere, without
the reference. Regenerating them needs the reference built (scripts/engine/
reference-build.sh) and is how a change in either engine is caught.

What is compared, per fixture:

  CALLS           call edges the reference's typed pass drew: its source, its target,
                  and the typed strategy the reference recorded, which must be one of
                  the strategies we report for that pair
  CALL_REFERENCE  callables passed as values, which only typed resolution produces
  IMPORTS         import edges: importing file, target and the local name bound

Both sides collapse to one row per (kind, source, target), as the reference's graph
does, and neither side keeps an edge from a definition to itself, which the reference
never draws. Edges the reference's textual stages drew are not compared: resolving
those is the job of this project's own stages, not the engine's.

Two further checks run on our side alone, reference or not:

  assert.tsv      what the fixture exists to prove, written by hand: '+' rows must
                  be present and '-' rows absent ('*' matches any field), so a
                  fixture's point survives even if both engines changed together
  file order      adding the files in reverse gives the same answers
  cache           resolving every file from its surface, as a build does for a file
                  its cache holds, gives the same answers as extracting it
"""
import argparse
import os
import pathlib
import sqlite3
import subprocess
import sys
import tempfile

REPO = pathlib.Path(__file__).resolve().parents[2]
RUN_DATA = REPO / "scripts/vendor/reference-run.txt"
DEFAULT_DUMP = REPO / "target/engine/tests/resolve_dump"

# The reference's textual strategies. Any other strategy on a call edge came from its
# typed pass.
TEXTUAL = {
    "import_map", "import_map_suffix", "qualified_suffix", "same_module", "suffix_match",
    "unique_name", "fuzzy", "callee_suffix", "field_type_hint", "service_pattern",
}


def run_field(key):
    for line in RUN_DATA.read_text().splitlines():
        if line.strip() and not line.startswith("#"):
            k, _, v = line.partition("\t")
            if k == key:
                return v
    sys.exit(f"error: {RUN_DATA.name} has no {key}")


def ours(dump, fixture, reverse=False, cached=False):
    """Our rows: {(kind, source, target): set of strategies}."""
    args = ([str(dump)] + (["--reverse"] if reverse else []) + (["--cached"] if cached else [])
            + [str(fixture)])
    out = subprocess.run(args, capture_output=True, text=True)
    if out.returncode != 0:
        sys.exit(f"error: resolve_dump failed on {fixture}:\n{out.stderr}")
    rows = {}
    for line in out.stdout.splitlines():
        f = line.split("\t")
        if f[0] == "IMPORTS":
            rows.setdefault(("IMPORTS", f[1], f[2]), set()).add(f[3])
        elif f[1] != f[2]:
            rows.setdefault((f[0], f[1], f[2]), set()).add(f[3] if f[0] == "CALLS" else "-")
    return rows


def reference(binary, fixture):
    """The reference's rows, in the same form, from the graph it builds."""
    with tempfile.TemporaryDirectory() as cache:
        os.chmod(cache, 0o700)
        env = dict(os.environ, **{run_field("cache_variable"): cache})
        args = [str(binary)] + run_field("index_arguments").split() + [
            '{"repo_path":"%s"}' % fixture]
        done = subprocess.run(args, cwd=fixture, env=env, capture_output=True, text=True)
        if done.returncode != 0:
            sys.exit(f"error: the reference failed on {fixture}:\n{done.stderr}")
        dbs = [p for p in pathlib.Path(cache).glob("*.db") if not p.name.startswith("_")]
        if len(dbs) != 1:
            sys.exit(f"error: expected one graph from the reference, found {len(dbs)}")
        db = sqlite3.connect(dbs[0])
        project = db.execute("select name from projects").fetchone()[0]

        def strip(qn):
            return qn[len(project) + 1:] if qn and qn.startswith(project + ".") else qn

        def source(label, qn, path):
            return f"FILE:{path}" if label in ("File", "Module") else strip(qn)

        rows = {}
        query = """
            select e.type, s.label, s.qualified_name, s.file_path, t.qualified_name,
                   json_extract(e.properties, '$.strategy'),
                   json_extract(e.properties, '$.local_name')
            from edges e join nodes s on s.id = e.source_id join nodes t on t.id = e.target_id
            where e.type in ('CALLS', 'CALL_REFERENCE', 'IMPORTS')"""
        for kind, slabel, sqn, spath, tqn, strategy, local in db.execute(query):
            target = strip(tqn)
            if kind == "IMPORTS":
                rows.setdefault(("IMPORTS", f"FILE:{spath}", target), set()).add(local or "")
                continue
            if kind == "CALLS" and (strategy is None or strategy in TEXTUAL):
                continue
            src = source(slabel, sqn, spath)
            if src != target:
                rows.setdefault((kind, src, target), set()).add(strategy if kind == "CALLS" else "-")
        db.close()
        return rows


def to_lines(rows):
    return sorted("\t".join(k) + "\t" + ",".join(sorted(v)) for k, v in rows.items())


def from_lines(text):
    rows = {}
    for line in text.splitlines():
        if line.strip() and not line.startswith("#"):
            kind, src, tgt, values = line.split("\t")
            rows[(kind, src, tgt)] = set(values.split(",")) if values else {""}
    return rows


def compare(label, expected, actual):
    """Differences between an expected and an actual row set, as readable lines."""
    problems = []
    for key in sorted(expected.keys() - actual.keys()):
        problems.append(f"  missing  {' '.join(key)}  ({label} has it, we do not)")
    for key in sorted(actual.keys() - expected.keys()):
        problems.append(f"  extra    {' '.join(key)}  (we have it, {label} does not)")
    for key in sorted(expected.keys() & actual.keys()):
        if key[0] == "CALL_REFERENCE":
            continue
        same = expected[key] == actual[key] if key[0] == "IMPORTS" else expected[key] <= actual[key]
        if not same:
            problems.append(f"  differs  {' '.join(key)}  {label}: {','.join(sorted(expected[key]))}"
                            f"  ours: {','.join(sorted(actual[key]))}")
    return problems


def check_asserts(fixture, rows):
    """Problems with the fixture's hand-written assertions, as readable lines."""
    path = fixture / "assert.tsv"
    if not path.exists():
        return []

    def matches(pattern, key, values):
        kind, src, tgt, value = pattern
        return (all(p in ("*", k) for p, k in zip((kind, src, tgt), key))
                and (value == "*" or value in values))

    problems = []
    for line in path.read_text().splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        sign, *pattern = line.split("\t")
        if len(pattern) != 4 or sign not in "+-":
            problems.append(f"  assert.tsv: malformed line: {line!r}")
            continue
        found = any(matches(pattern, key, values) for key, values in rows.items())
        if sign == "+" and not found:
            problems.append(f"  assertion failed: no row {' '.join(pattern)}")
        if sign == "-" and found:
            problems.append(f"  assertion failed: unexpected row {' '.join(pattern)}")
    return problems


HEADER = ("# Typed resolution the reference engine produced for this fixture. Generated by\n"
          "# scripts/engine/typed-differential.py --reference --update; do not edit.\n")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--reference", action="store_true")
    ap.add_argument("--update", action="store_true")
    ap.add_argument("--dump", default=str(DEFAULT_DUMP))
    ap.add_argument("fixtures", nargs="+")
    a = ap.parse_args()
    if a.update and not a.reference:
        sys.exit("error: --update needs --reference: the expected files are the reference's")
    binary = None
    if a.reference:
        built = subprocess.run([str(REPO / "scripts/engine/reference-build.sh")],
                               capture_output=True, text=True)
        if built.returncode != 0:
            sys.exit(built.stderr)
        binary = built.stdout.strip().splitlines()[-1]

    failed = 0
    for fixture in (pathlib.Path(f).resolve() for f in a.fixtures):
        expected_file = fixture / "expected.tsv"
        mine = ours(a.dump, fixture)
        problems = check_asserts(fixture, mine)
        if ours(a.dump, fixture, reverse=True) != mine:
            problems.append("  the answers change when the files are added in reverse order")
        if ours(a.dump, fixture, cached=True) != mine:
            problems.append("  the answers change when the files come from a cache")
        if a.reference:
            theirs = reference(binary, fixture)
            if a.update:
                expected_file.write_text(HEADER + "\n".join(to_lines(theirs)) + "\n")
            elif expected_file.exists() and from_lines(expected_file.read_text()) != theirs:
                problems.append("  expected.tsv is stale: the reference now answers differently")
            problems += compare("the reference", theirs, mine)
        else:
            if not expected_file.exists():
                problems.append("  no expected.tsv: generate it with --reference --update")
            else:
                problems += compare("expected.tsv", from_lines(expected_file.read_text()), mine)
        name = fixture.relative_to(REPO) if fixture.is_relative_to(REPO) else fixture
        if problems:
            failed += 1
            print(f"FAIL {name}")
            print("\n".join(problems))
        else:
            print(f"ok   {name}  ({len(mine)} rows)")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
