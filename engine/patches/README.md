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
| `0001-standalone-build.patch` | The engine is built here as separate translation units. Upstream compiles it as one, which lets a source use declarations it never includes. This adds the include that the separate build needs. |
| `0003-no-structural-profile.patch` | The structural profile is likewise computed by analysis outside this engine, and its subsystem is not vendored. This removes the call from the same function. |
| `0002-no-similarity-fingerprint.patch` | Near-duplicate detection is an analysis performed outside this engine, over its own token stream, so the subsystem that computed a fingerprint during extraction is not vendored. This removes the call. The body tokens the same function produces are kept, because search depends on them. |
