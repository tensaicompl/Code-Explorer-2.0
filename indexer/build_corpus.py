"""Builds the searchable corpus: walk source, chunk it, embed it, store it.

The entry point. Run it with no arguments to index every project and stream
found under CODEBASE_DIR, or name a project and optionally a stream to narrow
it.

Incremental by content hash. A file whose hash is unchanged since the last run
is not re-read, re-chunked or re-embedded, which is what makes a nightly refresh
over a large codebase cheap. Deleting a file removes its rows.

Work is streamed in batches rather than accumulated, so peak memory does not
scale with the size of the codebase being indexed.

The names re-exported at the bottom are a contract with three callers outside
this file -- see the note there before removing any of them.
"""
import gc
import importlib
import logging
import os
import re
import time

import psycopg2
import psycopg2.extras
from sentence_transformers import SentenceTransformer

from settings import (
    CODEBASE_DIR,
    DATA_EXTENSIONS,
    DB_BATCH_SIZE,
    DATABASE_URL,
    EMBED_BATCH_SIZE,
    EMBEDDING_MODEL,
    EXCLUDED_DIRS,
    MAX_DATA_FILE_SIZE,
    MAX_SOURCE_FILE_SIZE,
    SOURCE_EXTENSIONS,
)
from chunking import (
    chunk_ada_by_procedure,
    classify_ext,
    content_digest,
    offset_to_line_number,
    split_text_into_chunks,
    _strip_null_bytes,
)
from persistence import (
    count_symbols,
    ensure_chunk_table,
    ensure_symbol_tables,
    ensure_vector_index,
    load_indexed_hashes,
    persist_chunk_batch,
    persist_imports,
    persist_symbols,
    purge_files_from_index,
    purge_symbols_for_files,
)
from symbol_extractors import _extract_file_symbols, _redact_for_storage

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger("indexer")

def discover_project_streams() -> list:
    """Discover (project, stream) tuples from CODEBASE_DIR/{project}/{stream}/ structure."""
    results = []
    if not os.path.isdir(CODEBASE_DIR):
        logger.warning("CODEBASE_DIR not found: %s", CODEBASE_DIR)
        return results
    for project_name in sorted(os.listdir(CODEBASE_DIR)):
        project_path = os.path.join(CODEBASE_DIR, project_name)
        if not os.path.isdir(project_path) or project_name in EXCLUDED_DIRS:
            continue
        for stream_name in sorted(os.listdir(project_path)):
            stream_path = os.path.join(project_path, stream_name)
            if os.path.isdir(stream_path) and stream_name not in EXCLUDED_DIRS:
                results.append((project_name, stream_name))
    return results



def _scan_source_hashes(project_dir: str) -> dict:
    """Scan files on disk and return {rel_path: content_digest} without holding content in memory."""
    file_hashes = {}
    scanned = 0
    t0 = time.time()
    for root, dirs, filenames in os.walk(project_dir):
        dirs[:] = [d for d in dirs if d not in EXCLUDED_DIRS and not d.startswith(".")]
        for fname in filenames:
            ext = classify_ext(fname)
            if ext not in SOURCE_EXTENSIONS:
                continue
            filepath = os.path.join(root, fname)
            # Skip oversized files
            try:
                fsize = os.path.getsize(filepath)
                if fsize > MAX_SOURCE_FILE_SIZE:
                    continue
                if ext in DATA_EXTENSIONS and ext not in DDL_EXTENSIONS and fsize > MAX_DATA_FILE_SIZE:
                    continue
            except OSError:
                continue
            try:
                with open(filepath, "r", encoding="utf-8", errors="replace") as f:
                    content = _strip_null_bytes(f.read())
            except Exception:
                continue
            if not content.strip():
                continue
            rel_path = os.path.relpath(filepath, project_dir)
            file_hashes[rel_path] = content_digest(content)
            scanned += 1
            if scanned % 1000 == 0:
                logger.info("  Hash scan: %d files scanned (%.0fs elapsed)", scanned, time.time() - t0)
    logger.info("Hash scan complete: %d files in %.0fs", scanned, time.time() - t0)
    return file_hashes


