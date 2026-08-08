"""
Protocol Buffer extraction -> service / rpc / message / enum symbols —

This is the first extractor that produces `service` and `endpoint` nodes, which
matters beyond protobuf itself: `NODE_TYPE_TO_CATEGORY` (GraphView.tsx:82) routes
`service` to the INFRA filter, and until now no node in any graph carried that
type, so the INFRA chip filtered nothing at all.

STORAGE, as for DDL: rows go into the EXISTING symbols_* table rather than
a new one, so the whole incremental pipeline — per-file hashing, delete-on-change,
flush batching — applies unchanged, and the chat tools can find a message by name.

Kinds emitted, and what the builder does with each:

    service     -> `service` node        (INFRA filter)
    rpc         -> `endpoint` node       (DATA filter), contained by its service
    message     -> `class` node          (CODE), nested messages contained by outer
    enum        -> `class` node          (CODE)
    field       -> FOLDED into the owning message's `fields` metadata
    enum_value  -> FOLDED into the owning enum's `fields` metadata
    type_ref    -> `depends_on` EDGE from an rpc to its request/response message

`type_ref`, NOT SQL's `references`, and the distinction is load-bearing. Both
become depends_on edges through the same builder machinery, but they resolve
against DIFFERENT candidate sets: a foreign key means a table, a protobuf type
reference means a message. Sharing one namespace looked harmless and is not —
in `ff`, 26 message names also name a Rust struct (the generated code for those
very messages), and 16 table names also name a class or a message. A shared map
would have drawn rpc -> Rust-struct edges and dropped the real ones as ambiguous.

Folding follows the column precedent: a 40-field message must cost one graph node,
not forty.

Modelled on an earlier TypeScript protobuf parser, with
four deliberate changes, because a set of line-anchored regexes over the whole
file loses things that matter here:

  1. NESTING. Upstream anchors every pattern with `^message` — no leading
     whitespace — so a message nested inside another is invisible, and fields are
     attributed by scanning forward from a match rather than by scope. Here a
     single brace-depth scan carries a frame stack, so nesting is exact and a
     `oneof` block does not steal its message's fields.
  2. DOC COMMENTS. Upstream discards them. The `//` block immediately above a
     declaration becomes its summary — the same free, author-written
     source the tree-sitter path already uses.
  3. PACKAGE QUALIFICATION. `package tact.api.alert;` prefixes qualified_name, so
     a message sharing a bare name with one in another package stays
     distinguishable in search — and lets the builder resolve a type reference
     unambiguously (below).
  4. RPC TYPE REFERENCES. Upstream records an rpc as a bare path string and drops
     its signature. Here request and response types become `references` rows,
     which the builder turns into `depends_on` edges — the same machinery SQL
     foreign keys already use. Without them the protobuf subgraph is a set of
     disconnected containers.

NOT HANDLED, stated rather than discovered later: proto2 extensions and groups,
`option` blocks (their braces are counted, their contents are not parsed), and
`import` statements — a proto import names a file, and no view consumes file→file
import edges for non-code today, so those rows would be written and never read.
"""

from __future__ import annotations

import re

_RE_BLOCK_COMMENT = re.compile(r"/\*.*?\*/", re.DOTALL)
_RE_STRING = re.compile(r'"(?:[^"\\]|\\.)*"')

_RE_PACKAGE = re.compile(r"^\s*package\s+([\w.]+)\s*;")
_RE_BLOCK = re.compile(r"^\s*(message|enum|service|oneof)\s+(\w+)")
_RE_RPC = re.compile(
    r"^\s*rpc\s+(\w+)\s*\(\s*(stream\s+)?([\w.]+)\s*\)\s*"
    r"returns\s*\(\s*(stream\s+)?([\w.]+)\s*\)"
)
# A field line: an optional label, a type (dotted or a map<>), a name, then
# `= <tag>`. The tag is what separates a field from every other construct.
_RE_FIELD = re.compile(
    r"^\s*(?:(repeated|optional|required)\s+)?"
    r"((?:map\s*<[^>]+>)|[\w.]+)\s+(\w+)\s*=\s*\d+"
)
_RE_ENUM_VALUE = re.compile(r"^\s*(\w+)\s*=\s*-?\d+")

# `oneof` is a scoping construct in the file but not a node: its fields belong to
# the enclosing message, which is what a reader means by "the message's fields".
_CONTAINER_KINDS = {"message", "enum", "service"}

# Scalar types are not messages; a reference to one would resolve to nothing.
_SCALARS = {
    "double", "float", "int32", "int64", "uint32", "uint64", "sint32", "sint64",
    "fixed32", "fixed64", "sfixed32", "sfixed64", "bool", "string", "bytes",
}

_MAX_DOC_CHARS = 400


def _mask_strings(line: str) -> str:
    """Blank string literals so a brace or `//` inside one is not read as code."""
    return _RE_STRING.sub(lambda m: " " * len(m.group(0)), line)


