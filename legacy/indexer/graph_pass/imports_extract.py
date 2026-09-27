"""
Imports and inheritance -> structural edges —

DO THIS EARLY despite being Tier 2. Imports are what upgrade the resolver from
`scoped` to `import-guided`, and layer scoping was measured at 4.4% effectiveness — so this is the ONLY lever that materially improves edge precision.
It is also the edge family a layer view is drawn from, and `detailLevel:"class"`
in the dashboard walks `contains` edges (GraphView.tsx:463-483), so without them
that mode is inert.

GREENFIELD HERE. Nothing in the indexer references import/require/extends/
implements/superclass/with_clause node types.

EXACT INSERTION POINTS — a 6-touch-point signature change, not "just add a function":
  1. Add _TS_IMPORT_TYPES alongside _TS_DEF_TYPES (build_corpus.py:606-619)
     and _TS_CALL_TYPES (build_corpus.py:621-634).
  2. Add a third branch inside walk() (build_corpus.py:719-756), which today checks only
     `node.type in def_types` (:720) and `in call_types` (:740).
  3. Thread a third return value through _scan_treesitter_symbols and EVERY
     call site.
DO IT AS ONE UNIT. Splitting across agents produces conflicting edits to the same
function chain.

PER-LANGUAGE IMPORT NODE TYPES
    python      import_statement, import_from_statement
    java        import_declaration
    js/ts/tsx   import_statement
    kotlin      import_header
    c/cpp       preproc_include
    scala       import_declaration
    ada         `with` clauses (regex, alongside the existing Ada regexes)

ALSO ADD HERE, same edit: C/C++ class_specifier + struct_specifier to
_TS_DEF_TYPES. Today c/cpp capture ONLY function_definition, so C++ classes are
absent from the index entirely — a direct hit on the "classes" requirement
for C++-heavy codebases.

INHERITANCE IS GREENFIELD IN BOTH REPOS. EdgeType declares
"inherits" | "implements", but StructuralAnalysis["classes"] has exactly four
fields — name, lineRange, methods, properties — with no heritage field, and no earlier
extractor reads class_heritage even though tree-sitter exposes it. Small new work,
because the AST node is right there and unread.

PRIOR ART: the earlier viewer populates StructuralAnalysis.imports as
{source, specifiers[], lineNumber} in all 13 extractors — see
typescript-extractor.ts:359-383 + extractImportSpecifiers :66-98. Real, deterministic,
worth porting. Known limit: resolveImports (tree-sitter-plugin.ts:252-275) resolves
only relative specifiers; no extension probing, no index.ts, no tsconfig aliases.
"""
