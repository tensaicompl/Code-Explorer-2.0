"""Symbols, calls, imports and inheritance from a real parse tree.

Everything with a tree-sitter grammar goes through here, which is most of the
languages this indexes. Working from a parse tree rather than patterns is what
makes the call graph trustworthy: a call inside a comment or a string is not a
call, and a parser knows the difference.

Grammars are loaded lazily per language and a missing one is tolerated -- the
file simply yields no symbols rather than failing the run.
"""
import logging
import re

logger = logging.getLogger("indexer.symbols.treesitter")

TS_LANGUAGE_MAP = {
    '.py': 'python',
    '.c': 'c', '.h': 'c', '.cpp': 'cpp', '.hpp': 'cpp',
    '.hh': 'cpp', '.cc': 'cpp',
    '.rb': 'ruby',
    '.java': 'java', '.kt': 'kotlin',
    '.js': 'javascript', '.jsx': 'javascript',
    '.ts': 'typescript', '.tsx': 'tsx',
    '.sh': 'bash', '.bash': 'bash', '.ksh': 'bash',
    '.scala': 'scala',
    '.rs': 'rust',
}

# Cache belongs to the loader module, not to the package importing it.
_TS_PARSERS = {}

def _load_treesitter_parser(ext):
    """Get a cached tree-sitter parser for the given extension, or None."""
    lang_name = TS_LANGUAGE_MAP.get(ext)
    if not lang_name:
        return None, None

    if lang_name in _TS_PARSERS:
        return _TS_PARSERS[lang_name]

    try:
        import importlib
        import tree_sitter

        # The tree_sitter_typescript package exposes no plain language() — it ships
        # language_typescript() and language_tsx() instead. Upstream special-cased
        # only 'tsx', so '.ts' fell through to mod.language(), raised AttributeError,
        # and was swallowed by the handler below at debug level — silently yielding
        # ZERO symbols for every .ts file. Verified: 207/244 TS files produced no
        # symbols. See
        if lang_name == 'tsx':
            mod = importlib.import_module('tree_sitter_typescript')
            language = tree_sitter.Language(mod.language_tsx())
        elif lang_name == 'typescript':
            mod = importlib.import_module('tree_sitter_typescript')
            language = tree_sitter.Language(mod.language_typescript())
        else:
            module_name = f"tree_sitter_{lang_name}"
            mod = importlib.import_module(module_name)
            language = tree_sitter.Language(mod.language())

        parser = tree_sitter.Parser(language)
        _TS_PARSERS[lang_name] = (parser, language)
        return parser, language
    except Exception as e:
        # WARNING, not debug. A grammar that fails to load disables symbol
        # extraction for an entire language, and at debug level that is invisible.
        # This log level is the root cause of the .ts bug going unnoticed.
        logger.warning(
            "tree-sitter grammar unavailable for %s — ALL symbol extraction for this "
            "language is disabled: %s: %s", lang_name, type(e).__name__, e
        )
        _TS_PARSERS[lang_name] = (None, None)
        return None, None


# Definition node types per language
_TS_DEF_TYPES = {
    'java': {'method_declaration', 'class_declaration', 'interface_declaration', 'constructor_declaration'},
    'python': {'function_definition', 'class_definition'},
    'c': {'function_definition', 'struct_specifier'},
    # C++ captured ONLY function_definition, so every class in a C++ codebase
    # was absent from the index — a direct hit on the "classes" requirement.
    'cpp': {'function_definition', 'class_specifier', 'struct_specifier'},
    'ruby': {'method', 'singleton_method', 'class', 'module'},
    'javascript': {'function_declaration', 'method_definition', 'class_declaration'},
    'typescript': {'function_declaration', 'method_definition', 'class_declaration'},
    'tsx': {'function_declaration', 'method_definition', 'class_declaration'},
    'bash': {'function_definition'},
    'kotlin': {'function_declaration', 'class_declaration'},
    'scala': {'function_definition', 'function_declaration', 'class_definition', 'object_definition', 'trait_definition'},
    # Rust has no classes; `struct`/`enum`/`trait` are the type-level things a
    # reader navigates by, and `impl` blocks are where the methods live.
    'rust': {'function_item', 'struct_item', 'enum_item', 'trait_item', 'impl_item', 'mod_item'},
}

