import { useCallback, useEffect, useMemo, useState } from "react";
import {
  ReactFlow,
  ReactFlowProvider,
  Background,
  BackgroundVariant,
  Controls,
  MiniMap,
} from "@xyflow/react";
import type { Edge, Node } from "@xyflow/react";
import "@xyflow/react/dist/style.css";

import DomainClusterNode from "./DomainClusterNode";
import type { DomainClusterFlowNode } from "./DomainClusterNode";
import FlowNode from "./FlowNode";
import type { FlowFlowNode } from "./FlowNode";
import StepNode from "./StepNode";
import type { StepFlowNode } from "./StepNode";
import { useDashboardStore } from "../store";
import { useI18n } from "../contexts/I18nContext";
import { useTheme } from "../themes/index.ts";
import { mergeElkPositions, nodesToElkInput } from "../utils/layout";
import { applyElkLayout } from "../utils/elk-layout";
import { rankFlowSteps } from "../utils/domainOrder";
import type { KnowledgeGraph, GraphNode } from "@prx/core/types";

const nodeTypes = {
  "domain-cluster": DomainClusterNode,
  "flow-node": FlowNode,
  "step-node": StepNode,
};

function getDomainMeta(node: GraphNode) {
  return node.domainMeta;
}

interface BuiltGraph {
  nodes: Node[];
  edges: Edge[];
  dims: Map<string, { width: number; height: number }>;
}

function buildDomainOverview(graph: KnowledgeGraph): BuiltGraph {
  const dims = new Map<string, { width: number; height: number }>();
  const domainNodes = graph.nodes.filter((n) => n.type === "domain");

  // Count flows per domain
  const flowCountMap = new Map<string, number>();
  for (const edge of graph.edges) {
    if (edge.type === "contains_flow") {
      flowCountMap.set(edge.source, (flowCountMap.get(edge.source) ?? 0) + 1);
    }
  }

  const rfNodes: DomainClusterFlowNode[] = domainNodes.map((node) => {
    const meta = getDomainMeta(node);
    const data = {
      label: node.name,
      summary: node.summary,
      entities: meta?.entities as string[] | undefined,
      flowCount: flowCountMap.get(node.id) ?? 0,
      businessRules: meta?.businessRules as string[] | undefined,
      domainId: node.id,
    };
    dims.set(node.id, { width: 320, height: 180 });
    return {
      id: node.id,
      type: "domain-cluster" as const,
      position: { x: 0, y: 0 },
      data,
    };
  });

  const rfEdges: Edge[] = graph.edges
    .filter((e) => e.type === "cross_domain")
    .map((e, i) => ({
      id: `cd-${i}-${e.source}-${e.target}`,
      source: e.source,
      target: e.target,
      label: e.description ?? "",
      style: { stroke: "var(--color-accent)", strokeDasharray: "6 3", strokeWidth: 2 },
      labelStyle: { fill: "var(--color-text-muted)", fontSize: 10 },
      labelBgStyle: { fill: "var(--color-surface)", fillOpacity: 0.9 },
      labelBgPadding: [6, 4] as [number, number],
      labelBgBorderRadius: 4,
      animated: true,
    }));

  return { nodes: rfNodes as unknown as Node[], edges: rfEdges, dims };
}

