import { describe, it, expect, beforeEach } from "vitest";
import { createEffect, createRoot } from "solid-js";
import { readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";
import { flattenTheme, themeStore } from "../../stores/global/theme";
import { TOKEN_GROUPS } from "../../types/theme";
import type { Theme } from "../../types/theme";
import { ACCENTS } from "../../styles/generated/tokens";
import { DEFAULT_PRESET_ID } from "../../styles/themes";

function fakeRoot(): HTMLElement {
  return document.createElement("div");
}

// The store is an app-global singleton, so each block states the appearance it
// asserts against rather than inheriting the previous one.
function pin(polarity: "light" | "dark") {
  themeStore.setAppearance({ polarity, accent: "pine", prose_face: "system" });
}

describe("themeStore", () => {
  beforeEach(() => {
    themeStore.resetOverrides();
    pin("dark");
    themeStore.setPreset("warp-dark");
  });

  it("defaults to writ-light", () => {
    expect(DEFAULT_PRESET_ID).toBe("writ-light");
    themeStore.loadConfig({ preset: "", overrides: {} }, { polarity: "light", accent: "pine", prose_face: "system" });
    expect(themeStore.presetId()).toBe("writ-light");
    expect(themeStore.activePreset().name).toBe("Writ Light");
    expect(themeStore.polarity()).toBe("light");
  });

  it("flattens preset tokens into dot-keyed CSS values", () => {
    const tokens = themeStore.resolvedTokens();
    expect(tokens["bg.canvas"]).toBe("#0e0e14");
    expect(tokens["syntax.keyword"]).toBe("#bb9af7");
  });

  it("collapses a group's default leaf onto the group name", () => {
    // --writ-fg and --writ-border are what the generated sheet declares, so a
    // preset leaf has to land there and not on --writ-fg-default.
    const tokens = themeStore.resolvedTokens();
    expect(tokens["fg"]).toBe("#e0e0e0");
    expect(tokens["border"]).toBe("#1e1e2e");
    expect(tokens["fg.default"]).toBeUndefined();
    expect(tokens["border.default"]).toBeUndefined();
  });

  it("applies tokens to the root element as CSS variables", () => {
    const root = fakeRoot();
    themeStore.applyToRoot(root);
    expect(root.style.getPropertyValue("--writ-bg-canvas")).toBe("#0e0e14");
    expect(root.style.getPropertyValue("--writ-fg")).toBe("#e0e0e0");
  });

  it("resolves the accent triple for the effective polarity", () => {
    pin("dark");
    expect(themeStore.resolvedTokens()["accent"]).toBe(ACCENTS.pine.dark.base);
    pin("light");
    themeStore.setPreset("warp-light");
    expect(themeStore.resolvedTokens()["accent"]).toBe(ACCENTS.pine.light.base);
    themeStore.setAppearance({ polarity: "light", accent: "gold", prose_face: "system" });
    expect(themeStore.resolvedTokens()["accent"]).toBe(ACCENTS.gold.light.base);
  });

  it("the accent setting reaches a paired preset", () => {
    themeStore.setAppearance({ polarity: "dark", accent: "gold", prose_face: "system" });
    for (const id of ["writ-dark", "warp-dark"]) {
      themeStore.setPreset(id);
      expect(themeStore.accentApplies(), id).toBe(true);
      expect(themeStore.resolvedTokens()["accent"], id).toBe(ACCENTS.gold.dark.base);
      expect(themeStore.resolvedTokens()["accent.hover"], id).toBe(ACCENTS.gold.dark.hover);
      expect(themeStore.resolvedTokens()["accent.fg"], id).toBe(
        ACCENTS.gold.dark.foreground,
      );
    }
  });

  it("a preset that carries its own palette keeps its own accent", () => {
    themeStore.setAppearance({ polarity: "dark", accent: "gold", prose_face: "system" });
    themeStore.setPreset("dracula");
    expect(themeStore.accentApplies()).toBe(false);
    expect(themeStore.resolvedTokens()["accent"]).toBe("#bd93f9");
    themeStore.setPreset("tokyo-night");
    expect(themeStore.accentApplies()).toBe(false);
    expect(themeStore.resolvedTokens()["accent"]).toBe("#7aa2f7");
  });

  it("data-accent is only on the root while the setting paints the highlight", () => {
    const root = fakeRoot();
    themeStore.setAppearance({ polarity: "dark", accent: "plum", prose_face: "system" });
    themeStore.setPreset("writ-dark");
    themeStore.applyToRoot(root);
    expect(root.getAttribute("data-accent")).toBe("plum");
    // Left behind, the attribute would select the accent block in the
    // generated sheet and repaint a terminal preset's highlight.
    themeStore.setPreset("tokyo-night");
    themeStore.applyToRoot(root);
    expect(root.getAttribute("data-accent")).toBeNull();
  });

  it("a user override beats the accent choice", () => {
    themeStore.setAppearance({ polarity: "dark", accent: "gold", prose_face: "system" });
    expect(themeStore.setOverride("accent", "#ff7b00")).toBe(true);
    expect(themeStore.resolvedTokens()["accent"]).toBe("#ff7b00");
  });

  it("system polarity swaps within a preset pair", () => {
    themeStore.setAppearance({ polarity: "system", accent: "pine", prose_face: "system" });
    themeStore.setPreset("warp-dark");
    themeStore.setSystemPolarity("light");
    expect(themeStore.activePreset().id).toBe("warp-light");
    themeStore.setSystemPolarity("dark");
    expect(themeStore.activePreset().id).toBe("warp-dark");
  });

  it("a preset with no pair ignores system polarity", () => {
    themeStore.setAppearance({ polarity: "system", accent: "pine", prose_face: "system" });
    themeStore.setPreset("tokyo-night");
    themeStore.setSystemPolarity("light");
    expect(themeStore.activePreset().id).toBe("tokyo-night");
    expect(themeStore.polarity()).toBe("dark");
  });

  it("setOverride takes precedence over preset values", () => {
    expect(themeStore.setOverride("accent", "#ff7b00")).toBe(true);
    expect(themeStore.resolvedTokens()["accent"]).toBe("#ff7b00");
  });

  it("setOverride rejects invalid color values", () => {
    expect(themeStore.setOverride("accent", "not-a-color")).toBe(false);
    expect(themeStore.resolvedTokens()["accent"]).toBe(ACCENTS.pine.dark.base);
  });

  it("resetOverrides clears all overrides", () => {
    themeStore.setOverride("accent", "#ff7b00");
    themeStore.resetOverrides();
    expect(themeStore.resolvedTokens()["accent"]).toBe(ACCENTS.pine.dark.base);
  });

  it("setPreset switches preset and re-applies", () => {
    themeStore.setPreset("dracula");
    expect(themeStore.presetId()).toBe("dracula");
    expect(themeStore.resolvedTokens()["bg.canvas"]).toBe("#282a36");
  });

  it("setPreset preserves overrides on top of the new preset", () => {
    themeStore.setOverride("accent", "#ff7b00");
    themeStore.setPreset("tokyo-night");
    expect(themeStore.resolvedTokens()["accent"]).toBe("#ff7b00");
    expect(themeStore.resolvedTokens()["bg.canvas"]).toBe("#1a1b26");
  });

  it("ignores unknown preset ids", () => {
    themeStore.setPreset("does-not-exist");
    expect(themeStore.presetId()).toBe("warp-dark");
  });

  it("swaps the prose face token only when the alternate is chosen", () => {
    const root = fakeRoot();
    themeStore.setAppearance({ polarity: "dark", accent: "pine", prose_face: "quattro" });
    themeStore.applyToRoot(root);
    expect(root.style.getPropertyValue("--writ-font-prose")).toBe("var(--writ-font-prose-alt)");
    themeStore.setAppearance({ polarity: "dark", accent: "pine", prose_face: "system" });
    themeStore.applyToRoot(root);
    expect(root.style.getPropertyValue("--writ-font-prose")).toBe("");
  });

  it("loadConfig translates an override written in the old vocabulary", () => {
    // ADR-030 renamed the token groups. A stored override that still has a
    // successor is moved onto its new key rather than thrown away, and the
    // translated map comes back so the caller can persist it.
    const migrated = themeStore.loadConfig(
      {
        preset: "dracula",
        overrides: {
          "surface.background": "#123456",
          "foreground.subtle": "#abcdef",
          "accent.default": "#ff7b00",
          "syntax.keyword": "#0f0f0f",
        },
      },
      { polarity: "dark", accent: "pine", prose_face: "system" },
    );
    expect(themeStore.presetId()).toBe("dracula");
    expect(migrated).toEqual({
      "bg.canvas": "#123456",
      "fg.faint": "#abcdef",
      accent: "#ff7b00",
      "syntax.keyword": "#0f0f0f",
    });
    expect(themeStore.overrides()).toEqual(migrated);

    const root = fakeRoot();
    themeStore.applyToRoot(root);
    expect(root.style.getPropertyValue("--writ-bg-canvas")).toBe("#123456");
    expect(root.style.getPropertyValue("--writ-fg-faint")).toBe("#abcdef");
    expect(root.style.getPropertyValue("--writ-accent")).toBe("#ff7b00");
    expect(root.style.getPropertyValue("--writ-syntax-keyword")).toBe("#0f0f0f");
  });

  it("loadConfig drops an override naming a token the new vocabulary lost", () => {
    const migrated = themeStore.loadConfig(
      {
        preset: "dracula",
        overrides: {
          // border.focus is the accent now and border.pill is the border, so
          // neither has a key of its own to move to.
          "border.focus": "#ff0000",
          "border.pill": "#00ff00",
          "foreground.default": "not-a-color",
          "bg.sidebar": "#101010",
        },
      },
      { polarity: "dark", accent: "pine", prose_face: "system" },
    );
    expect(migrated).toEqual({ "bg.sidebar": "#101010" });
    expect(themeStore.overrides()).toEqual({ "bg.sidebar": "#101010" });
  });

  it("loadConfig reports nothing to write back for an already-current map", () => {
    const migrated = themeStore.loadConfig(
      { preset: "dracula", overrides: { "bg.canvas": "#123456" } },
      { polarity: "dark", accent: "pine", prose_face: "system" },
    );
    expect(migrated).toBeNull();
    expect(themeStore.overrides()).toEqual({ "bg.canvas": "#123456" });
  });

  it("loadConfig falls back to default preset on unknown id", () => {
    themeStore.loadConfig({ preset: "ghost-theme", overrides: {} });
    expect(themeStore.presetId()).toBe("writ-light");
  });

  it("toConfig serializes current state", () => {
    themeStore.setPreset("solarized-dark");
    themeStore.setOverride("accent", "#ff7b00");
    const config = themeStore.toConfig();
    expect(config.preset).toBe("solarized-dark");
    expect(config.overrides["accent"]).toBe("#ff7b00");
  });
});

describe("theme polarity and fast boot", () => {
  beforeEach(() => {
    themeStore.resetOverrides();
    pin("dark");
    themeStore.setPreset("warp-dark");
  });

  it("reports dark for dark presets and light for the light preset", () => {
    themeStore.setPreset("warp-dark");
    expect(themeStore.polarity()).toBe("dark");
    pin("light");
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

  it("applyToRoot writes data-theme and data-accent", () => {
    const root = fakeRoot();
    themeStore.setPreset("warp-dark");
    themeStore.applyToRoot(root);
    expect(root.getAttribute("data-theme")).toBe("dark");
    expect(root.getAttribute("data-accent")).toBe("pine");
    themeStore.setAppearance({ polarity: "light", accent: "plum", prose_face: "system" });
    themeStore.setPreset("warp-light");
    themeStore.applyToRoot(root);
    expect(root.getAttribute("data-theme")).toBe("light");
    expect(root.getAttribute("data-accent")).toBe("plum");
  });

  it("persists resolved variables and attributes for the pre-paint boot script", () => {
    pin("light");
    themeStore.setPreset("warp-light");
    themeStore.applyToRoot(fakeRoot());
    const raw = localStorage.getItem("writ-theme-vars-v3");
    expect(raw).toBeTruthy();
    const saved = JSON.parse(raw as string) as {
      vars: Record<string, string>;
      attrs: Record<string, string>;
    };
    expect(saved.vars["--writ-bg-canvas"]).toBe("#fbfbfd");
    expect(saved.vars["--writ-accent-fg"]).toBe(ACCENTS.pine.light.foreground);
    expect(saved.attrs).toEqual({ "data-theme": "light", "data-accent": "pine" });
  });

  it("keys the snapshot on a version no earlier build wrote", () => {
    // An older snapshot describes a property set the stylesheets no longer
    // read; replaying it would flash the wrong palette after an update.
    localStorage.setItem("writ-theme-vars", "{}");
    localStorage.setItem("writ-theme-vars-v2", "{}");
    themeStore.applyToRoot(fakeRoot());
    expect(localStorage.getItem("writ-theme-vars")).toBe("{}");
    expect(localStorage.getItem("writ-theme-vars-v2")).toBe("{}");
    expect(localStorage.getItem("writ-theme-vars-v3")).not.toBe("{}");
  });
});

describe("a theme change reaching the root", () => {
  beforeEach(() => {
    themeStore.resetOverrides();
    themeStore.setAppearance({ polarity: "system", accent: "pine", prose_face: "system" });
  });

  // Anything that paints from the DOM rather than from the store reads its
  // colours off the root when the theme changes: a canvas cannot inherit a
  // custom property. The properties have to be on the root by then, or the
  // reader paints the palette the theme is leaving.
  it("has the new palette on the root before an effect sees the change", async () => {
    themeStore.setSystemPolarity("dark");
    const seen: string[] = [];
    const stop = createRoot((dispose) => {
      createEffect(() => {
        themeStore.resolvedTokens();
        seen.push(document.documentElement.style.getPropertyValue("--writ-bg-canvas"));
      });
      return dispose;
    });
    await Promise.resolve();
    expect(seen).toHaveLength(1);
    const wasDark = seen[0];

    themeStore.setSystemPolarity("light");
    await Promise.resolve();
    expect(seen).toHaveLength(2);
    expect(seen[1]).not.toBe(wasDark);
    expect(seen[1]).toBe(document.documentElement.style.getPropertyValue("--writ-bg-canvas"));
    stop();
  });
});

describe("preset integrity", () => {
  it("every preset declares every required token group", () => {
    for (const preset of themeStore.presets()) {
      expect(preset.bg).toBeDefined();
      expect(preset.fg).toBeDefined();
      expect(preset.border).toBeDefined();
      expect(preset.accent).toBeDefined();
      expect(preset.status).toBeDefined();
      expect(preset.syntax).toBeDefined();
      // On-fill text tokens and polarity are required for AA + light support.
      expect(preset.accent.fg).toBeDefined();
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
    pin("dark");
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
      pin(preset.polarity);
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
