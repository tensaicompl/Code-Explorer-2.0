# Legacy comparison — Python golden (P2-13, triage only)

This report compares the superseded implementation's drawn call graph with PDX's
drawn `CALLS` graph on the same pinned Python repository, and explains a deterministic
sample of where they differ. It is triage input, not a measurement of correctness. The
legacy graph is not an oracle: a difference is a difference until the source shows
what it is. No threshold applies, and nothing in PDX was tuned.

| | |
|---|---|
| Repository | the `[[golden]]` entry of `bench/repos.lock` with `language = "python"`, at `d73fa1cdcbd8b1465c151db8924ba58b1dd14e35` |
| PDX graph | P2-12's segment of that commit: engine 4, segment schema 3, language matrix 2, secret detector 4, profile `structural` |
| Legacy graph | `legacy/indexer` from this repository (unchanged), `build_corpus.py` then `graph_pass.builder` |
| Produced by | `scripts/compare-legacy.py`; machine evidence under the ignored `bench/.cache/legacy-compare/` |

## Method

- **The legacy run.** The legacy indexer ran through `legacy/docker-compose.yml` with
  its `docker-indexer` profile, as an isolated Compose project:
  - PostgreSQL on a volume of its own, with no host port;
  - the staged source mounted read-only;
  - the CPU only;
  - no host environment.

  It built its corpus, embedding model included, and its graph builder wrote the
  graph. The raw call rows were exported, and the stack was taken down.
- **The same source bytes.** The legacy run read a copy of P2-12's verified checkout
  of the pinned commit. The copy holds the tracked files, less the one the segment
  records as redacted (`tests/test_apps/.env`), which PDX never reads either.
- **What is compared.** Only the legacy graph's `calls` edges; `contains` and
  `depends_on` are ignored. On the PDX side, only `CALLS` rows in a drawn band:
  `CALL_REFERENCE`, `TESTS`, `DEFINES_ROUTE` and every other kind are excluded.
- **The unit is a source/target pair** (decision 46). The legacy builder collapses
  every call between two nodes into one edge, so PDX's per-site `CALLS` rows are
  collapsed the same way. A pair's rows, bands, sites and engine strategies are kept
  as evidence. The legacy edge weight is normalised to the heaviest pair and is not
  read as a count.
- **The crosswalk.** Legacy and PDX ids are unrelated, so legacy endpoints are mapped
  to PDX nodes by evidence, never by order:
  - a file by its repository-relative path;
  - a symbol by its file and name, among PDX nodes of a compatible kind, never in
    another file;
  - then by the legacy qualified name, equal to the PDX one or a suffix of it after a
    dot;
  - then by an exactly equal start line.

  Each narrowing applies only when it leaves one candidate. An endpoint that does not
  map is never forced onto a node: its edge stays only-legacy and is labelled
  `endpoint-unmapped` or `endpoint-ambiguous`.
- **The counts.**
  - **same:** a legacy edge whose endpoints both map to the PDX pair.
  - **only-legacy:** every other legacy edge.
  - **only-new:** a PDX pair no legacy edge maps to.
- **The sample.** Differences are ranked by SHA-256 over side, source and target,
  and the first ten of each side are explained. Each was read at its source lines,
  with the PDX site evidence and the raw legacy call rows. The evidence file marks the
  same records with `sample_number`.

## Counts

```text
same          302
only-legacy   846
only-new      166
```

| Legacy `calls` edges | PDX `CALLS` rows | PDX unique pairs | PDX rows collapsed |
|---|---|---|---|
| 1,148 | 639 | 468 | 171 |

Accounting holds exactly:
- same + only-legacy = 302 + 846 = 1,148 legacy edges;
- same + only-new = 302 + 166 = 468 PDX pairs;
- no two legacy edges map to one PDX pair.

The committed golden agrees with the segment: its `CALLS` collapse to the same 468
qualified-name pairs, and no two node pairs share a qualified-name pair.