function buildDomainDetail(
  graph: KnowledgeGraph,
  domainId: string,
): BuiltGraph {
  // Find flows for this domain
  const flowIds = new Set(
    graph.edges
      .filter((e) => e.type === "contains_flow" && e.source === domainId)
      .map((e) => e.target),
  );

  const flowNodes = graph.nodes.filter((n) => flowIds.has(n.id));
  const stepEdges = graph.edges.filter(
    (e) => e.type === "flow_step" && flowIds.has(e.source),
  );
  const stepIds = new Set(stepEdges.map((e) => e.target));
  const stepNodes = graph.nodes.filter((n) => stepIds.has(n.id));

  // Build step order map. Ranked by ascending weight within each flow rather
  // than decoded from the weight's magnitude — see utils/domainOrder.ts.
  const stepOrderMap = rankFlowSteps(stepEdges);

  // Count steps per flow
  const stepCountMap = new Map<string, number>();
  for (const edge of stepEdges) {
    stepCountMap.set(edge.source, (stepCountMap.get(edge.source) ?? 0) + 1);
  }

  const dims = new Map<string, { width: number; height: number }>();

  const flowRfNodes: FlowFlowNode[] = flowNodes.map((node) => {
    const meta = getDomainMeta(node);
    dims.set(node.id, { width: 260, height: 120 });
    return {
      id: node.id,
      type: "flow-node" as const,
      position: { x: 0, y: 0 },
      data: {
        label: node.name,
        summary: node.summary,
        entryPoint: meta?.entryPoint as string | undefined,
        entryType: meta?.entryType as string | undefined,
        stepCount: stepCountMap.get(node.id) ?? 0,
        flowId: node.id,
      },
    };
  });
  const stepRfNodes: StepFlowNode[] = stepNodes.map((node) => {
    dims.set(node.id, { width: 200, height: 90 });
    return {
      id: node.id,
      type: "step-node" as const,
      position: { x: 0, y: 0 },
      data: {
        label: node.name,
        summary: node.summary,
        filePath: node.filePath,
        stepId: node.id,
        order: stepOrderMap.get(node.id) ?? 0,
      },
    };
  });
  const rfNodes: Node[] = [...flowRfNodes, ...stepRfNodes];

  const rfEdges: Edge[] = stepEdges.map((e, i) => ({
    id: `fs-${i}-${e.source}-${e.target}`,
    source: e.source,
    target: e.target,
    style: { stroke: "var(--color-border-medium)", strokeWidth: 1.5 },
    animated: false,
  }));

  return { nodes: rfNodes, edges: rfEdges, dims };
}

