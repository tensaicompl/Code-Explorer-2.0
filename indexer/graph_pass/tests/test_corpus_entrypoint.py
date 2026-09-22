"""Exercise the indexer's real dispatcher and failure reporting after refactoring."""
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

import build_corpus
from symbol_extractors import _extract_file_symbols


@pytest.mark.parametrize(
    ("extension", "source", "name"),
    [
        (".py", "def recovered():\n    return 1\n", "recovered"),
        (".ts", "export function recovered(): number { return 1; }", "recovered"),
        (".md", "# Recovery\n\nA working corpus.\n", "Recovery"),
        (".sql", "CREATE TABLE recovery (id INTEGER PRIMARY KEY);", "recovery"),
    ],
)
def test_dispatch_extracts_symbols(extension, source, name):
    symbols, _, _ = _extract_file_symbols(source, extension, "sample" + extension)
    assert name in {symbol["name"] for symbol in symbols}


def test_hash_scan_keeps_ddl_above_data_size_limit(tmp_path, monkeypatch):
    monkeypatch.setattr(build_corpus, "MAX_DATA_FILE_SIZE", 10)
    (tmp_path / "schema.sql").write_text("CREATE TABLE recovery (id INTEGER);")
    (tmp_path / "data.json").write_text('{"long": "data payload"}')
    assert set(build_corpus._scan_source_hashes(str(tmp_path))) == {"schema.sql"}


def test_failed_corpus_does_not_report_success(monkeypatch):
    monkeypatch.setattr(sys, "argv", ["build_corpus.py"])
    monkeypatch.setenv("INDEXER_DEVICE", "cpu")
    monkeypatch.setattr(build_corpus, "discover_project_streams", lambda: [("shop", "develop")])
    monkeypatch.setattr(build_corpus, "SentenceTransformer", lambda *a, **k: object())

    def fail(*args):
        raise RuntimeError("database unavailable")

    monkeypatch.setattr(build_corpus, "index_project_stream", fail)
    with pytest.raises(SystemExit, match="Indexing failed"):
        build_corpus.main()
