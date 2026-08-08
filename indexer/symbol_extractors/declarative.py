"""Symbols from files that declare rather than execute.

Configuration, schema, interface definitions, HTTP routes: things that describe
structure without being code in the sense the other extractors mean. Handled
separately because what counts as a symbol differs per format, and because these
files are the ones most likely to contain a secret -- see the redaction step,
which runs before anything is stored.
"""
import logging
import re

logger = logging.getLogger("indexer.symbols.declarative")

def _extract_route_symbols(content, ext, filename, ts_symbols):
    """
    HTTP routes for one file, or [] if graph_pass is unavailable.

    Lazily imported and failure-tolerant, exactly as the DDL and declarative
    paths are: a missing or broken graph_pass must degrade to "no routes", not
    take the indexer down with it.
    """
    try:
        from graph_pass.http_routes import extract_http_route_symbols
    except ImportError as e:
        logger.warning("graph_pass.http_routes unavailable (%s) — no route symbols", e)
        return []
    try:
        return extract_http_route_symbols(content, ext, filename, ts_symbols)
    except Exception:
        logger.exception("Route extraction failed for %s", filename)
        return []



DECLARATIVE_EXTRACTORS = {
    ".proto": ("proto_defs", "extract_proto_symbols"),
    ".md": ("markdown_docs", "extract_markdown_symbols"),
    ".env": ("env_config", "extract_env_symbols"),
}


def _redact_for_storage(ext, content):
    """
    The content as it will be embedded and stored, which is not always the file.

    Only dotenv files differ today. Adding `.env` to SOURCE_EXTENSIONS means the
    file is chunked, embedded, and returned verbatim by the chat's `search_code`
    tool — four of the six dotenv files in the indexed repos are live `.env`s,
    not `.example` templates, so that would put real credentials into the index
    and into model context. Names are the architectural fact and are kept; values
    are replaced.'s "parse but do not embed" split would be the general fix;
    this is the narrow one that does not need it.
    """
    if ext != ".env":
        return content
    try:
        from graph_pass.env_config import redact
    except ImportError as e:
        # Fail CLOSED. A missing redactor must not silently mean "store the
        # secrets"; skipping the content costs a search hit, the alternative
        # costs a credential.
        logger.warning("graph_pass.env_config unavailable — storing %s as empty "
                       "rather than in the clear: %s", ext, e)
        return "\n" * content.count("\n")
    return redact(content)


def _extract_symbols_declarative(content, ext, filename):
    """Protobuf / markdown / dotenv -> symbols's cheap band)."""
    module_name, func_name = DECLARATIVE_EXTRACTORS[ext]
    try:
        module = importlib.import_module(f"graph_pass.{module_name}")
    except ImportError as e:
        logger.warning("graph_pass.%s unavailable — no %s symbols: %s",
                       module_name, ext, e)
        return []
    return getattr(module, func_name)(content, filename)


def _extract_symbols_sql(content, filename):
    """
    SQL/DDL -> table / view / index / column symbols.

    Imported lazily so a missing graph_pass package degrades to "no DDL symbols"
    rather than breaking the whole indexer — the same tolerance the tree-sitter
    path has for a missing grammar.
    """
    try:
        from graph_pass.sql_ddl import extract_sql_symbols
    except ImportError as e:
        logger.warning("graph_pass.sql_ddl unavailable — no DDL symbols: %s", e)
        return []
    try:
        return extract_sql_symbols(content, filename)
    except Exception as e:
        logger.warning("SQL extraction failed for %s: %s", filename, e)
        return []


