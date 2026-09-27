"""Running the same search across several corpora at once.

When a user has access to more than one project and asks a question without
naming one, the agent is given these federated variants instead of the
single-corpus tools. Each fans the same query out across every permitted corpus,
tags each result with which project it came from, and merges them.

The tools mirror those in search.py deliberately: same names with a suffix, same
arguments apart from taking a list of registry keys, same result shape. That
symmetry is what lets definitions.py describe both sets from one template and
lets the agent switch between them without relearning anything.

Fan-out is bounded. A federated search over a dozen corpora issues a dozen
queries, so per-corpus result limits are lower than their single-corpus
equivalents -- the model gets breadth across projects rather than depth in one.
"""
from __future__ import annotations

import json
import logging
from concurrent.futures import ThreadPoolExecutor, as_completed
from typing import Dict, List

import psycopg2.extras

from .catalog import CORPUS_REGISTRY, ensure_corpus_registry, project_label
from .db import get_pooled_connection, load_embedding_model, release_pooled_connection
from .search import (
    browse_source_tree,
    find_files_by_name,
    find_subsystem_dependencies,
    lookup_symbol_usage,
    read_source_excerpt,
    regex_search_source,
    walk_call_chain,
)
from . import search as _single

logger = logging.getLogger("praxevia.corpus.cross_project")

def resolve_cross_project_scope(permitted_projects: List[str]) -> List[str]:
    """Return one indexed registry_key per permitted project.

    Stream preference: 'develop' first, then 'release'.
    """
    by_project: Dict[str, Dict[str, str]] = {}
    for key, info in CORPUS_REGISTRY.items():
        proj = info.get("project", key)
        if proj not in permitted_projects:
            continue
        if not info.get("indexed", False):
            continue
        stream = info.get("stream", "")
        by_project.setdefault(proj, {})[stream] = key

    keys = []
    for _proj, streams in sorted(by_project.items()):
        if "develop" in streams:
            keys.append(streams["develop"])
        elif "release" in streams:
            keys.append(streams["release"])
    return keys


def search_by_meaning_cross(keys: List[str], query: str, max_results: int = 15) -> dict:
    """Semantic search across multiple projects via UNION ALL."""
    model = load_embedding_model()
    query_embedding = model.encode(query).tolist()
    embedding_str = "[" + ",".join(str(x) for x in query_embedding) + "]"

    subqueries = []
    params = []
    for key in keys:
        table = f"code_chunks_{key}"
        label = project_label(key)
        subqueries.append(
            f'SELECT filename, code, start_line, end_line, '
            f'1 - (embedding <=> %s::vector) AS similarity, '
            f'%s AS project '
            f'FROM "{table}"'
        )
        params.extend([embedding_str, label])

    if not subqueries:
        return {"matches": [], "count": 0, "query": query, "cross_project": True}

    union_query = " UNION ALL ".join(subqueries)
    full_query = f"SELECT * FROM ({union_query}) combined ORDER BY similarity DESC LIMIT %s"
    params.append(max_results)

    conn = None
    try:
        conn = get_pooled_connection()
        cur = conn.cursor(cursor_factory=psycopg2.extras.RealDictCursor)
        cur.execute("SET LOCAL ivfflat.probes = 10")
        cur.execute(full_query, params)
        rows = cur.fetchall()
        cur.close()
    except Exception as e:
        return {"error": f"Cross-project search failed: {e}", "matches": [], "count": 0}
    finally:
        if conn:
            release_pooled_connection(conn)

    matches = []
    for row in rows:
        matches.append({
            "project": row["project"],
            "file_path": row["filename"],
            "chunk_content": row["code"],
            "start_line": row.get("start_line", 0),
            "end_line": row.get("end_line", 0),
            "similarity": round(float(row["similarity"]), 4),
        })

    return {"matches": matches, "count": len(matches), "query": query, "cross_project": True}


def browse_source_tree_cross(keys: List[str], path: str = "", depth: int = 2) -> dict:
    """List directories across multiple projects."""
    projects = {}
    for key in keys:
        label = project_label(key)
        projects[label] = browse_source_tree(key, path, min(depth, 3))
    return {"cross_project": True, "projects": projects}


