import { describe, it, expect } from "vitest";

import { validateGraph } from "@prx/core/schema";
import { rankFlowSteps } from "../domainOrder";
import type { GraphEdge } from "@prx/core/types";

import fixture from "./fixtures/business-flow-shop.json";

/**
 * Cross-language contract test.
 *
 * The producer is Python (`backend/app/graph/business_flow_synth.py`) and the
 * consumer is this TypeScript Zod schema. Nothing in either language checks the
 * other, so a field the producer forgets — `summary`, `tags` and `complexity`
 * are all REQUIRED — is not a type error anywhere. It is a node silently
 * dropped by `validateGraph` at runtime, taking its edges with it via the
 * referential-integrity pass, leaving a graph that renders as a few unconnected
 * boxes.
 *
 * The fixture is real output from the deterministic pass over the `shop` graph,
 * regenerated with:
 *
 *     python3 -c "import json,sys; sys.path.insert(0,'backend'); \
 *       from app.graph.business_flow_synth import \
 *         synthesize_business_flow_deterministic as syn; \
 *       json.dump(syn(json.load(open('.graphs/shop_develop.json'))), \
 *         open('<this dir>/fixtures/business-flow-shop.json','w'), indent=1)"
 *
 * `analyzedAt` is pinned by hand afterwards so the fixture does not churn.
 */
describe("deterministic business-flow output", () => {
  const result = validateGraph(fixture);

  it("passes validateGraph with no fatal error", () => {
    expect(result.fatal).toBeUndefined();
    expect(result.success).toBe(true);
  });

  it("drops nothing — no node or edge is silently discarded", () => {
    const dropped = result.issues.filter((i) => i.level === "dropped");
    expect(dropped).toEqual([]);
    expect(result.data!.nodes).toHaveLength(fixture.nodes.length);
    expect(result.data!.edges).toHaveLength(fixture.edges.length);
  });

  it("produces all three domain levels and all three domain edge types", () => {
    const types = new Set(result.data!.nodes.map((n) => n.type));
    expect(types).toContain("domain");
    expect(types).toContain("flow");
    expect(types).toContain("step");

    const edgeTypes = new Set(result.data!.edges.map((e) => e.type));
    expect(edgeTypes).toContain("contains_flow");
    expect(edgeTypes).toContain("flow_step");
  });

  it("wires every flow to a domain and every step to a flow", () => {
    const byId = new Map(result.data!.nodes.map((n) => [n.id, n]));
    const flowIds = new Set(
      result.data!.nodes.filter((n) => n.type === "flow").map((n) => n.id),
    );
    const stepIds = new Set(
      result.data!.nodes.filter((n) => n.type === "step").map((n) => n.id),
    );

    for (const e of result.data!.edges) {
      if (e.type === "contains_flow") {
        expect(byId.get(e.source)?.type).toBe("domain");
        expect(flowIds.has(e.target)).toBe(true);
      }
      if (e.type === "flow_step") {
        expect(flowIds.has(e.source)).toBe(true);
        expect(stepIds.has(e.target)).toBe(true);
      }
    }
  });

  it("orders every flow's steps contiguously from 1, the way the view decodes them", () => {
    const stepEdges = result.data!.edges.filter(
      (e) => e.type === "flow_step",
    ) as GraphEdge[];
    const order = rankFlowSteps(stepEdges);

    const perFlow = new Map<string, number[]>();
    for (const e of stepEdges) {
      const bucket = perFlow.get(e.source) ?? [];
      bucket.push(order.get(e.target)!);
      perFlow.set(e.source, bucket);
    }

    for (const [flow, ranks] of perFlow) {
      const sorted = [...ranks].sort((a, b) => a - b);
      expect(sorted, `flow ${flow}`).toEqual(
        Array.from({ length: ranks.length }, (_, i) => i + 1),
      );
    }
  });
});
