"""
SQL/DDL extraction -> table / view / index symbols —

This is the request's "databases" requirement, and it is Tier 2: NOT free.

STORAGE DECISION. DDL objects are written into the EXISTING symbols_* table with
kind in {table, view, index, column, references}, rather than into a new table.
That reuses the whole incremental pipeline — per-file hashing, delete-on-change,
flush batching — with zero schema change, and it makes DDL objects visible to
Praxevia's chat tools (`lookup_symbol_usage` can now find a table).

Columns are stored as kind='column' with parent_name=<table>. The builder folds
them into the owning table node's metadata rather than emitting a node each, so a
40-column table costs one graph node, not forty.

Modelled on a TypeScript SQL parser written for an earlier graph viewer, with
four deliberate improvements, since that file documents its own gaps:

  1. SCHEMA-QUALIFIED NAMES. Upstream's docstring says it "does not handle
     schema-qualified names (e.g., public.users)", and worse, its \\w+ pattern
     silently captures only the FIRST segment — `CREATE TABLE public.users`
     yields a table called "public". Here the qualifier is captured and kept.
  2. FOREIGN KEYS -> depends_on edges. Upstream discards constraints.
  3. STATEMENT END. Upstream searches for the literal ");", which misses `) ;`,
     `)\\n;` and trailing table options. Here the block is brace-matched.
  4. COMMENTS AND STRING LITERALS are masked before matching. Without that a
     commented-out CREATE TABLE becomes a table.

STILL NOT HANDLED, inherited and stated honestly: stored procedures, triggers,
ALTER TABLE ADD CONSTRAINT (only inline REFERENCES are seen), and dialect-specific
partitioning clauses.

MASKING ORDER is comments-then-strings, which is not a real tokenizer and has one
known failure: a `--` sequence INSIDE a string literal is treated as the start of
a comment. The alternative order fails on the far more common case of an
apostrophe inside a comment ("-- it's fine"), where an unterminated quote would
swallow everything up to the next quote, possibly lines away. Comments-first is
the lesser evil; a real fix means a tokenizer, which is sqlglot's job.
"""

from __future__ import annotations

import re

# Order matters: block comments before line comments, both before literals.
_RE_BLOCK_COMMENT = re.compile(r"/\*.*?\*/", re.DOTALL)
_RE_LINE_COMMENT = re.compile(r"--[^\n]*")
_RE_STRING = re.compile(r"'(?:[^']|'')*'")

_IDENT = r'(?:`|"|\[)?(\w+)(?:`|"|\])?'
_QUALIFIED = rf"(?:{_IDENT}\s*\.\s*)?{_IDENT}"

_RE_TABLE = re.compile(
    rf"CREATE\s+(?:GLOBAL\s+|LOCAL\s+)?(?:TEMP(?:ORARY)?\s+|UNLOGGED\s+)?TABLE\s+"
    rf"(?:IF\s+NOT\s+EXISTS\s+)?{_QUALIFIED}",
    re.IGNORECASE,
)
_RE_VIEW = re.compile(
    rf"CREATE\s+(?:OR\s+REPLACE\s+)?(?:MATERIALIZED\s+)?VIEW\s+"
    rf"(?:IF\s+NOT\s+EXISTS\s+)?{_QUALIFIED}",
    re.IGNORECASE,
)
_RE_INDEX = re.compile(
    rf"CREATE\s+(?:UNIQUE\s+)?INDEX\s+(?:CONCURRENTLY\s+)?"
    rf"(?:IF\s+NOT\s+EXISTS\s+)?{_QUALIFIED}",
    re.IGNORECASE,
)

# Inline column-level and table-level foreign keys.
_RE_REFERENCES = re.compile(rf"REFERENCES\s+{_QUALIFIED}", re.IGNORECASE)

_CONSTRAINT_START = re.compile(
    r"^(PRIMARY|FOREIGN|UNIQUE|CHECK|CONSTRAINT|INDEX|KEY|EXCLUDE|LIKE)\b",
    re.IGNORECASE,
)
_RE_COLUMN = re.compile(rf"^{_IDENT}\s+\S")


