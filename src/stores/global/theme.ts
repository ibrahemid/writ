import { createSignal, createMemo } from "solid-js";
import type { Theme, ThemeOverrides, ThemeConfig, ThemePolarity } from "../../types/theme";
import { TOKEN_GROUPS } from "../../types/theme";
import type { AppearanceConfig } from "../../types/config";
import type { AccentId } from "../../styles/generated/tokens";
import { ACCENTS } from "../../styles/generated/tokens";
import {
  PRESETS,
  getPreset,
  getDefaultPreset,
  pairedPreset,
  DEFAULT_PRESET_ID,
} from "../../styles/themes";

// Singleton — app-global, not window-scoped (ADR-009 E3).
// Theme is shared across every window; CSS custom properties on :root propagate.

const HEX_PATTERN = /^#[0-9a-fA-F]{3,8}$/;

// Mirror of the resolved CSS variables and root attributes, read by the inline
// boot script in index.html to paint the saved theme before the bundle loads
// (no FOUC). Versioned: a 0.3.5 snapshot describes the pre-ADR-030 palette and
// would flash the old dark theme on the first launch after the update.
const FAST_BOOT_KEY = "writ-theme-vars-v2";

const DEFAULT_APPEARANCE: AppearanceConfig = {
  polarity: "system",
  accent: "pine",
  prose_face: "system",
};

// Overrides are keyed by the pre-ADR-030 group names, which the token
// vocabulary replaced. They no longer describe anything the new palette paints,
// so they are dropped when a config loads rather than applied over it (ADR-030
// Consequences). A live edit still applies until the app restarts.
const RETIRED_OVERRIDE_GROUPS: ReadonlySet<string> = new Set(TOKEN_GROUPS);

const [presetId, setPresetId] = createSignal<string>(DEFAULT_PRESET_ID);
const [overrides, setOverridesSignal] = createSignal<ThemeOverrides>({});
const [appearance, setAppearanceSignal] = createSignal<AppearanceConfig>(DEFAULT_APPEARANCE);
const [systemPolarity, setSystemPolaritySignal] = createSignal<ThemePolarity>(detectSystemPolarity());

