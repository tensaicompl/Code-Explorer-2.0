#!/usr/bin/env python3
"""Extracts named functions, verbatim, from one C source into a new one.

Vendoring normally takes whole files. For the import resolver it cannot: the source
it lives in also scans manifests and discovers packages, which this project does in
Rust. So only the resolver's own functions are taken, together with the file-scope
declarations they use, and nothing else.

    extract-functions.py <source.c> <function-list> <output.c>

Functions are emitted in the order the source defines them, with their leading
comments, and byte for byte. A listed function the source does not define is an
error: it means the list and the pinned commit disagree.

The list may also carry directives, each on its own line:

    header <text>          a line of the comment that opens the output, saying what
                           the subset is; required, so every subset says so itself
    drop-include <path>    an include of the source the subset does not need
"""
import pathlib
import re
import sys


def blank_comments_and_strings(src):
    """Same length as the input, comments and literals blanked, so offsets map back."""
    out, i, n = [], 0, len(src)
    while i < n:
        if src.startswith("/*", i):
            j = src.find("*/", i + 2)
            j = n if j < 0 else j + 2
            out.append("".join(c if c == "\n" else " " for c in src[i:j]))
            i = j
        elif src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            out.append(" " * (j - i))
            i = j
        elif src[i] in "\"'":
            q, j = src[i], i + 1
            while j < n and src[j] != q:
                j += 2 if src[j] == "\\" else 1
            out.append(q + "".join(c if c == "\n" else " " for c in src[i + 1:j]) + (q if j < n else ""))
            i = j + 1
        else:
            out.append(src[i])
            i += 1
    return "".join(out)


DEF = re.compile(r"^(?:static\s+)?(?:inline\s+)?[A-Za-z_][\w\s\*]*?\b([A-Za-z_]\w*)\s*\([^;{]*?\)\s*\{", re.M)
IDENT = re.compile(r"\b[A-Za-z_]\w*\b")


def top_level_items(raw, clean):
    """Every file-scope item: (kind, name, start, end), in source order."""
    items, depth, i = [], 0, 0
    n = len(clean)
    # function definitions
    for m in DEF.finditer(clean):
        if clean[:m.start()].count("{") != clean[:m.start()].count("}"):
            continue
        d, j = 0, m.end() - 1
        while j < n:
            if clean[j] == "{":
                d += 1
            elif clean[j] == "}":
                d -= 1
                if d == 0:
                    break
            j += 1
        # widen the start over a leading comment block
        start = m.start()
        k = raw.rfind("\n", 0, start)
        while True:
            prev_end = k
            prev_start = raw.rfind("\n", 0, prev_end) if prev_end > 0 else -1
            line = raw[prev_start + 1:prev_end].strip() if prev_end > 0 else ""
            if line.startswith(("/*", "*", "//")) or line.endswith("*/"):
                k = prev_start
                start = prev_start + 1
                if prev_start < 0:
                    break
            else:
                break
        items.append(("function", m.group(1), start, j + 1))
    return sorted(items, key=lambda t: t[2])


