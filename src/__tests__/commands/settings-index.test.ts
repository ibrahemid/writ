import { describe, it, expect } from "vitest";
import {
  SETTINGS_INDEX,
  SECTION_LABELS,
  rankSettings,
  matchedSettingIds,
  sectionHasMatch,
} from "../../settings";

describe("settings index", () => {
  it("has unique ids and every section label defined", () => {
    const ids = SETTINGS_INDEX.map((e) => e.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const entry of SETTINGS_INDEX) {
      expect(SECTION_LABELS[entry.section]).toBeTruthy();
    }
  });

  it("returns nothing for an empty query", () => {
    expect(rankSettings("")).toEqual([]);
    expect(rankSettings("   ")).toEqual([]);
    expect(matchedSettingIds("").size).toBe(0);
  });

  it("matches a setting by title prefix", () => {
    const results = rankSettings("font");
    expect(results[0]?.id).toBe("editor.font_size");
  });

  it("matches a setting by keyword when the title does not contain the term", () => {
    const results = rankSettings("cli");
    expect(results.map((e) => e.id)).toContain("files.cli");
  });

  it("ranks an exact title above a keyword-only match", () => {
    const results = rankSettings("theme");
    expect(results[0]?.id).toBe("appearance.theme");
    expect(results.map((e) => e.id)).toContain("appearance.custom_colors");
  });

  it("matches by section label", () => {
    const results = rankSettings("preview");
    expect(results.every((e) => e.section === "preview")).toBe(true);
    expect(results.length).toBeGreaterThan(0);
  });

  it("default-app entries use the real writ-core claimable group ids", () => {
    // Mirrors crates/writ-core/src/default_app.rs claimable_types(); a drift here
    // means a default-app row passes an id the backend cannot resolve.
    const KNOWN_GROUP_IDS = ["plain-text", "markdown", "config-data", "source-code"];
    const defaultAppEntries = SETTINGS_INDEX.filter((e) => e.id.startsWith("files.default_app."));
    expect(defaultAppEntries.length).toBe(KNOWN_GROUP_IDS.length);
    for (const entry of defaultAppEntries) {
      const typeId = entry.id.slice("files.default_app.".length);
      expect(KNOWN_GROUP_IDS).toContain(typeId);
    }
  });

  it("reports section matches for filtering", () => {
    expect(sectionHasMatch("editor", "font")).toBe(true);
    expect(sectionHasMatch("updates", "font")).toBe(false);
    expect(sectionHasMatch("updates", "")).toBe(true);
  });
});
