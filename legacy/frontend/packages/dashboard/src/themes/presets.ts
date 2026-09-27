import type { AccentSwatch, CtaColors, ThemePreset } from "./types.ts";

/**
 * Praxevia palette.
 *
 * Sourced from the Praxevia brand site (praxevia-website.vercel.app):
 *   prx-900 #071D24  prx-800 #11313C  prx-700 #1A3F4D  prx-600 #2A5A6A
 *   prx-500 #76C0A1 (sage)  prx-400 #98D0B8  prx-300 #B9DFD0
 *   orange-500 #F26224  cta-hover #D9551E
 *   neutral 50..700 #F7FAFB #EEF3F5 #DDE7EA #BFD0D5 #9AABB3 #748B94 #566A73 #3A4A52
 *
 * Sage is the accent — it drives every border, glow, edge and interactive
 * surface, exactly as it does on the site. Orange is reserved for primary
 * actions and lives in `cta`, outside the accent derivation chain.
 */
const PRAXEVIA_CTA: CtaColors = { base: "#F26224", hover: "#D9551E", fg: "#FFFFFF" };

/**
 * Node-type colors. These encode meaning (file vs. class vs. endpoint) so all
 * thirteen must stay tellable apart, but hues are held to a common lightness
 * and saturation band so the graph reads as one system rather than confetti.
 */
const DARK_NODE_COLORS: Record<string, string> = {
  "node-file": "#6BA8CC",
  "node-function": "#76C0A1",
  "node-class": "#A38FCB",
  "node-module": "#C9A97A",
  "node-concept": "#CE8FA5",
  "node-config": "#5FC8C0",
  "node-document": "#A9C6DE",
  "node-service": "#8496CE",
  "node-table": "#93D3A6",
  "node-endpoint": "#EE9A63",
  "node-pipeline": "#DE93AE",
  "node-schema": "#D9C177",
  "node-resource": "#B7AEDC",
};

/** Same hues, darkened for contrast against the light #F4F7F9 canvas. */
const LIGHT_NODE_COLORS: Record<string, string> = {
  "node-file": "#3C7B9F",
  "node-function": "#3E8F6F",
  "node-class": "#6F5AA0",
  "node-module": "#9B7A42",
  "node-concept": "#A25A74",
  "node-config": "#2A8F88",
  "node-document": "#5589AC",
  "node-service": "#4C619F",
  "node-table": "#3F9D68",
  "node-endpoint": "#C26326",
  "node-pipeline": "#AE5C79",
  "node-schema": "#9C8330",
  "node-resource": "#7970AC",
};

const DARK_ACCENT_SWATCHES: AccentSwatch[] = [
  // id "orange" is a persisted localStorage value — kept for compatibility.
  { id: "orange", name: "Praxevia Sage", accent: "#76C0A1", accentDim: "#5FA88A", accentBright: "#98D0B8" },
  { id: "cta", name: "Praxevia Orange", accent: "#F26224", accentDim: "#D9551E", accentBright: "#F7A07A" },
  { id: "gold", name: "Sand", accent: "#C9A97A", accentDim: "#B39463", accentBright: "#DCC29A" },
  { id: "ocean", name: "Ocean", accent: "#6BA8CC", accentDim: "#5490B4", accentBright: "#8DC0DE" },
  { id: "emerald", name: "Emerald", accent: "#5FBF8C", accentDim: "#4BA675", accentBright: "#84D3A8" },
  { id: "rose", name: "Rose", accent: "#CE8FA5", accentDim: "#B7788E", accentBright: "#DFAABD" },
  { id: "purple", name: "Iris", accent: "#A38FCB", accentDim: "#8C77B6", accentBright: "#BBABDC" },
  { id: "amber", name: "Amber", accent: "#D9A94F", accentDim: "#BF9139", accentBright: "#E8C078" },
  { id: "teal", name: "Teal", accent: "#5FC8C0", accentDim: "#48ADA6", accentBright: "#87DAD4" },
  { id: "silver", name: "Silver", accent: "#9AABB3", accentDim: "#7F929B", accentBright: "#BFD0D5" },
];

