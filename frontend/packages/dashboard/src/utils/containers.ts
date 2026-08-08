import type {
  GraphNode,
  GraphEdge,
} from "@prx/core/types";
import { detectCommunities } from "./louvain";

export interface DerivedContainer {
  id: string;
  name: string;
  nodeIds: string[];
  strategy: "folder" | "community";
}

export interface DeriveResult {
  containers: DerivedContainer[];
  ungrouped: string[];
}

const MIN_BUCKET_COUNT = 2;
const MAX_CONCENTRATION = 0.7;
const MIN_NODES_FOR_SUPPRESSION = 3;
const ROOT_BUCKET = "~";

/**
 * Longest common prefix of the *directory* portion of paths, trimmed to a
 * `/` boundary. Using dirs (not full paths) avoids consuming the only
 * folder segment when all paths sit directly under the same folder
 * (e.g. `[auth/x, auth/y]` → LCP `""`, so we still group on `auth`).
 */
function commonPrefix(paths: string[]): string {
  if (paths.length === 0) return "";
  const dirs = paths.map((p) => {
    const slash = p.lastIndexOf("/");
    return slash >= 0 ? p.slice(0, slash) : "";
  });
  let prefix = dirs[0];
  for (const d of dirs) {
    while (!d.startsWith(prefix)) {
      prefix = prefix.slice(0, -1);
      if (!prefix) return "";
    }
  }
  const lastSlash = prefix.lastIndexOf("/");
  return lastSlash >= 0 ? prefix.slice(0, lastSlash + 1) : "";
}

function firstSegments(path: string, depth: number): string {
  const parts = path.split("/");
  // The last part is the file name, never a folder.
  return parts.slice(0, Math.min(depth, parts.length - 1)).join("/");
}

/**
 * How deep a folder path may be consumed before giving up on folder grouping.
 *
 * One segment is not always enough. A deep, uniform tree — `src/main/java/com/
 * <org>/<product>/...`, the standard Java layout — has its whole shared trunk
 * eaten by the common prefix, after which the next single segment is the same
 * for nearly every file. That lands one bucket above MAX_CONCENTRATION and
 * drops the layer into community detection, whose entire objective is to
 * maximise edges *inside* a community. The result renders as boxes with no
 * lines between them: measured on the real cso index, the `backend` layer gave
 * 47 clusters and **zero** drawable connections.
 *
 * Descending a level at a time keeps the grouping something a reader can name
 * (`service`, `controller`, `repository`) and only reaches for communities when
 * no folder depth separates the nodes at all. Ten levels because a Java tree
 * spends seven on `src/main/java/com/<org>/<product>/` before saying anything.
 */
const MAX_FOLDER_DEPTH = 10;

function groupByFolder(
  nodes: GraphNode[],
  depth = 1,
): { groups: Map<string, string[]>; rooted: string[] } {
  const withPath = nodes.filter((n) => n.filePath);
  const lcp = commonPrefix(withPath.map((n) => n.filePath!));
  const groups = new Map<string, string[]>();
  const rooted: string[] = [];
  for (const n of nodes) {
    if (!n.filePath) {
      rooted.push(n.id);
      continue;
    }
    const stripped = n.filePath.slice(lcp.length);
    if (!stripped.includes("/")) {
      rooted.push(n.id);
      continue;
    }
    const seg = firstSegments(stripped, depth);
    const arr = groups.get(seg) ?? [];
    arr.push(n.id);
    groups.set(seg, arr);
  }
  return { groups, rooted };
}

function shouldFallbackToCommunity(
  groups: Map<string, string[]>,
  rooted: string[],
  totalNodes: number,
): boolean {
  const bucketCount = groups.size + (rooted.length > 0 ? 1 : 0);
  if (bucketCount < MIN_BUCKET_COUNT) return true;
  for (const ids of groups.values()) {
    if (ids.length / totalNodes > MAX_CONCENTRATION) return true;
  }
  if (rooted.length / totalNodes > MAX_CONCENTRATION) return true;
  return false;
}

export function deriveContainers(
  nodes: GraphNode[],
  edges: GraphEdge[],
): DeriveResult {
  if (nodes.length === 0) {
    return { containers: [], ungrouped: [] };
  }

  // Descend until the folders actually separate the nodes. Each level is a
  // strictly finer partition, so the first one that passes is the coarsest
  // grouping that still says something.
  let { groups, rooted } = groupByFolder(nodes, 1);
  for (
    let depth = 2;
    depth <= MAX_FOLDER_DEPTH &&
    shouldFallbackToCommunity(groups, rooted, nodes.length);
    depth++
  ) {
    const deeper = groupByFolder(nodes, depth);
    // A deeper split that separates nothing new means the tree has run out of
    // structure; keep the shallower, more readable grouping.
    if (deeper.groups.size <= groups.size) break;
    groups = deeper.groups;
    rooted = deeper.rooted;
  }

  const useCommunity = shouldFallbackToCommunity(groups, rooted, nodes.length);
  let containers: DerivedContainer[];

  if (useCommunity) {
    const communities = detectCommunities(
      nodes.map((n) => n.id),
      edges,
    );
    const byCommunity = new Map<number, string[]>();
    for (const [nodeId, cid] of communities) {
      const arr = byCommunity.get(cid) ?? [];
      arr.push(nodeId);
      byCommunity.set(cid, arr);
    }
    const sorted = [...byCommunity.entries()].sort((a, b) => a[0] - b[0]);
    containers = sorted.map(([cid, ids], i) => ({
      id: `container:cluster-${cid}`,
      // A-Z for the first 26, then numeric. Avoids `String.fromCharCode(65+i)`
      // wrapping into `[`, `\`, `]` ... once the cluster count exceeds 26.
      name: i < 26 ? `Cluster ${String.fromCharCode(65 + i)}` : `Cluster ${i + 1}`,
      nodeIds: ids,
      strategy: "community" as const,
    }));
  } else {
    containers = [...groups.entries()].map(([seg, ids]) => ({
      id: `container:${seg}`,
      name: seg,
      nodeIds: ids,
      strategy: "folder" as const,
    }));
    if (rooted.length > 0) {
      containers.push({
        id: `container:${ROOT_BUCKET}`,
        name: ROOT_BUCKET,
        nodeIds: rooted,
        strategy: "folder" as const,
      });
    }
  }

  // Suppress single-child containers (their child becomes ungrouped).
  // Skip suppression for tiny layers — with so few nodes, even single-item
  // boxes carry useful folder context that shouldn't be discarded.
  const ungrouped: string[] = [];
  if (nodes.length >= MIN_NODES_FOR_SUPPRESSION) {
    containers = containers.filter((c) => {
      if (c.nodeIds.length === 1) {
        ungrouped.push(c.nodeIds[0]);
        return false;
      }
      return true;
    });
  }

  return { containers, ungrouped };
}
