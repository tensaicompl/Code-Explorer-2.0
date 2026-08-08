import { useEffect, useMemo, useRef, useState } from "react";
import type { GraphNode } from "@prx/core/types";
import { useDashboardStore } from "../store";

interface PathFinderModalProps {
  isOpen: boolean;
  onClose: () => void;
}

const MAX_PICKER_RESULTS = 50;

// node type -> Tailwind token classes (mirrors SearchBar's typeBadgeColors)
const typeBadgeColors: Record<string, string> = {
  file: "text-node-file border border-node-file/30 bg-node-file/10",
  function: "text-node-function border border-node-function/30 bg-node-function/10",
  class: "text-node-class border border-node-class/30 bg-node-class/10",
  module: "text-node-module border border-node-module/30 bg-node-module/10",
  concept: "text-node-concept border border-node-concept/30 bg-node-concept/10",
  config: "text-node-config border border-node-config/30 bg-node-config/10",
  document: "text-node-document border border-node-document/30 bg-node-document/10",
  service: "text-node-service border border-node-service/30 bg-node-service/10",
  table: "text-node-table border border-node-table/30 bg-node-table/10",
  endpoint: "text-node-endpoint border border-node-endpoint/30 bg-node-endpoint/10",
  pipeline: "text-node-pipeline border border-node-pipeline/30 bg-node-pipeline/10",
  schema: "text-node-schema border border-node-schema/30 bg-node-schema/10",
  resource: "text-node-resource border border-node-resource/30 bg-node-resource/10",
  domain: "text-node-concept border border-node-concept/30 bg-node-concept/10",
  flow: "text-node-pipeline border border-node-pipeline/30 bg-node-pipeline/10",
  step: "text-node-function border border-node-function/30 bg-node-function/10",
};

/**
 * Searchable / typeahead node picker — replaces a native <select> of every
 * node (unusable on large graphs). Filters by name + filePath + type,
 * keyboard-navigable, caps results, shows a type badge + file path.
 */
