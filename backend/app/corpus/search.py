"""The search tools the agent actually calls.

Eight of them, each answering one question about one corpus: what is in this
directory, what does this concept look like in code, where is this file, show me
these lines, find this pattern, where is this symbol defined and used, what calls
what, and what does this subsystem depend on.

They share a contract. Every tool takes a registry key as its first argument,
returns a plain dict, and reports failure as {"error": ...} rather than raising
-- the caller is an agent loop that must be able to show the model what went
wrong and let it try something else. Nothing here formats prose for a human; the
dicts are serialised to JSON and handed to the model as tool results.

`run_tool` at the bottom is the single dispatch point. Its keys are the tool
names the model sees, and they must stay in step with the schema in
definitions.py -- see the note there.
"""
from __future__ import annotations

import fnmatch as _fnmatch_mod
import json
import logging
import os
import re
import time
from collections import Counter
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path
from typing import Dict, List, Optional

import psycopg2
import psycopg2.extras

from ..config import (
    CODEBASE_DIR,
    MAX_FILE_READ_LINES,
    MAX_RESULTS_DEFAULT,
    SEARCH_TIMEOUT_SECONDS,
)
from .catalog import (
    CORPUS_REGISTRY,
    IGNORED_DIRS,
    SOURCE_EXTENSIONS,
    ensure_corpus_registry,
    sanitize_registry_key,
)
from .db import get_pooled_connection, load_embedding_model, release_pooled_connection

logger = logging.getLogger("praxevia.corpus.search")

# ============================================================
# Semantic search via pgvector
# ============================================================

def search_by_meaning(project: str, query: str, max_results: int = 20) -> dict:
    """Search for relevant code chunks using semantic similarity via pgvector."""
    if project not in CORPUS_REGISTRY:
        available = ", ".join(sorted(CORPUS_REGISTRY.keys()))
        return {"error": f"Unknown project: {project}. Available: {available}", "matches": [], "count": 0}

    table_name = f"code_chunks_{project}"

    if not CORPUS_REGISTRY[project].get("indexed"):
        return {
            "error": f"Project '{project}' has not been indexed yet. Run the indexer first.",
            "matches": [], "count": 0,
        }

    # Compute query embedding
    model = load_embedding_model()
    query_embedding = model.encode(query).tolist()
    # pgvector expects a string like '[0.1,0.2,...]'
    embedding_str = "[" + ",".join(str(x) for x in query_embedding) + "]"

    conn = None
    try:
        conn = get_pooled_connection()
        cur = conn.cursor(cursor_factory=psycopg2.extras.RealDictCursor)
        cur.execute("SET LOCAL ivfflat.probes = 10")
        cur.execute(
            f"""
            SELECT filename, code, start_line, end_line,
                   1 - (embedding <=> %s::vector) AS similarity
            FROM "{table_name}"
            ORDER BY embedding <=> %s::vector
            LIMIT %s
            """,
            (embedding_str, embedding_str, max_results),
        )
        rows = cur.fetchall()
        cur.close()
    except Exception as e:
        return {"error": f"Database query failed: {e}", "matches": [], "count": 0}
    finally:
        if conn:
            release_pooled_connection(conn)

    matches = []
    for row in rows:
        matches.append({
            "file_path": row["filename"],
            "chunk_content": row["code"],
            "start_line": row.get("start_line", 0),
            "end_line": row.get("end_line", 0),
            "similarity": round(float(row["similarity"]), 4),
        })

    return {
        "matches": matches,
        "count": len(matches),
        "query": query,
    }


# ============================================================
# Directory listing (Fix 2)
# ============================================================

def _dominant_extensions(directory: Path, max_exts: int = 3) -> str:
    """Return the top file extensions found directly under a directory (non-recursive)."""
    counts: Counter = Counter()
    try:
        for child in directory.iterdir():
            if child.is_file() and child.suffix:
                counts[child.suffix.lower()] += 1
            elif child.is_dir():
                # One level deeper — capped to avoid scanning thousands of files
                try:
                    for grandchild in list(child.iterdir())[:50]:
                        if grandchild.is_file() and grandchild.suffix:
                            counts[grandchild.suffix.lower()] += 1
                except Exception:
                    pass
    except Exception:
        return ""
    top = [ext for ext, _ in counts.most_common(max_exts)]
    return " ".join(top) if top else ""


