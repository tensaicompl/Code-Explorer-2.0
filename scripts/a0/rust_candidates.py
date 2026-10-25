#!/usr/bin/env python3
"""Issue 105 (R5): the Rust oracle call relations that end at a structural site without
a drawn edge, classified by the structural evidence that could resolve them.

    rust_candidates.py --corpus NAME --checkout DIR --audit DIR --rust-analyzer BIN
                       --scip FILE[=PREFIX] ... [--typed-rows FILE] --out FILE
    rust_candidates.py artifact --state LABEL=FILE ... --summary FILE --relations FILE

The second form writes the committed form of one or more classifications (the
`--out` documents of the first), each labelled by the engine it measured: their
populations and summaries, and every relation as one line of its state.

Reads A0's own rows for the corpus (`audit.py`'s `<corpus>.sites.jsonl` and
`.relations.jsonl` in `--audit`), the language's parser (rust-analyzer's syntax tree)
for the shape of each call and its receiver, the oracle's index for what each receiver
really is (its signature: `let x: T`, `x: T`, `fn f() -> T`), and, when given, the
engine's typed-pass rows (`engine/tests/differential_dump`'s `typed` rows, with the
repository's metadata) for why the typed pass gave no answer.

A relation can carry several classes. Its primary class is the first that applies, in
the order of CLASSES: path identity before receiver types, and among receivers the one
the call's syntax shows. Nothing here changes a resolution or a measure: it is a
diagnostic of the dataset A0 measured.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import audit  # noqa: E402
import report  # noqa: E402
import scip  # noqa: E402
import syntax  # noqa: E402

FORMAT = 1
DRAWN = ("typed", "import-guided", "inheritance-guided")

# The classes, in the order a relation's primary one is chosen.
CLASSES = (
    "crate_self_super_path",
    "re_export",
    "import_use_binding",
    "associated_static_path_type",
    "receiver_local_variable_type",
    "receiver_parameter_type",
    "receiver_field_type",
    "receiver_call_result_type",
    "receiver_self",
    "trait_bound",
    "generic_type_parameter",
    "impl_relation",
    "deref_wrapper_receiver",
    "external_type",
    "macro_generated",
    "unknown_other",
)

WRAPPERS = {"Box", "Arc", "Rc", "Pin", "RefCell", "Mutex", "RwLock", "Cow", "MutexGuard",
            "Ref", "RefMut", "ManuallyDrop"}
TRANSPARENT = {"PAREN_EXPR", "REF_EXPR", "AWAIT_EXPR", "TRY_EXPR", "PREFIX_EXPR"}


# --- syntax helpers ------------------------------------------------------------------


def significant(node):
    return [c for c in node.children if c.kind not in syntax.TRIVIA]


def text(data, node):
    return data[node.start:node.end].decode("utf-8", "replace")


def index_tree(root):
    """Parent links, and every node by kind, of one file's tree."""
    parent = {}
    stack = [root]
    while stack:
        n = stack.pop()
        for c in n.children:
            parent[id(c)] = n
            stack.append(c)
    return parent


def find_call(root, token):
    """The METHOD_CALL_EXPR or CALL_EXPR whose name token is `token`, innermost."""
    best = None
    stack = [root]
    while stack:
        n = stack.pop()
        if n.start > token[0] or n.end < token[1]:
            continue
        if n.kind == "METHOD_CALL_EXPR":
            ref = next((c for c in n.children if c.kind == "NAME_REF"), None)
            if ref and syntax._name_ref_token(ref) == token:
                best = n
        elif n.kind == "CALL_EXPR":
            sig = significant(n)
            if sig and sig[0].kind == "PATH_EXPR":
                path = next((c for c in sig[0].children if c.kind == "PATH"), None)
                if path and syntax._last_segment_token(path) == token:
                    best = n
        stack.extend(n.children)
    return best


def unwrap(node):
    """An expression with its parentheses, references, `.await`, `?` and unary
    operators taken off, and the wrappers met."""
    met = []
    while node is not None and node.kind in TRANSPARENT:
        met.append(node.kind)
        inner = [c for c in significant(node) if c.kind.endswith("_EXPR") or c.kind in (
            "PATH_EXPR", "LITERAL", "MACRO_EXPR")]
        node = inner[0] if inner else None
    return node, met