def _run_symbol_only_pass(conn, project: str, project_dir: str, file_hashes: dict):
    """Extract symbols and calls for all files without re-embedding."""
    total_symbols = 0
    total_calls = 0
    all_symbols = []
    all_calls = []
    all_imports = []
    files_processed = 0
    t0 = time.time()

    for rel_path, fhash in sorted(file_hashes.items()):
        ext = classify_ext(rel_path)
        if ext in SKIP_SYMBOL_EXTENSIONS:
            continue

        filepath = os.path.join(project_dir, rel_path)
        try:
            fsize = os.path.getsize(filepath)
            if fsize > MAX_SOURCE_FILE_SIZE:
                continue
            if ext in DATA_EXTENSIONS and ext not in DDL_EXTENSIONS and fsize > MAX_DATA_FILE_SIZE:
                continue
        except OSError:
            continue

        try:
            with open(filepath, "r", encoding="utf-8", errors="replace") as f:
                content = _strip_null_bytes(f.read())
        except Exception:
            continue

        try:
            syms, cals, imps = _extract_file_symbols(content, ext, rel_path)
            for s in syms:
                s['content_digest'] = fhash
            all_symbols.extend(syms)
            all_calls.extend(cals)
            all_imports.extend(imps)
        except Exception as e:
            # The twin of the `except` in index_project_stream: the same
            # NameError hid here too, and this path is the one used to re-derive
            # symbols after a truncate — the run where a silent failure looks
            # exactly like "this project genuinely has no symbols".
            logger.warning("Symbol extraction failed for %s: %s: %s",
                           rel_path, type(e).__name__, e)

        files_processed += 1

        if len(all_symbols) >= DB_BATCH_SIZE:
            persist_symbols(conn, project, all_symbols, all_calls)
            persist_imports(conn, project, all_imports)
            total_symbols += len(all_symbols)
            total_calls += len(all_calls)
            all_symbols = []
            all_imports = []
            all_calls = []
            all_imports = []

        if files_processed % 1000 == 0:
            elapsed = time.time() - t0
            logger.info(
                "  Symbol pass: %d files, %d symbols, %d calls (%.0fs elapsed)",
                files_processed, total_symbols + len(all_symbols),
                total_calls + len(all_calls), elapsed,
            )

    if all_symbols or all_calls:
        persist_symbols(conn, project, all_symbols, all_calls)
        total_symbols += len(all_symbols)
        total_calls += len(all_calls)
    persist_imports(conn, project, all_imports)

    elapsed = time.time() - t0
    logger.info(
        "Project %s: symbol-only pass complete — %d symbols, %d calls from %d files in %.0fs",
        project, total_symbols, total_calls, files_processed, elapsed,
    )


def _normalize_table_key(name: str) -> str:
    """Sanitize a name for use in table names: lowercase, non-alphanumeric replaced with _."""
    import re as _re
    return _re.sub(r'[^a-z0-9]', '_', name.lower())