function detectSystemPolarity(): ThemePolarity {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return "light";
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

/** The polarity the user asked for: the OS setting, or the one they pinned. */
const effectivePolarity = createMemo<ThemePolarity>(() => {
  const wanted = appearance().polarity;
  return wanted === "system" ? systemPolarity() : wanted;
});

// Following the system swaps within a preset pair. A preset with no pair
// (tokyo-night, dracula, solarized-dark, catppuccin-mocha) has nothing to swap
// to, so it renders in its own polarity whatever the system says.
const activePresetId = createMemo<string>(() => pairedPreset(presetId(), effectivePolarity()));

const activePreset = createMemo<Theme>(() => getPreset(activePresetId()) ?? getDefaultPreset());
const polarity = createMemo<ThemePolarity>(() => activePreset().polarity ?? "dark");
const accent = createMemo<AccentId>(() => appearance().accent);

/**
 * A preset's token groups as flat `group.token` keys. Only string leaves make
 * it through: a nested object has no CSS declaration to become, and writing one
 * anyway is how `--writ-site-traffic: [object Object]` used to reach `:root`.
 * Exported for the contract test that holds that line.
 */
export function flattenTheme(theme: Theme): Record<string, string> {
  const flat: Record<string, string> = {};
  for (const [group, tokens] of Object.entries(theme)) {
    if (group === "id" || group === "name" || group === "polarity") continue;
    // `site` was a block of website-only values that no app or site stylesheet
    // ever read. It is gone from the presets; the guard keeps a hand-edited
    // theme from putting it back on `:root`.
    if (group === "site") continue;
    if (typeof tokens !== "object" || tokens === null) continue;
    for (const [key, value] of Object.entries(tokens as Record<string, unknown>)) {
      if (typeof value !== "string") continue;
      flat[`${group}.${key}`] = value;
    }
  }
  return flat;
}

function tokenKeyToCssVar(key: string): string {
  return `--writ-${key.replaceAll(".", "-")}`;
}

function isValidColor(value: string): boolean {
  return HEX_PATTERN.test(value);
}

export const themeStore = {
  presetId,
  overrides,
  activePreset,
  polarity,
  appearance,
  accent,
  effectivePolarity,

  resolvedTokens: createMemo<Record<string, string>>(() => {
    const triple = ACCENTS[accent()][polarity()];
    return {
      ...flattenTheme(activePreset()),
      // The accent is its own axis: the chosen hue wins over the preset's, and
      // a user override still wins over both.
      "accent.default": triple.base,
      "accent.hover": triple.hover,
      "accent.foreground": triple.foreground,
      ...overrides(),
    };
  }),

  applyToRoot(root: HTMLElement = document.documentElement): void {
    // The generated sheet keys its dark and accent layers on root attributes,
    // so both have to reach the DOM as well as the custom properties.
    const attrs: Record<string, string> = {
      "data-theme": polarity(),
      "data-accent": accent(),
    };
    for (const [name, value] of Object.entries(attrs)) root.setAttribute(name, value);
    const resolved = this.resolvedTokens();
    const vars: Record<string, string> = {};
    for (const [key, value] of Object.entries(resolved)) {
      const cssVar = tokenKeyToCssVar(key);
      root.style.setProperty(cssVar, value);
      vars[cssVar] = value;
    }
    // The alternate prose face is a swap of one token, so every surface that
    // reads --writ-font-prose follows it and nothing else moves.
    const proseVar = "--writ-font-prose";
    if (appearance().prose_face === "quattro") {
      const alt = "var(--writ-font-prose-alt)";
      root.style.setProperty(proseVar, alt);
      vars[proseVar] = alt;
    } else {
      root.style.removeProperty(proseVar);
    }
    try {
      localStorage.setItem(FAST_BOOT_KEY, JSON.stringify({ vars, attrs }));
    } catch {
      // Private-mode / quota failures are non-fatal: the app still themes at
      // runtime; only the pre-paint fast boot is skipped.
    }
  },

  setPreset(id: string): void {
    if (!getPreset(id)) return;
    setPresetId(id);
    this.applyToRoot();
  },

  /** The OS light/dark setting changed; only follow-system reacts to it. */
  setSystemPolarity(next: ThemePolarity): void {
    setSystemPolaritySignal(next);
    this.applyToRoot();
  },

  setAppearance(next: AppearanceConfig): void {
    setAppearanceSignal({ ...next });
    this.applyToRoot();
  },

  setOverride(key: string, value: string): boolean {
    if (!isValidColor(value)) return false;
    setOverridesSignal((prev) => ({ ...prev, [key]: value }));
    this.applyToRoot();
    return true;
  },

  resetOverrides(): void {
    setOverridesSignal({});
    this.applyToRoot();
  },

  loadConfig(config: ThemeConfig, nextAppearance?: AppearanceConfig): void {
    if (config.preset && getPreset(config.preset)) {
      setPresetId(config.preset);
    } else {
      setPresetId(DEFAULT_PRESET_ID);
    }
    if (nextAppearance) setAppearanceSignal({ ...nextAppearance });
    const valid: ThemeOverrides = {};
    for (const [k, v] of Object.entries(config.overrides ?? {})) {
      if (typeof v !== "string" || !isValidColor(v)) continue;
      if (RETIRED_OVERRIDE_GROUPS.has(k.split(".")[0])) continue;
      valid[k] = v;
    }
    setOverridesSignal(valid);
    this.applyToRoot();
  },

  toConfig(): ThemeConfig {
    return { preset: presetId(), overrides: overrides() };
  },

  presets(): Readonly<Theme[]> {
    return PRESETS;
  },
};
