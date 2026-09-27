# ada-indexer

Emits a compiler-grade index for Ada sources, so that the graph can carry
compiler-confirmed relationships for Ada as it does for the languages with an
off-the-shelf indexer.

Kept as a separate process and a separate distribution for two reasons that hold
regardless of licensing: the toolchain it binds to is large and versioned
independently, and a crash in it must not take the indexing pipeline with it. It
runs with no network access.

The implementation arrives in the precise-band phase.
