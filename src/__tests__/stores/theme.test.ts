import { describe, it, expect, beforeEach } from "vitest";
import { themeStore } from "../../stores/global/theme";

function fakeRoot(): HTMLElement {
  return document.createElement("div");
}

describe("themeStore", () => {
  beforeEach(() => {
    themeStore.resetOverrides();
    themeStore.setPreset("warp-dark");
  });

  it("starts on the warp-dark preset", () => {
    expect(themeStore.presetId()).toBe("warp-dark");
    expect(themeStore.activePreset().name).toBe("Warp Dark");
  });

  it("flattens preset tokens into dot-keyed CSS values", () => {
    const tokens = themeStore.resolvedTokens();
    expect(tokens["surface.background"]).toBe("#0e0e14");
    expect(tokens["accent.default"]).toBe("#7aa2f7");
    expect(tokens["syntax.keyword"]).toBe("#bb9af7");
  });

  it("applies tokens to the root element as CSS variables", () => {
    const root = fakeRoot();
    themeStore.applyToRoot(root);
    expect(root.style.getPropertyValue("--writ-surface-background")).toBe("#0e0e14");
    expect(root.style.getPropertyValue("--writ-foreground-default")).toBe("#e0e0e0");
  });

  it("setOverride takes precedence over preset values", () => {
    expect(themeStore.setOverride("accent.default", "#ff7b00")).toBe(true);
    expect(themeStore.resolvedTokens()["accent.default"]).toBe("#ff7b00");
  });

  it("setOverride rejects invalid color values", () => {
    expect(themeStore.setOverride("accent.default", "not-a-color")).toBe(false);
    expect(themeStore.resolvedTokens()["accent.default"]).toBe("#7aa2f7");
  });

  it("resetOverrides clears all overrides", () => {
    themeStore.setOverride("accent.default", "#ff7b00");
    themeStore.resetOverrides();
    expect(themeStore.resolvedTokens()["accent.default"]).toBe("#7aa2f7");
  });

  it("setPreset switches preset and re-applies", () => {
    themeStore.setPreset("dracula");
    expect(themeStore.presetId()).toBe("dracula");
    expect(themeStore.resolvedTokens()["accent.default"]).toBe("#bd93f9");
  });

  it("setPreset preserves overrides on top of the new preset", () => {
    themeStore.setOverride("accent.default", "#ff7b00");
    themeStore.setPreset("tokyo-night");
    expect(themeStore.resolvedTokens()["accent.default"]).toBe("#ff7b00");
    expect(themeStore.resolvedTokens()["surface.background"]).toBe("#1a1b26");
  });

  it("ignores unknown preset ids", () => {
    themeStore.setPreset("does-not-exist");
    expect(themeStore.presetId()).toBe("warp-dark");
  });

  it("loadConfig restores preset and validated overrides", () => {
    themeStore.loadConfig({
      preset: "dracula",
      overrides: {
        "accent.default": "#ff7b00",
        "foreground.default": "not-a-color",
      },
    });
    expect(themeStore.presetId()).toBe("dracula");
    expect(themeStore.resolvedTokens()["accent.default"]).toBe("#ff7b00");
    expect(themeStore.resolvedTokens()["foreground.default"]).toBe("#f8f8f2");
  });

  it("loadConfig falls back to default preset on unknown id", () => {
    themeStore.loadConfig({ preset: "ghost-theme", overrides: {} });
    expect(themeStore.presetId()).toBe("warp-dark");
  });

  it("toConfig serializes current state", () => {
    themeStore.setPreset("solarized-dark");
    themeStore.setOverride("accent.default", "#ff7b00");
    const config = themeStore.toConfig();
    expect(config.preset).toBe("solarized-dark");
    expect(config.overrides["accent.default"]).toBe("#ff7b00");
  });
});

describe("theme polarity and fast boot", () => {
  beforeEach(() => {
    themeStore.resetOverrides();
    themeStore.setPreset("warp-dark");
  });

  it("reports dark for dark presets and light for the light preset", () => {
    themeStore.setPreset("warp-dark");
    expect(themeStore.polarity()).toBe("dark");
    themeStore.setPreset("warp-light");
    expect(themeStore.polarity()).toBe("light");
  });

  it("ships a light preset selectable in the picker", () => {
    const light = themeStore.presets().find((p) => p.id === "warp-light");
    expect(light?.polarity).toBe("light");
  });

  it("persists resolved variables for the pre-paint boot script", () => {
    themeStore.setPreset("warp-light");
    themeStore.applyToRoot(fakeRoot());
    const raw = localStorage.getItem("writ-theme-vars");
    expect(raw).toBeTruthy();
    const vars = JSON.parse(raw as string);
    expect(vars["--writ-surface-background"]).toBe("#fbfbfd");
    expect(vars["--writ-accent-foreground"]).toBe("#ffffff");
  });
});

describe("preset integrity", () => {
  it("every preset declares every required token group", () => {
    for (const preset of themeStore.presets()) {
      expect(preset.surface).toBeDefined();
      expect(preset.foreground).toBeDefined();
      expect(preset.border).toBeDefined();
      expect(preset.accent).toBeDefined();
      expect(preset.status).toBeDefined();
      expect(preset.syntax).toBeDefined();
      // On-fill text tokens and polarity are required for AA + light support.
      expect(preset.accent.foreground).toBeDefined();
      expect(preset.status.foreground).toBeDefined();
      expect(preset.polarity === "light" || preset.polarity === "dark").toBe(true);
    }
  });

  // A handful of site tokens are composite CSS values by design (an rgba seam,
  // multi-layer box-shadows, easing curves), not single colors. They are
  // checked for presence, not for hex. Every other token must be a hex color;
  // nested groups (e.g. site.traffic) have their leaf colors hex-checked.
  const COMPOSITE_TOKENS = new Set([
    "site.seam",
    "site.winShadow",
    "site.panelShadow",
    "site.ease",
    "site.spring",
  ]);

  it("every preset color parses as valid hex", () => {
    const hex = /^#[0-9a-fA-F]{3,8}$/;
    for (const preset of themeStore.presets()) {
      themeStore.setPreset(preset.id);
      const tokens = themeStore.resolvedTokens();
      for (const [key, rawValue] of Object.entries(tokens)) {
        const value: unknown = rawValue;
        if (COMPOSITE_TOKENS.has(key)) {
          expect(
            typeof value === "string" && value.trim().length > 0,
            `${preset.id}.${key} = ${String(value)}`,
          ).toBe(true);
          continue;
        }
        if (value !== null && typeof value === "object") {
          for (const [subKey, subValue] of Object.entries(value as Record<string, unknown>)) {
            expect(
              typeof subValue === "string" && hex.test(subValue),
              `${preset.id}.${key}.${subKey} = ${String(subValue)}`,
            ).toBe(true);
          }
          continue;
        }
        expect(
          typeof value === "string" && hex.test(value),
          `${preset.id}.${key} = ${String(value)}`,
        ).toBe(true);
      }
    }
  });
});