The 846 only-legacy edges break down as follows:

| Only-legacy | Edges |
|---|---|
| Both endpoints mapped, the pair absent from PDX `CALLS` | 599 |
| An endpoint unmapped | 247 |
| An endpoint ambiguous | 0 |

### Crosswalk

The 1,148 legacy edges have 2,296 endpoint references, or 806 distinct endpoints.

| | References | Distinct endpoints |
|---|---|---|
| Mapped uniquely | 2,017 | 679 (629 by file and name, 43 files by path, 7 by qualified name) |
| Unmapped | 279 | 127 |
| Ambiguous | 0 | 0 |

901 legacy edges have both endpoints mapped. Every unmapped endpoint is a definition
inside a function body, as its legacy qualified name shows: 115 of the 127 directly (a
function or class defined in a function), and 12 as methods of a class defined in a
function. 115 are in `tests/`, 10 in `src/` and 2 in `examples/`. The segment has no
node for such definitions (see issue 75). These are node-model differences, not
resolution ones.

### Legacy graph

| Measure | Value |
|---|---|
| Files | 88 |
| Symbols | 1,945 |
| Nodes | 2,045: 1,462 function, 306 endpoint, 160 class, 88 file, 21 module, 6 document, 2 table |
| Edges | 4,508: 3,359 `contains`, 1,148 `calls`, 1 `depends_on` |
| `unmappedKinds` | empty |
| `ambiguousReferences` | empty |
| `skippedLowPrecisionCallSites` | 0 |

Its resolver's own totals, as its sidecar reports them per call site (not per edge):

| callSites | internalCandidates | exact | scoped | importGuided | inheritanceGuided | ambiguous | blocked | external | drawn |
|---|---|---|---|---|---|---|---|---|---|
| 3,896 | 2,378 | 1,243 | 238 | 102 | 19 | 353 | 423 | 1,518 | 1,602 |

### PDX bands

Each row counts the unique pairs holding at least one `CALLS` row of that band. A pair
can hold several bands, so a column need not sum to its pairs.

| Band | All pairs (468) | Same (302) | Only-new (166) |
|---|---|---|---|
| typed | 246 | 201 | 45 |
| exact | 202 | 96 | 106 |
| scoped | 9 | 1 | 8 |
| import-guided | 11 | 5 | 6 |
| inheritance-guided | 2 | 1 | 1 |
| precise | 0 | 0 | 0 |

## What the differences are

These figures come from the evidence for every difference, not from the sample.

- **Only-legacy, both endpoints mapped (599).** PDX saw the call in all but three: it
  has a site at the legacy call's line for the same callee. What PDX did with it:

  | PDX outcome | Edges |
  |---|---|
  | A candidate no stage validates | 399 |
  | Several definitions survive every stage | 93 |
  | Blocked by the generic-name list | 54 |
  | Drew a `CALLS` edge to the same target from another source (mostly `typed`) | 27 |
  | Unresolved | 9 |
  | External | 7 |
  | Other | 10 |

  The candidates are overwhelmingly attribute calls whose receiver PDX cannot type:
  test fixtures such as `app` and `client`, and dictionaries. The legacy resolver drew
  each of these to the repository's one definition of the bare name, whatever the
  receiver. The blocked ones are `update`, `open`, `run`, `push` and `pop`.
- **Only-legacy, an endpoint unmapped (247).** All are nested definitions the segment
  does not model, as above.
- **Only-new (166).** Every one has a raw legacy call row with the same callee at the
  same line: the legacy pipeline saw the call and drew no edge for this pair.
  - **Sources:** 64 tests, 52 methods, 32 files and 18 functions.
  - **Targets:** 90 classes, 54 methods and 22 functions.
  - **Bands:** 106 exact, 45 typed, 8 scoped, 6 import-guided and 1
    inheritance-guided.
  - **None lies outside the legacy walk** (the legacy indexer skips dot-directories
    and directories named `scripts` or `build`).

