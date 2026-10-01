# Patches

Local changes to the vendored engine live here as patches, never as edits in place.

A refresh at a newer upstream commit re-runs the vendoring scripts, which replace the
sources wholesale, and then re-applies these. A patch that no longer applies stops
the refresh, which is the point: it means upstream changed the code underneath it and
someone must look, rather than the change being silently lost.

`scripts/vendor/apply-patches.sh` applies them in filename order and is idempotent:
a patch already applied is recognised and skipped.

| Patch | Why |
|---|---|
| `0001-standalone-build.patch` | The engine is built here as separate translation units. Upstream compiles it as one, which lets a source use declarations it never includes. This adds the includes the separate build needs, including three that clang rejects as implicit declarations where gcc only warns. |
| `0003-in-memory-sources.patch` | The cross-file resolution pass reads each file's source from disk. The engine interface hands it bytes instead, so this adds a thread-local provider the pass consults before the disk, and changes nothing else in it: the resolution logic runs as it was written. |
| `0002-no-analysis-subsystems.patch` | Two analyses are performed outside this engine: near-duplicate detection and structural profiling. The subsystems that computed their inputs during extraction are not vendored, so this removes both calls. The body tokens the same function produces are kept, because search depends on them. |
| `0004-supplied-crate-manifest.patch` | The cross-file pass reads the repository's root crate manifest from disk to learn its workspace members and dependencies. Reading and parsing manifests is the caller's job here, so the interface supplies the manifest already parsed, and this makes the pass take it in place of the file. The manifest's use is unchanged; with none supplied the pass behaves as the reference does for a repository without one. |
| `0006-resolution-run-record.patch` | The cross-file pass counts the files it resolves and the ones it skips, and knows whether it reached its end and collected the project's definitions, but reports all of it only to the log, which is process-wide. This has it also write that record into the project's own context (`engine/api/pipeline/pipeline_internal.h`), from which the interface reports whether a run did all its work (docs/plan/ISSUES.md, issue 21). The pass's logic is unchanged. |
| `0005-windows-thread-exit.patch` | On Windows the thread source registers a callback the loader runs as each thread exits, which releases that thread's heap in the allocator. The allocator is not linked here (`scripts/vendor/strip-list.txt`): nothing allocates from it, since every use of it elsewhere sits behind switches this build leaves off. The release was the one use outside those switches, so this puts it behind them. It also makes the callback's slot in the loader's table read-only, as the runtime's own entries are; a writable one makes Microsoft's linker warn that the table's sections disagree. The callback itself stays and still reports the thread's end. |

Patches must not overlap. Each one is checked on its own for whether it is already
applied, by trying to reverse it; two patches editing the same lines make that check
fail for whichever was applied first, and the refresh then stops on a tree that is
in fact correct. A change that belongs to the same reason as an existing patch is
merged into it rather than added beside it.