def _split_comment(line: str) -> tuple[str, str | None]:
    """Return (code, comment_text). comment_text is None when there is no `//`."""
    masked = _mask_strings(line)
    idx = masked.find("//")
    if idx == -1:
        return masked, None
    return masked[:idx], line[idx + 2 :].strip()


def _blank_block_comments(content: str) -> str:
    """Replace /* */ with same-length whitespace, preserving line numbers."""

    def repl(m: re.Match) -> str:
        return "".join(c if c == "\n" else " " for c in m.group(0))

    return _RE_BLOCK_COMMENT.sub(repl, content)


def _join_doc(lines: list[str]) -> str:
    return " ".join(l.strip() for l in lines if l.strip())[:_MAX_DOC_CHARS]


def extract_proto_symbols(content: str, rel_path: str) -> list[dict]:
    """
    Parse a .proto file into symbols_*-shaped dicts.

    Returns rows carrying the keys the indexer's symbol flush expects:
    name, qualified_name, kind, filename, line_start, line_end, parent_name, doc.
    """
    lines = _blank_block_comments(content).split("\n")

    out: list[dict] = []
    package = ""
    doc_buffer: list[str] = []
    stack: list[dict] = []  # frames: {kind, name, row|None, depth}
    depth = 0

    for lineno, raw in enumerate(lines, start=1):
        code, comment = _split_comment(raw)

        if comment is not None and not code.strip():
            doc_buffer.append(comment)
            continue
        if not code.strip():
            # A blank line ends a doc block. Without this, a licence header at the
            # top of the file would become the first declaration's summary.
            doc_buffer = []
            continue

        parent = next(
            (f for f in reversed(stack) if f["kind"] in _CONTAINER_KINDS), None
        )
        handled = False

        pkg = _RE_PACKAGE.match(code)
        block = _RE_BLOCK.match(code)
        rpc = _RE_RPC.match(code)

        if pkg:
            package = pkg.group(1)
            handled = True

        elif block:
            kind, name = block.group(1), block.group(2)
            if kind == "oneof":
                stack.append({"kind": "oneof", "name": name, "row": None,
                              "depth": depth})
            else:
                qualified = ".".join(
                    p for p in (package, parent["name"] if parent else "", name) if p
                )
                row = {
                    "name": name,
                    "qualified_name": qualified,
                    "kind": kind,
                    "filename": rel_path,
                    "line_start": lineno,
                    "line_end": lineno,
                    "parent_name": parent["name"] if parent else None,
                    "doc": _join_doc(doc_buffer),
                }
                out.append(row)
                stack.append({"kind": kind, "name": name, "row": row,
                              "depth": depth})
            handled = True

        elif rpc and parent and parent["kind"] == "service":
            name = rpc.group(1)
            out.append(
                {
                    "name": name,
                    "qualified_name": ".".join(
                        p for p in (package, parent["name"], name) if p
                    ),
                    "kind": "rpc",
                    "filename": rel_path,
                    "line_start": lineno,
                    "line_end": lineno,
                    "parent_name": parent["name"],
                    "doc": _join_doc(doc_buffer),
                }
            )
            # Request and response become depends_on edges's FK machinery).
            for type_name in (rpc.group(3), rpc.group(5)):
                if type_name in _SCALARS:
                    continue
                out.append(
                    {
                        "name": type_name,
                        "qualified_name": type_name,
                        "kind": "type_ref",
                        "filename": rel_path,
                        "line_start": lineno,
                        "line_end": lineno,
                        "parent_name": name,
                        "doc": "",
                    }
                )
            handled = True

        elif parent and parent["kind"] == "message":
            field = _RE_FIELD.match(code)
            if field:
                out.append(
                    {
                        "name": field.group(3),
                        "qualified_name": f"{parent['name']}.{field.group(3)}",
                        "kind": "field",
                        "filename": rel_path,
                        "line_start": lineno,
                        "line_end": lineno,
                        "parent_name": parent["name"],
                        "doc": "",
                    }
                )
                handled = True

        elif parent and parent["kind"] == "enum":
            value = _RE_ENUM_VALUE.match(code)
            if value:
                out.append(
                    {
                        "name": value.group(1),
                        "qualified_name": f"{parent['name']}.{value.group(1)}",
                        "kind": "enum_value",
                        "filename": rel_path,
                        "line_start": lineno,
                        "line_end": lineno,
                        "parent_name": parent["name"],
                        "doc": "",
                    }
                )
                handled = True

        if handled or code.strip():
            doc_buffer = []

        depth += code.count("{") - code.count("}")

        # Close every frame whose block ended on this line. Done once, at the end
        # of the iteration, so a single-line `message Foo {}` closes correctly.
        while stack and depth <= stack[-1]["depth"]:
            frame = stack.pop()
            if frame["row"] is not None:
                frame["row"]["line_end"] = lineno

    # An unbalanced file leaves frames open. Give them an honest end rather than a
    # line_end below line_start, which the builder would drop as a bad range.
    for frame in stack:
        if frame["row"] is not None:
            frame["row"]["line_end"] = len(lines)

    return out