## Twenty sampled differences

Ten of each side, by hash rank. Each "Sample" value is the record's `sample_number`
in `comparison/evidence.jsonl`. Locations are in the pinned checkout.

| Sample | Side | Source → target | PDX band(s) | Evidence | What the source shows, and why the graphs differ | Category |
|---|---|---|---|---|---|---|
| 1 | only-legacy (unmapped) | `routes_command` → `echo`, a view nested in `test_json_request_and_response` | — (external) | `src/flask/cli.py:1066`, `:1107`; `tests/test_testing.py:267` | Both lines call `echo` on the imported third-party `click` module. Legacy matched the bare name to the one in-repo `echo`, a test's nested view. PDX: external. | likely legacy over-resolution |
| 2 | only-legacy (mapped) | `Flask.update_template_context` → `update`, a view in the tutorial example | — (blocked) | `src/flask/app.py:617`, `:619` | `update` is called on the template context dictionary. Legacy's blocklist acts only when a name has several definitions, and the example has one. PDX blocks the generic name. | likely legacy over-resolution |
| 3 | only-legacy (unmapped) | `test_static_url_path_with_ending_slash` → `Flask`, a subclass nested in `test_custom_config_class` | — (PDX: exact to `src.flask.app.Flask`) | `tests/test_basic.py:1447`; `tests/test_config.py:202` | The call is `flask.Flask(...)`, the application class. Legacy had two `Flask` classes and narrowed by directory to the test-local subclass. PDX drew the real class. | likely legacy over-resolution |
| 4 | only-legacy (unmapped) | `test_from_prefixed_env_custom_prefix` → the same nested `Flask` | — (PDX: exact to `src.flask.app.Flask`) | `tests/test_config.py:73` | As 3, narrowed by the same file. | likely legacy over-resolution |
| 5 | only-legacy (mapped) | `runner` fixture → `Flask.test_cli_runner` | — (one candidate, not validated) | `examples/tutorial/tests/conftest.py:44` | The fixture calls `test_cli_runner` on its `app` parameter, an untyped pytest fixture holding the application. The target is right. PDX has it as its only candidate but no stage validates an untyped receiver. Legacy drew it by unique name. | legacy likely improvement / PDX miss |
| 6 | only-legacy (unmapped) | `app_index`, a view nested in a test → `Flask.url_for` | — (two candidates survive) | `tests/test_blueprints.py:257` | The call is the module helper `flask.url_for` (`helpers.url_for`), not the method legacy chose. PDX kept both definitions as candidates. The nested source has no PDX node. | likely legacy over-resolution |
| 7 | only-legacy (mapped) | `Blueprint.before_app_request` → `_AppCtxGlobals.setdefault` | — (candidate, not validated) | `src/flask/sansio/blueprints.py:619` | `setdefault` is called on the app's `before_request_funcs` dictionary. Legacy drew the repository's one `setdefault`. | likely legacy over-resolution |
| 8 | only-legacy (mapped) | `AppContext.__enter__` → `AppContext.push` | — (blocked) | `src/flask/ctx.py:507`, `:416` | `self.push()`, and `push` is defined once in the repository: a real call. PDX's generic-name list holds `push` and takes precedence over typed resolution (B.4). | legacy likely improvement / PDX miss |
| 9 | only-legacy (mapped) | file `src/flask/ctx.py` → `_CollectErrors.raise_any` | — (PDX: typed, from `AppContext.pop`) | `src/flask/ctx.py:504` | The call is in `AppContext.pop`. `ctx.py` defines two `pop` methods, so legacy cannot pick its source and falls back to the file. PDX draws the same call, typed, from the method; its pair is only-new. | multiplicity/model-shape difference |
| 10 | only-legacy (unmapped) | `error`, an async view nested in the `_async_app` fixture → `AppError` | — (PDX: exact, from the file) | `tests/test_async.py:56`, `:14` | `AppError` is raised inside the nested view. Both find the target. PDX has no node for the nested view and draws from the file (issue 75). | legacy endpoint could not be mapped |
| 11 | only-new | `_split_blueprint_path` → itself | exact | `src/flask/helpers.py:649` | A recursive call. The legacy builder drops self-loops by construction. | multiplicity/model-shape difference |
| 12 | only-new | file `tests/test_basic.py` → `_AppCtxGlobals.setdefault` | exact | `tests/test_basic.py:558` | `setdefault` is called on `flask.session`, the session object, inside a nested view. `_AppCtxGlobals` is the type of `g`, not of the session. PDX chose the repository's one definition of the name. Legacy's candidates included the nested view, also named `setdefault`; it narrowed to that and dropped the self-loop. | likely PDX over-resolution |
| 13 | only-new | `TagDict.to_json` → `TaggedJSONSerializer.tag` | typed (`lsp_method`, 0.90) | `src/flask/json/tag.py:112`, `:69`, `:289` | `self.serializer.tag(...)`, and the base class annotates `serializer` as `TaggedJSONSerializer`. Legacy saw two `tag` methods in the file and left it ambiguous. | PDX likely improvement |
| 14 | only-new | `test_templates_and_static` → `Flask` | exact | `tests/test_blueprints.py:217` | `flask.Flask(...)` is the application class. Legacy drew this caller to the test-local subclass instead (as 3). | PDX likely improvement |
| 15 | only-new | file `tests/test_blueprints.py` → `_AppCtxGlobals.setdefault` | exact (six sites) | `tests/test_blueprints.py:917`, `:925`, `:937`, `:945`, `:957`, `:965`; `src/flask/globals.py:47` | `setdefault` on `flask.g`, which proxies `_AppCtxGlobals`: the target is right. The calls are in functions nested in a test. Legacy draws from each nested function (unmapped); PDX from the file (issue 75). | different but both defensible modelling |
| 16 | only-new | `PassDict.to_json` → `TaggedJSONSerializer.tag` | typed (`lsp_method`, 0.90) | `src/flask/json/tag.py:128` | As 13. | PDX likely improvement |
| 17 | only-new | `test_find_best_app` → `Flask` | exact (seven sites) | `tests/test_cli.py:49` and six more | `Flask`, imported from the package, is called in class bodies nested in the test. Legacy attributes to the file (many nested classes named `Module`) and narrows `Flask` by directory to the test-local subclass. | PDX likely improvement |
| 18 | only-new | `test_config_from_file_json` → `Flask` | exact | `tests/test_config.py:32` | `flask.Flask(...)`. Legacy narrowed by the same file to the subclass nested in another test there. | PDX likely improvement |
| 19 | only-new | file `tests/test_helpers.py` → `Response` | exact (five sites) | `tests/test_helpers.py:237` and four more; `src/flask/json/provider.py:89`, `:189` | `flask.Response(...)` in nested views. Legacy compares names case-insensitively, so the class `Response` collides with two `response` methods, and it drew nothing. | PDX likely improvement |
| 20 | only-new | file `src/flask/sansio/scaffold.py` → `Scaffold._check_setup_finished` | scoped | `src/flask/sansio/scaffold.py:46` | A `self` call in the wrapper nested in the decorator `setupmethod`; `self` is annotated `Scaffold`, so the target is right. Legacy draws from the nested wrapper (unmapped); PDX from the file (issue 75). | different but both defensible modelling |

