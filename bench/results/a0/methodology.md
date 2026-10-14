# A0.1 and A0.2: structural accuracy against a semantic oracle

The early structural-trust checkpoint (decision 64, issue 95, gate G1b). A0.1
measures the drawn structural bands with 4.13.1's measures. A0.2 re-scores the same
fixed dataset under counterfactual policies. Neither changes resolver behaviour,
`Band::is_drawn`, the blocklist's order, extraction or the parser. A0.3, what is
done about the results, is the driver's decision.

This is not P5-06. No P5 task is started or done by it. The oracle is a
compiler-grade indexer run locally beside the structural engine, and no part of it
is in the product.

## Runs

The result files hold the latest run. Earlier runs stay in Git history:

| Run | Resolver measured | Results committed at |
|---|---|---|
| A0.1 baseline | `e746b1e` | `71bec72` (`git show 71bec72:bench/results/a0/<file>`) |
| post-R1, issue 103 (typed before the blocklist) | the commit "Issue 103: accept typed answers before blocklist", parent `2663ff5` | `d96bb2f` |
| post-R2, issue 104 (Rust trusted targets) | the commit "Issue 104: correct Rust trusted target resolution", parent `d96bb2f` | `d14ccd4` |
| post-111, issue 111 (Rust `use` trees) | the commit "Issue 111: parse Rust use trees structurally", parent `d6fbdd9` | that commit (these files) |

Every run uses the same golden pins, oracle versions and configuration,
`a0-match-1` and `a0-metrics-1`. The PDX diagnostic corpus is `e746b1e` in all, so
its change is the resolver's alone.

The post-R2 files record `engine_version` 7: at `d14ccd4` the engine called itself 7.
Issue 112 renumbered that engine 8 without changing what it extracts or answers.
Rerun at the hotfix, every document is identical but for `engine_version` (8, which
`run.py` now reads from `consts.rs` rather than writing out) and the `resolver`
label, and `mismatches.jsonl` and `counterfactuals.json` are byte-identical. The
post-111 files record engine 9.

The post-R1 run also corrects three harness outputs. None of the three changes a
baseline number: run over the baseline's datasets, they reproduce the committed
baseline documents and `mismatches.jsonl` exactly.

- An intermediate per-site `line` was one too high. The interface's span lines are
  already 1-based; result files recompute lines from bytes.
- The blocklist section counted a blocklisted name with a typed answer as
  suppressed even when Stage 3 draws it as typed.
- The provenance names the resolver measured (`resolver`) apart from the PDX corpus
  commit.

## Files

| File | What it holds |
|---|---|
| `python.json`, `rust.json` | One golden each: provenance, coverage, 4.13.1's measures band by band, the recall breakdown, `exact` and `scoped` false positives by category, overloads, the blocklist, counterfactuals and examples. `rust.json` also has macro token trees and receivers |
| `pdx-self.json` | The same for the PDX repository's Rust code. Diagnostic only: it can neither pass nor fail G1b |
| `counterfactuals.json` | A0.2's policies C0 to C6 for each corpus |
| `mismatches.jsonl` | Every drawn false positive and every oracle relation no drawn edge reproduces, for all three corpora: file, line, byte span, callee, band, structural target, oracle target, category |

The full site datasets are not committed. `scripts/a0/run.py` regenerates them, and
the result documents with them, from the pins and tool versions recorded here.

Every ratio is reported with its numerator and denominator. Results contain no
source text beyond a site's callee as the structural engine records it, no
absolute path and no URL. An oracle symbol is written without its scheme, manager
and version (`<package> <descriptors>`).

## Corpora

The goldens are read from `bench/repos.lock` at run time: the `python` and `rust`
entries, at their pinned commits, never substituted. Their checkouts are
`pdx-bench golden`'s, which must hold the pinned commit.

- **Python golden**: one tree.
- **Rust golden**: two Cargo workspaces, the root one and `examples/`, each indexed
  on its own. The two indexes are read as one oracle: an example's file is the root
  index's path prefixed with `examples/`.
- **PDX itself**, at the plan-control commit (`pdx_sha` in the provenance), Rust
  files only. A0.1 adds tooling and no product change, so the resolver measured is
  that commit's.

## The structural side

`pdx-bench audit-sites --root DIR --out FILE` runs the product's own Stages 1 to 3
(discovery, extraction with no cache, the symbol registry, resolution) in one
process. It writes every site with its Stage 3 band, target, candidates and engine
answer, and every definition with its byte span. It decides nothing.