def pattern_names(data, pat):
    """The names a pattern binds, with each name's IDENT_PAT node."""
    out = []
    stack = [pat]
    while stack:
        n = stack.pop()
        if n.kind == "IDENT_PAT":
            name = next((c for c in n.children if c.kind == "NAME"), None)
            if name:
                ident = next((t for t in name.children if t.kind == "IDENT"), None)
                if ident:
                    out.append((data[ident.start:ident.end].decode("utf-8", "replace"), n))
        stack.extend(n.children)
    return out


def path_segments(data, path):
    """The segments of a PATH node, outermost first, each with its name text."""
    segs = []
    node = path
    while node is not None and node.kind == "PATH":
        seg = next((c for c in node.children if c.kind == "PATH_SEGMENT"), None)
        inner = next((c for c in node.children if c.kind == "PATH"), None)
        if seg is not None:
            segs.append(seg)
        node = inner
    segs.reverse()
    names = []
    for seg in segs:
        ref = next((c for c in seg.children if c.kind == "NAME_REF"), None)
        if ref is not None:
            tok = syntax._name_ref_token(ref)
            names.append(data[tok[0]:tok[1]].decode("utf-8", "replace") if tok else "?")
        elif any(c.kind in ("PATH_TYPE", "TYPE_ANCHOR") for c in seg.children) or \
                any(c.kind == "L_ANGLE" for c in seg.children):
            names.append("<qualified>")
        else:
            names.append(text(data, seg))
    return names


class FileSyntax:
    """One file's tree and the facts the classification reads from it."""

    def __init__(self, data, dump):
        self.data = data
        self.root = syntax.parse_tree(dump)
        self.parent = index_tree(self.root)
        self.uses = self._uses()

    def ancestors(self, node):
        n = self.parent.get(id(node))
        while n is not None:
            yield n
            n = self.parent.get(id(n))

    def _uses(self):
        """Names the file's `use` items bind (anywhere in it), with their paths, and
        whether any `use` is a glob."""
        names = {}
        globs = []
        stack = [self.root]
        while stack:
            n = stack.pop()
            if n.kind == "USE":
                tree = next((c for c in n.children if c.kind == "USE_TREE"), None)
                if tree is not None:
                    self._use_tree(tree, [], names, globs)
                continue
            stack.extend(n.children)
        return {"names": names, "globs": globs}

    def _use_tree(self, tree, prefix, names, globs):
        path = next((c for c in tree.children if c.kind == "PATH"), None)
        segs = prefix + (path_segments(self.data, path) if path is not None else [])
        lst = next((c for c in tree.children if c.kind == "USE_TREE_LIST"), None)
        if lst is not None:
            for c in lst.children:
                if c.kind == "USE_TREE":
                    self._use_tree(c, segs, names, globs)
            return
        if any(c.kind == "STAR" for c in tree.children):
            globs.append(segs)
            return
        rename = next((c for c in tree.children if c.kind == "RENAME"), None)
        local = segs[-1] if segs else None
        if rename is not None:
            nm = next((c for c in rename.children if c.kind in ("NAME", "UNDERSCORE")), None)
            local = text(self.data, nm) if nm is not None else local
        if local == "self" and len(segs) >= 2:
            local = segs[-2]
            segs = segs[:-1]
        if local and local != "_":
            names.setdefault(local, segs)

    def binding(self, call, name):
        """How `name`, read at `call`, is bound: ('param', PARAM), ('let', LET_STMT,
        IDENT_PAT), ('pattern', kind) or None. The nearest binding before the call wins."""
        pos = call.start
        for anc in self.ancestors(call):
            if anc.kind in ("STMT_LIST", "BLOCK_EXPR"):
                best = None
                for st in significant(anc):
                    if st.start >= pos:
                        break
                    if st.kind == "LET_STMT":
                        pat = next((c for c in significant(st) if c.kind.endswith("_PAT")), None)
                        if pat is not None:
                            for nm, ip in pattern_names(self.data, pat):
                                if nm == name:
                                    best = ("let", st, ip)
                if best:
                    return best
            elif anc.kind == "MATCH_ARM":
                pat = next((c for c in significant(anc) if c.kind.endswith("_PAT")), None)
                if pat is not None and any(nm == name for nm, _ in pattern_names(self.data, pat)):
                    return ("pattern", "match_arm")
            elif anc.kind == "CLOSURE_EXPR":
                params = next((c for c in anc.children if c.kind == "PARAM_LIST"), None)
                if params is not None:
                    for p in params.children:
                        if p.kind == "PARAM" and any(nm == name for nm, _ in pattern_names(self.data, p)):
                            return ("closure_param", p)
            elif anc.kind in ("IF_EXPR", "WHILE_EXPR"):
                for cond in anc.children:
                    if cond.kind == "LET_EXPR" and cond.end <= pos and any(
                            nm == name for nm, _ in pattern_names(self.data, cond)):
                        return ("pattern", "if_let")
            elif anc.kind == "FOR_EXPR":
                pat = next((c for c in significant(anc) if c.kind.endswith("_PAT")), None)
                if pat is not None and pat.end <= pos and any(
                        nm == name for nm, _ in pattern_names(self.data, pat)):
                    return ("pattern", "for")
            elif anc.kind == "FN":
                params = next((c for c in anc.children if c.kind == "PARAM_LIST"), None)
                if params is not None:
                    for p in params.children:
                        if p.kind == "PARAM" and any(nm == name for nm, _ in pattern_names(self.data, p)):
                            return ("param", p)
                return None
        return None

    def generic_names(self, node):
        """The generic type parameters in force at `node` (its functions and impls)."""
        out = set()
        for anc in self.ancestors(node):
            if anc.kind in ("FN", "IMPL", "TRAIT", "STRUCT", "ENUM"):
                gl = next((c for c in anc.children if c.kind == "GENERIC_PARAM_LIST"), None)
                if gl is not None:
                    for p in gl.children:
                        if p.kind == "TYPE_PARAM":
                            nm = next((c for c in p.children if c.kind == "NAME"), None)
                            if nm is not None:
                                out.add(text(self.data, nm))
        return out

    def in_macro(self, node):
        return any(a.kind in ("MACRO_CALL", "MACRO_EXPR", "TOKEN_TREE") for a in
                   self.ancestors(node))