def browse_source_tree(project: str, path: str = "", depth: int = 3) -> dict:
    """Return directory tree up to `depth` levels for structural exploration."""
    proj = _resolve_project(project)
    if not proj:
        return {"error": f"Unknown project: {project}", "tree": []}

    codebase_dir: Path = proj["codebase_dir"]
    base = codebase_dir / path if path else codebase_dir

    # Resolve and guard against path traversal
    try:
        base = base.resolve()
        base.relative_to(codebase_dir.resolve())
    except (ValueError, FileNotFoundError):
        return {"error": f"Path not found or not allowed: {path}", "tree": []}

    if not base.exists():
        return {"error": f"Path not found: {path}", "tree": []}

    def _walk(directory: Path, current_depth: int) -> list:
        entries = []
        try:
            children = sorted(directory.iterdir(), key=lambda p: (p.is_file(), p.name.lower()))
        except PermissionError:
            return entries

        dirs = [c for c in children if c.is_dir() and not c.name.startswith(".") and c.name not in IGNORED_DIRS]
        files = [c for c in children if c.is_file()]

        for d in dirs:
            node = {
                "name": d.name,
                "type": "dir",
                "path": str(d.relative_to(codebase_dir)),
            }
            # Always include dominant extensions so Claude knows the language
            exts = _dominant_extensions(d)
            if exts:
                node["lang"] = exts

            if current_depth < depth:
                node["children"] = _walk(d, current_depth + 1)
            else:
                try:
                    sub_dirs = sum(1 for c in d.iterdir() if c.is_dir())
                    sub_files = sum(1 for c in d.iterdir() if c.is_file())
                    node["summary"] = f"{sub_dirs} dirs, {sub_files} files"
                except Exception:
                    pass
            entries.append(node)

        # Only list files at the requested path level (not in recursion)
        if current_depth == 1:
            for f in files[:20]:
                entries.append({
                    "name": f.name,
                    "type": "file",
                    "path": str(f.relative_to(codebase_dir)),
                })
            if len(files) > 20:
                entries.append({"name": f"... (+{len(files) - 20} more files)", "type": "info"})

        return entries

    tree = _walk(base, 1)
    return {
        "path": path or "/",
        "tree": tree,
        "total_dirs": sum(1 for e in tree if e.get("type") == "dir"),
    }


# ============================================================
# Tool definitions - built dynamically per project
# ============================================================


def _resolve_project(project: str) -> Optional[dict]:
    """Get project info from registry. Returns None if not found."""
    return CORPUS_REGISTRY.get(project)


def find_files_by_name(project: str, filename_pattern: str, path_contains: str = "", max_results: int = 50) -> dict:
    proj = _resolve_project(project)
    if not proj:
        return {"error": f"Unknown project: {project}", "files": [], "count": 0}

    codebase_dir = proj["codebase_dir"]
    if not codebase_dir.exists():
        return {"error": f"Codebase directory not found: {codebase_dir}", "files": [], "count": 0}

    matches = []
    total = 0
    deadline = time.monotonic() + SEARCH_TIMEOUT_SECONDS

    for root, dirs, files in os.walk(codebase_dir):
        dirs[:] = [d for d in dirs if d not in IGNORED_DIRS and not d.startswith(".")]
        if time.monotonic() > deadline:
            return {
                "files": matches,
                "count": len(matches),
                "truncated": True,
                "error": "Search timed out - try a more specific pattern",
            }

        for fname in files:
            if _fnmatch_mod.fnmatch(fname.lower(), filename_pattern.lower()):
                full_path = os.path.join(root, fname)
                rel_path = os.path.relpath(full_path, codebase_dir)
                if path_contains and path_contains not in rel_path:
                    continue
                total += 1
                if len(matches) < max_results:
                    matches.append(rel_path)

    result = {"files": matches, "count": total}
    if total > max_results:
        result["truncated"] = True
        result["showing"] = max_results
    return result


