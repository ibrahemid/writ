import { describe, it, expect, beforeEach } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";
import { flattenTheme, themeStore } from "../../stores/global/theme";
import { TOKEN_GROUPS } from "../../types/theme";
import type { Theme } from "../../types/theme";

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

  it("writ-light and writ-dark are registered presets", () => {
    const ids = themeStore.presets().map((p) => p.id);
    expect(ids).toContain("writ-light");
    expect(ids).toContain("writ-dark");
    expect(themeStore.presets().find((p) => p.id === "writ-light")?.polarity).toBe("light");
    expect(themeStore.presets().find((p) => p.id === "writ-dark")?.polarity).toBe("dark");
  });

  it("ships a light preset selectable in the picker", () => {
    const light = themeStore.presets().find((p) => p.id === "warp-light");
    expect(light?.polarity).toBe("light");
  });

  it("applyToRoot writes the polarity onto the root as data-theme", () => {
    const root = fakeRoot();
    themeStore.setPreset("warp-dark");
    themeStore.applyToRoot(root);
    expect(root.getAttribute("data-theme")).toBe("dark");
    themeStore.setPreset("warp-light");
    themeStore.applyToRoot(root);
    expect(root.getAttribute("data-theme")).toBe("light");
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

});

// Asserted against the files, not against the store: the flattener now drops a
// non-string leaf and a `site` group, so reading a preset through it would hide
// exactly the shape this contract exists to forbid.
describe("preset files", () => {
  const DIR = resolve(process.cwd(), "src/styles/themes");
  const FILES = readdirSync(DIR).filter((name) => name.endsWith(".json"));
  const HEX = /^#[0-9a-fA-F]{3,8}$/;

  function preset(file: string): Record<string, unknown> {
    return JSON.parse(readFileSync(resolve(DIR, file), "utf8")) as Record<string, unknown>;
  }

  it("finds every registered preset on disk", () => {
    expect(FILES.length).toBe(themeStore.presets().length);
  });

  for (const file of FILES) {
    describe(file, () => {
      const json = preset(file);

      it("declares the token groups and nothing else", () => {
        expect(Object.keys(json).sort()).toEqual(
          ["id", "name", "polarity", ...TOKEN_GROUPS].sort(),
        );
      });

      it("names itself and its polarity", () => {
        expect(typeof json.id === "string" && json.id.length > 0).toBe(true);
        expect(typeof json.name === "string" && (json.name as string).length > 0).toBe(true);
        expect(json.polarity === "light" || json.polarity === "dark").toBe(true);
      });

      it("carries a flat group of hex colours per token group", () => {
        for (const group of TOKEN_GROUPS) {
          const tokens: unknown = json[group];
          expect(
            typeof tokens === "object" && tokens !== null && !Array.isArray(tokens),
            `${group} is not a token group`,
          ).toBe(true);
          for (const [key, value] of Object.entries(tokens as Record<string, unknown>)) {
            expect(
              typeof value === "string" && HEX.test(value),
              `${group}.${key} = ${JSON.stringify(value)}`,
            ).toBe(true);
          }
        }
      });
    });
  }
});

describe("flattening a preset into CSS variables", () => {
  beforeEach(() => {
    themeStore.resetOverrides();
    themeStore.setPreset("warp-dark");
  });

  it("ignores nested objects instead of stringifying them", () => {
    const nested = {
      ...themeStore.activePreset(),
      extras: { traffic: { close: "#ff5f57" }, flat: "#123456" },
    } as unknown as Theme;
    const flat = flattenTheme(nested);
    expect(Object.values(flat)).not.toContain("[object Object]");
    expect(flat["extras.traffic"]).toBeUndefined();
    expect(flat["extras.flat"]).toBe("#123456");
  });

  it("writes no --writ-site-* variable to the root", () => {
    for (const preset of themeStore.presets()) {
      const root = fakeRoot();
      themeStore.setPreset(preset.id);
      themeStore.applyToRoot(root);
      const written = Array.from({ length: root.style.length }, (_, i) => root.style.item(i));
      expect(written.filter((name) => name.startsWith("--writ-site-")), preset.id).toEqual([]);
    }
  });

  it("maps a token key with several dots to a full kebab CSS name", () => {
    const root = fakeRoot();
    expect(themeStore.setOverride("syntax.tag.attribute", "#abcdef")).toBe(true);
    themeStore.applyToRoot(root);
    expect(root.style.getPropertyValue("--writ-syntax-tag-attribute")).toBe("#abcdef");
  });
});
