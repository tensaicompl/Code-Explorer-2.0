"""The protobuf, markdown and dotenv extractors, against fixtures built to trip them.

    cd indexer && ../.venv/bin/python -m pytest graph_pass/tests -q

WHY THESE EXIST. Both regressions found while building — protobuf type
references resolving against Rust structs, and markdown headings entering the
call resolver's candidate pool — looked completely fine until someone compared a
number to its documented value. Nothing about a wrong graph is loud. Every case
asserted here was verified once by hand at build time; committing it is what
stops the next change from quietly undoing it.

The fixtures deliberately contain what a regex-per-line parser gets wrong:
a nested message, a `oneof` whose fields belong to the enclosing message, a
single-line `message Foo {}`, a `#` heading inside a fenced code block, a level-3
heading that must fold rather than become a node, a multi-line quoted dotenv
value, and the `export VAR=` form.

`fixtures/env.example` has no leading dot ON PURPOSE — its name on disk is
irrelevant, because the extractor takes (content, rel_path). The real dispatch
decision is covered by `test_classify_ext`. Its values are the literal string
MUSTNOTLEAK so a redaction failure is greppable rather than plausible.
"""

from __future__ import annotations

import os
import sys
import types

import pytest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))

from graph_pass.env_config import extract_env_symbols, redact  # noqa: E402
from graph_pass.markdown_docs import extract_markdown_symbols  # noqa: E402
from graph_pass.proto_defs import extract_proto_symbols  # noqa: E402

FIXTURES = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
CANARY = "MUSTNOTLEAK"


def _read(name: str) -> str:
    with open(os.path.join(FIXTURES, name), encoding="utf-8") as fh:
        return fh.read()


def _by_kind(rows: list[dict], kind: str) -> list[dict]:
    return [r for r in rows if r["kind"] == kind]


def _names(rows: list[dict], kind: str) -> set[str]:
    return {r["name"] for r in _by_kind(rows, kind)}


# ── protobuf ────────────────────────────────────────────────────────────────

@pytest.fixture(scope="module")
def proto() -> list[dict]:
    return extract_proto_symbols(_read("shop.proto"), "api/shop.proto")


def test_proto_finds_the_service_and_its_rpcs(proto):
    assert _names(proto, "service") == {"OrderService"}
    assert _names(proto, "rpc") == {"PlaceOrder", "WatchOrder"}
    # Every rpc is owned by its service — this is what becomes the contains edge,
    # and without it the rpcs hang off the file and nothing else.
    assert all(r["parent_name"] == "OrderService" for r in _by_kind(proto, "rpc"))


def test_proto_sees_nested_messages(proto):
    """Upstream anchors `^message`, so an indented one is invisible."""
    assert "Shipping" in _names(proto, "message")
    shipping = next(r for r in proto if r["name"] == "Shipping")
    assert shipping["parent_name"] == "Order"
    assert shipping["qualified_name"] == "shop.api.v1.Order.Shipping"


def test_proto_attributes_oneof_fields_to_the_message(proto):
    """A `oneof` is a scope in the file, not a node — its fields are the message's."""
    order_fields = {r["name"] for r in _by_kind(proto, "field")
                    if r["parent_name"] == "Order"}
    assert {"card_token", "invoice_ref"} <= order_fields
    assert {"id", "customer_email", "items", "status", "metadata"} <= order_fields


def test_proto_single_line_block_closes_its_own_frame(proto):
    """`message Empty {}` opens and closes on one line."""
    empty = next(r for r in proto if r["name"] == "Empty")
    assert empty["line_start"] == empty["line_end"]


def test_proto_line_ranges_are_sane(proto):
    for r in proto:
        assert r["line_end"] >= r["line_start"], r
        assert r["line_start"] >= 1


def test_proto_lifts_doc_comments(proto):
    service = next(r for r in proto if r["kind"] == "service")
    assert service["doc"].startswith("Order lifecycle for the storefront.")
    place = next(r for r in proto if r["name"] == "PlaceOrder")
    assert place["doc"] == "Place a single order and get its confirmation."
    # A message with no comment above it gets an empty doc, not the previous one's.
    assert next(r for r in proto if r["name"] == "LineItem")["doc"] == ""