def _extract_ada_subprogram(all_lines: List[str], name: str) -> Optional[dict]:
    """Extract an Ada procedure/function body by name."""
    pattern = re.compile(
        r'\b(procedure|function)\s+' + re.escape(name) + r'\b',
        re.IGNORECASE,
    )
    end_pattern = re.compile(
        r'^\s*end\s+' + re.escape(name) + r'\s*;',
        re.IGNORECASE,
    )

    start_idx = None
    for i, line in enumerate(all_lines):
        if pattern.search(line):
            start_idx = i
            break

    if start_idx is None:
        return None

    end_idx = None
    for i in range(start_idx + 1, len(all_lines)):
        if end_pattern.search(all_lines[i]):
            end_idx = i
            break

    if end_idx is None:
        end_idx = min(start_idx + MAX_FILE_READ_LINES - 1, len(all_lines) - 1)

    extracted = [all_lines[j].rstrip("\n") for j in range(start_idx, end_idx + 1)]
    return {
        "start_line": start_idx + 1,
        "end_line": end_idx + 1,
        "content": "\n".join(extracted),
    }


def read_source_excerpt(project: str, file_path: str, start_line: int = 1, end_line: int = 0, procedure_name: str = "") -> dict:
    proj = _resolve_project(project)
    if not proj:
        return {"error": f"Unknown project: {project}", "content": ""}

    codebase_dir = proj["codebase_dir"]
    full_path = codebase_dir / file_path
    if not full_path.exists():
        for child in codebase_dir.iterdir():
            candidate = codebase_dir / child.name / file_path
            if candidate.exists():
                full_path = candidate
                break

    if not full_path.exists():
        return {"error": f"File not found: {file_path}", "content": ""}

    try:
        full_path.resolve().relative_to(codebase_dir.resolve())
    except ValueError:
        return {"error": "Path traversal not allowed", "content": ""}

    # Procedure extraction mode
    if procedure_name:
        try:
            with open(full_path, "r", encoding="utf-8", errors="replace") as f:
                all_lines = f.readlines()
        except Exception as e:
            return {"error": str(e), "content": ""}

        result = _extract_ada_subprogram(all_lines, procedure_name)
        if result is None:
            return {
                "error": f"Procedure/function '{procedure_name}' not found in {file_path}",
                "content": "",
                "total_lines_in_file": len(all_lines),
            }
        result["file"] = file_path
        result["total_lines_in_file"] = len(all_lines)
        result["procedure_name"] = procedure_name
        return result

    # Standard line-range mode
    if end_line <= 0:
        end_line = start_line + MAX_FILE_READ_LINES - 1

    lines = []
    total_lines = 0
    try:
        with open(full_path, "r", encoding="utf-8", errors="replace") as f:
            for i, line in enumerate(f, 1):
                total_lines = i
                if i < start_line:
                    continue
                if i > end_line:
                    break
                lines.append(line.rstrip("\n"))
    except Exception as e:
        return {"error": str(e), "content": ""}

    return {
        "file": file_path,
        "start_line": start_line,
        "end_line": min(end_line, start_line + len(lines) - 1) if lines else start_line,
        "total_lines_in_file": total_lines,
        "content": "\n".join(lines),
    }