def find_files_by_name_cross(keys: List[str], filename_pattern: str,
                            path_contains: str = "", max_results: int = 50) -> dict:
    """Find source files across multiple projects."""
    per_limit = max(max_results // len(keys), 10) if keys else max_results
    all_files = []
    for key in keys:
        result = find_files_by_name(key, filename_pattern, path_contains, per_limit)
        label = project_label(key)
        for f in result.get("files", []):
            all_files.append({"project": label, "file": f})

    return {
        "files": all_files[:max_results],
        "count": len(all_files),
        "cross_project": True,
        "truncated": len(all_files) > max_results,
    }


def regex_search_source_cross(keys: List[str], pattern: str, file_extension: str = "",
                           path_contains: str = "", max_results: int = 100,
                           context_lines: int = 0) -> dict:
    """Grep across multiple projects in parallel."""
    per_limit = max(max_results // len(keys), 20) if keys else max_results

    def _grep_one(key: str) -> tuple:
        result = regex_search_source(key, pattern, file_extension, path_contains,
                                  per_limit, context_lines)
        label = project_label(key)
        prefixed = [f"[{label}] {m}" for m in result.get("matches", [])]
        return prefixed, result.get("count", 0), result.get("files_searched", 0)

    all_matches = []
    total = 0
    files_searched = 0

    with ThreadPoolExecutor(max_workers=min(len(keys), 5)) as pool:
        futures = {pool.submit(_grep_one, k): k for k in keys}
        for future in as_completed(futures):
            try:
                matches, count, searched = future.result()
                all_matches.extend(matches)
                total += count
                files_searched += searched
            except Exception as e:
                logger.warning("Cross-project grep failed for %s: %s", futures[future], e)

    return {
        "matches": all_matches[:max_results],
        "count": total,
        "files_searched": files_searched,
        "cross_project": True,
        "truncated": len(all_matches) > max_results,
    }


def lookup_symbol_usage_cross(keys: List[str], symbol_name: str, kind: str = "") -> dict:
    """Find symbol references across multiple projects."""
    all_defs = []
    all_callers = []
    all_callees = []

    for key in keys:
        result = lookup_symbol_usage(key, symbol_name, kind)
        if "error" in result and "Symbol index not available" in result.get("error", ""):
            continue
        label = project_label(key)
        for d in result.get("definitions", []):
            d["project"] = label
            all_defs.append(d)
        for c in result.get("callers", [])[:50]:
            c["project"] = label
            all_callers.append(c)
        for c in result.get("callees", [])[:50]:
            c["project"] = label
            all_callees.append(c)

    return {
        "symbol": symbol_name,
        "definitions": all_defs,
        "definition_count": len(all_defs),
        "callers": all_callers,
        "caller_count": len(all_callers),
        "callees": all_callees,
        "callee_count": len(all_callees),
        "cross_project": True,
    }


def walk_call_chain_cross(keys: List[str], symbol_name: str,
                           direction: str = "callers", max_depth: int = 4,
                           file_filter: str = "",
                           max_results: int = 500) -> dict:
    """Trace call graph across multiple projects."""
    combined: Dict[str, list] = {"callers": [], "callees": [], "definitions": []}

    for key in keys:
        result = walk_call_chain(key, symbol_name, direction, min(max_depth, 6), file_filter, max_results)
        if "error" in result:
            continue
        label = project_label(key)
        for edge_type in ("callers", "callees", "definitions"):
            for item in result.get(edge_type, [])[:100]:
                item["project"] = label
                combined[edge_type].append(item)

    combined["symbol"] = symbol_name
    combined["direction"] = direction
    combined["max_depth"] = max_depth
    combined["cross_project"] = True
    if file_filter:
        combined["file_filter"] = file_filter
    return combined


def find_subsystem_dependencies_cross(keys: List[str], module_path: str) -> dict:
    """Find module dependencies across multiple projects."""
    all_imports = []
    all_exports = []

    for key in keys:
        result = find_subsystem_dependencies(key, module_path)
        if "error" in result:
            continue
        label = project_label(key)
        for imp in result.get("imports", []):
            imp["project"] = label
            all_imports.append(imp)
        for exp in result.get("exports", []):
            exp["project"] = label
            all_exports.append(exp)

    return {
        "module": module_path,
        "imports": all_imports,
        "import_count": len(all_imports),
        "exports": all_exports,
        "export_count": len(all_exports),
        "cross_project": True,
    }


def run_tool_cross_project(permitted_keys: List[str], tool_name: str, tool_input: dict) -> str:
    """Execute a tool across multiple projects."""
    if tool_name == "search_by_meaning":
        result = search_by_meaning_cross(
            permitted_keys,
            tool_input["query"],
            tool_input.get("max_results", 15),
        )
    elif tool_name == "browse_source_tree":
        result = browse_source_tree_cross(
            permitted_keys,
            tool_input.get("path", ""),
            min(int(tool_input.get("depth", 2)), 3),
        )
    elif tool_name == "find_files_by_name":
        result = find_files_by_name_cross(
            permitted_keys,
            tool_input["filename_pattern"],
            tool_input.get("path_contains", ""),
            tool_input.get("max_results", 50),
        )
    elif tool_name == "read_source_excerpt":
        proj_label = tool_input.get("project", "")
        target_key = None
        for k in permitted_keys:
            if project_label(k) == proj_label:
                target_key = k
                break
        if not target_key and permitted_keys:
            target_key = permitted_keys[0]
        if target_key:
            result = read_source_excerpt(
                target_key,
                tool_input["file_path"],
                tool_input.get("start_line", 1),
                tool_input.get("end_line", 0),
                tool_input.get("procedure_name", ""),
            )
            result["project"] = project_label(target_key)
        else:
            result = {"error": "No project specified for read_source_excerpt"}
    elif tool_name == "regex_search_source":
        result = regex_search_source_cross(
            permitted_keys,
            tool_input["pattern"],
            tool_input.get("file_extension", ""),
            tool_input.get("path_contains", ""),
            tool_input.get("max_results", 100),
            tool_input.get("context_lines", 0),
        )
    elif tool_name == "lookup_symbol_usage":
        result = lookup_symbol_usage_cross(
            permitted_keys,
            tool_input["symbol_name"],
            tool_input.get("kind", ""),
        )
    elif tool_name == "walk_call_chain":
        result = walk_call_chain_cross(
            permitted_keys,
            tool_input["symbol_name"],
            tool_input.get("direction", "callers"),
            min(int(tool_input.get("max_depth", 4)), 6),
            tool_input.get("file_filter", ""),
            min(int(tool_input.get("max_results", 500)), 2000),
        )
    elif tool_name == "find_subsystem_dependencies":
        result = find_subsystem_dependencies_cross(
            permitted_keys,
            tool_input["module_path"],
        )
    else:
        result = {"error": f"Unknown tool: {tool_name}"}

    return json.dumps(result, ensure_ascii=False)

