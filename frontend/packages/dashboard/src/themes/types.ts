export type PresetId =
  | "dark-gold"
  | "dark-ocean"
  | "dark-forest"
  | "dark-rose"
  | "light-minimal";

export interface AccentSwatch {
  id: string;
  name: string;
  accent: string;
  accentDim: string;
  accentBright: string;
}

/**
 * Primary call-to-action colors.
 *
 * Deliberately kept *outside* the accent-derivation chain in `theme-engine.ts`:
 * the accent tints every border, glass surface, glow and edge in the app, so a
 * high-energy brand color piped through it would flood the UI. The CTA color is
 * applied to a handful of primary buttons only — mirroring how the Praxevia
 * brand uses sage for interactive surfaces and orange for the one action that
 * matters on a given screen.
 */
export interface CtaColors {
  base: string;
  hover: string;
  fg: string;
}

export interface ThemePreset {
  id: PresetId;
  name: string;
  isDark: boolean;
  colors: Record<string, string>;
  accentSwatches: AccentSwatch[];
  defaultAccentId: string;
  /** Falls back to the accent swatch when a preset does not define one. */
  cta?: CtaColors;
}

export type HeadingFont = "serif" | "sans" | "mono";

export interface ThemeConfig {
  presetId: PresetId;
  accentId: string;
  headingFont?: HeadingFont;
}

export const DEFAULT_THEME_CONFIG: ThemeConfig = {
  presetId: "dark-gold",
  accentId: "orange",
  headingFont: "sans",
};