# Call node types per language
_TS_CALL_TYPES = {
    'java': {'method_invocation'},
    'python': {'call'},
    'c': {'call_expression'},
    'cpp': {'call_expression'},
    'ruby': {'call'},
    'javascript': {'call_expression'},
    'typescript': {'call_expression'},
    'tsx': {'call_expression'},
    'bash': {'command_name'},
    'kotlin': {'call_expression'},
    'scala': {'call_expression'},
    # `macro_invocation` matters in Rust in a way it does not elsewhere: a great
    # deal of real logic goes through macros, and dropping them would leave the
    # call graph of an idiomatic crate looking half-empty.
    'rust': {'call_expression', 'macro_invocation'},
}


# Dotted or bare identifiers inside an import / heritage clause. Deliberately
# permissive: the resolver treats these as hints, and a false hint costs a
# missed upgrade, never a wrong edge.
_IMPORT_TOKEN_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_.]*")

_HERITAGE_NOISE = {
    'extends', 'implements', 'public', 'private', 'protected', 'virtual',
    'class', 'interface', 'object', 'with', 'super', 'from', 'import',
    'include', 'package', 'as', 'static', 'final', 'abstract', 'open',
    # Rust
    'use', 'crate', 'self', 'pub', 'mod', 'impl', 'dyn', 'where',
}

# Import node types per language —
#
# Imports are what let the resolver move a call from `ambiguous` to
# `import-guided`. Measured on real data, layer scoping rescued 4.4 % of
# candidates; knowing which files a file can even see is the only lever that
# materially improves precision, and precision is what decides whether an edge
# is drawn at all.
_TS_IMPORT_TYPES = {
    'python': {'import_statement', 'import_from_statement'},
    'java': {'import_declaration'},
    'javascript': {'import_statement'},
    'typescript': {'import_statement'},
    'tsx': {'import_statement'},
    'kotlin': {'import_header'},
    'c': {'preproc_include'},
    'cpp': {'preproc_include'},
    'scala': {'import_declaration'},
    'rust': {'use_declaration'},
}

# Nodes carrying a superclass / implemented interface. Greenfield in both repos:
# `EdgeType` declares "inherits"/"implements" and nothing ever produced one.
_TS_HERITAGE_TYPES = {
    'java': {'superclass', 'super_interfaces'},
    'python': {'argument_list'},          # class C(Base) — the base list
    'javascript': {'class_heritage'},
    'typescript': {'class_heritage'},
    'tsx': {'class_heritage'},
    'kotlin': {'delegation_specifier'},
    'cpp': {'base_class_clause'},
    'scala': {'extends_clause'},
    # `impl Trait for Type` is the closest Rust gets to inheritance.
    'rust': {'trait_bounds', 'where_clause'},
}


# Documentation comments, per language. The author already wrote the summary;
# throwing it away and paying a model to guess a worse one would be perverse.
_DOC_PREFIXES = ("///", "//!", "*", "/**", "#'", "///<")

_DOC_STRIP = re.compile(r'^\s*(///<|///|//!|/\*\*|\*/|\*|#)\s?')


