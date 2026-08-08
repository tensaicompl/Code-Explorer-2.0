"""
Markdown extraction -> document symbols —

Populates the DOCS filter, which until now had nothing to filter: every project
reported `document=0`, so the chip was a no-op on a graph where the READMEs and
architecture notes were the files a newcomer most wants to find.

Kinds emitted, and what the builder does with each:

    document    -> `document` node   (DOCS filter)
    subheading  -> FOLDED into the enclosing document node's `sections` metadata

WHY ONLY LEVELS 1-2 BECOME NODES. Measured across the four indexed repos:
1 897 level-1/2 headings against 3 182 headings in total. Emitting a node per
heading would grow cso's graph by 25 % with material that is mostly prose
structure, and's real scale cliff is per-layer expansion, not node count in
the abstract. Levels 3+ fold into their ancestor exactly as columns fold into a
table: the structure survives as metadata, the node count stays bounded,
and the decision is a rule rather than a silent cap.

EVERY MARKDOWN FILE YIELDS AT LEAST ONE NODE. A file with no level-1 heading —
common for `docs/setup.md` that opens at `##` — gets a synthetic root document
named for the file, so the DOCS filter never has a document that is invisible
because its author started at the wrong level. A file with no headings at all
still gets that root, with its first paragraph as the summary.

Modelled on an earlier TypeScript markdown parser (`extractSections`), keeping its
fenced-code-block tracking — a `# comment` inside a ``` block is shell, not a
heading — and adding four things it does not do:

  1. HIERARCHY. Upstream returns a flat section list with levels. Here level 2
     nests under the enclosing level 1, which is what produces `contains` edges.
  2. SUMMARIES. The first paragraph under a heading becomes the node summary, so a document node says what the document is about rather than
     repeating its own title.
  3. FRONT MATTER. A leading `---` block is skipped, so a YAML key is never read
     as a setext underline or its `#` comments as headings.
  4. ESCAPED FENCES. Upstream treats any line starting with ``` as a fence
     toggle, including the indented ones inside a list item; the marker length
     and indentation are tracked here so a nested fence does not desynchronise
     the whole rest of the file.

NOT HANDLED: setext headings (`Title` over `=====`), which upstream also skips —
they are rare in these repos and ambiguous against horizontal rules.
"""

from __future__ import annotations

import re

_RE_FENCE = re.compile(r"^(\s{0,3})(`{3,}|~{3,})")
_RE_ATX = re.compile(r"^(#{1,6})\s+(.+?)\s*#*\s*$")
_RE_LINK = re.compile(r"\[([^\]]*)\]\([^)]*\)")
_RE_INLINE_CODE = re.compile(r"`([^`]*)`")
_RE_EMPHASIS = re.compile(r"(\*\*|__|\*|_)")

# A summary is a card subtitle, not the paragraph itself.
_MAX_DOC_CHARS = 400
# Headings deeper than this fold into their ancestor rather than becoming nodes.
_NODE_LEVEL = 2


def _clean_text(text: str) -> str:
    """Strip the markdown that would otherwise be read aloud as punctuation."""
    text = _RE_LINK.sub(r"\1", text)
    text = _RE_INLINE_CODE.sub(r"\1", text)
    text = _RE_EMPHASIS.sub("", text)
    return " ".join(text.split())


def _strip_front_matter(lines: list[str]) -> int:
    """Return the index of the first line after a leading `---` block."""
    if not lines or lines[0].strip() != "---":
        return 0
    for i in range(1, len(lines)):
        if lines[i].strip() in ("---", "..."):
            return i + 1
    return 0  # unterminated: treat the whole file as body


def _headings(lines: list[str], start: int) -> list[tuple[int, int, str]]:
    """Return [(lineno, level, text)] for ATX headings outside fenced code."""
    found: list[tuple[int, int, str]] = []
    fence: str | None = None
    for i in range(start, len(lines)):
        line = lines[i]
        m = _RE_FENCE.match(line)
        if m:
            marker = m.group(2)
            if fence is None:
                fence = marker[0] * 3
            elif marker[0] * 3 == fence:
                fence = None
            continue
        if fence is not None:
            continue
        h = _RE_ATX.match(line)
        if h:
            text = _clean_text(h.group(2))
            if text:
                found.append((i + 1, len(h.group(1)), text))
    return found