# --- types as the oracle signs them -------------------------------------------------


def base_type(sig_type):
    """The type a signature's type text names once references, `mut`, lifetimes and
    generic arguments are taken off, and the wrappers it is inside."""
    t = (sig_type or "").strip()
    wrappers = []
    while True:
        t = re.sub(r"^&(?:'\w+\s+)?(?:mut\s+)?", "", t).strip()
        m = re.match(r"^((?:[A-Za-z_][\w]*::)*)([A-Za-z_]\w*)\s*<(.*)>$", t)
        if m and m.group(2) in WRAPPERS:
            wrappers.append(m.group(2))
            inner = m.group(3)
            depth = 0
            for i, ch in enumerate(inner):
                if ch == "<":
                    depth += 1
                elif ch == ">":
                    depth -= 1
                elif ch == "," and depth == 0:
                    inner = inner[:i]
                    break
            t = inner.strip()
            continue
        break
    t = re.sub(r"<.*$", "", t).strip()
    t = t.split("::")[-1]
    return t, wrappers


def signature_type(signature):
    """The declared type in a local's, parameter's or field's signature."""
    if not signature:
        return None
    sig = signature.strip()
    m = re.match(r"^(?:let\s+)?(?:mut\s+)?(?:pub(?:\([^)]*\))?\s+)?[\w]+\s*:\s*(.+)$", sig,
                 re.S)
    return m.group(1).strip() if m else None


def return_type(signature):
    if not signature:
        return None
    m = re.search(r"->\s*(.+?)\s*(?:where\b.*)?$", signature, re.S)
    return m.group(1).strip() if m else None