def regex_search_source(project: str, pattern: str, file_extension: str = "", path_contains: str = "", max_results: int = 200, context_lines: int = 0) -> dict:
    proj = _resolve_project(project)
    if not proj:
        return {"error": f"Unknown project: {project}", "matches": [], "count": 0}

    codebase_dir = proj["codebase_dir"]
    if not codebase_dir.exists():
        return {"error": f"Codebase directory not found: {codebase_dir}", "matches": [], "count": 0}

    try:
        compiled = re.compile(pattern, re.IGNORECASE)
    except re.error as e:
        return {"error": f"Invalid regex: {e}", "matches": [], "count": 0}

    ctx = max(0, min(context_lines, 30))

    matches = []
    total = 0
    files_searched = 0
    deadline = time.monotonic() + SEARCH_TIMEOUT_SECONDS

    for root, dirs, files in os.walk(codebase_dir):
        dirs[:] = [d for d in dirs if d not in IGNORED_DIRS and not d.startswith(".")]
        if time.monotonic() > deadline:
            break

        for fname in files:
            ext = os.path.splitext(fname)[1].lower()
            if ext not in SOURCE_EXTENSIONS:
                continue
            if file_extension and ext != file_extension.lower():
                continue

            full_path = os.path.join(root, fname)
            rel_path = os.path.relpath(full_path, codebase_dir)

            if path_contains and path_contains not in rel_path:
                continue

            files_searched += 1
            try:
                if ctx > 0:
                    with open(full_path, "r", encoding="utf-8", errors="replace") as f:
                        all_lines = f.readlines()
                    for line_num_0, line in enumerate(all_lines):
                        if compiled.search(line):
                            total += 1
                            if len(matches) < max_results:
                                ln = line_num_0 + 1
                                start = max(0, line_num_0 - ctx)
                                end = min(len(all_lines), line_num_0 + ctx + 1)
                                context_block = []
                                for ci in range(start, end):
                                    prefix = ">>" if ci == line_num_0 else "  "
                                    context_block.append(f"{prefix} {ci + 1}: {all_lines[ci].rstrip()}")
                                matches.append(f"{rel_path}:{ln}:\n" + "\n".join(context_block))
                            if total >= max_results * 2:
                                break
                else:
                    with open(full_path, "r", encoding="utf-8", errors="replace") as f:
                        for line_num, line in enumerate(f, 1):
                            if compiled.search(line):
                                total += 1
                                if len(matches) < max_results:
                                    matches.append(f"{rel_path}:{line_num}: {line.rstrip()}")
                                if total >= max_results * 2:
                                    break
            except Exception:
                continue

            if time.monotonic() > deadline:
                break

    result = {
        "matches": matches,
        "count": total,
        "files_searched": files_searched,
    }
    if total > max_results:
        result["truncated"] = True
        result["showing"] = max_results
    if time.monotonic() > deadline:
        result["timed_out"] = True
    return result


# ============================================================
# Symbol index search
# ============================================================

