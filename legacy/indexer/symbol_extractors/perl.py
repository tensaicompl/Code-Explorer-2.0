"""Perl subroutines and packages.

Same approach as the Ada extractor and for the same reason: pattern matching
over the text, skipping anything ambiguous.
"""
import logging
import re

logger = logging.getLogger("indexer.symbols.perl")

def _scan_perl_symbols(content, filename):
    """Extract symbols and calls from Perl source using regex."""
    symbols = []
    calls = []

    re_sub = re.compile(r'^\s*sub\s+(\w+)', re.MULTILINE)
    re_call = re.compile(r'\b(\w[\w:]*)\s*\(')

    lines = content.split('\n')

    # Extract sub definitions with brace counting for line_end
    for m in re_sub.finditer(content):
        name = m.group(1)
        line_start = content[:m.start()].count('\n') + 1
        # Find line_end via brace counting
        brace_depth = 0
        line_end = line_start
        started = False
        for i in range(line_start - 1, len(lines)):
            for ch in lines[i]:
                if ch == '{':
                    brace_depth += 1
                    started = True
                elif ch == '}':
                    brace_depth -= 1
            if started and brace_depth <= 0:
                line_end = i + 1
                break
        else:
            line_end = len(lines)

        symbols.append({
            'name': name,
            'qualified_name': name,
            'kind': 'function',
            'filename': filename,
            'line_start': line_start,
            'line_end': line_end,
            'parent_name': None,
        })

        # Extract calls within this sub
        for li in range(line_start - 1, line_end):
            if li >= len(lines):
                break
            line = lines[li]
            # Skip comments
            comment_pos = line.find('#')
            if comment_pos >= 0:
                line = line[:comment_pos]
            for cm in re_call.finditer(line):
                callee = cm.group(1)
                if callee == name:
                    continue
                calls.append({
                    'caller_name': name,
                    'caller_file': filename,
                    'caller_line': li + 1,
                    'callee_name': callee,
                    'call_context': line.strip()[:120],
                })

    return symbols, calls