def test_proto_emits_type_ref_not_references(proto):
    """
    The kinds must NOT be merged. `references` resolves against tables; sharing it
    would resolve a protobuf payload against SQL and against every same-named
    class in the repo (26 such collisions in ff alone).
    """
    assert not _by_kind(proto, "references")
    refs = {(r["parent_name"], r["name"]) for r in _by_kind(proto, "type_ref")}
    assert ("PlaceOrder", "PlaceOrderRequest") in refs
    assert ("PlaceOrder", "Order") in refs
    assert ("WatchOrder", "OrderEvent") in refs


def test_proto_enum_values_fold_not_explode(proto):
    values = {r["name"] for r in _by_kind(proto, "enum_value")}
    assert "ORDER_STATUS_DELIVERED" in values
    assert all(r["parent_name"] == "OrderStatus" for r in _by_kind(proto, "enum_value"))


# ── markdown ────────────────────────────────────────────────────────────────

@pytest.fixture(scope="module")
def md() -> list[dict]:
    return extract_markdown_symbols(_read("shop.md"), "README.md")


def test_markdown_ignores_headings_inside_fenced_code(md):
    """A `# comment` in a ```bash block is shell, not a document heading."""
    names = _names(md, "document") | _names(md, "subheading")
    assert not any("shell comment" in n for n in names)
    assert not any("fence tracking" in n for n in names)


def test_markdown_skips_front_matter(md):
    names = _names(md, "document")
    assert "title: Shop fixture" not in names
    assert "Shop" in names


def test_markdown_nests_level_two_under_level_one(md):
    docs = {r["name"]: r for r in _by_kind(md, "document")}
    assert set(docs) == {"Shop", "Schema", "API", "Configuration"}
    assert docs["Shop"]["parent_name"] is None
    for name in ("Schema", "API", "Configuration"):
        assert docs[name]["parent_name"] == "Shop", name


def test_markdown_folds_level_three(md):
    """Level 3+ is metadata on its ancestor, like a column on its table."""
    subs = {r["name"]: r for r in _by_kind(md, "subheading")}
    assert "Tables" in subs
    assert subs["Tables"]["parent_name"] == "Schema"


def test_markdown_summarises_from_the_first_paragraph(md):
    docs = {r["name"]: r for r in _by_kind(md, "document")}
    assert docs["Shop"]["doc"].startswith("A deliberately tiny project")
    assert docs["API"]["doc"] == "Defined in api/shop.proto."
    # Inline code and links are unwrapped, not left as punctuation.
    assert "`" not in docs["API"]["doc"]


def test_markdown_spans_reach_the_next_heading(md):
    docs = {r["name"]: r for r in _by_kind(md, "document")}
    assert docs["Shop"]["line_end"] > docs["Configuration"]["line_start"]
    for r in md:
        assert r["line_end"] >= r["line_start"], r


def test_markdown_without_a_level_one_still_yields_a_node():
    rows = extract_markdown_symbols("## Setup\n\nRun it.\n", "docs/setup.md")
    docs = _by_kind(rows, "document")
    # The synthetic root keeps a file that opens at `##` visible in DOCS.
    assert docs[0]["name"] == "setup.md"
    assert docs[0]["parent_name"] is None
    assert {"setup.md", "Setup"} == {r["name"] for r in docs}


def test_markdown_with_no_headings_at_all_still_yields_a_node():
    rows = extract_markdown_symbols("Just prose, no headings.\n", "notes.md")
    docs = _by_kind(rows, "document")
    assert len(docs) == 1
    assert docs[0]["name"] == "notes.md"
    assert docs[0]["doc"] == "Just prose, no headings."


# ── dotenv ──────────────────────────────────────────────────────────────────

@pytest.fixture(scope="module")
def env_text() -> str:
    return _read("env.example")


