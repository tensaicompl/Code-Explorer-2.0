import type { GraphEdge, KnowledgeGraph } from "@prx/core/types";

/**
 * Project every semantic edge onto the nodes currently on screen.
 *
 * `calls` edges are symbol-to-symbol. The layer view defaults to file
 * granularity, and no call edge ever connects two *files*, so filtering edges
 * to the visible node set keeps none of them: measured on the real cso index,
 * opening the `backend` layer gave 48 container nodes and **0** edges. Same for
 * the overview one level up, for the same reason.
 *
 * Lifting walks each endpoint up the `contains` tree until it reaches a node
 * that is actually visible, which turns "these two functions call each other"
 * into "this file depends on that one". Results are deduplicated per
 * (source, target, type); `weight` carries the number of underlying calls,
 * saturating at 10 so one hot pair cannot flatten every other edge.
 *
 * `contains` is never lifted — it would self-loop by construction, and the
 * hierarchy is already expressed by container nesting.
 */
export function liftEdgesToVisible(
  graph: KnowledgeGraph,
  visibleNodeIds: Set<string>,
  keepType: (edgeType: string) => boolean,
): GraphEdge[] {
  const parent = new Map<string, string>();
  for (const e of graph.edges) {
    if (e.type === "contains" && !parent.has(e.target)) {
      parent.set(e.target, e.source);
    }
  }

  const lift = (id: string): string | null => {
    let cur: string | undefined = id;
    // Bounded by the containment depth (module > file > symbol). The cap is a
    // cycle guard, not a depth assumption.
    for (let hops = 0; cur !== undefined && hops < 16; hops++) {
      if (visibleNodeIds.has(cur)) return cur;
      cur = parent.get(cur);
    }
    return null;
  };

  const agg = new Map<string, { edge: GraphEdge; count: number }>();

  for (const e of graph.edges) {
    if (e.type === "contains") continue;
    if (!keepType(e.type)) continue;
    const src = lift(e.source);
    const tgt = lift(e.target);
    if (src === null || tgt === null || src === tgt) continue;

    const key = `${src.length}:${src} ${tgt.length}:${tgt} ${e.type}`;
    const hit = agg.get(key);
    if (hit) {
      hit.count++;
    } else {
      agg.set(key, {
        count: 1,
        edge: { ...e, source: src, target: tgt },
      });
    }
  }

  return [...agg.values()].map(({ edge, count }) => ({
    ...edge,
    weight: Math.min(1, count / 10),
    description:
      count > 1 ? `${count} ${edge.type}` : (edge.description ?? edge.type),
  }));
}

export interface LayerEdgeAggregation {
  sourceLayerId: string;
  targetLayerId: string;
  count: number;
  edgeTypes: string[];
}

export interface PortalInfo {
  layerId: string;
  layerName: string;
  connectionCount: number;
}

/**
 * Aggregate edges between layers. Counts how many graph edges cross
 * from one layer to another. Only considers edges where both endpoints
 * are assigned to a layer.
 */
export function aggregateLayerEdges(
  graph: KnowledgeGraph,
): LayerEdgeAggregation[] {
  const nodeToLayer = new Map<string, string>();
  for (const layer of graph.layers) {
    for (const nodeId of layer.nodeIds) {
      nodeToLayer.set(nodeId, layer.id);
    }
  }

  // Key: "layerA|layerB" (sorted) → aggregation
  const pairMap = new Map<
    string,
    { sourceLayerId: string; targetLayerId: string; count: number; edgeTypes: Set<string> }
  >();

  for (const edge of graph.edges) {
    const sourceLayer = nodeToLayer.get(edge.source);
    const targetLayer = nodeToLayer.get(edge.target);
    if (!sourceLayer || !targetLayer) continue;
    if (sourceLayer === targetLayer) continue;

    // Canonical key so A→B and B→A merge
    const [a, b] =
      sourceLayer < targetLayer
        ? [sourceLayer, targetLayer]
        : [targetLayer, sourceLayer];
    const key = `${a}|${b}`;

    const existing = pairMap.get(key);
    if (existing) {
      existing.count++;
      existing.edgeTypes.add(edge.type);
    } else {
      pairMap.set(key, {
        sourceLayerId: a,
        targetLayerId: b,
        count: 1,
        edgeTypes: new Set([edge.type]),
      });
    }
  }

  return Array.from(pairMap.values()).map((p) => ({
    sourceLayerId: p.sourceLayerId,
    targetLayerId: p.targetLayerId,
    count: p.count,
    edgeTypes: Array.from(p.edgeTypes),
  }));
}

