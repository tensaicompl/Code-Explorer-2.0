"""
dotenv extraction -> config symbols —

Populates the CONFIG filter, which reported `config=0` on every project.

    env_var  ->  `config` node   (CONFIG filter)

NAMES ONLY — NEVER VALUES. This is the one extractor whose input routinely holds
live credentials: of the six dotenv files in the indexed repos, four are real
`.env`/`.env.local` files rather than `.example` templates. A variable's NAME is
the architectural fact ("this service needs DATABASE_URL"); its value is a
secret. Nothing here reads past the `=`, and `build_corpus.py` redacts the same values
before the file reaches the chunker, so no secret enters `code_chunks_*` or the
chat's search tools either. Do not "improve" this by capturing values to show a
default — a `.env.example` and a `.env` are indistinguishable to this code, and
the failure is silent and permanent once embedded.

Modelled on an earlier TypeScript dotenv parser, fixing the two gaps its own docstring
admits, plus one it does not:

  1. `export VAR=value`, which upstream drops — the dominant form in a file meant
     to be `source`d rather than read by a dotenv library.
  2. Multi-line values (`KEY="line one\\nline two"`), where upstream reads each
     continuation line as a new variable. A quoted value that does not close on
     its line is consumed until it does.
  3. COMMENTS AS DOCS. The `#` block immediately above a variable becomes its
     summary, which for configuration is usually the only documentation there is.

FILENAME, NOT EXTENSION. `os.path.splitext(".env")` returns `(".env", "")` — the
extension of a dotfile is empty — and `.env.local` splits to `.local`. Every call
site therefore routes here through `build_corpus.py`'s `classify_ext`, not through a raw
`splitext`. Adding `.env` to `SOURCE_EXTENSIONS` alone would have matched
nothing, silently.
"""

from __future__ import annotations

import re

_RE_ASSIGN = re.compile(r"^\s*(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=(.*)$")

_MAX_DOC_CHARS = 400


def _unterminated_quote(value: str) -> str | None:
    """Return the quote character an assignment leaves open, if any."""
    value = value.strip()
    if not value or value[0] not in "\"'":
        return None
    quote = value[0]
    rest = value[1:]
    escaped = False
    for ch in rest:
        if escaped:
            escaped = False
            continue
        if ch == "\\":
            escaped = True
        elif ch == quote:
            return None
    return quote


def extract_env_symbols(content: str, rel_path: str) -> list[dict]:
    """
    Parse a dotenv file into symbols_*-shaped dicts. Variable names only.

    Returns rows carrying the keys the indexer's symbol flush expects:
    name, qualified_name, kind, filename, line_start, line_end, parent_name, doc.
    """
    lines = content.split("\n")
    out: list[dict] = []
    doc_buffer: list[str] = []
    pending_quote: str | None = None
    start_line = 0

    for lineno, raw in enumerate(lines, start=1):
        if pending_quote is not None:
            # Inside a multi-line value. Nothing here is a declaration, and the
            # content is by definition the secret half of the file.
            if _closes(raw, pending_quote):
                pending_quote = None
                out[-1]["line_end"] = lineno
            continue

        stripped = raw.strip()
        if not stripped:
            doc_buffer = []
            continue
        if stripped.startswith("#"):
            doc_buffer.append(stripped.lstrip("#").strip())
            continue

        m = _RE_ASSIGN.match(raw)
        if not m:
            doc_buffer = []
            continue

        name, value = m.group(1), m.group(2)
        start_line = lineno
        out.append(
            {
                "name": name,
                "qualified_name": name,
                "kind": "env_var",
                "filename": rel_path,
                "line_start": start_line,
                "line_end": start_line,
                "parent_name": None,
                "doc": " ".join(doc_buffer)[:_MAX_DOC_CHARS],
            }
        )
        doc_buffer = []
        pending_quote = _unterminated_quote(value)

    return out


def _closes(line: str, quote: str) -> bool:
    escaped = False
    for ch in line:
        if escaped:
            escaped = False
            continue
        if ch == "\\":
            escaped = True
        elif ch == quote:
            return True
    return False


def redact(content: str) -> str:
    """
    Replace every assignment's value with a placeholder, preserving line count.

    `build_corpus.py` embeds this instead of the file, so `code_chunks_*` — which the
    chat's semantic search reads verbatim — holds the variable names and none of
    the secrets. Line count is preserved because `offset_to_line_number` maps chunk
    offsets back to line numbers, and a dropped newline shifts every span after it.
    """
    out: list[str] = []
    pending_quote: str | None = None
    for raw in content.split("\n"):
        if pending_quote is not None:
            out.append("")
            if _closes(raw, pending_quote):
                pending_quote = None
            continue
        m = _RE_ASSIGN.match(raw)
        if not m:
            out.append(raw)
            continue
        prefix = raw[: m.start(2)]
        out.append(f"{prefix}<redacted>")
        pending_quote = _unterminated_quote(m.group(2))
    return "\n".join(out)
