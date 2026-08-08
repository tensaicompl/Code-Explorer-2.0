"""Ada declarations, bodies and calls, found by pattern rather than by parser.

There is no tree-sitter grammar in use for Ada here, so this walks the text with
regexes and a small amount of state. It is deliberately conservative: a
construct it cannot classify is skipped rather than guessed at, because a wrong
symbol is worse than a missing one -- it sends every later call-graph query down
a path that does not exist.
"""
import logging
import re

logger = logging.getLogger("indexer.symbols.ada")

ADA_KEYWORDS = {
    'if', 'then', 'else', 'elsif', 'end', 'begin', 'return', 'declare',
    'loop', 'while', 'for', 'exit', 'case', 'when', 'null', 'raise',
    'pragma', 'with', 'use', 'type', 'subtype', 'is', 'in', 'out',
    'not', 'and', 'or', 'xor', 'mod', 'rem', 'abs', 'new', 'all',
    'access', 'constant', 'limited', 'aliased', 'abstract', 'tagged',
    'range', 'others', 'reverse', 'select', 'accept', 'delay', 'abort',
    'requeue', 'terminate', 'entry', 'protected', 'task', 'body',
    'generic', 'package', 'procedure', 'function', 'separate', 'private',
    'record', 'array', 'delta', 'digits', 'renames', 'exception',
    'overriding', 'interface', 'synchronized', 'some', 'parallel',
}

RE_ADA_SUBPROGRAM = re.compile(
    r'^[ \t]*(?:overriding\s+)?(?:not\s+overriding\s+)?'
    r'(procedure|function)\s+(\w+)', re.IGNORECASE | re.MULTILINE)
RE_ADA_PACKAGE = re.compile(
    r'^[ \t]*package\s+(?:body\s+)?(\w[\w.]*)\s+is\b', re.IGNORECASE | re.MULTILINE)
RE_ADA_IS_NOT_BODY = re.compile(r'\bis\s+(?:separate|abstract|new|null)\b', re.IGNORECASE)
RE_ADA_IS_EXPR_FUNC = re.compile(r'\bis\s*\(', re.IGNORECASE)
RE_ADA_IS_KEYWORD = re.compile(r'\bis\b', re.IGNORECASE)
RE_ADA_BEGIN = re.compile(r'\bbegin\b', re.IGNORECASE)
RE_ADA_END_STRUCTURED = re.compile(r'\bend\s+(?:if|loop|case|select|record)\b', re.IGNORECASE)
RE_ADA_END_BLOCK = re.compile(r'\bend\s*(?:[\w][\w.]*\s*)?;', re.IGNORECASE)
RE_ADA_CALL = re.compile(r'\b(\w[\w.]*)\s*\(', re.IGNORECASE)


def _strip_ada_line_comment(line):
    """Remove -- comment, respecting string literals."""
    in_string = False
    for i, ch in enumerate(line):
        if ch == '"':
            in_string = not in_string
        elif not in_string and line[i:i+2] == '--':
            return line[:i]
    return line


def _ada_call_context_snippet(scope_lines, start_idx, max_chars=300):
    """Collect multi-line Ada call text (paren-balanced), capped at max_chars chars."""
    parts = []
    depth = 0
    for i in range(start_idx, min(start_idx + 20, len(scope_lines))):
        stripped = _strip_ada_line_comment(scope_lines[i]).strip()
        parts.append(stripped)
        for ch in stripped:
            if ch == '(':
                depth += 1
            elif ch == ')':
                depth -= 1
        # Stop once we've seen the opening paren and depth returns to 0
        if i > start_idx and depth <= 0:
            break
        # Also stop at a semicolon if no parens found yet (simple call without args)
        if depth == 0 and ';' in stripped:
            break
    return ' '.join(parts)[:max_chars]


def _strip_ada_string_literals(line):
    """Replace string literal contents with underscores to avoid false keyword matches."""
    result = []
    in_string = False
    for ch in line:
        if ch == '"':
            in_string = not in_string
            result.append(ch)
        elif in_string:
            result.append('_')
        else:
            result.append(ch)
    return ''.join(result)


