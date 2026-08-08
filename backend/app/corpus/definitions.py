"""The tool schema the model is shown.

This is prompt text, not documentation. Everything here lands in the model's
context on every single turn, and the wording is what decides whether it reaches
for the symbol index or falls back to grepping. Two consequences worth keeping
in mind when editing:

  * Say what a tool is *for* and when to prefer it over its neighbour. The
    descriptions below deliberately cross-reference each other -- "use this
    before that", "that one is cheaper for this case" -- because the failure
    mode in practice is not picking a broken tool, it is picking a workable but
    expensive one and burning the iteration budget.
  * Keep it short. Eight tools with generous prose is a fixed tax on every
    request in the conversation.

Both modes are generated from `_TOOLS` below rather than written out twice.
Single-corpus and federated schemas differ only in wording, a few default
limits, and one extra parameter, and maintaining two independent literals is how
the two drift until the model is told one thing and the dispatcher does another.

The `name` fields here are the contract. They must match `run_tool`'s dispatch
keys in search.py exactly; `TOOL_NAMES` is exported so nothing has to restate
them by hand -- see main.py, which derives its allow-list from it rather than
keeping a parallel copy.
"""
from __future__ import annotations

from typing import List

from .catalog import CORPUS_REGISTRY, project_label

# Parameter fragments reused across both modes.
_P_PATH = {
    "type": "string",
    "description": "Directory to inspect, relative to the source root. Omit for the top level.",
}
_P_DEPTH_SINGLE = {
    "type": "integer",
    "description": "How many levels below `path` to expand. Defaults to 3, capped at 4.",
}
_P_DEPTH_CROSS = {
    "type": "integer",
    "description": "How many levels below `path` to expand. Defaults to 2, capped at 3.",
}
_P_QUERY = {
    "type": "string",
    "description": (
        "What you are looking for, described in ordinary language -- "
        "'where sessions get expired', 'the retry policy for outbound calls', "
        "'code that writes the audit log'."
    ),
}
_P_FILENAME = {
    "type": "string",
    "description": "Filename glob, e.g. 'settings.py', '*.proto', 'Dockerfile*'.",
}
_P_PATH_CONTAINS = {
    "type": "string",
    "description": "Optional. Keep only paths containing this substring.",
}
_P_FILE_PATH = {
    "type": "string",
    "description": "Path to the file, relative to the source root.",
}
_P_START_LINE = {
    "type": "integer",
    "description": "First line to return, counting from 1. Defaults to the start of the file.",
}
_P_END_LINE = {
    "type": "integer",
    "description": "Last line to return. Defaults to 500 lines after the start.",
}
_P_PROCEDURE = {
    "type": "string",
    "description": (
        "Optional. Return only this subprogram's body, located by its declaration "
        "and closing statement. Useful for pulling one routine out of a long file."
    ),
}
_P_PATTERN = {
    "type": "string",
    "description": "Regular expression to match against each line of source.",
}
_P_EXTENSION = {
    "type": "string",
    "description": "Optional. Restrict the search to one file extension, e.g. '.sql'.",
}
_P_CONTEXT = {
    "type": "integer",
    "description": "Optional. Lines to include either side of a match, as grep -C does. Defaults to 0.",
}
_P_SYMBOL = {
    "type": "string",
    "description": (
        "Symbol to look up: a function, procedure, method, class or package. "
        "Qualified forms such as 'Module.Type.Method' are accepted."
    ),
}
_P_KIND = {
    "type": "string",
    "description": "Optional. Narrow definitions to one kind: procedure, function, package, class or method.",
}
_P_DIRECTION = {
    "type": "string",
    "enum": ["callers", "callees", "both"],
    "description": "Walk inbound ('callers'), outbound ('callees'), or both.",
}
_P_FILE_FILTER = {
    "type": "string",
    "description": "Optional. SQL ILIKE pattern restricting which files edges may come from, e.g. '%payments%'.",
}
_P_MODULE_PATH = {
    "type": "string",
    "description": "Directory naming the subsystem, e.g. 'billing' or 'billing/ledger'.",
}


