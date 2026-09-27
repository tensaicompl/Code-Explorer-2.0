/**
 * Citation resolution — the half of the `[[…]]` feature that lives in the browser.
 *
 * The model cites what its tool results contain: a repository-relative path, or
 * a path and a symbol name. It never sees a node id — an id is
 * `{type}:{path}:{name}` and the type comes from a kind→type map only the graph
 * builder knows. So resolution happens here, and these are the cases that decide
 * whether a citation becomes a chip or silently renders as nothing.
 *
 * The silence is the reason this file exists: an unresolved citation used to
 * disappear from the sentence, so the feature could be completely inert without
 * anything looking wrong. It was, for the whole life of the port.
 */
import { describe, expect, it } from "vitest";
import { buildRefIndex, resolveRef } from "../InsightAdvisor";
import type { GraphNode } from "@prx/core/types";

const node = (
  id: string, type: string, name: string, filePath?: string,
): GraphNode =>
  ({ id, type, name, summary: name, tags: [], complexity: "moderate",
     ...(filePath ? { filePath } : {}) }) as GraphNode;

const NODES: GraphNode[] = [
  node("file:src/graph/builder.py", "file", "builder.py", "src/graph/builder.py"),
  node("function:src/graph/builder.py:build_graph", "function", "build_graph",
       "src/graph/builder.py"),
  // Same name, two files: a bare `run` citation must NOT guess between them.
  node("function:src/graph/builder.py:run", "function", "run", "src/graph/builder.py"),
  node("function:src/cli.py:run", "function", "run", "src/cli.py"),
  node("file:src/cli.py", "file", "cli.py", "src/cli.py"),
  node("table:db/schema.sql:customers", "table", "customers", "db/schema.sql"),
  node("service:api/shop.proto:OrderService", "service", "OrderService",
       "api/shop.proto"),
  node("document:README.md:Overview", "document", "Overview", "README.md"),
  node("module:src/graph", "module", "graph", "src/graph"),
];

const index = buildRefIndex(new Map(NODES.map((n) => [n.id, n])));

describe("resolveRef", () => {
  it("takes an exact node id unchanged", () => {
    expect(resolveRef("function:src/graph/builder.py:build_graph", index))
      .toBe("function:src/graph/builder.py:build_graph");
  });

  it("resolves a bare path to that file's node, not a symbol inside it", () => {
    // Both a file node and two function nodes carry this filePath. A citation
    // of the path alone means the file.
    expect(resolveRef("src/graph/builder.py", index))
      .toBe("file:src/graph/builder.py");
  });

  it("resolves path:symbol", () => {
    expect(resolveRef("src/graph/builder.py:build_graph", index))
      .toBe("function:src/graph/builder.py:build_graph");
  });

  it("disambiguates a repeated symbol name by its path", () => {
    expect(resolveRef("src/cli.py:run", index)).toBe("function:src/cli.py:run");
    expect(resolveRef("src/graph/builder.py:run", index))
      .toBe("function:src/graph/builder.py:run");
  });

  it("refuses a bare name that more than one node answers to", () => {
    // Two `run`s. Jumping to either is a coin flip presented as a fact.
    expect(resolveRef("run", index)).toBeNull();
  });

  it("accepts a bare name that is unique graph-wide", () => {
    expect(resolveRef("build_graph", index))
      .toBe("function:src/graph/builder.py:build_graph");
    expect(resolveRef("OrderService", index))
      .toBe("service:api/shop.proto:OrderService");
  });

  it("tolerates the line range find_symbol_references reports", () => {
    expect(resolveRef("src/graph/builder.py:110-140", index))
      .toBe("file:src/graph/builder.py");
  });

  it("tolerates a leading ./ and stray whitespace", () => {
    expect(resolveRef("  ./src/cli.py  ", index)).toBe("file:src/cli.py");
  });

  it("is case-insensitive, because the model retypes paths", () => {
    expect(resolveRef("SRC/CLI.PY", index)).toBe("file:src/cli.py");
  });

  it("resolves the node types added", () => {
    expect(resolveRef("db/schema.sql:customers", index))
      .toBe("table:db/schema.sql:customers");
    expect(resolveRef("api/shop.proto:OrderService", index))
      .toBe("service:api/shop.proto:OrderService");
    expect(resolveRef("README.md:Overview", index))
      .toBe("document:README.md:Overview");
  });

  it("returns null for something that is not in the graph", () => {
    expect(resolveRef("src/nope.py", index)).toBeNull();
    expect(resolveRef("", index)).toBeNull();
  });
});

describe("buildRefIndex", () => {
  it("lets a file node win the path slot regardless of iteration order", () => {
    // Symbols first, file last — the file must still own the bare path.
    const reversed = new Map([...NODES].reverse().map((n) => [n.id, n]));
    expect(resolveRef("src/cli.py", buildRefIndex(reversed)))
      .toBe("file:src/cli.py");
  });

  it("keeps only names that are unique", () => {
    expect(index.byUniqueName.has("run")).toBe(false);
    expect(index.byUniqueName.get("build_graph"))
      .toBe("function:src/graph/builder.py:build_graph");
  });
});
