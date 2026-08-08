import { useDashboardStore } from "../store";
import { useI18n } from "../contexts/I18nContext";

// Shared layer color palette — used by LayerLegend, LayerClusterNode, PortalNode, and GraphView.
//
// Derived from the theme's node-type tokens rather than repeating their hex
// values. The same seven colours were previously hardcoded here AND defined in
// themes/presets.ts, so every preset except the default rendered its nodes in
// the new palette and its layers in the old one — the two drifted apart
// silently, because nothing reads a duplicated constant and notices.
//
// `color-mix` does the alpha, so a preset only has to change the base token and
// the fill and border follow.
const LAYER_TOKENS = [
  "node-file",      // blue   (API)
  "node-function",  // sage   (Data)
  "node-class",     // iris   (Service)
  "node-module",    // sand   (Config)
  "node-concept",   // rose   (UI)
  "node-config",    // teal   (Middleware)
  "node-document",  // slate  (Test)
];

export const LAYER_PALETTE = LAYER_TOKENS.map((token) => ({
  bg: `color-mix(in srgb, var(--color-${token}) 12%, transparent)`,
  border: `color-mix(in srgb, var(--color-${token}) 40%, transparent)`,
  label: `var(--color-${token})`,
}));

export function getLayerColor(index: number) {
  return LAYER_PALETTE[index % LAYER_PALETTE.length];
}

export default function LayerLegend() {
  const graph = useDashboardStore((s) => s.graph);
  const navigationLevel = useDashboardStore((s) => s.navigationLevel);
  const activeLayerId = useDashboardStore((s) => s.activeLayerId);
  const { t } = useI18n();

  const layers = graph?.layers ?? [];
  const hasLayers = layers.length > 0;

  if (!hasLayers) return null;

  const activeLayer = layers.find((l) => l.id === activeLayerId);

  return (
    <div className="flex items-center gap-2">
      <span className="text-[11px] font-medium text-text-secondary whitespace-nowrap">
        {navigationLevel === "overview"
          ? `${layers.length} ${t.layer.label}`
          : activeLayer?.name ?? t.layer.defaultName}
      </span>

      <div className="flex items-center gap-3">
        {layers.map((layer, i) => {
          const color = getLayerColor(i);
          const isActive = navigationLevel === "layer-detail" && layer.id === activeLayerId;
          return (
            <div key={layer.id} className="flex items-center gap-1 whitespace-nowrap">
              <span
                className="inline-block w-2 h-2 rounded-full"
                style={{
                  backgroundColor: color.label,
                  opacity: navigationLevel === "layer-detail" && !isActive ? 0.3 : 1,
                }}
              />
              <span
                className={`text-[11px] ${
                  isActive ? "text-text-primary font-medium" : "text-text-secondary"
                }`}
                style={{
                  opacity: navigationLevel === "layer-detail" && !isActive ? 0.4 : 1,
                }}
              >
                {layer.name}
                <span className="text-text-muted ml-0.5">
                  ({layer.nodeIds.length})
                </span>
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