def _doc_for_symbol(lines: list, line_start: int, lang_name: str) -> str:
    """
    First sentence of the doc comment attached to a definition, or ''.

    Two shapes, because languages disagree about which side the docs go:
      - ABOVE the definition — Rust `///`/`//!`, Java/TS `/** */`, shell `#`.
      - BELOW it — Python's docstring, which is the first statement in the body.

    Deliberately conservative. A wrong summary is worse than no summary: the
    node name is always a truthful fallback, so anything ambiguous returns ''.
    """
    idx = line_start - 1          # line_start is 1-based
    if idx < 0 or idx >= len(lines):
        return ""

    # --- Python-style: docstring on the line(s) after the definition ---------
    if lang_name in ("python", "ruby"):
        for probe in (idx + 1, idx + 2):   # allow a multi-line signature
            if probe >= len(lines):
                break
            t = lines[probe].strip()
            for q in ('"""', "'''"):
                if t.startswith(q):
                    body = t[3:]
                    if body.endswith(q) and len(body) > 3:
                        return _first_sentence(body[:-3])
                    # multi-line: take the first non-empty line of the body
                    if body.strip():
                        return _first_sentence(body)
                    if probe + 1 < len(lines):
                        return _first_sentence(lines[probe + 1])
                    return ""
            if t and not t.startswith(("@", ")")):
                break              # real code — there is no docstring
        return ""

    # --- Everything else: comment block immediately above -------------------
    collected = []
    i = idx - 1
    # Skip attributes/annotations that sit between the doc and the definition:
    # Rust `#[derive(...)]`, Java `@Override`, TS decorators.
    while i >= 0 and lines[i].strip().startswith(("#[", "@", "]")):
        i -= 1
    while i >= 0:
        t = lines[i].strip()
        if not t:
            break
        if t.startswith(_DOC_PREFIXES) or t.endswith("*/"):
            collected.append(t)
            i -= 1
            continue
        break
    if not collected:
        return ""
    collected.reverse()
    text = " ".join(_DOC_STRIP.sub("", c).strip() for c in collected)
    text = text.replace("*/", "").strip()
    return _first_sentence(text)


def _first_sentence(text: str) -> str:
    """One sentence, capped. Long enough to be useful, short enough for a card."""
    text = " ".join(text.split())
    if not text:
        return ""
    # Stop at the first sentence end, but only if something precedes it.
    for end in (". ", "! ", "? "):
        pos = text.find(end)
        if pos > 10:
            text = text[: pos + 1]
            break
    if text.endswith("."):
        text = text
    return text[:200].strip()


def _treesitter_symbol_name(node, lang_name):
    """Extract the name from a tree-sitter node."""
    # For Java method_invocation, look for .name child
    if lang_name == 'java' and node.type == 'method_invocation':
        for child in node.children:
            if child.type == 'identifier' and child.prev_sibling and child.prev_sibling.type == '.':
                return child.text.decode('utf-8', errors='replace')
            if child.type == 'identifier':
                name = child.text.decode('utf-8', errors='replace')
        # Return last identifier found
        for child in reversed(node.children):
            if child.type == 'identifier':
                return child.text.decode('utf-8', errors='replace')

    # For Python call, get the function part
    if lang_name == 'python' and node.type == 'call':
        func = node.child_by_field_name('function')
        if func:
            if func.type == 'identifier':
                return func.text.decode('utf-8', errors='replace')
            if func.type == 'attribute':
                attr = func.child_by_field_name('attribute')
                if attr:
                    return attr.text.decode('utf-8', errors='replace')

    # For Ruby call nodes, callee name is in field 'method', not 'name'
    if lang_name == 'ruby' and node.type == 'call':
        method_node = node.child_by_field_name('method')
        if method_node:
            return method_node.text.decode('utf-8', errors='replace')

    # For Scala call_expression, the callee is in the 'function' field
    if lang_name == 'scala' and node.type == 'call_expression':
        func = node.child_by_field_name('function')
        if func:
            if func.type == 'identifier':
                return func.text.decode('utf-8', errors='replace')
            if func.type == 'field_expression':
                field = func.child_by_field_name('field')
                if field:
                    return field.text.decode('utf-8', errors='replace')

    # Generic: look for first identifier child or name field
    name_node = node.child_by_field_name('name')
    if name_node:
        return name_node.text.decode('utf-8', errors='replace')

    for child in node.children:
        if child.type == 'identifier':
            return child.text.decode('utf-8', errors='replace')

    return None


