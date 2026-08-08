import { useEffect, useId, useRef, useState } from "react";

import { useTheme } from "../themes/index.ts";

/**
 * Renders a ```mermaid fence from the Advisor as an actual diagram.
 *
 * WHY THIS DID NOT EXIST. The backend has always asked for Mermaid — the chat
 * request carries `diagram_mode` (InsightAdvisor.tsx:380) and the system prompt
 * instructs the model accordingly — and the Advisor has always offered a
 * Mermaid/Text toggle. But `mermaid` was never a dependency, and the markdown
 * `code` override treated every `language-*` fence as a plain block. So the
 * model produced correct Mermaid and the UI printed it as monospace text: the
 * whole pipeline worked except the last step.
 *
 * FAILURE MUST NOT BE FATAL. This renders model output, and a model emits
 * invalid diagram syntax often enough that it has to be an expected state, not
 * an exception. `mermaid.parse` is asked first, and anything that throws falls
 * back to showing the source — which is exactly what the user saw before, so
 * the worst case is the old behaviour rather than a broken chat.
 *
 * LOADED ON DEMAND. Mermaid is ~500 KB. It is imported inside the effect rather
 * than at module scope so it is fetched the first time a diagram actually
 * appears, and never for a conversation that has none.
 */
export default function MermaidDiagram({ source }: { source: string }) {
  const { preset } = useTheme();
  const [svg, setSvg] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  // Mermaid needs a DOM id unique per diagram; two with the same id collide.
  const reactId = useId();
  const domId = `mermaid-${reactId.replace(/[^a-zA-Z0-9]/g, "")}`;

  useEffect(() => {
    let cancelled = false;
    setFailed(false);
    setSvg(null);

    (async () => {
      try {
        const mermaid = (await import("mermaid")).default;
        mermaid.initialize({
          startOnLoad: false,
          // `securityLevel: strict` keeps model-authored labels from becoming
          // markup — this is untrusted text in the same sense as any other
          // model output.
          securityLevel: "strict",
          theme: preset.isDark ? "dark" : "default",
          fontFamily: "inherit",
        });
        // Parse first: render() on invalid input can leave orphan nodes in the
        // document, and we would rather find out before it draws anything.
        await mermaid.parse(source);
        const { svg: rendered } = await mermaid.render(domId, source);
        if (!cancelled) setSvg(rendered);
      } catch {
        if (!cancelled) setFailed(true);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [source, preset.isDark, domId]);

  if (failed) {
    // The diagram could not be drawn. Showing the source beats showing nothing:
    // it is still the answer, just not a picture.
    return (
      <pre className="block bg-root/60 border border-border-subtle bevel-sm px-2 py-1.5 my-1.5 overflow-x-auto text-[11px] leading-relaxed font-mono whitespace-pre">
        {source}
      </pre>
    );
  }

  if (!svg) {
    return (
      <div className="my-1.5 px-2 py-3 text-[11px] text-text-muted flex items-center gap-2">
        <span className="w-3 h-3 border-2 border-accent/30 border-t-accent rounded-full animate-spin" />
        Drawing diagram…
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      // The chat panel is narrow and a flowchart is not. Scrolling the diagram
      // inside its own box keeps it from forcing the whole conversation wide.
      className="my-2 overflow-x-auto bg-root/40 border border-border-subtle bevel-sm p-2 [&_svg]:max-w-none"
      // Sanitised by mermaid itself under securityLevel: "strict".
      dangerouslySetInnerHTML={{ __html: svg }}
    />
  );
}