/**
 * Compute portal info for a given layer: which other layers are connected
 * and how many edges cross the boundary.
 * Accepts optional pre-computed aggregation to avoid redundant work.
 */
export function computePortals(
  graph: KnowledgeGraph,
  activeLayerId: string,
  precomputed?: LayerEdgeAggregation[],
): PortalInfo[] {
  const aggregated = precomputed ?? aggregateLayerEdges(graph);
  const layerNameMap = new Map(graph.layers.map((l) => [l.id, l.name]));

  const portalMap = new Map<string, number>();

  for (const agg of aggregated) {
    if (agg.sourceLayerId === activeLayerId) {
      portalMap.set(
        agg.targetLayerId,
        (portalMap.get(agg.targetLayerId) ?? 0) + agg.count,
      );
    } else if (agg.targetLayerId === activeLayerId) {
      portalMap.set(
        agg.sourceLayerId,
        (portalMap.get(agg.sourceLayerId) ?? 0) + agg.count,
      );
    }
  }

  return Array.from(portalMap.entries()).map(([layerId, count]) => ({
    layerId,
    layerName: layerNameMap.get(layerId) ?? layerId,
    connectionCount: count,
  }));
}

/**
 * For a given layer, find which file nodes in that layer connect to a
 * specific external layer. Returns the set of node IDs in activeLayer
 * that have edges crossing to targetLayerId.
 */
export function findCrossLayerFileNodes(
  graph: KnowledgeGraph,
  activeLayerId: string,
  targetLayerId: string,
): Set<string> {
  const activeNodeIds = new Set(
    graph.layers.find((l) => l.id === activeLayerId)?.nodeIds ?? [],
  );
  const targetNodeIds = new Set(
    graph.layers.find((l) => l.id === targetLayerId)?.nodeIds ?? [],
  );

  const result = new Set<string>();
  for (const edge of graph.edges) {
    if (activeNodeIds.has(edge.source) && targetNodeIds.has(edge.target)) {
      result.add(edge.source);
    }
    if (activeNodeIds.has(edge.target) && targetNodeIds.has(edge.source)) {
      result.add(edge.target);
    }
  }
  return result;
}

export interface AggregatedContainerEdge {
  sourceContainerId: string;
  targetContainerId: string;
  count: number;
  edgeTypes: string[];
}

export interface ContainerEdgeBuckets {
  intraContainer: GraphEdge[];
  interContainerAggregated: AggregatedContainerEdge[];
}

/**
 * Bucket edges into intra-container (preserved) and inter-container
 * (aggregated by directed (source,target) container pair).
 *
 * Direction is significant: A→B and B→A produce two independent
 * aggregated edges. Edges whose source or target has no entry in
 * `nodeToContainer` are silently dropped; callers that need a strict
 * mode should pre-filter.
 */
export function aggregateContainerEdges(
  edges: GraphEdge[],
  nodeToContainer: Map<string, string>,
): ContainerEdgeBuckets {
  const intra: GraphEdge[] = [];
  const interMap = new Map<
    string,
    {
      sourceContainerId: string;
      targetContainerId: string;
      count: number;
      edgeTypes: Set<string>;
    }
  >();

  for (const e of edges) {
    const sc = nodeToContainer.get(e.source);
    const tc = nodeToContainer.get(e.target);
    if (!sc || !tc) continue;
    if (sc === tc) {
      intra.push(e);
      continue;
    }
    // Length-prefix the source so container ids containing the separator
    // can't produce key collisions ("X Y" + "Z" vs "X" + "Y Z").
    const key = `${sc.length}:${sc}\0${tc}`;
    const existing = interMap.get(key);
    if (existing) {
      existing.count++;
      existing.edgeTypes.add(e.type);
    } else {
      interMap.set(key, {
        sourceContainerId: sc,
        targetContainerId: tc,
        count: 1,
        edgeTypes: new Set([e.type]),
      });
    }
  }

  const interContainerAggregated: AggregatedContainerEdge[] = [...interMap.values()].map(
    (v) => ({
      sourceContainerId: v.sourceContainerId,
      targetContainerId: v.targetContainerId,
      count: v.count,
      edgeTypes: [...v.edgeTypes],
    }),
  );

  return { intraContainer: intra, interContainerAggregated };
}