def lookup_symbol_usage(project: str, symbol_name: str, kind: str = "") -> dict:
    """Query symbol index for definitions, callers, and callees."""
    if project not in CORPUS_REGISTRY:
        available = ", ".join(sorted(CORPUS_REGISTRY.keys()))
        return {"error": f"Unknown project: {project}. Available: {available}"}

    sym_table = f"symbols_{project}"
    call_table = f"calls_{project}"

    conn = None
    try:
        conn = get_pooled_connection()
        cur = conn.cursor(cursor_factory=psycopg2.extras.RealDictCursor)

        # Check if symbol tables exist
        cur.execute(
            "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = %s)",
            (sym_table,),
        )
        if not cur.fetchone()["exists"]:
            cur.close()
            return {"error": f"Symbol index not available for project '{project}'. Re-run the indexer."}

        # Find definitions — support qualified names (dots)
        has_dot = '.' in symbol_name
        if has_dot:
            if kind:
                cur.execute(
                    f'SELECT name, qualified_name, kind, filename, line_start, line_end, parent_name '
                    f'FROM "{sym_table}" WHERE (lower(name) = lower(%s) OR lower(qualified_name) = lower(%s)) AND kind = %s '
                    f'ORDER BY filename, line_start LIMIT 100',
                    (symbol_name, symbol_name, kind),
                )
            else:
                cur.execute(
                    f'SELECT name, qualified_name, kind, filename, line_start, line_end, parent_name '
                    f'FROM "{sym_table}" WHERE (lower(name) = lower(%s) OR lower(qualified_name) = lower(%s)) '
                    f'ORDER BY filename, line_start LIMIT 100',
                    (symbol_name, symbol_name),
                )
        else:
            if kind:
                cur.execute(
                    f'SELECT name, qualified_name, kind, filename, line_start, line_end, parent_name '
                    f'FROM "{sym_table}" WHERE lower(name) = lower(%s) AND kind = %s '
                    f'ORDER BY filename, line_start LIMIT 100',
                    (symbol_name, kind),
                )
            else:
                cur.execute(
                    f'SELECT name, qualified_name, kind, filename, line_start, line_end, parent_name '
                    f'FROM "{sym_table}" WHERE lower(name) = lower(%s) '
                    f'ORDER BY filename, line_start LIMIT 100',
                    (symbol_name,),
                )
        definitions = [dict(row) for row in cur.fetchall()]

        # Find callers (who calls this symbol?) — include call_context and subsystem
        cur.execute(
            f'SELECT caller_name, caller_file, caller_line, COALESCE(call_context, \'\') as call_context, '
            f'split_part(caller_file, \'/\', 1) AS caller_subsystem '
            f'FROM "{call_table}" '
            f'WHERE lower(callee_name) = lower(%s) '
            f'ORDER BY caller_file, caller_line LIMIT 200',
            (symbol_name,),
        )
        callers = [dict(row) for row in cur.fetchall()]

        # Also try matching dotted callee names (e.g. "Pkg.Func")
        cur.execute(
            f'SELECT caller_name, caller_file, caller_line, COALESCE(call_context, \'\') as call_context, '
            f'split_part(caller_file, \'/\', 1) AS caller_subsystem '
            f'FROM "{call_table}" '
            f'WHERE callee_name ILIKE %s AND lower(callee_name) != lower(%s) '
            f'ORDER BY caller_file, caller_line LIMIT 100',
            (f"%.{symbol_name}", symbol_name),
        )
        callers_dotted = [dict(row) for row in cur.fetchall()]
        callers.extend(callers_dotted)

        # Find callees (what does this symbol call?) — include call_context
        cur.execute(
            f'SELECT callee_name, caller_file, caller_line, COALESCE(call_context, \'\') as call_context '
            f'FROM "{call_table}" '
            f'WHERE lower(caller_name) = lower(%s) '
            f'ORDER BY caller_file, caller_line LIMIT 200',
            (symbol_name,),
        )
        callees = [dict(row) for row in cur.fetchall()]

        cur.close()
    except Exception as e:
        return {"error": f"Symbol query failed: {e}"}
    finally:
        if conn:
            release_pooled_connection(conn)

    return {
        "symbol": symbol_name,
        "definitions": definitions,
        "definition_count": len(definitions),
        "callers": callers,
        "caller_count": len(callers),
        "callees": callees,
        "callee_count": len(callees),
    }


# ============================================================
# Transitive call graph tracing
# ============================================================