def test_env_captures_names_including_the_export_form(env_text):
    rows = extract_env_symbols(env_text, ".env.example")
    assert _names(rows, "env_var") == {
        "DATABASE_URL", "API_TOKEN", "SIGNING_MATERIAL", "LOG_LEVEL",
    }


def test_env_does_not_read_a_multiline_value_as_variables(env_text):
    """`NOT_A_VAR=` sits INSIDE a quoted value and is not a declaration."""
    assert "NOT_A_VAR" not in _names(extract_env_symbols(env_text, ".env"), "env_var")


def test_env_lifts_comments_as_docs(env_text):
    rows = {r["name"]: r for r in extract_env_symbols(env_text, ".env")}
    assert rows["DATABASE_URL"]["doc"] == "Connection string for the order database."
    assert rows["LOG_LEVEL"]["doc"] == ""  # separated by a blank line


def test_env_never_captures_a_value(env_text):
    for row in extract_env_symbols(env_text, ".env"):
        for field in ("name", "qualified_name", "doc"):
            assert CANARY not in row[field], row


def test_redaction_removes_every_value_and_keeps_the_line_count(env_text):
    red = redact(env_text)
    assert CANARY not in red
    assert "BEGIN BLOCK" not in red
    # Line count must survive: offset_to_line_number maps chunk offsets back to lines, so
    # one dropped newline shifts every span after it.
    assert red.count("\n") == env_text.count("\n")
    # Names and comments survive, which is the whole point of indexing the file.
    assert "DATABASE_URL=<redacted>" in red
    assert "export API_TOKEN=<redacted>" in red
    assert "# Connection string for the order database." in red


def test_redaction_leaves_a_file_with_no_assignments_alone():
    text = "# just a comment\n\nnot an assignment\n"
    assert redact(text) == text


# ── dispatch ────────────────────────────────────────────────────────────────

def _load_main():
    """
    Import build_corpus.py without its heavy runtime dependencies.

    build_corpus.py imports sentence_transformers at module scope, which pulls torch and
    a 550 MB model. The backend venv deliberately does not have them, so
    the import is stubbed rather than installed — nothing under test touches it.
    """
    if "sentence_transformers" not in sys.modules:
        stub = types.ModuleType("sentence_transformers")
        stub.SentenceTransformer = object
        sys.modules["sentence_transformers"] = stub
    import build_corpus  # noqa: PLC0415

    return build_corpus


@pytest.mark.parametrize(
    "path,expected",
    [
        (".env", ".env"),
        (".env.local", ".env"),
        (".env.k8s", ".env"),
        (".env.example", ".env"),
        ("frontend/.env.local", ".env"),
        ("api/shop.proto", ".proto"),
        ("README.md", ".md"),
        ("docs/GUIDE.MD", ".md"),
        ("src/main.rs", ".rs"),
        # Neither is a source file, and both must stay out of SOURCE_EXTENSIONS.
        # A dotfile other than .env* still comes back empty — `.env` is a special
        # case because it is the one dotfile worth parsing, not because dotfiles
        # in general get a synthetic extension.
        ("Makefile", ""),
        (".gitignore", ""),
    ],
)
def test_classify_ext(path, expected):
    """
    `os.path.splitext(".env")` is `(".env", "")` and `.env.local` is `.local`.

    Five of the six dotenv files in the indexed repos would never have been
    opened by an extension-keyed rule, and the run would have reported success.
    """
    assert _load_main().classify_ext(path) == expected


def test_the_three_extensions_are_actually_discoverable():
    """An extractor whose extension is not in SOURCE_EXTENSIONS does nothing, silently."""
    assert {".proto", ".md", ".env"} <= _load_main().SOURCE_EXTENSIONS


def test_only_dotenv_is_redacted_before_storage():
    """Redaction must not quietly mangle source files."""
    main = _load_main()
    code = "fn main() { let x = 1; }\n"
    assert main._redact_for_storage(".rs", code) == code
    assert CANARY not in main._redact_for_storage(".env", _read("env.example"))
