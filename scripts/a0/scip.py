"""A minimal reader of SCIP indexes (A0.1, decision 64), for the audit only.

Decodes the protocol-buffer wire format of the fields the audit reads, with the
standard library alone: the index's metadata, and each document's path, position
encoding, occurrences and symbols. Everything else is skipped. This is local audit
tooling, isolated from the product; it is not P5-02's loader.

Field numbers are SCIP's (`scip.proto`): Index {1 metadata, 2 documents, 3
external_symbols}; Metadata {1 version, 2 tool_info, 3 project_root, 4
text_document_encoding}; ToolInfo {1 name, 2 version, 3 arguments}; Document {1
relative_path, 2 occurrences, 3 symbols, 4 language, 6 position_encoding};
Occurrence {1 range, 2 symbol, 3 symbol_roles, 7 enclosing_range}; SymbolInformation
{1 symbol, 4 relationships, 5 kind, 6 display_name, 8 enclosing_symbol}; Relationship
{1 symbol, 2 is_reference, 3 is_implementation, 4 is_type_definition, 5 is_definition}.
"""

from __future__ import annotations

from dataclasses import dataclass, field

# SymbolRole bits.
ROLE_DEFINITION = 1
ROLE_IMPORT = 2
ROLE_WRITE = 4
ROLE_READ = 8
ROLE_GENERATED = 16
ROLE_TEST = 32
ROLE_FORWARD_DEFINITION = 64

# PositionEncoding.
ENCODING_UNSPECIFIED = 0
ENCODING_UTF8 = 1
ENCODING_UTF16 = 2
ENCODING_UTF32 = 3


@dataclass
class Occurrence:
    start_line: int
    start_char: int
    end_line: int
    end_char: int
    symbol: str
    roles: int


@dataclass
class Relationship:
    symbol: str
    is_reference: bool = False
    is_implementation: bool = False
    is_type_definition: bool = False
    is_definition: bool = False


@dataclass
class SymbolInfo:
    symbol: str
    kind: int = 0
    display_name: str = ""
    enclosing_symbol: str = ""
    relationships: list[Relationship] = field(default_factory=list)


@dataclass
class Document:
    relative_path: str
    language: str = ""
    position_encoding: int = ENCODING_UNSPECIFIED
    occurrences: list[Occurrence] = field(default_factory=list)
    symbols: list[SymbolInfo] = field(default_factory=list)


@dataclass
class Index:
    tool_name: str = ""
    tool_version: str = ""
    tool_arguments: list[str] = field(default_factory=list)
    project_root: str = ""
    documents: list[Document] = field(default_factory=list)
    external_symbols: list[SymbolInfo] = field(default_factory=list)


class DecodeError(ValueError):
    pass


def _varint(buf: bytes, i: int) -> tuple[int, int]:
    result = 0
    shift = 0
    while True:
        if i >= len(buf):
            raise DecodeError("truncated varint")
        b = buf[i]
        i += 1
        result |= (b & 0x7F) << shift
        if not b & 0x80:
            return result, i
        shift += 7
        if shift > 70:
            raise DecodeError("varint too long")


def _fields(buf: bytes):
    """(field number, wire type, value) for each field of one message."""
    i = 0
    n = len(buf)
    while i < n:
        key, i = _varint(buf, i)
        number, wire = key >> 3, key & 7
        if wire == 0:
            value, i = _varint(buf, i)
        elif wire == 1:
            value, i = buf[i : i + 8], i + 8
        elif wire == 2:
            length, i = _varint(buf, i)
            value, i = buf[i : i + length], i + length
            if i > n:
                raise DecodeError("truncated field")
        elif wire == 5:
            value, i = buf[i : i + 4], i + 4
        else:
            raise DecodeError(f"unsupported wire type {wire}")
        yield number, wire, value


def _int32(v: int) -> int:
    v &= 0xFFFFFFFFFFFFFFFF
    return v - (1 << 64) if v >= 1 << 63 else v


def _packed(wire: int, value) -> list[int]:
    if wire == 0:
        return [_int32(value)]
    out = []
    i = 0
    while i < len(value):
        v, i = _varint(value, i)
        out.append(_int32(v))
    return out


