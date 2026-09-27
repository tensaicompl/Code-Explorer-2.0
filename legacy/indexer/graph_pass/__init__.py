"""
Deterministic graph builder — THE novel component of this project.

Turns the existing search index (symbols_*, calls_*, file tree) into a
KnowledgeGraph with NO LLM involvement. The viewer this schema comes from builds
its graph from an in-session
LLM agent pipeline, which cannot run headless on a cron — so this is
written, not ported.

Hooks into indexer/build_corpus.py's existing per-file loop immediately after
    syms, cals = _extract_file_symbols(content, ext, rel_path)      # build_corpus.py:1223
reusing the already-open `content`, at zero extra file I/O. Mirror the symbol
pass's three call sites:
    setup_graph_tables      <- ensure_symbol_tables      (build_corpus.py:776-819, called 1143)
    delete_graph_for_files  <- purge_symbols_for_files (build_corpus.py:821-832, called 1173)
    flush_graph             <- the symbol flush

THREE ANTI-PATTERNS TO AVOID, all present in the code being mirrored:
  1. NO `ON CONFLICT DO NOTHING` — it silently drops rows. Measured: the indexer
     logged 7,664 calls, the table held 7,610. Use delete-then-insert per changed file.
  2. NO table-level recovery sentinel — `chunk_count > 0 AND symbol_count == 0`
     never fires again once one row survives, so a partial run cannot self-heal.
     Track per-file extraction state.
  3. NO unread content_digest column — symbols_*.content_digest is written and never read.

Modules:
    builder.py          symbols + calls + tree -> nodes, edges, layers
    resolver.py         callee-name -> symbol resolution + confidence bands
    layers.py           deterministic layer assignment (MANDATORY, see below)
    sql_ddl.py          .sql -> table / schema nodes  (= "databases")
    config_extract.py   yaml/json/toml/env/compose -> config / service nodes
    imports_extract.py  tree-sitter import capture -> imports edges
    emit.py             write graph_nodes / graph_edges, incrementally

LAYERS ARE MANDATORY. A graph with zero layers renders a fully
interactive, completely EMPTY canvas with no error message — worse than a crash,
because it looks like the tool ran and found nothing. Assert in the builder AND
in the API serializer:  1 <= len(layers) <= 60, every layer non-empty.
"""