TARGET_FORMS = (
    ("trait_impl_method", re.compile(r"impl#\[(?:`([^`]*)`|([^\]]*))\]\[(?:`([^`]*)`|([^\]]*))\]"
                                     r"[^/]*\(\)\.$")),
    ("inherent_method", re.compile(r"impl#\[(?:`([^`]*)`|([^\]]*))\][^\[/]*\(\)\.$")),
    ("trait_method", re.compile(r"([A-Za-z_]\w*)#[A-Za-z_]\w*\(\)\.$")),
    ("type", re.compile(r"([A-Za-z_]\w*)#$")),
    ("free_function", re.compile(r"[A-Za-z_]\w*\(\)\.$")),
)


def target_form(symbol):
    """What the oracle's target is: its form and, for a method, its self type and
    trait as the symbol spells them."""
    desc = (symbol or "").split(" ", 4)[-1]
    for form, rx in TARGET_FORMS:
        m = rx.search(desc)
        if not m:
            continue
        groups = [g for g in m.groups()]
        if form == "trait_impl_method":
            return form, groups[0] or groups[1], groups[2] or groups[3]
        if form == "inherent_method":
            return form, groups[0] or groups[1], None
        if form == "trait_method":
            return form, None, groups[0]
        if form == "type":
            return form, groups[0], None
        return form, None, None
    return "other", None, None


# --- classification ------------------------------------------------------------------