function DomainGraphViewInner() {
  const domainGraph = useDashboardStore((s) => s.domainGraph);
  const activeDomainId = useDashboardStore((s) => s.activeDomainId);
  const clearActiveDomain = useDashboardStore((s) => s.clearActiveDomain);
  const navigateToDomain = useDashboardStore((s) => s.navigateToDomain);
  const selectNode = useDashboardStore((s) => s.selectNode);
  const businessFlowStatus = useDashboardStore((s) => s.businessFlowStatus);
  const setBusinessFlowModalOpen = useDashboardStore((s) => s.setBusinessFlowModalOpen);
  const { t } = useI18n();
  const { preset } = useTheme();

  // Build structural nodes/edges/dims synchronously; only the layout call
  // itself is async, so we memo the structural pieces and run ELK in an
  // effect.
  const built = useMemo<BuiltGraph | null>(() => {
    if (!domainGraph) return null;
    if (activeDomainId) {
      return buildDomainDetail(domainGraph, activeDomainId);
    }
    return buildDomainOverview(domainGraph);
  }, [domainGraph, activeDomainId]);

  const [layout, setLayout] = useState<{ nodes: Node[]; edges: Edge[] }>({
    nodes: [],
    edges: [],
  });

  useEffect(() => {
    if (!built) {
      setLayout({ nodes: [], edges: [] });
      return;
    }
    let cancelled = false;
    const { nodes: nodesArray, edges: edgesArray, dims } = built;
    // DomainGraphView used dagre LR; preserve that direction with ELK.
    const elkInput = nodesToElkInput(nodesArray, edgesArray, dims, {
      "elk.direction": "RIGHT",
    });
    applyElkLayout(elkInput, { strict: import.meta.env.DEV })
      .then(({ positioned, issues }) => {
        if (cancelled) return;
        if (issues.length > 0) {
          // Funnel into store so WarningBanner surfaces them.
          useDashboardStore.getState().appendLayoutIssues(issues);
        }
        setLayout({
          nodes: mergeElkPositions(nodesArray, positioned),
          edges: edgesArray,
        });
      })
      .catch((err) => {
        if (cancelled) return;
        console.error("[domain ELK] layout failed:", err);
      });
    return () => {
      cancelled = true;
    };
  }, [built]);

  const { nodes, edges } = layout;

  /**
   * ONE CLICK drills into a domain, matching the structural view — GraphView's
   * own `onNodeClick` calls `drillIntoLayer` on a single click at the overview
   * level, so requiring a double-click here was an inconsistency, not a design.
   *
   * Handled on <ReactFlow> rather than inside the node components for a second,
   * load-bearing reason: React Flow computes
   * `hasPointerEvents = isSelectable || isDraggable || onClick || …` and sets
   * `pointer-events: none` on a node when all of them are false. With
   * `nodesDraggable` and `elementsSelectable` off — which is what makes a drag
   * pan instead of rearranging the diagram — a node's own DOM handler stops
   * receiving events entirely and the pane swallows every click. Passing
   * `onNodeClick` is what keeps the nodes clickable at all.
   */
  const onNodeClick = useCallback(
    (_: React.MouseEvent, node: { id: string; type?: string }) => {
      if (node.type === "domain-cluster") {
        navigateToDomain(node.id);
      } else {
        selectNode(node.id);
      }
    },
    [navigateToDomain, selectNode],
  );

  const onPaneClick = useCallback(() => selectNode(null), [selectNode]);

  if (!domainGraph) {
    return (
      <div className="h-full flex items-center justify-center text-text-muted text-sm">
        {t.domainView.noGraph}
      </div>
    );
  }

  return (
    <div className="h-full w-full relative">
      {/* Top-left toolbar. The graph canvas is where someone is standing when
          they decide the flows need regenerating, so the control belongs here
          rather than only in the sidebar panel they arrived from. "Back to
          domains" shares the row instead of stacking, so drilling into a domain
          never shifts the regenerate button out from under the cursor. */}
      <div className="absolute top-3 left-3 z-10 flex items-center gap-2">
        {activeDomainId && (
          <button
            type="button"
            onClick={() => clearActiveDomain()}
            className="px-3 py-1.5 text-xs bevel-sm bg-elevated border border-border-subtle text-text-secondary hover:text-text-primary transition-colors"
          >
            {t.domainView.backToDomains}
          </button>
        )}
        <button
          type="button"
          onClick={() => setBusinessFlowModalOpen(true)}
          disabled={businessFlowStatus === "generating"}
          title={t.businessFlow.regenerate}
          className="px-3 py-1.5 text-xs bevel-sm bg-elevated border border-border-subtle text-text-secondary hover:text-text-primary hover:border-accent/40 disabled:opacity-60 disabled:cursor-wait transition-colors flex items-center gap-1.5"
        >
          <svg
            width="12" height="12" viewBox="0 0 24 24" fill="none"
            stroke="currentColor" strokeWidth="2.5" strokeLinecap="round"
            strokeLinejoin="round" aria-hidden="true"
            className={businessFlowStatus === "generating" ? "animate-spin" : undefined}
          >
            <path d="M21 12a9 9 0 1 1-2.64-6.36" />
            <path d="M21 3v6h-6" />
          </svg>
          {businessFlowStatus === "generating"
            ? t.businessFlow.generating
            : t.businessFlow.regenerate}
        </button>
      </div>
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        onNodeClick={onNodeClick}
        onPaneClick={onPaneClick}
        // Dragging must MOVE THE CANVAS, not rearrange it. React Flow's
        // defaults make nodes draggable and elements selectable, so a drag
        // either tore a node out of the ELK layout or drew a selection box
        // across the diagram — neither of which means anything here, because
        // positions are computed and there is no bulk action to select for.
        // These are the same five props GraphView sets, for the same reason.
        nodesDraggable={false}
        nodesConnectable={false}
        elementsSelectable={false}
        edgesFocusable={false}
        edgesReconnectable={false}
        fitView
        fitViewOptions={{ padding: 0.2 }}
        minZoom={0.1}
        maxZoom={2}
        proOptions={{ hideAttribution: true }}
        // Without this React Flow renders its Controls with the light-theme
        // stylesheet — white buttons on the dark canvas, which is what they
        // looked like here until now. GraphView has always passed it.
        colorMode={preset.isDark ? "dark" : "light"}
      >
        <Background
          variant={BackgroundVariant.Dots}
          gap={20}
          size={1}
          color="var(--color-border-subtle)"
        />
        {/* Horizontal and pinned below the Insight Advisor launcher, matching
            GraphView: stacked vertically at the default offset they sat on top
            of the launcher's box. `!m-0` matters — React Flow panels carry a
            default 15px margin, so `left-4` alone resolves to 31px. */}
        <Controls
          orientation="horizontal"
          position="bottom-left"
          className="!bottom-3 !left-4 !m-0 !flex-row"
        />
        {/* 2x the default 200x150, and `pannable`/`zoomable` the same way
            GraphView's is: without them the minimap is a picture you can only
            look at. With them, dragging inside it moves the viewport, which is
            the fastest way across a domain that runs off both edges. */}
        <MiniMap
          pannable
          zoomable
          nodeColor="var(--color-accent)"
          maskColor="var(--glass-bg)"
          style={{ width: 400, height: 300 }}
          className="!bg-surface !border !border-border-subtle cursor-pointer"
        />
      </ReactFlow>
    </div>
  );
}

export default function DomainGraphView() {
  return (
    <ReactFlowProvider>
      <DomainGraphViewInner />
    </ReactFlowProvider>
  );
}
