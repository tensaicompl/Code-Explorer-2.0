"""
HTTP/REST route extraction -> `route` symbols (endpoint nodes) —

THE GAP THIS CLOSES. `config_extract.py` states it plainly: "HTTP/REST
endpoints. framework-registry.ts cannot hold route data — its schema has no such
field — and it has no consumer in the pipeline at all." Until now the only
`endpoint` nodes in any graph came from protobuf rpcs and GraphQL, so a
FastAPI or Spring service indexed as zero endpoints, and business-flow synthesis
— which seeds a flow from an entry point — had almost nothing to seed from.

STORAGE, as for DDL and protobuf: rows go into the EXISTING symbols_* table with
a new `kind`, so per-file hashing, delete-on-change and flush batching all apply
unchanged. `route` is mapped to the `endpoint` node type in builder.py's
KIND_TO_TYPE; a kind that is in neither that table nor FUNCTION_KINDS is
reported by the unmapped-kinds check, so forgetting the mapping is loud.

NO EXTRA I/O AND NO SECOND PARSE. This runs from inside `_extract_file_symbols`'s
tree-sitter branch, taking the `content` already in memory and the `symbols`
tree-sitter already produced. Re-opening every source file for a second regex
pass, on every index run, would be pure waste.

REGEXES, NOT THE AST, and that is deliberate rather than lazy. Axum registers a
route with an ordinary method call — `.route("/x", get(handler))` — which is
indistinguishable at the grammar level from any other call without knowing that
the enclosing function returns `Router`. Encoding that would mean per-framework
special cases inside the AST walk that every language shares. The frameworks
each have one unmistakable textual form, so text is the right tool.

WHAT IT COVERS, and why those. Grounded in the five real indexed codebases, not
in a general survey:

    FastAPI/Flask   `@router.get("/x")`, `@app.post("/x")`   awp (17 files), shop
    Spring          `@GetMapping("/x")`, `@RequestMapping`   cso (16 controllers)
    Axum            `.route("/x", get(handler))`             ff (40+ route files)
    Express/Koa     `router.get("/x", handler)`              none yet — cheap
    Next.js/Remix   `export async function GET`              none yet — cheap

The first three are evidenced; the last two come from an earlier extractor and
marked as unvalidated here.

WHAT IT DOES NOT COVER, stated rather than discovered later:
  * Path composition. A FastAPI `APIRouter(prefix="/api/cart")` or a Spring
    class-level `@RequestMapping("/api/v1")` means the emitted path is the LEAF
    ("/items"), not the full URL. Resolving prefixes needs the router variable
    traced to its construction and its mounting, which is a call-graph problem,
    not a regex one. The leaf plus the enclosing class or file is enough to find
    the code, which is what an endpoint node is for.
  * gRPC and GraphQL, which `proto_defs.py` and `config_extract.py` already own,
    far more precisely than a line regex could.
"""

from __future__ import annotations

import re

# Frameworks name their verbs differently; the graph should not.
_ANY = "ANY"

_MAX_PATH_CHARS = 200

# --- Python / JS decorator-call form -------------------------------------
# `@router.get("/items")`, `@app.post("")`, `@blueprint.route("/x")`.
#
# The router variable is `(\w+)`, not a hardcoded `app`: awp registers 17 of its
# 18 route files on `@router.`, and the earlier pattern only matched `app.`,
# so it would have found 2 routes in that project out of more than a hundred.
#
# The path group is OPTIONAL and may be empty: shop writes `@router.get("")` for
# a collection root (cart.py:24, health.py:20), which a pattern requiring a
# leading `/` drops silently.
_RE_DECORATOR_CALL = re.compile(
    r"""@(\w+)\s*\.\s*(get|post|put|patch|delete|route)\s*\(\s*(?:['"]([^'"]*)['"])?""",
    re.IGNORECASE,
)

# --- Spring (Java / Kotlin) ----------------------------------------------
# Covers three forms real projects actually use, all of which the earlier pattern missed:
#   @GetMapping                      (bare, no parens)   AlertCenterController:55
#   @PutMapping("/x") @DeleteMapping @PatchMapping       (verbs it omitted)
#   @RequestMapping(value = "/x")    (kwarg form)        RuleController:246
_RE_SPRING = re.compile(
    r"""@(Get|Post|Put|Patch|Delete|Request)Mapping\b"""
    r"""(?:\s*\(\s*(?:value\s*=\s*)?"([^"]*)")?"""
)

