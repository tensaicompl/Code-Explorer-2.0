"""Write graph_nodes_* / graph_edges_* incrementally —

Hook into build_corpus.py's existing per-file loop; its file-level md5 diffing is already
correct and costs nothing extra.

EDGES CROSS FILE BOUNDARIES, NODES DO NOT. When file X changes, edges INTO X's
symbols from unchanged files may go stale. Two options:
  (a) key edges by source_file, re-resolve only changed sources — leaves dangling
      edges after a rename until the next full pass
  (b) re-run the resolver over the whole edge set after each incremental index —
      a single pure SQL join
RECOMMENDATION: (b). Simpler, cannot drift, and the resolver is the component
whose correctness matters most. Measure it; fall back to (a) only if it
does not fit the refresh window.

Schema: key by project + stream + KIND from day one. The earlier viewer had one graph slot per
project and its satellite skills overwrite each other — do not inherit that. It
costs nothing now and is a migration later #10).

A second trigger path already exists for free: watch-and-index.sh polls
refresh-signal.json every 10s under the `managed-refresh` compose profile and
invokes build_corpus.py [project stream].
"""
