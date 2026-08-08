"""
Callee-name -> symbol resolution — The correctness core.

calls_* stores callee_name as a BARE STRING: no foreign key, no symbol id,
no target file. Resolution is therefore inference, and the defect is
symmetric — it produces both false edges and dropped edges.

MEASURED ON REAL DATA, awp/Python, 7,610 call sites):
    exact      1,187      \\
    ambiguous    699      /  = 1,886 internal candidates -> 62.9% exact
    external   5,724         (stdlib/3rd-party: Column, Depends, len, str —
                              CORRECTLY unresolved, ~75% of all sites)

62.9% sits in the 50-80% band: build as specified, confidence policy ON.

POLICY — do not trade this away for a denser-looking graph:
    exact, scoped -> emit a `calls` edge
    ambiguous     -> emit NOTHING. Never pick a winner arbitrarily.
    unresolved    -> emit nothing. Count it.
    confidence    -> a SIDECAR, never on the edge: GraphEdgeSchema is a plain
                     z.object() and silently strips unknown fields R2).
    statistics    -> surfaced in the UI. A user who can see the resolution rate
                     can calibrate trust; one who cannot will over-trust the picture.

LAYER SCOPING BARELY HELPS — 4.4% measured. Of 699 ambiguous sites, layer
scope rescued 31. So `import-guided` is the only lever that materially
improves precision — prioritise it.
"""

from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass, field

# Ambiguity is concentrated, so a blocklist buys most of the win for a few lines.
# Measured worst offenders in awp: `get` alone is 506 call sites — 6.6% of the
# entire repo — across 2 definitions; `to_dict` spans 16 files.
#
# These are generic member names whose call sites almost never identify a unique
# target, and drawing them produces confidently-wrong edges.
GENERIC_NAME_BLOCKLIST = {
    "get", "set", "close", "open", "run", "main", "start", "stop",
    "to_dict", "from_dict", "to_json", "from_json", "serialize", "deserialize",
    "handle", "execute", "process", "validate", "parse", "format",
    "update", "create", "delete", "remove", "add", "append", "clear",
    "read", "write", "load", "save", "send", "receive",
    "__init__", "__str__", "__repr__", "__enter__", "__exit__",
    "setup", "teardown", "init", "reset", "next", "value", "name",
}

# Languages whose call extraction is too noisy to draw edges from at all.
#   perl — no keyword filter: `if (`, `while (`, `for (` become callees
#   ada  — RE_ADA_CALL matches any `identifier(`; string literals are not stripped
#          during call extraction, so array indexing and aggregates become calls
LOW_PRECISION_EXTENSIONS = {".pl", ".pm"}

BAND_EXACT = "exact"
BAND_SCOPED = "scoped"
BAND_AMBIGUOUS = "ambiguous"
BAND_IMPORT = "import-guided"
BAND_INHERIT = "inheritance-guided"
BAND_BLOCKED = "blocked"
BAND_EXTERNAL = "external"

# Only these become edges.
# `import-guided` joins the drawn set because it is a STRONGER signal than
# `scoped`, not a weaker one: same-directory proximity is a guess about layout,
# whereas an import is the caller declaring what it can reach. Both still
# require exactly one surviving candidate.
DRAWABLE = {BAND_EXACT, BAND_SCOPED, BAND_IMPORT, BAND_INHERIT}


@dataclass
class ResolutionStats:
    """Published in graph_meta and surfaced in the UI."""

    total: int = 0
    by_band: dict[str, int] = field(default_factory=lambda: defaultdict(int))

    def record(self, band: str) -> None:
        self.total += 1
        self.by_band[band] += 1

    @property
    def internal_candidates(self) -> int:
        return self.total - self.by_band[BAND_EXTERNAL]

    @property
    def pct_exact_of_internal(self) -> float:
        """
        The metric that matters. `% of ALL call sites` is misleading because
        70-85% correctly target stdlib and third-party code.
        """
        denom = self.internal_candidates
        return round(100.0 * self.by_band[BAND_EXACT] / denom, 1) if denom else 0.0

    def as_dict(self) -> dict:
        return {
            "callSites": self.total,
            "internalCandidates": self.internal_candidates,
            "exact": self.by_band[BAND_EXACT],
            "scoped": self.by_band[BAND_SCOPED],
            "ambiguous": self.by_band[BAND_AMBIGUOUS],
            "importGuided": self.by_band[BAND_IMPORT],
            "inheritanceGuided": self.by_band[BAND_INHERIT],
            "blocked": self.by_band[BAND_BLOCKED],
            "external": self.by_band[BAND_EXTERNAL],
            "drawn": sum(self.by_band[b] for b in DRAWABLE),
            "pctExactOfInternal": self.pct_exact_of_internal,
        }