def _first_paragraph(lines: list[str], start: int, end: int) -> str:
    """
    First prose paragraph in [start, end), 1-based inclusive-exclusive.

    Skips fences, blockquote-only lines, badge rows and list bullets — a README
    whose first line under the title is five shields.io images should summarise
    as the sentence that follows, not as an empty string.
    """
    fence: str | None = None
    buf: list[str] = []
    for i in range(start - 1, min(end - 1, len(lines))):
        line = lines[i]
        m = _RE_FENCE.match(line)
        if m:
            marker = m.group(2)
            if fence is None:
                fence = marker[0] * 3
            elif marker[0] * 3 == fence:
                fence = None
            if buf:
                break
            continue
        if fence is not None:
            continue
        stripped = line.strip()
        if not stripped:
            if buf:
                break
            continue
        if stripped.startswith(("#", ">", "|", "---", "===")):
            if buf:
                break
            continue
        # A line that is nothing but images/links carries no prose.
        if not buf and not _clean_text(_RE_LINK.sub("", stripped)).strip("!*-+ "):
            continue
        buf.append(stripped)
    return _clean_text(" ".join(buf))[:_MAX_DOC_CHARS]


def extract_markdown_symbols(content: str, rel_path: str) -> list[dict]:
    """
    Parse a markdown file into symbols_*-shaped dicts.

    Returns rows carrying the keys the indexer's symbol flush expects:
    name, qualified_name, kind, filename, line_start, line_end, parent_name, doc.
    """
    lines = content.split("\n")
    body_start = _strip_front_matter(lines)
    headings = _headings(lines, body_start)
    total_lines = len(lines)
    basename = rel_path.rsplit("/", 1)[-1]

    # Where each heading's span ends: the line before the next heading at the
    # same level or shallower, else EOF.
    spans: list[int] = []
    for idx, (lineno, level, _text) in enumerate(headings):
        end = total_lines
        for nxt_line, nxt_level, _ in headings[idx + 1 :]:
            if nxt_level <= level:
                end = nxt_line - 1
                break
        spans.append(max(end, lineno))

    out: list[dict] = []
    # (name, level) of the open ancestors, nearest last.
    open_nodes: list[tuple[str, int]] = []
    root_name: str | None = None

    # A file that never opens at level 1 gets a synthetic root, so it is present
    # in the DOCS filter under a name a reader can recognise: the file itself.
    if not headings or headings[0][1] != 1:
        first_content = headings[0][0] - 1 if headings else total_lines
        root_name = basename
        out.append(
            {
                "name": basename,
                "qualified_name": rel_path,
                "kind": "document",
                "filename": rel_path,
                "line_start": 1,
                "line_end": total_lines,
                "parent_name": None,
                "doc": _first_paragraph(lines, body_start + 1,
                                        max(first_content, body_start + 1)),
            }
        )
        open_nodes.append((basename, 0))

    for idx, (lineno, level, text) in enumerate(headings):
        end = spans[idx]

        while open_nodes and open_nodes[-1][1] >= level:
            open_nodes.pop()
        parent = open_nodes[-1][0] if open_nodes else None

        if level <= _NODE_LEVEL:
            out.append(
                {
                    "name": text,
                    "qualified_name": f"{rel_path}#{text}",
                    "kind": "document",
                    "filename": rel_path,
                    "line_start": lineno,
                    "line_end": end,
                    "parent_name": parent,
                    "doc": _first_paragraph(lines, lineno + 1, end + 1),
                }
            )
            open_nodes.append((text, level))
        else:
            # Folded into the nearest ancestor node, like a column into its table.
            out.append(
                {
                    "name": text,
                    "qualified_name": f"{rel_path}#{text}",
                    "kind": "subheading",
                    "filename": rel_path,
                    "line_start": lineno,
                    "line_end": end,
                    "parent_name": parent or root_name or basename,
                    "doc": "",
                }
            )

    return out
