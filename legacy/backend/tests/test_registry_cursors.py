"""The registry uses dictionary rows for configured projects and tuples for legacy ones."""
from unittest.mock import Mock

import pytest

from app.harvest.registry import _chunk_count, _indexed_keys


def test_indexed_tables_from_dictionary_cursor():
    cursor = Mock()
    cursor.fetchall.return_value = [{"table_name": "code_chunks_shop_develop"}]
    assert _indexed_keys(cursor) == {"shop_develop"}


@pytest.mark.parametrize("row", [(570,), {"chunk_count": 570}])
def test_chunk_counts_from_both_cursor_types(row):
    cursor = Mock()
    cursor.fetchone.return_value = row
    assert _chunk_count(cursor, "shop_develop") == 570
