import { useDashboardStore } from "../store";
import { useI18n } from "../contexts/I18nContext";

import { SourceRow } from "./SourcePanel";

/** Same resolution order as App.tsx — the panel describes the loaded graph. */
function currentScope(): { project: string; stream: string } {
  const p = new URLSearchParams(window.location.search);
  return {
    project: p.get("project") || localStorage.getItem("prx-project") || "default",
    stream: p.get("stream") || localStorage.getItem("prx-stream") || "main",
  };
}

export default function ProjectOverview({ token }: { token?: string | null }) {
  const scope = currentScope();
  const graph = useDashboardStore((s) => s.graph);
  const domainGraph = useDashboardStore((s) => s.domainGraph);
  const isKnowledgeGraph = useDashboardStore((s) => s.isKnowledgeGraph);
  const setViewMode = useDashboardStore((s) => s.setViewMode);
  const businessFlowStatus = useDashboardStore((s) => s.businessFlowStatus);
  const businessFlowError = useDashboardStore((s) => s.businessFlowError);
  const setBusinessFlowModalOpen = useDashboardStore((s) => s.setBusinessFlowModalOpen);
  const { t } = useI18n();

  if (!graph) {
    return (
      <div className="w-full flex items-center justify-center py-16">
        <p className="text-text-muted text-sm">{t.common.loading}</p>
      </div>
    );
  }

  const { project, nodes, edges, layers } = graph;

  const typeCounts: Record<string, number> = {};
  for (const node of nodes) {
    typeCounts[node.type] = (typeCounts[node.type] ?? 0) + 1;
  }

  const complexityCounts: Record<string, number> = { simple: 0, moderate: 0, complex: 0 };
  for (const node of nodes) {
    if (node.complexity) {
      complexityCounts[node.complexity] = (complexityCounts[node.complexity] ?? 0) + 1;
    }
  }

  const nodeConnections = new Map<string, number>();
  for (const edge of edges) {
    nodeConnections.set(edge.source, (nodeConnections.get(edge.source) ?? 0) + 1);
    nodeConnections.set(edge.target, (nodeConnections.get(edge.target) ?? 0) + 1);
  }
  const topNodes = Array.from(nodeConnections.entries())
    .sort((a, b) => b[1] - a[1])
    .slice(0, 5)
    .map(([nodeId, count]) => {
      const node = nodes.find((n) => n.id === nodeId);
      return { id: nodeId, name: node?.name ?? nodeId, count };
    });

  const avgConnections = nodes.length > 0 ? (edges.length * 2 / nodes.length).toFixed(1) : "0";

  const categoryBreakdown = [
    { label: t.projectOverview.code, color: "var(--color-node-file)", count: (typeCounts["file"] ?? 0) + (typeCounts["function"] ?? 0) + (typeCounts["class"] ?? 0) + (typeCounts["module"] ?? 0) + (typeCounts["concept"] ?? 0) },
    { label: t.projectOverview.config, color: "var(--color-node-config)", count: typeCounts["config"] ?? 0 },
    { label: t.projectOverview.docs, color: "var(--color-node-document)", count: typeCounts["document"] ?? 0 },
    { label: t.projectOverview.infra, color: "var(--color-node-service)", count: (typeCounts["service"] ?? 0) + (typeCounts["resource"] ?? 0) + (typeCounts["pipeline"] ?? 0) },
    { label: t.projectOverview.data, color: "var(--color-node-table)", count: (typeCounts["table"] ?? 0) + (typeCounts["endpoint"] ?? 0) + (typeCounts["schema"] ?? 0) },
    { label: t.projectOverview.domain, color: "var(--color-node-concept)", count: (typeCounts["domain"] ?? 0) + (typeCounts["flow"] ?? 0) + (typeCounts["step"] ?? 0) },
  ];
  const hasNonCodeNodes = categoryBreakdown.some((c) => c.label !== t.projectOverview.code && c.count > 0);

  return (
    // Neither `h-full` nor `overflow-auto`: the sidebar's own container already
    // scrolls (App.tsx `flex-1 min-h-0 overflow-auto`). Claiming the full height
    // and a second scroller made this panel eat the wheel and push anything
    // composed below it — LearnPanel — out of reach.
    <div className="w-full p-5 animate-fade-slide-in">
      {/* Project name */}
      <h2 className="font-heading text-2xl text-text-primary mb-1">{project.name}</h2>
      <p className="text-sm text-text-secondary leading-relaxed mb-6">{project.description}</p>

      {/* Stats grid */}
      <div className="grid grid-cols-2 gap-3 mb-6">
        <div className="bg-elevated bevel-sm p-3 border border-border-subtle">
          <div className="text-2xl font-mono font-medium text-accent">{nodes.length}</div>
          <div className="text-[11px] text-text-muted uppercase tracking-wider mt-1">{t.projectOverview.nodes}</div>
        </div>
        <div className="bg-elevated bevel-sm p-3 border border-border-subtle">
          <div className="text-2xl font-mono font-medium text-accent">{edges.length}</div>
          <div className="text-[11px] text-text-muted uppercase tracking-wider mt-1">{t.projectOverview.edges}</div>
        </div>
        <div className="bg-elevated bevel-sm p-3 border border-border-subtle">
          <div className="text-2xl font-mono font-medium text-accent">{layers.length}</div>
          <div className="text-[11px] text-text-muted uppercase tracking-wider mt-1">{t.projectOverview.layers}</div>
        </div>
        <div className="bg-elevated bevel-sm p-3 border border-border-subtle">
          <div className="text-2xl font-mono font-medium text-accent">{Object.keys(typeCounts).length}</div>
          <div className="text-[11px] text-text-muted uppercase tracking-wider mt-1">{t.projectOverview.types}</div>
        </div>
      </div>

      {/* File Types breakdown */}
      {hasNonCodeNodes && (
        <div className="mb-5">
          <h3 className="text-[11px] font-semibold text-accent uppercase tracking-wider mb-2">{t.projectOverview.fileTypes}</h3>
          <div className="space-y-1.5">
            {categoryBreakdown.filter((c) => c.count > 0).map((cat) => (
              <div key={cat.label} className="flex items-center gap-2">
                <span
                  className="w-2.5 h-2.5 rounded-full shrink-0"
                  style={{ backgroundColor: cat.color }}
                />
                <span className="text-xs text-text-secondary flex-1">{cat.label}</span>
                <span className="text-xs font-mono text-text-muted">{cat.count}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Business flows.
          Placed near the TOP, not at the bottom: the tour entry point was moved
          out of this panel precisely because burying a mode switch under the
          statistics meant scrolling past the whole overview to find it (see the
          note at the foot of this file). Knowledge graphs have no business-flow
          view, so the gate matches the header toggle's own condition. */}
      {!isKnowledgeGraph && (
        <div className="mb-5">
          <button
            type="button"
            onClick={() =>
              domainGraph ? setViewMode("domain") : setBusinessFlowModalOpen(true)
            }
            disabled={businessFlowStatus === "generating"}
            className="w-full px-3 py-2 text-xs font-medium bevel-sm bg-elevated border border-border-subtle text-text-secondary hover:text-text-primary hover:border-accent/40 disabled:opacity-60 disabled:cursor-wait transition-colors flex items-center justify-between gap-2"
          >
            <span>
              {businessFlowStatus === "generating"
                ? t.businessFlow.generating
                : domainGraph
                ? t.businessFlow.open
                : t.businessFlow.generate}
            </span>
            {domainGraph && businessFlowStatus !== "generating" && (
              <span className="text-text-muted font-mono">
                {domainGraph.nodes.filter((n) => n.type === "domain").length}
              </span>
            )}
          </button>
          {businessFlowStatus === "failed" && (
            <p className="text-[11px] text-red-400 mt-1.5 leading-relaxed">
              {t.businessFlow.failed}
              {businessFlowError ? `: ${businessFlowError}` : ""}
            </p>
          )}
          {/* No "regenerate" link here. Once a graph exists this panel's job is
              to get you INTO the view; regenerating is offered from the graph
              canvas itself, where you are standing when you decide the flows
              are wrong. Offering it in both places is the same duplication the
              tour button was moved out of this panel to avoid. */}
        </div>
      )}

      {/* Source — above LANGUAGES because everything below is derived FROM it,
          so showing the derived facts before their origin reads backwards. */}
      <SourceRow
        project={scope.project}
        stream={scope.stream}
        token={token ?? null}
      />

      {/* Languages */}
      {project.languages.length > 0 && (
        <div className="mb-5">
          <h3 className="text-[11px] font-semibold text-accent uppercase tracking-wider mb-2">{t.projectOverview.languages}</h3>
          <div className="flex flex-wrap gap-1.5">
            {project.languages.map((lang) => (
              <span key={lang} className="text-[11px] glass text-text-secondary px-2.5 py-1 bevel-sm">
                {lang}
              </span>
            ))}
          </div>
        </div>
      )}

      {/* Frameworks */}
      {project.frameworks.length > 0 && (
        <div className="mb-5">
          <h3 className="text-[11px] font-semibold text-accent uppercase tracking-wider mb-2">{t.projectOverview.frameworks}</h3>
          <div className="flex flex-wrap gap-1.5">
            {project.frameworks.map((fw) => (
              <span key={fw} className="text-[11px] glass text-text-secondary px-2.5 py-1 bevel-sm">
                {fw}
              </span>
            ))}
          </div>
        </div>
      )}

      {/* Node Type Breakdown */}
      <div className="mb-5">
        <h3 className="text-[11px] font-semibold text-accent uppercase tracking-wider mb-3">{t.projectOverview.nodeTypeDistribution}</h3>
        <div className="space-y-2">
          {Object.entries(typeCounts)
            .sort((a, b) => b[1] - a[1])
            .map(([type, count]) => {
              const percentage = ((count / nodes.length) * 100).toFixed(0);
              return (
                <div key={type}>
                  <div className="flex items-center justify-between text-xs mb-1">
                    <span className="text-text-secondary capitalize">{type}</span>
                    <span className="text-text-muted font-mono">{count} ({percentage}%)</span>
                  </div>
                  <div className="w-full h-1.5 bg-elevated rounded-full overflow-hidden">
                    <div
                      className="h-full bg-accent/50 transition-all duration-500"
                      style={{ width: `${percentage}%` }}
                    />
                  </div>
                </div>
              );
            })}
        </div>
      </div>

      {/* Complexity Breakdown */}
      {Object.values(complexityCounts).some((c) => c > 0) && (
        <div className="mb-5">
          <h3 className="text-[11px] font-semibold text-accent uppercase tracking-wider mb-3">{t.projectOverview.complexityDistribution}</h3>
          <div className="grid grid-cols-3 gap-2">
            <div className="bg-elevated bevel-sm p-2 border border-border-subtle text-center">
              <div className="text-lg font-mono font-medium text-green-400">{complexityCounts.simple}</div>
              <div className="text-[10px] text-text-muted uppercase tracking-wider mt-0.5">{t.projectOverview.simple}</div>
            </div>
            <div className="bg-elevated bevel-sm p-2 border border-border-subtle text-center">
              <div className="text-lg font-mono font-medium text-yellow-400">{complexityCounts.moderate}</div>
              <div className="text-[10px] text-text-muted uppercase tracking-wider mt-0.5">{t.projectOverview.moderate}</div>
            </div>
            <div className="bg-elevated bevel-sm p-2 border border-border-subtle text-center">
              <div className="text-lg font-mono font-medium text-red-400">{complexityCounts.complex}</div>
              <div className="text-[10px] text-text-muted uppercase tracking-wider mt-0.5">{t.projectOverview.complex}</div>
            </div>
          </div>
        </div>
      )}

      {/* Top Connected Nodes */}
      {topNodes.length > 0 && (
        <div className="mb-5">
          <h3 className="text-[11px] font-semibold text-accent uppercase tracking-wider mb-3">{t.projectOverview.mostConnectedNodes}</h3>
          <div className="space-y-2">
            {topNodes.map((node, idx) => (
              <div
                key={node.id}
                className="flex items-center gap-2 text-xs bg-elevated bevel-sm p-2 border border-border-subtle"
              >
                <div className="w-5 h-5 shrink-0 rounded-full bg-accent/20 flex items-center justify-center text-[10px] font-bold text-accent">
                  {idx + 1}
                </div>
                <span className="flex-1 text-text-primary truncate">{node.name}</span>
                <span className="text-text-muted font-mono shrink-0">{node.count}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Average Connections */}
      <div className="mb-5 bg-elevated bevel-sm p-3 border border-border-subtle">
        <div className="flex items-center justify-between">
          <span className="text-xs text-text-secondary">{t.projectOverview.avgConnectionsPerNode}</span>
          <span className="text-lg font-mono font-medium text-accent">{avgConnections}</span>
        </div>
      </div>

      {/* Analyzed at */}
      <div className="text-[11px] text-text-muted mb-6">
        {t.common.analyzed}: {new Date(project.analyzedAt).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' })}
      </div>

      {/* Starting a tour lives in the TOUR tab, not here. This panel answers
          "what am I looking at"; the tour is a mode you enter, and having its
          entry point buried under the statistics meant scrolling past the whole
          overview to find it — and it appeared in two places at once, since
          LearnPanel offered the same button. */}
    </div>
  );
}
