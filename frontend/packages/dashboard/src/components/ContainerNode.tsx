import { memo } from "react";
import { Handle, Position } from "@xyflow/react";
import type { NodeProps, Node } from "@xyflow/react";
import { getLayerColor } from "./LayerLegend";

export interface ContainerNodeData extends Record<string, unknown> {
  containerId: string;
  name: string;
  childCount: number;
  strategy: "folder" | "community";
  colorIndex: number;
  isExpanded: boolean;
  hasSearchHits: boolean;
  searchHitCount?: number;
  isFocusedViaChild: boolean;
  onToggle: (containerId: string) => void;
}

export type ContainerFlowNode = Node<ContainerNodeData, "container">;

function ContainerNodeComponent({ data, width, height }: NodeProps<ContainerFlowNode>) {
  const color = getLayerColor(data.colorIndex);

  const borderColor =
    data.isExpanded || data.isFocusedViaChild
      ? "var(--color-accent-overlay-border)"
      : "var(--color-border-medium)";
  const borderWidth = data.isExpanded || data.isFocusedViaChild ? 1.5 : 1;

  const labelDimmed = data.name === "~";
  const labelText = labelDimmed ? "(root)" : data.name;

  const handleToggle = (e: React.SyntheticEvent) => {
    e.stopPropagation();
    data.onToggle(data.containerId);
  };

  return (
    <div
      role="button"
      tabIndex={0}
      aria-expanded={data.isExpanded}
      aria-label={`${labelText} container, ${data.childCount} item${data.childCount !== 1 ? "s" : ""}, ${data.isExpanded ? "expanded" : "collapsed"}`}
      className="bevel cursor-pointer transition-all focus:outline-none focus:ring-2 focus:ring-accent/60"
      style={{
        width,
        height,
        background: "rgba(255,255,255,0.02)",
        border: `${borderWidth}px solid ${borderColor}`,
        position: "relative",
      }}
      onClick={handleToggle}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          handleToggle(e);
        }
      }}
    >
      {/*
        REQUIRED, not decorative. React Flow resolves an edge by looking up a
        source handle on the source node and a target handle on the target node;
        a node with neither causes every edge touching it to be dropped
        SILENTLY — no warning, no console error, the line simply is not there.
        ContainerNode shipped without handles, so the layer view mounted 1 of
        130 aggregated edges while the overview (LayerClusterNode, which has
        them) drew its connections correctly. That asymmetry is the whole reason
        drilling into a layer looked like it had no dependencies.
      */}
      <Handle type="target" position={Position.Top} className="!bg-text-muted !w-2 !h-2" />

      <div
        className="flex items-center justify-between font-heading"
        style={{
          padding: "12px 16px",
          color: color.label,
          fontSize: 14,
          fontWeight: 400,
        }}
      >
        <span
          className={labelDimmed ? "opacity-50" : ""}
          style={{ display: "flex", alignItems: "center", gap: 6 }}
        >
          {data.isExpanded && <span style={{ fontSize: 10 }}>▾</span>}
          {labelText}
          {data.searchHitCount != null && data.searchHitCount > 0 && (
            <span
              className="font-mono"
              style={{
                marginLeft: 6,
                fontSize: 10,
                background: "var(--color-accent-overlay-bg)",
                color: "var(--color-accent)",
                padding: "1px 6px",
                border: "1px solid var(--color-border-medium)",
              }}
            >
              {data.searchHitCount} hit{data.searchHitCount !== 1 ? "s" : ""}
            </span>
          )}
        </span>
        <span style={{ color: "var(--color-text-secondary)", fontSize: 11 }}>{data.childCount}</span>
      </div>

      {/*
        Collapsed containers carry a density bar instead of empty space. A
        collapsed box says only "there is something in here"; the bar says how
        much, so a 200-file package and a 3-file one stop looking identical.
      */}
      {!data.isExpanded && (
        <div style={{ padding: "0 16px" }}>
          <div
            style={{
              height: 3,
              background: "var(--color-border-subtle)",
              overflow: "hidden",
            }}
          >
            <div
              style={{
                height: "100%",
                width: `${Math.min(100, Math.round((Math.log2(data.childCount + 1) / Math.log2(257)) * 100))}%`,
                background: color.label,
                opacity: 0.55,
              }}
            />
          </div>
          <div
            className="font-mono"
            style={{ marginTop: 6, fontSize: 10, color: "var(--color-text-muted)" }}
          >
            {data.childCount} {data.childCount === 1 ? "item" : "items"} · click to open
          </div>
        </div>
      )}

      <Handle type="source" position={Position.Bottom} className="!bg-text-muted !w-2 !h-2" />
    </div>
  );
}

const ContainerNode = memo(ContainerNodeComponent);
ContainerNode.displayName = "ContainerNode";

export default ContainerNode;