def _scan_treesitter_symbols(content, ext, filename):
    """Extract symbols and calls using tree-sitter."""
    parser, language = _load_treesitter_parser(ext)
    if parser is None:
        return [], [], []

    lang_name = TS_LANGUAGE_MAP[ext]
    def_types = _TS_DEF_TYPES.get(lang_name, set())
    call_types = _TS_CALL_TYPES.get(lang_name, set())
    import_types = _TS_IMPORT_TYPES.get(lang_name, set())
    heritage_types = _TS_HERITAGE_TYPES.get(lang_name, set())

    try:
        tree = parser.parse(content.encode('utf-8'))
    except Exception:
        return [], [], []

    symbols = []
    calls = []
    imports = []
    content_lines = content.split('\n')

    def _kind_from_type(node_type):
        # Rust names its type-level constructs `*_item`, none of which contain
        # "class" or "interface", so they were all landing as `function` — a
        # struct rendered as a function is simply wrong on the graph.
        if node_type in ('struct_item', 'enum_item', 'trait_item', 'impl_item'):
            return 'class'
        if node_type == 'mod_item':
            return 'module'
        if 'class' in node_type or 'interface' in node_type or node_type == 'module':
            return 'class'
        if 'constructor' in node_type:
            return 'constructor'
        if 'method' in node_type:
            return 'method'
        return 'function'

    def walk(node, scope_stack):
        if node.type in def_types:
            name = _treesitter_symbol_name(node, lang_name)
            if name:
                parent = scope_stack[-1] if scope_stack else None
                qualified = f"{parent}.{name}" if parent else name
                symbols.append({
                    'name': name,
                    'qualified_name': qualified,
                    'kind': _kind_from_type(node.type),
                    'filename': filename,
                    'line_start': node.start_point[0] + 1,
                    'line_end': node.end_point[0] + 1,
                    'parent_name': parent,
                    'doc': _doc_for_symbol(
                        content_lines, node.start_point[0] + 1, lang_name),
                })
                scope_stack.append(name)
                for child in node.children:
                    walk(child, scope_stack)
                scope_stack.pop()
                return

        if node.type in import_types:
            # The raw text is kept rather than a parsed specifier: every
            # language spells this differently, and the resolver only needs to
            # know which names a file can see, not the grammar it saw them in.
            raw = node.text.decode('utf-8', errors='replace').strip()
            for tok in _IMPORT_TOKEN_RE.findall(raw):
                # The regex sees the statement keyword too (`import`, `from`,
                # `#include`), which is grammar, not a module name.
                if tok in _HERITAGE_NOISE:
                    continue
                imports.append({
                    'filename': filename,
                    'module': tok,
                    'line': node.start_point[0] + 1,
                    'kind': 'import',
                })

        # The parent must be a DEFINITION. Python spells a base-class list
        # `argument_list`, which is also the node type for the arguments of
        # every function call — without this guard, awp produced 13,651
        # "inheritance" rows that were really call arguments (`load_repo_config
        # <- repos.json`). Java and JS are unaffected but the rule is the same
        # for all of them, so there is one thing to reason about.
        if (node.type in heritage_types and scope_stack
                and node.parent is not None and node.parent.type in def_types):
            raw = node.text.decode('utf-8', errors='replace').strip()
            for tok in _IMPORT_TOKEN_RE.findall(raw):
                # `extends`/`implements`/`public` etc. are grammar, not names.
                if tok in _HERITAGE_NOISE:
                    continue
                imports.append({
                    'filename': filename,
                    'module': tok,
                    'line': node.start_point[0] + 1,
                    'kind': 'inherits',
                    'owner': scope_stack[-1],
                })

        if node.type in call_types:
            callee = _treesitter_symbol_name(node, lang_name)
            if callee:
                caller = '.'.join(scope_stack) if scope_stack else '<module>'
                call_line_idx = node.start_point[0]
                call_context_line = node.text.decode('utf-8', errors='replace').strip()[:300]
                calls.append({
                    'caller_name': caller,
                    'caller_file': filename,
                    'caller_line': call_line_idx + 1,
                    'callee_name': callee,
                    'call_context': call_context_line,
                })

        for child in node.children:
            walk(child, scope_stack)

    walk(tree.root_node, [])
    return symbols, calls, imports