def _range(values: list[int]) -> tuple[int, int, int, int]:
    if len(values) == 3:
        return values[0], values[1], values[0], values[2]
    if len(values) == 4:
        return values[0], values[1], values[2], values[3]
    raise DecodeError(f"a range of {len(values)} elements")


def _relationship(buf: bytes) -> Relationship:
    r = Relationship(symbol="")
    for number, _wire, value in _fields(buf):
        if number == 1:
            r.symbol = value.decode("utf-8")
        elif number == 2:
            r.is_reference = bool(value)
        elif number == 3:
            r.is_implementation = bool(value)
        elif number == 4:
            r.is_type_definition = bool(value)
        elif number == 5:
            r.is_definition = bool(value)
    return r


def _symbol(buf: bytes) -> SymbolInfo:
    s = SymbolInfo(symbol="")
    for number, _wire, value in _fields(buf):
        if number == 1:
            s.symbol = value.decode("utf-8")
        elif number == 4:
            s.relationships.append(_relationship(value))
        elif number == 5:
            s.kind = value
        elif number == 6:
            s.display_name = value.decode("utf-8")
        elif number == 8:
            s.enclosing_symbol = value.decode("utf-8")
    return s


def _occurrence(buf: bytes) -> Occurrence:
    rng: list[int] = []
    symbol = ""
    roles = 0
    for number, wire, value in _fields(buf):
        if number == 1:
            rng.extend(_packed(wire, value))
        elif number == 2:
            symbol = value.decode("utf-8")
        elif number == 3:
            roles = value
    sl, sc, el, ec = _range(rng)
    return Occurrence(sl, sc, el, ec, symbol, roles)


def _document(buf: bytes) -> Document:
    d = Document(relative_path="")
    for number, _wire, value in _fields(buf):
        if number == 1:
            d.relative_path = value.decode("utf-8")
        elif number == 2:
            d.occurrences.append(_occurrence(value))
        elif number == 3:
            d.symbols.append(_symbol(value))
        elif number == 4:
            d.language = value.decode("utf-8")
        elif number == 6:
            d.position_encoding = value
    return d


def read_index(data: bytes) -> Index:
    index = Index()
    for number, _wire, value in _fields(data):
        if number == 1:
            for n2, _w2, v2 in _fields(value):
                if n2 == 2:
                    for n3, _w3, v3 in _fields(v2):
                        if n3 == 1:
                            index.tool_name = v3.decode("utf-8")
                        elif n3 == 2:
                            index.tool_version = v3.decode("utf-8")
                        elif n3 == 3:
                            index.tool_arguments.append(v3.decode("utf-8"))
                elif n2 == 3:
                    index.project_root = v2.decode("utf-8")
        elif number == 2:
            index.documents.append(_document(value))
        elif number == 3:
            index.external_symbols.append(_symbol(value))
    return index


def is_local(symbol: str) -> bool:
    """A SCIP local symbol: one with no meaning outside its document."""
    return symbol.startswith("local ")


def package(symbol: str) -> str | None:
    """A global symbol's package name: its third space-separated field (`<scheme>
    <manager> <name> <version> <descriptors>`; a space inside a field is doubled)."""
    if is_local(symbol):
        return None
    fields = []
    i = 0
    cur = []
    while i < len(symbol) and len(fields) < 4:
        ch = symbol[i]
        if ch == " ":
            if i + 1 < len(symbol) and symbol[i + 1] == " ":
                cur.append(" ")
                i += 2
                continue
            fields.append("".join(cur))
            cur = []
        else:
            cur.append(ch)
        i += 1
    if len(fields) < 3:
        return None
    return fields[2]


def short(symbol: str | None) -> str | None:
    """A symbol without its scheme, manager and version: `<package> <descriptors>`. The
    version field can be a URL (rust-analyzer's standard library), which results never
    carry."""
    if not symbol or is_local(symbol):
        return symbol
    fields = []
    i = 0
    start = 0
    while i < len(symbol) and len(fields) < 4:
        if symbol[i] == " ":
            if i + 1 < len(symbol) and symbol[i + 1] == " ":
                i += 2
                continue
            fields.append(symbol[start:i])
            start = i + 1
        i += 1
    if len(fields) < 4:
        return symbol
    return fields[2] + " " + symbol[start:]
