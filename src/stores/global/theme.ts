import { createSignal, createMemo } from "solid-js";
import type { Theme, ThemeOverrides, ThemeConfig, ThemePolarity } from "../../types/theme";
import { migrateOverrideKey, tokenKey } from "../../types/theme";
import type { AppearanceConfig } from "../../types/config";
import type { AccentId } from "../../styles/generated/tokens";
import { ACCENTS } from "../../styles/generated/tokens";
import {
  PRESETS,
  getPreset,
  getDefaultPreset,
  pairedPreset,
  takesAccentSetting,
  DEFAULT_PRESET_ID,
} from "../../styles/themes";

// Singleton — app-global, not window-scoped (ADR-009 E3).
// Theme is shared across every window; CSS custom properties on :root propagate.

const HEX_PATTERN = /^#[0-9a-fA-F]{3,8}$/;

// Mirror of the resolved CSS variables and root attributes, read by the inline
// boot script in index.html to paint the saved theme before the bundle loads
// (no FOUC). Versioned: a snapshot from an older property set paints names the
// stylesheets no longer read, so it would flash the wrong palette on the first
// launch after an update. v3 is the ADR-030 vocabulary.
const FAST_BOOT_KEY = "writ-theme-vars-v3";

const DEFAULT_APPEARANCE: AppearanceConfig = {
  polarity: "system",
  accent: "pine",
  prose_face: "system",
};

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

/** Whether the active preset defers its highlight to the accent setting. */
const accentApplies = createMemo<boolean>(() => takesAccentSetting(activePresetId()));

/**
 * A preset's token groups as flat token keys (`tokenKey`, so a `default` leaf
 * is the bare group name). Only string leaves make
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
    for (const [leaf, value] of Object.entries(tokens as Record<string, unknown>)) {
      if (typeof value !== "string") continue;
      flat[tokenKey(group, leaf)] = value;
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
  accentApplies,
  effectivePolarity,

  resolvedTokens: createMemo<Record<string, string>>(() => {
    const base = flattenTheme(activePreset());
    // The accent is its own axis over a neutral preset: the chosen hue wins
    // over the preset's, and a user override still wins over both. A preset
    // that carries its own palette keeps its own accent.
    if (accentApplies()) {
      const triple = ACCENTS[accent()][polarity()];
      base["accent"] = triple.base;
      base["accent.hover"] = triple.hover;
      base["accent.fg"] = triple.foreground;
    }
    return { ...base, ...overrides() };
  }),

  applyToRoot(root: HTMLElement = document.documentElement): void {
    // The generated sheet keys its dark and accent layers on root attributes,
    // so both have to reach the DOM as well as the custom properties.
    // data-accent selects the accent block in the generated sheet, so it only
    // goes on the root when the accent setting is what paints the highlight.
    const attrs: Record<string, string> = { "data-theme": polarity() };
    if (accentApplies()) attrs["data-accent"] = accent();
    for (const [name, value] of Object.entries(attrs)) root.setAttribute(name, value);
    if (!accentApplies()) root.removeAttribute("data-accent");
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

  /**
   * Applies a stored theme config. Overrides written before ADR-030 are keyed
   * by the old group names; each one with a successor is translated, and one
   * naming a token the new vocabulary dropped is discarded.
   *
   * Returns the translated map when it differs from what was stored, so the
   * caller can write it back and the next load has nothing to do. Returns
   * `null` when the stored map was already current.
   */
  loadConfig(config: ThemeConfig, nextAppearance?: AppearanceConfig): ThemeOverrides | null {
    if (config.preset && getPreset(config.preset)) {
      setPresetId(config.preset);
    } else {
      setPresetId(DEFAULT_PRESET_ID);
    }
    if (nextAppearance) setAppearanceSignal({ ...nextAppearance });
    const stored = config.overrides ?? {};
    const valid: ThemeOverrides = {};
    let changed = false;
    for (const [key, value] of Object.entries(stored)) {
      if (typeof value !== "string" || !isValidColor(value)) {
        changed = true;
        continue;
      }
      const current = migrateOverrideKey(key);
      if (current === null) {
        changed = true;
        continue;
      }
      if (current !== key) changed = true;
      valid[current] = value;
    }
    setOverridesSignal(valid);
    this.applyToRoot();
    return changed ? valid : null;
  },

  toConfig(): ThemeConfig {
    return { preset: presetId(), overrides: overrides() };
  },

  presets(): Readonly<Theme[]> {
    return PRESETS;
  },
};
