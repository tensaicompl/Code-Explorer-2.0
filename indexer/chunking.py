"""Turning a source file into the pieces that get embedded.

Two strategies. Most files are split on size with an overlap, which is crude but
language-agnostic. Ada files are split on subprogram boundaries instead, because
the symbol extractor has already located them and a chunk that holds exactly one
procedure retrieves far better than one that holds the tail of one and the head
of the next.
"""
import hashlib
import os
import re

from settings import CHUNK_OVERLAP, CHUNK_SIZE, DATA_EXTENSIONS

#: An Ada subprogram longer than this is sub-chunked rather than kept whole.
ADA_PROC_CHUNK_MAX = 3000

def classify_ext(path: str) -> str:
    """
    The extension this file is dispatched on — filename-aware, not `splitext`.

    `os.path.splitext(".env")` returns `(".env", "")`: a dotfile has no extension
    as far as the standard library is concerned, and `.env.local` splits to
    `.local`, `.env.k8s` to `.k8s`. Of the six dotenv files in the indexed repos
    exactly one is named plainly `.env`, so an extension-keyed rule would have
    matched one file in six and reported success.

    Every site that decides "is this a source file / which extractor / is this
    over the data cap" routes through here, so the three answers cannot diverge.
    """
    name = os.path.basename(path)
    if name == ".env" or name.startswith(".env."):
        return ".env"
    return os.path.splitext(name)[1].lower()



def split_text_into_chunks(text: str, chunk_size: int = CHUNK_SIZE, overlap: int = CHUNK_OVERLAP) -> list:
    """Split text into overlapping chunks. Returns list of (split_text_into_chunks, start_char, end_char)."""
    if len(text) <= chunk_size:
        return [(text, 0, len(text))]

    chunks = []
    start = 0
    while start < len(text):
        end = start + chunk_size
        chunk = text[start:end]

        # Try to break at a newline boundary
        if end < len(text):
            last_newline = chunk.rfind("\n")
            if last_newline > chunk_size // 2:
                end = start + last_newline + 1
                chunk = text[start:end]

        chunks.append((chunk, start, end))
        start = end - overlap
        if start >= len(text):
            break

    return chunks


def offset_to_line_number(text: str, char_pos: int) -> int:
    return text[:char_pos].count("\n") + 1



def chunk_ada_by_procedure(content: str, symbols: list) -> list:
    """Chunk an Ada file aligned to procedure/function scope boundaries.

    Each procedure/function body (line_start..line_end from symbol extraction) becomes
    its own chunk (or sub-chunks if > ADA_PROC_CHUNK_MAX chars), prefixed with a comment
    identifying the procedure. Inter-procedure code (package headers, with clauses, etc.)
    is chunked with the normal character-based chunker.

    Returns list of (split_text_into_chunks, start_char, end_char) — same format as split_text_into_chunks().
    """
    lines = content.split('\n')

    # Build sorted list of (line_start, line_end, qualified_name) for procedures/functions
    proc_scopes = sorted(
        [
            (s['line_start'], s['line_end'], s.get('qualified_name', s['name']))
            for s in symbols
            if s['kind'] in ('procedure', 'function') and s.get('line_end', 0) > s.get('line_start', 0)
        ],
        key=lambda x: x[0],
    )

    if not proc_scopes:
        # No procedures found — fall back to default chunker
        return split_text_into_chunks(content)

    def lines_to_char(line_1based):
        """Convert 1-based line number to char offset in content."""
        char = 0
        for i, ln in enumerate(lines):
            if i + 1 >= line_1based:
                return char
            char += len(ln) + 1  # +1 for newline
        return len(content)

    result = []
    covered_up_to_line = 0  # 1-based, exclusive end of last covered region

    for line_start, line_end, qual_name in proc_scopes:
        # Clamp line_end to actual file length
        line_end = min(line_end, len(lines))

        # Gap between previous covered region and this procedure
        if line_start > covered_up_to_line + 1:
            gap_start_char = lines_to_char(covered_up_to_line + 1)
            gap_end_char = lines_to_char(line_start)
            gap_text = content[gap_start_char:gap_end_char]
            if gap_text.strip():
                for sub in split_text_into_chunks(gap_text):
                    sub_text, sub_s, sub_e = sub
                    result.append((sub_text, gap_start_char + sub_s, gap_start_char + sub_e))

        # Procedure body chunk(s)
        proc_start_char = lines_to_char(line_start)
        proc_end_char = lines_to_char(line_end + 1)
        proc_body = content[proc_start_char:proc_end_char]
        prefix = f"-- [{qual_name}]\n"

        if len(prefix) + len(proc_body) <= ADA_PROC_CHUNK_MAX:
            chunk_str = prefix + proc_body
            result.append((chunk_str, proc_start_char, proc_end_char))
        else:
            # Sub-chunk large procedure bodies
            for idx, sub in enumerate(split_text_into_chunks(proc_body)):
                sub_text, sub_s, sub_e = sub
                header = prefix if idx == 0 else f"-- [{qual_name} cont.]\n"
                chunk_str = header + sub_text
                result.append((chunk_str, proc_start_char + sub_s, proc_start_char + sub_e))

        covered_up_to_line = line_end

    # Tail: anything after the last procedure
    if covered_up_to_line < len(lines):
        tail_start_char = lines_to_char(covered_up_to_line + 1)
        tail_text = content[tail_start_char:]
        if tail_text.strip():
            for sub in split_text_into_chunks(tail_text):
                sub_text, sub_s, sub_e = sub
                result.append((sub_text, tail_start_char + sub_s, tail_start_char + sub_e))

    return result if result else split_text_into_chunks(content)


def content_digest(content: str) -> str:
    """Compute a short hash of file content for change detection."""
    return hashlib.md5(content.encode("utf-8")).hexdigest()


def _strip_null_bytes(content: str) -> str:
    """Remove NUL bytes that PostgreSQL text columns cannot store."""
    return content.replace("\x00", "")


