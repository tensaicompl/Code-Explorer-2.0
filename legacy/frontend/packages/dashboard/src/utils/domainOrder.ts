import type { GraphEdge } from "@prx/core/types";

/**
 * Ranks `flow_step` edges within each flow by ascending `weight`, and returns
 * each step's 1-based position in that ranking.
 *
 * `weight` on a `flow_step` edge is a relative ORDER KEY, not a slot number to
 * decode by magnitude. The previous decode, `Math.round(weight * 10)`, has
 * exactly eleven possible outputs (0..10) no matter what the producer emits, so
 * it caps a flow at ten steps by construction — an eleventh step collides onto
 * a number already taken and the two render with the same position.
 *
 * Reading order by RANK instead means this function never has to know what
 * scale the producer used, so the producer is free to choose one that cannot
 * collide. It also degrades gracefully on ties: `Array.prototype.sort` is
 * stable (guaranteed by ECMA-262 since ES2019), so equal weights fall back to
 * edge-array order rather than producing duplicate positions.
 *
 * Edges are grouped by `source` (the flow) BEFORE ranking. This is the one trap
 * a naive version falls into: DomainGraphView's domain-detail view builds a
 * single `stepEdges` array spanning every flow in the active domain, so ranking
 * it whole would interleave two different flows' step numbers.
 */
export function rankFlowSteps(stepEdges: GraphEdge[]): Map<string, number> {
  const byFlow = new Map<string, GraphEdge[]>();
  for (const edge of stepEdges) {
    const bucket = byFlow.get(edge.source);
    if (bucket) bucket.push(edge);
    else byFlow.set(edge.source, [edge]);
  }

  const order = new Map<string, number>();
  for (const edges of byFlow.values()) {
    const sorted = [...edges].sort((a, b) => a.weight - b.weight);
    sorted.forEach((edge, i) => order.set(edge.target, i + 1));
  }
  return order;
}
