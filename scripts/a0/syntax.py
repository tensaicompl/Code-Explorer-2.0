"""Where the oracle's language tools see calls (A0.1, decision 64).

Call sites come from each language's own reference parser, never from the
structural engine's: CPython's `ast` for Python, rust-analyzer's parser
(`rust-analyzer parse`) for Rust. Every position is a byte offset into the file as
it is on disk.

Each call is reported with its *name token*: the identifier naming what is called,
the last segment of a path or the member after a dot, never a receiver. That token
is what a site of the structural engine and an oracle occurrence are joined on
(`methodology.md`, matching rule M1).
"""

from __future__ import annotations

import ast
import re
import subprocess
from dataclasses import dataclass


@dataclass(frozen=True)
class CallSite:
    """A call as the language's own parser sees it."""

    token: tuple[int, int] | None  # the name token's bytes; None when no name is called
    node: tuple[int, int]  # the whole call's bytes
    shape: str  # see SHAPES
    macro: str | None = None  # the enclosing macro invocation's name, for macro shapes


# call: a call of a name or path; method: a member call (`a.f()`); other: a call of
# an expression with no name (`(f)()`, `x[0]()`); macro_tt: call-shaped text inside
# a macro invocation's token tree; macro_rules_body: call-shaped text inside a
# `macro_rules!` definition.
SHAPES = ("call", "method", "other", "macro_tt", "macro_rules_body")


@dataclass(frozen=True)
class MacroInvocation:
    token: tuple[int, int]
    name: str
    tree: tuple[int, int]  # the token tree's bytes


def line_starts(data: bytes) -> list[int]:
    starts = [0]
    for i, b in enumerate(data):
        if b == 0x0A:
            starts.append(i + 1)
    return starts


# --- Python ------------------------------------------------------------------------


def python_calls(data: bytes) -> list[CallSite] | None:
    """Every call of a Python file, or None when CPython cannot parse it."""
    try:
        tree = ast.parse(data)
    except (SyntaxError, ValueError):
        return None
    starts = line_starts(data)

    def at(line: int, col: int) -> int:
        # ast columns are UTF-8 byte offsets within the line.
        return starts[line - 1] + col

    out = []
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        whole = (at(node.lineno, node.col_offset), at(node.end_lineno, node.end_col_offset))
        func = node.func
        if isinstance(func, ast.Name):
            s = at(func.lineno, func.col_offset)
            out.append(CallSite((s, s + len(func.id.encode())), whole, "call"))
        elif isinstance(func, ast.Attribute):
            e = at(func.end_lineno, func.end_col_offset)
            out.append(CallSite((e - len(func.attr.encode()), e), whole, "method"))
        else:
            out.append(CallSite(None, whole, "other"))
    out.sort(key=lambda c: (c.node, c.token or (-1, -1)))
    return out


def python_import_spans(data: bytes) -> list[tuple[int, int]]:
    """The bytes of every import statement of a Python file."""
    try:
        tree = ast.parse(data)
    except (SyntaxError, ValueError):
        return []
    starts = line_starts(data)
    out = []
    for node in ast.walk(tree):
        if isinstance(node, (ast.Import, ast.ImportFrom)):
            out.append((starts[node.lineno - 1] + node.col_offset,
                        starts[node.end_lineno - 1] + node.end_col_offset))
    return sorted(out)


# --- Rust --------------------------------------------------------------------------

_LINE = re.compile(r"^( *)([A-Z0-9_]+)@(\d+)\.\.(\d+)(?: (.*))?$")


@dataclass
class Node:
    kind: str
    start: int
    end: int
    text: str | None
    children: list["Node"]


def parse_tree(dump: str) -> Node:
    """rust-analyzer's syntax tree dump, as nodes."""
    root = None
    stack: list[tuple[int, Node]] = []
    for raw in dump.splitlines():
        m = _LINE.match(raw)
        if not m:
            continue  # a token whose text runs over lines continues the line before
        depth = len(m.group(1)) // 2
        node = Node(m.group(2), int(m.group(3)), int(m.group(4)), m.group(5), [])
        while stack and stack[-1][0] >= depth:
            stack.pop()
        if stack:
            stack[-1][1].children.append(node)
        else:
            root = node
        stack.append((depth, node))
    if root is None:
        raise ValueError("an empty syntax tree")
    return root


TRIVIA = {"WHITESPACE", "COMMENT"}


def _significant(children: list[Node]) -> list[Node]:
    return [c for c in children if c.kind not in TRIVIA]


def _name_ref_token(name_ref: Node) -> tuple[int, int] | None:
    for c in name_ref.children:
        if c.kind in ("IDENT", "SELF_TYPE_KW", "SELF_KW", "SUPER_KW", "CRATE_KW"):
            return (c.start, c.end)
    return None


def _last_segment_token(path: Node) -> tuple[int, int] | None:
    segment = None
    for c in path.children:
        if c.kind == "PATH_SEGMENT":
            segment = c
    if segment is None:
        return None
    for c in segment.children:
        if c.kind == "NAME_REF":
            return _name_ref_token(c)
    return None


def _path_name(path: Node, data: bytes) -> str:
    t = _last_segment_token(path)
    return data[t[0] : t[1]].decode("utf-8", "replace") if t else ""