def _scan_ada_symbols(content, filename):
    """Extract symbols and calls from Ada source using regex + state machine."""
    raw_lines = content.split('\n')
    symbols = []
    calls = []

    # Phase 1: Extract package definitions
    # Track library-level (dotted) package for resolving nested package names.
    # In Ada, `package body Curtain.Profile is` is library-level (dotted name).
    # `package body FRA_List is` declared inside it is nested — its true qualified
    # name is Curtain.Profile.FRA_List.
    package_prefixes = []  # list of (line_num, qualified_pkg_name)
    current_library_pkg = ""  # most recent dotted (library-level) package
    for m in RE_ADA_PACKAGE.finditer(content):
        line_num = content[:m.start()].count('\n') + 1
        raw_name = m.group(1)
        if '.' in raw_name:
            # Library-level child package (e.g. "Curtain.Profile") — use as-is
            qualified_pkg = raw_name
            current_library_pkg = raw_name
        else:
            # Likely nested package — prefix with enclosing library package
            if current_library_pkg:
                qualified_pkg = f"{current_library_pkg}.{raw_name}"
            else:
                qualified_pkg = raw_name
        package_prefixes.append((line_num, qualified_pkg))
        symbols.append({
            'name': raw_name.split('.')[-1],
            'qualified_name': qualified_pkg,
            'kind': 'package',
            'filename': filename,
            'line_start': line_num,
            'line_end': line_num,
            'parent_name': current_library_pkg if '.' not in raw_name and current_library_pkg else None,
        })

    # Phase 2: Extract subprogram definitions
    for m in RE_ADA_SUBPROGRAM.finditer(content):
        kind = m.group(1).lower()
        name = m.group(2)
        line_num = content[:m.start()].count('\n') + 1
        # Find enclosing package for qualified_name
        pkg_prefix = ""
        for pkg_line, pkg_name in reversed(package_prefixes):
            if pkg_line < line_num:
                pkg_prefix = pkg_name
                break
        qualified = f"{pkg_prefix}.{name}" if pkg_prefix else name
        symbols.append({
            'name': name,
            'qualified_name': qualified,
            'kind': kind,
            'filename': filename,
            'line_start': line_num,
            'line_end': line_num,
            'parent_name': pkg_prefix or None,
        })

    # Phase 3a: Find subprogram bodies (multi-line aware state machine)
    # Walk lines looking for procedure/function, then accumulate until we find
    # ';' (declaration -> skip) or 'is' (body -> record).
    body_starts = []  # list of (name, kind, decl_line, is_line)
    n_lines = len(raw_lines)
    i = 0
    while i < n_lines:
        stripped = _strip_ada_string_literals(_strip_ada_line_comment(raw_lines[i]))
        m = RE_ADA_SUBPROGRAM.match(raw_lines[i])
        if not m:
            i += 1
            continue
        kind = m.group(1).lower()
        name = m.group(2)
        decl_line = i + 1  # 1-based

        # Accumulate lines until we resolve: ';' at paren depth 0 before 'is' = declaration,
        # 'is' at paren depth 0 = body start. Semicolons inside parameter lists are ignored.
        accum = stripped
        is_line = None
        j = i
        while j < n_lines:
            # Check if 'is' appears at paren depth 0
            paren_depth = 0
            found_is = False
            found_semi_at_zero = False
            pos = 0
            while pos < len(accum):
                ch = accum[pos]
                if ch == '(':
                    paren_depth += 1
                elif ch == ')':
                    paren_depth = max(0, paren_depth - 1)
                elif ch == ';' and paren_depth == 0:
                    found_semi_at_zero = True
                    break
                elif paren_depth == 0 and accum[pos:pos+2].lower() == 'is' and pos + 2 < len(accum):
                    # Check word boundary: char before must be non-word, char after must be non-word
                    before_ok = (pos == 0 or not accum[pos-1].isalnum() and accum[pos-1] != '_')
                    after_ok = not accum[pos+2].isalnum() and accum[pos+2] != '_'
                    if before_ok and after_ok:
                        found_is = True
                        break
                elif paren_depth == 0 and accum[pos:pos+2].lower() == 'is' and pos + 2 == len(accum):
                    before_ok = (pos == 0 or not accum[pos-1].isalnum() and accum[pos-1] != '_')
                    if before_ok:
                        found_is = True
                        break
                pos += 1

            if found_is:
                # Check if it's a non-body 'is' (separate, abstract, new, null)
                if RE_ADA_IS_NOT_BODY.search(accum):
                    break  # not a body
                # Check if it's an expression function: is (
                if RE_ADA_IS_EXPR_FUNC.search(accum):
                    break  # expression function, not a body with begin/end
                is_line = j + 1  # 1-based
                break

            if found_semi_at_zero:
                break  # declaration, not a body

            j += 1
            if j < n_lines:
                next_stripped = _strip_ada_string_literals(_strip_ada_line_comment(raw_lines[j]))
                accum += ' ' + next_stripped

        if is_line is not None:
            body_starts.append((name, kind, decl_line, is_line))

        i = j + 1 if j > i else i + 1

    # Phase 3b: Find scope end via begin/end depth counting.
    # Process innermost bodies first so we can skip their lines when processing outer bodies.
    body_starts_sorted = sorted(body_starts, key=lambda x: x[3], reverse=True)
    claimed_ranges = []  # list of (start_0based, end_0based) for resolved inner bodies
    scopes = []  # list of (name, decl_line, end_line)
    for name, kind, decl_line, is_line in body_starts_sorted:
        depth = 0
        found_begin = False
        end_line = n_lines  # fallback: end of file
        k = is_line - 1  # 0-based start (the is-line itself)
        while k < n_lines:
            # Skip lines claimed by already-resolved inner bodies
            skip_to = None
            for cs, ce in claimed_ranges:
                if cs <= k <= ce:
                    skip_to = ce + 1
                    break
            if skip_to is not None:
                k = skip_to
                continue

            stripped = _strip_ada_string_literals(_strip_ada_line_comment(raw_lines[k]))

            # Count 'begin' keywords
            for _ in RE_ADA_BEGIN.finditer(stripped):
                depth += 1
                found_begin = True

            # Count 'end ...;' but exclude structured ends (end if/loop/case/select/record)
            line_for_end = RE_ADA_END_STRUCTURED.sub('', stripped)
            for _ in RE_ADA_END_BLOCK.finditer(line_for_end):
                depth -= 1

            if found_begin and depth <= 0:
                end_line = k + 1  # 1-based
                break

            k += 1

        claimed_ranges.append((is_line - 1, end_line - 1))
        scopes.append((name, decl_line, end_line))

    # Phase 4: Update line_end for symbols that match a scope
    for sym in symbols:
        if sym['kind'] in ('procedure', 'function'):
            for sname, sstart, send in scopes:
                if sname.lower() == sym['name'].lower() and sstart == sym['line_start']:
                    sym['line_end'] = send
                    break

    # Phase 5: Extract calls within each scope
    # Build qualified scope name lookup from symbols
    scope_qualified = {}
    for sym in symbols:
        if sym['kind'] in ('procedure', 'function'):
            scope_qualified[(sym['name'].lower(), sym['line_start'])] = sym['qualified_name']

    for scope_name, scope_start, scope_end in scopes:
        qualified_scope = scope_qualified.get((scope_name.lower(), scope_start), scope_name)
        scope_lines = raw_lines[scope_start - 1:scope_end]
        for idx, line in enumerate(scope_lines):
            line = _strip_ada_line_comment(line)
            for cm in RE_ADA_CALL.finditer(line):
                callee = cm.group(1)
                callee_base = callee.split('.')[-1].lower()
                if callee_base in ADA_KEYWORDS:
                    continue
                if callee_base == scope_name.lower():
                    continue  # skip self-recursion noise
                calls.append({
                    'caller_name': qualified_scope,
                    'caller_file': filename,
                    'caller_line': scope_start + idx,
                    'callee_name': callee,
                    'call_context': _ada_call_context_snippet(scope_lines, idx),
                })

    return symbols, calls


