"""Extracting symbols, calls and imports from a source file.

One dispatcher over four strategies, chosen by file extension: a real parse tree
where a grammar exists, hand-written pattern matching for Ada and Perl where one
does not, and a declarative reader for configuration and schema files.

Callers use `extract_file_symbols`; which strategy ran is an implementation
detail they should not need to know.
"""
import logging

# Named explicitly rather than star-imported: every extractor entry point is
# underscore-prefixed, and `import *` skips those, which would leave the
# dispatcher below calling names that are not bound.
from .ada import _scan_ada_symbols
from .declarative import (
    _extract_route_symbols,
    _extract_symbols_declarative,
    _extract_symbols_sql,
    _redact_for_storage,
)
from .perl import _scan_perl_symbols
from .treesitter import _load_treesitter_parser, _scan_treesitter_symbols

logger = logging.getLogger("indexer.symbols")

TS_LANGUAGE_MAP = {
    '.py': 'python',
    '.c': 'c', '.h': 'c', '.cpp': 'cpp', '.hpp': 'cpp',
    '.hh': 'cpp', '.cc': 'cpp',
    '.rb': 'ruby',
    '.java': 'java', '.kt': 'kotlin',
    '.js': 'javascript', '.jsx': 'javascript',
    '.ts': 'typescript', '.tsx': 'tsx',
    '.sh': 'bash', '.bash': 'bash', '.ksh': 'bash',
    '.scala': 'scala',
    '.rs': 'rust',
}

# Extensions handled by regex extractors (not in tree-sitter-languages)
ADA_EXTENSIONS = {'.ads', '.adb', '.ada'}
PERL_EXTENSIONS = {'.pl', '.pm'}

# Extensions to skip for symbol extraction (config/data/markup)
#
# '.sql' REMOVED — Upstream gated SQL out here, so DDL never reached
# extraction at either call site and no row anywhere described a database table.
# SQL now dispatches to graph_pass.sql_ddl and yields table/view/index/column
# symbols, which is the "databases" half of the product requirement.
SKIP_SYMBOL_EXTENSIONS = {'.json', '.yaml', '.yml',
                          '.gpr', '.properties', '.gradle',
                          '.idl', '.incl'}

# Data files are capped at MAX_DATA_FILE_SIZE (1 MB) because they are pure data
# with no semantic value for *search*. DDL is different: a large migration dump is
# exactly the file most worth parsing. These extensions are exempted from the data
# cap and fall under MAX_SOURCE_FILE_SIZE (5 MB) instead.
#
# Known limitation: exempting them also means a large .sql file is chunked and
# embedded, which notes is unnecessary — DDL parsing does not need the
# embedding. Separating "parse but do not embed" needs the two paths split, which
# is more invasive than this phase warrants. The 5 MB ceiling bounds the cost.
DDL_EXTENSIONS = {'.sql'}

# Cached tree-sitter parsers
_TS_PARSERS = {}

# Ada keywords to filter out of call extraction

def _extract_file_symbols(content, ext, filename):
    """Dispatch symbol extraction based on file extension."""
    if ext in ADA_EXTENSIONS:
        return (*_scan_ada_symbols(content, filename), [])
    if ext in PERL_EXTENSIONS:
        return (*_scan_perl_symbols(content, filename), [])
    if ext in DDL_EXTENSIONS:
        # DDL has definitions but no call sites.
        return _extract_symbols_sql(content, filename), [], []
    if ext in DECLARATIVE_EXTRACTORS:
        # Declarative formats: definitions, no calls, no imports.
        return _extract_symbols_declarative(content, ext, filename), [], []
    if ext in TS_LANGUAGE_MAP:
        symbols, calls, imports = _scan_treesitter_symbols(content, ext, filename)
        # Routes ride along on the tree-sitter pass: same `content` already in
        # memory, same `symbols` already parsed, so this costs one regex sweep
        # and no extra I/O. Until this existed the only `endpoint` nodes in any
        # graph came from protobuf and GraphQL, and a FastAPI or Spring service
        # indexed as zero endpoints.
        symbols = symbols + _extract_route_symbols(content, ext, filename, symbols)
        return symbols, calls, imports
    # THREE values. Both call sites unpack a triple; the two-tuple this used to
    # return raised ValueError for every extension with no extractor, which the
    # caller then logged as a failed extraction rather than as "nothing to do".
    return [], [], []


# ext -> (module, function) under graph_pass. Imported lazily, exactly as the DDL
# path is, so a missing graph_pass degrades to "no symbols of this kind" rather
# than taking the whole indexer down.
