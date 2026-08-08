import { describe, it, expect } from "vitest";

import { rankFlowSteps } from "../domainOrder";
import type { GraphEdge } from "@prx/core/types";

const fs = (source: string, target: string, weight: number): GraphEdge => ({
  source,
  target,
  type: "flow_step",
  direction: "forward",
  weight,
});

/** The producer's encoding: weight_i = i / (N + 1), never rounded. */
const encode = (i: number, n: number) => i / (n + 1);

describe("rankFlowSteps", () => {
  it("ranks one flow's steps 1..N in ascending weight order", () => {
    const order = rankFlowSteps([
      fs("flow:a", "step:3", encode(3, 3)),
      fs("flow:a", "step:1", encode(1, 3)),
      fs("flow:a", "step:2", encode(2, 3)),
    ]);
    expect(order.get("step:1")).toBe(1);
    expect(order.get("step:2")).toBe(2);
    expect(order.get("step:3")).toBe(3);
  });

  it("keeps flows separate — the trap the old magnitude decode never hit", () => {
    // DomainGraphView passes every flow in the active domain as one array.
    // Ranking it whole would number these 1..4 instead of 1..2 twice.
    const order = rankFlowSteps([
      fs("flow:a", "step:a1", encode(1, 2)),
      fs("flow:a", "step:a2", encode(2, 2)),
      fs("flow:b", "step:b1", encode(1, 2)),
      fs("flow:b", "step:b2", encode(2, 2)),
    ]);
    expect(order.get("step:a1")).toBe(1);
    expect(order.get("step:a2")).toBe(2);
    expect(order.get("step:b1")).toBe(1);
    expect(order.get("step:b2")).toBe(2);
  });

  it.each([1, 3, 5, 15, 40])(
    "round-trips the producer's encoding for N=%i, including past the old 10-step ceiling",
    (n) => {
      const edges = Array.from({ length: n }, (_, k) =>
        fs("flow:a", `step:${k + 1}`, encode(k + 1, n)),
      );
      // Shuffled deterministically — rank must come from weight, not array order.
      const shuffled = [...edges].reverse();
      const order = rankFlowSteps(shuffled);
      for (let i = 1; i <= n; i++) {
        expect(order.get(`step:${i}`)).toBe(i);
      }
      expect(new Set(order.values()).size).toBe(n);
    },
  );

  it("keeps every encoded weight strictly inside the schema's [0,1] bound", () => {
    for (const n of [1, 3, 5, 15, 40]) {
      for (let i = 1; i <= n; i++) {
        const w = encode(i, n);
        expect(w).toBeGreaterThan(0);
        expect(w).toBeLessThan(1);
      }
    }
  });

  it("degrades to stable edge order on ties rather than colliding", () => {
    const order = rankFlowSteps([
      fs("flow:a", "step:x", 0.5),
      fs("flow:a", "step:y", 0.5),
    ]);
    expect(order.get("step:x")).toBe(1);
    expect(order.get("step:y")).toBe(2);
  });

  it("returns an empty map for a flow with no steps", () => {
    expect(rankFlowSteps([]).size).toBe(0);
  });
});
