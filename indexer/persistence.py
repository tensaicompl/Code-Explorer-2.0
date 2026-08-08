"""Writing chunks, symbols, calls and imports into Postgres.

Every table is per corpus, named with the registry key -- so a project is
re-indexed, dropped or inspected without touching any other. The DDL lives here
and only here; the backend reads these tables but never creates them.
"""
import logging

import psycopg2
import psycopg2.extras
from sentence_transformers import SentenceTransformer

from settings import DB_BATCH_SIZE

logger = logging.getLogger("indexer.persistence")

def ensure_symbol_tables(conn, project):
    """Create symbols and calls tables for a project."""
    sym_table = f"symbols_{project}"
    call_table = f"calls_{project}"
    imp_table = f"imports_{project}"
    cur = conn.cursor()
    cur.execute(f"""
        CREATE TABLE IF NOT EXISTS "{imp_table}" (
            id SERIAL PRIMARY KEY,
            filename TEXT NOT NULL,
            module TEXT NOT NULL,
            line INT NOT NULL DEFAULT 0,
            kind TEXT NOT NULL DEFAULT 'import',
            owner TEXT,
            UNIQUE(filename, module, kind, owner)
        )
    """)
    cur.execute(f'CREATE INDEX IF NOT EXISTS "{imp_table}_file" ON "{imp_table}" (filename)')
    # Additive migration: an index built before doc extraction has no such
    # column, and dropping the table would cost a full re-embed.
    cur.execute(f'ALTER TABLE IF EXISTS "{sym_table}" ADD COLUMN IF NOT EXISTS doc TEXT NOT NULL DEFAULT \'\'')
    cur.execute(f"""
        CREATE TABLE IF NOT EXISTS "{sym_table}" (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            qualified_name TEXT,
            kind TEXT NOT NULL,
            filename TEXT NOT NULL,
            line_start INT NOT NULL,
            line_end INT NOT NULL,
            parent_name TEXT,
            doc TEXT NOT NULL DEFAULT '',
            content_digest TEXT NOT NULL DEFAULT '',
            UNIQUE(filename, name, line_start)
        )
    """)
    cur.execute(f'CREATE INDEX IF NOT EXISTS "{sym_table}_name_idx" ON "{sym_table}" (lower(name))')
    cur.execute(f"""
        CREATE TABLE IF NOT EXISTS "{call_table}" (
            id SERIAL PRIMARY KEY,
            caller_name TEXT NOT NULL,
            caller_file TEXT NOT NULL,
            caller_line INT NOT NULL,
            callee_name TEXT NOT NULL,
            call_context TEXT DEFAULT '',
            UNIQUE(caller_file, caller_line, callee_name)
        )
    """)
    # Migration: add call_context column if table already exists without it
    cur.execute(f"""
        DO $$ BEGIN
            ALTER TABLE "{call_table}" ADD COLUMN call_context TEXT DEFAULT '';
        EXCEPTION WHEN duplicate_column THEN NULL;
        END $$
    """)
    cur.execute(f'CREATE INDEX IF NOT EXISTS "{call_table}_caller_idx" ON "{call_table}" (lower(caller_name))')
    cur.execute(f'CREATE INDEX IF NOT EXISTS "{call_table}_callee_idx" ON "{call_table}" (lower(callee_name))')
    cur.execute(f'CREATE INDEX IF NOT EXISTS "{sym_table}_qname_idx" ON "{sym_table}" (lower(qualified_name))')
    conn.commit()
    cur.close()


def purge_symbols_for_files(conn, project, filenames):
    """Remove symbols and calls for the given filenames."""
    if not filenames:
        return
    sym_table = f"symbols_{project}"
    call_table = f"calls_{project}"
    flist = list(filenames)
    cur = conn.cursor()
    cur.execute(f'DELETE FROM "{sym_table}" WHERE filename = ANY(%s)', (flist,))
    cur.execute(f'DELETE FROM "{call_table}" WHERE caller_file = ANY(%s)', (flist,))
    try:
        cur.execute(f'DELETE FROM "imports_{project}" WHERE filename = ANY(%s)', (flist,))
    except Exception:
        conn.rollback()  # table predates this version; nothing to purge
    conn.commit()
    cur.close()


