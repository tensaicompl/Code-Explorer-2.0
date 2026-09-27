import { describe, expect, it } from "vitest";
import { buildAncestry, shortenDir, crumbLabel } from "../ancestry";
import type { KnowledgeGraph, GraphNode } from "@prx/core/types";

const n = (id: string, type: string, name: string, filePath?: string) =>
  ({ id, type, name, summary: name, tags: [], complexity: "moderate", filePath }) as GraphNode;

const e = (source: string, target: string) =>
  ({ source, target, type: "contains", direction: "forward", weight: 1 }) as never;

/** Mirrors the real shape: a method contained by BOTH its file and its class. */
const GRAPH = {
  version: "1.0.0",
  kind: "codebase",
  project: {
    name: "demo", languages: [], frameworks: [],
    description: "", analyzedAt: "", gitCommitHash: "",
  },
  nodes: [
    n("module:a/b/c/d", "module", "d", "a/b/c/d"),
    n("file:a/b/c/d/Foo.java", "file", "Foo.java", "a/b/c/d/Foo.java"),
    n("class:a/b/c/d/Foo.java:Foo", "class", "Foo", "a/b/c/d/Foo.java"),
    n("function:a/b/c/d/Foo.java:Foo.bar", "function", "bar", "a/b/c/d/Foo.java"),
    n("file:root.txt", "file", "root.txt", "root.txt"),
  ],
  edges: [
    e("module:a/b/c/d", "file:a/b/c/d/Foo.java"),
    e("file:a/b/c/d/Foo.java", "class:a/b/c/d/Foo.java:Foo"),
    e("file:a/b/c/d/Foo.java", "function:a/b/c/d/Foo.java:Foo.bar"),
    e("class:a/b/c/d/Foo.java:Foo", "function:a/b/c/d/Foo.java:Foo.bar"),
  ],
  layers: [{
    id: "layer:backend", name: "backend", description: "",
    nodeIds: [
      "module:a/b/c/d", "file:a/b/c/d/Foo.java",
      "class:a/b/c/d/Foo.java:Foo", "function:a/b/c/d/Foo.java:Foo.bar",
      "file:root.txt",
    ],
  }],
  tour: [],
} as unknown as KnowledgeGraph;

const nodesById = new Map(GRAPH.nodes.map((x) => [x.id, x]));
const layerOf = new Map(GRAPH.layers[0].nodeIds.map((id) => [id, "layer:backend"]));
const parents = new Map<string, string[]>();
for (const edge of GRAPH.edges) {
  const arr = parents.get(edge.target);
  if (arr) arr.push(edge.source);
  else parents.set(edge.target, [edge.source]);
}

const build = (id: string) => buildAncestry(GRAPH, id, nodesById, layerOf, parents);

describe("buildAncestry", () => {
  it("prefers the deepest parent so the class is not skipped", () => {
    // The method has TWO contains parents, file and class. Taking whichever
    // came first would drop the class — the crumb that tells two identically
    // named methods in one file apart.
    const trail = build("function:a/b/c/d/Foo.java:Foo.bar");
    expect(trail.map((c) => c.kind)).toEqual([
      "layer", "module", "file", "class", "symbol",
    ]);
    expect(trail.map((c) => c.label)).toEqual([
      "backend", "a/b/c/d", "Foo.java", "Foo", "bar",
    ]);
  });

  it("labels a module with its full path, not its last segment", () => {
    // Module nodes are named `d` but carry `a/b/c/d`. Using `name` would make
    // every leaf directory in the repo look the same.
    const trail = build("file:a/b/c/d/Foo.java");
    expect(trail.find((c) => c.kind === "module")?.label).toBe("a/b/c/d");
  });

  it("always starts at the layer", () => {
    expect(build("file:root.txt")[0]).toMatchObject({
      kind: "layer", label: "backend",
    });
  });

  it("returns nothing for an unknown node instead of throwing", () => {
    expect(build("nope")).toEqual([]);
  });

  it("terminates on a containment cycle", () => {
    const cyc = new Map(parents);
    cyc.set("module:a/b/c/d", ["file:a/b/c/d/Foo.java"]); // d contains Foo contains d
    expect(() =>
      buildAncestry(GRAPH, "function:a/b/c/d/Foo.java:Foo.bar", nodesById, layerOf, cyc),
    ).not.toThrow();
  });
});

describe("shortenDir", () => {
  it("keeps the tail, which is the part that differentiates", () => {
    expect(shortenDir("backend/src/main/java/eu/eurocontrol/cap/docgen/domain"))
      .toBe("…/docgen/domain");
  });

  it("leaves short paths alone", () => {
    expect(shortenDir("backend")).toBe("backend");
    expect(shortenDir("a/b")).toBe("a/b");
  });

  it("only abbreviates directory crumbs", () => {
    const trail = build("function:a/b/c/d/Foo.java:Foo.bar");
    const byKind = Object.fromEntries(trail.map((c) => [c.kind, crumbLabel(c)]));
    expect(byKind.module).toBe("…/c/d");
    expect(byKind.file).toBe("Foo.java");   // never shortened
    expect(byKind.class).toBe("Foo");
    expect(byKind.symbol).toBe("bar");
  });
});