class CalleeResolver:
    """
    Resolves a bare callee name to a single symbol, or refuses to.

    Build once per project+stream, then call resolve() per call site.
    `symbols` rows need at least: name, filename, and the caller-assigned node id.
    """

    def __init__(self, symbols: list[dict], node_id_of, imports=None) -> None:
        self._by_name: dict[str, list[dict]] = defaultdict(list)
        for s in symbols:
            self._by_name[s["name"].lower()].append(s)
        self._node_id_of = node_id_of
        self.stats = ResolutionStats()

        # caller file -> the set of names that file imports, lowercased.
        # Absent for languages with no import extractor, in which case this
        # whole band simply never fires and behaviour is unchanged.
        self._imports: dict[str, set[str]] = defaultdict(set)
        # class -> its declared bases, lowercased. Populated from the same
        # table; `kind` separates the two families.
        self._bases: dict[str, set[str]] = defaultdict(set)
        for row in imports or ():
            if row.get("kind") == "inherits":
                owner = str(row.get("owner") or "").lower()
                if owner:
                    self._bases[owner].add(str(row["module"]).lower())
            else:
                self._imports[row["filename"]].add(str(row["module"]).lower())

        # Which class declares each symbol, for the inheritance band.
        self._declared_by: dict[str, str] = {}
        for sym in symbols:
            parent = (sym.get("parent_name") or "").lower()
            if parent:
                self._declared_by[id(sym)] = parent

    def _ancestors(self, cls: str) -> set[str]:
        """`cls` plus everything it inherits from, transitively.

        Depth-capped rather than cycle-checked-and-unbounded: a heritage graph
        assembled from a permissive regex can contain loops, and a resolver that
        hangs on one bad file is worse than one that gives up after ten levels.
        """
        out, frontier = {cls}, {cls}
        for _ in range(10):
            nxt = set()
            for c in frontier:
                nxt |= self._bases.get(c, set())
            nxt -= out
            if not nxt:
                break
            out |= nxt
            frontier = nxt
        return out

    def resolve(
        self, callee_name: str, caller_file: str, caller_name: str = "",
    ) -> tuple[str | None, str]:
        """
        Return (target_node_id_or_None, band).

        Narrowing order is same-file -> same-directory -> same-layer, taking the
        narrowest scope that yields exactly one candidate. Anything still
        ambiguous returns None: a wrong edge on a graph is a fact users believe.
        """
        key = (callee_name or "").lower()

        candidates = self._by_name.get(key)
        if not candidates:
            self.stats.record(BAND_EXTERNAL)
            return None, BAND_EXTERNAL

        if key in GENERIC_NAME_BLOCKLIST and len(candidates) > 1:
            self.stats.record(BAND_BLOCKED)
            return None, BAND_BLOCKED

        if len(candidates) == 1:
            self.stats.record(BAND_EXACT)
            return self._node_id_of(candidates[0]), BAND_EXACT

        for scope in (_same_file, _same_dir, _same_layer):
            narrowed = [c for c in candidates if scope(c["filename"], caller_file)]
            if len(narrowed) == 1:
                self.stats.record(BAND_SCOPED)
                return self._node_id_of(narrowed[0]), BAND_SCOPED

        # Import-guided. Lexical scope has failed, so fall back to what the
        # caller can actually SEE: a file cannot call into a module it never
        # imported. This is the lever exists for — layer scoping was
        # measured at 4.4 % effectiveness, because "same top-level folder" is
        # far too coarse to separate same-named symbols.
        #
        # Still requires exactly one survivor. Narrowing five candidates to two
        # is not an answer, and picking one of the two would be the arbitrary
        # guess this resolver refuses to make.
        seen = self._imports.get(caller_file)
        if seen:
            visible = [c for c in candidates if _import_visible(c["filename"], seen)]
            if len(visible) == 1:
                self.stats.record(BAND_IMPORT)
                return self._node_id_of(visible[0]), BAND_IMPORT

        # Inheritance-guided. The case imports cannot settle: Java routinely
        # declares the same method name on many classes, and an import names a
        # FILE, not which of the classes in it was meant. If the calling class
        # inherits from one of the declaring classes, that is the one it means.
        caller_cls = (caller_name or "").split(".")[0].lower()
        if caller_cls:
            family = self._ancestors(caller_cls)
            kin = [
                c for c in candidates
                if self._declared_by.get(id(c), "") in family
            ]
            if len(kin) == 1:
                self.stats.record(BAND_INHERIT)
                return self._node_id_of(kin[0]), BAND_INHERIT

        self.stats.record(BAND_AMBIGUOUS)
        return None, BAND_AMBIGUOUS


def _import_visible(candidate_file: str, imported: set[str]) -> bool:
    """
    Could a file importing `imported` reach a symbol defined in `candidate_file`?

    Matched on the file stem, because that is the one token every language's
    import spelling has in common: Java's `eu.eurocontrol.cap.SopBlock`,
    Python's `pkg.module`, and TypeScript's `./module` all end in the name of
    the file that defines the thing. Comparing whole paths would fail on all
    three; comparing the stem fails only when two files share a basename, and
    that case is caught by the single-survivor requirement above.
    """
    stem = candidate_file.rsplit("/", 1)[-1]
    stem = stem.rsplit(".", 1)[0].lower()
    if not stem:
        return False
    if stem in imported:
        return True
    return any(tok.endswith("." + stem) or tok.endswith("/" + stem) for tok in imported)


def _same_file(a: str, b: str) -> bool:
    return a == b


def _same_dir(a: str, b: str) -> bool:
    return a.rsplit("/", 1)[0] == b.rsplit("/", 1)[0]


def _same_layer(a: str, b: str) -> bool:
    return a.split("/", 1)[0] == b.split("/", 1)[0]