A drawn edge is a resolution in a drawn band that Stage 4 persists. Stage 4 needs a
position and a node-type path, so a site typed resolution found by itself (inside a
macro's token tree, for instance) is counted in coverage but draws no edge. These
are reported as `resolution_not_materialized`.

The datasets reproduce the goldens' drawn `CALLS` and `CALL_REFERENCE` edges
exactly, band by band. That was checked against `bench/golden/` before any oracle
was read.

## The oracles

| Language | Oracle | How it runs |
|---|---|---|
| Python | `scip-python` 0.6.6 (pyright-based), as 4.6.1 names it | Installed with npm into an isolated prefix, the npm integrity hash recorded. It runs over an export of the pinned tree. The tree's dependencies come from its own `uv.lock` (`uv sync --frozen`, the `tests` and `typing` groups) into a virtual environment beside it. An explicit `pyrightconfig.json` indexes the whole tree at the project's Python version, 3.10 |
| Rust | `rust-analyzer scip`, as 4.6.1 names it, the `rust-analyzer` component of the pinned toolchain 1.98.1 | Downloaded from the toolchain's channel manifest with its SHA-256 checked, with `rust-src` of the same release for the standard library. It runs with all Cargo features, build scripts and procedural macros. The golden's dependencies come from its `Cargo.lock` into a Cargo home of its own. PDX's own are read offline from the existing cache |

Call sites come from each language's own reference parser, never from the
structural engine's. Python uses CPython's `ast`. Rust uses rust-analyzer's parser
(`rust-analyzer parse`); there, `CALL_EXPR` and `METHOD_CALL_EXPR` are calls, a
tuple-struct pattern is not, and a macro invocation is not a call. Call-shaped text
inside a macro invocation's token tree (a name, an optional turbofish, a
parenthesised tree) is reported separately as `macro_tt`, and inside a
`macro_rules!` body as `macro_rules_body`.

### What the oracle cannot see

These have no verdict. They are never counted false:

- a call whose name has no oracle occurrence (`none`). In Python this is most often
  a method on an untyped value, such as a pytest fixture parameter;
- a name the oracle binds to a document-local symbol the document never defines, or
  defines by an import it could not follow (`unresolved_local`,
  `unresolved_import`). scip-python 0.6.6 gives Python's builtins such symbols;
- a symbol the repository defines that maps to no structural definition, or to
  several (`unmapped`, `ambiguous`), unless rule M6's second clause applies;
- a file the oracle did not index. In the Rust golden these are
  `axum-macros/tests/**`, the compile-test fixtures, which belong to no crate.

## Matching rules (`a0-match-1`)

**M1. Site to oracle occurrence.** A structural site and an oracle call are joined
on one byte range: the name token. That is the identifier naming what is called,
the last name of the callee outside generic arguments, never a receiver.

- For a structural site it is read from the callee's own source bytes.
- When the engine names the callee in words of its own:
  - **M1b**: the name token of the one parser call spanning exactly the site's bytes;
  - else **M1c**: the one parser call inside the site's bytes with the site's name;
  - else **M1d**: the site's last name, when it is the site's name (a callable
    passed as a value).
- Two candidates under any rule give no token.

Line numbers, nearest occurrences and names alone are never used.

**M2. Oracle occurrence.** These are the non-definition occurrences at exactly the
name token's byte range. SCIP positions are converted to bytes with the document's
declared encoding. rust-analyzer declares UTF-8. scip-python declares none; its
pyright base counts UTF-16 units, which matters only on non-ASCII lines (28
occurrences in the Python golden). Several distinct symbols at one token are
`multiple`, with no verdict.

**M3. What the symbol is.**

- **Local**: a local symbol the document defines outside an import statement.
- **The repository's**: a symbol some indexed document defines. For Rust, also any
  symbol of a package the repository defines, so a macro-generated item with no
  source definition is still the repository's. For Python only the first test
  applies, since scip-python names dependencies it cannot identify after the
  project.
- **External**: anything else.

**M4. Oracle definition to structural definition.** A symbol's definition occurrence
maps to the innermost structural definition of the same name whose byte span holds
the occurrence. Spans nest, so an outer definition holds an inner one's name but
never the reverse.

- A symbol whose occurrences sit on the names of several definitions sharing one
  qualified name is **ambiguous**. scip-python gives a function's `@overload` stubs
  and its implementation one symbol, so the oracle cannot say which one a call
  reaches.
- **M4b (Rust).** rust-analyzer gives items of different crates one symbol:
  integration tests and example binaries of one package. Item paths are unique
  within a crate, so a reference in a document that defines the symbol exactly once
  names that document's definition.

**M5. Same target.** Two definitions are the same target when they are the same
definition. In Python, a class and its own `__init__` or `__new__` are also the
same target.

