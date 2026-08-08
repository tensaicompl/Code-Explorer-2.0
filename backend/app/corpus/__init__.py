"""Searching what the indexer built.

  db.py            the embedding model and the connection pool
  catalog.py       which corpora exist, and how to name them
  definitions.py   the tool schema the model is shown
  search.py        the tools themselves, scoped to one corpus
  cross_project.py the same tools federated across several

The indexer produces the corpus; this package serves and searches it. The one
piece of shared knowledge between them is the registry key -- the rule in
catalog.sanitize_registry_key that turns a project and stream name into a table
suffix -- which both sides derive independently and must derive identically.

Callers should import from here rather than reaching into the submodules.
"""
from .catalog import (
    CORPUS_REGISTRY,
    ensure_corpus_registry,
    get_corpus_summary,
    project_label,
    refresh_corpus_registry,
    sanitize_registry_key,
)
from .cross_project import resolve_cross_project_scope, run_tool_cross_project
from .definitions import (
    TOOL_NAMES,
    build_cross_project_tool_schema,
    build_tool_schema,
)
from .search import read_source_excerpt, run_tool

__all__ = [
    "CORPUS_REGISTRY",
    "TOOL_NAMES",
    "build_cross_project_tool_schema",
    "build_tool_schema",
    "ensure_corpus_registry",
    "get_corpus_summary",
    "project_label",
    "read_source_excerpt",
    "refresh_corpus_registry",
    "resolve_cross_project_scope",
    "run_tool",
    "run_tool_cross_project",
    "sanitize_registry_key",
]