def index_project_stream(model: SentenceTransformer, project: str, stream: str):
    """Incrementally index a project+stream — only process changed/new files."""
    project_dir = os.path.join(CODEBASE_DIR, project, stream)
    project_key = f"{_normalize_table_key(project)}_{_normalize_table_key(stream)}"
    table_name = f"code_chunks_{project_key}"

    # Get embedding dimension from a test encode
    test_emb = model.encode(["test"])
    embedding_dim = len(test_emb[0])

    conn = psycopg2.connect(DATABASE_URL)
    ensure_chunk_table(conn, table_name, embedding_dim)
    ensure_symbol_tables(conn, project_key)

    # Get already-indexed file hashes
    indexed_hashes = load_indexed_hashes(conn, table_name)

    # Scan current files on disk — only hashes, not content
    current_hashes = _scan_source_hashes(project_dir)

    # Determine what changed
    current_filenames = set(current_hashes.keys())
    indexed_filenames = set(indexed_hashes.keys())

    deleted_files = indexed_filenames - current_filenames
    new_files = current_filenames - indexed_filenames
    changed_files = {
        f for f in current_filenames & indexed_filenames
        if current_hashes[f] != indexed_hashes[f]
    }
    files_to_index = new_files | changed_files
    unchanged_count = len(current_filenames) - len(files_to_index)

    logger.info(
        "Project %s/%s: %d files on disk, %d unchanged, %d new, %d changed, %d deleted",
        project, stream, len(current_filenames), unchanged_count,
        len(new_files), len(changed_files), len(deleted_files),
    )

    # Remove deleted and changed files from index
    files_to_remove = deleted_files | changed_files
    purge_files_from_index(conn, table_name, files_to_remove)
    purge_symbols_for_files(conn, project_key, files_to_remove)

    if not files_to_index:
        # Check if symbols are missing despite chunks existing — run symbol-only pass
        chunk_count = len(indexed_hashes)
        symbol_count = count_symbols(conn, project_key)
        if chunk_count > 0 and symbol_count == 0:
            logger.info(
                "Project %s/%s: chunks exist (%d files) but symbols empty — running symbol-only pass.",
                project, stream, chunk_count,
            )
            _run_symbol_only_pass(conn, project_key, project_dir, indexed_hashes)
        else:
            logger.info("Project %s/%s: nothing to re-index, skipping.", project, stream)
        conn.close()
        return

    # Index new and changed files — read content on-the-fly to avoid OOM
    files_processed = 0
    total_chunks = 0
    total_symbols = 0
    total_calls = 0
    batch = []
    all_symbols = []
    all_imports = []
    all_calls = []
    t0 = time.time()

    for rel_path in sorted(files_to_index):
        filepath = os.path.join(project_dir, rel_path)
        ext = classify_ext(rel_path)
        # Skip oversized files
        try:
            fsize = os.path.getsize(filepath)
            if fsize > MAX_SOURCE_FILE_SIZE:
                continue
            if ext in DATA_EXTENSIONS and ext not in DDL_EXTENSIONS and fsize > MAX_DATA_FILE_SIZE:
                continue
        except OSError:
            continue
        try:
            with open(filepath, "r", encoding="utf-8", errors="replace") as f:
                content = _strip_null_bytes(f.read())
        except Exception:
            continue
        fhash = current_hashes[rel_path]

        # Extract symbols and calls (done before chunking for Ada-aware chunking)
        syms, cals, imps = [], [], []
        if ext not in SKIP_SYMBOL_EXTENSIONS:
            try:
                syms, cals, imps = _extract_file_symbols(content, ext, rel_path)
                for s in syms:
                    s['content_digest'] = fhash
                all_symbols.extend(syms)
                all_calls.extend(cals)
                all_imports.extend(imps)
            except Exception as e:
                # Log it. This `except` previously swallowed a NameError —
                # `all_imports` was never declared in this function — and
                # because the failing line came AFTER the symbol and call
                # appends, those two succeeded and only imports were lost.
                # 2,036 files indexed cleanly with zero imports stored and no
                # sign anything was wrong. Degradation may be graceful; it must
                # not be silent.
                logger.warning("Symbol extraction failed for %s: %s: %s",
                               rel_path, type(e).__name__, e)

        # Chunk for embeddings — Ada files use procedure-aware chunking.
        #
        # `embeddable` is the content as it will be STORED: code_chunks_*.code is
        # returned verbatim by the chat's search tools, so a dotenv file's values
        # are stripped first. Line numbers survive because the redaction
        # is line-preserving; hashing still uses the real content, so editing a
        # secret still invalidates the chunk.
        embeddable = _redact_for_storage(ext, content)
        if ext in ADA_EXTENSIONS:
            chunks = chunk_ada_by_procedure(embeddable, syms)
        else:
            chunks = split_text_into_chunks(embeddable)
        for idx, (chunk_str, start_char, end_char) in enumerate(chunks):
            if not chunk_str.strip():
                continue
            start_line = offset_to_line_number(embeddable, start_char)
            end_line = offset_to_line_number(embeddable, min(end_char, len(embeddable) - 1))
            batch.append((rel_path, chunk_str, start_line, end_line, idx, fhash))

        files_processed += 1

        if len(batch) >= EMBED_BATCH_SIZE:
            total_chunks += persist_chunk_batch(conn, table_name, model, batch)
            batch = []

        # Flush symbols and calls periodically to avoid memory buildup
        if len(all_symbols) >= DB_BATCH_SIZE or len(all_calls) >= DB_BATCH_SIZE * 10:
            persist_symbols(conn, project_key, all_symbols, all_calls)
            persist_imports(conn, project_key, all_imports)
            total_symbols += len(all_symbols)
            total_calls += len(all_calls)
            all_symbols = []
            all_calls = []
            all_imports = []

        if files_processed % 1000 == 0:
            elapsed = time.time() - t0
            logger.info(
                "  Progress: %d files, %d chunks, %d symbols, %d calls (%.0fs elapsed)",
                files_processed, total_chunks, total_symbols + len(all_symbols),
                total_calls + len(all_calls), elapsed,
            )
            gc.collect()

    if batch:
        total_chunks += persist_chunk_batch(conn, table_name, model, batch)

    # Flush remaining symbols
    if all_symbols or all_calls:
        persist_symbols(conn, project_key, all_symbols, all_calls)
        persist_imports(conn, project_key, all_imports)
        total_symbols += len(all_symbols)
        total_calls += len(all_calls)

    # Rebuild vector index if data changed
    ensure_vector_index(conn, table_name)

    conn.close()
    elapsed = time.time() - t0
    logger.info(
        "Project %s/%s: %d files re-indexed (%d chunks, %d symbols, %d calls) in %.0fs",
        project, stream, files_processed, total_chunks, total_symbols, total_calls, elapsed,
    )


