import { describe, expect, it } from "vitest";
import {
  buildHtml, buildMarkdown, exportFilename,
} from "../chatExport";
import type { ExportMessage, ExportMeta } from "../chatExport";

const META: ExportMeta = {
  scope: "myproject/main",
  model: "Opus 5",
  role: "Developer",
  exportedAt: "2026-08-03 21:00:00 UTC",
};

const MESSAGES: ExportMessage[] = [
  { role: "user", content: "Where is `setWordStyle` defined?" },
  {
    role: "assistant",
    content: [
      "It lives in **SopContentBlock.java**.",
      "",
      "```java",
      "public void setWordStyle(String s) { … }",
      "```",
      "",
      "| File | Layer |",
      "| --- | --- |",
      "| SopContentBlock.java | backend |",
    ].join("\n"),
  },
  { role: "assistant", content: "Something went wrong.", isError: true },
];

describe("buildMarkdown", () => {
  it("keeps fenced blocks verbatim", () => {
    const md = buildMarkdown(MESSAGES, META);
    // The whole reason export is lossless here: nothing is rasterised, so the
    // model's own markdown — diagrams included — survives untouched.
    expect(md).toContain("```java");
    expect(md).toContain("public void setWordStyle(String s) { … }");
  });

  it("records the codebase the answers were grounded in", () => {
    const md = buildMarkdown(MESSAGES, META);
    expect(md).toContain("**Codebase:** myproject/main");
    expect(md).toContain("**Model:** Opus 5");
    expect(md).toContain("**Role:** Developer");
  });

  it("distinguishes speakers, and marks errors as errors", () => {
    const md = buildMarkdown(MESSAGES, META);
    expect(md).toContain("## You");
    expect(md).toContain("## Insight Advisor");
    expect(md).toContain("## Insight Advisor (error)");
  });

  it("handles an empty transcript without producing junk", () => {
    const md = buildMarkdown([], META);
    expect(md).toContain("# Insight Advisor transcript");
    expect(md).not.toContain("## You");
  });
});

describe("buildHtml", () => {
  it("renders markdown through the same pipeline as the chat", async () => {
    const html = await buildHtml(MESSAGES, META);
    expect(html).toContain("<strong>SopContentBlock.java</strong>");
    expect(html).toContain("<code");                    // fenced block became code
    expect(html).toContain("<table>");                  // GFM table, so remarkGfm ran
  });

  it("is self-contained — no external requests", async () => {
    const html = await buildHtml(MESSAGES, META);
    expect(html).toContain("<style>");
    // A CSP-less viewer opening this offline must still see it correctly.
    expect(html).not.toMatch(/<link[^>]+href=/i);
    expect(html).not.toMatch(/<script[^>]+src=/i);
    expect(html).not.toMatch(/https?:\/\/(?!www\.w3\.org)/);
  });

  it("escapes metadata so a project name cannot inject markup", async () => {
    const html = await buildHtml([], { ...META, scope: '<img src=x onerror=alert(1)>' });
    expect(html).not.toContain("<img src=x");
    expect(html).toContain("&lt;img src=x");
  });

  it("marks error turns distinctly", async () => {
    const html = await buildHtml(MESSAGES, META);
    expect(html).toContain('class="turn error"');
  });
});

describe("exportFilename", () => {
  it("slugifies the scope so the name is filesystem-safe", () => {
    expect(exportFilename("myproject/main", "md", "2026-08-03"))
      .toBe("praxevia-myproject-main-2026-08-03.md");
  });

  it("falls back when the scope has nothing usable in it", () => {
    expect(exportFilename("///", "html", "2026-08-03"))
      .toBe("praxevia-chat-2026-08-03.html");
  });
});
