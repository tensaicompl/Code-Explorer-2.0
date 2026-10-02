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
| `0006-resolution-run-record.patch` | What a typed-resolution run loses, made visible to the run (docs/plan/ISSUES.md, issue 21). The cross-file pass writes its record of the files it resolved and skipped into the project's own context. Every failed allocation is counted where it fails (`engine/api/lost_work.h`): in the memory core and the arena, and, through counting versions of the C library's allocators, at each direct allocation in the resolution sources whose failure loses work; allocations that only cost time, or whose failure falls back to the complete answer, are left as they were. Each resolver's per-file work budget counts the first time it runs out; the TypeScript resolver's two budgets are counted by `0007`, which also removes their prints. Definitions, answers and call carriers are copied whole or not at all, so a failure never leaves one half-built, and the registry no longer stores or dereferences a name it failed to copy. A test-only switch, `PDX_ENGINE_TEST_FAIL_ANSWER_ON`, fails the copy of an answer as a failed allocation would. The resolution logic is unchanged. |
| `0005-windows-thread-exit.patch` | On Windows the thread source registers a callback the loader runs as each thread exits, which releases that thread's heap in the allocator. The allocator is not linked here (`scripts/vendor/strip-list.txt`): nothing allocates from it, since every use of it elsewhere sits behind switches this build leaves off. The release was the one use outside those switches, so this puts it behind them. It also makes the callback's slot in the loader's table read-only, as the runtime's own entries are; a writable one makes Microsoft's linker warn that the table's sections disagree. The callback itself stays and still reports the thread's end. |
| `0007-typescript-budget-silent.patch` | The interface writes nothing to standard output or standard error. The TypeScript resolver printed a line to standard error the first time either of its work budgets, for parsing type text and for evaluating expressions, ran out in a file, whatever the logging switches said (docs/plan/ISSUES.md, issue 28). This replaces each print with the count of lost work the run already relies on. The budgets, and evaluation degrading to an unknown type when they run out, are unchanged. |
| `0008-no-offset-from-null.patch` | Found by the sanitizer corpus (`make check-asan`). The C# and Java resolvers build their shared registry from a copy of the project's definitions, which is NULL when the pass kept none, and then take an offset from it; with nothing to register the offset is zero, but an offset from NULL is undefined behaviour in C, which the undefined-behaviour sanitizer rejects. The cross-file pass builds the C# registry whenever extraction found any definitions, so a project whose definitions it keeps none of (a documentation-only one) reached it. This passes NULL on unchanged when there is no array. Java's builder has the same shape and no caller today; it is guarded the same way, so that the first caller does not find the defect again. |

Patches must not overlap. Each one is checked on its own for whether it is already
applied, by trying to reverse it; two patches editing the same lines make that check
fail for whichever was applied first, and the refresh then stops on a tree that is
in fact correct. A change that belongs to the same reason as an existing patch is
merged into it rather than added beside it.