**M6. Verdict of a drawn edge.**

- **correct**: the oracle's mapped target is the edge's target (M5).
- **false_positive** when:
  - the oracle names another definition of the repository, something external, or
    a local binding; or
  - the oracle names a repository symbol that maps nowhere, while the edge's target
    maps to a different symbol (M4 read backwards).
- **unverifiable** otherwise.

## Measures (`a0-metrics-1`)

These are 4.13.1's, exactly:

- **precision** = drawn edges whose site has an oracle verdict and whose target the
  oracle confirms ÷ drawn edges whose site has an oracle verdict;
- **recall** = oracle call relations at parsed structural call sites that a drawn
  edge reproduces ÷ oracle call relations at parsed structural call sites;
- **oracle-verifiable fraction** = drawn edges with a verdict ÷ drawn edges.

An **oracle call relation** is a call, as the language's parser finds it, whose name
the oracle resolves to one repository definition that the structural model makes
callable: a function, method, constructor, class, struct or macro. A parsed
structural call site is a Stage 3 call site.

Precision covers every drawn edge, `CALL_REFERENCE` included. The result documents
also give calls and references apart. Recall covers calls only.

The recall breakdown keeps site loss and resolution loss apart.

Extraction loss (the site is not a Stage 3 call site):

| Bucket | Meaning |
|---|---|
| `site_missing` | no structural site at all |
| `site_missing_macro_token_tree` | the call is written inside a macro's token tree and has no site |
| `site_missing_macro_rules_body` | the call is inside a `macro_rules!` body and has no site |
| `site_not_a_call_site` | the site exists only as a typed-only question or a reference, and was unconfirmed |

Resolution loss (the site is a Stage 3 call site):

| Bucket | Meaning |
|---|---|
| `resolution_not_drawn:<band>` | Stage 3 gave a band that is not drawn |
| `resolution_not_materialized:<band>` | Stage 3 gave a drawn band, but Stage 4 cannot persist the site |
| `drawn_wrong` | an edge is drawn to another target |

`end_to_end_recall` divides by every oracle relation, at a parsed site or not.

## A0.2's counterfactuals

These re-score the one A0.1 dataset. The oracle's verdicts and relations do not
change; only which edge a site would draw does.

| Policy | Change |
|---|---|
| C0 | Current behaviour |
| C1 | `exact` is not drawn |
| C2 | `scoped` is not drawn |
| C3 | Neither `exact` nor `scoped` is drawn |
| C4 | A blocklisted site with a valid typed answer draws it as `typed`. Valid means the engine's verdict is typed (`from_engine`) and its target maps to exactly one definition, as Stage 3 maps one |
| C5 | A Rust site in `exact` or `scoped` whose receiver is unproven is not drawn. See below |
| C6 | C3, C4 and C5 together |

**Unproven receiver (C5).** This mirrors the protection Python's extraction gives:
a receiver rooted in `self` or an imported name is trusted.

- **Proven**: the receiver is exactly `self` or `Self`; or it is a path whose first
  part is `crate`, `self`, `super` or `Self`, or a name the file defines or imports
  by name.
- **Unproven**: a value (`x.f()`, `self.a.f()`), or any other path. A name in scope
  only through a glob import counts as unproven.

Each policy reports drawn edges, precision with the oracle-verifiable fraction,
recall, the false positives it removes and the true positives it loses or adds.

## False positives of `exact` and `scoped`

Each is given every category it fits, and the first in this order is its primary:

1. self-edge
2. external receiver mistaken for local (the oracle's target is external or local)
3. constructor collision
4. hierarchy evidence missing (the receiver is `self`, `Self`, `cls`, `super` or
   `this`)
5. import provenance failure (the name or its receiver's first part is bound by an
   import)
6. unknown or unproven receiver
7. same short name on an unrelated type
8. module or package uniqueness only (`scoped`)
9. other

## Reproducing

```
make engine && cargo build -p pdx-bench
python3 scripts/a0/test_a0.py
python3 scripts/a0/run.py --work DIR \
    --scip-python BIN --rust-analyzer BIN --rust-src DIR \
    --toolchain ~/.rustup/toolchains/1.98.1-x86_64-unknown-linux-gnu \
    --self-sha <PDX corpus commit> --resolver "<the resolver measured>"
```

`run.py` checks the sanity fixtures first. These are the cases in
`bench/a0/sanity/`, each marked `A0: <case> -> <expected>`, and every one must pass
before any number is accepted.

The run is deterministic for the same inputs: the same pins, lock files, tool
versions and configuration. Run time is informational only.