# --- Rust / Axum ----------------------------------------------------------
# `.route("/{id}", get(get_handler))`. No earlier pattern covered this at all, which
# left ff — the largest indexed codebase — with an entirely invisible REST API.
# The handler name is captured directly, so no adjacency guessing is needed.
# `.nest("/flights", flights::routes())` is intentionally NOT matched: it
# composes prefixes rather than registering a handler.
_RE_AXUM = re.compile(
    r"""\.route\s*\(\s*"([^"]*)"\s*,\s*"""
    r"""(get|post|put|patch|delete|head|options)\s*\(\s*([\w:]+)\s*\)""",
)

# --- Express / Koa --------------------------------------------------------
# DELIBERATELY NARROW, and this is the correction that matters most in this
# file. `\w+\.get("/x")` also describes every HTTP CLIENT call ever written:
# measured against the real codebases, the wide form turned
# `apiClient.get("/monitoring/traffic-counts")` (ff TrafficOverview.tsx:176)
# and `http.post('/api/v1/config/obm/test')` (cso Connectivity.test.tsx:112)
# into "endpoints", inflating ff from a handful of real routes to 546 mostly
# imaginary ones. An endpoint node means "requests enter the system HERE"; a
# node that actually means "this UI calls out to somewhere else" is worse than
# a missing one, because it is confidently wrong.
#
# Two constraints restore precision: the receiver must LOOK like a server (bare
# `app`/`router`/`server`, or a `…Router`/`…App` suffix), and a handler argument
# must follow the path — clients take options or nothing, routes take a handler.
_RE_EXPRESS = re.compile(
    r"""\b(app|router|server|\w*(?:Router|App))\s*\.\s*"""
    r"""(get|post|put|patch|delete|all)\s*\(\s*['"](/[^'"]*)['"]\s*,""",
)

# --- Next.js / Remix ------------------------------------------------------
# The FILE PATH is the route here; the match only supplies the verb.
_RE_NEXTJS = re.compile(
    r"""export\s+(?:async\s+)?function\s+(GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS)\b""",
)

_PY_JS_EXTS = {".py", ".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs"}
_JVM_EXTS = {".java", ".kt"}
_RUST_EXTS = {".rs"}
_JS_EXTS = {".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs"}

# A decorator and the function it decorates are adjacent, but not always on
# consecutive lines: cso stacks `@GetMapping` above `@PreAuthorize(...)` above
# the method (AlertCenterController.java:55-57). Wide enough for a few stacked
# annotations, narrow enough that the next unrelated function is not claimed.
_HANDLER_SEARCH_LINES = 15

_CALLABLE_KINDS = {"function", "method", "constructor"}


# Comments and docstrings, per language family. Blanked before scanning.
_RE_TRIPLE = re.compile(r'"""[\s\S]*?"""|\'\'\'[\s\S]*?\'\'\'')
_RE_HASH_COMMENT = re.compile(r"#[^\n]*")
_RE_BLOCK_COMMENT = re.compile(r"/\*[\s\S]*?\*/")
_RE_LINE_COMMENT = re.compile(r"//[^\n]*")


def _blank(match: re.Match) -> str:
    """Replace a span with spaces, keeping newlines so offsets and lines hold."""
    return "".join(c if c == "\n" else " " for c in match.group(0))


def _mask_noncode(content: str, ext: str) -> str:
    """
    Blank comments and docstrings, preserving every character offset.

    Without this, a usage example in a docstring becomes an endpoint. That is
    not hypothetical: awp's `database.py:51` documents `@router.get("/workflows")`
    inside `get_db`'s docstring, and the graph gained a `/workflows` endpoint in
    a module that serves no requests at all.

    Only comments and TRIPLE-quoted blocks are masked. Ordinary string literals
    are left alone because the route path itself is one.
    """
    if ext == ".py":
        content = _RE_TRIPLE.sub(_blank, content)
        return _RE_HASH_COMMENT.sub(_blank, content)
    content = _RE_BLOCK_COMMENT.sub(_blank, content)
    return _RE_LINE_COMMENT.sub(_blank, content)


def _line_of(content: str, index: int) -> int:
    """1-based line number of a character offset."""
    return content.count("\n", 0, index) + 1


def _handler_after(symbols: list[dict], line: int) -> dict | None:
    """
    The first callable defined at or below `line`, within the search window.

    Mirrors the adjacency assumption `_doc_for_symbol` already makes in
    build_corpus.py — a decorator belongs to the next definition below it.
    """
    best = None
    for sym in symbols:
        if sym.get("kind") not in _CALLABLE_KINDS:
            continue
        start = sym.get("line_start") or 0
        if line <= start <= line + _HANDLER_SEARCH_LINES:
            if best is None or start < best["line_start"]:
                best = sym
    return best