const LIGHT_ACCENT_SWATCHES: AccentSwatch[] = [
  { id: "orange", name: "Praxevia Sage", accent: "#3E8F6F", accentDim: "#2F7359", accentBright: "#5FA88A" },
  { id: "cta", name: "Praxevia Orange", accent: "#D9551E", accentDim: "#B84617", accentBright: "#F26224" },
  { id: "indigo", name: "Indigo", accent: "#4C619F", accentDim: "#3D5085", accentBright: "#6479B7" },
  { id: "ocean", name: "Ocean", accent: "#3C7B9F", accentDim: "#2F6685", accentBright: "#5593B7" },
  { id: "emerald", name: "Emerald", accent: "#3F9D68", accentDim: "#318454", accentBright: "#5AB682" },
  { id: "rose", name: "Rose", accent: "#A25A74", accentDim: "#89485F", accentBright: "#BB748D" },
  { id: "purple", name: "Iris", accent: "#6F5AA0", accentDim: "#5C4986", accentBright: "#8973B8" },
  { id: "amber", name: "Amber", accent: "#9C8330", accentDim: "#836D25", accentBright: "#B79C48" },
  { id: "teal", name: "Teal", accent: "#2A8F88", accentDim: "#1F7670", accentBright: "#42A8A0" },
  { id: "slate", name: "Slate", accent: "#566A73", accentDim: "#44555D", accentBright: "#748B94" },
];

export const PRESETS: ThemePreset[] = [
  {
    // Preset ids are persisted in localStorage under "prx-theme" — they are
    // storage keys, not labels, so they stay stable across rebrands.
    id: "dark-gold",
    name: "Praxevia Dark",
    isDark: true,
    defaultAccentId: "orange",
    accentSwatches: DARK_ACCENT_SWATCHES,
    cta: PRAXEVIA_CTA,
    colors: {
      root: "#071D24",
      surface: "#0D2831",
      elevated: "#1A3F4D",
      panel: "#11313C",
      "text-primary": "#EEF3F5",
      "text-secondary": "#9AABB3",
      "text-muted": "#748B94",
      ...DARK_NODE_COLORS,
    },
  },
  {
    id: "dark-ocean",
    name: "Deep Ocean",
    isDark: true,
    defaultAccentId: "ocean",
    accentSwatches: DARK_ACCENT_SWATCHES,
    cta: PRAXEVIA_CTA,
    colors: {
      root: "#08161F",
      surface: "#0E2130",
      elevated: "#1A3348",
      panel: "#122839",
      "text-primary": "#E8EFF4",
      "text-secondary": "#93A6B4",
      "text-muted": "#6B808F",
      ...DARK_NODE_COLORS,
    },
  },
  {
    id: "dark-forest",
    name: "Deep Forest",
    isDark: true,
    defaultAccentId: "emerald",
    accentSwatches: DARK_ACCENT_SWATCHES,
    cta: PRAXEVIA_CTA,
    colors: {
      root: "#08201B",
      surface: "#0D2C25",
      elevated: "#194237",
      panel: "#11362D",
      "text-primary": "#EAF3EF",
      "text-secondary": "#96B0A6",
      "text-muted": "#6E8A80",
      ...DARK_NODE_COLORS,
    },
  },
  {
    id: "dark-rose",
    name: "Deep Rose",
    isDark: true,
    defaultAccentId: "rose",
    accentSwatches: DARK_ACCENT_SWATCHES,
    cta: PRAXEVIA_CTA,
    colors: {
      root: "#1C1219",
      surface: "#271A23",
      elevated: "#3B2833",
      panel: "#31212C",
      "text-primary": "#F3EBEF",
      "text-secondary": "#B29CA7",
      "text-muted": "#8B7580",
      ...DARK_NODE_COLORS,
    },
  },
  {
    id: "light-minimal",
    name: "Praxevia Light",
    isDark: false,
    defaultAccentId: "orange",
    accentSwatches: LIGHT_ACCENT_SWATCHES,
    cta: PRAXEVIA_CTA,
    colors: {
      root: "#F4F7F9",
      surface: "#EEF3F5",
      elevated: "#FFFFFF",
      panel: "#F7FAFB",
      "text-primary": "#071D24",
      "text-secondary": "#566A73",
      "text-muted": "#748B94",
      ...LIGHT_NODE_COLORS,
    },
  },
];

export function getPreset(id: string): ThemePreset {
  return PRESETS.find((p) => p.id === id) ?? PRESETS[0];
}

export function getAccent(preset: ThemePreset, accentId: string): AccentSwatch {
  return (
    preset.accentSwatches.find((s) => s.id === accentId) ??
    preset.accentSwatches.find((s) => s.id === preset.defaultAccentId) ??
    preset.accentSwatches[0]
  );
}