def main():
    source, listing, output = map(pathlib.Path, sys.argv[1:4])
    raw = source.read_text()
    clean = blank_comments_and_strings(raw)
    entries = [l.strip() for l in listing.read_text().splitlines() if l.strip() and not l.startswith("#")]
    # "drop-include <path>" removes an include the source carries for code that is
    # not extracted. Named in the list, with its reason beside it, so what is dropped
    # is reviewed rather than inferred.
    drop_includes = {e.split(None, 1)[1] for e in entries if e.startswith("drop-include ")}
    header_lines = [e.split(None, 1)[1] if " " in e else "" for e in entries if e.startswith("header")]
    if not header_lines:
        sys.exit("error: the list has no header directive saying what the subset is")
    wanted = [e for e in entries if not e.startswith(("drop-include ", "header"))]

    functions = top_level_items(raw, clean)
    by_name = {name: (s, e) for _, name, s, e in functions}
    missing = [w for w in wanted if w not in by_name]
    if missing:
        sys.exit(f"error: the source does not define: {', '.join(missing)}")

    fn_ranges = [(s, e) for _, _, s, e in functions]

    def in_function(pos):
        return any(s <= pos < e for s, e in fn_ranges)

    chosen = sorted(((by_name[w][0], by_name[w][1], w) for w in wanted))
    used = set()
    for s, e, _ in chosen:
        used |= set(IDENT.findall(clean[s:e]))

    preamble = []
    offset = 0
    buffered = []
    for line in raw.splitlines(keepends=True):
        start = offset
        offset += len(line)
        if in_function(start):
            continue
        stripped = line.strip()
        if stripped.startswith("#include"):
            named = re.match(r'#include\s*[<"]([^>"]+)[>"]', stripped)
            target = named.group(1) if named else stripped
            if target not in drop_includes:
                preamble.append(line)
            continue
        if stripped.startswith(("#define", "#undef", "#if", "#ifdef", "#ifndef", "#else", "#elif", "#endif")):
            preamble.append(line)
            continue
        buffered.append(line)

    # File-scope declarations, split on top-level semicolons. Each is kept when a
    # chosen function, or a declaration already kept, uses a name it declares; the
    # names it merely mentions (its types, `const`, `char`) do not count.
    decl_text = "".join(buffered)
    decl_clean = blank_comments_and_strings(decl_text)
    decls, depth, last = [], 0, 0
    for idx, ch in enumerate(decl_clean):
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
        elif ch == ";" and depth == 0:
            decls.append((decl_text[last:idx + 1], decl_clean[last:idx + 1]))
            last = idx + 1

    candidates = []
    for text, cleaned in decls:
        body_at = len(cleaned) - len(cleaned.lstrip())
        if body_at >= len(cleaned):
            continue
        statement = text[body_at:].rstrip()
        leading = text[:body_at]
        candidates.append((attached_comment(leading) + statement + "\n",
                           declared_names(cleaned[body_at:]),
                           set(IDENT.findall(cleaned[body_at:]))))

    kept = [False] * len(candidates)
    changed = True
    while changed:
        changed = False
        for k, (_, declared, mentioned) in enumerate(candidates):
            if not kept[k] and declared & used:
                kept[k] = True
                used |= mentioned
                changed = True
    kept_decls = [candidates[k][0] for k in range(len(candidates)) if kept[k]]

    body = [raw[s:e] + "\n" for s, e, _ in chosen]
    header = "/*\n" + "".join(f" * {l}\n".replace(" * \n", " *\n") for l in header_lines) + " */\n\n"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(header + "".join(preamble) + "\n" + "\n".join(kept_decls) + "\n" + "\n".join(body))
    print(f"extracted {len(chosen)} functions, {len(kept_decls)} file-scope declarations, "
          f"{len(preamble)} preprocessor lines -> {output}")


def attached_comment(leading):
    """The comment block directly above a declaration, with no blank line between."""
    lines = leading.split("\n")
    # `leading` ends where the declaration starts, so its last element is the
    # indentation before it; walk up from the line above.
    block = []
    for line in reversed(lines[:-1]):
        s = line.strip()
        if not s:
            break
        block.append(line)
    block.reverse()
    text = "\n".join(block)
    if not block or not blank_comments_and_strings(text).strip() == "":
        return ""
    return text + "\n"


def declared_names(stmt):
    """The names a file-scope declaration introduces, from its comment-free text."""
    names = set()
    # Tags and enumerators.
    for m in re.finditer(r"\b(?:struct|union|enum)\s+([A-Za-z_]\w*)\s*\{", stmt):
        names.add(m.group(1))
    for m in re.finditer(r"\benum\b[^{]*\{([^}]*)\}", stmt):
        for part in m.group(1).split(","):
            ident = IDENT.match(part.strip())
            if ident:
                names.add(ident.group(0))
    # Declarators: the identifier ending each top-level declarator, before an array
    # bound, an initialiser, a parameter list, a comma or the semicolon.
    depth, flat = 0, []
    for ch in stmt:
        if ch in "{(":
            depth += 1
            flat.append(" " if ch == "{" else "(")
            continue
        if ch in "})":
            depth -= 1
            continue
        flat.append(ch if depth == 0 else " ")
    flat = "".join(flat)
    for m in re.finditer(r"([A-Za-z_]\w*)\s*(?=\[|=|\(|,|;)", flat):
        if m.group(1) not in ("struct", "union", "enum", "const", "static"):
            names.add(m.group(1))
    return names


if __name__ == "__main__":
    main()
