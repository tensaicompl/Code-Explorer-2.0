import type { KnowledgeGraph, GraphNode } from "@prx/core/types";

export interface Crumb {
  /** Node id, or a layer id for the first crumb. Always navigable. */
  id: string;
  label: string;
  kind: "layer" | "module" | "file" | "class" | "symbol";
  /** Full, untruncated text for a tooltip. */
  title: string;
}

/**
 * The containment path from a node's layer down to the node itself.
 *
 * Two things make this less trivial than walking one parent pointer:
 *
 * 1. A node can have MORE THAN ONE `contains` parent. A Java method is
 *    contained by both its file and its class — measured on the real cso
 *    index, 2,177 of 5,033 nodes have two. Taking the first one found drops
 *    the class, which is exactly the crumb that distinguishes two identically
 *    named methods in the same file. We pick the DEEPEST parent, so the chain
 *    comes out file -> class -> method rather than file -> method.
 *
 * 2. Module nodes exist per directory but are named after the last segment
 *    only (`domain`), while carrying the full path in `filePath`. The label
 *    therefore comes from `filePath`, not from `name`.
 */
export function buildAncestry(
  graph: KnowledgeGraph,
  nodeId: string,
  nodesById: Map<string, GraphNode>,
  layerOf: Map<string, string>,
  parents: Map<string, string[]>,
): Crumb[] {
  const node = nodesById.get(nodeId);
  if (!node) return [];

  // Walk up, choosing the deepest parent at each step. Depth is measured by
  // how far that parent is from a root, which is what makes `class` win over
  // `file` for a method: the class is itself contained by the file.
  const depthCache = new Map<string, number>();
  const depthOf = (id: string, guard = 0): number => {
    if (guard > 32) return guard; // containment cycle; refuse to spin
    const hit = depthCache.get(id);
    if (hit !== undefined) return hit;
    const ps = parents.get(id) ?? [];
    const d = ps.length === 0 ? 0 : 1 + Math.max(...ps.map((p) => depthOf(p, guard + 1)));
    depthCache.set(id, d);
    return d;
  };

  const chain: GraphNode[] = [];
  const seen = new Set<string>();
  let cur: string | undefined = nodeId;
  while (cur && !seen.has(cur)) {
    seen.add(cur);
    const n = nodesById.get(cur);
    if (n) chain.push(n);
    // Annotated, not inferred: `cur` is reassigned from `ps` on the next line,
    // so leaving `ps` to inference makes its type depend on itself through the
    // loop's back-edge and TS gives up (TS7022), taking `a`/`b` below down with
    // it. One annotation breaks the cycle.
    const ps: string[] = (parents.get(cur) ?? []).filter((p) => !seen.has(p));
    if (ps.length === 0) break;
    cur = ps.reduce((a, b) => (depthOf(b) > depthOf(a) ? b : a));
  }
  chain.reverse();

  const crumbs: Crumb[] = [];

  const layerId = layerOf.get(nodeId) ?? layerOf.get(chain[0]?.id ?? "");
  if (layerId) {
    const layer = graph.layers.find((l) => l.id === layerId);
    if (layer) {
      crumbs.push({ id: layer.id, label: layer.name, kind: "layer", title: layer.name });
    }
  }

  for (const n of chain) {
    // The layer crumb already stands for the whole subtree; a module whose
    // path IS the layer would just repeat it.
    if (n.type === "module" && n.filePath && crumbs.length === 1 &&
        n.filePath === crumbs[0].label) continue;

    const kind: Crumb["kind"] =
      n.type === "module" ? "module"
      : n.type === "file" ? "file"
      : n.type === "class" ? "class"
      : "symbol";

    const full = kind === "module" ? (n.filePath ?? n.name) : n.name;
    crumbs.push({ id: n.id, label: full, kind, title: n.filePath ?? n.name });
  }

  return crumbs;
}

/**
 * Shorten a directory crumb to its last `keep` segments.
 *
 * `backend/src/main/java/eu/eurocontrol/` is identical for every file in the
 * repository, so it costs width without telling the reader anything. The tail
 * is what differentiates. Files, classes and symbols are never shortened —
 * they are the part that identifies the result.
 */
export function shortenDir(path: string, keep = 2): string {
  const parts = path.split("/").filter(Boolean);
  if (parts.length <= keep) return path;
  return `…/${parts.slice(-keep).join("/")}`;
}

/** The crumb label as displayed: directories abbreviated, everything else whole. */
export function crumbLabel(c: Crumb): string {
  return c.kind === "module" ? shortenDir(c.label) : c.label;
}