| Category (human triage, not a metric) | Samples |
|---|---|
| PDX likely improvement | 6 (13, 14, 16, 17, 18, 19) |
| Likely legacy over-resolution | 6 (1, 2, 3, 4, 6, 7) |
| Legacy likely improvement / PDX miss | 2 (5, 8) |
| Multiplicity/model-shape difference | 2 (9, 11) |
| Different but both defensible modelling | 2 (15, 20) |
| Legacy endpoint could not be mapped | 1 (10) |
| Likely PDX over-resolution | 1 (12) |
| Uncertain | 0 |

## Typed edges

- **Shares of typed evidence.**
  - Same pairs: 201 of 302.
  - Only-new pairs: 45 of 166.
  - Mapped only-legacy edges: in 27, PDX drew the same target, mostly typed, from
    another source.
- **The sample.** It holds two typed only-new pairs (13 and 16). Both are right, and
  both rest on a declared attribute type the legacy resolver cannot see.
- **Beyond the sample.** I also read the call text of all 45 only-new typed pairs, as
  a supplementary check outside the twenty. Strategies: 36 `lsp_method` (score 0.90),
  4 `lsp_callable_alias` (0.97), 3 `lsp_direct` (0.95), 3 `lsp_constructor` (0.85).
  - They are `self` calls to methods the class or its declared base defines, calls
    through annotated attributes, direct calls, and constructors.
  - Their targets are what the receivers and names say. The three rows at exactly
    the threshold, 0.85, are constructors of repository classes:
    - `NoAppException`, twice, in the module that defines it;
    - `_CollectErrors`, imported by name into `ctx.py`.
  - Many are only-new because the legacy builder attributes them to the file: a
    method name such as `save_session` occurs twice in its file.
