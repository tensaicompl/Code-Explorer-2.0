import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

export interface ExportMessage {
  role: "user" | "assistant";
  content: string;
  isError?: boolean;
}

export interface ExportMeta {
  /** "cso/develop" — the graph the answers were grounded in. */
  scope: string;
  model: string;
  role: string;
  /** Passed in rather than read from the clock so the caller owns time. */
  exportedAt: string;
}

/**
 * Chat transcript export — Markdown and HTML.
 *
 * A note on a trap that does NOT apply here, so nobody re-introduces the
 * workaround for it: the earlier export scraped rendered Mermaid SVGs out of
 * the live DOM, which meant a collapsed dock silently exported no diagrams.
 * This chat never rasterises anything — `markdownComponents` renders a fenced
 * block as plain `<code>` — so the markdown the model produced IS the whole
 * artefact, and exporting it is lossless. No DOM reads, and the transcript does
 * not need to stay mounted.
 */
export function buildMarkdown(messages: ExportMessage[], meta: ExportMeta): string {
  const head = [
    `# Insight Advisor transcript`,
    ``,
    `- **Codebase:** ${meta.scope}`,
    `- **Model:** ${meta.model}`,
    `- **Role:** ${meta.role}`,
    `- **Exported:** ${meta.exportedAt}`,
    ``,
    `---`,
    ``,
  ].join("\n");

  const body = messages
    .map((m) => {
      const who = m.role === "user" ? "## You" : m.isError ? "## Insight Advisor (error)" : "## Insight Advisor";
      return `${who}\n\n${m.content.trim()}\n`;
    })
    .join("\n");

  return `${head}${body}`;
}

const HTML_STYLE = `
:root { color-scheme: dark; }
body { margin:0; padding:2rem 1.25rem; background:#071D24; color:#D6E2E6;
       font-family:Inter,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;
       line-height:1.6; }
main { max-width:820px; margin:0 auto; }
h1 { font-weight:300; letter-spacing:-0.01em; font-size:1.6rem; margin:0 0 1rem; }
.meta { font-size:0.8rem; color:#7C949B; border-bottom:1px solid #17323B;
        padding-bottom:1rem; margin-bottom:2rem; }
.meta div { margin:0.15rem 0; }
.turn { margin-bottom:1.75rem; }
.who { font-size:0.7rem; text-transform:uppercase; letter-spacing:0.1em;
       color:#76C0A1; margin-bottom:0.5rem; }
.turn.user .who { color:#7C949B; }
.turn.error .who { color:#F26224; }
.bubble { background:#0C2831; border:1px solid #17323B; padding:0.9rem 1.1rem;
          clip-path:polygon(8px 0,100% 0,100% calc(100% - 8px),calc(100% - 8px) 100%,0 100%,0 8px); }
.turn.user .bubble { background:rgba(118,192,161,0.10); }
code { font-family:"JetBrains Mono",ui-monospace,SFMono-Regular,Menlo,monospace;
       font-size:0.85em; background:#071D24; padding:0.1em 0.35em; }
pre, code.block { display:block; background:#071D24; border:1px solid #17323B;
                  padding:0.7rem 0.9rem; overflow-x:auto; margin:0.75rem 0; }
a { color:#76C0A1; }
table { border-collapse:collapse; margin:0.75rem 0; }
th, td { border:1px solid #17323B; padding:0.35rem 0.6rem; text-align:left; }
`;

const escapeHtml = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/**
 * Self-contained HTML, no external requests.
 *
 * The markdown is rendered through the SAME `react-markdown` pipeline the chat
 * uses on screen, via `react-dom/server`, so an exported answer cannot drift
 * from what the user actually read. That module is imported dynamically: export
 * is a rare, user-initiated action and there is no reason to carry a server
 * renderer in the main bundle.
 */
export async function buildHtml(
  messages: ExportMessage[],
  meta: ExportMeta,
): Promise<string> {
  const { renderToStaticMarkup } = await import("react-dom/server");

  const turns = messages
    .map((m) => {
      const cls = m.role === "user" ? "turn user" : m.isError ? "turn error" : "turn";
      const who = m.role === "user" ? "You" : m.isError ? "Insight Advisor — error" : "Insight Advisor";
      const rendered = renderToStaticMarkup(
        <ReactMarkdown remarkPlugins={[remarkGfm]}>{m.content}</ReactMarkdown>,
      );
      return `<div class="${cls}"><div class="who">${who}</div><div class="bubble">${rendered}</div></div>`;
    })
    .join("\n");

  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Insight Advisor — ${escapeHtml(meta.scope)}</title>
<style>${HTML_STYLE}</style>
</head>
<body>
<main>
<h1>Insight Advisor transcript</h1>
<div class="meta">
<div><strong>Codebase:</strong> ${escapeHtml(meta.scope)}</div>
<div><strong>Model:</strong> ${escapeHtml(meta.model)}</div>
<div><strong>Role:</strong> ${escapeHtml(meta.role)}</div>
<div><strong>Exported:</strong> ${escapeHtml(meta.exportedAt)}</div>
</div>
${turns}
</main>
</body>
</html>`;
}

/** Trigger a browser download for an in-memory string. */
export function downloadText(filename: string, mime: string, content: string): void {
  const url = URL.createObjectURL(new Blob([content], { type: `${mime};charset=utf-8` }));
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  // Revoking immediately can cancel the download in some browsers; one tick is
  // enough for the navigation to have started.
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

/** `praxevia-myproject-main-2026-08-03.md` — safe on every filesystem. */
export function exportFilename(scope: string, ext: string, stamp: string): string {
  const slug = scope.replace(/[^a-zA-Z0-9]+/g, "-").replace(/^-|-$/g, "").toLowerCase();
  return `praxevia-${slug || "chat"}-${stamp}.${ext}`;
}