def main():
    import sys

    all_combos = discover_project_streams()
    if not all_combos:
        logger.warning("No project/stream combos found in %s", CODEBASE_DIR)
        return

    # Optional narrowing: build_corpus.py [project] [stream]
    filter_project = sys.argv[1] if len(sys.argv) > 1 else None
    filter_stream = sys.argv[2] if len(sys.argv) > 2 else None

    combos = all_combos
    if filter_project:
        combos = [(p, s) for p, s in combos if p == filter_project]
    if filter_stream:
        combos = [(p, s) for p, s in combos if s == filter_stream]

    if not combos:
        logger.warning("No matching project/stream combos (filter: project=%s, stream=%s). Available: %s",
                        filter_project, filter_stream, all_combos)
        return

    logger.info("Discovered project/stream combos: %s", combos)

    logger.info("Loading embedding model: %s", EMBEDDING_MODEL)
    t0 = time.time()
    device = os.environ.get("INDEXER_DEVICE", "").lower()
    if not device:
        import torch
        if torch.backends.mps.is_available():
            device = "mps"
        elif torch.cuda.is_available():
            device = "cuda"
        else:
            device = "cpu"
    logger.info("Using device: %s", device)
    model = SentenceTransformer(EMBEDDING_MODEL, trust_remote_code=True, device=device)
    logger.info("Model loaded in %.1fs", time.time() - t0)

    for project, stream in combos:
        logger.info("=== Indexing project: %s / stream: %s ===", project, stream)
        try:
            index_project_stream(model, project, stream)
        except Exception:
            logger.exception("Failed to index project: %s / stream: %s", project, stream)

    logger.info("All project/stream combos indexed.")


if __name__ == "__main__":
    main()


# ---------------------------------------------------------------------------
# Re-export contract
#
# Three callers reach into this module by attribute after importing it, and none
# of them is covered by the test suite -- so getting this list wrong fails at
# run time, in a cron job or a container, rather than at import:
#
#   reextract_symbols.py            CODEBASE_DIR, discover_project_streams,
#                                   load_indexed_hashes, _run_symbol_only_pass,
#                                   _load_treesitter_parser
#   watch-and-index.sh (heredoc)    discover_project_streams
#   graph_pass/tests/               classify_ext, SOURCE_EXTENSIONS,
#     test_declarative_extractors     _redact_for_storage
#
# Everything named here is imported above from the module that now owns it; the
# names are restated so that moving one again is a change in one place.
# ---------------------------------------------------------------------------
from symbol_extractors import _load_treesitter_parser  # noqa: E402,F401

__all__ = [
    "CODEBASE_DIR",
    "SOURCE_EXTENSIONS",
    "_load_treesitter_parser",
    "_redact_for_storage",
    "_run_symbol_only_pass",
    "classify_ext",
    "discover_project_streams",
    "index_project_stream",
    "load_indexed_hashes",
    "main",
]