def count_symbols(conn, project) -> int:
    """Return number of rows in symbols table, or 0 if table doesn't exist."""
    sym_table = f"symbols_{project}"
    cur = conn.cursor()
    try:
        cur.execute(f'SELECT COUNT(*) FROM "{sym_table}"')
        count = cur.fetchone()[0]
    except Exception:
        conn.rollback()
        count = 0
    cur.close()
    return count


def persist_imports(conn, project, all_imports):
    """
    Batch insert import and inheritance rows —

    Kept out of `persist_symbols` on purpose: an import is not a symbol, and
    putting it in the symbols table would turn every `import foo` into a graph
    node. These rows exist to tell the resolver which names a file can see.
    """
    if not all_imports:
        return
    imp_table = f"imports_{project}"
    cur = conn.cursor()
    rows = [
        (i['filename'], i['module'], i.get('line', 0),
         i.get('kind', 'import'), i.get('owner'))
        for i in all_imports
    ]
    psycopg2.extras.execute_values(
        cur,
        f'INSERT INTO "{imp_table}" (filename, module, line, kind, owner) VALUES %s '
        f'ON CONFLICT (filename, module, kind, owner) DO NOTHING',
        rows,
    )
    conn.commit()
    cur.close()


def persist_symbols(conn, project, all_symbols, all_calls):
    """Batch insert symbols and calls."""
    if not all_symbols and not all_calls:
        return
    sym_table = f"symbols_{project}"
    call_table = f"calls_{project}"
    cur = conn.cursor()

    if all_symbols:
        sym_rows = [
            (s['name'], s.get('qualified_name', s['name']), s['kind'],
             s['filename'], s['line_start'], s['line_end'],
             s.get('parent_name'), s.get('doc', ''), s.get('content_digest', ''))
            for s in all_symbols
        ]
        psycopg2.extras.execute_values(
            cur,
            f"""INSERT INTO "{sym_table}" (name, qualified_name, kind, filename, line_start, line_end, parent_name, doc, content_digest)
                VALUES %s ON CONFLICT (filename, name, line_start) DO NOTHING""",
            sym_rows,
            page_size=DB_BATCH_SIZE,
        )

    if all_calls:
        call_rows = [
            (c['caller_name'], c['caller_file'], c['caller_line'], c['callee_name'],
             c.get('call_context', ''))
            for c in all_calls
        ]
        psycopg2.extras.execute_values(
            cur,
            f"""INSERT INTO "{call_table}" (caller_name, caller_file, caller_line, callee_name, call_context)
                VALUES %s ON CONFLICT (caller_file, caller_line, callee_name) DO NOTHING""",
            call_rows,
            page_size=DB_BATCH_SIZE,
        )

    conn.commit()
    cur.close()



def ensure_chunk_table(conn, table_name: str, embedding_dim: int):
    """Create the pgvector table if it doesn't exist, with content_digest column."""
    cur = conn.cursor()
    cur.execute("CREATE EXTENSION IF NOT EXISTS vector")
    cur.execute(f"""
        CREATE TABLE IF NOT EXISTS "{table_name}" (
            id SERIAL PRIMARY KEY,
            filename TEXT NOT NULL,
            code TEXT NOT NULL,
            embedding vector({embedding_dim}) NOT NULL,
            start_line INTEGER NOT NULL DEFAULT 0,
            end_line INTEGER NOT NULL DEFAULT 0,
            chunk_index INTEGER NOT NULL DEFAULT 0,
            content_digest TEXT NOT NULL DEFAULT '',
            UNIQUE(filename, chunk_index)
        )
    """)
    # Add content_digest column if table already exists without it (migration)
    cur.execute(f"""
        DO $$ BEGIN
            ALTER TABLE "{table_name}" ADD COLUMN content_digest TEXT NOT NULL DEFAULT '';
        EXCEPTION WHEN duplicate_column THEN NULL;
        END $$
    """)
    conn.commit()
    cur.close()