def _results_cap(default: int) -> dict:
    return {"type": "integer", "description": f"Most results to return. Defaults to {default}."}


#: (name, single-mode description, cross-mode description, single params, cross params, required)
_TOOLS = [
    (
        "browse_source_tree",
        "Lay out the directory structure so you can see how the codebase is organised. "
        "Start here whenever the question is about architecture, subsystems or where "
        "something lives -- it is far cheaper than searching blind. Drill in with `path` "
        "once you know which part matters.",
        "Lay out the directory structure of every project you can see, grouped by project. "
        "Start here to work out which project a question is actually about.",
        {"path": _P_PATH, "depth": _P_DEPTH_SINGLE},
        {"path": _P_PATH, "depth": _P_DEPTH_CROSS},
        [],
    ),
    (
        "search_by_meaning",
        "Find code by what it does rather than what it is called, matching your description "
        "against embeddings of every indexed chunk. Reach for this when you know the behaviour "
        "but not the vocabulary the codebase uses for it. When you already know the exact "
        "string or pattern, regex_search_source is more precise and much faster.",
        "Find code by what it does across every project you can see. Each result carries a "
        "`project` field saying where it came from.",
        {"query": _P_QUERY, "max_results": _results_cap(20)},
        {"query": _P_QUERY, "max_results": _results_cap(15)},
        ["query"],
    ),
    (
        "find_files_by_name",
        "Locate files by name or glob anywhere in the tree. Use it to turn a half-remembered "
        "filename into a real path before reading it.",
        "Locate files by name or glob across every project you can see. Each result carries a "
        "`project` field.",
        {
            "filename_pattern": _P_FILENAME,
            "path_contains": _P_PATH_CONTAINS,
            "max_results": _results_cap(50),
        },
        {
            "filename_pattern": _P_FILENAME,
            "path_contains": _P_PATH_CONTAINS,
            "max_results": _results_cap(50),
        },
        ["filename_pattern"],
    ),
    (
        "read_source_excerpt",
        "Read the source itself. Use it once a search has told you where to look, rather than "
        "as a way of exploring -- it returns at most 500 lines per call. Narrow with "
        "start_line/end_line, or name a subprogram to get just that routine.",
        "Read the source itself. Because several projects are in scope you must say which one "
        "with `project`.",
        {
            "file_path": _P_FILE_PATH,
            "start_line": _P_START_LINE,
            "end_line": _P_END_LINE,
            "procedure_name": _P_PROCEDURE,
        },
        None,  # built below: needs the project enum
        ["file_path"],
    ),
    (
        "regex_search_source",
        "Match a regular expression against the source line by line. This is the tool for exact "
        "text: a literal string, a naming convention, a call spelled a particular way. Ask for "
        "context lines when you need to read the match in situ, and raise max_results when you "
        "genuinely need every occurrence rather than a sample.",
        "Match a regular expression across every project you can see. Results are prefixed with "
        "the project and stream they came from.",
        {
            "pattern": _P_PATTERN,
            "file_extension": _P_EXTENSION,
            "path_contains": _P_PATH_CONTAINS,
            "max_results": _results_cap(200),
            "context_lines": _P_CONTEXT,
        },
        {
            "pattern": _P_PATTERN,
            "file_extension": _P_EXTENSION,
            "path_contains": _P_PATH_CONTAINS,
            "max_results": _results_cap(100),
            "context_lines": _P_CONTEXT,
        },
        ["pattern"],
    ),
    (
        "find_subsystem_dependencies",
        "Summarise how one subsystem is wired to the rest, in both directions: what it reaches "
        "into, and what reaches into it. Answers are grouped by the module on the other end with "
        "symbol counts, which is what you want for drawing a dependency picture or judging blast "
        "radius. Needs the symbol index, and beats tracing symbols one at a time by a wide margin.",
        "Summarise how one subsystem is wired to the rest, reported per project.",
        {"module_path": _P_MODULE_PATH},
        {"module_path": _P_MODULE_PATH},
        ["module_path"],
    ),
    (
        "lookup_symbol_usage",
        "Ask the symbol index where something is defined and who touches it, one hop out. "
        "Returns definitions with file and line, callers with the calling line quoted, and "
        "callees. Prefer it to grep for anything call-graph shaped -- grep cannot tell a call "
        "from a comment. Symbol matching ignores case where the language does. For chains longer "
        "than one hop use walk_call_chain instead of calling this repeatedly.",
        "Ask the symbol index where something is defined and who touches it, across every project. "
        "Each result carries a `project` field. Single hop only.",
        {"symbol_name": _P_SYMBOL, "kind": _P_KIND},
        {"symbol_name": _P_SYMBOL, "kind": _P_KIND},
        ["symbol_name"],
    ),
    (
        "walk_call_chain",
        "Follow call edges outward from a symbol across as many hops as you ask for. This is the "
        "tool for 'everything that can eventually reach X' -- one call does what dozens of "
        "single-hop lookups would, and each edge is returned with its hop distance and the "
        "calling line. Narrow with file_filter when a symbol is called from everywhere.",
        "Follow call edges outward from a symbol, within each project separately -- edges are not "
        "resolved across project boundaries. Each result carries a `project` field.",
        {
            "symbol_name": _P_SYMBOL,
            "direction": _P_DIRECTION,
            "max_depth": {
                "type": "integer",
                "description": "How many hops to follow. Defaults to 6, capped at 10.",
            },
            "file_filter": _P_FILE_FILTER,
            "max_results": _results_cap(500),
        },
        {
            "symbol_name": _P_SYMBOL,
            "direction": _P_DIRECTION,
            "max_depth": {
                "type": "integer",
                "description": "How many hops to follow. Defaults to 4, capped at 6.",
            },
            "file_filter": _P_FILE_FILTER,
            "max_results": _results_cap(500),
        },
        ["symbol_name"],
    ),
]