def walk_call_chain(project: str, symbol_name: str, direction: str = "callers",
                     max_depth: int = 6, file_filter: str = "",
                     max_results: int = 500) -> dict:
    """Trace transitive call chains using recursive CTE."""
    if project not in CORPUS_REGISTRY:
        available = ", ".join(sorted(CORPUS_REGISTRY.keys()))
        return {"error": f"Unknown project: {project}. Available: {available}"}

    call_table = f"calls_{project}"
    sym_table = f"symbols_{project}"
    max_depth = min(max(1, max_depth), 10)
    max_results = min(max(1, max_results), 2000)

    conn = None
    try:
        conn = get_pooled_connection()
        cur = conn.cursor(cursor_factory=psycopg2.extras.RealDictCursor)

        # Check if call table exists
        cur.execute(
            "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = %s)",
            (call_table,),
        )
        if not cur.fetchone()["exists"]:
            cur.close()
            return {"error": f"Symbol index not available for project '{project}'. Re-run the indexer."}

        # Extract the base name (last component after dot) for matching
        base_name = symbol_name.split('.')[-1] if '.' in symbol_name else symbol_name
        is_qualified = '.' in symbol_name

        # When a qualified name is provided (e.g. "Package.Class.Derive"),
        # pre-check for matching symbols to disambiguate from unrelated symbols
        # with the same base name. Strategy: split into prefix + base, find symbols
        # where qualified_name starts with prefix AND base name matches.
        # This runs a fast indexed query ONCE, not per-row in the CTE.
        callee_name_filter = ""
        if is_qualified:
            prefix = symbol_name.rsplit('.', 1)[0]  # "Package.Class"
            cur.execute(
                f'SELECT DISTINCT lower(qualified_name) FROM "{sym_table}" '
                f'WHERE lower(name) = lower(%s) '
                f'  AND (lower(qualified_name) = lower(%s) '
                f'    OR lower(qualified_name) LIKE lower(%s))',
                (base_name, symbol_name, f"{prefix}.%"),
            )
            matched_qnames = [row["lower"] for row in cur.fetchall()]
            if matched_qnames:
                # Build a callee_name filter using the resolved qualified names.
                # In the calls table, callee_name can be unqualified ("Derive"),
                # partially qualified ("Profile.Derive"), or fully qualified.
                # We match on: exact qualified name OR any suffix of matched qnames.
                suffix_patterns = set()
                for qn in matched_qnames:
                    parts = qn.split('.')
                    for i in range(len(parts)):
                        suffix_patterns.add('.'.join(parts[i:]))
                placeholders = ",".join(["%s"] * len(suffix_patterns))
                callee_name_filter = (
                    f"AND lower(callee_name) IN ({placeholders})"
                )
                matched_suffixes = list(suffix_patterns)

        results = {}

        if direction in ("callers", "both"):
            # Trace callers: who calls this symbol, who calls those callers, etc.
            file_filter_clause = ""
            params = [base_name, f"%.{base_name}"]
            if callee_name_filter:
                params.extend(matched_suffixes)
            params.append(max_depth)
            if file_filter:
                file_filter_clause = f"AND cc_final.caller_file ILIKE %s"
                params.append(file_filter)

            cur.execute(
                f"""
                WITH RECURSIVE call_chain AS (
                    -- Seed: direct callers of the target symbol
                    SELECT caller_name, callee_name, caller_file, caller_line,
                           COALESCE(call_context, '') as call_context,
                           1 as depth, ARRAY[lower(callee_name)] as path
                    FROM "{call_table}"
                    WHERE (lower(callee_name) = lower(%s)
                       OR callee_name ILIKE %s)
                    {callee_name_filter}
                    UNION ALL
                    -- Recursive: callers of callers
                    -- Strategy A: exact match on qualified caller_name (cross-package calls)
                    -- Strategy B: base-name match within the same file (within-package unqualified calls)
                    SELECT c.caller_name, c.callee_name, c.caller_file, c.caller_line,
                           COALESCE(c.call_context, '') as call_context,
                           cc.depth + 1, cc.path || lower(c.callee_name)
                    FROM "{call_table}" c
                    JOIN call_chain cc ON (
                        lower(c.callee_name) = lower(cc.caller_name)
                        OR (
                            c.caller_file = cc.caller_file
                            AND position('.' IN cc.caller_name) > 0
                            AND lower(c.callee_name) = lower(
                                substring(cc.caller_name FROM '([^.]+)$')
                            )
                        )
                    )
                    WHERE cc.depth < %s
                      AND lower(c.caller_name) != ALL(cc.path)
                )
                SELECT DISTINCT caller_name, callee_name, caller_file, caller_line, call_context, depth,
                       split_part(cc_final.caller_file, '/', 1) AS caller_subsystem
                FROM call_chain cc_final
                WHERE 1=1 {file_filter_clause}
                ORDER BY depth, caller_file, caller_line
                LIMIT {max_results}
                """,
                params,
            )
            results["callers"] = [dict(row) for row in cur.fetchall()]
            results["caller_count"] = len(results["callers"])
            if results["caller_count"] >= max_results:
                results["caller_truncated"] = True
                results["caller_note"] = (
                    f"WARNING: Results truncated at {max_results} edges. The full call graph is larger. "
                    "Use file_filter to narrow results (e.g. '%checkout%'), increase max_results, "
                    "or use a more specific qualified symbol_name."
                )

        if direction in ("callees", "both"):
            # Trace callees: what does this symbol call, what do those callees call, etc.
            file_filter_clause = ""
            # Match on full qualified name, base name, and suffix for the callees seed
            params = [symbol_name, base_name, f"%.{base_name}", max_depth]
            if file_filter:
                file_filter_clause = f"AND cc_final.caller_file ILIKE %s"
                params.append(file_filter)

            cur.execute(
                f"""
                WITH RECURSIVE call_chain AS (
                    -- Seed: direct callees of the target symbol
                    SELECT caller_name, callee_name, caller_file, caller_line,
                           COALESCE(call_context, '') as call_context,
                           1 as depth, ARRAY[lower(caller_name)] as path
                    FROM "{call_table}"
                    WHERE lower(caller_name) = lower(%s)
                       OR lower(caller_name) = lower(%s)
                       OR caller_name ILIKE %s
                    UNION ALL
                    -- Recursive: callees of callees
                    SELECT c.caller_name, c.callee_name, c.caller_file, c.caller_line,
                           COALESCE(c.call_context, '') as call_context,
                           cc.depth + 1, cc.path || lower(c.caller_name)
                    FROM "{call_table}" c
                    JOIN call_chain cc ON lower(c.caller_name) = lower(cc.callee_name)
                    WHERE cc.depth < %s
                      AND lower(c.callee_name) != ALL(cc.path)
                )
                SELECT DISTINCT caller_name, callee_name, caller_file, caller_line, call_context, depth,
                       split_part(cc_final.caller_file, '/', 1) AS caller_subsystem
                FROM call_chain cc_final
                WHERE 1=1 {file_filter_clause}
                ORDER BY depth, caller_file, caller_line
                LIMIT {max_results}
                """,
                params,
            )
            results["callees"] = [dict(row) for row in cur.fetchall()]
            results["callee_count"] = len(results["callees"])
            if results["callee_count"] >= max_results:
                results["callee_truncated"] = True
                results["callee_note"] = (
                    f"WARNING: Results truncated at {max_results} edges. The full call graph is larger. "
                    "Use file_filter to narrow results, increase max_results, "
                    "or use a more specific qualified symbol_name."
                )

        # Also look up definitions for context
        cur.execute(
            f'SELECT name, qualified_name, kind, filename, line_start, line_end '
            f'FROM "{sym_table}" WHERE lower(name) = lower(%s) OR lower(qualified_name) = lower(%s) '
            f'ORDER BY filename, line_start LIMIT 20',
            (base_name, symbol_name),
        )
        results["definitions"] = [dict(row) for row in cur.fetchall()]

        cur.close()
    except Exception as e:
        return {"error": f"Call graph trace failed: {e}"}
    finally:
        if conn:
            release_pooled_connection(conn)

    results["symbol"] = symbol_name
    results["direction"] = direction
    results["max_depth"] = max_depth
    results["max_results"] = max_results
    if file_filter:
        results["file_filter"] = file_filter
    return results