class Classifier:
    def __init__(self, args):
        self.args = args
        self.root = args.checkout
        self.sources = audit.Sources(self.root)
        self.oracle = audit.Oracle()
        self.signatures = {}
        for spec in args.scip:
            path, _, prefix = spec.partition("=")
            index = scip.read_index(Path(path).read_bytes())
            self.oracle.load(index, prefix, self.sources, scip.ENCODING_UTF8, "rust")
            for doc in index.documents:
                for s in doc.symbols:
                    if s.signature:
                        self.signatures.setdefault(s.symbol, s.signature)
            for s in index.external_symbols:
                if s.signature:
                    self.signatures.setdefault(s.symbol, s.signature)
        env = dict(os.environ)
        self.parse = syntax.rust_analyzer_parser(args.rust_analyzer, env)
        self.files = {}
        self.typed = defaultdict(list)
        if args.typed_rows:
            with open(args.typed_rows) as f:
                for line in f:
                    r = json.loads(line)
                    if r.get("c") == "typed":
                        self.typed[(r["file"], r["start"], r["end"])].append(r)
        self.defs = {}
        self.type_names = set()

    def file(self, path):
        if path not in self.files:
            data = self.sources.get(path)
            self.files[path] = FileSyntax(data, self.parse(data)) if data is not None else None
        return self.files[path]

    def symbol_at(self, path, token):
        refs = self.oracle.occurrences.get(path, {}).get(tuple(token), [])
        return refs[0][0] if refs else None

    def signature_of_name(self, path, node_or_token):
        token = node_or_token if isinstance(node_or_token, tuple) else (
            node_or_token.start, node_or_token.end)
        sym = self.symbol_at(path, token)
        return self.signatures.get(sym) if sym else None

    def typed_outcome(self, site):
        span = site.get("span")
        if not span:
            return "no_span"
        rows = self.typed.get((site["file"], span[0], span[1]), [])
        if not rows:
            return "no_typed_row" if self.typed else None
        out = sorted({r["strategy"] + (":" + (r.get("reason") or "")
                                        if r["strategy"] == "lsp_unresolved" else "")
                      for r in rows})
        return ",".join(out)

    def classify(self, rel, site):
        fs = self.file(site["file"])
        token = tuple(rel["token"])
        form, self_type, trait = target_form(site.get("oracle_symbol"))
        target_base = base_type(self_type)[0] if self_type else None
        rec = {
            "file": site["file"],
            "line": site["line"],
            "token": list(token),
            "callee": site["callee"],
            "band": site["band"],
            "candidates": site.get("candidate_count"),
            "weak_band": site["band"] if site["band"] in ("exact", "scoped") else None,
            "weak_candidate_correct": (site.get("verdict") == "correct")
            if site["band"] in ("exact", "scoped") else None,
            "typed_outcome": self.typed_outcome(site),
            "oracle_target": site.get("oracle_target_qn"),
            "oracle_target_in_registry": site.get("oracle_target") is not None,
            "target_form": form,
            "target_self_type": self_type,
            "target_trait": trait,
            "target_type_represented": bool(target_base and target_base in self.type_names),
        }
        classes = []
        evidence = {}
        call = find_call(fs.root, token) if fs else None
        if call is None:
            rec.update(shape="not_found", classes=["unknown_other"], primary="unknown_other",
                       evidence={}, recoverable="unknown")
            return rec
        if fs.in_macro(call):
            classes.append("macro_generated")
            evidence["macro"] = "inside a macro invocation's arguments"
        generics = fs.generic_names(call)
        data = fs.data
        if call.kind == "CALL_EXPR":
            rec["shape"] = "path_call"
            callee = significant(call)[0]
            path = next((c for c in callee.children if c.kind == "PATH"), None)
            segs = path_segments(data, path) if path is not None else []
            evidence["path"] = "::".join(segs)
            head = segs[0] if segs else None
            if head in ("crate", "self", "super"):
                classes.append("crate_self_super_path")
            imported = fs.uses["names"].get(head) if head else None
            if imported:
                classes.append("import_use_binding")
                evidence["import"] = "::".join(imported)
                if imported[0] in ("crate", "self", "super"):
                    classes.append("crate_self_super_path")
            elif head and len(segs) >= 1 and fs.uses["globs"] and head not in generics:
                if head != "Self" and head not in ("crate", "self", "super"):
                    evidence["glob_imports"] = ["::".join(g) for g in fs.uses["globs"]]
                    classes.append("import_use_binding")
            if len(segs) >= 2:
                qualifier = segs[-2]
                if qualifier == "Self" or qualifier[:1].isupper() or qualifier == "<qualified>":
                    classes.append("associated_static_path_type")
                if qualifier in generics or (segs[0] in generics):
                    classes.append("generic_type_parameter")
                    if form in ("trait_method", "trait_impl_method"):
                        classes.append("trait_bound")
            if self._re_exported(rec, segs, imported):
                classes.append("re_export")
            if form == "trait_impl_method" and "associated_static_path_type" in classes:
                classes.append("impl_relation")
        else:
            rec["shape"] = "method_call"
            receiver = significant(call)[0]
            inner, wrappers = unwrap(receiver)
            evidence["receiver"] = text(data, receiver)[:80]
            kind = inner.kind if inner is not None else None
            recv_type = None
            if kind == "PATH_EXPR":
                path = next((c for c in inner.children if c.kind == "PATH"), None)
                segs = path_segments(data, path) if path is not None else []
                name = segs[0] if len(segs) == 1 else None
                if name == "self":
                    classes.append("receiver_self")
                    recv_type = self_type
                elif name:
                    b = fs.binding(call, name)
                    ident_tok = syntax._last_segment_token(path)
                    sig = self.signature_of_name(site["file"], ident_tok) if ident_tok else None
                    recv_type = signature_type(sig)
                    evidence["receiver_signature"] = sig
                    if b and b[0] == "let":
                        classes.append("receiver_local_variable_type")
                        evidence["local"] = self._let_evidence(fs, b[1])
                    elif b and b[0] == "param":
                        classes.append("receiver_parameter_type")
                        ty = next((c for c in significant(b[1]) if c.kind.endswith("_TYPE")),
                                  None)
                        tyt = text(data, ty) if ty is not None else None
                        evidence["parameter_type"] = tyt
                        evidence["parameter"] = self._type_evidence(tyt, generics)
                    elif b and b[0] == "closure_param":
                        classes.append("receiver_local_variable_type")
                        ty = next((c for c in significant(b[1]) if c.kind.endswith("_TYPE")),
                                  None)
                        evidence["local"] = "closure parameter, annotated" if ty is not None \
                            else "closure parameter, unannotated"
                    elif b and b[0] == "pattern":
                        classes.append("receiver_local_variable_type")
                        evidence["local"] = "pattern binding (%s)" % b[1]
                    else:
                        evidence["receiver_binding"] = "not found (static, const or global)"
                        classes.append("unknown_other")
                else:
                    classes.append("receiver_call_result_type" if segs else "unknown_other")
                    evidence["receiver_path"] = "::".join(segs)
            elif kind == "FIELD_EXPR":
                classes.append("receiver_field_type")
                ref = next((c for c in inner.children if c.kind == "NAME_REF"), None)
                tok = syntax._name_ref_token(ref) if ref is not None else None
                sig = self.signature_of_name(site["file"], tok) if tok else None
                evidence["field_signature"] = sig
                recv_type = signature_type(sig)
            elif kind in ("CALL_EXPR", "METHOD_CALL_EXPR"):
                classes.append("receiver_call_result_type")
                if kind == "METHOD_CALL_EXPR":
                    ref = next((c for c in inner.children if c.kind == "NAME_REF"), None)
                    tok = syntax._name_ref_token(ref) if ref is not None else None
                else:
                    p = next((c for c in significant(inner)[0].children if c.kind == "PATH"),
                             None) if significant(inner) and significant(inner)[0].kind == \
                        "PATH_EXPR" else None
                    tok = syntax._last_segment_token(p) if p is not None else None
                sig = self.signature_of_name(site["file"], tok) if tok else None
                evidence["inner_call_signature"] = sig
                recv_type = return_type(sig)
            elif kind in ("MACRO_EXPR", "MACRO_CALL"):
                classes.append("macro_generated")
            elif kind == "LITERAL" or kind in ("TUPLE_EXPR", "ARRAY_EXPR"):
                classes.append("external_type")
                evidence["receiver_literal"] = True
            else:
                classes.append("unknown_other")
                evidence["receiver_kind"] = kind
            if wrappers:
                evidence["receiver_wrappers"] = wrappers
            if recv_type:
                evidence["receiver_type"] = recv_type
                base, wraps = base_type(recv_type)
                evidence["receiver_base_type"] = base
                if wraps:
                    classes.append("deref_wrapper_receiver")
                if base in generics or re.fullmatch(r"[A-Z]\d?", base or ""):
                    classes.append("generic_type_parameter")
                    if form in ("trait_method", "trait_impl_method"):
                        classes.append("trait_bound")
                elif target_base and base and base != target_base:
                    if base not in self.type_names:
                        classes.append("external_type")
                    else:
                        classes.append("deref_wrapper_receiver")
                if base and base not in self.type_names and base not in generics and \
                        "external_type" not in classes and not (target_base == base):
                    classes.append("external_type")
            if form == "trait_impl_method":
                classes.append("impl_relation")
            elif form == "trait_method":
                classes.append("trait_bound" if "generic_type_parameter" in classes
                               else "impl_relation")
        classes = [c for c in CLASSES if c in set(classes)] or ["unknown_other"]
        rec["classes"] = classes
        rec["primary"] = classes[0]
        rec["evidence"] = evidence
        rec["recoverable"] = self._recoverable(rec, classes, evidence)
        return rec

    def _let_evidence(self, fs, let):
        sig = significant(let)
        ty = next((c for c in sig if c.kind.endswith("_TYPE")), None)
        init = None
        seen_eq = False
        for c in sig:
            if c.kind == "EQ":
                seen_eq = True
            elif seen_eq and (c.kind.endswith("_EXPR") or c.kind == "LITERAL"):
                init = c
                break
        if ty is not None:
            return "annotated"
        if init is None:
            return "no initializer"
        inner, _ = unwrap(init)
        k = inner.kind if inner is not None else None
        if k == "RECORD_EXPR":
            return "constructor (struct literal)"
        if k == "CALL_EXPR":
            callee = significant(inner)[0]
            if callee.kind == "PATH_EXPR":
                p = next((c for c in callee.children if c.kind == "PATH"), None)
                segs = path_segments(fs.data, p) if p is not None else []
                if len(segs) >= 2 and (segs[-2][:1].isupper() or segs[-2] == "Self"):
                    return "constructor (associated function)"
                if len(segs) == 1 and segs[0][:1].isupper():
                    return "constructor (tuple struct)"
                return "call result (function)"
            return "call result (other)"
        if k == "METHOD_CALL_EXPR":
            return "call result (method)"
        if k in ("MACRO_EXPR", "MACRO_CALL"):
            return "macro"
        if k == "PATH_EXPR":
            return "another binding"
        if k == "CLOSURE_EXPR":
            return "closure"
        return "other (%s)" % k

    @staticmethod
    def _type_evidence(tyt, generics):
        if not tyt:
            return "unannotated"
        t = tyt.strip()
        if t.startswith("impl "):
            return "impl Trait"
        if t.startswith("dyn ") or "dyn " in t:
            return "dyn Trait"
        base, wraps = base_type(t)
        if base in generics:
            return "generic parameter"
        if wraps:
            return "wrapper of a concrete type"
        return "concrete type"

    def _re_exported(self, rec, segs, imported):
        """Whether the path (or the import its head stands for) names the target
        through a module other than the target's own: a re-export."""
        target = rec.get("oracle_target")
        if not target:
            return False
        named = list(imported or []) + segs[1:] if imported else segs
        modules = [s for s in named[:-1] if s not in ("crate", "self", "super")
                   and not s[:1].isupper() and s != "<qualified>"]
        if not modules:
            return False
        parts = target.split(".")
        tmods = [p for p in parts if p not in ("src", "mod", "lib", "main")]
        last_mod = modules[-1].replace("-", "_")
        return last_mod not in [m.replace("-", "_") for m in tmods]

    @staticmethod
    def _recoverable(rec, classes, ev):
        """Whether structural evidence PDX could read names the target: `structural`
        (a path's identity, a declared or constructed type), `propagation` (a call's
        declared return type), `proof` (a trait bound or impl relation to prove),
        `unknown` (nothing structural: an unannotated closure or pattern binding, a
        macro, an external value)."""
        p = rec["primary"]
        if p in ("crate_self_super_path", "re_export", "import_use_binding",
                 "associated_static_path_type"):
            return "structural"
        if p == "receiver_local_variable_type":
            local = ev.get("local", "")
            if local == "annotated" or local.startswith("constructor"):
                return "structural"
            if local.startswith("call result"):
                return "propagation"
            return "unknown"
        if p == "receiver_parameter_type":
            pe = ev.get("parameter")
            if pe in ("concrete type", "wrapper of a concrete type"):
                return "structural"
            if pe in ("generic parameter", "impl Trait", "dyn Trait"):
                return "proof"
            return "unknown"
        if p == "receiver_field_type":
            return "structural" if ev.get("field_signature") else "unknown"
        if p == "receiver_call_result_type":
            return "propagation" if ev.get("inner_call_signature") else "unknown"
        if p == "receiver_self":
            return "structural"
        if p in ("trait_bound", "generic_type_parameter", "impl_relation"):
            return "proof"
        return "unknown"


