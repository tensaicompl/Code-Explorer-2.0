/**
 * Every graph in `.graphs/` must load with ZERO validation issues.
 *
 * This runs `validateGraph` — the same call `App.tsx:215` makes on load — over
 * the real builder output, not a fixture. `realgraph.diag.test.ts` parses those
 * files but never validates them, so nothing else in this repo would notice a
 * builder change that the schema silently repairs.
 *
 * SILENT REPAIR IS THE FAILURE MODE, not rejection. `autoFixGraph` rewrites a
 * missing summary to the node name, a missing type to "file", an unknown
 * complexity to "moderate" — and records an `auto-corrected` issue for each. One
 * such issue per node on a 5 910-node graph is 5 910 entries in WarningBanner,
 * which is how a real warning gets buried (see `builder.py:_node`, which exists
 * precisely because of that). So the assertion is zero issues, not "no errors".
 *
 * Skips itself when no graph has been built, so a clean checkout stays green.
 */
import { describe, expect, it } from "vitest";
import { readFileSync, existsSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { validateGraph } from "@prx/core/schema";

const GRAPH_DIR = process.env.GRAPH_DIR ?? join(__dirname, "../../../../../../.graphs");

const files = existsSync(GRAPH_DIR)
  ? readdirSync(GRAPH_DIR).filter((f) => f.endsWith(".json") && !f.endsWith(".meta.json"))
  : [];

describe.skipIf(files.length === 0)("built graphs validate clean", () => {
  it.each(files)("%s", (file) => {
    const raw = JSON.parse(readFileSync(join(GRAPH_DIR, file), "utf8"));
    const result = validateGraph(raw);

    if (result.issues.length) {
      // Group before reporting: 600 identical messages are one defect, and the
      // failure output has to say which.
      const byMessage = new Map<string, number>();
      for (const i of result.issues) {
        const key = `${i.level}/${i.category}: ${i.message.replace(/\[\d+\]/g, "[n]").replace(/\("[^"]*"\)/, '("…")')}`;
        byMessage.set(key, (byMessage.get(key) ?? 0) + 1);
      }
      const summary = [...byMessage.entries()]
        .sort((a, b) => b[1] - a[1])
        .map(([m, n]) => `  ${n}x ${m}`)
        .join("\n");
      throw new Error(`${file}: ${result.issues.length} validation issues\n${summary}`);
    }

    expect(result.fatal).toBeUndefined();
    expect(result.success).toBe(true);
    // The parsed graph must survive, or the dashboard renders nothing.
    expect(result.data?.nodes.length).toBe(raw.nodes.length);
  });
});
