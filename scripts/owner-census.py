#!/usr/bin/env python3
"""Who owns the call sites of an indexed checkout, counted from the engine's own
declaration occurrences (docs/plan/ISSUES.md, issues 75 and 76; decision 48).

    scripts/owner-census.py [--list] <segment.db> <checkout>

Reads the files the segment parsed, with the language the build assigned each, from
the segment (read-only), and runs the engine's census program over them
(engine/tests/owner_census.c, built by `make engine`) in the checkout the segment was
indexed from. Prints the program's rows; `orphan` must be 0. Diagnostic only: not part
of `make check`.
"""

import argparse
import os
import sqlite3
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CENSUS = os.path.join(ROOT, "target", "engine", "tests", "owner_census")


def parsed_files(segment):
    uri = "file:" + os.path.abspath(segment) + "?mode=ro&immutable=1"
    db = sqlite3.connect(uri, uri=True)
    try:
        return db.execute(
            "SELECT language, path FROM files WHERE status IN ('parsed', 'partial') "
            "ORDER BY path"
        ).fetchall()
    finally:
        db.close()


def engine_language(language, path):
    """The engine grammar a build extracts the file with: TypeScript's `.tsx` files are
    read as `tsx`, the one dialect the language matrix names (pdx_core::languages)."""
    return "tsx" if language == "typescript" and path.endswith(".tsx") else language


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--list", action="store_true", help="also list each orphan site")
    parser.add_argument(
        "--occurrences",
        action="store_true",
        help="print each callable's raw declaration occurrence instead of the census",
    )
    parser.add_argument("segment")
    parser.add_argument("checkout")
    args = parser.parse_args(argv)
    if not os.access(CENSUS, os.X_OK):
        sys.exit(f"{os.path.relpath(CENSUS, ROOT)} is not built: run `make engine`")
    listing = "".join(
        f"{engine_language(language, path)}\t{path}\n" for language, path in parsed_files(args.segment)
    )
    flags = (["--list"] if args.list else []) + (["--occurrences"] if args.occurrences else [])
    command = [CENSUS] + flags + [args.checkout]
    return subprocess.run(command, input=listing, text=True, check=False).returncode


if __name__ == "__main__":
    sys.exit(main())