# ============================================================
# Module-level dependency query (Fix 4)
# ============================================================

def find_subsystem_dependencies(project: str, module_path: str) -> dict:
    """Find cross-module dependencies for a subsystem directory, grouped by module.

    Returns:
      - imports: external modules this module calls, with symbol_count + sample_symbols
      - exports: external modules that call into this module, with symbol_count + sample_symbols
    Each entry is one external module, making it easy to draw a dependency diagram.
    """
    if project not in CORPUS_REGISTRY:
        return {"error": f"Unknown project: {project}"}

    sym_table = f"symbols_{project}"
    call_table = f"calls_{project}"

    # Normalise: strip leading slash, ensure ends without slash
    module_path = module_path.strip("/")

    conn = None
    try:
        conn = get_pooled_connection()
        cur = conn.cursor(cursor_factory=psycopg2.extras.RealDictCursor)

        # Check tables exist
        cur.execute(
            "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = %s)",
            (sym_table,),
        )
        if not cur.fetchone()["exists"]:
            cur.close()
            return {"error": f"Symbol index not available for project '{project}'. Re-run the indexer."}

        # IMPORTS grouped by external module:
        # Which external modules does this module call, and how many distinct symbols?
        cur.execute(
            f"""
            SELECT
                COALESCE(split_part(s.filename, '/', 1), '(unresolved)') AS external_module,
                count(DISTINCT c.callee_name)                             AS symbol_count,
                (array_agg(DISTINCT c.callee_name ORDER BY c.callee_name))[1:10] AS sample_symbols
            FROM "{call_table}" c
            LEFT JOIN "{sym_table}" s ON lower(s.name) = lower(c.callee_name)
            WHERE c.caller_file ILIKE %s
              AND (s.filename IS NULL OR s.filename NOT ILIKE %s)
            GROUP BY external_module
            ORDER BY symbol_count DESC
            LIMIT 30
            """,
            (f"{module_path}/%", f"{module_path}/%"),
        )
        imports = [dict(row) for row in cur.fetchall()]

        # EXPORTS grouped by external module:
        # Which external modules call symbols defined in this module?
        cur.execute(
            f"""
            SELECT
                split_part(c.caller_file, '/', 1)       AS external_module,
                count(DISTINCT s.name)                   AS symbol_count,
                (array_agg(DISTINCT s.name ORDER BY s.name))[1:10] AS sample_symbols
            FROM "{sym_table}" s
            JOIN "{call_table}" c ON lower(c.callee_name) = lower(s.name)
            WHERE s.filename ILIKE %s
              AND c.caller_file NOT ILIKE %s
            GROUP BY external_module
            ORDER BY symbol_count DESC
            LIMIT 30
            """,
            (f"{module_path}/%", f"{module_path}/%"),
        )
        exports = [dict(row) for row in cur.fetchall()]

        cur.close()
    except Exception as e:
        return {"error": f"Module dependency query failed: {e}"}
    finally:
        if conn:
            release_pooled_connection(conn)

    return {
        "module": module_path,
        "imports": imports,           # what the module depends on
        "import_count": len(imports),
        "exports": exports,           # what other modules depend on from this module
        "export_count": len(exports),
    }


