import { describe, it, expect } from "vitest";
import {
  SETTINGS_INDEX,
  SECTION_LABELS,
  SECTION_ORDER,
  UnknownSettingError,
  rankSections,
  rankSettings,
  matchedSettingIds,
  sectionHasMatch,
  settingDescription,
  settingTitle,
} from "../../settings";
import { APPS } from "../../lib/apps";

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

  it("titles the light-dark row for the choice it offers, not for its section", () => {
    expect(SETTINGS_INDEX.find((e) => e.id === "appearance.polarity")?.title).toBe("Light and dark");
  });

  it("finds the light-dark row under the words it used to be titled with", () => {
    for (const query of ["appearance", "dark", "light", "system"]) {
      expect(rankSettings(query).map((e) => e.id), query).toContain("appearance.polarity");
    }
  });

  it("matches by section label", () => {
    // "Advanced" is a section label and nothing else, so every hit is one the
    // label alone surfaced.
    const results = rankSettings("advanced");
    expect(results.every((e) => e.section === "advanced")).toBe(true);
    expect(results.length).toBeGreaterThan(0);
  });

  it("offers one file-types row, not a row per claimable group", () => {
    // The group ids now come from the backend at render time, so the index
    // carries no id that could drift from crates/writ-core/src/default_app.rs.
    const defaultAppEntries = SETTINGS_INDEX.filter((e) => e.id.startsWith("files.default_app"));
    expect(defaultAppEntries.map((e) => e.id)).toEqual(["files.default_app"]);
  });

  it("reports section matches for filtering", () => {
    expect(sectionHasMatch("editor", "font")).toBe(true);
    expect(sectionHasMatch("about", "font")).toBe(false);
    expect(sectionHasMatch("about", "")).toBe(true);
  });

  it("holds the eight sections named for the job, in order", () => {
    expect(SECTION_ORDER.map((id) => SECTION_LABELS[id])).toEqual([
      "General",
      "Editor",
      "Appearance",
      "Apps",
      "Integrations",
      "Shortcuts",
      "Advanced",
      "About",
    ]);
  });

  it("leaves no section with a single row", () => {
    for (const section of SECTION_ORDER) {
      if (section === "shortcuts") continue;
      const rows = SETTINGS_INDEX.filter((e) => e.section === section).length;
      expect(rows, section).toBeGreaterThan(1);
    }
  });

  it("puts the folder, the format, the window's chord and open at login in General", () => {
    const general = SETTINGS_INDEX.filter((e) => e.section === "general").map((e) => e.id);
    expect(general).toEqual([
      "notes.folder",
      "files.default_extension",
      "hotkey.toggle",
      "general.launch_at_login",
    ]);
  });

  it("takes the terminal command, the file types and the watched folder out of Advanced", () => {
    for (const id of ["files.cli", "files.default_app", "files.inbox_folder", "sidebar.inbox"]) {
      expect(SETTINGS_INDEX.find((e) => e.id === id)?.section, id).toBe("integrations");
    }
  });

  it("finds the terminal command under Integrations", () => {
    const top = rankSettings("terminal")[0];
    expect(top?.id).toBe("files.cli");
    expect(top?.section).toBe("integrations");
  });

  it("names a row by the index title, and refuses an id it does not hold", () => {
    expect(settingTitle("editor.spelling_dialect")).toBe("Dictionary");
    expect(() => settingTitle("no.such.row")).toThrow(UnknownSettingError);
  });

  it("titles no row with a trailing colon", () => {
    for (const entry of SETTINGS_INDEX) {
      expect(entry.title.endsWith(":"), entry.id).toBe(false);
    }
  });

  it("places every app page row under Apps, on pages its apps own", () => {
    for (const entry of SETTINGS_INDEX.filter((e) => e.pages)) {
      expect(entry.section, entry.id).toBe("apps");
      for (const page of entry.pages!) expect(entry.requires ?? [], entry.id).toContain(page);
    }
  });

  it("gives every row one plain sentence that says more than its label", () => {
    for (const entry of SETTINGS_INDEX) {
      expect(entry.description.trim(), entry.id).not.toBe("");
      expect(entry.description, entry.id).toMatch(/^[A-Z*].*[.]$/s);
      expect(entry.description.toLowerCase(), entry.id).not.toBe(`${entry.title.toLowerCase()}.`);
    }
    expect(settingDescription("files.default_extension")).toBe(
      "For files you create. A file you name keeps the extension you type.",
    );
    expect(() => settingDescription("no.such.row")).toThrow(UnknownSettingError);
  });

  it("describes each app's switch in the words its card uses", () => {
    for (const app of APPS) expect(settingDescription(app.settingId), app.id).toBe(app.detail);
  });

  it("no longer offers the setup screen as a row; Help and the palette replay it", () => {
    expect(SETTINGS_INDEX.some((entry) => entry.id === "apps.screen")).toBe(false);
  });

  it("offers a section whose name holds the query, the ones it starts first", () => {
    expect(rankSections("app")).toEqual(["appearance", "apps"]);
    expect(rankSections("ced")).toEqual(["advanced"]);
    expect(rankSections("")).toEqual([]);
    expect(rankSections("font")).toEqual([]);
  });
});