function NodePicker({
  label,
  nodes,
  selectedId,
  onSelect,
  excludeId,
}: {
  label: string;
  nodes: GraphNode[];
  selectedId: string;
  onSelect: (id: string) => void;
  excludeId?: string;
}) {
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);
  const containerRef = useRef<HTMLDivElement>(null);

  const selectedNode = useMemo(
    () => nodes.find((n) => n.id === selectedId) ?? null,
    [nodes, selectedId],
  );

  useEffect(() => {
    if (selectedNode) setQuery(selectedNode.name);
  }, [selectedNode]);

  const results = useMemo(() => {
    const q = query.trim().toLowerCase();
    const out: GraphNode[] = [];
    for (const n of nodes) {
      if (excludeId && n.id === excludeId) continue;
      if (q) {
        const hay = `${n.name} ${n.filePath ?? ""} ${n.type}`.toLowerCase();
        if (!hay.includes(q)) continue;
      }
      out.push(n);
      if (out.length >= MAX_PICKER_RESULTS) break;
    }
    return out;
  }, [nodes, query, excludeId]);

  useEffect(() => setActiveIndex(0), [query, open]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  const choose = (node: GraphNode) => {
    onSelect(node.id);
    setQuery(node.name);
    setOpen(false);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setOpen(true);
      setActiveIndex((i) => Math.min(i + 1, results.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIndex((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter") {
      if (open && results[activeIndex]) {
        e.preventDefault();
        choose(results[activeIndex]);
      }
    } else if (e.key === "Escape" && open) {
      // Close only the dropdown, not the whole modal.
      e.nativeEvent.stopImmediatePropagation();
      setOpen(false);
    }
  };

  const badge = (type: string) => typeBadgeColors[type] ?? typeBadgeColors.file;

  return (
    <div ref={containerRef} className="relative">
      <label className="block text-xs font-semibold text-text-secondary uppercase tracking-wider mb-2">
        {label}
      </label>
      <input
        type="text"
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          setOpen(true);
          if (selectedId) onSelect("");
        }}
        onFocus={() => setOpen(true)}
        onKeyDown={handleKeyDown}
        placeholder="Type to search by name, path, or type…"
        className="w-full bg-elevated text-text-primary text-sm bevel-sm px-3 py-2 border border-border-subtle focus:outline-none focus:border-accent/50 placeholder-text-muted"
      />

      {selectedNode?.filePath && !open && (
        <div className="mt-1 text-xs text-text-muted truncate">{selectedNode.filePath}</div>
      )}

      {open && (
        <div className="absolute left-0 right-0 top-full mt-1 z-20 glass bevel-sm bevel-elevate max-h-64 overflow-y-auto">
          {results.length === 0 ? (
            <div className="px-3 py-2 text-sm text-text-muted">No matching nodes</div>
          ) : (
            results.map((node, idx) => (
              <button
                key={node.id}
                type="button"
                onMouseDown={(e) => e.preventDefault()}
                onMouseEnter={() => setActiveIndex(idx)}
                onClick={() => choose(node)}
                className={`w-full flex items-center gap-2 px-3 py-2 text-left transition-colors ${
                  idx === activeIndex ? "bg-elevated" : "hover:bg-elevated"
                }`}
              >
                <span
                  className={`text-[10px] font-semibold uppercase tracking-wider px-1.5 py-0.5 bevel-sm shrink-0 ${badge(node.type)}`}
                >
                  {node.type}
                </span>
                <span className="flex-1 min-w-0">
                  <span className="block text-sm text-text-primary truncate">{node.name}</span>
                  {node.filePath && (
                    <span className="block text-xs text-text-muted truncate">{node.filePath}</span>
                  )}
                </span>
              </button>
            ))
          )}
          {results.length >= MAX_PICKER_RESULTS && (
            <div className="px-3 py-1.5 text-[10px] text-text-muted border-t border-border-subtle">
              Showing first {MAX_PICKER_RESULTS} — refine your search
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export default function PathFinderModal({ isOpen, onClose }: PathFinderModalProps) {
  const graph = useDashboardStore((s) => s.graph);
  const selectNode = useDashboardStore((s) => s.selectNode);
  const [fromNodeId, setFromNodeId] = useState("");
  const [toNodeId, setToNodeId] = useState("");
  const [path, setPath] = useState<string[] | null>(null);
  const [searching, setSearching] = useState(false);
  const modalRef = useRef<HTMLDivElement>(null);

  // Close on outside click
  useEffect(() => {
    if (!isOpen) return;

    const handleClickOutside = (e: MouseEvent) => {
      if (modalRef.current && !modalRef.current.contains(e.target as Node)) {
        onClose();
      }
    };

    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [isOpen, onClose]);

  // Close on Escape
  useEffect(() => {
    if (!isOpen) return;

    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onClose();
      }
    };

    document.addEventListener("keydown", handleEscape);
    return () => document.removeEventListener("keydown", handleEscape);
  }, [isOpen, onClose]);

  if (!isOpen || !graph) return null;

  const nodes = graph.nodes;
  const edges = graph.edges;

  // BFS to find shortest path
  const findPath = () => {
    if (!fromNodeId || !toNodeId || fromNodeId === toNodeId) {
      setPath(null);
      return;
    }

    setSearching(true);

    // Build adjacency list (bidirectional traversal for path finding)
    const adjacency = new Map<string, string[]>();
    for (const edge of edges) {
      if (!adjacency.has(edge.source)) {
        adjacency.set(edge.source, []);
      }
      adjacency.get(edge.source)!.push(edge.target);
      // Also traverse in reverse so we can find paths through backward edges
      if (!adjacency.has(edge.target)) {
        adjacency.set(edge.target, []);
      }
      adjacency.get(edge.target)!.push(edge.source);
    }

    // BFS
    const queue: Array<{ nodeId: string; path: string[] }> = [
      { nodeId: fromNodeId, path: [fromNodeId] },
    ];
    const visited = new Set<string>([fromNodeId]);

    while (queue.length > 0) {
      const { nodeId, path: currentPath } = queue.shift()!;

      if (nodeId === toNodeId) {
        setPath(currentPath);
        setSearching(false);
        return;
      }

      const neighbors = adjacency.get(nodeId) ?? [];
      for (const neighbor of neighbors) {
        if (!visited.has(neighbor)) {
          visited.add(neighbor);
          queue.push({ nodeId: neighbor, path: [...currentPath, neighbor] });
        }
      }
    }

    // No path found
    setPath([]);
    setSearching(false);
  };

  const handleNodeClick = (nodeId: string) => {
    selectNode(nodeId);
    onClose();
  };

  const nodeMap = new Map(nodes.map((n) => [n.id, n]));

  return (
    <div className="fixed inset-0 z-[100] flex items-center justify-center bg-root/80 backdrop-blur-sm">
      <div
        ref={modalRef}
        className="glass-heavy bevel bevel-elevate w-full max-w-2xl max-h-[80vh] overflow-hidden animate-fade-slide-in"
      >
        {/* Header */}
        <div className="flex items-center justify-between px-5 py-4 border-b border-border-subtle">
          <div className="flex items-center gap-3">
            <svg className="w-5 h-5 text-accent" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M13 7h8m0 0v8m0-8l-8 8-4-4-6 6"
              />
            </svg>
            <h2 className="font-heading text-xl text-text-primary">Dependency Path Finder</h2>
          </div>
          <button
            onClick={onClose}
            className="text-text-muted hover:text-text-primary transition-colors"
          >
            <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        {/* Body */}
        <div className="p-5 space-y-4 overflow-y-auto max-h-[calc(80vh-180px)]">
          <p className="text-sm text-text-secondary">
            Find the shortest path between two nodes in the dependency graph.
          </p>

          {/* From Node */}
          <NodePicker
            label="From Node"
            nodes={nodes}
            selectedId={fromNodeId}
            excludeId={toNodeId}
            onSelect={(id) => {
              setFromNodeId(id);
              setPath(null);
            }}
          />

          {/* To Node */}
          <NodePicker
            label="To Node"
            nodes={nodes}
            selectedId={toNodeId}
            excludeId={fromNodeId}
            onSelect={(id) => {
              setToNodeId(id);
              setPath(null);
            }}
          />

          {/* Find Path Button */}
          <button
            onClick={findPath}
            disabled={!fromNodeId || !toNodeId || fromNodeId === toNodeId || searching}
            className="w-full bg-accent/10 border border-accent/30 text-accent text-sm font-medium py-2.5 px-4 bevel-sm hover:bg-accent/20 transition-all duration-200 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {searching ? "Searching..." : "Find Path"}
          </button>

          {/* Path Result */}
          {path !== null && (
            <div className="mt-4">
              {path.length === 0 ? (
                <div className="bg-red-900/20 border border-red-700/50 bevel-sm p-4 text-center">
                  <svg
                    className="w-8 h-8 text-red-400 mx-auto mb-2"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2}
                      d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
                    />
                  </svg>
                  <p className="text-sm text-red-200">No path found between these nodes.</p>
                </div>
              ) : (
                <div className="bg-elevated border border-border-subtle bevel-sm p-4">
                  <div className="flex items-center gap-2 mb-3">
                    <svg
                      className="w-4 h-4 text-green-400"
                      fill="none"
                      stroke="currentColor"
                      viewBox="0 0 24 24"
                    >
                      <path
                        strokeLinecap="round"
                        strokeLinejoin="round"
                        strokeWidth={2}
                        d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z"
                      />
                    </svg>
                    <h3 className="text-sm font-semibold text-text-primary">
                      Path Found ({path.length} nodes)
                    </h3>
                  </div>
                  <div className="space-y-2">
                    {path.map((nodeId, idx) => {
                      const node = nodeMap.get(nodeId);
                      if (!node) return null;

                      const isLast = idx === path.length - 1;

                      return (
                        <div key={nodeId}>
                          <button
                            onClick={() => handleNodeClick(nodeId)}
                            className="w-full flex items-center gap-3 p-2 bg-surface bevel-sm hover:bg-elevated transition-colors text-left"
                          >
                            <div className="w-6 h-6 shrink-0 rounded-full bg-accent/20 flex items-center justify-center text-xs font-bold text-accent">
                              {idx + 1}
                            </div>
                            <div className="flex-1 min-w-0">
                              <div className="text-sm text-text-primary truncate">{node.name}</div>
                              <div className="text-xs text-text-muted capitalize">{node.type}</div>
                            </div>
                            <svg
                              className="w-4 h-4 text-text-muted"
                              fill="none"
                              stroke="currentColor"
                              viewBox="0 0 24 24"
                            >
                              <path
                                strokeLinecap="round"
                                strokeLinejoin="round"
                                strokeWidth={2}
                                d="M9 5l7 7-7 7"
                              />
                            </svg>
                          </button>
                          {!isLast && (
                            <div className="flex items-center justify-center my-1">
                              <svg
                                className="w-4 h-4 text-accent"
                                fill="none"
                                stroke="currentColor"
                                viewBox="0 0 24 24"
                              >
                                <path
                                  strokeLinecap="round"
                                  strokeLinejoin="round"
                                  strokeWidth={2}
                                  d="M19 14l-7 7m0 0l-7-7m7 7V3"
                                />
                              </svg>
                            </div>
                          )}
                        </div>
                      );
                    })}
                  </div>
                </div>
              )}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end gap-3 px-5 py-4 border-t border-border-subtle">
          <button
            onClick={onClose}
            className="px-4 py-2 text-sm text-text-secondary hover:text-text-primary transition-colors"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