#: Every tool name the model may be offered, in schema order.
#:
#: Exported so callers that need to police tool names -- the /api/tools route in
#: particular -- can derive the set from here instead of maintaining a copy that
#: silently goes stale the next time a tool is renamed.
TOOL_NAMES = tuple(name for name, *_ in _TOOLS)


def _schema(name: str, description: str, properties: dict, required: list) -> dict:
    return {
        "name": name,
        "description": description,
        "input_schema": {
            "type": "object",
            "properties": properties,
            "required": required,
        },
    }


def build_tool_schema(project: str) -> list:
    """The tools offered when the conversation is scoped to one corpus.

    An unknown key yields no tools at all rather than an error: the agent then
    has nothing to call and says so, which is a better failure than offering
    tools that will reject every invocation.
    """
    if project not in CORPUS_REGISTRY:
        return []

    return [
        _schema(name, single_desc, single_props, required)
        for name, single_desc, _cross_desc, single_props, _cross_props, required in _TOOLS
    ]


def build_cross_project_tool_schema(permitted_keys: List[str]) -> list:
    """The tools offered when several corpora are in scope at once.

    Reading a file is the one operation that cannot be federated -- a path means
    nothing without saying which project it belongs to -- so that tool alone
    gains a required `project` argument, constrained to the corpora this caller
    may actually see.
    """
    labels = [project_label(key) for key in permitted_keys]
    project_param = {
        "type": "string",
        "enum": labels,
        "description": "Which project to read from, as project/stream.",
    }

    schemas = []
    for name, _single_desc, cross_desc, single_props, cross_props, required in _TOOLS:
        if name == "read_source_excerpt":
            properties = {"project": project_param, **single_props}
            required = ["project", *required]
        else:
            properties = cross_props if cross_props is not None else single_props
        schemas.append(_schema(name, cross_desc, properties, required))

    return schemas