def _blank_out(text: str, pattern: re.Pattern) -> str:
    """
    Replace matches with same-length whitespace, preserving newlines.

    Byte offsets and line numbers must survive masking, or every lineRange after
    the first comment is wrong.
    """

    def repl(m: re.Match) -> str:
        return "".join(c if c == "\n" else " " for c in m.group(0))

    return pattern.sub(repl, text)


def _strip_noise(sql: str) -> str:
    for pattern in (_RE_BLOCK_COMMENT, _RE_LINE_COMMENT, _RE_STRING):
        sql = _blank_out(sql, pattern)
    return sql


def _qualified_name(m: re.Match) -> tuple[str, str]:
    """Return (bare_name, qualified_name) from a _QUALIFIED match."""
    schema, name = m.group(1), m.group(2)
    return name, (f"{schema}.{name}" if schema else name)


def _line_of(text: str, index: int) -> int:
    return text.count("\n", 0, index) + 1


def _balanced_block(text: str, start: int) -> tuple[int, int] | None:
    """Byte range of the (...) block following `start`, brace-matched."""
    open_idx = text.find("(", start)
    if open_idx == -1:
        return None
    depth = 0
    for i in range(open_idx, len(text)):
        if text[i] == "(":
            depth += 1
        elif text[i] == ")":
            depth -= 1
            if depth == 0:
                return open_idx, i
    return None


def _split_top_level(body: str) -> list[str]:
    """Split a column list on commas that are not inside parentheses."""
    parts: list[str] = []
    depth = 0
    buf: list[str] = []
    for ch in body:
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        if ch == "," and depth == 0:
            parts.append("".join(buf))
            buf = []
        else:
            buf.append(ch)
    if buf:
        parts.append("".join(buf))
    return parts


def extract_sql_symbols(content: str, rel_path: str) -> list[dict]:
    """
    Parse DDL into symbols_*-shaped dicts.

    Returns rows with the keys the indexer's symbol flush expects:
    name, qualified_name, kind, filename, line_start, line_end, parent_name.
    """
    clean = _strip_noise(content)
    out: list[dict] = []

    for m in _RE_TABLE.finditer(clean):
        name, qualified = _qualified_name(m)
        start_line = _line_of(clean, m.start())
        block = _balanced_block(clean, m.end())
        end_line = _line_of(clean, block[1]) if block else start_line

        out.append(
            {
                "name": name,
                "qualified_name": qualified,
                "kind": "table",
                "filename": rel_path,
                "line_start": start_line,
                "line_end": end_line,
                "parent_name": None,
            }
        )

        if not block:
            continue

        body = clean[block[0] + 1 : block[1]]
        offset = 0
        for part in _split_top_level(body):
            trimmed = part.strip()
            # Absolute line of this part. Counting relative to `start_line` would
            # be wrong whenever the "(" is not on the CREATE TABLE line.
            part_line = _line_of(clean, block[0] + 1 + offset)
            offset += len(part) + 1  # +1 for the comma consumed by the split
            if not trimmed:
                continue

            # A FOREIGN KEY / REFERENCES clause names another table.
            for ref in _RE_REFERENCES.finditer(trimmed):
                _, ref_qualified = _qualified_name(ref)
                out.append(
                    {
                        "name": ref_qualified,
                        "qualified_name": ref_qualified,
                        "kind": "references",
                        "filename": rel_path,
                        "line_start": part_line,
                        "line_end": part_line,
                        "parent_name": name,
                    }
                )

            if _CONSTRAINT_START.match(trimmed):
                continue
            col = _RE_COLUMN.match(trimmed)
            if col:
                out.append(
                    {
                        "name": col.group(1),
                        "qualified_name": f"{qualified}.{col.group(1)}",
                        "kind": "column",
                        "filename": rel_path,
                        "line_start": part_line,
                        "line_end": part_line,
                        "parent_name": name,
                    }
                )

    for regex, kind in ((_RE_VIEW, "view"), (_RE_INDEX, "index")):
        for m in regex.finditer(clean):
            name, qualified = _qualified_name(m)
            line = _line_of(clean, m.start())
            out.append(
                {
                    "name": name,
                    "qualified_name": qualified,
                    "kind": kind,
                    "filename": rel_path,
                    "line_start": line,
                    "line_end": line,
                    "parent_name": None,
                }
            )

    return out