def _enclosing_class(symbols: list[dict], line: int) -> dict | None:
    """The class whose line range contains `line` — Spring's controller."""
    for sym in symbols:
        if sym.get("kind") != "class":
            continue
        start, end = sym.get("line_start") or 0, sym.get("line_end") or 0
        if start <= line <= end:
            return sym
    return None


def _nextjs_path(filename: str) -> str:
    """
    Derive the URL from a Next.js App Router file path.

    `app/api/users/[id]/route.ts` -> `/api/users/{id}`. The convention IS the
    route here, so the path comes from the filename rather than from the match.
    """
    parts = filename.replace("\\", "/").split("/")
    if parts and parts[-1].split(".")[0] in ("route", "page"):
        parts = parts[:-1]
    for marker in ("app", "pages"):
        if marker in parts:
            parts = parts[parts.index(marker) + 1 :]
            break
    if parts and parts[0] == "api":
        pass  # keep it: /api is part of the URL
    segments = [
        "{" + p.strip("[]") + "}" if p.startswith("[") and p.endswith("]") else p
        for p in parts
        if p
    ]
    return "/" + "/".join(segments)


def _row(verb: str, path: str, filename: str, line: int, handler: str | None,
         parent: str | None) -> dict:
    """
    One `route` symbol, in the shape persist_symbols writes.

    `name` is "VERB /path" because that is what a person searches for and what
    the endpoint node shows. The handler goes in `doc` rather than the name: it
    is useful context, but two routes on the same handler must still read as two
    different endpoints.
    """
    path = (path or "/")[:_MAX_PATH_CHARS]
    if not path.startswith("/"):
        path = "/" + path
    return {
        "name": f"{verb.upper()} {path}",
        "qualified_name": f"{filename}:{verb.upper()} {path}",
        "kind": "route",
        "filename": filename,
        "line_start": line,
        "line_end": line,
        "parent_name": parent,
        "doc": f"Handled by {handler}." if handler else "",
    }


def extract_http_route_symbols(
    content: str, ext: str, filename: str, ts_symbols: list[dict] | None = None,
) -> list[dict]:
    """
    Detect HTTP routes in one already-read source file.

    `ts_symbols` is the tree-sitter output for the SAME file, used only to name
    the handler and find the enclosing controller class — never re-parsed here.
    """
    symbols = ts_symbols or []
    # Scan CODE only. A route in a comment or a docstring example is not a route.
    content = _mask_noncode(content, ext)
    out: list[dict] = []
    seen: set[tuple[str, str, int]] = set()

    def add(verb: str, path: str, line: int, handler: str | None, parent: str | None):
        key = (verb.upper(), path or "/", line)
        if key in seen:
            return
        seen.add(key)
        out.append(_row(verb, path, filename, line, handler, parent))

    if ext in _PY_JS_EXTS:
        for m in _RE_DECORATOR_CALL.finditer(content):
            verb = m.group(2).lower()
            line = _line_of(content, m.start())
            handler = _handler_after(symbols, line)
            add(
                _ANY if verb == "route" else verb,
                m.group(3) if m.group(3) is not None else "",
                line,
                handler["name"] if handler else None,
                handler["name"] if handler else None,
            )

    if ext in _JVM_EXTS:
        for m in _RE_SPRING.finditer(content):
            verb = m.group(1)
            line = _line_of(content, m.start())
            cls = _enclosing_class(symbols, line)
            # A class-level @RequestMapping declares the controller's base path,
            # not a route. Skipping it keeps a phantom endpoint per controller
            # out of the graph.
            if verb == "Request" and cls and (cls.get("line_start") or 0) >= line:
                continue
            handler = _handler_after(symbols, line)
            add(
                _ANY if verb == "Request" else verb,
                m.group(2) or "",
                line,
                handler["name"] if handler else None,
                cls["name"] if cls else (handler["name"] if handler else None),
            )

    if ext in _RUST_EXTS:
        for m in _RE_AXUM.finditer(content):
            handler = m.group(3).rsplit("::", 1)[-1]
            add(m.group(2), m.group(1), _line_of(content, m.start()), handler, handler)

    if ext in _JS_EXTS:
        for m in _RE_EXPRESS.finditer(content):
            line = _line_of(content, m.start())
            handler = _handler_after(symbols, line)
            add(
                m.group(2), m.group(3), line,
                handler["name"] if handler else None,
                handler["name"] if handler else None,
            )
        for m in _RE_NEXTJS.finditer(content):
            add(m.group(1), _nextjs_path(filename), _line_of(content, m.start()),
                m.group(1), None)

    return out