def load_indexed_hashes(conn, table_name: str) -> dict:
    """Return a dict of {filename: content_digest} from already-indexed data."""
    cur = conn.cursor()
    cur.execute(f'SELECT DISTINCT filename, content_digest FROM "{table_name}"')
    result = {row[0]: row[1] for row in cur.fetchall()}
    cur.close()
    conn.commit()  # close implicit transaction — don't hold it open during disk scan
    return result


def purge_files_from_index(conn, table_name: str, filenames: set):
    """Remove all chunks for the given filenames."""
    if not filenames:
        return
    cur = conn.cursor()
    cur.execute(
        f'DELETE FROM "{table_name}" WHERE filename = ANY(%s)',
        (list(filenames),),
    )
    conn.commit()
    cur.close()


def ensure_vector_index(conn, table_name: str):
    """Create the ivfflat vector index after data is loaded."""
    cur = conn.cursor()
    cur.execute(f'SELECT COUNT(*) FROM "{table_name}"')
    count = cur.fetchone()[0]
    if count < 100:
        logger.info("Too few rows (%d) for ivfflat index, skipping", count)
        cur.close()
        return
    lists = min(count // 10, 1000)
    cur.execute(f'DROP INDEX IF EXISTS "{table_name}_embedding_idx"')
    # Retry with fewer lists if memory is exceeded
    while lists >= 10:
        try:
            logger.info("Creating ivfflat vector index with %d lists...", lists)
            cur.execute(f"""
                CREATE INDEX "{table_name}_embedding_idx"
                ON "{table_name}" USING ivfflat (embedding vector_cosine_ops)
                WITH (lists = {lists})
            """)
            conn.commit()
            cur.close()
            logger.info("Vector index created with %d lists", lists)
            return
        except Exception as e:
            conn.rollback()
            if "maintenance_work_mem" in str(e) or "memory" in str(e).lower():
                lists = lists // 2
                logger.warning("Index creation failed (memory), retrying with %d lists...", lists)
            else:
                raise
    cur.close()
    logger.warning("Could not create vector index — all attempts exceeded memory")


def persist_chunk_batch(conn, table_name: str, model: SentenceTransformer, batch: list) -> int:
    """Embed and insert a batch of chunks. Returns number of rows inserted."""
    if not batch:
        return 0

    texts = [b[1] for b in batch]
    embeddings = model.encode(texts, show_progress_bar=False)

    # Release MPS GPU cache after each encode to prevent memory accumulation
    try:
        if str(model.device).startswith("mps"):
            import torch
            torch.mps.empty_cache()
    except Exception:
        pass

    rows = []
    for i, (filename, chunk_text_str, start_line, end_line, chunk_idx, fhash) in enumerate(batch):
        emb = embeddings[i].tolist()
        rows.append((filename, chunk_text_str, emb, start_line, end_line, chunk_idx, fhash))

    cur = conn.cursor()
    psycopg2.extras.execute_values(
        cur,
        f"""INSERT INTO "{table_name}" (filename, code, embedding, start_line, end_line, chunk_index, content_digest)
            VALUES %s ON CONFLICT (filename, chunk_index) DO UPDATE SET
                code = EXCLUDED.code,
                embedding = EXCLUDED.embedding,
                start_line = EXCLUDED.start_line,
                end_line = EXCLUDED.end_line,
                content_digest = EXCLUDED.content_digest""",
        rows,
        template="(%s, %s, %s::vector, %s, %s, %s, %s)",
        page_size=DB_BATCH_SIZE,
    )
    conn.commit()
    cur.close()
    return len(rows)