# ============================================================
# Tool execution dispatcher (project-scoped)
# ============================================================

def run_tool(project: str, tool_name: str, tool_input: dict) -> str:
    if tool_name == "browse_source_tree":
        result = browse_source_tree(
            project,
            tool_input.get("path", ""),
            min(int(tool_input.get("depth", 3)), 4),
        )

    elif tool_name == "search_by_meaning":
        result = search_by_meaning(
            project,
            tool_input["query"],
            tool_input.get("max_results", 20),
        )

    elif tool_name == "find_files_by_name":
        result = find_files_by_name(
            project,
            tool_input["filename_pattern"],
            tool_input.get("path_contains", ""),
            tool_input.get("max_results", 50),
        )

    elif tool_name == "read_source_excerpt":
        result = read_source_excerpt(
            project,
            tool_input["file_path"],
            tool_input.get("start_line", 1),
            tool_input.get("end_line", 0),
            tool_input.get("procedure_name", ""),
        )

    elif tool_name == "regex_search_source":
        result = regex_search_source(
            project,
            tool_input["pattern"],
            tool_input.get("file_extension", ""),
            tool_input.get("path_contains", ""),
            tool_input.get("max_results", 200),
            tool_input.get("context_lines", 0),
        )

    elif tool_name == "lookup_symbol_usage":
        result = lookup_symbol_usage(
            project,
            tool_input["symbol_name"],
            tool_input.get("kind", ""),
        )

    elif tool_name == "walk_call_chain":
        result = walk_call_chain(
            project,
            tool_input["symbol_name"],
            tool_input.get("direction", "callers"),
            min(int(tool_input.get("max_depth", 6)), 10),
            tool_input.get("file_filter", ""),
            min(int(tool_input.get("max_results", 500)), 2000),
        )

    elif tool_name == "find_subsystem_dependencies":
        result = find_subsystem_dependencies(
            project,
            tool_input["module_path"],
        )

    else:
        result = {"error": f"Unknown tool: {tool_name}"}

    return json.dumps(result, ensure_ascii=False)


# ============================================================
# Cross-project support
# ============================================================