- **Contradictions.** No sampled or reviewed typed edge contradicts its source.

## Limitations

- **Not an oracle.** The legacy graph is not an oracle, and no compiler oracle was
  used. The categories are one reader's triage of 20 differences, not precision or
  recall.
- **Multiplicity.** The legacy graph collapses call multiplicity. The comparison is
  pair-level, and says nothing about how many sites either side found.
- **Identity models.** The two use different node identity models. The crosswalk maps
  only what evidence supports, and an unmapped legacy endpoint is not a PDX miss: all
  of them here are definitions nested in functions, which the segment does not model.
- **Attribution.** Source attribution differs:
  - legacy falls back to the file when a caller's name is not unique in its file;
  - PDX attributes calls inside nested definitions to the file (issue 75).

  A pair can therefore differ while both sides draw the same call.
- **Scope.** One Python golden only, structural `CALLS` only.
- **Legacy call count.** The legacy indexer logged 3,906 extracted call rows; its call
  table holds 3,896, which is what its graph builder read.

## Conclusion

- **What dominates.**
  - Most only-legacy edges are calls PDX sees and deliberately does not draw:
    attribute calls on receivers it cannot type, names that several definitions
    share, and blocklisted generic names. The legacy resolver draws these by unique
    bare name, whatever the receiver.
  - The rest are nested definitions the segment has no node for.
  - Most only-new pairs are calls legacy saw but could not settle, or settled
    elsewhere:
    - its case-insensitive name matching;
    - its directory narrowing to a test-local class of the same name;
    - its file fallback for non-unique caller names;
    - its dropped self-loops.
- **Typed edges.** The sampled new typed edges look useful, and so do all 45 only-new
  typed pairs on reading. None looked suspect.
- **Systematic PDX gaps.** The legacy-only samples show none in resolution. They show
  two deliberate conservatisms:
  - untyped receivers stay candidates (sample 5);
  - the generic-name list outranks a typed `self` call (sample 8).

  These trade recall for precision by design. In the sample, legacy's bare-name rule
  drew wrong edges (samples 1, 2, 6, 7) at least as often as right ones. The one
  systematic gap is attribution, not resolution: calls inside nested definitions come
  from the file. It is recorded as issue 75, for the driver.
- **Typed acceptance.** There is not enough evidence for a `TYPED_MIN_SCORE` SCR. No
  typed edge was found wrong, including the three accepted at the 0.85 threshold.
- **The one PDX over-resolution** in the sample (12) is an `exact` edge, not a typed
  one: a method name with one definition in the repository, drawn for a call on an
  object of another type.

No SCR warranted from this sample. `TYPED_MIN_SCORE` stays 0.85.