def artifact(argv):
    """The committed form of classifications: `{"format", "classes", "states": {label:
    {corpus, matching_version, population, summary}}}`, and each state's relations as
    lines, `state` added, in the state's order. Results hold no source text beyond a
    callee as A0's results carry it (`report.callee_text`: literals and comments elided):
    a relation's `evidence` (the receiver's signature as the oracle and the source write
    it) stays in the classification document and is not written here."""
    ap = argparse.ArgumentParser(prog="rust_candidates.py artifact")
    ap.add_argument("--state", action="append", required=True, help="LABEL=classification")
    ap.add_argument("--summary", required=True, type=Path)
    ap.add_argument("--relations", required=True, type=Path)
    args = ap.parse_args(argv)
    states = {}
    lines = []
    for spec in args.state:
        label, _, path = spec.partition("=")
        doc = json.loads(Path(path).read_text())
        if not label or label in states or doc.get("format") != FORMAT:
            sys.exit(f"error: {spec}: a new label and a format {FORMAT} classification")
        states[label] = {k: doc[k] for k in ("corpus", "matching_version", "population",
                                             "summary")}
        for r in doc["relations"]:
            row = {k: v for k, v in r.items() if k != "evidence"}
            row["callee"] = report.callee_text(row.get("callee"))
            lines.append(json.dumps({"state": label, **row}, sort_keys=True,
                                    separators=(",", ":")))
    out = {"format": FORMAT, "classes": list(CLASSES), "states": states}
    args.summary.write_text(json.dumps(out, indent=1, sort_keys=True) + "\n")
    args.relations.write_text("\n".join(lines) + "\n")
    return 0


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if argv[:1] == ["artifact"]:
        return artifact(argv[1:])
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--corpus", required=True)
    ap.add_argument("--checkout", required=True, type=Path)
    ap.add_argument("--audit", required=True, type=Path)
    ap.add_argument("--sites", type=Path,
                    help="the structural dataset (pdx-bench audit-sites), for candidate counts")
    ap.add_argument("--rust-analyzer", required=True)
    ap.add_argument("--scip", action="append", default=[])
    ap.add_argument("--typed-rows", type=Path)
    ap.add_argument("--out", required=True, type=Path)
    args = ap.parse_args(argv)
    c = Classifier(args)
    rows = {}
    with open(args.audit / f"{args.corpus}.sites.jsonl") as f:
        for line in f:
            r = json.loads(line)
            rows[(r["file"], r["site"])] = r
    if args.sites:
        counts = {}
        with open(args.sites) as f:
            for line in f:
                r = json.loads(line)
                if r.get("t") == "site":
                    counts[(r["path"], r["i"])] = len(r.get("candidates") or [])
                elif r.get("t") == "def" and r.get("kind") in (
                        "Struct", "Enum", "TypeAlias", "Interface", "Class", "Trait"):
                    c.type_names.add(r["name"])
        for key, r in rows.items():
            r["candidate_count"] = counts.get(key)
    population = Counter()
    out = []
    with open(args.audit / f"{args.corpus}.relations.jsonl") as f:
        for line in f:
            rel = json.loads(line)
            if rel.get("oracle_status") != "mapped" or not rel.get("oracle_target_callable"):
                continue
            ps = [s for s in rel["pdx_sites"] if s["status"] == "resolved"]
            if not ps:
                population["no_site"] += 1
                continue
            if len(ps) > 1:
                population["site_ambiguous"] += 1
                continue
            site = rows[(rel["file"], ps[0]["site"])]
            if site["band"] in DRAWN and site.get("materialized", True):
                population["drawn_" + ("correct" if site.get("verdict") == "correct"
                                       else "wrong")] += 1
                continue
            band = site["band"] if site.get("materialized", True) else site["band"] + \
                "_not_materialized"
            population[band] += 1
            out.append(c.classify(rel, site))
    out.sort(key=lambda r: (r["file"], r["token"]))
    summary = summarise(out)
    doc = {
        "format": FORMAT,
        "corpus": args.corpus,
        "matching_version": audit.ALGORITHM_VERSION,
        "classes": list(CLASSES),
        "population": dict(sorted(population.items())),
        "summary": summary,
        "relations": out,
    }
    args.out.write_text(json.dumps(doc, indent=1, sort_keys=True) + "\n")
    print(json.dumps({"population": doc["population"], "primary": summary["primary"]},
                     indent=1))
    return 0


def summarise(rows):
    primary = Counter(r["primary"] for r in rows)
    any_label = Counter(c for r in rows for c in r["classes"])
    recoverable = Counter(r["recoverable"] for r in rows)
    by_primary = {}
    for cls in CLASSES:
        sub = [r for r in rows if r["primary"] == cls]
        if not sub:
            continue
        by_primary[cls] = {
            "relations": len(sub),
            "bands": dict(Counter(r["band"] for r in sub).most_common()),
            "recoverable": dict(Counter(r["recoverable"] for r in sub).most_common()),
            "typed_outcome": dict(Counter(r["typed_outcome"] for r in sub).most_common(6)),
            "target_forms": dict(Counter(r["target_form"] for r in sub).most_common()),
        }
    return {
        "relations": len(rows),
        "primary": dict(primary.most_common()),
        "any_label": dict(any_label.most_common()),
        "recoverable": dict(recoverable.most_common()),
        "by_primary": by_primary,
    }


if __name__ == "__main__":
    sys.exit(main())
