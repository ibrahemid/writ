import { createSignal, createMemo } from "solid-js";
import type { Theme, ThemeOverrides, ThemeConfig, ThemePolarity } from "../../types/theme";
import { PRESETS, getPreset, getDefaultPreset, DEFAULT_PRESET_ID } from "../../styles/themes";

// Singleton — app-global, not window-scoped (ADR-009 E3).
// Theme is shared across every window; CSS custom properties on :root propagate.

const HEX_PATTERN = /^#[0-9a-fA-F]{3,8}$/;

// Mirror of the resolved CSS variables, read by the inline boot script in
// index.html to paint the saved theme before the bundle loads (no FOUC).
const FAST_BOOT_KEY = "writ-theme-vars";

const [presetId, setPresetId] = createSignal<string>(DEFAULT_PRESET_ID);
const [overrides, setOverridesSignal] = createSignal<ThemeOverrides>({});

const activePreset = createMemo<Theme>(() => getPreset(presetId()) ?? getDefaultPreset());
const polarity = createMemo<ThemePolarity>(() => activePreset().polarity ?? "dark");

function flattenTheme(theme: Theme): Record<string, string> {
  const flat: Record<string, string> = {};
  for (const [group, tokens] of Object.entries(theme)) {
    if (group === "id" || group === "name" || group === "polarity") continue;
    if (typeof tokens !== "object" || tokens === null) continue;
    for (const [key, value] of Object.entries(tokens as Record<string, string>)) {
      flat[`${group}.${key}`] = value;
    }
  }
  return flat;
}

function tokenKeyToCssVar(key: string): string {
  return `--writ-${key.replace(".", "-")}`;
}

function isValidColor(value: string): boolean {
  return HEX_PATTERN.test(value);
}

export const themeStore = {
  presetId,
  overrides,
  activePreset,
  polarity,

  resolvedTokens: createMemo<Record<string, string>>(() => {
    const base = flattenTheme(activePreset());
    return { ...base, ...overrides() };
  }),

  applyToRoot(root: HTMLElement = document.documentElement): void {
    const resolved = this.resolvedTokens();
    const snapshot: Record<string, string> = {};
    for (const [key, value] of Object.entries(resolved)) {
      const cssVar = tokenKeyToCssVar(key);
      root.style.setProperty(cssVar, value);
      snapshot[cssVar] = value;
    }
    try {
      localStorage.setItem(FAST_BOOT_KEY, JSON.stringify(snapshot));
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

  loadConfig(config: ThemeConfig): void {
    if (config.preset && getPreset(config.preset)) {
      setPresetId(config.preset);
    } else {
      setPresetId(DEFAULT_PRESET_ID);
    }
    const valid: ThemeOverrides = {};
    for (const [k, v] of Object.entries(config.overrides ?? {})) {
      if (typeof v === "string" && isValidColor(v)) valid[k] = v;
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