def _tree_calls(tree: Node, shape: str, macro: str | None, out: list, invocations: list,
                data: bytes) -> None:
    """Call-shaped text in a token tree: an identifier, an optional turbofish, then a
    parenthesised token tree; and macro invocations nested in it."""
    items = _significant(tree.children)
    for i, tok in enumerate(items):
        if tok.kind == "TOKEN_TREE":
            _tree_calls(tok, shape, macro, out, invocations, data)
            continue
        if tok.kind != "IDENT":
            continue
        j = i + 1
        if j < len(items) and items[j].kind == "BANG" and j + 1 < len(items) \
                and items[j + 1].kind == "TOKEN_TREE":
            name = data[tok.start : tok.end].decode("utf-8", "replace")
            invocations.append(MacroInvocation((tok.start, tok.end), name,
                                               (items[j + 1].start, items[j + 1].end)))
            continue
        if j + 1 < len(items) and items[j].kind == "COLON2" and items[j + 1].kind == "L_ANGLE":
            depth = 0
            k = j + 1
            while k < len(items):
                if items[k].kind == "L_ANGLE":
                    depth += 1
                elif items[k].kind == "R_ANGLE":
                    depth -= 1
                    if depth == 0:
                        break
                elif items[k].kind == "SHR":
                    depth -= 2
                    if depth <= 0:
                        break
                k += 1
            j = k + 1
        if j < len(items) and items[j].kind == "TOKEN_TREE":
            opener = _significant(items[j].children)
            if opener and opener[0].kind == "L_PAREN":
                # The call starts where its path does: back over `segment ::` pairs.
                k = i
                while k >= 2 and items[k - 1].kind == "COLON2" and items[k - 2].kind in (
                        "IDENT", "SELF_TYPE_KW", "SELF_KW", "SUPER_KW", "CRATE_KW"):
                    k -= 2
                out.append(CallSite((tok.start, tok.end), (items[k].start, items[j].end),
                                    shape, macro))


def rust_calls(data: bytes, parse: callable) -> tuple[list[CallSite], list[MacroInvocation]]:
    """Every call and macro invocation of a Rust file. `parse` turns source text into
    rust-analyzer's syntax tree dump."""
    root = parse_tree(parse(data))
    calls: list[CallSite] = []
    invocations: list[MacroInvocation] = []

    def walk(node: Node, in_attr: bool) -> None:
        kind = node.kind
        if kind == "CALL_EXPR":
            sig = _significant(node.children)
            callee = sig[0] if sig else None
            token = None
            if callee is not None and callee.kind == "PATH_EXPR":
                path = next((c for c in callee.children if c.kind == "PATH"), None)
                token = _last_segment_token(path) if path else None
            calls.append(CallSite(token, (node.start, node.end), "call" if token else "other"))
        elif kind == "METHOD_CALL_EXPR":
            name_ref = next((c for c in node.children if c.kind == "NAME_REF"), None)
            token = _name_ref_token(name_ref) if name_ref else None
            calls.append(CallSite(token, (node.start, node.end), "method" if token else "other"))
        elif kind == "MACRO_CALL" and not in_attr:
            path = next((c for c in node.children if c.kind == "PATH"), None)
            tree = next((c for c in node.children if c.kind == "TOKEN_TREE"), None)
            if path is not None and tree is not None:
                name = _path_name(path, data)
                token = _last_segment_token(path)
                if token:
                    invocations.append(MacroInvocation(token, name, (tree.start, tree.end)))
                _tree_calls(tree, "macro_tt", name, calls, invocations, data)
            return
        elif kind == "MACRO_RULES" or kind == "MACRO_DEF":
            for c in node.children:
                if c.kind == "TOKEN_TREE":
                    _tree_calls(c, "macro_rules_body", None, calls, [], data)
            return
        elif kind in ("ATTR", "META"):
            in_attr = True
        for c in node.children:
            walk(c, in_attr)

    walk(root, False)
    calls.sort(key=lambda c: (c.node, c.token or (-1, -1), c.shape))
    invocations.sort(key=lambda m: (m.token, m.tree))
    return calls, invocations


def rust_analyzer_parser(binary: str, env: dict[str, str]):
    """`rust-analyzer parse`, reading source on standard input."""

    def parse(data: bytes) -> str:
        done = subprocess.run([binary, "parse"], input=data, capture_output=True, env=env,
                              check=True)
        return done.stdout.decode("utf-8", "replace")

    return parse


# --- Names of a structural site ------------------------------------------------------

_IDENT = re.compile(rb"[A-Za-z_][A-Za-z0-9_]*|r#[A-Za-z_][A-Za-z0-9_]*")


def callee_name_token(callee: bytes, start: int, language: str) -> tuple[int, int] | None:
    """The name token of a structural site's callee text, which starts at byte `start`:
    its last identifier outside generic arguments (Rust's `::<…>` and `<…>` after a
    path segment). None when the text names nothing."""
    depth = 0
    last = None
    i = 0
    n = len(callee)
    while i < n:
        b = callee[i : i + 1]
        if language == "rust" and b == b"<":
            depth += 1
            i += 1
            continue
        if language == "rust" and b == b">" and depth > 0:
            depth -= 1
            i += 1
            continue
        m = _IDENT.match(callee, i)
        if m:
            if depth == 0:
                last = (start + m.start(), start + m.end())
            i = m.end()
            continue
        i += 1
    return last
