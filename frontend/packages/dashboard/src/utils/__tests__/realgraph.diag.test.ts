/**
 * Diagnostic against the REAL built graph, not a fixture.
 *
 * Reproduces the exact pipeline the layer view runs — filter to the layer,
 * derive containers, bucket edges — and reports how many connections survive as
 * drawable inter-container edges. Skips itself when no graph has been built, so
 * it is safe in a clean checkout.
 */
import { describe, expect, it } from "vitest";
import { readFileSync, existsSync } from "node:fs";
import { deriveContainers } from "../containers";
import { aggregateContainerEdges, liftEdgesToVisible } from "../edgeAggregation";
import type { KnowledgeGraph } from "@prx/core/types";

const GRAPH = process.env.DIAG_GRAPH ?? "";
const LAYER = process.env.DIAG_LAYER ?? "layer:backend";

const ONLY = process.env.DIAG_TYPES ? new Set(process.env.DIAG_TYPES.split(",")) : null;
const STRUCTURAL = new Set([
  "file", "function", "class", "module", "concept",
  "config", "document", "service", "table", "endpoint",
  "pipeline", "schema", "resource", "domain", "flow", "step",
]);

describe.skipIf(!GRAPH || !existsSync(GRAPH))("real graph layer view", () => {
  it("reports drawable connections inside a layer", () => {
    const graph: KnowledgeGraph = JSON.parse(readFileSync(GRAPH, "utf8"));
    const layer = graph.layers.find((l) => l.id === LAYER)!;
    const layerIds = new Set(layer.nodeIds);

    const nodes = graph.nodes.filter(
      (n) => layerIds.has(n.id) && STRUCTURAL.has(n.type) && (!ONLY || ONLY.has(n.type)),
    );
    const visible = new Set(nodes.map((n) => n.id));
    const tA = performance.now();
    const edges = liftEdgesToVisible(graph, visible, () => true);
    const tB = performance.now();
    const { containers, ungrouped } = deriveContainers(nodes, edges);
    const tC = performance.now();
    console.log(`  [czas] liftEdgesToVisible ${(tB - tA).toFixed(1)}ms, deriveContainers ${(tC - tB).toFixed(1)}ms`);
    const n2c = new Map<string, string>();
    for (const c of containers) for (const id of c.nodeIds) n2c.set(id, c.id);
    for (const id of ungrouped) n2c.set(id, id);

    const { intraContainer, interContainerAggregated } = aggregateContainerEdges(
      edges,
      n2c,
    );

    console.log(`\n=== ${LAYER} ===`);
    console.log(`  węzły widoczne:        ${nodes.length}`);
    console.log(`  kontenery:             ${containers.length} (+${ungrouped.length} luzem)`);
    console.log(`  krawędzie po lifcie:   ${edges.length}`);
    console.log(`  INTRA (w kontenerze):  ${intraContainer.length}`);
    console.log(`  INTER (rysowane):      ${interContainerAggregated.length}`);
    console.log(
      "  największe kontenery:  " +
        containers
          .slice()
          .sort((a, b) => b.nodeIds.length - a.nodeIds.length)
          .slice(0, 6)
          .map((c) => `${c.id}(${c.nodeIds.length})`)
          .join(", "),
    );

    expect(edges.length).toBeGreaterThan(0);
  });
});
